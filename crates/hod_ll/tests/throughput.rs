//! WP-096: deterministic checks of the work the reader does, one per cut
//! of `spec/landed/ll/WP-096.md`. None measures time: each counts what a
//! cut removed (bytes allocated beyond need, lookups, passes), so that a
//! change that brings the waste back fails whatever the machine's load.

mod common;

use hod_ir::Arch;
use hod_ll::{BodyParser, ParsedModule, parse_bodies};

use common::{handwritten, parse};

/// §11.1: a body handed to the caller holds no spare capacity: it is a
/// copy of exactly the size of its tables, while the parser builds in
/// storage of its own. Before WP-096 each table grew by doubling in the
/// body itself, which the session then kept.
#[test]
fn bodies_are_handed_over_without_spare_capacity() {
    let mut bodies_seen = 0;
    for (name, text) in handwritten() {
        let Ok(mut parsed) = parse(&text, Arch::X86_64) else { continue };
        let ParsedModule { module, text } = &mut parsed;
        let mut parser = BodyParser::new(text);
        for function in text.functions() {
            let Ok(Some(body)) = parser.parse(module, function) else { continue };
            assert_eq!(
                body.allocated_bytes(),
                body.table_bytes(),
                "{name}: a body of {} instructions has spare capacity",
                body.inst_count()
            );
            bodies_seen += 1;
            parser.recycle(body);
        }
    }
    assert!(bodies_seen > 50, "the hand-written modules have bodies ({bodies_seen})");
}

/// §11.1: the bodies `parse_bodies` hands out are the bodies `parse`
/// returns, with or without recycling.
#[test]
fn parse_bodies_and_parse_agree() {
    for (name, text) in handwritten() {
        let Ok(mut parsed) = parse(&text, Arch::X86_64) else { continue };
        let ParsedModule { module, text } = &mut parsed;
        let mut copy = module.clone();
        let mut from_parse = Vec::new();
        let mut parser = BodyParser::new(text);
        for function in text.functions() {
            let body = parser.parse(&mut copy, function).ok().flatten();
            from_parse.push(body.clone());
            if let Some(body) = body {
                parser.recycle(body);
            }
        }
        let mut at = 0;
        let _ = parse_bodies::<hod_ll::LlError>(module, text, |_, parsed| {
            assert_eq!(parsed.body, from_parse[at].as_ref(), "{name}: function {at}");
            at += 1;
            Ok(())
        });
    }
}

/// A module whose only debug node is `node`, numbered 0.
fn debug_module(node: &str) -> String {
    format!(
        "target triple = \"x86_64-unknown-linux-gnu\"\n\
         !llvm.module.flags = !{{!1}}\n\
         !1 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !0 = {node}\n"
    )
}

/// §7.4: a field named twice is refused at its second
/// spelling, and a required field missing at the closing parenthesis,
/// whatever the length of the name, and a node written in place inside
/// a field has fields of its own: the same name inside it is no repeat.
#[test]
fn field_rules_of_debug_nodes() {
    let refused = |node: &str, at: &str, message: &str| {
        let text = debug_module(node);
        let Err(error) = parse(text.as_bytes(), Arch::X86_64) else { panic!("{node}: accepted") };
        assert_eq!(error.message, message, "{node}");
        let offset = text.find("!0 = ").expect("the node") + 5 + node.rfind(at).expect("the place");
        assert_eq!(error.offset, offset, "{node}");
    };
    refused(
        r#"!DIFile(filename: "a", directory: "b", filename: "c")"#,
        "filename: \"c\"",
        "field 'filename' cannot be specified more than once",
    );
    refused(r#"!DIFile(filename: "a")"#, ")", "missing required field 'directory'");
    refused(
        "distinct !DICompileUnit(language: DW_LANG_C99, file: !DIFile(filename: \"a\", directory: \"b\"), \
         splitDebugInlining: false, emissionKind: FullDebug, splitDebugInlining: true)",
        "splitDebugInlining: true",
        "field 'splitDebugInlining' cannot be specified more than once",
    );
    // `name` inside the base type is not a repeat of the outer `name`;
    // the outer `tag` written again is.
    refused(
        "!DIDerivedType(tag: DW_TAG_typedef, name: \"t\", baseType: !DIBasicType(name: \"int\", \
         size: 32, encoding: DW_ATE_signed), tag: DW_TAG_typedef)",
        "tag: DW_TAG_typedef)",
        "field 'tag' cannot be specified more than once",
    );
    refused(
        "!DIDerivedType(name: \"t\", baseType: !DIBasicType(name: \"int\", size: 32, \
         encoding: DW_ATE_signed), tag: DW_TAG_typedef, name: \"u\")",
        "name: \"u\"",
        "field 'name' cannot be specified more than once",
    );
}

/// §11.1: a body of `HAND_OVER_BYTES` or more is the parser's storage
/// handed over, not a copy, so it is held once; the next body is built
/// in new storage and is a copy again.
#[test]
fn a_large_body_is_handed_over_not_copied() {
    let mut text = String::from("define void @big(ptr %p) {\n");
    for _ in 0..20_000 {
        text.push_str("  store i32 1, ptr %p, align 4\n");
    }
    text.push_str("  ret void\n}\ndefine void @small() {\n  ret void\n}\n");
    let mut parsed = parse(text.as_bytes(), Arch::X86_64).expect("the module");
    let ParsedModule { module, text } = &mut parsed;
    let mut parser = BodyParser::new(text);
    let [big, small] = text.functions() else { panic!("two functions") };
    let big = parser.parse(module, big).expect("parsed").expect("a body");
    assert!(big.table_bytes() >= hod_ll::HAND_OVER_BYTES, "{}", big.table_bytes());
    assert!(big.allocated_bytes() > big.table_bytes(), "the storage, grown by doubling");
    let small = parser.parse(module, small).expect("parsed").expect("a body");
    assert_eq!(small.allocated_bytes(), small.table_bytes());
}
