//! The `Target` trait: `spec/codegen.md` §8.2.
//!
//! The driver owns the walk; a target owns everything that depends on the
//! instruction set. The value-location policy (§4), the `phi` protocol
//! (§4.5), the call sequence (§7.1) and the entry code for arguments
//! (§4.8) are implemented once, in this crate, on top of a target's
//! **glue** (the primitive moves) and its `assign`.

use hod_ir::{Body, Inst, Module, ParamAttrs, TypeId, TypeTable, Value};

use crate::abi::{AbiUnsupported, Assignment, Sig};
use crate::cx::FnCx;
use crate::error::CgResult;
use crate::frame::FrameRef;
use crate::label::{FixupKind, Label};
use crate::loc::Class;
use crate::record::FrameDesc;
use crate::reg::{PReg, RegInfo};
use crate::sym::SymRef;

/// What a call instruction transfers control to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallTarget {
    /// A symbol: a direct call. The target emits the instruction with its
    /// relocation ([`FnCx::reloc`]).
    Sym(SymRef),
    /// The address in this register: [`RegInfo::callee`].
    Reg(PReg),
}

/// What the call sequence tells the target about the call instruction it
/// asks for ([`Target::emit_call`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallEmit {
    /// The callee.
    pub target: CallTarget,
    /// Whether the call is made through a variadic function type.
    pub variadic: bool,
    /// [`Assignment::vector_regs_used`] of the call: for a convention
    /// that passes that count to a variadic callee.
    pub vector_regs_used: u8,
}

/// A place in a function that an indirect branch may reach, other than
/// the function's entry (which the target marks in
/// [`Target::begin_function`]): see [`Target::branch_target`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchTarget {
    /// The start of a block whose address is taken (`blockaddress`).
    Block,
    /// The start of a landing pad, where the unwinder jumps.
    LandingPad,
}

/// A code generator for one instruction set (§8.2).
///
/// A target is a zero-sized or small `Sync` value. Everything generic in
/// this crate is monomorphized over it, so its lowering is inlined into
/// the walk: there is no trait object on the per-instruction path.
///
/// # Glue
///
/// [`load`](Target::load), [`store`](Target::store),
/// [`copy`](Target::copy), [`copy_from_ptr`](Target::copy_from_ptr),
/// [`mov`](Target::mov), [`materialize`](Target::materialize),
/// [`materialize_mem`](Target::materialize_mem),
/// [`frame_addr`](Target::frame_addr), [`adjust_sp`](Target::adjust_sp),
/// [`extend_arg`](Target::extend_arg) and [`jump`](Target::jump) are the
/// primitive moves the generic policy is made of. The rules for all of
/// them:
///
/// - Besides its operands a glue function writes only **scratch**
///   registers ([`RegInfo::scratch`]), and never the register it was
///   given as an operand: the policy passes scratch registers to glue
///   (a `phi` copy loads into one and stores from it), so a glue function
///   that needs a register for a far offset takes a scratch register
///   *other than its operand*. That is why a target has two general
///   scratch registers.
/// - It calls nothing and allocates no register from the pool.
/// - It never fails for an offset or a size: a far offset costs a scratch
///   register, not an error.
/// - It does not change the stack pointer, except `adjust_sp`.
pub trait Target: Sized + Sync {
    /// What the target remembers about the function being compiled
    /// between `begin_function`, the lowerings and `end_function` (a
    /// reserved slot, whether the function allocates dynamically). Reset
    /// to its default before each function: keep it plain data. Lists
    /// that grow belong in this crate's tables (tail exits are one).
    type FnState: Default;

    /// The target's trap instruction: what `unreachable` emits, what
    /// follows a `noreturn` call, and what stands for an unreachable
    /// block whose address is taken (§4.7, §6.1, §7.1).
    const TRAP: &'static [u8];

    /// The alignment of a function's code, in bytes.
    const FN_ALIGN: u32;

    /// The byte that pads the space between functions.
    const PAD_BYTE: u8;

    /// The code of a function of one pointer argument that only returns:
    /// the resume and destroy function of the noop coroutine frame
    /// (`spec/codegen.md` "Coroutines", WP-105). Empty: the target does
    /// not lower `llvm.coro.noop`.
    const NOOP_CORO_FN: &'static [u8] = &[];

    /// [`Target::NOOP_CORO_FN`] with the target's indirect-branch landing
    /// instruction first, for a module with `cf-protection-branch` or
    /// `branch-target-enforcement` (WP-184, `spec/codegen.md` §5.6.25):
    /// `endbr64; ret` on x86-64. Empty: `llvm.coro.noop` is refused in
    /// such a module.
    const NOOP_CORO_FN_MARKED: &'static [u8] = &[];

    /// Whether the target compiles coroutine bodies (WP-106, §7.6): it
    /// implements [`Target::coro_stubs`] and [`Target::coro_dispatch`],
    /// addresses [`FrameRef::Coro`] from its frame base register, and
    /// lowers the body intrinsics. `false`: a coroutine is refused as
    /// `coro:target`.
    const CORO_BODIES: bool = false;

    /// The stack alignment in bytes: the largest alignment the local area
    /// gives a slot (§5.1), and what a call's stack-argument area is
    /// rounded to (§5.3).
    const STACK_ALIGN: u32;

    /// The registers (§2.1).
    fn reg_info(&self) -> &RegInfo;

    /// The class of a first-class type (§2.2). Total over the types
    /// `classify` accepts.
    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class;

    /// Where the arguments and the return value of `sig` travel (§7.2).
    /// Serves a call's outgoing placement and a function's incoming
    /// arguments alike.
    ///
    /// # Errors
    /// The name of what the target cannot pass. The facade runs `assign`
    /// over a module's signatures when it decides support, so code
    /// generation does not normally meet one; if it does, the function is
    /// `Unsupported` under that name.
    fn assign(
        &self,
        types: &TypeTable,
        sig: &Sig<'_>,
        out: &mut Assignment,
    ) -> Result<(), AbiUnsupported>;

    /// The target as it compiles `body` of `module` (§8.2), for a target
    /// whose convention depends on the function's attributes (x86-64:
    /// the target features that decide where a wide vector travels,
    /// `spec/abi-x64.md` §1.3). While the function is compiled, the value
    /// returned is the target: every `assign` (the function's own
    /// signature and each call in it) and every other method is called
    /// on it. `None`, the default, keeps `self`.
    fn for_function(&self, module: &Module, body: &Body) -> Option<Self> {
        let _ = (module, body);
        None
    }

    /// Emits the prologue in its fixed-size form and reserves what the
    /// target needs in the frame (§5.4): the frame record, a register
    /// save area for a variadic function, the incoming-argument base.
    /// Called first, when the local area is empty, so the reservations
    /// are its first bytes. It may write scratch registers only: the
    /// argument registers still hold the arguments.
    ///
    /// The body's stack slots are laid out and the entry code for the
    /// arguments (§4.8) is emitted *after* this returns, by the driver.
    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()>;

    /// Called when the body has been emitted and the local area's size is
    /// final (§5.4): patches the prologue, emits the epilogue at
    /// [`FnCx::epilogue`] and after it one tail exit per entry of
    /// [`FnCx::tail_exit`], with their CFI rows ([`FnCx::cfi`]), and
    /// fills the frame description ([`FnCx::set_frame_desc`]).
    ///
    /// # Errors
    /// `Unsupported("limit:frame-size")` for a frame the prologue cannot
    /// express.
    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()>;

    /// Called at the start of a block an indirect branch may reach
    /// (§5.6.25), after its label, its [`BlockOffset`](crate::BlockOffset)
    /// and its landing-pad row are recorded and before anything else of
    /// the block (its `phi` loads): the target emits the landing
    /// instruction its branch-protection scheme asks for (x86-64
    /// `endbr64` under `cf-protection-branch`), so the recorded offset is
    /// that instruction's. For a block that is both, `Block` is passed.
    /// Also called, with `LandingPad`, at the start of an `invoke`'s pad
    /// stub (§5.3), where the unwinder lands instead of the pad. The
    /// default emits nothing.
    ///
    /// # Errors
    /// None by default.
    fn branch_target(&self, cx: &mut FnCx<'_, Self>, at: BranchTarget) -> CgResult<()> {
        let _ = (cx, at);
        Ok(())
    }

    /// The offset of `place` from the function's DWARF frame base
    /// ([`FrameDesc::frame_base`]) once `end_function` has filled `desc`,
    /// valid from the end of the prologue to the start of each exit
    /// sequence (§9.5): what a variable's `DW_OP_fbreg` says. `None` for
    /// a place the frame base does not reach by a fixed offset; the
    /// variables there are refused (`debug:frame-place`). The default is
    /// `None` for every place.
    fn frame_base_offset(&self, desc: &FrameDesc, place: FrameRef) -> Option<i64> {
        let _ = (desc, place);
        None
    }

    /// Lowers one instruction that is not a `phi`: terminators and calls
    /// included. One `match` on the opcode that dispatches to the
    /// target's modules, one per instruction group.
    ///
    /// The driver has already skipped the instruction if it is dead and
    /// has no effect (§4.7), has written the incoming slots of the
    /// successors' `phi`s if it is a terminator other than `invoke`
    /// (§4.5), and releases the operands afterwards (§4.2).
    ///
    /// # Errors
    /// `Internal` for a case `classify` should have rejected.
    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()>;

    /// Glue: loads the low `bytes` bytes of `reg` from `src`. What the
    /// bits above them become is the target's choice (§2.2: unspecified).
    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32);

    /// Glue: stores the low `bytes` bytes of `reg` to `dst`.
    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32);

    /// Glue: copies `bytes` bytes from `src` to `dst`. The two do not
    /// overlap, or are the same place.
    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32);

    /// Glue: copies `bytes` bytes from the memory at `ptr + offset` to
    /// `dst`: the pointee of a `byval` argument into the stack-argument
    /// area (§7.2, `PartKind::Pointee`). `ptr` is left unchanged.
    fn copy_from_ptr(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        ptr: PReg,
        offset: u32,
        bytes: u32,
    );

    /// Glue: copies register `src` to `dst`, whole. Both are of one
    /// class.
    fn mov(&self, cx: &mut FnCx<'_, Self>, dst: PReg, src: PReg);

    /// Glue: produces an operand that has no location (§3) in `reg`: a
    /// constant of class `Gpr` or `Fpr` (`undef` and `poison` are zero),
    /// a symbol's address (§7.4), a stack slot's address
    /// ([`FnCx::slot_place`]; a load of the pointer slot for an
    /// over-aligned one).
    ///
    /// # Errors
    /// `Internal` for a value `classify` should have rejected in this
    /// position (a thread-local symbol, a constant expression).
    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()>;

    /// Glue: writes a constant of class `Mem` (a wide integer, an
    /// aggregate, `undef` of either: zero) to the frame slot `dst`, in
    /// its in-memory image (`spec/ir.md` §4.3).
    ///
    /// # Errors
    /// `Internal` for a constant `classify` should have rejected.
    fn materialize_mem(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, value: Value)
    -> CgResult<()>;

    /// Glue: puts the address of `of` in general register `reg`.
    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef);

    /// Glue: adds `delta` bytes, a multiple of the stack alignment, to
    /// the stack pointer: negative before the stack arguments of a call
    /// are placed, positive after it returns (§5.3).
    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64);

    /// Glue: brings the value in `reg`, an argument or return value of
    /// integer type `ty` with attributes `attrs`, to the form the
    /// target's ABI requires of its *producer* (§2.2: `zeroext`,
    /// `signext`, a plain `i1`), in place. The low bits of the type's
    /// width are unchanged; nothing to do for most values. Called by the
    /// call sequence for each general-register argument part before it is
    /// placed, and by [`FnCx::place_return`].
    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs);

    /// Glue (§7.2): the producer's side of a part of kind
    /// [`PartKind::Lanes`](crate::abi::PartKind::Lanes). `from` is the
    /// value: a register of the value's class holding it (a mask's bits
    /// in a general register; left unchanged), or the frame place of the
    /// vector's image (the part's `offset` already added). `to` is the part's place: its register, or its
    /// stack slot. Writes scratch registers and `to` only: the other
    /// argument and return registers may already hold their parts.
    ///
    /// # Errors
    /// `Unsupported("call:part-lanes")` by default: a target whose
    /// `assign` never makes such a part needs no more. A target refuses
    /// with the same name a register side of a class it does not move,
    /// rather than read it as another class (WP-167).
    fn lanes_out(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &crate::abi::Part,
        from: crate::abi::LaneLoc,
        to: crate::abi::LaneLoc,
    ) -> CgResult<()> {
        let _ = (cx, part, from, to);
        Err(crate::error::CgError::unsupported(crate::error::CALL_PART_LANES))
    }

    /// Glue (§7.2): the consumer's side of a part of kind
    /// [`PartKind::Lanes`](crate::abi::PartKind::Lanes). `from` is the
    /// part's place (a register, or an incoming stack slot); `to` is the
    /// value: a register of the value's class (for a mask a general
    /// register into whose bits `first..first + lanes` the elements' bits
    /// are inserted, the other bits unchanged), or the
    /// frame place of the vector's image (the part's `offset` already
    /// added), whose elements the part carries are written; for a mask
    /// the part carrying element 0 also clears the image's bits above the
    /// last element. Writes scratch registers and `to` only.
    ///
    /// # Errors
    /// As [`Target::lanes_out`].
    fn lanes_in(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &crate::abi::Part,
        from: crate::abi::LaneLoc,
        to: crate::abi::LaneLoc,
    ) -> CgResult<()> {
        let _ = (cx, part, from, to);
        Err(crate::error::CgError::unsupported(crate::error::CALL_PART_LANES))
    }

    /// Glue: an unconditional branch to `label`, in the form
    /// [`FnCx::far`] asks for, recorded with [`FnCx::fixup`].
    ///
    /// # Errors
    /// What [`FnCx::fixup`] reports.
    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()>;

    /// Emits the call instruction itself, with its relocation for a
    /// direct callee, and whatever the target's convention wants right
    /// before it (x86-64: the vector-register count of a variadic call).
    /// Arguments are placed and registers displaced already; the call
    /// sequence records the call site around what this emits (§7.1), so
    /// it must emit exactly one instruction that can return here, last.
    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()>;

    /// Entry code for an over-aligned stack slot (§5.1): lowers the stack
    /// pointer by `size` bytes, aligns it to `align`, and stores the
    /// resulting address in the pointer slot `ptr`. Called after the
    /// entry code for the arguments; it may write scratch registers only.
    fn dynamic_slot(
        &self,
        cx: &mut FnCx<'_, Self>,
        ptr: FrameRef,
        size: u64,
        align: u64,
    ) -> CgResult<()>;

    /// WP-183, §5.6.24: the entry code of a function with a stack guard
    /// ([`FnCx::stack_guard`]): the guard value stored in the guarded
    /// block's guard word (and in the low guard when the plan has one),
    /// and the address of each slot of the block stored in its pointer
    /// slot. The block's place is the target's, known in `end_function`,
    /// which also emits the checks. Called after the entry code for the
    /// arguments and before the over-aligned slots' (§5.1); it may write
    /// scratch registers only. The default refuses the function.
    ///
    /// # Errors
    /// [`SSP_TARGET`](crate::SSP_TARGET) by default.
    fn enter_guard(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        let _ = cx;
        Err(crate::error::CgError::unsupported(crate::guard::SSP_TARGET))
    }

    /// Writes `displacement` into the field of `kind` at offset `at` of
    /// `code`, the function's code so far (§6.2). The displacement is
    /// the label's offset minus `at`; what `at` points at (the
    /// instruction, the field) is the target's convention with itself.
    /// Returns `false`, leaving the code alone, when the field cannot
    /// hold the displacement: the function is then compiled again in far
    /// mode (§6.3).
    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool;

    /// WP-106, §7.6: after `end_function` of a coroutine, its two entry
    /// stubs, which build the same machine frame as the prologue, set
    /// the frame base from their argument and the mode to 1, and
    /// dispatch on the frame's suspend index to the re-entry labels
    /// ([`FnCx::coro_reentries`]) with the suspend result 0 (`F.resume`)
    /// or 1 (`F.destroy`). The target records where they are with
    /// [`FnCx::set_coro_entries`].
    ///
    /// # Errors
    /// `Unsupported("coro:target")` by default.
    fn coro_stubs(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        let _ = cx;
        Err(crate::error::CgError::unsupported(crate::coro_body::CORO_TARGET))
    }

    /// WP-106, §7.6: the dispatch of a suspend `switch` on `cond`, a
    /// `coro.suspend` result: a branch to `resume` when it is 0 and to
    /// `destroy` when it is 1, falling through otherwise (−1). Writes no
    /// register but scratch registers.
    ///
    /// # Errors
    /// What [`FnCx::fixup`] reports; `Unsupported("coro:target")` by
    /// default.
    fn coro_dispatch(
        &self,
        cx: &mut FnCx<'_, Self>,
        cond: PReg,
        resume: Label,
        destroy: Label,
    ) -> CgResult<()> {
        let _ = (cx, cond, resume, destroy);
        Err(crate::error::CgError::unsupported(crate::coro_body::CORO_TARGET))
    }

    /// WP-107, §7.6: symmetric transfer, emitted right after the call of
    /// an `llvm.coro.await.suspend.handle` wrapper returned, while its
    /// result (the handle to resume, a pointer) is still in the C
    /// convention's first return register. Loads the handle's resume
    /// function from its offset 0; in a resumed activation (mode 1)
    /// branches to `exit`, a tail exit ([`FnCx::coro_tail_exit`]) that
    /// tears the machine frame down and jumps to the callee register
    /// with the handle as the argument; in the ramp (mode 0) calls it
    /// (recorded with [`FnCx::record_call_site`]) and falls through. No
    /// access to the coroutine's own frame: the only memory read are the
    /// handle's first word and the mode word.
    ///
    /// # Errors
    /// What [`FnCx::fixup`] reports; `Unsupported("coro:target")` by
    /// default.
    fn coro_transfer(&self, cx: &mut FnCx<'_, Self>, exit: Label) -> CgResult<()> {
        let _ = (cx, exit);
        Err(crate::error::CgError::unsupported(crate::coro_body::CORO_TARGET))
    }
}
