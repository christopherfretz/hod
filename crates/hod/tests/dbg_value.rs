//! WP-162 (`spec/dwarf.md` §6.23): variables described by `#dbg_value`
//! records get location lists whose ranges are code offsets of the final
//! code, on both targets.
//!
//! The module is a chain of blocks, each starting with records (a
//! constant for `k`, an instruction result with a home for `w`) before a
//! call on a line of its own, then a compare and a branch out. On x86-64
//! the shrink pass relaxes every branch (`spec/codegen.md` §5.6.15), so
//! every offset after the first moves; the ranges go through the same
//! mapping as the line table (`shrink.rs` `move_rows`). So:
//! - `ranges_follow_the_final_code`: each of `k`'s entries starts at the
//!   first line row of its block's call and ends at the next block's
//!   (the block ends in a branch, §6.23), and `w`'s one entry (its equal
//!   adjacent ranges merged) covers the chain; every bound is the
//!   address of a line row, both objects verify;
//! - `split_lists_are_indexed`: the same module as split DWARF has a
//!   `.debug_loclists.dwo` and verifies.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];
/// The blocks of the chain.
const BLOCKS: u32 = 8;

/// The line of block `k`'s call, and of its compare.
const fn call_line(k: u32) -> u32 {
    100 + 10 * k
}
const RET_LINE: u32 = 300;

fn module(triple: &str, split: bool) -> String {
    let mut body = String::new();
    body.push_str("entry:\n  %a = add i32 %n, 1, !dbg !20\n  br label %b1, !dbg !20\n");
    let mut meta = String::new();
    for k in 1..=BLOCKS {
        let next = if k == BLOCKS { "out".to_owned() } else { format!("b{}", k + 1) };
        let (call, cmp) = (100 + 2 * k, 101 + 2 * k);
        let _ = write!(
            body,
            "b{k}:\n    #dbg_value(i32 %a, !31, !DIExpression(), !40)\n    \
             #dbg_value(i32 {k}, !30, !DIExpression(), !40)\n  \
             call void @use(i32 %a), !dbg !{call}\n  \
             %c{k} = icmp eq i32 %n, {k}, !dbg !{cmp}\n  \
             br i1 %c{k}, label %out, label %{next}, !dbg !{cmp}\n"
        );
        let _ = writeln!(meta, "!{call} = !DILocation(line: {}, scope: !10)", call_line(k));
        let _ = writeln!(meta, "!{cmp} = !DILocation(line: {}, scope: !10)", call_line(k) + 1);
    }
    let _ = write!(body, "out:\n  ret i32 %a, !dbg !21\n");
    let split_name = if split { ", splitDebugFilename: \"chain.dwo\"" } else { "" };
    format!(
        r#"target triple = "{triple}"

declare void @use(i32)

define dso_local i32 @f(i32 noundef %n) !dbg !10 {{
{body}}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: true, runtimeVersion: 0{split_name}, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "chain.c", directory: "/hod/tests")
!5 = !{{i32 7, !"Dwarf Version", i32 5}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
!10 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0, retainedNodes: !13)
!11 = !DISubroutineType(types: !12)
!12 = !{{!14, !14}}
!13 = !{{!30, !31}}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!20 = !DILocation(line: 2, scope: !10)
!21 = !DILocation(line: {RET_LINE}, scope: !10)
!30 = !DILocalVariable(name: "k", scope: !10, file: !1, line: 3, type: !14)
!31 = !DILocalVariable(name: "w", scope: !10, file: !1, line: 4, type: !14)
!40 = !DILocation(line: 3, scope: !10)
{meta}"#
    )
}

/// hod's object of `text`; with `dwo`, split DWARF into that file.
fn compile(text: &str, dwo: Option<&Path>) -> Vec<u8> {
    let mut options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    if let Some(dwo) = dwo {
        options.split_dwarf_file = Some(PathBuf::from("chain.dwo"));
        options.split_dwarf_output = Some(dwo.to_path_buf());
    }
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("{e}"))
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The first address of each line in `--debug-line`'s rows.
fn line_starts(dwarfdump: &Path, object: &Path) -> Vec<(u32, u64)> {
    let text = output(Command::new(dwarfdump).arg("--debug-line").arg(object));
    let mut starts: Vec<(u32, u64)> = Vec::new();
    for row in text.lines() {
        let mut words = row.split_whitespace();
        let (Some(addr), Some(line)) = (words.next(), words.next()) else { continue };
        let (Some(addr), Ok(line)) =
            (addr.strip_prefix("0x").and_then(|h| u64::from_str_radix(h, 16).ok()), line.parse())
        else {
            continue;
        };
        if !starts.iter().any(|&(l, _)| l == line) {
            starts.push((line, addr));
        }
    }
    starts
}

/// The entries `[start, end)` and their operations of variable `name`'s
/// location list, as `--debug-info` prints it.
fn entries(dwarfdump: &Path, object: &Path, name: &str) -> Vec<(u64, u64, String)> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.contains("DW_TAG_") {
            pending.clear();
        }
        // A resolved entry prints as `[start, end): ops`; a split unit's,
        // whose addresses are not resolved, as `DW_LLE_offset_pair (a, b): ops`.
        let entry = t.strip_prefix('[').or_else(|| {
            t.strip_prefix("DW_LLE_offset_pair").map(|r| r.trim_start().trim_start_matches('('))
        });
        if let Some(rest) = entry {
            let Some((range, ops)) = rest.split_once("): ") else { continue };
            let Some((a, b)) = range.split_once(", ") else { continue };
            let hex = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
            pending.push((hex(a), hex(b), ops.trim_end_matches(')').to_owned()));
        }
        if t.starts_with("DW_AT_name") && t.contains(&format!("(\"{name}\")")) {
            out = std::mem::take(&mut pending);
        }
    }
    out
}

#[test]
fn ranges_follow_the_final_code() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dwarfdump = &dwarfdump;
    let dir = Scratch::new("dbg-value-chain");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let object = dir.write(&format!("{arch}.o"), compile(&module(triple, false), None));
        let verify = output(Command::new(dwarfdump).arg("--verify").arg(&object));
        assert!(verify.contains("No errors."), "{arch}: {verify}");
        let rows = line_starts(dwarfdump, &object);
        let row = |line: u32| {
            rows.iter()
                .find(|&&(l, _)| l == line)
                .map_or_else(|| panic!("{arch}: no row of line {line}"), |&(_, a)| a)
        };
        let k = entries(dwarfdump, &object, "k");
        let want: Vec<(u64, u64, String)> = (1..=BLOCKS)
            .map(|b| {
                let end = if b == BLOCKS { row(RET_LINE) } else { row(call_line(b + 1)) };
                (row(call_line(b)), end, format!("DW_OP_consts +{b}, DW_OP_stack_value"))
            })
            .collect();
        assert_eq!(k, want, "{arch}: k's entries");
        let w = entries(dwarfdump, &object, "w");
        assert_eq!(w.len(), 1, "{arch}: w's entries {w:?}");
        assert_eq!((w[0].0, w[0].1), (row(call_line(1)), row(RET_LINE)), "{arch}: w's range");
        assert!(w[0].2.starts_with("DW_OP_fbreg"), "{arch}: w's location {}", w[0].2);
        let lists = output(Command::new(dwarfdump).arg("--debug-loclists").arg(&object));
        assert!(lists.contains("DW_LLE_base_addressx"), "{arch}: {lists}");
    }
}

#[test]
fn split_lists_are_indexed() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dwarfdump = &dwarfdump;
    let dir = Scratch::new("dbg-value-split");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let dwo = dir.join(format!("{arch}.dwo"));
        let object = dir.write(&format!("{arch}.o"), compile(&module(triple, true), Some(&dwo)));
        for file in [&object, &dwo] {
            let verify = output(Command::new(dwarfdump).arg("--verify").arg(file));
            assert!(verify.contains("No errors."), "{arch}: {verify}");
        }
        let info = output(Command::new(dwarfdump).arg("--debug-info").arg(&dwo));
        assert_eq!(info.matches("indexed (0x").count(), 2, "{arch}: {info}");
        let lists = output(Command::new(dwarfdump).arg("--debug-loclists").arg(&dwo));
        assert!(lists.contains("offset_entry_count = 0x00000002"), "{arch}: {lists}");
        let k = entries(dwarfdump, &dwo, "k");
        assert_eq!(k.len(), BLOCKS as usize, "{arch}: k's entries {k:?}");
    }
}

/// The chain with the odd blocks' calls in a lexical block, so that the
/// block has several ranges (a `DW_FORM_rnglistx` list, WP-163), and a
/// variable of that block described by a value record (a location list).
fn module_with_block(triple: &str, split: bool) -> String {
    let mut text = module(triple, split);
    for k in (1..=BLOCKS).step_by(2) {
        let call = 100 + 2 * k;
        let from = format!("!{call} = !DILocation(line: {}, scope: !10)", call_line(k));
        let to = format!("!{call} = !DILocation(line: {}, scope: !50)", call_line(k));
        text = text.replace(&from, &to);
    }
    text = text.replace("b1:\n", "b1:\n    #dbg_value(i32 %a, !51, !DIExpression(), !52)\n");
    text.push_str(
        "!50 = distinct !DILexicalBlock(scope: !10, file: !1, line: 5)\n\
         !51 = !DILocalVariable(name: \"inner\", scope: !50, file: !1, line: 6, type: !14)\n\
         !52 = !DILocation(line: 6, scope: !50)\n",
    );
    text
}

/// WP-163's range lists and these location lists in one unit, split and
/// not: both index their own section, the objects verify, and the
/// block's variable has its list.
#[test]
fn lists_beside_block_ranges() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dwarfdump = &dwarfdump;
    let dir = Scratch::new("dbg-value-block");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let plain =
            dir.write(&format!("{arch}.o"), compile(&module_with_block(triple, false), None));
        let dwo = dir.join(format!("{arch}-split.dwo"));
        let split = dir.write(
            &format!("{arch}-split.o"),
            compile(&module_with_block(triple, true), Some(&dwo)),
        );
        for file in [&plain, &split, &dwo] {
            let verify = output(Command::new(dwarfdump).arg("--verify").arg(file));
            assert!(verify.contains("No errors."), "{arch}: {verify}");
        }
        for info in [&plain, &dwo] {
            let text = output(Command::new(dwarfdump).arg("--debug-info").arg(info));
            assert!(text.contains("DW_AT_ranges\t(indexed (0x0) rangelist"), "{arch}: {text}");
            let inner = entries(dwarfdump, info, "inner");
            assert!(!inner.is_empty(), "{arch}: inner's entries {text}");
            assert_eq!(entries(dwarfdump, info, "k").len(), BLOCKS as usize, "{arch}: {text}");
        }
    }
}

/// One block of records with the expressions of §6.23, then the uses.
fn expressions(triple: &str, version: u32, record: &str) -> String {
    format!(
        r#"target triple = "{triple}"

declare void @use(i32)
declare void @usep(ptr)

define dso_local i32 @g(i32 noundef %n, ptr noundef %p) !dbg !10 {{
entry:
  %a = add i32 %n, 1, !dbg !20
  %q = getelementptr i8, ptr %p, i64 8, !dbg !20
  %w = sext i32 %n to i64, !dbg !20
  br label %b, !dbg !20
b:
    #dbg_value(i32 %a, !30, !DIExpression(DW_OP_plus_uconst, 4, DW_OP_stack_value), !40)
    #dbg_value(ptr %q, !31, !DIExpression(DW_OP_deref), !40)
    #dbg_value(i32 %a, !32, !DIExpression(DW_OP_constu, 1, DW_OP_shr, DW_OP_stack_value), !40)
    #dbg_value(i32 -3, !33, !DIExpression(), !40)
    #dbg_value(i32 40, !34, !DIExpression(), !40)
    #dbg_value(i64 %w, !35, !DIExpression(DW_OP_constu, 1, DW_OP_shr, DW_OP_stack_value), !40)
    #dbg_value(i32 %a, !36, !DIExpression(), !40)
{record}  call void @use(i32 %a), !dbg !21
  call void @usep(ptr %q), !dbg !21
  call void @usep(ptr null), !dbg !21
  %t = trunc i64 %w to i32, !dbg !21
  ret i32 %t, !dbg !22
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "expr.c", directory: "/hod/tests")
!5 = !{{i32 7, !"Dwarf Version", i32 {version}}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{{!14, !14}}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !DIBasicType(name: "unsigned", size: 32, encoding: DW_ATE_unsigned)
!16 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!17 = !DIDerivedType(tag: DW_TAG_typedef, name: "sint", file: !1, line: 1, baseType: !14)
!20 = !DILocation(line: 2, scope: !10)
!21 = !DILocation(line: 3, scope: !10)
!22 = !DILocation(line: 4, scope: !10)
!30 = !DILocalVariable(name: "x", scope: !10, file: !1, line: 5, type: !14)
!31 = !DILocalVariable(name: "y", scope: !10, file: !1, line: 6, type: !14)
!32 = !DILocalVariable(name: "z", scope: !10, file: !1, line: 7, type: !15)
!33 = !DILocalVariable(name: "s", scope: !10, file: !1, line: 8, type: !14)
!34 = !DILocalVariable(name: "u", scope: !10, file: !1, line: 9, type: !15)
!35 = !DILocalVariable(name: "h", scope: !10, file: !1, line: 10, type: !16)
!36 = !DILocalVariable(name: "e", scope: !10, file: !1, line: 11, type: !17)
!37 = !DILocalVariable(name: "r", scope: !10, file: !1, line: 12, type: !14)
!40 = !DILocation(line: 5, scope: !10)
"#
    )
}

/// The operations of `entries`, with the frame offsets left out.
fn ops_of(entries: &[(u64, u64, String)]) -> Vec<String> {
    entries
        .iter()
        .map(|(.., ops)| {
            ops.split(", ")
                .map(|op| if op.starts_with("DW_OP_fbreg") { "DW_OP_fbreg" } else { op })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect()
}

/// The expressions hod writes (§6.23): a home's value loaded for a value
/// expression (`DW_OP_deref_size` for a narrow one), a memory location by
/// a trailing `DW_OP_deref`, no entry for a narrow value under an
/// operation whose low bits depend on the high ones (`DW_OP_shr`), a
/// signed constant by `DW_OP_consts` only for a base type that is signed
/// itself (not through a typedef, as `llc`).
#[test]
fn expressions_are_written() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("dbg-value-expr");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let object = dir.write(&format!("{arch}.o"), compile(&expressions(triple, 5, ""), None));
        let verify = output(Command::new(&dwarfdump).arg("--verify").arg(&object));
        assert!(verify.contains("No errors."), "{arch}: {verify}");
        let ops = |name: &str| ops_of(&entries(&dwarfdump, &object, name));
        assert_eq!(
            ops("x"),
            ["DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_plus_uconst 0x4, DW_OP_stack_value"],
            "{arch}"
        );
        assert_eq!(ops("y"), ["DW_OP_fbreg, DW_OP_deref"], "{arch}");
        assert!(ops("z").is_empty(), "{arch}: {:?}", ops("z"));
        assert_eq!(ops("s"), ["DW_OP_consts -3, DW_OP_stack_value"], "{arch}");
        assert_eq!(ops("u"), ["DW_OP_constu 0x28, DW_OP_stack_value"], "{arch}");
        assert_eq!(
            ops("h"),
            ["DW_OP_fbreg, DW_OP_deref, DW_OP_constu 0x1, DW_OP_shr, DW_OP_stack_value"],
            "{arch}"
        );
        assert_eq!(ops("e"), ["DW_OP_fbreg"], "{arch}");
    }
}

fn declined(text: &str) -> String {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    match compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default()) {
        Err(hod::CompileError::Unsupported(u)) => match u.construct {
            hod::Construct::Named(name) => name.into_owned(),
            other => panic!("declined, but not by name: {other:?}"),
        },
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

/// Each refusal of §6.23, in the module above with that one construct;
/// and DWARF 4, whose transcoder has no location-list form (§6.20).
#[test]
fn refusals_are_named() {
    let cases = [
        (
            hod_dwarf::DEBUG_DBG_VALUE_FRAGMENT,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_LLVM_fragment, 0, 16), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_stack_value), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_plus_uconst, 1), !40)\n",
        ),
        (
            hod_cg::DEBUG_DBG_VALUE_ARGLIST,
            "    #dbg_value(!DIArgList(i32 %a, i32 %n), !37, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_stack_value), !40)\n",
        ),
    ];
    for triple in TRIPLES {
        for (name, record) in cases {
            assert_eq!(declined(&expressions(triple, 5, record)), name, "{triple}: {record}");
        }
        assert_eq!(declined(&expressions(triple, 4, "")), hod_dwarf::DWARF_4_SHAPE, "{triple}");
    }
}
