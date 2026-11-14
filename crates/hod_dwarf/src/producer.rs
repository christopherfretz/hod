//! The unit's `DW_AT_producer` (`spec/dwarf.md` §6.30, WP-221).
//!
//! With `-grecord-command-line` (`-grecord-gcc-switches`) the front end
//! records its command line as the compile unit's `flags:`, and `llc`
//! 19 and 22 (`DwarfDebug::constructDwarfCompileUnit`, off Apple
//! targets) write the producer, a space and the flags as the one
//! producer string: in the ordinary unit, the split unit and the
//! skeleton that stands alone, in version 4 and 5 alike. Empty flags
//! leave the producer as it is (an empty producer stays empty); an
//! empty producer with flags gives a leading space.

use std::borrow::Cow;

use hod_ir::DiCompileUnit;

/// The producer string `llc` writes for `cu`: borrowed from the unit
/// when it has no flags, so a unit without a recorded command line
/// allocates nothing.
pub(crate) fn producer<'a>(cu: &DiCompileUnit<'a>) -> Cow<'a, [u8]> {
    if cu.flags.is_empty() {
        return Cow::Borrowed(cu.producer);
    }
    let mut s = Vec::with_capacity(cu.producer.len() + 1 + cu.flags.len());
    s.extend_from_slice(cu.producer);
    s.push(b' ');
    s.extend_from_slice(cu.flags);
    Cow::Owned(s)
}
