//! The per-target constants of the line table (`spec/dwarf.md` §4.2).

use hod_obj::{A64Reloc, Arch, I386Reloc, Machine, RelocKind, X64Reloc};

/// What the line table looks like for one target, as `llc` writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineTarget {
    /// The machine (WP-314: also i386).
    pub machine: Machine,
    /// `minimum_instruction_length`: address advances are in units of it.
    pub min_inst_length: u8,
    /// The size of an address in bytes.
    pub address_size: u8,
    /// The relocation of a 4-byte offset into a string section.
    pub offset_reloc: RelocKind,
    /// The relocation of an address.
    pub address_reloc: RelocKind,
}

/// AArch64.
pub const AARCH64: LineTarget = LineTarget {
    machine: Machine::AArch64,
    min_inst_length: 1,
    address_size: 8,
    offset_reloc: RelocKind::A64(A64Reloc::Abs32),
    address_reloc: RelocKind::A64(A64Reloc::Abs64),
};

/// x86-64.
pub const X86_64: LineTarget = LineTarget {
    machine: Machine::X86_64,
    min_inst_length: 1,
    address_size: 8,
    offset_reloc: RelocKind::X64(X64Reloc::Abs32),
    address_reloc: RelocKind::X64(X64Reloc::Abs64),
};

/// 32-bit x86 (WP-314, `spec/codegen-x86.md` §13): 4-byte addresses,
/// both relocations `R_386_32`.
pub const I386: LineTarget = LineTarget {
    machine: Machine::I386,
    min_inst_length: 1,
    address_size: 4,
    offset_reloc: RelocKind::I386(I386Reloc::Abs32),
    address_reloc: RelocKind::I386(I386Reloc::Abs32),
};

/// The table of `arch`.
pub const fn for_arch(arch: Arch) -> &'static LineTarget {
    match arch {
        Arch::AArch64 => &AARCH64,
        Arch::X86_64 => &X86_64,
    }
}

/// The table of `machine`.
pub const fn for_machine(machine: Machine) -> &'static LineTarget {
    match machine.arch() {
        Some(arch) => for_arch(arch),
        None => &I386,
    }
}

/// `line_base` (`spec/dwarf.md` §4.1).
pub const LINE_BASE: i8 = -5;
/// `line_range`.
pub const LINE_RANGE: u8 = 14;
/// `opcode_base`.
pub const OPCODE_BASE: u8 = 13;
/// `standard_opcode_lengths`, for opcodes 1 to 12.
pub const STANDARD_OPCODE_LENGTHS: [u8; 12] = [0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1];
