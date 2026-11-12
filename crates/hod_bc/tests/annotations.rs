//! `spec/bc.md` §5.5 (WP-095): `llvm.global.annotations` in section
//! `llvm.metadata` is dropped, as `llc` emits nothing of it; anywhere else
//! it is noted `special-global:llvm.global.annotations`. The same holds
//! for `hod_ll` (`crates/hod_a64/tests/tail2.rs`).

mod common;

const ANNOTATED: &str = r#"
target triple = "aarch64-unknown-linux-gnu"
@.str = private unnamed_addr constant [5 x i8] c"note\00", section "llvm.metadata"
@.file = private unnamed_addr constant [4 x i8] c"a.c\00", section "llvm.metadata"
@llvm.global.annotations = appending global [1 x { ptr, ptr, ptr, i32, ptr }] [{ ptr, ptr, ptr, i32, ptr } { ptr @f, ptr @.str, ptr @.file, i32 3, ptr null }], section "SECTION"

define i32 @f() {
  ret i32 0
}
"#;

fn notes(section: &str) -> Option<Vec<String>> {
    let text = ANNOTATED.replace("SECTION", section);
    let mut found = None;
    for major in hod_testutil::LLVM_MAJORS {
        if common::llvm_bin(major).is_none() {
            continue;
        }
        let bitcode = common::assemble(major, text.as_bytes()).expect("llvm-as");
        let read = hod_bc::read_module(&bitcode, &hod_bc::ReadOptions::default()).expect("hod_bc");
        let notes: Vec<String> =
            read.module.unsupported().map(|n| n.construct.to_string()).collect();
        assert!(found.as_ref().is_none_or(|f| f == &notes), "LLVM {major}: {notes:?}");
        found = Some(notes);
    }
    found
}

#[test]
fn annotations_in_llvm_metadata_are_dropped() {
    if let Some(notes) = notes("llvm.metadata") {
        assert!(notes.is_empty(), "{notes:?}");
    }
}

#[test]
fn annotations_elsewhere_are_noted() {
    if let Some(notes) = notes("other") {
        assert!(notes.iter().any(|n| n == "special-global:llvm.global.annotations"), "{notes:?}");
    }
}
