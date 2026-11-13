//! The end of a tail call's branch in a DWARF 4 call site on x86-64
//! (WP-163, `spec/dwarf.md` §6.21): `DW_AT_low_pc` of a
//! `DW_TAG_GNU_call_site` with `DW_AT_GNU_tail_call` is the address right
//! after the branch, which the target reports once it has written the
//! branch (`FnCx::tail_branch_ends_here`) and the shrink pass maps
//! (`spec/codegen.md` §5.6.15). The function's own branches are relaxed
//! to `rel8` before the tail exits, so the exits' offsets have moved.
//!
//! Each tail site's address must be the end of a `jmp` of the final
//! bytes, as decoded by `llvm-objdump`: with a length assumed instead of
//! reported, a target that wrote another branch (`jmp *%rax`, 2 bytes,
//! against `jmp *%r11`, 3) would fail here.
#![cfg(target_os = "linux")]

use std::path::Path;
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

/// A direct and an indirect `musttail` call, after a loop whose branches
/// the shrink pass relaxes, in a subprogram that describes its calls.
fn ir() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n{}",
        hod_ir::X86_64_LINUX_DATA_LAYOUT,
        r#"
@fp = global ptr null

define i32 @tails(i32 %a) !dbg !10 {
entry:
  br label %loop, !dbg !11
loop:
  %i = phi i32 [ 0, %entry ], [ %n, %loop ]
  %n = add i32 %i, 1, !dbg !11
  %c = icmp slt i32 %n, %a, !dbg !11
  br i1 %c, label %loop, label %out, !dbg !11
out:
  %big = icmp sgt i32 %a, 3, !dbg !12
  br i1 %big, label %direct, label %indirect, !dbg !12
direct:
  %r = musttail call i32 @ext(i32 %a), !dbg !13
  ret i32 %r, !dbg !13
indirect:
  %f = load ptr, ptr @fp, !dbg !14
  %s = musttail call i32 %f(i32 %a), !dbg !14
  ret i32 %s, !dbg !14
}

declare !dbg !20 i32 @ext(i32)

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!5, !6}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "tails.c", directory: "/tails")
!3 = !DISubroutineType(types: !4)
!4 = !{}
!5 = !{i32 7, !"Dwarf Version", i32 4}
!6 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "tails", scope: !1, file: !1, line: 10, type: !3, scopeLine: 10, flags: DIFlagAllCallsDescribed, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DILocation(line: 11, column: 3, scope: !10)
!12 = !DILocation(line: 12, column: 3, scope: !10)
!13 = !DILocation(line: 13, column: 3, scope: !10)
!14 = !DILocation(line: 14, column: 3, scope: !10)
!20 = !DISubprogram(name: "ext", scope: !1, file: !1, line: 4, type: !3, spFlags: 0)
"#
    )
}

/// `(address, length, mnemonic)` of each instruction of `.text`.
fn instructions(object: &Path) -> Option<Vec<(u64, u64, String)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--x86-asm-syntax=intel"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut insns: Vec<(u64, u64, String)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        let Ok(at) = u64::from_str_radix(at.trim(), 16) else { continue };
        let (bytes, text) = rest.split_once('\t').unwrap_or((rest, ""));
        let len = bytes.split_whitespace().filter(|b| u8::from_str_radix(b, 16).is_ok()).count();
        let mnemonic = text.split_whitespace().next().unwrap_or("").to_owned();
        match insns.last_mut() {
            Some(last) if mnemonic.is_empty() => last.1 += len as u64,
            _ if len == 0 => {}
            _ => insns.push((at, len as u64, mnemonic)),
        }
    }
    Some(insns)
}

/// The `DW_AT_low_pc` of each tail call's GNU call site, in order.
fn tail_sites(object: &Path) -> Option<Vec<u64>> {
    let dwarfdump = tool("llvm-dwarfdump")?;
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-dwarfdump: {}", outcome.stderr_text());
    let mut sites = Vec::new();
    let (mut in_site, mut tail, mut low) = (false, false, None);
    let mut flush = |in_site: bool, tail: bool, low: Option<u64>| {
        if in_site && tail {
            sites.push(low.expect("a tail call site has DW_AT_low_pc"));
        }
    };
    for line in outcome.stdout_text().lines() {
        if line.contains("DW_TAG_") {
            flush(in_site, tail, low);
            (in_site, tail, low) = (line.contains("DW_TAG_GNU_call_site"), false, None);
        } else if line.contains("DW_AT_GNU_tail_call") {
            tail = true;
        } else if line.contains("DW_AT_low_pc") {
            let hex = line.split('(').nth(1).and_then(|v| v.split(')').next());
            low = hex.and_then(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok());
        }
    }
    flush(in_site, tail, low);
    Some(sites)
}

#[test]
fn tail_call_sites_end_at_the_written_branch() {
    let dir = Scratch::new("tail-branch-end");
    let object = dir.write("tails.o", hod(&ir()));
    let (Some(insns), Some(sites)) = (instructions(&object), tail_sites(&object)) else { return };
    // The loop's branches shrank, so the exits moved.
    assert!(
        insns.iter().any(|(_, len, m)| *len == 2 && m.starts_with('j')),
        "no relaxed branch before the exits: {insns:?}"
    );
    assert_eq!(sites.len(), 2, "two tail call sites: {sites:x?}");
    let mut lengths = Vec::new();
    for &end in &sites {
        let Some((_, len, _)) =
            insns.iter().find(|(at, len, m)| at + len == end && m.as_str() == "jmp")
        else {
            panic!("the tail site at {end:#x} is not the end of a jmp: {insns:x?}");
        };
        lengths.push(*len);
    }
    // `jmp rel32` to the symbol; `jmp` through a register.
    assert_eq!(lengths[0], 5, "the direct tail call's branch");
    assert!(matches!(lengths[1], 2 | 3), "the indirect tail call's branch: {lengths:?}");
}
