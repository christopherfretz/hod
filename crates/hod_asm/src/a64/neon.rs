//! Advanced SIMD instructions on vector operands (`spec/asm.md` §20):
//! the floating-point three same and two-register miscellaneous classes,
//! the compares, the multiply-adds by element, the three different
//! (long and high-narrowing) class with its `2` forms, and the structure
//! loads and stores.
//!
//! Each form is one `hod_mc_a64` encoder call where the crate has one
//! (`neon_fp3`, `neon_fp2`, `neon_fp2_scalar`, `neon_long3`,
//! `ldst_multi`, `ldst_lane`, `ld_rep`); the words code generation never
//! writes (the compares and multiply-adds of the three same class, the
//! by-element class, the post-index structure forms) are built here from
//! the class's word, as `prefetch.rs` builds its own. `tests/llvm_mc.rs`
//! holds every form to `llvm-mc` 22's and 19's words.
//!
//! Every form needs `neon`; a `4h`/`8h` floating-point form needs
//! `fullfp16`; `pmull`/`pmull2` of `1d`/`2d` need `aes` (as `llc` 19
//! and 22 gate them by the function's `target-features`).

use hod_mc_a64 as mc;
use hod_mc_a64::neon::{Fp2, Fp3, Long3, Multi};
use hod_mc_a64::{Elem, FpFormat, RegSp, VReg, VecBits};

use super::Features;
use super::encode::Encoded;
use super::operand::{Gp, Mem, Offset, Op, VList, View};
use crate::reason::{Refusal, operand, other};

type R<T> = Result<T, Refusal>;

fn fail<T>(what: &'static str) -> R<T> {
    Err(Refusal(what))
}

fn word(w: u32) -> Encoded<'static> {
    Encoded { word: w, fixup: None }
}

fn checked(w: Option<u32>) -> R<Encoded<'static>> {
    w.map(word).ok_or(Refusal(operand::RANGE))
}

/// A vector register with an arrangement: its number and shape.
fn vreg(op: &Op<'_>) -> R<(VReg, u8, View)> {
    match op {
        Op::VReg(n, count, view) => Ok((reg(*n)?, *count, *view)),
        o if o.is_vector() => fail(operand::VECTOR),
        Op::Gp(_) | Op::Fp(..) => fail(operand::REGISTER),
        _ => fail(operand::FORM),
    }
}

fn reg(n: u8) -> R<VReg> {
    VReg::new(n).ok_or(Refusal(operand::REGISTER))
}

/// The integer shape of an arrangement (`8b`..`2d`).
fn shape(count: u8, view: View) -> Option<(VecBits, Elem)> {
    Some(match (count, view) {
        (8, View::B) => (VecBits::V64, Elem::B),
        (16, View::B) => (VecBits::V128, Elem::B),
        (4, View::H) => (VecBits::V64, Elem::H),
        (8, View::H) => (VecBits::V128, Elem::H),
        (2, View::S) => (VecBits::V64, Elem::S),
        (4, View::S) => (VecBits::V128, Elem::S),
        (1, View::D) => (VecBits::V64, Elem::D),
        (2, View::D) => (VecBits::V128, Elem::D),
        _ => return None,
    })
}

/// The floating-point shape of an arrangement (`4h`, `8h`, `2s`, `4s`,
/// `2d`); a half-precision one needs `fullfp16`.
fn fp_shape(count: u8, view: View, features: Features) -> R<(VecBits, FpFormat)> {
    let s = match (count, view) {
        (4, View::H) => (VecBits::V64, FpFormat::H),
        (8, View::H) => (VecBits::V128, FpFormat::H),
        (2, View::S) => (VecBits::V64, FpFormat::S),
        (4, View::S) => (VecBits::V128, FpFormat::S),
        (2, View::D) => (VecBits::V128, FpFormat::D),
        _ => return fail(operand::VECTOR),
    };
    if s.1 == FpFormat::H && !features.fp16 {
        return fail(other::FEATURE);
    }
    Ok(s)
}

/// Floating-point vector registers of one shape, all the same.
fn fp_regs<const N: usize>(
    ops: &[Op<'_>],
    features: Features,
) -> R<(VecBits, FpFormat, [VReg; N])> {
    if ops.len() != N {
        return fail(operand::FORM);
    }
    let (_, count, view) = vreg(&ops[0])?;
    let (bits, fmt) = fp_shape(count, view, features)?;
    let mut regs = [VReg::V0; N];
    for (r, op) in regs.iter_mut().zip(ops) {
        let (v, c, w) = vreg(op)?;
        if (c, w) != (count, view) {
            return fail(operand::VECTOR);
        }
        *r = v;
    }
    Ok((bits, fmt, regs))
}

/// Whether `op` is `#0.0` or `#0`, the zero of the compares.
fn is_zero(op: &Op<'_>) -> Option<bool> {
    match op {
        Op::FpImm(f) => Some(*f == 0.0 && f.is_sign_positive()),
        Op::Imm(v, true) => Some(*v == 0),
        _ => None,
    }
}

fn rrr(w: u32, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    w | u32::from(rm.index()) << 16 | u32::from(rn.index()) << 5 | u32::from(rd.index())
}

/// A three same or two-register word of the single-precision class for
/// `bits` of `fmt`: bit 22 for double, the half-precision class (as
/// `hod_mc_a64` turns its own words) for half.
fn fp_class(single: u32, half: u32, bits: VecBits, fmt: FpFormat) -> u32 {
    let w = match fmt {
        FpFormat::H => half,
        FpFormat::S => single,
        FpFormat::D => single | 1 << 22,
    };
    w | (bits as u32) << 30
}

/// The half-precision three same word from the single-precision one
/// (`hod_mc_a64::neon::Fp3`'s rule).
const fn half3(w: u32) -> u32 {
    (w & !0x0020_c000) | 0x0040_0000
}

/// The half-precision two-register miscellaneous word (`Fp2`'s rule).
const fn half2(w: u32) -> u32 {
    w | 0x0078_0000
}

// The three same words `hod_mc_a64::neon::Fp3` does not have (single
// precision, `Q` 0).
const FMLA: u32 = 0x0e20_cc00;
const FMLS: u32 = 0x0ea0_cc00;
const FCMEQ: u32 = 0x0e20_e400;
const FCMGE: u32 = 0x2e20_e400;
const FCMGT: u32 = 0x2ea0_e400;

// The compares with zero (two-register miscellaneous).
const FCMGT_ZERO: u32 = 0x0ea0_c800;
const FCMEQ_ZERO: u32 = 0x0ea0_d800;
const FCMLT_ZERO: u32 = 0x0ea0_e800;
const FCMGE_ZERO: u32 = 0x2ea0_c800;
const FCMLE_ZERO: u32 = 0x2ea0_d800;

// The vector × indexed element class, single/double precision (`sz` at
// bit 22); the half-precision words clear bit 23.
const FMLA_ELEM: u32 = 0x0f80_1000;
const FMLS_ELEM: u32 = 0x0f80_5000;
const FMUL_ELEM: u32 = 0x0f80_9000;
const FMULX_ELEM: u32 = 0x2f80_9000;

fn fp3_op(m: &str) -> Option<Fp3> {
    Fp3::ALL.iter().copied().find(|op| op.mnemonic() == m)
}

fn fp2_op(m: &str) -> Option<Fp2> {
    match m {
        // `FEAT_FRINTTS`, which hod does not read, and `frecpx`, which has
        // no vector form.
        "frint32z" | "frint32x" | "frint64z" | "frint64x" | "frecpx" => None,
        _ => Fp2::ALL.iter().copied().find(|op| op.mnemonic() == m),
    }
}

/// The mnemonics of this module, and whether each also has a scalar
/// form `encode::float` takes when no operand is a vector.
pub fn known(m: &str) -> bool {
    fp3_op(m).is_some()
        || fp2_op(m).is_some()
        || long_op(m).is_some()
        || structure(m).is_some()
        || matches!(m, "fmla" | "fmls" | "fcmeq" | "fcmge" | "fcmgt" | "fcmle" | "fcmlt" | "fmulx")
}

/// A three different mnemonic: the operation and whether it is the `2`
/// form (the upper halves).
fn long_op(m: &str) -> Option<(Long3, bool)> {
    let (base, upper) = match m.strip_suffix('2') {
        Some(b) => (b, true),
        None => (m, false),
    };
    Long3::ALL.iter().copied().find(|op| op.mnemonic() == base).map(|op| (op, upper))
}

/// A structure load or store: load, the structure size `n`, and the
/// replicating form (`ld<n>r`).
fn structure(m: &str) -> Option<(bool, u32, bool)> {
    let b = m.as_bytes();
    let load = match b.get(..2)? {
        b"ld" => true,
        b"st" => false,
        _ => return None,
    };
    let n = match b.get(2)? {
        c @ b'1'..=b'4' => u32::from(c - b'0'),
        _ => return None,
    };
    match &b[3..] {
        b"" => Some((load, n, false)),
        b"r" if load => Some((load, n, true)),
        _ => None,
    }
}

/// Encodes `m` (lower case) when it is a mnemonic of this module and an
/// operand is a vector: `Ok(None)` otherwise.
///
/// # Errors
/// [`other::FEATURE`] without `neon` (or `fullfp16`, `aes` as the form
/// needs); `asm:operand/vector` for an arrangement the form does not
/// take; `asm:operand/register`, `asm:operand/form`,
/// `asm:operand/range`.
pub fn encode<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Option<Encoded<'a>>> {
    if !known(m) || !ops.iter().any(Op::is_vector) {
        return Ok(None);
    }
    if !features.neon {
        return fail(other::FEATURE);
    }
    if let Some((load, n, rep)) = structure(m) {
        return structure_insn(load, n, rep, ops).map(Some);
    }
    if let Some((op, upper)) = long_op(m) {
        return long(op, upper, ops, features).map(Some);
    }
    if matches!(ops.get(2), Some(Op::VLane(..))) {
        return by_element(m, ops, features).map(Some);
    }
    let e = match m {
        "fmla" | "fmls" | "fcmeq" | "fcmge" | "fcmgt" | "fcmle" | "fcmlt" => {
            let [dst, src, other] = ops else { return fail(operand::FORM) };
            if let Some(zero) = is_zero(other) {
                if !zero {
                    return fail(operand::FP_IMMEDIATE);
                }
                let (bits, fmt, [rd, rn]) = fp_regs::<2>(&[*dst, *src], features)?;
                let base = match m {
                    "fcmeq" => FCMEQ_ZERO,
                    "fcmge" => FCMGE_ZERO,
                    "fcmgt" => FCMGT_ZERO,
                    "fcmle" => FCMLE_ZERO,
                    "fcmlt" => FCMLT_ZERO,
                    _ => return fail(operand::FORM),
                };
                let class = fp_class(base, half2(base), bits, fmt);
                word(class | u32::from(rn.index()) << 5 | u32::from(rd.index()))
            } else {
                let (bits, fmt, [rd, rn, rm]) = fp_regs::<3>(ops, features)?;
                // `fcmle`/`fcmlt` of two registers are `fcmge`/`fcmgt`
                // with the sources swapped, as `llvm-mc` writes them.
                let (base, rn, rm) = match m {
                    "fmla" => (FMLA, rn, rm),
                    "fmls" => (FMLS, rn, rm),
                    "fcmeq" => (FCMEQ, rn, rm),
                    "fcmge" => (FCMGE, rn, rm),
                    "fcmgt" => (FCMGT, rn, rm),
                    "fcmle" => (FCMGE, rm, rn),
                    _ => (FCMGT, rm, rn),
                };
                word(rrr(fp_class(base, half3(base), bits, fmt), rd, rn, rm))
            }
        }
        _ => {
            if let Some(op) = fp3_op(m) {
                let (bits, fmt, [rd, rn, rm]) = fp_regs::<3>(ops, features)?;
                checked(mc::neon::try_neon_fp3(op, bits, fmt, rd, rn, rm))?
            } else if let Some(op) = fp2_op(m) {
                let (bits, fmt, [rd, rn]) = fp_regs::<2>(ops, features)?;
                checked(mc::neon::try_neon_fp2(op, bits, fmt, rd, rn))?
            } else {
                return fail(operand::FORM);
            }
        }
    };
    Ok(Some(e))
}

/// The scalar Advanced SIMD form of a floating-point two-register
/// instruction (`fcvtns s0, s1`, `scvtf d0, d1`): `Ok(None)` when `m` has
/// none or the operands are not two FP registers of one view.
///
/// # Errors
/// [`other::FEATURE`] without `neon`, or `fullfp16` for `h` registers.
pub fn fp2_scalar<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Option<Encoded<'a>>> {
    let Some(op) = fp2_op(m).filter(|op| op.scalar_format(FpFormat::S)) else { return Ok(None) };
    let (d, vd, n, vn) = match ops {
        [Op::Fp(d, vd), Op::Fp(n, vn)] => (d, vd, n, vn),
        // The fixed-point forms (`fcvtzs d0, d1, #3`).
        [Op::Fp(..), Op::Fp(..), _, ..] => return fail(operand::FORM),
        _ => return Ok(None),
    };
    if vd != vn {
        return fail(operand::REGISTER);
    }
    let fmt = match vd {
        View::H => FpFormat::H,
        View::S => FpFormat::S,
        View::D => FpFormat::D,
        _ => return fail(operand::REGISTER),
    };
    if !features.neon || (fmt == FpFormat::H && !features.fp16) {
        return fail(other::FEATURE);
    }
    checked(mc::neon::try_neon_fp2_scalar(op, fmt, reg(*d)?, reg(*n)?)).map(Some)
}

/// `<op> <Vd>.<T>, <Vn>.<T>, <Vm>.<Ts>[<index>]`: the vector forms of
/// `fmla`, `fmls`, `fmul`, `fmulx` by element. Single precision takes
/// `v0`..`v31` and the index 0..3 (`H:L`), double `v0`..`v31` and 0..1
/// (`H`), half `v0`..`v15` and 0..7 (`H:L:M`).
fn by_element<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Encoded<'a>> {
    let base = match m {
        "fmla" => FMLA_ELEM,
        "fmls" => FMLS_ELEM,
        "fmul" => FMUL_ELEM,
        "fmulx" => FMULX_ELEM,
        _ => return fail(operand::VECTOR),
    };
    let [d, n, Op::VLane(rm, lane, index)] = ops else { return fail(operand::FORM) };
    let (bits, fmt, [rd, rn]) = fp_regs::<2>(&[*d, *n], features)?;
    let (rm, index) = (u32::from(*rm), u32::from(*index));
    let fields = match (fmt, lane) {
        (FpFormat::S, View::S) if index < 4 => (index & 1) << 21 | (index >> 1) << 11 | rm << 16,
        (FpFormat::D, View::D) if index < 2 => 1 << 22 | index << 11 | rm << 16,
        (FpFormat::H, View::H) if index < 8 && rm < 16 => {
            (index & 1) << 20 | (index >> 1 & 1) << 21 | (index >> 2) << 11 | rm << 16
        }
        (FpFormat::H, View::H) if index < 8 => return fail(operand::REGISTER),
        (_, v) if *v == lane_view(fmt) => return fail(operand::RANGE),
        _ => return fail(operand::VECTOR),
    };
    let base = if fmt == FpFormat::H { base & !(1 << 23) } else { base };
    Ok(word(
        base | (bits as u32) << 30 | fields | u32::from(rn.index()) << 5 | u32::from(rd.index()),
    ))
}

fn lane_view(fmt: FpFormat) -> View {
    match fmt {
        FpFormat::H => View::H,
        FpFormat::S => View::S,
        FpFormat::D => View::D,
    }
}

/// The element twice as wide, as the view of the wide arrangement
/// (`D` widens to the `1q` of `pmull`).
fn wide_of(e: Elem) -> (u8, View) {
    match e {
        Elem::B => (8, View::H),
        Elem::H => (4, View::S),
        Elem::S => (2, View::D),
        Elem::D => (1, View::Q),
    }
}

/// The three different class (`umlal v0.2d, v1.2s, v2.2s`, `umlal2
/// v0.2d, v1.4s, v2.4s`, `addhn v0.8b, v1.8h, v2.8h`, `pmull v0.1q,
/// v1.1d, v2.1d`): the narrow element is the sources' (the result's for
/// the high-narrowing cases), 64 bits of it, or 128 for the `2` form.
fn long<'a>(op: Long3, upper: bool, ops: &[Op<'a>], features: Features) -> R<Encoded<'a>> {
    let [dst, src1, src2] = ops else { return fail(operand::FORM) };
    let ((rd, dc, dv), (rn, nc, nv), (rm, mc_, mv)) = (vreg(dst)?, vreg(src1)?, vreg(src2)?);
    let narrow = if op.narrows() { (dc, dv) } else { (nc, nv) };
    let Some((bits, elem)) = shape(narrow.0, narrow.1) else { return fail(operand::VECTOR) };
    if (bits == VecBits::V128) != upper || !op.takes(elem) {
        return fail(operand::VECTOR);
    }
    let wide = wide_of(elem);
    let fits = if op.narrows() {
        (nc, nv) == wide && (mc_, mv) == wide
    } else {
        (dc, dv) == wide && (mc_, mv) == (nc, nv)
    };
    if !fits {
        return fail(operand::VECTOR);
    }
    if op == Long3::Pmull && elem == Elem::D && !features.aes {
        return fail(other::FEATURE);
    }
    let lower = checked(mc::neon::try_neon_long3(op, elem, rd, rn, rm))?.word;
    Ok(word(lower | u32::from(upper) << 30))
}

/// The address of a structure load or store: `[xn|sp]`, then nothing,
/// `#imm` (equal to the bytes moved) or an X register (post-index).
fn structure_address(ops: &[Op<'_>], bytes: u32) -> R<(RegSp, Option<u32>)> {
    let (mem, post) = match ops {
        [Op::Mem(mem)] => (mem, None),
        [Op::Mem(mem), post] => (mem, Some(post)),
        _ => return fail(operand::FORM),
    };
    let Mem { base, offset: Offset::None, writeback: false } = *mem else {
        return fail(operand::FORM);
    };
    let rn = match base {
        Gp::X(n) => RegSp::new(n),
        Gp::Sp => Some(RegSp::SP),
        _ => None,
    }
    .ok_or(Refusal(operand::REGISTER))?;
    let rm = match post {
        None => None,
        Some(Op::Imm(v, _)) if *v == i64::from(bytes) => Some(31),
        Some(Op::Imm(..)) => return fail(operand::RANGE),
        Some(Op::Gp(Gp::X(m))) => Some(u32::from(*m)),
        Some(Op::Gp(_)) => return fail(operand::REGISTER),
        Some(_) => return fail(operand::FORM),
    };
    Ok((rn, rm))
}

/// `ld1`..`ld4`, `st1`..`st4` (multiple structures, or one element of
/// each register), `ld1r`..`ld4r`; with `[xn]` or post-index.
fn structure_insn<'a>(load: bool, n: u32, rep: bool, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (list, rest) = match ops.split_first() {
        Some((Op::VList(list), rest)) => (*list, rest),
        Some((o, _)) if o.is_vector() => return fail(operand::VECTOR),
        _ => return fail(operand::FORM),
    };
    let VList { first, len, count, view, index } = list;
    let rt = reg(first)?;
    let len = u32::from(len);
    let w = if let Some(index) = index {
        if rep || len != n {
            return fail(operand::VECTOR);
        }
        let elem = match view {
            View::B => Elem::B,
            View::H => Elem::H,
            View::S => Elem::S,
            View::D => Elem::D,
            View::Q => return fail(operand::VECTOR),
        };
        let (rn, rm) = structure_address(rest, n * elem.bits() / 8)?;
        let w = checked(mc::neon::try_ldst_lane(load, n, elem, u32::from(index), rt, rn))?.word;
        post(w, rm)
    } else {
        let Some((bits, elem)) = shape(count, view) else { return fail(operand::VECTOR) };
        if rep {
            if len != n {
                return fail(operand::VECTOR);
            }
            let (rn, rm) = structure_address(rest, n * elem.bits() / 8)?;
            post(checked(mc::neon::try_ld_rep(n, bits, elem, rt, rn))?.word, rm)
        } else {
            let op = match (load, n, len) {
                (true, 1, 1) => Multi::Ld1,
                (true, 1, 2) => Multi::Ld1x2,
                (true, 1, 3) => Multi::Ld1x3,
                (true, 1, 4) => Multi::Ld1x4,
                (true, 2, 2) => Multi::Ld2,
                (true, 3, 3) => Multi::Ld3,
                (true, 4, 4) => Multi::Ld4,
                (false, 1, 1) => Multi::St1,
                (false, 1, 2) => Multi::St1x2,
                (false, 1, 3) => Multi::St1x3,
                (false, 1, 4) => Multi::St1x4,
                (false, 2, 2) => Multi::St2,
                (false, 3, 3) => Multi::St3,
                (false, 4, 4) => Multi::St4,
                _ => return fail(operand::VECTOR),
            };
            if op.interleaves() && elem == Elem::D && bits == VecBits::V64 {
                return fail(operand::VECTOR);
            }
            let (rn, rm) = structure_address(rest, len * bits.bits() / 8)?;
            post(checked(mc::neon::try_ldst_multi(op, bits, elem, rt, rn))?.word, rm)
        }
    };
    Ok(word(w))
}

/// The post-index form of a structure word: bit 23, and `Rm` (31 for an
/// immediate).
fn post(w: u32, rm: Option<u32>) -> u32 {
    match rm {
        None => w,
        Some(rm) => w | 1 << 23 | rm << 16,
    }
}
