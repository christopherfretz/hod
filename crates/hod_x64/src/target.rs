//! `X64`, the x86-64 Linux implementation of `hod_cg::Target`
//! (`spec/codegen.md` §5.6): the register description, the value
//! classes, the glue, and the one dispatch of every instruction to the
//! module of its group. Owned by WP-116.
//!
//! # Dispatch
//!
//! [`Target::lower`] is one `match` and is final: a group package fills
//! in its module's entry points and does not edit it. Where an opcode
//! serves several groups, the class of the value decides (§2.2), as on
//! AArch64, with `x86_fp80` (class `Mem`) going to `x87` and vectors of
//! class `Mem` to `lanes`:
//!
//! | instruction | `Gpr` | `Fpr` | `Mem` |
//! |---|---|---|---|
//! | binary, `freeze`, `select` | `int` | `float` | `x87` (binary), `lanes` (binary), `aggr` |
//! | cast | `int` (both sides `Gpr`) | `float` (either side `Fpr`) | `x87`, `lanes`, `aggr` (either side `Mem`) |
//! | `icmp` | `int` | (vectors: `float`) | `aggr` (the operands) |
//! | `fcmp`, `fneg` | | `float` | `x87`, `lanes` |
//! | `load`, `store` (the value's class) | `mem` | `mem` | `aggr` |
//!
//! Before the match, every instruction whose result or an operand is a
//! mask (a vector of `i1`) goes to `mask`. `alloca`, `ptradd`,
//! `llvm.stacksave`, `llvm.stackrestore` are `mem`; `extractvalue`,
//! `insertvalue` and the `*.with.overflow` intrinsics are `aggr`;
//! `condbr`, `switch`, `indirectbr` are `control`; calls, `invoke`,
//! `landingpad`, `resume`, `va_arg` and the `llvm.va_*` intrinsics are
//! `call`, a call of inline assembly is `barrier`; the floating-point
//! intrinsics are `fpintr`, the thread-local ones `tls`, the coroutine
//! handle ones `coro`, every other intrinsic `intrinsic`; `br`, `ret`
//! and `unreachable` are `term`; the vector element operations are
//! `vector`; atomics are `atomic`. Funclets are refused here.

use hod_cg::abi::{AbiUnsupported, Assignment, LaneLoc, Part, Sig};
use hod_cg::{
    CallEmit, CgResult, Class, FixupKind, FnCx, FrameDesc, FrameRef, Label, PReg, RegInfo, RegSet,
    Target,
};
use hod_ir::{
    Body, CallSite, FnStr, Inst, InstKind, Intrinsic, Module, Opcode, ParamAttrs, SymbolKind,
    TypeId, TypeKind, TypeTable, Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::Size;

use crate::common::{self, gp, put, unsupported, xm};
use crate::features::{self, FnFeatures};
use crate::reg::{self, R8, R9, RAX, RBX, RDI, RDX, RSI, XMM0, XMM8, XMM13, XMM14, XMM15};
use crate::{
    abi, aggr, atomic, barrier, call, control, coro, coro_body, float, fpintr, frame, int,
    intrinsic, lanes, mask, mem, term, tls, vector, x87,
};

/// The x86-64 Linux target (System V AMD64 psABI): constants, and what
/// the features of the function being compiled decide
/// (`features::of_function`, `spec/codegen.md` §5.6.9), which
/// [`Target::for_function`] fills in. [`X64`], the value, is a function
/// without `target-cpu` and `target-features`: the x86-64 baseline.
#[derive(Clone, Copy, Default, Debug)]
pub struct X64 {
    /// The function's features, as far as they change what hod emits.
    pub features: FnFeatures,
    /// Whether the function is a coroutine body: `rbx` is its frame base
    /// and not in its register pool (WP-137, `crate::coro_body`).
    pub coro: bool,
    /// Whether this value is a function's own target already
    /// ([`X64::of_attrs`]): [`Target::for_function`] then keeps it rather
    /// than read the function's attributes again (WP-166: a caller makes
    /// one per attribute set and compiles every function of the set with
    /// it).
    pub own: bool,
}

impl X64 {
    /// The target of a function whose attribute set is `attrs`: what its
    /// `target-cpu` and `target-features` decide, and its coroutine mark.
    pub fn of_attrs(attrs: hod_ir::FnAttrsView<'_>) -> X64 {
        let text = |what| attrs.string(what).and_then(|s| std::str::from_utf8(s).ok());
        let features = features::of_function(text(FnStr::TargetCpu), text(FnStr::TargetFeatures));
        let coro = attrs.has(hod_ir::FnAttrs::PRESPLIT_COROUTINE);
        X64 { features, coro, own: true }
    }
}

/// The target for a function without `target-cpu` and
/// `target-features` (the baseline x86-64 CPU, no AVX).
pub const X64: X64 = X64 {
    features: FnFeatures { abi: abi::Features { avx: false, avx512: false }, no_x87: false },
    coro: false,
    own: false,
};

/// What the target remembers about the function being compiled
/// (`Target::FnState`): the frame's, and one part per group module that
/// needs any. Plain data, reset before each function.
#[derive(Clone, Copy, Default, Debug)]
pub struct FnState {
    /// The frame: the prologue's patched immediate (`frame`, WP-116).
    pub frame: frame::FrameState,
}

/// The general registers `from..=to` by hardware number, `rsp` excluded.
const fn gprs(from: u8, to: u8) -> RegSet {
    let mut set = RegSet::EMPTY;
    let mut n = from;
    while n <= to {
        if n != 4 {
            set = set.with(PReg::new(n));
        }
        n += 1;
    }
    set
}

/// `xmm<from>`..=`xmm<to>`.
const fn xmms(from: PReg, to: PReg) -> RegSet {
    let mut set = RegSet::EMPTY;
    let mut n = from.index();
    while n <= to.index() {
        set = set.with(PReg::new(n));
        n += 1;
    }
    set
}

/// The registers (§5.6): pool `rbx`, `r12`..`r15`, `xmm8`..`xmm13`;
/// holdable also `rsi`, `rdi`, `r8`, `r9`, `xmm0`..`xmm7`; scratch `r10`,
/// `r11`, `xmm14`, `xmm15`. `rax`, `rcx`, `rdx` are general registers
/// that never hold a value: implicit operands (`mul`, `div`, shifts by
/// `cl`, `cmpxchg`, `al` of a variadic call) are free to use them.
/// Landing-pad data `rax`, `rdx`; the callee register `r11`.
pub const REG_INFO: RegInfo = RegInfo {
    gpr: gprs(0, 15),
    fpr: xmms(XMM0, XMM15),
    pool: RegSet::of(&[RBX, reg::R12, reg::R13, reg::R14, reg::R15]).union(xmms(XMM8, XMM13)),
    holdable: RegSet::of(&[RBX, reg::R12, reg::R13, reg::R14, reg::R15, RSI, RDI, R8, R9])
        .union(xmms(XMM0, XMM13)),
    scratch: RegSet::of(&[reg::R10, reg::R11, XMM14, XMM15]),
    eh_data: [RAX, RDX],
    callee: reg::R11,
    gpr_bytes: 8,
    fpr_bytes: 16,
    dwarf: reg::dwarf,
};

/// The class of a vector type (§2.2): `Fpr` when it fills an `xmm`
/// register's low 8 or all 16 bytes (a power-of-two count), a mask
/// `Gpr`, `Mem` otherwise. `<1 x i128>` is `Mem` (WP-152, §5.6.18): it
/// travels in two general registers, as an `i128` does.
fn vector_class(types: &TypeTable, ty: TypeId, elem: TypeId, len: u32) -> Class {
    let bits = types.size_in_bits_of(ty);
    let elem_ok = !matches!(types.kind(elem), TypeKind::Int(1));
    let wide_int = matches!(types.kind(elem), TypeKind::Int(128));
    if len.is_power_of_two() && (bits == 64 || bits == 128) && elem_ok && !wide_int {
        Class::Fpr
    } else if !elem_ok && len <= mask::MAX_MASK {
        Class::Gpr
    } else {
        Class::Mem
    }
}

/// Whether `ty` is `x86_fp80`, or a vector of `Mem` class: the two
/// families of class `Mem` that are not aggregates.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemKind {
    X87,
    Lanes,
    Aggr,
}

fn mem_kind(types: &TypeTable, ty: TypeId) -> MemKind {
    match types.kind(ty) {
        TypeKind::X86Fp80 => MemKind::X87,
        TypeKind::Vector { .. } => MemKind::Lanes,
        _ => MemKind::Aggr,
    }
}

/// The `call` arm of [`Target::lower`]: a call proper, inline assembly,
/// or the package that owns the intrinsic.
fn lower_call<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, site: &CallSite<'a>) -> CgResult<()> {
    match site.intrinsic() {
        None if matches!(site.callee_value().kind(), ValueKind::Asm(_)) => barrier::asm(cx, inst),
        None => call::call(cx, inst),
        Some(which @ (Intrinsic::VaStart | Intrinsic::VaEnd | Intrinsic::VaCopy)) => {
            call::va_intrinsic(cx, inst, which, site.args())
        }
        Some(which @ (Intrinsic::StackSave | Intrinsic::StackRestore)) => {
            mem::stack_intrinsic(cx, inst, which, site.args())
        }
        Some(
            which @ (Intrinsic::SaddWithOverflow
            | Intrinsic::UaddWithOverflow
            | Intrinsic::SsubWithOverflow
            | Intrinsic::UsubWithOverflow
            | Intrinsic::SmulWithOverflow
            | Intrinsic::UmulWithOverflow),
        ) => aggr::overflow_intrinsic(cx, inst, which, site.args()),
        Some(which @ (Intrinsic::ThreadLocalAddress | Intrinsic::ThreadPointer)) => {
            tls::intrinsic(cx, inst, which, site.args())
        }
        Some(
            which @ (Intrinsic::CoroResume
            | Intrinsic::CoroDestroy
            | Intrinsic::CoroDone
            | Intrinsic::CoroPromise
            | Intrinsic::CoroNoop),
        ) => coro::intrinsic(cx, inst, which, site.args()),
        Some(
            which @ (Intrinsic::CoroId
            | Intrinsic::CoroAlloc
            | Intrinsic::CoroBegin
            | Intrinsic::CoroEnd
            | Intrinsic::CoroFree
            | Intrinsic::CoroSize
            | Intrinsic::CoroAlign
            | Intrinsic::CoroSave
            | Intrinsic::CoroSuspend
            | Intrinsic::CoroFrame
            | Intrinsic::CoroIsInRamp
            | Intrinsic::CoroAwaitSuspendVoid
            | Intrinsic::CoroAwaitSuspendBool
            | Intrinsic::CoroAwaitSuspendHandle),
        ) => coro_body::intrinsic(cx, inst, which, site.args()),
        Some(which) if fpintr::is_fp(which) => fpintr::intrinsic(cx, inst, which, site.args()),
        Some(which) => intrinsic::intrinsic(cx, inst, which, site.args()),
    }
}

/// The cast arm of [`Target::lower`]: `x87` when either side is
/// `x86_fp80`, `lanes` for a non-`bitcast` with a `Mem`-class vector on
/// either side, then by class as on AArch64.
fn lower_cast<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    class: Class,
) -> CgResult<()> {
    let types = cx.types();
    let from = common::value_ty(cx, arg);
    let from_class = cx.class_of(arg);
    let x87 = mem_kind(types, from) == MemKind::X87 || mem_kind(types, inst.ty()) == MemKind::X87;
    let vector = (from_class == Class::Mem && mem_kind(types, from) == MemKind::Lanes)
        || (class == Class::Mem && mem_kind(types, inst.ty()) == MemKind::Lanes);
    match (from_class, class) {
        _ if x87 => x87::cast(cx, inst, arg),
        _ if vector && inst.opcode() != Opcode::BitCast => {
            vector::wide_cast(cx, inst, arg).unwrap_or_else(|| lanes::cast(cx, inst, arg))
        }
        (Class::Mem, _) | (_, Class::Mem) => aggr::cast(cx, inst, arg),
        (Class::Fpr, _) | (_, Class::Fpr) => float::cast(cx, inst, arg),
        (Class::Gpr, Class::Gpr) => int::cast(cx, inst, arg),
    }
}

impl Target for X64 {
    type FnState = FnState;

    /// `ud2`, as `llc` emits for `unreachable` and `llvm.trap`.
    const TRAP: &'static [u8] = &[0x0f, 0x0b];
    const FN_ALIGN: u32 = 16;
    /// `int3` before a data island: never executed.
    const PAD_BYTE: u8 = 0xcc;
    const STACK_ALIGN: u32 = 16;
    /// `ret`: `__NoopCoro_ResumeDestroy` (WP-137, `crate::coro`).
    const NOOP_CORO_FN: &'static [u8] = &[0xc3];
    /// WP-137: `crate::coro_body`.
    const CORO_BODIES: bool = true;

    fn reg_info(&self) -> &RegInfo {
        // WP-137: a coroutine's frame base `rbx` is not in its pool.
        coro_body::reg_info(self.coro)
    }

    #[inline]
    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class {
        match types.kind(ty) {
            TypeKind::Int(bits) if bits <= 64 => Class::Gpr,
            TypeKind::Ptr { .. } => Class::Gpr,
            TypeKind::Half
            | TypeKind::BFloat
            | TypeKind::Float
            | TypeKind::Double
            | TypeKind::Fp128 => Class::Fpr,
            TypeKind::Vector { elem, len } => vector_class(types, ty, elem, len),
            _ => Class::Mem,
        }
    }

    fn assign(
        &self,
        types: &TypeTable,
        sig: &Sig<'_>,
        out: &mut Assignment,
    ) -> Result<(), AbiUnsupported> {
        // §5.6.9: without the x87, a signature with an `x86_fp80` is the
        // function's `-x87` refusal (this runs before `begin_function`).
        if self.features.no_x87
            && (features::has_x86_fp80(types, sig.ret)
                || sig.params.iter().any(|&p| features::has_x86_fp80(types, p)))
        {
            out.clear();
            return Err(AbiUnsupported(features::NO_X87));
        }
        // The features of the function being compiled decide, for its
        // own signature and each call's (`spec/abi-x64.md` §1.3).
        abi::assign(types, sig, self.features.abi, out)?;
        // §5.1: a `float` or `double` piece in `st0`/`st1` is placed and
        // not yet lowered (WP-131).
        if out.ret.iter().any(abi::x87_float_part) {
            out.clear();
            return Err(AbiUnsupported(abi::RET_X87_PART));
        }
        call::guard(sig, out).inspect_err(|_| out.clear())
    }

    fn for_function(&self, module: &Module, body: &Body) -> Option<Self> {
        if self.own {
            return None;
        }
        let SymbolKind::Function { attrs, .. } = module.symbol(body.symbol()).kind() else {
            return None;
        };
        Some(X64::of_attrs(module.fn_attrs(attrs)))
    }

    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        if self.features.no_x87 && features::uses_x86_fp80(cx.module, cx.body) {
            return unsupported(features::NO_X87);
        }
        intrinsic::begin(cx);
        frame::begin(cx)
    }

    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        frame::end(cx)
    }

    fn frame_base_offset(&self, desc: &FrameDesc, place: FrameRef) -> Option<i64> {
        frame::base_offset(desc, place)
    }

    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()> {
        // An instruction `llc` deletes in a function that asks
        // `is.constant` or `objectsize` (`spec/codegen.md` §5.6.8).
        if intrinsic::dropped(inst) {
            return Ok(());
        }
        if let Some(done) = mask::lower(cx, inst) {
            return done;
        }
        let types = cx.types();
        let class = X64.class_of(types, inst.ty());
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => match (class, mem_kind(types, inst.ty())) {
                (Class::Gpr, _) => int::binary(cx, inst, lhs, rhs),
                (Class::Fpr, _) => float::binary(cx, inst, lhs, rhs),
                (Class::Mem, MemKind::X87) => x87::binary(cx, inst, lhs, rhs),
                (Class::Mem, MemKind::Lanes) => vector::wide_binary(cx, inst, lhs, rhs)
                    .unwrap_or_else(|| lanes::binary(cx, inst, lhs, rhs)),
                (Class::Mem, MemKind::Aggr) => aggr::binary(cx, inst, lhs, rhs),
            },
            InstKind::Unary { arg } => match (inst.opcode(), class, mem_kind(types, inst.ty())) {
                (Opcode::FNeg, Class::Fpr, _) => float::fneg(cx, inst, arg),
                (Opcode::FNeg, Class::Mem, MemKind::X87) => x87::fneg(cx, inst, arg),
                (Opcode::FNeg, Class::Mem, _) => {
                    vector::wide_fneg(cx, inst, arg).unwrap_or_else(|| lanes::fneg(cx, inst, arg))
                }
                (_, Class::Gpr, _) => int::freeze(cx, inst, arg),
                (_, Class::Fpr, _) => float::freeze(cx, inst, arg),
                (_, Class::Mem, _) => aggr::freeze(cx, inst, arg),
            },
            InstKind::Cast { arg } => lower_cast(cx, inst, arg, class),
            InstKind::ICmp { pred, lhs, rhs } => match cx.class_of(lhs) {
                Class::Gpr => int::icmp(cx, inst, pred, lhs, rhs),
                Class::Fpr => float::vector_icmp(cx, inst, pred, lhs, rhs),
                Class::Mem => aggr::icmp(cx, inst, pred, lhs, rhs),
            },
            InstKind::FCmp { pred, lhs, rhs } => {
                if mem_kind(types, common::value_ty(cx, lhs)) == MemKind::X87 {
                    x87::fcmp(cx, inst, pred, lhs, rhs)
                } else {
                    float::fcmp(cx, inst, pred, lhs, rhs)
                }
            }
            InstKind::Load { ptr, mem } if mem.ordering().is_some() => {
                atomic::load(cx, inst, ptr, mem)
            }
            InstKind::Store { value, ptr, mem } if mem.ordering().is_some() => {
                atomic::store(cx, inst, value, ptr, mem)
            }
            InstKind::Load { ptr, mem } => match class {
                Class::Mem => aggr::load(cx, inst, ptr, mem),
                _ => mem::load(cx, inst, ptr, mem),
            },
            InstKind::Store { value, ptr, mem } => match cx.class_of(value) {
                Class::Mem => aggr::store(cx, inst, value, ptr, mem),
                _ => mem::store(cx, inst, value, ptr, mem),
            },
            InstKind::Alloca { count, elem_size, align } => {
                mem::alloca(cx, inst, count, elem_size, align)
            }
            InstKind::PtrAdd(add) => mem::ptr_add(cx, inst, &add),
            InstKind::ExtractValue { agg, offset } => aggr::extract_value(cx, inst, agg, offset),
            InstKind::InsertValue { agg, value, offset } => {
                aggr::insert_value(cx, inst, agg, value, offset)
            }
            InstKind::Select { cond, if_true, if_false } => match class {
                Class::Gpr => int::select(cx, inst, cond, if_true, if_false),
                Class::Fpr => float::select(cx, inst, cond, if_true, if_false),
                Class::Mem => aggr::select(cx, inst, cond, if_true, if_false),
            },
            InstKind::Call(site) => lower_call(cx, inst, &site),
            InstKind::VaArg { list } => call::va_arg(cx, inst, list),
            InstKind::LandingPad(pad) => call::landing_pad(cx, inst, &pad),
            InstKind::Resume { value } => call::resume(cx, inst, value),
            InstKind::Br { target } => term::br(cx, target),
            InstKind::CondBr { cond, if_true, if_false } => {
                control::cond_br(cx, inst, cond, if_true, if_false)
            }
            InstKind::Switch(switch) => control::switch(cx, inst, &switch),
            InstKind::IndirectBr { addr, .. } => control::indirect_br(cx, inst, addr),
            InstKind::Ret { value } => term::ret(cx, inst, value),
            InstKind::Unreachable => term::unreachable(cx),
            InstKind::Fence { ordering, scope } => atomic::fence(cx, ordering, scope),
            InstKind::CmpXchg { ptr, expected, replacement, mem, failure, weak } => {
                atomic::cmpxchg(cx, inst, ptr, expected, replacement, mem, failure, weak)
            }
            InstKind::AtomicRmw { op, ptr, value, mem } => {
                atomic::rmw(cx, inst, op, ptr, value, mem)
            }
            InstKind::ExtractElement { vector, index } => {
                vector::extract_element(cx, inst, vector, index)
            }
            InstKind::InsertElement { vector, value, index } => {
                vector::insert_element(cx, inst, vector, value, index)
            }
            InstKind::ShuffleVector { first, second, mask } => {
                vector::shuffle_vector(cx, inst, first, second, mask)
            }
            InstKind::Funclet => unsupported(crate::tail::FUNCLET),
            InstKind::Phi(_) => cx.bug("a phi reached the lowering"),
        }
    }

    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32) {
        if reg::is_x87(reg) {
            return x87::load_part(cx, reg, src);
        }
        if reg::is_fpr(reg) {
            common::load_fpr(cx, xm(reg), src, bytes);
        } else {
            common::load_gpr(cx, gp(reg), src, bytes);
        }
    }

    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32) {
        if reg::is_x87(reg) {
            return x87::store_part(cx, reg, dst);
        }
        if reg::is_fpr(reg) {
            common::store_fpr(cx, xm(reg), dst, bytes);
        } else {
            common::store_gpr(cx, gp(reg), dst, bytes);
        }
    }

    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32) {
        if dst == src || bytes == 0 {
            return;
        }
        common::copy_frame(cx, dst, src, bytes);
    }

    fn copy_from_ptr(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        ptr: PReg,
        offset: u32,
        bytes: u32,
    ) {
        common::copy_from_ptr(cx, dst, gp(ptr), offset, bytes);
    }

    fn mov(&self, cx: &mut FnCx<'_, Self>, dst: PReg, src: PReg) {
        if dst == src {
            return;
        }
        if reg::is_fpr(dst) {
            put(cx, mc::movap_rr(mc::FpSize::D, xm(dst), xm(src)));
        } else {
            put(cx, mc::mov_rr(Size::Q, gp(dst), gp(src)));
        }
    }

    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()> {
        // WP-147: a constant mask, a vector constant with addresses.
        if let Some(done) = vector::materialize(cx, reg, value) {
            return done;
        }
        if reg::is_fpr(reg) {
            return float::materialize(cx, xm(reg), value);
        }
        match value.kind() {
            ValueKind::Const(_) | ValueKind::Sym(_) | ValueKind::Slot(_) => {
                common::materialize_gpr(cx, gp(reg), value)
            }
            _ => cx.bug("materialize of something that is no constant"),
        }
    }

    fn materialize_mem(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        value: Value,
    ) -> CgResult<()> {
        aggr::materialize_mem(cx, dst, value)
    }

    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef) {
        common::frame_address(cx, gp(reg), of);
    }

    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64) {
        frame::adjust_sp(cx, delta);
    }

    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs) {
        // `spec/abi-x64.md` §7, every width: WP-125.
        call::extend_arg(cx, gp(reg), ty, attrs);
    }

    fn lanes_out(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &Part,
        from: LaneLoc,
        to: LaneLoc,
    ) -> CgResult<()> {
        crate::vcall::lanes_out(cx, part, from, to)
    }

    fn lanes_in(
        &self,
        cx: &mut FnCx<'_, Self>,
        part: &Part,
        from: LaneLoc,
        to: LaneLoc,
    ) -> CgResult<()> {
        crate::vcall::lanes_in(cx, part, from, to)
    }

    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()> {
        common::branch(cx, label)
    }

    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()> {
        frame::emit_call(cx, call.target, call.variadic, call.vector_regs_used)?;
        cx.call_returns_here(call.target, reg::dwarf); // WP-113: `spec/dwarf.md` §6.19
        Ok(())
    }

    fn dynamic_slot(
        &self,
        cx: &mut FnCx<'_, Self>,
        ptr: FrameRef,
        size: u64,
        align: u64,
    ) -> CgResult<()> {
        frame::dynamic_slot(cx, ptr, size, align)
    }

    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool {
        common::patch_branch(code, at, kind, displacement)
    }

    fn coro_stubs(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        coro_body::stubs(cx)
    }

    fn coro_dispatch(
        &self,
        cx: &mut FnCx<'_, Self>,
        cond: PReg,
        resume: Label,
        destroy: Label,
    ) -> CgResult<()> {
        coro_body::dispatch(cx, cond, resume, destroy)
    }

    fn coro_transfer(&self, cx: &mut FnCx<'_, Self>, exit: Label) -> CgResult<()> {
        coro_body::transfer(cx, exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6: the description satisfies `hod_cg`'s invariants.
    #[test]
    fn reg_info_is_valid() {
        REG_INFO.check().unwrap();
        assert_eq!(REG_INFO.gpr.len(), 15);
        assert_eq!(REG_INFO.pool.intersection(REG_INFO.gpr).len(), 5);
        assert_eq!(REG_INFO.pool.intersection(REG_INFO.fpr).len(), 6);
        assert_eq!(REG_INFO.holdable.intersection(REG_INFO.gpr).len(), 9);
        assert_eq!(REG_INFO.holdable.intersection(REG_INFO.fpr).len(), 14);
        for never in [RAX, reg::RCX, RDX, reg::RSP] {
            assert!(!REG_INFO.holdable.contains(never));
        }
        for arg in abi::ARG_GPRS {
            assert!(REG_INFO.gpr.contains(arg));
        }
        assert_eq!(X64::TRAP, mc::ud2().bytes());
    }

    // WP-166: a function's own target (`X64::of_attrs`) is kept by
    // `for_function`, which reads the attributes only for the baseline.
    #[test]
    fn an_own_target_is_kept() {
        let text = b"target triple = \"x86_64-unknown-linux-gnu\"\n\
                     define void @f() #0 {\n  ret void\n}\n\
                     attributes #0 = { \"target-features\"=\"+avx2\" }\n";
        let parsed = hod_ll::parse_module(text, &hod_ll::ParseOptions::default()).unwrap();
        let (mut module, side) = (parsed.module, parsed.text);
        let mut bodies = Vec::new();
        hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
            bodies.push(parsed.body.expect("the body is read").clone());
            Ok::<(), hod_ll::LlError>(())
        })
        .unwrap();
        let own = X64.for_function(&module, &bodies[0]).expect("the baseline reads the features");
        assert!(own.own && own.features.abi.avx && !own.coro);
        assert!(own.for_function(&module, &bodies[0]).is_none());
        let SymbolKind::Function { attrs, .. } = module.symbol(bodies[0].symbol()).kind() else {
            panic!("f is a function")
        };
        assert_eq!(X64::of_attrs(module.fn_attrs(attrs)).features, own.features);
    }
}
