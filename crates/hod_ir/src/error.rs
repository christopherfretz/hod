//! Errors (`spec/ir.md` §1): every builder method that can be misused
//! returns one. Panics are reserved for an id from another module.

use std::fmt;

/// What an [`IrError`] reports.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IrErrorKind {
    /// A data-layout string is malformed (§2.1).
    InvalidDataLayout,
    /// A module's data layout differs from the target's in a way that
    /// changes layout (§2.1).
    DataLayoutMismatch,
    /// A target triple, or a data layout for a target, that hod has no
    /// description for (§2.1).
    UnsupportedTarget,
    /// A type that is not valid in this position: a `void` array element, a
    /// vector of structs, an integer width outside 1..=2^23 (§3).
    InvalidType,
    /// A sized type is required and this one has no size (§3).
    UnsizedType,
    /// A type's size or alignment is not representable (§3): sizes are
    /// `u64` bytes whose bit count must also fit `u64`.
    TypeTooLarge,
    /// Operand types disagree with each other or with the operation (§4).
    TypeMismatch,
    /// An operand is not valid in this position: the wrong number of
    /// aggregate elements, a float bit pattern wider than its type, an
    /// operator of the wrong class (§4).
    InvalidOperand,
    /// A struct field index that is out of range or not a constant integer
    /// (§4.2).
    BadIndex,
    /// A constant that cannot be written as static data: an `Expr`, a
    /// scalable vector, an address in a field of an unsupported width (§4).
    UnsupportedInitializer,
    /// A table has no id left for another entry (§1: ids are `u32`).
    TableFull,
    /// A symbol's name or description breaks a rule of §2.2: a linkage its
    /// kind cannot have, a non-default visibility on a local symbol, a
    /// declaration in a COMDAT, an alias of a declaration.
    InvalidSymbol,
    /// A symbol that is a definition is described or defined again (§2.2).
    SymbolRedefined,
    /// A symbol is referred to and was never declared (§2.2).
    UndefinedSymbol,
    /// A module flag is malformed, is set twice, or is a flag hod knows
    /// with a value it cannot have (§2.1).
    InvalidModuleFlag,
    /// A value reserved in a function builder was never defined (§5.3).
    UndefinedValue,
    /// The blocks of a function body are not well formed: an instruction
    /// outside a block or after a terminator, a block without a
    /// terminator, a `phi` or a landing pad out of place, a `phi` whose
    /// incoming blocks are not the block's predecessors (§5.1, §5.4).
    BadBlock,
    /// The use facts a body records differ from the ones its instructions
    /// imply (§5.6).
    UseFactsMismatch,
}

/// An error from building IR. Never a panic.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IrError {
    /// Category.
    pub kind: IrErrorKind,
    /// Human-readable description.
    pub message: String,
}

impl IrError {
    /// An error of `kind` with `message`.
    pub fn new(kind: IrErrorKind, message: impl Into<String>) -> Self {
        IrError { kind, message: message.into() }
    }
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for IrError {}

/// Shorthand result type.
pub type IrResult<T> = Result<T, IrError>;

/// `Err(IrError::new(kind, message))`, for `return err(..)`.
pub(crate) fn err<T>(kind: IrErrorKind, message: impl Into<String>) -> IrResult<T> {
    Err(IrError::new(kind, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1: an error is a kind and a message.
    #[test]
    fn error_displays_kind_and_message() {
        let e = IrError::new(IrErrorKind::InvalidType, "i0");
        assert_eq!(e.to_string(), "InvalidType: i0");
        assert_eq!(err::<()>(IrErrorKind::BadIndex, "x").unwrap_err().kind, IrErrorKind::BadIndex);
    }
}
