//! Instructions on values of class `Fpr` (`spec/codegen.md` §2.2):
//! floating-point binary operations, `fneg`, `freeze`, casts with a
//! floating-point or vector side, `fcmp`, `icmp` of vectors, `select`.
//!
//! **Owner: WP-039.** Tests: `tests/float.rs`. The dispatch in `target.rs`
//! is final; every entry point starts with
//! [`deferred_first`](crate::common::deferred_first). The decisions are
//! `spec/codegen.md` §5.5.1, "Floating point (WP-039)":
//!
//! - `float` and `double` use the scalar instructions; `half` is computed
//!   in single precision (`fcvt` in, the operation, `fcvt` out), which is
//!   exact for `fadd` .. `fdiv` (single precision has more than twice
//!   half's significand plus two bits) and for compares and conversions;
//!   `fp128` arithmetic calls the `__addtf3` family. `frem` calls
//!   `fmod`/`fmodf`.
//! - The default floating-point environment only: no instruction here
//!   reads or writes `FPCR`/`FPSR`, and a NaN an operation computes is
//!   whatever the hardware produces (`FPCR.DN` clear on Linux: the first
//!   NaN operand, quieted), which `LangRef` allows (`spec/interp.md` §5.1).
//! - `fcmp` defers itself when the next instruction consumes it (§4.6);
//!   [`flags`] is the one routine that sets `NZCV` from an `fcmp` and says
//!   which condition(s) to test, for `select` here and `condbr` later.
//! - Out-of-range `fptoui`/`fptosi` are poison (LangRef): the saturating
//!   `fcvtz*` result is as good as any.

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn};
use hod_ir::{FloatPredicate, Inst, InstKind, IntPredicate, Opcode, TypeId, TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{
    Arr, Cond, FpArr, FpFormat, FpSize, Load, MemSize, RegZr, Size, VMemSize, VReg, VecBits,
};

use crate::common::{
    Cursor, V31, X16, deferred_first, load_fpr_at, load_gpr_at, put, size_for_bits, store_fpr_at,
    store_gpr_at, unsupported, value_ty, vr, xr,
};
use crate::target::A64;

/// A binary operation on vector-register values.
pub const BINARY: &str = "a64:fp-binary";
/// `fneg`.
pub const FNEG: &str = "a64:fneg";
/// `freeze` of a vector-register value.
pub const FREEZE: &str = "a64:fp-freeze";
/// A cast with a vector-register side.
pub const CAST: &str = "a64:fp-cast";
/// `fcmp`.
pub const FCMP: &str = "a64:fcmp";
/// `icmp` of vector-register values.
pub const VECTOR_ICMP: &str = "a64:vector-icmp";
/// `select` of vector-register values.
pub const SELECT: &str = "a64:fp-select";

/// Arithmetic, a compare or a conversion of `bfloat`: the base
/// architecture has no `bfloat` instruction (`BFCVT` is `FEAT_BF16`, which
/// the encoder does not provide). Moves (`select`, `freeze`, `bitcast`)
/// of `bfloat` are lowered.
pub const BFLOAT: &str = "a64:bfloat";
/// An `fp128` operation without a runtime function. Since WP-067 every
/// `fp128` operation the IR has is lowered (`fcmp` through `__eqtf2`,
/// `__lttf2`, `__letf2`, `__unordtf2`; conversions through the
/// `__extend*tf2`, `__trunctf*2`, `__fix*tf*`, `__float*tf` families;
/// `frem` through `fmodl`); the name stays for a form that cannot occur.
pub const FP128_LIBCALL: &str = "a64:fp128-libcall";
/// `frem` of `half`: lowered since WP-067 (through `fmodf`); the name
/// stays for the record.
pub const HALF_FREM: &str = "a64:half-frem";
/// `fcmp` of vectors: its `<N x i1>` result is of class `Mem`.
pub const VECTOR_FCMP: &str = "a64:vector-fcmp";
/// `select` with a vector condition (`<N x i1>`, class `Mem`).
pub const VECTOR_SELECT: &str = "a64:vector-select";
/// A vector operation with no single instruction in the encoder:
/// integer division and remainder, `mul` of 64-bit elements, `frem`, any
/// arithmetic on vectors of `half`, and elements that are not 8, 16, 32
/// or 64 bits wide.
pub const VECTOR_OP: &str = "a64:vector-op";

// ---- shapes ----

/// The scalar formats the hardware computes in (`half` through single).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fp {
    H,
    S,
    D,
}

impl Fp {
    fn format(self) -> FpFormat {
        match self {
            Fp::H => FpFormat::H,
            Fp::S => FpFormat::S,
            Fp::D => FpFormat::D,
        }
    }

    /// The size an operation computes in: `half` in single precision.
    fn size(self) -> FpSize {
        if self == Fp::D { FpSize::D } else { FpSize::S }
    }
}

/// What an `Fpr`-class type is, for the lowering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// `half`, `float`, `double`.
    Scalar(Fp),
    /// `bfloat`.
    BFloat,
    /// `fp128`.
    Quad,
    /// A vector of 8-, 16-, 32- or 64-bit integers or of pointers. A
    /// 64-bit vector of one 64-bit element computes as `2D`: the upper
    /// half of the register is unspecified.
    IntVec(Arr),
    /// A vector of `float` or `double` (`<1 x double>` as `2D`).
    FpVec(FpArr),
    /// A vector of `half`.
    HalfVec,
    /// Any other vector of class `Fpr` (elements of `bfloat`, `fp128`,
    /// `i128`, or integers of a width that is not a byte multiple).
    OtherVec,
}

impl Shape {
    /// Whether the value fills the whole 128-bit register.
    fn is_wide(self, bits: u64) -> bool {
        bits > 64 || self == Shape::Quad
    }
}

/// The shape of `ty`, a type of class `Fpr`.
fn shape(cx: &FnCx<'_, A64>, ty: TypeId) -> Shape {
    let types = cx.types();
    match types.kind(ty) {
        TypeKind::Half => Shape::Scalar(Fp::H),
        TypeKind::Float => Shape::Scalar(Fp::S),
        TypeKind::Double => Shape::Scalar(Fp::D),
        TypeKind::BFloat => Shape::BFloat,
        TypeKind::Fp128 => Shape::Quad,
        TypeKind::Vector { elem, .. } => {
            let wide = types.size_in_bits_of(ty) > 64;
            let elem_bits = match types.kind(elem) {
                TypeKind::Ptr { .. } => 64,
                TypeKind::Int(bits) => bits,
                TypeKind::Float => {
                    return Shape::FpVec(if wide { FpArr::S4 } else { FpArr::S2 });
                }
                TypeKind::Double => return Shape::FpVec(FpArr::D2),
                TypeKind::Half => return Shape::HalfVec,
                _ => return Shape::OtherVec,
            };
            let arr = match (elem_bits, wide) {
                (8, false) => Arr::B8,
                (8, true) => Arr::B16,
                (16, false) => Arr::H4,
                (16, true) => Arr::H8,
                (32, false) => Arr::S2,
                (32, true) => Arr::S4,
                (64, _) => Arr::D2,
                _ => return Shape::OtherVec,
            };
            Shape::IntVec(arr)
        }
        _ => Shape::OtherVec,
    }
}

/// The register-width view of a vector arrangement.
fn bits_of(arr: Arr) -> VecBits {
    arr.vec_bits()
}

// ---- conditions (§4.6) ----

/// The condition(s) under which an `fcmp` whose flags [`flags`] set
/// holds: `first`, or `second` when there is one (`one` and `ueq` need
/// two). `fcmp true` and `fcmp false` are `EQ` and `NE` after a compare
/// that sets `Z`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FpCond {
    /// The first condition.
    pub first: Cond,
    /// The second condition: the predicate holds when either does.
    pub second: Option<Cond>,
}

/// The `NZCV` conditions of each predicate after `FCMP` (unordered sets
/// `C` and `V`; less than sets `N`; equal sets `Z` and `C`; greater than
/// sets `C`): the table of LLVM's `changeFPCCToAArch64CC`.
pub fn conditions(pred: FloatPredicate) -> FpCond {
    let one = |first| FpCond { first, second: None };
    match pred {
        FloatPredicate::Oeq | FloatPredicate::True => one(Cond::Eq),
        FloatPredicate::Ogt => one(Cond::Gt),
        FloatPredicate::Oge => one(Cond::Ge),
        FloatPredicate::Olt => one(Cond::Mi),
        FloatPredicate::Ole => one(Cond::Ls),
        FloatPredicate::One => FpCond { first: Cond::Mi, second: Some(Cond::Gt) },
        FloatPredicate::Ord => one(Cond::Vc),
        FloatPredicate::Uno => one(Cond::Vs),
        FloatPredicate::Ueq => FpCond { first: Cond::Eq, second: Some(Cond::Vs) },
        FloatPredicate::Ugt => one(Cond::Hi),
        FloatPredicate::Uge => one(Cond::Pl),
        FloatPredicate::Ult => one(Cond::Lt),
        FloatPredicate::Ule => one(Cond::Le),
        FloatPredicate::Une | FloatPredicate::False => one(Cond::Ne),
    }
}

/// Performs the scalar `fcmp` `compare` (deferred or being lowered) into
/// `NZCV` and returns the condition(s) under which it holds. Its operands
/// are consumed: their registers are free again when this returns, so the
/// caller may pin two more vector registers.
///
/// For the integration of `condbr` (WP-037's `control.rs`): when the
/// condition of a `condbr` is a deferred `fcmp` (`FnCx::deferred` gives
/// an instruction whose kind is `InstKind::FCmp`), call
/// `deferred_first(cx, inst, Some(cond))`, then this, then branch on
/// `first` and, if present, on `second` to the true block, and fall or
/// branch to the false block. Nothing between this call and the branch
/// may set the flags; materializing constants, loads and register moves
/// do not.
///
/// An `fcmp` of `fp128` is calls of runtime functions (WP-067): every
/// register is displaced and every temporary ends, so the caller asks for
/// nothing before this and for what it needs after it.
///
/// # Errors
/// [`VECTOR_FCMP`], [`BFLOAT`] for the operand types it does not compare;
/// `Internal` if `compare` is not an `fcmp`.
pub fn flags<'a>(cx: &mut FnCx<'a, A64>, compare: Inst<'a>) -> CgResult<FpCond> {
    // `spec/dwarf.md` §5.8: the compare's code is at the compare's
    // location, inside its consumer's.
    let outer = cx.line_loc();
    cx.line_switch(cx.row_loc_of(compare));
    let flags = compare_flags(cx, compare);
    cx.line_switch(outer);
    flags
}

/// [`flags`] without the line switch.
fn compare_flags<'a>(cx: &mut FnCx<'a, A64>, compare: Inst<'a>) -> CgResult<FpCond> {
    let InstKind::FCmp { pred, lhs, rhs } = compare.kind() else {
        return cx.bug("flags of an instruction that is not an fcmp");
    };
    if shape(cx, value_ty(cx, lhs)) == Shape::BFloat {
        return crate::tail2::bfloat_flags(cx, compare, pred, lhs, rhs);
    }
    let format = compare_format(cx, lhs)?;
    deferred_first(cx, compare, None)?;
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp(Size::X, RegZr::ZR, RegZr::ZR));
        return Ok(conditions(pred));
    }
    let Some(fp) = format else { return quad_flags(cx, pred, lhs, rhs) };
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    let (va, vb) = (vr(a), vr(b));
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::S, va, FpFormat::H, va));
        put(cx, mc::fcvt(FpFormat::S, vb, FpFormat::H, vb));
    }
    put(cx, mc::fcmp(fp.size(), va, vb));
    give_back(cx, lhs, a);
    give_back(cx, rhs, b);
    Ok(conditions(pred))
}

/// The format of an `fcmp` operand (`None` for `fp128`), or the refusal.
fn compare_format(cx: &FnCx<'_, A64>, operand: Value) -> CgResult<Option<Fp>> {
    match shape(cx, value_ty(cx, operand)) {
        Shape::Scalar(fp) => Ok(Some(fp)),
        Shape::Quad => Ok(None),
        // WP-095: `flags` hands a `bfloat` compare to `tail2` first.
        Shape::BFloat => Ok(Some(Fp::S)),
        _ => unsupported(VECTOR_FCMP),
    }
}

// ---- `fp128` through runtime functions (WP-067) ----

/// Calls `func`, whose result is an integer, and returns its register.
fn runtime_int(cx: &mut FnCx<'_, A64>, func: RuntimeFn, args: &[CallArg]) -> CgResult<PReg> {
    match cx.call_runtime(func, args, None)? {
        Some(reg) => Ok(reg),
        None => cx.bug("a runtime function returned nothing"),
    }
}

/// The `NZCV` of `fcmp <pred> fp128 %lhs, %rhs` (`pred` neither `true`
/// nor `false`), as LLVM's `softenSetCCOperands` computes it: one call
/// whose `int` result is compared with 0 (signed), the operands swapped
/// for `ogt`, `oge`, `ule` and `ult` so that only the `__eqtf2`,
/// `__lttf2`, `__letf2` and `__unordtf2` results' signs are relied on:
/// `__lttf2` and `__letf2` are positive for unordered operands in libgcc
/// and compiler-rt alike, `__eqtf2` is 0 only for ordered equal ones.
/// `ueq` and `one` need two calls ([`quad_two_calls`]).
fn quad_flags(
    cx: &mut FnCx<'_, A64>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<FpCond> {
    use FloatPredicate as P;
    use RuntimeFn::{EqTf2, LeTf2, LtTf2, UnordTf2};
    let (func, swap, first) = match pred {
        P::Oeq => (EqTf2, false, Cond::Eq),
        P::Une => (EqTf2, false, Cond::Ne),
        P::Olt => (LtTf2, false, Cond::Lt),
        P::Uge => (LtTf2, false, Cond::Ge),
        P::Ogt => (LtTf2, true, Cond::Lt),
        P::Ule => (LtTf2, true, Cond::Ge),
        P::Ole => (LeTf2, false, Cond::Le),
        P::Ugt => (LeTf2, false, Cond::Gt),
        P::Oge => (LeTf2, true, Cond::Le),
        P::Ult => (LeTf2, true, Cond::Gt),
        P::Ord => (UnordTf2, false, Cond::Eq),
        P::Uno => (UnordTf2, false, Cond::Ne),
        P::Ueq | P::One => return quad_two_calls(cx, pred, lhs, rhs),
        P::True | P::False => return cx.bug("a constant fcmp predicate at the call"),
    };
    let (x, y) = if swap { (rhs, lhs) } else { (lhs, rhs) };
    let r = runtime_int(cx, func, &[CallArg::Value(x), CallArg::Value(y)])?;
    put(cx, mc::cmp_imm(Size::W, xr(r).sp(), 0));
    cx.free_temp(r);
    Ok(FpCond { first, second: None })
}

/// `fcmp ueq` and `fcmp one` of `fp128`: `__eqtf2` and `__unordtf2` of
/// copies of the operands kept in a frame slot (each call consumes its
/// arguments), the first result kept there too; then `cmp unord, #0` ·
/// `ccmp eq, #0, #Z, eq`: `Z` is set when unordered or equal, so `ueq`
/// is `EQ` and `one` is `NE`.
fn quad_two_calls(
    cx: &mut FnCx<'_, A64>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<FpCond> {
    let slot = cx.alloc_slot(48, 16)?;
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    let mut at = Cursor::frame(cx, slot, X16);
    store_fpr_at(cx, VMemSize::Q, vr(a), &mut at);
    at.advance(16);
    store_fpr_at(cx, VMemSize::Q, vr(b), &mut at);
    give_back(cx, lhs, a);
    give_back(cx, rhs, b);
    let eq = quad_call_from(cx, slot, RuntimeFn::EqTf2)?;
    let mut at = Cursor::frame(cx, slot, X16);
    at.advance(32);
    store_gpr_at(cx, MemSize::W, xr(eq).zr(), &mut at);
    let unord = quad_call_from(cx, slot, RuntimeFn::UnordTf2)?;
    let mut at = Cursor::frame(cx, slot, X16);
    at.advance(32);
    load_gpr_at(cx, Load::W, X16, &mut at);
    put(cx, mc::cmp_imm(Size::W, xr(unord).sp(), 0));
    put(cx, mc::ccmp(Size::W, X16.zr(), RegZr::ZR, 0b0100, Cond::Eq));
    cx.free_temp(unord);
    let first = if pred == FloatPredicate::Ueq { Cond::Eq } else { Cond::Ne };
    Ok(FpCond { first, second: None })
}

/// Calls `func` on the two `fp128` values at `slot` and `slot + 16`,
/// loaded into temporaries.
fn quad_call_from(cx: &mut FnCx<'_, A64>, slot: FrameRef, func: RuntimeFn) -> CgResult<PReg> {
    let t1 = cx.temp(Class::Fpr)?;
    let t2 = cx.temp(Class::Fpr)?;
    let mut at = Cursor::frame(cx, slot, X16);
    load_fpr_at(cx, VMemSize::Q, vr(t1), &mut at);
    at.advance(16);
    load_fpr_at(cx, VMemSize::Q, vr(t2), &mut at);
    runtime_int(cx, func, &[CallArg::Reg(t1), CallArg::Reg(t2)])
}

/// A conversion with an `fp128` side (WP-067): one runtime call, as `llc`
/// makes it. `half` is widened to `float` first for `fpext` (exact);
/// `fptrunc` to `half` calls `__trunctfhf2` (one rounding). An integer of
/// a width other than 32 and 64 is extended to the register size first
/// (§2.2); the `fptoui`/`fptosi` result above the width is unspecified.
fn quad_cast<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    from: Option<Shape>,
    to: Option<Shape>,
) -> CgResult<()> {
    let op = inst.opcode();
    deferred_first(cx, inst, None)?;
    let (func, args) = match (op, from, to) {
        (Opcode::FPExt, Some(Shape::Scalar(Fp::H)), Some(Shape::Quad)) => {
            let a = cx.use_reg(arg)?;
            let t = cx.temp(Class::Fpr)?;
            put(cx, mc::fcvt(FpFormat::S, vr(t), FpFormat::H, vr(a)));
            give_back(cx, arg, a);
            (RuntimeFn::ExtendSfTf2, CallArg::Reg(t))
        }
        (Opcode::FPExt, Some(Shape::Scalar(Fp::S)), Some(Shape::Quad)) => {
            (RuntimeFn::ExtendSfTf2, CallArg::Value(arg))
        }
        (Opcode::FPExt, Some(Shape::Scalar(Fp::D)), Some(Shape::Quad)) => {
            (RuntimeFn::ExtendDfTf2, CallArg::Value(arg))
        }
        (Opcode::FPTrunc, Some(Shape::Quad), Some(Shape::Scalar(fp))) => {
            let func = match fp {
                Fp::H => RuntimeFn::TruncTfHf2,
                Fp::S => RuntimeFn::TruncTfSf2,
                Fp::D => RuntimeFn::TruncTfDf2,
            };
            (func, CallArg::Value(arg))
        }
        (Opcode::FPToSI | Opcode::FPToUI, Some(Shape::Quad), None) => {
            let wide = int_width(cx, inst.ty())? > 32;
            let func = match (op == Opcode::FPToSI, wide) {
                (true, false) => RuntimeFn::FixTfSi,
                (true, true) => RuntimeFn::FixTfDi,
                (false, false) => RuntimeFn::FixunsTfSi,
                (false, true) => RuntimeFn::FixunsTfDi,
            };
            (func, CallArg::Value(arg))
        }
        (Opcode::SIToFP | Opcode::UIToFP, None, Some(Shape::Quad)) => {
            let bits = int_width(cx, value_ty(cx, arg))?;
            let signed = op == Opcode::SIToFP;
            let func = match (signed, bits > 32) {
                (true, false) => RuntimeFn::FloatSiTf,
                (true, true) => RuntimeFn::FloatDiTf,
                (false, false) => RuntimeFn::FloatunSiTf,
                (false, true) => RuntimeFn::FloatunDiTf,
            };
            if bits == 32 || bits == 64 {
                (func, CallArg::Value(arg))
            } else {
                let a = cx.use_reg(arg)?;
                let t = cx.temp(Class::Gpr)?;
                let extend = if signed { mc::sbfx } else { mc::ubfx };
                put(cx, extend(size_for_bits(bits), xr(t).zr(), xr(a).zr(), 0, bits));
                give_back(cx, arg, a);
                (func, CallArg::Reg(t))
            }
        }
        _ => return unsupported(CAST),
    };
    let Some(reg) = cx.call_runtime(func, &[args], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    cx.finish(inst, reg)
}

/// `frem half`: the operands widened to `float` into temporaries (one
/// pinned at a time besides an operand, through `v31`), `fmodf`, the
/// result narrowed. `fmod` is exact, so the one rounding is the
/// narrowing's, as `llc`'s promotion does.
fn half_frem<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(lhs)?;
    let t1 = cx.temp(Class::Fpr)?;
    put(cx, mc::fcvt(FpFormat::S, vr(t1), FpFormat::H, vr(a)));
    give_back(cx, lhs, a);
    let b = cx.use_reg(rhs)?;
    put(cx, mc::fcvt(FpFormat::S, V31, FpFormat::H, vr(b)));
    give_back(cx, rhs, b);
    let t2 = cx.temp(Class::Fpr)?;
    put(cx, mc::fmov(FpSize::S, vr(t2), V31));
    let r = fmod_call(cx, Fp::H, t1, t2)?;
    cx.finish(inst, r)
}

/// Releases a consumed operand's register before the instruction ends.
fn give_back(cx: &mut FnCx<'_, A64>, operand: Value, reg: PReg) {
    cx.release(operand);
    cx.free_temp(reg);
}

// ---- entry points ----

/// `%r = <op> %lhs, %rhs` of class `Fpr`.
///
/// # Errors
/// [`BINARY`] for an integer operation on a scalar (never valid IR),
/// [`VECTOR_OP`], [`BFLOAT`].
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let op = inst.opcode();
    match shape(cx, inst.ty()) {
        Shape::Scalar(fp) if op == Opcode::FRem => match fp {
            Fp::S | Fp::D => libcall(cx, inst, fmod_of(fp), lhs, rhs),
            Fp::H => half_frem(cx, inst, lhs, rhs),
        },
        Shape::Scalar(fp) => {
            let Some(word) = scalar_op(op) else { return unsupported(BINARY) };
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(lhs)?;
            let b = cx.use_reg(rhs)?;
            scalar_binary(cx, word, fp, vr(a), vr(b));
            cx.finish(inst, a)
        }
        Shape::Quad => {
            let func = match op {
                Opcode::FAdd => RuntimeFn::AddTf3,
                Opcode::FSub => RuntimeFn::SubTf3,
                Opcode::FMul => RuntimeFn::MulTf3,
                Opcode::FDiv => RuntimeFn::DivTf3,
                Opcode::FRem => RuntimeFn::Fmodl,
                _ => return unsupported(BINARY),
            };
            libcall(cx, inst, func, lhs, rhs)
        }
        Shape::BFloat => crate::tail2::bfloat_binary(cx, inst, lhs, rhs),
        Shape::FpVec(arr) => {
            let word = match op {
                Opcode::FAdd => mc::fadd_vec,
                Opcode::FSub => mc::fsub_vec,
                Opcode::FMul => mc::fmul_vec,
                Opcode::FDiv => mc::fdiv_vec,
                _ => return crate::lanes::binary(cx, inst, lhs, rhs),
            };
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(lhs)?;
            let b = cx.use_reg(rhs)?;
            put(cx, word(arr, vr(a), vr(a), vr(b)));
            cx.finish(inst, a)
        }
        Shape::IntVec(arr) => int_vector(cx, inst, arr, lhs, rhs),
        Shape::HalfVec | Shape::OtherVec => crate::lanes::binary(cx, inst, lhs, rhs),
    }
}

/// The scalar instruction of a floating-point binary opcode.
type Fp3 = fn(FpSize, VReg, VReg, VReg) -> u32;

/// `va = va <word> vb` in format `fp`: `half` through single precision.
fn scalar_binary(cx: &mut FnCx<'_, A64>, word: Fp3, fp: Fp, va: VReg, vb: VReg) {
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::S, va, FpFormat::H, va));
        put(cx, mc::fcvt(FpFormat::S, vb, FpFormat::H, vb));
    }
    put(cx, word(fp.size(), va, va, vb));
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::H, va, FpFormat::S, va));
    }
}

/// The scalar format of a vector element type of `half`, `float` or
/// `double` (what the lane operations below take); `Internal` otherwise.
fn lane_format(cx: &FnCx<'_, A64>, elem: TypeId) -> CgResult<Fp> {
    match shape(cx, elem) {
        Shape::Scalar(fp) => Ok(fp),
        _ => cx.bug("a floating-point lane of a type that is not half, float or double"),
    }
}

/// One lane of an element-wise `fadd`, `fsub`, `fmul` or `fdiv`
/// (`lanes.rs`, WP-080): `va = va <op> vb` for elements of `elem`
/// (`half`, `float`, `double`), as [`binary`] computes a scalar.
///
/// # Errors
/// [`BINARY`] for another opcode.
pub fn lane_binary(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    elem: TypeId,
    va: VReg,
    vb: VReg,
) -> CgResult<()> {
    let fp = lane_format(cx, elem)?;
    let Some(word) = scalar_op(op) else { return unsupported(BINARY) };
    scalar_binary(cx, word, fp, va, vb);
    Ok(())
}

/// The function `frem` of `fp` calls (`half` through `fmodf`).
fn fmod_of(fp: Fp) -> RuntimeFn {
    if fp == Fp::D { RuntimeFn::Fmod } else { RuntimeFn::Fmodf }
}

/// `fmod` of the two values in the pool temporaries `t1` and `t2` (a
/// `half` already widened to single precision), the result narrowed
/// back for `half`: the call and the result's register.
fn fmod_call(cx: &mut FnCx<'_, A64>, fp: Fp, t1: PReg, t2: PReg) -> CgResult<PReg> {
    let Some(r) = cx.call_runtime(fmod_of(fp), &[CallArg::Reg(t1), CallArg::Reg(t2)], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::H, vr(r), FpFormat::S, vr(r)));
    }
    Ok(r)
}

/// One lane of an element-wise `frem` (`lanes.rs`, WP-080): `fmod` of
/// the pool temporaries `a` and `b` (elements of `elem`), as [`binary`]
/// calls it for a scalar; the result's register (every temporary has
/// ended at the call).
///
/// # Errors
/// What the call reports.
pub fn lane_frem(cx: &mut FnCx<'_, A64>, elem: TypeId, a: PReg, b: PReg) -> CgResult<PReg> {
    let fp = lane_format(cx, elem)?;
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::S, vr(a), FpFormat::H, vr(a)));
        put(cx, mc::fcvt(FpFormat::S, vr(b), FpFormat::H, vr(b)));
    }
    fmod_call(cx, fp, a, b)
}

fn scalar_op(op: Opcode) -> Option<Fp3> {
    Some(match op {
        Opcode::FAdd => mc::fadd,
        Opcode::FSub => mc::fsub,
        Opcode::FMul => mc::fmul,
        Opcode::FDiv => mc::fdiv,
        _ => return None,
    })
}

/// A binary operation on a vector of integers.
fn int_vector<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arr: Arr,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let op = inst.opcode();
    if !matches!(
        op,
        Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Shl
            | Opcode::LShr
            | Opcode::AShr
    ) || (op == Opcode::Mul && arr == Arr::D2)
    {
        return crate::lanes::binary(cx, inst, lhs, rhs);
    }
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    let (va, vb) = (vr(a), vr(b));
    let bits = bits_of(arr);
    let word = match op {
        Opcode::Add => mc::add_vec(arr, va, va, vb),
        Opcode::Sub => mc::sub_vec(arr, va, va, vb),
        Opcode::Mul => mc::mul_vec(arr, va, va, vb),
        Opcode::And => mc::and_vec(bits, va, va, vb),
        Opcode::Or => mc::orr_vec(bits, va, va, vb),
        Opcode::Xor => mc::eor_vec(bits, va, va, vb),
        // An amount of the width or more is poison (LangRef): `ushl`
        // gives zero, which is as good as any value.
        Opcode::Shl => mc::ushl(arr, va, va, vb),
        Opcode::LShr | Opcode::AShr => {
            // A right shift is a left shift by the negated amount.
            put(cx, mc::neg_vec(arr, vb, vb));
            if op == Opcode::LShr { mc::ushl(arr, va, va, vb) } else { mc::sshl(arr, va, va, vb) }
        }
        _ => return cx.bug("an integer vector operation without a lowering"),
    };
    put(cx, word);
    cx.finish(inst, a)
}

/// `%r = <op> %lhs, %rhs` as a call of runtime function `func` (§7.3).
fn libcall<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    func: RuntimeFn,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let Some(reg) = cx.call_runtime(func, &[CallArg::Value(lhs), CallArg::Value(rhs)], None)?
    else {
        return cx.bug("a runtime function returned nothing");
    };
    cx.finish(inst, reg)
}

/// `%r = fneg %arg`: the sign bit flipped, nothing else, NaNs included.
///
/// # Errors
/// [`FNEG`] for a vector of integers (never valid IR) or of an element
/// type without a lowering.
pub fn fneg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let shape = shape(cx, inst.ty());
    if matches!(shape, Shape::IntVec(_) | Shape::OtherVec) {
        return unsupported(FNEG);
    }
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    negate(cx, shape, vr(a))?;
    cx.finish(inst, a)
}

/// One lane of an element-wise `fneg` (`lanes.rs`, WP-080): the sign bit
/// of the element of `elem` (`half`, `float`, `double`) in `va` flipped,
/// as [`fneg`] does it for a scalar. Writes `v31` for `half`.
///
/// # Errors
/// `Internal` for another element type.
pub fn lane_fneg(cx: &mut FnCx<'_, A64>, elem: TypeId, va: VReg) -> CgResult<()> {
    let fp = lane_format(cx, elem)?;
    negate(cx, Shape::Scalar(fp), va)
}

/// `fneg` of a value of `shape` in `va`, in place.
fn negate(cx: &mut FnCx<'_, A64>, shape: Shape, va: VReg) -> CgResult<()> {
    match shape {
        Shape::Scalar(Fp::S) => put(cx, mc::fneg(FpSize::S, va, va)),
        Shape::Scalar(Fp::D) => put(cx, mc::fneg(FpSize::D, va, va)),
        Shape::FpVec(arr) => put(cx, mc::fneg_vec(arr, va, va)),
        Shape::Scalar(Fp::H) | Shape::BFloat | Shape::HalfVec => {
            // Bit 15 of every 16-bit element.
            put(cx, mc::movi(Arr::H8, V31, 0x80, 8));
            put(cx, mc::eor_vec(VecBits::V128, va, va, V31));
        }
        Shape::Quad => {
            put(cx, mc::fmov_hi_to_gpr(X16.zr(), va));
            put(cx, mc::eor_imm(Size::X, X16.sp(), X16.zr(), 1 << 63));
            put(cx, mc::fmov_gpr_to_hi(va, X16.zr()));
        }
        Shape::IntVec(_) | Shape::OtherVec => return cx.bug("fneg of an integer vector"),
    }
    Ok(())
}

/// `%r = freeze %arg` of class `Fpr`: a copy (§4.7; `undef` and `poison`
/// are zero).
///
/// # Errors
/// What placing the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    cx.finish(inst, a)
}

/// `%r = <cast> %arg` with a side of class `Fpr` and none of class `Mem`.
///
/// # Errors
/// [`CAST`] for a vector cast without a one-instruction lowering, and for
/// a cast between `fp128` and a vector; [`BFLOAT`].
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    let (from_ty, to_ty) = (value_ty(cx, arg), inst.ty());
    let from_gpr = cx.class_of(arg) == Class::Gpr;
    let to_gpr = hod_cg::Target::class_of(&A64, cx.types(), to_ty) == Class::Gpr;
    if op == Opcode::BitCast {
        return bitcast(cx, inst, arg, from_gpr, to_gpr);
    }
    let from = if from_gpr { None } else { Some(shape(cx, from_ty)) };
    let to = if to_gpr { None } else { Some(shape(cx, to_ty)) };
    if from == Some(Shape::BFloat) || to == Some(Shape::BFloat) {
        return crate::tail2::bfloat_cast(cx, inst, arg);
    }
    if from == Some(Shape::Quad) || to == Some(Shape::Quad) {
        return quad_cast(cx, inst, arg, from, to);
    }
    match (from, to) {
        (Some(Shape::Scalar(a)), Some(Shape::Scalar(b))) => {
            if !matches!(op, Opcode::FPTrunc | Opcode::FPExt) || a == b {
                return unsupported(CAST);
            }
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            convert(cx, Conv::Fp(a, b), r, r);
            cx.finish(inst, r)
        }
        (Some(Shape::Scalar(fp)), None) => {
            let bits = int_width(cx, to_ty)?;
            let signed = match op {
                Opcode::FPToSI => true,
                Opcode::FPToUI => false,
                _ => return unsupported(CAST),
            };
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(arg)?;
            let rd = cx.def_reg(inst)?;
            convert(cx, Conv::ToInt { fp, bits, signed }, a, rd);
            cx.finish(inst, rd)
        }
        (None, Some(Shape::Scalar(fp))) => {
            let bits = int_width(cx, from_ty)?;
            let signed = match op {
                Opcode::SIToFP => true,
                Opcode::UIToFP => false,
                _ => return unsupported(CAST),
            };
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(arg)?;
            let rd = cx.def_reg(inst)?;
            convert(cx, Conv::FromInt { bits, signed, fp }, a, rd);
            cx.finish(inst, rd)
        }
        (Some(from), Some(to)) => vector_cast(cx, inst, arg, op, from, to),
        _ => unsupported(CAST),
    }
}

/// A conversion between scalars with a floating-point side.
#[derive(Clone, Copy)]
enum Conv {
    /// `fpext`/`fptrunc` between two formats.
    Fp(Fp, Fp),
    /// `fptosi` (`signed`) or `fptoui` to an integer of `bits` bits.
    ToInt { fp: Fp, bits: u32, signed: bool },
    /// `sitofp` (`signed`) or `uitofp` from an integer of `bits` bits.
    FromInt { bits: u32, signed: bool, fp: Fp },
}

/// The conversion `conv` of the scalar in `src` into `dst` (which may be
/// `src` for [`Conv::Fp`]). Writes `src`.
fn convert(cx: &mut FnCx<'_, A64>, conv: Conv, src: PReg, dst: PReg) {
    match conv {
        Conv::Fp(a, b) => put(cx, mc::fcvt(b.format(), vr(dst), a.format(), vr(src))),
        Conv::ToInt { fp, bits, signed } => {
            let va = vr(src);
            if fp == Fp::H {
                put(cx, mc::fcvt(FpFormat::S, va, FpFormat::H, va));
            }
            let word = if signed { mc::fcvtzs } else { mc::fcvtzu };
            put(cx, word(size_for_bits(bits), xr(dst).zr(), fp.size(), va));
        }
        Conv::FromInt { bits, signed, fp } => {
            let ra = xr(src).zr();
            // §2.2: the bits above the width are unspecified.
            let sz = size_for_bits(bits);
            if bits != 32 && bits != 64 {
                let extend = if signed { mc::sbfx } else { mc::ubfx };
                put(cx, extend(sz, ra, ra, 0, bits));
            }
            let vd = vr(dst);
            let word = if signed { mc::scvtf } else { mc::ucvtf };
            // `half` through single: an integer of magnitude 2^24 or more
            // is infinity in `half` either way, and below that the single
            // is exact, so this rounds once.
            put(cx, word(fp.size(), vd, sz, ra));
            if fp == Fp::H {
                put(cx, mc::fcvt(FpFormat::H, vd, FpFormat::S, vd));
            }
        }
    }
}

/// One lane of an element-wise conversion with a floating-point side
/// (`lanes.rs`, WP-080): `fpext`, `fptrunc`, `fptoui`, `fptosi`,
/// `uitofp`, `sitofp` of the element of `from` in `src` to one of `to` in
/// `dst` (a register of `to`'s class; `src` itself for `fpext` and
/// `fptrunc` is allowed), as [`cast`] converts a scalar. Writes `src`.
///
/// # Errors
/// [`CAST`] for another conversion or element type.
pub fn lane_convert(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    from: TypeId,
    to: TypeId,
    src: PReg,
    dst: PReg,
) -> CgResult<()> {
    let fp_of = |ty| match shape(cx, ty) {
        Shape::Scalar(fp) => Some(fp),
        _ => None,
    };
    let int_of = |ty| cx.types().int_bits(ty).filter(|b| (1..=64).contains(b));
    let conv = match (op, fp_of(from), fp_of(to)) {
        (Opcode::FPExt | Opcode::FPTrunc, Some(a), Some(b)) if a != b => Conv::Fp(a, b),
        (Opcode::FPToSI | Opcode::FPToUI, Some(fp), None) => {
            let Some(bits) = int_of(to) else { return unsupported(CAST) };
            Conv::ToInt { fp, bits, signed: op == Opcode::FPToSI }
        }
        (Opcode::SIToFP | Opcode::UIToFP, None, Some(fp)) => {
            let Some(bits) = int_of(from) else { return unsupported(CAST) };
            Conv::FromInt { bits, signed: op == Opcode::SIToFP, fp }
        }
        _ => return unsupported(CAST),
    };
    convert(cx, conv, src, dst);
    Ok(())
}

/// The width of an integer type of class `Gpr`.
fn int_width(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<u32> {
    match cx.types().int_bits(ty) {
        Some(bits) if (1..=64).contains(&bits) => Ok(bits),
        _ => unsupported(CAST),
    }
}

/// `bitcast` with a side of class `Fpr`: the bits unchanged. Between two
/// `Fpr`-class types of one size it is a copy; with a `Gpr` side, an
/// `fmov` of the `W` or `X` view (`half` and `bfloat` travel in the low
/// 16 bits of a `W` view: the bits above are unspecified on both sides).
fn bitcast<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    from_gpr: bool,
    to_gpr: bool,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    match (from_gpr, to_gpr) {
        (false, false) => cx.finish(inst, a),
        (true, false) => {
            let bits = cx.types().size_in_bits_of(inst.ty());
            let sz = if bits <= 32 { Size::W } else { Size::X };
            let rd = cx.def_reg(inst)?;
            put(cx, mc::fmov_gpr_to_fp(sz, vr(rd), xr(a).zr()));
            cx.finish(inst, rd)
        }
        (false, true) => {
            let bits = cx.types().size_in_bits_of(inst.ty());
            let sz = if bits <= 32 { Size::W } else { Size::X };
            let rd = cx.def_reg(inst)?;
            put(cx, mc::fmov_fp_to_gpr(sz, xr(rd).zr(), vr(a)));
            cx.finish(inst, rd)
        }
        (true, true) => cx.bug("a bitcast between two general registers in the float group"),
    }
}

/// A cast between two vectors of class `Fpr`: the forms that are one
/// instruction.
fn vector_cast<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    op: Opcode,
    from: Shape,
    to: Shape,
) -> CgResult<()> {
    type Two = fn(VReg, VReg) -> u32;
    let word: Two = match (op, from, to) {
        (Opcode::FPExt, Shape::FpVec(FpArr::S2), Shape::FpVec(FpArr::D2)) => {
            |rd, rn| mc::fcvtl(FpSize::D, rd, rn)
        }
        (Opcode::FPTrunc, Shape::FpVec(FpArr::D2), Shape::FpVec(FpArr::S2)) => {
            |rd, rn| mc::fcvtn(FpSize::D, rd, rn)
        }
        (Opcode::FPToSI, Shape::FpVec(f), Shape::IntVec(i)) if f.arr() == i => {
            return same_lanes(cx, inst, arg, f, mc::fcvtzs_vec);
        }
        (Opcode::FPToUI, Shape::FpVec(f), Shape::IntVec(i)) if f.arr() == i => {
            return same_lanes(cx, inst, arg, f, mc::fcvtzu_vec);
        }
        (Opcode::SIToFP, Shape::IntVec(i), Shape::FpVec(f)) if f.arr() == i => {
            return same_lanes(cx, inst, arg, f, mc::scvtf_vec);
        }
        (Opcode::UIToFP, Shape::IntVec(i), Shape::FpVec(f)) if f.arr() == i => {
            return same_lanes(cx, inst, arg, f, mc::ucvtf_vec);
        }
        (Opcode::Trunc, Shape::IntVec(Arr::H8), Shape::IntVec(Arr::B8)) => {
            |rd, rn| mc::xtn(mc::Elem::B, rd, rn)
        }
        (Opcode::Trunc, Shape::IntVec(Arr::S4), Shape::IntVec(Arr::H4)) => {
            |rd, rn| mc::xtn(mc::Elem::H, rd, rn)
        }
        (Opcode::Trunc, Shape::IntVec(Arr::D2), Shape::IntVec(Arr::S2)) => {
            |rd, rn| mc::xtn(mc::Elem::S, rd, rn)
        }
        (Opcode::ZExt, Shape::IntVec(Arr::B8), Shape::IntVec(Arr::H8)) => {
            |rd, rn| mc::uxtl(mc::Elem::B, rd, rn)
        }
        (Opcode::ZExt, Shape::IntVec(Arr::H4), Shape::IntVec(Arr::S4)) => {
            |rd, rn| mc::uxtl(mc::Elem::H, rd, rn)
        }
        (Opcode::ZExt, Shape::IntVec(Arr::S2), Shape::IntVec(Arr::D2)) => {
            |rd, rn| mc::uxtl(mc::Elem::S, rd, rn)
        }
        (Opcode::SExt, Shape::IntVec(Arr::B8), Shape::IntVec(Arr::H8)) => {
            |rd, rn| mc::sxtl(mc::Elem::B, rd, rn)
        }
        (Opcode::SExt, Shape::IntVec(Arr::H4), Shape::IntVec(Arr::S4)) => {
            |rd, rn| mc::sxtl(mc::Elem::H, rd, rn)
        }
        (Opcode::SExt, Shape::IntVec(Arr::S2), Shape::IntVec(Arr::D2)) => {
            |rd, rn| mc::sxtl(mc::Elem::S, rd, rn)
        }
        // Vectors of pointers and of 64-bit integers: the same bits.
        (Opcode::PtrToInt | Opcode::IntToPtr, Shape::IntVec(Arr::D2), Shape::IntVec(Arr::D2))
            if cx.types().size_in_bits_of(inst.ty())
                == cx.types().size_in_bits_of(value_ty(cx, arg)) =>
        {
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(arg)?;
            return cx.finish(inst, a);
        }
        _ => return crate::lanes::cast(cx, inst, arg),
    };
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    put(cx, word(vr(a), vr(a)));
    cx.finish(inst, a)
}

/// A conversion between vectors of one arrangement.
fn same_lanes<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    arr: FpArr,
    word: fn(FpArr, VReg, VReg) -> u32,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    put(cx, word(arr, vr(a), vr(a)));
    cx.finish(inst, a)
}

/// `%r = fcmp <pred> %lhs, %rhs`: deferred when the next instruction
/// consumes it (§4.6), else `cset` (and `csinc` for a predicate of two
/// conditions) after [`flags`].
///
/// # Errors
/// What [`flags`] reports.
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (pred, rhs);
    compare_format(cx, lhs)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let cond = flags(cx, inst)?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd).zr();
    put(cx, mc::cset(Size::W, r, cond.first));
    if let Some(second) = cond.second {
        // `r = second ? 1 : r`.
        put(cx, mc::csinc(Size::W, r, r, RegZr::ZR, second.invert()));
    }
    cx.finish(inst, rd)
}

/// `%r = icmp <pred> %lhs, %rhs` of vectors in vector registers.
///
/// # Errors
/// Always [`VECTOR_ICMP`]: the `<N x i1>` result is of class `Mem`, whose
/// in-memory form is a bit per element; WP-040's representation of it is
/// not settled.
pub fn vector_icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(VECTOR_ICMP)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Fpr`. A deferred
/// `fcmp` condition is fused ([`flags`]); otherwise bit 0 of the
/// condition is tested (§2.2). Up to 64 bits by `fcsel` of the `D` view,
/// 128 bits by a mask and `bsl`.
///
/// # Errors
/// [`VECTOR_SELECT`] for a vector condition; what [`flags`] reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    if cx.class_of(cond) != Class::Gpr {
        return unsupported(VECTOR_SELECT);
    }
    let fused = cx.deferred(cond).filter(|d| matches!(d.kind(), InstKind::FCmp { .. }));
    deferred_first(cx, inst, fused.map(|_| cond))?;
    let c = if let Some(compare) = fused {
        flags(cx, compare)?
    } else {
        let r = cx.use_reg(cond)?;
        put(cx, mc::tst_imm(Size::W, xr(r).zr(), 1));
        give_back(cx, cond, r);
        FpCond { first: Cond::Ne, second: None }
    };
    let a = cx.use_reg(if_true)?;
    let b = cx.use_reg(if_false)?;
    let (va, vb) = (vr(a), vr(b));
    let bits = cx.types().size_in_bits_of(inst.ty());
    if shape(cx, inst.ty()).is_wide(bits) {
        put(cx, mc::csetm(Size::X, X16.zr(), c.first));
        if let Some(second) = c.second {
            // `x16 = second ? ~0 : x16`.
            put(cx, mc::csinv(Size::X, X16.zr(), X16.zr(), RegZr::ZR, second.invert()));
        }
        put(cx, mc::dup_gpr(Arr::D2, V31, X16.zr()));
        put(cx, mc::bsl(VecBits::V128, V31, va, vb));
        put(cx, mc::mov_vec(VecBits::V128, va, V31));
    } else {
        if let Some(second) = c.second {
            put(cx, mc::fcsel(FpSize::D, vb, va, vb, second));
        }
        put(cx, mc::fcsel(FpSize::D, va, va, vb, c.first));
    }
    cx.finish(inst, a)
}

// ---- loads and stores (WP-067) ----

/// The one access of an `Fpr`-class value in memory: its store size,
/// which is 2, 4, 8 or 16 bytes (`target::vector_class` admits vectors
/// of 64 and 128 bits only; a vector of elements narrower than a byte is
/// bit-packed in the register as in memory).
fn access(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<VMemSize> {
    Ok(match cx.types().size_of(ty) {
        2 => VMemSize::H,
        4 => VMemSize::S,
        8 => VMemSize::D,
        16 => VMemSize::Q,
        _ => return cx.bug("an Fpr-class value whose store size is no register view"),
    })
}

/// The address operand `ptr` if it is a deferred `ptradd` that the
/// addressing fuses (as `mem.rs` decides).
fn fused_ptr(cx: &FnCx<'_, A64>, ptr: Value) -> Option<Value> {
    cx.deferred(ptr).filter(|d| matches!(d.kind(), InstKind::PtrAdd(_))).map(|_| ptr)
}

/// `%r = load <ty>, ptr %ptr` of class `Fpr` (the hook in `mem.rs`, which
/// has refused an atomic load): one `ldr`/`ldur` of exactly the store
/// size at the address `address` computes. `address` is `mem.rs`'s: it
/// fuses a deferred `ptradd` and asks for the address operands, so it
/// runs after the result's register is taken (a vector register, which
/// no address computation writes).
///
/// # Errors
/// What asking for the operands reports.
pub fn load<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ptr: Value,
    address: impl FnOnce(&mut FnCx<'a, A64>) -> CgResult<Cursor>,
) -> CgResult<()> {
    deferred_first(cx, inst, fused_ptr(cx, ptr))?;
    let sz = access(cx, inst.ty())?;
    let rd = cx.def_reg(inst)?;
    let mut at = address(cx)?;
    load_fpr_at(cx, sz, vr(rd), &mut at);
    cx.finish(inst, rd)
}

/// `store <ty> %value, ptr %ptr` of class `Fpr` (the hook in `mem.rs`,
/// which has refused an atomic store): one `str`/`stur` of exactly the
/// store size.
///
/// # Errors
/// What asking for the operands reports.
pub fn store<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    address: impl FnOnce(&mut FnCx<'a, A64>) -> CgResult<Cursor>,
) -> CgResult<()> {
    deferred_first(cx, inst, fused_ptr(cx, ptr))?;
    let sz = access(cx, value_ty(cx, value))?;
    let src = cx.use_reg(value)?;
    let mut at = address(cx)?;
    store_fpr_at(cx, sz, vr(src), &mut at);
    Ok(())
}
