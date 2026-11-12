//! The DWARF and LLVM vocabulary debug nodes are written in
//! (`spec/ir.md` §6.9): tag, encoding, language, calling-convention and
//! operation numbers with their LLVM names, the `DIFlag*` and `DISPFlag*`
//! bits, and the small enumerations of a compile unit and a file.
//!
//! A node stores numbers. The names are here for the producers that read
//! LLVM text and for the printer; the tables were read out of LLVM 22's
//! assembler (`tests/debug_survey.rs` checks them against it).

/// The number of `name` in `table`, which is sorted by name.
fn number(table: &[(&str, u16)], name: &[u8]) -> Option<u16> {
    table.binary_search_by(|(known, _)| known.as_bytes().cmp(name)).ok().map(|at| table[at].1)
}

/// The first name of `number` in `table`.
fn name(table: &'static [(&'static str, u16)], number: u16) -> Option<&'static str> {
    table.iter().find(|(_, known)| *known == number).map(|(name, _)| *name)
}

/// The number of a `DW_TAG_*` name.
pub fn dw_tag(name: &[u8]) -> Option<u16> {
    number(DW_TAGS, name)
}

/// The `DW_TAG_*` name of a number.
pub fn dw_tag_name(tag: u16) -> Option<&'static str> {
    name(DW_TAGS, tag)
}

/// The number of a `DW_ATE_*` name.
pub fn dw_ate(name: &[u8]) -> Option<u16> {
    number(DW_ATES, name)
}

/// The `DW_ATE_*` name of a number.
pub fn dw_ate_name(encoding: u16) -> Option<&'static str> {
    name(DW_ATES, encoding)
}

/// The number of a `DW_LANG_*` name.
pub fn dw_lang(name: &[u8]) -> Option<u16> {
    number(DW_LANGS, name)
}

/// The `DW_LANG_*` name of a number.
pub fn dw_lang_name(language: u16) -> Option<&'static str> {
    name(DW_LANGS, language)
}

/// The number of a `DW_CC_*` name.
pub fn dw_cc(name: &[u8]) -> Option<u16> {
    number(DW_CCS, name)
}

/// The `DW_CC_*` name of a number.
pub fn dw_cc_name(cc: u16) -> Option<&'static str> {
    name(DW_CCS, cc)
}

/// `DW_TAG_array_type`.
pub const DW_TAG_ARRAY_TYPE: u16 = 0x01;
/// `DW_TAG_class_type`.
pub const DW_TAG_CLASS_TYPE: u16 = 0x02;
/// `DW_TAG_enumeration_type`.
pub const DW_TAG_ENUMERATION_TYPE: u16 = 0x04;
/// `DW_TAG_imported_declaration`.
pub const DW_TAG_IMPORTED_DECLARATION: u16 = 0x08;
/// `DW_TAG_member`.
pub const DW_TAG_MEMBER: u16 = 0x0d;
/// `DW_TAG_pointer_type`.
pub const DW_TAG_POINTER_TYPE: u16 = 0x0f;
/// `DW_TAG_reference_type`.
pub const DW_TAG_REFERENCE_TYPE: u16 = 0x10;
/// `DW_TAG_structure_type`.
pub const DW_TAG_STRUCTURE_TYPE: u16 = 0x13;
/// `DW_TAG_typedef`.
pub const DW_TAG_TYPEDEF: u16 = 0x16;
/// `DW_TAG_union_type`.
pub const DW_TAG_UNION_TYPE: u16 = 0x17;
/// `DW_TAG_inheritance`.
pub const DW_TAG_INHERITANCE: u16 = 0x1c;
/// `DW_TAG_ptr_to_member_type`.
pub const DW_TAG_PTR_TO_MEMBER_TYPE: u16 = 0x1f;
/// `DW_TAG_base_type`.
pub const DW_TAG_BASE_TYPE: u16 = 0x24;
/// `DW_TAG_const_type`.
pub const DW_TAG_CONST_TYPE: u16 = 0x26;
/// `DW_TAG_template_value_parameter`.
pub const DW_TAG_TEMPLATE_VALUE_PARAMETER: u16 = 0x30;
/// `DW_TAG_variable`.
pub const DW_TAG_VARIABLE: u16 = 0x34;
/// `DW_TAG_volatile_type`.
pub const DW_TAG_VOLATILE_TYPE: u16 = 0x35;
/// `DW_TAG_restrict_type`.
pub const DW_TAG_RESTRICT_TYPE: u16 = 0x37;
/// `DW_TAG_imported_module`.
pub const DW_TAG_IMPORTED_MODULE: u16 = 0x3a;
/// `DW_TAG_unspecified_type`.
pub const DW_TAG_UNSPECIFIED_TYPE: u16 = 0x3b;
/// `DW_TAG_rvalue_reference_type`.
pub const DW_TAG_RVALUE_REFERENCE_TYPE: u16 = 0x42;
/// `DW_TAG_template_alias`.
pub const DW_TAG_TEMPLATE_ALIAS: u16 = 0x43;
/// `DW_TAG_atomic_type`.
pub const DW_TAG_ATOMIC_TYPE: u16 = 0x47;
/// `DW_TAG_GNU_template_template_param`.
pub const DW_TAG_GNU_TEMPLATE_TEMPLATE_PARAM: u16 = 0x4106;
/// `DW_TAG_GNU_template_parameter_pack`.
pub const DW_TAG_GNU_TEMPLATE_PARAMETER_PACK: u16 = 0x4107;

/// `DIFlag*` bits: the `flags:` field of types, variables and subprograms.
///
/// The low two bits are one field, the accessibility: `PRIVATE`,
/// `PROTECTED` and `PUBLIC` are its three values, read with
/// [`DiFlags::accessibility`]. Bits 16 and 17 are likewise the
/// inheritance model of a pointer to member.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct DiFlags(pub u32);

macro_rules! flag_set {
    ($ty:ident, $prefix:literal, $($(#[$meta:meta])* $name:ident = $value:expr, $llvm:literal;)*) => {
        impl $ty {
            $( $(#[$meta])* #[doc = concat!("`", $prefix, $llvm, "`.")]
            pub const $name: $ty = $ty($value); )*

            /// Every name LLVM writes, with its bits.
            pub const NAMES: &'static [(&'static str, u32)] =
                &[$((concat!($prefix, $llvm), $value),)*];

            /// The flag LLVM writes as `name`.
            pub fn named(name: &[u8]) -> Option<$ty> {
                Self::NAMES
                    .iter()
                    .find(|(known, _)| known.as_bytes() == name)
                    .map(|(_, bits)| $ty(*bits))
            }

            /// Whether every bit of `other` is set.
            #[inline]
            pub const fn contains(self, other: $ty) -> bool {
                self.0 & other.0 == other.0
            }

            /// Whether no bit is set.
            #[inline]
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }
        }

        impl std::ops::BitOr for $ty {
            type Output = $ty;
            #[inline]
            fn bitor(self, other: $ty) -> $ty {
                $ty(self.0 | other.0)
            }
        }
    };
}

flag_set! { DiFlags, "DIFlag",
    PRIVATE = 1, "Private";
    PROTECTED = 2, "Protected";
    PUBLIC = 3, "Public";
    FWD_DECL = 1 << 2, "FwdDecl";
    APPLE_BLOCK = 1 << 3, "AppleBlock";
    RESERVED_BIT4 = 1 << 4, "ReservedBit4";
    VIRTUAL = 1 << 5, "Virtual";
    ARTIFICIAL = 1 << 6, "Artificial";
    EXPLICIT = 1 << 7, "Explicit";
    PROTOTYPED = 1 << 8, "Prototyped";
    OBJC_CLASS_COMPLETE = 1 << 9, "ObjcClassComplete";
    OBJECT_POINTER = 1 << 10, "ObjectPointer";
    VECTOR = 1 << 11, "Vector";
    STATIC_MEMBER = 1 << 12, "StaticMember";
    LVALUE_REFERENCE = 1 << 13, "LValueReference";
    RVALUE_REFERENCE = 1 << 14, "RValueReference";
    EXPORT_SYMBOLS = 1 << 15, "ExportSymbols";
    SINGLE_INHERITANCE = 1 << 16, "SingleInheritance";
    MULTIPLE_INHERITANCE = 2 << 16, "MultipleInheritance";
    VIRTUAL_INHERITANCE = 3 << 16, "VirtualInheritance";
    INTRODUCED_VIRTUAL = 1 << 18, "IntroducedVirtual";
    BIT_FIELD = 1 << 19, "BitField";
    NO_RETURN = 1 << 20, "NoReturn";
    TYPE_PASS_BY_VALUE = 1 << 22, "TypePassByValue";
    TYPE_PASS_BY_REFERENCE = 1 << 23, "TypePassByReference";
    ENUM_CLASS = 1 << 24, "EnumClass";
    THUNK = 1 << 25, "Thunk";
    NON_TRIVIAL = 1 << 26, "NonTrivial";
    BIG_ENDIAN = 1 << 27, "BigEndian";
    LITTLE_ENDIAN = 1 << 28, "LittleEndian";
    ALL_CALLS_DESCRIBED = 1 << 29, "AllCallsDescribed";
}

impl DiFlags {
    /// The accessibility field: 0 (none), [`DiFlags::PRIVATE`],
    /// [`DiFlags::PROTECTED`] or [`DiFlags::PUBLIC`].
    #[inline]
    pub const fn accessibility(self) -> DiFlags {
        DiFlags(self.0 & 3)
    }
}

/// `DISPFlag*` bits: the `spFlags:` field of a subprogram.
///
/// The low two bits are the virtuality: 0, `VIRTUAL` or `PURE_VIRTUAL`.
/// LLVM's older spelling of the same facts as separate fields
/// (`isLocal:`, `isDefinition:`, `isOptimized:`, `virtuality:`) is folded
/// into these bits by [`crate::DiNode::set`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct SpFlags(pub u32);

flag_set! { SpFlags, "DISPFlag",
    VIRTUAL = 1, "Virtual";
    PURE_VIRTUAL = 2, "PureVirtual";
    LOCAL_TO_UNIT = 1 << 2, "LocalToUnit";
    DEFINITION = 1 << 3, "Definition";
    OPTIMIZED = 1 << 4, "Optimized";
    PURE = 1 << 5, "Pure";
    ELEMENTAL = 1 << 6, "Elemental";
    RECURSIVE = 1 << 7, "Recursive";
    MAIN_SUBPROGRAM = 1 << 8, "MainSubprogram";
    DELETED = 1 << 9, "Deleted";
    OBJC_DIRECT = 1 << 11, "ObjCDirect";
}

macro_rules! small_enum {
    ($(#[$meta:meta])* $ty:ident { $($(#[$vmeta:meta])* $variant:ident = $value:literal, $llvm:literal;)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
        pub enum $ty {
            $( $(#[$vmeta])* #[doc = concat!("`", $llvm, "`.")] $variant = $value, )*
        }

        impl $ty {
            /// Every member, with the name LLVM writes.
            pub const NAMES: &'static [(&'static str, $ty)] = &[$(($llvm, $ty::$variant),)*];

            /// The member LLVM writes as `name`.
            pub fn named(name: &[u8]) -> Option<$ty> {
                Self::NAMES.iter().find(|(known, _)| known.as_bytes() == name).map(|(_, m)| *m)
            }

            /// The member with LLVM's number `code`.
            pub fn from_code(code: u64) -> Option<$ty> {
                match code {
                    $( $value => Some($ty::$variant), )*
                    _ => None,
                }
            }

            /// LLVM's number for the member.
            #[inline]
            pub const fn code(self) -> u32 {
                self as u32
            }

            /// The name LLVM writes.
            pub fn name(self) -> &'static str {
                match self {
                    $( $ty::$variant => $llvm, )*
                }
            }
        }
    };
}

small_enum! {
    /// How much debug information a compile unit asks for
    /// (`emissionKind:`).
    EmissionKind {
        /// The unit and everything reached only from it is ignored.
        #[default]
        NoDebug = 0, "NoDebug";
        /// Types, variables, scopes and lines.
        FullDebug = 1, "FullDebug";
        /// Subprograms and lines; no types and no variables.
        LineTablesOnly = 2, "LineTablesOnly";
        /// Line directives only: lines without subprogram entries.
        DebugDirectivesOnly = 3, "DebugDirectivesOnly";
    }
}

small_enum! {
    /// Which name index a compile unit asks for (`nameTableKind:`).
    NameTableKind {
        /// The target's default: none for DWARF 4 on Linux,
        /// `.debug_names` for DWARF 5.
        #[default]
        Default = 0, "Default";
        /// `.debug_gnu_pubnames` and `.debug_gnu_pubtypes`.
        Gnu = 1, "GNU";
        /// No index.
        None = 2, "None";
        /// The Apple accelerator tables.
        Apple = 3, "Apple";
    }
}

small_enum! {
    /// The kind of a file's checksum (`checksumkind:`).
    ChecksumKind {
        /// No checksum.
        #[default]
        None = 0, "CSK_None";
        /// MD5: 32 hexadecimal digits. What DWARF 5 line tables carry.
        Md5 = 1, "CSK_MD5";
        /// SHA-1.
        Sha1 = 2, "CSK_SHA1";
        /// SHA-256.
        Sha256 = 3, "CSK_SHA256";
    }
}

/// The `DW_TAG_*` names LLVM 22 knows, with their numbers.
/// Sorted by name.
pub const DW_TAGS: &[(&str, u16)] = &[
    ("DW_TAG_APPLE_property", 0x4200),
    ("DW_TAG_BORLAND_property", 0xb000),
    ("DW_TAG_GNU_call_site", 0x4109),
    ("DW_TAG_GNU_call_site_parameter", 0x410a),
    ("DW_TAG_GNU_formal_parameter_pack", 0x4108),
    ("DW_TAG_GNU_template_parameter_pack", 0x4107),
    ("DW_TAG_GNU_template_template_param", 0x4106),
    ("DW_TAG_LLVM_annotation", 0x6000),
    ("DW_TAG_LLVM_ptrauth_type", 0x4300),
    ("DW_TAG_MIPS_loop", 0x4081),
    ("DW_TAG_access_declaration", 0x23),
    ("DW_TAG_array_type", 1),
    ("DW_TAG_atomic_type", 0x47),
    ("DW_TAG_base_type", 0x24),
    ("DW_TAG_call_site", 0x48),
    ("DW_TAG_call_site_parameter", 0x49),
    ("DW_TAG_catch_block", 0x25),
    ("DW_TAG_class_template", 0x4103),
    ("DW_TAG_class_type", 2),
    ("DW_TAG_coarray_type", 0x44),
    ("DW_TAG_common_block", 0x1a),
    ("DW_TAG_common_inclusion", 0x1b),
    ("DW_TAG_compile_unit", 0x11),
    ("DW_TAG_condition", 0x3f),
    ("DW_TAG_const_type", 0x26),
    ("DW_TAG_constant", 0x27),
    ("DW_TAG_dwarf_procedure", 0x36),
    ("DW_TAG_dynamic_type", 0x46),
    ("DW_TAG_entry_point", 3),
    ("DW_TAG_enumeration_type", 4),
    ("DW_TAG_enumerator", 0x28),
    ("DW_TAG_file_type", 0x29),
    ("DW_TAG_formal_parameter", 5),
    ("DW_TAG_format_label", 0x4101),
    ("DW_TAG_friend", 0x2a),
    ("DW_TAG_function_template", 0x4102),
    ("DW_TAG_generic_subrange", 0x45),
    ("DW_TAG_immutable_type", 0x4b),
    ("DW_TAG_imported_declaration", 8),
    ("DW_TAG_imported_module", 0x3a),
    ("DW_TAG_imported_unit", 0x3d),
    ("DW_TAG_inheritance", 0x1c),
    ("DW_TAG_inlined_subroutine", 0x1d),
    ("DW_TAG_interface_type", 0x38),
    ("DW_TAG_label", 0xa),
    ("DW_TAG_lexical_block", 0xb),
    ("DW_TAG_member", 0xd),
    ("DW_TAG_module", 0x1e),
    ("DW_TAG_namelist", 0x2b),
    ("DW_TAG_namelist_item", 0x2c),
    ("DW_TAG_namespace", 0x39),
    ("DW_TAG_null", 0),
    ("DW_TAG_packed_type", 0x2d),
    ("DW_TAG_partial_unit", 0x3c),
    ("DW_TAG_pointer_type", 0xf),
    ("DW_TAG_ptr_to_member_type", 0x1f),
    ("DW_TAG_reference_type", 0x10),
    ("DW_TAG_restrict_type", 0x37),
    ("DW_TAG_rvalue_reference_type", 0x42),
    ("DW_TAG_set_type", 0x20),
    ("DW_TAG_shared_type", 0x40),
    ("DW_TAG_skeleton_unit", 0x4a),
    ("DW_TAG_string_type", 0x12),
    ("DW_TAG_structure_type", 0x13),
    ("DW_TAG_subprogram", 0x2e),
    ("DW_TAG_subrange_type", 0x21),
    ("DW_TAG_subroutine_type", 0x15),
    ("DW_TAG_template_alias", 0x43),
    ("DW_TAG_template_type_parameter", 0x2f),
    ("DW_TAG_template_value_parameter", 0x30),
    ("DW_TAG_thrown_type", 0x31),
    ("DW_TAG_try_block", 0x32),
    ("DW_TAG_type_unit", 0x41),
    ("DW_TAG_typedef", 0x16),
    ("DW_TAG_union_type", 0x17),
    ("DW_TAG_unspecified_parameters", 0x18),
    ("DW_TAG_unspecified_type", 0x3b),
    ("DW_TAG_variable", 0x34),
    ("DW_TAG_variant", 0x19),
    ("DW_TAG_variant_part", 0x33),
    ("DW_TAG_volatile_type", 0x35),
    ("DW_TAG_with_stmt", 0x22),
];

/// The `DW_ATE_*` base-type encodings LLVM 22 knows.
/// Sorted by name.
pub const DW_ATES: &[(&str, u16)] = &[
    ("DW_ATE_ASCII", 0x12),
    ("DW_ATE_UCS", 0x11),
    ("DW_ATE_UTF", 0x10),
    ("DW_ATE_address", 1),
    ("DW_ATE_boolean", 2),
    ("DW_ATE_complex_float", 3),
    ("DW_ATE_decimal_float", 0xf),
    ("DW_ATE_edited", 0xc),
    ("DW_ATE_float", 4),
    ("DW_ATE_imaginary_float", 9),
    ("DW_ATE_numeric_string", 0xb),
    ("DW_ATE_packed_decimal", 0xa),
    ("DW_ATE_signed", 5),
    ("DW_ATE_signed_char", 6),
    ("DW_ATE_signed_fixed", 0xd),
    ("DW_ATE_unsigned", 7),
    ("DW_ATE_unsigned_char", 8),
    ("DW_ATE_unsigned_fixed", 0xe),
];

/// The `DW_LANG_*` source languages LLVM 22 knows.
/// Sorted by name.
pub const DW_LANGS: &[(&str, u16)] = &[
    ("DW_LANG_Ada2005", 0x2e),
    ("DW_LANG_Ada2012", 0x2f),
    ("DW_LANG_Ada83", 3),
    ("DW_LANG_Ada95", 0xd),
    ("DW_LANG_Assembly", 0x31),
    ("DW_LANG_BLISS", 0x25),
    ("DW_LANG_BORLAND_Delphi", 0xb000),
    ("DW_LANG_C", 2),
    ("DW_LANG_C11", 0x1d),
    ("DW_LANG_C17", 0x2c),
    ("DW_LANG_C89", 1),
    ("DW_LANG_C99", 0xc),
    ("DW_LANG_CPP_for_OpenCL", 0x38),
    ("DW_LANG_C_plus_plus", 4),
    ("DW_LANG_C_plus_plus_03", 0x19),
    ("DW_LANG_C_plus_plus_11", 0x1a),
    ("DW_LANG_C_plus_plus_14", 0x21),
    ("DW_LANG_C_plus_plus_17", 0x2a),
    ("DW_LANG_C_plus_plus_20", 0x2b),
    ("DW_LANG_C_sharp", 0x32),
    ("DW_LANG_Cobol74", 5),
    ("DW_LANG_Cobol85", 6),
    ("DW_LANG_Crystal", 0x28),
    ("DW_LANG_D", 0x13),
    ("DW_LANG_Dylan", 0x20),
    ("DW_LANG_Fortran03", 0x22),
    ("DW_LANG_Fortran08", 0x23),
    ("DW_LANG_Fortran18", 0x2d),
    ("DW_LANG_Fortran77", 7),
    ("DW_LANG_Fortran90", 8),
    ("DW_LANG_Fortran95", 0xe),
    ("DW_LANG_GLSL", 0x34),
    ("DW_LANG_GLSL_ES", 0x35),
    ("DW_LANG_GOOGLE_RenderScript", 0x8e57),
    ("DW_LANG_Go", 0x16),
    ("DW_LANG_HIP", 0x30),
    ("DW_LANG_HLSL", 0x36),
    ("DW_LANG_Haskell", 0x18),
    ("DW_LANG_Hylo", 0x42),
    ("DW_LANG_Java", 0xb),
    ("DW_LANG_Julia", 0x1f),
    ("DW_LANG_Kotlin", 0x26),
    ("DW_LANG_Metal", 0x3d),
    ("DW_LANG_Mips_Assembler", 0x8001),
    ("DW_LANG_Modula2", 0xa),
    ("DW_LANG_Modula3", 0x17),
    ("DW_LANG_Mojo", 0x33),
    ("DW_LANG_Move", 0x41),
    ("DW_LANG_OCaml", 0x1b),
    ("DW_LANG_ObjC", 0x10),
    ("DW_LANG_ObjC_plus_plus", 0x11),
    ("DW_LANG_OpenCL", 0x15),
    ("DW_LANG_OpenCL_CPP", 0x37),
    ("DW_LANG_PLI", 0xf),
    ("DW_LANG_Pascal83", 9),
    ("DW_LANG_Python", 0x14),
    ("DW_LANG_RenderScript", 0x24),
    ("DW_LANG_Ruby", 0x40),
    ("DW_LANG_Rust", 0x1c),
    ("DW_LANG_SYCL", 0x39),
    ("DW_LANG_Swift", 0x1e),
    ("DW_LANG_UPC", 0x12),
    ("DW_LANG_Zig", 0x27),
];

/// The `DW_CC_*` calling conventions LLVM 22 knows.
/// Sorted by name.
pub const DW_CCS: &[(&str, u16)] = &[
    ("DW_CC_BORLAND_fastcall", 0xb6),
    ("DW_CC_BORLAND_msfastcall", 0xb3),
    ("DW_CC_BORLAND_msreturn", 0xb4),
    ("DW_CC_BORLAND_pascal", 0xb2),
    ("DW_CC_BORLAND_safecall", 0xb0),
    ("DW_CC_BORLAND_stdcall", 0xb1),
    ("DW_CC_BORLAND_thiscall", 0xb5),
    ("DW_CC_GDB_IBM_OpenCL", 0xff),
    ("DW_CC_GNU_borland_fastcall_i386", 0x41),
    ("DW_CC_GNU_renesas_sh", 0x40),
    ("DW_CC_LLVM_AAPCS", 0xc3),
    ("DW_CC_LLVM_AAPCS_VFP", 0xc4),
    ("DW_CC_LLVM_IntelOclBicc", 0xc5),
    ("DW_CC_LLVM_M68kRTD", 0xcc),
    ("DW_CC_LLVM_OpenCLKernel", 0xc7),
    ("DW_CC_LLVM_PreserveAll", 0xca),
    ("DW_CC_LLVM_PreserveMost", 0xc9),
    ("DW_CC_LLVM_PreserveNone", 0xcd),
    ("DW_CC_LLVM_RISCVVLSCall", 0xd0),
    ("DW_CC_LLVM_RISCVVectorCall", 0xce),
    ("DW_CC_LLVM_SpirFunction", 0xc6),
    ("DW_CC_LLVM_Swift", 0xc8),
    ("DW_CC_LLVM_SwiftTail", 0xcf),
    ("DW_CC_LLVM_Win64", 0xc1),
    ("DW_CC_LLVM_X86RegCall", 0xcb),
    ("DW_CC_LLVM_X86_64SysV", 0xc2),
    ("DW_CC_LLVM_vectorcall", 0xc0),
    ("DW_CC_nocall", 3),
    ("DW_CC_normal", 1),
    ("DW_CC_pass_by_reference", 4),
    ("DW_CC_pass_by_value", 5),
    ("DW_CC_program", 2),
];

/// The `DW_OP_*` names LLVM 22 knows, with their codes, except the
/// numbered families `DW_OP_lit0..31` (0x30), `DW_OP_reg0..31` (0x50) and
/// `DW_OP_breg0..31` (0x70), which [`dw_op`] computes. Sorted by name.
pub const DW_OPS: &[(&str, u16)] = &[
    ("DW_OP_GNU_addr_index", 0xfb),
    ("DW_OP_GNU_const_index", 0xfc),
    ("DW_OP_GNU_entry_value", 0xf3),
    ("DW_OP_GNU_implicit_pointer", 0xf2),
    ("DW_OP_GNU_push_tls_address", 0xe0),
    ("DW_OP_LLVM_arg", 0x1005),
    ("DW_OP_LLVM_convert", 0x1001),
    ("DW_OP_LLVM_entry_value", 0x1003),
    ("DW_OP_LLVM_extract_bits_sext", 0x1006),
    ("DW_OP_LLVM_extract_bits_zext", 0x1007),
    ("DW_OP_LLVM_fragment", 0x1000),
    ("DW_OP_LLVM_implicit_pointer", 0x1004),
    ("DW_OP_LLVM_tag_offset", 0x1002),
    ("DW_OP_LLVM_user", 0xe9),
    ("DW_OP_WASM_location", 0xed),
    ("DW_OP_WASM_location_int", 0xee),
    ("DW_OP_abs", 0x19),
    ("DW_OP_addr", 0x3),
    ("DW_OP_addrx", 0xa1),
    ("DW_OP_and", 0x1a),
    ("DW_OP_bit_piece", 0x9d),
    ("DW_OP_bra", 0x28),
    ("DW_OP_bregx", 0x92),
    ("DW_OP_call2", 0x98),
    ("DW_OP_call4", 0x99),
    ("DW_OP_call_frame_cfa", 0x9c),
    ("DW_OP_call_ref", 0x9a),
    ("DW_OP_const1s", 0x9),
    ("DW_OP_const1u", 0x8),
    ("DW_OP_const2s", 0xb),
    ("DW_OP_const2u", 0xa),
    ("DW_OP_const4s", 0xd),
    ("DW_OP_const4u", 0xc),
    ("DW_OP_const8s", 0xf),
    ("DW_OP_const8u", 0xe),
    ("DW_OP_const_type", 0xa4),
    ("DW_OP_consts", 0x11),
    ("DW_OP_constu", 0x10),
    ("DW_OP_constx", 0xa2),
    ("DW_OP_convert", 0xa8),
    ("DW_OP_deref", 0x6),
    ("DW_OP_deref_size", 0x94),
    ("DW_OP_deref_type", 0xa6),
    ("DW_OP_div", 0x1b),
    ("DW_OP_drop", 0x13),
    ("DW_OP_dup", 0x12),
    ("DW_OP_entry_value", 0xa3),
    ("DW_OP_eq", 0x29),
    ("DW_OP_fbreg", 0x91),
    ("DW_OP_form_tls_address", 0x9b),
    ("DW_OP_ge", 0x2a),
    ("DW_OP_gt", 0x2b),
    ("DW_OP_implicit_pointer", 0xa0),
    ("DW_OP_implicit_value", 0x9e),
    ("DW_OP_le", 0x2c),
    ("DW_OP_lt", 0x2d),
    ("DW_OP_minus", 0x1c),
    ("DW_OP_mod", 0x1d),
    ("DW_OP_mul", 0x1e),
    ("DW_OP_ne", 0x2e),
    ("DW_OP_neg", 0x1f),
    ("DW_OP_nop", 0x96),
    ("DW_OP_not", 0x20),
    ("DW_OP_or", 0x21),
    ("DW_OP_over", 0x14),
    ("DW_OP_pick", 0x15),
    ("DW_OP_piece", 0x93),
    ("DW_OP_plus", 0x22),
    ("DW_OP_plus_uconst", 0x23),
    ("DW_OP_push_object_address", 0x97),
    ("DW_OP_regval_type", 0xa5),
    ("DW_OP_regx", 0x90),
    ("DW_OP_reinterpret", 0xa9),
    ("DW_OP_rot", 0x17),
    ("DW_OP_shl", 0x24),
    ("DW_OP_shr", 0x25),
    ("DW_OP_shra", 0x26),
    ("DW_OP_skip", 0x2f),
    ("DW_OP_stack_value", 0x9f),
    ("DW_OP_swap", 0x16),
    ("DW_OP_xderef", 0x18),
    ("DW_OP_xderef_size", 0x95),
    ("DW_OP_xderef_type", 0xa7),
    ("DW_OP_xor", 0x27),
];

/// The numbered operation families: name prefix and first code.
const DW_OP_FAMILIES: [(&str, u16); 3] =
    [("DW_OP_lit", 0x30), ("DW_OP_reg", 0x50), ("DW_OP_breg", 0x70)];

/// The code of a `DW_OP_*` name.
pub fn dw_op(name: &[u8]) -> Option<u64> {
    if let Some(code) = number(DW_OPS, name) {
        return Some(u64::from(code));
    }
    DW_OP_FAMILIES.iter().find_map(|&(prefix, first)| {
        let digits = name.strip_prefix(prefix.as_bytes())?;
        let index: u16 = std::str::from_utf8(digits).ok()?.parse().ok()?;
        let canonical = digits.len() == 1 || digits[0] != b'0';
        (index < 32 && canonical).then(|| u64::from(first + index))
    })
}

/// The `DW_OP_*` name of a code.
pub fn dw_op_name(code: u64) -> Option<String> {
    let code = u16::try_from(code).ok()?;
    if let Some(name) = name(DW_OPS, code) {
        return Some(name.to_string());
    }
    DW_OP_FAMILIES
        .iter()
        .find(|&&(_, first)| (first..first + 32).contains(&code))
        .map(|&(prefix, first)| format!("{prefix}{}", code - first))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.9: the tables are sorted (the lookups search them) and the
    /// names and numbers convert both ways.
    #[test]
    fn name_tables_are_sorted_and_convert() {
        for table in [DW_TAGS, DW_ATES, DW_LANGS, DW_CCS, DW_OPS] {
            assert!(table.windows(2).all(|pair| pair[0].0.as_bytes() < pair[1].0.as_bytes()));
        }
        assert_eq!(dw_tag(b"DW_TAG_member"), Some(DW_TAG_MEMBER));
        assert_eq!(dw_tag(b"DW_TAG_GNU_template_parameter_pack"), Some(0x4107));
        assert_eq!(dw_tag(b"DW_TAG_nonsense"), None);
        assert_eq!(dw_tag_name(DW_TAG_BASE_TYPE), Some("DW_TAG_base_type"));
        assert_eq!(dw_ate(b"DW_ATE_UTF"), Some(0x10));
        assert_eq!(dw_ate_name(5), Some("DW_ATE_signed"));
        assert_eq!(dw_lang(b"DW_LANG_C_plus_plus_14"), Some(0x21));
        assert_eq!(dw_lang_name(0x1d), Some("DW_LANG_C11"));
        assert_eq!(dw_cc(b"DW_CC_nocall"), Some(3));
        assert_eq!(dw_cc_name(1), Some("DW_CC_normal"));
        assert_eq!(dw_cc_name(7), None);
        assert_eq!(dw_op(b"DW_OP_LLVM_fragment"), Some(0x1000));
        assert_eq!(dw_op(b"DW_OP_breg31"), Some(0x8f));
        assert_eq!(dw_op(b"DW_OP_lit0"), Some(0x30));
        assert_eq!(dw_op(b"DW_OP_reg32"), None);
        assert_eq!(dw_op(b"DW_OP_reg07"), None);
        assert_eq!(dw_op_name(0x51).as_deref(), Some("DW_OP_reg1"));
        assert_eq!(dw_op_name(0x9f).as_deref(), Some("DW_OP_stack_value"));
        assert_eq!(dw_op_name(0x1fff), None);
        assert_eq!(dw_op_name(1 << 40), None);
    }

    /// §6.9: flags are bit sets with two multi-bit fields.
    #[test]
    fn flags_combine_and_name() {
        let flags = DiFlags::PUBLIC | DiFlags::ARTIFICIAL;
        assert!(flags.contains(DiFlags::ARTIFICIAL) && !flags.contains(DiFlags::VIRTUAL));
        assert_eq!(flags.accessibility(), DiFlags::PUBLIC);
        assert_eq!(DiFlags::named(b"DIFlagTypePassByReference"), Some(DiFlags(1 << 23)));
        assert_eq!(DiFlags::named(b"DIFlagZero"), None);
        assert!(DiFlags::default().is_empty());
        assert_eq!(SpFlags::named(b"DISPFlagDefinition"), Some(SpFlags::DEFINITION));
        assert_eq!((SpFlags::DEFINITION | SpFlags::LOCAL_TO_UNIT).0, 12);
        assert_eq!(EmissionKind::named(b"LineTablesOnly"), Some(EmissionKind::LineTablesOnly));
        assert_eq!(EmissionKind::from_code(3), Some(EmissionKind::DebugDirectivesOnly));
        assert_eq!(EmissionKind::from_code(4), None);
        assert_eq!(NameTableKind::Gnu.name(), "GNU");
        assert_eq!(ChecksumKind::named(b"CSK_MD5").map(ChecksumKind::code), Some(1));
    }
}
