//! Test infrastructure of `hod_cg`, shared by its integration tests:
//! a small register machine with a `Target` implementation and an
//! executor, and the differential harness against `hod_interp`
//! (`spec/codegen.md` §12).
//!
//! None of this ships: it lives under `tests/` so that no build of the
//! library contains it.

// Each test binary uses its own part of this module.
#![allow(dead_code)]
// The executor states the machine's semantics: floats compare exactly,
// operands are `a`, `b`, `c`, and decoding is one long match.
#![allow(
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::unused_self
)]

pub mod exec;
pub mod harness;
pub mod isa;
pub mod target;
