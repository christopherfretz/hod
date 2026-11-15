//! Uniqued metadata in DIE trees (WP-228, `spec/ir.md` §6.3): kiln++
//! writes a coroutine frame's `void *` as a second node equal to the
//! first (`!142 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType:
//! null, size: 64)` beside `!17`). LLVM's parser makes the two one node,
//! so `llc`'s unit has one `void *` DIE; so must hod's. The module here
//! is that shape written by hand: a function whose parameter's type is
//! one `void *` and whose local variable's type is the other. hod's tree
//! is `llc` 19's and 22's, both objects verify, and there is one pointer
//! type DIE (hod at main 686ff25b wrote two).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use hod_testutil::{Scratch, tool};
use support::{count_tag, first_difference, hod_object, llc_object, trees};

const MODULE: &str = r#"target triple = "aarch64-unknown-linux-gnu"

define void @f(ptr %p) !dbg !3 {
entry:
  %p.addr = alloca ptr, align 8
  %q = alloca ptr, align 8
  store ptr %p, ptr %p.addr, align 8
    #dbg_declare(ptr %p.addr, !7, !DIExpression(), !10)
    #dbg_declare(ptr %q, !8, !DIExpression(), !10)
  store ptr %p, ptr %q, align 8, !dbg !10
  ret void, !dbg !11
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!12, !13}
!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln-shaped", emissionKind: FullDebug)
!1 = !DIFile(filename: "a.cpp", directory: "/")
!3 = distinct !DISubprogram(name: "f", linkageName: "_Z1fPv", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0)
!4 = !DISubroutineType(types: !{null, !5})
!5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!7 = !DILocalVariable(name: "p", arg: 1, scope: !3, file: !1, line: 1, type: !5)
!8 = !DILocalVariable(name: "q", scope: !3, file: !1, line: 2, type: !9)
!9 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!10 = !DILocation(line: 2, column: 3, scope: !3)
!11 = !DILocation(line: 3, column: 1, scope: !3)
!12 = !{i32 2, !"Debug Info Version", i32 3}
!13 = !{i32 7, !"Dwarf Version", i32 5}
"#;

#[test]
fn equal_void_pointers_are_one_die() {
    let scratch = Scratch::new("dies-uniquing");
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let h = hod_object(&scratch, "hod", MODULE);
    support::verify(&dwarfdump, &h);
    let th = trees(&dwarfdump, &h);
    assert_eq!(th.len(), 1, "one unit");
    assert_eq!(count_tag(&th[0], "DW_TAG_pointer_type"), 1, "one void *");
    let mut compared = 0;
    for name in ["llc-22", "llc-19"] {
        let Some(llc) = tool(name) else { continue };
        let l = llc_object(&scratch, &llc, name, MODULE);
        support::verify(&dwarfdump, &l);
        let tl = trees(&dwarfdump, &l);
        assert_eq!(tl.len(), 1, "{name}: one unit");
        assert_eq!(count_tag(&tl[0], "DW_TAG_pointer_type"), 1, "{name}");
        if let Some(d) = first_difference(&th[0], &tl[0]) {
            panic!("hod's tree, then {name}'s: {d}");
        }
        compared += 1;
    }
    eprintln!("equal_void_pointers_are_one_die: {compared} trees equal to llc's");
}
