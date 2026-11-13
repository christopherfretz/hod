//! The x86-64 ABI follow-ups (WP-140, `spec/codegen.md` §5.6.9,
//! `spec/abi-x64.md` §1.2, §1.3, §5.1):
//!
//! - the function's features decide its signatures: wide vectors and
//!   vectors of `i1` under SSE only, placed as `llc` places them (hod's
//!   object mixed with `llc`'s, both directions, under the emulated
//!   runner); the AVX/AVX-512 refusals; the CPU tables checked against
//!   `llc` 19 and 22;
//! - `-x87` refused only for a function with an `x86_fp80` value;
//! - the support-matrix rows no instruction group owned: `sret`, `byval`,
//!   `fastcc`, `insertvalue`/`extractvalue` of integer and pointer
//!   members, as a two-module program checked against `hod_interp`, with
//!   each module from hod or from `llc` (all four pairings), linked by
//!   bfd, lld and mold.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use aggr_gen::{PIE_FLAGS, header, llc, llc_of, parse, pic, text_bytes};
use hod_cg::Target as _;
use hod_cg::abi::{AbiUnsupported, Assignment, Place, Sig};
use hod_cg::{CgError, Stress};
use hod_interp::{Interp, Program, Val};
use hod_ir::{SymbolKind, TypeKind};
use hod_testutil::{Scratch, run_with};
use hod_x64::{X64, abi, features, reg};
use linking::Toolchain;

/// The object hod compiles from `.ll` text (header included).
fn hod(text: &str) -> Result<Vec<u8>, hod_x64::ModuleError> {
    let (module, bodies) = parse(text);
    hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default()))
}

/// The refusal of a module, or `""` when hod compiles it.
fn refusal(text: &str) -> &'static str {
    match hod(text) {
        Ok(_) => "",
        Err(hod_x64::ModuleError::Function {
            error: CgError::Unsupported { construct }, ..
        }) => construct,
        Err(other) => panic!("not a refusal: {other:?}"),
    }
}

/// A module of `body` with the function attribute group `#0` = `attrs`.
fn module(body: &str, attrs: &str) -> String {
    format!("{}{body}\nattributes #0 = {{ {attrs} }}\n{PIE_FLAGS}", header())
}

/// Writes hod's object of `text` as `<name>.o`.
fn hod_object(scratch: &Scratch, name: &str, text: &str) -> PathBuf {
    let bytes = hod(text).unwrap_or_else(|e| panic!("hod refused {name}: {e:?}\n{text}"));
    scratch.write(&format!("{name}.o"), bytes)
}

/// Links `objects` with each linker and runs the program; returns its
/// standard output, the same for every linker.
fn link_and_run(toolchain: &Toolchain, scratch: &Scratch, tag: &str, objects: &[&Path]) -> String {
    let mut seen: Option<String> = None;
    for linker in &toolchain.linkers {
        let exe = scratch.join(format!("{tag}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &["-pie"], objects, &exe);
        assert!(
            ran.success(),
            "{tag} ({}): {}{}",
            linker.name,
            ran.stdout_text(),
            ran.stderr_text()
        );
        let out = ran.stdout_text();
        if let Some(seen) = &seen {
            assert_eq!(&out, seen, "{tag}: {} differs", linker.name);
        }
        seen = Some(out);
    }
    seen.unwrap_or_default()
}

// --- The function's features -----------------------------------------------

/// The functions of the vector program: hod's side. `@h_*` are compiled
/// by hod, `@l_*` by `llc`; each `@h_*` calls an `@l_*` so that both
/// directions cross the boundary.
const VEC_HOD: &str = "
declare <8 x i32> @l_add8(<8 x i32>)
declare <4 x i64> @l_rot(<4 x i64>, i64)

define <8 x i32> @h_add8(<8 x i32> %v) #0 {
  %r = call <8 x i32> @l_add8(<8 x i32> %v)
  ret <8 x i32> %r
}
define <4 x i64> @h_rot(<4 x i64> %v, i64 %k) #0 {
  %r = call <4 x i64> @l_rot(<4 x i64> %v, i64 %k)
  ret <4 x i64> %r
}
";

/// `llc`'s side: the callees, and the drivers C calls (through memory and
/// integers, which no feature changes).
const VEC_LLC: &str = "
declare <8 x i32> @h_add8(<8 x i32>)
declare <4 x i64> @h_rot(<4 x i64>, i64)

define <8 x i32> @l_add8(<8 x i32> %v) #0 {
  %r = add <8 x i32> %v, <i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8>
  ret <8 x i32> %r
}
define <4 x i64> @l_rot(<4 x i64> %v, i64 %k) #0 {
  %s = shufflevector <4 x i64> %v, <4 x i64> poison, <4 x i32> <i32 1, i32 2, i32 3, i32 0>
  %i = insertelement <4 x i64> poison, i64 %k, i32 0
  %j = shufflevector <4 x i64> %i, <4 x i64> poison, <4 x i32> zeroinitializer
  %r = xor <4 x i64> %s, %j
  ret <4 x i64> %r
}

define void @d_add8(ptr %in, ptr %out) #0 {
  %v = load <8 x i32>, ptr %in
  %r = call <8 x i32> @h_add8(<8 x i32> %v)
  store <8 x i32> %r, ptr %out
  ret void
}
define void @d_rot(ptr %in, i64 %k, ptr %out) #0 {
  %v = load <4 x i64>, ptr %in
  %r = call <4 x i64> @h_rot(<4 x i64> %v, i64 %k)
  store <4 x i64> %r, ptr %out
  ret void
}
";

/// The C driver of the vector program: prints what came back.
const VEC_MAIN: &str = r#"
#include <stdio.h>
#include <stdint.h>
void d_add8(const int32_t *in, int32_t *out);
void d_rot(const int64_t *in, int64_t k, int64_t *out);
int main(void) {
    int32_t in8[8] = {10, -20, 30, -40, 50, -60, 70, 2147483647}, out8[8];
    d_add8(in8, out8);
    for (int i = 0; i < 8; i++) printf("%d ", out8[i]);
    printf("\n");
    int64_t in4[4] = {1, -2, 0x123456789abcdefLL, -0x7fffffffffffffffLL}, out4[4];
    d_rot(in4, 0x5555, out4);
    for (int i = 0; i < 4; i++) printf("%lld ", (long long)out4[i]);
    printf("\n");
    return 0;
}
"#;

/// What [`VEC_MAIN`] prints, worked out by hand.
fn vec_expected() -> String {
    let in8: [i32; 8] = [10, -20, 30, -40, 50, -60, 70, i32::MAX];
    let mut out = String::new();
    for (i, v) in in8.iter().enumerate() {
        write!(out, "{} ", v.wrapping_add(i32::try_from(i).unwrap() + 1)).unwrap();
    }
    out += "\n";
    let in4: [i64; 4] = [1, -2, 0x0123_4567_89ab_cdef, -0x7fff_ffff_ffff_ffff];
    for i in 0..4 {
        write!(out, "{} ", in4[(i + 1) % 4] ^ 0x5555).unwrap();
    }
    out + "\n"
}

/// Function attributes under which a function has neither AVX nor
/// AVX-512 (`spec/abi-x64.md` §1.3), as a front end writes them.
const WITHOUT_AVX: [&str; 5] = [
    "nounwind",
    "\"target-cpu\"=\"x86-64\"",
    "\"target-cpu\"=\"x86-64\" \"target-features\"=\"+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87\"",
    "\"target-cpu\"=\"nehalem\" \"target-features\"=\"+sse4.2,+popcnt\"",
    "\"target-cpu\"=\"haswell\" \"target-features\"=\"-avx\"",
];

/// §1.3: without AVX, `<8 x i32>` and `<4 x i64>` are two `xmm` registers
/// each and `<4 x i1>`/`<16 x i1>` one `xmm`, exactly where `llc` puts
/// them: hod's functions call `llc`'s and are called by `llc`'s, under
/// each spelling of "no AVX" (the haswell one only reaches hod: `llc`'s
/// side keeps `x86-64`, which is the same convention, since the emulated
/// CPU has no AVX).
///
/// Vectors of `i1` are not in the program: a `Lanes` part is refused as
/// `x64:todo:vcall` until WP-128 (`crates/hod_x64/src/vcall.rs:20`); their
/// placement is `own_signature_takes_the_function_features`' and the abi
/// grid's.
#[test]
fn wide_vectors_follow_the_function() {
    let scratch = Scratch::new("x64-wp140-vec");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let main = toolchain.compile_c(&scratch, "vec-main", VEC_MAIN, &["-fPIE"]);
    let expected = vec_expected();
    for major in [19, 22] {
        let llc_side = module(VEC_LLC, "nounwind \"target-cpu\"=\"x86-64\"");
        let Some(callees) = llc(&scratch, major, &format!("vec-llc{major}"), &llc_side) else {
            continue;
        };
        // `llc` alone first: the program means what `vec_expected` says.
        let hod_side = module(VEC_HOD, "nounwind \"target-cpu\"=\"x86-64\"");
        let Some(llc_hod_side) = llc(&scratch, major, &format!("vec-h-llc{major}"), &hod_side)
        else {
            continue;
        };
        let out = link_and_run(&toolchain, &scratch, "vec-llc", &[&main, &callees, &llc_hod_side]);
        assert_eq!(out, expected, "llc {major} alone");
        for (n, attrs) in WITHOUT_AVX.iter().enumerate() {
            let object = hod_object(&scratch, &format!("vec-hod{n}"), &module(VEC_HOD, attrs));
            let out = link_and_run(
                &toolchain,
                &scratch,
                &format!("vec{major}-{n}"),
                &[&main, &callees, &object],
            );
            assert_eq!(out, expected, "hod with {attrs} against llc {major}");
        }
    }
}

/// The assignment `X64::for_function` gives the signature of `callee` (a
/// function of the module) inside the first function of `text`.
fn assignment_in(text: &str, callee: &[u8]) -> Result<Assignment, &'static str> {
    let (module, bodies) = parse(text);
    let target = X64.for_function(&module, &bodies[0]).expect("x86-64 reads the features");
    let sym = module.find_symbol(callee).expect("the callee");
    let function = module.symbol(sym);
    let SymbolKind::Function { cc, ret_attrs, param_attrs, .. } = function.kind() else {
        panic!("no function");
    };
    let TypeKind::Function { ret, params, variadic } = module.types.kind(function.ty()) else {
        panic!("no function type");
    };
    let fixed = u32::try_from(params.len()).unwrap();
    let sig = Sig { cc, ret, ret_attrs, params, param_attrs, fixed, variadic };
    let mut out = Assignment::default();
    target.assign(&module.types, &sig, &mut out).map_err(|AbiUnsupported(name)| name)?;
    // Without AVX the parts are `abi::assign`'s without features, which
    // the grid checks against `llc` (`spec/abi-x64.md` §11.1).
    let mut plain = Assignment::default();
    abi::assign(&module.types, &sig, abi::Features::default(), &mut plain).unwrap();
    assert_eq!(out, plain, "{text}");
    Ok(out)
}

/// §1.3: a function without AVX places its own wide vectors and vectors of
/// `i1` in `xmm` registers, as `llc` does; with AVX the wide ones are
/// refused, with AVX-512 the `i1` ones too. A call's signature is assigned
/// with the caller's features, whatever the callee declares.
#[test]
fn own_signature_takes_the_function_features() {
    let wide = "define <8 x i32> @f(<8 x i32> %v, <4 x i64> %w) #0 {\n  ret <8 x i32> %v\n}\n";
    let mask = "define <16 x i1> @f(<4 x i1> %v, <16 x i1> %w) #0 {\n  ret <16 x i1> %w\n}\n";
    let call = "declare <4 x i64> @g(<4 x i64>) #1\n\
        define void @f() #0 {\n  ret void\n}\n\
        attributes #1 = { \"target-cpu\"=\"skylake-avx512\" }\n";
    let wide_vector = abi::WIDE_VECTOR_ABI;
    for attrs in WITHOUT_AVX {
        for body in [wide, mask] {
            let asg = assignment_in(&module(body, attrs), b"f").expect(attrs);
            assert!(
                asg.args.iter().all(|part| matches!(part.place, Place::Reg(r) if reg::is_fpr(r)))
            );
            assert!(
                asg.ret.iter().all(|part| matches!(part.place, Place::Reg(r) if reg::is_fpr(r)))
            );
        }
        assert!(assignment_in(&module(call, attrs), b"g").is_ok(), "{attrs}");
    }
    for attrs in ["\"target-features\"=\"+avx\"", "\"target-cpu\"=\"haswell\""] {
        assert_eq!(assignment_in(&module(wide, attrs), b"f"), Err(wide_vector));
        assert_eq!(assignment_in(&module(call, attrs), b"g"), Err(wide_vector));
        assert!(assignment_in(&module(mask, attrs), b"f").is_ok());
    }
    let avx512 = "\"target-cpu\"=\"skylake-avx512\"";
    assert_eq!(assignment_in(&module(mask, avx512), b"f"), Err(wide_vector));
}

/// §1.3 through the whole compiler: with AVX (or a CPU that may have it) a
/// signature with a vector wider than 128 bits is refused; with AVX-512
/// also one with a vector of `i1`; a 128-bit vector is fine either way.
/// The caller's features decide, whatever the callee declares. Without
/// AVX, a wide vector (a `Mem` value) compiles, and a vector of `i1`
/// reaches WP-128's refusal (`x64:todo:vcall`), past the convention.
#[test]
fn avx_signatures_are_refused_by_the_function() {
    let wide = "define <4 x i64> @f(<4 x i64> %v) #0 {\n  ret <4 x i64> %v\n}\n";
    let mask = "define <4 x i1> @f(<4 x i1> %v) #0 {\n  ret <4 x i1> %v\n}\n";
    let narrow = "define <2 x i64> @f(<2 x i64> %v) #0 {\n  ret <2 x i64> %v\n}\n";
    let call = "declare <4 x i64> @g(<4 x i64>) #1\n\
        define <4 x i64> @f(<4 x i64> %v) #0 {\n  %r = call <4 x i64> @g(<4 x i64> %v)\n  \
        ret <4 x i64> %r\n}\nattributes #1 = { \"target-cpu\"=\"skylake-avx512\" }\n";
    let wide_vector = abi::WIDE_VECTOR_ABI;
    let vcall = hod_x64::vcall::VCALL;
    let rows: [(&str, &str, &str); 14] = [
        (wide, "nounwind", ""),
        (wide, "\"target-cpu\"=\"x86-64-v2\"", ""),
        (wide, "\"target-features\"=\"+avx\"", wide_vector),
        (wide, "\"target-features\"=\"+fma\"", wide_vector),
        (wide, "\"target-cpu\"=\"haswell\"", wide_vector),
        (wide, "\"target-cpu\"=\"some-future-cpu\"", wide_vector),
        (wide, "\"target-cpu\"=\"haswell\" \"target-features\"=\"-avx\"", ""),
        (mask, "\"target-cpu\"=\"haswell\"", vcall),
        (mask, "\"target-cpu\"=\"skylake-avx512\"", wide_vector),
        (mask, "\"target-cpu\"=\"haswell\" \"target-features\"=\"+avx512vl\"", wide_vector),
        (mask, "\"target-cpu\"=\"x86-64-v4\" \"target-features\"=\"-avx512f\"", vcall),
        (narrow, "\"target-cpu\"=\"skylake-avx512\"", ""),
        // The callee's features do not count: the caller has none.
        (call, "nounwind", ""),
        (call, "\"target-features\"=\"+avx2\"", wide_vector),
    ];
    for (body, attrs, expected) in rows {
        assert_eq!(refusal(&module(body, attrs)), expected, "{attrs}\n{body}");
    }
}

/// CPUs whose AVX and AVX-512 `features::of_function` was asked about,
/// with `llc` 19 and 22's answers. Every CPU of `hod_x64::features`'
/// tables is here but `prescott`, which has no 64-bit mode.
const CPUS: [&str; 51] = [
    "generic",
    "x86-64",
    "x86-64-v2",
    "nocona",
    "core2",
    "penryn",
    "nehalem",
    "corei7",
    "westmere",
    "bonnell",
    "atom",
    "silvermont",
    "slm",
    "goldmont",
    "goldmont-plus",
    "tremont",
    "k8",
    "k8-sse3",
    "amdfam10",
    "barcelona",
    "btver1",
    "x86-64-v3",
    "sandybridge",
    "corei7-avx",
    "ivybridge",
    "core-avx-i",
    "haswell",
    "core-avx2",
    "broadwell",
    "skylake",
    "alderlake",
    "raptorlake",
    "meteorlake",
    "gracemont",
    "arrowlake",
    "lunarlake",
    "pantherlake",
    "sierraforest",
    "grandridge",
    "clearwaterforest",
    "btver2",
    "bdver1",
    "bdver2",
    "bdver3",
    "bdver4",
    "znver1",
    "znver2",
    "znver3",
    "x86-64-v4",
    "skylake-avx512",
    "znver4",
];

/// Feature strings checked against `llc` on top of a CPU.
const FEATURES: [(&str, &str); 8] = [
    ("x86-64", "+avx"),
    ("x86-64", "+avx2"),
    ("x86-64", "+avx512f"),
    ("x86-64", "+fma"),
    ("haswell", "-avx"),
    ("haswell", "-sse4.2"),
    ("x86-64-v4", "-avx512f"),
    ("x86-64-v4", "-avx2"),
];

/// §1.3's tables: for every CPU (and feature string) hod knows, `llc`
/// passes a `<16 x i32>` in `zmm` exactly when hod says AVX-512, in `ymm`
/// exactly when hod says AVX only, and in `xmm` registers otherwise. A CPU
/// one `llc` does not know (it falls back to `generic`) must be one hod
/// treats as having both, which only refuses.
#[test]
fn cpu_tables_agree_with_llc() {
    let scratch = Scratch::new("x64-wp140-cpus");
    let mut cases: Vec<(String, Option<&str>)> =
        CPUS.iter().map(|cpu| ((*cpu).to_owned(), None)).collect();
    cases.extend(FEATURES.iter().map(|(cpu, f)| ((*cpu).to_owned(), Some(*f))));
    let mut text = header();
    for n in 0..cases.len() {
        write!(
            text,
            "define void @f{n}(<16 x i32> %v, ptr %p) #{n} {{\n  store <16 x i32> %v, ptr %p\n  \
             ret void\n}}\n"
        )
        .unwrap();
    }
    for (n, (cpu, features)) in cases.iter().enumerate() {
        let features = features.map_or(String::new(), |f| format!(" \"target-features\"=\"{f}\""));
        writeln!(text, "attributes #{n} = {{ nounwind \"target-cpu\"=\"{cpu}\"{features} }}")
            .unwrap();
    }
    for major in [19, 22] {
        let Some(llc) = llc_of(major) else { continue };
        let input = scratch.write(&format!("cpus{major}.ll"), &text);
        let mut command = Command::new(llc);
        command.args(["-O0", "-mtriple=x86_64-unknown-linux-gnu", "-o", "-"]).arg(&input);
        let outcome = run_with(&mut command, &[], Duration::from_mins(3));
        assert!(outcome.success(), "llc {major}: {}", outcome.stderr_text());
        let asm = outcome.stdout_text();
        let unknown = outcome.stderr_text();
        let mut checked = 0;
        for (n, (cpu, extra)) in cases.iter().enumerate() {
            let start = asm.find(&format!("\nf{n}:")).unwrap_or_else(|| panic!("f{n} in\n{asm}"));
            let end = asm[start..].find(".Lfunc_end").map_or(asm.len(), |e| start + e);
            let code = &asm[start..end];
            let llc_says = (code.contains("ymm") || code.contains("zmm"), code.contains("zmm"));
            let hod = features::of_function(Some(cpu), *extra).abi;
            if unknown.contains(&format!("'{cpu}' is not a recognized processor")) {
                assert!(hod.avx && hod.avx512, "llc {major} does not know {cpu}");
                continue;
            }
            assert_eq!((hod.avx, hod.avx512), llc_says, "{cpu} {extra:?} under llc {major}");
            checked += 1;
        }
        eprintln!("x64 wp140 cpus: llc {major}: {checked} of {} agree", cases.len());
    }
}

// --- `uwtable` -----------------------------------------------------------------

/// The number of FDEs `llvm-dwarfdump --eh-frame` lists in `object`.
fn fdes(object: &Path) -> Option<usize> {
    let dump = hod_testutil::tool("llvm-dwarfdump")?;
    let mut command = Command::new(dump);
    command.arg("--eh-frame").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-dwarfdump: {}", outcome.stderr_text());
    Some(outcome.stdout_text().lines().filter(|line| line.contains(" FDE ")).count())
}

/// The row `attr:uwtable` (an irdiff decline no group owned): a function
/// gets an unwind entry exactly when `llc` gives it one, `nounwind` or
/// not, with `uwtable` of either kind or without. What the entry says is
/// the skeleton's (`skeleton::cfi_is_design_section_4`).
#[test]
fn uwtable_gives_the_unwind_entries_llc_gives() {
    let scratch = Scratch::new("x64-wp140-uwtable");
    let body = "define i32 @f(i32 %a) #0 {\n  %r = add i32 %a, 1\n  ret i32 %r\n}\n";
    for (n, attrs) in
        ["nounwind", "nounwind uwtable", "nounwind uwtable(sync)", "uwtable", "noinline"]
            .iter()
            .enumerate()
    {
        let text = module(body, attrs);
        let ours = hod_object(&scratch, &format!("uw{n}"), &text);
        let Some(theirs) = llc(&scratch, 22, &format!("uw{n}-llc"), &text) else { return };
        let Some(hod_fdes) = fdes(&ours) else { return };
        assert_eq!(Some(hod_fdes), fdes(&theirs), "{attrs}");
        assert_eq!(hod_fdes, usize::from(*attrs != "nounwind"), "{attrs}");
    }
}

// --- `-x87` ------------------------------------------------------------------

/// §5.6.9: `-x87` refuses only a function with an `x86_fp80` value, and a
/// function without one compiles to the same object either way.
#[test]
fn no_x87_is_refused_only_with_x86_fp80() {
    let plain = "define i64 @f(i64 %a) #0 {\n  %r = add i64 %a, 1\n  ret i64 %r\n}\n";
    let without = module(plain, "nounwind");
    let with = module(plain, "nounwind \"target-features\"=\"-x87\"");
    let a = hod(&without).unwrap();
    let b = hod(&with).unwrap();
    assert_eq!(a, b, "-x87 changes nothing for a function without x86_fp80");
    let no_x87 = features::NO_X87;
    let fp80 = [
        "define x86_fp80 @f() #0 {\n  ret x86_fp80 0xK3FFF8000000000000000\n}\n",
        "define void @f(x86_fp80 %x) #0 {\n  ret void\n}\n",
        "define void @f(ptr %p, ptr %q) #0 {\n  %x = load x86_fp80, ptr %p\n  \
         store x86_fp80 %x, ptr %q\n  ret void\n}\n",
        "define void @f(ptr %p) #0 {\n  store { i8, [2 x x86_fp80] } zeroinitializer, ptr %p\n  \
         ret void\n}\n",
        "declare void @g(x86_fp80)\ndefine void @f() #0 {\n  \
         call void @g(x86_fp80 0xK3FFF8000000000000000)\n  ret void\n}\n",
    ];
    for body in fp80 {
        let text = module(body, "nounwind \"target-features\"=\"-x87\"");
        assert_eq!(refusal(&text), no_x87, "{body}");
    }
}

// --- The rows no group owned ---------------------------------------------------

/// The types both modules of the row program use.
const ROW_TYPES: &str = "
%Big = type { i64, i32, [5 x i16], i8 }
%Small = type { i8, i8, i8 }
%Wide = type { i64, i64, i64 }
";

/// The callees (module B). `@sum_big` and `@twice` scribble over their
/// `byval` copy, which the caller's object must not see.
const ROW_CALLEES: &str = "
define void @make_big(ptr sret(%Big) align 8 %out, i64 %x) {
  store i64 %x, ptr %out
  %p1 = getelementptr %Big, ptr %out, i64 0, i32 1
  %t = trunc i64 %x to i32
  %m = mul i32 %t, 3
  store i32 %m, ptr %p1
  %h = getelementptr %Big, ptr %out, i64 0, i32 2, i64 0
  store i16 0, ptr %h
  %h1 = getelementptr i16, ptr %h, i64 1
  store i16 -3, ptr %h1
  %h2 = getelementptr i16, ptr %h, i64 2
  store i16 6, ptr %h2
  %h3 = getelementptr i16, ptr %h, i64 3
  store i16 -9, ptr %h3
  %h4 = getelementptr i16, ptr %h, i64 4
  store i16 12, ptr %h4
  %p3 = getelementptr %Big, ptr %out, i64 0, i32 3
  store i8 -7, ptr %p3
  ret void
}

define i64 @sum_big(ptr byval(%Big) align 8 %b) {
  %v = load %Big, ptr %b
  %f0 = extractvalue %Big %v, 0
  %f1 = extractvalue %Big %v, 1
  %f2 = extractvalue %Big %v, 2, 4
  %f3 = extractvalue %Big %v, 3
  %w1 = sext i32 %f1 to i64
  %w2 = sext i16 %f2 to i64
  %w3 = sext i8 %f3 to i64
  %s0 = add i64 %f0, %w1
  %s1 = mul i64 %s0, 31
  %s2 = add i64 %s1, %w2
  %s3 = mul i64 %s2, 31
  %s4 = add i64 %s3, %w3
  store i64 -1, ptr %b
  %p1 = getelementptr %Big, ptr %b, i64 0, i32 1
  store i32 -1, ptr %p1
  ret i64 %s4
}

define i64 @sum_big9(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a8,
                     ptr byval(%Big) align 16 %b) {
  %int = ptrtoint ptr %b to i64
  %misalign = and i64 %int, 15
  %v = load %Big, ptr %b
  %f0 = extractvalue %Big %v, 0
  %f2 = extractvalue %Big %v, 2, 1
  %w2 = sext i16 %f2 to i64
  %s0 = add i64 %a1, %a8
  %s1 = mul i64 %s0, 1000
  %s2 = add i64 %s1, %f0
  %s3 = mul i64 %s2, 100
  %s4 = add i64 %s3, %w2
  %s5 = mul i64 %s4, 16
  %s6 = add i64 %s5, %misalign
  ret i64 %s6
}

define i64 @small(ptr byval(%Small) align 1 %s) {
  %v = load %Small, ptr %s
  %a = extractvalue %Small %v, 0
  %b = extractvalue %Small %v, 1
  %c = extractvalue %Small %v, 2
  %wa = sext i8 %a to i64
  %wb = sext i8 %b to i64
  %wc = sext i8 %c to i64
  %x = mul i64 %wa, 10000
  %y = mul i64 %wb, 100
  %s1 = add i64 %x, %y
  %s2 = add i64 %s1, %wc
  ret i64 %s2
}

define void @twice(ptr sret(%Big) align 8 %out, ptr byval(%Big) align 8 %in) {
  %v = load %Big, ptr %in
  %f0 = extractvalue %Big %v, 0
  %d0 = shl i64 %f0, 1
  %f1 = extractvalue %Big %v, 1
  %d1 = shl i32 %f1, 1
  %f3 = extractvalue %Big %v, 3
  %d3 = add i8 %f3, %f3
  %a = insertvalue %Big %v, i64 %d0, 0
  %b = insertvalue %Big %a, i32 %d1, 1
  %c = insertvalue %Big %b, i8 %d3, 3
  %g = extractvalue %Big %c, 2, 3
  %g2 = mul i16 %g, 2
  %d = insertvalue %Big %c, i16 %g2, 2, 3
  store %Big %d, ptr %out
  store i64 0, ptr %in
  ret void
}

define fastcc i64 @fast(i64 %a, i32 %b, ptr %p, { i64, i64 } %agg, i8 signext %c) {
  %x = load i64, ptr %p
  %y = extractvalue { i64, i64 } %agg, 0
  %z = extractvalue { i64, i64 } %agg, 1
  %wb = sext i32 %b to i64
  %wc = sext i8 %c to i64
  %s0 = mul i64 %a, 7
  %s1 = add i64 %s0, %wb
  %s2 = xor i64 %s1, %x
  %s3 = sub i64 %s2, %y
  %s4 = mul i64 %s3, %z
  %s5 = add i64 %s4, %wc
  ret i64 %s5
}

define fastcc %Wide @fast_wide(i64 %x, i64 %y) {
  %a = insertvalue %Wide poison, i64 %x, 0
  %s = add i64 %x, %y
  %b = insertvalue %Wide %a, i64 %s, 1
  %m = mul i64 %x, %y
  %c = insertvalue %Wide %b, i64 %m, 2
  ret %Wide %c
}

define { i32, ptr } @pair(i32 %a, ptr %p) {
  %x = insertvalue { i32, ptr } poison, i32 %a, 0
  %y = insertvalue { i32, ptr } %x, ptr %p, 1
  ret { i32, ptr } %y
}

define { i8, ptr, i64 } @swap_mix({ i64, ptr, i8 } %v) {
  %a = extractvalue { i64, ptr, i8 } %v, 0
  %p = extractvalue { i64, ptr, i8 } %v, 1
  %c = extractvalue { i64, ptr, i8 } %v, 2
  %x = insertvalue { i8, ptr, i64 } poison, i8 %c, 0
  %y = insertvalue { i8, ptr, i64 } %x, ptr %p, 1
  %z = insertvalue { i8, ptr, i64 } %y, i64 %a, 2
  ret { i8, ptr, i64 } %z
}
";

/// The callers (module A): `@test` returns a checksum of everything the
/// callees gave back and of the caller's objects after the calls.
const ROW_CALLERS: &str = "
define internal i64 @mix(i64 %acc, i64 %v) {
  %a = mul i64 %acc, 1000003
  %b = xor i64 %a, %v
  ret i64 %b
}

define internal fastcc i64 @local_fast(%Wide %w, i32 %k) {
  %a = extractvalue %Wide %w, 0
  %b = extractvalue %Wide %w, 2
  %wk = zext i32 %k to i64
  %s = sub i64 %b, %a
  %r = add i64 %s, %wk
  ret i64 %r
}

define i64 @test() {
  %big = alloca %Big, align 8
  %big2 = alloca %Big, align 16
  %out = alloca %Big, align 8
  %sm = alloca %Small, align 1
  %cell = alloca i64, align 8
  call void @make_big(ptr sret(%Big) align 8 %big, i64 3000000021)
  %v0 = load %Big, ptr %big
  %e0 = extractvalue %Big %v0, 0
  %e1 = extractvalue %Big %v0, 1
  %e1w = sext i32 %e1 to i64
  %e2 = extractvalue %Big %v0, 2, 3
  %e2w = sext i16 %e2 to i64
  %c0 = call i64 @mix(i64 1, i64 %e0)
  %c1 = call i64 @mix(i64 %c0, i64 %e1w)
  %c2 = call i64 @mix(i64 %c1, i64 %e2w)

  %s1 = call i64 @sum_big(ptr byval(%Big) align 8 %big)
  %c3 = call i64 @mix(i64 %c2, i64 %s1)
  %after = load i64, ptr %big
  %c4 = call i64 @mix(i64 %c3, i64 %after)

  %fp = select i1 true, ptr @sum_big, ptr null
  %s2 = call i64 %fp(ptr byval(%Big) align 8 %big)
  %c5 = call i64 @mix(i64 %c4, i64 %s2)

  call void @make_big(ptr sret(%Big) align 8 %big2, i64 -5)
  %s9 = call i64 @sum_big9(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 8,
                           ptr byval(%Big) align 16 %big2)
  %c6 = call i64 @mix(i64 %c5, i64 %s9)

  call void @twice(ptr sret(%Big) align 8 %out, ptr byval(%Big) align 8 %big)
  %v1 = load %Big, ptr %out
  %t0 = extractvalue %Big %v1, 0
  %t1 = extractvalue %Big %v1, 1
  %t1w = zext i32 %t1 to i64
  %t2 = extractvalue %Big %v1, 2, 3
  %t2w = sext i16 %t2 to i64
  %t3 = extractvalue %Big %v1, 3
  %t3w = sext i8 %t3 to i64
  %c7 = call i64 @mix(i64 %c6, i64 %t0)
  %c8 = call i64 @mix(i64 %c7, i64 %t1w)
  %c9 = call i64 @mix(i64 %c8, i64 %t2w)
  %c10 = call i64 @mix(i64 %c9, i64 %t3w)
  %kept = load i64, ptr %big
  %c11 = call i64 @mix(i64 %c10, i64 %kept)

  %sv0 = insertvalue %Small poison, i8 7, 0
  %sv1 = insertvalue %Small %sv0, i8 -2, 1
  %sv2 = insertvalue %Small %sv1, i8 9, 2
  store %Small %sv2, ptr %sm
  %s3 = call i64 @small(ptr byval(%Small) align 1 %sm)
  %c12 = call i64 @mix(i64 %c11, i64 %s3)

  store i64 77, ptr %cell
  %agg0 = insertvalue { i64, i64 } poison, i64 5, 0
  %agg1 = insertvalue { i64, i64 } %agg0, i64 -3, 1
  %f = call fastcc i64 @fast(i64 11, i32 -4, ptr %cell, { i64, i64 } %agg1, i8 signext -9)
  %c13 = call i64 @mix(i64 %c12, i64 %f)
  %w = call fastcc %Wide @fast_wide(i64 40000000000, i64 -3)
  %w2 = extractvalue %Wide %w, 1
  %c14 = call i64 @mix(i64 %c13, i64 %w2)
  %l = call fastcc i64 @local_fast(%Wide %w, i32 4000000000)
  %c15 = call i64 @mix(i64 %c14, i64 %l)

  %pr = call { i32, ptr } @pair(i32 -123, ptr %cell)
  %pa = extractvalue { i32, ptr } %pr, 0
  %paw = sext i32 %pa to i64
  %pp = extractvalue { i32, ptr } %pr, 1
  %pv = load i64, ptr %pp
  %same = icmp eq ptr %pp, %cell
  %samew = zext i1 %same to i64
  %c16 = call i64 @mix(i64 %c15, i64 %paw)
  %c17 = call i64 @mix(i64 %c16, i64 %pv)
  %c18 = call i64 @mix(i64 %c17, i64 %samew)

  %m0 = insertvalue { i64, ptr, i8 } poison, i64 -9000000000, 0
  %m1 = insertvalue { i64, ptr, i8 } %m0, ptr %cell, 1
  %m2 = insertvalue { i64, ptr, i8 } %m1, i8 100, 2
  %sw = call { i8, ptr, i64 } @swap_mix({ i64, ptr, i8 } %m2)
  %x0 = extractvalue { i8, ptr, i64 } %sw, 0
  %x0w = zext i8 %x0 to i64
  %x1 = extractvalue { i8, ptr, i64 } %sw, 1
  %x1v = load i64, ptr %x1
  %x2 = extractvalue { i8, ptr, i64 } %sw, 2
  %c19 = call i64 @mix(i64 %c18, i64 %x0w)
  %c20 = call i64 @mix(i64 %c19, i64 %x1v)
  %c21 = call i64 @mix(i64 %c20, i64 %x2)
  ret i64 %c21
}
";

/// The declarations module A needs of module B.
const ROW_DECLS: &str = "
declare void @make_big(ptr sret(%Big) align 8, i64)
declare i64 @sum_big(ptr byval(%Big) align 8)
declare i64 @sum_big9(i64, i64, i64, i64, i64, i64, i64, ptr byval(%Big) align 16)
declare i64 @small(ptr byval(%Small) align 1)
declare void @twice(ptr sret(%Big) align 8, ptr byval(%Big) align 8)
declare fastcc i64 @fast(i64, i32, ptr, { i64, i64 }, i8 signext)
declare fastcc %Wide @fast_wide(i64, i64)
declare { i32, ptr } @pair(i32, ptr)
declare { i8, ptr, i64 } @swap_mix({ i64, ptr, i8 })
";

/// The C driver of the row program.
const ROW_MAIN: &str = r#"
#include <stdio.h>
long long test(void);
int main(void) {
    printf("%lld\n", test());
    return 0;
}
"#;

/// The rows `attr:sret`, `attr:byval`, `cc:fastcc` and `inst:insertvalue`/
/// `inst:extractvalue` of `Int` and `Ptr` members: `hod_interp` runs the
/// whole program for the expected checksum; the callers and the callees
/// are each compiled by hod and by `llc` 19 and 22, the four pairings
/// linked by bfd, lld and mold and run. Prints hod's text bytes against
/// `llc` 22's for the same IR.
#[test]
fn rows_run_against_the_interpreter_and_llc() {
    let whole = format!("{}{ROW_TYPES}{ROW_CALLEES}{ROW_CALLERS}{PIE_FLAGS}", header());
    let (module, bodies) = parse(&whole);
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let test = module.find_symbol(b"test").expect("@test");
    let mut interp = Interp::new(&program);
    let expected = match interp.run(test, &[]).result {
        Ok(Some(Val::Int { bits: 64, value })) => {
            format!("{}\n", u64::try_from(value).unwrap().cast_signed())
        }
        other => panic!("the interpreter: {other:?}"),
    };

    let module_a = format!("{}{ROW_TYPES}{ROW_DECLS}{ROW_CALLERS}{PIE_FLAGS}", header());
    let module_b = format!("{}{ROW_TYPES}{ROW_CALLEES}{PIE_FLAGS}", header());
    let scratch = Scratch::new("x64-wp140-rows");
    let hod_a = hod_object(&scratch, "rows-a-hod", &module_a);
    let hod_b = hod_object(&scratch, "rows-b-hod", &module_b);
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let main = toolchain.compile_c(&scratch, "rows-main", ROW_MAIN, &["-fPIE"]);
    let out = link_and_run(&toolchain, &scratch, "rows-hod", &[&main, &hod_a, &hod_b]);
    assert_eq!(out, expected, "hod alone");
    let mut runs = 3;
    for major in [19, 22] {
        let Some(llc_a) = llc(&scratch, major, &format!("rows-a-llc{major}"), &module_a) else {
            continue;
        };
        let Some(llc_b) = llc(&scratch, major, &format!("rows-b-llc{major}"), &module_b) else {
            continue;
        };
        for (tag, a, b) in
            [("llc", &llc_a, &llc_b), ("hod-a", &hod_a, &llc_b), ("hod-b", &llc_a, &hod_b)]
        {
            let out =
                link_and_run(&toolchain, &scratch, &format!("rows-{tag}{major}"), &[&main, a, b]);
            assert_eq!(out, expected, "{tag} with llc {major}");
            runs += 3;
        }
        if major == 22
            && let (Some(h), Some(l)) = (
                text_bytes(&hod_a).zip(text_bytes(&hod_b)).map(|(a, b)| a + b),
                text_bytes(&llc_a).zip(text_bytes(&llc_b)).map(|(a, b)| a + b),
            )
        {
            #[allow(clippy::cast_precision_loss, reason = "a ratio to print")]
            let ratio = h as f64 / l as f64;
            eprintln!("x64 wp140 rows: text hod {h} B, llc 22 {l} B (ratio {ratio:.2})");
        }
    }
    eprintln!("x64 wp140 rows: checksum {} in {runs} linked runs", expected.trim());
}
