//! What x86-64 changes in the DIEs (`spec/dwarf.md` §6.17, WP-136).
//!
//! - **Register numbers** (System V psABI, "DWARF Register Number
//!   Mapping"): what a frame base or a register location names. The code
//!   generator gives the frame base's number (`hod_x64::reg::dwarf`); the
//!   table is here so that the DIE writer and its tests name registers by
//!   the psABI's numbers, not by literals.
//! - **Thread-local variables**: `llc` for x86-64 writes their location as
//!   `DW_OP_const8u <x@DTPOFF>, DW_OP_GNU_push_tls_address` (the offset
//!   relocated by `R_X86_64_DTPOFF64` against the variable's own symbol);
//!   AArch64's `llc` writes no location, nor does hod there. Under split
//!   DWARF `llc` puts the offset in `.debug_addr` instead
//!   (`DW_OP_GNU_const_index`), which hod does not write: refused as
//!   [`crate::scope::TLS_LOCATION`].

use hod_ir::Arch;
use hod_obj::{RelocKind, X64Reloc};

use crate::Error;
use crate::die::{Reloc, Sec, Target, UnitPlan, Writer, unsupported};
use crate::scope::TLS_LOCATION;

/// `rax`.
pub const RAX: u16 = 0;
/// `rdx`.
pub const RDX: u16 = 1;
/// `rcx`.
pub const RCX: u16 = 2;
/// `rbx`.
pub const RBX: u16 = 3;
/// `rsi`.
pub const RSI: u16 = 4;
/// `rdi`.
pub const RDI: u16 = 5;
/// `rbp`: the frame base of every function hod writes (`DW_OP_reg6`).
pub const RBP: u16 = 6;
/// `rsp`.
pub const RSP: u16 = 7;
/// `r8`; `r9` to `r15` follow (9 to 15).
pub const R8: u16 = 8;
/// The return address column (`rip`).
pub const RIP: u16 = 16;
/// `xmm0`; `xmm1` to `xmm15` follow (18 to 32).
pub const XMM0: u16 = 17;
/// `st0`; `st1` to `st7` follow (34 to 40).
pub const ST0: u16 = 33;
/// `mm0`; `mm1` to `mm7` follow (42 to 48).
pub const MM0: u16 = 41;

/// The DWARF number of the general-purpose register with hardware
/// encoding `hw` (0 `rax`, 1 `rcx`, 2 `rdx`, 3 `rbx`, 4 `rsp`, 5 `rbp`,
/// 6 `rsi`, 7 `rdi`, 8 to 15 `r8` to `r15`): the psABI's order is not the
/// encoding's for the first eight. `None` beyond 15.
pub const fn gpr(hw: u8) -> Option<u16> {
    const LOW: [u16; 8] = [RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI];
    match hw {
        0..=7 => Some(LOW[hw as usize]),
        8..=15 => Some(R8 + (hw as u16 - 8)),
        _ => None,
    }
}

/// The DWARF number of `xmm<n>`; `None` beyond 15.
pub const fn xmm(n: u8) -> Option<u16> {
    if n < 16 { Some(XMM0 + n as u16) } else { None }
}

/// The DWARF number of `st<n>`; `None` beyond 7.
pub const fn st(n: u8) -> Option<u16> {
    if n < 8 { Some(ST0 + n as u16) } else { None }
}

/// The relocation of a thread-local variable's offset in its module's
/// TLS block, 8 bytes, on `arch`; `None` where `llc` writes no location.
pub const fn tls_offset_reloc(arch: hod_obj::Arch) -> Option<RelocKind> {
    match arch {
        hod_obj::Arch::X86_64 => Some(RelocKind::X64(X64Reloc::DtpOff64)),
        hod_obj::Arch::AArch64 => None,
    }
}

/// The kind of a relocation of the DIE sections: `target`'s address
/// (`wide`) or offset relocation, and a thread-local offset's
/// ([`Target::Tls`], made only where the target has one:
/// [`tls_offset_reloc`]).
pub(crate) fn reloc_kind(to: Target, wide: bool, target: &crate::target::LineTarget) -> RelocKind {
    match to {
        Target::Tls(_) => tls_offset_reloc(target.arch).unwrap_or(target.address_reloc),
        _ if wide => target.address_reloc,
        _ => target.offset_reloc,
    }
}

/// `DW_OP_const8u`.
const DW_OP_CONST8U: u8 = 0x0e;
/// `DW_OP_GNU_push_tls_address`: `llc`'s choice when it tunes for gdb,
/// the default for every ELF target but the `PlayStation`'s.
const DW_OP_GNU_PUSH_TLS_ADDRESS: u8 = 0xe0;
/// The location's length: the two ops and the 8-byte offset.
const TLS_EXPR_LEN: u8 = 10;

/// Whether a thread-local variable of a module for `arch` gets a
/// location (x86-64), so that its DIE is written with one.
pub(crate) fn tls_has_location(arch: Option<Arch>) -> bool {
    arch == Some(Arch::X86_64)
}

impl Writer<'_> {
    /// `DW_AT_location` (`DW_FORM_exprloc`) of a thread-local variable
    /// at `symbol` + `addend`: `DW_OP_const8u`, the offset relocated by
    /// [`Target::Tls`], `DW_OP_GNU_push_tls_address`.
    pub(crate) fn tls_location(&mut self, symbol: hod_obj::SymbolId, addend: i64) {
        self.info.push(TLS_EXPR_LEN);
        self.info.push(DW_OP_CONST8U);
        let offset = self.info.len();
        self.relocs.push(Reloc { sec: Sec::Info, offset, target: Target::Tls(symbol), addend });
        self.info.extend_from_slice(&[0; 8]);
        self.info.push(DW_OP_GNU_PUSH_TLS_ADDRESS);
    }
}

/// The hook of the split writer: a thread-local variable with a location
/// is refused there (its offset would be a `.debug_addr` entry).
///
/// # Errors
/// [`TLS_LOCATION`] for the first such variable.
pub(crate) fn refuse_split_tls(plan: &UnitPlan<'_>) -> Result<(), Error> {
    if !tls_has_location(plan.scope_dies.arch) {
        return Ok(());
    }
    match plan.scope_dies.vars.iter().find(|(_, shape)| shape.tls) {
        Some(&(die, _)) => Err(unsupported(plan.dies[die as usize].node, TLS_LOCATION)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The psABI's table (figure 3.36): the first eight in its order, not
    /// the encoding's.
    #[test]
    fn register_numbers() {
        let names = [(0, RAX), (1, RCX), (2, RDX), (3, RBX), (4, RSP), (5, RBP), (6, RSI)];
        for (hw, dwarf) in names {
            assert_eq!(gpr(hw), Some(dwarf));
        }
        assert_eq!((RAX, RDX, RCX, RBX, RSI, RDI, RBP, RSP), (0, 1, 2, 3, 4, 5, 6, 7));
        assert_eq!(gpr(7), Some(5));
        assert_eq!(gpr(8), Some(8));
        assert_eq!(gpr(15), Some(15));
        assert_eq!(gpr(16), None);
        assert_eq!(RIP, 16);
        assert_eq!((xmm(0), xmm(15), xmm(16)), (Some(17), Some(32), None));
        assert_eq!((st(0), st(7), st(8)), (Some(33), Some(40), None));
        assert_eq!(MM0, 41);
    }

    #[test]
    fn tls_relocation() {
        assert_eq!(
            tls_offset_reloc(hod_obj::Arch::X86_64).and_then(RelocKind::elf_name),
            Some("R_X86_64_DTPOFF64")
        );
        assert_eq!(tls_offset_reloc(hod_obj::Arch::AArch64), None);
    }
}
