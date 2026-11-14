//! The x86 vector intrinsics past SSSE3 (WP-202, `spec/codegen.md`
//! §5.6.33): every `llvm.x86.*` name of the SSE4.1, SSE4.2, AES, PCLMUL,
//! SHA, AVX, AVX2, FMA, F16C and GFNI headers that hod builds, called by
//! generated functions on edge and random lanes, with every immediate
//! of a list per intrinsic, compiled by hod (plain, and under every
//! stress option) and by `llc -O0 -fast-isel=0` of LLVM 22 and 19,
//! linked with one C driver and run on a CPU with the extensions (QEMU's
//! `max` model, or the host): every leg prints the same bytes.
//!
//! The driver asks the CPU (`__builtin_cpu_supports`) for each
//! function's extensions and prints a skip line instead of calling a
//! function the CPU lacks one of: the same line on every leg. No input
//! is random at run time; the oracle is the hardware's (the emulator's)
//! result of `llc`'s instruction.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "a test generator: `.ll` and C text built by hand"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;

use float_gen::{Rng, ll_header, llc, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::Scratch;
use linking::Toolchain;

/// An intrinsic: name, result type, parameter types, the CPU features
/// it needs (`__builtin_cpu_supports` names, space-separated).
type Row = (&'static str, &'static str, &'static [&'static str], &'static str);

const F4: &str = "<4 x float>";
const D2: &str = "<2 x double>";
const F8: &str = "<8 x float>";
const D4: &str = "<4 x double>";
const B16: &str = "<16 x i8>";
const W8: &str = "<8 x i16>";
const I4: &str = "<4 x i32>";
const Q2: &str = "<2 x i64>";
const B32: &str = "<32 x i8>";
const W16: &str = "<16 x i16>";
const I8: &str = "<8 x i32>";
const Q4: &str = "<4 x i64>";

/// Every intrinsic of this package, with LLVM's signature.
const ROWS: &[Row] = &[
    ("llvm.x86.sse41.round.ps", F4, &[F4, "i32"], "sse4.1"),
    ("llvm.x86.sse41.round.pd", D2, &[D2, "i32"], "sse4.1"),
    ("llvm.x86.sse41.round.ss", F4, &[F4, F4, "i32"], "sse4.1"),
    ("llvm.x86.sse41.round.sd", D2, &[D2, D2, "i32"], "sse4.1"),
    ("llvm.x86.sse41.blendvps", F4, &[F4, F4, F4], "sse4.1"),
    ("llvm.x86.sse41.blendvpd", D2, &[D2, D2, D2], "sse4.1"),
    ("llvm.x86.sse41.pblendvb", B16, &[B16, B16, B16], "sse4.1"),
    ("llvm.x86.sse41.dpps", F4, &[F4, F4, "i8"], "sse4.1"),
    ("llvm.x86.sse41.dppd", D2, &[D2, D2, "i8"], "sse4.1"),
    ("llvm.x86.sse41.insertps", F4, &[F4, F4, "i8"], "sse4.1"),
    ("llvm.x86.sse41.mpsadbw", W8, &[B16, B16, "i8"], "sse4.1"),
    ("llvm.x86.sse41.packusdw", W8, &[I4, I4], "sse4.1"),
    ("llvm.x86.sse41.phminposuw", W8, &[W8], "sse4.1"),
    ("llvm.x86.sse41.ptestz", "i32", &[Q2, Q2], "sse4.1"),
    ("llvm.x86.sse41.ptestc", "i32", &[Q2, Q2], "sse4.1"),
    ("llvm.x86.sse41.ptestnzc", "i32", &[Q2, Q2], "sse4.1"),
    ("llvm.x86.sse42.pcmpistrm128", B16, &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistri128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistria128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistric128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistrio128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistris128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpistriz128", "i32", &[B16, B16, "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestrm128", B16, &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestri128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestria128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestric128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestrio128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestris128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.sse42.pcmpestriz128", "i32", &[B16, "i32", B16, "i32", "i8"], "sse4.2"),
    ("llvm.x86.aesni.aesenc", Q2, &[Q2, Q2], "aes"),
    ("llvm.x86.aesni.aesenclast", Q2, &[Q2, Q2], "aes"),
    ("llvm.x86.aesni.aesdec", Q2, &[Q2, Q2], "aes"),
    ("llvm.x86.aesni.aesdeclast", Q2, &[Q2, Q2], "aes"),
    ("llvm.x86.aesni.aesimc", Q2, &[Q2], "aes"),
    ("llvm.x86.aesni.aeskeygenassist", Q2, &[Q2, "i8"], "aes"),
    ("llvm.x86.pclmulqdq", Q2, &[Q2, Q2, "i8"], "pclmul"),
    ("llvm.x86.sha1rnds4", I4, &[I4, I4, "i8"], "sha"),
    ("llvm.x86.sha1nexte", I4, &[I4, I4], "sha"),
    ("llvm.x86.sha1msg1", I4, &[I4, I4], "sha"),
    ("llvm.x86.sha1msg2", I4, &[I4, I4], "sha"),
    ("llvm.x86.sha256msg1", I4, &[I4, I4], "sha"),
    ("llvm.x86.sha256msg2", I4, &[I4, I4], "sha"),
    ("llvm.x86.sha256rnds2", I4, &[I4, I4, I4], "sha"),
    ("llvm.x86.avx.addsub.pd.256", D4, &[D4, D4], "avx"),
    ("llvm.x86.avx.addsub.ps.256", F8, &[F8, F8], "avx"),
    ("llvm.x86.avx.max.pd.256", D4, &[D4, D4], "avx"),
    ("llvm.x86.avx.max.ps.256", F8, &[F8, F8], "avx"),
    ("llvm.x86.avx.min.pd.256", D4, &[D4, D4], "avx"),
    ("llvm.x86.avx.min.ps.256", F8, &[F8, F8], "avx"),
    ("llvm.x86.avx.hadd.pd.256", D4, &[D4, D4], "avx"),
    ("llvm.x86.avx.hadd.ps.256", F8, &[F8, F8], "avx"),
    ("llvm.x86.avx.hsub.pd.256", D4, &[D4, D4], "avx"),
    ("llvm.x86.avx.hsub.ps.256", F8, &[F8, F8], "avx"),
    ("llvm.x86.avx.blendv.pd.256", D4, &[D4, D4, D4], "avx"),
    ("llvm.x86.avx.blendv.ps.256", F8, &[F8, F8, F8], "avx"),
    ("llvm.x86.avx.dp.ps.256", F8, &[F8, F8, "i8"], "avx"),
    ("llvm.x86.avx.cmp.pd.256", D4, &[D4, D4, "i8"], "avx"),
    ("llvm.x86.avx.cmp.ps.256", F8, &[F8, F8, "i8"], "avx"),
    ("llvm.x86.avx.cvt.pd2.ps.256", F4, &[D4], "avx"),
    ("llvm.x86.avx.cvt.ps2dq.256", I8, &[F8], "avx"),
    ("llvm.x86.avx.cvt.pd2dq.256", I4, &[D4], "avx"),
    ("llvm.x86.avx.cvtt.pd2dq.256", I4, &[D4], "avx"),
    ("llvm.x86.avx.cvtt.ps2dq.256", I8, &[F8], "avx"),
    ("llvm.x86.avx.rcp.ps.256", F8, &[F8], "avx"),
    ("llvm.x86.avx.rsqrt.ps.256", F8, &[F8], "avx"),
    ("llvm.x86.avx.round.pd.256", D4, &[D4, "i32"], "avx"),
    ("llvm.x86.avx.round.ps.256", F8, &[F8, "i32"], "avx"),
    ("llvm.x86.avx.vpermilvar.ps", F4, &[F4, I4], "avx"),
    ("llvm.x86.avx.vpermilvar.pd", D2, &[D2, Q2], "avx"),
    ("llvm.x86.avx.vpermilvar.ps.256", F8, &[F8, I8], "avx"),
    ("llvm.x86.avx.vpermilvar.pd.256", D4, &[D4, Q4], "avx"),
    ("llvm.x86.avx.vtestz.ps", "i32", &[F4, F4], "avx"),
    ("llvm.x86.avx.vtestz.pd", "i32", &[D2, D2], "avx"),
    ("llvm.x86.avx.vtestz.ps.256", "i32", &[F8, F8], "avx"),
    ("llvm.x86.avx.vtestz.pd.256", "i32", &[D4, D4], "avx"),
    ("llvm.x86.avx.vtestc.ps", "i32", &[F4, F4], "avx"),
    ("llvm.x86.avx.vtestc.pd", "i32", &[D2, D2], "avx"),
    ("llvm.x86.avx.vtestc.ps.256", "i32", &[F8, F8], "avx"),
    ("llvm.x86.avx.vtestc.pd.256", "i32", &[D4, D4], "avx"),
    ("llvm.x86.avx.vtestnzc.ps", "i32", &[F4, F4], "avx"),
    ("llvm.x86.avx.vtestnzc.pd", "i32", &[D2, D2], "avx"),
    ("llvm.x86.avx.vtestnzc.ps.256", "i32", &[F8, F8], "avx"),
    ("llvm.x86.avx.vtestnzc.pd.256", "i32", &[D4, D4], "avx"),
    ("llvm.x86.avx.ptestz.256", "i32", &[Q4, Q4], "avx"),
    ("llvm.x86.avx.ptestc.256", "i32", &[Q4, Q4], "avx"),
    ("llvm.x86.avx.ptestnzc.256", "i32", &[Q4, Q4], "avx"),
    ("llvm.x86.avx.movmsk.pd.256", "i32", &[D4], "avx"),
    ("llvm.x86.avx.movmsk.ps.256", "i32", &[F8], "avx"),
    ("llvm.x86.avx.ldu.dq.256", B32, &["ptr"], "avx"),
    ("llvm.x86.avx.maskload.ps", F4, &["ptr", I4], "avx"),
    ("llvm.x86.avx.maskstore.ps", "void", &["ptr", I4, F4], "avx"),
    ("llvm.x86.avx.maskload.pd", D2, &["ptr", Q2], "avx"),
    ("llvm.x86.avx.maskstore.pd", "void", &["ptr", Q2, D2], "avx"),
    ("llvm.x86.avx.maskload.ps.256", F8, &["ptr", I8], "avx"),
    ("llvm.x86.avx.maskstore.ps.256", "void", &["ptr", I8, F8], "avx"),
    ("llvm.x86.avx.maskload.pd.256", D4, &["ptr", Q4], "avx"),
    ("llvm.x86.avx.maskstore.pd.256", "void", &["ptr", Q4, D4], "avx"),
    ("llvm.x86.avx.vzeroall", "void", &[], "avx"),
    ("llvm.x86.avx.vzeroupper", "void", &[], "avx"),
    ("llvm.x86.avx2.packssdw", W16, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.packsswb", B32, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.packusdw", W16, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.packuswb", B32, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pavg.b", B32, &[B32, B32], "avx2"),
    ("llvm.x86.avx2.pavg.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pblendvb", B32, &[B32, B32, B32], "avx2"),
    ("llvm.x86.avx2.phadd.d", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.phadd.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.phadd.sw", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.phsub.d", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.phsub.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.phsub.sw", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pmadd.ub.sw", W16, &[B32, B32], "avx2"),
    ("llvm.x86.avx2.pmadd.wd", I8, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pmovmskb", "i32", &[B32], "avx2"),
    ("llvm.x86.avx2.pmul.hr.sw", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pmulh.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.pmulhu.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.psad.bw", Q4, &[B32, B32], "avx2"),
    ("llvm.x86.avx2.pshuf.b", B32, &[B32, B32], "avx2"),
    ("llvm.x86.avx2.psign.b", B32, &[B32, B32], "avx2"),
    ("llvm.x86.avx2.psign.w", W16, &[W16, W16], "avx2"),
    ("llvm.x86.avx2.psign.d", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.psll.w", W16, &[W16, W8], "avx2"),
    ("llvm.x86.avx2.psll.d", I8, &[I8, I4], "avx2"),
    ("llvm.x86.avx2.psll.q", Q4, &[Q4, Q2], "avx2"),
    ("llvm.x86.avx2.pslli.w", W16, &[W16, "i32"], "avx2"),
    ("llvm.x86.avx2.pslli.d", I8, &[I8, "i32"], "avx2"),
    ("llvm.x86.avx2.pslli.q", Q4, &[Q4, "i32"], "avx2"),
    ("llvm.x86.avx2.psrl.w", W16, &[W16, W8], "avx2"),
    ("llvm.x86.avx2.psrl.d", I8, &[I8, I4], "avx2"),
    ("llvm.x86.avx2.psrl.q", Q4, &[Q4, Q2], "avx2"),
    ("llvm.x86.avx2.psrli.w", W16, &[W16, "i32"], "avx2"),
    ("llvm.x86.avx2.psrli.d", I8, &[I8, "i32"], "avx2"),
    ("llvm.x86.avx2.psrli.q", Q4, &[Q4, "i32"], "avx2"),
    ("llvm.x86.avx2.psra.w", W16, &[W16, W8], "avx2"),
    ("llvm.x86.avx2.psra.d", I8, &[I8, I4], "avx2"),
    ("llvm.x86.avx2.psrai.w", W16, &[W16, "i32"], "avx2"),
    ("llvm.x86.avx2.psrai.d", I8, &[I8, "i32"], "avx2"),
    ("llvm.x86.avx2.psllv.d", I4, &[I4, I4], "avx2"),
    ("llvm.x86.avx2.psllv.d.256", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.psllv.q", Q2, &[Q2, Q2], "avx2"),
    ("llvm.x86.avx2.psllv.q.256", Q4, &[Q4, Q4], "avx2"),
    ("llvm.x86.avx2.psrlv.d", I4, &[I4, I4], "avx2"),
    ("llvm.x86.avx2.psrlv.d.256", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.psrlv.q", Q2, &[Q2, Q2], "avx2"),
    ("llvm.x86.avx2.psrlv.q.256", Q4, &[Q4, Q4], "avx2"),
    ("llvm.x86.avx2.psrav.d", I4, &[I4, I4], "avx2"),
    ("llvm.x86.avx2.psrav.d.256", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.permd", I8, &[I8, I8], "avx2"),
    ("llvm.x86.avx2.permps", F8, &[F8, I8], "avx2"),
    ("llvm.x86.avx2.mpsadbw", W16, &[B32, B32, "i8"], "avx2"),
    ("llvm.x86.avx2.maskload.d", I4, &["ptr", I4], "avx2"),
    ("llvm.x86.avx2.maskstore.d", "void", &["ptr", I4, I4], "avx2"),
    ("llvm.x86.avx2.maskload.q", Q2, &["ptr", Q2], "avx2"),
    ("llvm.x86.avx2.maskstore.q", "void", &["ptr", Q2, Q2], "avx2"),
    ("llvm.x86.avx2.maskload.d.256", I8, &["ptr", I8], "avx2"),
    ("llvm.x86.avx2.maskstore.d.256", "void", &["ptr", I8, I8], "avx2"),
    ("llvm.x86.avx2.maskload.q.256", Q4, &["ptr", Q4], "avx2"),
    ("llvm.x86.avx2.maskstore.q.256", "void", &["ptr", Q4, Q4], "avx2"),
    ("llvm.x86.fma.vfmaddsub.ps", F4, &[F4, F4, F4], "fma"),
    ("llvm.x86.fma.vfmaddsub.pd", D2, &[D2, D2, D2], "fma"),
    ("llvm.x86.fma.vfmaddsub.ps.256", F8, &[F8, F8, F8], "fma"),
    ("llvm.x86.fma.vfmaddsub.pd.256", D4, &[D4, D4, D4], "fma"),
    ("llvm.x86.vcvtps2ph.128", W8, &[F4, "i32"], "f16c"),
    ("llvm.x86.vcvtps2ph.256", W8, &[F8, "i32"], "f16c"),
    ("llvm.x86.vgf2p8affineqb.128", B16, &[B16, B16, "i8"], "gfni"),
    ("llvm.x86.vgf2p8affineqb.256", B32, &[B32, B32, "i8"], "gfni avx"),
    ("llvm.x86.vgf2p8affineinvqb.128", B16, &[B16, B16, "i8"], "gfni"),
    ("llvm.x86.vgf2p8affineinvqb.256", B32, &[B32, B32, "i8"], "gfni avx"),
    ("llvm.x86.vgf2p8mulb.128", B16, &[B16, B16], "gfni"),
    ("llvm.x86.vgf2p8mulb.256", B32, &[B32, B32], "gfni avx"),
];

/// WP-173's scalar compares, whose predicate has five bits with AVX.
const AVX_COMPARES: &[Row] = &[
    ("llvm.x86.sse.cmp.ss", F4, &[F4, F4, "i8"], "avx"),
    ("llvm.x86.sse2.cmp.sd", D2, &[D2, D2, "i8"], "avx"),
];

/// The AVX2 gathers (WP-207): pass-through, base, index, mask, scale.
/// Run by [`x86_gathers_agree_with_llc`], whose inputs keep every address
/// a lane reads inside a mapped table.
const GATHERS: &[Row] = &[
    ("llvm.x86.avx2.gather.d.d", I4, &[I4, "ptr", I4, I4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.d.256", I8, &[I8, "ptr", I8, I8, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.q", Q2, &[Q2, "ptr", I4, Q2, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.q.256", Q4, &[Q4, "ptr", I4, Q4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.d", I4, &[I4, "ptr", Q2, I4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.d.256", I4, &[I4, "ptr", Q4, I4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.q", Q2, &[Q2, "ptr", Q2, Q2, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.q.256", Q4, &[Q4, "ptr", Q4, Q4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.ps", F4, &[F4, "ptr", I4, F4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.ps.256", F8, &[F8, "ptr", I8, F8, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.pd", D2, &[D2, "ptr", I4, D2, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.d.pd.256", D4, &[D4, "ptr", I4, D4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.ps", F4, &[F4, "ptr", Q2, F4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.ps.256", F4, &[F4, "ptr", Q4, F4, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.pd", D2, &[D2, "ptr", Q2, D2, "i8"], "avx2"),
    ("llvm.x86.avx2.gather.q.pd.256", D4, &[D4, "ptr", Q4, D4, "i8"], "avx2"),
];

/// Every feature of [`ROWS`], on every generated function.
const FEATURES: &str = "+cx8,+fxsr,+mmx,+sse,+sse2,+x87,+sse3,+ssse3,+sse4.1,+sse4.2,+popcnt,\
                        +avx,+avx2,+fma,+f16c,+aes,+pclmul,+sha,+gfni";

/// The bytes of one case: four vector operands of 32 bytes, then the
/// `i32` count (128) and the two string lengths (132, 136).
const CASE: usize = 160;
/// Where the count is.
const AT_COUNT: usize = 128;
/// Shift counts: at and around the lane widths.
const COUNTS: [u32; 16] = [0, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256, u32::MAX];
/// Constant shift counts (each one function).
const CONST_COUNTS: [i64; 10] = [0, 1, 3, 15, 16, 31, 32, 63, 64, 300];
/// The explicit string lengths: negative ones, and past 8 and 16
/// elements (the instruction takes the absolute value, saturated).
const LENGTHS: [i32; 14] = [-20, -17, -16, -1, 0, 1, 3, 8, 15, 16, 17, 100, i32::MIN, i32::MAX];

/// The immediates `name` is called with (each one function).
fn immediates(name: &str) -> Vec<i64> {
    let n = name.trim_start_matches("llvm.x86.");
    let list: &[i64] = match n {
        _ if n.contains("round") => &[0, 1, 2, 3, 4, 8, 9, 10, 11, 12],
        "sse41.dpps" | "avx.dp.ps.256" => &[0xff, 0x31, 0xf1, 0x7c, 0x00, 0x8e],
        "sse41.dppd" => &[0x33, 0x31, 0x22, 0xff, 0x10],
        "sse41.insertps" => &[0x00, 0x1d, 0xc0, 0x4f, 0xff, 0x31],
        "sse41.mpsadbw" => &[0, 1, 2, 3, 4, 5, 6, 7],
        "avx2.mpsadbw" => &[0, 0x05, 0x2a, 0x3f, 0x38],
        "pclmulqdq" => &[0, 1, 0x10, 0x11],
        "aesni.aeskeygenassist" => &[0, 1, 0x1b, 0x80],
        "sha1rnds4" => &[0, 1, 2, 3],
        _ if n.contains("pcmp") => {
            &[0x00, 0x04, 0x08, 0x0c, 0x01, 0x18, 0x1a, 0x34, 0x40, 0x44, 0x4c, 0x3a, 0x70, 0x0d]
        }
        _ if n.contains("gf2p8affine") => &[0, 0x63, 0xff],
        "avx.cmp.ps.256" | "avx.cmp.pd.256" | "sse.cmp.ss" | "sse2.cmp.sd" => &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31,
        ],
        _ if n.starts_with("vcvtps2ph") => &[0, 1, 2, 3, 4],
        _ => &[0],
    };
    list.to_vec()
}

/// Whether the `i32` operand of `name` is an immediate.
fn i32_immediate(name: &str) -> bool {
    name.contains("round") || name.contains("vcvtps2ph")
}

/// One generated function: what it calls, its operand types (for the
/// inputs) and the features it needs.
struct Func {
    name: String,
    params: &'static [&'static str],
    needs: &'static str,
}

/// The vector operand of type `ty` in slot `slot`, as an argument.
fn vector_arg(ty: &str, slot: usize) -> String {
    format!("{ty} %v{slot}")
}

/// The generated module's functions (`void @tN(ptr %in, ptr %out)`) and
/// their declarations.
fn functions() -> (String, Vec<Func>) {
    let mut text = String::new();
    let mut decls = String::new();
    let mut funcs = Vec::new();
    for &(name, ret, params, needs) in ROWS.iter().chain(AVX_COMPARES) {
        writeln!(decls, "declare {ret} @{name}({})", params.join(", ")).unwrap();
        // The operand lists of the variants.
        let mut variants: Vec<Vec<String>> = Vec::new();
        let mut base: Vec<String> = Vec::new();
        let mut slot = 0;
        let mut lengths = 0;
        let mut imm_at = None;
        let mut count_at = None;
        for (i, &p) in params.iter().enumerate() {
            match p {
                "ptr" if i == 0 && ret == "void" => base.push("ptr %out".into()),
                // A masked load's memory: the fourth vector slot; an
                // unaligned load one byte in.
                "ptr" if name.contains("ldu") => base.push("ptr %in1".into()),
                "ptr" => base.push("ptr %s3".into()),
                "i8" => {
                    imm_at = Some(i);
                    base.push(String::new());
                }
                "i32" if i32_immediate(name) => {
                    imm_at = Some(i);
                    base.push(String::new());
                }
                "i32" if name.contains("pcmpestr") => {
                    base.push(format!("i32 %len{lengths}"));
                    lengths += 1;
                }
                "i32" => {
                    count_at = Some(i);
                    base.push("i32 %count".into());
                }
                _ => {
                    base.push(vector_arg(p, slot));
                    slot += 1;
                }
            }
        }
        if let Some(at) = imm_at {
            for k in immediates(name) {
                let mut v = base.clone();
                v[at] = format!("{} {k}", params[at]);
                variants.push(v);
            }
        } else {
            variants.push(base.clone());
        }
        if let Some(at) = count_at {
            for k in CONST_COUNTS {
                let mut v = base.clone();
                v[at] = format!("i32 {k}");
                variants.push(v);
            }
        }
        // The same value twice, and a constant second operand.
        let vectors: Vec<usize> =
            (0..params.len()).filter(|&i| params[i].starts_with('<')).collect();
        if let [x, y, ..] = vectors[..]
            && params[x] == params[y]
        {
            let mut v = variants[0].clone();
            v[y] = vector_arg(params[y], 0);
            variants.push(v);
            if imm_at.is_none() {
                let mut v = variants[0].clone();
                v[y] = format!("{} zeroinitializer", params[y]);
                variants.push(v);
            }
        }
        for args in variants {
            let k = funcs.len();
            let call = format!("call {ret} @{name}({})", args.join(", "));
            let (result, store) = if ret == "void" {
                (format!("  {call}\n"), String::new())
            } else {
                (format!("  %r = {call}\n"), format!("  store {ret} %r, ptr %out, align 1\n"))
            };
            let mut loads = String::new();
            for s in 0..4 {
                writeln!(loads, "  %s{s} = getelementptr i8, ptr %in, i64 {}", 32 * s).unwrap();
            }
            for (slot, &p) in params.iter().filter(|p| p.starts_with('<')).enumerate() {
                writeln!(loads, "  %v{slot} = load {p}, ptr %s{slot}, align 1").unwrap();
            }
            writeln!(
                loads,
                "  %in1 = getelementptr i8, ptr %in, i64 1\n  \
                 %pc = getelementptr i8, ptr %in, i64 {AT_COUNT}\n  \
                 %count = load i32, ptr %pc, align 4\n  \
                 %pl0 = getelementptr i8, ptr %in, i64 {}\n  %len0 = load i32, ptr %pl0, align 4\n  \
                 %pl1 = getelementptr i8, ptr %in, i64 {}\n  %len1 = load i32, ptr %pl1, align 4",
                AT_COUNT + 4,
                AT_COUNT + 8
            )
            .unwrap();
            writeln!(
                text,
                "define void @t{k}(ptr %in, ptr %out) #0 {{\n{loads}{result}{store}  ret void\n}}\n"
            )
            .unwrap();
            funcs.push(Func { name: format!("{name}({})", args.join(", ")), params, needs });
        }
    }
    text.push_str(&decls);
    writeln!(text, "attributes #0 = {{ \"target-features\"=\"{FEATURES}\" }}").unwrap();
    (text, funcs)
}

/// Edge values of a lane of `elem` (its bits).
fn edges(elem: &str) -> Vec<u64> {
    match elem {
        "float" => vec![
            0,
            0x8000_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x3fc0_0000,
            0xc020_0000,
            0x3f00_0000,
            0xbf00_0000,
            0x4020_0000,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0xffc0_0000,
            0x7f80_0001,
            0x7fc1_2345,
            1,
            0x807f_ffff,
            0x0080_0000,
            0x7f7f_ffff,
            0x4f00_0000,
            0xcf00_0000,
            0x477f_e000,
            0x4780_0000,
            0x3380_0000,
            0x387f_c000,
        ],
        "double" => vec![
            0,
            1 << 63,
            0x3ff0_0000_0000_0000,
            0xbff0_0000_0000_0000,
            0x3ff8_0000_0000_0000,
            0xc004_0000_0000_0000,
            0x3fe0_0000_0000_0000,
            0x4004_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0x7ff8_0000_1234_5678,
            1,
            0x800f_ffff_ffff_ffff,
            0x0010_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
            0x41e0_0000_0000_0000,
            0xc1e0_0000_0000_0000,
            0x41df_ffff_ffc0_0000,
            0x47ef_ffff_e000_0000,
        ],
        int => {
            let bits: u32 = int.trim_start_matches('i').parse().unwrap();
            let mask = if bits == 64 { u64::MAX } else { (1 << bits) - 1 };
            let min = 1u64 << (bits - 1);
            let mut v = vec![0, 1, 2, 3, mask, mask - 1, min, min - 1, min + 1, 0x55, 0x80, 0xff];
            v.extend([0x7f, 0x81, 0x7fff, 0x8000, 0x8001, 0xffff, 0x1_0000, 0x7fff_ffff]);
            v.extend([0x8000_0000, 0xffff_ff80, 0x4000, 0xc000, 15, 16, 0x8f, 0x61, 0x62]);
            v.into_iter().map(|x| x & mask).collect()
        }
    }
}

/// The element type and lane count of a vector type's text.
fn lanes_of(ty: &str) -> (&str, usize) {
    let inner = ty.trim_start_matches('<').trim_end_matches('>');
    let (n, elem) = inner.split_once(" x ").unwrap();
    (elem, n.parse().unwrap())
}

/// The bytes of a vector of `ty`: lanes from the edges (often), random,
/// or `fill`.
fn vector(rng: &mut Rng, ty: &str, fill: Option<u64>) -> Vec<u8> {
    let (elem, n) = lanes_of(ty);
    let bytes = match elem {
        "float" => 4,
        "double" => 8,
        int => int.trim_start_matches('i').parse::<usize>().unwrap() / 8,
    };
    let list = edges(elem);
    let mut out = Vec::new();
    for _ in 0..n {
        let bits = match fill {
            Some(f) => f,
            None if rng.below(3) < 2 => *rng.pick(&list),
            None => rng.next(),
        };
        out.extend_from_slice(&bits.to_le_bytes()[..bytes]);
    }
    out
}

/// `bytes` (a vector of `ty`) with each NaN lane made 1.5.
fn no_nan(bytes: &mut [u8], ty: &str) {
    let (elem, _) = lanes_of(ty);
    if elem == "double" {
        for lane in bytes.chunks_mut(8) {
            let v = u64::from_le_bytes(lane.try_into().unwrap());
            if f64::from_bits(v).is_nan() {
                lane.copy_from_slice(&1.5f64.to_bits().to_le_bytes());
            }
        }
    } else {
        for lane in bytes.chunks_mut(4) {
            let v = u32::from_le_bytes(lane.try_into().unwrap());
            if f32::from_bits(v).is_nan() {
                lane.copy_from_slice(&1.5f32.to_bits().to_le_bytes());
            }
        }
    }
}

/// The inputs of function `f`: [`CASE`] bytes each.
fn inputs(rng: &mut Rng, f: &Func, n: usize) -> Vec<[u8; CASE]> {
    let mut sets = Vec::new();
    for i in 0..n {
        let mut set = [0u8; CASE];
        for k in &mut set {
            *k = rng.next() as u8;
        }
        let fill = match i {
            0 => Some(0),
            1 => Some(u64::MAX),
            2 => Some(1 << 63),
            _ => None,
        };
        let vectors = f.params.iter().filter(|p| p.starts_with('<'));
        for (slot, ty) in vectors.enumerate() {
            let mut bytes = vector(rng, ty, fill);
            // Which of two NaN factors an FMA returns is the operand
            // order of the instruction, which `llc` commutes as it folds
            // a load: LLVM leaves it open. Only the first factor and the
            // addend are NaN.
            if f.name.starts_with("llvm.x86.fma.") && slot == 1 {
                no_nan(&mut bytes, ty);
            }
            set[32 * slot..32 * slot + bytes.len()].copy_from_slice(&bytes);
        }
        let count = COUNTS.get(i).copied().unwrap_or_else(|| rng.next() as u32 % 70);
        set[AT_COUNT..AT_COUNT + 4].copy_from_slice(&count.to_le_bytes());
        for k in 0..2 {
            let len = *rng.pick(&LENGTHS);
            set[AT_COUNT + 4 + 4 * k..AT_COUNT + 8 + 4 * k].copy_from_slice(&len.to_le_bytes());
        }
        sets.push(set);
    }
    sets
}

/// The distinct features of [`ROWS`], in order.
fn all_needs() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for &(.., needs) in ROWS {
        for need in needs.split(' ') {
            if !out.contains(&need) {
                out.push(need);
            }
        }
    }
    out
}

/// The C driver: runs every function on its inputs and prints the 32
/// result bytes of each call, or a skip line where the CPU lacks one of
/// the function's features.
fn driver(funcs: &[Func], sets: &[Vec<[u8; CASE]>]) -> String {
    let needs = all_needs();
    let mut c = String::from("#include <stdio.h>\n#include <string.h>\n");
    for k in 0..funcs.len() {
        writeln!(c, "void t{k}(const void *, void *);").unwrap();
    }
    c.push_str("typedef void (*fn)(const void *, void *);\nstatic const fn fns[] = {");
    for k in 0..funcs.len() {
        write!(c, "t{k},").unwrap();
    }
    c.push_str("};\n/* The features each function needs, a bit per feature. */\n");
    c.push_str("static const unsigned needs[] = {");
    for f in funcs {
        let bits: u32 =
            f.needs.split(' ').map(|n| 1 << needs.iter().position(|m| *m == n).unwrap()).sum();
        write!(c, "{bits},").unwrap();
    }
    c.push_str("};\nstatic const unsigned char in[][");
    writeln!(c, "{CASE}] __attribute__((aligned(32))) = {{").unwrap();
    for set in sets.iter().flatten() {
        c.push('{');
        for byte in set {
            write!(c, "{byte},").unwrap();
        }
        c.push_str("},\n");
    }
    writeln!(c, "}};\nstatic const int per = {};", sets[0].len()).unwrap();
    c.push_str("int main(void) {\n  unsigned have = 0;\n  __builtin_cpu_init();\n");
    for (bit, need) in needs.iter().enumerate() {
        writeln!(c, "  if (__builtin_cpu_supports(\"{need}\")) have |= 1u << {bit};").unwrap();
    }
    c.push_str(
        "  for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++)\n    \
         for (int i = 0; i < per; i++) {\n      \
         if ((needs[f] & have) != needs[f]) { printf(\"%u %d skip\\n\", f, i); continue; }\n      \
         unsigned char out[32] __attribute__((aligned(32)));\n      memset(out, 0, 32);\n      \
         fns[f](in[f * per + i], out);\n      printf(\"%u %d \", f, i);\n      \
         for (int k = 0; k < 32; k++) printf(\"%02x\", out[k]);\n      printf(\"\\n\");\n    }\n  \
         return 0;\n}\n",
    );
    c
}

/// §5.6.33: every intrinsic hod builds computes what `llc`'s instruction
/// computes, on LLVM 22 and 19, hod plain and stressed, under bfd and
/// lld, on a CPU with the extensions.
#[test]
fn x86_vector_extensions_agree_with_llc() {
    let (body, funcs) = functions();
    let text = format!("{}{body}", ll_header());
    let mut rng = Rng(0x0202);
    let sets: Vec<Vec<[u8; CASE]>> = funcs.iter().map(|f| inputs(&mut rng, f, 24)).collect();
    let cases: usize = sets.iter().map(Vec::len).sum();

    let (module, bodies) = parse(&text);
    let scratch = Scratch::new("x64-x86-ext");
    let mut legs = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("hod {mode}: {e}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc {major}"), object));
        }
    }
    eprintln!(
        "x86 extensions: {} intrinsics, {} functions, {cases} cases",
        ROWS.len(),
        funcs.len()
    );

    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let driver = toolchain.compile_c(&scratch, "driver", &driver(&funcs, &sets), &[]);
    let mut runs: Vec<(String, String)> = Vec::new();
    for (leg, object) in &legs {
        let linkers: Vec<_> = if leg == "hod plain" {
            toolchain.linkers.iter().filter(|l| matches!(l.name, "bfd" | "lld")).collect()
        } else {
            toolchain.linkers.iter().take(1).collect()
        };
        for linker in linkers {
            let program = scratch.join(format!("{}-{}", leg.replace(' ', "-"), linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            assert!(
                ran.success(),
                "{leg} {}: {:?}\n{}",
                linker.name,
                ran.status,
                ran.stderr_text()
            );
            runs.push((format!("{leg} ({})", linker.name), ran.stdout_text()));
        }
    }
    let Some((reference_leg, reference)) =
        runs.iter().find(|(leg, _)| leg.starts_with("llc 22")).or(runs.first())
    else {
        return;
    };
    assert_eq!(reference.lines().count(), cases, "{reference_leg}: one line per case");
    let skipped = reference.lines().filter(|l| l.ends_with(" skip")).count();
    let mut wrong = Vec::new();
    for (leg, out) in &runs {
        assert_eq!(out.lines().count(), cases, "{leg}: one line per case");
        for (want, got) in reference.lines().zip(out.lines()) {
            if want != got {
                let f: usize = want.split(' ').next().unwrap().parse().unwrap();
                wrong.push(format!("{leg}: {}: want {want}, got {got}", funcs[f].name));
            }
        }
    }
    eprintln!(
        "x86 extensions: legs {:?}; {skipped} of {cases} cases skipped (the CPU lacks a feature)",
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(40)].join("\n")
    );
}

/// Compiles `text` (functions after the module header) with hod.
fn compile(text: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// The `target-features` item that turns on what a row needs.
fn feature_items(needs: &str) -> String {
    needs.split(' ').map(|n| format!("+{n}")).collect::<Vec<_>>().join(",")
}

/// §5.6.33: an intrinsic in a function whose features do not enable it
/// is refused by its own name; enabled, or implied by a later feature,
/// it builds; turned off again by a later item, refused.
#[test]
fn only_with_the_feature() {
    for &(name, ret, params, needs) in ROWS.iter().chain(GATHERS) {
        let mut loads = String::new();
        let args: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(i, p)| match *p {
                "ptr" => "ptr %p".into(),
                "i8" => "i8 1".into(),
                "i32" if i32_immediate(name) => "i32 1".into(),
                _ => {
                    writeln!(loads, "  %v{i} = load {p}, ptr %p, align 1").unwrap();
                    format!("{p} %v{i}")
                }
            })
            .collect();
        let (call, store) = if ret == "void" {
            (String::new(), String::new())
        } else {
            ("%r = ".to_string(), format!("  store {ret} %r, ptr %p, align 1\n"))
        };
        let module = |features: Option<&str>| {
            let (mark, attrs) = match features {
                Some(f) => (" #0", format!("attributes #0 = {{ \"target-features\"=\"{f}\" }}\n")),
                None => ("", String::new()),
            };
            format!(
                "declare {ret} @{name}({})\ndefine void @f(ptr %p){mark} {{\n{loads}  \
                 {call}call {ret} @{name}({})\n{store}  ret void\n}}\n{attrs}",
                params.join(", "),
                args.join(", ")
            )
        };
        let refusal = format!("intrinsic:{name}");
        let refused = |text: String| match compile(&text) {
            Err(CgError::Unsupported { construct }) => assert_eq!(construct, refusal, "{text}"),
            other => panic!("{name}: {other:?}\n{text}"),
        };
        let on = feature_items(needs);
        refused(module(None));
        refused(module(Some("+sse,+sse2,+sse3,+ssse3")));
        compile(&module(Some(&format!("+sse,+sse2,{on}"))))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // Each feature turned off again after it.
        for need in needs.split(' ') {
            refused(module(Some(&format!("{on},-{need}"))));
        }
        // SSE2 turned off takes every extension with it.
        refused(module(Some(&format!("{on},-sse2"))));
        // What implies the feature.
        let implied = match needs {
            "sse4.1" | "sse4.2" => Some("+avx"),
            "avx" => Some("+avx2"),
            "avx2" | "fma" | "f16c" => Some("+avx512f"),
            "aes" => Some("+vaes"),
            "pclmul" => Some("+vpclmulqdq"),
            _ => None,
        };
        if let Some(implied) = implied {
            compile(&module(Some(implied))).unwrap_or_else(|e| panic!("{name} {implied}: {e}"));
        }
    }
    // The 256-bit GFNI forms need AVX besides GFNI.
    let gfni = "declare <32 x i8> @llvm.x86.vgf2p8mulb.256(<32 x i8>, <32 x i8>)\n\
                define void @f(ptr %p) #0 {\n  %v = load <32 x i8>, ptr %p, align 1\n  \
                %r = call <32 x i8> @llvm.x86.vgf2p8mulb.256(<32 x i8> %v, <32 x i8> %v)\n  \
                store <32 x i8> %r, ptr %p, align 1\n  ret void\n}\n\
                attributes #0 = { \"target-features\"=\"+gfni\" }\n";
    assert_eq!(compile(gfni), Err(CgError::unsupported("intrinsic:llvm.x86.vgf2p8mulb.256")));
}

/// Every intrinsic of this package is an x86-64 row and refused on
/// AArch64; a call whose types are not LLVM's is refused as a signature.
#[test]
fn rows_and_signatures() {
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for &(name, ..) in ROWS.iter().chain(GATHERS) {
        let row = format!("intrinsic:{name}");
        assert_eq!(x64.supports(&row), Some(true), "{row}");
        assert_eq!(a64.supports(&row), Some(false), "{row}");
    }
    let wrong = "declare i32 @llvm.x86.avx2.pmovmskb(<16 x i8>)\n\
                 define i32 @f(<16 x i8> %v) #0 {\n  \
                 %r = call i32 @llvm.x86.avx2.pmovmskb(<16 x i8> %v)\n  ret i32 %r\n}\n\
                 attributes #0 = { \"target-features\"=\"+avx2\" }\n";
    assert_eq!(compile(wrong), Err(CgError::unsupported(hod_x64::x86::SIGNATURE)));
    // A gather's scale other than 1, 2, 4 or 8 (`llc` stops at it).
    let scale = "declare <4 x i32> @llvm.x86.avx2.gather.d.d(<4 x i32>, ptr, <4 x i32>, <4 x i32>, i8)\n\
                 define <4 x i32> @f(<4 x i32> %v, ptr %p) #0 {\n  \
                 %r = call <4 x i32> @llvm.x86.avx2.gather.d.d(<4 x i32> %v, ptr %p, <4 x i32> %v, \
                 <4 x i32> %v, i8 3)\n  ret <4 x i32> %r\n}\n\
                 attributes #0 = { \"target-features\"=\"+avx2\" }\n";
    assert_eq!(compile(scale), Err(CgError::unsupported(hod_x64::x86::SIGNATURE)));
}

/// The bytes of one gather case: pass-through, index and mask (32 bytes
/// each), the base's offset into the table (`i64`, at 96) and whether the
/// base is null (at 104).
const GCASE: usize = 112;
/// The mapped table: `TABLE` bytes between two pages the driver makes
/// inaccessible, so that a lane read outside it faults.
const TABLE: i64 = 8192;
/// A guard page.
const PAGE: i64 = 4096;

/// How a gather function is called.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Each operand its own value.
    Plain,
    /// A null base: the `q` indices absolute addresses, the `d` forms
    /// with every lane masked off.
    NullBase,
    /// The mask is the pass-through value.
    MaskIsSource,
    /// The mask is the index value: a lane is read where its index is
    /// negative (the base at the table's end).
    MaskIsIndex,
}

/// One generated gather function.
struct GFunc {
    name: &'static str,
    ret: &'static str,
    index: &'static str,
    scale: i64,
    shape: Shape,
}

/// The module: `void @gN(ptr %in, ptr %base, ptr %out)` storing the
/// result, the mask and the index (so that a mask the instruction clears
/// is seen to keep its value).
fn gather_functions() -> (String, Vec<GFunc>) {
    let mut text = String::new();
    let mut funcs = Vec::new();
    for &(name, ret, params, _) in GATHERS {
        let index = params[2];
        writeln!(text, "declare {ret} @{name}({})", params.join(", ")).unwrap();
        let mut shapes = vec![Shape::Plain, Shape::NullBase, Shape::MaskIsSource];
        if index == ret {
            shapes.push(Shape::MaskIsIndex);
        }
        for shape in shapes {
            for scale in [1, 2, 4, 8] {
                let k = funcs.len();
                let (src, base, mask) = match shape {
                    Shape::Plain => ("%src", "%base", "%mask"),
                    Shape::NullBase => ("%src", "null", "%mask"),
                    Shape::MaskIsSource => ("%mask", "%base", "%mask"),
                    Shape::MaskIsIndex => ("%src", "%base", "%idx"),
                };
                let mask_ty = if shape == Shape::MaskIsIndex { index } else { ret };
                writeln!(
                    text,
                    "define void @g{k}(ptr %in, ptr %base, ptr %out) #0 {{\n  \
                     %pi = getelementptr i8, ptr %in, i64 32\n  \
                     %pm = getelementptr i8, ptr %in, i64 64\n  \
                     %src = load {ret}, ptr %in, align 1\n  \
                     %idx = load {index}, ptr %pi, align 1\n  \
                     %mask = load {ret}, ptr %pm, align 1\n  \
                     %r = call {ret} @{name}({ret} {src}, ptr {base}, {index} %idx, \
                     {mask_ty} {mask}, i8 {scale})\n  \
                     store {ret} %r, ptr %out, align 1\n  \
                     %om = getelementptr i8, ptr %out, i64 32\n  \
                     store {ret} %mask, ptr %om, align 1\n  \
                     %oi = getelementptr i8, ptr %out, i64 64\n  \
                     store {index} %idx, ptr %oi, align 1\n  \
                     ret void\n}}\n"
                )
                .unwrap();
                funcs.push(GFunc { name, ret, index, scale, shape });
            }
        }
    }
    writeln!(text, "attributes #0 = {{ \"target-features\"=\"{FEATURES}\" }}").unwrap();
    (text, funcs)
}

/// The inputs of gather function `f`. Each lane the gather reads (its
/// mask element's sign set) addresses an element inside the table; each
/// other lane addresses one in a guard page, or anywhere where the base
/// is null: read, it faults. Bases at the start, inside (odd), in the
/// middle and at the end of the table; negative indices; misaligned
/// elements where the scale is below the element size.
fn gather_inputs(rng: &mut Rng, f: &GFunc, n: usize) -> Vec<[u8; GCASE]> {
    let (elem, lanes) = lanes_of(f.ret);
    let size: i64 = if matches!(elem, "double" | "i64") { 8 } else { 4 };
    let (ielem, index_lanes) = lanes_of(f.index);
    let isize: usize = if ielem == "i64" { 8 } else { 4 };
    let used = lanes.min(index_lanes);
    let mut sets = Vec::new();
    for i in 0..n {
        let mut set = [0u8; GCASE];
        let src = vector(rng, f.ret, None);
        set[..src.len()].copy_from_slice(&src);
        // Unused index and mask lanes random.
        for b in &mut set[32..96] {
            *b = rng.next() as u8;
        }
        let base_off: i64 = match f.shape {
            Shape::MaskIsIndex => TABLE,
            _ => *rng.pick(&[0, 1001, TABLE / 2, TABLE]),
        };
        let null = f.shape == Shape::NullBase;
        for lane in 0..used {
            let on = match i {
                0 => true,
                1 => false,
                _ => rng.below(5) < 3,
            } && !(null && isize == 4);
            // The element's address, relative to the table.
            // 8 bytes of margin: the index rounds the address down by
            // less than the scale.
            let target = if on {
                8 + (rng.next() % (TABLE - size - 16) as u64) as i64
            } else if rng.below(2) == 0 || f.shape == Shape::MaskIsIndex {
                TABLE + 8 + (rng.next() % (PAGE - size - 16) as u64) as i64
            } else {
                -PAGE + 8 + (rng.next() % (PAGE - size - 16) as u64) as i64
            };
            // The index: the base's offset (none for a null base) plus the
            // index times the scale, at or below the target.
            let from = if null { 0 } else { base_off };
            let index = (target - from).div_euclid(f.scale);
            let at = 32 + lane * isize;
            set[at..at + isize].copy_from_slice(&index.to_le_bytes()[..isize]);
            if f.shape != Shape::MaskIsIndex {
                let m = rng.next() & !(1 << (8 * size - 1));
                let m = if on { m | 1 << (8 * size - 1) } else { m };
                let at = 64 + lane * size as usize;
                set[at..at + size as usize].copy_from_slice(&m.to_le_bytes()[..size as usize]);
            }
        }
        if f.shape == Shape::MaskIsSource {
            // The pass-through is the mask: keep the source bytes equal.
            let (head, tail) = set.split_at_mut(64);
            head[..32].copy_from_slice(&tail[..32]);
        }
        set[96..104].copy_from_slice(&base_off.to_le_bytes());
        set[104] = u8::from(null);
        sets.push(set);
    }
    sets
}

/// The gathers' C driver: maps the table between two inaccessible pages,
/// fills it, and prints the 96 bytes each call stores.
fn gather_driver(funcs: &[GFunc], sets: &[Vec<[u8; GCASE]>]) -> String {
    let mut c = String::from("#include <stdio.h>\n#include <string.h>\n#include <sys/mman.h>\n");
    for k in 0..funcs.len() {
        writeln!(c, "void g{k}(const void *, const void *, void *);").unwrap();
    }
    c.push_str("typedef void (*fn)(const void *, const void *, void *);\n");
    c.push_str("static const fn fns[] = {");
    for k in 0..funcs.len() {
        write!(c, "g{k},").unwrap();
    }
    c.push_str("};\n/* How many index lanes are 64 bits. */\nstatic const int q[] = {");
    for f in funcs {
        let lanes = if f.index.contains("i64") { lanes_of(f.index).1 } else { 0 };
        write!(c, "{lanes},").unwrap();
    }
    c.push_str("};\n/* The scale: a null base's index is the address over it. */\n");
    c.push_str("static const int scale[] = {");
    for f in funcs {
        write!(c, "{},", f.scale).unwrap();
    }
    writeln!(c, "}};\nstatic const unsigned char in[][{GCASE}] = {{").unwrap();
    for set in sets.iter().flatten() {
        c.push('{');
        for byte in set {
            write!(c, "{byte},").unwrap();
        }
        c.push_str("},\n");
    }
    writeln!(c, "}};\nstatic const int per = {};", sets[0].len()).unwrap();
    write!(
        c,
        "int main(void) {{\n  \
         unsigned char *map = mmap(0, {total}, PROT_READ | PROT_WRITE, \
         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n  \
         if (map == MAP_FAILED) return 2;\n  \
         setvbuf(stdout, 0, _IOLBF, 0);\n  \
         unsigned char *table = map + {PAGE};\n  \
         unsigned long long s = 0x9e3779b97f4a7c15ull;\n  \
         for (long i = 0; i < {TABLE}; i++) {{ s ^= s << 13; s ^= s >> 7; s ^= s << 17; \
         table[i] = (unsigned char)s; }}\n  \
         if (mprotect(map, {PAGE}, PROT_NONE) || mprotect(table + {TABLE}, {PAGE}, PROT_NONE)) \
         return 3;\n  \
         __builtin_cpu_init();\n  \
         int have = __builtin_cpu_supports(\"avx2\");\n  \
         for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++)\n    \
         for (int i = 0; i < per; i++) {{\n      \
         if (!have) {{ printf(\"%u %d skip\\n\", f, i); continue; }}\n      \
         unsigned char buf[{GCASE}] __attribute__((aligned(32)));\n      \
         memcpy(buf, in[f * per + i], {GCASE});\n      \
         long long off; memcpy(&off, buf + 96, 8);\n      \
         const unsigned char *base = table + off;\n      \
         if (buf[104]) {{\n        \
         base = 0;\n        \
         for (int k = 0; k < q[f]; k++) {{ long long v; memcpy(&v, buf + 32 + 8 * k, 8); \
         v += (long long)table / scale[f]; memcpy(buf + 32 + 8 * k, &v, 8); }}\n      \
         }}\n      \
         unsigned char out[96] __attribute__((aligned(32)));\n      memset(out, 0, 96);\n      \
         fns[f](buf, base, out);\n      \
         if (buf[104]) for (int k = 0; k < q[f]; k++) {{ long long v; \
         memcpy(&v, out + 64 + 8 * k, 8); v -= (long long)table / scale[f]; memcpy(out + 64 + 8 * k, &v, 8); }}\n      \
         printf(\"%u %d \", f, i);\n      \
         for (int k = 0; k < 96; k++) printf(\"%02x\", out[k]);\n      printf(\"\\n\");\n    }}\n  \
         return 0;\n}}\n",
        total = 2 * PAGE + TABLE
    )
    .unwrap();
    c
}

/// §5.6.33 (6): every AVX2 gather, at every scale, computes what `llc`'s
/// instruction computes, on LLVM 22 and 19, hod plain and stressed,
/// under bfd and lld; no lane whose mask is clear is read (it would
/// fault), and the mask and index values keep their bits.
#[test]
fn x86_gathers_agree_with_llc() {
    let (body, funcs) = gather_functions();
    let text = format!("{}{body}", ll_header());
    let mut rng = Rng(0x0207);
    let sets: Vec<Vec<[u8; GCASE]>> =
        funcs.iter().map(|f| gather_inputs(&mut rng, f, 24)).collect();
    let cases: usize = sets.iter().map(Vec::len).sum();

    let (module, bodies) = parse(&text);
    let scratch = Scratch::new("x64-x86-gather");
    let mut legs = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("hod {mode}: {e}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc {major}"), object));
        }
    }
    eprintln!(
        "x86 gathers: {} intrinsics, {} functions, {cases} cases",
        GATHERS.len(),
        funcs.len()
    );

    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let driver = toolchain.compile_c(&scratch, "driver", &gather_driver(&funcs, &sets), &[]);
    let mut runs: Vec<(String, String)> = Vec::new();
    for (leg, object) in &legs {
        for linker in toolchain.linkers.iter().filter(|l| matches!(l.name, "bfd" | "lld")) {
            let program = scratch.join(format!("{}-{}", leg.replace(' ', "-"), linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            // The last line printed names the case before the one that
            // stopped the program.
            assert!(
                ran.success(),
                "{leg} {}: {:?} after {:?}\n{}",
                linker.name,
                ran.status,
                ran.stdout_text().lines().last().map(|l| &l[..l.len().min(12)]),
                ran.stderr_text()
            );
            runs.push((format!("{leg} ({})", linker.name), ran.stdout_text()));
        }
    }
    let Some((reference_leg, reference)) =
        runs.iter().find(|(leg, _)| leg.starts_with("llc 22")).or(runs.first())
    else {
        return;
    };
    assert_eq!(reference.lines().count(), cases, "{reference_leg}: one line per case");
    let skipped = reference.lines().filter(|l| l.ends_with(" skip")).count();
    let mut wrong = Vec::new();
    for (leg, out) in &runs {
        assert_eq!(out.lines().count(), cases, "{leg}: one line per case");
        for (want, got) in reference.lines().zip(out.lines()) {
            if want != got {
                let f: usize = want.split(' ').next().unwrap().parse().unwrap();
                let g = &funcs[f];
                wrong.push(format!("{leg}: {} scale {}: want {want}, got {got}", g.name, g.scale));
            }
        }
    }
    eprintln!(
        "x86 gathers: legs {:?}; {skipped} of {cases} cases skipped (no AVX2)",
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(40)].join("\n")
    );
}
