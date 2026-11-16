//! Scalar SSE floating point: design 0008's I12 (WP-317,
//! `spec/codegen-x86.md` "SSE").
//!
//! A function's `float` values are SSE values when its CPU has SSE, its
//! `double` values when it has SSE2 ([`is_sse`]; the feature set as
//! `llc` decides it, per function). [`owns`] is the one decision point:
//! an instruction with an SSE value as its result or an operand is
//! lowered here, every other floating instruction by `x87` exactly as
//! before.
//!
//! SSE values are class `Mem`, as on the x87 path: a frame slot of 4 or 8
//! bytes, so the arguments (on the stack), `phi`s and the `st0` glue of
//! returns and call results are the ones `x87` and `abi` already have.
//! An operation loads its first operand into `xmm6` (`movss`/`movsd`),
//! operates with the second from its slot, and stores the result to its
//! home: every result is rounded to its type at once, as `llc`'s, so
//! no value computed here has an 80-bit copy. A call result of an SSE
//! type is stored from `st0` at its type's width (§11's glue), and also
//! at 80 bits for its run as on the x87 path: only an `fpext`/`fptrunc`
//! reads that copy ([`fp_cast`]), as `llc` does.
//!
//! What `llc` computes on the x87 in an SSE function is computed there
//! here too, from the value's home: `x86_fp80` conversions, conversions
//! from and to 64-bit integers (`fild`/`fistp`), the libm calls (results
//! in `st0`).

use hod_cg::sym::{FpOp, FpType, fp_routine};
use hod_cg::{CallTarget, CgError, CgResult, FnCx, FrameRef, RuntimeFn, SymRef};
use hod_ir::{
    Arch, Const, FloatPredicate, FnStr, Inst, InstId, InstKind, Intrinsic, MemFlags, Opcode, RmwOp,
    SymbolKind, TypeId, TypeKind, Value, ValueKind, X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpCmp, FpSize, Mem, Reg, Size, St, X87Size, Xmm};

use crate::common::{
    EAX, ECX, EDX, ESP, const_int, copy_frame, frame_address, frame_inst, gp, put, sym_inst,
    unsupported, value_ty,
};
use crate::target::X86;
use crate::x87::{CAST, I64_CAST, INTRINSIC};

/// The first operand and the result. (`minnum`/`maxnum` also take
/// `xmm4` and `xmm5`: no value of this target is in an `xmm` register.)
const X: Xmm = Xmm::Xmm6;

/// An SSE type: `float` (`S`) or `double` (`D`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sse {
    S,
    D,
}

impl Sse {
    const fn fp(self) -> FpSize {
        match self {
            Sse::S => FpSize::S,
            Sse::D => FpSize::D,
        }
    }

    const fn bytes(self) -> u32 {
        match self {
            Sse::S => 4,
            Sse::D => 8,
        }
    }

    const fn x87(self) -> X87Size {
        match self {
            Sse::S => X87Size::S,
            Sse::D => X87Size::D,
        }
    }

    const fn fp_type(self) -> FpType {
        match self {
            Sse::S => FpType::Float,
            Sse::D => FpType::Double,
        }
    }
}

// ---- which values are SSE values ----

/// The CPU features of the function, as `llc` decides them (the same
/// rule as `x87`'s): its `target-cpu`, else `-mcpu`, else `generic`;
/// then its `target-features`.
fn features(cx: &FnCx<'_, X86>) -> X86Features {
    let mcpu = cx.target.cpu();
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return X86Features::of_function(None, None, mcpu, &[]).unwrap_or(X86Features::NONE);
    };
    let attrs = cx.module.fn_attrs(attrs);
    let text = |what| attrs.string(what).and_then(|s| std::str::from_utf8(s).ok());
    X86Features::of_function(text(FnStr::TargetCpu), text(FnStr::TargetFeatures), mcpu, &[])
        .unwrap_or(X86Features::NONE)
}

/// The SSE type of `ty` in this function, if it is one.
fn sse_of(cx: &FnCx<'_, X86>, ty: TypeId) -> Option<Sse> {
    let (sse, need) = match cx.types().kind(ty) {
        TypeKind::Float => (Sse::S, X86Features::SSE),
        TypeKind::Double => (Sse::D, X86Features::SSE2),
        _ => return None,
    };
    features(cx).has(need).then_some(sse)
}

/// Whether `ty` is an SSE type in this function: `float` with SSE,
/// `double` with SSE2.
pub fn is_sse(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    sse_of(cx, ty).is_some()
}

fn is_sse_value(cx: &FnCx<'_, X86>, v: Value) -> bool {
    is_sse(cx, value_ty(cx, v))
}

fn sse(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<Sse> {
    match sse_of(cx, ty) {
        Some(s) => Ok(s),
        None => cx.bug("an SSE operation on a type that is no SSE type"),
    }
}

/// Whether `inst` is this module's: an SSE value as its result or an
/// operand. The one dispatch test of `target.rs`, before `x87`'s.
pub fn owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    match inst.kind() {
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::Load { .. }
        | InstKind::Select { .. } => is_sse(cx, inst.ty()),
        InstKind::Cast { arg } => is_sse(cx, inst.ty()) || is_sse_value(cx, arg),
        InstKind::FCmp { lhs, .. } => is_sse_value(cx, lhs),
        InstKind::Store { value, .. } => is_sse_value(cx, value),
        InstKind::Call(site) => {
            site.intrinsic().is_some()
                && (is_sse(cx, inst.ty()) || site.args().iter().any(|&a| is_sse_value(cx, a)))
        }
        _ => false,
    }
}

// ---- the frame ----

/// `movss`/`movsd xmm, place`.
fn load_x(cx: &mut FnCx<'_, X86>, s: Sse, x: Xmm, place: FrameRef) {
    frame_inst(cx, place, |m| mc::movs_rm(s.fp(), x, m));
}

/// `movss`/`movsd place, xmm`.
fn store_x(cx: &mut FnCx<'_, X86>, s: Sse, place: FrameRef, x: Xmm) {
    frame_inst(cx, place, |m| mc::movs_mr(s.fp(), m, x));
}

/// Operand `v` into `x`.
fn get(cx: &mut FnCx<'_, X86>, s: Sse, x: Xmm, v: Value) -> CgResult<()> {
    let place = cx.use_mem(v)?;
    load_x(cx, s, x, place);
    Ok(())
}

/// `x` to `inst`'s home.
fn set(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, s: Sse, x: Xmm) -> CgResult<()> {
    let inst = cx.body.inst(inst.id());
    let r = cx.def_mem(inst)?;
    store_x(cx, s, r, x);
    Ok(())
}

/// `[esp + disp]`.
fn sp(disp: i32) -> Mem {
    Mem::base(ESP, disp)
}

/// Takes `n` bytes below the stack pointer.
fn below(cx: &mut FnCx<'_, X86>, n: u32) {
    put(cx, mc::lea(Size::D, ESP, sp(-(n as i32))));
}

/// Gives back `n` bytes.
fn above(cx: &mut FnCx<'_, X86>, n: u32) {
    put(cx, mc::lea(Size::D, ESP, sp(n as i32)));
}

/// `inst`'s result: a copy of `v`.
fn copy_of<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, v: Value, bytes: u32) -> CgResult<()> {
    let src = cx.use_mem(v)?;
    let r = cx.def_mem(inst)?;
    if r != src {
        copy_frame(cx, r, src, bytes);
    }
    Ok(())
}

// ---- constants (`Target::materialize_mem`) ----

/// Writes the constant `value` of an SSE type to `dst` by `mov`
/// immediates of its encoding (as `x87`: no constant pool, so no
/// relocation under PIC).
///
/// # Errors
/// A constant that is no floating constant.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    let s = sse(cx, value_ty(cx, value))?;
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let bits: u64 = match cx.module.consts.get(id) {
        Const::Float { bits, .. } => bits,
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => 0,
        _ => return unsupported(crate::common::CONSTANT),
    };
    write_bits(cx, dst, s.bytes(), bits);
    Ok(())
}

fn write_bits(cx: &mut FnCx<'_, X86>, dst: FrameRef, bytes: u32, bits: u64) {
    for at in (0..bytes).step_by(4) {
        let piece = (bits >> (8 * at)) as u32;
        frame_inst(cx, dst.offset_by(at), |m| mc::mov_mi(Size::D, m, i64::from(piece as i32)));
    }
}

/// A 32-bit word `bits` in the 16-byte area below the stack pointer, at
/// `disp`.
fn word(cx: &mut FnCx<'_, X86>, disp: i32, bits: u32) {
    put(cx, mc::mov_mi(Size::D, sp(disp), i64::from(bits as i32)));
}

// ---- memory ----

#[derive(Clone, Copy)]
enum At {
    Frame(FrameRef),
    Sym(hod_ir::SymId),
    Reg(Reg),
}

fn address(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<At> {
    match ptr.kind() {
        ValueKind::Slot(slot) => match cx.slot_place(slot)? {
            hod_cg::SlotPlace::Direct(place) => Ok(At::Frame(place)),
            hod_cg::SlotPlace::Indirect(pointer) => {
                frame_inst(cx, pointer, |m| mc::mov_rm(Size::D, EDX, m));
                Ok(At::Reg(EDX))
            }
        },
        ValueKind::Sym(sym)
            if !matches!(
                hod_cg::sym_access(cx.module, cx.options, sym),
                hod_cg::Access::Tls(_)
            ) =>
        {
            Ok(At::Sym(sym))
        }
        _ => Ok(At::Reg(gp(cx.use_reg(ptr)?))),
    }
}

fn access(cx: &mut FnCx<'_, X86>, at: At, make: impl FnOnce(Mem) -> mc::Inst) -> CgResult<()> {
    match at {
        At::Frame(place) => {
            frame_inst(cx, place, make);
            Ok(())
        }
        At::Sym(sym) => sym_inst(cx, sym, 0, make),
        At::Reg(base) => {
            put(cx, make(Mem::base(base, 0)));
            Ok(())
        }
    }
}

fn check(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() { unsupported(crate::mem::ATOMIC) } else { Ok(()) }
}

fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    let s = sse(cx, inst.ty())?;
    let at = address(cx, ptr)?;
    access(cx, at, |m| mc::movs_rm(s.fp(), X, m))?;
    set(cx, inst, s, X)
}

fn store(cx: &mut FnCx<'_, X86>, value: Value, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    let s = sse(cx, value_ty(cx, value))?;
    get(cx, s, X, value)?;
    let at = address(cx, ptr)?;
    access(cx, at, |m| mc::movs_mr(s.fp(), m, X))
}

// ---- arithmetic ----

fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let s = sse(cx, inst.ty())?;
    let op: fn(FpSize, Xmm, Mem) -> mc::Inst = match inst.opcode() {
        Opcode::FAdd => mc::adds_rm,
        Opcode::FSub => mc::subs_rm,
        Opcode::FMul => mc::muls_rm,
        Opcode::FDiv => mc::divs_rm,
        Opcode::FRem => return libcall(cx, inst, routine(FpOp::Rem, s)?, &[lhs, rhs], s),
        _ => return cx.bug("an integer opcode on a floating type"),
    };
    let b = cx.use_mem(rhs)?;
    get(cx, s, X, lhs)?;
    frame_inst(cx, b, |m| op(s.fp(), X, m));
    set(cx, inst, s, X)
}

/// `fneg` (the sign bit flipped in memory, as `llc`'s `xorps` of a sign
/// mask) and `freeze` (a copy).
fn unary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let s = sse(cx, inst.ty())?;
    copy_of(cx, inst, arg, s.bytes())?;
    if inst.opcode() == Opcode::FNeg {
        let r = cx.def_mem(inst)?;
        frame_inst(cx, r.offset_by(s.bytes() - 1), |m| mc::xor_mi(Size::B, m, -0x80));
    }
    Ok(())
}

// ---- compares ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FpCond {
    One(Cond),
    All(Cond, Cond),
    Any(Cond, Cond),
}

/// Whether the operands are swapped and how the flags of `ucomis x, y`
/// read (unordered sets `ZF`, `PF`, `CF`), as `x87`'s `fucomi`.
const fn conditions(pred: FloatPredicate) -> (bool, FpCond) {
    use FloatPredicate as P;
    match pred {
        P::Oeq => (false, FpCond::All(Cond::E, Cond::Np)),
        P::Ogt => (false, FpCond::One(Cond::A)),
        P::Oge => (false, FpCond::One(Cond::Ae)),
        P::Olt => (true, FpCond::One(Cond::A)),
        P::Ole => (true, FpCond::One(Cond::Ae)),
        // `true` is `cmp eax, eax` (ZF set), `false` reads `ZF` clear.
        P::One | P::False => (false, FpCond::One(Cond::Ne)),
        P::Ord => (false, FpCond::One(Cond::Np)),
        P::Ueq | P::True => (false, FpCond::One(Cond::E)),
        P::Ugt => (true, FpCond::One(Cond::B)),
        P::Uge => (true, FpCond::One(Cond::Be)),
        P::Ult => (false, FpCond::One(Cond::B)),
        P::Ule => (false, FpCond::One(Cond::Be)),
        P::Une => (false, FpCond::Any(Cond::Ne, Cond::P)),
        P::Uno => (false, FpCond::One(Cond::P)),
    }
}

fn fcmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    if !matches!(cx.types().kind(inst.ty()), TypeKind::Int(1)) {
        return unsupported(crate::target::VECTOR);
    }
    let s = sse(cx, value_ty(cx, lhs))?;
    let (swap, cond) = conditions(pred);
    let (x, y) = if swap { (rhs, lhs) } else { (lhs, rhs) };
    let yp = cx.use_mem(y)?;
    get(cx, s, X, x)?;
    let rd = cx.def_reg(inst)?;
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp_rr(Size::D, EAX, EAX));
    } else {
        frame_inst(cx, yp, |m| mc::ucomis_rm(s.fp(), X, m));
    }
    match cond {
        FpCond::One(c) => put(cx, mc::setcc_r(c, EAX)),
        FpCond::All(c1, c2) | FpCond::Any(c1, c2) => {
            put(cx, mc::setcc_r(c1, EAX));
            put(cx, mc::setcc_r(c2, ECX));
            let all = matches!(cond, FpCond::All(..));
            put(cx, if all { mc::and_rr(Size::B, EAX, ECX) } else { mc::or_rr(Size::B, EAX, ECX) });
        }
    }
    put(cx, mc::movzx_rr(Size::D, gp(rd), Size::B, EAX));
    cx.finish(inst, rd)
}

/// `select`: of a constant condition the value it picks; else a `cmov`
/// of the two homes' addresses (every SSE CPU has `cmov`) and a copy
/// through `xmm6`.
fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let s = sse(cx, inst.ty())?;
    if cx.types().int_bits(value_ty(cx, cond)) != Some(1) {
        return unsupported(crate::target::VECTOR);
    }
    if let Some(c) = const_int(cx, cond) {
        return copy_of(cx, inst, if c & 1 != 0 { if_true } else { if_false }, s.bytes());
    }
    let t = cx.use_mem(if_true)?;
    let f = cx.use_mem(if_false)?;
    cx.put(cond, crate::reg::EAX)?;
    put(cx, mc::test_ri(Size::B, EAX, 1));
    frame_address(cx, ECX, t);
    frame_address(cx, EDX, f);
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::D, EDX, ECX));
    put(cx, mc::movs_rm(s.fp(), X, Mem::base(EDX, 0)));
    set(cx, inst, s, X)
}

// ---- conversions ----

fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let from = value_ty(cx, arg);
    let to = inst.ty();
    match inst.opcode() {
        Opcode::FPExt | Opcode::FPTrunc => fp_cast(cx, inst, arg, from, to),
        Opcode::SIToFP | Opcode::UIToFP => from_int(cx, inst, arg),
        Opcode::FPToSI | Opcode::FPToUI => to_int(cx, inst, arg),
        Opcode::BitCast => bitcast(cx, inst, arg, from, to),
        _ => unsupported(CAST),
    }
}

/// The x87 width of a floating type on the other side of a conversion
/// (`float`, `double` without SSE2, `x86_fp80`), or its refusal.
fn x87_of(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<X87Size> {
    match cx.types().kind(ty) {
        TypeKind::Float => Ok(X87Size::S),
        TypeKind::Double => Ok(X87Size::D),
        TypeKind::X86Fp80 => Ok(X87Size::T),
        TypeKind::Half => unsupported(crate::x87::HALF),
        TypeKind::BFloat => unsupported(crate::x87::BFLOAT),
        TypeKind::Fp128 => unsupported(crate::x87::FP128),
        TypeKind::Vector { .. } => unsupported(crate::target::VECTOR),
        _ => unsupported(CAST),
    }
}

/// `fpext`/`fptrunc` (`float`, `double`, `x86_fp80`): the operand's
/// 80-bit copy for this use if `x87` keeps one, else its home, pushed
/// (`x87::ret_copy`/`load_ret`: the bracket that serves a `ret` serves
/// any user), and `fstp` to the result's home: one rounding.
///
/// Two operands are read wider than their home (measured, `llc` 19 and
/// 22): a view of a call result of an SSE type ([`Views`]: `x`, or for
/// `fptrunc (fpext w)` to another type `x` rounded to `w`'s type); and
/// at SSE without SSE2 a computed `double`, an x87 value that `llc`
/// rounds to `float` directly (`fstps`), through §11's copy. Every other
/// operand has no copy and its home holds it exactly.
fn fp_cast<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    arg: Value,
    from: TypeId,
    to: TypeId,
) -> CgResult<()> {
    let (a, b) = (x87_of(cx, from)?, x87_of(cx, to)?);
    let bytes = match a {
        X87Size::S => 4,
        X87Size::D => 8,
        X87Size::T => 10,
    };
    if !push_source(cx, inst, arg, to)? {
        let src = cx.use_mem(arg)?;
        crate::x87::ret_copy(cx, inst, arg, true);
        crate::x87::load_ret(cx, src, bytes);
    }
    let inst = cx.body.inst(inst.id());
    let r = cx.def_mem(inst)?;
    frame_inst(cx, r, |m| mc::fstp_m(b, m));
    Ok(())
}

/// Pushes what `fpext`/`fptrunc` `inst` to `to` reads when its operand
/// involves a view ([`Source::View`], [`Source::Rounded`]); whether it
/// did (else nothing is emitted).
fn push_source(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, arg: Value, to: TypeId) -> CgResult<bool> {
    match source(cx, inst, arg, to)? {
        Source::View(slot) => frame_inst(cx, slot, |m| mc::fld_m(X87Size::T, m)),
        Source::Rounded(slot, size) => {
            // `x` rounded to `w`'s type below the stack pointer (`w`'s home
            // is no operand of this instruction).
            frame_inst(cx, slot, |m| mc::fld_m(X87Size::T, m));
            below(cx, 12);
            put(cx, mc::fstp_m(size, sp(0)));
            put(cx, mc::fld_m(size, sp(0)));
            above(cx, 12);
        }
        Source::Operand => return Ok(false),
    }
    Ok(true)
}

/// From `x87::cast`: an `fpext`/`fptrunc` between x87 types (at SSE
/// without SSE2, `double` and `x86_fp80`) whose operand is a view, or an
/// `fpext` of one: pushes what `llc` reads (as [`fp_cast`]). Whether it
/// did; nothing is emitted otherwise (and never without an SSE call
/// result in the function).
///
/// # Errors
/// What the operand's type reports.
pub fn x87_cast_source(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, arg: Value) -> CgResult<bool> {
    let to = inst.ty();
    push_source(cx, inst, arg, to)
}

/// What an `fpext`/`fptrunc` pushes ([`Views`]).
enum Source {
    /// `x` of a call result.
    View(FrameRef),
    /// `fptrunc (fpext w)` of a view `w` to another type: `x` rounded to
    /// `w`'s type (the slot of `x`, `w`'s width).
    Rounded(FrameRef, X87Size),
    /// The operand: its §11 copy if `x87` keeps one, else its home.
    Operand,
}

fn source(cx: &FnCx<'_, X86>, inst: Inst<'_>, arg: Value, to: TypeId) -> CgResult<Source> {
    if cx.state.sse.live.is_empty() {
        return Ok(Source::Operand);
    }
    let user = inst.id().raw();
    let w = if inst.opcode() == Opcode::FPTrunc { ext_operand(cx, arg) } else { None };
    let Some(w) = w else {
        return Ok(view(cx, arg, user, VIEW_DEPTH).map_or(Source::Operand, Source::View));
    };
    let Some(slot) = view(cx, w, user, VIEW_DEPTH) else { return Ok(Source::Operand) };
    let ty = value_ty(cx, w);
    // `llc` folds a store of the result into the conversion (a store of
    // `x` at the result's width) when every use is such a store.
    if ty == to || only_stored(cx, inst) {
        Ok(Source::View(slot))
    } else {
        Ok(Source::Rounded(slot, x87_of(cx, ty)?))
    }
}

/// Whether every use of `inst`'s result is a store of it in its block.
fn only_stored(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let at = inst.id().raw();
    let result = inst.result();
    let uses = inst.uses().count();
    let end = reach(cx, at);
    let stores = (at + 1..end)
        .filter(|&raw| {
            matches!(cx.body.inst(InstId::new(raw)).kind(),
                InstKind::Store { value, .. } if value == result)
        })
        .count();
    uses > 0 && stores == uses as usize
}

/// `bitcast` between an SSE type and the integer of its width: a copy.
fn bitcast<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    arg: Value,
    from: TypeId,
    to: TypeId,
) -> CgResult<()> {
    let types = cx.types();
    let (fp, other) = if is_sse(cx, from) { (from, to) } else { (to, from) };
    let bits = types.int_bits(other);
    let s = sse(cx, fp)?;
    if bits != Some(8 * s.bytes()) {
        return unsupported(if matches!(types.kind(other), TypeKind::Vector { .. }) {
            crate::target::VECTOR
        } else {
            CAST
        });
    }
    if s == Sse::D {
        return copy_of(cx, inst, arg, s.bytes());
    }
    if is_sse(cx, from) {
        let src = cx.use_mem(arg)?;
        let rd = cx.def_reg(inst)?;
        frame_inst(cx, src, |m| mc::mov_rm(Size::D, gp(rd), m));
        cx.finish(inst, rd)
    } else {
        let r = cx.use_reg(arg)?;
        let dst = cx.def_mem(inst)?;
        frame_inst(cx, dst, |m| mc::mov_mr(Size::D, m, gp(r)));
        Ok(())
    }
}

/// `sitofp`/`uitofp`. From at most 32 bits: the integer extended in
/// `ecx`; signed, or unsigned narrower than 32 bits, `cvtsi2s`; an
/// unsigned `i32` by `fild` of 64 bits with a zero high word, exact, and
/// `fstp` to the result (one rounding, as `llc`'s `2^52` trick). From an
/// `i64` slot: signed, `fild` and `fstp` (one rounding, as `llc`);
/// unsigned to `float`, `llc`'s x87 sequence (`fild`, `fadd` of 0 or
/// `2^64` at 80 bits, `fstp`); unsigned to `double`, the high word
/// times `2^32` plus the low word, both exact, added by `addsd` (one
/// rounding, as `llc`'s `2^52`/`2^84` sequence).
fn from_int<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let s = sse(cx, inst.ty())?;
    let signed = inst.opcode() == Opcode::SIToFP;
    let bits = match cx.types().int_bits(value_ty(cx, arg)) {
        Some(bits) if bits <= 32 || bits == 64 => bits,
        Some(_) => return unsupported(I64_CAST),
        None => return unsupported(crate::target::VECTOR),
    };
    if bits == 64 {
        let slot = cx.use_mem(arg)?;
        if signed || s == Sse::S {
            frame_inst(cx, slot, |m| mc::fild_m(Size::Q, m));
            if !signed {
                frame_inst(cx, slot.offset_by(4), |m| mc::mov_rm(Size::D, ECX, m));
                put(cx, mc::sar_ri(Size::D, ECX, 31));
                put(cx, mc::and_ri(Size::D, ECX, 0x5F80_0000));
                below(cx, 16);
                put(cx, mc::mov_mr(Size::D, sp(0), ECX));
                put(cx, mc::fld_m(X87Size::S, sp(0)));
                put(cx, mc::faddp(St::St1));
                above(cx, 16);
            }
            let r = cx.def_mem(inst)?;
            frame_inst(cx, r, |m| mc::fstp_m(s.x87(), m));
            return Ok(());
        }
        // Unsigned to `double`: [0] = the word, [4] = 0, [8] = 2^32.
        below(cx, 16);
        word(cx, 4, 0);
        frame_inst(cx, slot.offset_by(4), |m| mc::mov_rm(Size::D, ECX, m));
        put(cx, mc::mov_mr(Size::D, sp(0), ECX));
        put(cx, mc::fild_m(Size::Q, sp(0)));
        put(cx, mc::fstp_m(X87Size::D, sp(8)));
        put(cx, mc::movs_rm(FpSize::D, X, sp(8)));
        word(cx, 8, 0);
        word(cx, 12, 0x41f0_0000);
        put(cx, mc::muls_rm(FpSize::D, X, sp(8)));
        frame_inst(cx, slot, |m| mc::mov_rm(Size::D, ECX, m));
        put(cx, mc::mov_mr(Size::D, sp(0), ECX));
        put(cx, mc::fild_m(Size::Q, sp(0)));
        put(cx, mc::fstp_m(X87Size::D, sp(8)));
        put(cx, mc::adds_rm(FpSize::D, X, sp(8)));
        above(cx, 16);
        return set(cx, inst, s, X);
    }
    let r = cx.use_reg(arg)?;
    put(cx, mc::mov_rr(Size::D, ECX, gp(r)));
    let ext = if signed { crate::alu::Ext::Sign } else { crate::alu::Ext::Zero };
    crate::alu::extend_to(cx, ECX, bits, ext);
    if !signed && bits == 32 {
        below(cx, 16);
        put(cx, mc::mov_mr(Size::D, sp(0), ECX));
        word(cx, 4, 0);
        put(cx, mc::fild_m(Size::Q, sp(0)));
        above(cx, 16);
        let r = cx.def_mem(inst)?;
        frame_inst(cx, r, |m| mc::fstp_m(s.x87(), m));
        return Ok(());
    }
    put(cx, mc::cvtsi2s_rr(s.fp(), X, Size::D, ECX));
    set(cx, inst, s, X)
}

/// `fptosi`/`fptoui`. To at most 32 bits: `cvtts2si` (a narrower result
/// is its low bits); an unsigned `i32` as `llc`: `x - 2^31` and `x`
/// truncated, the first masked by the sign of the second and or-ed in.
/// To an `i64`: the x87 path's sequence from the home (`fld`, the
/// control word around `fistp qword`; unsigned, `2^63` subtracted when
/// `x >= 2^63` and the flag in bit 63).
fn to_int<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let s = sse(cx, value_ty(cx, arg))?;
    let unsigned = inst.opcode() == Opcode::FPToUI;
    let bits = match cx.types().int_bits(inst.ty()) {
        Some(bits) if bits <= 32 || bits == 64 => bits,
        Some(_) => return unsupported(I64_CAST),
        None => return unsupported(crate::target::VECTOR),
    };
    let src = cx.use_mem(arg)?;
    if bits == 64 {
        return to_i64(cx, inst, src, s, unsigned);
    }
    let rd = cx.def_reg(inst)?;
    if unsigned && bits == 32 {
        load_x(cx, s, X, src);
        below(cx, 16);
        match s {
            Sse::S => word(cx, 0, 0x4f00_0000),
            Sse::D => {
                word(cx, 0, 0);
                word(cx, 4, 0x41e0_0000);
            }
        }
        put(cx, mc::subs_rm(s.fp(), X, sp(0)));
        above(cx, 16);
        put(cx, mc::cvtts2si_rr(Size::D, ECX, s.fp(), X));
        frame_inst(cx, src, |m| mc::cvtts2si_rm(Size::D, EAX, s.fp(), m));
        put(cx, mc::mov_rr(Size::D, EDX, EAX));
        put(cx, mc::sar_ri(Size::D, EDX, 31));
        put(cx, mc::and_rr(Size::D, ECX, EDX));
        put(cx, mc::or_rr(Size::D, EAX, ECX));
    } else {
        frame_inst(cx, src, |m| mc::cvtts2si_rm(Size::D, EAX, s.fp(), m));
    }
    put(cx, mc::mov_rr(Size::D, gp(rd), EAX));
    cx.finish(inst, rd)
}

/// `2^63` as a `float` (`0x5F000000`).
const TWO_63: i64 = 0x5F00_0000;

fn to_i64<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    src: FrameRef,
    s: Sse,
    unsigned: bool,
) -> CgResult<()> {
    frame_inst(cx, src, |m| mc::fld_m(s.x87(), m));
    let r = cx.def_mem(inst)?;
    below(cx, 16);
    if unsigned {
        put(cx, mc::mov_mi(Size::D, sp(12), TWO_63));
        put(cx, mc::fld_m(X87Size::S, sp(12)));
        put(cx, mc::fucomip(St::St1));
        put(cx, mc::setcc_r(Cond::Be, EDX));
        put(cx, mc::movzx_rr(Size::D, EDX, Size::B, EDX));
        put(cx, mc::mov_rr(Size::D, ECX, EDX));
        put(cx, mc::neg_r(Size::D, ECX));
        put(cx, mc::and_ri(Size::D, ECX, TWO_63));
        put(cx, mc::mov_mr(Size::D, sp(12), ECX));
        put(cx, mc::fsub_m(FpSize::S, sp(12)));
        put(cx, mc::shl_ri(Size::D, EDX, 31));
    }
    put(cx, mc::fnstcw_m(sp(0)));
    put(cx, mc::movzx_rm(Size::D, ECX, Size::W, sp(0)));
    put(cx, mc::or_ri(Size::D, ECX, 0xc00));
    put(cx, mc::mov_mr(Size::W, sp(2), ECX));
    put(cx, mc::fldcw_m(sp(2)));
    put(cx, mc::fistp_m(Size::Q, sp(4)));
    put(cx, mc::fldcw_m(sp(0)));
    for at in [0, 4] {
        put(cx, mc::mov_rm(Size::D, ECX, sp(4 + at)));
        if unsigned && at == 4 {
            put(cx, mc::xor_rr(Size::D, ECX, EDX));
        }
        frame_inst(cx, r.offset_by(at as u32), |m| mc::mov_mr(Size::D, m, ECX));
    }
    above(cx, 16);
    Ok(())
}

// ---- intrinsics and libm ----

fn routine_op(which: Intrinsic<'_>) -> Option<FpOp> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Sin => O::Sin,
        I::Cos => O::Cos,
        I::Tan => O::Tan,
        I::Exp => O::Exp,
        I::Exp2 => O::Exp2,
        I::Log => O::Log,
        I::Log2 => O::Log2,
        I::Log10 => O::Log10,
        I::Pow => O::Pow,
        I::Floor => O::Floor,
        I::Ceil => O::Ceil,
        I::Trunc => O::Trunc,
        I::Round => O::Round,
        I::RoundEven => O::RoundEven,
        I::NearbyInt => O::NearbyInt,
        I::Rint => O::Rint,
        I::Fma => O::Fma,
        I::Asin => O::Asin,
        I::Acos => O::Acos,
        I::Atan => O::Atan,
        I::Atan2 => O::Atan2,
        I::Sinh => O::Sinh,
        I::Cosh => O::Cosh,
        I::Tanh => O::Tanh,
        I::Exp10 => O::Exp10,
        _ => return None,
    })
}

/// libm's routine of `op` for `s` (`floorf`, `floor`).
fn routine(op: FpOp, s: Sse) -> CgResult<RuntimeFn> {
    match fp_routine(op, s.fp_type(), Arch::X86_64) {
        Some(func) => Ok(func),
        None => unsupported(INTRINSIC),
    }
}

fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &[Value],
) -> CgResult<()> {
    let Some(s) = sse_of(cx, inst.ty()) else {
        return unsupported(if matches!(cx.types().kind(inst.ty()), TypeKind::Vector { .. }) {
            crate::target::VECTOR
        } else {
            INTRINSIC
        });
    };
    if args.iter().any(|&a| value_ty(cx, a) != inst.ty()) {
        return unsupported(INTRINSIC);
    }
    match (which, args) {
        (Intrinsic::Sqrt, &[x]) => {
            let src = cx.use_mem(x)?;
            frame_inst(cx, src, |m| mc::sqrts_rm(s.fp(), X, m));
            set(cx, inst, s, X)
        }
        (Intrinsic::Fabs, &[x]) => {
            copy_of(cx, inst, x, s.bytes())?;
            let r = cx.def_mem(inst)?;
            frame_inst(cx, r.offset_by(s.bytes() - 1), |m| mc::and_mi(Size::B, m, 0x7f));
            Ok(())
        }
        (Intrinsic::CopySign, &[x, y]) => {
            let ys = cx.use_mem(y)?;
            copy_of(cx, inst, x, s.bytes())?;
            let r = cx.def_mem(inst)?;
            let sign = s.bytes() - 1;
            frame_inst(cx, ys.offset_by(sign), |m| mc::movzx_rm(Size::D, ECX, Size::B, m));
            put(cx, mc::and_ri(Size::D, ECX, 0x80));
            frame_inst(cx, r.offset_by(sign), |m| mc::and_mi(Size::B, m, 0x7f));
            frame_inst(cx, r.offset_by(sign), |m| mc::or_mr(Size::B, m, ECX));
            Ok(())
        }
        (Intrinsic::FmulAdd, &[a, b, c]) => {
            // `llc` without FMA: `muls` then `adds`, two roundings.
            let (bp, cp) = (cx.use_mem(b)?, cx.use_mem(c)?);
            get(cx, s, X, a)?;
            frame_inst(cx, bp, |m| mc::muls_rm(s.fp(), X, m));
            frame_inst(cx, cp, |m| mc::adds_rm(s.fp(), X, m));
            set(cx, inst, s, X)
        }
        (Intrinsic::MinNum | Intrinsic::MaxNum, &[a, b]) => min_max(cx, inst, s, a, b),
        _ => match routine_op(which) {
            Some(op) => libcall(cx, inst, routine(op, s)?, args, s),
            None => unsupported(INTRINSIC),
        },
    }
}

/// `minnum`/`maxnum` as `llc` (measured, 19 and 22): `b` where `a` is a
/// NaN, else `mins b, a` (`b < a ? b : a`; `maxs`: `b > a ? b : a`).
fn min_max<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, s: Sse, a: Value, b: Value) -> CgResult<()> {
    let (ap, bp) = (cx.use_mem(a)?, cx.use_mem(b)?);
    let (mask, other) = (Xmm::Xmm5, Xmm::Xmm4);
    load_x(cx, s, mask, ap);
    put(cx, mc::cmps_rr(FpCmp::Unord, s.fp(), mask, mask));
    load_x(cx, s, X, bp);
    let op = if matches!(cx.body.inst(inst.id()).kind(), InstKind::Call(site)
        if site.intrinsic() == Some(Intrinsic::MinNum))
    {
        mc::mins_rm
    } else {
        mc::maxs_rm
    };
    frame_inst(cx, ap, |m| op(s.fp(), X, m));
    load_x(cx, s, other, bp);
    put(cx, mc::andp_rr(s.fp(), other, mask));
    put(cx, mc::andnp_rr(s.fp(), mask, X));
    put(cx, mc::orp_rr(s.fp(), mask, other));
    set(cx, inst, s, mask)
}

/// A call of libm's `func` with `args` on the stack at their width and
/// the result in `st0`, stored at its type's width (rounded at once, as
/// `llc` stores it).
fn libcall<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    func: RuntimeFn,
    args: &[Value],
    s: Sse,
) -> CgResult<()> {
    let mut places = Vec::with_capacity(args.len());
    for &a in args {
        places.push(cx.use_mem(a)?);
    }
    let area = (s.bytes() * args.len() as u32).next_multiple_of(16);
    below(cx, area);
    for (i, &place) in places.iter().enumerate() {
        copy_frame(cx, FrameRef::Outgoing(s.bytes() * i as u32), place, s.bytes());
    }
    let at = cx.offset();
    crate::pic::emit_call(cx, CallTarget::Sym(SymRef::Runtime(func)), 0)?;
    let len = cx.offset() - at;
    cx.record_call_site(at, len);
    above(cx, area);
    let inst = cx.body.inst(inst.id());
    let r = cx.def_mem(inst)?;
    frame_inst(cx, r, |m| mc::fstp_m(s.x87(), m));
    Ok(())
}

// ---- the 80-bit value of a call result (`llc`'s fold) ----

/// The 80-bit copies of the SSE call results of the function
/// (`FnState::sse`).
///
/// `llc` takes a `float`/`double` call result out of `st0` as an exact
/// `fp_round` of the 80-bit register, and its DAG (one per block) folds
/// that node into an `fpext`, an `fptrunc` or a `ret` of the result in
/// the same block (measured, 19 and 22, `pentium4` and `pentium3`; the
/// spec's §12 "The 80-bit value of a call result"). A *view* is a value
/// that `llc` still has as the 80-bit register `x`: the call result, an
/// `fpext` of a view, and an `fptrunc` of an `fpext` of a view back to
/// the view's type. In the call's block, up to an `atomicrmw` that
/// `llc` expands into a loop (a new block), and across anything else
/// (calls included: `llc` spills `x` at 80 bits):
/// - an `fpext` of a view to `U` is `x` rounded to `U`;
/// - an `fptrunc` of a view to `U` is `x` rounded to `U` once; but an
///   `fptrunc` of an `fpext` of a view `w` to `U` is `w` when `U` is
///   `w`'s type, else `w`'s rounded value converted;
/// - a `ret` of a view returns `x`.
///
/// Every other use reads the value rounded to its type (its home).
#[derive(Clone, Default, Debug)]
pub struct Views {
    /// The SSE call results with a copy: the call, the 12-byte slot of
    /// `x`, and the end of its reach (the end of its block or an
    /// `atomicrmw` loop), in the order of the calls.
    live: Vec<(u32, FrameRef, u32)>,
    /// The copy the call being lowered stores ([`store_call_copy`]).
    pending: Option<(u32, FrameRef, u32)>,
}

/// Bounds the chains of [`view`] (an `fpext` of an `fpext` ...).
const VIEW_DEPTH: u32 = 8;

/// Whether `llc` (`AtomicExpand`, which runs at `-O0`) turns `inst` into
/// a loop, which ends the block's DAG (measured: an `and`/`or`/`xor`
/// whose result is used, any 64-bit operation, the floating ones, `nand`,
/// `min`/`max` and the rest; `xchg`, `add`, `sub` of at most 32 bits are
/// one instruction, and so are `cmpxchg` and an unused `and`/`or`/`xor`).
fn splits_block(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let InstKind::AtomicRmw { op, value, .. } = inst.kind() else { return false };
    let ty = value_ty(cx, value);
    let types = cx.types();
    let narrow = match types.kind(ty) {
        TypeKind::Float | TypeKind::Ptr { .. } => true,
        _ => types.int_bits(ty).is_some_and(|b| b <= 32),
    };
    let one = match op {
        RmwOp::Xchg | RmwOp::Add | RmwOp::Sub => true,
        RmwOp::And | RmwOp::Or | RmwOp::Xor => inst.uses().is_unused(),
        _ => false,
    };
    !(narrow && one)
}

/// The first raw index past the reach of the call result at `at`: the end
/// of its block, or an `atomicrmw` loop before it.
fn reach(cx: &FnCx<'_, X86>, at: u32) -> u32 {
    let layout = cx.body.layout();
    let k = layout.partition_point(|&b| cx.body.block(b).inst_range().end <= at);
    let end = layout.get(k).map_or(at + 1, |&b| cx.body.block(b).inst_range().end);
    (at + 1..end).find(|&raw| splits_block(cx, cx.body.inst(InstId::new(raw)))).unwrap_or(end)
}

/// Whether instruction `user` reads `v` as a view: an `fpext`, an
/// `fptrunc` or a `ret` of it.
fn reads_view(user: Inst<'_>, v: Value) -> bool {
    match user.kind() {
        InstKind::Cast { arg } => {
            arg == v && matches!(user.opcode(), Opcode::FPExt | Opcode::FPTrunc)
        }
        InstKind::Ret { value } => value == Some(v),
        _ => false,
    }
}

/// Brackets the call sequence of `inst` (from `x87::call_result`): a
/// result of an SSE type that an `fpext`, `fptrunc` or `ret` in its reach
/// reads gets a 12-byte slot for `x`, stored by [`store_call_copy`].
/// Whether the result is of an SSE type (then `x87` keeps no copy of
/// its own).
///
/// # Errors
/// What allocating the slot reports.
pub fn call_result(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, on: bool) -> CgResult<bool> {
    cx.state.sse.pending = None;
    if !is_sse(cx, inst.ty()) {
        return Ok(false);
    }
    if on && !inst.uses().is_unused() {
        let at = inst.id().raw();
        let end = reach(cx, at);
        let result = inst.result();
        if (at + 1..end).any(|raw| reads_view(cx.body.inst(InstId::new(raw)), result)) {
            let slot = cx.alloc_slot(12, 4)?;
            cx.state.sse.pending = Some((at, slot, end));
        }
    }
    Ok(true)
}

/// Stores `st0` at 80 bits to the slot [`call_result`] took, keeping
/// `st0` (from `x87::store_call_result`, before the glue store).
pub fn store_call_copy(cx: &mut FnCx<'_, X86>) {
    if let Some(copy) = cx.state.sse.pending.take() {
        put(cx, mc::fld_st(St::St0));
        frame_inst(cx, copy.1, |m| mc::fstp_m(X87Size::T, m));
        cx.state.sse.live.push(copy);
    }
}

/// The slot of `x` if `v` is a view for instruction `user` (the struct
/// comment).
fn view(cx: &FnCx<'_, X86>, v: Value, user: u32, depth: u32) -> Option<FrameRef> {
    let id = v.as_inst()?;
    let inst = cx.body.inst(id);
    match (inst.opcode(), inst.kind()) {
        (Opcode::FPExt, InstKind::Cast { arg }) if depth > 0 => view(cx, arg, user, depth - 1),
        (Opcode::FPTrunc, InstKind::Cast { arg }) if depth > 0 => {
            let w = ext_operand(cx, arg)?;
            (value_ty(cx, w) == inst.ty()).then(|| view(cx, w, user, depth - 1))?
        }
        (Opcode::Call, _) => {
            let live = &cx.state.sse.live;
            let k = live.binary_search_by_key(&id.raw(), |e| e.0).ok()?;
            let (at, slot, end) = live[k];
            (at < user && user < end).then_some(slot)
        }
        _ => None,
    }
}

/// The operand of `v` if `v` is an `fpext`.
fn ext_operand(cx: &FnCx<'_, X86>, v: Value) -> Option<Value> {
    let inst = cx.body.inst(v.as_inst()?);
    match inst.kind() {
        InstKind::Cast { arg } if inst.opcode() == Opcode::FPExt => Some(arg),
        _ => None,
    }
}

/// The slot of `x` that `user` reads for `value`, if `value` is a view
/// (from `x87::ret_copy` for a `ret`, and from `x87::push` for any x87
/// operation: at SSE without SSE2 an `fpext` of a `float` call result to
/// `double` is `x` for every x87 use in its reach, measured `llc` 19 and
/// 22).
pub fn ret_view(cx: &FnCx<'_, X86>, value: Value, user: Inst<'_>) -> Option<FrameRef> {
    if cx.state.sse.live.is_empty() {
        return None;
    }
    view(cx, value, user.id().raw(), VIEW_DEPTH)
}

// ---- the dispatch ----

/// The lowering of an instruction [`owns`] says is this module's.
///
/// # Errors
/// The refusals of this module; what the operands report.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    match inst.kind() {
        InstKind::Binary { lhs, rhs } => binary(cx, inst, lhs, rhs),
        InstKind::Unary { arg } => unary(cx, inst, arg),
        InstKind::Load { ptr, mem } => load(cx, inst, ptr, mem),
        InstKind::Store { value, ptr, mem } => store(cx, value, ptr, mem),
        InstKind::Select { cond, if_true, if_false } => select(cx, inst, cond, if_true, if_false),
        InstKind::Cast { arg } => cast(cx, inst, arg),
        InstKind::FCmp { pred, lhs, rhs } => fcmp(cx, inst, pred, lhs, rhs),
        InstKind::Call(site) => match site.intrinsic() {
            Some(which) => intrinsic(cx, inst, which, site.args()),
            None => unsupported(INTRINSIC),
        },
        _ => Err(CgError::unsupported(crate::x87::FLOAT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_predicate_has_a_condition() {
        for &pred in FloatPredicate::ALL {
            let _ = conditions(pred);
        }
        assert_eq!(conditions(FloatPredicate::Ugt), (true, FpCond::One(Cond::B)));
    }

    #[test]
    fn the_forms_are_admitted_in_32_bit_mode() {
        let m = Mem::base(ESP, 4);
        for inst in [
            mc::movs_rm(FpSize::D, X, m),
            mc::movs_mr(FpSize::S, m, Xmm::Xmm7),
            mc::adds_rm(FpSize::D, X, m),
            mc::ucomis_rm(FpSize::S, X, m),
            mc::cvtsi2s_rr(FpSize::D, X, Size::D, ECX),
            mc::cvtts2si_rm(Size::D, EAX, FpSize::S, m),
            mc::cvtss2sd_rm(X, m),
            mc::cmps_rr(FpCmp::Unord, FpSize::D, Xmm::Xmm5, Xmm::Xmm5),
            mc::andnp_rr(FpSize::S, Xmm::Xmm5, X),
        ] {
            mc::Mode::X86.admit(inst).expect("32-bit mode admits it");
        }
    }
}
