//! AVX-512 (WP-208, `spec/codegen.md` §5.6.35, design
//! `docs/design/0007-avx512.md`).
//!
//! **The masked memory intrinsics** (`llvm.masked.load`, `.store`,
//! `.expandload`, `.compressstore`), which hod builds lane by lane
//! whatever the features: generated functions over every vector shape
//! the AVX-512 headers use, with a variable mask and with constant ones,
//! compiled by hod (plain, under every stress option, and from LLVM 22's
//! spelling of the signatures) and by `llc -O0 -fast-isel=0` of LLVM 22
//! and 19 (without features, where it scalarizes, and with AVX2, where it
//! uses `vmaskmov` for the 4- and 8-byte lanes), linked with one C driver
//! and run on the emulator: every leg prints the same bytes. The memory
//! a call reads or writes ends at a page the driver has made
//! inaccessible, so a lane whose mask bit is clear and that touched
//! memory would fault.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
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

/// The vector shapes of the masked loads and stores of the AVX-512
/// headers (F, VL, BW, FP16), and a few others: element type, lanes,
/// element bytes.
const SHAPES: &[(&str, u32, u32)] = &[
    ("float", 16, 4),
    ("double", 8, 8),
    ("i32", 16, 4),
    ("i64", 8, 8),
    ("i16", 32, 2),
    ("i8", 64, 1),
    ("half", 32, 2),
    ("float", 8, 4),
    ("double", 4, 8),
    ("i32", 8, 4),
    ("i64", 4, 8),
    ("i16", 16, 2),
    ("i8", 32, 1),
    ("float", 4, 4),
    ("double", 2, 8),
    ("i32", 4, 4),
    ("i64", 2, 8),
    ("i16", 8, 2),
    ("i8", 16, 1),
    ("i32", 3, 4),
    ("float", 2, 4),
];

/// The four intrinsics: name, whether it loads, whether it packs the set
/// lanes (expand and compress).
const OPS: &[(&str, bool, bool)] = &[
    ("load", true, false),
    ("store", false, false),
    ("expandload", true, true),
    ("compressstore", false, true),
];

/// One generated function `void tK(ptr %mem, ptr %vec, i64 %bits)`: a
/// load reads `%mem` under the mask into the vector at `%vec` (whose
/// bytes are the pass-through), a store writes the vector at `%vec` to
/// `%mem`.
struct Func {
    op: usize,
    shape: usize,
    /// A constant mask, or `None` for the low lanes of `%bits`.
    fixed: Option<u64>,
}

/// The LLVM mangling of a vector of `lanes` `elem`.
fn mangle(elem: &str, lanes: u32) -> String {
    let e = match elem {
        "float" => "f32",
        "double" => "f64",
        "half" => "f16",
        other => other,
    };
    format!("v{lanes}{e}")
}

/// The constant `<N x i1>` of `bits`.
fn mask_const(lanes: u32, bits: u64) -> String {
    let items: Vec<String> =
        (0..lanes).map(|i| format!("i1 {}", u8::from(bits >> i & 1 == 1))).collect();
    format!("<{}>", items.join(", "))
}

/// The module text of `funcs`: LLVM 19's signatures (`new` false, which
/// LLVM 22 also reads, upgrading them) or LLVM 22's (`new` true), every
/// function with `attrs` (`""` for none).
fn module(funcs: &[Func], new: bool, attrs: &str) -> String {
    let mut text = ll_header();
    let mut declared = std::collections::BTreeSet::new();
    for (k, f) in funcs.iter().enumerate() {
        let (op, load, packed) = OPS[f.op];
        let (elem, lanes, _) = SHAPES[f.shape];
        let vty = format!("<{lanes} x {elem}>");
        let mty = format!("<{lanes} x i1>");
        let suffix =
            if packed { mangle(elem, lanes) } else { format!("{}.p0", mangle(elem, lanes)) };
        let name = format!("llvm.masked.{op}.{suffix}");
        let align = if new { "" } else { "i32, " };
        if declared.insert(name.clone()) {
            let decl = match (load, packed) {
                (true, false) => format!("{vty} @{name}(ptr, {align}{mty}, {vty})"),
                (false, false) => format!("void @{name}({vty}, ptr, {align}{mty})"),
                (true, true) => format!("{vty} @{name}(ptr, {mty}, {vty})"),
                (false, true) => format!("void @{name}({vty}, ptr, {mty})"),
            };
            writeln!(text, "declare {decl}").unwrap();
        }
        let mark = if attrs.is_empty() { "" } else { " #0" };
        writeln!(text, "define void @t{k}(ptr %mem, ptr %vec, i64 %bits){mark} {{").unwrap();
        let m = if let Some(bits) = f.fixed {
            mask_const(lanes, bits)
        } else {
            if lanes == 64 {
                writeln!(text, "  %m = bitcast i64 %bits to {mty}").unwrap();
            } else {
                writeln!(text, "  %b = trunc i64 %bits to i{lanes}").unwrap();
                writeln!(text, "  %m = bitcast i{lanes} %b to {mty}").unwrap();
            }
            "%m".to_string()
        };
        writeln!(text, "  %v = load {vty}, ptr %vec, align 1").unwrap();
        let (ptr, align) = if new { ("ptr align 1 %mem", "") } else { ("ptr %mem", "i32 1, ") };
        let call = match (load, packed) {
            (true, false) => format!("%r = call {vty} @{name}({ptr}, {align}{mty} {m}, {vty} %v)"),
            (false, false) => format!("call void @{name}({vty} %v, {ptr}, {align}{mty} {m})"),
            (true, true) => format!("%r = call {vty} @{name}(ptr %mem, {mty} {m}, {vty} %v)"),
            (false, true) => format!("call void @{name}({vty} %v, ptr %mem, {mty} {m})"),
        };
        writeln!(text, "  {call}").unwrap();
        if load {
            writeln!(text, "  store {vty} %r, ptr %vec, align 1").unwrap();
        }
        text.push_str("  ret void\n}\n");
    }
    if !attrs.is_empty() {
        writeln!(text, "attributes #0 = {{ \"target-features\"=\"{attrs}\" }}").unwrap();
    }
    text
}

/// Every function: each intrinsic on each shape with a variable mask, and
/// with constant masks (none, all, alternating, the top lane) on some.
fn functions() -> Vec<Func> {
    let mut funcs = Vec::new();
    for op in 0..OPS.len() {
        for shape in 0..SHAPES.len() {
            funcs.push(Func { op, shape, fixed: None });
        }
        for shape in [0, 5, 13, 19] {
            let lanes = SHAPES[shape].1;
            let all = if lanes == 64 { u64::MAX } else { (1 << lanes) - 1 };
            for fixed in [0, all, all & 0x5555_5555_5555_5555, 1 << (lanes - 1)] {
                funcs.push(Func { op, shape, fixed: Some(fixed) });
            }
        }
    }
    funcs
}

/// The C driver: for each function and case, the mask, the memory
/// bytes and the vector bytes from the tables; the memory a call may
/// touch placed right below an inaccessible page. Prints the vector
/// after a load, and the memory (with the 8 bytes below it) after a
/// store.
fn driver(funcs: &[Func], cases: &[Vec<(u64, Vec<u8>)>]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n#include <sys/mman.h>\n",
    );
    for k in 0..funcs.len() {
        writeln!(c, "void t{k}(void *, void *, uint64_t);").unwrap();
    }
    c.push_str("typedef void (*fn)(void *, void *, uint64_t);\nstatic const fn fns[] = {");
    for k in 0..funcs.len() {
        write!(c, "t{k},").unwrap();
    }
    c.push_str("};\n/* load, packed, element bytes, lanes */\nstatic const int kind[][4] = {");
    for f in funcs {
        let (_, load, packed) = OPS[f.op];
        let (_, lanes, bytes) = SHAPES[f.shape];
        write!(c, "{{{},{},{bytes},{lanes}}},", u8::from(load), u8::from(packed)).unwrap();
    }
    let per = cases[0].len();
    c.push_str("};\nstatic const uint64_t bits[] = {");
    for (_, set) in funcs.iter().zip(cases) {
        for (bits, _) in set {
            write!(c, "{bits}u,").unwrap();
        }
    }
    c.push_str("};\nstatic const unsigned char data[][1024] = {");
    for set in cases {
        for (_, bytes) in set {
            c.push('{');
            for b in bytes {
                write!(c, "{b},").unwrap();
            }
            c.push_str("},\n");
        }
    }
    writeln!(c, "}};\nstatic const int per = {per};").unwrap();
    c.push_str(
        "int main(void) {\n  \
         unsigned char *map = mmap(0, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n  \
         if (map == MAP_FAILED) return 2;\n\
         #ifndef NO_GUARD\n  \
         if (mprotect(map + 4096, 4096, PROT_NONE)) return 2;\n\
         #endif\n  \
         unsigned char *guard = map + 4096;\n  \
         setvbuf(stdout, 0, _IOLBF, 0);\n  \
         for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++)\n    \
         for (int i = 0; i < per; i++) {\n      \
         const int load = kind[f][0], packed = kind[f][1], size = kind[f][2], lanes = kind[f][3];\n      \
         uint64_t m = bits[f * per + i];\n      \
         if (lanes < 64) m &= (1ull << lanes) - 1;\n      \
         int span = 0;\n      \
         for (int l = 0; l < lanes; l++)\n        \
         if (m >> l & 1) span = packed ? span + size : (l + 1) * size;\n      \
         const unsigned char *d = data[f * per + i];\n      \
         unsigned char *mem = guard - span;\n      \
         memcpy(map + 4096 - 600, d, 600);\n      \
         unsigned char vec[512];\n      \
         memcpy(vec, d + 512, 512);\n      \
         fns[f](mem, vec, bits[f * per + i]);\n      \
         printf(\"%u %d \", f, i);\n      \
         if (load) for (int k = 0; k < lanes * size; k++) printf(\"%02x\", vec[k]);\n      \
         else for (int k = -8; k < span; k++) printf(\"%02x\", mem[k]);\n      \
         printf(\"\\n\");\n    }\n  \
         return 0;\n}\n",
    );
    c
}

/// §5.6.35: the masked memory intrinsics hod builds lane by lane compute
/// what `llc`'s code computes, and touch no memory of a lane whose bit is
/// clear.
#[test]
fn masked_memory_intrinsics_agree_with_llc() {
    let funcs = functions();
    let mut rng = Rng(0x0208);
    let cases: Vec<Vec<(u64, Vec<u8>)>> = funcs
        .iter()
        .map(|f| {
            (0..8)
                .map(|i| {
                    // A constant mask's function ignores the bits; the
                    // driver places the memory by them.
                    let bits = match (f.fixed, i) {
                        (Some(fixed), _) => fixed,
                        (None, 0) => 0,
                        (None, 1) => u64::MAX,
                        _ => rng.next(),
                    };
                    let bytes = (0..1024).map(|_| rng.next() as u8).collect();
                    (bits, bytes)
                })
                .collect()
        })
        .collect();
    let lines: usize = cases.iter().map(Vec::len).sum();

    let old = module(&funcs, false, "");
    let new = module(&funcs, true, "");
    let avx2 = module(&funcs, false, "+avx,+avx2");
    let scratch = Scratch::new("x64-avx512-masked");
    let mut legs = Vec::new();
    for (mode, stress, text) in [
        ("plain", Stress::default(), &old),
        ("stress", Stress::ALL, &old),
        ("llvm22-form", Stress::default(), &new),
        ("avx2", Stress::default(), &avx2),
    ] {
        let (module, bodies) = parse(text);
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("hod {mode}: {e}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    for major in [22, 19] {
        for (mode, text) in [("plain", &old), ("avx2", &avx2)] {
            if let Some(object) = llc(&scratch, major, &format!("llc{major}-{mode}"), text) {
                legs.push((format!("llc {major} {mode}"), object));
            }
        }
    }
    if let Some(object) = llc(&scratch, 22, "llc22-llvm22-form", &new) {
        legs.push(("llc 22 llvm22-form".to_string(), object));
    }
    eprintln!("masked: {} functions, {lines} cases, {} legs", funcs.len(), legs.len());

    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let source = driver(&funcs, &cases);
    let guarded = toolchain.compile_c(&scratch, "driver", &source, &[]);
    // The AVX2 `llc` legs fault under QEMU on a `vmaskmov` whose
    // masked-off lanes reach the inaccessible page (measured; the
    // hardware suppresses that fault): they run without it.
    let unguarded = toolchain.compile_c(&scratch, "driver-unguarded", &source, &["-DNO_GUARD"]);
    let mut runs: Vec<(String, String)> = Vec::new();
    let mut crashed = Vec::new();
    for (leg, object) in &legs {
        let linkers: Vec<_> = if leg == "hod plain" {
            toolchain.linkers.iter().filter(|l| matches!(l.name, "bfd" | "lld")).collect()
        } else {
            toolchain.linkers.iter().take(1).collect()
        };
        let driver =
            if leg.starts_with("llc") && leg.ends_with("avx2") { &unguarded } else { &guarded };
        for linker in linkers {
            let leg = format!("{leg} ({})", linker.name);
            let program = scratch.join(leg.replace([' ', '(', ')'], "-"));
            let ran = toolchain.link_and_run(linker, &[], &[driver, object], &program);
            let out = ran.stdout_text();
            if !ran.success() {
                crashed.push(format!(
                    "{leg}: {:?} after `{}`\n{}",
                    ran.status,
                    out.lines().last().unwrap_or("").get(..12).unwrap_or(""),
                    ran.stderr_text()
                ));
            }
            runs.push((leg, out));
        }
    }
    assert!(crashed.is_empty(), "{}", crashed.join("\n"));
    let (reference_leg, reference) =
        &runs[runs.iter().position(|r| r.0.starts_with("llc 22")).unwrap_or(0)];
    assert_eq!(reference.lines().count(), lines, "{reference_leg}: one line per case");
    let mut wrong = Vec::new();
    for (leg, out) in &runs {
        assert_eq!(out.lines().count(), lines, "{leg}: one line per case");
        for (want, got) in reference.lines().zip(out.lines()) {
            if want != got {
                wrong.push(format!("{leg}: want {want}\n  got {got}"));
            }
        }
    }
    eprintln!(
        "masked: legs {:?} agree on {lines} cases",
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(20)].join("\n")
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

/// §5.6.35: a mask of more than 64 lanes is refused as `x64:mask-wide`;
/// the rows are x86-64 ones, not built on AArch64.
#[test]
fn masked_refusals_and_rows() {
    let wide = "declare <128 x i8> @llvm.masked.load.v128i8.p0(ptr, i32, <128 x i1>, <128 x i8>)\n\
                define void @f(ptr %p, ptr %q) {\n  %m = load <128 x i1>, ptr %q, align 1\n  \
                %v = load <128 x i8>, ptr %q, align 1\n  \
                %r = call <128 x i8> @llvm.masked.load.v128i8.p0(ptr %p, i32 1, <128 x i1> %m, <128 x i8> %v)\n  \
                store <128 x i8> %r, ptr %q, align 1\n  ret void\n}\n";
    assert_eq!(compile(wide), Err(CgError::unsupported("x64:mask-wide")));
    let x64 = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let a64 = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for name in [
        "intrinsic:llvm.masked.load",
        "intrinsic:llvm.masked.store",
        "intrinsic:llvm.masked.expandload",
        "intrinsic:llvm.masked.compressstore",
    ] {
        assert_eq!(x64.supports(name), Some(true), "{name}");
        assert_eq!(a64.supports(name), Some(false), "{name}");
    }
}
