//! L1: the encoding layer of `docs/VALIDATION.md`, for `hod_mc_x64`.
//! Contract and test method: `spec/mc-x64.md`.
//!
//! Each sweep module builds a [`Batch`] of `(bytes, assembly)` pairs for
//! one section of the spec; the tests here check each batch against LLVM's
//! assembler and disassembler, once per installed LLVM major; `selftest`
//! shows the harness reporting batches that disagree. `coverage` and the
//! pure tests of `fixups` need no tools.

#![allow(clippy::similar_names, reason = "operand names follow the Intel SDM: rr/rm/mr/ri/mi")]

#[macro_use]
mod harness;
mod table;
mod text;

mod alu;
mod mem;

use harness::Batch;

/// A sweep: its spec section and the function adding its cases.
type Group = (&'static str, fn(&mut Batch));

/// Every sweep, by spec section.
pub const GROUPS: &[Group] =
    &[("memory-operands", mem::build), ("arithmetic-and-logic", alu::build)];

fn check(group: &str) {
    let (label, build) = GROUPS.iter().find(|(label, _)| *label == group).expect("a group name");
    let mut batch = Batch::default();
    build(&mut batch);
    batch.check(label);
}

macro_rules! differential {
    ($($test:ident => $group:literal;)*) => {$(
        #[test]
        fn $test() {
            check($group);
        }
    )*};
}

differential! {
    memory_operands_match_llvm => "memory-operands";
    arithmetic_and_logic_matches_llvm => "arithmetic-and-logic";
}
