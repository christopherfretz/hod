//! Values (`spec/ir.md` §5.3): what an instruction operand can name, in
//! four bytes, and the two ids that are local to a function body.

use std::fmt;

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{AsmId, ConstId, SymId};

macro_rules! body_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            /// The id with raw value `raw`.
            #[inline]
            pub const fn new(raw: u32) -> Self {
                Self(raw)
            }

            /// The raw value.
            #[inline]
            pub const fn raw(self) -> u32 {
                self.0
            }

            /// The id as an index into its body's table.
            #[inline]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

body_id!(
    /// An instruction of a function body (§5.4).
    ///
    /// In a finished [`crate::Body`] instruction ids are positions: the
    /// instructions of a block are consecutive ids, and blocks follow each
    /// other in layout order, so `id + 1` is the next instruction of the
    /// block unless `id` is its terminator.
    InstId,
    "%"
);
body_id!(
    /// A fixed stack object of a function body (§5.2).
    SlotId,
    "slot"
);
body_id!(
    /// A metadata operand of a function body (§5.3): a string or a node
    /// of strings, an argument of an intrinsic call only.
    MetaId,
    "!"
);

/// What a [`Value`] names. Obtained from [`Value::kind`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ValueKind {
    /// The result of an instruction of the same body.
    Inst(InstId),
    /// The function's argument with this index.
    Arg(u32),
    /// A constant of the module's pool.
    Const(ConstId),
    /// The address of a symbol.
    Sym(SymId),
    /// The address of a stack slot of the same body.
    Slot(SlotId),
    /// An inline-assembly callee. Valid only as the callee of a call.
    Asm(AsmId),
    /// A metadata operand of the same body (`Body::metadata`). Valid only
    /// as an argument of an intrinsic call whose parameter is `metadata`.
    Meta(MetaId),
    /// No value: the callee of an intrinsic call, a `ret void`.
    None,
}

/// An operand: a tag and an index in four bytes (§5.3).
///
/// The index has 29 bits. A constructor given a larger index fails with
/// `TableFull`, so every `Value` that exists decodes to a [`ValueKind`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(u32);

const TAG_SHIFT: u32 = 29;
const INDEX_MASK: u32 = (1 << TAG_SHIFT) - 1;

pub(crate) const TAG_INST: u32 = 0;
pub(crate) const TAG_ARG: u32 = 1;
pub(crate) const TAG_CONST: u32 = 2;
pub(crate) const TAG_SYM: u32 = 3;
pub(crate) const TAG_SLOT: u32 = 4;
pub(crate) const TAG_ASM: u32 = 5;
pub(crate) const TAG_META: u32 = 6;

/// Instruction indices from here up name reserved values
/// ([`crate::FunctionBuilder::reserve`]); a finished body has none.
pub(crate) const RESERVED_BASE: u32 = 1 << 28;

impl Value {
    /// The largest index a value can carry.
    pub const MAX_INDEX: u32 = INDEX_MASK;

    /// No value.
    pub const NONE: Value = Value(u32::MAX);

    #[inline]
    fn make(tag: u32, index: u32, what: &str) -> IrResult<Value> {
        if index > INDEX_MASK {
            return err(
                IrErrorKind::TableFull,
                format!("{what} {index} does not fit the 29-bit index of a value"),
            );
        }
        Ok(Value((tag << TAG_SHIFT) | index))
    }

    /// The result of instruction `id`.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn inst(id: InstId) -> IrResult<Value> {
        Self::make(TAG_INST, id.0, "instruction")
    }

    /// Argument `index` of the function.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn arg(index: u32) -> IrResult<Value> {
        Self::make(TAG_ARG, index, "argument")
    }

    /// The constant `id`.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn constant(id: ConstId) -> IrResult<Value> {
        Self::make(TAG_CONST, id.raw(), "constant")
    }

    /// The address of symbol `id`.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn sym(id: SymId) -> IrResult<Value> {
        Self::make(TAG_SYM, id.raw(), "symbol")
    }

    /// The address of stack slot `id`.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn slot(id: SlotId) -> IrResult<Value> {
        Self::make(TAG_SLOT, id.0, "stack slot")
    }

    /// The inline-assembly callee `id`.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn asm(id: AsmId) -> IrResult<Value> {
        Self::make(TAG_ASM, id.raw(), "inline assembly")
    }

    /// The metadata operand `id` of the body.
    ///
    /// # Errors
    /// `TableFull` if the index does not fit 29 bits.
    #[inline]
    pub fn meta(id: MetaId) -> IrResult<Value> {
        Self::make(TAG_META, id.0, "metadata operand")
    }

    /// A value from a tag and an index already known to fit.
    #[inline]
    pub(crate) const fn from_parts(tag: u32, index: u32) -> Value {
        debug_assert!(index <= INDEX_MASK, "value index out of range");
        Value((tag << TAG_SHIFT) | index)
    }

    /// A raw word kept in an operand field or pool entry that does not hold
    /// a value (a block id, half of an offset). Read back with
    /// [`Value::bits`], never as a value.
    #[inline]
    pub(crate) const fn from_bits(raw: u32) -> Value {
        Value(raw)
    }

    /// The four bytes.
    #[inline]
    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    /// The tag: one of the `TAG_*` constants, or 7 for [`Value::NONE`].
    #[inline]
    pub(crate) const fn tag(self) -> u32 {
        self.0 >> TAG_SHIFT
    }

    /// The index under the tag.
    #[inline]
    pub(crate) const fn payload(self) -> u32 {
        self.0 & INDEX_MASK
    }

    /// What the value names.
    #[inline]
    pub const fn kind(self) -> ValueKind {
        let index = self.0 & INDEX_MASK;
        match self.0 >> TAG_SHIFT {
            TAG_INST => ValueKind::Inst(InstId(index)),
            TAG_ARG => ValueKind::Arg(index),
            TAG_CONST => ValueKind::Const(ConstId::new(index)),
            TAG_SYM => ValueKind::Sym(SymId::new(index)),
            TAG_SLOT => ValueKind::Slot(SlotId(index)),
            TAG_ASM => ValueKind::Asm(AsmId::new(index)),
            TAG_META => ValueKind::Meta(MetaId(index)),
            _ => ValueKind::None,
        }
    }

    /// Whether this is [`Value::NONE`].
    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 >> TAG_SHIFT > TAG_META
    }

    /// The metadata operand this is.
    #[inline]
    pub const fn as_meta(self) -> Option<MetaId> {
        match self.kind() {
            ValueKind::Meta(id) => Some(id),
            _ => None,
        }
    }

    /// The instruction whose result this is.
    #[inline]
    pub const fn as_inst(self) -> Option<InstId> {
        if self.0 >> TAG_SHIFT == TAG_INST { Some(InstId(self.0)) } else { None }
    }

    /// The constant this is.
    #[inline]
    pub const fn as_const(self) -> Option<ConstId> {
        match self.kind() {
            ValueKind::Const(id) => Some(id),
            _ => None,
        }
    }

    /// The symbol whose address this is.
    #[inline]
    pub const fn as_sym(self) -> Option<SymId> {
        match self.kind() {
            ValueKind::Sym(id) => Some(id),
            _ => None,
        }
    }

    /// The stack slot whose address this is.
    #[inline]
    pub const fn as_slot(self) -> Option<SlotId> {
        match self.kind() {
            ValueKind::Slot(id) => Some(id),
            _ => None,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            ValueKind::Inst(id) => write!(f, "{id:?}"),
            ValueKind::Arg(index) => write!(f, "arg{index}"),
            ValueKind::Const(id) => write!(f, "{id:?}"),
            ValueKind::Sym(id) => write!(f, "{id:?}"),
            ValueKind::Slot(id) => write!(f, "{id:?}"),
            ValueKind::Asm(id) => write!(f, "{id:?}"),
            ValueKind::Meta(id) => write!(f, "{id:?}"),
            ValueKind::None => f.write_str("none"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.3: a value is four bytes and every kind reads back.
    #[test]
    fn value_is_four_bytes_and_round_trips() {
        assert_eq!(size_of::<Value>(), 4);
        assert_eq!(size_of::<InstId>(), 4);
        assert_eq!(size_of::<SlotId>(), 4);
        let cases = [
            (Value::inst(InstId::new(7)).unwrap(), ValueKind::Inst(InstId::new(7)), "%7"),
            (Value::arg(3).unwrap(), ValueKind::Arg(3), "arg3"),
            (Value::constant(ConstId::new(9)).unwrap(), ValueKind::Const(ConstId::new(9)), "c9"),
            (Value::sym(SymId::new(1)).unwrap(), ValueKind::Sym(SymId::new(1)), "sym1"),
            (Value::slot(SlotId::new(0)).unwrap(), ValueKind::Slot(SlotId::new(0)), "slot0"),
            (Value::asm(AsmId::new(2)).unwrap(), ValueKind::Asm(AsmId::new(2)), "asm2"),
            (Value::meta(MetaId::new(4)).unwrap(), ValueKind::Meta(MetaId::new(4)), "!4"),
            (Value::NONE, ValueKind::None, "none"),
        ];
        for (index, (value, kind, text)) in cases.iter().enumerate() {
            assert_eq!(value.kind(), *kind);
            assert_eq!(format!("{value:?}"), *text);
            assert_eq!(value.is_none(), *kind == ValueKind::None);
            for (other, (different, ..)) in cases.iter().enumerate() {
                assert_eq!(value == different, index == other, "{value:?} vs {different:?}");
            }
        }
        assert_eq!(cases[0].0.as_inst(), Some(InstId::new(7)));
        assert_eq!(cases[1].0.as_inst(), None);
        assert_eq!(cases[2].0.as_const(), Some(ConstId::new(9)));
        assert_eq!(cases[3].0.as_sym(), Some(SymId::new(1)));
        assert_eq!(cases[4].0.as_slot(), Some(SlotId::new(0)));
        assert_eq!(cases[4].0.as_const(), None);
        assert_eq!(Value::NONE.as_sym(), None);
        assert_eq!(Value::NONE.as_slot(), None);
        assert_eq!(cases[6].0.as_meta(), Some(MetaId::new(4)));
        assert_eq!(Value::NONE.as_meta(), None);
    }

    /// §5.3: the index is 29 bits; a larger one is `TableFull`, not a
    /// value that reads back as something else.
    #[test]
    fn value_index_limit_is_an_error() {
        let max = Value::MAX_INDEX;
        assert_eq!(Value::arg(max).unwrap().kind(), ValueKind::Arg(max));
        assert_eq!(
            Value::constant(ConstId::new(max)).unwrap().kind(),
            ValueKind::Const(ConstId::new(max))
        );
        let over = max + 1;
        assert_eq!(Value::inst(InstId::new(over)).unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(Value::arg(over).unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(Value::constant(ConstId::new(over)).unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(Value::sym(SymId::new(u32::MAX)).unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(Value::slot(SlotId::new(over)).unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(Value::asm(AsmId::new(over)).unwrap_err().kind, IrErrorKind::TableFull);
    }
}
