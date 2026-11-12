//! WP-062 (`spec/dwarf.md` §6.14): the compile unit's split-DWARF fields
//! (`splitDebugFilename`, `dwoId`, `splitDebugInlining`) read the same
//! from text (`hod_ll`) and from bitcode (`hod_bc`), with LLVM's default
//! for an omitted `splitDebugInlining` (`true`), and nothing about them is
//! refused any more.

mod common;

use hod_ir::{DiNode, Module};

const MAJORS: [u32; 2] = [19, 22];

/// `(splitDebugFilename, dwoId, splitDebugInlining)` of the module's
/// only unit.
fn split_fields(m: &Module) -> (Vec<u8>, u64, bool) {
    let units = m.debug.compile_units();
    assert_eq!(units.len(), 1);
    let DiNode::CompileUnit(cu) = m.debug.node(units[0]) else { panic!("not a unit") };
    (cu.split_debug_filename.to_vec(), cu.dwo_id, cu.split_debug_inlining)
}

fn text_module(text: &str) -> Module {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    parsed.module
}

fn bitcode_module(bitcode: &[u8]) -> Module {
    hod_bc::read_module(bitcode, &hod_bc::ReadOptions::default())
        .unwrap_or_else(|e| panic!("hod_bc: {e}"))
        .module
}

fn unit(fields: &str) -> String {
    format!(
        "source_filename = \"u.c\"\n\
         target triple = \"aarch64-unknown-linux-gnu\"\n\
         !llvm.dbg.cu = !{{!0}}\n\
         !llvm.module.flags = !{{!2, !3}}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"p\", \
         isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug{fields})\n\
         !1 = !DIFile(filename: \"u.c\", directory: \"/w\")\n\
         !2 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
         !3 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n"
    )
}

/// `(fields, (splitDebugFilename, dwoId, splitDebugInlining))`.
type Case = (&'static str, (&'static [u8], u64, bool));

/// What clang writes for `-gsplit-dwarf`, and what a clang module's
/// skeleton reference writes, with and without `splitDebugInlining`.
#[test]
fn split_fields_read_alike_from_text_and_bitcode() {
    let cases: [Case; 4] = [
        (", splitDebugFilename: \"a.dwo\", splitDebugInlining: false", (b"a.dwo", 0, false)),
        (
            ", splitDebugFilename: \"m.pcm\", dwoId: 1234567890123",
            (b"m.pcm", 1_234_567_890_123, true),
        ),
        ("", (b"", 0, true)),
        (", splitDebugInlining: true", (b"", 0, true)),
    ];
    for (fields, (name, id, inlining)) in cases {
        let text = unit(fields);
        let m = text_module(&text);
        assert_eq!(split_fields(&m), (name.to_vec(), id, inlining), "hod_ll: {fields}");
        assert!(m.debug.unsupported_constructs().is_empty(), "{fields}");
        // Printed back, LLVM reads the same unit (`splitDebugInlining`
        // is written even at hod's default).
        let mut printed = String::new();
        let none: [&hod_ir::Body; 0] = [];
        m.print(&mut printed, hod_ir::Dialect::LLVM22, none).unwrap_or_else(|e| panic!("{e}"));
        let again = text_module(&printed);
        assert_eq!(split_fields(&again), split_fields(&m), "printed: {fields}");
        for major in MAJORS {
            let Ok(bitcode) = common::assemble(major, text.as_bytes()) else {
                eprintln!("skipped: no llvm-as {major}");
                continue;
            };
            let b = bitcode_module(&bitcode);
            assert_eq!(split_fields(&b), split_fields(&m), "hod_bc {major}: {fields}");
        }
    }
}
