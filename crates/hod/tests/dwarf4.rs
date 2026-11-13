//! DWARF version 4 through the facade (WP-114, `spec/dwarf.md` §6.20):
//! a module whose `Dwarf Version` flag is 4 is written as version 4 on
//! both targets; versions 2 and 3 are refused as `debug:dwarf-version`,
//! split DWARF with version 4 as `debug:split-dwarf-4`. The comparison
//! with `llc` is `xtask/tests/debug_dwarf4.rs`.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

/// clang 22's `-g -O0 -gdwarf-4` IR of a small C function, its producer
/// and paths shortened.
const TINY: &str = r#"source_filename = "tiny.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

@counter = dso_local global i32 3, align 4, !dbg !30

define dso_local i32 @g(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
    #dbg_declare(ptr %3, !17, !DIExpression(), !18)
  %4 = load i32, ptr %2, align 4, !dbg !19
  %5 = add nsw i32 %4, 1, !dbg !20
  store i32 %5, ptr %3, align 4, !dbg !18
  %6 = load i32, ptr %3, align 4, !dbg !21
  ret i32 %6, !dbg !22
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !32, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tiny.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 4}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!16 = !DILocation(line: 1, column: 11, scope: !10)
!17 = !DILocalVariable(name: "y", scope: !10, file: !1, line: 2, type: !13)
!18 = !DILocation(line: 2, column: 7, scope: !10)
!19 = !DILocation(line: 2, column: 11, scope: !10)
!20 = !DILocation(line: 2, column: 13, scope: !10)
!21 = !DILocation(line: 3, column: 10, scope: !10)
!22 = !DILocation(line: 3, column: 3, scope: !10)
!30 = !DIGlobalVariableExpression(var: !31, expr: !DIExpression())
!31 = distinct !DIGlobalVariable(name: "counter", scope: !0, file: !1, line: 5, type: !13, isLocal: false, isDefinition: true)
!32 = !{!30}
"#;

const X86_64: (&str, &str) = (
    "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
    "x86_64-unknown-linux-gnu",
);

/// `TINY` for x86-64.
fn x86_64(text: &str) -> String {
    text.replace("e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128", X86_64.0)
        .replace("aarch64-unknown-linux-gnu", X86_64.1)
        .replace(r#" "frame-pointer"="non-leaf""#, r#" "frame-pointer"="all""#)
}

fn options() -> hod::Options {
    hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() }
}

fn compile_as(text: &str, options: &hod::Options) -> Result<Vec<u8>, CompileError> {
    compile_with(text.as_bytes(), options, &Settings::default(), &mut Report::default())
}

/// The decline's construct name; panics on anything else.
fn declined(text: &str, options: &hod::Options) -> String {
    match compile_as(text, options) {
        Err(CompileError::Unsupported(u)) => match u.construct {
            Construct::Named(name) => name.into_owned(),
            other => panic!("declined, but not by name: {other:?}"),
        },
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn with_version(text: &str, version: u32) -> String {
    text.replace(r#""Dwarf Version", i32 4"#, &format!(r#""Dwarf Version", i32 {version}"#))
}

#[test]
fn versions_2_and_3_are_refused_by_name() {
    for text in [TINY.to_owned(), x86_64(TINY)] {
        for version in [2, 3] {
            assert_eq!(declined(&with_version(&text, version), &options()), "debug:dwarf-version");
        }
    }
}

/// The object of a version 4 module has `.debug_str` and no section of
/// version 5's (`.debug_str_offsets`, `.debug_addr`, `.debug_line_str`,
/// `.debug_rnglists`); with `llvm-dwarfdump`, its unit and its line
/// table say version 4 and it verifies.
#[test]
fn a_version_4_module_is_written_as_version_4_on_both_targets() {
    let scratch = Scratch::new("dwarf4-facade");
    for (name, text) in [("aarch64", TINY.to_owned()), ("x86_64", x86_64(TINY))] {
        let object = compile_as(&text, &options()).unwrap_or_else(|e| panic!("{name}: {e}"));
        for section in [&b".debug_info"[..], b".debug_abbrev", b".debug_line", b".debug_str"] {
            assert!(contains(&object, section), "{name}: no {}", String::from_utf8_lossy(section));
        }
        for section in
            [&b".debug_str_offsets"[..], b".debug_line_str", b".debug_addr", b".debug_rnglists"]
        {
            assert!(!contains(&object, section), "{name}: {}", String::from_utf8_lossy(section));
        }
        let Some(dwarfdump) = tool("llvm-dwarfdump") else { continue };
        let path = scratch.write(&format!("{name}.o"), &object);
        let dump = |args: &[&str]| {
            let mut command = std::process::Command::new(&dwarfdump);
            command.args(args).arg(&path);
            let out = run_with(&mut command, b"", Duration::from_mins(1));
            assert!(out.success(), "{name}: {}", out.stderr_text());
            out.stdout_text()
        };
        assert!(dump(&["--verify"]).contains("No errors."), "{name}");
        assert!(dump(&["--debug-info"]).contains("version = 0x0004"), "{name}");
        let lines = dump(&["--debug-line"]);
        assert!(lines.contains("version: 4"), "{name}: {lines}");
        assert!(lines.contains("name: \"tiny.c\""), "{name}: {lines}");
        let info = dump(&["--debug-info"]);
        assert!(info.contains("DW_AT_name\t(\"counter\")"), "{name}: {info}");
    }
}

/// `-gsplit-dwarf -gdwarf-4` (GNU fission) is refused by its name.
#[test]
fn split_dwarf_4_is_refused_by_name() {
    let text = TINY.replace(
        "splitDebugInlining: false",
        "splitDebugFilename: \"tiny.dwo\", splitDebugInlining: false",
    );
    let options = hod::Options { split_dwarf_file: Some(PathBuf::from("tiny.dwo")), ..options() };
    for text in [text.clone(), x86_64(&text)] {
        assert_eq!(declined(&text, &options), hod_dwarf::SPLIT_DWARF_4);
    }
}

/// `n` C functions, each with two parameters, three locals, a local
/// structure type of two members: nine DIEs.
fn source(n: usize) -> String {
    let mut out = String::new();
    for k in 0..n {
        let _ = writeln!(
            out,
            "int f{k}(int a, int b) {{ int x = a + {k}; int y = b * x; \
             struct s{k} {{ int p, q; }} s = {{x, y}}; return s.p + s.q; }}"
        );
    }
    out
}

/// The whole compilation of `text`, best of `runs`.
fn timed(text: &str, runs: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        compile_as(text, &options()).unwrap();
        best = best.min(start.elapsed().as_secs_f64());
    }
    best
}

/// ns per DIE of version 4 and of version 5 on the same IR (the
/// compilation with the DIEs less the one without), at N and 2N
/// functions; the ratio says whether the transcoder stays linear.
#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn version_4_die_cost() {
    let Some(clang) = tool("clang") else { return };
    let scratch = Scratch::new("dwarf4-cost");
    let triple = if cfg!(target_arch = "x86_64") { X86_64.1 } else { "aarch64-unknown-linux-gnu" };
    for version in [5, 4] {
        let mut per = Vec::new();
        for n in [2_000, 4_000] {
            let c = scratch.write(&format!("f{n}.c"), source(n));
            let ll = scratch.join(format!("f{n}-{version}.ll"));
            let mut command = std::process::Command::new(&clang);
            command.arg(format!("--target={triple}"));
            command.args(["-g", "-O0", "-S", "-emit-llvm", &format!("-gdwarf-{version}")]);
            command.arg(&c).arg("-o").arg(&ll);
            assert!(run_with(&mut command, b"", Duration::from_mins(2)).success());
            let text = std::fs::read_to_string(&ll).unwrap();
            let with = timed(&text, 5);
            let without =
                timed(&text.replace("emissionKind: FullDebug", "emissionKind: NoDebug"), 5);
            let object = compile_as(&text, &options()).unwrap();
            let dies = 9 * n + 2;
            let ns = (with - without).max(0.0) * 1e9 / dies as f64;
            per.push(ns);
            eprintln!(
                "DWARF {version} n={n}: {:.2} ms with DIEs, {:.2} ms without; {ns:.0} ns per DIE; \
                 object {} bytes",
                with * 1e3,
                without * 1e3,
                object.len()
            );
        }
        eprintln!("DWARF {version}: ns per DIE at 2N / at N = {:.2}", per[1] / per[0].max(1e-9));
    }
}
