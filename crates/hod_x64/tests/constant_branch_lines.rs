//! The line row of a branch on a constant on x86-64 (WP-252,
//! `spec/dwarf.md` §5.10): a `condbr` or `switch` on a constant folded to
//! the block emitted next writes a `nop` at its location when its line
//! would otherwise have no row, and nothing else changes. Compared with
//! the (function, line) sets of `llc -O0` (each of its three selectors,
//! LLVM 19 and 22, keeps code on such a line).
#![cfg(target_os = "linux")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel};
use hod_testutil::{Scratch, run_with, tool};

/// Each function: its body (`!Lk` is line 10 + k, column 3; `!Ck` the
/// same line at column 9), the lines of its branches on a constant that
/// get a `nop`, and the lines `llc` and hod both have code at.
const FUNCTIONS: &[(&str, &str, &[u64])] = &[
    (
        "cond_true",
        "store i32 1, ptr %p, !dbg !L1
  br i1 true, label %a, label %b, !dbg !L2
a:
  store i32 2, ptr %p, !dbg !L3
  br label %b, !dbg !L3
b:
  ret void, !dbg !L4",
        &[12],
    ),
    (
        // Seed 110478's shape: an unused compare and its extension.
        "cond_false_unused",
        "store i32 1, ptr %p, !dbg !L1
  %c = icmp eq ptr %p, null, !dbg !L2
  %z = zext i1 %c to i32, !dbg !L2
  br i1 false, label %a, label %b, !dbg !L2
b:
  store i32 3, ptr %p, !dbg !L3
  ret void, !dbg !L3
a:
  store i32 2, ptr %p, !dbg !L4
  ret void, !dbg !L4",
        &[12],
    ),
    (
        "switch_const",
        "store i32 1, ptr %p, !dbg !L1
  switch i32 2, label %d [
    i32 2, label %c
  ], !dbg !L2
c:
  store i32 2, ptr %p, !dbg !L3
  br label %d, !dbg !L3
d:
  ret void, !dbg !L4",
        &[12],
    ),
    (
        "two_in_a_row",
        "store i32 1, ptr %p, !dbg !L1
  br i1 true, label %a, label %c, !dbg !L2
a:
  br i1 true, label %b, label %c, !dbg !L3
b:
  store i32 2, ptr %p, !dbg !L4
  br label %c, !dbg !L4
c:
  ret void, !dbg !L5",
        &[12, 13],
    ),
    (
        "in_loop",
        "store i32 0, ptr %p, !dbg !L1
  br label %h, !dbg !L1
h:
  %n = load i32, ptr %p, !dbg !L2
  %k = icmp slt i32 %n, 3, !dbg !L2
  br i1 %k, label %i, label %x, !dbg !L2
i:
  br i1 true, label %body, label %x, !dbg !L3
body:
  %m = add i32 %n, 1, !dbg !L4
  store i32 %m, ptr %p, !dbg !L4
  br label %h, !dbg !L4
x:
  ret void, !dbg !L5",
        &[13],
    ),
    (
        // The load the unused compare reads is code of the line, just
        // before the branch: no `nop`.
        "same_line",
        "%v = load i32, ptr %p, !dbg !L1
  %c = icmp eq i32 %v, 0, !dbg !L1
  br i1 true, label %a, label %b, !dbg !C1
a:
  store i32 2, ptr %p, !dbg !L2
  br label %b, !dbg !L2
b:
  ret void, !dbg !L3",
        &[],
    ),
    (
        // The chosen block is not next: a `jmp`, no `nop`.
        "not_next",
        "store i32 1, ptr %p, !dbg !L1
  br i1 true, label %b, label %a, !dbg !L2
a:
  store i32 2, ptr %p, !dbg !L3
  br label %b, !dbg !L3
b:
  ret void, !dbg !L4",
        &[],
    ),
];

/// The module: one subprogram per function; without `debug`, no `!dbg`
/// at all; `kind` is the unit's `emissionKind`.
fn ir(debug: bool, kind: &str) -> String {
    let mut text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    );
    let mut metadata = String::new();
    for (k, (name, body, _)) in FUNCTIONS.iter().enumerate() {
        let sp = 20 + 20 * k;
        let mut function = format!("define void @{name}(ptr %p) !dbg !{sp} {{\n  {body}\n}}\n");
        for line in 1..=5 {
            for (tag, offset, column) in [("L", 0, 3), ("C", 10, 9)] {
                let id = sp + line + offset;
                function = function.replace(&format!("!{tag}{line}"), &format!("!{id}"));
                let _ = writeln!(
                    metadata,
                    "!{id} = !DILocation(line: {}, column: {column}, scope: !{sp})",
                    10 + line
                );
            }
        }
        text.push_str(&function);
        let _ = writeln!(
            metadata,
            "!{sp} = distinct !DISubprogram(name: \"{name}\", scope: !1, file: !1, line: 10, \
             type: !3, scopeLine: 10, spFlags: DISPFlagDefinition, unit: !0)"
        );
    }
    let _ = write!(
        text,
        r#"
!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: {kind})
!1 = !DIFile(filename: "branches.c", directory: "/branches")
!3 = !DISubroutineType(types: !4)
!4 = !{{}}
!5 = !{{i32 7, !"Dwarf Version", i32 5}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
"#
    );
    text.push_str(&metadata);
    if debug {
        return text;
    }
    // Without debug information: the attachments and the metadata go.
    let mut plain = String::new();
    for line in text.lines() {
        if line.starts_with('!') {
            continue;
        }
        let line = match line.find(", !dbg !") {
            Some(at) => &line[..at],
            None => line,
        };
        let line = match line.find(" !dbg !") {
            Some(at) => format!("{} {{", &line[..at]),
            None => line.to_owned(),
        };
        plain.push_str(&line);
        plain.push('\n');
    }
    plain
}

fn hod(text: &str) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &parsed.text, |_, parsed| {
        bodies.push(
            parsed.body.unwrap_or_else(|| panic!("unsupported {:?}", parsed.unsupported)).clone(),
        );
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    hod_x64::compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("hod: {e}"))
}

/// `llc -O0` of `major` with `selector`'s flags.
fn llc(dir: &Scratch, major: u32, selector: &[&str], text: &str) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let tag = selector.join("").replace(['-', '='], "");
    let input = dir.write(&format!("llc{major}.ll"), text);
    let output = dir.join(format!("llc{major}{tag}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-mtriple=x86_64-unknown-linux-gnu"])
        .args(selector)
        .args(["-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} {selector:?} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// `(function, address, mnemonic)` of each instruction.
fn instructions(object: &Path) -> Option<Vec<(String, u64, String)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn", "--x86-asm-syntax=intel"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut function = String::new();
    let mut out = Vec::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.split_once(" <").and_then(|(_, r)| r.strip_suffix(">:")) {
            name.clone_into(&mut function);
            continue;
        }
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        let Ok(at) = u64::from_str_radix(at.trim(), 16) else { continue };
        let mnemonic = rest.split_whitespace().next().unwrap_or("").to_owned();
        if !mnemonic.is_empty() {
            out.push((function.clone(), at, mnemonic));
        }
    }
    Some(out)
}

/// The line table's rows: (address, line, `is_stmt`), sequence ends as
/// line `u64::MAX`.
fn rows(object: &Path) -> Option<Vec<(u64, u64, bool)>> {
    let dwarfdump = tool("llvm-dwarfdump")?;
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-line").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-dwarfdump: {}", outcome.stderr_text());
    let mut rows = Vec::new();
    for line in outcome.stdout_text().lines() {
        let mut fields = line.split_whitespace();
        let Some(at) = fields.next().and_then(|a| a.strip_prefix("0x")) else { continue };
        let (Ok(at), Some(Ok(number))) =
            (u64::from_str_radix(at, 16), fields.next().map(str::parse::<u64>))
        else {
            continue;
        };
        let end = line.contains("end_sequence");
        rows.push((at, if end { u64::MAX } else { number }, line.contains("is_stmt")));
    }
    Some(rows)
}

/// The line of the instruction at `at`: the last row at or before it.
fn line_at(rows: &[(u64, u64, bool)], at: u64) -> u64 {
    rows.iter().rev().find(|r| r.0 <= at).map_or(0, |r| r.1)
}

/// Per function: the source lines after the scope line its instructions
/// are at (the scope line is P3 of §5.7 where `llc` has no frame), and
/// the lines of its `nop`s that start a statement row.
fn shape(object: &Path, function: &str) -> Option<(Vec<u64>, Vec<u64>)> {
    let insns = instructions(object)?;
    let rows = rows(object)?;
    let mine: Vec<&(String, u64, String)> = insns.iter().filter(|i| i.0 == function).collect();
    assert!(!mine.is_empty(), "{function} in {}", object.display());
    let mut lines: Vec<u64> =
        mine.iter().map(|i| line_at(&rows, i.1)).filter(|&l| l > 10 && l != u64::MAX).collect();
    lines.sort_unstable();
    lines.dedup();
    let nops = mine
        .iter()
        .filter(|i| i.2 == "nop")
        .filter_map(|i| rows.iter().find(|r| r.0 == i.1 && r.2 && r.1 != u64::MAX).map(|r| r.1))
        .collect();
    Some((lines, nops))
}

/// The `nop`s of `function`'s code, not counting the padding after it.
fn body_nops(insns: &[(String, u64, String)], function: &str) -> usize {
    let mine: Vec<&str> = insns.iter().filter(|i| i.0 == function).map(|i| i.2.as_str()).collect();
    let end = mine.iter().rposition(|m| !m.starts_with("nop") && *m != "int3").map_or(0, |e| e + 1);
    mine[..end].iter().filter(|m| **m == "nop").count()
}

const SELECTORS: [&[&str]; 3] = [
    &["-global-isel=0", "-fast-isel=0"],
    &["-global-isel=0", "-fast-isel=1"],
    &["-global-isel=1", "-global-isel-abort=2"],
];

#[test]
fn a_branch_on_a_constant_keeps_its_line() {
    let dir = Scratch::new("constant-branch-lines-x64");
    let text = ir(true, "FullDebug");
    let object = dir.write("hod.o", hod(&text));
    for (function, _, folded) in FUNCTIONS {
        let Some((lines, nops)) = shape(&object, function) else { return };
        assert_eq!(nops, *folded, "hod {function}: the nops' rows ({lines:?})");
        for major in [19, 22] {
            for (k, selector) in SELECTORS.iter().enumerate() {
                let Some(reference) = llc(&dir, major, selector, &text) else { continue };
                let Some((theirs, _)) = shape(&reference, function) else { continue };
                if k == 0 {
                    // The oracle (SelectionDAG): the same (function, line) set.
                    assert_eq!(lines, theirs, "{function}: hod against llc {major}");
                }
                for line in *folded {
                    assert!(theirs.contains(line), "llc {major} {selector:?} {function} {line}");
                }
            }
        }
    }
}

#[test]
fn no_nop_without_line_rows() {
    let dir = Scratch::new("constant-branch-lines-x64-plain");
    for (name, text) in [("plain", ir(false, "FullDebug")), ("nodebug", ir(true, "NoDebug"))] {
        let object = dir.write(&format!("{name}.o"), hod(&text));
        let Some(insns) = instructions(&object) else { return };
        for (function, _, _) in FUNCTIONS {
            assert_eq!(body_nops(&insns, function), 0, "{name}: a nop in {function}");
        }
    }
    // The code is that of the module with line rows less its `nop`s.
    let with = dir.write("with.o", hod(&ir(true, "FullDebug")));
    let without = dir.write("without.o", hod(&ir(false, "FullDebug")));
    let (Some(a), Some(b)) = (instructions(&with), instructions(&without)) else { return };
    for (function, _, folded) in FUNCTIONS {
        assert_eq!(body_nops(&a, function), folded.len(), "{function}");
    }
    let code = |v: Vec<(String, u64, String)>| -> Vec<(String, String)> {
        v.into_iter().filter(|i| !i.2.starts_with("nop")).map(|i| (i.0, i.2)).collect()
    };
    assert_eq!(code(a), code(b));
}
