//! What function DIEs cost (`spec/dwarf.md` §7.3): generated functions
//! at the scale the design must stay linear at.
//!
//! - `wide`: one function with `N` locals, each declared and stored;
//! - `deep`: one function whose scopes nest `N` deep, a local in each;
//! - `many`: `N / 10` functions of ten locals in three blocks each.
//!
//! The gate test compiles all three at `N` = 20,000 and checks the DIE
//! counts (no time is measured). The measurement (ignored) times the
//! module emission of each at `N` and `2N`, best of five, and prints ns
//! per DIE and the ratio of the two:
//!
//! ```text
//! cargo test --release -p hod_a64 --test dies_throughput -- --ignored --nocapture
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
!6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n!7 = !{}\n";

/// One function `f{f}` with `n` locals; local `i` is in scope
/// `scope_of(i)` (non-decreasing in `i`).
fn function(
    f: usize,
    n: usize,
    next: &mut usize,
    meta: &mut String,
    code: &mut String,
    scope_of: &dyn Fn(usize) -> usize,
) {
    let sp = *next;
    *next += 1;
    writeln!(
        meta,
        "!{sp} = distinct !DISubprogram(name: \"f{f}\", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)"
    )
    .unwrap();
    // Scope 0 is the subprogram; scope k > 0 a block in scope k - 1.
    let mut scopes = vec![sp];
    writeln!(code, "define dso_local void @f{f}() !dbg !{sp} {{\nentry:").unwrap();
    for i in 0..n {
        writeln!(code, "  %v{i} = alloca i32, align 4").unwrap();
    }
    for i in 0..n {
        let s = scope_of(i);
        while scopes.len() <= s {
            let id = *next;
            *next += 1;
            let parent = scopes[scopes.len() - 1];
            writeln!(
                meta,
                "!{id} = distinct !DILexicalBlock(scope: !{parent}, file: !1, line: {}, column: 3)",
                scopes.len() + 1
            )
            .unwrap();
            scopes.push(id);
        }
        let (var, loc) = (*next, *next + 1);
        *next += 2;
        writeln!(
            meta,
            "!{var} = !DILocalVariable(name: \"v{i}\", scope: !{}, file: !1, line: {}, type: !6)\n\
             !{loc} = !DILocation(line: {}, column: 5, scope: !{})",
            scopes[s],
            i + 2,
            i + 2,
            scopes[s]
        )
        .unwrap();
        writeln!(
            code,
            "    #dbg_declare(ptr %v{i}, !{var}, !DIExpression(), !{loc})\n  store i32 {i}, ptr %v{i}, align 4, !dbg !{loc}"
        )
        .unwrap();
    }
    let ret = *next;
    *next += 1;
    writeln!(meta, "!{ret} = !DILocation(line: {}, column: 1, scope: !{sp})", n + 3).unwrap();
    writeln!(code, "  ret void, !dbg !{ret}\n}}").unwrap();
}

/// The generated module of a shape at size `n`.
fn module(shape: &str, n: usize) -> String {
    let (mut meta, mut code) = (String::new(), String::new());
    let mut next = 10;
    match shape {
        "wide" => function(0, n, &mut next, &mut meta, &mut code, &|_| 0),
        "deep" => function(0, n, &mut next, &mut meta, &mut code, &|i| i),
        _ => {
            for f in 0..n / 10 {
                function(f, 10, &mut next, &mut meta, &mut code, &|i| i / 4);
            }
        }
    }
    format!("{}{code}\n{HEAD}{meta}", ll_header(22))
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
    let scratch = hod_testutil::Scratch::new(&format!("dies-scale-{name}"));
    let path = scratch.write("o.o", bytes);
    let out =
        hod_testutil::run(&dwarfdump, [std::ffi::OsString::from("--debug-info"), path.into()]);
    assert!(out.success(), "{}", out.stderr_text());
    let text = out.stdout_text();
    let count = |tag: &str| text.lines().filter(|l| l.trim_end().ends_with(tag)).count();
    Some((count("DW_TAG_subprogram"), count("DW_TAG_variable"), count("DW_TAG_lexical_block")))
}

/// At `N` every shape compiles; the DIEs are counted at `N`, but for
/// `deep`'s, counted at 2,000 (`llvm-dwarfdump` itself overflows its
/// stack on a tree 20,000 deep; hod writes it without recursion).
#[test]
fn twenty_thousand_scale() {
    for (shape, n, check) in [("wide", N, N), ("deep", N, 2_000), ("many", N, N)] {
        let (_, bytes) = emit_timed(&module(shape, n), 1);
        assert!(!bytes.is_empty());
        let bytes = if check == n { bytes } else { emit_timed(&module(shape, check), 1).1 };
        let want = match shape {
            "wide" => (1, check, 0),
            "deep" => (1, check, check - 1),
            _ => (check / 10, check, 2 * check / 10),
        };
        if let Some(got) = tags(&bytes, shape) {
            assert_eq!(got, want, "{shape}: (subprograms, variables, blocks)");
        }
    }
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn function_die_cost() {
    for shape in ["wide", "deep", "many"] {
        let mut per = Vec::new();
        for n in [N, 2 * N] {
            let text = module(shape, n);
            let (with, _) = emit_timed(&text, 5);
            let bare = text.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
            let (without, _) = emit_timed(&bare, 5);
            // A DIE per variable, block and function.
            let dies = match shape {
                "wide" => n + 1,
                "deep" => 2 * n,
                _ => n + 3 * n / 10,
            };
            let ns = (with - without).max(0.0) * 1e9 / dies as f64;
            per.push(ns);
            eprintln!(
                "{shape} n={n}: emission {:.2} ms with DIEs, {:.2} ms without; {ns:.0} ns per DIE",
                with * 1e3,
                without * 1e3
            );
        }
        eprintln!("{shape}: ns per DIE at 2N / at N = {:.2}", per[1] / per[0].max(1e-9));
    }
}
