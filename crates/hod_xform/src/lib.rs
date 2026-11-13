//! IR-to-IR lowerings required before code generation.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/` for its contract.

pub mod bitset;
pub mod coro;

pub use bitset::{BitSet, Idx};
