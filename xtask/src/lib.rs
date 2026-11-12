//! The harness behind `cargo xtask`. Contract: `spec/harness.md`.
//!
//! A library so that tests elsewhere in the workspace can call the same
//! code the command runs (the kiln end-to-end test builds its shim here).

#[cfg(unix)]
pub mod shim;
