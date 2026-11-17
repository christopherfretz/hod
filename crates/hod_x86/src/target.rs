//! `X86`, the i686 Linux implementation of `hod_cg::Target`
//! (`spec/codegen-x86.md`): the register description, the value classes,
//! the glue, and the one dispatch of every instruction. Owned by WP-293.
//!
//! # Dispatch
//!
//! [`Target::lower`] is one `match`. A value of class `Gpr` (`i1`..`i32`,
//! `ptr`) goes to `alu`, `mem` or `control`; every other value is class
//! `Mem` and is refused by the family that owns it ([`refusal_of`]):
//! `x86:i64` (`int`, I7), `x86:float` (`x87`, I9), `x86:vector` (I12),
//! `x86:aggregate` (I11). A package that lowers a family replaces its
//! refusal arm here with a call into its module.

use hod_cg::abi::{AbiUnsupported, Assignment, Sig};
use hod_cg::const_fold::Action as FoldAction;
use hod_cg::{
    CallEmit, CgError, CgResult, Class, FixupKind, FnCx, FrameDesc, FrameRef, Label, PReg, RegInfo,
    RegSet, Target,
};
use hod_ir::{
    Body, FnStr, Inst, InstKind, Module, Opcode, ParamAttrs, SymbolKind, TypeId, TypeTable, Value,
    X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Size, Xmm};

use crate::common::{self, gp, put, unsupported};
use crate::reg::{EAX, EBX, ECX, EDI, EDX, ESI, xmm};
use crate::{abi, alu, call, control, frame, int, mem};

/// A vector value: design 0008's I12.
pub const VECTOR: &str = "x86:vector";
/// An aggregate value (`extractvalue`, `insertvalue`, a struct or array
/// in a register position), a result returned in memory: I11.
pub const AGGREGATE: &str = "x86:aggregate";
/// A funclet pad (Windows exception handling).
pub const FUNCLET: &str = "x86:funclet";

/// The i686 Linux target (i386 System V psABI, every relocation model).
/// `cpu` is the command line's `-mcpu`, the CPU of a function without a
/// `target-cpu` attribute (as `llc` applies it); [`X86`] the target
/// without one (`generic`).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct X86 {
    cpu: Option<&'static str>,
    /// The features of the function being compiled (`for_function`).
    features: Option<X86Features>,
    /// WP-335: the function is a coroutine body, `esi` its frame base
    /// (`crate::coro_body`), set by `for_function`.
    pub coro: bool,
}

/// The target without `-mcpu`.
#[allow(non_upper_case_globals, reason = "the value of the former unit struct, kept by name")]
pub const X86: X86 = X86 { cpu: None, features: None, coro: false };

impl X86 {
    /// The target for a compile with `-mcpu` `cpu`: a name of
    /// `hod_ir::I386_CPUS` (another name is `llc`'s "not a recognized
    /// processor": ignored, `generic`).
    #[must_use]
    pub fn with_cpu(cpu: Option<&str>) -> X86 {
        let known = cpu.and_then(|cpu| {
            hod_ir::I386_CPUS.iter().find(|&&(name, _)| name == cpu).map(|&(name, _)| name)
        });
        X86 { cpu: known, features: None, coro: false }
    }

    /// The `-mcpu` CPU, if any.
    #[must_use]
    pub const fn cpu(&self) -> Option<&'static str> {
        self.cpu
    }

    /// The feature set of the function being compiled (its `target-cpu`
    /// and `target-features`, `-mcpu` as the fallback); of `-mcpu` alone
    /// outside a function.
    #[must_use]
    pub fn features(&self) -> X86Features {
        self.features.unwrap_or_else(|| feature_set(None, None, self.cpu))
    }
}

/// `X86Features::of_function`, an unknown name as no feature.
fn feature_set(cpu: Option<&str>, features: Option<&str>, mcpu: Option<&str>) -> X86Features {
    X86Features::of_function(cpu, features, mcpu, &[]).unwrap_or(X86Features::NONE)
}

/// What the target remembers about the function being compiled.
#[derive(Clone, Default, Debug)]
pub struct FnState {
    /// The frame: the prologue's patched immediate.
    pub frame: frame::FrameState,
    /// The 80-bit copies of `float`/`double` results (I9).
    pub x87: crate::x87::Copies,
    /// The 80-bit values of SSE call results (I12).
    pub sse: crate::sse::Views,
    /// The stack guard (WP-324).
    pub ssp: crate::ssp::GuardState,
    /// The coroutine stubs' labels (WP-335).
    pub coro: crate::coro_body::CoroState,
}

/// The registers: pool and holdable `ebx`, `esi`, `edi` (and `xmm0`..
/// `xmm5`, which no value of this package uses); scratch `ecx`, `edx`,
/// `xmm6`, `xmm7`. `eax` is a general register that never holds a value
/// and that no glue writes: the dividend, `select`'s condition, a
/// `ptradd` index, the result. The callee register is `edx`; landing-pad
/// data `eax`, `edx` (unused until I10).
pub const REG_INFO: RegInfo = RegInfo {
    gpr: RegSet::of(&[EAX, ECX, EDX, EBX, ESI, EDI]),
    fpr: RegSet::of(&[xmm(0), xmm(1), xmm(2), xmm(3), xmm(4), xmm(5), xmm(6), xmm(7)]),
    pool: RegSet::of(&[EBX, ESI, EDI, xmm(0), xmm(1), xmm(2), xmm(3), xmm(4), xmm(5)]),
    holdable: RegSet::of(&[EBX, ESI, EDI, xmm(0), xmm(1), xmm(2), xmm(3), xmm(4), xmm(5)]),
    scratch: RegSet::of(&[ECX, EDX, xmm(6), xmm(7)]),
    eh_data: [EAX, EDX],
    callee: EDX,
    gpr_bytes: 4,
    fpr_bytes: 16,
    dwarf: crate::reg::dwarf,
};

/// The refusal of a value of type `ty` that is no `Gpr` value.
pub fn refusal_of(cx: &FnCx<'_, X86>, ty: TypeId) -> CgError {
    CgError::unsupported(abi::type_refusal(cx.types(), ty))
}

/// The refusal for an instruction whose result (or, for `void`, the
/// operand `value`) is no `Gpr` value.
fn refuse<R>(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<R> {
    Err(refusal_of(cx, ty))
}

/// Whether `vector::owns` or `sse::owns` takes `inst` (WP-317, §16 and
/// §17; [`sse_family`] asks `vector` first : a vector `fcmp`/`bitcast` is not scalar).
/// `Target::lower` asks after `atomic`'s arms (an ordered access of a
/// `float` or a vector is `atomic`'s) and before `x87`'s and `agg`'s;
/// neither owner claims `extractvalue`/`insertvalue` or an aggregate.
fn sse_family_owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    crate::vector::owns(cx, inst) || crate::sse::owns(cx, inst)
}

/// Lowers an instruction [`sse_family_owns`] takes.
fn sse_family<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    if crate::vector::owns(cx, inst) {
        crate::vector::lower(cx, inst)
    } else {
        crate::sse::lower(cx, inst)
    }
}

/// `extractvalue`: of the `{ ptr, i32 }` landing-pad value (`eh`, §14),
/// else the general aggregate's (`agg`, §13.2). The layouts are the same
/// (`eax`'s word at 0, `edx`'s at 4); `atomic`'s `cmpxchg` arm comes
/// before both.
fn extract_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    if crate::eh::owns_aggregate(cx, inst) {
        crate::eh::extract_value(cx, inst, agg, offset)
    } else {
        crate::agg::extract_value(cx, inst, agg, offset)
    }
}

/// `insertvalue`: as [`extract_value`].
fn insert_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    if crate::eh::owns_aggregate(cx, inst) {
        crate::eh::insert_value(cx, inst, agg, value, offset)
    } else {
        crate::agg::insert_value(cx, inst, agg, value, offset)
    }
}

/// The cast arm: both sides `Gpr`, or the refusal of the side that is
/// not.
fn lower_cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let from = common::value_ty(cx, arg);
    let types = cx.types();
    if !abi::is_gpr(types, from) {
        return refuse(cx, from);
    }
    if !abi::is_gpr(types, inst.ty()) {
        return refuse(cx, inst.ty());
    }
    alu::cast(cx, inst, arg)
}

impl Target for X86 {
    type FnState = FnState;

    /// `ud2`, as `llc` emits for `unreachable` and `llvm.trap`.
    const TRAP: &'static [u8] = &[0x0f, 0x0b];
    const FN_ALIGN: u32 = 16;
    /// `nop`, as `llc` pads i386 code.
    const PAD_BYTE: u8 = 0x90;
    const STACK_ALIGN: u32 = 16;
    /// WP-327: `ret` (`crate::coro`).
    const NOOP_CORO_FN: &'static [u8] = crate::coro::NOOP_FN;
    /// WP-335: `crate::coro_body`.
    const CORO_BODIES: bool = true;

    fn reg_info(&self) -> &RegInfo {
        // WP-335: a coroutine's frame base `esi` is not in its pool.
        crate::coro_body::reg_info(self.coro)
    }

    #[inline]
    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class {
        if abi::is_gpr(types, ty) { Class::Gpr } else { Class::Mem }
    }

    fn assign(
        &self,
        types: &TypeTable,
        sig: &Sig<'_>,
        out: &mut Assignment,
    ) -> Result<(), AbiUnsupported> {
        abi::assign_for(types, sig, out, self.features())
    }

    fn for_function(&self, module: &Module, body: &Body) -> Option<Self> {
        let SymbolKind::Function { attrs, .. } = module.symbol(body.symbol()).kind() else {
            return None;
        };
        let attrs = module.fn_attrs(attrs);
        let text = |what| attrs.string(what).and_then(|s| std::str::from_utf8(s).ok());
        let features = feature_set(text(FnStr::TargetCpu), text(FnStr::TargetFeatures), self.cpu);
        // WP-335: a coroutine body keeps `esi` as its frame base.
        let coro = attrs.has(hod_ir::FnAttrs::PRESPLIT_COROUTINE);
        Some(X86 { cpu: self.cpu, features: Some(features), coro })
    }

    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        crate::pic::check(cx)?;
        crate::dynstack::check(cx)?;
        frame::begin(cx)
    }

    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        frame::end(cx)
    }

    fn frame_base_offset(&self, desc: &FrameDesc, place: FrameRef) -> Option<i64> {
        frame::base_offset(desc, place)
    }

    #[allow(clippy::too_many_lines, reason = "one dispatch match over the families")]
    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()> {
        match cx.fold_action(inst) {
            // An `invoke` the fold turns into a call, or one that loses
            // its normal edge, is the call sequence's (WP-270, WP-314).
            FoldAction::Emit | FoldAction::Call(_) | FoldAction::NoNormal => {}
            FoldAction::Drop => return Ok(()),
            FoldAction::Jump(target) => return control::br(cx, target),
            FoldAction::Unreachable => return control::unreachable(cx),
        }
        let gpr = abi::is_gpr(cx.types(), inst.ty());
        match inst.kind() {
            InstKind::Load { ptr, mem } if mem.ordering().is_some() => {
                crate::atomic::load(cx, inst, ptr, mem)
            }
            InstKind::Store { value, ptr, mem } if mem.ordering().is_some() => {
                crate::atomic::store(cx, inst, value, ptr, mem)
            }
            InstKind::ExtractValue { agg, offset } if crate::atomic::is_cmpxchg_result(cx, agg) => {
                crate::atomic::extract(cx, inst, agg, offset)
            }
            _ if crate::fpclass::owns(cx, inst) => crate::fpclass::lower(cx, inst),
            // I12: after `atomic`'s arms, before x87's; no aggregate's.
            _ if sse_family_owns(cx, inst) => sse_family(cx, inst),
            _ if crate::x87::owns(cx, inst) => crate::x87::lower(cx, inst),
            _ if crate::agg::owns(cx, inst) => crate::agg::lower(cx, inst),
            InstKind::Binary { lhs, rhs } if gpr => alu::binary(cx, inst, lhs, rhs),
            InstKind::Binary { lhs, rhs } if int::is_wide(cx, inst.result()) => {
                int::binary(cx, inst, lhs, rhs)
            }
            InstKind::Unary { arg }
                if int::is_wide(cx, inst.result()) && inst.opcode() == Opcode::Freeze =>
            {
                int::freeze(cx, inst, arg)
            }
            InstKind::Cast { arg } if int::is_cast(cx, inst, arg) => int::cast(cx, inst, arg),
            InstKind::ICmp { pred, lhs, rhs } if int::is_wide(cx, lhs) => {
                int::icmp(cx, inst, pred, lhs, rhs)
            }
            InstKind::Store { value, ptr, mem } if int::is_wide(cx, value) => {
                int::store(cx, inst, value, ptr, mem)
            }
            InstKind::Select { cond, if_true, if_false } if int::is_wide(cx, inst.result()) => {
                int::select(cx, inst, cond, if_true, if_false)
            }
            InstKind::Unary { arg } if gpr && inst.opcode() != Opcode::FNeg => {
                alu::freeze(cx, inst, arg)
            }
            InstKind::Cast { arg } => lower_cast(cx, inst, arg),
            InstKind::ICmp { pred, lhs, rhs } => {
                let ty = common::value_ty(cx, lhs);
                if abi::is_gpr(cx.types(), ty) && gpr {
                    alu::icmp(cx, inst, pred, lhs, rhs)
                } else {
                    refuse(cx, ty)
                }
            }
            InstKind::FCmp { .. } => unsupported(crate::x87::FLOAT),
            InstKind::Load { ptr, mem } if gpr => mem::load(cx, inst, ptr, mem),
            InstKind::Load { ptr, mem } if int::is_wide(cx, inst.result()) => {
                int::load(cx, inst, ptr, mem)
            }
            InstKind::Store { value, ptr, mem } => {
                let ty = common::value_ty(cx, value);
                if abi::is_gpr(cx.types(), ty) {
                    mem::store(cx, inst, value, ptr, mem)
                } else {
                    refuse(cx, ty)
                }
            }
            InstKind::Alloca { count, elem_size, align } => {
                crate::dynstack::alloca(cx, inst, count, elem_size, align)
            }
            InstKind::PtrAdd(add) => mem::ptr_add(cx, inst, &add),
            InstKind::ExtractValue { agg, offset } => extract_value(cx, inst, agg, offset),
            InstKind::InsertValue { agg, value, offset } => {
                insert_value(cx, inst, agg, value, offset)
            }
            InstKind::Select { cond, if_true, if_false } if gpr => {
                alu::select(cx, inst, cond, if_true, if_false)
            }
            // The result is no `Gpr` value.
            InstKind::Binary { .. }
            | InstKind::Unary { .. }
            | InstKind::Load { .. }
            | InstKind::Select { .. } => refuse(cx, inst.ty()),
            InstKind::Call(site) => call::lower(cx, inst, &site),
            InstKind::VaArg { list } => crate::va::va_arg(cx, inst, list),
            InstKind::LandingPad(pad) => crate::eh::landing_pad(cx, inst, &pad),
            InstKind::Resume { value } => crate::eh::resume(cx, value),
            InstKind::Br { target } => control::br(cx, target),
            InstKind::CondBr { cond, if_true, if_false } => {
                control::cond_br(cx, cond, if_true, if_false)
            }
            InstKind::Switch(switch) => control::switch(cx, &switch),
            InstKind::IndirectBr { addr, .. } => crate::ibr::indirect_br(cx, inst, addr),
            InstKind::Ret { value } => control::ret(cx, inst, value),
            InstKind::Unreachable => control::unreachable(cx),
            InstKind::Fence { .. } | InstKind::CmpXchg { .. } | InstKind::AtomicRmw { .. } => {
                crate::atomic::lower(cx, inst)
            }
            InstKind::ExtractElement { .. }
            | InstKind::InsertElement { .. }
            | InstKind::ShuffleVector { .. } => unsupported(VECTOR),
            InstKind::Funclet => unsupported(FUNCLET),
            InstKind::Phi(_) => cx.bug("a phi reached the lowering"),
        }
    }

    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32) {
        if reg == abi::ST0 {
            return crate::x87::load_ret(cx, src, bytes);
        }
        if !crate::reg::is_gpr(reg) {
            let x = Xmm::ALL[usize::from(reg.index() - xmm(0).index())];
            return crate::sse::load_xmm(cx, x, src, bytes);
        }
        // Only general registers hold values of this package.
        common::load_gpr(cx, gp(reg), src, bytes);
    }

    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32) {
        if reg == abi::ST0 {
            return crate::x87::store_call_result(cx, dst, bytes);
        }
        if !crate::reg::is_gpr(reg) {
            let x = Xmm::ALL[usize::from(reg.index() - xmm(0).index())];
            return crate::sse::store_xmm(cx, x, dst, bytes);
        }
        common::store_gpr(cx, gp(reg), dst, bytes);
    }

    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32) {
        if dst == src || bytes == 0 {
            return;
        }
        if let FrameRef::Incoming(_) = dst {
            // WP-324: a `musttail` call's arguments go to the real area.
            return crate::ssp::raw_incoming(cx, |cx| common::copy_frame(cx, dst, src, bytes));
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
        if dst != src {
            put(cx, mc::mov_rr(Size::D, gp(dst), gp(src)));
        }
    }

    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()> {
        if !crate::reg::is_gpr(reg) {
            return unsupported(crate::x87::FLOAT);
        }
        common::materialize_gpr(cx, gp(reg), value)
    }

    fn materialize_mem(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        value: Value,
    ) -> CgResult<()> {
        if int::is_wide(cx, value) {
            return int::materialize_mem(cx, dst, value);
        }
        if crate::agg::is_aggregate(cx, common::value_ty(cx, value)) {
            return crate::agg::materialize_mem(cx, dst, value);
        }
        if cx.types().is_vector(common::value_ty(cx, value)) {
            return crate::vector::materialize_mem(cx, dst, value);
        }
        if crate::sse::is_sse_or_half(cx, common::value_ty(cx, value)) {
            return crate::sse::materialize_mem(cx, dst, value);
        }
        // Floating point; any other type is refused by its family there.
        crate::x87::materialize_mem(cx, dst, value)
    }

    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef) {
        common::frame_address(cx, gp(reg), of);
    }

    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64) {
        frame::adjust_sp(cx, delta);
    }

    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs) {
        let Some(bits) = cx.types().int_bits(ty) else { return };
        let ext = if attrs.has(ParamAttrs::SIGNEXT) {
            alu::Ext::Sign
        } else if attrs.has(ParamAttrs::ZEROEXT) {
            alu::Ext::Zero
        } else {
            return;
        };
        alu::extend_to(cx, gp(reg), bits, ext);
    }

    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()> {
        common::branch(cx, label)
    }

    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()> {
        // WP-314: the call returns right after its instruction, before
        // `esp` is moved back over a popped hidden pointer
        // (`spec/dwarf.md` §6.19, as on the other targets).
        crate::pic::emit_call(cx, call.target, 0)?;
        cx.call_returns_here(call.target, crate::reg::dwarf);
        // WP-324: `endbr32` right after the call instruction, before the
        // stack adjustment, as `llc`'s IBT pass places it.
        crate::cet::after_call(cx, call.target);
        if call.callee_pop != 0 {
            frame::adjust_sp(cx, -i64::from(call.callee_pop));
        }
        Ok(())
    }

    fn branch_target(&self, cx: &mut FnCx<'_, Self>, _at: hod_cg::BranchTarget) -> CgResult<()> {
        crate::cet::landing(cx); // WP-324
        Ok(())
    }

    fn dynamic_slot(
        &self,
        cx: &mut FnCx<'_, Self>,
        ptr: FrameRef,
        size: u64,
        align: u64,
    ) -> CgResult<()> {
        crate::dynstack::dynamic_slot(cx, ptr, size, align)
    }

    fn enter_guard(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        crate::ssp::enter(cx)
    }

    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool {
        common::patch_branch(code, at, kind, displacement)
    }

    fn coro_stubs(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        crate::coro_body::stubs(cx)
    }

    fn coro_dispatch(
        &self,
        cx: &mut FnCx<'_, Self>,
        cond: PReg,
        resume: Label,
        destroy: Label,
    ) -> CgResult<()> {
        crate::coro_body::dispatch(cx, cond, resume, destroy)
    }

    fn coro_transfer(&self, cx: &mut FnCx<'_, Self>, exit: Label) -> CgResult<()> {
        crate::coro_body::transfer(cx, exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_info_is_valid() {
        REG_INFO.check().unwrap();
        assert_eq!(REG_INFO.pool.intersection(REG_INFO.gpr).len(), 3);
        for never in [EAX, ECX, EDX, crate::reg::ESP, crate::reg::EBP] {
            assert!(!REG_INFO.holdable.contains(never));
        }
        assert_eq!(X86::TRAP, mc::ud2().bytes());
    }
}
