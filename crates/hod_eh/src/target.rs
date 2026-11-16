//! The per-target constants of `.eh_frame` (`spec/eh.md` §2).

use hod_obj::elf::{SHT_PROGBITS, SHT_X86_64_UNWIND};
use hod_obj::{A64Reloc, Arch, I386Reloc, Machine, RelocKind, X64Reloc};

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
    /// The machine (WP-314: a field `machine`, so that 32-bit x86, which
    /// has no [`Arch`], has a table).
    pub machine: Machine,
    /// The size of an address: of a type-info stub, a `DW.ref` slot and
    /// `.debug_frame`'s locations and ranges (8, or 4 on 32-bit x86).
    pub address_size: u8,
    /// The 4-byte absolute relocation (`.debug_frame`'s CIE pointer).
    pub abs32_reloc: RelocKind,
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
    /// The relocation of an FDE's initial location (PC-relative, of
    /// [`CfiTarget::fde_size`] bytes).
    pub pc_reloc: RelocKind,
    /// The relocation of the personality and LSDA pointers.
    pub pointer_reloc: RelocKind,
    /// The size of the personality and LSDA pointers, which their encodings
    /// imply.
    pub pointer_size: u8,
    /// The absolute address-sized relocation of a `DW.ref` slot, a
    /// type-info stub and a `.debug_frame` location.
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
    machine: Machine::AArch64,
    address_size: 8,
    abs32_reloc: RelocKind::A64(A64Reloc::Abs32),
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

/// AArch64 Linux under the large code model (`spec/eh.md` §2.1, WP-209):
/// as [`AARCH64`], the FDE's initial location and range 8 bytes
/// (`DW_EH_PE_pcrel|sdata8`, `R_AARCH64_PREL64`), as `llc` 19 and 22
/// write them outside PIC (`MCObjectFileInfo::initELFMCObjectFileInfo`);
/// the personality and LSDA pointers are 8 bytes already.
pub const AARCH64_LARGE: CfiTarget = CfiTarget {
    fde_encoding: pe::PCREL | pe::SDATA8,
    pc_reloc: RelocKind::A64(A64Reloc::Prel64),
    ..AARCH64
};

/// x86-64 Linux: `rsp` is DWARF 7, the return address column is 16 (`rip`),
/// and at entry the return address is at `CFA - 8` with `CFA = rsp + 8`.
pub const X86_64: CfiTarget = CfiTarget {
    machine: Machine::X86_64,
    address_size: 8,
    abs32_reloc: RelocKind::X64(X64Reloc::Abs32),
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

/// x86-64 Linux under the large code model (`spec/eh.md` §2.1, WP-209):
/// as [`X86_64`], every pointer 8 bytes (`DW_EH_PE_sdata8`, so that no
/// field assumes the code within 2 GiB of `.eh_frame` or of
/// `.gcc_except_table`): FDE locations and ranges, the personality, the
/// LSDA and the type-info entries, relocated by `R_X86_64_PC64`. These
/// are `llc`'s encodings for position-independent code under the large
/// model (`MCObjectFileInfo::initELFMCObjectFileInfo`).
pub const X86_64_LARGE: CfiTarget = CfiTarget {
    fde_encoding: pe::PCREL | pe::SDATA8,
    personality_encoding: pe::INDIRECT | pe::PCREL | pe::SDATA8,
    lsda_encoding: pe::PCREL | pe::SDATA8,
    pc_reloc: RelocKind::X64(X64Reloc::Pc64),
    pointer_reloc: RelocKind::X64(X64Reloc::Pc64),
    pointer_size: 8,
    ..X86_64
};

/// 32-bit x86 Linux (WP-314, design 0008 §1.6): `esp` is DWARF 4, the
/// return address column is 8 (`eip`), at entry `CFA = esp + 4` and the
/// return address is at `CFA - 4`. The encodings are those `llc` 19 and
/// 22 write under PIC (`-relocation-model=pic`): FDE locations, the LSDA
/// and the type-info entries `pcrel|sdata4`, the personality
/// `indirect|pcrel|sdata4` through its `DW.ref` slot, all relocated by
/// `R_386_PC32`; hod writes them under every model, as on x86-64 (`llc`'s
/// static encodings are absolute, `R_386_32`: both are correct in a
/// static link, and the PC-relative ones also in a PIE or a shared
/// object). The section is `SHT_PROGBITS`, aligned to 4.
pub const I386: CfiTarget = CfiTarget {
    machine: Machine::I386,
    address_size: 4,
    abs32_reloc: RelocKind::I386(I386Reloc::Abs32),
    code_align: 1,
    data_align: -4,
    return_address: 8,
    initial: &[Op::DefCfa { reg: 4, offset: 4 }, Op::Offset { reg: 8, cfa_offset: -4 }],
    fde_encoding: pe::PCREL | pe::SDATA4,
    personality_encoding: pe::INDIRECT | pe::PCREL | pe::SDATA4,
    lsda_encoding: pe::PCREL | pe::SDATA4,
    pc_reloc: RelocKind::I386(I386Reloc::Pc32),
    pointer_reloc: RelocKind::I386(I386Reloc::Pc32),
    pointer_size: 4,
    abs_reloc: RelocKind::I386(I386Reloc::Abs32),
    section_type: SHT_PROGBITS,
    section_align: 4,
};

/// The table for `machine`, under the large code model when `large`
/// (64-bit machines only: [`for_model`]); [`I386`] for 32-bit x86.
pub const fn for_machine(machine: Machine, large: bool) -> &'static CfiTarget {
    match machine.arch() {
        Some(arch) => for_model(arch, large),
        None => &I386,
    }
}

impl CfiTarget {
    /// The size of an FDE's initial location and range: 8 for
    /// `DW_EH_PE_sdata8`, else 4.
    pub const fn fde_size(&self) -> usize {
        if self.fde_encoding & 0x0f == pe::SDATA8 { 8 } else { 4 }
    }
}

/// The table for `arch`.
pub const fn for_arch(arch: Arch) -> &'static CfiTarget {
    match arch {
        Arch::AArch64 => &AARCH64,
        Arch::X86_64 => &X86_64,
    }
}

/// The table for `arch`, under the large code model when `large`
/// ([`X86_64_LARGE`], [`AARCH64_LARGE`]).
pub const fn for_model(arch: Arch, large: bool) -> &'static CfiTarget {
    match arch {
        Arch::X86_64 if large => &X86_64_LARGE,
        Arch::AArch64 if large => &AARCH64_LARGE,
        _ => for_arch(arch),
    }
}
