//! LLVM bitcode reader producing `hod_ir`.
//!
//! The contract is `spec/bc.md`. This crate reads the bitstream container
//! ([`bitstream`]) and the module level of the bitcode LLVM 19 to 22
//! write into a `hod_ir::Module`, making the decisions `hod_ll` makes for
//! the same module as text (`spec/ll.md`), so that the two front doors
//! deliver the same module. Function bodies are found, stepped over by
//! their length and handed on as bit spans ([`ModuleBits::functions`])
//! with the tables a body reader needs.

pub mod bitstream;
pub mod codes;
mod error;
mod raw;

pub use error::{BcError, BcErrorKind, BcResult};

/// The limits an input may not pass (`spec/bc.md` §8); passing one is a
/// [`BcErrorKind::Limit`] error.
pub mod limits {
    /// The most blocks open at once. LLVM nests four deep.
    pub const MAX_BLOCK_DEPTH: usize = 64;

    /// The most abbreviation operands one block (with BLOCKINFO) defines.
    pub const MAX_ABBREV_OPS: usize = 1 << 20;
}

/// Walks the blocks of `data` without building anything: the first pass
/// alone, for tests and timing.
///
/// # Errors
/// As the first pass of the module read.
#[doc(hidden)]
pub fn walk_only(data: &[u8]) -> BcResult<usize> {
    raw::walk(data).map(|raw| raw.pool.len())
}
