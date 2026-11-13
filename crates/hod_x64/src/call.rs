//! Calls and what goes with them (`spec/codegen.md` §7.1, §5.6.5,
//! `spec/abi-x64.md` §7, §8, §9): calls and `invoke` through `hod_cg`'s
//! call sequence, `musttail`, landing pads and `resume`, the variadic
//! register save area and the `llvm.va_*` intrinsics, the extension a
//! producer owes. `va_arg` is refused.
//!
//! **Owner: WP-125.** Tests: `tests/call.rs`. The dispatch in `target.rs`
//! is final. The call instruction itself (`mov al, n` for a variadic
//! callee, `call rel32` with `R_X86_64_PLT32`, `call r11`) and the tail
//! exits are emitted by `crate::frame`, which `hod_cg`'s call sequence
//! drives; where each argument and result travels is `crate::abi::assign`
//! (WP-118). The core keeps the no-deferral rule of §5.6.1 by itself: a
//! call emits every deferred argument before it places any, so an
//! instruction that writes `rcx`/`rdx` never runs inside the argument
//! window.
//!
//! # What this module adds to the core's call sequence
//!
//! - [`enter`], from the prologue: the address a function hands back in
//!   `rax` (an `sret` parameter, `spec/abi-x64.md` §6.1, or the hidden
//!   address of a value returned in memory, §5.2) is saved in a slot of
//!   its own, and [`before_return`] loads it into `rax` in the epilogue,
//!   which every `ret` reaches. A variadic function also saves `rdi`..`r9`,
//!   `xmm0`..`xmm7` (the 176-byte register save area, §9.3) and `al`.
//! - [`extend_arg`]: the extension of §7, for every width.
//! - After a call whose `Mem`-class result holds integer leaves whose
//!   width is not a multiple of 8 (`{ i7, i64 }`, `i65`): the bits above
//!   each such leaf are cleared in the result's home, since a producer
//!   owes nothing above a leaf (§7) and memory holds them zero
//!   (`spec/codegen.md` §2.2). An `invoke`'s are cleared by the call
//!   sequence before its branch (WP-084).
//! - A `musttail` call in a variadic function first reloads the argument
//!   registers that carry no named argument from the register save area,
//!   and its tail exit reloads `al` ([`before_tail_exit`]), so the callee
//!   receives them as this function did (§8.5); `r10` and `r11` are not
//!   passed on, and the stack area beyond the named arguments is never
//!   written.
//! - The refusals of the forms the sequence does not handle (operand
//!   bundles, a `callbr` that is not inline assembly).

use hod_cg::abi::{AbiUnsupported, Assignment, Place, Sig};
use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, RuntimeFn, Target};
use hod_ir::{
    Clause, Const, Inst, Intrinsic, LandingPad, Opcode, ParamAttrs, ParamMem, SymId, TailKind,
    TypeId, TypeKind, Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{FpSize, Mem, Reg, Size, Xmm};

use crate::common::{
    R11, deferred_first, frame_address, frame_inst, gp, load_gpr, put, store_gpr, unsupported,
    value_ty,
};
use crate::target::X64;

/// A call form this group does not lower: a `callbr` whose callee is not
/// inline assembly.
pub const CALL: &str = "x64:call";
/// A call with an operand bundle.
pub const CALL_BUNDLE: &str = "x64:call-bundle";
/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy` outside what is
/// lowered: `va_start` in a function that is not variadic, an operand
/// that is not a pointer.
pub const VA_INTRINSIC: &str = "x64:va-intrinsic";
/// `va_arg`: as on AArch64, front ends expand `va_arg` into IR themselves
/// (`spec/abi-x64.md` §9.3); the classifier refuses it as `inst:va_arg`
/// before it reaches the code generator.
pub const VA_ARG: &str = "x64:va_arg";
/// A `landingpad` whose type is not `{ ptr, i32 }`.
pub const LANDING_PAD: &str = "x64:landingpad";
/// A `landingpad` clause whose type information is not a symbol's
/// address or `null`, or a `filter` that is not an array of them.
pub const LANDING_PAD_CLAUSE: &str = "x64:landingpad-clause";
/// `resume` of a value that is not `{ ptr, i32 }`.
pub const RESUME: &str = "x64:resume";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] =
    &[CALL, CALL_BUNDLE, VA_INTRINSIC, VA_ARG, LANDING_PAD, LANDING_PAD_CLAUSE, RESUME];

/// The size of the SysV `va_list`: `{ i32, i32, ptr, ptr }`.
pub const VA_LIST_BYTES: u32 = 24;
/// The size of the register save area (`spec/abi-x64.md` §9.3): six
/// general registers, then eight `xmm` registers of 16 bytes.
pub const SAVE_AREA_BYTES: u32 = 176;
/// The offset of `xmm0` in the register save area.
const SAVE_AREA_XMM: u32 = 48;

/// What a function's frame holds for its returns and for `va_start`,
/// filled by [`enter`]. Part of `crate::frame::FrameState`.
#[derive(Clone, Copy, Default, Debug)]
pub struct CallState {
    /// The slot holding the address the function returns in `rax`: its
    /// `sret` parameter, or the hidden address of a value returned in
    /// memory (WP-126 stores the value through it).
    pub ret_addr: Option<FrameRef>,
    /// The register save area of a variadic function.
    pub save_area: Option<FrameRef>,
    /// The slot holding `al` as it was at entry (a variadic function).
    pub al: Option<FrameRef>,
    /// The general argument registers the named parameters use.
    pub gpr_used: u8,
    /// The vector argument registers the named parameters use.
    pub fpr_used: u8,
    /// The offset of the first variable argument on the stack.
    pub stack_offset: u32,
}

/// Whether the function being compiled is variadic.
pub fn is_variadic(cx: &FnCx<'_, X64>) -> bool {
    matches!(cx.types().kind(cx.body.fn_type()), TypeKind::Function { variadic: true, .. })
}

/// The register the function's `sret` parameter arrives in, if it has
/// one.
fn sret_register(cx: &FnCx<'_, X64>) -> CgResult<Option<Reg>> {
    if cx.fn_assignment().sret_returned_in.is_none() {
        return Ok(None);
    }
    let symbol = cx.module.symbol(cx.body.symbol());
    let Some(index) =
        (0..cx.body.arg_count() as usize).find(|&i| symbol.param_attrs(i).mem() == ParamMem::Sret)
    else {
        return cx.bug("an sret assignment without an sret parameter");
    };
    match cx.fn_assignment().args.iter().find(|part| part.arg as usize == index) {
        Some(part) => match part.place {
            Place::Reg(reg) => Ok(Some(gp(reg))),
            Place::Stack(_) => cx.bug("an sret parameter on the stack"),
        },
        None => cx.bug("an sret parameter without a part"),
    }
}

/// Called by the prologue once the stack pointer is final, before any
/// argument register is read: saves the address the function returns in
/// `rax` (§6.1, §5.2) and, in a variadic function, the register save
/// area and `al` (§9.3). Writes no register.
///
/// # Errors
/// `limit:frame-size`, from the frame; `Internal` for an assignment that
/// contradicts the signature.
pub fn enter(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let returned = match cx.fn_assignment().ret_memory {
        Some(memory) => match memory.address {
            Place::Reg(reg) => Some(gp(reg)),
            Place::Stack(_) => return cx.bug("a return address on the stack"),
        },
        None => sret_register(cx)?,
    };
    let mut state = CallState::default();
    if let Some(reg) = returned {
        let slot = cx.alloc_slot(8, 8)?;
        store_gpr(cx, reg, slot, 8);
        state.ret_addr = Some(slot);
    }
    if is_variadic(cx) {
        let va = cx.fn_assignment().va;
        state.gpr_used = va.gpr_used.min(6);
        state.fpr_used = va.fpr_used.min(crate::abi::ARG_XMMS);
        state.stack_offset = va.stack_offset;
        let area = cx.alloc_slot(u64::from(SAVE_AREA_BYTES), 16)?;
        for (index, &reg) in crate::abi::ARG_GPRS.iter().enumerate() {
            store_gpr(cx, gp(reg), area.offset_by(8 * index as u32), 8);
        }
        for n in 0..crate::abi::ARG_XMMS {
            let xmm = Xmm::new(n).expect("xmm0..xmm7");
            let at = area.offset_by(SAVE_AREA_XMM + 16 * u32::from(n));
            frame_inst(cx, at, |m| mc::movup_mr(FpSize::S, m, xmm));
        }
        let al = cx.alloc_slot(8, 8)?;
        frame_inst(cx, al, |m| mc::mov_mr(Size::B, m, Reg::Rax));
        state.save_area = Some(area);
        state.al = Some(al);
    }
    cx.state.frame.call = state;
    Ok(())
}

/// Called at the start of the epilogue: the saved return address back
/// in `rax`, where the function has one.
pub fn before_return(cx: &mut FnCx<'_, X64>) {
    if let Some(slot) = cx.state.frame.call.ret_addr {
        load_gpr(cx, Reg::Rax, slot, 8);
    }
}

/// Called at the start of each tail exit, before the teardown: in a
/// variadic function, `al` as it was at entry (§8.5).
pub fn before_tail_exit(cx: &mut FnCx<'_, X64>) {
    if let Some(slot) = cx.state.frame.call.al {
        frame_inst(cx, slot, |m| mc::movzx_rm(Size::D, Reg::Rax, Size::B, m));
    }
}

/// `Target::extend_arg` (`spec/abi-x64.md` §7): brings the integer in
/// `rd`, of type `ty` (for a type wider than 64 bits, its last piece), to
/// what a producer owes under `zeroext`/`signext`: below 32 bits to 32,
/// between 32 and 64 to 64, the last piece of a wider one to 64. Without either attribute, and for 32 and 64
/// bits, nothing. Writes the flags (`and`, `shl`, `sar`, `shr`): only
/// before a call, at a return, or on a scratch register after a call.
pub fn extend_arg(cx: &mut FnCx<'_, X64>, rd: Reg, ty: TypeId, attrs: ParamAttrs) {
    let Some(bits) = cx.types().int_bits(ty) else { return };
    let signed = if attrs.has(ParamAttrs::SIGNEXT) {
        true
    } else if attrs.has(ParamAttrs::ZEROEXT) {
        false
    } else {
        return;
    };
    if bits > 64 {
        // The last piece of a wide integer is owed to all 64 bits.
        extend_piece(cx, rd, bits % 64, signed);
    } else {
        extend_bits(cx, rd, bits, signed);
    }
}

/// Extends the low `bits` bits of `rd` to 64 bits (`shl`·`sar`/`shr`);
/// nothing for 0. Writes the flags.
fn extend_piece(cx: &mut FnCx<'_, X64>, rd: Reg, bits: u32, signed: bool) {
    if bits == 0 {
        return;
    }
    let shift = (64 - bits) as u8;
    put(cx, mc::shl_ri(Size::Q, rd, shift));
    let back = if signed { mc::sar_ri(Size::Q, rd, shift) } else { mc::shr_ri(Size::Q, rd, shift) };
    put(cx, back);
}

/// Extends the low `bits` bits of `rd` by sign or by zero: to 32 bits
/// below 32 (`movzx`/`movsx` for 8 and 16, else `and` or `shl`·`sar`),
/// to 64 bits between 33 and 63 (`shl`·`shr`/`sar`); nothing for 0, 32
/// and 64. Writes the flags except for 8 and 16 bits.
pub fn extend_bits(cx: &mut FnCx<'_, X64>, rd: Reg, bits: u32, signed: bool) {
    match bits {
        0 | 32 | 64.. => {}
        8 | 16 => crate::common::extend(cx, rd, bits, Some(signed)),
        1..=31 => {
            let shift = (32 - bits) as u8;
            if signed {
                put(cx, mc::shl_ri(Size::D, rd, shift));
                put(cx, mc::sar_ri(Size::D, rd, shift));
            } else {
                put(cx, mc::and_ri(Size::D, rd, (1i64 << bits) - 1));
            }
        }
        33..=63 => extend_piece(cx, rd, bits, signed),
    }
}

/// What of a right assignment (`abi::assign`) the lowerings that have
/// landed do not honour yet; `Target::assign` asks for every signature,
/// a definition's and a call's. WP-131 removes the condition.
///
/// # Errors
/// [`crate::abi::ABI`] for a part in an x87 register (`Target::load` and
/// `store` know the general and the vector registers only).
pub fn guard(sig: &Sig<'_>, assignment: &Assignment) -> Result<(), AbiUnsupported> {
    let _ = sig;
    let x87 = |place: Place| matches!(place, Place::Reg(reg) if crate::abi::is_x87(reg));
    let parts = assignment.args.iter().chain(&assignment.ret);
    if parts.into_iter().any(|p| x87(p.place)) {
        return Err(AbiUnsupported(crate::abi::ABI));
    }
    Ok(())
}

/// A call or `invoke` of a function, not an intrinsic: the core's call
/// sequence (`cx.call`), with what this module adds to it (module
/// documentation).
///
/// # Errors
/// [`CALL`], [`CALL_BUNDLE`]; what `assign` and the call sequence report.
pub fn call<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("a call lowering of something that is no call");
    };
    if inst.opcode() == Opcode::CallBr || matches!(site.callee_value().kind(), ValueKind::Asm(_)) {
        // Inline assembly goes to `barrier` in the dispatch.
        return unsupported(CALL);
    }
    if site.bundle_count() != 0 {
        return unsupported(CALL_BUNDLE);
    }
    let musttail = site.tail() == TailKind::MustTail;
    let result = inst.ty();
    let odd = !musttail
        && result != TypeId::VOID
        && cx.target.class_of(cx.types(), result) == Class::Mem
        && has_odd_leaf(cx, result);
    if musttail && is_variadic(cx) {
        forward_variadic_registers(cx, inst)?;
    }
    cx.call(inst)?;
    // An `invoke`'s are cleared by the call sequence before its branch
    // (WP-084).
    if odd && inst.opcode() != Opcode::Invoke && !cx.is_dead(inst) {
        let home = cx.def_mem(inst)?;
        clear_above_leaves(cx, result, home, 0);
    }
    Ok(())
}

/// The most leaves a result returned in registers has (`rax`, `rdx`,
/// `rcx`, `xmm0`..`xmm3`, `st0`, `st1`), with room: a type with more is
/// returned in memory, where its producer wrote each leaf's in-memory
/// image.
const MAX_REG_LEAVES: u64 = 16;

/// The number of leaves of `ty`, or `None` once it exceeds `budget`.
fn leaves(cx: &FnCx<'_, X64>, ty: TypeId, budget: u64) -> Option<u64> {
    match cx.types().kind(ty) {
        TypeKind::Struct { fields, .. } => {
            let mut total = 0;
            for &field in fields {
                total += leaves(cx, field, budget - total)?;
            }
            Some(total)
        }
        TypeKind::Array { elem, len } => {
            let each = leaves(cx, elem, budget)?;
            let total = each.checked_mul(len)?;
            (total <= budget).then_some(total)
        }
        _ => (budget >= 1).then_some(1),
    }
}

/// The width of `ty` if it is an integer whose width is not a multiple
/// of 8.
fn odd_int(cx: &FnCx<'_, X64>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|bits| bits % 8 != 0)
}

/// Whether `ty`, a `Mem`-class type that may come back in registers, has
/// an integer leaf whose width is not a multiple of 8.
fn has_odd_leaf(cx: &FnCx<'_, X64>, ty: TypeId) -> bool {
    leaves(cx, ty, MAX_REG_LEAVES).is_some() && any_odd(cx, ty)
}

fn any_odd(cx: &FnCx<'_, X64>, ty: TypeId) -> bool {
    match cx.types().kind(ty) {
        TypeKind::Struct { fields, .. } => fields.iter().any(|&field| any_odd(cx, field)),
        TypeKind::Array { elem, len } => len != 0 && any_odd(cx, elem),
        _ => odd_int(cx, ty).is_some(),
    }
}

/// Clears the bits above every odd-width integer leaf of `ty`, whose
/// image is at `at + offset`: the byte holding the leaf's top bit is
/// masked to the bits below it (`spec/ir.md` §4.3). Writes `r11` and the
/// flags.
fn clear_above_leaves(cx: &mut FnCx<'_, X64>, ty: TypeId, at: FrameRef, offset: u32) {
    let types = cx.types();
    match types.kind(ty) {
        TypeKind::Struct { fields, .. } => {
            let offsets = types.field_offsets(ty).unwrap_or(&[]);
            for (&field, &field_offset) in fields.iter().zip(offsets) {
                clear_above_leaves(cx, field, at, offset + field_offset as u32);
            }
        }
        TypeKind::Array { elem, len } => {
            let stride = types.alloc_size_of(elem) as u32;
            for index in 0..len as u32 {
                clear_above_leaves(cx, elem, at, offset + index * stride);
            }
        }
        _ => {
            if let Some(bits) = odd_int(cx, ty) {
                let byte = at.offset_by(offset + bits / 8);
                load_gpr(cx, R11, byte, 1);
                put(cx, mc::and_ri(Size::D, R11, (1 << (bits % 8)) - 1));
                store_gpr(cx, R11, byte, 1);
            }
        }
    }
}

/// Before a `musttail` call in a variadic function: every register value
/// displaced, then the general and vector argument registers that carry
/// no named argument reloaded from the register save area, as they were
/// at entry. The call sequence then writes only the registers of the
/// named arguments, `r11` and the scratch registers and the stack area of
/// the named arguments; the tail exit reloads `al`.
fn forward_variadic_registers<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    cx.displace_all()?;
    let state = cx.state.frame.call;
    let Some(area) = state.save_area else {
        return cx.bug("a variadic function without its register save area");
    };
    for (index, &reg) in crate::abi::ARG_GPRS.iter().enumerate().skip(state.gpr_used.into()) {
        load_gpr(cx, gp(reg), area.offset_by(8 * index as u32), 8);
    }
    for n in state.fpr_used..crate::abi::ARG_XMMS {
        let xmm = Xmm::new(n).expect("xmm0..xmm7");
        let at = area.offset_by(SAVE_AREA_XMM + 16 * u32::from(n));
        frame_inst(cx, at, |m| mc::movup_rm(FpSize::S, xmm, m));
    }
    Ok(())
}

/// The `va_list` operand of a `va_*` intrinsic in a register.
fn va_list_reg(cx: &mut FnCx<'_, X64>, list: Value) -> CgResult<Reg> {
    if cx.class_of(list) != Class::Gpr
        || !matches!(cx.types().kind(value_ty(cx, list)), TypeKind::Ptr { .. })
    {
        return unsupported(VA_INTRINSIC);
    }
    let reg = cx.use_reg(list)?;
    Ok(gp(reg))
}

/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy` (`spec/abi-x64.md`
/// §9.2, §9.3): `va_start` fills the four fields of the `va_list` from
/// the named arguments' counts, the incoming stack area and the register
/// save area; `va_copy` copies its 24 bytes; `va_end` does nothing.
///
/// # Errors
/// [`VA_INTRINSIC`] for `va_start` outside a variadic function or an
/// operand that is not a pointer.
pub fn va_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::VaEnd, [_]) => Ok(()),
        (Intrinsic::VaStart, &[list]) => {
            let state = cx.state.frame.call;
            let Some(area) = state.save_area.filter(|_| is_variadic(cx)) else {
                return unsupported(VA_INTRINSIC);
            };
            let ap = va_list_reg(cx, list)?;
            let gp_offset = 8 * i64::from(state.gpr_used);
            let fp_offset = i64::from(SAVE_AREA_XMM) + 16 * i64::from(state.fpr_used);
            put(cx, mc::mov_mi(Size::D, Mem::base(ap, 0), gp_offset));
            put(cx, mc::mov_mi(Size::D, Mem::base(ap, 4), fp_offset));
            frame_address(cx, R11, FrameRef::Incoming(state.stack_offset));
            put(cx, mc::mov_mr(Size::Q, Mem::base(ap, 8), R11));
            frame_address(cx, R11, area);
            put(cx, mc::mov_mr(Size::Q, Mem::base(ap, 16), R11));
            Ok(())
        }
        (Intrinsic::VaCopy, &[dst, src]) => {
            let dst = va_list_reg(cx, dst)?;
            let src = va_list_reg(cx, src)?;
            put(cx, mc::movup_rm(FpSize::S, crate::common::XMM15, Mem::base(src, 0)));
            put(cx, mc::mov_rm(Size::Q, R11, Mem::base(src, 16)));
            put(cx, mc::movup_mr(FpSize::S, Mem::base(dst, 0), crate::common::XMM15));
            put(cx, mc::mov_mr(Size::Q, Mem::base(dst, 16), R11));
            Ok(())
        }
        _ => unsupported(VA_INTRINSIC),
    }
}

/// `%r = va_arg ptr %list, <ty>`: refused ([`VA_ARG`]), as on AArch64.
///
/// # Errors
/// Always [`VA_ARG`].
pub fn va_arg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, list: Value) -> CgResult<()> {
    let _ = (cx, inst, list);
    unsupported(VA_ARG)
}

/// Whether `ty` is `{ ptr, i32 }`, the landing pad's value.
fn is_pad_type(cx: &FnCx<'_, X64>, ty: TypeId) -> bool {
    matches!(
        cx.types().kind(ty),
        TypeKind::Struct { fields: [ptr, TypeId::I32], packed: false }
            if matches!(cx.types().kind(*ptr), TypeKind::Ptr { addr_space: 0 })
    )
}

/// The type-information symbol of a `catch` clause or `filter` element:
/// `None` for `null` (catch-all).
fn type_info(cx: &FnCx<'_, X64>, id: hod_ir::ConstId) -> CgResult<Option<SymId>> {
    match cx.module.consts.get(id) {
        Const::Null(_) => Ok(None),
        Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
        _ => unsupported(LANDING_PAD_CLAUSE),
    }
}

/// `%r = landingpad { ptr, i32 } [cleanup] [clauses]` (`spec/codegen.md`
/// §4.8, §9.4): the clauses go to the pad's row in order; the exception
/// pointer (`rax`) and the selector (`edx`), the registers the unwinder
/// sets (`RegInfo::eh_data`), are stored into the result.
///
/// # Errors
/// [`LANDING_PAD`], [`LANDING_PAD_CLAUSE`].
pub fn landing_pad<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pad: &LandingPad<'a>,
) -> CgResult<()> {
    if !is_pad_type(cx, inst.ty()) {
        return unsupported(LANDING_PAD);
    }
    for &clause in pad.clauses() {
        match clause {
            Clause::Catch(id) => {
                let sym = type_info(cx, id)?;
                cx.pad_clause(sym)?;
            }
            Clause::Filter(id) => {
                let elems: &[hod_ir::ConstId] = match cx.module.consts.get(id) {
                    Const::Aggregate { elems, .. } => elems,
                    Const::Zero(ty) if cx.types().alloc_size_of(ty) == 0 => &[],
                    _ => return unsupported(LANDING_PAD_CLAUSE),
                };
                let syms = elems.iter().map(|&e| type_info(cx, e)).collect::<CgResult<Vec<_>>>()?;
                cx.pad_filter(&syms)?;
            }
        }
    }
    let home = cx.def_mem(inst)?;
    let [exception, selector] = crate::REG_INFO.eh_data;
    store_gpr(cx, gp(exception), home, 8);
    store_gpr(cx, gp(selector), home.offset_by(8), 4);
    Ok(())
}

/// `resume { ptr, i32 } %value`: `_Unwind_Resume` of the exception
/// pointer, through the call sequence (so the call has a call-site row),
/// then the trap.
///
/// # Errors
/// [`RESUME`]; what the call sequence reports.
pub fn resume<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, value: Value) -> CgResult<()> {
    if !is_pad_type(cx, value_ty(cx, value)) {
        return unsupported(RESUME);
    }
    deferred_first(cx, inst, None)?;
    let from = cx.use_mem(value)?;
    let exception = cx.temp(Class::Gpr)?;
    load_gpr(cx, gp(exception), from, 8);
    cx.call_runtime(RuntimeFn::UnwindResume, &[CallArg::Reg(exception)], None)?;
    cx.trap();
    Ok(())
}
