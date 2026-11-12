//! The in-process half of the round trip: LLVM text into `hod_ir` with
//! `hod_ll`, and back to LLVM text with `hod_ir`'s printer.
//! Contract: `spec/harness.md` "roundtrip", "What an entry goes through"
//! steps 1 and 2.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use hod_ir::{Dialect, Module, PrintError, SymId};
use hod_ll::{BodyParser, ParseOptions, parse_module};

use super::Outcome;

/// Where a panic happened.
#[derive(Clone, Copy)]
enum Stage {
    Parse,
    Print,
}

/// Parses `source` (module and every body) and prints it in the dialect
/// of `major`. `Err` is the entry's outcome and its detail.
pub fn ingest(source: &[u8], major: u32) -> Result<String, (Outcome, String)> {
    let dialect = Dialect::new(major)
        .map_err(|e| (Outcome::HarnessError, format!("no dialect for LLVM {major}: {e}")))?;
    let stage = Cell::new(Stage::Parse);
    match catch_unwind(AssertUnwindSafe(|| parse_and_print(source, dialect, &stage))) {
        Ok(result) => result,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic without a message".to_owned());
            let outcome = match stage.get() {
                Stage::Parse => Outcome::ParserCrashed,
                Stage::Print => Outcome::PrinterCrashed,
            };
            Err((outcome, format!("panicked: {message}")))
        }
    }
}

fn parse_and_print(
    source: &[u8],
    dialect: Dialect,
    stage: &Cell<Stage>,
) -> Result<String, (Outcome, String)> {
    let parser_error = |detail: String| (Outcome::ParserError, detail);
    let mut parsed = parse_module(source, &ParseOptions::default())
        .map_err(|e| parser_error(format!("module: {e}")))?;
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        let sym = Some(function.sym);
        match parser.parse(&mut parsed.module, function) {
            Ok(Some(body)) => {
                if let Err(e) = body.verify(&parsed.module) {
                    return Err(parser_error(format!(
                        "the body of {} does not verify: {e}",
                        symbol_name(&parsed.module, sym)
                    )));
                }
                bodies.push(body);
            }
            // Recorded against the symbol: reported below.
            Ok(None) => {}
            Err(e) => {
                return Err(parser_error(format!(
                    "body of {}: {e}",
                    symbol_name(&parsed.module, sym)
                )));
            }
        }
    }
    let module = &parsed.module;
    let constructs: BTreeSet<&str> = module.unsupported().map(|u| u.construct).collect();
    if !constructs.is_empty() {
        let list: Vec<&str> = constructs.into_iter().collect();
        return Err((Outcome::ParserUnsupported, list.join(", ")));
    }
    if let Err(e) = module.verify_symbols() {
        return Err(parser_error(format!("the module's symbols do not verify: {e}")));
    }
    stage.set(Stage::Print);
    let mut text = String::with_capacity(source.len());
    match module.print(&mut text, dialect, &bodies) {
        Ok(()) => Ok(text),
        Err(PrintError::Unsupported { construct, symbol }) => Err((
            Outcome::PrinterNotExpressible,
            format!("{construct}{}", at_symbol(module, symbol)),
        )),
        Err(PrintError::Malformed { what, symbol }) => {
            Err((Outcome::PrinterError, format!("malformed: {what}{}", at_symbol(module, symbol))))
        }
        Err(e) => Err((Outcome::PrinterError, e.to_string())),
    }
}

/// ` (in @name)`, or nothing.
fn at_symbol(module: &Module, symbol: Option<SymId>) -> String {
    symbol.map_or_else(String::new, |_| format!(" (in {})", symbol_name(module, symbol)))
}

/// `@name` of a symbol, for messages.
fn symbol_name(module: &Module, symbol: Option<SymId>) -> String {
    match symbol {
        Some(symbol) => {
            let name = String::from_utf8_lossy(module.symbol(symbol).name());
            if name.is_empty() { format!("{symbol:?}") } else { format!("@{name}") }
        }
        None => "-".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIPLE: &str = "target triple = \"aarch64-unknown-linux-gnu\"\n";

    // spec/harness.md "roundtrip", "What an entry goes through", steps 1
    // and 2: each way out of the in-process half.
    #[test]
    fn ingest_outcomes() {
        let ok = format!("{TRIPLE}define i32 @main() {{\n  ret i32 0\n}}\n");
        let text = ingest(ok.as_bytes(), 22).unwrap();
        assert!(text.contains("define") && text.contains("@main"), "{text}");
        assert_eq!(ingest(ok.as_bytes(), 19).map(|t| t.contains("@main")), Ok(true));

        let (outcome, detail) = ingest(b"this is not IR", 22).unwrap_err();
        assert_eq!(outcome, Outcome::ParserError, "{detail}");
        assert!(detail.starts_with("module: "), "{detail}");

        let bad_body = format!("{TRIPLE}define i32 @main() {{\n  ret i64 0\n}}\n");
        let (outcome, detail) = ingest(bad_body.as_bytes(), 22).unwrap_err();
        assert_eq!(outcome, Outcome::ParserError, "{detail}");
        assert!(detail.starts_with("body of @main: "), "{detail}");

        // `alloca inalloca`: a body construct the parser names
        // (spec/ll.md §3.4).
        let inalloca =
            format!("{TRIPLE}define void @f() {{\n  %a = alloca inalloca i32\n  ret void\n}}\n");
        let (outcome, detail) = ingest(inalloca.as_bytes(), 22).unwrap_err();
        assert_eq!(outcome, Outcome::ParserUnsupported, "{detail}");
        assert_eq!(detail, "alloca:inalloca");

        // A constant expression LLVM 21 and later cannot write.
        let mul = format!(
            "{TRIPLE}@g = global i64 0\n\
             @h = global i64 mul (i64 ptrtoint (ptr @g to i64), i64 3)\n"
        );
        match ingest(mul.as_bytes(), 22) {
            Err((Outcome::PrinterNotExpressible, detail)) => {
                assert!(detail.starts_with("const-expr:mul"), "{detail}");
            }
            // An LLVM 22 parser may refuse it already.
            Err((Outcome::ParserError | Outcome::ParserUnsupported, _)) => {}
            other => panic!("{other:?}"),
        }

        let (outcome, _) = ingest(ok.as_bytes(), 18).unwrap_err();
        assert_eq!(outcome, Outcome::HarnessError);
    }
}
