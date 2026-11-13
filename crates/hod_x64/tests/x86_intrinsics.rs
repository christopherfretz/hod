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
        let a = params.first().copied().unwrap_or("i32");
        let b = params.get(1).copied().filter(|p| p.starts_with('<')).unwrap_or(a);
        let mut variants: Vec<(String, Vec<String>)> = Vec::new();
        let vector_count =
            (name.contains(".psll.") || name.contains(".psrl.")) || name.contains(".psra.");
        match params {
            [] => variants.push((String::new(), Vec::new())),
            ["ptr"] => variants
                .push(("  %q = getelementptr i8, ptr %in, i64 1\n".into(), vec!["ptr %q".into()])),
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
                 %c = load i32, ptr %pc, align 4\n{prelude}{result}{store}  ret void\n}}\n"
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
    let unknown = "declare void @llvm.x86.sse.ldmxcsr(ptr)\ndefine void @f(ptr %p) {\n  \
                   call void @llvm.x86.sse.ldmxcsr(ptr %p)\n  ret void\n}\n";
    let (m, bodies) = parse(&format!("{}{unknown}", ll_header()));
    // No row on either target: `classify` names it whatever the target.
    assert_eq!(x64.supports("intrinsic:llvm.x86.sse.ldmxcsr"), None);
    assert_eq!(a64.supports("intrinsic:llvm.x86.sse.ldmxcsr"), None);
    {
        let classifier = hod_ir::Classifier::new(x64);
        let refused = classifier.body(&m, &bodies[0]).expect_err("an unknown intrinsic");
        assert_eq!(format!("{}", refused.construct), "intrinsic:llvm.x86.sse.ldmxcsr");
    }
    // A signature LLVM does not declare the intrinsic with.
    let wrong = "declare i32 @llvm.x86.sse2.pmovmskb.128(<8 x i8>)\ndefine i32 @f(<8 x i8> %v) {\n  \
                 %r = call i32 @llvm.x86.sse2.pmovmskb.128(<8 x i8> %v)\n  ret i32 %r\n}\n";
    assert_eq!(compile(wrong), Err(CgError::unsupported(hod_x64::x86::SIGNATURE)));
}
