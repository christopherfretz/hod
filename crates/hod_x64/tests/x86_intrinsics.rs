//! The x86 target intrinsics (WP-173, `spec/codegen.md` §5.6.22): every
//! `llvm.x86.*` name hod builds, called by generated functions on inputs
//! at the edges (zeros of both signs, all-ones, sign bits, NaNs with
//! payloads, infinities, denormals, the bounds of every conversion and
//! saturation, shift counts at and around the lane widths), compiled by
//! hod (plain, and under every stress option) and by `llc -O0
//! -fast-isel=0` of LLVM 22 and 19, linked with one C driver and run
//! under `qemu-x86_64`: every leg prints the same bytes.
//!
//! hod's programs run on the baseline CPU model (no SSE3, so an
//! instruction above SSE2 would trap); `llc`'s on the model with SSE3 and
//! SSSE3, which its code for those intrinsics needs. hod's plain object
//! is linked by bfd and lld.
//!
//! The intrinsics have no meaning in the interpreter, so `llc` is the
//! oracle: the hardware's (the emulator's) result of `llc`'s instruction.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::needless_range_loop,
    reason = "a test generator: `.ll` and C text built by hand"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;

use float_gen::{Rng, disassembly, ll_header, llc, mnemonic, parse, pic, text_size};
use hod_cg::{CgError, Stress};
use hod_testutil::Scratch;
use hod_testutil::target::cpu_with_features;
use linking::Toolchain;

/// The extension an intrinsic needs: 0 none, 1 SSE3, 2 SSSE3.
type Level = u8;

/// An intrinsic: name, result type, parameter types, level.
type Row = (&'static str, &'static str, &'static [&'static str], Level);

const F4: &str = "<4 x float>";
const D2: &str = "<2 x double>";
const B16: &str = "<16 x i8>";
const W8: &str = "<8 x i16>";
const I4: &str = "<4 x i32>";
const Q2: &str = "<2 x i64>";

/// Every intrinsic hod builds (the `hod_ir` names of WP-173 but `pause`,
/// which `wide_arith.rs` tests), with LLVM's signature.
const ROWS: &[Row] = &[
    ("llvm.x86.sse.cmp.ss", F4, &[F4, F4, "i8"], 0),
    ("llvm.x86.sse.comieq.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.comige.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.comigt.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.comile.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.comilt.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.comineq.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomieq.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomige.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomigt.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomile.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomilt.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.ucomineq.ss", "i32", &[F4, F4], 0),
    ("llvm.x86.sse.cvtss2si", "i32", &[F4], 0),
    ("llvm.x86.sse.cvtss2si64", "i64", &[F4], 0),
    ("llvm.x86.sse.cvttss2si", "i32", &[F4], 0),
    ("llvm.x86.sse.cvttss2si64", "i64", &[F4], 0),
    ("llvm.x86.sse.max.ps", F4, &[F4, F4], 0),
    ("llvm.x86.sse.max.ss", F4, &[F4, F4], 0),
    ("llvm.x86.sse.min.ps", F4, &[F4, F4], 0),
    ("llvm.x86.sse.min.ss", F4, &[F4, F4], 0),
    ("llvm.x86.sse.movmsk.ps", "i32", &[F4], 0),
    ("llvm.x86.sse.sfence", "void", &[], 0),
    ("llvm.x86.sse.ldmxcsr", "void", &["ptr"], 0),
    ("llvm.x86.sse.stmxcsr", "void", &["ptr"], 0),
    ("llvm.x86.sse.rcp.ps", F4, &[F4], 0),
    ("llvm.x86.sse.rcp.ss", F4, &[F4], 0),
    ("llvm.x86.sse.rsqrt.ps", F4, &[F4], 0),
    ("llvm.x86.sse.rsqrt.ss", F4, &[F4], 0),
    ("llvm.x86.sse2.cmp.sd", D2, &[D2, D2, "i8"], 0),
    ("llvm.x86.sse2.comieq.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.comige.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.comigt.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.comile.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.comilt.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.comineq.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomieq.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomige.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomigt.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomile.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomilt.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.ucomineq.sd", "i32", &[D2, D2], 0),
    ("llvm.x86.sse2.cvtpd2dq", I4, &[D2], 0),
    ("llvm.x86.sse2.cvtpd2ps", F4, &[D2], 0),
    ("llvm.x86.sse2.cvtps2dq", I4, &[F4], 0),
    ("llvm.x86.sse2.cvtsd2si", "i32", &[D2], 0),
    ("llvm.x86.sse2.cvtsd2si64", "i64", &[D2], 0),
    ("llvm.x86.sse2.cvtsd2ss", F4, &[F4, D2], 0),
    ("llvm.x86.sse2.cvttpd2dq", I4, &[D2], 0),
    ("llvm.x86.sse2.cvttps2dq", I4, &[F4], 0),
    ("llvm.x86.sse2.cvttsd2si", "i32", &[D2], 0),
    ("llvm.x86.sse2.cvttsd2si64", "i64", &[D2], 0),
    ("llvm.x86.sse2.lfence", "void", &[], 0),
    ("llvm.x86.sse2.mfence", "void", &[], 0),
    ("llvm.x86.sse2.clflush", "void", &["ptr"], 0),
    ("llvm.x86.sse2.maskmov.dqu", "void", &[B16, B16, "ptr"], 0),
    ("llvm.x86.sse2.max.pd", D2, &[D2, D2], 0),
    ("llvm.x86.sse2.max.sd", D2, &[D2, D2], 0),
    ("llvm.x86.sse2.min.pd", D2, &[D2, D2], 0),
    ("llvm.x86.sse2.min.sd", D2, &[D2, D2], 0),
    ("llvm.x86.sse2.movmsk.pd", "i32", &[D2], 0),
    ("llvm.x86.sse2.packssdw.128", W8, &[I4, I4], 0),
    ("llvm.x86.sse2.packsswb.128", B16, &[W8, W8], 0),
    ("llvm.x86.sse2.packuswb.128", B16, &[W8, W8], 0),
    ("llvm.x86.sse2.pavg.b", B16, &[B16, B16], 0),
    ("llvm.x86.sse2.pavg.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.pmadd.wd", I4, &[W8, W8], 0),
    ("llvm.x86.sse2.pmovmskb.128", "i32", &[B16], 0),
    ("llvm.x86.sse2.pmulh.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.pmulhu.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.psad.bw", Q2, &[B16, B16], 0),
    ("llvm.x86.sse2.psll.d", I4, &[I4, I4], 0),
    ("llvm.x86.sse2.psll.q", Q2, &[Q2, Q2], 0),
    ("llvm.x86.sse2.psll.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.pslli.d", I4, &[I4, "i32"], 0),
    ("llvm.x86.sse2.pslli.q", Q2, &[Q2, "i32"], 0),
    ("llvm.x86.sse2.pslli.w", W8, &[W8, "i32"], 0),
    ("llvm.x86.sse2.psra.d", I4, &[I4, I4], 0),
    ("llvm.x86.sse2.psra.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.psrai.d", I4, &[I4, "i32"], 0),
    ("llvm.x86.sse2.psrai.w", W8, &[W8, "i32"], 0),
    ("llvm.x86.sse2.psrl.d", I4, &[I4, I4], 0),
    ("llvm.x86.sse2.psrl.q", Q2, &[Q2, Q2], 0),
    ("llvm.x86.sse2.psrl.w", W8, &[W8, W8], 0),
    ("llvm.x86.sse2.psrli.d", I4, &[I4, "i32"], 0),
    ("llvm.x86.sse2.psrli.q", Q2, &[Q2, "i32"], 0),
    ("llvm.x86.sse2.psrli.w", W8, &[W8, "i32"], 0),
    ("llvm.x86.sse3.addsub.pd", D2, &[D2, D2], 1),
    ("llvm.x86.sse3.addsub.ps", F4, &[F4, F4], 1),
    ("llvm.x86.sse3.hadd.pd", D2, &[D2, D2], 1),
    ("llvm.x86.sse3.hadd.ps", F4, &[F4, F4], 1),
    ("llvm.x86.sse3.hsub.pd", D2, &[D2, D2], 1),
    ("llvm.x86.sse3.hsub.ps", F4, &[F4, F4], 1),
    ("llvm.x86.sse3.ldu.dq", B16, &["ptr"], 1),
    ("llvm.x86.ssse3.phadd.d.128", I4, &[I4, I4], 2),
    ("llvm.x86.ssse3.phadd.sw.128", W8, &[W8, W8], 2),
    ("llvm.x86.ssse3.phadd.w.128", W8, &[W8, W8], 2),
    ("llvm.x86.ssse3.phsub.d.128", I4, &[I4, I4], 2),
    ("llvm.x86.ssse3.phsub.sw.128", W8, &[W8, W8], 2),
    ("llvm.x86.ssse3.phsub.w.128", W8, &[W8, W8], 2),
    ("llvm.x86.ssse3.pmadd.ub.sw.128", W8, &[B16, B16], 2),
    ("llvm.x86.ssse3.pmul.hr.sw.128", W8, &[W8, W8], 2),
    ("llvm.x86.ssse3.pshuf.b.128", B16, &[B16, B16], 2),
    ("llvm.x86.ssse3.psign.b.128", B16, &[B16, B16], 2),
    ("llvm.x86.ssse3.psign.d.128", I4, &[I4, I4], 2),
    ("llvm.x86.ssse3.psign.w.128", W8, &[W8, W8], 2),
];

/// The features of the functions that call an SSE3 or SSSE3 intrinsic.
const FEATURES: &str = "+cx8,+fxsr,+mmx,+sse,+sse2,+sse3,+ssse3,+x87";

/// WP-181: `%m0` the caller's MXCSR, `%mx` it with the flags cleared
/// and the rounding mode the low two bits of `%c`.
const MXCSR_SET: &str = "  %m0 = alloca i32, align 4\n  %mx = alloca i32, align 4\n  \
     call void @llvm.x86.sse.stmxcsr(ptr %m0)\n  %v0 = load i32, ptr %m0, align 4\n  \
     %keep = and i32 %v0, -24640\n  %cm = shl i32 %c, 13\n  %mode = and i32 %cm, 24576\n  \
     %w = or i32 %keep, %mode\n  store i32 %w, ptr %mx, align 4\n";

/// WP-181: after `ldmxcsr(%mx)`: `cvtss2si` and an `addss` in its
/// rounding mode, the MXCSR they leave, then `%m0` back.
const MXCSR_READ: &str = "  %r1 = call i32 @llvm.x86.sse.cvtss2si(<4 x float> %a)\n  \
     store i32 %r1, ptr %out, align 16\n  %o4 = getelementptr i8, ptr %out, i64 4\n  \
     %a0 = extractelement <4 x float> %a, i32 0\n  %b0 = extractelement <4 x float> %b, i32 0\n  \
     %s = fadd float %a0, %b0\n  %o8 = getelementptr i8, ptr %out, i64 8\n  \
     store float %s, ptr %o8, align 8\n  call void @llvm.x86.sse.stmxcsr(ptr %o4)\n  \
     call void @llvm.x86.sse.ldmxcsr(ptr %m0)\n";

/// Shift counts: at, around and far above every lane width.
const COUNTS: [u32; 16] = [0, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256, u32::MAX];

/// Constant `i32` counts of the `pslli` family (each one function).
const CONST_COUNTS: [i64; 10] = [0, 1, 3, 15, 16, 31, 32, 63, 64, 300];

/// The `cmpss`/`cmpsd` predicates: the eight, and two with bits above
/// the three the legacy encoding reads.
const PREDICATES: [u8; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 9, 31];

/// One generated function: its text and the element types of its two
/// input vectors (for the inputs).
struct Func {
    name: String,
    a: &'static str,
    b: &'static str,
}

/// The generated module's functions (`void @tN(ptr %in, ptr %out)`) and
/// their declarations, attributes appended.
fn functions() -> (String, Vec<Func>) {
    let mut text = String::new();
    let mut decls = String::new();
    let mut funcs = Vec::new();
    for &(name, ret, params, level) in ROWS {
        let declared = format!("declare {ret} @{name}({})\n", params.join(", "));
        decls.push_str(&declared);
        let attrs = if level > 0 { " #0" } else { "" };
        let mxcsr = name.ends_with("mxcsr");
        let a = if mxcsr { F4 } else { params.first().copied().unwrap_or("i32") };
        let b = params.get(1).copied().filter(|p| p.starts_with('<')).unwrap_or(a);
        let mut variants: Vec<(String, Vec<String>)> = Vec::new();
        // What a variant does after the call (WP-181: MXCSR put back).
        let mut post = String::new();
        let vector_count =
            (name.contains(".psll.") || name.contains(".psrl.")) || name.contains(".psra.");
        match (name, params) {
            // WP-181: MXCSR from a valid image only (a reserved bit
            // faults): the original with its flags cleared and the
            // rounding mode from the low two bits of the count; then a
            // conversion and an addition in that mode, the flags they
            // raise, and the original back.
            ("llvm.x86.sse.ldmxcsr", _) => {
                variants.push((MXCSR_SET.into(), vec!["ptr %mx".into()]));
                post.push_str(MXCSR_READ);
            }
            ("llvm.x86.sse.stmxcsr", _) => {
                let prelude = format!(
                    "{MXCSR_SET}  call void @llvm.x86.sse.ldmxcsr(ptr %mx)\n  \
                     %a0 = extractelement <4 x float> %a, i32 0\n  \
                     %b0 = extractelement <4 x float> %b, i32 0\n  %q = fdiv float %a0, %b0\n  \
                     %o8 = getelementptr i8, ptr %out, i64 8\n  store float %q, ptr %o8, align 8\n"
                );
                variants.push((prelude, vec!["ptr %out".into()]));
                post.push_str("  call void @llvm.x86.sse.ldmxcsr(ptr %m0)\n");
            }
            (_, [x, y, "ptr"]) => {
                variants.push((
                    String::new(),
                    vec![format!("{x} %a"), format!("{y} %b"), "ptr %out".into()],
                ));
                variants.push((
                    String::new(),
                    vec![format!("{x} %a"), format!("{y} %a"), "ptr %out".into()],
                ));
            }
            (_, params) => match params {
                [] => variants.push((String::new(), Vec::new())),
                ["ptr"] => variants.push((
                    "  %q = getelementptr i8, ptr %in, i64 1\n".into(),
                    vec!["ptr %q".into()],
                )),
                [x] => variants.push((String::new(), vec![format!("{x} %a")])),
                [x, "i32"] => {
                    variants.push((String::new(), vec![format!("{x} %a"), "i32 %c".into()]));
                    for k in CONST_COUNTS {
                        variants.push((String::new(), vec![format!("{x} %a"), format!("i32 {k}")]));
                    }
                }
                [x, y, "i8"] => {
                    for p in PREDICATES {
                        variants.push((
                            String::new(),
                            vec![format!("{x} %a"), format!("{y} %b"), format!("i8 {p}")],
                        ));
                    }
                }
                [x, y] if vector_count => {
                    let count = format!(
                        "  %c64 = zext i32 %c to i64\n  %bq = bitcast {y} %b to <2 x i64>\n  \
                     %bq2 = insertelement <2 x i64> %bq, i64 %c64, i32 0\n  \
                     %bc = bitcast <2 x i64> %bq2 to {y}\n"
                    );
                    variants.push((count, vec![format!("{x} %a"), format!("{y} %bc")]));
                }
                [x, y] => {
                    variants.push((String::new(), vec![format!("{x} %a"), format!("{y} %b")]));
                    if x == y {
                        variants.push((String::new(), vec![format!("{x} %a"), format!("{y} %a")]));
                    }
                }
                _ => panic!("{name}: no generator for {params:?}"),
            },
        }
        for (prelude, args) in variants {
            let k = funcs.len();
            let call = format!("call {ret} @{name}({})", args.join(", "));
            let (result, store) = if ret == "void" {
                (format!("  {call}\n"), String::new())
            } else {
                (format!("  %r = {call}\n"), format!("  store {ret} %r, ptr %out, align 16\n"))
            };
            writeln!(
                text,
                "define void @t{k}(ptr %in, ptr %out){attrs} {{\n  \
                 %a = load {a}, ptr %in, align 16\n  %pb = getelementptr i8, ptr %in, i64 16\n  \
                 %b = load {b}, ptr %pb, align 16\n  %pc = getelementptr i8, ptr %in, i64 32\n  \
                 %c = load i32, ptr %pc, align 4\n{prelude}{result}{store}{post}  ret void\n}}\n"
            )
            .unwrap();
            funcs.push(Func { name: format!("{name}({})", args.join(", ")), a, b });
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
            0xff83_2100,
            1,
            0x807f_ffff,
            0x0080_0000,
            0x7f7f_ffff,
            0x4f00_0000,
            0xcf00_0000,
            0x4eff_ffff,
            0x5f00_0000,
            0xdf00_0000,
            0x5eff_ffff,
            0x5f80_0000,
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
            0xfff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0x7ff8_0000_1234_5678,
            0xfff4_0000_0000_4321,
            1,
            0x800f_ffff_ffff_ffff,
            0x0010_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
            0x41e0_0000_0000_0000,
            0xc1e0_0000_0000_0000,
            0x41df_ffff_ffc0_0000,
            0xc1e0_0000_0020_0000,
            0x43e0_0000_0000_0000,
            0xc3e0_0000_0000_0000,
            0x43df_ffff_ffff_ffff,
            0x47ef_ffff_e000_0000,
            0x3810_0000_0000_0000,
            0x36a0_0000_0000_0000,
        ],
        int => {
            let bits: u32 = int.trim_start_matches('i').parse().unwrap();
            let mask = if bits == 64 { u64::MAX } else { (1 << bits) - 1 };
            let min = 1u64 << (bits - 1);
            let mut v = vec![0, 1, 2, 3, mask, mask - 1, min, min - 1, min + 1, 0x55, 0x80, 0xff];
            v.extend([0x7f, 0x81, 0x7fff, 0x8000, 0x8001, 0xffff, 0x1_0000, 0x7fff_ffff]);
            v.extend([0x8000_0000, 0xffff_ff80, 0x4000, 0xc000, 15, 16, 0x8f]);
            v.into_iter().map(|x| x & mask).collect()
        }
    }
}

/// The element type and lane count of a vector type's text.
fn lanes_of(ty: &str) -> (&str, u32) {
    let inner = ty.trim_start_matches('<').trim_end_matches('>');
    let (n, elem) = inner.split_once(" x ").unwrap_or(("4", "i32"));
    (elem, n.parse().unwrap())
}

/// 16 bytes of a vector of `ty` (`i32` and `ptr` operands take the
/// 32-bit vector's): lanes from the edges (often) or random.
fn vector(rng: &mut Rng, ty: &str, fill: Option<u64>) -> [u8; 16] {
    let ty = if ty.starts_with('<') { ty } else { I4 };
    let (elem, n) = lanes_of(ty);
    let width = 16 / n as usize;
    let list = edges(elem);
    let mut out = [0u8; 16];
    for lane in 0..n as usize {
        let bits = match fill {
            Some(f) => f,
            None if rng.below(3) < 2 => *rng.pick(&list),
            None => rng.next(),
        };
        out[lane * width..(lane + 1) * width].copy_from_slice(&bits.to_le_bytes()[..width]);
    }
    out
}

/// The inputs of function `f`: 48 bytes each (`a`, `b`, the count at
/// 32).
fn inputs(rng: &mut Rng, f: &Func, n: usize) -> Vec<[u8; 48]> {
    let mut sets = Vec::new();
    let nan = |elem: &str, k: u64| -> u64 {
        if elem == "double" {
            0x7ff8_0000_0000_0000 | (k * 0x1111)
        } else {
            0x7fc0_0000 | (k * 0x111)
        }
    };
    let (ea, _) = lanes_of(if f.a.starts_with('<') { f.a } else { I4 });
    let (eb, _) = lanes_of(if f.b.starts_with('<') { f.b } else { I4 });
    for i in 0..n {
        let mut set = [0u8; 48];
        let (a, b) = match i {
            // NaNs of different payloads in every lane of both operands
            // (which operand's NaN wins), signed zeros against each other.
            0 => (vector(rng, f.a, Some(nan(ea, 1))), vector(rng, f.b, Some(nan(eb, 2)))),
            1 => {
                let mut a = vector(rng, f.a, Some(nan(ea, 3)));
                if ea == "float" {
                    a[4..8].copy_from_slice(&0xffc0_5000u32.to_le_bytes());
                    a[12..16].copy_from_slice(&0x7f80_0007u32.to_le_bytes());
                } else if ea == "double" {
                    a[8..16].copy_from_slice(&0xfff8_0000_0000_0777u64.to_le_bytes());
                }
                (a, vector(rng, f.b, None))
            }
            2 => (vector(rng, f.a, Some(0)), vector(rng, f.b, Some(1 << 63))),
            3 => (vector(rng, f.a, Some(1 << 63)), vector(rng, f.b, Some(0))),
            4 => (vector(rng, f.a, Some(u64::MAX)), vector(rng, f.b, Some(u64::MAX))),
            _ => (vector(rng, f.a, None), vector(rng, f.b, None)),
        };
        set[..16].copy_from_slice(&a);
        set[16..32].copy_from_slice(&b);
        let count = if i < COUNTS.len() { COUNTS[i] } else { rng.next() as u32 % 70 };
        set[32..36].copy_from_slice(&count.to_le_bytes());
        set[36..44].copy_from_slice(&rng.next().to_le_bytes());
        set[44..].copy_from_slice(&rng.next().to_le_bytes()[..4]);
        sets.push(set);
    }
    sets
}

/// The C driver: runs every function on its inputs and prints the 16
/// result bytes of each call.
fn driver(funcs: &[Func], sets: &[Vec<[u8; 48]>]) -> String {
    let mut c = String::from("#include <stdio.h>\n#include <string.h>\n");
    for k in 0..funcs.len() {
        writeln!(c, "void t{k}(const void *, void *);").unwrap();
    }
    c.push_str("typedef void (*fn)(const void *, void *);\nstatic const fn fns[] = {");
    for k in 0..funcs.len() {
        write!(c, "t{k},").unwrap();
    }
    c.push_str("};\nstatic const unsigned char in[][48] __attribute__((aligned(16))) = {\n");
    for set in sets.iter().flatten() {
        c.push('{');
        for byte in set {
            write!(c, "{byte},").unwrap();
        }
        c.push_str("},\n");
    }
    writeln!(c, "}};\nstatic const int per = {};", sets[0].len()).unwrap();
    c.push_str(
        "int main(void) {\n  for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++)\n    \
         for (int i = 0; i < per; i++) {\n      \
         unsigned char out[16] __attribute__((aligned(16)));\n      memset(out, 0, 16);\n      \
         fns[f](in[f * per + i], out);\n      printf(\"%u %d \", f, i);\n      \
         for (int k = 0; k < 16; k++) printf(\"%02x\", out[k]);\n      printf(\"\\n\");\n    }\n  \
         return 0;\n}\n",
    );
    c
}

/// Whether `m` is an instruction above SSE2 that SSE3 or SSSE3 adds.
fn above_sse2(m: &str) -> bool {
    m.starts_with("hadd")
        || m.starts_with("hsub")
        || m.starts_with("addsub")
        || m.starts_with("phadd")
        || m.starts_with("phsub")
        || m.starts_with("psign")
        || m.starts_with("pabs")
        || matches!(m, "lddqu" | "pshufb" | "pmulhrsw" | "pmaddubsw" | "palignr" | "movddup")
        || m.starts_with("movsh")
        || m.starts_with('v')
}

/// §5.6.22: every intrinsic hod builds computes what `llc`'s instruction
/// computes, on LLVM 22 and 19, hod plain and stressed, under bfd and
/// lld; hod's code is SSE2 only.
#[test]
fn x86_intrinsics_agree_with_llc() {
    let (body, funcs) = functions();
    let text = format!("{}{body}", ll_header());
    let mut rng = Rng(0x0173);
    let sets: Vec<Vec<[u8; 48]>> = funcs.iter().map(|f| inputs(&mut rng, f, 40)).collect();
    let cases: usize = sets.iter().map(Vec::len).sum();

    let (module, bodies) = parse(&text);
    let scratch = Scratch::new("x64-x86-intrinsics");
    let mut legs = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("hod {mode}: {e}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    let hod_bytes = text_size(&legs[0].1);
    let mut llc_bytes = 0;
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            if major == 22 {
                llc_bytes = text_size(&object);
            }
            legs.push((format!("llc {major}"), object));
        }
    }
    if let Some(lines) = disassembly(&body, Stress::default()) {
        for line in &lines {
            assert!(!above_sse2(mnemonic(line)), "hod wrote {line}");
        }
    }
    eprintln!(
        "x86 intrinsics: {} intrinsics, {} functions, {cases} cases; .text hod {hod_bytes} \
         bytes, llc 22 {llc_bytes} (hod/llc {:.2})",
        ROWS.len(),
        funcs.len(),
        hod_bytes as f64 / llc_bytes.max(1) as f64
    );

    let Some(base) = Toolchain::x86_64(&scratch) else { return };
    // A toolchain of its own scratch directory (each makes its linker
    // directories there).
    let featured_scratch = Scratch::new("x64-x86-intrinsics-cpu");
    let featured = Toolchain::x86_64(&featured_scratch).map(|t| {
        let cpu = cpu_with_features(t.target.emulated_cpu(), &["sse3", "ssse3"]);
        t.with_emulated_cpu(&cpu)
    });
    let Some(featured) = featured else { return };
    let driver = base.compile_c(&scratch, "driver", &driver(&funcs, &sets), &[]);
    let mut runs: Vec<(String, String)> = Vec::new();
    for (leg, object) in &legs {
        let toolchain = if leg.starts_with("llc") { &featured } else { &base };
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
    eprintln!("x86 intrinsics: legs {:?}", runs.iter().map(|r| &r.0).collect::<Vec<_>>());
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

/// §5.6.22: an SSE3 or SSSE3 intrinsic in a function whose features do
/// not enable it is refused by its own name; enabled, or implied by a
/// later extension, it builds; turned off again by a later item, refused.
#[test]
fn above_the_baseline_only_with_the_feature() {
    for &(name, ret, params, level) in ROWS.iter().filter(|r| r.3 > 0) {
        let args: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(i, p)| if *p == "ptr" { "ptr %p".into() } else { format!("{p} %v{i}") })
            .collect();
        let mut loads = String::new();
        for (i, p) in params.iter().enumerate().filter(|(_, p)| **p != "ptr") {
            writeln!(loads, "  %v{i} = load {p}, ptr %p, align 16").unwrap();
        }
        let module = |features: Option<&str>| {
            let (mark, attrs) = match features {
                Some(f) => (" #0", format!("attributes #0 = {{ \"target-features\"=\"{f}\" }}\n")),
                None => ("", String::new()),
            };
            format!(
                "declare {ret} @{name}({})\ndefine void @f(ptr %p){mark} {{\n{loads}  \
                 %r = call {ret} @{name}({})\n  store {ret} %r, ptr %p, align 16\n  ret void\n}}\n\
                 {attrs}",
                params.join(", "),
                args.join(", ")
            )
        };
        let refusal = format!("intrinsic:{name}");
        let refused = |text: String| match compile(&text) {
            Err(CgError::Unsupported { construct }) => assert_eq!(construct, refusal, "{text}"),
            other => panic!("{name}: {other:?}\n{text}"),
        };
        refused(module(None));
        refused(module(Some("+sse,+sse2")));
        let needed = if level == 1 { "+sse3" } else { "+ssse3" };
        compile(&module(Some(&format!("+sse,+sse2,{needed}")))).unwrap();
        compile(&module(Some("+sse4.2"))).unwrap();
        compile(&module(Some("+avx2"))).unwrap();
        refused(module(Some(&format!("{needed},-sse3"))));
        if level == 2 {
            refused(module(Some("+sse3")));
        }
    }
}

/// Every intrinsic of this package is an x86-64 row and refused on
/// AArch64; an `llvm.x86.*` name hod does not build stays unknown, which
/// `classify` refuses by that name on both targets.
#[test]
fn rows_and_refusals_by_name() {
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for &(name, ..) in ROWS {
        let row = format!("intrinsic:{name}");
        assert_eq!(x64.supports(&row), Some(true), "{row}");
        assert_eq!(a64.supports(&row), Some(false), "{row}");
    }
    // WP-181: `monitor`/`mwait` stay unknown (no encoding).
    let unknown = "declare void @llvm.x86.sse3.monitor(ptr, i32, i32)\ndefine void @f(ptr %p) {\n  \
                   call void @llvm.x86.sse3.monitor(ptr %p, i32 0, i32 0)\n  ret void\n}\n";
    let (m, bodies) = parse(&format!("{}{unknown}", ll_header()));
    // No row on either target: `classify` names it whatever the target.
    for name in ["intrinsic:llvm.x86.sse3.monitor", "intrinsic:llvm.x86.sse3.mwait"] {
        assert_eq!(x64.supports(name), None, "{name}");
        assert_eq!(a64.supports(name), None, "{name}");
    }
    {
        let classifier = hod_ir::Classifier::new(x64);
        let refused = classifier.body(&m, &bodies[0]).expect_err("an unknown intrinsic");
        assert_eq!(format!("{}", refused.construct), "intrinsic:llvm.x86.sse3.monitor");
    }
    // A signature LLVM does not declare the intrinsic with.
    let wrong = "declare i32 @llvm.x86.sse2.pmovmskb.128(<8 x i8>)\ndefine i32 @f(<8 x i8> %v) {\n  \
                 %r = call i32 @llvm.x86.sse2.pmovmskb.128(<8 x i8> %v)\n  ret i32 %r\n}\n";
    assert_eq!(compile(wrong), Err(CgError::unsupported(hod_x64::x86::SIGNATURE)));
}

// ---- §5.6.29: the system and state intrinsics (WP-190) ----

/// Every feature an intrinsic of §5.6.29 needs, on every function of the
/// execution test.
const SYS_FEATURES: &str = "+cx8,+fxsr,+mmx,+sse,+sse2,+x87,+xsave,+xsaveopt,+xsavec,+xsaves,\
                            +rdrnd,+rdseed,+clflushopt,+clwb,+bmi,+bmi2,+crc32,+fsgsbase,+rtm,\
                            +rdpid,+pku,+serialize,+shstk";

/// The features of the CPU model the execution test runs on (each one
/// QEMU 10 implements in user mode; SSE3 to SSE4.2 because the C
/// library's startup selects its string functions by them).
const SYS_CPU: &[&str] = &[
    "sse3",
    "ssse3",
    "sse4.1",
    "sse4.2",
    "rdtscp",
    "xsave",
    "xsaveopt",
    "rdrand",
    "rdseed",
    "clflushopt",
    "clwb",
    "bmi1",
    "bmi2",
    "fsgsbase",
    "rdpid",
];

/// A deterministic intrinsic: name, result type, parameter types.
type SysRow = (&'static str, &'static str, &'static [&'static str]);

/// The deterministic intrinsics: hod's result equals `llc`'s byte for byte.
const SYS_DETERMINISTIC: &[SysRow] = &[
    ("llvm.x86.addcarry.32", "{ i8, i32 }", &["i8", "i32", "i32"]),
    ("llvm.x86.addcarry.64", "{ i8, i64 }", &["i8", "i64", "i64"]),
    ("llvm.x86.subborrow.32", "{ i8, i32 }", &["i8", "i32", "i32"]),
    ("llvm.x86.subborrow.64", "{ i8, i64 }", &["i8", "i64", "i64"]),
    ("llvm.x86.bmi.bextr.32", "i32", &["i32", "i32"]),
    ("llvm.x86.bmi.bextr.64", "i64", &["i64", "i64"]),
    ("llvm.x86.bmi.bzhi.32", "i32", &["i32", "i32"]),
    ("llvm.x86.bmi.bzhi.64", "i64", &["i64", "i64"]),
    ("llvm.x86.bmi.pdep.32", "i32", &["i32", "i32"]),
    ("llvm.x86.bmi.pdep.64", "i64", &["i64", "i64"]),
    ("llvm.x86.bmi.pext.32", "i32", &["i32", "i32"]),
    ("llvm.x86.bmi.pext.64", "i64", &["i64", "i64"]),
    ("llvm.x86.sse42.crc32.32.8", "i32", &["i32", "i8"]),
    ("llvm.x86.sse42.crc32.32.16", "i32", &["i32", "i16"]),
    ("llvm.x86.sse42.crc32.32.32", "i32", &["i32", "i32"]),
    ("llvm.x86.sse42.crc32.64.64", "i64", &["i64", "i64"]),
    // A `nop` without shadow stacks (QEMU's user mode has none): the
    // operand.
    ("llvm.x86.rdsspd", "i32", &["i32"]),
    ("llvm.x86.rdsspq", "i64", &["i64"]),
];

/// The operand values the deterministic intrinsics are called on.
const SYS_EDGES: [u64; 18] = [
    0,
    1,
    2,
    0xff,
    0x0804,
    0x2010,
    0xff00,
    0x4020,
    0x8000_0000,
    0xffff_ffff,
    0x1_0000_0000,
    0x5555_5555_5555_5555,
    0xaaaa_aaaa_aaaa_aaaa,
    0x0f0f_00ff_ff00_f0f0,
    0x7fff_ffff_ffff_ffff,
    0x8000_0000_0000_0000,
    0xffff_ffff_ffff_fffe,
    u64::MAX,
];

/// The state intrinsics the driver runs: name of the save and restore
/// functions' intrinsics, and whether they take the component mask.
const SYS_STATE: &[(&str, &str, bool)] = &[
    ("llvm.x86.fxsave", "llvm.x86.fxrstor", false),
    ("llvm.x86.fxsave64", "llvm.x86.fxrstor64", false),
    ("llvm.x86.xsave", "llvm.x86.xrstor", true),
    ("llvm.x86.xsave64", "llvm.x86.xrstor64", true),
    ("llvm.x86.xsaveopt", "llvm.x86.xrstor", true),
    ("llvm.x86.xsaveopt64", "llvm.x86.xrstor64", true),
];

/// Intrinsics only compiled (QEMU's user mode traps them, or they need a
/// privilege or a transaction): the function, its `.ll` body, and the
/// mnemonic hod's code of it must contain.
const SYS_COMPILED: &[(&str, &str, &str)] = &[
    ("rdpmc", "%r = call i64 @llvm.x86.rdpmc(i32 %a)\n  ret i64 %r", "rdpmc"),
    ("xsetbv", "call void @llvm.x86.xsetbv(i32 %a, i32 %a, i32 0)\n  ret i64 0", "xsetbv"),
    ("xsavec", "call void @llvm.x86.xsavec(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xsavec"),
    ("xsavec64", "call void @llvm.x86.xsavec64(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xsavec64"),
    ("xsaves", "call void @llvm.x86.xsaves(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xsaves"),
    ("xsaves64", "call void @llvm.x86.xsaves64(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xsaves64"),
    ("xrstors", "call void @llvm.x86.xrstors(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xrstors"),
    ("xrstors64", "call void @llvm.x86.xrstors64(ptr %p, i32 0, i32 %a)\n  ret i64 0", "xrstors64"),
    (
        "xbegin",
        "%r = call i32 @llvm.x86.xbegin()\n  %z = zext i32 %r to i64\n  ret i64 %z",
        "xbegin",
    ),
    ("xend", "call void @llvm.x86.xend()\n  ret i64 0", "xend"),
    ("xabort", "call void @llvm.x86.xabort(i8 7)\n  ret i64 0", "xabort"),
    ("xtest", "%r = call i32 @llvm.x86.xtest()\n  %z = zext i32 %r to i64\n  ret i64 %z", "xtest"),
    (
        "rdpkru",
        "%r = call i32 @llvm.x86.rdpkru()\n  %z = zext i32 %r to i64\n  ret i64 %z",
        "rdpkru",
    ),
    ("wrpkru", "call void @llvm.x86.wrpkru(i32 %a)\n  ret i64 0", "wrpkru"),
    ("serialize", "call void @llvm.x86.serialize()\n  ret i64 0", "serialize"),
    ("incsspd", "call void @llvm.x86.incsspd(i32 %a)\n  ret i64 0", "incsspd"),
    (
        "incsspq",
        "%w = zext i32 %a to i64\n  call void @llvm.x86.incsspq(i64 %w)\n  ret i64 0",
        "incsspq",
    ),
    ("wrfsbase32", "call void @llvm.x86.wrfsbase.32(i32 %a)\n  ret i64 0", "wrfsbase"),
    (
        "wrfsbase64",
        "%w = zext i32 %a to i64\n  call void @llvm.x86.wrfsbase.64(i64 %w)\n  ret i64 0",
        "wrfsbase",
    ),
];

/// The run-time functions the driver calls for the shape checks and the
/// state round trips, after the deterministic ones.
fn sys_runtime() -> String {
    let mut ll = String::new();
    for (k, &(save, restore, masked)) in SYS_STATE.iter().enumerate() {
        for (role, name) in [("save", save), ("restore", restore)] {
            let call = if masked {
                format!("call void @{name}(ptr %p, i32 0, i32 3)")
            } else {
                format!("call void @{name}(ptr %p)")
            };
            writeln!(ll, "define void @{role}{k}(ptr %p) #0 {{\n  {call}\n  ret void\n}}").unwrap();
        }
    }
    ll.push_str(
        "define i64 @xgetbv0() #0 {\n  %r = call i64 @llvm.x86.xgetbv(i32 0)\n  ret i64 %r\n}\n\
         define i64 @rdtsc() #0 {\n  %r = call i64 @llvm.x86.rdtsc()\n  ret i64 %r\n}\n\
         define void @rdtscp(ptr %out) #0 {\n  %r = call { i64, i32 } @llvm.x86.rdtscp()\n  \
         %t = extractvalue { i64, i32 } %r, 0\n  %a = extractvalue { i64, i32 } %r, 1\n  \
         store i64 %t, ptr %out\n  %o = getelementptr i8, ptr %out, i64 8\n  \
         store i32 %a, ptr %o\n  ret void\n}\n\
         define i64 @rdfsbase64() #0 {\n  %r = call i64 @llvm.x86.rdfsbase.64()\n  ret i64 %r\n}\n\
         define i32 @rdfsbase32() #0 {\n  %r = call i32 @llvm.x86.rdfsbase.32()\n  ret i32 %r\n}\n\
         define i64 @rdgsbase64() #0 {\n  %r = call i64 @llvm.x86.rdgsbase.64()\n  ret i64 %r\n}\n\
         define i32 @rdgsbase32() #0 {\n  %r = call i32 @llvm.x86.rdgsbase.32()\n  ret i32 %r\n}\n\
         define void @wrgsbase64(i64 %v) #0 {\n  call void @llvm.x86.wrgsbase.64(i64 %v)\n  \
         ret void\n}\n\
         define void @wrgsbase32(i32 %v) #0 {\n  call void @llvm.x86.wrgsbase.32(i32 %v)\n  \
         ret void\n}\n\
         define i32 @rdpid() #0 {\n  %r = call i32 @llvm.x86.rdpid()\n  ret i32 %r\n}\n\
         define void @lines(ptr %p) #0 {\n  call void @llvm.x86.clflushopt(ptr %p)\n  \
         call void @llvm.x86.clwb(ptr %p)\n  ret void\n}\n\
         define i64 @flags(i64 %x) #0 {\n  %m = and i64 %x, 2261\n  %v = or i64 %m, 514\n  \
         call void @llvm.x86.flags.write.u64(i64 %v)\n  \
         %r = call i64 @llvm.x86.flags.read.u64()\n  %k = and i64 %r, 4095\n  ret i64 %k\n}\n",
    );
    for name in ["rdrand", "rdseed"] {
        for bits in [16, 32, 64] {
            writeln!(
                ll,
                "define void @{name}{bits}(ptr %out) #0 {{\n  \
                 %r = call {{ i{bits}, i32 }} @llvm.x86.{name}.{bits}()\n  \
                 %v = extractvalue {{ i{bits}, i32 }} %r, 0\n  \
                 %f = extractvalue {{ i{bits}, i32 }} %r, 1\n  store i{bits} %v, ptr %out\n  \
                 %o = getelementptr i8, ptr %out, i64 8\n  store i32 %f, ptr %o\n  ret void\n}}"
            )
            .unwrap();
        }
    }
    for &(name, body, _) in SYS_COMPILED {
        writeln!(ll, "define i64 @c_{name}(ptr %p, i32 %a) #0 {{\n  {body}\n}}").unwrap();
    }
    ll
}

/// The declarations of every intrinsic the functions call.
fn sys_declarations() -> String {
    let mut d = String::new();
    for &(name, ret, params) in SYS_DETERMINISTIC {
        writeln!(d, "declare {ret} @{name}({})", params.join(", ")).unwrap();
    }
    for name in ["fxsave", "fxsave64", "fxrstor", "fxrstor64", "clflushopt", "clwb"] {
        writeln!(d, "declare void @llvm.x86.{name}(ptr)").unwrap();
    }
    for name in [
        "xsave",
        "xsave64",
        "xrstor",
        "xrstor64",
        "xsaveopt",
        "xsaveopt64",
        "xsavec",
        "xsavec64",
        "xsaves",
        "xsaves64",
        "xrstors",
        "xrstors64",
    ] {
        writeln!(d, "declare void @llvm.x86.{name}(ptr, i32, i32)").unwrap();
    }
    for bits in [16, 32, 64] {
        writeln!(d, "declare {{ i{bits}, i32 }} @llvm.x86.rdrand.{bits}()").unwrap();
        writeln!(d, "declare {{ i{bits}, i32 }} @llvm.x86.rdseed.{bits}()").unwrap();
    }
    d.push_str(
        "declare i64 @llvm.x86.xgetbv(i32)\ndeclare void @llvm.x86.xsetbv(i32, i32, i32)\n\
         declare i64 @llvm.x86.rdtsc()\ndeclare { i64, i32 } @llvm.x86.rdtscp()\n\
         declare i64 @llvm.x86.rdpmc(i32)\n\
         declare i64 @llvm.x86.flags.read.u64()\ndeclare void @llvm.x86.flags.write.u64(i64)\n\
         declare i64 @llvm.x86.rdfsbase.64()\ndeclare i32 @llvm.x86.rdfsbase.32()\n\
         declare i64 @llvm.x86.rdgsbase.64()\ndeclare i32 @llvm.x86.rdgsbase.32()\n\
         declare void @llvm.x86.wrfsbase.64(i64)\ndeclare void @llvm.x86.wrfsbase.32(i32)\n\
         declare void @llvm.x86.wrgsbase.64(i64)\ndeclare void @llvm.x86.wrgsbase.32(i32)\n\
         declare i32 @llvm.x86.rdpid()\ndeclare i32 @llvm.x86.xbegin()\n\
         declare void @llvm.x86.xend()\ndeclare void @llvm.x86.xabort(i8 immarg)\n\
         declare i32 @llvm.x86.xtest()\ndeclare i32 @llvm.x86.rdpkru()\n\
         declare void @llvm.x86.wrpkru(i32)\ndeclare void @llvm.x86.serialize()\n\
         declare void @llvm.x86.incsspd(i32)\ndeclare void @llvm.x86.incsspq(i64)\n",
    );
    d
}

/// The deterministic functions `d<k>(ptr in, ptr out)`: the operands are
/// the three 64-bit words at `in` (truncated), or (`same`) the first word
/// for every operand; an integer result is stored at `out`, a pair's
/// fields at `out` and `out + 8`.
fn sys_deterministic() -> (String, Vec<&'static str>) {
    let mut ll = String::new();
    let mut names = Vec::new();
    for same in [false, true] {
        for &(name, ret, params) in SYS_DETERMINISTIC {
            let k = names.len();
            names.push(name);
            let mut f = format!("define void @d{k}(ptr %in, ptr %out) #0 {{\n");
            let mut args = Vec::new();
            for (i, ty) in params.iter().enumerate() {
                let word = if same { 0 } else { i };
                writeln!(f, "  %g{i} = getelementptr i8, ptr %in, i64 {}", word * 8).unwrap();
                writeln!(f, "  %w{i} = load i64, ptr %g{i}").unwrap();
                if *ty == "i64" {
                    args.push(format!("i64 %w{i}"));
                } else {
                    writeln!(f, "  %a{i} = trunc i64 %w{i} to {ty}").unwrap();
                    args.push(format!("{ty} %a{i}"));
                }
            }
            if same {
                // One value for every operand of the type of an earlier one.
                for i in 1..params.len() {
                    if let Some(j) = (0..i).find(|&j| params[j] == params[i]) {
                        let shared = args[j].clone();
                        args[i] = shared;
                    }
                }
            }
            writeln!(f, "  %r = call {ret} @{name}({})", args.join(", ")).unwrap();
            if let Some(inner) = ret.strip_prefix("{ ").and_then(|r| r.strip_suffix(" }")) {
                let (t0, t1) = inner.split_once(", ").unwrap();
                writeln!(
                    f,
                    "  %r0 = extractvalue {ret} %r, 0\n  %r1 = extractvalue {ret} %r, 1\n  \
                     store {t0} %r0, ptr %out\n  %o = getelementptr i8, ptr %out, i64 8\n  \
                     store {t1} %r1, ptr %o"
                )
                .unwrap();
            } else {
                writeln!(f, "  store {ret} %r, ptr %out").unwrap();
            }
            f.push_str("  ret void\n}\n");
            ll.push_str(&f);
        }
    }
    (ll, names)
}

/// The C driver of the §5.6.29 test: prints the deterministic results,
/// the state round trips and the shape checks, one line each.
fn sys_driver(count: usize, inputs: &[[u64; 3]]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n\
         typedef void (*dfn)(const void *, void *);\n",
    );
    for k in 0..count {
        writeln!(c, "void d{k}(const void *, void *);").unwrap();
    }
    c.push_str("static const dfn ds[] = {");
    for k in 0..count {
        write!(c, "d{k},").unwrap();
    }
    c.push_str("};\nstatic const uint64_t in[][3] = {\n");
    for set in inputs {
        writeln!(c, "{{{:#x}u,{:#x}u,{:#x}u}},", set[0], set[1], set[2]).unwrap();
    }
    c.push_str("};\ntypedef void (*sfn)(void *);\n");
    for k in 0..SYS_STATE.len() {
        writeln!(c, "void save{k}(void *); void restore{k}(void *);").unwrap();
    }
    c.push_str("static const sfn saves[] = {");
    for k in 0..SYS_STATE.len() {
        write!(c, "save{k},").unwrap();
    }
    c.push_str("};\nstatic const sfn restores[] = {");
    for k in 0..SYS_STATE.len() {
        write!(c, "restore{k},").unwrap();
    }
    c.push_str(
        "};\n\
         uint64_t xgetbv0(void); uint64_t rdtsc(void); void rdtscp(void *);\n\
         uint64_t rdfsbase64(void); uint32_t rdfsbase32(void);\n\
         uint64_t rdgsbase64(void); uint32_t rdgsbase32(void);\n\
         void wrgsbase64(uint64_t); void wrgsbase32(uint32_t);\n\
         uint32_t rdpid(void); void lines(void *); uint64_t flags(uint64_t);\n\
         void rdrand16(void *); void rdrand32(void *); void rdrand64(void *);\n\
         void rdseed16(void *); void rdseed32(void *); void rdseed64(void *);\n\
         static uint32_t csr(void) { uint32_t v; __asm__ volatile(\"stmxcsr %0\" : \"=m\"(v)); \
         return v; }\n\
         static uint16_t cw(void) { uint16_t v; __asm__ volatile(\"fnstcw %0\" : \"=m\"(v)); \
         return v; }\n\
         static unsigned char a[1024] __attribute__((aligned(64)));\n\
         static unsigned char b[1024] __attribute__((aligned(64)));\n\
         static unsigned char s[1024] __attribute__((aligned(64)));\n\
         static uint16_t u16(const unsigned char *p) { uint16_t v; memcpy(&v, p, 2); return v; }\n\
         static uint32_t u32(const unsigned char *p) { uint32_t v; memcpy(&v, p, 4); return v; }\n\
         int main(void) {\n\
           const int per = sizeof in / sizeof *in / (sizeof ds / sizeof *ds);\n\
           for (unsigned f = 0; f < sizeof ds / sizeof *ds; f++)\n\
             for (int i = 0; i < per; i++) {\n\
               unsigned char out[16];\n\
               memset(out, 0, 16);\n\
               ds[f](in[f * per + i], out);\n\
               printf(\"d %u %d \", f, i);\n\
               for (int k = 0; k < 16; k++) printf(\"%02x\", out[k]);\n\
               printf(\"\\n\");\n\
             }\n\
           for (unsigned k = 0; k < sizeof saves / sizeof *saves; k++) {\n\
             memset(a, 0, sizeof a); memset(b, 0, sizeof b); memset(s, 0, sizeof s);\n\
             saves[k](a);\n\
             int fcw0 = u16(a) == cw(), csr0 = u32(a + 24) == csr();\n\
             memcpy(b, a, sizeof b);\n\
             uint16_t fcw = 0x0f7f; uint32_t mx = 0x7f80;\n\
             memcpy(b, &fcw, 2); memcpy(b + 24, &mx, 4);\n\
             b[512] |= 3;\n\
             restores[k](b);\n\
             uint16_t cw1 = cw(); uint32_t csr1 = csr();\n\
             saves[k](s);\n\
             restores[k](a);\n\
             printf(\"state %u %d %d %04x %08x %04x %08x %04x %08x\\n\", k, fcw0, csr0, cw1, \
         csr1, u16(s), u32(s + 24), cw(), csr());\n\
           }\n\
           printf(\"xgetbv0 %llx\\n\", (unsigned long long)xgetbv0());\n\
           uint64_t t0 = rdtsc(), t1 = rdtsc();\n\
           unsigned char p0[16], p1[16];\n\
           rdtscp(p0); rdtscp(p1);\n\
           uint64_t c0, c1; memcpy(&c0, p0, 8); memcpy(&c1, p1, 8);\n\
           printf(\"rdtsc %d %d %d\\n\", t1 >= t0, c1 >= c0, u32(p0 + 8) == u32(p1 + 8));\n\
           uint64_t self; __asm__ volatile(\"movq %%fs:0, %0\" : \"=r\"(self));\n\
           printf(\"fsbase %d %d\\n\", rdfsbase64() == self, rdfsbase32() == (uint32_t)self);\n\
           uint64_t gs = rdgsbase64();\n\
           wrgsbase64(0x123456789abcull); uint64_t g1 = rdgsbase64();\n\
           wrgsbase32(0x87654321u); uint32_t g2 = rdgsbase32(); uint64_t g3 = rdgsbase64();\n\
           wrgsbase64(gs);\n\
           printf(\"gsbase %llx %x %llx %d\\n\", (unsigned long long)g1, g2, \
         (unsigned long long)g3, rdgsbase64() == gs);\n\
           (void)rdpid(); lines(a);\n\
           printf(\"rdpid lines\\n\");\n\
           const uint64_t fl[] = {0, 1, 4, 0x10, 0x40, 0x80, 0x800, 0x8d5, ~0ull};\n\
           for (unsigned k = 0; k < sizeof fl / sizeof *fl; k++)\n\
             printf(\"flags %llx %llx\\n\", (unsigned long long)fl[k], \
         (unsigned long long)flags(fl[k]));\n\
           void (*const rs[])(void *) = {rdrand16, rdrand32, rdrand64, rdseed16, rdseed32, \
         rdseed64};\n\
           for (unsigned k = 0; k < 6; k++) {\n\
             uint64_t v[4]; int ok = 1;\n\
             for (int i = 0; i < 4; i++) {\n\
               unsigned char o[16]; memset(o, 0xcc, 16); rs[k](o);\n\
               ok &= u32(o + 8) == 1; v[i] = 0;\n\
               memcpy(&v[i], o, k % 3 == 0 ? 2 : k % 3 == 1 ? 4 : 8);\n\
             }\n\
             printf(\"random %u %d %d\\n\", k, ok, !(v[0] == v[1] && v[1] == v[2] && \
         v[2] == v[3]));\n\
           }\n\
           return 0;\n\
         }\n",
    );
    c
}

/// §5.6.29: every system and state intrinsic hod runs agrees with `llc`
/// 22 and 19 (hod plain under bfd and lld, and stressed), on a CPU model
/// with the features QEMU implements: the deterministic ones byte for
/// byte; the state round trips (a restored image's control words are the
/// ones in force and saved again; the saved MXCSR is `stmxcsr`'s, the
/// saved control word `fnstcw`'s); the non-deterministic ones by shape
/// (the time-stamp counter does not go back, `rdrand`/`rdseed` deliver
/// with the carry set and not one value four times, `rdfsbase` is the
/// thread pointer, the `gs` base round trips).
#[test]
fn system_intrinsics_agree_with_llc() {
    let (det, names) = sys_deterministic();
    let body = format!(
        "{}{det}{}attributes #0 = {{ \"target-features\"=\"{SYS_FEATURES}\" }}\n",
        sys_declarations(),
        sys_runtime()
    );
    let text = format!("{}{body}", ll_header());
    let mut rng = Rng(0x0190);
    let per = 32;
    let mut inputs = Vec::new();
    for _ in 0..names.len() {
        for i in 0..per {
            let mut set = [0u64; 3];
            for (j, word) in set.iter_mut().enumerate() {
                *word = if i < SYS_EDGES.len() && j == 0 {
                    SYS_EDGES[i]
                } else if rng.below(3) == 0 {
                    rng.next()
                } else {
                    *rng.pick(&SYS_EDGES)
                };
            }
            inputs.push(set);
        }
    }

    let (module, bodies) = parse(&text);
    let scratch = Scratch::new("x64-x86-system");
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
    if let Some(lines) = disassembly(&body, Stress::default()) {
        let mnemonics: Vec<&str> = lines.iter().map(|l| mnemonic(l)).collect();
        for &(name, _, want) in SYS_COMPILED {
            assert!(mnemonics.contains(&want), "{name}: no {want} in hod's code");
        }
    }

    let Some(base) = Toolchain::x86_64(&scratch) else { return };
    let cpu = cpu_with_features(base.target.emulated_cpu(), SYS_CPU);
    let toolchain = base.with_emulated_cpu(&cpu);
    let driver = toolchain.compile_c(&scratch, "driver", &sys_driver(names.len(), &inputs), &[]);
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
    // The shapes, on the reference.
    for line in reference.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        match fields[0] {
            "state" => {
                // `xsaveopt` does not write a component in its initial
                // state: only the round trip.
                if fields[1].parse::<usize>().unwrap() < 4 {
                    assert_eq!(&fields[2..4], ["1", "1"], "{reference_leg}: {line}");
                }
                assert_eq!(&fields[4..6], ["0f7f", "00007f80"], "{reference_leg}: {line}");
                assert_eq!(&fields[6..8], ["0f7f", "00007f80"], "{reference_leg}: {line}");
            }
            "rdtsc" | "fsbase" => assert!(fields[1..].iter().all(|f| *f == "1"), "{line}"),
            "gsbase" => assert_eq!(line, "gsbase 123456789abc 87654321 87654321 1"),
            "random" => assert_eq!(&fields[2..], ["1", "1"], "{line}"),
            _ => {}
        }
    }
    for line in reference.lines().filter(|l| !l.starts_with("d ")) {
        eprintln!("{reference_leg}: {line}");
    }
    let cases = reference.lines().count();
    assert!(cases > names.len() * per, "{reference_leg}: {reference}");
    let mut wrong = Vec::new();
    for (leg, out) in &runs {
        assert_eq!(out.lines().count(), cases, "{leg}: one line per case");
        for (want, got) in reference.lines().zip(out.lines()) {
            if want != got {
                let what = match want.split(' ').collect::<Vec<_>>()[..] {
                    ["d", f, ..] => names[f.parse::<usize>().unwrap()],
                    _ => "",
                };
                wrong.push(format!("{leg}: {what}: want {want}, got {got}"));
            }
        }
    }
    eprintln!(
        "x86 system intrinsics: {} functions, {cases} lines; legs {:?}",
        names.len(),
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(40)].join("\n")
    );
}

/// Every intrinsic of §5.6.29: name, result type, parameter types, the
/// feature it needs (`""`: the baseline), the mnemonic of its instruction.
const SYS_ALL: &[(&str, &str, &[&str], &str, &str)] = &[
    ("flags.read.u64", "i64", &[], "", "pushfq"),
    ("flags.write.u64", "void", &["i64"], "", "popfq"),
    ("rdtsc", "i64", &[], "", "rdtsc"),
    ("rdtscp", "{ i64, i32 }", &[], "", "rdtscp"),
    ("rdpmc", "i64", &["i32"], "", "rdpmc"),
    ("addcarry.32", "{ i8, i32 }", &["i8", "i32", "i32"], "", "adc"),
    ("addcarry.64", "{ i8, i64 }", &["i8", "i64", "i64"], "", "adc"),
    ("subborrow.32", "{ i8, i32 }", &["i8", "i32", "i32"], "", "sbb"),
    ("subborrow.64", "{ i8, i64 }", &["i8", "i64", "i64"], "", "sbb"),
    ("fxsave", "void", &["ptr"], "fxsr", "fxsave"),
    ("fxsave64", "void", &["ptr"], "fxsr", "fxsave64"),
    ("fxrstor", "void", &["ptr"], "fxsr", "fxrstor"),
    ("fxrstor64", "void", &["ptr"], "fxsr", "fxrstor64"),
    ("xgetbv", "i64", &["i32"], "xsave", "xgetbv"),
    ("xsetbv", "void", &["i32", "i32", "i32"], "xsave", "xsetbv"),
    ("xsave", "void", &["ptr", "i32", "i32"], "xsave", "xsave"),
    ("xsave64", "void", &["ptr", "i32", "i32"], "xsave", "xsave64"),
    ("xrstor", "void", &["ptr", "i32", "i32"], "xsave", "xrstor"),
    ("xrstor64", "void", &["ptr", "i32", "i32"], "xsave", "xrstor64"),
    ("xsaveopt", "void", &["ptr", "i32", "i32"], "xsaveopt", "xsaveopt"),
    ("xsaveopt64", "void", &["ptr", "i32", "i32"], "xsaveopt", "xsaveopt64"),
    ("xsavec", "void", &["ptr", "i32", "i32"], "xsavec", "xsavec"),
    ("xsavec64", "void", &["ptr", "i32", "i32"], "xsavec", "xsavec64"),
    ("xsaves", "void", &["ptr", "i32", "i32"], "xsaves", "xsaves"),
    ("xsaves64", "void", &["ptr", "i32", "i32"], "xsaves", "xsaves64"),
    ("xrstors", "void", &["ptr", "i32", "i32"], "xsaves", "xrstors"),
    ("xrstors64", "void", &["ptr", "i32", "i32"], "xsaves", "xrstors64"),
    ("rdrand.16", "{ i16, i32 }", &[], "rdrnd", "rdrand"),
    ("rdrand.32", "{ i32, i32 }", &[], "rdrnd", "rdrand"),
    ("rdrand.64", "{ i64, i32 }", &[], "rdrnd", "rdrand"),
    ("rdseed.16", "{ i16, i32 }", &[], "rdseed", "rdseed"),
    ("rdseed.32", "{ i32, i32 }", &[], "rdseed", "rdseed"),
    ("rdseed.64", "{ i64, i32 }", &[], "rdseed", "rdseed"),
    ("clflushopt", "void", &["ptr"], "clflushopt", "clflushopt"),
    ("clwb", "void", &["ptr"], "clwb", "clwb"),
    ("bmi.bextr.32", "i32", &["i32", "i32"], "bmi", "bextr"),
    ("bmi.bextr.64", "i64", &["i64", "i64"], "bmi", "bextr"),
    ("bmi.bzhi.32", "i32", &["i32", "i32"], "bmi2", "bzhi"),
    ("bmi.bzhi.64", "i64", &["i64", "i64"], "bmi2", "bzhi"),
    ("bmi.pdep.32", "i32", &["i32", "i32"], "bmi2", "pdep"),
    ("bmi.pdep.64", "i64", &["i64", "i64"], "bmi2", "pdep"),
    ("bmi.pext.32", "i32", &["i32", "i32"], "bmi2", "pext"),
    ("bmi.pext.64", "i64", &["i64", "i64"], "bmi2", "pext"),
    ("sse42.crc32.32.8", "i32", &["i32", "i8"], "crc32", "crc32"),
    ("sse42.crc32.32.16", "i32", &["i32", "i16"], "crc32", "crc32"),
    ("sse42.crc32.32.32", "i32", &["i32", "i32"], "crc32", "crc32"),
    ("sse42.crc32.64.64", "i64", &["i64", "i64"], "crc32", "crc32"),
    ("rdfsbase.32", "i32", &[], "fsgsbase", "rdfsbase"),
    ("rdfsbase.64", "i64", &[], "fsgsbase", "rdfsbase"),
    ("rdgsbase.32", "i32", &[], "fsgsbase", "rdgsbase"),
    ("rdgsbase.64", "i64", &[], "fsgsbase", "rdgsbase"),
    ("wrfsbase.32", "void", &["i32"], "fsgsbase", "wrfsbase"),
    ("wrfsbase.64", "void", &["i64"], "fsgsbase", "wrfsbase"),
    ("wrgsbase.32", "void", &["i32"], "fsgsbase", "wrgsbase"),
    ("wrgsbase.64", "void", &["i64"], "fsgsbase", "wrgsbase"),
    ("xbegin", "i32", &[], "rtm", "xbegin"),
    ("xend", "void", &[], "rtm", "xend"),
    ("xabort", "void", &["i8"], "rtm", "xabort"),
    ("xtest", "i32", &[], "rtm", "xtest"),
    ("rdpid", "i32", &[], "rdpid", "rdpid"),
    ("rdpkru", "i32", &[], "pku", "rdpkru"),
    ("wrpkru", "void", &["i32"], "pku", "wrpkru"),
    ("serialize", "void", &[], "serialize", "serialize"),
    ("incsspd", "void", &["i32"], "shstk", "incsspd"),
    ("incsspq", "void", &["i64"], "shstk", "incsspq"),
    ("rdsspd", "i32", &["i32"], "shstk", "rdsspd"),
    ("rdsspq", "i64", &["i64"], "shstk", "rdsspq"),
];

/// A function calling `llvm.x86.<name>` on its parameters (`xabort`'s
/// operand the constant 7), under `features` if any.
fn sys_module(name: &str, ret: &str, params: &[&str], features: Option<&str>) -> String {
    let args: Vec<String> = params
        .iter()
        .map(|p| match *p {
            "ptr" => "ptr %p".into(),
            "i8" if name == "xabort" => "i8 7".into(),
            ty => format!("{ty} %{}", &ty[1..].replace(' ', "")).replace('%', "%v"),
        })
        .collect();
    let (mark, attrs) = match features {
        Some(f) => (" #0", format!("attributes #0 = {{ \"target-features\"=\"{f}\" }}\n")),
        None => ("", String::new()),
    };
    let call = if ret == "void" {
        format!("call void @llvm.x86.{name}({})", args.join(", "))
    } else {
        format!("%r = call {ret} @llvm.x86.{name}({})", args.join(", "))
    };
    let decl_params: Vec<&str> =
        params.iter().map(|p| if name == "xabort" { "i8 immarg" } else { p }).collect();
    format!(
        "declare {ret} @llvm.x86.{name}({})\n\
         define void @f(ptr %p, i8 %v8, i16 %v16, i32 %v32, i64 %v64){mark} {{\n  {call}\n  \
         ret void\n}}\n{attrs}",
        decl_params.join(", ")
    )
}

/// §5.6.29 (2): an intrinsic above the baseline in a function without its
/// feature is refused by its own name; with it (or a feature implying it)
/// it builds, and hod's code holds its instruction and no other of
/// §5.6.29's; a later item turning it (or what it needs) off refuses it
/// again. The baseline ones build without any feature.
#[test]
fn system_intrinsics_only_with_their_feature() {
    let all: Vec<&str> =
        SYS_ALL.iter().map(|r| r.4).filter(|m| !matches!(*m, "adc" | "sbb")).collect();
    for &(name, ret, params, feature, mnemonic_of) in SYS_ALL {
        let module = |features: Option<&str>| sys_module(name, ret, params, features);
        let refusal = format!("intrinsic:llvm.x86.{name}");
        let refused = |text: String| match compile(&text) {
            Err(CgError::Unsupported { construct }) => assert_eq!(construct, refusal, "{text}"),
            other => panic!("{name}: {other:?}\n{text}"),
        };
        let built = |text: String| {
            compile(&text).unwrap_or_else(|e| panic!("{name}: {e:?}\n{text}"));
            if let Some(lines) = disassembly(&text, Stress::default()) {
                let mnemonics: Vec<&str> = lines.iter().map(|l| mnemonic(l)).collect();
                assert!(mnemonics.contains(&mnemonic_of), "{name}: {lines:?}");
                for m in &mnemonics {
                    assert!(*m == mnemonic_of || !all.contains(m), "{name}: wrote {m}");
                }
            }
        };
        if feature.is_empty() {
            built(module(None));
            continue;
        }
        refused(module(None));
        refused(module(Some("+cx8,+mmx,+sse,+sse2,+x87")));
        built(module(Some(&format!("+sse,+sse2,+{feature}"))));
        refused(module(Some(&format!("+{feature},-{feature}"))));
        match feature {
            "xsave" => {
                for implying in ["xsaveopt", "xsavec", "xsaves"] {
                    built(module(Some(&format!("+{implying}"))));
                }
            }
            "xsaveopt" | "xsavec" | "xsaves" => {
                refused(module(Some(&format!("+{feature},-xsave"))));
                refused(module(Some("+xsave")));
            }
            "crc32" => {
                built(module(Some("+sse4.2")));
                built(module(Some("+avx2")));
                refused(module(Some("+sse4.1")));
                refused(module(Some("+sse4.2,-crc32")));
            }
            "bmi2" => refused(module(Some("+bmi"))),
            _ => {}
        }
    }
}

/// §5.6.29: every intrinsic of it is an x86-64 row and refused on
/// AArch64; a name of the headers hod does not build has no row, and
/// `classify` refuses it by that name.
#[test]
fn system_rows_and_refusals_by_name() {
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for &(name, ..) in SYS_ALL {
        let row = format!("intrinsic:llvm.x86.{name}");
        assert_eq!(x64.supports(&row), Some(true), "{row}");
        assert_eq!(a64.supports(&row), Some(false), "{row}");
    }
    for name in ["wbinvd", "clzero", "rdpru", "senduipi", "tbm.bextri.u32", "wrssd"] {
        let row = format!("intrinsic:llvm.x86.{name}");
        assert_eq!(x64.supports(&row), None, "{row}");
    }
    let unknown = "declare void @llvm.x86.wbinvd()\ndefine void @f() {\n  \
                   call void @llvm.x86.wbinvd()\n  ret void\n}\n";
    let (m, bodies) = parse(&format!("{}{unknown}", ll_header()));
    let refused = hod_ir::Classifier::new(x64).body(&m, &bodies[0]).expect_err("unknown");
    assert_eq!(format!("{}", refused.construct), "intrinsic:llvm.x86.wbinvd");
    // A signature LLVM does not declare the intrinsic with.
    let wrong = "declare i32 @llvm.x86.rdtsc()\ndefine i32 @f() {\n  \
                 %r = call i32 @llvm.x86.rdtsc()\n  ret i32 %r\n}\n";
    assert_eq!(compile(wrong), Err(CgError::unsupported(hod_x64::x86::SIGNATURE)));
}
