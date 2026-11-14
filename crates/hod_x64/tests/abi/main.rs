//! The cross-check of `hod_x64::abi::assign` against `llc`
//! (`spec/abi-x64.md` §11): hod-assembled callers against LLVM-compiled
//! callees and the reverse, for a grid of signatures.

// Test code: short names for registers and types read better than long
// ones, and a table of cases is long.
#![allow(clippy::similar_names, clippy::many_single_char_names, clippy::too_many_lines)]

mod cc;
mod data;
mod emit;
mod errors;
mod grid;
mod ir;
mod perf;
mod run;
mod sigs;
mod table;
mod ty;
mod variadic;
mod wide;
