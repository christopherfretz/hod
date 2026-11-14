//! The x86 target intrinsics (`llvm.x86.*`, `spec/codegen.md` §5.6.22,
//! "x86 target intrinsics (WP-173)"): the names clang 19 and 22 emit at
//! `-O0` for the functions of `<xmmintrin.h>`, `<emmintrin.h>`,
//! `<pmmintrin.h>` and `<tmmintrin.h>` that do not lower to generic IR.
//!
//! **Owner: WP-173** (the WP-181 rows: MXCSR, `clflush`, the
//! approximations, `maskmovdqu`). Tests: `tests/x86_intrinsics.rs`.
//!
//! - **SSE and SSE2** (the x86-64 baseline, no feature check): one
//!   instruction each, as `llc -O0` writes it: `maxps`/`minps`/`maxss`/...
//!   (the hardware's NaN and signed-zero rule: the second operand),
//!   `cmpss`/`cmpsd` with the predicate's low three bits (the legacy
//!   encoding reads only those, as `llc`'s does), `comiss`/`ucomiss`/
//!   `comisd`/`ucomisd` and the flags of the predicate (`lt` and `le`
//!   swap the operands and read `a`/`ae`; `eq` is `e` and `np`, `neq`
//!   `ne` or `p`), the conversions (`cvtss2si`, `cvttsd2si`, ... and the
//!   packed ones), the mask extractions, `pack*`, `pavg*`, `pmaddwd`,
//!   `pmulh*`, `psadbw`, the shifts by a vector count (its low 64 bits)
//!   and by an `i32` count (a constant as `imm8`, saturated at 255,
//!   which every count at or above the lane width equals; a variable one
//!   through `movd xmm15`), and the fences. WP-181: `ldmxcsr`/`stmxcsr`
//!   and `clflush` on the pointer operand's memory, `rcpps`/`rcpss`/
//!   `rsqrtps`/`rsqrtss` in place (the emulator's approximation, as for
//!   `llc`'s), `maskmovdqu` with the address moved to `rdi` (kept in
//!   `rax` around it).
//! - **SSE3** (only where the function's features enable `sse3`): built
//!   from SSE2 instructions, lane for lane the same operations:
//!   `hadd`/`hsub` gather the even and the odd lanes (`shufps`,
//!   `unpcklpd`/`unpckhpd`) and add or subtract them, the even lane the
//!   first operand; `addsub` computes both and keeps the even lanes of
//!   the difference; `ldu.dq` is `movdqu`.
//! - **SSSE3** (only with `ssse3`): lane by lane in general registers on
//!   frame images of the operands (`rax`, `rcx`, `rdx`, `r11`), the
//!   result's image loaded back with `movups`.
//!
//! An intrinsic above the baseline in a function without its feature is
//! refused by its own name (`intrinsic:llvm.x86...`), as `llc` refuses to
//! select it. Every other `llvm.x86.*` name is not on `hod_ir`'s list and
//! `classify` refuses it by its name; AArch64 refuses all of them
//! (`never`). hod writes only SSE2 instructions here, whatever the
//! features (§5.6.2), but for `cmp.ss`/`cmp.sd` with a predicate above 7
//! in a function with AVX (`vcmpss`/`vcmpsd`, WP-202).
//!
//! The vector intrinsics past SSSE3 (SSE4.1 to AVX2, AES, PCLMUL, SHA,
//! FMA, F16C, GFNI; §5.6.33, WP-202) are in [`ext`].
//!
//! The system and state intrinsics (`<fxsrintrin.h>`, `<ia32intrin.h>`,
//! `<xsave*intrin.h>`, `<bmi*intrin.h>`, `<crc32intrin.h>`, ...;
//! §5.6.29, WP-190) are in [`sys`].

pub mod ext;
pub mod sys;

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg};
use hod_ir::{FnStr, Inst, Intrinsic, SymbolKind, TypeKind, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpCmp, FpSize, Mem, Reg, Scale, Size, Xmm};

use crate::common::{
    R11, XMM14, XMM15, deferred_first, frame_address, frame_inst, gp, load_fpr, put, store_fpr,
    unsupported, value_ty, xm,
};
use crate::intrinsic::const_int;
use crate::target::X64;

/// A call of an `llvm.x86.*` intrinsic whose operand or result types are
/// not the ones LLVM declares it with.
pub const SIGNATURE: &str = "x64:x86-intrinsic-signature";

/// Every stable name this module refuses with, besides the intrinsics'
/// own names ([`refusal`]).
pub const UNSUPPORTED: &[&str] = &[SIGNATURE];

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// The instruction-set extension an intrinsic needs beyond SSE2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// SSE3.
    Sse3,
    /// SSSE3.
    Ssse3,
    /// AVX (WP-202: the five-bit compare predicates).
    Avx,
}

impl Level {
    /// The `target-features` name.
    pub const fn feature(self) -> &'static str {
        match self {
            Level::Sse3 => "sse3",
            Level::Ssse3 => "ssse3",
            Level::Avx => "avx",
        }
    }

    /// Its position in the chain `sse3` < `ssse3` < `sse4.1` < `sse4.2`
    /// < `avx`, each implying the ones before it.
    const fn rank(self) -> usize {
        match self {
            Level::Sse3 => 0,
            Level::Ssse3 => 1,
            Level::Avx => 4,
        }
    }
}

/// The chain of SSE extensions, each implying those before it.
const CHAIN: [&str; 5] = ["sse3", "ssse3", "sse4.1", "sse4.2", "avx"];

/// Whether feature item `name` (without its sign) implies the extension
/// at `rank` of [`CHAIN`] when it is on.
fn implies(name: &[u8], rank: usize) -> bool {
    let at = CHAIN.iter().position(|c| c.as_bytes() == name);
    match at {
        Some(at) => at >= rank,
        // Every AVX feature implies AVX, which implies SSE4.2.
        None => name.starts_with(b"avx") || matches!(name, b"fma" | b"f16c" | b"xop" | b"fma4"),
    }
}

/// Whether item `name` turned off disables the extension at `rank`: it
/// or one it needs (`sse`, `sse2`, an earlier member of [`CHAIN`]).
fn disables(name: &[u8], rank: usize) -> bool {
    if matches!(name, b"sse" | b"sse2") {
        return true;
    }
    CHAIN.iter().position(|c| c.as_bytes() == name).is_some_and(|at| at <= rank)
}

/// Whether the function's `target-features` enable `level`: the last
/// item that implies it (`+`) or disables it (`-`) decides; without one
/// it is off (a `target-cpu` alone does not count: clang writes the
/// features the CPU has).
pub fn enabled(cx: &FnCx<'_, X64>, level: Level) -> bool {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return false;
    };
    let Some(features) = cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) else {
        return false;
    };
    let rank = level.rank();
    let mut on = false;
    for item in features.split(|&b| b == b',') {
        match item.split_first() {
            Some((b'+', name)) if implies(name, rank) => on = true,
            Some((b'-', name)) if disables(name, rank) => on = false,
            _ => {}
        }
    }
    on
}

/// The extension `which` needs beyond SSE2, if any.
pub fn level(which: Intrinsic<'_>) -> Option<Level> {
    let name = which.name();
    if name.starts_with("llvm.x86.sse3.") {
        Some(Level::Sse3)
    } else if name.starts_with("llvm.x86.ssse3.") {
        Some(Level::Ssse3)
    } else {
        None
    }
}

/// The stable name `which` is refused with in a function whose features
/// do not enable it: `intrinsic:` and its LLVM name.
pub fn refusal(which: Intrinsic<'_>) -> &'static str {
    use Intrinsic as I;
    match which {
        I::X86Sse3AddsubPd => "intrinsic:llvm.x86.sse3.addsub.pd",
        I::X86Sse3AddsubPs => "intrinsic:llvm.x86.sse3.addsub.ps",
        I::X86Sse3HaddPd => "intrinsic:llvm.x86.sse3.hadd.pd",
        I::X86Sse3HaddPs => "intrinsic:llvm.x86.sse3.hadd.ps",
        I::X86Sse3HsubPd => "intrinsic:llvm.x86.sse3.hsub.pd",
        I::X86Sse3HsubPs => "intrinsic:llvm.x86.sse3.hsub.ps",
        I::X86Sse3LduDq => "intrinsic:llvm.x86.sse3.ldu.dq",
        I::X86Ssse3PhaddD => "intrinsic:llvm.x86.ssse3.phadd.d.128",
        I::X86Ssse3PhaddSw => "intrinsic:llvm.x86.ssse3.phadd.sw.128",
        I::X86Ssse3PhaddW => "intrinsic:llvm.x86.ssse3.phadd.w.128",
        I::X86Ssse3PhsubD => "intrinsic:llvm.x86.ssse3.phsub.d.128",
        I::X86Ssse3PhsubSw => "intrinsic:llvm.x86.ssse3.phsub.sw.128",
        I::X86Ssse3PhsubW => "intrinsic:llvm.x86.ssse3.phsub.w.128",
        I::X86Ssse3PmaddUbSw => "intrinsic:llvm.x86.ssse3.pmadd.ub.sw.128",
        I::X86Ssse3PmulHrSw => "intrinsic:llvm.x86.ssse3.pmul.hr.sw.128",
        I::X86Ssse3PshufB => "intrinsic:llvm.x86.ssse3.pshuf.b.128",
        I::X86Ssse3PsignB => "intrinsic:llvm.x86.ssse3.psign.b.128",
        I::X86Ssse3PsignD => "intrinsic:llvm.x86.ssse3.psign.d.128",
        I::X86Ssse3PsignW => "intrinsic:llvm.x86.ssse3.psign.w.128",
        _ => SIGNATURE,
    }
}

/// The shapes of an intrinsic's operands and result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    /// A 128-bit vector (class `Fpr`).
    V,
    /// `i32`.
    I32,
    /// `i64`.
    I64,
    /// `i8` (an `immarg`).
    I8,
    /// `ptr`.
    P,
    /// No result.
    Void,
}

/// What a lowering does.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// `a = a op b` by one packed instruction.
    Two(fn(Xmm, Xmm) -> mc::Inst),
    /// `a = op a` in place.
    One(fn(Xmm, Xmm) -> mc::Inst),
    /// `cmpss`/`cmpsd` with an immediate predicate.
    Cmp(FpSize),
    /// `comis*`/`ucomis*`: ordered, size, predicate.
    Comi(bool, FpSize, Pred),
    /// To a general register: `cvt(t)s*2si`, `movmskp*`, `pmovmskb`.
    ToGpr(fn(Size, Reg, Xmm) -> mc::Inst, Size),
    /// Shift by an `i32` count: by immediate, by register.
    ShiftI(fn(Xmm, u8) -> mc::Inst, fn(Xmm, Xmm) -> mc::Inst),
    /// A fence.
    Fence(mc::Inst),
    /// One instruction on the memory at the pointer operand: `ldmxcsr`,
    /// `stmxcsr`, `clflush` (WP-181).
    AtPtr(fn(Mem) -> mc::Inst),
    /// `maskmovdqu`: data, mask, and the address in `rdi` (WP-181).
    MaskMov,
    /// SSE3 horizontal and alternating arithmetic.
    Sse3(Sse3),
    /// `lddqu`: an unaligned load.
    Load,
    /// SSSE3, lane by lane.
    Lanes(Ssse3),
}

/// The predicate of a `comi`/`ucomi` intrinsic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pred {
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
    Neq,
}

/// The SSE3 operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sse3 {
    Hadd(FpSize),
    Hsub(FpSize),
    Addsub(FpSize),
}

/// The SSSE3 operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ssse3 {
    /// Horizontal add (`true`) or subtract of lanes of `bytes`, saturated.
    Horizontal { add: bool, bytes: u32, sat: bool },
    /// `pmaddubsw`.
    MaddUbSw,
    /// `pmulhrsw`.
    MulHrSw,
    /// `pshufb`.
    ShufB,
    /// `psign` of lanes of `bytes`.
    Sign(u32),
}

/// The signature (result, operands) and the lowering of `which`, if it
/// is one of this module's.
#[allow(clippy::too_many_lines, reason = "one row per intrinsic")]
fn row(which: Intrinsic<'_>) -> Option<(K, &'static [K], Op)> {
    use Intrinsic as I;
    use K::{I8, I32, I64, P, V, Void};
    const VV: &[K] = &[V, V];
    const V1: &[K] = &[V];
    const VI: &[K] = &[V, I32];
    const VVI: &[K] = &[V, V, I8];
    const NONE: &[K] = &[];
    let comi = |ordered, fp, pred| (I32, VV, Op::Comi(ordered, fp, pred));
    let (s, d) = (FpSize::S, FpSize::D);
    Some(match which {
        I::X86SseCmpSs => (V, VVI, Op::Cmp(s)),
        I::X86Sse2CmpSd => (V, VVI, Op::Cmp(d)),
        I::X86SseComieqSs => comi(true, s, Pred::Eq),
        I::X86SseComigeSs => comi(true, s, Pred::Ge),
        I::X86SseComigtSs => comi(true, s, Pred::Gt),
        I::X86SseComileSs => comi(true, s, Pred::Le),
        I::X86SseComiltSs => comi(true, s, Pred::Lt),
        I::X86SseComineqSs => comi(true, s, Pred::Neq),
        I::X86SseUcomieqSs => comi(false, s, Pred::Eq),
        I::X86SseUcomigeSs => comi(false, s, Pred::Ge),
        I::X86SseUcomigtSs => comi(false, s, Pred::Gt),
        I::X86SseUcomileSs => comi(false, s, Pred::Le),
        I::X86SseUcomiltSs => comi(false, s, Pred::Lt),
        I::X86SseUcomineqSs => comi(false, s, Pred::Neq),
        I::X86Sse2ComieqSd => comi(true, d, Pred::Eq),
        I::X86Sse2ComigeSd => comi(true, d, Pred::Ge),
        I::X86Sse2ComigtSd => comi(true, d, Pred::Gt),
        I::X86Sse2ComileSd => comi(true, d, Pred::Le),
        I::X86Sse2ComiltSd => comi(true, d, Pred::Lt),
        I::X86Sse2ComineqSd => comi(true, d, Pred::Neq),
        I::X86Sse2UcomieqSd => comi(false, d, Pred::Eq),
        I::X86Sse2UcomigeSd => comi(false, d, Pred::Ge),
        I::X86Sse2UcomigtSd => comi(false, d, Pred::Gt),
        I::X86Sse2UcomileSd => comi(false, d, Pred::Le),
        I::X86Sse2UcomiltSd => comi(false, d, Pred::Lt),
        I::X86Sse2UcomineqSd => comi(false, d, Pred::Neq),
        I::X86SseCvtss2si => {
            (I32, V1, Op::ToGpr(|z, r, x| mc::cvts2si_rr(z, r, FpSize::S, x), Size::D))
        }
        I::X86SseCvtss2si64 => {
            (I64, V1, Op::ToGpr(|z, r, x| mc::cvts2si_rr(z, r, FpSize::S, x), Size::Q))
        }
        I::X86SseCvttss2si => {
            (I32, V1, Op::ToGpr(|z, r, x| mc::cvtts2si_rr(z, r, FpSize::S, x), Size::D))
        }
        I::X86SseCvttss2si64 => {
            (I64, V1, Op::ToGpr(|z, r, x| mc::cvtts2si_rr(z, r, FpSize::S, x), Size::Q))
        }
        I::X86Sse2Cvtsd2si => {
            (I32, V1, Op::ToGpr(|z, r, x| mc::cvts2si_rr(z, r, FpSize::D, x), Size::D))
        }
        I::X86Sse2Cvtsd2si64 => {
            (I64, V1, Op::ToGpr(|z, r, x| mc::cvts2si_rr(z, r, FpSize::D, x), Size::Q))
        }
        I::X86Sse2Cvttsd2si => {
            (I32, V1, Op::ToGpr(|z, r, x| mc::cvtts2si_rr(z, r, FpSize::D, x), Size::D))
        }
        I::X86Sse2Cvttsd2si64 => {
            (I64, V1, Op::ToGpr(|z, r, x| mc::cvtts2si_rr(z, r, FpSize::D, x), Size::Q))
        }
        I::X86SseMovmskPs => {
            (I32, V1, Op::ToGpr(|_, r, x| mc::movmskp_rr(FpSize::S, r, x), Size::D))
        }
        I::X86Sse2MovmskPd => {
            (I32, V1, Op::ToGpr(|_, r, x| mc::movmskp_rr(FpSize::D, r, x), Size::D))
        }
        I::X86Sse2Pmovmskb => (I32, V1, Op::ToGpr(|_, r, x| mc::pmovmskb_rr(r, x), Size::D)),
        I::X86SseMaxPs => (V, VV, Op::Two(|a, b| mc::maxp_rr(FpSize::S, a, b))),
        I::X86SseMinPs => (V, VV, Op::Two(|a, b| mc::minp_rr(FpSize::S, a, b))),
        I::X86SseMaxSs => (V, VV, Op::Two(|a, b| mc::maxs_rr(FpSize::S, a, b))),
        I::X86SseMinSs => (V, VV, Op::Two(|a, b| mc::mins_rr(FpSize::S, a, b))),
        I::X86Sse2MaxPd => (V, VV, Op::Two(|a, b| mc::maxp_rr(FpSize::D, a, b))),
        I::X86Sse2MinPd => (V, VV, Op::Two(|a, b| mc::minp_rr(FpSize::D, a, b))),
        I::X86Sse2MaxSd => (V, VV, Op::Two(|a, b| mc::maxs_rr(FpSize::D, a, b))),
        I::X86Sse2MinSd => (V, VV, Op::Two(|a, b| mc::mins_rr(FpSize::D, a, b))),
        I::X86Sse2Cvtsd2ss => (V, VV, Op::Two(mc::cvtsd2ss_rr)),
        I::X86Sse2Packssdw => (V, VV, Op::Two(mc::packssdw_rr)),
        I::X86Sse2Packsswb => (V, VV, Op::Two(mc::packsswb_rr)),
        I::X86Sse2Packuswb => (V, VV, Op::Two(mc::packuswb_rr)),
        I::X86Sse2PavgB => (V, VV, Op::Two(mc::pavgb_rr)),
        I::X86Sse2PavgW => (V, VV, Op::Two(mc::pavgw_rr)),
        I::X86Sse2PmaddWd => (V, VV, Op::Two(mc::pmaddwd_rr)),
        I::X86Sse2PmulhW => (V, VV, Op::Two(mc::pmulhw_rr)),
        I::X86Sse2PmulhuW => (V, VV, Op::Two(mc::pmulhuw_rr)),
        I::X86Sse2PsadBw => (V, VV, Op::Two(mc::psadbw_rr)),
        I::X86Sse2PsllD => (V, VV, Op::Two(mc::pslld_rr)),
        I::X86Sse2PsllQ => (V, VV, Op::Two(mc::psllq_rr)),
        I::X86Sse2PsllW => (V, VV, Op::Two(mc::psllw_rr)),
        I::X86Sse2PsraD => (V, VV, Op::Two(mc::psrad_rr)),
        I::X86Sse2PsraW => (V, VV, Op::Two(mc::psraw_rr)),
        I::X86Sse2PsrlD => (V, VV, Op::Two(mc::psrld_rr)),
        I::X86Sse2PsrlQ => (V, VV, Op::Two(mc::psrlq_rr)),
        I::X86Sse2PsrlW => (V, VV, Op::Two(mc::psrlw_rr)),
        I::X86Sse2PslliD => (V, VI, Op::ShiftI(mc::pslld_ri, mc::pslld_rr)),
        I::X86Sse2PslliQ => (V, VI, Op::ShiftI(mc::psllq_ri, mc::psllq_rr)),
        I::X86Sse2PslliW => (V, VI, Op::ShiftI(mc::psllw_ri, mc::psllw_rr)),
        I::X86Sse2PsraiD => (V, VI, Op::ShiftI(mc::psrad_ri, mc::psrad_rr)),
        I::X86Sse2PsraiW => (V, VI, Op::ShiftI(mc::psraw_ri, mc::psraw_rr)),
        I::X86Sse2PsrliD => (V, VI, Op::ShiftI(mc::psrld_ri, mc::psrld_rr)),
        I::X86Sse2PsrliQ => (V, VI, Op::ShiftI(mc::psrlq_ri, mc::psrlq_rr)),
        I::X86Sse2PsrliW => (V, VI, Op::ShiftI(mc::psrlw_ri, mc::psrlw_rr)),
        I::X86Sse2Cvtpd2dq => (V, V1, Op::One(mc::cvtpd2dq_rr)),
        I::X86Sse2Cvtpd2ps => (V, V1, Op::One(mc::cvtpd2ps_rr)),
        I::X86Sse2Cvtps2dq => (V, V1, Op::One(mc::cvtps2dq_rr)),
        I::X86Sse2Cvttpd2dq => (V, V1, Op::One(mc::cvttpd2dq_rr)),
        I::X86Sse2Cvttps2dq => (V, V1, Op::One(mc::cvttps2dq_rr)),
        I::X86SseSfence => (Void, NONE, Op::Fence(mc::sfence())),
        I::X86Sse2Lfence => (Void, NONE, Op::Fence(mc::lfence())),
        I::X86Sse2Mfence => (Void, NONE, Op::Fence(mc::mfence())),
        I::X86SseLdmxcsr => (Void, &[P], Op::AtPtr(mc::ldmxcsr_m)),
        I::X86SseStmxcsr => (Void, &[P], Op::AtPtr(mc::stmxcsr_m)),
        I::X86Sse2Clflush => (Void, &[P], Op::AtPtr(mc::clflush_m)),
        I::X86SseRcpPs => (V, V1, Op::One(mc::rcpps_rr)),
        I::X86SseRcpSs => (V, V1, Op::One(mc::rcpss_rr)),
        I::X86SseRsqrtPs => (V, V1, Op::One(mc::rsqrtps_rr)),
        I::X86SseRsqrtSs => (V, V1, Op::One(mc::rsqrtss_rr)),
        I::X86Sse2MaskmovDqu => (Void, &[V, V, P], Op::MaskMov),
        I::X86Sse3AddsubPd => (V, VV, Op::Sse3(Sse3::Addsub(d))),
        I::X86Sse3AddsubPs => (V, VV, Op::Sse3(Sse3::Addsub(s))),
        I::X86Sse3HaddPd => (V, VV, Op::Sse3(Sse3::Hadd(d))),
        I::X86Sse3HaddPs => (V, VV, Op::Sse3(Sse3::Hadd(s))),
        I::X86Sse3HsubPd => (V, VV, Op::Sse3(Sse3::Hsub(d))),
        I::X86Sse3HsubPs => (V, VV, Op::Sse3(Sse3::Hsub(s))),
        I::X86Sse3LduDq => (V, &[P], Op::Load),
        I::X86Ssse3PhaddD => (V, VV, hz(true, 4, false)),
        I::X86Ssse3PhaddSw => (V, VV, hz(true, 2, true)),
        I::X86Ssse3PhaddW => (V, VV, hz(true, 2, false)),
        I::X86Ssse3PhsubD => (V, VV, hz(false, 4, false)),
        I::X86Ssse3PhsubSw => (V, VV, hz(false, 2, true)),
        I::X86Ssse3PhsubW => (V, VV, hz(false, 2, false)),
        I::X86Ssse3PmaddUbSw => (V, VV, Op::Lanes(Ssse3::MaddUbSw)),
        I::X86Ssse3PmulHrSw => (V, VV, Op::Lanes(Ssse3::MulHrSw)),
        I::X86Ssse3PshufB => (V, VV, Op::Lanes(Ssse3::ShufB)),
        I::X86Ssse3PsignB => (V, VV, Op::Lanes(Ssse3::Sign(1))),
        I::X86Ssse3PsignD => (V, VV, Op::Lanes(Ssse3::Sign(4))),
        I::X86Ssse3PsignW => (V, VV, Op::Lanes(Ssse3::Sign(2))),
        _ => return None,
    })
}

/// A horizontal SSSE3 operation.
const fn hz(add: bool, bytes: u32, sat: bool) -> Op {
    Op::Lanes(Ssse3::Horizontal { add, bytes, sat })
}

/// Whether `ty` is what `k` says.
fn fits(cx: &FnCx<'_, X64>, ty: hod_ir::TypeId, k: K) -> bool {
    let types = cx.types();
    match (k, types.kind(ty)) {
        (K::V, TypeKind::Vector { elem, len }) => {
            let bits = match types.kind(elem) {
                TypeKind::Int(bits) => bits,
                TypeKind::Float => 32,
                TypeKind::Double => 64,
                _ => return false,
            };
            u64::from(bits) * u64::from(len) == 128 && matches!(bits, 8 | 16 | 32 | 64)
        }
        (K::I32, TypeKind::Int(32))
        | (K::I64, TypeKind::Int(64))
        | (K::I8, TypeKind::Int(8))
        | (K::P, TypeKind::Ptr { .. })
        | (K::Void, TypeKind::Void) => true,
        _ => false,
    }
}

/// A call of `which` with `args`, if `which` is an x86 intrinsic of this
/// module.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> Option<CgResult<()>> {
    let Some((ret, params, op)) = row(which) else {
        return sys::intrinsic(cx, inst, which, args)
            .or_else(|| ext::intrinsic(cx, inst, which, args))
            .or_else(|| crate::vector::masked(cx, inst, which, args));
    };
    Some(lower(cx, inst, which, args, ret, params, op))
}

#[allow(clippy::too_many_lines, reason = "one arm per kind of row")]
fn lower<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
    ret: K,
    params: &[K],
    op: Op,
) -> CgResult<()> {
    if args.len() != params.len()
        || !fits(cx, inst.ty(), ret)
        || args.iter().zip(params).any(|(&a, &k)| !fits(cx, value_ty(cx, a), k))
        || args.iter().zip(params).any(|(&a, &k)| k == K::V && cx.class_of(a) != Class::Fpr)
    {
        return unsupported(SIGNATURE);
    }
    if let Some(level) = level(which)
        && !enabled(cx, level)
    {
        return unsupported(refusal(which));
    }
    deferred_first(cx, inst, None)?;
    match op {
        Op::Two(make) => {
            let (a, b) = pair(cx, args[0], args[1])?;
            put(cx, make(xm(a), xm(b)));
            cx.finish(inst, a)
        }
        Op::One(make) => {
            let a = cx.use_reg(args[0])?;
            put(cx, make(xm(a), xm(a)));
            cx.finish(inst, a)
        }
        Op::Cmp(fp) => {
            let Some(pred) = const_int(cx, args[2]) else {
                return cx.bug("cmpss/cmpsd of a predicate that is not a constant");
            };
            let (a, b) = pair(cx, args[0], args[1])?;
            let pred = (pred & 0xff) as u8;
            if pred & 0x1f >= 8 && enabled(cx, Level::Avx) {
                // WP-202: with AVX the predicate has five bits (`llc`
                // writes `vcmpss`); the legacy encoding reads three.
                let op = if fp == FpSize::S { mc::V3Imm::Cmpss } else { mc::V3Imm::Cmpsd };
                put(cx, mc::v3_rrri(op, mc::Width::X, xm(a), xm(a), xm(b), pred));
            } else {
                put(cx, mc::cmps_rr(FpCmp::ALL[usize::from(pred & 7)], fp, xm(a), xm(b)));
            }
            cx.finish(inst, a)
        }
        Op::Comi(ordered, fp, pred) => {
            let (a, b) = pair(cx, args[0], args[1])?;
            let (x, y) = if matches!(pred, Pred::Lt | Pred::Le) { (b, a) } else { (a, b) };
            let rd = cx.def_reg(inst)?;
            let compare = if ordered { mc::comis_rr } else { mc::ucomis_rr };
            put(cx, compare(fp, xm(x), xm(y)));
            match pred {
                Pred::Eq | Pred::Neq => {
                    let eq = pred == Pred::Eq;
                    put(cx, mc::setcc_r(if eq { Cond::Np } else { Cond::P }, RCX));
                    put(cx, mc::setcc_r(if eq { Cond::E } else { Cond::Ne }, RAX));
                    let both = if eq { mc::and_rr } else { mc::or_rr };
                    put(cx, both(Size::B, RAX, RCX));
                }
                Pred::Gt | Pred::Lt => put(cx, mc::setcc_r(Cond::A, RAX)),
                Pred::Ge | Pred::Le => put(cx, mc::setcc_r(Cond::Ae, RAX)),
            }
            put(cx, mc::movzx_rr(Size::D, gp(rd), Size::B, RAX));
            cx.finish(inst, rd)
        }
        Op::ToGpr(make, size) => {
            let a = cx.use_reg(args[0])?;
            let rd = cx.def_reg(inst)?;
            put(cx, make(size, gp(rd), xm(a)));
            cx.finish(inst, rd)
        }
        Op::ShiftI(by_imm, by_reg) => {
            if let Some(count) = const_int(cx, args[1]) {
                let a = cx.use_reg(args[0])?;
                // Every count at or above the lane width gives the same
                // lanes; `imm8` holds up to 255.
                let count = u8::try_from(count & 0xffff_ffff).unwrap_or(u8::MAX);
                put(cx, by_imm(xm(a), count));
                return cx.finish(inst, a);
            }
            let a = cx.use_reg(args[0])?;
            let n = cx.use_reg(args[1])?;
            // `movd` zero-extends the 32-bit count into the 64 bits the
            // shift reads, as `llc`'s.
            put(cx, mc::movd_xr(Size::D, XMM15, gp(n)));
            put(cx, by_reg(xm(a), XMM15));
            cx.finish(inst, a)
        }
        Op::Fence(fence) => {
            put(cx, fence);
            Ok(())
        }
        Op::AtPtr(make) => {
            let p = cx.use_reg(args[0])?;
            put(cx, make(Mem::base(gp(p), 0)));
            Ok(())
        }
        Op::MaskMov => {
            let (data, mask) = pair(cx, args[0], args[1])?;
            let p = cx.use_reg(args[2])?;
            // The store's address is `rdi`, which may hold a value: kept
            // in `rax` (which never holds one) around the instruction.
            let rdi = Reg::Rdi;
            put(cx, mc::mov_rr(Size::Q, RAX, rdi));
            put(cx, mc::mov_rr(Size::Q, rdi, gp(p)));
            put(cx, mc::maskmovdqu_rr(xm(data), xm(mask)));
            put(cx, mc::mov_rr(Size::Q, rdi, RAX));
            Ok(())
        }
        Op::Load => {
            let p = cx.use_reg(args[0])?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::movdqu_rm(xm(rd), Mem::base(gp(p), 0)));
            cx.finish(inst, rd)
        }
        Op::Sse3(kind) => sse3(cx, inst, kind, args[0], args[1]),
        Op::Lanes(kind) => ssse3(cx, inst, kind, args[0], args[1]),
    }
}

/// The registers of two operands (one register when they are the same
/// value: a register value may be asked for once).
fn pair(cx: &mut FnCx<'_, X64>, a: Value, b: Value) -> CgResult<(PReg, PReg)> {
    let ra = cx.use_reg(a)?;
    let rb = if a == b { ra } else { cx.use_reg(b)? };
    Ok((ra, rb))
}

/// The SSE3 arithmetic from SSE2 instructions; the result in `a`'s
/// register.
#[allow(clippy::many_single_char_names, reason = "the operands and their registers")]
fn sse3<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    kind: Sse3,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let (ra, rb) = pair(cx, a, b)?;
    let (x, y) = (xm(ra), xm(rb));
    // `xmm14` the second gathered operand (or the difference), `xmm15`
    // the lane mask: the scratch registers, no temporary.
    let t = XMM14;
    put(cx, mc::movap_rr(FpSize::S, t, x));
    match kind {
        Sse3::Hadd(fp) | Sse3::Hsub(fp) => {
            // `t` the odd lanes of `a`, then `b`; `x` the even ones.
            if fp == FpSize::S {
                put(cx, mc::shufp_rri(fp, t, y, 0b11_01_11_01));
                put(cx, mc::shufp_rri(fp, x, y, 0b10_00_10_00));
            } else {
                put(cx, mc::unpckhp_rr(fp, t, y));
                put(cx, mc::unpcklp_rr(fp, x, y));
            }
            let arith = if matches!(kind, Sse3::Hadd(_)) { mc::addp_rr } else { mc::subp_rr };
            put(cx, arith(fp, x, t));
        }
        Sse3::Addsub(fp) => {
            // `t = a - b`, `x = a + b`; the even lanes from `t`.
            put(cx, mc::subp_rr(fp, t, y));
            put(cx, mc::addp_rr(fp, x, y));
            put(cx, mc::pcmpeqd_rr(XMM15, XMM15));
            if fp == FpSize::S {
                put(cx, mc::psrlq_ri(XMM15, 32));
            } else {
                put(cx, mc::psrldq_ri(XMM15, 8));
            }
            put(cx, mc::andp_rr(FpSize::S, t, XMM15));
            put(cx, mc::andnp_rr(FpSize::S, XMM15, x));
            put(cx, mc::orp_rr(FpSize::S, XMM15, t));
            put(cx, mc::movap_rr(FpSize::S, x, XMM15));
        }
    }
    cx.finish(inst, ra)
}

/// A 16-byte, 16-aligned frame image of the vector in `x`.
fn image(cx: &mut FnCx<'_, X64>, x: Xmm) -> CgResult<FrameRef> {
    let slot = cx.alloc_slot(16, 16)?;
    store_fpr(cx, x, slot, 16);
    Ok(slot)
}

/// The size of a lane of `bytes`.
fn lane_size(bytes: u32) -> Size {
    match bytes {
        1 => Size::B,
        2 => Size::W,
        _ => Size::D,
    }
}

/// Loads lane `at` (a byte offset) of `bytes` of `img` into `rd`,
/// sign-extended (`signed`) or zero-extended to 32 bits.
fn lane_load(cx: &mut FnCx<'_, X64>, rd: Reg, img: FrameRef, at: u32, bytes: u32, signed: bool) {
    let place = img.offset_by(at);
    frame_inst(cx, place, |m| match (bytes, signed) {
        (4, _) => mc::mov_rm(Size::D, rd, m),
        (_, true) => mc::movsx_rm(Size::D, rd, lane_size(bytes), m),
        (_, false) => mc::movzx_rm(Size::D, rd, lane_size(bytes), m),
    });
}

/// Stores the low `bytes` of `rs` to lane `at` of `img`.
fn lane_store(cx: &mut FnCx<'_, X64>, img: FrameRef, at: u32, bytes: u32, rs: Reg) {
    frame_inst(cx, img.offset_by(at), |m| mc::mov_mr(lane_size(bytes), m, rs));
}

/// Clamps the 32-bit `r` to `i16` through `rdx`.
fn saturate16(cx: &mut FnCx<'_, X64>, r: Reg) {
    put(cx, mc::cmp_ri(Size::D, r, 0x7fff));
    put(cx, mc::mov_ri(Size::D, RDX, 0x7fff));
    put(cx, mc::cmovcc_rr(Cond::G, Size::D, r, RDX));
    put(cx, mc::cmp_ri(Size::D, r, -0x8000));
    put(cx, mc::mov_ri(Size::D, RDX, -0x8000));
    put(cx, mc::cmovcc_rr(Cond::L, Size::D, r, RDX));
}

/// The SSSE3 operations lane by lane: both operands' images, the
/// result's written lane by lane through `rax`, `rcx`, `rdx`, `r11`.
fn ssse3<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    kind: Ssse3,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let (ra, rb) = pair(cx, a, b)?;
    let ia = image(cx, xm(ra))?;
    let ib = if a == b { ia } else { image(cx, xm(rb))? };
    let out = cx.alloc_slot(16, 16)?;
    match kind {
        Ssse3::Horizontal { add, bytes, sat } => {
            let lanes = 16 / bytes;
            for j in 0..lanes {
                let (src, k) = if j < lanes / 2 { (ia, j * 2) } else { (ib, (j - lanes / 2) * 2) };
                lane_load(cx, RAX, src, k * bytes, bytes, true);
                lane_load(cx, RCX, src, (k + 1) * bytes, bytes, true);
                let arith = if add { mc::add_rr } else { mc::sub_rr };
                put(cx, arith(Size::D, RAX, RCX));
                if sat {
                    saturate16(cx, RAX);
                }
                lane_store(cx, out, j * bytes, bytes, RAX);
            }
        }
        Ssse3::MaddUbSw => {
            for j in 0..8 {
                lane_load(cx, RAX, ia, 2 * j, 1, false);
                lane_load(cx, RCX, ib, 2 * j, 1, true);
                put(cx, mc::imul_rr(Size::D, RAX, RCX));
                lane_load(cx, RCX, ia, 2 * j + 1, 1, false);
                lane_load(cx, R11, ib, 2 * j + 1, 1, true);
                put(cx, mc::imul_rr(Size::D, RCX, R11));
                put(cx, mc::add_rr(Size::D, RAX, RCX));
                saturate16(cx, RAX);
                lane_store(cx, out, 2 * j, 2, RAX);
            }
        }
        Ssse3::MulHrSw => {
            for j in 0..8 {
                lane_load(cx, RAX, ia, 2 * j, 2, true);
                lane_load(cx, RCX, ib, 2 * j, 2, true);
                put(cx, mc::imul_rr(Size::D, RAX, RCX));
                put(cx, mc::add_ri(Size::D, RAX, 0x4000));
                put(cx, mc::sar_ri(Size::D, RAX, 15));
                lane_store(cx, out, 2 * j, 2, RAX);
            }
        }
        Ssse3::ShufB => {
            let Some(index) = RCX.index() else { return cx.bug("rcx is an index register") };
            frame_address(cx, R11, ia);
            for j in 0..16 {
                lane_load(cx, RAX, ib, j, 1, false);
                put(cx, mc::mov_rr(Size::D, RCX, RAX));
                put(cx, mc::and_ri(Size::D, RCX, 15));
                let at = Mem::base_index(R11, index, Scale::S1, 0);
                put(cx, mc::movzx_rm(Size::D, RCX, Size::B, at));
                // A selector with bit 7 set gives 0.
                put(cx, mc::mov_ri(Size::D, RDX, 0));
                put(cx, mc::test_ri(Size::D, RAX, 0x80));
                put(cx, mc::cmovcc_rr(Cond::Ne, Size::D, RCX, RDX));
                lane_store(cx, out, j, 1, RCX);
            }
        }
        Ssse3::Sign(bytes) => {
            for j in 0..16 / bytes {
                lane_load(cx, RAX, ia, j * bytes, bytes, true);
                lane_load(cx, RCX, ib, j * bytes, bytes, true);
                put(cx, mc::mov_rr(Size::D, RDX, RAX));
                put(cx, mc::neg_r(Size::D, RDX));
                // Negative: `-a`; zero: 0 (`rcx`); positive: `a`.
                put(cx, mc::test_rr(Size::D, RCX, RCX));
                put(cx, mc::cmovcc_rr(Cond::S, Size::D, RAX, RDX));
                put(cx, mc::cmovcc_rr(Cond::E, Size::D, RAX, RCX));
                lane_store(cx, out, j * bytes, bytes, RAX);
            }
        }
    }
    load_fpr(cx, xm(ra), out, 16);
    cx.finish(inst, ra)
}
