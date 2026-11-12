//! Relocation kinds. Contract: `spec/object.md` "Relocations".
//!
//! Kinds are named after the ELF relocation they denote: "ELF for the Arm
//! 64-bit Architecture (AArch64)", section "Relocation", and the "System V
//! ABI, AMD64 Architecture Processor Supplement", section "Relocation
//! Types".

use crate::model::Arch;

/// Declares a relocation-kind enum: one variant per named relocation with
/// its ELF type number and psABI name, plus `Other(u32)`.
macro_rules! reloc_kinds {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $variant:ident = $number:literal, $elf_name:literal; )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("`", $elf_name, "` (", stringify!($number), ").")]
                $variant,
            )*
            /// Any other relocation type, by its ELF number. A number that
            /// has a named variant should be given as that variant:
            /// `Other(n)` and the variant are written identically but do
            /// not compare equal.
            Other(u32),
        }

        impl $name {
            /// Every named kind, in declaration order.
            pub const NAMED: &[Self] = &[ $( Self::$variant, )* ];

            /// The ELF relocation type number (the low 32 bits of `r_info`).
            pub const fn elf_type(self) -> u32 {
                match self {
                    $( Self::$variant => $number, )*
                    Self::Other(number) => number,
                }
            }

            /// The kind for an ELF relocation type number: the named
            /// variant when there is one, else `Other`.
            pub const fn from_elf_type(number: u32) -> Self {
                match number {
                    $( $number => Self::$variant, )*
                    other => Self::Other(other),
                }
            }

            /// The psABI name, such as `R_AARCH64_CALL26`; `None` for
            /// `Other`.
            pub const fn elf_name(self) -> Option<&'static str> {
                match self {
                    $( Self::$variant => Some($elf_name), )*
                    Self::Other(_) => None,
                }
            }
        }
    };
}

reloc_kinds! {
    /// AArch64 relocations ("ELF for the Arm 64-bit Architecture",
    /// "Relocation types").
    A64Reloc {
        None = 0, "R_AARCH64_NONE";
        // Static data relocations.
        Abs64 = 257, "R_AARCH64_ABS64";
        Abs32 = 258, "R_AARCH64_ABS32";
        Abs16 = 259, "R_AARCH64_ABS16";
        Prel64 = 260, "R_AARCH64_PREL64";
        Prel32 = 261, "R_AARCH64_PREL32";
        Prel16 = 262, "R_AARCH64_PREL16";
        // Group relocations to build an unsigned value with MOVZ/MOVK.
        MovwUabsG0 = 263, "R_AARCH64_MOVW_UABS_G0";
        MovwUabsG0Nc = 264, "R_AARCH64_MOVW_UABS_G0_NC";
        MovwUabsG1 = 265, "R_AARCH64_MOVW_UABS_G1";
        MovwUabsG1Nc = 266, "R_AARCH64_MOVW_UABS_G1_NC";
        MovwUabsG2 = 267, "R_AARCH64_MOVW_UABS_G2";
        MovwUabsG2Nc = 268, "R_AARCH64_MOVW_UABS_G2_NC";
        MovwUabsG3 = 269, "R_AARCH64_MOVW_UABS_G3";
        // PC-relative addressing.
        LdPrelLo19 = 273, "R_AARCH64_LD_PREL_LO19";
        AdrPrelLo21 = 274, "R_AARCH64_ADR_PREL_LO21";
        AdrPrelPgHi21 = 275, "R_AARCH64_ADR_PREL_PG_HI21";
        AdrPrelPgHi21Nc = 276, "R_AARCH64_ADR_PREL_PG_HI21_NC";
        AddAbsLo12Nc = 277, "R_AARCH64_ADD_ABS_LO12_NC";
        Ldst8AbsLo12Nc = 278, "R_AARCH64_LDST8_ABS_LO12_NC";
        // Control flow.
        TstBr14 = 279, "R_AARCH64_TSTBR14";
        CondBr19 = 280, "R_AARCH64_CONDBR19";
        Jump26 = 282, "R_AARCH64_JUMP26";
        Call26 = 283, "R_AARCH64_CALL26";
        Ldst16AbsLo12Nc = 284, "R_AARCH64_LDST16_ABS_LO12_NC";
        Ldst32AbsLo12Nc = 285, "R_AARCH64_LDST32_ABS_LO12_NC";
        Ldst64AbsLo12Nc = 286, "R_AARCH64_LDST64_ABS_LO12_NC";
        Ldst128AbsLo12Nc = 299, "R_AARCH64_LDST128_ABS_LO12_NC";
        // GOT-relative.
        AdrGotPage = 311, "R_AARCH64_ADR_GOT_PAGE";
        Ld64GotLo12Nc = 312, "R_AARCH64_LD64_GOT_LO12_NC";
        Plt32 = 314, "R_AARCH64_PLT32";
        GotPcrel32 = 315, "R_AARCH64_GOTPCREL32";
        // Thread-local storage, general dynamic.
        TlsgdAdrPage21 = 513, "R_AARCH64_TLSGD_ADR_PAGE21";
        TlsgdAddLo12Nc = 514, "R_AARCH64_TLSGD_ADD_LO12_NC";
        // Thread-local storage, initial exec.
        TlsieMovwGottprelG1 = 539, "R_AARCH64_TLSIE_MOVW_GOTTPREL_G1";
        TlsieMovwGottprelG0Nc = 540, "R_AARCH64_TLSIE_MOVW_GOTTPREL_G0_NC";
        TlsieAdrGottprelPage21 = 541, "R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21";
        TlsieLd64GottprelLo12Nc = 542, "R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC";
        TlsieLdGottprelPrel19 = 543, "R_AARCH64_TLSIE_LD_GOTTPREL_PREL19";
        // Thread-local storage, local exec.
        TlsleMovwTprelG2 = 544, "R_AARCH64_TLSLE_MOVW_TPREL_G2";
        TlsleMovwTprelG1 = 545, "R_AARCH64_TLSLE_MOVW_TPREL_G1";
        TlsleMovwTprelG1Nc = 546, "R_AARCH64_TLSLE_MOVW_TPREL_G1_NC";
        TlsleMovwTprelG0 = 547, "R_AARCH64_TLSLE_MOVW_TPREL_G0";
        TlsleMovwTprelG0Nc = 548, "R_AARCH64_TLSLE_MOVW_TPREL_G0_NC";
        TlsleAddTprelHi12 = 549, "R_AARCH64_TLSLE_ADD_TPREL_HI12";
        TlsleAddTprelLo12 = 550, "R_AARCH64_TLSLE_ADD_TPREL_LO12";
        TlsleAddTprelLo12Nc = 551, "R_AARCH64_TLSLE_ADD_TPREL_LO12_NC";
        TlsleLdst8TprelLo12 = 552, "R_AARCH64_TLSLE_LDST8_TPREL_LO12";
        TlsleLdst8TprelLo12Nc = 553, "R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC";
        TlsleLdst16TprelLo12 = 554, "R_AARCH64_TLSLE_LDST16_TPREL_LO12";
        TlsleLdst16TprelLo12Nc = 555, "R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC";
        TlsleLdst32TprelLo12 = 556, "R_AARCH64_TLSLE_LDST32_TPREL_LO12";
        TlsleLdst32TprelLo12Nc = 557, "R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC";
        TlsleLdst64TprelLo12 = 558, "R_AARCH64_TLSLE_LDST64_TPREL_LO12";
        TlsleLdst64TprelLo12Nc = 559, "R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC";
        // Thread-local storage, descriptors.
        TlsdescLdPrel19 = 560, "R_AARCH64_TLSDESC_LD_PREL19";
        TlsdescAdrPrel21 = 561, "R_AARCH64_TLSDESC_ADR_PREL21";
        TlsdescAdrPage21 = 562, "R_AARCH64_TLSDESC_ADR_PAGE21";
        TlsdescLd64Lo12 = 563, "R_AARCH64_TLSDESC_LD64_LO12";
        TlsdescAddLo12 = 564, "R_AARCH64_TLSDESC_ADD_LO12";
        TlsdescOffG1 = 565, "R_AARCH64_TLSDESC_OFF_G1";
        TlsdescOffG0Nc = 566, "R_AARCH64_TLSDESC_OFF_G0_NC";
        TlsdescLdr = 567, "R_AARCH64_TLSDESC_LDR";
        TlsdescAdd = 568, "R_AARCH64_TLSDESC_ADD";
        TlsdescCall = 569, "R_AARCH64_TLSDESC_CALL";
        TlsleLdst128TprelLo12 = 570, "R_AARCH64_TLSLE_LDST128_TPREL_LO12";
        TlsleLdst128TprelLo12Nc = 571, "R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC";
    }
}

reloc_kinds! {
    /// x86-64 relocations ("System V ABI, AMD64 Architecture Processor
    /// Supplement", "Relocation Types").
    X64Reloc {
        None = 0, "R_X86_64_NONE";
        Abs64 = 1, "R_X86_64_64";
        Pc32 = 2, "R_X86_64_PC32";
        Plt32 = 4, "R_X86_64_PLT32";
        GotPcrel = 9, "R_X86_64_GOTPCREL";
        Abs32 = 10, "R_X86_64_32";
        Abs32S = 11, "R_X86_64_32S";
        Abs16 = 12, "R_X86_64_16";
        Pc16 = 13, "R_X86_64_PC16";
        Abs8 = 14, "R_X86_64_8";
        Pc8 = 15, "R_X86_64_PC8";
        DtpOff64 = 17, "R_X86_64_DTPOFF64";
        TpOff64 = 18, "R_X86_64_TPOFF64";
        TlsGd = 19, "R_X86_64_TLSGD";
        TlsLd = 20, "R_X86_64_TLSLD";
        DtpOff32 = 21, "R_X86_64_DTPOFF32";
        GotTpOff = 22, "R_X86_64_GOTTPOFF";
        TpOff32 = 23, "R_X86_64_TPOFF32";
        Pc64 = 24, "R_X86_64_PC64";
        GotOff64 = 25, "R_X86_64_GOTOFF64";
        GotPc32 = 26, "R_X86_64_GOTPC32";
        GotPc32Tlsdesc = 34, "R_X86_64_GOTPC32_TLSDESC";
        TlsdescCall = 35, "R_X86_64_TLSDESC_CALL";
        GotPcrelX = 41, "R_X86_64_GOTPCRELX";
        RexGotPcrelX = 42, "R_X86_64_REX_GOTPCRELX";
    }
}

/// A relocation kind of either architecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelocKind {
    /// An AArch64 relocation.
    A64(A64Reloc),
    /// An x86-64 relocation.
    X64(X64Reloc),
}

impl RelocKind {
    /// The architecture the kind belongs to.
    pub const fn arch(self) -> Arch {
        match self {
            Self::A64(_) => Arch::AArch64,
            Self::X64(_) => Arch::X86_64,
        }
    }

    /// The ELF relocation type number.
    pub const fn elf_type(self) -> u32 {
        match self {
            Self::A64(kind) => kind.elf_type(),
            Self::X64(kind) => kind.elf_type(),
        }
    }

    /// The kind `arch` gives to an ELF relocation type number.
    pub const fn from_elf_type(arch: Arch, number: u32) -> Self {
        match arch {
            Arch::AArch64 => Self::A64(A64Reloc::from_elf_type(number)),
            Arch::X86_64 => Self::X64(X64Reloc::from_elf_type(number)),
        }
    }

    /// The psABI name; `None` for `Other`.
    pub const fn elf_name(self) -> Option<&'static str> {
        match self {
            Self::A64(kind) => kind.elf_name(),
            Self::X64(kind) => kind.elf_name(),
        }
    }
}

impl From<A64Reloc> for RelocKind {
    fn from(kind: A64Reloc) -> Self {
        Self::A64(kind)
    }
}

impl From<X64Reloc> for RelocKind {
    fn from(kind: X64Reloc) -> Self {
        Self::X64(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/object.md` "Relocations": numbers round-trip, and no two named
    /// kinds share a number or a name.
    #[test]
    fn named_kinds_round_trip_and_are_distinct() {
        fn check<K: Copy + PartialEq + std::fmt::Debug>(
            named: &[K],
            number: impl Fn(K) -> u32,
            back: impl Fn(u32) -> K,
            name: impl Fn(K) -> Option<&'static str>,
            prefix: &str,
        ) {
            let mut seen_numbers = std::collections::BTreeSet::new();
            let mut seen_names = std::collections::BTreeSet::new();
            for &kind in named {
                assert_eq!(back(number(kind)), kind);
                assert!(seen_numbers.insert(number(kind)), "{kind:?} repeats a number");
                let name = name(kind).unwrap();
                assert!(name.starts_with(prefix), "{name}");
                assert!(seen_names.insert(name), "{name} repeats");
            }
        }
        check(
            A64Reloc::NAMED,
            A64Reloc::elf_type,
            A64Reloc::from_elf_type,
            A64Reloc::elf_name,
            "R_AARCH64_",
        );
        check(
            X64Reloc::NAMED,
            X64Reloc::elf_type,
            X64Reloc::from_elf_type,
            X64Reloc::elf_name,
            "R_X86_64_",
        );
    }

    /// The numbers the code generator depends on first, against the psABI
    /// tables. (The whole table is checked against `llvm-readelf` and
    /// binutils `readelf` in `tests/tools.rs`.)
    #[test]
    fn well_known_numbers() {
        assert_eq!(A64Reloc::Abs64.elf_type(), 257);
        assert_eq!(A64Reloc::AdrPrelPgHi21.elf_type(), 275);
        assert_eq!(A64Reloc::AddAbsLo12Nc.elf_type(), 277);
        assert_eq!(A64Reloc::Jump26.elf_type(), 282);
        assert_eq!(A64Reloc::Call26.elf_type(), 283);
        assert_eq!(A64Reloc::AdrGotPage.elf_type(), 311);
        assert_eq!(A64Reloc::TlsdescCall.elf_type(), 569);
        assert_eq!(X64Reloc::Abs64.elf_type(), 1);
        assert_eq!(X64Reloc::Plt32.elf_type(), 4);
        assert_eq!(X64Reloc::RexGotPcrelX.elf_type(), 42);
    }

    #[test]
    fn other_is_an_escape() {
        assert_eq!(A64Reloc::Other(1024).elf_type(), 1024);
        assert_eq!(A64Reloc::from_elf_type(1024), A64Reloc::Other(1024));
        assert_eq!(A64Reloc::Other(1024).elf_name(), None);
        let kind = RelocKind::from_elf_type(Arch::X86_64, 2);
        assert_eq!(kind, RelocKind::X64(X64Reloc::Pc32));
        assert_eq!(kind.arch(), Arch::X86_64);
        assert_eq!(kind.elf_type(), 2);
        assert_eq!(RelocKind::from(A64Reloc::Call26).elf_name(), Some("R_AARCH64_CALL26"));
    }
}
