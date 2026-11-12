//! Function DIEs (`spec/dwarf.md` §6.4, §8.7): subprograms, parameters,
//! locals, lexical blocks and labels, compared with `llc`'s.

mod common;

use common::parse;
use hod_cg::Options;

/// Exploration aid: compiles `HOD_DIES_IN` to `HOD_DIES_OUT`.
#[test]
#[ignore = "an aid for exploring by hand"]
fn compile_by_hand() {
    let (Ok(input), Ok(output)) = (std::env::var("HOD_DIES_IN"), std::env::var("HOD_DIES_OUT"))
    else {
        return;
    };
    let text = std::fs::read_to_string(&input).unwrap();
    let (module, bodies) = parse(&text);
    let bytes = hod_a64::compile_to_object(&module, &bodies, &Options::default())
        .unwrap_or_else(|e| panic!("{input}: {e}"));
    std::fs::write(&output, bytes).unwrap();
}
