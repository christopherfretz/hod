//! The line rows of a `zext`/`sext` of a fused `icmp` on x86-64 (WP-163,
//! `spec/dwarf.md` §6.17 "Fused compares"): `llc` gives the `setcc` the
//! compare's location and only the `movzx` (and `neg`) the cast's, with
//! any consumer, a store included; hod does the same.
//!
//! What stays different, and is classified, not fixed: a `zext i1` to
//! `i8` is no instruction for `llc` (`setcc` writes the byte), and when a
//! store follows `llc` folds the `setcc` into it (`setl (%rdx)`, at the
//! store's line), so the cast's line has no row in `llc`'s table; hod
//! widens the byte (`movzx`, at the cast's line) and stores the register.
#![cfg(target_os = "linux")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel};
use hod_testutil::{Scratch, run_with, tool};

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

fn llc(dir: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("llc{major}.ll"), text);
    let output = dir.join(format!("llc{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-fast-isel=0", "-mtriple=x86_64-unknown-linux-gnu"])
        .args(["-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// Each function: a compare at line 11, its cast at 12, the consumer at
/// 13, each function its own subprogram and locations.
fn ir() -> String {
    let mut text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    );
    let bodies = [
        ("zext_store32", "%z = zext i1 %c to i32, !dbg !L2\n  store i32 %z, ptr %p, !dbg !L3"),
        ("sext_store32", "%z = sext i1 %c to i32, !dbg !L2\n  store i32 %z, ptr %p, !dbg !L3"),
        (
            "zext_add",
            "%z = zext i1 %c to i32, !dbg !L2\n  %y = add i32 %z, %a, !dbg !L3\n  \
             store i32 %y, ptr %p, !dbg !L3",
        ),
        ("zext_store8", "%z = zext i1 %c to i8, !dbg !L2\n  store i8 %z, ptr %p, !dbg !L3"),
    ];
    let mut metadata = String::new();
    for (k, (name, body)) in bodies.iter().enumerate() {
        let sp = 20 + 10 * k;
        let mut function = format!(
            "define void @{name}(i32 %a, i32 %b, ptr %p) !dbg !{sp} {{\n  \
             %c = icmp slt i32 %a, %b, !dbg !L1\n  {body}\n  ret void, !dbg !L4\n}}\n"
        );
        for line in 1..=4 {
            function = function.replace(&format!("!L{line}"), &format!("!{}", sp + line));
            let _ = writeln!(
                metadata,
                "!{} = !DILocation(line: {}, column: 3, scope: !{sp})",
                sp + line,
                10 + line
            );
        }
        text.push_str(&function);
        let _ = writeln!(
            metadata,
            "!{sp} = distinct !DISubprogram(name: \"{name}\", scope: !1, file: !1, line: 10, \
             type: !3, scopeLine: 10, spFlags: DISPFlagDefinition, unit: !0)"
        );
    }
    text.push_str(
        r#"
!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!5, !6}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "casts.c", directory: "/casts")
!3 = !DISubroutineType(types: !4)
!4 = !{}
!5 = !{i32 7, !"Dwarf Version", i32 5}
!6 = !{i32 2, !"Debug Info Version", i32 3}
"#,
    );
    text.push_str(&metadata);
    text
}

/// `(address, mnemonic)` of each instruction, by function name.
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

/// The line table's rows: (address, line), sequence ends as `u64::MAX`.
fn rows(object: &Path) -> Option<Vec<(u64, u64)>> {
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
        rows.push((at, if line.contains("end_sequence") { u64::MAX } else { number }));
    }
    Some(rows)
}

/// The line of the instruction at `at`: the last row at or before it.
fn line_at(rows: &[(u64, u64)], at: u64) -> u64 {
    rows.iter().rev().find(|r| r.0 <= at).map_or(0, |r| r.1)
}

/// Per function: the line of its `setcc` (`None` when folded into a
/// store), and the lines its instructions are at.
fn shape(object: &Path, function: &str) -> Option<(Option<u64>, Vec<u64>)> {
    let insns = instructions(object)?;
    let rows = rows(object)?;
    let mine: Vec<&(String, u64, String)> = insns.iter().filter(|i| i.0 == function).collect();
    assert!(!mine.is_empty(), "{function} in {}", object.display());
    let setcc = mine.iter().find(|i| i.2.starts_with("set")).map(|i| line_at(&rows, i.1));
    let mut lines: Vec<u64> = mine.iter().map(|i| line_at(&rows, i.1)).collect();
    lines.dedup();
    Some((setcc, lines))
}

#[test]
fn the_setcc_of_a_fused_cast_is_at_the_compare() {
    let dir = Scratch::new("zext-lines");
    let text = ir();
    let object = dir.write("hod.o", hod(&text));
    for function in ["zext_store32", "sext_store32", "zext_add", "zext_store8"] {
        let Some((setcc, lines)) = shape(&object, function) else { return };
        assert_eq!(setcc, Some(11), "hod {function}: the setcc's line ({lines:?})");
        assert!(lines.contains(&12), "hod {function}: the cast's movzx at its line ({lines:?})");
        for major in [19, 22] {
            let Some(reference) = llc(&dir, major, &text) else { continue };
            let Some((theirs, their_lines)) = shape(&reference, function) else { continue };
            if function == "zext_store8" {
                // The classified difference: `setl (%rdx)` at the store's
                // line, no row at the cast's.
                assert_eq!(theirs, Some(13), "llc {major} {function} ({their_lines:?})");
                assert!(!their_lines.contains(&12), "llc {major} {function} ({their_lines:?})");
            } else {
                assert_eq!(theirs, setcc, "llc {major} {function}: the setcc's line");
                assert!(their_lines.contains(&12), "llc {major} {function} ({their_lines:?})");
            }
        }
    }
}
