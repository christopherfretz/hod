//! `spec/bc.md` §9.3: a handwritten module (`tests/data/module.ll`) that
//! holds most of what the module level carries, read from each installed
//! LLVM's bitcode, is the module `hod_ll` reads from `llvm-dis` of it; and
//! the dump holds what the file wrote, so the agreement is not vacuous.

mod common;

const MODULE: &[u8] = include_bytes!("data/module.ll");

fn dumps(major: u32) -> Option<(String, String)> {
    common::llvm_bin(major)?;
    let bitcode = common::assemble(major, MODULE).expect("llvm-as");
    let text = common::disassemble(major, &bitcode).expect("llvm-dis");
    let (ll, ll_numbered) = common::read_text(&text).expect("hod_ll");
    let (bc, bc_numbered) = common::read_bitcode(&bitcode).expect("hod_bc");
    Some((common::dump(&ll, &ll_numbered), common::dump(&bc, &bc_numbered)))
}

#[test]
fn handwritten_module_agrees_and_is_not_vacuous() {
    let mut ran = 0;
    for major in [19, 22] {
        let Some((ll, bc)) = dumps(major) else { continue };
        ran += 1;
        if ll != bc {
            for (a, b) in ll.lines().zip(bc.lines()).filter(|(a, b)| a != b) {
                eprintln!("ll: {a}\nbc: {b}");
            }
            panic!("LLVM {major}: hod_bc and hod_ll differ");
        }
        for want in [
            "g1 Internal",
            "section=Some(\".mysec\")",
            "comdat=Some(\"c1/Any\")",
            "comdat=Some(\"aw/Largest\")",
            "tls=InitialExec",
            "g3 External",
            "uwtable(Sync)",
            "cc=8",
            "Sret/Some(\"{i32, ptr0, [2 x i16]}\")/Some(8)",
            "AddrSpaceCast",
            "init=Half f3c00",
            "i64 18446744073709551611",
            "pic: Big",
            "i128 [0, 8000000000000000]",
            "type=X86Fp80",
            "init=Fp128 f3fff0000",
            "<4 x Float>",
            "splat(",
            "int(@g0+0)",
            "@g1-@g0",
            "c\"ab\\0\"",
            "blockaddress(@f",
            "dso_local_equivalent @decl",
            "no_cfi @decl",
            "init=@vt+16",
            "a1 ",
            "alias of",
            "ifunc resolved by resolver",
            "personality=Some(\"pers\")",
            "body=true",
            "unsupported type:ppc_fp128",
            "unsupported const-expr:ptrauth",
            "<anonymous Some(0)>",
            "unsupported symbol:anonymous-external Some(\"__unnamed_2\")",
            "ctor 65535 f None",
            "ctor 101 aw Some(\"g0\")",
            "linker option [\"-lfoo\", \"-lbar\"]",
            "library [\"m\"]",
            "module asm \"nop\\n.globl asm_symbol\\n\"",
            "flag ",
            "used=true",
            "compiler_used=true",
        ] {
            assert!(bc.contains(want), "LLVM {major}: the dump lacks {want:?}:\n{bc}");
        }
    }
    if ran == 0 {
        eprintln!("skipped: no LLVM 19 or 22 installed");
    }
}
