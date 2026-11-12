//! The per-target constants of `.eh_frame` (`spec/eh.md` §2).

use hod_obj::elf::{SHT_PROGBITS, SHT_X86_64_UNWIND};
use hod_obj::{A64Reloc, Arch, RelocKind, X64Reloc};

use crate::Op;

/// `DW_EH_PE_*` pointer encodings (LSB 5.0 "DWARF Extensions").
pub mod pe {
    /// Signed 4-byte value.
    pub const SDATA4: u8 = 0x0b;
    /// Signed 8-byte value.
    pub const SDATA8: u8 = 0x0c;
    /// Relative to the address of the field.
    pub const PCREL: u8 = 0x10;
    /// The field holds the address of the pointer.
    pub const INDIRECT: u8 = 0x80;
}

/// What `.eh_frame` looks like for one target, as `llc` writes it for
/// Linux position-independent code (`spec/eh.md` §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfiTarget {
    /// The architecture.
    pub arch: Arch,
    /// The CIE's code alignment factor.
    pub code_align: u32,
    /// The CIE's data alignment factor.
    pub data_align: i32,
    /// The DWARF register that holds the return address.
    pub return_address: u8,
    /// The CIE's initial instructions: the rules at a function's first
    /// instruction.
    pub initial: &'static [Op],
    /// The encoding of an FDE's initial location (and so of its range).
    pub fde_encoding: u8,
    /// The encoding of the personality pointer in a CIE.
    pub personality_encoding: u8,
    /// The encoding of the LSDA pointer in an FDE.
    pub lsda_encoding: u8,
    /// The relocation of an FDE's initial location (4 bytes, PC-relative).
    pub pc_reloc: RelocKind,
    /// The relocation of the personality and LSDA pointers.
    pub pointer_reloc: RelocKind,
    /// The size of the personality and LSDA pointers, which their encodings
    /// imply.
    pub pointer_size: u8,
    /// The absolute pointer-sized relocation of a `DW.ref` slot.
    pub abs_reloc: RelocKind,
    /// The section's `sh_type`.
    pub section_type: u32,
    /// The section's alignment, which is also what the last FDE is padded
    /// to.
    pub section_align: u8,
}

/// AArch64 Linux: `sp` is DWARF 31, `x30` holds the return address,
/// instructions are 4 bytes but `llc` keeps the code alignment factor at 1.
pub const AARCH64: CfiTarget = CfiTarget {
    arch: Arch::AArch64,
    code_align: 1,
    data_align: -4,
    return_address: 30,
    initial: &[Op::DefCfa { reg: 31, offset: 0 }],
    fde_encoding: pe::PCREL | pe::SDATA4,
    personality_encoding: pe::INDIRECT | pe::PCREL | pe::SDATA8,
    lsda_encoding: pe::PCREL | pe::SDATA8,
    pc_reloc: RelocKind::A64(A64Reloc::Prel32),
    pointer_reloc: RelocKind::A64(A64Reloc::Prel64),
    pointer_size: 8,
    abs_reloc: RelocKind::A64(A64Reloc::Abs64),
    section_type: SHT_PROGBITS,
    section_align: 8,
};

/// x86-64 Linux: `rsp` is DWARF 7, the return address column is 16 (`rip`),
/// and at entry the return address is at `CFA - 8` with `CFA = rsp + 8`.
pub const X86_64: CfiTarget = CfiTarget {
    arch: Arch::X86_64,
    code_align: 1,
    data_align: -8,
    return_address: 16,
    initial: &[Op::DefCfa { reg: 7, offset: 8 }, Op::Offset { reg: 16, cfa_offset: -8 }],
    fde_encoding: pe::PCREL | pe::SDATA4,
    personality_encoding: pe::INDIRECT | pe::PCREL | pe::SDATA4,
    lsda_encoding: pe::PCREL | pe::SDATA4,
    pc_reloc: RelocKind::X64(X64Reloc::Pc32),
    pointer_reloc: RelocKind::X64(X64Reloc::Pc32),
    pointer_size: 4,
    abs_reloc: RelocKind::X64(X64Reloc::Abs64),
    section_type: SHT_X86_64_UNWIND,
    section_align: 8,
};

/// The table for `arch`.
pub const fn for_arch(arch: Arch) -> &'static CfiTarget {
    match arch {
        Arch::AArch64 => &AARCH64,
        Arch::X86_64 => &X86_64,
    }
}
