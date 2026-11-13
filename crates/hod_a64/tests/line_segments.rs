//! The row locations and segments of `spec/dwarf.md` §5.8 (WP-097), as
//! `llvm-dwarfdump` reads hod's object:
//!
//! - an instruction emitted inside another's lowering (a deferred
//!   `getelementptr` asked for by the store of its value) has its own
//!   line's row;
//! - an `unreachable` has no row of its own (its line has none);
//! - a zero-offset `getelementptr` has no row of its own, one with an
//!   offset that is computed on its own does.

mod common;
#[path = "lines_support/mod.rs"]
mod support;

use common::{ll_header, parse};
use hod_a64::compile_to_object;
use hod_cg::Options;
use hod_testutil::{Scratch, tool};
use support::{DRow, rows_by_function};

const BODY: &str = r#"
@g = dso_local global i32 0, align 4

@gp = dso_local global ptr null, align 8

define dso_local void @forced(ptr %p) !dbg !10 {
entry:
  %b = getelementptr inbounds i8, ptr %p, i64 8, !dbg !11
  store volatile ptr %b, ptr @gp, align 8, !dbg !12
  ret void, !dbg !13
}

declare void @abort() noreturn

define dso_local void @trap(i32 %x) !dbg !20 {
entry:
  store volatile i32 %x, ptr @g, align 4, !dbg !21
  call void @abort(), !dbg !21
  unreachable, !dbg !22
}

%Pair = type { i32, i32 }

define dso_local void @fields(ptr %p, i32 %x) !dbg !30 {
entry:
  %a = getelementptr inbounds %Pair, ptr %p, i32 0, i32 0, !dbg !31
  %y = add i32 %x, 1, !dbg !32
  store volatile i32 %y, ptr %a, align 4, !dbg !32
  %b = getelementptr inbounds %Pair, ptr %p, i32 0, i32 1, !dbg !33
  %w = add i32 %x, 2, !dbg !34
  store volatile i32 %w, ptr %b, align 4, !dbg !34
  ret void, !dbg !35
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "seg.c", directory: "/hod/tests/lines")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{null}
!10 = distinct !DISubprogram(name: "forced", scope: !1, file: !1, line: 4, type: !4, scopeLine: 4, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DILocation(line: 5, column: 11, scope: !10)
!12 = !DILocation(line: 6, column: 7, scope: !10)
!13 = !DILocation(line: 7, column: 3, scope: !10)
!20 = distinct !DISubprogram(name: "trap", scope: !1, file: !1, line: 10, type: !4, scopeLine: 10, spFlags: DISPFlagDefinition, unit: !0)
!21 = !DILocation(line: 11, column: 3, scope: !20)
!22 = !DILocation(line: 12, column: 3, scope: !20)
!30 = distinct !DISubprogram(name: "fields", scope: !1, file: !1, line: 20, type: !4, scopeLine: 20, spFlags: DISPFlagDefinition, unit: !0)
!31 = !DILocation(line: 21, column: 12, scope: !30)
!32 = !DILocation(line: 22, column: 5, scope: !30)
!33 = !DILocation(line: 23, column: 12, scope: !30)
!34 = !DILocation(line: 24, column: 5, scope: !30)
!35 = !DILocation(line: 25, column: 1, scope: !30)
"#;

fn rows() -> Option<std::collections::BTreeMap<String, Vec<DRow>>> {
    let (dwarfdump, nm) = (tool("llvm-dwarfdump")?, tool("llvm-nm")?);
    let (module, bodies) = parse(&format!("{}{BODY}", ll_header(22)));
    let object =
        compile_to_object(&module, &bodies, &Options::default()).unwrap_or_else(|e| panic!("{e}"));
    let dir = Scratch::new("line-segments");
    let path = dir.write("seg.o", object);
    Some(rows_by_function(&dwarfdump, &nm, &path))
}

fn lines(rows: &[DRow]) -> Vec<(u32, u32)> {
    rows.iter().filter(|r| r.stmt).map(|r| (r.line, r.column)).collect()
}

// spec/dwarf.md §5.8: the code of a deferred instruction asked for by its
// user is at the deferred instruction's location.
#[test]
fn forced_code_has_its_own_row() {
    let Some(by) = rows() else { return };
    let got = lines(&by["forced"]);
    let at = |l: (u32, u32)| got.iter().position(|r| *r == l);
    let (address, store) = (at((5, 11)), at((6, 7)));
    assert!(address.is_some() && store.is_some() && address < store, "{got:?}");
}

// spec/dwarf.md §5.8: `unreachable` has no row (llc writes no code).
#[test]
fn unreachable_has_no_row() {
    let Some(by) = rows() else { return };
    let rows = &by["trap"];
    assert!(rows.iter().any(|r| r.line == 11), "{rows:#?}");
    assert!(!rows.iter().any(|r| r.line == 12), "{rows:#?}");
}

// spec/dwarf.md §5.8: a zero-offset `getelementptr` has no row of its
// own; one with an offset keeps its own location.
#[test]
fn zero_offset_address_has_no_row() {
    let Some(by) = rows() else { return };
    let rows = &by["fields"];
    assert!(!rows.iter().any(|r| r.line == 21), "{rows:#?}");
    for line in [22, 24, 25] {
        assert!(rows.iter().any(|r| r.line == line && r.stmt), "{line}: {rows:#?}");
    }
}
