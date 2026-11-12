//! What inlined subroutines cost (`spec/dwarf.md` §6.9, §7.3; WP-085):
//! generated inlining at the scale the design must stay linear at.
//!
//! - `calls`: one function with `N` calls of one callee inlined, a
//!   parameter in each (`N` inlined subroutines of one abstract instance);
//! - `deep`: a chain of `N` callees, each inlined into the one before
//!   (`N` abstract instances, inlined subroutines nested `N` deep);
//! - `many`: `N / 10` functions of ten inlined calls each.
//!
//! The gate test compiles all three at `N` = 20,000 and checks the DIE
//! counts (no time is measured; `deep`'s are counted at 2,000, where
//! `llvm-dwarfdump` does not overflow its own stack). The measurement
//! (ignored) times the module emission of each at `N` and `2N`, best of
//! five, and prints ns per DIE and the ratio of the two:
//!
//! ```text
//! cargo test --release -p hod_a64 --test dies_inline_throughput -- --ignored --nocapture
//! ```

mod common;

use std::fmt::Write as _;
use std::time::Instant;

use common::{ll_header, parse};
use hod_a64::A64;
use hod_cg::{Chunk, Options, Scratch as CgScratch, compile_function, emit_module};

const N: usize = 20_000;

const HEAD: &str = "!llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!2, !3}\n\
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"gen\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)\n\
!1 = !DIFile(filename: \"gen.c\", directory: \"/hod/gen\")\n\
!2 = !{i32 7, !\"Dwarf Version\", i32 5}\n!3 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
!4 = !DISubroutineType(types: !5)\n!5 = !{null}\n\
!6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n!7 = !{}\n\
!8 = distinct !DISubprogram(name: \"callee\", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)\n\
!9 = !DILocalVariable(name: \"v\", arg: 1, scope: !8, file: !1, line: 1, type: !6)\n";

fn subprogram(meta: &mut String, id: usize, name: &str) {
    writeln!(
        meta,
        "!{id} = distinct !DISubprogram(name: \"{name}\", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)"
    )
    .unwrap();
}

/// A function `f{f}` with `n` calls of `callee` inlined, each with its
/// parameter declared and stored.
fn calls(f: usize, n: usize, next: &mut usize, meta: &mut String, code: &mut String) {
    let sp = *next;
    *next += 1;
    subprogram(meta, sp, &format!("f{f}"));
    writeln!(code, "define dso_local void @f{f}() !dbg !{sp} {{\nentry:").unwrap();
    for i in 0..n {
        writeln!(code, "  %v{i} = alloca i32, align 4").unwrap();
    }
    for i in 0..n {
        let (call, loc) = (*next, *next + 1);
        *next += 2;
        writeln!(
            meta,
            "!{call} = !DILocation(line: {}, column: 3, scope: !{sp})\n\
             !{loc} = !DILocation(line: 1, column: 5, scope: !8, inlinedAt: !{call})",
            i + 2
        )
        .unwrap();
        writeln!(
            code,
            "    #dbg_declare(ptr %v{i}, !9, !DIExpression(), !{loc})\n  store i32 {i}, ptr %v{i}, align 4, !dbg !{loc}"
        )
        .unwrap();
    }
    let ret = *next;
    *next += 1;
    writeln!(meta, "!{ret} = !DILocation(line: {}, column: 1, scope: !{sp})", n + 3).unwrap();
    writeln!(code, "  ret void, !dbg !{ret}\n}}").unwrap();
}

/// One function into which `c0` is inlined, `c1` into `c0`, and so on
/// `n` deep, each level with a variable declared and stored.
fn deep(n: usize, next: &mut usize, meta: &mut String, code: &mut String) {
    let sp = *next;
    *next += 1;
    subprogram(meta, sp, "f");
    writeln!(code, "define dso_local void @f() !dbg !{sp} {{\nentry:").unwrap();
    for k in 0..n {
        writeln!(code, "  %v{k} = alloca i32, align 4").unwrap();
    }
    // The call of level k is in level k - 1's scope (the function's for
    // k = 0), inlined at level k - 1's call.
    let (mut caller, mut caller_at) = (sp, None::<usize>);
    for k in 0..n {
        let (c, var, call, loc) = (*next, *next + 1, *next + 2, *next + 3);
        *next += 4;
        subprogram(meta, c, &format!("c{k}"));
        let at = caller_at.map(|a| format!(", inlinedAt: !{a}")).unwrap_or_default();
        writeln!(
            meta,
            "!{var} = !DILocalVariable(name: \"v{k}\", scope: !{c}, file: !1, line: 2, type: !6)\n\
             !{call} = !DILocation(line: 3, column: 3, scope: !{caller}{at})\n\
             !{loc} = !DILocation(line: 2, column: 5, scope: !{c}, inlinedAt: !{call})"
        )
        .unwrap();
        writeln!(
            code,
            "    #dbg_declare(ptr %v{k}, !{var}, !DIExpression(), !{loc})\n  store i32 {k}, ptr %v{k}, align 4, !dbg !{loc}"
        )
        .unwrap();
        (caller, caller_at) = (c, Some(call));
    }
    let ret = *next;
    *next += 1;
    writeln!(meta, "!{ret} = !DILocation(line: 4, column: 1, scope: !{sp})").unwrap();
    writeln!(code, "  ret void, !dbg !{ret}\n}}").unwrap();
}

/// The generated module of a shape at size `n`.
fn module(shape: &str, n: usize) -> String {
    let (mut meta, mut code) = (String::new(), String::new());
    let mut next = 10;
    match shape {
        "calls" => calls(0, n, &mut next, &mut meta, &mut code),
        "deep" => deep(n, &mut next, &mut meta, &mut code),
        _ => {
            for f in 0..n / 10 {
                calls(f, 10, &mut next, &mut meta, &mut code);
            }
        }
    }
    format!("{}{code}\n{HEAD}{meta}", ll_header(22))
}

/// The DIEs a shape at `n` has: (subprograms, inlined subroutines,
/// parameters and variables).
fn want(shape: &str, n: usize) -> (usize, usize, usize) {
    match shape {
        // f and callee's abstract instance; a parameter per call and
        // the abstract one.
        "calls" => (2, n, n + 1),
        // f and n abstract instances; a variable per level, twice.
        "deep" => (n + 1, n, 2 * n),
        _ => (n / 10 + 1, n, n + 1),
    }
}

/// The module's chunk, compiled once; then the module emission (which
/// writes the DIEs), timed.
fn emit_timed(text: &str, runs: usize) -> (f64, Vec<u8>) {
    let (module, bodies) = parse(text);
    let options = Options::default();
    let mut scratch = CgScratch::new();
    let mut chunk = Chunk::new();
    for body in &bodies {
        compile_function(&A64, &module, body, &options, &mut scratch, &mut chunk).unwrap();
    }
    let chunks = [chunk];
    let mut best = f64::MAX;
    let mut bytes = Vec::new();
    for _ in 0..runs {
        let start = Instant::now();
        let emitted = emit_module(&module, &options, &chunks).unwrap();
        best = best.min(start.elapsed().as_secs_f64());
        bytes = hod_obj::elf::to_vec(&emitted.object).unwrap();
    }
    (best, bytes)
}

/// The DIEs of `bytes`' `.debug_info` by tag, read with `llvm-dwarfdump`
/// when there is one.
fn tags(bytes: &[u8], name: &str) -> Option<(usize, usize, usize)> {
    let dwarfdump = hod_testutil::tool("llvm-dwarfdump")?;
    let scratch = hod_testutil::Scratch::new(&format!("dies-inline-scale-{name}"));
    let path = scratch.write("o.o", bytes);
    let out =
        hod_testutil::run(&dwarfdump, [std::ffi::OsString::from("--debug-info"), path.into()]);
    assert!(out.success(), "{}", out.stderr_text());
    let text = out.stdout_text();
    let count = |tag: &str| text.lines().filter(|l| l.trim_end().ends_with(tag)).count();
    Some((
        count("DW_TAG_subprogram"),
        count("DW_TAG_inlined_subroutine"),
        count("DW_TAG_formal_parameter") + count("DW_TAG_variable"),
    ))
}

/// At `N` every shape compiles; the DIEs are counted at `N`, but `deep`'s
/// at 2,000.
#[test]
fn twenty_thousand_scale() {
    for (shape, n, check) in [("calls", N, N), ("deep", N, 2_000), ("many", N, N)] {
        let (_, bytes) = emit_timed(&module(shape, n), 1);
        assert!(!bytes.is_empty());
        let bytes = if check == n { bytes } else { emit_timed(&module(shape, check), 1).1 };
        if let Some(got) = tags(&bytes, shape) {
            assert_eq!(got, want(shape, check), "{shape}: (subprograms, inlined, variables)");
        }
    }
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn inlined_die_cost() {
    for shape in ["calls", "deep", "many"] {
        let mut per = Vec::new();
        for n in [N, 2 * N] {
            let text = module(shape, n);
            let (with, bytes) = emit_timed(&text, 5);
            let bare = text.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
            let (without, _) = emit_timed(&bare, 5);
            let (s, i, v) = want(shape, n);
            let dies = s + i + v;
            let ns = (with - without).max(0.0) * 1e9 / dies as f64;
            per.push(ns);
            eprintln!(
                "{shape} n={n}: emission {:.2} ms with DIEs, {:.2} ms without; {ns:.0} ns per DIE; object {} bytes",
                with * 1e3,
                without * 1e3,
                bytes.len()
            );
        }
        eprintln!("{shape}: ns per DIE at 2N / at N = {:.2}", per[1] / per[0].max(1e-9));
    }
}
