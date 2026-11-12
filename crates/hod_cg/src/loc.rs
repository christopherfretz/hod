//! Value classes and locations: `spec/codegen.md` §2.2, §3.

use crate::frame::FrameRef;
use crate::reg::PReg;

/// How a target holds a value of a first-class type (§2.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Class {
    /// In one general register.
    Gpr,
    /// In one floating-point/vector register.
    Fpr,
    /// Only in memory: a frame slot of the type's alloc size.
    Mem,
}

/// Where a tracked value (an instruction result or an argument) is (§3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loc {
    /// Not yet defined, or dead, or already consumed by its one use.
    None,
    /// Only in this register.
    Reg(PReg),
    /// In a frame slot: its home for a value that has one, a spill slot
    /// for a register value that was displaced.
    Frame(FrameRef),
    /// Not computed: the next instruction fuses it or asks for it (§4.6).
    Deferred,
}

/// What §4.1 decided for a value, at its first mention.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// Not mentioned yet.
    Unknown,
    /// No use.
    Dead,
    /// One use, in the defining block: lives in a register (or the spill
    /// slot it was displaced to) from its definition to its use.
    Reg,
    /// Stored to its home slot at its definition, loaded at each use.
    Home,
}

/// The per-value entry of the location table (§3): 16 bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValState {
    pub(crate) loc: Loc,
    pub(crate) kind: Kind,
    pub(crate) class: Class,
    /// The bytes glue loads and stores for the value (classes `Gpr` and
    /// `Fpr`).
    pub(crate) width: u8,
    /// The local-area offset of a `phi`'s incoming slot (§4.5);
    /// [`NO_SLOT`] until it is first needed.
    pub(crate) incoming: u32,
}

/// "No incoming slot yet."
pub(crate) const NO_SLOT: u32 = u32::MAX;

impl ValState {
    pub(crate) const UNKNOWN: ValState = ValState {
        loc: Loc::None,
        kind: Kind::Unknown,
        class: Class::Gpr,
        width: 0,
        incoming: NO_SLOT,
    };
}

/// A tracked value: an instruction result or an argument, as one word.
/// The owner of a register is one of these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Key(pub(crate) u32);

const ARG_BIT: u32 = 1 << 31;

impl Key {
    pub(crate) const NONE: Key = Key(u32::MAX);

    #[inline]
    pub(crate) fn inst(index: u32) -> Key {
        Key(index)
    }

    #[inline]
    pub(crate) fn arg(index: u32) -> Key {
        Key(index | ARG_BIT)
    }

    /// The argument index, if this is an argument.
    #[inline]
    pub(crate) fn as_arg(self) -> Option<u32> {
        (self.0 & ARG_BIT != 0).then_some(self.0 & !ARG_BIT)
    }

    /// The key of `value` if it is an instruction result or an argument.
    #[inline]
    pub(crate) fn of(value: hod_ir::Value) -> Option<Key> {
        match value.kind() {
            hod_ir::ValueKind::Inst(id) => Some(Key::inst(id.raw())),
            hod_ir::ValueKind::Arg(index) => Some(Key::arg(index)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §11: the location table costs 16 bytes per value.
    #[test]
    fn a_table_entry_is_sixteen_bytes() {
        assert_eq!(size_of::<ValState>(), 16);
        assert_eq!(Key::arg(3).as_arg(), Some(3));
        assert_eq!(Key::inst(3).as_arg(), None);
    }
}
