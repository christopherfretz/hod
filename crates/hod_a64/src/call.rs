//! Calls and what goes with them (`spec/codegen.md` §7.1, `abi-a64.md`
//! §8, §9): calls and `invoke` through `hod_cg`'s call sequence,
//! `musttail`, landing pads and `resume`, the variadic save area and the
//! `llvm.va_*` intrinsics. `va_arg` is refused.
//!
//! **Owner: WP-038.** Tests: `tests/call.rs`. The dispatch in `target.rs`
//! is final. The call instruction itself (`bl` with `R_AARCH64_CALL26`,
//! `blr`) and the tail exits are emitted by `crate::frame` (WP-033),
//! which `hod_cg`'s call sequence drives; where each argument and result
//! travels is `crate::abi::assign` (WP-034).
//!
//! # What this module adds to the core's call sequence
//!
//! - The refusals of the forms the sequence does not handle (inline
//!   assembly callees, operand bundles, `callbr`).
//! - After a call whose `Mem`-class result holds integer leaves whose
//!   width is not a multiple of 8 (`{ i7, i64 }`, `i65`): the bits above
//!   each such leaf are cleared in the result's home, since a producer
//!   owes nothing above a leaf (`abi-a64.md` §7) and memory holds them
//!   zero (`spec/codegen.md` §2.2). An `invoke` with such a result is
//!   refused ([`INVOKE_ODD_RESULT`]): its result is used on the normal
//!   edge, after the branch the sequence emits.
//! - A `musttail` call in a variadic function first reloads the
//!   argument registers that carry no named argument from the register
//!   save area, so the callee receives them as this function did
//!   (`abi-a64.md` §8.5); the stack area beyond the named arguments is
//!   never written.
//! - The variadic save area (`abi-a64.md` §9.3), `va_start`, `va_copy`,
//!   `va_end`; `landingpad` and `resume` (§9.4).

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, RuntimeFn, Target};
use hod_ir::{
    Clause, Const, Inst, Intrinsic, LandingPad, Opcode, SymId, TailKind, TypeId, TypeKind, Value,
    ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Reg, Size, VMemSize, VReg};

use crate::common::{
    Cursor, FP, X16, X17, add_const, deferred_first, load_fpr_at, load_gpr_at, mov_imm, put,
    store_fpr_at, store_gpr_at, unsupported, value_ty, xr,
};
use crate::frame::is_variadic;
use crate::target::A64;

/// A call form this group does not lower: a `callbr`.
pub const CALL: &str = "a64:call";
/// A call of inline assembly (`hod_asm`).
pub const CALL_INLINE_ASM: &str = "a64:call-inline-asm";
/// A call with an operand bundle.
pub const CALL_BUNDLE: &str = "a64:call-bundle";
/// An `invoke` whose `Mem`-class result has an integer leaf whose width
/// is not a multiple of 8: the bits above it cannot be cleared before the
/// normal edge is taken.
pub const INVOKE_ODD_RESULT: &str = "a64:invoke-odd-int-result";
/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy` outside what is
/// lowered: `va_start` in a function that is not variadic, an operand
/// that is not a pointer.
pub const VA_INTRINSIC: &str = "a64:va-intrinsic";
/// `va_arg`: `llc` miscompiles it on this target, so there is no oracle;
/// C front ends expand `va_arg` into IR themselves.
pub const VA_ARG: &str = "a64:va_arg";
/// A `landingpad` whose type is not `{ ptr, i32 }`.
pub const LANDING_PAD: &str = "a64:landingpad";
/// A `landingpad` clause whose type information is not a symbol's
/// address or `null`, or a `filter` that is not an array of them.
pub const LANDING_PAD_CLAUSE: &str = "a64:landingpad-clause";
/// `resume` of a value that is not `{ ptr, i32 }`.
pub const RESUME: &str = "a64:resume";
/// The definition of a variadic function: its register save area. Kept
/// for the coverage log; the definition is lowered.
pub const VARIADIC: &str = "a64:variadic-definition";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[
    CALL,
    CALL_INLINE_ASM,
    CALL_BUNDLE,
    INVOKE_ODD_RESULT,
    VA_INTRINSIC,
    VA_ARG,
    LANDING_PAD,
    LANDING_PAD_CLAUSE,
    RESUME,
];

/// The size of AAPCS64's `va_list`.
pub const VA_LIST_BYTES: u32 = 32;

/// What a variadic function's frame holds for `va_start` (the save
/// area's offsets), filled by [`reserve_variadic`]. Part of
/// `crate::target::FnState`.
#[derive(Clone, Copy, Default, Debug)]
pub struct VaState {
    /// The local-area offset just past `x7` in the general save area
    /// (`__gr_top`).
    pub gr_top: u32,
    /// The local-area offset just past `q7` in the vector save area
    /// (`__vr_top`).
    pub vr_top: u32,
    /// The general argument registers the named parameters use (`g`).
    pub gpr_used: u8,
    /// The vector argument registers the named parameters use (`f`).
    pub fpr_used: u8,
    /// The offset of the first variable argument on the stack.
    pub stack_offset: u32,
}

/// Called by the prologue of a variadic function, after the frame record
/// and the incoming base are reserved: reserves and fills the register
/// save areas (`abi-a64.md` §9.3): `x[g]`..`x7` at 8 bytes each and
/// `q[f]`..`q7` at 16 bytes each, ascending, ending at `__gr_top` and
/// `__vr_top`. Writes scratch registers only.
///
/// # Errors
/// `limit:frame-size`, from the frame.
pub fn reserve_variadic(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let va = cx.fn_assignment().va;
    let (g, f) = (va.gpr_used.min(8), va.fpr_used.min(8));
    let mut state =
        VaState { gpr_used: g, fpr_used: f, stack_offset: va.stack_offset, ..VaState::default() };
    let gr_bytes = 8 * u32::from(8 - g);
    let vr_bytes = 16 * u32::from(8 - f);
    let FrameRef::Local(gr) = cx.alloc_slot(u64::from(gr_bytes.max(8)), 8)? else {
        return cx.bug("a save area outside the local area");
    };
    let FrameRef::Local(vr) = cx.alloc_slot(u64::from(vr_bytes.max(16)), 16)? else {
        return cx.bug("a save area outside the local area");
    };
    state.gr_top = gr + gr_bytes;
    state.vr_top = vr + vr_bytes;
    let mut at = Cursor::frame(cx, FrameRef::Local(gr), X16);
    for n in g..8 {
        store_gpr_at(cx, MemSize::X, Reg::new(n).expect("x0..x7").zr(), &mut at);
        at.advance(8);
    }
    let mut at = Cursor::frame(cx, FrameRef::Local(vr), X16);
    for n in f..8 {
        store_fpr_at(cx, VMemSize::Q, VReg::new(n).expect("v0..v7"), &mut at);
        at.advance(16);
    }
    cx.state.va = state;
    Ok(())
}

/// A call or `invoke` of a function, not an intrinsic: the core's call
/// sequence (`cx.call`), with what this module adds to it (module
/// documentation).
///
/// # Errors
/// [`CALL`], [`CALL_INLINE_ASM`], [`CALL_BUNDLE`], [`INVOKE_ODD_RESULT`];
/// what `assign` and the call sequence report.
pub fn call<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("a call lowering of something that is no call");
    };
    if inst.opcode() == Opcode::CallBr {
        return unsupported(CALL);
    }
    if matches!(site.callee_value().kind(), ValueKind::Asm(_)) {
        return unsupported(CALL_INLINE_ASM);
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
    if odd && inst.opcode() == Opcode::Invoke {
        return unsupported(INVOKE_ODD_RESULT);
    }
    if musttail && is_variadic(cx) {
        forward_variadic_registers(cx, inst)?;
    }
    cx.call(inst)?;
    if odd && !cx.is_dead(inst) {
        let home = cx.def_mem(inst)?;
        clear_above_leaves(cx, result, home, 0);
    }
    Ok(())
}

/// The most leaves a result returned in registers has: eight of each
/// register class (`abi-a64.md` §5.1). A type with more is returned in
/// memory, where its producer wrote each leaf's in-memory image.
const MAX_REG_LEAVES: u64 = 16;

/// The number of leaves of `ty`, or `None` once it exceeds `budget`.
fn leaves(cx: &FnCx<'_, A64>, ty: TypeId, budget: u64) -> Option<u64> {
    let types = cx.types();
    match types.kind(ty) {
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
fn odd_int(cx: &FnCx<'_, A64>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|bits| bits % 8 != 0)
}

/// Whether `ty`, a `Mem`-class type that may come back in registers, has
/// an integer leaf whose width is not a multiple of 8.
fn has_odd_leaf(cx: &FnCx<'_, A64>, ty: TypeId) -> bool {
    if leaves(cx, ty, MAX_REG_LEAVES).is_none() {
        return false;
    }
    any_odd(cx, ty)
}

fn any_odd(cx: &FnCx<'_, A64>, ty: TypeId) -> bool {
    match cx.types().kind(ty) {
        TypeKind::Struct { fields, .. } => fields.iter().any(|&field| any_odd(cx, field)),
        TypeKind::Array { elem, len } => len != 0 && any_odd(cx, elem),
        _ => odd_int(cx, ty).is_some(),
    }
}

/// Clears the bits above every odd-width integer leaf of `ty`, whose
/// image is at `at + offset`: the byte holding the leaf's top bit is
/// masked to the bits below it (`spec/ir.md` §4.3). `ty` has at most
/// [`MAX_REG_LEAVES`] leaves. Writes `x16` and `x17`.
fn clear_above_leaves(cx: &mut FnCx<'_, A64>, ty: TypeId, at: FrameRef, offset: u32) {
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
                let mut cursor = Cursor::frame(cx, byte, X17);
                load_gpr_at(cx, Load::B, X16, &mut cursor);
                put(cx, mc::and_imm(Size::W, X16.sp(), X16.zr(), (1 << (bits % 8)) - 1));
                store_gpr_at(cx, MemSize::B, X16.zr(), &mut cursor);
            }
        }
    }
}

/// Before a `musttail` call in a variadic function: every register value
/// displaced, then `x[g]`..`x7` and `q[f]`..`q7` reloaded from the save
/// areas, as they were at entry. The call sequence then writes only the
/// registers of the named arguments, the scratch registers and the stack
/// area of the named arguments.
fn forward_variadic_registers<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    cx.displace_all()?;
    let va = cx.state.va;
    let gr = va.gr_top - 8 * u32::from(8 - va.gpr_used);
    let mut at = Cursor::frame(cx, FrameRef::Local(gr), X16);
    for n in va.gpr_used..8 {
        load_gpr_at(cx, Load::X, Reg::new(n).expect("x0..x7"), &mut at);
        at.advance(8);
    }
    let vr = va.vr_top - 16 * u32::from(8 - va.fpr_used);
    let mut at = Cursor::frame(cx, FrameRef::Local(vr), X16);
    for n in va.fpr_used..8 {
        load_fpr_at(cx, VMemSize::Q, VReg::new(n).expect("v0..v7"), &mut at);
        at.advance(16);
    }
    Ok(())
}

/// The `va_list` operand of a `va_*` intrinsic in a register.
fn va_list_reg(cx: &mut FnCx<'_, A64>, list: Value) -> CgResult<Reg> {
    if cx.class_of(list) != Class::Gpr
        || !matches!(cx.types().kind(value_ty(cx, list)), TypeKind::Ptr { .. })
    {
        return unsupported(VA_INTRINSIC);
    }
    let reg = cx.use_reg(list)?;
    Ok(xr(reg))
}

/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy` (`abi-a64.md` §9.3):
/// `va_start` fills the five fields of the `va_list` from the save areas
/// and the incoming base; `va_copy` copies its 32 bytes; `va_end` does
/// nothing.
///
/// # Errors
/// [`VA_INTRINSIC`] for `va_start` outside a variadic function or an
/// operand that is not a pointer.
pub fn va_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::VaEnd, [_]) => Ok(()),
        (Intrinsic::VaStart, &[list]) => {
            let Some(base) = cx.state.frame.incoming_base.filter(|_| is_variadic(cx)) else {
                return unsupported(VA_INTRINSIC);
            };
            let ap = va_list_reg(cx, list)?;
            let va = cx.state.va;
            put(cx, mc::ldr_off(Load::X, X16.zr(), FP, i64::from(base)));
            add_const(cx, X16.sp(), X16.sp(), i64::from(va.stack_offset));
            put(cx, mc::str_off(MemSize::X, X16.zr(), ap.sp(), 0));
            add_const(cx, X16.sp(), FP, i64::from(va.gr_top));
            add_const(cx, X17.sp(), FP, i64::from(va.vr_top));
            put(cx, mc::stp_off(Size::X, X16.zr(), X17.zr(), ap.sp(), 8));
            let gr_offs = -8 * i64::from(8 - va.gpr_used);
            let vr_offs = -16 * i64::from(8 - va.fpr_used);
            mov_imm(cx, Size::W, X16, gr_offs as u64);
            mov_imm(cx, Size::W, X17, vr_offs as u64);
            put(cx, mc::stp_off(Size::W, X16.zr(), X17.zr(), ap.sp(), 24));
            Ok(())
        }
        (Intrinsic::VaCopy, &[dst, src]) => {
            let dst = va_list_reg(cx, dst)?;
            let src = va_list_reg(cx, src)?;
            for offset in [0, 16] {
                put(cx, mc::ldp_off(Size::X, X16.zr(), X17.zr(), src.sp(), offset));
                put(cx, mc::stp_off(Size::X, X16.zr(), X17.zr(), dst.sp(), offset));
            }
            Ok(())
        }
        _ => unsupported(VA_INTRINSIC),
    }
}

/// `%r = va_arg ptr %list, <ty>`: refused ([`VA_ARG`]).
///
/// # Errors
/// Always [`VA_ARG`].
pub fn va_arg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, list: Value) -> CgResult<()> {
    let _ = (cx, inst, list);
    unsupported(VA_ARG)
}

/// Whether `ty` is `{ ptr, i32 }`, the landing pad's value.
fn is_pad_type(cx: &FnCx<'_, A64>, ty: TypeId) -> bool {
    matches!(
        cx.types().kind(ty),
        TypeKind::Struct { fields: [ptr, TypeId::I32], packed: false }
            if matches!(cx.types().kind(*ptr), TypeKind::Ptr { addr_space: 0 })
    )
}

/// The type-information symbol of a `catch` clause or `filter` element:
/// `None` for `null` (catch-all).
fn type_info(cx: &FnCx<'_, A64>, id: hod_ir::ConstId) -> CgResult<Option<SymId>> {
    match cx.module.consts.get(id) {
        Const::Null(_) => Ok(None),
        Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
        _ => unsupported(LANDING_PAD_CLAUSE),
    }
}

/// `%r = landingpad { ptr, i32 } [cleanup] [clauses]` (`spec/codegen.md`
/// §4.8, §9.4): the clauses go to the pad's row in order; the exception
/// pointer (`x0`) and the selector (`w1`) are stored into the result.
///
/// # Errors
/// [`LANDING_PAD`], [`LANDING_PAD_CLAUSE`].
pub fn landing_pad<'a>(
    cx: &mut FnCx<'a, A64>,
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
                let mut syms = [None; 64];
                if elems.len() > syms.len() {
                    // A long exception specification: rare enough to pay
                    // for a vector.
                    let all =
                        elems.iter().map(|&e| type_info(cx, e)).collect::<CgResult<Vec<_>>>()?;
                    cx.pad_filter(&all)?;
                    continue;
                }
                for (slot, &elem) in syms.iter_mut().zip(elems) {
                    *slot = type_info(cx, elem)?;
                }
                cx.pad_filter(&syms[..elems.len()])?;
            }
        }
    }
    let home = cx.def_mem(inst)?;
    let [exception, selector] = crate::REG_INFO.eh_data;
    let mut at = Cursor::frame(cx, home, X16);
    store_gpr_at(cx, MemSize::X, xr(exception).zr(), &mut at);
    at.advance(8);
    store_gpr_at(cx, MemSize::W, xr(selector).zr(), &mut at);
    Ok(())
}

/// `resume { ptr, i32 } %value`: `_Unwind_Resume` of the exception
/// pointer, through the call sequence (so the call has a call-site row),
/// then the trap.
///
/// # Errors
/// [`RESUME`]; what the call sequence reports.
pub fn resume<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, value: Value) -> CgResult<()> {
    if !is_pad_type(cx, value_ty(cx, value)) {
        return unsupported(RESUME);
    }
    deferred_first(cx, inst, None)?;
    let from = cx.use_mem(value)?;
    let exception = cx.temp(Class::Gpr)?;
    let mut at = Cursor::frame(cx, from, X16);
    load_gpr_at(cx, Load::X, xr(exception), &mut at);
    cx.call_runtime(RuntimeFn::UnwindResume, &[CallArg::Reg(exception)], None)?;
    cx.trap();
    Ok(())
}
