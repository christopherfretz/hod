//! The AArch64 target intrinsics (`spec/codegen.md` §5.5.1 "NEON
//! intrinsics (WP-083)"): `llvm.aarch64.neon.*`, `llvm.aarch64.crypto.*`,
//! `llvm.aarch64.sisd.*`, the scalar `llvm.aarch64.cls`/`cls64`, and the
//! barriers and hints (`dmb`, `dsb`, `isb`, `hint`).
//!
//! **Owner: WP-083.** Tests: `tests/neon.rs`. Entry: [`intrinsic`], one
//! call from `intrinsic.rs`.
//!
//! Each intrinsic maps to a [`Low`], a lowering family; the family reads
//! the call's types and picks the encoder family of `hod_mc_a64::neon`
//! (vector, scalar, or the general-register forms). Like `crc32`
//! (WP-084), an intrinsic is lowered whatever the function's target
//! features say: features are availability, and hod does not choose
//! instructions by them (§5.5.2).
//!
//! # Operands
//!
//! The first two `Fpr` operands are [`FnCx::use_reg`]'d (a register
//! value dies at its use, so the lowering overwrites it, and an
//! accumulating instruction's destination is operand 0's register); the
//! others are put into `v30`, `v31` and then temporaries. A `Gpr` scalar
//! operand (`i32`, `i64`) is moved into the next of those with `fmov`. A
//! `Gpr` result is moved out with `fmov` into the first general operand's
//! register or a new one. Instructions that take consecutive registers
//! (`tbl`/`tbx` tables, the structure loads and stores) use the block
//! `v24`..`v27`, taken before any operand is read.
//!
//! # Refusals
//!
//! - [`TARGET`] (`a64:intrinsic-target`): a known intrinsic with a
//!   signature this module does not lower.
//! - [`IMMEDIATE`] (`a64:intrinsic-immediate`): a shift amount, lane or
//!   barrier option that is not a constant in range.
//! - The intrinsics whose matrix row is not supported never get here
//!   (`intrinsic:<name>`).

use hod_cg::{CgResult, Class, FnCx, PReg};
use hod_ir::{Const, Inst, Intrinsic, TypeId, TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{
    Aes, Arr, Barrier, Dot, Elem, Fp2, Fp3, FpAcross, FpFormat, FpPair, FpSize, Int2, Int3, Long3,
    Multi, Sha2Reg, Sha3Reg, ShiftImm, Size, VReg, VecBits,
};

use crate::common::{
    Cursor, V30, V31, X16, deferred_first, put, store_fpr_at, unsupported, vr, xr,
};
use crate::intrinsic::{INTRINSIC, const_int};
use crate::target::A64;

/// A known target intrinsic with a signature hod does not lower.
pub const TARGET: &str = "a64:intrinsic-target";
/// An immediate operand (shift amount, lane, barrier option) that is not
/// a constant in range.
pub const IMMEDIATE: &str = "a64:intrinsic-immediate";

/// The first register of the block for consecutive registers.
const BLOCK: u8 = 24;

/// A lowering family.
#[derive(Clone, Copy, Debug)]
enum Low {
    /// Integer three same, vector or scalar; `SQRDMLAH`/`SQRDMLSH`
    /// accumulate into operand 0.
    Int3(Int3),
    /// `sq{,r}dmulh.lane{,q}`: element `lane` of operand 1 duplicated,
    /// then the three same.
    Int3Lane(Int3),
    /// Floating-point three same, vector or scalar.
    Fp3(Fp3),
    /// `facge`, `facgt`: an integer result of floating-point operands.
    FpCompare(Fp3),
    /// Integer two-register miscellaneous of the same shape (and
    /// `suqadd`/`usqadd`, which accumulate).
    Int2(Int2),
    /// `{s,u}addlp`.
    PairLong(bool),
    /// `sqxtn`, `uqxtn`, `sqxtun` and their `scalar.` forms.
    Narrow(Int2),
    /// Floating-point two-register miscellaneous of the same shape.
    Fp2(Fp2),
    /// `fcvt{n,m,a,p,z}{s,u}`: integers from floating point.
    FpToInt(Fp2),
    /// `vcvtfp2fx{s,u}`, `vcvtfx{s,u}2fp`: fixed-point conversions.
    Fixed(ShiftImm),
    /// `vsli`, `vsri`: shift and insert into operand 0.
    Insert(ShiftImm),
    /// The narrowing right shifts by an immediate.
    ShiftNarrow(ShiftImm),
    /// `sqshlu`: a shift by a constant splat.
    ShiftSplat(ShiftImm),
    /// `{s,u}addlv`.
    AddLong(bool),
    /// `{s,u}addv`, `{s,u}{max,min}v`: an integer reduction.
    Reduce(Reduce),
    /// `faddv`, `f{max,min}{,nm}v`.
    FpReduce(Option<FpAcross>),
    /// Long and high-narrowing three different.
    Long3(Long3),
    /// `pmull64`.
    Pmull64,
    /// `sqdmulls.scalar`.
    SqdmullScalar,
    /// The products accumulated into operand 0.
    Dot(Dot),
    /// `vcmla.rot<n>`.
    Cmla(u32),
    /// `vcadd.rot{90,270}`.
    Cadd(bool),
    /// `aese`, `aesd` (two operands) and `aesmc`, `aesimc` (one).
    Aes(Aes),
    /// The SHA three-register forms, tied to operand 0.
    Sha3(Sha3Reg),
    /// The SHA two-register forms.
    Sha2(Sha2Reg),
    /// `eor3{s,u}` (`true`), `bcax{s,u}` (`false`).
    Ternary(bool),
    /// `xar`.
    Xar,
    /// `sm3partw1`, `sm3partw2` (tied to operand 0) and `sm4ekey`.
    Sm3(mc::Sm3Reg),
    /// `sm3ss1`.
    Sm3ss1,
    /// `sm3tt{1,2}{a,b}`: tied to operand 0, the lane of operand 2 last.
    Sm3tt(mc::Sm3tt),
    /// `sm4e`, tied to operand 0.
    Sm4e,
    /// `fjcvtzs` into a general register.
    Jcvt,
    /// `fcvtxn` (vector) and `sisd.fcvtxn`.
    Fcvtxn,
    /// `vcvtfp2hf` (`true`) and `vcvthf2fp`.
    HalfConvert(bool),
    /// `tbl<n>` (`false`) and `tbx<n>`.
    Table(bool, u32),
    /// Multiple-structure loads and stores.
    Multi(Multi),
    /// `ld<n>lane` (`true`), `st<n>lane`.
    Lane(bool, u32),
    /// `ld<n>r`.
    Replicate(u32),
    /// `llvm.aarch64.cls` (`false`), `cls64`.
    Cls(bool),
    /// `dmb`, `dsb`, `isb`, `hint`.
    System(Sys),
}

#[derive(Clone, Copy, Debug)]
enum Reduce {
    Add,
    Smax,
    Smin,
    Umax,
    Umin,
}

#[derive(Clone, Copy, Debug)]
enum Sys {
    Dmb,
    Dsb,
    Isb,
    Hint,
}

#[allow(clippy::too_many_lines, reason = "one arm per intrinsic")]
fn low(which: Intrinsic<'_>) -> Option<Low> {
    use Intrinsic as I;
    use Low as L;
    Some(match which {
        I::Aarch64NeonShadd => L::Int3(Int3::Shadd),
        I::Aarch64NeonUhadd => L::Int3(Int3::Uhadd),
        I::Aarch64NeonSqadd => L::Int3(Int3::Sqadd),
        I::Aarch64NeonUqadd => L::Int3(Int3::Uqadd),
        I::Aarch64NeonSrhadd => L::Int3(Int3::Srhadd),
        I::Aarch64NeonUrhadd => L::Int3(Int3::Urhadd),
        I::Aarch64NeonShsub => L::Int3(Int3::Shsub),
        I::Aarch64NeonUhsub => L::Int3(Int3::Uhsub),
        I::Aarch64NeonSqsub => L::Int3(Int3::Sqsub),
        I::Aarch64NeonUqsub => L::Int3(Int3::Uqsub),
        I::Aarch64NeonSshl => L::Int3(Int3::Sshl),
        I::Aarch64NeonUshl => L::Int3(Int3::Ushl),
        I::Aarch64NeonSqshl => L::Int3(Int3::Sqshl),
        I::Aarch64NeonUqshl => L::Int3(Int3::Uqshl),
        I::Aarch64NeonSrshl => L::Int3(Int3::Srshl),
        I::Aarch64NeonUrshl => L::Int3(Int3::Urshl),
        I::Aarch64NeonSqrshl => L::Int3(Int3::Sqrshl),
        I::Aarch64NeonUqrshl => L::Int3(Int3::Uqrshl),
        I::Aarch64NeonSmax => L::Int3(Int3::Smax),
        I::Aarch64NeonUmax => L::Int3(Int3::Umax),
        I::Aarch64NeonSmin => L::Int3(Int3::Smin),
        I::Aarch64NeonUmin => L::Int3(Int3::Umin),
        I::Aarch64NeonSabd => L::Int3(Int3::Sabd),
        I::Aarch64NeonUabd => L::Int3(Int3::Uabd),
        I::Aarch64NeonPmul => L::Int3(Int3::Pmul),
        I::Aarch64NeonSmaxp => L::Int3(Int3::Smaxp),
        I::Aarch64NeonUmaxp => L::Int3(Int3::Umaxp),
        I::Aarch64NeonSminp => L::Int3(Int3::Sminp),
        I::Aarch64NeonUminp => L::Int3(Int3::Uminp),
        I::Aarch64NeonSqdmulh => L::Int3(Int3::Sqdmulh),
        I::Aarch64NeonSqrdmulh => L::Int3(Int3::Sqrdmulh),
        I::Aarch64NeonAddp => L::Int3(Int3::Addp),
        I::Aarch64NeonSqrdmlah => L::Int3(Int3::Sqrdmlah),
        I::Aarch64NeonSqrdmlsh => L::Int3(Int3::Sqrdmlsh),
        I::Aarch64NeonSqdmulhLane | I::Aarch64NeonSqdmulhLaneq => L::Int3Lane(Int3::Sqdmulh),
        I::Aarch64NeonSqrdmulhLane | I::Aarch64NeonSqrdmulhLaneq => L::Int3Lane(Int3::Sqrdmulh),
        I::Aarch64NeonFmax => L::Fp3(Fp3::Fmax),
        I::Aarch64NeonFmin => L::Fp3(Fp3::Fmin),
        I::Aarch64NeonFmaxnm => L::Fp3(Fp3::Fmaxnm),
        I::Aarch64NeonFminnm => L::Fp3(Fp3::Fminnm),
        I::Aarch64NeonFmulx => L::Fp3(Fp3::Fmulx),
        I::Aarch64NeonFrecps => L::Fp3(Fp3::Frecps),
        I::Aarch64NeonFrsqrts => L::Fp3(Fp3::Frsqrts),
        I::Aarch64NeonFabd | I::Aarch64SisdFabd => L::Fp3(Fp3::Fabd),
        I::Aarch64NeonFaddp => L::Fp3(Fp3::Faddp),
        I::Aarch64NeonFmaxp => L::Fp3(Fp3::Fmaxp),
        I::Aarch64NeonFminp => L::Fp3(Fp3::Fminp),
        I::Aarch64NeonFmaxnmp => L::Fp3(Fp3::Fmaxnmp),
        I::Aarch64NeonFminnmp => L::Fp3(Fp3::Fminnmp),
        I::Aarch64NeonFacge => L::FpCompare(Fp3::Facge),
        I::Aarch64NeonFacgt => L::FpCompare(Fp3::Facgt),
        I::Aarch64NeonAbs => L::Int2(Int2::Abs),
        I::Aarch64NeonSqabs => L::Int2(Int2::Sqabs),
        I::Aarch64NeonSqneg => L::Int2(Int2::Sqneg),
        I::Aarch64NeonCls => L::Int2(Int2::Cls),
        I::Aarch64NeonUrecpe => L::Int2(Int2::Urecpe),
        I::Aarch64NeonUrsqrte => L::Int2(Int2::Ursqrte),
        I::Aarch64NeonSuqadd => L::Int2(Int2::Suqadd),
        I::Aarch64NeonUsqadd => L::Int2(Int2::Usqadd),
        I::Aarch64NeonSaddlp => L::PairLong(true),
        I::Aarch64NeonUaddlp => L::PairLong(false),
        I::Aarch64NeonSqxtn | I::Aarch64NeonScalarSqxtn => L::Narrow(Int2::Sqxtn),
        I::Aarch64NeonUqxtn | I::Aarch64NeonScalarUqxtn => L::Narrow(Int2::Uqxtn),
        I::Aarch64NeonSqxtun | I::Aarch64NeonScalarSqxtun => L::Narrow(Int2::Sqxtun),
        I::Aarch64NeonFrecpe => L::Fp2(Fp2::Frecpe),
        I::Aarch64NeonFrsqrte => L::Fp2(Fp2::Frsqrte),
        I::Aarch64NeonFrecpx => L::Fp2(Fp2::Frecpx),
        I::Aarch64NeonFrint32x | I::Aarch64Frint32x => L::Fp2(Fp2::Frint32x),
        I::Aarch64NeonFrint32z | I::Aarch64Frint32z => L::Fp2(Fp2::Frint32z),
        I::Aarch64NeonFrint64x | I::Aarch64Frint64x => L::Fp2(Fp2::Frint64x),
        I::Aarch64NeonFrint64z | I::Aarch64Frint64z => L::Fp2(Fp2::Frint64z),
        I::Aarch64NeonFcvtns => L::FpToInt(Fp2::Fcvtns),
        I::Aarch64NeonFcvtnu => L::FpToInt(Fp2::Fcvtnu),
        I::Aarch64NeonFcvtms => L::FpToInt(Fp2::Fcvtms),
        I::Aarch64NeonFcvtmu => L::FpToInt(Fp2::Fcvtmu),
        I::Aarch64NeonFcvtas => L::FpToInt(Fp2::Fcvtas),
        I::Aarch64NeonFcvtau => L::FpToInt(Fp2::Fcvtau),
        I::Aarch64NeonFcvtps => L::FpToInt(Fp2::Fcvtps),
        I::Aarch64NeonFcvtpu => L::FpToInt(Fp2::Fcvtpu),
        I::Aarch64NeonFcvtzs => L::FpToInt(Fp2::Fcvtzs),
        I::Aarch64NeonFcvtzu => L::FpToInt(Fp2::Fcvtzu),
        I::Aarch64NeonVcvtfp2fxs => L::Fixed(ShiftImm::Fcvtzs),
        I::Aarch64NeonVcvtfp2fxu => L::Fixed(ShiftImm::Fcvtzu),
        I::Aarch64NeonVcvtfxs2fp => L::Fixed(ShiftImm::Scvtf),
        I::Aarch64NeonVcvtfxu2fp => L::Fixed(ShiftImm::Ucvtf),
        I::Aarch64NeonVsli => L::Insert(ShiftImm::Sli),
        I::Aarch64NeonVsri => L::Insert(ShiftImm::Sri),
        I::Aarch64NeonRshrn => L::ShiftNarrow(ShiftImm::Rshrn),
        I::Aarch64NeonSqshrn => L::ShiftNarrow(ShiftImm::Sqshrn),
        I::Aarch64NeonSqrshrn => L::ShiftNarrow(ShiftImm::Sqrshrn),
        I::Aarch64NeonSqshrun => L::ShiftNarrow(ShiftImm::Sqshrun),
        I::Aarch64NeonSqrshrun => L::ShiftNarrow(ShiftImm::Sqrshrun),
        I::Aarch64NeonUqshrn => L::ShiftNarrow(ShiftImm::Uqshrn),
        I::Aarch64NeonUqrshrn => L::ShiftNarrow(ShiftImm::Uqrshrn),
        I::Aarch64NeonSqshlu => L::ShiftSplat(ShiftImm::Sqshlu),
        I::Aarch64NeonSaddlv => L::AddLong(true),
        I::Aarch64NeonUaddlv => L::AddLong(false),
        I::Aarch64NeonSaddv | I::Aarch64NeonUaddv => L::Reduce(Reduce::Add),
        I::Aarch64NeonSmaxv => L::Reduce(Reduce::Smax),
        I::Aarch64NeonSminv => L::Reduce(Reduce::Smin),
        I::Aarch64NeonUmaxv => L::Reduce(Reduce::Umax),
        I::Aarch64NeonUminv => L::Reduce(Reduce::Umin),
        I::Aarch64NeonFaddv => L::FpReduce(None),
        I::Aarch64NeonFmaxv => L::FpReduce(Some(FpAcross::Fmaxv)),
        I::Aarch64NeonFminv => L::FpReduce(Some(FpAcross::Fminv)),
        I::Aarch64NeonFmaxnmv => L::FpReduce(Some(FpAcross::Fmaxnmv)),
        I::Aarch64NeonFminnmv => L::FpReduce(Some(FpAcross::Fminnmv)),
        I::Aarch64NeonSmull => L::Long3(Long3::Smull),
        I::Aarch64NeonUmull => L::Long3(Long3::Umull),
        I::Aarch64NeonPmull => L::Long3(Long3::Pmull),
        I::Aarch64NeonSqdmull => L::Long3(Long3::Sqdmull),
        I::Aarch64NeonRaddhn => L::Long3(Long3::Raddhn),
        I::Aarch64NeonRsubhn => L::Long3(Long3::Rsubhn),
        I::Aarch64NeonPmull64 => L::Pmull64,
        I::Aarch64NeonSqdmullsScalar => L::SqdmullScalar,
        I::Aarch64NeonSdot => L::Dot(Dot::Sdot),
        I::Aarch64NeonUdot => L::Dot(Dot::Udot),
        I::Aarch64NeonUsdot => L::Dot(Dot::Usdot),
        I::Aarch64NeonSmmla => L::Dot(Dot::Smmla),
        I::Aarch64NeonUmmla => L::Dot(Dot::Ummla),
        I::Aarch64NeonUsmmla => L::Dot(Dot::Usmmla),
        I::Aarch64NeonFmlal => L::Dot(Dot::Fmlal),
        I::Aarch64NeonFmlal2 => L::Dot(Dot::Fmlal2),
        I::Aarch64NeonFmlsl => L::Dot(Dot::Fmlsl),
        I::Aarch64NeonFmlsl2 => L::Dot(Dot::Fmlsl2),
        I::Aarch64NeonVcmlaRot0 => L::Cmla(0),
        I::Aarch64NeonVcmlaRot90 => L::Cmla(1),
        I::Aarch64NeonVcmlaRot180 => L::Cmla(2),
        I::Aarch64NeonVcmlaRot270 => L::Cmla(3),
        I::Aarch64NeonVcaddRot90 => L::Cadd(false),
        I::Aarch64NeonVcaddRot270 => L::Cadd(true),
        I::Aarch64CryptoAese => L::Aes(Aes::Aese),
        I::Aarch64CryptoAesd => L::Aes(Aes::Aesd),
        I::Aarch64CryptoAesmc => L::Aes(Aes::Aesmc),
        I::Aarch64CryptoAesimc => L::Aes(Aes::Aesimc),
        I::Aarch64CryptoSha1c => L::Sha3(Sha3Reg::Sha1c),
        I::Aarch64CryptoSha1p => L::Sha3(Sha3Reg::Sha1p),
        I::Aarch64CryptoSha1m => L::Sha3(Sha3Reg::Sha1m),
        I::Aarch64CryptoSha1su0 => L::Sha3(Sha3Reg::Sha1su0),
        I::Aarch64CryptoSha256h => L::Sha3(Sha3Reg::Sha256h),
        I::Aarch64CryptoSha256h2 => L::Sha3(Sha3Reg::Sha256h2),
        I::Aarch64CryptoSha256su1 => L::Sha3(Sha3Reg::Sha256su1),
        I::Aarch64CryptoSha512h => L::Sha3(Sha3Reg::Sha512h),
        I::Aarch64CryptoSha512h2 => L::Sha3(Sha3Reg::Sha512h2),
        I::Aarch64CryptoSha512su1 => L::Sha3(Sha3Reg::Sha512su1),
        I::Aarch64CryptoRax1 => L::Sha3(Sha3Reg::Rax1),
        I::Aarch64CryptoSha1h => L::Sha2(Sha2Reg::Sha1h),
        I::Aarch64CryptoSha1su1 => L::Sha2(Sha2Reg::Sha1su1),
        I::Aarch64CryptoSha256su0 => L::Sha2(Sha2Reg::Sha256su0),
        I::Aarch64CryptoSha512su0 => L::Sha2(Sha2Reg::Sha512su0),
        I::Aarch64CryptoEor3s | I::Aarch64CryptoEor3u => L::Ternary(true),
        I::Aarch64CryptoBcaxs | I::Aarch64CryptoBcaxu => L::Ternary(false),
        I::Aarch64CryptoXar => L::Xar,
        I::Aarch64CryptoSm3partw1 => L::Sm3(mc::Sm3Reg::Sm3partw1),
        I::Aarch64CryptoSm3partw2 => L::Sm3(mc::Sm3Reg::Sm3partw2),
        I::Aarch64CryptoSm4ekey => L::Sm3(mc::Sm3Reg::Sm4ekey),
        I::Aarch64CryptoSm3ss1 => L::Sm3ss1,
        I::Aarch64CryptoSm3tt1a => L::Sm3tt(mc::Sm3tt::Sm3tt1a),
        I::Aarch64CryptoSm3tt1b => L::Sm3tt(mc::Sm3tt::Sm3tt1b),
        I::Aarch64CryptoSm3tt2a => L::Sm3tt(mc::Sm3tt::Sm3tt2a),
        I::Aarch64CryptoSm3tt2b => L::Sm3tt(mc::Sm3tt::Sm3tt2b),
        I::Aarch64CryptoSm4e => L::Sm4e,
        I::Aarch64Fjcvtzs => L::Jcvt,
        I::Aarch64NeonFcvtxn | I::Aarch64SisdFcvtxn => L::Fcvtxn,
        I::Aarch64NeonVcvtfp2hf => L::HalfConvert(true),
        I::Aarch64NeonVcvthf2fp => L::HalfConvert(false),
        I::Aarch64NeonTbl1 => L::Table(false, 1),
        I::Aarch64NeonTbl2 => L::Table(false, 2),
        I::Aarch64NeonTbl3 => L::Table(false, 3),
        I::Aarch64NeonTbl4 => L::Table(false, 4),
        I::Aarch64NeonTbx1 => L::Table(true, 1),
        I::Aarch64NeonTbx2 => L::Table(true, 2),
        I::Aarch64NeonTbx3 => L::Table(true, 3),
        I::Aarch64NeonTbx4 => L::Table(true, 4),
        I::Aarch64NeonLd1x2 => L::Multi(Multi::Ld1x2),
        I::Aarch64NeonLd1x3 => L::Multi(Multi::Ld1x3),
        I::Aarch64NeonLd1x4 => L::Multi(Multi::Ld1x4),
        I::Aarch64NeonLd2 => L::Multi(Multi::Ld2),
        I::Aarch64NeonLd3 => L::Multi(Multi::Ld3),
        I::Aarch64NeonLd4 => L::Multi(Multi::Ld4),
        I::Aarch64NeonSt1x2 => L::Multi(Multi::St1x2),
        I::Aarch64NeonSt1x3 => L::Multi(Multi::St1x3),
        I::Aarch64NeonSt1x4 => L::Multi(Multi::St1x4),
        I::Aarch64NeonSt2 => L::Multi(Multi::St2),
        I::Aarch64NeonSt3 => L::Multi(Multi::St3),
        I::Aarch64NeonSt4 => L::Multi(Multi::St4),
        I::Aarch64NeonLd2lane => L::Lane(true, 2),
        I::Aarch64NeonLd3lane => L::Lane(true, 3),
        I::Aarch64NeonLd4lane => L::Lane(true, 4),
        I::Aarch64NeonSt2lane => L::Lane(false, 2),
        I::Aarch64NeonSt3lane => L::Lane(false, 3),
        I::Aarch64NeonSt4lane => L::Lane(false, 4),
        I::Aarch64NeonLd2r => L::Replicate(2),
        I::Aarch64NeonLd3r => L::Replicate(3),
        I::Aarch64NeonLd4r => L::Replicate(4),
        I::Aarch64Cls => L::Cls(false),
        I::Aarch64Cls64 => L::Cls(true),
        I::Aarch64Dmb => L::System(Sys::Dmb),
        I::Aarch64Dsb => L::System(Sys::Dsb),
        I::Aarch64Isb => L::System(Sys::Isb),
        I::Aarch64Hint => L::System(Sys::Hint),
        _ => return None,
    })
}

/// Whether this module lowers `which` (for some signature): the
/// intrinsics whose matrix row is supported.
pub fn lowers(which: Intrinsic<'_>) -> bool {
    low(which).is_some()
}

// ---- types ----

/// What a value of the call is, as the encoder families see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum T {
    /// A 64- or 128-bit vector; `fp` for floating-point elements.
    Vec { bits: VecBits, elem: Elem, fp: Option<FpFormat> },
    /// An integer of this many bits.
    Int(u32),
    /// A floating-point scalar.
    Fp(FpFormat),
    /// A pointer.
    Ptr,
    /// `void`.
    Void,
    /// Anything else (bfloat, other vectors, structs).
    Other,
}

impl T {
    /// The vector's width and element, also for floating point.
    fn vec(self) -> Option<(VecBits, Elem)> {
        match self {
            T::Vec { bits, elem, .. } => Some((bits, elem)),
            _ => None,
        }
    }

    /// A vector of integers.
    fn ivec(self) -> Option<(VecBits, Elem)> {
        match self {
            T::Vec { bits, elem, fp: None } => Some((bits, elem)),
            _ => None,
        }
    }

    /// A vector of floating point.
    fn fvec(self) -> Option<(VecBits, FpFormat)> {
        match self {
            T::Vec { bits, fp: Some(fmt), .. } => Some((bits, fmt)),
            _ => None,
        }
    }

    /// The scalar view of a one-element vector or a scalar: the element.
    fn scalar(self) -> Option<Elem> {
        match self {
            T::Vec { bits: VecBits::V64, elem: Elem::D, .. } | T::Int(64) => Some(Elem::D),
            T::Int(32) => Some(Elem::S),
            T::Fp(fmt) => Some(fp_elem(fmt)),
            _ => None,
        }
    }

    /// The scalar floating-point format of a scalar or `<1 x double>`.
    fn fp_scalar(self) -> Option<FpFormat> {
        match self {
            T::Fp(fmt) | T::Vec { bits: VecBits::V64, elem: Elem::D, fp: Some(fmt) } => Some(fmt),
            _ => None,
        }
    }

    fn is_gpr(self) -> bool {
        matches!(self, T::Int(_) | T::Ptr)
    }
}

fn fp_elem(fmt: FpFormat) -> Elem {
    match fmt {
        FpFormat::H => Elem::H,
        FpFormat::S => Elem::S,
        FpFormat::D => Elem::D,
    }
}

fn kind_of(cx: &FnCx<'_, A64>, ty: TypeId) -> T {
    kind_of_as(cx, ty, false)
}

/// [`kind_of`], and with `bfloat_as_h` a vector of `bfloat` is one of
/// 16-bit integers: what the structure loads and stores move (WP-205).
fn kind_of_as(cx: &FnCx<'_, A64>, ty: TypeId, bfloat_as_h: bool) -> T {
    let types = cx.types();
    match types.kind(ty) {
        TypeKind::Void => T::Void,
        TypeKind::Int(bits) if bits <= 64 => T::Int(bits),
        TypeKind::Ptr { addr_space: 0 } => T::Ptr,
        TypeKind::Half => T::Fp(FpFormat::H),
        TypeKind::Float => T::Fp(FpFormat::S),
        TypeKind::Double => T::Fp(FpFormat::D),
        TypeKind::Vector { elem, len } => {
            let (elem, fp, bits) = match types.kind(elem) {
                TypeKind::Int(8) => (Elem::B, None, 8),
                TypeKind::Int(16) => (Elem::H, None, 16),
                TypeKind::Int(32) => (Elem::S, None, 32),
                TypeKind::Int(64) => (Elem::D, None, 64),
                TypeKind::Half => (Elem::H, Some(FpFormat::H), 16),
                TypeKind::BFloat if bfloat_as_h => (Elem::H, None, 16),
                TypeKind::Float => (Elem::S, Some(FpFormat::S), 32),
                TypeKind::Double => (Elem::D, Some(FpFormat::D), 64),
                _ => return T::Other,
            };
            match u64::from(len) * bits {
                64 => T::Vec { bits: VecBits::V64, elem, fp },
                128 => T::Vec { bits: VecBits::V128, elem, fp },
                _ => T::Other,
            }
        }
        _ => T::Other,
    }
}

fn value_kind(cx: &FnCx<'_, A64>, value: Value) -> T {
    kind_of(cx, crate::common::value_ty(cx, value))
}

fn arr(bits: VecBits, elem: Elem) -> Option<Arr> {
    Some(match (bits, elem) {
        (VecBits::V64, Elem::B) => Arr::B8,
        (VecBits::V128, Elem::B) => Arr::B16,
        (VecBits::V64, Elem::H) => Arr::H4,
        (VecBits::V128, Elem::H) => Arr::H8,
        (VecBits::V64, Elem::S) => Arr::S2,
        (VecBits::V128, Elem::S) => Arr::S4,
        (VecBits::V128, Elem::D) => Arr::D2,
        (VecBits::V64, Elem::D) => return None,
    })
}

fn gpr_size(t: T) -> Size {
    if matches!(t, T::Int(64) | T::Ptr) { Size::X } else { Size::W }
}

/// An encoder result: the word, or [`TARGET`] when the checked form
/// refuses the shape.
fn word(word: Option<u32>) -> CgResult<u32> {
    word.map_or_else(|| unsupported(TARGET), Ok)
}

fn imm(cx: &FnCx<'_, A64>, value: Value) -> CgResult<u32> {
    match const_int(cx, value) {
        Some(v) => u32::try_from(v).map_or_else(|_| unsupported(IMMEDIATE), Ok),
        None => unsupported(IMMEDIATE),
    }
}

/// The element of a constant splat vector (or the constant integer),
/// zero-extended.
fn splat(cx: &FnCx<'_, A64>, value: Value) -> Option<u64> {
    let id = value.as_const()?;
    match cx.module.consts.get(id) {
        Const::Splat { elem, .. } => match cx.module.consts.get(elem) {
            Const::Int { value, .. } => Some(value),
            _ => None,
        },
        Const::Int { value, .. } => Some(value),
        Const::Zero(_) => Some(0),
        Const::Aggregate { elems, .. } => {
            // A vector constant with every element equal; `undef` and
            // `poison` elements may be anything (WP-205: clang's scalar
            // `vqshlub_n_s8` sets lane 0 only).
            let mut first = None;
            for &e in elems {
                let value = match cx.module.consts.get(e) {
                    Const::Int { value, .. } => value,
                    Const::Undef(_) | Const::Poison(_) => continue,
                    _ => return None,
                };
                if *first.get_or_insert(value) != value {
                    return None;
                }
            }
            first
        }
        _ => None,
    }
}

// ---- operands ----

/// The registers of a call's register operands.
struct Regs {
    v: Vec<VReg>,
    /// The `use_reg`'d vector registers, which may hold the result.
    fpr: Vec<PReg>,
    /// The first general register operand, which may hold the result.
    gpr: Option<PReg>,
}

fn preg(r: VReg) -> PReg {
    crate::reg::v(r.index())
}

/// Puts each of `args` in a vector register (see the module docs).
fn load(cx: &mut FnCx<'_, A64>, args: &[Value]) -> CgResult<Regs> {
    let kinds: Vec<T> = args.iter().map(|&a| value_kind(cx, a)).collect();
    let mut v: Vec<Option<VReg>> = vec![None; args.len()];
    let mut fpr = Vec::new();
    let mut gpr_regs: Vec<Option<PReg>> = vec![None; args.len()];
    let mut gpr = None;
    let earlier =
        |v: &[Option<VReg>], i: usize| (0..i).find(|&j| args[j] == args[i] && v[j].is_some());
    for i in 0..args.len() {
        if kinds[i].is_gpr() || fpr.len() == 2 {
            continue;
        }
        if let Some(j) = earlier(&v, i) {
            v[i] = v[j];
            continue;
        }
        let p = cx.use_reg(args[i])?;
        fpr.push(p);
        v[i] = Some(vr(p));
    }
    for i in 0..args.len() {
        if !kinds[i].is_gpr() {
            continue;
        }
        if let Some(j) = (0..i).find(|&j| args[j] == args[i] && gpr_regs[j].is_some()) {
            gpr_regs[i] = gpr_regs[j];
            continue;
        }
        let p = cx.use_reg(args[i])?;
        gpr.get_or_insert(p);
        gpr_regs[i] = Some(p);
    }
    let mut scratch = [V30, V31].into_iter();
    for i in 0..args.len() {
        if v[i].is_some() {
            continue;
        }
        if let Some(j) = earlier(&v, i) {
            v[i] = v[j];
            continue;
        }
        let r = match scratch.next() {
            Some(r) => r,
            None => vr(cx.temp(Class::Fpr)?),
        };
        match gpr_regs[i] {
            Some(g) => put(cx, mc::fmov_gpr_to_fp(gpr_size(kinds[i]), r, xr(g).zr())),
            None => cx.put(args[i], preg(r))?,
        }
        v[i] = Some(r);
    }
    Ok(Regs { v: v.into_iter().map(|r| r.unwrap_or(V30)).collect(), fpr, gpr })
}

/// The result of `inst`, computed in `rv`: kept there if it is an
/// operand's register, else moved to the result's register.
///
/// A general result is the low bits of `rv`, zero-extended, also those of
/// the signed reductions of bytes and halfwords (`saddv`, `smaxv`,
/// `sminv`) as `llc` does; `saddlv` of bytes is `tail3::saddlv` (WP-101).
fn done<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, regs: &Regs, rv: VReg) -> CgResult<()> {
    let t = kind_of(cx, inst.ty());
    if t.is_gpr() {
        let g = match regs.gpr {
            Some(g) => g,
            None => cx.def_reg(inst)?,
        };
        put(cx, mc::fmov_fp_to_gpr(gpr_size(t), xr(g).zr(), rv));
        return cx.finish(inst, g);
    }
    if let Some(&p) = regs.fpr.iter().find(|&&p| vr(p) == rv) {
        return cx.finish(inst, p);
    }
    let d = cx.def_reg(inst)?;
    put(cx, mc::mov_vec(VecBits::V128, vr(d), rv));
    cx.finish(inst, d)
}

// ---- the entry point ----

/// A call of target intrinsic `which` with arguments `args`; any other
/// intrinsic is refused as `intrinsic.rs` refused it before.
///
/// # Errors
/// [`TARGET`], [`IMMEDIATE`]; [`INTRINSIC`] for an intrinsic that is not
/// this module's; what placing the operands reports.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let Some(low) = low(which) else { return unsupported(INTRINSIC) };
    let ret = kind_of(cx, inst.ty());
    let memory = matches!(low, Low::Multi(_) | Low::Lane(..) | Low::Replicate(_));
    let kinds: Vec<T> =
        args.iter().map(|&a| kind_of_as(cx, crate::common::value_ty(cx, a), memory)).collect();
    match low {
        Low::Table(extend, n) => return table(cx, inst, extend, n, args, ret),
        Low::Multi(op) => return multi(cx, inst, op, args, &kinds, ret),
        Low::Lane(load, n) => return lane(cx, inst, load, n, args, &kinds, ret),
        Low::Replicate(n) => return replicate(cx, inst, n, args, ret),
        Low::Cls(wide) => return cls(cx, inst, wide, args),
        Low::System(op) => return system(cx, inst, op, args),
        _ => {}
    }
    // Every other family: register operands, then at most one immediate
    // last.
    let (regs_args, imm_arg) = match low {
        Low::Fixed(_)
        | Low::Insert(_)
        | Low::ShiftNarrow(_)
        | Low::Xar
        | Low::Int3Lane(_)
        | Low::Sm3tt(_) => {
            let (imm_last, operands) =
                args.split_last().map_or((None, args), |(l, r)| (Some(*l), r));
            (operands, imm_last)
        }
        Low::ShiftSplat(_) => (&args[..1.min(args.len())], args.get(1).copied()),
        _ => (args, None),
    };
    let w = plan(cx, low, ret, &kinds, imm_arg)?;
    deferred_first(cx, inst, None)?;
    let regs = load(cx, regs_args)?;
    if let Plan::Gpr { word } = w {
        let g = cx.def_reg(inst)?;
        put(cx, (word & !0x3ff) | u32::from(regs.v[0].index()) << 5 | u32::from(xr(g).index()));
        return cx.finish(inst, g);
    }
    let r = |k: usize| regs.v.get(k).copied().unwrap_or(V30);
    let rv = emit(cx, w, &r)?;
    done(cx, inst, &regs, rv)
}

/// The instructions of a register-operand family, decided before any
/// operand is read: `Op(k, word)` writes into operand `k`'s register;
/// `Lane` duplicates an element first.
#[derive(Clone, Copy, Debug)]
enum Plan {
    /// One instruction into operand `0`'s register, its operands
    /// `rd = r0`, `rn = r(n)`, `rm = r(m)` if any; the word with zero register
    /// fields.
    One { word: u32, n: usize, m: Option<usize>, a: Option<usize> },
    /// Two instructions into `r0`: the first `r0 <- r0`, then the second.
    Two { first: u32, second: u32 },
    /// `dup r1, r1[lane]` (`arr`), then `word` as in `One`.
    Lane { dup: u32, word: u32 },
    /// Into the general result directly: `word` with `rd` the result.
    Gpr { word: u32 },
}

/// A word with the register fields of `rd`, `rn`, `rm` and `ra` (bits
/// 14:10, for the four-register forms) or-ed in.
fn fields(word: u32, rd: VReg, rn: VReg, rm: Option<VReg>, ra: Option<VReg>) -> u32 {
    let rm = rm.map_or(0, |r| u32::from(r.index()) << 16);
    let ra = ra.map_or(0, |r| u32::from(r.index()) << 10);
    word | rm | ra | u32::from(rn.index()) << 5 | u32::from(rd.index())
}

fn emit(cx: &mut FnCx<'_, A64>, plan: Plan, r: &dyn Fn(usize) -> VReg) -> CgResult<VReg> {
    let r0 = r(0);
    match plan {
        Plan::One { word, n, m, a } => {
            put(cx, fields(word, r0, r(n), m.map(r), a.map(r)));
        }
        Plan::Two { first, second } => {
            put(cx, fields(first, r0, r0, Some(r0), None));
            put(cx, fields(second, r0, r0, None, None));
        }
        Plan::Lane { dup, word } => {
            let r1 = r(1);
            put(cx, fields(dup, r1, r1, None, None));
            put(cx, fields(word, r0, r0, Some(r1), None));
        }
        Plan::Gpr { .. } => return cx.bug("a general-register plan in emit"),
    }
    Ok(r0)
}

/// The encoder word of `low` for these types, with zero register fields.
#[allow(clippy::too_many_lines, reason = "one arm per family")]
fn plan(
    cx: &FnCx<'_, A64>,
    low: Low,
    ret: T,
    kinds: &[T],
    imm_arg: Option<Value>,
) -> CgResult<Plan> {
    let v0 = VReg::V0;
    let one = |word: u32| Plan::One { word, n: 0, m: Some(1), a: None };
    let k = |i: usize| kinds.get(i).copied().unwrap_or(T::Other);
    let immediate = |cx: &FnCx<'_, A64>| match imm_arg {
        Some(v) => imm(cx, v),
        None => unsupported(IMMEDIATE),
    };
    Ok(match low {
        Low::Int3(op) => {
            let tied = matches!(op, Int3::Sqrdmlah | Int3::Sqrdmlsh);
            let w = if let Some((bits, elem)) = ret.ivec().filter(|&(b, e)| arr(b, e).is_some()) {
                word(mc::try_neon_int3(op, bits, elem, v0, v0, v0))?
            } else if let Some(elem) = ret.scalar() {
                word(mc::try_neon_int3_scalar(op, elem, v0, v0, v0))?
            } else {
                return unsupported(TARGET);
            };
            if tied { Plan::One { word: w, n: 1, m: Some(2), a: None } } else { one(w) }
        }
        Low::Int3Lane(op) => {
            let (Some((bits, elem)), Some((from, _))) = (ret.ivec(), k(1).ivec()) else {
                return unsupported(TARGET);
            };
            let lane = immediate(cx)?;
            let Some(a) = arr(bits, elem) else { return unsupported(TARGET) };
            let lanes = if from == VecBits::V128 { elem.lanes() } else { elem.lanes() / 2 };
            if lane >= lanes {
                return unsupported(IMMEDIATE);
            }
            Plan::Lane {
                dup: word(mc::try_dup_elem(a, v0, v0, lane))?,
                word: word(mc::try_neon_int3(op, bits, elem, v0, v0, v0))?,
            }
        }
        Low::Fp3(op) => {
            if let Some((bits, fmt)) =
                ret.fvec().filter(|&(b, f)| !(b == VecBits::V64 && f == FpFormat::D))
            {
                one(word(mc::try_neon_fp3(op, bits, fmt, v0, v0, v0))?)
            } else if let Some(fmt) = ret.fp_scalar() {
                if op.has_scalar() {
                    one(word(mc::try_neon_fp3_scalar(op, fmt, v0, v0, v0))?)
                } else {
                    // Half precision: the single-precision word in its
                    // `FEAT_FP16` form (WP-205).
                    let sz = if fmt == FpFormat::D { FpSize::D } else { FpSize::S };
                    let f = match op {
                        Fp3::Fmax => mc::fmax,
                        Fp3::Fmin => mc::fmin,
                        Fp3::Fmaxnm => mc::fmaxnm,
                        Fp3::Fminnm => mc::fminnm,
                        _ => return unsupported(TARGET),
                    };
                    let w = f(sz, v0, v0, v0);
                    one(if fmt == FpFormat::H { mc::fp_half(w) } else { w })
                }
            } else {
                return unsupported(TARGET);
            }
        }
        Low::FpCompare(op) => {
            if let Some((bits, fmt)) =
                k(0).fvec().filter(|&(b, f)| !(b == VecBits::V64 && f == FpFormat::D))
            {
                one(word(mc::try_neon_fp3(op, bits, fmt, v0, v0, v0))?)
            } else if let Some(fmt) = k(0).fp_scalar() {
                one(word(mc::try_neon_fp3_scalar(op, fmt, v0, v0, v0))?)
            } else {
                return unsupported(TARGET);
            }
        }
        Low::Int2(op) => {
            let w = if let Some((bits, elem)) = ret.ivec().filter(|&(b, e)| arr(b, e).is_some()) {
                word(mc::try_neon_int2(op, bits, elem, v0, v0))?
            } else if let Some(elem) = ret.scalar() {
                word(mc::try_neon_int2_scalar(op, elem, v0, v0))?
            } else {
                return unsupported(TARGET);
            };
            // `suqadd`/`usqadd` accumulate operand 1 into operand 0.
            let n = usize::from(matches!(op, Int2::Suqadd | Int2::Usqadd));
            Plan::One { word: w, n, m: None, a: None }
        }
        Low::PairLong(signed) => {
            let Some((bits, elem)) = k(0).ivec() else { return unsupported(TARGET) };
            let w = if signed {
                word(mc::try_neon_int2(Int2::Saddlp, bits, elem, v0, v0))?
            } else {
                let Some(a) = arr(bits, elem) else { return unsupported(TARGET) };
                word(mc::try_uaddlp(a, v0, v0))?
            };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::Narrow(op) => {
            let w = match (ret, k(0)) {
                (
                    T::Vec { bits: VecBits::V64, elem, fp: None },
                    T::Vec { bits: VecBits::V128, .. },
                ) => word(mc::try_neon_int2(op, VecBits::V64, elem, v0, v0))?,
                (T::Int(32), T::Int(64)) => word(mc::try_neon_int2_scalar(op, Elem::S, v0, v0))?,
                _ => return unsupported(TARGET),
            };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::Fp2(op) => {
            let w = if let Some((bits, fmt)) =
                ret.fvec().filter(|&(b, f)| !(b == VecBits::V64 && f == FpFormat::D))
            {
                word(mc::try_neon_fp2(op, bits, fmt, v0, v0))?
            } else if let Some(fmt) = ret.fp_scalar() {
                let frint = match op {
                    Fp2::Frint32z => Some((false, false)),
                    Fp2::Frint32x => Some((false, true)),
                    Fp2::Frint64z => Some((true, false)),
                    Fp2::Frint64x => Some((true, true)),
                    _ => None,
                };
                match (frint, fmt) {
                    (Some((wide, exact)), FpFormat::S | FpFormat::D) => {
                        mc::frint_scalar(wide, exact, fmt == FpFormat::D, v0, v0)
                    }
                    _ => word(mc::try_neon_fp2_scalar(op, fmt, v0, v0))?,
                }
            } else {
                return unsupported(TARGET);
            };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::FpToInt(op) => {
            if let (Some((bits, fmt)), Some(_)) = (k(0).fvec(), ret.ivec())
                && !(bits == VecBits::V64 && fmt == FpFormat::D)
            {
                let w = word(mc::try_neon_fp2(op, bits, fmt, v0, v0))?;
                return Ok(Plan::One { word: w, n: 0, m: None, a: None });
            }
            match (ret, k(0).fp_scalar()) {
                (T::Vec { .. }, Some(fmt)) => {
                    let w = word(mc::try_neon_fp2_scalar(op, fmt, v0, v0))?;
                    Plan::One { word: w, n: 0, m: None, a: None }
                }
                // `i16` of half precision: the scalar SIMD form, saturating
                // to 16 bits, as `llc` 22 (WP-205; `llc` 19 cannot select it).
                (T::Int(16), Some(FpFormat::H)) => {
                    let w = word(mc::try_neon_fp2_scalar(op, FpFormat::H, v0, v0))?;
                    Plan::One { word: w, n: 0, m: None, a: None }
                }
                (T::Int(32 | 64), Some(fmt)) => {
                    let fsz = if fmt == FpFormat::D { FpSize::D } else { FpSize::S };
                    let f = match op {
                        Fp2::Fcvtns => mc::fcvtns,
                        Fp2::Fcvtnu => mc::fcvtnu,
                        Fp2::Fcvtms => mc::fcvtms,
                        Fp2::Fcvtmu => mc::fcvtmu,
                        Fp2::Fcvtas => mc::fcvtas,
                        Fp2::Fcvtau => mc::fcvtau,
                        Fp2::Fcvtps => mc::fcvtps,
                        Fp2::Fcvtpu => mc::fcvtpu,
                        Fp2::Fcvtzs => mc::fcvtzs,
                        Fp2::Fcvtzu => mc::fcvtzu,
                        _ => return unsupported(TARGET),
                    };
                    let w = f(gpr_size(ret), mc::RegZr::ZR, fsz, v0);
                    Plan::Gpr { word: if fmt == FpFormat::H { mc::fp_half(w) } else { w } }
                }
                _ => return unsupported(TARGET),
            }
        }
        Low::Fixed(op) => {
            let amount = immediate(cx)?;
            let to_int = matches!(op, ShiftImm::Fcvtzs | ShiftImm::Fcvtzu);
            let (fp, int) = if to_int { (k(0), ret) } else { (ret, k(0)) };
            let w = match (fp, int) {
                (T::Vec { bits, elem, fp: Some(_) }, T::Vec { fp: None, .. })
                    if arr(bits, elem).is_some() =>
                {
                    mc::try_neon_shift(op, bits, elem, v0, v0, amount)
                }
                (f, i) if f.fp_scalar().is_some() => match (f.fp_scalar(), i) {
                    (Some(FpFormat::S), T::Int(32)) => {
                        mc::try_neon_shift_scalar(op, Elem::S, v0, v0, amount)
                    }
                    (Some(FpFormat::D), T::Int(64) | T::Vec { .. }) => {
                        mc::try_neon_shift_scalar(op, Elem::D, v0, v0, amount)
                    }
                    // Half precision and a 32- or 64-bit integer: `llc`'s
                    // 16-bit fixed point in the `H` view (the integer's low
                    // 16 bits in, the result zero-extended out; WP-205).
                    (Some(FpFormat::H), T::Int(32 | 64)) => {
                        mc::try_neon_shift_scalar(op, Elem::H, v0, v0, amount)
                    }
                    _ => None,
                },
                _ => return unsupported(TARGET),
            };
            let Some(w) = w else { return unsupported(IMMEDIATE) };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::Insert(op) => {
            let amount = immediate(cx)?;
            let w = if let Some((bits, elem)) = ret.ivec().filter(|&(b, e)| arr(b, e).is_some()) {
                mc::try_neon_shift(op, bits, elem, v0, v0, amount)
            } else if ret.scalar() == Some(Elem::D) {
                mc::try_neon_shift_scalar(op, Elem::D, v0, v0, amount)
            } else {
                return unsupported(TARGET);
            };
            let Some(w) = w else { return unsupported(IMMEDIATE) };
            Plan::One { word: w, n: 1, m: None, a: None }
        }
        Low::ShiftNarrow(op) => {
            let amount = immediate(cx)?;
            let w = match (ret, k(0)) {
                (
                    T::Vec { bits: VecBits::V64, elem, fp: None },
                    T::Vec { bits: VecBits::V128, .. },
                ) => mc::try_neon_shift(op, VecBits::V64, elem, v0, v0, amount),
                (T::Int(32), T::Int(64)) => mc::try_neon_shift_scalar(op, Elem::S, v0, v0, amount),
                _ => return unsupported(TARGET),
            };
            let Some(w) = w else { return unsupported(IMMEDIATE) };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::ShiftSplat(op) => {
            let Some(amount) = imm_arg.and_then(|v| splat(cx, v)) else {
                return unsupported(IMMEDIATE);
            };
            let amount = u32::try_from(amount).unwrap_or(u32::MAX);
            let w = if let Some((bits, elem)) = ret.ivec().filter(|&(b, e)| arr(b, e).is_some()) {
                mc::try_neon_shift(op, bits, elem, v0, v0, amount)
            } else if let Some(elem) = ret.scalar() {
                mc::try_neon_shift_scalar(op, elem, v0, v0, amount)
            } else {
                return unsupported(TARGET);
            };
            let Some(w) = w else { return unsupported(IMMEDIATE) };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::AddLong(signed) => {
            let Some((bits, elem)) = k(0).ivec() else { return unsupported(TARGET) };
            let w = if bits == VecBits::V64 && elem == Elem::S {
                // Two words: the pairwise long add.
                if signed {
                    word(mc::try_neon_int2(Int2::Saddlp, bits, elem, v0, v0))?
                } else {
                    word(mc::try_uaddlp(Arr::S2, v0, v0))?
                }
            } else if signed {
                word(mc::try_saddlv(bits, elem, v0, v0))?
            } else {
                let Some(a) = arr(bits, elem) else { return unsupported(TARGET) };
                word(mc::try_uaddlv(a, v0, v0))?
            };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::Reduce(op) => {
            let Some((bits, elem)) = k(0).ivec() else { return unsupported(TARGET) };
            let w = match (bits, elem) {
                (VecBits::V128, Elem::D) if matches!(op, Reduce::Add) => mc::addp_scalar(v0, v0),
                (VecBits::V64, Elem::S) => {
                    let pair = match op {
                        Reduce::Add => Int3::Addp,
                        Reduce::Smax => Int3::Smaxp,
                        Reduce::Smin => Int3::Sminp,
                        Reduce::Umax => Int3::Umaxp,
                        Reduce::Umin => Int3::Uminp,
                    };
                    word(mc::try_neon_int3(pair, bits, elem, v0, v0, v0))?
                }
                _ => {
                    let Some(a) = arr(bits, elem) else { return unsupported(TARGET) };
                    let f = match op {
                        Reduce::Add => mc::try_addv,
                        Reduce::Smax => mc::try_smaxv,
                        Reduce::Smin => mc::try_sminv,
                        Reduce::Umax => mc::try_umaxv,
                        Reduce::Umin => mc::try_uminv,
                    };
                    word(f(a, v0, v0))?
                }
            };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::FpReduce(op) => {
            let Some((bits, fmt)) = k(0).fvec() else { return unsupported(TARGET) };
            let pair = match op {
                None => FpPair::Faddp,
                Some(FpAcross::Fmaxv) => FpPair::Fmaxp,
                Some(FpAcross::Fminv) => FpPair::Fminp,
                Some(FpAcross::Fmaxnmv) => FpPair::Fmaxnmp,
                Some(FpAcross::Fminnmv) => FpPair::Fminnmp,
            };
            match (bits, fmt, op) {
                // Two elements: the scalar pairwise form.
                (VecBits::V64, FpFormat::S, _) | (VecBits::V128, FpFormat::D, _) => {
                    Plan::One { word: mc::neon_fp_pair(pair, fmt, v0, v0), n: 0, m: None, a: None }
                }
                (_, _, Some(across)) => Plan::One {
                    word: word(mc::try_neon_fp_across(across, bits, fmt, v0, v0))?,
                    n: 0,
                    m: None,
                    a: None,
                },
                (VecBits::V128, FpFormat::S, None) => Plan::Two {
                    first: word(mc::try_neon_fp3(Fp3::Faddp, bits, fmt, v0, v0, v0))?,
                    second: mc::neon_fp_pair(FpPair::Faddp, fmt, v0, v0),
                },
                _ => return unsupported(TARGET),
            }
        }
        Low::Long3(op) => {
            let elem = if op.narrows() { ret.ivec() } else { k(0).ivec() };
            let Some((_, elem)) = elem else { return unsupported(TARGET) };
            one(word(mc::try_neon_long3(op, elem, v0, v0, v0))?)
        }
        Low::Pmull64 => one(word(mc::try_neon_long3(Long3::Pmull, Elem::D, v0, v0, v0))?),
        Low::SqdmullScalar => one(word(mc::try_sqdmull_scalar(Elem::S, v0, v0, v0))?),
        Low::Dot(op) => {
            let Some((bits, _)) = ret.vec() else { return unsupported(TARGET) };
            Plan::One {
                word: word(mc::try_neon_dot(op, bits, v0, v0, v0))?,
                n: 1,
                m: Some(2),
                a: None,
            }
        }
        Low::Cmla(rot) => {
            let Some((bits, fmt)) = ret.fvec() else { return unsupported(TARGET) };
            Plan::One {
                word: word(mc::try_fcmla(bits, fmt, v0, v0, v0, rot))?,
                n: 1,
                m: Some(2),
                a: None,
            }
        }
        Low::Cadd(rot270) => {
            let Some((bits, fmt)) = ret.fvec() else { return unsupported(TARGET) };
            one(word(mc::try_fcadd(bits, fmt, v0, v0, v0, rot270))?)
        }
        Low::Aes(op) => {
            let m = usize::from(matches!(op, Aes::Aese | Aes::Aesd));
            Plan::One { word: mc::aes(op, v0, v0), n: m, m: None, a: None }
        }
        Low::Sha3(op) => {
            if matches!(op, Sha3Reg::Rax1) {
                one(mc::sha3reg(op, v0, v0, v0))
            } else {
                Plan::One { word: mc::sha3reg(op, v0, v0, v0), n: 1, m: Some(2), a: None }
            }
        }
        Low::Sha2(op) => {
            let n = usize::from(!matches!(op, Sha2Reg::Sha1h));
            Plan::One { word: mc::sha2reg(op, v0, v0), n, m: None, a: None }
        }
        Low::Ternary(eor) => {
            let w = if eor { mc::eor3(v0, v0, v0, v0) } else { mc::bcax(v0, v0, v0, v0) };
            Plan::One { word: w, n: 0, m: Some(1), a: Some(2) }
        }
        Low::Xar => {
            let amount = immediate(cx)?;
            let Some(w) = mc::try_xar(v0, v0, v0, amount) else { return unsupported(IMMEDIATE) };
            one(w)
        }
        Low::Sm3(op) => {
            let w = mc::sm3reg(op, v0, v0, v0);
            if matches!(op, mc::Sm3Reg::Sm4ekey) {
                one(w)
            } else {
                Plan::One { word: w, n: 1, m: Some(2), a: None }
            }
        }
        Low::Sm3ss1 => Plan::One { word: mc::sm3ss1(v0, v0, v0, v0), n: 0, m: Some(1), a: Some(2) },
        Low::Sm3tt(op) => {
            let index = immediate(cx)?;
            let Some(w) = mc::try_sm3tt(op, v0, v0, v0, index) else {
                return unsupported(IMMEDIATE);
            };
            Plan::One { word: w, n: 1, m: Some(2), a: None }
        }
        Low::Sm4e => Plan::One { word: mc::sm4e(v0, v0), n: 1, m: None, a: None },
        Low::Jcvt => {
            if k(0).fp_scalar() != Some(FpFormat::D) || ret != T::Int(32) {
                return unsupported(TARGET);
            }
            Plan::Gpr { word: mc::fjcvtzs(mc::RegZr::ZR, v0) }
        }
        Low::Fcvtxn => {
            let w =
                if ret.vec().is_some() { mc::fcvtxn(v0, v0) } else { mc::fcvtxn_scalar(v0, v0) };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::HalfConvert(narrow) => {
            let w =
                if narrow { mc::fcvtn(FpSize::S, v0, v0) } else { mc::fcvtl(FpSize::S, v0, v0) };
            Plan::One { word: w, n: 0, m: None, a: None }
        }
        Low::Table(..)
        | Low::Multi(_)
        | Low::Lane(..)
        | Low::Replicate(_)
        | Low::Cls(_)
        | Low::System(_) => {
            return cx.bug("a memory or system family in plan");
        }
    })
}

// ---- consecutive registers: tables, structures ----

/// The block register `k` (`v24 + k`).
fn block(k: u32) -> VReg {
    VReg::new(BLOCK + u8::try_from(k).unwrap_or(0)).unwrap_or(V30)
}

/// Makes `v24`..`v24 + n - 1` temporaries, before anything else of the
/// instruction is placed (a deferred operand included).
fn take_block(cx: &mut FnCx<'_, A64>, n: u32) -> CgResult<()> {
    for k in 0..n {
        cx.take_reg(preg(block(k)))?;
    }
    Ok(())
}

/// Puts `values` into the block, in order.
fn put_block(cx: &mut FnCx<'_, A64>, values: &[Value]) -> CgResult<()> {
    for (k, &value) in (0u32..).zip(values) {
        cx.put(value, preg(block(k)))?;
    }
    Ok(())
}

/// The base register of a structure access.
fn base(cx: &mut FnCx<'_, A64>, ptr: Value) -> CgResult<mc::RegSp> {
    let p = cx.use_reg(ptr)?;
    Ok(xr(p).sp())
}

/// The vectors of a structure result type: `n` fields of one 64- or
/// 128-bit vector type.
fn fields_of(cx: &FnCx<'_, A64>, ty: TypeId, n: u32) -> Option<(VecBits, Elem)> {
    let fields = cx.types().struct_fields(ty)?;
    let first = *fields.first()?;
    if fields.len() != n as usize || fields.iter().any(|&f| f != first) {
        return None;
    }
    kind_of_as(cx, first, true).vec()
}

/// Stores the block's first `n` registers to the result's slot.
fn store_block<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, n: u32, bits: VecBits) -> CgResult<()> {
    let slot = cx.def_mem(inst)?;
    let bytes = bits.bits() / 8;
    let mut at = Cursor::frame(cx, slot, X16);
    for k in 0..n {
        store_fpr_at(cx, crate::common::fpr_view(bytes), block(k), &mut at);
        at.advance(bytes);
    }
    Ok(())
}

/// `tbl<n>`/`tbx<n>`: `(tables.., index)` or `(fallback, tables.., index)`.
fn table<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    extend: bool,
    n: u32,
    args: &'a [Value],
    ret: T,
) -> CgResult<()> {
    let first = usize::from(extend);
    let Some((bits, Elem::B)) = ret.ivec() else { return unsupported(TARGET) };
    if args.len() != first + n as usize + 1 {
        return unsupported(TARGET);
    }
    let tables = &args[first..first + n as usize];
    if tables.iter().any(|&t| value_kind(cx, t).ivec() != Some((VecBits::V128, Elem::B))) {
        return unsupported(TARGET);
    }
    let w = word(mc::try_tbl_n(extend, n, bits, VReg::V0, VReg::V0, VReg::V0))?;
    take_block(cx, n)?;
    deferred_first(cx, inst, None)?;
    put_block(cx, tables)?;
    let index = args[args.len() - 1];
    let regs = if extend { load(cx, &[args[0], index])? } else { load(cx, &[index])? };
    let (rd, rm) = (regs.v[0], regs.v[regs.v.len() - 1]);
    put(cx, fields(w, rd, block(0), Some(rm), None));
    done(cx, inst, &regs, rd)
}

/// The multiple-structure loads (`(ptr)`, a struct result) and stores
/// (`(vectors.., ptr)`).
fn multi<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: Multi,
    args: &'a [Value],
    kinds: &[T],
    ret: T,
) -> CgResult<()> {
    let n = op.registers();
    let load = matches!(
        op,
        Multi::Ld1x2 | Multi::Ld1x3 | Multi::Ld1x4 | Multi::Ld2 | Multi::Ld3 | Multi::Ld4
    );
    let shape = if load {
        if ret != T::Other || args.len() != 1 || kinds[0] != T::Ptr {
            return unsupported(TARGET);
        }
        fields_of(cx, inst.ty(), n)
    } else {
        if args.len() != n as usize + 1
            || kinds[n as usize] != T::Ptr
            || kinds[1..n as usize].iter().any(|&k| k != kinds[0])
        {
            return unsupported(TARGET);
        }
        kinds[0].vec()
    };
    let Some((bits, elem)) = shape else { return unsupported(TARGET) };
    // `<1 x i64>` structures do not interleave: `ld1`/`st1` of `n`.
    let op = match (op, bits, elem) {
        (Multi::Ld2, VecBits::V64, Elem::D) => Multi::Ld1x2,
        (Multi::Ld3, VecBits::V64, Elem::D) => Multi::Ld1x3,
        (Multi::Ld4, VecBits::V64, Elem::D) => Multi::Ld1x4,
        (Multi::St2, VecBits::V64, Elem::D) => Multi::St1x2,
        (Multi::St3, VecBits::V64, Elem::D) => Multi::St1x3,
        (Multi::St4, VecBits::V64, Elem::D) => Multi::St1x4,
        _ => op,
    };
    let w = word(mc::try_ldst_multi(op, bits, elem, VReg::V0, mc::RegSp::SP))?;
    take_block(cx, n)?;
    deferred_first(cx, inst, None)?;
    if !load {
        put_block(cx, &args[..n as usize])?;
    }
    let rn = base(cx, args[args.len() - 1])?;
    put(cx, (w & !0x3ff) | rn.code() << 5 | u32::from(block(0).index()));
    if load { store_block(cx, inst, n, bits) } else { Ok(()) }
}

/// `ld<n>lane` (`(vectors.., lane, ptr)`, a struct result) and
/// `st<n>lane` (`(vectors.., lane, ptr)`).
#[allow(clippy::too_many_arguments, reason = "the call's parts")]
fn lane<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    load: bool,
    n: u32,
    args: &'a [Value],
    kinds: &[T],
    ret: T,
) -> CgResult<()> {
    let n_us = n as usize;
    if args.len() != n_us + 2
        || kinds[n_us + 1] != T::Ptr
        || kinds[1..n_us].iter().any(|&k| k != kinds[0])
    {
        return unsupported(TARGET);
    }
    if load && (ret != T::Other || fields_of(cx, inst.ty(), n) != kinds[0].vec()) {
        return unsupported(TARGET);
    }
    if !load && ret != T::Void {
        return unsupported(TARGET);
    }
    let Some((bits, elem)) = kinds[0].vec() else { return unsupported(TARGET) };
    let index = imm(cx, args[n_us])?;
    let lanes = if bits == VecBits::V128 { elem.lanes() } else { elem.lanes() / 2 };
    if index >= lanes {
        return unsupported(IMMEDIATE);
    }
    let w = word(mc::try_ldst_lane(load, n, elem, index, VReg::V0, mc::RegSp::SP))?;
    take_block(cx, n)?;
    deferred_first(cx, inst, None)?;
    put_block(cx, &args[..n_us])?;
    let rn = base(cx, args[n_us + 1])?;
    put(cx, (w & !0x3ff) | rn.code() << 5 | u32::from(block(0).index()));
    if load { store_block(cx, inst, n, bits) } else { Ok(()) }
}

/// `ld<n>r`: `(ptr)`, a struct result.
fn replicate<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    n: u32,
    args: &'a [Value],
    ret: T,
) -> CgResult<()> {
    if ret != T::Other || args.len() != 1 || value_kind(cx, args[0]) != T::Ptr {
        return unsupported(TARGET);
    }
    let Some((bits, elem)) = fields_of(cx, inst.ty(), n) else { return unsupported(TARGET) };
    let w = word(mc::try_ld_rep(n, bits, elem, VReg::V0, mc::RegSp::SP))?;
    take_block(cx, n)?;
    deferred_first(cx, inst, None)?;
    let rn = base(cx, args[0])?;
    put(cx, (w & !0x3ff) | rn.code() << 5 | u32::from(block(0).index()));
    store_block(cx, inst, n, bits)
}

// ---- general registers and system ----

/// `llvm.aarch64.cls(i32)` and `cls64(i64)`, both `i32` results.
fn cls<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, wide: bool, args: &'a [Value]) -> CgResult<()> {
    let want = if wide { 64 } else { 32 };
    let (&[x], T::Int(32)) = (args, kind_of(cx, inst.ty())) else { return unsupported(TARGET) };
    if value_kind(cx, x) != T::Int(want) {
        return unsupported(TARGET);
    }
    deferred_first(cx, inst, None)?;
    let p = cx.use_reg(x)?;
    let sz = if wide { Size::X } else { Size::W };
    put(cx, mc::cls(sz, xr(p).zr(), xr(p).zr()));
    cx.finish(inst, p)
}

/// `dmb`, `dsb` (a constant option), `isb` (option 15 only), `hint`.
fn system<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, op: Sys, args: &'a [Value]) -> CgResult<()> {
    let &[x] = args else { return unsupported(TARGET) };
    let option = imm(cx, x)?;
    let w = match op {
        Sys::Dmb | Sys::Dsb => {
            let Some(&b) = Barrier::ALL.iter().find(|&&b| b as u32 == option) else {
                return unsupported(IMMEDIATE);
            };
            if matches!(op, Sys::Dmb) { mc::dmb(b) } else { mc::dsb(b) }
        }
        Sys::Isb if option == 15 => mc::isb(),
        Sys::Isb => return unsupported(IMMEDIATE),
        Sys::Hint => match mc::try_hint(option) {
            Some(w) => w,
            None => return unsupported(IMMEDIATE),
        },
    };
    deferred_first(cx, inst, None)?;
    put(cx, w);
    Ok(())
}
