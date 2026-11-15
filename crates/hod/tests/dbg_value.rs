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
use hod_testutil::{Scratch, run_with, tool, tools_required};

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
/// itself (not through a typedef, as `llc`). DWARF 4 writes the same
/// entries in `.debug_loc` (WP-221, §6.30).
#[test]
fn expressions_are_written() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("dbg-value-expr");
    for (triple, version) in TRIPLES.iter().flat_map(|t| [(*t, 5), (*t, 4)]) {
        let arch = format!("{}-v{version}", triple.split('-').next().unwrap_or(triple));
        let object =
            dir.write(&format!("{arch}.o"), compile(&expressions(triple, version, ""), None));
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
            // WP-230: `llc`'s `emitConstu` writes a small constant as a
            // literal.
            ["DW_OP_fbreg, DW_OP_deref, DW_OP_lit1, DW_OP_shr, DW_OP_stack_value"],
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

/// Each refusal of §6.23, in the module above with that one construct.
/// (DWARF 4 was one until WP-221 wrote `.debug_loc`, §6.30.)
#[test]
fn refusals_are_named() {
    let cases = [
        // WP-225 (§6.31): a fragment that is not whole bytes; a
        // conversion wider than 64 bits other than to 128 (WP-230,
        // §6.34), to a float, or under a memory location.
        (
            hod_dwarf::DEBUG_DBG_VALUE_FRAGMENT,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_LLVM_fragment, 0, 12), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_LLVM_convert, 96, DW_ATE_signed, DW_OP_stack_value), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_float, DW_OP_stack_value), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(ptr %q, !37, !DIExpression(DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_deref), !40)\n",
        ),
        (
            hod_dwarf::DEBUG_DBG_VALUE_EXPR,
            "    #dbg_value(i32 %a, !37, !DIExpression(DW_OP_plus_uconst, 1), !40)\n",
        ),
        // WP-230 (§6.32): a `DIArgList` that is not a computed value.
        (
            hod_cg::DEBUG_DBG_VALUE_ARGLIST,
            "    #dbg_value(!DIArgList(ptr %q, i32 %n), !37, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_deref), !40)\n",
        ),
    ];
    for triple in TRIPLES {
        for (name, record) in cases {
            assert_eq!(declined(&expressions(triple, 5, record)), name, "{triple}: {record}");
        }
    }
}

/// WP-225 (§6.31): conversions and fragments, after one block of values
/// with homes. `split`: split DWARF (`chain.dwo`, `compile`'s name).
fn pieces(triple: &str, split: bool) -> String {
    let split_name = if split { ", splitDebugFilename: \"chain.dwo\"" } else { "" };
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
    #dbg_value(i32 %a, !30, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_LLVM_convert, 64, DW_ATE_signed, DW_OP_stack_value), !40)
    #dbg_value(i64 %w, !31, !DIExpression(DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_LLVM_convert, 8, DW_ATE_unsigned, DW_OP_stack_value), !40)
    #dbg_value(i64 0, !34, !DIExpression(DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_LLVM_convert, 8, DW_ATE_unsigned, DW_OP_stack_value), !40)
    #dbg_value(i8 -56, !35, !DIExpression(DW_OP_LLVM_convert, 8, DW_ATE_signed, DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_stack_value), !40)
    #dbg_value(i1 true, !36, !DIExpression(DW_OP_not, DW_OP_LLVM_convert, 1, DW_ATE_unsigned, DW_OP_LLVM_convert, 32, DW_ATE_unsigned, DW_OP_stack_value), !40)
    #dbg_value(i32 5, !37, !DIExpression(), !40)
    #dbg_value(i64 %w, !50, !DIExpression(DW_OP_LLVM_fragment, 0, 64), !40)
    #dbg_value(i64 3, !50, !DIExpression(DW_OP_LLVM_fragment, 64, 64), !40)
    #dbg_value(ptr %q, !51, !DIExpression(DW_OP_plus_uconst, 8, DW_OP_stack_value, DW_OP_LLVM_fragment, 64, 64), !40)
    #dbg_value(ptr %q, !52, !DIExpression(DW_OP_deref, DW_OP_LLVM_fragment, 0, 64), !40)
    #dbg_value(i32 %a, !53, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_LLVM_convert, 64, DW_ATE_signed, DW_OP_stack_value, DW_OP_LLVM_fragment, 64, 64), !40)
    #dbg_value(i32 %a, !54, !DIExpression(DW_OP_LLVM_fragment, 0, 64), !40)
  call void @use(i32 %a), !dbg !21
    #dbg_value(i64 poison, !50, !DIExpression(DW_OP_LLVM_fragment, 0, 64), !40)
    #dbg_value(i32 0, !37, !DIExpression(), !40)
  call void @usep(ptr %q), !dbg !23
    #dbg_value(ptr %q, !50, !DIExpression(DW_OP_deref), !40)
  call void @usep(ptr null), !dbg !24
  %t = trunc i64 %w to i32, !dbg !24
  ret i32 %t, !dbg !22
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: true, runtimeVersion: 0{split_name}, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "pieces.c", directory: "/hod/tests")
!5 = !{{i32 7, !"Dwarf Version", i32 5}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{{!14, !14}}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!16 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!17 = !DIBasicType(name: "_Bool", size: 8, encoding: DW_ATE_boolean)
!18 = !DIBasicType(name: "unsigned char", size: 8, encoding: DW_ATE_unsigned_char)
!20 = !DILocation(line: 2, scope: !10)
!21 = !DILocation(line: 3, scope: !10)
!22 = !DILocation(line: 4, scope: !10)
!23 = !DILocation(line: 13, scope: !10)
!24 = !DILocation(line: 14, scope: !10)
!30 = !DILocalVariable(name: "wide", scope: !10, file: !1, line: 5, type: !16)
!31 = !DILocalVariable(name: "low", scope: !10, file: !1, line: 6, type: !18)
!34 = !DILocalVariable(name: "flag", scope: !10, file: !1, line: 9, type: !17)
!35 = !DILocalVariable(name: "folded", scope: !10, file: !1, line: 10, type: !14)
!36 = !DILocalVariable(name: "nflag", scope: !10, file: !1, line: 11, type: !14)
!37 = !DILocalVariable(name: "b5", scope: !10, file: !1, line: 12, type: !17)
!45 = !DICompositeType(tag: DW_TAG_structure_type, name: "P", file: !1, line: 1, size: 128, elements: !46)
!46 = !{{!47, !48}}
!47 = !DIDerivedType(tag: DW_TAG_member, name: "x", scope: !45, file: !1, line: 1, baseType: !16, size: 64)
!48 = !DIDerivedType(tag: DW_TAG_member, name: "y", scope: !45, file: !1, line: 1, baseType: !16, size: 64, offset: 64)
!50 = !DILocalVariable(name: "pa", scope: !10, file: !1, line: 13, type: !45)
!51 = !DILocalVariable(name: "pb", scope: !10, file: !1, line: 14, type: !45)
!52 = !DILocalVariable(name: "pc", scope: !10, file: !1, line: 15, type: !45)
!53 = !DILocalVariable(name: "pd", scope: !10, file: !1, line: 16, type: !45)
!54 = !DILocalVariable(name: "pe", scope: !10, file: !1, line: 17, type: !45)
!40 = !DILocation(line: 5, scope: !10)
"#
    )
}

/// `ops_of`, with each `DW_OP_convert`'s base type by name only.
fn named_ops(entries: &[(u64, u64, String)]) -> Vec<String> {
    ops_of(entries)
        .into_iter()
        .map(|e| {
            e.split(", ")
                .map(|op| match op.strip_prefix("DW_OP_convert (") {
                    Some(rest) => format!(
                        "DW_OP_convert {}",
                        rest.split_once(") ").map_or(rest, |(_, n)| n).trim_matches('"')
                    ),
                    None => op.to_owned(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect()
}

/// §6.31, as `llc` 22 writes each shape (`spec/landed/dwarf/WP-225.md`):
/// a conversion of a home's value by `DW_OP_convert` and a base type DIE
/// (the unit's first children); the leading conversions of a constant
/// folded into it; LLVM 22's constants (a boolean is 0 or 1, an `i1` is
/// not sign-extended); fragments composed into pieces, gaps as empty
/// pieces, a poison record ending its piece, a record of the whole
/// variable ending every piece, and a piece wider than its value left
/// out. Split, the conversions take `llc`'s legacy form.
#[test]
fn pieces_and_conversions() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("dbg-value-pieces");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let plain = dir.write(&format!("{arch}.o"), compile(&pieces(triple, false), None));
        let dwo = dir.join(format!("{arch}-split.dwo"));
        let split =
            dir.write(&format!("{arch}-split.o"), compile(&pieces(triple, true), Some(&dwo)));
        for file in [&plain, &split, &dwo] {
            let verify = output(Command::new(&dwarfdump).arg("--verify").arg(file));
            assert!(verify.contains("No errors."), "{arch}: {verify}");
        }
        let ops = |file: &Path, name: &str| named_ops(&entries(&dwarfdump, file, name));
        let info = output(Command::new(&dwarfdump).arg("--debug-info").arg(&plain));
        let first: Vec<&str> = info
            .lines()
            .skip_while(|l| !l.contains("DW_TAG_compile_unit"))
            .filter(|l| l.contains("DW_TAG_"))
            .skip(1)
            .take(7)
            .map(|l| l.split("DW_TAG_").nth(1).unwrap_or(l))
            .collect();
        assert_eq!(first[..6], ["base_type"; 6], "{arch}: {info}");
        assert_eq!(first[6], "subprogram", "{arch}: {info}");
        assert_eq!(
            ops(&plain, "wide"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_convert DW_ATE_signed_32, DW_OP_convert DW_ATE_signed_64, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "low"),
            [
                "DW_OP_fbreg, DW_OP_deref, DW_OP_convert DW_ATE_unsigned_64, DW_OP_convert DW_ATE_unsigned_8, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(ops(&plain, "flag"), ["DW_OP_lit0, DW_OP_stack_value"], "{arch}");
        assert_eq!(ops(&plain, "folded"), ["DW_OP_consts -56, DW_OP_stack_value"], "{arch}");
        assert_eq!(
            ops(&plain, "nflag"),
            [
                "DW_OP_consts +1, DW_OP_not, DW_OP_convert DW_ATE_unsigned_1, DW_OP_convert DW_ATE_unsigned_32, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "b5"),
            ["DW_OP_lit1, DW_OP_stack_value", "DW_OP_lit0, DW_OP_stack_value"],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "pa"),
            [
                "DW_OP_fbreg, DW_OP_piece 0x8, DW_OP_lit3, DW_OP_stack_value, DW_OP_piece 0x8",
                "DW_OP_piece 0x8, DW_OP_lit3, DW_OP_stack_value, DW_OP_piece 0x8",
                "DW_OP_fbreg, DW_OP_deref",
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "pb"),
            [
                "DW_OP_piece 0x8, DW_OP_fbreg, DW_OP_deref, DW_OP_plus_uconst 0x8, DW_OP_stack_value, DW_OP_piece 0x8"
            ],
            "{arch}"
        );
        assert_eq!(ops(&plain, "pc"), ["DW_OP_fbreg, DW_OP_deref, DW_OP_piece 0x8"], "{arch}");
        assert_eq!(
            ops(&plain, "pd"),
            [
                "DW_OP_piece 0x8, DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_convert DW_ATE_signed_32, DW_OP_convert DW_ATE_signed_64, DW_OP_stack_value, DW_OP_piece 0x8"
            ],
            "{arch}"
        );
        assert!(ops(&plain, "pe").is_empty(), "{arch}: {:?}", ops(&plain, "pe"));
        // Split: `emitLegacySExt` for the widening, nothing for the
        // truncation; no base type DIEs.
        assert_eq!(
            ops(&dwo, "wide"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_dup, DW_OP_constu 0x1f, DW_OP_shr, DW_OP_lit0, DW_OP_not, DW_OP_mul, DW_OP_constu 0x20, DW_OP_shl, DW_OP_or, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(ops(&dwo, "low"), ["DW_OP_fbreg, DW_OP_deref, DW_OP_stack_value"], "{arch}");
        assert_eq!(
            ops(&dwo, "nflag"),
            ["DW_OP_consts +1, DW_OP_not, DW_OP_constu 0x1, DW_OP_and, DW_OP_stack_value"],
            "{arch}"
        );
        let dwo_info = output(Command::new(&dwarfdump).arg("--debug-info").arg(&dwo));
        assert!(!dwo_info.contains("DW_ATE_signed_32"), "{arch}: {dwo_info}");
    }
}

/// WP-230 (§6.32): records whose values are a `DIArgList`, after one
/// block of values with homes.
fn arglists(triple: &str, split: bool) -> String {
    let split_name = if split { ", splitDebugFilename: \"chain.dwo\"" } else { "" };
    format!(
        r#"target triple = "{triple}"

declare void @use(i32)
declare void @use64(i64)
declare void @usep(ptr)

define dso_local i32 @g(i32 noundef %n, ptr noundef %p, i64 noundef %k) !dbg !10 {{
entry:
  %a = add i32 %n, 1, !dbg !20
  %w = sext i32 %n to i64, !dbg !20
  br label %b, !dbg !20
b:
    #dbg_value(!DIArgList(ptr %p, i64 %k), !30, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_constu, 4, DW_OP_mul, DW_OP_plus, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(i32 %a, i32 %n), !31, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_minus, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(i32 %a, i32 %n), !32, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_minus, DW_OP_constu, 1, DW_OP_shra, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(i32 %a, i64 %w), !33, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_LLVM_convert, 64, DW_ATE_signed, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(i64 100, i32 -1), !34, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(ptr %p, i64 poison), !35, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_stack_value), !40)
    #dbg_value(ptr %p, !36, !DIExpression(DW_OP_LLVM_fragment, 0, 64), !40)
    #dbg_value(!DIArgList(i64 %w, ptr %p), !36, !DIExpression(DW_OP_LLVM_arg, 1, DW_OP_LLVM_arg, 0, DW_OP_plus, DW_OP_stack_value, DW_OP_LLVM_fragment, 64, 64), !40)
    #dbg_value(i64 %w, !37, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_plus_uconst, 2, DW_OP_stack_value), !40)
    #dbg_value(!DIArgList(i32 %a, i32 %a), !38, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_mul, DW_OP_stack_value), !40)
  call void @use(i32 %a), !dbg !21
  call void @use(i32 %n), !dbg !21
  call void @usep(ptr %p), !dbg !21
  call void @use64(i64 %k), !dbg !21
  call void @use64(i64 %w), !dbg !21
  ret i32 %a, !dbg !22
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug{split_name}, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "arglist.c", directory: "/hod/tests")
!5 = !{{i32 7, !"Dwarf Version", i32 5}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{{!14, !14}}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !DIBasicType(name: "unsigned long", size: 64, encoding: DW_ATE_unsigned)
!16 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!17 = !DICompositeType(tag: DW_TAG_structure_type, name: "pair", file: !1, line: 1, size: 128, elements: !18)
!18 = !{{}}
!20 = !DILocation(line: 2, scope: !10)
!21 = !DILocation(line: 3, scope: !10)
!22 = !DILocation(line: 4, scope: !10)
!30 = !DILocalVariable(name: "ix", scope: !10, file: !1, line: 5, type: !15)
!31 = !DILocalVariable(name: "d", scope: !10, file: !1, line: 6, type: !14)
!32 = !DILocalVariable(name: "half", scope: !10, file: !1, line: 7, type: !14)
!33 = !DILocalVariable(name: "c", scope: !10, file: !1, line: 8, type: !16)
!34 = !DILocalVariable(name: "k", scope: !10, file: !1, line: 9, type: !15)
!35 = !DILocalVariable(name: "pz", scope: !10, file: !1, line: 10, type: !15)
!36 = !DILocalVariable(name: "f", scope: !10, file: !1, line: 11, type: !17)
!37 = !DILocalVariable(name: "one", scope: !10, file: !1, line: 12, type: !16)
!38 = !DILocalVariable(name: "sq", scope: !10, file: !1, line: 13, type: !14)
!40 = !DILocation(line: 5, scope: !10)
"#
    )
}

/// §6.32, as `llc` 19 and 22 write each shape under `SelectionDAG`
/// (`spec/landed/dwarf/WP-230.md`; `DW_OP_fbreg` against `DW_OP_breg`,
/// `DW_OP_deref_size` against `llc`'s `DW_OP_deref` of a narrow value):
/// each value pushed where `DW_OP_LLVM_arg` names it, a constant
/// sign-extended (an `unsigned long`'s `i32 -1` is all ones), an
/// expression's small `DW_OP_constu` a literal; no entry when a value has
/// no place or a narrow value meets `DW_OP_shra`; a fragment composed
/// with a single record's; a single value named by `DW_OP_LLVM_arg`.
/// Split, the conversions take `llc`'s legacy form.
#[test]
fn arglists_are_written() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("dbg-value-arglist");
    for triple in TRIPLES {
        let arch = triple.split('-').next().unwrap_or(triple);
        let plain = dir.write(&format!("{arch}.o"), compile(&arglists(triple, false), None));
        let dwo = dir.join(format!("{arch}-split.dwo"));
        let split =
            dir.write(&format!("{arch}-split.o"), compile(&arglists(triple, true), Some(&dwo)));
        for file in [&plain, &split, &dwo] {
            let verify = output(Command::new(&dwarfdump).arg("--verify").arg(file));
            assert!(verify.contains("No errors."), "{arch}: {verify}");
        }
        let ops = |file: &Path, name: &str| named_ops(&entries(&dwarfdump, file, name));
        assert_eq!(
            ops(&plain, "ix"),
            [
                "DW_OP_fbreg, DW_OP_deref, DW_OP_fbreg, DW_OP_deref, DW_OP_lit4, DW_OP_mul, DW_OP_plus, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "d"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_minus, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert!(ops(&plain, "half").is_empty(), "{arch}: {:?}", ops(&plain, "half"));
        assert_eq!(
            ops(&plain, "c"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_convert DW_ATE_signed_32, DW_OP_convert DW_ATE_signed_64, DW_OP_fbreg, DW_OP_deref, DW_OP_plus, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "k"),
            ["DW_OP_constu 0x64, DW_OP_lit0, DW_OP_not, DW_OP_plus, DW_OP_stack_value"],
            "{arch}"
        );
        assert!(ops(&plain, "pz").is_empty(), "{arch}: {:?}", ops(&plain, "pz"));
        assert_eq!(
            ops(&plain, "f"),
            [
                "DW_OP_fbreg, DW_OP_piece 0x8, DW_OP_fbreg, DW_OP_deref, DW_OP_fbreg, DW_OP_deref, DW_OP_plus, DW_OP_stack_value, DW_OP_piece 0x8"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "one"),
            ["DW_OP_fbreg, DW_OP_deref, DW_OP_plus_uconst 0x2, DW_OP_stack_value"],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "sq"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_mul, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&dwo, "c"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_dup, DW_OP_constu 0x1f, DW_OP_shr, DW_OP_lit0, DW_OP_not, DW_OP_mul, DW_OP_constu 0x20, DW_OP_shl, DW_OP_or, DW_OP_fbreg, DW_OP_deref, DW_OP_plus, DW_OP_stack_value"
            ],
            "{arch}"
        );
    }
}

/// WP-230 (§6.34): conversions to 128 bits, the shapes of libc++'s
/// `__itoa` at `-O1`: values with homes widened (one as a fragment, one
/// signed), a widened constant, an `i128` value narrowed, and an
/// argument list with an `i128` value.
fn wide(triple: &str, split: bool) -> String {
    let split_name = if split { ", splitDebugFilename: \"chain.dwo\"" } else { "" };
    format!(
        r#"target triple = "{triple}"

declare void @g(i64)

define i64 @f(i64 %n, i32 %m, ptr %p) !dbg !10 {{
entry:
  %w = zext i64 %n to i128, !dbg !20
  %s = shl i128 %w, 64, !dbg !20
  %m1 = add i32 %m, 1, !dbg !20
  br label %b, !dbg !20
b:
  call void @g(i64 1), !dbg !20
    #dbg_value(i64 %n, !30, !DIExpression(DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_LLVM_convert, 128, DW_ATE_unsigned, DW_OP_stack_value), !20)
    #dbg_value(i32 %m1, !31, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_unsigned, DW_OP_LLVM_convert, 128, DW_ATE_unsigned, DW_OP_stack_value, DW_OP_LLVM_fragment, 0, 128), !20)
    #dbg_value(i32 %m1, !32, !DIExpression(DW_OP_LLVM_convert, 32, DW_ATE_signed, DW_OP_LLVM_convert, 128, DW_ATE_signed, DW_OP_stack_value), !20)
    #dbg_value(i64 7, !33, !DIExpression(DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_LLVM_convert, 128, DW_ATE_unsigned, DW_OP_stack_value), !20)
    #dbg_value(i128 %s, !34, !DIExpression(DW_OP_LLVM_convert, 128, DW_ATE_unsigned, DW_OP_LLVM_convert, 32, DW_ATE_unsigned, DW_OP_constu, 15, DW_OP_and, DW_OP_stack_value), !20)
    #dbg_value(!DIArgList(i128 %s, ptr %p), !35, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_LLVM_convert, 64, DW_ATE_unsigned, DW_OP_LLVM_convert, 128, DW_ATE_unsigned, DW_OP_or, DW_OP_stack_value), !20)
  call void @g(i64 2), !dbg !20
  %t = trunc i128 %s to i64, !dbg !20
  call void @g(i64 %t), !dbg !20
  call void @g(i64 %n), !dbg !20
  %z = zext i32 %m1 to i64, !dbg !20
  call void @g(i64 %z), !dbg !20
  %q = ptrtoint ptr %p to i64, !dbg !20
  call void @g(i64 %q), !dbg !20
  ret i64 %n, !dbg !20
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!5, !6}}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "hand", isOptimized: true, runtimeVersion: 0{split_name}, emissionKind: FullDebug)
!1 = !DIFile(filename: "wide.c", directory: "/hod/tests")
!5 = !{{i32 7, !"Dwarf Version", i32 5}}
!6 = !{{i32 2, !"Debug Info Version", i32 3}}
!10 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{{}}
!14 = !DIBasicType(name: "unsigned __int128", size: 128, encoding: DW_ATE_unsigned)
!15 = !DIBasicType(name: "__int128", size: 128, encoding: DW_ATE_signed)
!16 = !DIBasicType(name: "unsigned int", size: 32, encoding: DW_ATE_unsigned)
!20 = !DILocation(line: 2, scope: !10)
!30 = !DILocalVariable(name: "wide", scope: !10, file: !1, line: 3, type: !14)
!31 = !DILocalVariable(name: "piece", scope: !10, file: !1, line: 4, type: !40)
!32 = !DILocalVariable(name: "signed", scope: !10, file: !1, line: 5, type: !15)
!33 = !DILocalVariable(name: "konst", scope: !10, file: !1, line: 6, type: !14)
!34 = !DILocalVariable(name: "narrow", scope: !10, file: !1, line: 7, type: !16)
!35 = !DILocalVariable(name: "joined", scope: !10, file: !1, line: 8, type: !14)
!40 = !DICompositeType(tag: DW_TAG_structure_type, name: "P", file: !1, line: 1, size: 256, elements: !41)
!41 = !{{!42, !43}}
!42 = !DIDerivedType(tag: DW_TAG_member, name: "lo", scope: !40, file: !1, line: 1, baseType: !14, size: 128)
!43 = !DIDerivedType(tag: DW_TAG_member, name: "hi", scope: !40, file: !1, line: 1, baseType: !14, size: 128, offset: 128)
"#
    )
}

/// §6.34, as `llc` 22 writes each shape (both targets, its `-O0`
/// selectors; `llc`'s home is `DW_OP_breg` of the stack pointer and an
/// 8-byte one is read by `DW_OP_deref_size 0x8`): a home's value
/// converted to 128 bits through `DW_ATE_unsigned_128` and
/// `DW_ATE_signed_128` (16 bytes); the widened constant pushed and
/// converted, not folded (AArch64's `GlobalISel`; x86-64's `FastISel` writes
/// a bare `DW_OP_stack_value`, which describes nothing); no entry for an
/// `i128` value, alone or in an argument list. Split, the legacy form:
/// `emitLegacyZExt(64)` for the unsigned widening as `llc`, and no
/// entry for the signed one (`llc` leaves its sign in 64 bits).
#[test]
fn conversions_to_128_bits() {
    conversions_to_128_bits_on(&TRIPLES);
}

/// `llc`'s objects of [`wide`] for `triple` (`-O0`): plain, and the
/// split one's `.dwo`; `None` without `llc` 22.
fn llc_wide(dir: &Scratch, triple: &str) -> Option<(PathBuf, PathBuf)> {
    let Some(llc) = tool("llc-22") else {
        assert!(!tools_required(), "llc-22 is required");
        return None;
    };
    let arch = triple.split('-').next().unwrap_or(triple);
    let input = dir.write(&format!("{arch}.ll"), wide(triple, false));
    let theirs = dir.join(format!("{arch}.llc.o"));
    let mut command = Command::new(&llc);
    command.args(["-O0", "-relocation-model=pic", "-filetype=obj"]);
    output(command.arg(&input).arg("-o").arg(&theirs));
    let input = dir.write(&format!("{arch}-split.ll"), wide(triple, true));
    let dwo = dir.join(format!("{arch}.llc.dwo"));
    let mut command = Command::new(&llc);
    command.args(["-O0", "-relocation-model=pic", "-filetype=obj"]);
    command
        .arg("-split-dwarf-file=chain.dwo")
        .arg(format!("-split-dwarf-output={}", dwo.display()));
    output(command.arg(&input).arg("-o").arg(dir.join(format!("{arch}-split.llc.o"))));
    Some((theirs, dwo))
}

fn conversions_to_128_bits_on(triples: &[&str]) {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("dbg-value-128");
    for &triple in triples {
        let arch = triple.split('-').next().unwrap_or(triple);
        let plain = dir.write(&format!("{arch}.o"), compile(&wide(triple, false), None));
        let dwo = dir.join(format!("{arch}-split.dwo"));
        let split = dir.write(&format!("{arch}-split.o"), compile(&wide(triple, true), Some(&dwo)));
        for file in [&plain, &split, &dwo] {
            let verify = output(Command::new(&dwarfdump).arg("--verify").arg(file));
            assert!(verify.contains("No errors."), "{arch}: {verify}");
        }
        let ops = |file: &Path, name: &str| named_ops(&entries(&dwarfdump, file, name));
        let info = output(Command::new(&dwarfdump).arg("--debug-info").arg(&plain));
        let base = info.split("DW_AT_name\t(\"DW_ATE_unsigned_128\")").nth(1).unwrap_or_default();
        let size = base.lines().find(|l| l.contains("DW_AT_byte_size")).unwrap_or_default();
        assert!(size.contains("(16)") || size.contains("(0x10)"), "{arch}: {info}");
        let u128 = "DW_OP_convert DW_ATE_unsigned_128, DW_OP_stack_value";
        assert_eq!(
            ops(&plain, "wide"),
            [format!("DW_OP_fbreg, DW_OP_deref, DW_OP_convert DW_ATE_unsigned_64, {u128}")],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "piece"),
            [format!(
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_convert DW_ATE_unsigned_32, {u128}, DW_OP_piece 0x10"
            )],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "signed"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_convert DW_ATE_signed_32, DW_OP_convert DW_ATE_signed_128, DW_OP_stack_value"
            ],
            "{arch}"
        );
        assert_eq!(
            ops(&plain, "konst"),
            [format!("DW_OP_lit7, DW_OP_convert DW_ATE_unsigned_64, {u128}")],
            "{arch}"
        );
        for file in [&plain, &dwo] {
            for name in ["narrow", "joined"] {
                assert!(ops(file, name).is_empty(), "{arch} {name}: {:?}", ops(file, name));
            }
        }
        let zext64 = "DW_OP_lit1, DW_OP_constu 0x40, DW_OP_shl, DW_OP_lit1, DW_OP_minus, DW_OP_and, DW_OP_stack_value";
        assert_eq!(ops(&dwo, "wide"), [format!("DW_OP_fbreg, DW_OP_deref, {zext64}")], "{arch}");
        assert_eq!(
            ops(&dwo, "piece"),
            [
                "DW_OP_fbreg, DW_OP_deref_size 0x4, DW_OP_constu 0xffffffff, DW_OP_and, DW_OP_stack_value, DW_OP_piece 0x10"
            ],
            "{arch}"
        );
        assert!(ops(&dwo, "signed").is_empty(), "{arch}: {:?}", ops(&dwo, "signed"));
        assert_eq!(ops(&dwo, "konst"), [format!("DW_OP_lit7, {zext64}")], "{arch}");
        // The same against `llc` 22 itself, where its locations describe
        // the value (its home named from the stack pointer).
        let Some((theirs, theirs_dwo)) = llc_wide(&dir, triple) else { continue };
        let llc_ops = |file: &Path, name: &str| -> Vec<String> {
            named_ops(&entries(&dwarfdump, file, name))
                .into_iter()
                .map(|e| {
                    e.split(", ")
                        .map(|op| match op {
                            _ if op.starts_with("DW_OP_breg31 WSP")
                                || op.starts_with("DW_OP_breg7 RSP") =>
                            {
                                "DW_OP_fbreg"
                            }
                            "DW_OP_deref_size 0x8" => "DW_OP_deref",
                            _ => op,
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect()
        };
        for name in ["wide", "piece", "signed", "joined"] {
            assert_eq!(ops(&plain, name), llc_ops(&theirs, name), "{arch} {name}");
        }
        for name in ["wide", "piece", "joined"] {
            assert_eq!(ops(&dwo, name), llc_ops(&theirs_dwo, name), "{arch} split {name}");
        }
        if triple.starts_with("aarch64") {
            assert_eq!(ops(&plain, "konst"), llc_ops(&theirs, "konst"), "{arch}");
            assert_eq!(ops(&dwo, "konst"), llc_ops(&theirs_dwo, "konst"), "{arch}");
            assert!(llc_ops(&theirs, "narrow").is_empty(), "{arch}");
        } else {
            assert_eq!(llc_ops(&theirs, "konst"), ["DW_OP_stack_value"], "{arch}");
            // x86-64's FastISel reads the low 8 bytes of the `i128`'s
            // home and narrows them; hod writes no entry for an `i128`
            // value (§6.34).
            let narrow = llc_ops(&theirs, "narrow");
            assert!(
                narrow.len() == 1 && narrow[0].starts_with("DW_OP_fbreg, DW_OP_deref, DW_OP_convert DW_ATE_unsigned_128, DW_OP_convert DW_ATE_unsigned_32"),
                "{arch}: {narrow:?}"
            );
        }
    }
}
