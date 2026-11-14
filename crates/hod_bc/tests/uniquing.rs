//! `spec/bc.md` §6.2 and `spec/ir.md` §6.3 (WP-228): equal records of
//! nodes that are not `distinct` are one node, as LLVM's bitcode reader
//! makes them.
//!
//! LLVM's writers never write two such records (their nodes are unique
//! already), so the inputs are made: `tests/data/uniquing-llvm{19,22}.bc`
//! are `llvm-as` 19 and 22 of
//!
//! ```text
//! @a = global ptr null, align 8, !dbg !10
//! @b = global ptr null, align 8, !dbg !11
//! !3 = distinct !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
//! !4 = distinct !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
//! !5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64)
//! !6 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !4, size: 64)
//! ```
//!
//! (with a compile unit listing the two variables, `@a` of type `!5` and
//! `@b` of type `!6`) with the distinct bit (bit 0 of `op0`) of the two
//! `BASIC_TYPE` records cleared in the bitstream. `llvm-dis` of either
//! prints one `DIBasicType` and one pointer type: the reader merged the
//! two basic types, then the two pointers that name them. `hod_bc` must
//! read the same, and what it reads must be what `hod_ll` reads from
//! `llvm-dis` of the file.

mod common;

use std::path::Path;

use hod_ir::{DiId, DiKind, Module};

/// The nodes of `kind` that were not merged: (all, distinct).
fn live(module: &Module, kind: DiKind) -> (usize, usize) {
    let debug = &module.debug;
    let ids = (0..debug.node_count() as u32)
        .map(DiId::new)
        .filter(|&id| debug.kind(id) == kind && !debug.is_merged(id));
    ids.fold((0, 0), |(all, distinct), id| (all + 1, distinct + usize::from(debug.is_distinct(id))))
}

#[test]
fn equal_records_are_one_node() {
    for major in [19, 22] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/data/uniquing-llvm{major}.bc"));
        let bitcode = std::fs::read(&path).unwrap();
        let read = hod_bc::read_module(&bitcode, &hod_bc::ReadOptions::default())
            .unwrap_or_else(|e| panic!("{major}: {e}"));
        let module = &read.module;
        assert_eq!(live(module, DiKind::BasicType), (1, 0), "{major}: one int");
        assert_eq!(live(module, DiKind::DerivedType), (1, 0), "{major}: one int *");
        module.verify_debug().unwrap();
        let Ok(text) = common::disassemble(major, &bitcode) else {
            eprintln!("note: no llvm-dis-{major}: the comparison with LLVM is skipped");
            continue;
        };
        let lines = String::from_utf8(text.clone()).unwrap();
        let count = |kind: &str| lines.lines().filter(|line| line.contains(kind)).count();
        assert_eq!((count("!DIBasicType("), count("!DIDerivedType(")), (1, 1), "{major}: LLVM");
        let by_text = common::read_text_bodies(&text).unwrap();
        let by_bitcode = common::read_bitcode_bodies(&bitcode).unwrap();
        assert_eq!(by_text, by_bitcode, "{major}: hod_ll of llvm-dis, hod_bc");
    }
}
