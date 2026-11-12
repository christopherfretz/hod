//! `FnCx`: what `hod_cg` provides to a lowering (`spec/codegen.md` §8.1),
//! and the per-worker scratch state behind it (§11).
//!
//! This file has the context itself and the services that are plain
//! bookkeeping: code, labels, records, the frame. The value-location
//! policy is in `policy`, the `phi` protocol in `phi`, calls in `call`,
//! the walk in `driver`.

use hod_ir::{BlockId, Body, Inst, InstId, LocId, Module, ParamAttrs, SlotId, SymId, TypeId};
use hod_obj::RelocKind;

use crate::abi::Assignment;
use crate::error::{CgError, CgResult, internal};
use crate::frame::{Frame, FrameRef, SlotPlace};
use crate::label::{FixupKind, Label, Labels};
use crate::loc::{Key, ValState};
use crate::options::Options;
use crate::record::{CfiOp, CfiRow, Chunk, ClauseRow, FrameDesc, IslandRow, Mark, PadRow, Reloc};
use crate::reg::{RegInfo, RegSet};
use crate::sym::SymRef;
use crate::target::{CallTarget, Target};

/// A tail exit: where a `musttail` call branches to, and what the exit
/// sequence at that label ends in (§5.4, §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TailExit {
    /// The label `end_function` binds at the start of the exit sequence.
    pub label: Label,
    /// What the teardown is followed by a branch to.
    pub target: CallTarget,
}

/// A data island waiting to be emitted after the function's code.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Island {
    pub(crate) label: Label,
    /// Its bytes in `Scratch::island_data`.
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub(crate) align: u32,
}

/// The state one function compile needs besides its output: the location
/// table, the register state, the labels, the frame (§11). A worker owns
/// one and passes it to every `compile_function`; it is reset, not
/// freed, between functions, so a warm one makes a compile
/// allocation-free.
#[derive(Debug)]
pub struct Scratch {
    /// The location table: one entry per instruction, one per argument.
    pub(crate) vals: Vec<ValState>,
    pub(crate) args: Vec<ValState>,
    /// The value each register holds; meaningful where `occupied` is set.
    pub(crate) owner: [Key; 64],
    /// Registers that hold a value.
    pub(crate) occupied: RegSet,
    /// Registers nothing may displace until the instruction is emitted.
    pub(crate) pinned: RegSet,
    /// Registers taken as temporaries by the instruction being lowered.
    pub(crate) temps: RegSet,
    pub(crate) frame: Frame,
    pub(crate) labels: Labels,
    /// Where each body slot is, by `SlotId`.
    pub(crate) slots: Vec<SlotPlace>,
    /// By `BlockId`: whether the block can be reached (§6.1).
    pub(crate) reachable: Vec<bool>,
    pub(crate) work: Vec<u32>,
    /// By `BlockId`: the terminator (id + 1) that last wrote the block's
    /// incoming slots, so a successor listed twice is stored once (§4.5).
    pub(crate) phi_mark: Vec<u32>,
    /// By `BlockId`: the block's index among the function's pad rows + 1.
    pub(crate) pad_of_block: Vec<u32>,
    /// Per call-site row of this function: the unwind destination whose
    /// offset the row's landing address still waits for, or `u32::MAX`.
    pub(crate) site_pads: Vec<u32>,
    /// The assignment of the call being lowered, and of the function.
    pub(crate) asg: Assignment,
    pub(crate) fn_asg: Assignment,
    pub(crate) sig_params: Vec<TypeId>,
    pub(crate) sig_attrs: Vec<ParamAttrs>,
    pub(crate) tail_exits: Vec<TailExit>,
    pub(crate) islands: Vec<Island>,
    pub(crate) island_data: Vec<u8>,
}

impl Default for Scratch {
    fn default() -> Scratch {
        Scratch {
            vals: Vec::new(),
            args: Vec::new(),
            owner: [Key::NONE; 64],
            occupied: RegSet::EMPTY,
            pinned: RegSet::EMPTY,
            temps: RegSet::EMPTY,
            frame: Frame::default(),
            labels: Labels::default(),
            slots: Vec::new(),
            reachable: Vec::new(),
            work: Vec::new(),
            phi_mark: Vec::new(),
            pad_of_block: Vec::new(),
            site_pads: Vec::new(),
            asg: Assignment::default(),
            fn_asg: Assignment::default(),
            sig_params: Vec::new(),
            sig_attrs: Vec::new(),
            tail_exits: Vec::new(),
            islands: Vec::new(),
            island_data: Vec::new(),
        }
    }
}

impl Scratch {
    /// A scratch state with nothing allocated.
    pub fn new() -> Scratch {
        Scratch::default()
    }

    /// The bytes the scratch tables have allocated.
    pub fn allocated_bytes(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.vals)
            + bytes(&self.args)
            + bytes(&self.slots)
            + bytes(&self.reachable)
            + bytes(&self.work)
            + bytes(&self.phi_mark)
            + bytes(&self.pad_of_block)
            + bytes(&self.site_pads)
            + bytes(&self.sig_params)
            + bytes(&self.sig_attrs)
            + bytes(&self.tail_exits)
            + bytes(&self.islands)
            + bytes(&self.island_data)
    }
}

/// What `hod_cg` provides to a target's lowering (§8.1): one function
/// being compiled.
///
/// The fields are read access to the inputs. Everything else is a
/// method, grouped as §8.1 groups them:
///
/// | group | methods |
/// |---|---|
/// | operands | [`loc`](FnCx::loc), [`use_reg`](FnCx::use_reg), [`put`](FnCx::put), [`use_mem`](FnCx::use_mem), [`deferred`](FnCx::deferred), [`class_of`](FnCx::class_of) |
/// | results | [`def_reg`](FnCx::def_reg), [`finish`](FnCx::finish), [`def_mem`](FnCx::def_mem), [`may_defer`](FnCx::may_defer), [`defer`](FnCx::defer) |
/// | registers | [`temp`](FnCx::temp), [`take_reg`](FnCx::take_reg), [`free_temp`](FnCx::free_temp), [`displace`](FnCx::displace), [`displace_all`](FnCx::displace_all), [`scratch_reg`](FnCx::scratch_reg) |
/// | calls | [`call`](FnCx::call), [`call_runtime`](FnCx::call_runtime), [`record_call_site`](FnCx::record_call_site), [`place_return`](FnCx::place_return), [`fn_assignment`](FnCx::fn_assignment), [`tail_exit`](FnCx::tail_exit), [`phi_stores`](FnCx::phi_stores) |
/// | frame | [`alloc_slot`](FnCx::alloc_slot), [`slot_place`](FnCx::slot_place), [`frame_size`](FnCx::frame_size) |
/// | code | [`offset`](FnCx::offset), [`emit`](FnCx::emit) and the `emit_*` family, [`code_mut`](FnCx::code_mut), [`block_label`](FnCx::block_label), [`epilogue`](FnCx::epilogue), [`new_label`](FnCx::new_label), [`bind`](FnCx::bind), [`fixup`](FnCx::fixup), [`falls_through`](FnCx::falls_through), [`far`](FnCx::far), [`trap`](FnCx::trap) |
/// | records | [`reloc`](FnCx::reloc), [`island`](FnCx::island), [`cfi`](FnCx::cfi), [`set_frame_desc`](FnCx::set_frame_desc), [`type_id`](FnCx::type_id), [`pad_clause`](FnCx::pad_clause), [`pad_filter`](FnCx::pad_filter) |
///
/// Call sites, line rows and block offsets are recorded by the driver,
/// not by lowerings.
///
/// # Writing a lowering
///
/// A lowering asks for its operands, asks for a place for its result,
/// emits, and says where the result is. Three lowerings of the test
/// target (`crates/hod_cg/tests/common/target.rs`), whose machine has
/// instructions `BIN op, bits, dst, a, b` with register-or-slot
/// operands, slightly shortened:
///
/// ```ignore
/// // %r = add i32 %a, %b                         (class Gpr)
/// fn binary(cx: Cx, inst: Inst, lhs: Value, rhs: Value) -> CgResult<()> {
///     // Computed into the first operand's register: it is dead after
///     // this instruction, or a temporary.
///     let a = cx.use_reg(lhs)?;                 // in a register, pinned
///     let b = if matches!(cx.loc(rhs)?, Loc::Frame(_)) {
///         mem(cx.use_mem(rhs)?)                 // read from its slot
///     } else {
///         reg(cx.use_reg(rhs)?)
///     };
///     cx.put_ins(Enc::op(isa::BIN).u8(op).u8(bits).opnd(reg(a)).opnd(reg(a)).opnd(b));
///     cx.finish(inst, a)                        // §4.1: keep it there, or store it to its home
/// }
///
/// // %c = icmp ult i32 %a, %b                    (§4.6: deferred when it may be)
/// fn icmp(cx: Cx, inst: Inst, pred: IntPredicate, lhs: Value, rhs: Value) -> CgResult<()> {
///     if cx.may_defer(inst) {
///         return cx.defer(inst);                // emit nothing; the operands stay where they are
///     }
///     let (bits, a, b) = compare_operands(cx, lhs, rhs)?;
///     let d = cx.def_reg(inst)?;
///     cx.put_ins(Enc::op(isa::ICMP).u8(pred as u8).u8(bits).u8(d.index()).opnd(a).opnd(b));
///     cx.finish(inst, d)
/// }
///
/// // br i1 %c, label %t, label %f                (a deferred compare fused into JCMP)
/// fn cond_br(cx: Cx, cond: Value, if_true: BlockId, if_false: BlockId) -> CgResult<()> {
///     let to = cx.block_label(if_true);
///     if let Some(compare) = cx.deferred(cond)
///         && let InstKind::ICmp { pred, lhs, rhs } = compare.kind()
///     {
///         let (bits, a, b) = compare_operands(cx, lhs, rhs)?;   // the compare's own operands
///         branch(cx, isa::JCMP, |ins| ins.u8(pred as u8).u8(bits).opnd(a).opnd(b), to)?;
///     } else {
///         let c = cx.use_reg(cond)?;            // not deferred (or: asking emits it now)
///         branch(cx, isa::JNZ, |ins| ins.u8(c.index()), to)?;
///     }
///     if !cx.falls_through(if_false) {
///         cx.target.jump(cx, cx.block_label(if_false))?;
///     }
///     Ok(())
/// }
/// ```
///
/// Before any lowering runs, the test target asks for an operand that
/// is [`deferred`](FnCx::deferred) and that the instruction does not
/// fuse, so that it is emitted while no other operand holds a register.
///
/// What a lowering must not do: keep a register across a call it makes
/// (a call clobbers every register; pass the value as an operand of the
/// call), pin more registers of a class than the pool's minimum (§2.1;
/// consume long operand lists one at a time and release with
/// [`free_temp`](FnCx::free_temp) and [`release`](FnCx::release)), or
/// leave a result in a scratch register.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools, reason = "independent per-attempt flags of the driver")]
pub struct FnCx<'a, T: Target> {
    /// The module: types, constants, symbols. Read-only while functions
    /// are compiled.
    pub module: &'a Module,
    /// The function being compiled.
    pub body: &'a Body,
    /// The target.
    pub target: &'a T,
    /// The options.
    pub options: &'a Options,
    /// The target's per-function state.
    pub state: T::FnState,
    pub(crate) s: &'a mut Scratch,
    pub(crate) out: &'a mut Chunk,
    /// Where the function's rows begin in the chunk's tables.
    pub(crate) mark: Mark,
    pub(crate) ri: RegInfo,
    /// The pool in use: the target's, or its minimum under stress.
    pub(crate) pool: RegSet,
    pub(crate) far: bool,
    /// Set when a fixup's field could not hold its displacement (§6.3).
    pub(crate) out_of_range: bool,
    /// The instruction the driver is at.
    pub(crate) cur: InstId,
    /// The block being emitted.
    pub(crate) block: BlockId,
    /// The reachable block emitted after the current one, if any.
    pub(crate) next_block: Option<BlockId>,
    /// Set by a `musttail` call: the rest of the block is not emitted.
    pub(crate) skip_rest: bool,
    /// Set while a deferred instruction is being emitted on request: it
    /// may not be deferred again.
    pub(crate) no_defer: bool,
    /// How many instructions in a row are deferred right now (§4.6).
    pub(crate) deferred_run: u32,
    pub(crate) epilogue: Label,
    /// The number of the function's CFI rows when the epilogue was bound:
    /// the rows that state the body's rules (§9.3); `u32::MAX` before.
    pub(crate) body_cfi: u32,
    pub(crate) has_pads: bool,
    /// Whether the function's block offsets are recorded (§9.6).
    pub(crate) block_addrs: bool,
    pub(crate) frame_desc: FrameDesc,
    /// The largest alignment a data island asked for.
    pub(crate) island_align: u32,
    /// The attributes of the function's return value.
    pub(crate) ret_attrs: ParamAttrs,
    /// Whether line rows are recorded, and the location of the last
    /// instruction that emitted code (§9.5).
    pub(crate) lines: bool,
    pub(crate) last_loc: Option<LocId>,
    /// `ret`s emitted, and the location of the last one (§9.5).
    pub(crate) rets: u32,
    pub(crate) ret_loc: LocId,
}

impl<'a, T: Target> FnCx<'a, T> {
    // ---- read access ----

    /// The module's type table.
    #[inline]
    pub fn types(&self) -> &'a hod_ir::TypeTable {
        &self.module.types
    }

    /// The instruction the driver is at: the one being lowered, or the
    /// one that asked for a deferred instruction.
    #[inline]
    pub fn inst(&self) -> Inst<'a> {
        self.body.inst(self.cur)
    }

    /// The target's registers.
    #[inline]
    pub fn reg_info(&self) -> &RegInfo {
        &self.ri
    }

    /// Whether the function is being compiled in far mode (§6.3): the
    /// target must then emit only branch forms, island loads and block
    /// addresses that reach the whole function.
    #[inline]
    pub fn far(&self) -> bool {
        self.far
    }

    // ---- code ----

    /// The current code offset, from the start of the function.
    #[inline]
    pub fn offset(&self) -> u32 {
        self.out.code.len() as u32 - self.mark.code
    }

    /// Appends bytes to the function's code.
    #[inline]
    pub fn emit(&mut self, bytes: &[u8]) {
        self.out.code.extend_from_slice(bytes);
    }

    /// Appends one byte.
    #[inline]
    pub fn emit_u8(&mut self, byte: u8) {
        self.out.code.push(byte);
    }

    /// Appends a 16-bit word, little-endian.
    #[inline]
    pub fn emit_u16(&mut self, word: u16) {
        self.out.code.extend_from_slice(&word.to_le_bytes());
    }

    /// Appends a 32-bit word, little-endian: an AArch64 instruction.
    #[inline]
    pub fn emit_u32(&mut self, word: u32) {
        self.out.code.extend_from_slice(&word.to_le_bytes());
    }

    /// Appends a 64-bit word, little-endian.
    #[inline]
    pub fn emit_u64(&mut self, word: u64) {
        self.out.code.extend_from_slice(&word.to_le_bytes());
    }

    /// The function's code so far, for patching already emitted bytes
    /// (the prologue, in `end_function`).
    #[inline]
    pub fn code_mut(&mut self) -> &mut [u8] {
        &mut self.out.code[self.mark.code as usize..]
    }

    /// The function's code so far.
    #[inline]
    pub fn code(&self) -> &[u8] {
        &self.out.code[self.mark.code as usize..]
    }

    /// Appends the target's trap instruction (§4.7).
    #[inline]
    pub fn trap(&mut self) {
        self.out.code.extend_from_slice(T::TRAP);
    }

    // ---- labels ----

    /// The label of block `block`: bound when the block is emitted.
    #[inline]
    pub fn block_label(&self, block: BlockId) -> Label {
        Label(block.raw())
    }

    /// The label of the epilogue, which `end_function` binds. `ret`
    /// branches to it unless [`FnCx::ret_falls_through`].
    #[inline]
    pub fn epilogue(&self) -> Label {
        self.epilogue
    }

    /// A new label local to a lowering, a tail exit or an island.
    #[inline]
    pub fn new_label(&mut self) -> Label {
        self.s.labels.create()
    }

    /// Binds `label` to the current offset and patches the fixups that
    /// wait for it (§6.2).
    ///
    /// # Errors
    /// `Internal` when a fixup's field cannot hold its displacement: the
    /// driver then compiles the function again in far mode (§6.3). Also
    /// for a label bound twice.
    pub fn bind(&mut self, label: Label) -> CgResult<()> {
        let at = self.offset();
        if label == self.epilogue {
            self.body_cfi = self.out.cfi.len() as u32 - self.mark.cfi;
        }
        let code = &mut self.out.code[self.mark.code as usize..];
        match self.s.labels.bind(label, at, |at, kind, disp| T::patch_branch(code, at, kind, disp))
        {
            Ok(true) => Ok(()),
            Ok(false) => internal(format!("label {} is bound twice", label.0)),
            Err(_) => self.branch_out_of_range(),
        }
    }

    /// Records that the field of `kind` at offset `at`, already emitted
    /// with a placeholder, refers to `label`. A bound label is patched
    /// in at once (`Target::patch_branch`), an unbound one when it is
    /// bound.
    ///
    /// # Errors
    /// `Internal` when the field cannot hold the displacement (§6.3), as
    /// for [`FnCx::bind`].
    pub fn fixup(&mut self, at: u32, kind: FixupKind, label: Label) -> CgResult<()> {
        if label.0 >= self.s.labels.len() {
            return internal(format!("label {} is not a label of this function", label.0));
        }
        let code = &mut self.out.code[self.mark.code as usize..];
        match self
            .s
            .labels
            .refer(label, at, kind, |at, kind, disp| T::patch_branch(code, at, kind, disp))
        {
            Ok(()) => Ok(()),
            Err(_) => self.branch_out_of_range(),
        }
    }

    /// Binds `label` to offset `at`, which is not the current one: the
    /// trap that stands for several unreachable blocks (§6.1).
    pub(crate) fn bind_at(&mut self, label: Label, at: u32) -> CgResult<()> {
        let code = &mut self.out.code[self.mark.code as usize..];
        match self.s.labels.bind(label, at, |at, kind, disp| T::patch_branch(code, at, kind, disp))
        {
            Ok(_) => Ok(()),
            Err(_) => self.branch_out_of_range(),
        }
    }

    #[cold]
    fn branch_out_of_range(&mut self) -> CgResult<()> {
        self.out_of_range = true;
        internal("branch out of range")
    }

    /// The offset `label` is bound to, if it is bound.
    #[inline]
    pub fn label_offset(&self, label: Label) -> Option<u32> {
        self.s.labels.offset(label)
    }

    /// Whether `block` is the block emitted right after the current one:
    /// a branch to it is omitted (§6.2).
    #[inline]
    pub fn falls_through(&self, block: BlockId) -> bool {
        self.next_block == Some(block)
    }

    /// Whether the current block is the last one emitted, so that a
    /// `ret` at its end falls through into the epilogue (§5.4).
    #[inline]
    pub fn ret_falls_through(&self) -> bool {
        self.next_block.is_none()
    }

    /// Whether `block` can be reached (§6.1). Unreachable blocks are not
    /// emitted.
    #[inline]
    pub fn is_reachable(&self, block: BlockId) -> bool {
        self.s.reachable.get(block.index()).copied().unwrap_or(false)
    }

    // ---- frame ----

    /// A new frame slot of `size` bytes aligned to `align`, a power of
    /// two up to the stack alignment (§5.1). Never reused.
    ///
    /// # Errors
    /// `Unsupported("limit:frame-size")`; `Internal` for the alignment.
    pub fn alloc_slot(&mut self, size: u64, align: u32) -> CgResult<FrameRef> {
        self.s.frame.alloc(size, align)
    }

    /// Where body slot `slot` is (§5.1): what `Value::slot` means.
    ///
    /// # Errors
    /// `Internal` for a slot the body does not have.
    pub fn slot_place(&self, slot: SlotId) -> CgResult<SlotPlace> {
        match self.s.slots.get(slot.index()) {
            Some(&place) => Ok(place),
            None => internal(format!("{slot:?} is not a slot of this body")),
        }
    }

    /// The size of the local area so far; final in `end_function`.
    #[inline]
    pub fn frame_size(&self) -> u32 {
        self.s.frame.size()
    }

    // ---- records ----

    /// Adds a relocation at `offset` of the function's code (§9.2).
    #[inline]
    pub fn reloc(&mut self, offset: u32, kind: RelocKind, sym: SymRef, addend: i64) {
        self.out.relocs.push(Reloc { offset, kind, sym, addend });
    }

    /// Adds a call-frame instruction that holds from the current offset
    /// (§9.3). The target emits the rows of its prologue and of its exit
    /// sequences.
    #[inline]
    pub fn cfi(&mut self, op: CfiOp) {
        let offset = self.offset();
        self.out.cfi.push(CfiRow { offset, op });
    }

    /// Adds a call-frame instruction that holds from `offset`, an offset
    /// of code already emitted (§9.3): for the rows of a prologue that
    /// `begin_function` emitted before the frame size its rows state was
    /// known, added in `end_function`. Rows stay in offset order, so
    /// `offset` is at least that of the function's last row and at most
    /// the current offset.
    ///
    /// # Errors
    /// `Internal` for an offset out of that order.
    pub fn cfi_at(&mut self, offset: u32, op: CfiOp) -> CgResult<()> {
        let last = self.out.cfi[self.mark.cfi as usize..].last().map_or(0, |row| row.offset);
        if offset < last || offset > self.offset() {
            return internal(format!(
                "a CFI row at {offset}, out of order (last row at {last}, code at {})",
                self.offset()
            ));
        }
        self.out.cfi.push(CfiRow { offset, op });
        Ok(())
    }

    /// Sets the function's frame description (§9.3), in `end_function`.
    #[inline]
    pub fn set_frame_desc(&mut self, desc: FrameDesc) {
        self.frame_desc = desc;
    }

    /// Places `bytes` in a data island after the function's code,
    /// aligned to `align` (a power of two), and returns its label, for a
    /// PC-relative load through [`FnCx::fixup`] (§7.5).
    ///
    /// # Errors
    /// `Internal` for an alignment that is not a power of two.
    pub fn island(&mut self, bytes: &[u8], align: u32) -> CgResult<Label> {
        if !align.is_power_of_two() {
            return internal(format!("a data island aligned to {align}"));
        }
        let label = self.s.labels.create();
        let start = self.s.island_data.len() as u32;
        self.s.island_data.extend_from_slice(bytes);
        self.s.islands.push(Island { label, start, len: bytes.len() as u32, align });
        self.island_align = self.island_align.max(align);
        Ok(label)
    }

    /// The type id of type-info symbol `sym` in this function's
    /// type-info table (§9.4): its index + 1, in order of first mention.
    /// `None` is catch-all. What `llvm.eh.typeid.for` lowers to.
    pub fn type_id(&mut self, sym: Option<SymId>) -> u32 {
        let table = &self.out.type_ids[self.mark.type_ids as usize..];
        if let Some(index) = table.iter().position(|&entry| entry == sym) {
            return index as u32 + 1;
        }
        self.out.type_ids.push(sym);
        self.out.type_ids.len() as u32 - self.mark.type_ids
    }

    /// Adds a `catch` clause for type-info symbol `sym` (`None`:
    /// catch-all) to the landing pad of the block being emitted (§9.4).
    /// The `LandingPad` lowering calls this for each clause, in order.
    ///
    /// # Errors
    /// `Internal` when the current block is not a landing pad.
    pub fn pad_clause(&mut self, sym: Option<SymId>) -> CgResult<()> {
        let id = self.type_id(sym);
        self.push_clause(ClauseRow::Catch(id))
    }

    /// Adds a `filter` clause listing `syms` to the landing pad of the
    /// block being emitted (§9.4).
    ///
    /// # Errors
    /// `Internal` when the current block is not a landing pad.
    pub fn pad_filter(&mut self, syms: &[Option<SymId>]) -> CgResult<()> {
        let start = self.out.filter_ids.len() as u32 - self.mark.filter_ids;
        for &sym in syms {
            let id = self.type_id(sym);
            self.out.filter_ids.push(id);
        }
        let end = self.out.filter_ids.len() as u32 - self.mark.filter_ids;
        self.push_clause(ClauseRow::Filter(start..end))
    }

    fn push_clause(&mut self, clause: ClauseRow) -> CgResult<()> {
        let at = self.out.clauses.len() as u32 - self.mark.clauses;
        let pads = &mut self.out.pads[self.mark.pads as usize..];
        let Some(pad) = pads.last_mut().filter(|pad| pad.clauses.end == at) else {
            return internal("a landing-pad clause outside a landing pad");
        };
        pad.clauses.end = at + 1;
        self.out.clauses.push(clause);
        Ok(())
    }

    /// Starts the pad row of landing-pad block `block`, at the current
    /// offset.
    pub(crate) fn begin_pad(&mut self, block: BlockId, cleanup: bool) {
        let clauses = self.out.clauses.len() as u32 - self.mark.clauses;
        let offset = self.offset();
        self.out.pads.push(PadRow { block, offset, cleanup, clauses: clauses..clauses });
        let index = self.out.pads.len() as u32 - self.mark.pads;
        if let Some(entry) = self.s.pad_of_block.get_mut(block.index()) {
            *entry = index;
        }
    }

    /// Emits the data islands after the function's exit sequences
    /// (§7.5).
    pub(crate) fn emit_islands(&mut self) -> CgResult<()> {
        for index in 0..self.s.islands.len() {
            let island = self.s.islands[index];
            let at = self.offset().next_multiple_of(island.align);
            let pad = (at - self.offset()) as usize;
            self.out.code.extend(std::iter::repeat_n(T::PAD_BYTE, pad));
            self.bind(island.label)?;
            let data = island.start as usize..(island.start + island.len) as usize;
            self.out.code.extend_from_slice(&self.s.island_data[data]);
            self.out.islands.push(IslandRow { offset: at, len: island.len });
        }
        Ok(())
    }

    /// A `CgError::Internal` that names the instruction being lowered.
    #[cold]
    pub fn bug<R>(&self, what: &str) -> CgResult<R> {
        Err(CgError::Internal(format!(
            "{what} (at {:?} = {})",
            self.cur,
            self.inst().opcode().name()
        )))
    }
}
