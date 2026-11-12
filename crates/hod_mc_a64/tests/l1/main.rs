//! L1: the encoding layer of `docs/VALIDATION.md`, for `hod_mc_a64`.
//! Contract and test method: `spec/mc-a64.md`.
//!
//! Each sweep module builds a [`Batch`] of `(word, assembly)` pairs for one
//! section of the spec; the tests here check each batch against LLVM's
//! assembler and disassembler; `selftest` shows the harness reporting
//! batches that disagree. `bitmask`, `movimm`, `coverage` and the pure tests
//! of `fixups` and `fpimm` need no tools.

#![allow(
    clippy::similar_names,
    reason = "operand names follow the Arm ARM: rn/rm, rt/rt2, immr/imms, movz/movn"
)]

#[macro_use]
mod harness;
mod table;
mod text;

mod arith;
mod atomic;
mod bitfield;
mod bitmask;
mod branch;
mod condsel;
mod coverage;
mod fixups;
mod fp;
mod fpimm;
mod fpmem;
mod logical;
mod mem;
mod movimm;
mod selftest;
mod simd;
mod system;
mod wide;

use harness::Batch;

/// A sweep: its spec section and the function adding its cases.
type Group = (&'static str, fn(&mut Batch));

/// Every sweep, by spec section.
pub const GROUPS: &[Group] = &[
    ("arithmetic", arith::build),
    ("logical", logical::build),
    ("logical-immediates", bitmask::build),
    ("moves", wide::build),
    ("constant-materialization", movimm::build),
    ("shifts-and-bitfields", bitfield::build),
    ("conditional", condsel::build),
    ("branches-and-addresses", branch::build),
    ("fixups", fixups::build),
    ("loads-and-stores", mem::build),
    ("atomics", atomic::build),
    ("system", system::build),
    ("scalar-floating-point", fp::build),
    ("fp-constant-materialization", fpimm::build),
    ("fp-simd-loads-and-stores", fpmem::build),
    ("simd", simd::build),
];

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
    arithmetic_matches_llvm => "arithmetic";
    logical_matches_llvm => "logical";
    logical_immediates_match_llvm => "logical-immediates";
    moves_match_llvm => "moves";
    constant_materialization_matches_llvm => "constant-materialization";
    shifts_and_bitfields_match_llvm => "shifts-and-bitfields";
    conditional_matches_llvm => "conditional";
    branches_and_addresses_match_llvm => "branches-and-addresses";
    fixups_match_llvm => "fixups";
    loads_and_stores_match_llvm => "loads-and-stores";
    atomics_match_llvm => "atomics";
    system_matches_llvm => "system";
    scalar_floating_point_matches_llvm => "scalar-floating-point";
    fp_constant_materialization_matches_llvm => "fp-constant-materialization";
    fp_simd_loads_and_stores_match_llvm => "fp-simd-loads-and-stores";
    simd_matches_llvm => "simd";
}
