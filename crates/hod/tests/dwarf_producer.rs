//! WP-221 part A (`spec/dwarf.md` §6.30): the command line a front end
//! records (`-grecord-command-line`, `-grecord-gcc-switches`) as the
//! compile unit's `flags:` is in `DW_AT_producer`, after the producer
//! and a space, as `llc` 19 and 22 write it: in the ordinary unit
//! (DWARF 5 and 4), in the split unit, and in the skeleton that stands
//! alone; on both targets; from the text and from the bitcode.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

/// clang 22's `-g -O0` IR of a small C function, its producer and paths
/// shortened, with a recorded command line.
const TINY: &str = r#"source_filename = "tiny.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

define dso_local i32 @g(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
  %3 = load i32, ptr %2, align 4, !dbg !19
  ret i32 %3, !dbg !22
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang version 22", isOptimized: false, flags: "/usr/bin/clang -g -grecord-command-line -O0 tiny.c", runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tiny.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!16 = !DILocation(line: 1, column: 11, scope: !10)
!19 = !DILocation(line: 2, column: 10, scope: !10)
!22 = !DILocation(line: 2, column: 3, scope: !10)
"#;

const FLAGS: &str = r#"flags: "/usr/bin/clang -g -grecord-command-line -O0 tiny.c", "#;
const PRODUCER: &str = r#"producer: "clang version 22""#;

/// `text` for x86-64.
fn x86_64(text: &str) -> String {
    text.replace(
        "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128",
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
    )
    .replace("aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu")
    .replace(r#" "frame-pointer"="non-leaf""#, r#" "frame-pointer"="all""#)
}

/// One shape of the unit: how the IR is changed, and whether it is
/// split (the `.dwo` sections in the object, `-split-dwarf-file` alone).
struct Shape {
    name: &'static str,
    edit: fn(&str) -> String,
    split: bool,
}

const SHAPES: [Shape; 4] = [
    Shape { name: "dwarf 5", edit: str::to_owned, split: false },
    Shape {
        name: "dwarf 4",
        edit: |t| t.replace(r#""Dwarf Version", i32 5"#, r#""Dwarf Version", i32 4"#),
        split: false,
    },
    Shape {
        name: "split unit",
        edit: |t| {
            t.replace("splitDebugInlining", r#"splitDebugFilename: "tiny.dwo", splitDebugInlining"#)
        },
        split: true,
    },
    // A line-tables-only unit has no split unit: its skeleton stands
    // alone and carries the producer (§6.15).
    Shape {
        name: "skeleton alone",
        edit: |t| {
            t.replace("emissionKind: FullDebug", "emissionKind: LineTablesOnly")
                .replace("    #dbg_declare(ptr %2, !15, !DIExpression(), !16)\n", "")
        },
        split: true,
    },
];

/// The producers: with flags, an empty producer with flags (a leading
/// space), no flags, neither.
type Producer = (&'static str, fn(&str) -> String);
const PRODUCERS: [Producer; 4] = [
    ("producer and flags", str::to_owned),
    ("empty producer, flags", |t| t.replace(PRODUCER, r#"producer: """#)),
    ("producer alone", |t| t.replace(FLAGS, "")),
    ("neither", |t| t.replace(FLAGS, "").replace(PRODUCER, r#"producer: """#)),
];

fn options(split: bool) -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        split_dwarf_file: split.then(|| "tiny.dwo".into()),
        ..hod::Options::default()
    }
}

fn hod_object(input: &[u8], split: bool) -> Vec<u8> {
    compile_with(input, &options(split), &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let out = run_with(command, b"", Duration::from_mins(2));
    assert!(out.success(), "{what}: {}", out.stderr_text());
    out.stdout_text()
}

/// The unit headings and `DW_AT_producer` lines of `object`, in order;
/// asserts that the object verifies.
fn producers(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let verify = run_ok(Command::new(dwarfdump).arg("--verify").arg(object), "verify");
    assert!(verify.contains("No errors."), "{}: {verify}", object.display());
    let info = run_ok(Command::new(dwarfdump).arg("--debug-info").arg(object), "dwarfdump");
    info.lines()
        .filter(|l| l.contains("DW_AT_producer") || l.ends_with("contents:"))
        .map(|l| l.trim().to_owned())
        .collect()
}

fn llc_object(llc: &Path, scratch: &Scratch, name: &str, text: &str, split: bool) -> Vec<String> {
    let input = scratch.write(&format!("{name}.ll"), text);
    let object = scratch.join(format!("{name}.llc.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-filetype=obj"]);
    if split {
        command.arg("-split-dwarf-file=tiny.dwo");
    }
    command.arg(&input).arg("-o").arg(&object);
    run_ok(&mut command, "llc");
    producers(&tool("llvm-dwarfdump").expect("dwarfdump"), &object)
}

/// Every shape, producer and target from hand-written IR: hod's
/// producers are exactly what `llc` 19 and 22 write, and the recorded
/// command line is in them.
#[test]
fn producer_and_flags_as_llc_writes_them() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dwarf-producer");
    let llcs: Vec<_> = ["llc-19", "llc-22"].into_iter().filter_map(tool).collect();
    let mut compared = 0;
    for (target, base) in [("aarch64", TINY.to_owned()), ("x86_64", x86_64(TINY))] {
        for shape in &SHAPES {
            for (producer, edit) in &PRODUCERS {
                let text = edit(&(shape.edit)(&base));
                let name = format!("{target}-{}-{producer}", shape.name).replace([' ', ','], "_");
                let object =
                    scratch.write(&format!("{name}.o"), hod_object(text.as_bytes(), shape.split));
                let ours = producers(&dwarfdump, &object);
                let what = format!("{target}, {}, {producer}", shape.name);
                let lines: Vec<_> = ours.iter().filter(|l| l.contains("DW_AT_producer")).collect();
                assert_eq!(lines.len(), 1, "{what}: {ours:?}");
                let expected = match *producer {
                    "producer and flags" => {
                        "(\"clang version 22 /usr/bin/clang -g -grecord-command-line -O0 tiny.c\")"
                    }
                    "empty producer, flags" => {
                        "(\" /usr/bin/clang -g -grecord-command-line -O0 tiny.c\")"
                    }
                    "producer alone" => "(\"clang version 22\")",
                    _ => "(\"\")",
                };
                assert!(lines[0].ends_with(expected), "{what}: {}", lines[0]);
                for llc in &llcs {
                    let theirs = llc_object(llc, &scratch, &name, &text, shape.split);
                    assert_eq!(ours, theirs, "{what} against {}", llc.display());
                    compared += 1;
                }
            }
        }
    }
    assert!(llcs.is_empty() || compared == 2 * SHAPES.len() * PRODUCERS.len() * llcs.len());
}

/// The bitcode of the same module (`llvm-as` 19 and 22) gives the same
/// object as its text.
#[test]
fn bitcode_reads_the_flags_too() {
    let scratch = Scratch::new("dwarf-producer-bc");
    for (target, text) in [("aarch64", TINY.to_owned()), ("x86_64", x86_64(TINY))] {
        let from_text = hod_object(text.as_bytes(), false);
        for as_ in ["llvm-as-19", "llvm-as-22"] {
            let Some(llvm_as) = tool(as_) else { continue };
            let input = scratch.write(&format!("{target}.ll"), &text);
            let bc = scratch.join(format!("{target}-{as_}.bc"));
            run_ok(Command::new(&llvm_as).arg(&input).arg("-o").arg(&bc), "llvm-as");
            let from_bc = hod_object(&std::fs::read(&bc).unwrap(), false);
            assert!(from_bc == from_text, "{target}, {as_}: the bitcode's object differs");
        }
    }
}

/// clang 19 and 22 with `-grecord-command-line` (C and C++), both
/// targets: hod's producer is `llc`'s of the same major.
#[test]
fn clang_recorded_command_line() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dwarf-producer-clang");
    let c = scratch.write("rec.c", "int f(int x) { return x + 1; }\n");
    let cxx = scratch.write(
        "rec.cpp",
        "template <class T> T id(T t) { return t; }\nint g() { return id(3); }\n",
    );
    for major in ["19", "22"] {
        let (Some(clang), Some(llc)) =
            (tool(&format!("clang-{major}")), tool(&format!("llc-{major}")))
        else {
            continue;
        };
        for triple in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"] {
            for (source, split) in [(&c, false), (&cxx, false), (&cxx, true)] {
                let name = format!(
                    "{major}-{triple}-{}-{split}",
                    source.extension().unwrap().to_string_lossy()
                );
                let ll = scratch.join(format!("{name}.ll"));
                let mut command = Command::new(&clang);
                command.arg(format!("--target={triple}"));
                command.args(["-O0", "-g", "-grecord-command-line", "-S", "-emit-llvm"]);
                if split {
                    command.arg("-gsplit-dwarf");
                }
                command.arg(source).arg("-o").arg(&ll);
                run_ok(&mut command, "clang");
                let mut text = std::fs::read_to_string(&ll).unwrap();
                if split {
                    // The `.dwo` name the options give.
                    let key = "splitDebugFilename: \"";
                    let at = text.find(key).unwrap() + key.len();
                    let end = at + text[at..].find('"').unwrap();
                    text.replace_range(at..end, "tiny.dwo");
                }
                let object =
                    scratch.write(&format!("{name}.o"), hod_object(text.as_bytes(), split));
                let ours = producers(&dwarfdump, &object);
                assert!(
                    ours.iter().any(|l| l.contains("-grecord-command-line")),
                    "{name}: {ours:?}"
                );
                let theirs = llc_object(&llc, &scratch, &name, &text, split);
                assert_eq!(ours, theirs, "{name}");
            }
        }
    }
}
