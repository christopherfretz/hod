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
