//! Errors (`spec/ll.md` §8).

use std::fmt;

use hod_ir::{IrError, IrErrorKind};

/// What kind of failure an [`LlError`] is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum LlErrorKind {
    /// A byte sequence that is no token: a stray character, a string or a
    /// `/* */` comment that does not end, a malformed number.
    Lex,
    /// A token that the grammar does not allow where it stands.
    Syntax,
    /// A reference to something the module does not define: a named type,
    /// a COMDAT, a metadata node, a global value, a block.
    Undefined,
    /// A second definition of a type, a COMDAT, an attribute group, a
    /// metadata node or a global value.
    Redefined,
    /// A value of the wrong type for its place, or a literal that its type
    /// cannot hold.
    Type,
    /// One of the named limits of [`crate::limits`] was passed.
    Limit,
    /// The module names no target and the caller gave no default.
    Target,
    /// `hod_ir` refused what the text describes; the payload is its kind.
    Ir(IrErrorKind),
}

/// A parse failure: what, and where in the source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LlError {
    /// The kind of failure.
    pub kind: LlErrorKind,
    /// Byte offset in the source of the token the failure was met at.
    pub offset: usize,
    /// Line of `offset`, from 1.
    pub line: u32,
    /// Column of `offset` in bytes, from 1.
    pub column: u32,
    /// What was wrong, in words.
    pub message: String,
}

impl fmt::Display for LlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for LlError {}

/// A failure before its line and column are known: the parser carries
/// only the offset, and the position is computed once, at the API
/// boundary. Boxed so that the `Ok` path of every parsing function
/// returns in registers.
#[derive(Debug)]
pub(crate) struct Fail {
    pub kind: LlErrorKind,
    pub offset: u32,
    pub message: String,
}

pub(crate) type PResult<T> = Result<T, Box<Fail>>;

pub(crate) fn fail<T>(kind: LlErrorKind, offset: u32, message: impl Into<String>) -> PResult<T> {
    Err(Box::new(Fail { kind, offset, message: message.into() }))
}

/// Wraps a refusal of `hod_ir` met at `offset`.
#[allow(clippy::unnecessary_box_returns)] // The error of `PResult` is boxed.
pub(crate) fn ir_fail(offset: u32, error: IrError) -> Box<Fail> {
    Box::new(Fail { kind: LlErrorKind::Ir(error.kind), offset, message: error.message })
}

impl Fail {
    /// Adds the line and column of the offset in `source`.
    pub(crate) fn locate(self, source: &[u8]) -> LlError {
        let offset = (self.offset as usize).min(source.len());
        let before = &source[..offset];
        let line = before.split(|&byte| byte == b'\n').count();
        let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |at| at + 1);
        LlError {
            kind: self.kind,
            offset,
            line: u32::try_from(line).unwrap_or(u32::MAX),
            column: u32::try_from(offset - line_start + 1).unwrap_or(u32::MAX),
            message: self.message,
        }
    }
}
