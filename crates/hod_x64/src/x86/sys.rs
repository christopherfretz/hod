//! The x86 system and state intrinsics (`spec/codegen.md` §5.6.29, WP-190):
//! the `llvm.x86.*` names clang 19 and 22 emit at `-O0` for the functions
//! of the non-vector headers (`<ia32intrin.h>`, `<fxsrintrin.h>`,
//! `<xsaveintrin.h>` and its variants, `<immintrin.h>`'s `_rdrand*`,
//! `<adcintrin.h>`, `<bmiintrin.h>`, `<bmi2intrin.h>`, `<crc32intrin.h>`,
//! `<rtmintrin.h>`, ...), each as `llc -O0` writes it.
//!
//! **Owner: WP-190.** Tests: `tests/x86_intrinsics.rs`.
//!
//! - **Baseline** (no feature check): `flags.read.u64`/`flags.write.u64`
//!   are `pushfq; pop r` and `push r; popfq` (no red zone: the eight bytes
//!   below `rsp` are free; the CFA is `rbp + 16` throughout, so the unwind
//!   rows do not change); `rdtsc`, `rdtscp`, `rdpmc` combine `edx:eax`;
//!   `addcarry`/`subborrow` turn the carry byte into CF (`add cl, -1`) and
//!   `adc`/`sbb`.
//! - **Feature-gated**: every other intrinsic is the instruction itself,
//!   only in a function whose `target-features` enable the feature clang
//!   requires for its header function ([`Feature`]); otherwise refused by
//!   its own name (`intrinsic:llvm.x86...`). `llc` 22 selects some of them
//!   without the feature (`crc32`, `xgetbv`, `xsetbv`, the BMI ones,
//!   `rdssp`, `incssp`, `rdpkru`); hod does not write an instruction the
//!   function does not declare.

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg};
use hod_ir::{FnStr, Inst, Intrinsic, SymbolKind, TypeId, TypeKind, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Bmi, Cond, Mem, Reg, SegBase, Size, XState};

use crate::common::{deferred_first, frame_inst, gp, put, unsupported, value_ty};
use crate::intrinsic::const_int;
use crate::target::X64;

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// A CPU feature an intrinsic of this module needs, by its LLVM
/// `target-features` name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    Fxsr,
    Xsave,
    Xsaveopt,
    Xsavec,
    Xsaves,
    Rdrnd,
    Rdseed,
    Clflushopt,
    Clwb,
    Bmi,
    Bmi2,
    Crc32,
    Fsgsbase,
    Rtm,
    Rdpid,
    Pku,
    Serialize,
    Shstk,
}

impl Feature {
    /// The `target-features` name.
    pub const fn name(self) -> &'static str {
        match self {
            Feature::Fxsr => "fxsr",
            Feature::Xsave => "xsave",
            Feature::Xsaveopt => "xsaveopt",
            Feature::Xsavec => "xsavec",
            Feature::Xsaves => "xsaves",
            Feature::Rdrnd => "rdrnd",
            Feature::Rdseed => "rdseed",
            Feature::Clflushopt => "clflushopt",
            Feature::Clwb => "clwb",
            Feature::Bmi => "bmi",
            Feature::Bmi2 => "bmi2",
            Feature::Crc32 => "crc32",
            Feature::Fsgsbase => "fsgsbase",
            Feature::Rtm => "rtm",
            Feature::Rdpid => "rdpid",
            Feature::Pku => "pku",
            Feature::Serialize => "serialize",
            Feature::Shstk => "shstk",
        }
    }

    /// Whether item `name`, turned on, turns this feature on: itself, a
    /// feature that implies it in LLVM (`xsaveopt`, `xsavec`, `xsaves`
    /// imply `xsave`; `sse4.2` and every later SSE or AVX level imply
    /// `crc32`).
    fn implied_by(self, name: &[u8]) -> bool {
        name == self.name().as_bytes()
            || match self {
                Feature::Xsave => matches!(name, b"xsaveopt" | b"xsavec" | b"xsaves"),
                Feature::Crc32 => super::implies(name, SSE42_RANK),
                _ => false,
            }
    }

    /// Whether item `name`, turned off, turns this feature off: itself or
    /// a feature it needs (`xsave` for the three that imply it).
    fn disabled_by(self, name: &[u8]) -> bool {
        name == self.name().as_bytes()
            || (matches!(self, Feature::Xsaveopt | Feature::Xsavec | Feature::Xsaves)
                && name == b"xsave")
    }
}

/// `sse4.2`'s position in WP-173's chain of SSE levels.
const SSE42_RANK: usize = 3;

/// Whether the function's `target-features` enable `feature`: the last
/// item that turns it on or off decides; without one it is off (as for
/// WP-173's levels, a `target-cpu` alone does not count).
pub fn has(cx: &FnCx<'_, X64>, feature: Feature) -> bool {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return false;
    };
    let Some(features) = cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) else {
        return false;
    };
    let mut on = false;
    for item in features.split(|&b| b == b',') {
        match item.split_first() {
            Some((b'+', name)) if feature.implied_by(name) => on = true,
            Some((b'-', name)) if feature.disabled_by(name) => on = false,
            _ => {}
        }
    }
    on
}

/// The shapes of the operands and results.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    /// An integer of this many bits.
    I(u32),
    /// `ptr`.
    P,
    /// `{ iA, iB }`.
    Pair(u32, u32),
    /// No result.
    Void,
}

/// What a lowering does.
#[derive(Clone, Copy, Debug)]
enum Op {
    FlagsRead,
    FlagsWrite,
    /// A state save or restore at the pointer; `masked`: with the
    /// component mask (high, low) in `edx:eax`.
    State(XState, bool),
    /// `rdtsc` (`None`), or `rdpmc`/`xgetbv` of the `ecx` operand: the
    /// 64-bit `edx:eax`.
    Counter(Option<fn() -> mc::Inst>),
    Rdtscp,
    /// `addcarry` (`false`) or `subborrow` of `size`.
    Carry(bool, Size),
    Xsetbv,
    /// `rdrand` (`false`) or `rdseed` of `size`.
    Random(bool, Size),
    /// A cache-line operation at the pointer.
    Line(fn(Mem) -> mc::Inst),
    Bmi(Bmi, Size),
    /// `crc32` of a source of `size`.
    Crc32(Size),
    SegRead(SegBase, Size),
    SegWrite(SegBase, Size),
    Xbegin,
    /// One instruction with no operand.
    Fixed(fn() -> mc::Inst),
    Xabort,
    Xtest,
    Rdpid,
    Rdpkru,
    Wrpkru,
    Incssp(Size),
    Rdssp(Size),
}

/// The signature, the feature and the lowering of `which`, if it is one
/// of this module's.
#[allow(clippy::too_many_lines, reason = "one row per intrinsic")]
fn row(which: Intrinsic<'_>) -> Option<(K, &'static [K], Option<Feature>, Op)> {
    use Feature as F;
    use Intrinsic as I;
    use K::{I as Int, P, Pair, Void};
    const NONE: &[K] = &[];
    const PTR: &[K] = &[P];
    const MASKED: &[K] = &[P, Int(32), Int(32)];
    const I32: &[K] = &[Int(32)];
    const I64: &[K] = &[Int(64)];
    let state = |op, f| (Void, PTR, Some(f), Op::State(op, false));
    let masked = |op, f| (Void, MASKED, Some(f), Op::State(op, true));
    let random = |seed, bits, size, f| (Pair(bits, 32), NONE, Some(f), Op::Random(seed, size));
    let bmi = |op, f, bits, size| {
        (
            Int(bits),
            if bits == 32 { &[Int(32), Int(32)][..] } else { &[Int(64), Int(64)][..] },
            Some(f),
            Op::Bmi(op, size),
        )
    };
    let (d, q) = (Size::D, Size::Q);
    Some(match which {
        I::X86FlagsReadU64 => (Int(64), NONE, None, Op::FlagsRead),
        I::X86FlagsWriteU64 => (Void, I64, None, Op::FlagsWrite),
        I::X86Fxsave => state(XState::Fxsave, F::Fxsr),
        I::X86Fxsave64 => state(XState::Fxsave64, F::Fxsr),
        I::X86Fxrstor => state(XState::Fxrstor, F::Fxsr),
        I::X86Fxrstor64 => state(XState::Fxrstor64, F::Fxsr),
        I::X86Rdtsc => (Int(64), NONE, None, Op::Counter(None)),
        I::X86Rdtscp => (Pair(64, 32), NONE, None, Op::Rdtscp),
        I::X86Rdpmc => (Int(64), I32, None, Op::Counter(Some(mc::rdpmc))),
        I::X86Addcarry32 => (Pair(8, 32), &[Int(8), Int(32), Int(32)], None, Op::Carry(false, d)),
        I::X86Addcarry64 => (Pair(8, 64), &[Int(8), Int(64), Int(64)], None, Op::Carry(false, q)),
        I::X86Subborrow32 => (Pair(8, 32), &[Int(8), Int(32), Int(32)], None, Op::Carry(true, d)),
        I::X86Subborrow64 => (Pair(8, 64), &[Int(8), Int(64), Int(64)], None, Op::Carry(true, q)),
        I::X86Xgetbv => (Int(64), I32, Some(F::Xsave), Op::Counter(Some(mc::xgetbv))),
        I::X86Xsetbv => (Void, &[Int(32), Int(32), Int(32)], Some(F::Xsave), Op::Xsetbv),
        I::X86Rdrand16 => random(false, 16, Size::W, F::Rdrnd),
        I::X86Rdrand32 => random(false, 32, d, F::Rdrnd),
        I::X86Rdrand64 => random(false, 64, q, F::Rdrnd),
        I::X86Rdseed16 => random(true, 16, Size::W, F::Rdseed),
        I::X86Rdseed32 => random(true, 32, d, F::Rdseed),
        I::X86Rdseed64 => random(true, 64, q, F::Rdseed),
        I::X86Xsave => masked(XState::Xsave, F::Xsave),
        I::X86Xsave64 => masked(XState::Xsave64, F::Xsave),
        I::X86Xrstor => masked(XState::Xrstor, F::Xsave),
        I::X86Xrstor64 => masked(XState::Xrstor64, F::Xsave),
        I::X86Xsaveopt => masked(XState::Xsaveopt, F::Xsaveopt),
        I::X86Xsaveopt64 => masked(XState::Xsaveopt64, F::Xsaveopt),
        I::X86Xsavec => masked(XState::Xsavec, F::Xsavec),
        I::X86Xsavec64 => masked(XState::Xsavec64, F::Xsavec),
        I::X86Xsaves => masked(XState::Xsaves, F::Xsaves),
        I::X86Xsaves64 => masked(XState::Xsaves64, F::Xsaves),
        I::X86Xrstors => masked(XState::Xrstors, F::Xsaves),
        I::X86Xrstors64 => masked(XState::Xrstors64, F::Xsaves),
        I::X86Clflushopt => (Void, PTR, Some(F::Clflushopt), Op::Line(mc::clflushopt_m)),
        I::X86Clwb => (Void, PTR, Some(F::Clwb), Op::Line(mc::clwb_m)),
        I::X86BmiBextr32 => bmi(Bmi::Bextr, F::Bmi, 32, d),
        I::X86BmiBextr64 => bmi(Bmi::Bextr, F::Bmi, 64, q),
        I::X86BmiBzhi32 => bmi(Bmi::Bzhi, F::Bmi2, 32, d),
        I::X86BmiBzhi64 => bmi(Bmi::Bzhi, F::Bmi2, 64, q),
        I::X86BmiPdep32 => bmi(Bmi::Pdep, F::Bmi2, 32, d),
        I::X86BmiPdep64 => bmi(Bmi::Pdep, F::Bmi2, 64, q),
        I::X86BmiPext32 => bmi(Bmi::Pext, F::Bmi2, 32, d),
        I::X86BmiPext64 => bmi(Bmi::Pext, F::Bmi2, 64, q),
        I::X86Sse42Crc32x8 => (Int(32), &[Int(32), Int(8)], Some(F::Crc32), Op::Crc32(Size::B)),
        I::X86Sse42Crc32x16 => (Int(32), &[Int(32), Int(16)], Some(F::Crc32), Op::Crc32(Size::W)),
        I::X86Sse42Crc32x32 => (Int(32), &[Int(32), Int(32)], Some(F::Crc32), Op::Crc32(d)),
        I::X86Sse42Crc64x64 => (Int(64), &[Int(64), Int(64)], Some(F::Crc32), Op::Crc32(q)),
        I::X86Rdfsbase32 => (Int(32), NONE, Some(F::Fsgsbase), Op::SegRead(SegBase::RdFs, d)),
        I::X86Rdfsbase64 => (Int(64), NONE, Some(F::Fsgsbase), Op::SegRead(SegBase::RdFs, q)),
        I::X86Rdgsbase32 => (Int(32), NONE, Some(F::Fsgsbase), Op::SegRead(SegBase::RdGs, d)),
        I::X86Rdgsbase64 => (Int(64), NONE, Some(F::Fsgsbase), Op::SegRead(SegBase::RdGs, q)),
        I::X86Wrfsbase32 => (Void, I32, Some(F::Fsgsbase), Op::SegWrite(SegBase::WrFs, d)),
        I::X86Wrfsbase64 => (Void, I64, Some(F::Fsgsbase), Op::SegWrite(SegBase::WrFs, q)),
        I::X86Wrgsbase32 => (Void, I32, Some(F::Fsgsbase), Op::SegWrite(SegBase::WrGs, d)),
        I::X86Wrgsbase64 => (Void, I64, Some(F::Fsgsbase), Op::SegWrite(SegBase::WrGs, q)),
        I::X86Xbegin => (Int(32), NONE, Some(F::Rtm), Op::Xbegin),
        I::X86Xend => (Void, NONE, Some(F::Rtm), Op::Fixed(mc::xend)),
        I::X86Xabort => (Void, &[Int(8)], Some(F::Rtm), Op::Xabort),
        I::X86Xtest => (Int(32), NONE, Some(F::Rtm), Op::Xtest),
        I::X86Rdpid => (Int(32), NONE, Some(F::Rdpid), Op::Rdpid),
        I::X86Rdpkru => (Int(32), NONE, Some(F::Pku), Op::Rdpkru),
        I::X86Wrpkru => (Void, I32, Some(F::Pku), Op::Wrpkru),
        I::X86Serialize => (Void, NONE, Some(F::Serialize), Op::Fixed(mc::serialize)),
        I::X86Incsspd => (Void, I32, Some(F::Shstk), Op::Incssp(d)),
        I::X86Incsspq => (Void, I64, Some(F::Shstk), Op::Incssp(q)),
        I::X86Rdsspd => (Int(32), I32, Some(F::Shstk), Op::Rdssp(d)),
        I::X86Rdsspq => (Int(64), I64, Some(F::Shstk), Op::Rdssp(q)),
        _ => return None,
    })
}

/// The feature `which` needs, if it is one of this module's and needs
/// one.
pub fn feature(which: Intrinsic<'_>) -> Option<Feature> {
    row(which).and_then(|(_, _, f, _)| f)
}

/// The stable name `which` is refused with in a function whose features
/// do not enable it: `intrinsic:` and its LLVM name.
pub fn refusal(which: Intrinsic<'_>) -> &'static str {
    use Intrinsic as I;
    match which {
        I::X86Fxsave => "intrinsic:llvm.x86.fxsave",
        I::X86Fxsave64 => "intrinsic:llvm.x86.fxsave64",
        I::X86Fxrstor => "intrinsic:llvm.x86.fxrstor",
        I::X86Fxrstor64 => "intrinsic:llvm.x86.fxrstor64",
        I::X86Xgetbv => "intrinsic:llvm.x86.xgetbv",
        I::X86Xsetbv => "intrinsic:llvm.x86.xsetbv",
        I::X86Rdrand16 => "intrinsic:llvm.x86.rdrand.16",
        I::X86Rdrand32 => "intrinsic:llvm.x86.rdrand.32",
        I::X86Rdrand64 => "intrinsic:llvm.x86.rdrand.64",
        I::X86Rdseed16 => "intrinsic:llvm.x86.rdseed.16",
        I::X86Rdseed32 => "intrinsic:llvm.x86.rdseed.32",
        I::X86Rdseed64 => "intrinsic:llvm.x86.rdseed.64",
        I::X86Xsave => "intrinsic:llvm.x86.xsave",
        I::X86Xsave64 => "intrinsic:llvm.x86.xsave64",
        I::X86Xrstor => "intrinsic:llvm.x86.xrstor",
        I::X86Xrstor64 => "intrinsic:llvm.x86.xrstor64",
        I::X86Xsaveopt => "intrinsic:llvm.x86.xsaveopt",
        I::X86Xsaveopt64 => "intrinsic:llvm.x86.xsaveopt64",
        I::X86Xsavec => "intrinsic:llvm.x86.xsavec",
        I::X86Xsavec64 => "intrinsic:llvm.x86.xsavec64",
        I::X86Xsaves => "intrinsic:llvm.x86.xsaves",
        I::X86Xsaves64 => "intrinsic:llvm.x86.xsaves64",
        I::X86Xrstors => "intrinsic:llvm.x86.xrstors",
        I::X86Xrstors64 => "intrinsic:llvm.x86.xrstors64",
        I::X86Clflushopt => "intrinsic:llvm.x86.clflushopt",
        I::X86Clwb => "intrinsic:llvm.x86.clwb",
        I::X86BmiBextr32 => "intrinsic:llvm.x86.bmi.bextr.32",
        I::X86BmiBextr64 => "intrinsic:llvm.x86.bmi.bextr.64",
        I::X86BmiBzhi32 => "intrinsic:llvm.x86.bmi.bzhi.32",
        I::X86BmiBzhi64 => "intrinsic:llvm.x86.bmi.bzhi.64",
        I::X86BmiPdep32 => "intrinsic:llvm.x86.bmi.pdep.32",
        I::X86BmiPdep64 => "intrinsic:llvm.x86.bmi.pdep.64",
        I::X86BmiPext32 => "intrinsic:llvm.x86.bmi.pext.32",
        I::X86BmiPext64 => "intrinsic:llvm.x86.bmi.pext.64",
        I::X86Sse42Crc32x8 => "intrinsic:llvm.x86.sse42.crc32.32.8",
        I::X86Sse42Crc32x16 => "intrinsic:llvm.x86.sse42.crc32.32.16",
        I::X86Sse42Crc32x32 => "intrinsic:llvm.x86.sse42.crc32.32.32",
        I::X86Sse42Crc64x64 => "intrinsic:llvm.x86.sse42.crc32.64.64",
        I::X86Rdfsbase32 => "intrinsic:llvm.x86.rdfsbase.32",
        I::X86Rdfsbase64 => "intrinsic:llvm.x86.rdfsbase.64",
        I::X86Rdgsbase32 => "intrinsic:llvm.x86.rdgsbase.32",
        I::X86Rdgsbase64 => "intrinsic:llvm.x86.rdgsbase.64",
        I::X86Wrfsbase32 => "intrinsic:llvm.x86.wrfsbase.32",
        I::X86Wrfsbase64 => "intrinsic:llvm.x86.wrfsbase.64",
        I::X86Wrgsbase32 => "intrinsic:llvm.x86.wrgsbase.32",
        I::X86Wrgsbase64 => "intrinsic:llvm.x86.wrgsbase.64",
        I::X86Xbegin => "intrinsic:llvm.x86.xbegin",
        I::X86Xend => "intrinsic:llvm.x86.xend",
        I::X86Xabort => "intrinsic:llvm.x86.xabort",
        I::X86Xtest => "intrinsic:llvm.x86.xtest",
        I::X86Rdpid => "intrinsic:llvm.x86.rdpid",
        I::X86Rdpkru => "intrinsic:llvm.x86.rdpkru",
        I::X86Wrpkru => "intrinsic:llvm.x86.wrpkru",
        I::X86Serialize => "intrinsic:llvm.x86.serialize",
        I::X86Incsspd => "intrinsic:llvm.x86.incsspd",
        I::X86Incsspq => "intrinsic:llvm.x86.incsspq",
        I::X86Rdsspd => "intrinsic:llvm.x86.rdsspd",
        I::X86Rdsspq => "intrinsic:llvm.x86.rdsspq",
        _ => super::SIGNATURE,
    }
}

/// Whether `ty` is what `k` says.
fn fits(cx: &FnCx<'_, X64>, ty: TypeId, k: K) -> bool {
    let types = cx.types();
    match k {
        K::I(bits) => types.int_bits(ty) == Some(bits),
        K::P => matches!(types.kind(ty), TypeKind::Ptr { .. }),
        K::Void => matches!(types.kind(ty), TypeKind::Void),
        K::Pair(lo, hi) => types.struct_fields(ty).is_some_and(|fields| {
            matches!(fields, [first, second]
                if types.int_bits(*first) == Some(lo) && types.int_bits(*second) == Some(hi))
        }),
    }
}

/// A call of `which` with `args`, if `which` is an intrinsic of this
/// module.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> Option<CgResult<()>> {
    let (ret, params, feature, op) = row(which)?;
    Some(lower(cx, inst, which, args, (ret, params, feature), op))
}

/// The registers of `args` (one register for a value given twice: a
/// register value may be asked for once).
fn regs<const N: usize>(cx: &mut FnCx<'_, X64>, args: &[Value]) -> CgResult<[Reg; N]> {
    let mut out = [RAX; N];
    let mut got: [Option<PReg>; N] = [None; N];
    for i in 0..N {
        let reg = match (0..i).find(|&j| args[j] == args[i]) {
            Some(j) => got[j],
            None => Some(cx.use_reg(args[i])?),
        };
        got[i] = reg;
        if let Some(reg) = reg {
            out[i] = gp(reg);
        }
    }
    Ok(out)
}

/// `rax = rdx << 32 | eax`: the 64-bit result of a counter read.
fn combine(cx: &mut FnCx<'_, X64>) {
    // The instructions zero bits 32 to 63 of both.
    put(cx, mc::shl_ri(Size::Q, RDX, 32));
    put(cx, mc::or_rr(Size::Q, RAX, RDX));
}

/// Stores the low `size` of `rs` at `off` of `slot`.
fn store(cx: &mut FnCx<'_, X64>, slot: FrameRef, off: u64, size: Size, rs: Reg) -> CgResult<()> {
    let Ok(off) = u32::try_from(off) else {
        return cx.bug("a field offset of a two-integer struct beyond 4 GiB");
    };
    frame_inst(cx, slot.offset_by(off), |m| mc::mov_mr(size, m, rs));
    Ok(())
}

#[allow(clippy::too_many_lines, reason = "one arm per kind of lowering")]
fn lower<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
    (ret, params, feature): (K, &[K], Option<Feature>),
    op: Op,
) -> CgResult<()> {
    if args.len() != params.len()
        || !fits(cx, inst.ty(), ret)
        || args.iter().zip(params).any(|(&a, &k)| !fits(cx, value_ty(cx, a), k))
        || args.iter().any(|&a| cx.class_of(a) != Class::Gpr)
    {
        return unsupported(super::SIGNATURE);
    }
    if let Some(feature) = feature
        && !has(cx, feature)
    {
        return unsupported(refusal(which));
    }
    deferred_first(cx, inst, None)?;
    let second = |cx: &FnCx<'a, X64>| cx.types().field_offset(inst.ty(), 1);
    match op {
        Op::FlagsRead => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::pushfq());
            put(cx, mc::pop_r(gp(rd)));
            cx.finish(inst, rd)
        }
        Op::FlagsWrite => {
            let [r] = regs::<1>(cx, args)?;
            put(cx, mc::push_r(r));
            put(cx, mc::popfq());
            Ok(())
        }
        Op::State(op, masked) => {
            let p = if masked {
                let [p, hi, lo] = regs::<3>(cx, args)?;
                put(cx, mc::mov_rr(Size::D, RDX, hi));
                put(cx, mc::mov_rr(Size::D, RAX, lo));
                p
            } else {
                regs::<1>(cx, args)?[0]
            };
            put(cx, mc::xstate_m(op, Mem::base(p, 0)));
            Ok(())
        }
        Op::Counter(read) => {
            if let Some(read) = read {
                let [c] = regs::<1>(cx, args)?;
                put(cx, mc::mov_rr(Size::D, RCX, c));
                put(cx, read());
            } else {
                put(cx, mc::rdtsc());
            }
            combine(cx);
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::Q, gp(rd), RAX));
            cx.finish(inst, rd)
        }
        Op::Rdtscp => {
            let slot = cx.def_mem(inst)?;
            put(cx, mc::rdtscp());
            combine(cx);
            store(cx, slot, 0, Size::Q, RAX)?;
            store(cx, slot, second(cx), Size::D, RCX)
        }
        Op::Carry(sub, size) => {
            let [c, a, b] = regs::<3>(cx, args)?;
            let slot = cx.def_mem(inst)?;
            put(cx, mc::mov_rr(size, RAX, a));
            put(cx, mc::mov_rr(Size::D, RCX, c));
            // CF = the carry byte is not 0.
            put(cx, mc::add_ri(Size::B, RCX, -1));
            put(cx, if sub { mc::sbb_rr(size, RAX, b) } else { mc::adc_rr(size, RAX, b) });
            put(cx, mc::setcc_r(Cond::B, RCX));
            store(cx, slot, second(cx), size, RAX)?;
            store(cx, slot, 0, Size::B, RCX)
        }
        Op::Xsetbv => {
            let [c, hi, lo] = regs::<3>(cx, args)?;
            put(cx, mc::mov_rr(Size::D, RCX, c));
            put(cx, mc::mov_rr(Size::D, RDX, hi));
            put(cx, mc::mov_rr(Size::D, RAX, lo));
            put(cx, mc::xsetbv());
            Ok(())
        }
        Op::Random(seed, size) => {
            let slot = cx.def_mem(inst)?;
            put(cx, if seed { mc::rdseed_r(size, RAX) } else { mc::rdrand_r(size, RAX) });
            // 1 when a number was delivered; the hardware zeroes the
            // destination otherwise, as `llc`'s `cmovb` relies on.
            put(cx, mc::setcc_r(Cond::B, RCX));
            put(cx, mc::movzx_rr(Size::D, RCX, Size::B, RCX));
            store(cx, slot, 0, size, RAX)?;
            store(cx, slot, second(cx), Size::D, RCX)
        }
        Op::Line(make) => {
            let [p] = regs::<1>(cx, args)?;
            put(cx, make(Mem::base(p, 0)));
            Ok(())
        }
        Op::Bmi(op, size) => {
            let [x, y] = regs::<2>(cx, args)?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::bmi_rrr(op, size, gp(rd), x, y));
            cx.finish(inst, rd)
        }
        Op::Crc32(size) => {
            let ra = cx.use_reg(args[0])?;
            let b = if args[1] == args[0] { gp(ra) } else { gp(cx.use_reg(args[1])?) };
            put(cx, mc::crc32_rr(size, gp(ra), b));
            cx.finish(inst, ra)
        }
        Op::SegRead(op, size) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::segbase_r(op, size, gp(rd)));
            cx.finish(inst, rd)
        }
        Op::SegWrite(op, size) => {
            let [r] = regs::<1>(cx, args)?;
            put(cx, mc::segbase_r(op, size, r));
            Ok(())
        }
        Op::Xbegin => {
            let rd = cx.def_reg(inst)?;
            // On an abort execution resumes after `xbegin` (displacement
            // 0) with the status in `eax`; on the start `eax` keeps -1.
            put(cx, mc::mov_ri(Size::D, RAX, -1));
            put(cx, mc::xbegin(0));
            put(cx, mc::mov_rr(Size::D, gp(rd), RAX));
            cx.finish(inst, rd)
        }
        Op::Fixed(make) => {
            put(cx, make());
            Ok(())
        }
        Op::Xabort => {
            let Some(imm) = const_int(cx, args[0]) else {
                return cx.bug("xabort of an operand that is not a constant");
            };
            put(cx, mc::xabort(imm as u8));
            Ok(())
        }
        Op::Xtest => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::xtest());
            put(cx, mc::setcc_r(Cond::Ne, RAX));
            put(cx, mc::movzx_rr(Size::D, gp(rd), Size::B, RAX));
            cx.finish(inst, rd)
        }
        Op::Rdpid => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::rdpid_r(gp(rd)));
            cx.finish(inst, rd)
        }
        Op::Rdpkru => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::xor_rr(Size::D, RCX, RCX));
            put(cx, mc::rdpkru());
            put(cx, mc::mov_rr(Size::D, gp(rd), RAX));
            cx.finish(inst, rd)
        }
        Op::Wrpkru => {
            let [r] = regs::<1>(cx, args)?;
            put(cx, mc::mov_rr(Size::D, RAX, r));
            put(cx, mc::xor_rr(Size::D, RCX, RCX));
            put(cx, mc::xor_rr(Size::D, RDX, RDX));
            put(cx, mc::wrpkru());
            Ok(())
        }
        Op::Incssp(size) => {
            let [r] = regs::<1>(cx, args)?;
            put(cx, mc::incssp_r(size, r));
            Ok(())
        }
        Op::Rdssp(size) => {
            // A `nop` without shadow stacks: the result is the operand.
            let ra = cx.use_reg(args[0])?;
            put(cx, mc::rdssp_r(size, gp(ra)));
            cx.finish(inst, ra)
        }
    }
}
