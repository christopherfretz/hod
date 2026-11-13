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
mod atomic;
mod bits;
mod branch;
mod bt;
mod cond;
mod coverage;
mod fixups;
mod link;
mod mem;
mod mov;
mod packed;
mod selftest;
mod shift;
mod sse;
mod system;
mod tls;

use harness::Batch;

/// A sweep: its spec section and the function adding its cases.
type Group = (&'static str, fn(&mut Batch));

/// Every sweep, by spec section.
pub const GROUPS: &[Group] = &[
    ("memory-operands", mem::build),
    ("arithmetic-and-logic", alu::build),
    ("moves-and-extensions", mov::build),
    ("shifts", shift::build),
    ("conditional", cond::build),
    ("branches-and-stack", branch::build),
    ("bit-counting", bits::build),
    ("atomics", atomic::build),
    ("system-and-padding", system::build),
    ("absolute-fields", mem::abs32),
    ("bit-tests", bt::build),
    ("scalar-floating-point", sse::build),
    ("xmm-memory-operands", sse::memory),
    ("thread-local-storage", tls::build),
    ("locked-memory-destinations", atomic::locked_memory),
    ("relocatable-immediates", mov::relocatable),
    ("packed-integer", packed::integer),
    ("packed-floating-point", packed::floating_point),
    ("packed-memory-operands", packed::memory),
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
    memory_operands_match_llvm => "memory-operands";
    arithmetic_and_logic_matches_llvm => "arithmetic-and-logic";
    moves_and_extensions_match_llvm => "moves-and-extensions";
    shifts_match_llvm => "shifts";
    conditional_matches_llvm => "conditional";
    branches_and_stack_match_llvm => "branches-and-stack";
    bit_counting_matches_llvm => "bit-counting";
    atomics_match_llvm => "atomics";
    system_and_padding_matches_llvm => "system-and-padding";
    absolute_fields_match_llvm => "absolute-fields";
    bit_tests_match_llvm => "bit-tests";
    scalar_floating_point_matches_llvm => "scalar-floating-point";
    xmm_memory_operands_match_llvm => "xmm-memory-operands";
    thread_local_storage_matches_llvm => "thread-local-storage";
    locked_memory_destinations_match_llvm => "locked-memory-destinations";
    relocatable_immediates_match_llvm => "relocatable-immediates";
    packed_integer_matches_llvm => "packed-integer";
    packed_floating_point_matches_llvm => "packed-floating-point";
    packed_memory_operands_match_llvm => "packed-memory-operands";
}
