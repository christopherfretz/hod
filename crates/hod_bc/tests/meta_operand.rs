//! Metadata operands of intrinsic calls (WP-064, `spec/ir.md` §5.3,
//! `spec/ll.md` §11.6, `spec/bc.md` §11.6): both readers read a string,
//! a node of strings in place, and a node of strings by number (of the
//! module, or, in bitcode, of the function's own METADATA block, with and
//! without debug information) into the same metadata operand; anything
//! else, and any callee that is not on hod's list, stays
//! `metadata-arg:<callee>`.

mod common;

use hod_ir::{Body, InstKind, Module};

const HEAD: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
                    target triple = \"aarch64-unknown-linux-gnu\"\n\
                    declare i64 @llvm.read_register.i64(metadata)\n\
                    declare void @llvm.write_register.i64(metadata, i64)\n";

/// What reading one function gave: the metadata operand of its first
/// call as `(node, strings)`, or the body's refusals.
type Outcome = Result<(bool, Vec<String>), Vec<String>>;

fn first_meta(body: &Body) -> (bool, Vec<String>) {
    for inst in body.insts() {
        if let InstKind::Call(site) = inst.kind() {
            let meta = site.args()[0].as_meta().expect("a metadata operand");
            let meta = body.metadata(meta).expect("in the body");
            return (meta.is_node(), meta.strings().map(str::to_owned).collect());
        }
    }
    panic!("no call");
}

fn outcome(body: Option<&Body>, unsupported: &[std::borrow::Cow<'static, str>]) -> Outcome {
    match body {
        Some(body) => Ok(first_meta(body)),
        None => Err(unsupported.iter().map(ToString::to_string).collect()),
    }
}

fn verified(module: &Module, body: Option<&Body>) {
    if let Some(body) = body {
        body.verify(module).unwrap_or_else(|e| panic!("verify: {e}"));
    }
}

fn by_text(text: &str) -> Outcome {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let mut module = parsed.module;
    let mut got = None;
    hod_ll::parse_bodies(&mut module, &parsed.text, |module, read| {
        verified(module, read.body);
        got.get_or_insert_with(|| outcome(read.body, read.unsupported));
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap();
    got.expect("a body")
}

fn by_bitcode(bitcode: &[u8]) -> Outcome {
    let read = hod_bc::read_module(bitcode, &hod_bc::ReadOptions::default()).unwrap();
    let mut module = read.module;
    let mut got = None;
    hod_bc::read_bodies(&mut module, &read.bits, |module, body| {
        verified(module, body.body);
        got.get_or_insert_with(|| outcome(body.body, body.unsupported));
        Ok::<(), hod_bc::BcError>(())
    })
    .unwrap();
    got.expect("a body")
}

/// Every route agrees on `functions` with `want`.
fn agree(functions: &str, want: &Outcome) {
    let text = format!("{HEAD}{functions}");
    assert_eq!(&by_text(&text), want, "hod_ll\n{text}");
    for major in [22, 19] {
        let Ok(bitcode) = common::assemble(major, text.as_bytes()) else { continue };
        assert_eq!(&by_bitcode(&bitcode), want, "hod_bc of llvm-as {major}\n{text}");
    }
}

fn read(name: &str) -> String {
    format!(
        "define i64 @f() {{\n  %a = call i64 @llvm.read_register.i64(metadata {name})\n  ret i64 %a\n}}\n"
    )
}

fn node(strings: &[&str]) -> (bool, Vec<String>) {
    (true, strings.iter().map(|s| (*s).to_owned()).collect())
}

// §5.3: a node of strings, in place or by number, and a string.
#[test]
fn strings_and_nodes_of_strings_are_metadata_operands() {
    agree(&read("!{!\"sp\"}"), &Ok(node(&["sp"])));
    agree(&format!("{}!0 = !{{!\"cntvct_el0\"}}\n", read("!0")), &Ok(node(&["cntvct_el0"])));
    agree(&format!("{}!7 = !{{!\"a\", !\"b c\"}}\n", read("!7")), &Ok(node(&["a", "b c"])));
    agree(&read("!{}"), &Ok(node(&[])));
    agree(&read("!{!\"a\\22b\"}"), &Ok(node(&["a\"b"])));
    agree(&read("!\"fpcr\""), &Ok((false, vec!["fpcr".to_owned()])));
    // A write: the operand is the first argument too.
    agree(
        "define void @g(i64 %v) {\n  call void @llvm.write_register.i64(metadata !{!\"nzcv\"}, i64 %v)\n  ret void\n}\n",
        &Ok(node(&["nzcv"])),
    );
}

// §5.3: in bitcode, a node only one function names lives in that
// function's METADATA block; with debug information the module pass
// reads it, without it the body reader does. Both give the operand.
#[test]
fn a_function_local_node_reads_with_and_without_debug_information() {
    let plain = format!("{}!0 = !{{!\"sp\"}}\n", read("!0"));
    agree(&plain, &Ok(node(&["sp"])));
    let debug = "define i64 @f() !dbg !5 {\n  %a = call i64 @llvm.read_register.i64(metadata !0), !dbg !8\n  \
                 ret i64 %a, !dbg !8\n}\n\
                 !llvm.dbg.cu = !{!1}\n!llvm.module.flags = !{!3, !4}\n\
                 !0 = !{!\"x29\"}\n\
                 !1 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !2, emissionKind: FullDebug)\n\
                 !2 = !DIFile(filename: \"a.cpp\", directory: \"/\")\n\
                 !3 = !{i32 2, !\"Debug Info Version\", i32 3}\n!4 = !{i32 7, !\"Dwarf Version\", i32 5}\n\
                 !5 = distinct !DISubprogram(name: \"f\", scope: !2, file: !2, line: 1, type: !6, unit: !1, spFlags: DISPFlagDefinition)\n\
                 !6 = !DISubroutineType(types: !7)\n!7 = !{}\n\
                 !8 = !DILocation(line: 2, scope: !5)\n";
    agree(debug, &Ok(node(&["x29"])));
}

// §5.3, `spec/ll.md` §11.6: metadata that is not strings, a `distinct`
// node, and every callee not on hod's list stay refused by name.
#[test]
fn other_metadata_and_other_callees_stay_refused() {
    let refused = |what: &str| Err(vec![format!("metadata-arg:{what}")]);
    let callee = "llvm.read_register.i64";
    agree(&read("!{i32 1}"), &refused(callee));
    agree(&read("!{!\"sp\", i32 1}"), &refused(callee));
    agree(&read("!{!{!\"sp\"}}"), &refused(callee));
    agree(&format!("{}!0 = distinct !{{!\"sp\"}}\n", read("!0")), &refused(callee));
    agree(&read("i64 0"), &refused(callee));
    // The constrained floating-point family is not on the list.
    let text = format!(
        "{HEAD}declare double @llvm.experimental.constrained.fadd.f64(double, double, metadata, metadata)\n\
         define double @f(double %a) strictfp {{\n  %r = call double @llvm.experimental.constrained.fadd.f64(double %a, double %a, metadata !\"round.dynamic\", metadata !\"fpexcept.strict\") strictfp\n  ret double %r\n}}\n"
    );
    let want = "metadata-arg:llvm.experimental.constrained.fadd.f64".to_owned();
    let refusals = by_text(&text).expect_err("refused");
    assert!(refusals.contains(&want), "{refusals:?}");
    for major in [22, 19] {
        let Ok(bitcode) = common::assemble(major, text.as_bytes()) else { continue };
        let refusals = by_bitcode(&bitcode).expect_err("refused");
        assert!(refusals.contains(&want), "{major}: {refusals:?}");
    }
}
