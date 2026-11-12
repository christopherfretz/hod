//! The harness behind `cargo xtask`. Contract: `spec/harness.md`.
//!
//! A library so that tests elsewhere in the workspace, and the harness's
//! own commands, can call the same code a command runs (the kiln
//! end-to-end test builds its shim here; later commands reuse the
//! build-link-run-compare core of [`irdiff`]).

#[cfg(unix)]
pub mod corpus;
#[cfg(unix)]
pub mod irdiff;
#[cfg(unix)]
pub mod shim;
