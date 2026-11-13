//! What no group package takes: the AArch64 crate's `tail` modules hold
//! the rare constructs (wide `switch`, wide `mul.with.overflow`,
//! `readcyclecounter`, ...), added once a group has landed. Nothing is
//! routed here yet; the names below are the dispatch's own refusals.

/// A funclet pad (Windows exception handling): never on this target.
pub const FUNCLET: &str = "x64:funclet";
