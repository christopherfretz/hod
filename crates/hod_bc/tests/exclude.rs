//! `!exclude` on a global (WP-273, `spec/bc.md` §5.8 and §11.7,
//! `spec/ll.md` §3.3): both readers mark a variable whose first `!exclude` node is a
//! tuple without operands, drop one with operands and one on a function,
//! and leave no `global-metadata:exclude` note; the bitcode of `llvm-as`
//! 19 and 22 reads as the text does.

mod common;

use hod_ir::Module;

const TEXT: &[u8] = b"target triple = \"x86_64-unknown-linux-gnu\"\n\
@__mason_meta = private constant [4 x i8] c\"a\\00\\FF\\C3\", section \".mason.meta\", align 8, !exclude !0\n\
@llvm.compiler.used = appending global [1 x ptr] [ptr @__mason_meta], section \"llvm.metadata\"\n\
@plain = global i32 1, !exclude !0\n\
@distinct = global i32 1, section \".d\", !exclude !1\n\
@operands = global i32 1, section \".o\", !exclude !2\n\
@first = global i32 1, section \".f\", !exclude !2, !exclude !0\n\
@none = global i32 1, section \".n\"\n\
@decl = external global i32, !exclude !0\n\
define void @f() !exclude !0 {\n  ret void\n}\n\
declare !exclude !0 void @g()\n\
!0 = !{}\n\
!1 = distinct !{}\n\
!2 = !{i32 1}\n";

/// The names of the excluded variables, and every note.
fn summary(module: &Module) -> (Vec<String>, Vec<String>) {
    let excluded = module
        .symbols()
        .filter(hod_ir::Symbol::is_excluded)
        .map(|symbol| String::from_utf8_lossy(symbol.name()).into_owned())
        .collect();
    let notes = module.unsupported().map(|note| note.construct.to_owned()).collect();
    (excluded, notes)
}

#[test]
fn exclude_is_read_by_both_readers() {
    let (text_module, _) = common::read_text(TEXT).expect("hod_ll");
    let expected = (
        vec![
            "__mason_meta".to_owned(),
            "plain".to_owned(),
            "distinct".to_owned(),
            "decl".to_owned(),
        ],
        Vec::<String>::new(),
    );
    assert_eq!(summary(&text_module), expected);
    let mut ran = 0;
    for major in [19, 22] {
        if common::llvm_bin(major).is_none() {
            continue;
        }
        ran += 1;
        let bitcode = common::assemble(major, TEXT).expect("llvm-as");
        let (module, _) = common::read_bitcode(&bitcode).expect("hod_bc");
        assert_eq!(summary(&module), expected, "LLVM {major}");
        // A definition's own attachment is in its function block.
        let bodies = common::read_bitcode_bodies(&bitcode).expect("hod_bc bodies");
        assert!(!bodies.contains("global-metadata"), "LLVM {major}: {bodies}");
    }
    assert!(ran > 0 || std::env::var_os("HOD_REQUIRE_TOOLS").is_none(), "no llvm-as");
}

/// A node that is not a tuple is not read: the old note stays.
#[test]
fn exclude_of_another_node_is_refused() {
    const OTHER: &[u8] = b"target triple = \"x86_64-unknown-linux-gnu\"\n\
@x = global i32 1, section \".x\", !exclude !0\n\
!0 = !DIExpression()\n";
    let (module, _) = common::read_text(OTHER).expect("hod_ll");
    assert_eq!(summary(&module), (vec![], vec!["global-metadata:exclude".to_owned()]));
    for major in [19, 22] {
        if common::llvm_bin(major).is_none() {
            continue;
        }
        let Ok(bitcode) = common::assemble(major, OTHER) else { continue };
        let (module, _) = common::read_bitcode(&bitcode).expect("hod_bc");
        assert_eq!(
            summary(&module),
            (vec![], vec!["global-metadata:exclude".to_owned()]),
            "LLVM {major}"
        );
    }
}
