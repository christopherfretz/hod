//! The line rows of fused compares on x86-64 (WP-157, `spec/dwarf.md`
//! §5.8, §6.17): a compare fused into its consumer (a `condbr` on an
//! `icmp`, an `fcmp` of `double` or of `x86_fp80`; a `select` of class
//! `Gpr` and a `zext` on an `icmp`; a `select` of `x86_fp80` on an
//! `x86_fp80` `fcmp`) keeps the compare's location for the compare's
//! instruction, and the consumer's instruction (`jcc`, `cmovcc`,
//! `movzx`) has the consumer's, as in `llc`'s x86-64 program.
//!
//! The function is a chain of fused branches whose `jcc`s the shrink
//! pass relaxes to `rel8` (`spec/codegen.md` §5.6.15), so every row after
//! the first branch has moved: each row must still start an instruction,
//! and the instructions of each row must be the ones of its line. The
//! same check holds on `llc`'s object of the same IR (LLVM 19 and 22,
//! when installed), and hod's set of lines is `llc`'s plus the scope
//! line of the start row (P3, `spec/dwarf.md` §5.7).
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel};
use hod_testutil::{Scratch, run_with, tool};

/// The header of a `.ll` module for x86-64 Linux.
fn ll_header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// hod's object of `.ll` text, position-independent as every x86-64
/// test links it.
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

/// `llc -O0 -fast-isel=0 -relocation-model=pic` of LLVM `major` for
/// x86-64 of `text`; `None` without that `llc`.
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

/// The function: line `10` is its scope line; each pair is (the
/// compare's line, the consumer's line).
fn ir() -> String {
    let mut text = ll_header();
    text.push_str(
        r#"
define i32 @chain(i32 %a, i32 %b, double %x, double %y, x86_fp80 %p, x86_fp80 %q) !dbg !10 {
  %c1 = icmp slt i32 %a, %b, !dbg !11
  br i1 %c1, label %b1, label %b2, !dbg !12
b1:
  %c2 = fcmp olt double %x, %y, !dbg !13
  br i1 %c2, label %b2, label %b3, !dbg !14
b2:
  %c3 = fcmp une double %x, %y, !dbg !15
  br i1 %c3, label %b3, label %b4, !dbg !16
b3:
  %c4 = fcmp olt x86_fp80 %p, %q, !dbg !17
  br i1 %c4, label %b4, label %b5, !dbg !18
b5:
  ret i32 9, !dbg !28
b4:
  %c5 = icmp ugt i32 %a, 7, !dbg !19
  %s = select i1 %c5, i32 %a, i32 %b, !dbg !20
  %c6 = icmp eq i32 %s, %b, !dbg !21
  %z = zext i1 %c6 to i32, !dbg !22
  %c7 = fcmp ueq x86_fp80 %p, %q, !dbg !23
  %w = select i1 %c7, x86_fp80 %p, x86_fp80 %q, !dbg !24
  %t = fptosi x86_fp80 %w to i32, !dbg !25
  %r = add i32 %z, %t, !dbg !26
  ret i32 %r, !dbg !27
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!5, !6}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "fused.c", directory: "/fused")
!3 = !DISubroutineType(types: !4)
!4 = !{}
!5 = !{i32 7, !"Dwarf Version", i32 5}
!6 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "chain", scope: !1, file: !1, line: 10, type: !3, scopeLine: 10, spFlags: DISPFlagDefinition, unit: !0)
"#,
    );
    for line in 11..=28 {
        let _ = writeln!(text, "!{line} = !DILocation(line: {line}, column: 3, scope: !10)");
    }
    text
}

/// What the consumer's row holds: a conditional jump, a conditional
/// move, or the `movzx` of a `zext`.
#[derive(Clone, Copy, Debug)]
enum Consumer {
    Jcc,
    Cmov,
    Movzx,
}

/// (the compare's line, the consumer's line, the consumer).
const PAIRS: [(u64, u64, Consumer); 7] = [
    (11, 12, Consumer::Jcc),
    (13, 14, Consumer::Jcc),
    (15, 16, Consumer::Jcc),
    (17, 18, Consumer::Jcc),
    (19, 20, Consumer::Cmov),
    (21, 22, Consumer::Movzx),
    (23, 24, Consumer::Cmov),
];

#[derive(Debug)]
struct Insn {
    at: u64,
    len: u64,
    first: u8,
    mnemonic: String,
}

/// The instructions of the object's `.text` (one function).
fn instructions(object: &Path) -> Option<Vec<Insn>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--x86-asm-syntax=intel"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut insns: Vec<Insn> = Vec::new();
    for line in outcome.stdout_text().lines() {
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        let Ok(at) = u64::from_str_radix(at.trim(), 16) else { continue };
        let (bytes, text) = rest.split_once('\t').unwrap_or((rest, ""));
        let bytes: Vec<u8> =
            bytes.split_whitespace().filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        let mnemonic = text.split_whitespace().next().unwrap_or("").to_owned();
        match insns.last_mut() {
            Some(last) if mnemonic.is_empty() => last.len += bytes.len() as u64,
            _ if bytes.is_empty() => {}
            _ => insns.push(Insn { at, len: bytes.len() as u64, first: bytes[0], mnemonic }),
        }
    }
    Some(insns)
}

/// The rows of the object's line table: (address, line), the end of the
/// sequence as line `u64::MAX`.
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

/// The mnemonics of the instructions in the rows of `line`; every row
/// must start an instruction.
fn at_line<'i>(name: &str, insns: &'i [Insn], rows: &[(u64, u64)], line: u64) -> Vec<&'i Insn> {
    let mut found = Vec::new();
    for (k, &(start, l)) in rows.iter().enumerate() {
        if l != line {
            continue;
        }
        let end = rows.get(k + 1).map_or(u64::MAX, |r| r.0);
        assert!(
            start == end || insns.iter().any(|i| i.at == start),
            "{name}: the row of line {line} at {start:#x} does not start an instruction"
        );
        found.extend(insns.iter().filter(|i| i.at >= start && i.at < end));
    }
    found
}

fn is_compare(m: &str) -> bool {
    ["cmp", "sub", "test", "ucomisd", "fucom"].iter().any(|p| m.starts_with(p))
}

fn is_consumer(m: &str) -> bool {
    m.starts_with('j') || m.starts_with("cmov") || m.starts_with("fcmov")
}

/// The rows of each pair in `object`; with `relaxed`, also that each
/// fused `jcc` is a `rel8` branch. The lines of the table.
fn check(name: &str, object: &Path, relaxed: bool) -> Option<BTreeSet<u64>> {
    let insns = instructions(object)?;
    let rows = rows(object)?;
    for (compare, consumer, kind) in PAIRS {
        let at_compare = at_line(name, &insns, &rows, compare);
        assert!(
            at_compare.iter().any(|i| is_compare(&i.mnemonic)),
            "{name}: no compare at line {compare}: {at_compare:?}\nrows {rows:x?}"
        );
        assert!(
            !at_compare.iter().any(|i| is_consumer(&i.mnemonic)),
            "{name}: the consumer of line {consumer} is at the compare's line {compare}: \
             {at_compare:?}"
        );
        let at_consumer = at_line(name, &insns, &rows, consumer);
        let want = |i: &&&Insn| match kind {
            Consumer::Jcc => i.mnemonic.starts_with('j') && i.mnemonic != "jmp",
            Consumer::Cmov => i.mnemonic.starts_with("cmov") || i.mnemonic.starts_with("fcmov"),
            Consumer::Movzx => i.mnemonic == "movzx",
        };
        let Some(insn) = at_consumer.iter().find(want) else {
            panic!("{name}: no {kind:?} at line {consumer}: {at_consumer:?}\nrows {rows:x?}");
        };
        assert!(
            !at_consumer.iter().any(|i| is_compare(&i.mnemonic) && i.mnemonic != "sub"),
            "{name}: a compare at the consumer's line {consumer}: {at_consumer:?}"
        );
        if relaxed && matches!(kind, Consumer::Jcc) {
            assert!(
                insn.len == 2 && (0x70..=0x7f).contains(&insn.first),
                "{name}: the jcc of line {consumer} is not relaxed: {insn:?}"
            );
        }
    }
    Some(rows.iter().map(|r| r.1).filter(|&l| l != 0 && l != u64::MAX).collect())
}

#[test]
fn fused_compares_keep_their_rows_through_the_shrink_pass() {
    let dir = Scratch::new("fused-lines");
    let text = ir();
    let object = dir.write("hod.o", hod(&text));
    let Some(hod_lines) = check("hod", &object, true) else { return };
    for major in [19, 22] {
        let Some(reference) = llc(&dir, major, &text) else { continue };
        let name = format!("llc {major}");
        let Some(mut llc_lines) = check(&name, &reference, false) else { continue };
        // P3: hod's start row at the scope line, which `llc`'s frameless
        // function lacks.
        llc_lines.insert(10);
        assert_eq!(hod_lines, llc_lines, "the lines of hod's table and {name}'s");
    }
}
