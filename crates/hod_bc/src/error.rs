//! Errors (`spec/bc.md` §8).

use std::fmt;

use hod_ir::{IrError, IrErrorKind};

/// What kind of failure a [`BcError`] is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum BcErrorKind {
    /// The bytes are not a well-formed bitstream: a bad magic number, a
    /// read past the end of the file or of a block, an abbreviation that
    /// is defined badly or used before it is defined, a block that ends
    /// where it should not.
    Malformed,
    /// Well-formed bitcode in an encoding this reader does not take: an
    /// older encoding (typed pointers, version 0 or 1 value numbering, a
    /// module without a string table) or a record it does not know. The
    /// payload is the stable name of the construct (`spec/bc.md` §3.4).
    Unsupported(&'static str),
    /// A record whose contents make no sense: too few operands, an index
    /// that names nothing, an encoded field out of its range.
    Invalid,
    /// A value of the wrong type for its place; a type that contains
    /// itself.
    Type,
    /// A second definition of a symbol or a COMDAT.
    Redefined,
    /// One of the limits of [`crate::limits`] was passed.
    Limit,
    /// The module names no target and the caller gave no default.
    Target,
    /// `hod_ir` refused what the bitcode describes; the payload is its
    /// kind.
    Ir(IrErrorKind),
}

/// A read failure: what, and where in the input.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BcError {
    /// The kind of failure.
    pub kind: BcErrorKind,
    /// Bit offset in the input (from the first byte of what was passed,
    /// wrapper header included) of the record, block or field the failure
    /// was met at.
    pub bit: u64,
    /// What was wrong, in words.
    pub message: String,
}

impl fmt::Display for BcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bit {} (byte {}): ", self.bit, self.bit / 8)?;
        if let BcErrorKind::Unsupported(name) = self.kind {
            write!(f, "unsupported {name}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for BcError {}

/// The result of the crate's fallible steps.
pub type BcResult<T> = Result<T, BcError>;

/// A failure of kind `kind` at bit `bit`.
#[cold]
pub(crate) fn fail<T>(kind: BcErrorKind, bit: u64, message: impl Into<String>) -> BcResult<T> {
    Err(BcError { kind, bit, message: message.into() })
}

/// The error of `hod_ir`'s refusal `error` at bit `bit`.
#[cold]
pub(crate) fn ir_error(bit: u64, error: &IrError) -> BcError {
    BcError { kind: BcErrorKind::Ir(error.kind), bit, message: error.to_string() }
}

/// `result`, with an `hod_ir` refusal turned into a [`BcError`] at `bit`.
pub(crate) fn ir<T>(bit: u64, result: Result<T, IrError>) -> BcResult<T> {
    result.map_err(|error| ir_error(bit, &error))
}
