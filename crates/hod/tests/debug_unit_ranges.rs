//! WP-305 (`spec/dwarf.md` §5.6, §6.17): the compile unit's code ranges
//! and the line table's sequences are the same runs of functions, as
//! `llc` 19 and 22 write them, when functions without a subprogram
//! (`nodebug`) and functions in another section sit between the debug
//! functions of `.text`: such a function ends the run before it, in the
//! unit's range as in the line table. Checked on AArch64 and x86-64,
//! with and without `-function-sections`, by naming each range and each
//! sequence by the functions it covers (from the symbol table), so that
//! the comparison does not depend on code sizes; every object verifies
//! with `llvm-dwarfdump` 19 and 22.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

/// One function of a shape: its name, its section (`None`: `.text`),
/// whether it has a subprogram, and how many additions its body has
/// (functions of one shape have different sizes).
#[derive(Clone, Copy)]
struct F(&'static str, Option<&'static str>, bool, u32);

/// The shapes measured from `llc` (`spec/landed/dwarf/WP-305.md`).
const SHAPES: [(&str, &[F]); 6] = [
    (
        "between",
        &[
            F("f1", None, true, 1),
            F("f2", Some(".text.mine"), true, 2),
            F("f3", None, true, 3),
            F("nd", None, false, 4),
            F("f4", None, true, 5),
            F("main", None, true, 6),
        ],
    ),
    ("nodebug first", &[F("nd", None, false, 4), F("f1", None, true, 1), F("main", None, true, 2)]),
    ("nodebug last", &[F("f1", None, true, 1), F("main", None, true, 2), F("nd", None, false, 4)]),
    (
        "two nodebug",
        &[
            F("f1", None, true, 1),
            F("nd", None, false, 4),
            F("nd2", None, false, 5),
            F("main", None, true, 2),
        ],
    ),
    (
        "nodebug elsewhere",
        &[F("f1", None, true, 1), F("main", None, true, 2), F("nd", Some(".text.mine"), false, 4)],
    ),
    (
        "other section between",
        &[F("f1", None, true, 1), F("f2", Some(".text.mine"), true, 2), F("f3", None, true, 3)],
    ),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    Aarch64,
    X86_64,
}

/// The `-g -O0` IR of `shape` for `arch`, as clang 22 writes it for
/// `int f(int x) { return x + 1 + ...; }`.
fn module(arch: Arch, shape: &[F]) -> String {
    let (layout, triple, fp) = match arch {
        Arch::Aarch64 => (
            "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128",
            "aarch64-unknown-linux-gnu",
            "non-leaf",
        ),
        Arch::X86_64 => (
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
            "x86_64-unknown-linux-gnu",
            "all",
        ),
    };
    let mut text = format!(
        "source_filename = \"runs.c\"\ntarget datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n\n"
    );
    let mut meta = String::new();
    let mut next = 10;
    for (line, &F(name, section, debug, adds)) in shape.iter().enumerate() {
        let line = line + 1;
        let section = section.map(|s| format!(" section \"{s}\"")).unwrap_or_default();
        let (sp, loc) = if debug {
            let sp = next;
            next += 2;
            let _ = write!(
                meta,
                "!{sp} = distinct !DISubprogram(name: \"{name}\", scope: !1, file: !1, line: {line}, type: !5, scopeLine: {line}, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)\n\
                 !{} = !DILocation(line: {line}, column: 3, scope: !{sp})\n",
                sp + 1
            );
            (format!(" !dbg !{sp}"), format!(", !dbg !{}", sp + 1))
        } else {
            (String::new(), String::new())
        };
        let _ = write!(
            text,
            "define dso_local i32 @{name}(i32 noundef %0) #0{section}{sp} {{\n  %2 = alloca i32, align 4\n  store i32 %0, ptr %2, align 4\n  %v0 = load i32, ptr %2, align 4{loc}\n"
        );
        for i in 0..adds {
            let n = i + 1;
            let _ = write!(
                text,
                "  %v{n} = add nsw i32 %v{i}, {n}{loc}\n  store volatile i32 %v{n}, ptr %2, align 4{loc}\n"
            );
        }
        let _ = write!(text, "  ret i32 %v{adds}{loc}\n}}\n\n");
    }
    let _ = write!(
        text,
        "attributes #0 = {{ noinline nounwind optnone \"frame-pointer\"=\"{fp}\" }}\n\n\
         !llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!2, !3}}\n\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"clang version 22\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"runs.c\", directory: \"/hod/tests\")\n\
         !2 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
         !3 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !5 = !DISubroutineType(types: !6)\n!6 = !{{!8, !8}}\n!7 = !{{}}\n\
         !8 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n{meta}"
    );
    text
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let out = run_with(command, b"", Duration::from_mins(2));
    assert!(out.success(), "{what}: {}", out.stderr_text());
    out.stdout_text()
}

fn hex(s: &str) -> u64 {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap()
}

/// The defined functions of `object`: (section, start, end, name).
fn functions(readelf: &Path, object: &Path) -> Vec<(String, u64, u64, String)> {
    let sections = run_ok(Command::new(readelf).args(["-S", "-W"]).arg(object), "readelf -S");
    let mut names = std::collections::HashMap::new();
    for l in sections.lines() {
        if let Some((index, rest)) =
            l.trim_start().strip_prefix('[').and_then(|r| r.split_once(']'))
            && let (Ok(index), Some(name)) =
                (index.trim().parse::<u32>(), rest.split_whitespace().next())
        {
            names.insert(index.to_string(), name.to_owned());
        }
    }
    let symbols = run_ok(Command::new(readelf).args(["-s", "-W"]).arg(object), "readelf -s");
    symbols
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 8 && f[3] == "FUNC").then(|| {
                let (start, size) = (hex(f[1]), f[2].parse::<u64>().unwrap());
                (names[f[6]].clone(), start, start + size, f[7].to_owned())
            })
        })
        .collect()
}

/// `[lo, hi)` as the functions it covers: `section:f,g`, the section
/// whose functions start at `lo` and end at `hi`.
fn name(funcs: &[(String, u64, u64, String)], lo: u64, hi: u64) -> String {
    let mut found = Vec::new();
    let mut sections: Vec<&String> = funcs.iter().map(|f| &f.0).collect();
    sections.sort();
    sections.dedup();
    for section in sections {
        let mut covered: Vec<_> =
            funcs.iter().filter(|f| &f.0 == section && lo <= f.1 && f.1 < hi).collect();
        covered.sort_by_key(|f| f.1);
        if covered.first().is_some_and(|f| f.1 == lo) && covered.last().is_some_and(|f| f.2 == hi) {
            let names: Vec<&str> = covered.iter().map(|f| f.3.as_str()).collect();
            found.push(format!("{section}:{}", names.join(",")));
        }
    }
    assert_eq!(found.len(), 1, "[{lo:#x}, {hi:#x}) in {funcs:?}: {found:?}");
    found.pop().unwrap()
}

/// What an object says of its runs: whether the unit has
/// `DW_AT_ranges`, its ranges and the line sequences, each sorted.
#[derive(Debug, PartialEq, Eq)]
struct Runs {
    ranges: bool,
    unit: Vec<String>,
    sequences: Vec<String>,
}

fn runs(dwarfdumps: &[PathBuf], readelf: &Path, object: &Path) -> Runs {
    for d in dwarfdumps {
        let verify = run_ok(Command::new(d).arg("--verify").arg(object), "verify");
        assert!(verify.contains("No errors."), "{}: {} {verify}", object.display(), d.display());
    }
    let funcs = functions(readelf, object);
    let info = run_ok(Command::new(&dwarfdumps[0]).arg("--debug-info").arg(object), "info");
    let unit = info.split("DW_TAG_subprogram").next().unwrap();
    let ranges = unit.contains("DW_AT_ranges");
    let mut spans = Vec::new();
    if ranges {
        let list = unit.split("DW_AT_ranges").nth(1).unwrap();
        for l in list.lines().skip(1) {
            let Some(r) = l.trim().strip_prefix('[') else { break };
            let (lo, hi) = r.split_once(", ").unwrap();
            spans.push(name(&funcs, hex(lo), hex(hi.trim_end_matches(')'))));
        }
    } else {
        let attr = |a: &str| {
            let l = unit.lines().find(|l| l.contains(a)).unwrap_or_else(|| panic!("{a}: {unit}"));
            hex(l.split('(').nth(1).unwrap().trim_end_matches(')'))
        };
        spans.push(name(&funcs, attr("DW_AT_low_pc"), attr("DW_AT_high_pc")));
    }
    let line = run_ok(Command::new(&dwarfdumps[0]).arg("--debug-line").arg(object), "line");
    let mut sequences = Vec::new();
    let mut start = None;
    for l in line.lines().filter(|l| l.starts_with("0x")) {
        let address = hex(l.split_whitespace().next().unwrap());
        let lo = *start.get_or_insert(address);
        if l.contains("end_sequence") {
            sequences.push(name(&funcs, lo, address));
            start = None;
        }
    }
    spans.sort();
    sequences.sort();
    Runs { ranges, unit: spans, sequences }
}

/// Every shape, both targets, with and without `-function-sections`:
/// hod's unit ranges and line sequences are `llc` 19's and 22's, and
/// hod's ranges are its sequences.
#[test]
fn unit_ranges_are_the_runs_llc_writes() {
    let llcs: Vec<PathBuf> = [19, 22].iter().filter_map(|m| tool(&format!("llc-{m}"))).collect();
    let dwarfdumps: Vec<PathBuf> =
        [22, 19].iter().filter_map(|m| tool(&format!("llvm-dwarfdump-{m}"))).collect();
    let readelf = tool("llvm-readelf");
    if llcs.len() < 2 || dwarfdumps.len() < 2 || readelf.is_none() {
        assert!(!tools_required(), "llc, llvm-dwarfdump 19 and 22 and llvm-readelf are required");
        eprintln!("skipped: tools missing");
        return;
    }
    let readelf = readelf.unwrap();
    let scratch = Scratch::new("debug-unit-ranges");
    for (label, shape) in SHAPES {
        for arch in [Arch::Aarch64, Arch::X86_64] {
            for function_sections in [false, true] {
                let tag = format!("{}-{arch:?}-{function_sections}", label.replace(' ', "-"));
                let text = module(arch, shape);
                let input = scratch.write(&format!("{tag}.ll"), &text);
                let options = hod::Options {
                    output_kind: OutputKind::Object,
                    function_sections,
                    ..hod::Options::default()
                };
                let bytes = compile_with(
                    text.as_bytes(),
                    &options,
                    &Settings::default(),
                    &mut Report::default(),
                )
                .unwrap_or_else(|e| panic!("{tag}: hod: {e}"));
                let hod =
                    runs(&dwarfdumps, &readelf, &scratch.write(&format!("{tag}.hod.o"), &bytes));
                assert_eq!(hod.unit, hod.sequences, "{tag}: hod's ranges are its sequences");
                for llc in &llcs {
                    let object = scratch.join(format!("{tag}.llc.o"));
                    let mut command = Command::new(llc);
                    command.args(["-O0", "-filetype=obj"]);
                    if arch == Arch::Aarch64 {
                        command.args(["-global-isel=0", "-fast-isel=0"]);
                    }
                    if function_sections {
                        command.arg("-function-sections");
                    }
                    run_ok(command.arg(&input).arg("-o").arg(&object), "llc");
                    let want = runs(&dwarfdumps, &readelf, &object);
                    assert_eq!(hod, want, "{tag}: hod against {}", llc.display());
                }
            }
        }
    }
}
