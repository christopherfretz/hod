//! The x86 vector intrinsics past SSSE3 (`spec/codegen.md` §5.6.33,
//! WP-202): the `llvm.x86.*` names clang 19 and 22 emit at `-O0` for the
//! functions of `<smmintrin.h>`, `<nmmintrin.h>`, `<wmmintrin.h>` (AES,
//! PCLMUL), `<shaintrin.h>`, `<avxintrin.h>`, `<avx2intrin.h>`,
//! `<fmaintrin.h>`, `<f16cintrin.h>` and `<gfniintrin.h>` (the 128- and
//! 256-bit forms).
//!
//! **Owner: WP-202.** Tests: `tests/x86_ext.rs`. Spec: §5.6.33.
//!
//! Each intrinsic is accepted only in a function whose `target-features`
//! enable what clang requires for its header function ([`Need`]), and
//! refused by its own name (`intrinsic:llvm.x86...`) otherwise. With the
//! feature, hod writes the instruction `llc` selects, or instructions
//! that compute the same bits:
//!
//! - **128-bit vectors** (class `Fpr`, in an `xmm`): the legacy encoding
//!   in place on the first operand's register (`packusdw`, `ptest`,
//!   `roundps`, `dpps`, `insertps`, `mpsadbw`, `pclmulqdq`, the AES, SHA
//!   and GFNI instructions), also in a function with AVX, where `llc`
//!   writes the `VEX` form (the same lanes; the upper half of a `ymm`
//!   holds no value of hod's). The variable blends (`blendvps`,
//!   `blendvpd`, `pblendvb`, which read the mask from `xmm0`) are the
//!   sign mask of each element (`psrad`, `pshufd`, `pcmpgtb` against 0)
//!   and `pand`/`pandn`/`por`. `pcmpistrm`/`pcmpestrm` write `xmm0` and
//!   `sha256rnds2` reads it: `xmm0` is kept in `xmm15` around them. The
//!   string compares' lengths go in `eax` and `edx`, the index comes in
//!   `ecx`, which hold no values. The AVX-only 128-bit forms
//!   (`vpermilvar`, `vtestps`, `vpsllvd`, the masked moves, the FMA
//!   alternations, `vcvtps2ph`) are their `VEX.128` instruction.
//! - **256-bit vectors** (class `Mem`, their bytes in a frame slot,
//!   `spec/abi-x64.md` §1.3): the first operand loaded into `ymm14`
//!   (`vmovdqu`), the instruction with the second operand from its slot
//!   (`ymm15` for the third), the result stored back with `vmovdqu`; a
//!   scalar or 128-bit result goes to its register as for the 128-bit
//!   forms. `vzeroall` is `vzeroupper`: hod keeps no value in an upper
//!   half, `vzeroall` would clear the `xmm`s that hold values, and what
//!   the registers hold is not observable by the program.

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg};
use hod_ir::{FnStr, Inst, Intrinsic, SymbolKind, TypeId, TypeKind, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{
    Cond, Ext as Xt, ExtImm as E, Fma, FpSize, Mem, Reg, Size, V2, V2Imm, V3, V3Imm, VBlend, VMask,
    VShift, Width, Xmm,
};

use crate::common::{
    XMM14, XMM15, deferred_first, frame_inst, gp, load_fpr, put, store_fpr, unsupported, value_ty,
    xm,
};
use crate::intrinsic::const_int;
use crate::target::X64;

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;
const XMM0: Xmm = Xmm::Xmm0;

/// `avx`'s position in WP-173's chain of SSE levels.
const AVX_RANK: usize = 4;
/// `sse4.1`'s.
const SSE41_RANK: usize = 2;
/// `sse4.2`'s.
const SSE42_RANK: usize = 3;

/// What an intrinsic needs the function's features to enable: the
/// feature of clang's header function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Need {
    /// `sse4.1` (or a later SSE level, or AVX).
    Sse41,
    /// `sse4.2`.
    Sse42,
    /// `aes` (or `vaes`).
    Aes,
    /// `pclmul` (or `vpclmulqdq`).
    Pclmul,
    /// `sha`.
    Sha,
    /// `avx`.
    Avx,
    /// `avx2` (or an AVX-512 or AVX10 feature).
    Avx2,
    /// `fma`.
    Fma,
    /// `f16c`.
    F16c,
    /// `gfni`.
    Gfni,
    /// `gfni` and `avx`: the 256-bit GFNI forms.
    GfniAvx,
}

/// Whether item `name` (without its sign) turned off turns SSE2 off.
fn no_sse2(name: &[u8]) -> bool {
    matches!(name, b"sse" | b"sse2")
}

/// Whether an AVX-512 or AVX10 feature, or one of the AVX extensions
/// that imply AVX2 (`avxvnni`, `avxifma`, ...): every `avx...` but `avx`.
fn implies_avx2(name: &[u8]) -> bool {
    name.starts_with(b"avx") && name != b"avx"
}

/// Whether an AVX-512 or AVX10 feature (each implies FMA and F16C).
fn avx512(name: &[u8]) -> bool {
    name.starts_with(b"avx512") || name.starts_with(b"avx10")
}

impl Need {
    /// Whether item `name`, turned on, turns this need on.
    fn implied_by(self, name: &[u8]) -> bool {
        match self {
            Need::Sse41 => super::implies(name, SSE41_RANK),
            Need::Sse42 => super::implies(name, SSE42_RANK),
            Need::Avx => super::implies(name, AVX_RANK),
            Need::Avx2 => implies_avx2(name),
            Need::Aes => matches!(name, b"aes" | b"vaes"),
            Need::Pclmul => matches!(name, b"pclmul" | b"vpclmulqdq"),
            Need::Sha => name == b"sha",
            Need::Fma => name == b"fma" || avx512(name),
            Need::F16c => name == b"f16c" || avx512(name),
            Need::Gfni | Need::GfniAvx => name == b"gfni",
        }
    }

    /// Whether item `name`, turned off, turns this need off: the feature
    /// or one it needs.
    fn disabled_by(self, name: &[u8]) -> bool {
        match self {
            Need::Sse41 => super::disables(name, SSE41_RANK),
            Need::Sse42 => super::disables(name, SSE42_RANK),
            Need::Avx => super::disables(name, AVX_RANK),
            Need::Avx2 => name == b"avx2" || super::disables(name, AVX_RANK),
            Need::Fma => name == b"fma" || super::disables(name, AVX_RANK),
            Need::F16c => name == b"f16c" || super::disables(name, AVX_RANK),
            Need::Aes => name == b"aes" || no_sse2(name),
            Need::Pclmul => name == b"pclmul" || no_sse2(name),
            Need::Sha => name == b"sha" || no_sse2(name),
            Need::Gfni | Need::GfniAvx => name == b"gfni" || no_sse2(name),
        }
    }
}

/// Whether the function's `target-features` enable `need`: the last item
/// that turns it on or off decides; without one it is off (as for
/// WP-173's levels, a `target-cpu` alone does not count).
pub fn has(cx: &FnCx<'_, X64>, need: Need) -> bool {
    if need == Need::GfniAvx && !has(cx, Need::Avx) {
        return false;
    }
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return false;
    };
    let Some(features) = cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) else {
        return false;
    };
    let mut on = false;
    for item in features.split(|&b| b == b',') {
        match item.split_first() {
            Some((b'+', name)) if need.implied_by(name) => on = true,
            Some((b'-', name)) if need.disabled_by(name) => on = false,
            _ => {}
        }
    }
    on
}

/// The shapes of the operands and results.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    /// A 128-bit vector (class `Fpr`).
    X,
    /// A 256-bit vector (class `Mem`).
    Y,
    /// `i32`.
    I32,
    /// `i8` (an `immarg`).
    I8,
    /// `ptr`.
    P,
    /// No result.
    Void,
}

/// The lanes whose sign bit selects in an emulated `blendv`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lane {
    B,
    D,
    Q,
}

/// What a string compare gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Out {
    /// The mask (`xmm0`).
    Mask,
    /// The index (`ecx`).
    Index,
    /// A flag.
    Flag(Cond),
}

/// The mask extractions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Msk {
    B,
    Ps,
    Pd,
}

/// What a lowering does.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// A legacy `0F38` instruction: `a = a op b`, or `a = op a`.
    Ext(Xt),
    /// A legacy `0F3A` instruction and the immediate (the last operand):
    /// `a = a op b`, or `a = op a`.
    ExtImm(E),
    /// `ptest` (`None`) or a `VEX` test and the flag that is the result.
    Test(Option<V2>, Cond),
    /// A variable blend of 128-bit vectors from SSE2 instructions.
    Blend128(Lane),
    /// A string compare.
    Str(E, Out),
    /// `sha256rnds2` (its third operand in `xmm0`).
    Sha256rnds2,
    /// A `VEX` instruction `a op b`; with `true`, `b op a`.
    V3(V3, bool),
    /// A `VEX` instruction `a op b` and the immediate.
    V3Imm(V3Imm),
    /// A `VEX` instruction of one operand.
    V2(V2),
    /// A `VEX` instruction of one operand and the immediate.
    V2Imm(V2Imm),
    /// A mask extraction of a 256-bit vector.
    Movmsk(Msk),
    /// A `VEX` variable blend.
    VBlend(VBlend),
    /// A shift by an `i32` count.
    ShiftI(VShift, V3),
    /// An FMA alternation.
    Fma(Fma),
    /// A masked load: pointer, mask.
    MaskLoad(VMask),
    /// A masked store: pointer, mask, data.
    MaskStore(VMask),
    /// An unaligned load of 256 bits.
    LoadU,
    /// `vcvtps2ph`.
    Cvtps2ph,
    /// `vzeroupper`.
    Zeroupper,
    /// An AVX2 gather: pass-through, pointer, index, mask, scale (WP-207).
    Gather(mc::Gather),
}

const fn v3(op: V3) -> Op {
    Op::V3(op, false)
}

/// The row of `which`: its refusal, result, operands, need and lowering.
type Row = (&'static str, K, &'static [K], Need, Op);

/// The row of `which`, if it is one of this module's.
#[allow(clippy::too_many_lines, reason = "one row per intrinsic")]
fn row(which: Intrinsic<'_>) -> Option<Row> {
    use Intrinsic as I;
    use K::{I8, I32, P, Void, X, Y};
    Some(match which {
        I::X86Sse41RoundPs => {
            ("intrinsic:llvm.x86.sse41.round.ps", X, &[X, I32], Need::Sse41, Op::ExtImm(E::Roundps))
        }
        I::X86Sse41RoundPd => {
            ("intrinsic:llvm.x86.sse41.round.pd", X, &[X, I32], Need::Sse41, Op::ExtImm(E::Roundpd))
        }
        I::X86Sse41RoundSs => (
            "intrinsic:llvm.x86.sse41.round.ss",
            X,
            &[X, X, I32],
            Need::Sse41,
            Op::ExtImm(E::Roundss),
        ),
        I::X86Sse41RoundSd => (
            "intrinsic:llvm.x86.sse41.round.sd",
            X,
            &[X, X, I32],
            Need::Sse41,
            Op::ExtImm(E::Roundsd),
        ),
        I::X86Sse41Blendvps => {
            ("intrinsic:llvm.x86.sse41.blendvps", X, &[X, X, X], Need::Sse41, Op::Blend128(Lane::D))
        }
        I::X86Sse41Blendvpd => {
            ("intrinsic:llvm.x86.sse41.blendvpd", X, &[X, X, X], Need::Sse41, Op::Blend128(Lane::Q))
        }
        I::X86Sse41Pblendvb => {
            ("intrinsic:llvm.x86.sse41.pblendvb", X, &[X, X, X], Need::Sse41, Op::Blend128(Lane::B))
        }
        I::X86Sse41Dpps => {
            ("intrinsic:llvm.x86.sse41.dpps", X, &[X, X, I8], Need::Sse41, Op::ExtImm(E::Dpps))
        }
        I::X86Sse41Dppd => {
            ("intrinsic:llvm.x86.sse41.dppd", X, &[X, X, I8], Need::Sse41, Op::ExtImm(E::Dppd))
        }
        I::X86Sse41Insertps => (
            "intrinsic:llvm.x86.sse41.insertps",
            X,
            &[X, X, I8],
            Need::Sse41,
            Op::ExtImm(E::Insertps),
        ),
        I::X86Sse41Mpsadbw => (
            "intrinsic:llvm.x86.sse41.mpsadbw",
            X,
            &[X, X, I8],
            Need::Sse41,
            Op::ExtImm(E::Mpsadbw),
        ),
        I::X86Sse41Packusdw => {
            ("intrinsic:llvm.x86.sse41.packusdw", X, &[X, X], Need::Sse41, Op::Ext(Xt::Packusdw))
        }
        I::X86Sse41Phminposuw => {
            ("intrinsic:llvm.x86.sse41.phminposuw", X, &[X], Need::Sse41, Op::Ext(Xt::Phminposuw))
        }
        I::X86Sse41Ptestz => {
            ("intrinsic:llvm.x86.sse41.ptestz", I32, &[X, X], Need::Sse41, Op::Test(None, Cond::E))
        }
        I::X86Sse41Ptestc => {
            ("intrinsic:llvm.x86.sse41.ptestc", I32, &[X, X], Need::Sse41, Op::Test(None, Cond::B))
        }
        I::X86Sse41Ptestnzc => (
            "intrinsic:llvm.x86.sse41.ptestnzc",
            I32,
            &[X, X],
            Need::Sse41,
            Op::Test(None, Cond::A),
        ),
        I::X86Sse42Pcmpistrm128 => (
            "intrinsic:llvm.x86.sse42.pcmpistrm128",
            X,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistrm, Out::Mask),
        ),
        I::X86Sse42Pcmpistri128 => (
            "intrinsic:llvm.x86.sse42.pcmpistri128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Index),
        ),
        I::X86Sse42Pcmpistria128 => (
            "intrinsic:llvm.x86.sse42.pcmpistria128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Flag(Cond::A)),
        ),
        I::X86Sse42Pcmpistric128 => (
            "intrinsic:llvm.x86.sse42.pcmpistric128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Flag(Cond::B)),
        ),
        I::X86Sse42Pcmpistrio128 => (
            "intrinsic:llvm.x86.sse42.pcmpistrio128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Flag(Cond::O)),
        ),
        I::X86Sse42Pcmpistris128 => (
            "intrinsic:llvm.x86.sse42.pcmpistris128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Flag(Cond::S)),
        ),
        I::X86Sse42Pcmpistriz128 => (
            "intrinsic:llvm.x86.sse42.pcmpistriz128",
            I32,
            &[X, X, I8],
            Need::Sse42,
            Op::Str(E::Pcmpistri, Out::Flag(Cond::E)),
        ),
        I::X86Sse42Pcmpestrm128 => (
            "intrinsic:llvm.x86.sse42.pcmpestrm128",
            X,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestrm, Out::Mask),
        ),
        I::X86Sse42Pcmpestri128 => (
            "intrinsic:llvm.x86.sse42.pcmpestri128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Index),
        ),
        I::X86Sse42Pcmpestria128 => (
            "intrinsic:llvm.x86.sse42.pcmpestria128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Flag(Cond::A)),
        ),
        I::X86Sse42Pcmpestric128 => (
            "intrinsic:llvm.x86.sse42.pcmpestric128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Flag(Cond::B)),
        ),
        I::X86Sse42Pcmpestrio128 => (
            "intrinsic:llvm.x86.sse42.pcmpestrio128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Flag(Cond::O)),
        ),
        I::X86Sse42Pcmpestris128 => (
            "intrinsic:llvm.x86.sse42.pcmpestris128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Flag(Cond::S)),
        ),
        I::X86Sse42Pcmpestriz128 => (
            "intrinsic:llvm.x86.sse42.pcmpestriz128",
            I32,
            &[X, I32, X, I32, I8],
            Need::Sse42,
            Op::Str(E::Pcmpestri, Out::Flag(Cond::E)),
        ),
        I::X86AesniAesenc => {
            ("intrinsic:llvm.x86.aesni.aesenc", X, &[X, X], Need::Aes, Op::Ext(Xt::Aesenc))
        }
        I::X86AesniAesenclast => {
            ("intrinsic:llvm.x86.aesni.aesenclast", X, &[X, X], Need::Aes, Op::Ext(Xt::Aesenclast))
        }
        I::X86AesniAesdec => {
            ("intrinsic:llvm.x86.aesni.aesdec", X, &[X, X], Need::Aes, Op::Ext(Xt::Aesdec))
        }
        I::X86AesniAesdeclast => {
            ("intrinsic:llvm.x86.aesni.aesdeclast", X, &[X, X], Need::Aes, Op::Ext(Xt::Aesdeclast))
        }
        I::X86AesniAesimc => {
            ("intrinsic:llvm.x86.aesni.aesimc", X, &[X], Need::Aes, Op::Ext(Xt::Aesimc))
        }
        I::X86AesniAeskeygenassist => (
            "intrinsic:llvm.x86.aesni.aeskeygenassist",
            X,
            &[X, I8],
            Need::Aes,
            Op::ExtImm(E::Aeskeygenassist),
        ),
        I::X86Pclmulqdq => {
            ("intrinsic:llvm.x86.pclmulqdq", X, &[X, X, I8], Need::Pclmul, Op::ExtImm(E::Pclmulqdq))
        }
        I::X86Sha1rnds4 => {
            ("intrinsic:llvm.x86.sha1rnds4", X, &[X, X, I8], Need::Sha, Op::ExtImm(E::Sha1rnds4))
        }
        I::X86Sha1nexte => {
            ("intrinsic:llvm.x86.sha1nexte", X, &[X, X], Need::Sha, Op::Ext(Xt::Sha1nexte))
        }
        I::X86Sha1msg1 => {
            ("intrinsic:llvm.x86.sha1msg1", X, &[X, X], Need::Sha, Op::Ext(Xt::Sha1msg1))
        }
        I::X86Sha1msg2 => {
            ("intrinsic:llvm.x86.sha1msg2", X, &[X, X], Need::Sha, Op::Ext(Xt::Sha1msg2))
        }
        I::X86Sha256msg1 => {
            ("intrinsic:llvm.x86.sha256msg1", X, &[X, X], Need::Sha, Op::Ext(Xt::Sha256msg1))
        }
        I::X86Sha256msg2 => {
            ("intrinsic:llvm.x86.sha256msg2", X, &[X, X], Need::Sha, Op::Ext(Xt::Sha256msg2))
        }
        I::X86Sha256rnds2 => {
            ("intrinsic:llvm.x86.sha256rnds2", X, &[X, X, X], Need::Sha, Op::Sha256rnds2)
        }
        I::X86AvxAddsubPd256 => {
            ("intrinsic:llvm.x86.avx.addsub.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Addsubpd))
        }
        I::X86AvxAddsubPs256 => {
            ("intrinsic:llvm.x86.avx.addsub.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Addsubps))
        }
        I::X86AvxMaxPd256 => {
            ("intrinsic:llvm.x86.avx.max.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Maxpd))
        }
        I::X86AvxMaxPs256 => {
            ("intrinsic:llvm.x86.avx.max.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Maxps))
        }
        I::X86AvxMinPd256 => {
            ("intrinsic:llvm.x86.avx.min.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Minpd))
        }
        I::X86AvxMinPs256 => {
            ("intrinsic:llvm.x86.avx.min.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Minps))
        }
        I::X86AvxHaddPd256 => {
            ("intrinsic:llvm.x86.avx.hadd.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Haddpd))
        }
        I::X86AvxHaddPs256 => {
            ("intrinsic:llvm.x86.avx.hadd.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Haddps))
        }
        I::X86AvxHsubPd256 => {
            ("intrinsic:llvm.x86.avx.hsub.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Hsubpd))
        }
        I::X86AvxHsubPs256 => {
            ("intrinsic:llvm.x86.avx.hsub.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Hsubps))
        }
        I::X86AvxBlendvPd256 => (
            "intrinsic:llvm.x86.avx.blendv.pd.256",
            Y,
            &[Y, Y, Y],
            Need::Avx,
            Op::VBlend(VBlend::Blendvpd),
        ),
        I::X86AvxBlendvPs256 => (
            "intrinsic:llvm.x86.avx.blendv.ps.256",
            Y,
            &[Y, Y, Y],
            Need::Avx,
            Op::VBlend(VBlend::Blendvps),
        ),
        I::X86AvxDpPs256 => {
            ("intrinsic:llvm.x86.avx.dp.ps.256", Y, &[Y, Y, I8], Need::Avx, Op::V3Imm(V3Imm::Dpps))
        }
        I::X86AvxCmpPd256 => (
            "intrinsic:llvm.x86.avx.cmp.pd.256",
            Y,
            &[Y, Y, I8],
            Need::Avx,
            Op::V3Imm(V3Imm::Cmppd),
        ),
        I::X86AvxCmpPs256 => (
            "intrinsic:llvm.x86.avx.cmp.ps.256",
            Y,
            &[Y, Y, I8],
            Need::Avx,
            Op::V3Imm(V3Imm::Cmpps),
        ),
        I::X86AvxCvtPd2Ps256 => {
            ("intrinsic:llvm.x86.avx.cvt.pd2.ps.256", X, &[Y], Need::Avx, Op::V2(V2::Cvtpd2ps))
        }
        I::X86AvxCvtPs2dq256 => {
            ("intrinsic:llvm.x86.avx.cvt.ps2dq.256", Y, &[Y], Need::Avx, Op::V2(V2::Cvtps2dq))
        }
        I::X86AvxCvtPd2dq256 => {
            ("intrinsic:llvm.x86.avx.cvt.pd2dq.256", X, &[Y], Need::Avx, Op::V2(V2::Cvtpd2dq))
        }
        I::X86AvxCvttPd2dq256 => {
            ("intrinsic:llvm.x86.avx.cvtt.pd2dq.256", X, &[Y], Need::Avx, Op::V2(V2::Cvttpd2dq))
        }
        I::X86AvxCvttPs2dq256 => {
            ("intrinsic:llvm.x86.avx.cvtt.ps2dq.256", Y, &[Y], Need::Avx, Op::V2(V2::Cvttps2dq))
        }
        I::X86AvxRcpPs256 => {
            ("intrinsic:llvm.x86.avx.rcp.ps.256", Y, &[Y], Need::Avx, Op::V2(V2::Rcpps))
        }
        I::X86AvxRsqrtPs256 => {
            ("intrinsic:llvm.x86.avx.rsqrt.ps.256", Y, &[Y], Need::Avx, Op::V2(V2::Rsqrtps))
        }
        I::X86AvxRoundPd256 => (
            "intrinsic:llvm.x86.avx.round.pd.256",
            Y,
            &[Y, I32],
            Need::Avx,
            Op::V2Imm(V2Imm::Roundpd),
        ),
        I::X86AvxRoundPs256 => (
            "intrinsic:llvm.x86.avx.round.ps.256",
            Y,
            &[Y, I32],
            Need::Avx,
            Op::V2Imm(V2Imm::Roundps),
        ),
        I::X86AvxVpermilvarPs => {
            ("intrinsic:llvm.x86.avx.vpermilvar.ps", X, &[X, X], Need::Avx, v3(V3::Permilps))
        }
        I::X86AvxVpermilvarPd => {
            ("intrinsic:llvm.x86.avx.vpermilvar.pd", X, &[X, X], Need::Avx, v3(V3::Permilpd))
        }
        I::X86AvxVpermilvarPs256 => {
            ("intrinsic:llvm.x86.avx.vpermilvar.ps.256", Y, &[Y, Y], Need::Avx, v3(V3::Permilps))
        }
        I::X86AvxVpermilvarPd256 => {
            ("intrinsic:llvm.x86.avx.vpermilvar.pd.256", Y, &[Y, Y], Need::Avx, v3(V3::Permilpd))
        }
        I::X86AvxVtestzPs => (
            "intrinsic:llvm.x86.avx.vtestz.ps",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::E),
        ),
        I::X86AvxVtestzPd => (
            "intrinsic:llvm.x86.avx.vtestz.pd",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::E),
        ),
        I::X86AvxVtestzPs256 => (
            "intrinsic:llvm.x86.avx.vtestz.ps.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::E),
        ),
        I::X86AvxVtestzPd256 => (
            "intrinsic:llvm.x86.avx.vtestz.pd.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::E),
        ),
        I::X86AvxVtestcPs => (
            "intrinsic:llvm.x86.avx.vtestc.ps",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::B),
        ),
        I::X86AvxVtestcPd => (
            "intrinsic:llvm.x86.avx.vtestc.pd",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::B),
        ),
        I::X86AvxVtestcPs256 => (
            "intrinsic:llvm.x86.avx.vtestc.ps.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::B),
        ),
        I::X86AvxVtestcPd256 => (
            "intrinsic:llvm.x86.avx.vtestc.pd.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::B),
        ),
        I::X86AvxVtestnzcPs => (
            "intrinsic:llvm.x86.avx.vtestnzc.ps",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::A),
        ),
        I::X86AvxVtestnzcPd => (
            "intrinsic:llvm.x86.avx.vtestnzc.pd",
            I32,
            &[X, X],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::A),
        ),
        I::X86AvxVtestnzcPs256 => (
            "intrinsic:llvm.x86.avx.vtestnzc.ps.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testps), Cond::A),
        ),
        I::X86AvxVtestnzcPd256 => (
            "intrinsic:llvm.x86.avx.vtestnzc.pd.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Testpd), Cond::A),
        ),
        I::X86AvxPtestz256 => (
            "intrinsic:llvm.x86.avx.ptestz.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Ptest), Cond::E),
        ),
        I::X86AvxPtestc256 => (
            "intrinsic:llvm.x86.avx.ptestc.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Ptest), Cond::B),
        ),
        I::X86AvxPtestnzc256 => (
            "intrinsic:llvm.x86.avx.ptestnzc.256",
            I32,
            &[Y, Y],
            Need::Avx,
            Op::Test(Some(V2::Ptest), Cond::A),
        ),
        I::X86AvxMovmskPd256 => {
            ("intrinsic:llvm.x86.avx.movmsk.pd.256", I32, &[Y], Need::Avx, Op::Movmsk(Msk::Pd))
        }
        I::X86AvxMovmskPs256 => {
            ("intrinsic:llvm.x86.avx.movmsk.ps.256", I32, &[Y], Need::Avx, Op::Movmsk(Msk::Ps))
        }
        I::X86AvxLduDq256 => ("intrinsic:llvm.x86.avx.ldu.dq.256", Y, &[P], Need::Avx, Op::LoadU),
        I::X86AvxMaskloadPs => (
            "intrinsic:llvm.x86.avx.maskload.ps",
            X,
            &[P, X],
            Need::Avx,
            Op::MaskLoad(VMask::Maskmovps),
        ),
        I::X86AvxMaskstorePs => (
            "intrinsic:llvm.x86.avx.maskstore.ps",
            Void,
            &[P, X, X],
            Need::Avx,
            Op::MaskStore(VMask::Maskmovps),
        ),
        I::X86AvxMaskloadPd => (
            "intrinsic:llvm.x86.avx.maskload.pd",
            X,
            &[P, X],
            Need::Avx,
            Op::MaskLoad(VMask::Maskmovpd),
        ),
        I::X86AvxMaskstorePd => (
            "intrinsic:llvm.x86.avx.maskstore.pd",
            Void,
            &[P, X, X],
            Need::Avx,
            Op::MaskStore(VMask::Maskmovpd),
        ),
        I::X86AvxMaskloadPs256 => (
            "intrinsic:llvm.x86.avx.maskload.ps.256",
            Y,
            &[P, Y],
            Need::Avx,
            Op::MaskLoad(VMask::Maskmovps),
        ),
        I::X86AvxMaskstorePs256 => (
            "intrinsic:llvm.x86.avx.maskstore.ps.256",
            Void,
            &[P, Y, Y],
            Need::Avx,
            Op::MaskStore(VMask::Maskmovps),
        ),
        I::X86AvxMaskloadPd256 => (
            "intrinsic:llvm.x86.avx.maskload.pd.256",
            Y,
            &[P, Y],
            Need::Avx,
            Op::MaskLoad(VMask::Maskmovpd),
        ),
        I::X86AvxMaskstorePd256 => (
            "intrinsic:llvm.x86.avx.maskstore.pd.256",
            Void,
            &[P, Y, Y],
            Need::Avx,
            Op::MaskStore(VMask::Maskmovpd),
        ),
        I::X86AvxVzeroall => {
            ("intrinsic:llvm.x86.avx.vzeroall", Void, &[], Need::Avx, Op::Zeroupper)
        }
        I::X86AvxVzeroupper => {
            ("intrinsic:llvm.x86.avx.vzeroupper", Void, &[], Need::Avx, Op::Zeroupper)
        }
        I::X86Avx2Packssdw => {
            ("intrinsic:llvm.x86.avx2.packssdw", Y, &[Y, Y], Need::Avx2, v3(V3::Packssdw))
        }
        I::X86Avx2Packsswb => {
            ("intrinsic:llvm.x86.avx2.packsswb", Y, &[Y, Y], Need::Avx2, v3(V3::Packsswb))
        }
        I::X86Avx2Packusdw => {
            ("intrinsic:llvm.x86.avx2.packusdw", Y, &[Y, Y], Need::Avx2, v3(V3::Packusdw))
        }
        I::X86Avx2Packuswb => {
            ("intrinsic:llvm.x86.avx2.packuswb", Y, &[Y, Y], Need::Avx2, v3(V3::Packuswb))
        }
        I::X86Avx2PavgB => {
            ("intrinsic:llvm.x86.avx2.pavg.b", Y, &[Y, Y], Need::Avx2, v3(V3::Pavgb))
        }
        I::X86Avx2PavgW => {
            ("intrinsic:llvm.x86.avx2.pavg.w", Y, &[Y, Y], Need::Avx2, v3(V3::Pavgw))
        }
        I::X86Avx2Pblendvb => (
            "intrinsic:llvm.x86.avx2.pblendvb",
            Y,
            &[Y, Y, Y],
            Need::Avx2,
            Op::VBlend(VBlend::Pblendvb),
        ),
        I::X86Avx2PhaddD => {
            ("intrinsic:llvm.x86.avx2.phadd.d", Y, &[Y, Y], Need::Avx2, v3(V3::Phaddd))
        }
        I::X86Avx2PhaddW => {
            ("intrinsic:llvm.x86.avx2.phadd.w", Y, &[Y, Y], Need::Avx2, v3(V3::Phaddw))
        }
        I::X86Avx2PhaddSw => {
            ("intrinsic:llvm.x86.avx2.phadd.sw", Y, &[Y, Y], Need::Avx2, v3(V3::Phaddsw))
        }
        I::X86Avx2PhsubD => {
            ("intrinsic:llvm.x86.avx2.phsub.d", Y, &[Y, Y], Need::Avx2, v3(V3::Phsubd))
        }
        I::X86Avx2PhsubW => {
            ("intrinsic:llvm.x86.avx2.phsub.w", Y, &[Y, Y], Need::Avx2, v3(V3::Phsubw))
        }
        I::X86Avx2PhsubSw => {
            ("intrinsic:llvm.x86.avx2.phsub.sw", Y, &[Y, Y], Need::Avx2, v3(V3::Phsubsw))
        }
        I::X86Avx2PmaddUbSw => {
            ("intrinsic:llvm.x86.avx2.pmadd.ub.sw", Y, &[Y, Y], Need::Avx2, v3(V3::Pmaddubsw))
        }
        I::X86Avx2PmaddWd => {
            ("intrinsic:llvm.x86.avx2.pmadd.wd", Y, &[Y, Y], Need::Avx2, v3(V3::Pmaddwd))
        }
        I::X86Avx2Pmovmskb => {
            ("intrinsic:llvm.x86.avx2.pmovmskb", I32, &[Y], Need::Avx2, Op::Movmsk(Msk::B))
        }
        I::X86Avx2PmulHrSw => {
            ("intrinsic:llvm.x86.avx2.pmul.hr.sw", Y, &[Y, Y], Need::Avx2, v3(V3::Pmulhrsw))
        }
        I::X86Avx2PmulhW => {
            ("intrinsic:llvm.x86.avx2.pmulh.w", Y, &[Y, Y], Need::Avx2, v3(V3::Pmulhw))
        }
        I::X86Avx2PmulhuW => {
            ("intrinsic:llvm.x86.avx2.pmulhu.w", Y, &[Y, Y], Need::Avx2, v3(V3::Pmulhuw))
        }
        I::X86Avx2PsadBw => {
            ("intrinsic:llvm.x86.avx2.psad.bw", Y, &[Y, Y], Need::Avx2, v3(V3::Psadbw))
        }
        I::X86Avx2PshufB => {
            ("intrinsic:llvm.x86.avx2.pshuf.b", Y, &[Y, Y], Need::Avx2, v3(V3::Pshufb))
        }
        I::X86Avx2PsignB => {
            ("intrinsic:llvm.x86.avx2.psign.b", Y, &[Y, Y], Need::Avx2, v3(V3::Psignb))
        }
        I::X86Avx2PsignW => {
            ("intrinsic:llvm.x86.avx2.psign.w", Y, &[Y, Y], Need::Avx2, v3(V3::Psignw))
        }
        I::X86Avx2PsignD => {
            ("intrinsic:llvm.x86.avx2.psign.d", Y, &[Y, Y], Need::Avx2, v3(V3::Psignd))
        }
        I::X86Avx2PsllW => {
            ("intrinsic:llvm.x86.avx2.psll.w", Y, &[Y, X], Need::Avx2, v3(V3::Psllw))
        }
        I::X86Avx2PsllD => {
            ("intrinsic:llvm.x86.avx2.psll.d", Y, &[Y, X], Need::Avx2, v3(V3::Pslld))
        }
        I::X86Avx2PsllQ => {
            ("intrinsic:llvm.x86.avx2.psll.q", Y, &[Y, X], Need::Avx2, v3(V3::Psllq))
        }
        I::X86Avx2PslliW => (
            "intrinsic:llvm.x86.avx2.pslli.w",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psllw, V3::Psllw),
        ),
        I::X86Avx2PslliD => (
            "intrinsic:llvm.x86.avx2.pslli.d",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Pslld, V3::Pslld),
        ),
        I::X86Avx2PslliQ => (
            "intrinsic:llvm.x86.avx2.pslli.q",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psllq, V3::Psllq),
        ),
        I::X86Avx2PsrlW => {
            ("intrinsic:llvm.x86.avx2.psrl.w", Y, &[Y, X], Need::Avx2, v3(V3::Psrlw))
        }
        I::X86Avx2PsrlD => {
            ("intrinsic:llvm.x86.avx2.psrl.d", Y, &[Y, X], Need::Avx2, v3(V3::Psrld))
        }
        I::X86Avx2PsrlQ => {
            ("intrinsic:llvm.x86.avx2.psrl.q", Y, &[Y, X], Need::Avx2, v3(V3::Psrlq))
        }
        I::X86Avx2PsrliW => (
            "intrinsic:llvm.x86.avx2.psrli.w",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psrlw, V3::Psrlw),
        ),
        I::X86Avx2PsrliD => (
            "intrinsic:llvm.x86.avx2.psrli.d",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psrld, V3::Psrld),
        ),
        I::X86Avx2PsrliQ => (
            "intrinsic:llvm.x86.avx2.psrli.q",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psrlq, V3::Psrlq),
        ),
        I::X86Avx2PsraW => {
            ("intrinsic:llvm.x86.avx2.psra.w", Y, &[Y, X], Need::Avx2, v3(V3::Psraw))
        }
        I::X86Avx2PsraD => {
            ("intrinsic:llvm.x86.avx2.psra.d", Y, &[Y, X], Need::Avx2, v3(V3::Psrad))
        }
        I::X86Avx2PsraiW => (
            "intrinsic:llvm.x86.avx2.psrai.w",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psraw, V3::Psraw),
        ),
        I::X86Avx2PsraiD => (
            "intrinsic:llvm.x86.avx2.psrai.d",
            Y,
            &[Y, I32],
            Need::Avx2,
            Op::ShiftI(VShift::Psrad, V3::Psrad),
        ),
        I::X86Avx2PsllvD => {
            ("intrinsic:llvm.x86.avx2.psllv.d", X, &[X, X], Need::Avx2, v3(V3::Psllvd))
        }
        I::X86Avx2PsllvD256 => {
            ("intrinsic:llvm.x86.avx2.psllv.d.256", Y, &[Y, Y], Need::Avx2, v3(V3::Psllvd))
        }
        I::X86Avx2PsllvQ => {
            ("intrinsic:llvm.x86.avx2.psllv.q", X, &[X, X], Need::Avx2, v3(V3::Psllvq))
        }
        I::X86Avx2PsllvQ256 => {
            ("intrinsic:llvm.x86.avx2.psllv.q.256", Y, &[Y, Y], Need::Avx2, v3(V3::Psllvq))
        }
        I::X86Avx2PsrlvD => {
            ("intrinsic:llvm.x86.avx2.psrlv.d", X, &[X, X], Need::Avx2, v3(V3::Psrlvd))
        }
        I::X86Avx2PsrlvD256 => {
            ("intrinsic:llvm.x86.avx2.psrlv.d.256", Y, &[Y, Y], Need::Avx2, v3(V3::Psrlvd))
        }
        I::X86Avx2PsrlvQ => {
            ("intrinsic:llvm.x86.avx2.psrlv.q", X, &[X, X], Need::Avx2, v3(V3::Psrlvq))
        }
        I::X86Avx2PsrlvQ256 => {
            ("intrinsic:llvm.x86.avx2.psrlv.q.256", Y, &[Y, Y], Need::Avx2, v3(V3::Psrlvq))
        }
        I::X86Avx2PsravD => {
            ("intrinsic:llvm.x86.avx2.psrav.d", X, &[X, X], Need::Avx2, v3(V3::Psravd))
        }
        I::X86Avx2PsravD256 => {
            ("intrinsic:llvm.x86.avx2.psrav.d.256", Y, &[Y, Y], Need::Avx2, v3(V3::Psravd))
        }
        I::X86Avx2Permd => {
            ("intrinsic:llvm.x86.avx2.permd", Y, &[Y, Y], Need::Avx2, Op::V3(V3::Permd, true))
        }
        I::X86Avx2Permps => {
            ("intrinsic:llvm.x86.avx2.permps", Y, &[Y, Y], Need::Avx2, Op::V3(V3::Permps, true))
        }
        I::X86Avx2Mpsadbw => (
            "intrinsic:llvm.x86.avx2.mpsadbw",
            Y,
            &[Y, Y, I8],
            Need::Avx2,
            Op::V3Imm(V3Imm::Mpsadbw),
        ),
        I::X86Avx2MaskloadD => (
            "intrinsic:llvm.x86.avx2.maskload.d",
            X,
            &[P, X],
            Need::Avx2,
            Op::MaskLoad(VMask::Pmaskmovd),
        ),
        I::X86Avx2MaskstoreD => (
            "intrinsic:llvm.x86.avx2.maskstore.d",
            Void,
            &[P, X, X],
            Need::Avx2,
            Op::MaskStore(VMask::Pmaskmovd),
        ),
        I::X86Avx2MaskloadQ => (
            "intrinsic:llvm.x86.avx2.maskload.q",
            X,
            &[P, X],
            Need::Avx2,
            Op::MaskLoad(VMask::Pmaskmovq),
        ),
        I::X86Avx2MaskstoreQ => (
            "intrinsic:llvm.x86.avx2.maskstore.q",
            Void,
            &[P, X, X],
            Need::Avx2,
            Op::MaskStore(VMask::Pmaskmovq),
        ),
        I::X86Avx2MaskloadD256 => (
            "intrinsic:llvm.x86.avx2.maskload.d.256",
            Y,
            &[P, Y],
            Need::Avx2,
            Op::MaskLoad(VMask::Pmaskmovd),
        ),
        I::X86Avx2MaskstoreD256 => (
            "intrinsic:llvm.x86.avx2.maskstore.d.256",
            Void,
            &[P, Y, Y],
            Need::Avx2,
            Op::MaskStore(VMask::Pmaskmovd),
        ),
        I::X86Avx2MaskloadQ256 => (
            "intrinsic:llvm.x86.avx2.maskload.q.256",
            Y,
            &[P, Y],
            Need::Avx2,
            Op::MaskLoad(VMask::Pmaskmovq),
        ),
        I::X86Avx2MaskstoreQ256 => (
            "intrinsic:llvm.x86.avx2.maskstore.q.256",
            Void,
            &[P, Y, Y],
            Need::Avx2,
            Op::MaskStore(VMask::Pmaskmovq),
        ),
        I::X86FmaVfmaddsubPs => (
            "intrinsic:llvm.x86.fma.vfmaddsub.ps",
            X,
            &[X, X, X],
            Need::Fma,
            Op::Fma(Fma::Vfmaddsub213ps),
        ),
        I::X86FmaVfmaddsubPd => (
            "intrinsic:llvm.x86.fma.vfmaddsub.pd",
            X,
            &[X, X, X],
            Need::Fma,
            Op::Fma(Fma::Vfmaddsub213pd),
        ),
        I::X86FmaVfmaddsubPs256 => (
            "intrinsic:llvm.x86.fma.vfmaddsub.ps.256",
            Y,
            &[Y, Y, Y],
            Need::Fma,
            Op::Fma(Fma::Vfmaddsub213ps),
        ),
        I::X86FmaVfmaddsubPd256 => (
            "intrinsic:llvm.x86.fma.vfmaddsub.pd.256",
            Y,
            &[Y, Y, Y],
            Need::Fma,
            Op::Fma(Fma::Vfmaddsub213pd),
        ),
        I::X86Vcvtps2ph128 => {
            ("intrinsic:llvm.x86.vcvtps2ph.128", X, &[X, I32], Need::F16c, Op::Cvtps2ph)
        }
        I::X86Vcvtps2ph256 => {
            ("intrinsic:llvm.x86.vcvtps2ph.256", X, &[Y, I32], Need::F16c, Op::Cvtps2ph)
        }
        I::X86Vgf2p8affineqb128 => (
            "intrinsic:llvm.x86.vgf2p8affineqb.128",
            X,
            &[X, X, I8],
            Need::Gfni,
            Op::ExtImm(E::Gf2p8affineqb),
        ),
        I::X86Vgf2p8affineqb256 => (
            "intrinsic:llvm.x86.vgf2p8affineqb.256",
            Y,
            &[Y, Y, I8],
            Need::GfniAvx,
            Op::V3Imm(V3Imm::Gf2p8affineqb),
        ),
        I::X86Vgf2p8affineinvqb128 => (
            "intrinsic:llvm.x86.vgf2p8affineinvqb.128",
            X,
            &[X, X, I8],
            Need::Gfni,
            Op::ExtImm(E::Gf2p8affineinvqb),
        ),
        I::X86Vgf2p8affineinvqb256 => (
            "intrinsic:llvm.x86.vgf2p8affineinvqb.256",
            Y,
            &[Y, Y, I8],
            Need::GfniAvx,
            Op::V3Imm(V3Imm::Gf2p8affineinvqb),
        ),
        I::X86Vgf2p8mulb128 => {
            ("intrinsic:llvm.x86.vgf2p8mulb.128", X, &[X, X], Need::Gfni, Op::Ext(Xt::Gf2p8mulb))
        }
        I::X86Vgf2p8mulb256 => {
            ("intrinsic:llvm.x86.vgf2p8mulb.256", Y, &[Y, Y], Need::GfniAvx, v3(V3::Gf2p8mulb))
        }
        I::X86Avx2GatherDD => (
            "intrinsic:llvm.x86.avx2.gather.d.d",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherdd),
        ),
        I::X86Avx2GatherDD256 => (
            "intrinsic:llvm.x86.avx2.gather.d.d.256",
            Y,
            &[Y, P, Y, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherdd),
        ),
        I::X86Avx2GatherDQ => (
            "intrinsic:llvm.x86.avx2.gather.d.q",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherdq),
        ),
        I::X86Avx2GatherDQ256 => (
            "intrinsic:llvm.x86.avx2.gather.d.q.256",
            Y,
            &[Y, P, X, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherdq),
        ),
        I::X86Avx2GatherQD => (
            "intrinsic:llvm.x86.avx2.gather.q.d",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherqd),
        ),
        I::X86Avx2GatherQD256 => (
            "intrinsic:llvm.x86.avx2.gather.q.d.256",
            X,
            &[X, P, Y, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherqd),
        ),
        I::X86Avx2GatherQQ => (
            "intrinsic:llvm.x86.avx2.gather.q.q",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherqq),
        ),
        I::X86Avx2GatherQQ256 => (
            "intrinsic:llvm.x86.avx2.gather.q.q.256",
            Y,
            &[Y, P, Y, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Pgatherqq),
        ),
        I::X86Avx2GatherDPs => (
            "intrinsic:llvm.x86.avx2.gather.d.ps",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherdps),
        ),
        I::X86Avx2GatherDPs256 => (
            "intrinsic:llvm.x86.avx2.gather.d.ps.256",
            Y,
            &[Y, P, Y, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherdps),
        ),
        I::X86Avx2GatherDPd => (
            "intrinsic:llvm.x86.avx2.gather.d.pd",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherdpd),
        ),
        I::X86Avx2GatherDPd256 => (
            "intrinsic:llvm.x86.avx2.gather.d.pd.256",
            Y,
            &[Y, P, X, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherdpd),
        ),
        I::X86Avx2GatherQPs => (
            "intrinsic:llvm.x86.avx2.gather.q.ps",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherqps),
        ),
        I::X86Avx2GatherQPs256 => (
            "intrinsic:llvm.x86.avx2.gather.q.ps.256",
            X,
            &[X, P, Y, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherqps),
        ),
        I::X86Avx2GatherQPd => (
            "intrinsic:llvm.x86.avx2.gather.q.pd",
            X,
            &[X, P, X, X, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherqpd),
        ),
        I::X86Avx2GatherQPd256 => (
            "intrinsic:llvm.x86.avx2.gather.q.pd.256",
            Y,
            &[Y, P, Y, Y, I8],
            Need::Avx2,
            Op::Gather(mc::Gather::Gatherqpd),
        ),
        _ => return None,
    })
}

/// Whether `ty` is what `k` says.
fn fits(cx: &FnCx<'_, X64>, ty: TypeId, k: K) -> bool {
    let types = cx.types();
    match (k, types.kind(ty)) {
        (K::X | K::Y, TypeKind::Vector { elem, len }) => {
            let bits = match types.kind(elem) {
                TypeKind::Int(bits) => bits,
                TypeKind::Float => 32,
                TypeKind::Double => 64,
                _ => return false,
            };
            let want = if k == K::X { 128 } else { 256 };
            u64::from(bits) * u64::from(len) == want && matches!(bits, 8 | 16 | 32 | 64)
        }
        (K::I32, TypeKind::Int(32))
        | (K::I8, TypeKind::Int(8))
        | (K::P, TypeKind::Ptr { .. })
        | (K::Void, TypeKind::Void) => true,
        _ => false,
    }
}

/// The class a value of shape `k` has.
fn class(k: K) -> Option<Class> {
    match k {
        K::X => Some(Class::Fpr),
        K::Y => Some(Class::Mem),
        _ => None,
    }
}

/// The stable name `which` is refused with in a function whose features
/// do not enable it, if it is one of this module's.
pub fn refusal(which: Intrinsic<'_>) -> Option<&'static str> {
    row(which).map(|row| row.0)
}

/// What `which` needs, if it is one of this module's.
pub fn need(which: Intrinsic<'_>) -> Option<Need> {
    row(which).map(|row| row.3)
}

/// A call of `which` with `args`, if `which` is an intrinsic of this
/// module.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> Option<CgResult<()>> {
    let row = row(which)?;
    Some(lower(cx, inst, args, row))
}

#[allow(clippy::too_many_lines, reason = "one arm per kind of row")]
fn lower<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    args: &'a [Value],
    (refused, ret, params, need, op): Row,
) -> CgResult<()> {
    let wrong_class =
        |cx: &FnCx<'a, X64>, a: Value, k: K| class(k).is_some_and(|want| cx.class_of(a) != want);
    if args.len() != params.len()
        || !fits(cx, inst.ty(), ret)
        || args.iter().zip(params).any(|(&a, &k)| !fits(cx, value_ty(cx, a), k))
        || args.iter().zip(params).any(|(&a, &k)| wrong_class(cx, a, k))
    {
        return unsupported(super::SIGNATURE);
    }
    if !has(cx, need) {
        return unsupported(refused);
    }
    deferred_first(cx, inst, None)?;
    // The immediate: the last operand, where the row has one.
    let imm = |cx: &FnCx<'a, X64>| -> CgResult<u8> {
        let last = *args.last().unwrap_or(&args[0]);
        match const_int(cx, last) {
            Some(v) => Ok((v & 0xff) as u8),
            None => cx.bug("an x86 intrinsic's immediate that is not a constant"),
        }
    };
    let vectors = params.iter().filter(|&&k| matches!(k, K::X | K::Y)).count();
    match op {
        Op::Ext(ext) => {
            if vectors == 1 {
                let a = cx.use_reg(args[0])?;
                put(cx, mc::ext_rr(ext, xm(a), xm(a)));
                return cx.finish(inst, a);
            }
            let [a, b] = xregs(cx, [args[0], args[1]])?;
            put(cx, mc::ext_rr(ext, xm(a), xm(b)));
            cx.finish(inst, a)
        }
        Op::ExtImm(ext) => {
            let imm = imm(cx)?;
            if vectors == 1 {
                let a = cx.use_reg(args[0])?;
                put(cx, mc::ext_rri(ext, xm(a), xm(a), imm));
                return cx.finish(inst, a);
            }
            let [a, b] = xregs(cx, [args[0], args[1]])?;
            put(cx, mc::ext_rri(ext, xm(a), xm(b), imm));
            cx.finish(inst, a)
        }
        Op::Test(vex, cond) => {
            if params[0] == K::Y {
                let Some(op) = vex else { return cx.bug("a 256-bit legacy test") };
                let (a, b) = (cx.use_mem(args[0])?, cx.use_mem(args[1])?);
                load_y(cx, XMM14, a);
                frame_inst(cx, b, |m| mc::v2_rm(op, Width::Y, XMM14, m));
            } else {
                let [a, b] = xregs(cx, [args[0], args[1]])?;
                put(
                    cx,
                    match vex {
                        Some(op) => mc::v2_rr(op, Width::X, xm(a), xm(b)),
                        None => mc::ext_rr(Xt::Ptest, xm(a), xm(b)),
                    },
                );
            }
            flag(cx, inst, cond)
        }
        Op::Blend128(lane) => {
            let (ra, b, mask) = three(cx, args[0], args[1], args[2])?;
            let (a, b) = (xm(ra), xm(b));
            // `xmm14` the sign of each element of the mask, all its bits.
            match lane {
                Lane::B => {
                    put(cx, mc::pxor_rr(XMM14, XMM14));
                    put(cx, mc::pcmpgtb_rr(XMM14, mask));
                }
                Lane::D | Lane::Q => {
                    put(cx, mc::movdqa_rr(XMM14, mask));
                    put(cx, mc::psrad_ri(XMM14, 31));
                    if lane == Lane::Q {
                        put(cx, mc::pshufd_rri(XMM14, XMM14, 0b11_11_01_01));
                    }
                }
            }
            // `b` where the sign is set, `a` elsewhere.
            put(cx, mc::movdqa_rr(XMM15, XMM14));
            put(cx, mc::pand_rr(XMM15, b));
            put(cx, mc::pandn_rr(XMM14, a));
            put(cx, mc::por_rr(XMM14, XMM15));
            put(cx, mc::movdqa_rr(a, XMM14));
            cx.finish(inst, ra)
        }
        Op::Str(ext, out) => string_compare(cx, inst, args, ext, out, imm(cx)?),
        Op::Sha256rnds2 => {
            let (ra, rb, k) = three(cx, args[0], args[1], args[2])?;
            // `xmm0` kept in a slot of its own around the instruction
            // (after the operands are in registers: one may be in it).
            let saved = cx.alloc_slot(16, 16)?;
            store_fpr(cx, XMM0, saved, 16);
            put(cx, mc::movdqa_rr(XMM14, xm(ra)));
            put(cx, mc::movdqa_rr(XMM0, k));
            if xm(rb) == XMM0 {
                // `b` was in `xmm0`: it is the saved value.
                frame_inst(cx, saved, |m| mc::ext_rm(Xt::Sha256rnds2, XMM14, m));
            } else {
                put(cx, mc::ext_rr(Xt::Sha256rnds2, XMM14, xm(rb)));
            }
            load_fpr(cx, XMM0, saved, 16);
            // In `a`'s register: no third one is pinned.
            put(cx, mc::movdqa_rr(xm(ra), XMM14));
            cx.finish(inst, ra)
        }
        Op::V3(op, swap) => {
            let (first, second) = if swap { (args[1], args[0]) } else { (args[0], args[1]) };
            if ret == K::X {
                let [a, b] = xregs(cx, [first, second])?;
                put(cx, mc::v3_rrr(op, Width::X, xm(a), xm(a), xm(b)));
                return cx.finish(inst, a);
            }
            let a = cx.use_mem(first)?;
            // A shift's count is a 128-bit vector, in a register.
            let b = if cx.class_of(second) == Class::Fpr {
                Err(cx.use_reg(second)?)
            } else {
                Ok(cx.use_mem(second)?)
            };
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM14, a);
            match b {
                Ok(b) => frame_inst(cx, b, |m| mc::v3_rrm(op, Width::Y, XMM14, XMM14, m)),
                Err(b) => put(cx, mc::v3_rrr(op, Width::Y, XMM14, XMM14, xm(b))),
            }
            store_y(cx, inst, r)
        }
        Op::V3Imm(op) => {
            let imm = imm(cx)?;
            let (a, b) = (cx.use_mem(args[0])?, cx.use_mem(args[1])?);
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM14, a);
            frame_inst(cx, b, |m| mc::v3_rrmi(op, Width::Y, XMM14, XMM14, m, imm));
            store_y(cx, inst, r)
        }
        Op::V2(op) => {
            let a = cx.use_mem(args[0])?;
            let r = def_y(cx, inst, ret)?;
            frame_inst(cx, a, |m| mc::v2_rm(op, Width::Y, XMM14, m));
            store_y(cx, inst, r)
        }
        Op::V2Imm(op) => {
            let imm = imm(cx)?;
            let a = cx.use_mem(args[0])?;
            let r = def_y(cx, inst, ret)?;
            frame_inst(cx, a, |m| mc::v2_rmi(op, Width::Y, XMM14, m, imm));
            store_y(cx, inst, r)
        }
        Op::Movmsk(kind) => {
            let a = cx.use_mem(args[0])?;
            load_y(cx, XMM14, a);
            let rd = cx.def_reg(inst)?;
            put(
                cx,
                match kind {
                    Msk::B => mc::vpmovmskb_rr(Width::Y, gp(rd), XMM14),
                    Msk::Ps => mc::vmovmskp_rr(false, Width::Y, gp(rd), XMM14),
                    Msk::Pd => mc::vmovmskp_rr(true, Width::Y, gp(rd), XMM14),
                },
            );
            cx.finish(inst, rd)
        }
        Op::VBlend(op) => {
            if ret == K::X {
                return cx.bug("a 128-bit VEX blend row");
            }
            let (a, b, mask) = (cx.use_mem(args[0])?, cx.use_mem(args[1])?, cx.use_mem(args[2])?);
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM14, a);
            load_y(cx, XMM15, mask);
            frame_inst(cx, b, |m| mc::vblend_rrmr(op, Width::Y, XMM14, XMM14, m, XMM15));
            store_y(cx, inst, r)
        }
        Op::ShiftI(by_imm, by_reg) => {
            let a = cx.use_mem(args[0])?;
            let count = const_int(cx, args[1]);
            let n = match count {
                Some(_) => None,
                None => Some(cx.use_reg(args[1])?),
            };
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM14, a);
            match (count, n) {
                (Some(count), _) => {
                    // Every count at or above the lane width gives the
                    // same lanes; `imm8` holds up to 255.
                    let count = u8::try_from(count & 0xffff_ffff).unwrap_or(u8::MAX);
                    put(cx, mc::vshift_rri(by_imm, Width::Y, XMM14, XMM14, count));
                }
                (None, Some(n)) => {
                    // `movd` zero-extends the count into the 64 bits the
                    // shift reads, as `llc`'s.
                    put(cx, mc::movd_xr(Size::D, XMM15, gp(n)));
                    put(cx, mc::v3_rrr(by_reg, Width::Y, XMM14, XMM14, XMM15));
                }
                (None, None) => return cx.bug("a shift count neither constant nor a register"),
            }
            store_y(cx, inst, r)
        }
        Op::Fma(op) => {
            if ret == K::X {
                let (a, b, c) = three(cx, args[0], args[1], args[2])?;
                // `213`: `a = b·a ± c`.
                put(cx, mc::fma_rrr(op, Width::X, xm(a), xm(b), c));
                return cx.finish(inst, a);
            }
            let (a, b, c) = (cx.use_mem(args[0])?, cx.use_mem(args[1])?, cx.use_mem(args[2])?);
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM14, a);
            load_y(cx, XMM15, b);
            frame_inst(cx, c, |m| mc::fma_rrm(op, Width::Y, XMM14, XMM15, m));
            store_y(cx, inst, r)
        }
        Op::MaskLoad(op) => {
            let p = cx.use_reg(args[0])?;
            let at = Mem::base(gp(p), 0);
            if ret == K::X {
                let mask = cx.use_reg(args[1])?;
                put(cx, mc::vmask_load(op, Width::X, XMM14, xm(mask), at));
                put(cx, mc::movdqa_rr(xm(mask), XMM14));
                return cx.finish(inst, mask);
            }
            let mask = cx.use_mem(args[1])?;
            let r = def_y(cx, inst, ret)?;
            load_y(cx, XMM15, mask);
            put(cx, mc::vmask_load(op, Width::Y, XMM14, XMM15, at));
            store_y(cx, inst, r)
        }
        Op::MaskStore(op) => {
            let p = cx.use_reg(args[0])?;
            let at = Mem::base(gp(p), 0);
            if params[1] == K::X {
                let [mask, data] = xregs(cx, [args[1], args[2]])?;
                put(cx, mc::vmask_store(op, Width::X, at, xm(mask), xm(data)));
                return Ok(());
            }
            let (mask, data) = (cx.use_mem(args[1])?, cx.use_mem(args[2])?);
            load_y(cx, XMM15, mask);
            load_y(cx, XMM14, data);
            put(cx, mc::vmask_store(op, Width::Y, at, XMM15, XMM14));
            Ok(())
        }
        Op::LoadU => {
            let p = cx.use_reg(args[0])?;
            let r = def_y(cx, inst, ret)?;
            put(cx, mc::v2_rm(V2::Movdqu, Width::Y, XMM14, Mem::base(gp(p), 0)));
            store_y(cx, inst, r)
        }
        Op::Cvtps2ph => {
            let imm = imm(cx)?;
            if params[0] == K::X {
                let a = cx.use_reg(args[0])?;
                put(cx, mc::vcvtps2ph_rri(Width::X, xm(a), xm(a), imm));
                return cx.finish(inst, a);
            }
            let a = cx.use_mem(args[0])?;
            load_y(cx, XMM14, a);
            put(cx, mc::vcvtps2ph_rri(Width::Y, XMM14, XMM14, imm));
            let rd = cx.def_reg(inst)?;
            put(cx, mc::movdqa_rr(xm(rd), XMM14));
            cx.finish(inst, rd)
        }
        Op::Zeroupper => {
            put(cx, mc::vzeroupper());
            Ok(())
        }
        Op::Gather(op) => gather(cx, inst, args, op, (ret, params[2]), imm(cx)?),
    }
}

/// An AVX2 gather (§5.6.33 (6)): `args` the pass-through, the pointer,
/// the index, the mask; `shape` the widths of the result (and mask) and
/// of the index. The destination is `xmm14`/`ymm14`, the pass-through
/// copied in. A 128-bit mask or index is used in its own register (the
/// gather clears the mask; a register operand may be overwritten), a
/// 256-bit one is loaded from its slot into `ymm15`, and when both are 256
/// bits the index goes to a temporary of the pool (no register value is
/// pinned then). The three are distinct registers, as the instruction
/// requires. A 128-bit result is finished in an operand's register, so at
/// most two vector registers are pinned at once.
fn gather<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    args: &'a [Value],
    op: mc::Gather,
    (ret, index): (K, K),
    scale: u8,
) -> CgResult<()> {
    let scale = match scale {
        1 => mc::Scale::S1,
        2 => mc::Scale::S2,
        4 => mc::Scale::S4,
        8 => mc::Scale::S8,
        // LLVM's verifier lets any `i8` through; `llc` stops at it.
        _ => return unsupported(super::SIGNATURE),
    };
    let (src, ptr, idx, mask) = (args[0], args[1], args[2], args[3]);
    let width = if ret == K::Y || index == K::Y { Width::Y } else { Width::X };
    // The pass-through into `xmm14`/`ymm14` first: its register, when it
    // has one of its own, is given back at once.
    if ret == K::Y {
        let s = cx.use_mem(src)?;
        load_y(cx, XMM14, s);
    } else if src != idx && src != mask {
        let s = cx.use_reg(src)?;
        put(cx, mc::movdqa_rr(XMM14, xm(s)));
        cx.release(src);
        cx.free_temp(s);
    }
    // The index and the mask: registers, distinct.
    let (ri, rm) = match (index, ret) {
        (K::X, K::X) => {
            let [ri, rm] = xregs(cx, [idx, mask])?;
            (Some(ri), Some(rm))
        }
        (K::X, _) => (Some(cx.use_reg(idx)?), None),
        (_, K::X) => (None, Some(cx.use_reg(mask)?)),
        _ => (None, None),
    };
    if ret == K::X && (src == idx || src == mask) {
        let s = if src == idx { ri } else { rm };
        let Some(s) = s else { return cx.bug("a gather's pass-through register") };
        put(cx, mc::movdqa_rr(XMM14, xm(s)));
    }
    let mut temp = None;
    let (index_reg, mask_reg) = match (ri, rm) {
        (Some(ri), Some(rm)) if ri == rm => {
            // The same value: the mask a copy in `xmm15`.
            put(cx, mc::movdqa_rr(XMM15, xm(rm)));
            (xm(ri), XMM15)
        }
        (Some(ri), Some(rm)) => (xm(ri), xm(rm)),
        (Some(ri), None) => {
            let m = cx.use_mem(mask)?;
            load_y(cx, XMM15, m);
            (xm(ri), XMM15)
        }
        (None, Some(rm)) => {
            let i = cx.use_mem(idx)?;
            load_y(cx, XMM15, i);
            (XMM15, xm(rm))
        }
        (None, None) => {
            let (i, m) = (cx.use_mem(idx)?, cx.use_mem(mask)?);
            let t = cx.temp(Class::Fpr)?;
            temp = Some(t);
            load_y(cx, xm(t), i);
            load_y(cx, XMM15, m);
            (xm(t), XMM15)
        }
    };
    let base = cx.use_reg(ptr)?;
    let at = mc::Vsib { base: gp(base), index: index_reg, scale, disp: 0 };
    put(cx, mc::gather(op, width, XMM14, at, mask_reg));
    if let Some(t) = temp {
        cx.free_temp(t);
    }
    if ret == K::Y {
        let r = def_y(cx, inst, ret)?;
        return store_y(cx, inst, r);
    }
    // In the index's or the mask's register: no third one is pinned.
    let Some(out) = ri.or(rm) else { return cx.bug("a 128-bit gather without a register") };
    put(cx, mc::movdqa_rr(xm(out), XMM14));
    cx.finish(inst, out)
}

/// The registers of the 128-bit `args` (one register for a value given
/// more than once: a register value may be asked for once).
fn xregs<const N: usize>(cx: &mut FnCx<'_, X64>, args: [Value; N]) -> CgResult<[PReg; N]> {
    let mut out: [Option<PReg>; N] = [None; N];
    for i in 0..N {
        out[i] = Some(match (0..i).find(|&j| args[j] == args[i]) {
            Some(j) => out[j].unwrap_or_else(|| unreachable!()),
            None => cx.use_reg(args[i])?,
        });
    }
    Ok(out.map(|r| r.unwrap_or_else(|| unreachable!())))
}

/// The registers of 128-bit operands `a` and `b`, and where `c` is:
/// copied into `xmm15` (its register given back at once, so the lowering
/// never pins three registers: the pool may have two), or the register of
/// `a` or `b` when it is the same value.
fn three(cx: &mut FnCx<'_, X64>, a: Value, b: Value, c: Value) -> CgResult<(PReg, PReg, Xmm)> {
    if c == a || c == b {
        let [ra, rb] = xregs(cx, [a, b])?;
        return Ok((ra, rb, xm(if c == a { ra } else { rb })));
    }
    let rc = cx.use_reg(c)?;
    put(cx, mc::movdqa_rr(XMM15, xm(rc)));
    cx.release(c);
    cx.free_temp(rc);
    let [ra, rb] = xregs(cx, [a, b])?;
    Ok((ra, rb, XMM15))
}

/// Loads the 256 bits at `slot` into `y`.
fn load_y(cx: &mut FnCx<'_, X64>, y: Xmm, slot: FrameRef) {
    frame_inst(cx, slot, |m| mc::v2_rm(V2::Movdqu, Width::Y, y, m));
}

/// Where the result goes: its slot for a 256-bit one, nothing yet for a
/// 128-bit one (asked for after the instruction).
fn def_y<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ret: K) -> CgResult<Option<FrameRef>> {
    if ret == K::Y { cx.def_mem(inst).map(Some) } else { Ok(None) }
}

/// The result in `ymm14` (or its low half in `xmm14`) to its place.
fn store_y<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, r: Option<FrameRef>) -> CgResult<()> {
    if let Some(r) = r {
        frame_inst(cx, r, |m| mc::vmovdqu_mr(Width::Y, m, XMM14));
        return Ok(());
    }
    let rd = cx.def_reg(inst)?;
    put(cx, mc::movap_rr(FpSize::S, xm(rd), XMM14));
    cx.finish(inst, rd)
}

/// The `i32` result: 1 where `cond` holds after a flag-setting test.
fn flag<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, cond: Cond) -> CgResult<()> {
    put(cx, mc::setcc_r(cond, RAX));
    let rd = cx.def_reg(inst)?;
    put(cx, mc::movzx_rr(Size::D, gp(rd), Size::B, RAX));
    cx.finish(inst, rd)
}

/// `pcmpistr*` (`a`, `b`, imm) and `pcmpestr*` (`a`, `la`, `b`, `lb`,
/// imm): the lengths in `eax` and `edx`, the index in `ecx`, the mask in
/// `xmm0` (kept in `xmm15` around the instruction).
fn string_compare<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    args: &'a [Value],
    ext: E,
    out: Out,
    imm: u8,
) -> CgResult<()> {
    let explicit = args.len() == 5;
    let (a, b) = if explicit {
        let [a, b] = xregs(cx, [args[0], args[2]])?;
        // Each length copied and its register given back at once.
        for (at, to) in [(1, RAX), (3, RDX)] {
            if at == 3 && args[3] == args[1] {
                put(cx, mc::mov_rr(Size::D, RDX, RAX));
                continue;
            }
            let len = cx.use_reg(args[at])?;
            put(cx, mc::mov_rr(Size::D, to, gp(len)));
            cx.release(args[at]);
            cx.free_temp(len);
        }
        (a, b)
    } else {
        let [a, b] = xregs(cx, [args[0], args[1]])?;
        (a, b)
    };
    match out {
        Out::Mask => {
            put(cx, mc::movdqa_rr(XMM15, XMM0));
            put(cx, mc::ext_rri(ext, xm(a), xm(b), imm));
            put(cx, mc::movdqa_rr(XMM14, XMM0));
            put(cx, mc::movdqa_rr(XMM0, XMM15));
            // In `a`'s register: no third one is pinned.
            put(cx, mc::movdqa_rr(xm(a), XMM14));
            cx.finish(inst, a)
        }
        Out::Index => {
            put(cx, mc::ext_rri(ext, xm(a), xm(b), imm));
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::D, gp(rd), RCX));
            cx.finish(inst, rd)
        }
        Out::Flag(cond) => {
            put(cx, mc::ext_rri(ext, xm(a), xm(b), imm));
            flag(cx, inst, cond)
        }
    }
}
