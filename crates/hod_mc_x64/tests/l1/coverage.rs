//! The sweeps reach every public constructor: `spec/mc-x64.md` "Test
//! method". Needs no tools: it only builds the batches and counts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::harness::Batch;
use crate::table::CONSTRUCTORS;

/// Public `const fn`s of the crate that are not instruction constructors.
const NOT_CONSTRUCTORS: [&str; 3] = ["mov_imm_fits", "extends", "nop_len_ok"];

fn identifier(text: &str) -> &str {
    let end = text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(text.len());
    &text[..end]
}

/// The constructors the crate's source defines: `(name, has a checked
/// form)`. Free functions are `pub const fn` at the start of a line. An
/// entry of the `checked!` or `family!` macro is `pub fn` indented once,
/// with ` / try_name` after the name when it has a checked form; nothing
/// else in the crate is written that way (methods are `pub const fn`).
fn defined() -> BTreeMap<String, bool> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(&src).expect("the crate's src directory") {
        let path = entry.expect("a directory entry").path();
        let text = std::fs::read_to_string(&path).expect("a source file");
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("pub const fn ") {
                let name = identifier(rest);
                assert!(
                    found.insert(name.to_string(), false).is_none(),
                    "`{name}` is defined twice"
                );
            } else if let Some(rest) = line.strip_prefix("    pub fn ") {
                let name = identifier(rest);
                let checked = rest[name.len()..].starts_with(" / try_");
                assert!(
                    found.insert(name.to_string(), checked).is_none(),
                    "`{name}` is defined twice"
                );
            }
        }
    }
    for name in NOT_CONSTRUCTORS {
        assert!(found.remove(name).is_some(), "`{name}` is no longer defined: update the list");
    }
    found
}

#[test]
fn every_constructor_is_swept() {
    let defined = defined();
    let table: BTreeSet<&str> = CONSTRUCTORS.iter().map(|(name, _)| *name).collect();
    assert_eq!(table.len(), CONSTRUCTORS.len(), "table.rs has a duplicate row");
    let names: BTreeSet<&str> = defined.keys().map(String::as_str).collect();
    let untabled: Vec<_> = names.difference(&table).collect();
    let stale: Vec<_> = table.difference(&names).collect();
    assert!(untabled.is_empty(), "constructors without a row in table.rs: {untabled:?}");
    assert!(stale.is_empty(), "table.rs rows without a constructor: {stale:?}");

    let mut total = Batch::default();
    let mut encodings = 0;
    let mut rejections = 0;
    for (label, build) in crate::GROUPS {
        let mut batch = Batch::default();
        build(&mut batch);
        let counts = batch.counts();
        let (e, r) = counts.values().fold((0, 0), |(e, r), (ce, cr)| (e + ce, r + cr));
        println!("l1 group {label}: {e} encodings, {r} rejections");
        encodings += e;
        rejections += r;
        build(&mut total);
    }
    println!("l1 total: {encodings} encodings, {rejections} rejections");
    assert!(encodings >= 250_000, "the sweeps shrank to {encodings} encodings");

    let counts = total.counts();
    for (name, checked) in &defined {
        let (e, r) = counts.get(name.as_str()).copied().unwrap_or((0, 0));
        println!("l1 constructor {name}: {e} encodings, {r} rejections");
        assert!(e > 0, "constructor `{name}` is never swept against LLVM");
        if *checked {
            assert!(r > 0, "`try_{name}` is never shown a rejected operand");
        }
    }
}

/// The unchecked form checks its precondition where debug assertions are
/// on (`spec/mc-x64.md` "Checked and unchecked forms").
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "add_ri: operand precondition violated")]
fn an_unchecked_form_asserts_its_precondition() {
    use hod_mc_x64::{Reg, Size, add_ri};
    let _ = add_ri(Size::Q, Reg::Rax, std::hint::black_box(1 << 31));
}
