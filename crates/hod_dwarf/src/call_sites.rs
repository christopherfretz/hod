//! Subprograms with `DIFlagAllCallsDescribed` (`spec/dwarf.md` §6.14,
//! WP-098). clang sets it on every definition of an optimized unit
//! (`-O1` and up with `-g`, gdb tuning, DWARF 5). `llc` then writes
//! `DW_AT_call_all_calls` on the subprogram and a `DW_TAG_call_site` per
//! call (`DW_AT_call_origin`, a declaration DIE of the callee,
//! `DW_AT_call_return_pc`, the address after the call), which needs the
//! generated code's call return addresses; hod's code generator does not
//! report them yet, so such a subprogram is refused as
//! [`DEBUG_CALL_SITES`], whether or not it has a declaration (the flag is
//! the definition's). `DISPFlagOptimized` itself changes nothing in
//! `llc`'s tree off Apple targets (`DW_AT_APPLE_optimized` is an Apple
//! extension) and is written as `-O0`'s.

use hod_ir::{DiFlags, DiSubprogram};

/// A definition with `DIFlagAllCallsDescribed`: its call-site entries
/// need the code's call return addresses.
pub const DEBUG_CALL_SITES: &str = "debug:call-sites";

/// [`DEBUG_CALL_SITES`] when `sp` describes its calls.
pub(crate) fn check(sp: &DiSubprogram<'_>) -> Result<(), &'static str> {
    if sp.flags.0 & DiFlags::ALL_CALLS_DESCRIBED.0 != 0 {
        return Err(DEBUG_CALL_SITES);
    }
    Ok(())
}
