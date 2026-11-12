//! The fixed abbreviation table (`spec/dwarf.md` §6.1).
//!
//! Every DIE shape hod writes is a *family*: a tag, whether it has
//! children, and an ordered list of attribute slots, each either always
//! present or controlled by one option bit. A family owns a fixed block of
//! abbreviation codes, `base .. base + block`; the code of a DIE is its
//! family's base plus the mask of its option bits (and, for a family whose
//! children are optional, the children bit above them). Codes are
//! therefore known when the program is compiled: no DIE is hashed or
//! interned, and runs of DIEs written in parallel agree on them.
//!
//! The table is append-only. A family's position and block never change;
//! a package that needs a shape no family has appends a family at the
//! end. A *reserved* family (owned by a later package) may have its slots
//! refined by its owner within its block. `.debug_abbrev` holds the
//! entries of the codes an object used, in code order.

use crate::leb::write_uleb;

/// `DW_TAG_*` (DWARF 5 §7.5.3, table 7.3).
#[allow(missing_docs)]
pub mod tag {
    pub const ARRAY_TYPE: u16 = 0x01;
    pub const CLASS_TYPE: u16 = 0x02;
    pub const ENUMERATION_TYPE: u16 = 0x04;
    pub const FORMAL_PARAMETER: u16 = 0x05;
    pub const IMPORTED_DECLARATION: u16 = 0x08;
    pub const LABEL: u16 = 0x0a;
    pub const LEXICAL_BLOCK: u16 = 0x0b;
    pub const MEMBER: u16 = 0x0d;
    pub const POINTER_TYPE: u16 = 0x0f;
    pub const REFERENCE_TYPE: u16 = 0x10;
    pub const COMPILE_UNIT: u16 = 0x11;
    pub const STRUCTURE_TYPE: u16 = 0x13;
    pub const SUBROUTINE_TYPE: u16 = 0x15;
    pub const TYPEDEF: u16 = 0x16;
    pub const UNION_TYPE: u16 = 0x17;
    pub const UNSPECIFIED_PARAMETERS: u16 = 0x18;
    pub const INHERITANCE: u16 = 0x1c;
    pub const PTR_TO_MEMBER_TYPE: u16 = 0x1f;
    pub const SUBRANGE_TYPE: u16 = 0x21;
    pub const BASE_TYPE: u16 = 0x24;
    pub const CONST_TYPE: u16 = 0x26;
    pub const ENUMERATOR: u16 = 0x28;
    pub const FRIEND: u16 = 0x2a;
    pub const SUBPROGRAM: u16 = 0x2e;
    pub const TEMPLATE_TYPE_PARAMETER: u16 = 0x2f;
    pub const TEMPLATE_VALUE_PARAMETER: u16 = 0x30;
    pub const VARIABLE: u16 = 0x34;
    pub const VOLATILE_TYPE: u16 = 0x35;
    pub const RESTRICT_TYPE: u16 = 0x37;
    pub const NAMESPACE: u16 = 0x39;
    pub const IMPORTED_MODULE: u16 = 0x3a;
    pub const UNSPECIFIED_TYPE: u16 = 0x3b;
    pub const RVALUE_REFERENCE_TYPE: u16 = 0x42;
    pub const TEMPLATE_ALIAS: u16 = 0x43;
    pub const ATOMIC_TYPE: u16 = 0x47;
    // WP-085.
    pub const INLINED_SUBROUTINE: u16 = 0x1d;
    pub const GNU_TEMPLATE_TEMPLATE_PARAM: u16 = 0x4106;
    pub const GNU_TEMPLATE_PARAMETER_PACK: u16 = 0x4107;
}

/// `DW_AT_*` (DWARF 5 §7.5.4, table 7.5).
#[allow(missing_docs)]
pub mod at {
    pub const LOCATION: u16 = 0x02;
    pub const NAME: u16 = 0x03;
    pub const BYTE_SIZE: u16 = 0x0b;
    pub const BIT_SIZE: u16 = 0x0d;
    pub const STMT_LIST: u16 = 0x10;
    pub const LOW_PC: u16 = 0x11;
    pub const HIGH_PC: u16 = 0x12;
    pub const LANGUAGE: u16 = 0x13;
    pub const IMPORT: u16 = 0x18;
    pub const COMP_DIR: u16 = 0x1b;
    pub const CONST_VALUE: u16 = 0x1c;
    pub const CONTAINING_TYPE: u16 = 0x1d;
    pub const DEFAULT_VALUE: u16 = 0x1e;
    pub const LOWER_BOUND: u16 = 0x22;
    pub const PRODUCER: u16 = 0x25;
    pub const UPPER_BOUND: u16 = 0x2f;
    pub const ACCESSIBILITY: u16 = 0x32;
    pub const ADDRESS_CLASS: u16 = 0x33;
    pub const ARTIFICIAL: u16 = 0x34;
    pub const CALLING_CONVENTION: u16 = 0x36;
    pub const COUNT: u16 = 0x37;
    pub const DATA_MEMBER_LOCATION: u16 = 0x38;
    pub const DECL_FILE: u16 = 0x3a;
    pub const DECL_LINE: u16 = 0x3b;
    pub const DECL_COLUMN: u16 = 0x39;
    pub const PROTOTYPED: u16 = 0x27;
    pub const DECLARATION: u16 = 0x3c;
    pub const ENCODING: u16 = 0x3e;
    pub const EXTERNAL: u16 = 0x3f;
    pub const FRAME_BASE: u16 = 0x40;
    pub const FRIEND: u16 = 0x41;
    pub const SPECIFICATION: u16 = 0x47;
    pub const TYPE: u16 = 0x49;
    pub const VIRTUALITY: u16 = 0x4c;
    pub const VTABLE_ELEM_LOCATION: u16 = 0x4d;
    pub const RANGES: u16 = 0x55;
    pub const EXPLICIT: u16 = 0x63;
    pub const OBJECT_POINTER: u16 = 0x64;
    pub const ENDIANITY: u16 = 0x65;
    pub const REFERENCE: u16 = 0x77;
    pub const RVALUE_REFERENCE: u16 = 0x78;
    pub const NORETURN: u16 = 0x87;
    pub const DELETED: u16 = 0x8a;
    pub const DATA_BIT_OFFSET: u16 = 0x6b;
    pub const ENUM_CLASS: u16 = 0x6d;
    pub const LINKAGE_NAME: u16 = 0x6e;
    pub const STR_OFFSETS_BASE: u16 = 0x72;
    pub const ADDR_BASE: u16 = 0x73;
    pub const RNGLISTS_BASE: u16 = 0x74;
    pub const ALIGNMENT: u16 = 0x88;
    pub const EXPORT_SYMBOLS: u16 = 0x89;
    // WP-085.
    pub const INLINE: u16 = 0x20;
    pub const ABSTRACT_ORIGIN: u16 = 0x31;
    pub const CALL_COLUMN: u16 = 0x57;
    pub const CALL_FILE: u16 = 0x58;
    pub const CALL_LINE: u16 = 0x59;
    pub const GNU_VECTOR: u16 = 0x2107;
    pub const GNU_TEMPLATE_NAME: u16 = 0x2110;
}

/// `DW_FORM_*` (DWARF 5 §7.5.6, table 7.6).
#[allow(missing_docs)]
pub mod form {
    pub const ADDR: u8 = 0x01;
    pub const DATA2: u8 = 0x05;
    pub const DATA4: u8 = 0x06;
    pub const DATA1: u8 = 0x0b;
    pub const SDATA: u8 = 0x0d;
    pub const UDATA: u8 = 0x0f;
    pub const REF4: u8 = 0x13;
    pub const SEC_OFFSET: u8 = 0x17;
    pub const EXPRLOC: u8 = 0x18;
    pub const FLAG_PRESENT: u8 = 0x19;
    pub const STRX: u8 = 0x1a;
    pub const ADDRX: u8 = 0x1b;
    pub const RNGLISTX: u8 = 0x23;
}

/// Whether a family's DIEs have children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Children {
    /// Never.
    No,
    /// Always.
    Yes,
    /// Chosen per DIE by the bit above the option bits.
    Optional,
}

/// One attribute of a family: always present (`bit` 0) or present when
/// option bit `bit - 1` of the mask is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// `DW_AT_*`.
    pub at: u16,
    /// `DW_FORM_*`.
    pub form: u8,
    /// 0 for always; `k` for option bit `k - 1`.
    pub bit: u8,
}

/// A DIE shape and its block of abbreviation codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Family {
    /// A name for messages and the spec's table.
    pub name: &'static str,
    /// `DW_TAG_*`.
    pub tag: u16,
    /// Whether the DIEs have children.
    pub children: Children,
    /// The number of codes the family owns: a power of two, at least
    /// `2^(option bits + children bit)`.
    pub block: u16,
    /// The attributes, in the order they are written.
    pub slots: &'static [Slot],
    /// The work package that writes it; a later package owns a reserved
    /// family.
    pub owner: &'static str,
}

const fn s(at: u16, form: u8) -> Slot {
    Slot { at, form, bit: 0 }
}

const fn o(bit: u8, at: u16, form: u8) -> Slot {
    Slot { at, form, bit: bit + 1 }
}

const fn fam(
    name: &'static str,
    tag: u16,
    children: Children,
    block: u16,
    owner: &'static str,
    slots: &'static [Slot],
) -> Family {
    Family { name, tag, children, block, slots, owner }
}

use Children::{No, Optional, Yes};
use form::{
    ADDR, ADDRX, DATA1, DATA2, DATA4, EXPRLOC, FLAG_PRESENT, REF4, RNGLISTX, SDATA, SEC_OFFSET,
    STRX, UDATA,
};

/// The index of a family in [`FAMILIES`].
#[allow(missing_docs)]
pub mod fam {
    pub const FORMAL_PARAMETER: usize = 0;
    pub const VARIABLE: usize = 1;
    pub const MEMBER: usize = 2;
    pub const SUBPROGRAM_DECL: usize = 3;
    pub const SUBPROGRAM: usize = 4;
    pub const POINTER_TYPE: usize = 5;
    pub const BASE_TYPE: usize = 6;
    pub const NAMESPACE: usize = 46;
    pub const GLOBAL_VARIABLE: usize = 47;
    pub const COMPILE_UNIT: usize = 48;
    // Families 0 to 48 above are WP-053's table; WP-058 appends the
    // general shapes of the type graph (`spec/dwarf.md` §6.1, §6.8).
    pub const CONST_TYPE: usize = 7;
    pub const REFERENCE_TYPE: usize = 8;
    pub const RVALUE_REFERENCE_TYPE: usize = 9;
    pub const VOLATILE_TYPE: usize = 10;
    pub const RESTRICT_TYPE: usize = 11;
    pub const ATOMIC_TYPE: usize = 12;
    pub const TYPEDEF: usize = 13;
    pub const INHERITANCE: usize = 19;
    pub const STRUCTURE_TYPE: usize = 20;
    pub const STRUCTURE_DECL: usize = 23;
    pub const ENUMERATION_TYPE: usize = 26;
    pub const ENUMERATOR: usize = 27;
    pub const ARRAY_TYPE: usize = 28;
    pub const SUBRANGE: usize = 29;
    pub const SUBRANGE_VAR_COUNT: usize = 30;
    pub const SUBROUTINE_TYPE: usize = 31;
    pub const UNSPECIFIED_PARAMETERS: usize = 32;
    pub const PTR_TO_MEMBER_TYPE: usize = 33;
    pub const UNSPECIFIED_TYPE: usize = 34;
    pub const VIRTUAL_DECL: usize = 36;
    pub const SPECIAL_DECL: usize = 37;
    pub const STATIC_MEMBER: usize = 40;
    pub const FRIEND: usize = 45;
    pub const BASE_TYPE_ANY: usize = 49;
    pub const TYPEDEF_ANY: usize = 50;
    pub const TEMPLATE_ALIAS: usize = 51;
    pub const MEMBER_ANY: usize = 52;
    pub const INHERITANCE_VIRTUAL: usize = 53;
    pub const STATIC_VARIABLE_ANY: usize = 54;
    pub const STATIC_MEMBER_ANY: usize = 55;
    /// Structure, class, union: `+ 0`, `+ 1`, `+ 2`.
    pub const STRUCTURE_ANY: usize = 56;
    /// Their declarations: `+ 0`, `+ 1`, `+ 2`.
    pub const STRUCTURE_DECL_ANY: usize = 59;
    pub const ENUMERATION_ANY: usize = 62;
    pub const ARRAY_ANY: usize = 63;
    pub const SUBROUTINE_ANY: usize = 64;
    pub const METHOD: usize = 65;
    pub const METHOD_QUALIFIED: usize = 66;
    pub const GLOBAL_NAMELESS: usize = 67;
    pub const INDEX_TYPE: usize = 68;
    // WP-057 appends the subprogram definitions no reserved family holds.
    pub const SUBPROGRAM_ANY: usize = 69;
    pub const SUBPROGRAM_MEMBER: usize = 70;
    pub const LABEL_COLUMN: usize = 71;
    pub const LEXICAL_BLOCK: usize = 38;
    pub const LABEL: usize = 39;
    // WP-085: abstract instances and inlined subroutines.
    pub const SUBPROGRAM_ABSTRACT: usize = 72;
    pub const SUBPROGRAM_ABSTRACT_MEMBER: usize = 73;
    pub const SUBPROGRAM_OF_ABSTRACT: usize = 74;
    pub const INLINED_SUBROUTINE: usize = 75;
    pub const LEXICAL_BLOCK_OF_ABSTRACT: usize = 76;
    pub const LEXICAL_BLOCK_ABSTRACT: usize = 77;
    pub const FORMAL_PARAMETER_OF_ABSTRACT: usize = 78;
    pub const VARIABLE_OF_ABSTRACT: usize = 79;
    pub const LABEL_OF_ABSTRACT: usize = 80;
}

/// Every family, in code order (`spec/dwarf.md` §6.1 has the same table).
/// Frequent shapes come first so that their codes take one byte.
/// Append only.
pub const FAMILIES: &[Family] = &[
    // 0
    fam(
        "formal_parameter",
        tag::FORMAL_PARAMETER,
        No,
        16,
        "WP-057",
        &[
            o(0, at::LOCATION, EXPRLOC),
            o(1, at::NAME, STRX),
            o(2, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            s(at::TYPE, REF4),
            o(3, at::ARTIFICIAL, FLAG_PRESENT),
        ],
    ),
    fam(
        "variable (local)",
        tag::VARIABLE,
        No,
        8,
        "WP-057",
        &[
            o(0, at::LOCATION, EXPRLOC),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            s(at::TYPE, REF4),
            o(2, at::ARTIFICIAL, FLAG_PRESENT),
        ],
    ),
    fam(
        "member",
        tag::MEMBER,
        No,
        32,
        "WP-058",
        &[
            o(0, at::NAME, STRX),
            s(at::TYPE, REF4),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::DATA_MEMBER_LOCATION, UDATA),
            o(3, at::ARTIFICIAL, FLAG_PRESENT),
            o(4, at::ACCESSIBILITY, DATA1),
        ],
    ),
    fam(
        "subprogram (declaration)",
        tag::SUBPROGRAM,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::TYPE, REF4),
            s(at::DECLARATION, FLAG_PRESENT),
            o(3, at::EXTERNAL, FLAG_PRESENT),
        ],
    ),
    fam(
        "subprogram (definition)",
        tag::SUBPROGRAM,
        Optional,
        32,
        "WP-057",
        &[
            s(at::LOW_PC, ADDRX),
            s(at::HIGH_PC, DATA4),
            s(at::FRAME_BASE, EXPRLOC),
            o(0, at::LINKAGE_NAME, STRX),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::TYPE, REF4),
            o(3, at::EXTERNAL, FLAG_PRESENT),
        ],
    ),
    // 5
    fam(
        "pointer_type",
        tag::POINTER_TYPE,
        No,
        8,
        "WP-053",
        &[o(0, at::TYPE, REF4), o(1, at::NAME, STRX), o(2, at::ADDRESS_CLASS, UDATA)],
    ),
    fam(
        "base_type",
        tag::BASE_TYPE,
        No,
        2,
        "WP-053",
        &[s(at::NAME, STRX), s(at::ENCODING, DATA1), s(at::BYTE_SIZE, UDATA)],
    ),
    fam("const_type", tag::CONST_TYPE, No, 2, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam("reference_type", tag::REFERENCE_TYPE, No, 2, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam(
        "rvalue_reference_type",
        tag::RVALUE_REFERENCE_TYPE,
        No,
        2,
        "WP-058",
        &[o(0, at::TYPE, REF4)],
    ),
    // 10
    fam("volatile_type", tag::VOLATILE_TYPE, No, 2, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam("restrict_type", tag::RESTRICT_TYPE, No, 2, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam("atomic_type", tag::ATOMIC_TYPE, No, 2, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam(
        "typedef",
        tag::TYPEDEF,
        No,
        8,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::ALIGNMENT, UDATA),
        ],
    ),
    fam(
        "template_type_parameter",
        tag::TEMPLATE_TYPE_PARAMETER,
        No,
        8,
        "WP-059",
        &[o(0, at::TYPE, REF4), o(1, at::NAME, STRX), o(2, at::DEFAULT_VALUE, FLAG_PRESENT)],
    ),
    // 15
    fam(
        "template_value_parameter",
        tag::TEMPLATE_VALUE_PARAMETER,
        No,
        16,
        "WP-059",
        &[
            o(0, at::TYPE, REF4),
            o(1, at::NAME, STRX),
            o(2, at::DEFAULT_VALUE, FLAG_PRESENT),
            o(3, at::CONST_VALUE, SDATA),
        ],
    ),
    fam(
        "template_value_parameter (address)",
        tag::TEMPLATE_VALUE_PARAMETER,
        No,
        4,
        "WP-059",
        &[o(0, at::TYPE, REF4), o(1, at::NAME, STRX), s(at::LOCATION, EXPRLOC)],
    ),
    fam(
        "GNU_template_parameter_pack",
        tag::GNU_TEMPLATE_PARAMETER_PACK,
        Optional,
        4,
        "WP-059",
        &[o(0, at::NAME, STRX)],
    ),
    fam(
        "GNU_template_template_param",
        tag::GNU_TEMPLATE_TEMPLATE_PARAM,
        No,
        2,
        "WP-059",
        &[o(0, at::NAME, STRX), s(at::GNU_TEMPLATE_NAME, STRX)],
    ),
    fam(
        "inheritance",
        tag::INHERITANCE,
        No,
        4,
        "WP-058",
        &[
            s(at::TYPE, REF4),
            s(at::DATA_MEMBER_LOCATION, UDATA),
            o(0, at::ACCESSIBILITY, DATA1),
            o(1, at::VIRTUALITY, DATA1),
        ],
    ),
    // 20
    fam(
        "structure_type",
        tag::STRUCTURE_TYPE,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::CONTAINING_TYPE, REF4),
            o(1, at::CALLING_CONVENTION, DATA1),
            o(2, at::NAME, STRX),
            s(at::BYTE_SIZE, UDATA),
            o(3, at::DECL_FILE, UDATA),
            o(3, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "class_type",
        tag::CLASS_TYPE,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::CONTAINING_TYPE, REF4),
            o(1, at::CALLING_CONVENTION, DATA1),
            o(2, at::NAME, STRX),
            s(at::BYTE_SIZE, UDATA),
            o(3, at::DECL_FILE, UDATA),
            o(3, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "union_type",
        tag::UNION_TYPE,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::CONTAINING_TYPE, REF4),
            o(1, at::CALLING_CONVENTION, DATA1),
            o(2, at::NAME, STRX),
            s(at::BYTE_SIZE, UDATA),
            o(3, at::DECL_FILE, UDATA),
            o(3, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "structure_type (declaration)",
        tag::STRUCTURE_TYPE,
        Optional,
        4,
        "WP-058",
        &[o(0, at::NAME, STRX), s(at::DECLARATION, FLAG_PRESENT)],
    ),
    fam(
        "class_type (declaration)",
        tag::CLASS_TYPE,
        Optional,
        4,
        "WP-058",
        &[o(0, at::NAME, STRX), s(at::DECLARATION, FLAG_PRESENT)],
    ),
    // 25
    fam(
        "union_type (declaration)",
        tag::UNION_TYPE,
        Optional,
        4,
        "WP-058",
        &[o(0, at::NAME, STRX), s(at::DECLARATION, FLAG_PRESENT)],
    ),
    fam(
        "enumeration_type",
        tag::ENUMERATION_TYPE,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            o(1, at::ENUM_CLASS, FLAG_PRESENT),
            o(2, at::NAME, STRX),
            s(at::BYTE_SIZE, UDATA),
            o(3, at::DECL_FILE, UDATA),
            o(3, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "enumerator",
        tag::ENUMERATOR,
        No,
        4,
        "WP-058",
        &[s(at::NAME, STRX), o(0, at::CONST_VALUE, UDATA), o(1, at::CONST_VALUE, SDATA)],
    ),
    fam("array_type", tag::ARRAY_TYPE, Yes, 2, "WP-058", &[s(at::TYPE, REF4)]),
    fam(
        "subrange_type",
        tag::SUBRANGE_TYPE,
        No,
        8,
        "WP-058",
        &[
            s(at::TYPE, REF4),
            o(1, at::LOWER_BOUND, SDATA),
            o(0, at::COUNT, UDATA),
            o(2, at::UPPER_BOUND, SDATA),
        ],
    ),
    // 30
    fam(
        "subrange_type (variable count)",
        tag::SUBRANGE_TYPE,
        No,
        2,
        "WP-058",
        &[s(at::TYPE, REF4), s(at::COUNT, REF4)],
    ),
    fam("subroutine_type", tag::SUBROUTINE_TYPE, Optional, 4, "WP-058", &[o(0, at::TYPE, REF4)]),
    fam("unspecified_parameters", tag::UNSPECIFIED_PARAMETERS, No, 1, "WP-058", &[]),
    fam(
        "ptr_to_member_type",
        tag::PTR_TO_MEMBER_TYPE,
        No,
        2,
        "WP-058",
        &[s(at::TYPE, REF4), s(at::CONTAINING_TYPE, REF4)],
    ),
    fam("unspecified_type", tag::UNSPECIFIED_TYPE, No, 2, "WP-058", &[o(0, at::NAME, STRX)]),
    // 35
    fam(
        "subprogram (out-of-line definition)",
        tag::SUBPROGRAM,
        Optional,
        16,
        "WP-059",
        &[
            s(at::LOW_PC, ADDRX),
            s(at::HIGH_PC, DATA4),
            s(at::FRAME_BASE, EXPRLOC),
            o(0, at::OBJECT_POINTER, REF4),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::LINKAGE_NAME, STRX),
            s(at::SPECIFICATION, REF4),
        ],
    ),
    fam(
        "subprogram (virtual declaration)",
        tag::SUBPROGRAM,
        Optional,
        32,
        "WP-058",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::TYPE, REF4),
            s(at::VIRTUALITY, DATA1),
            s(at::VTABLE_ELEM_LOCATION, EXPRLOC),
            s(at::DECLARATION, FLAG_PRESENT),
            o(3, at::EXTERNAL, FLAG_PRESENT),
            s(at::CONTAINING_TYPE, REF4),
        ],
    ),
    fam(
        "subprogram (special declaration)",
        tag::SUBPROGRAM,
        Optional,
        256,
        "WP-058",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            s(at::NAME, STRX),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::TYPE, REF4),
            s(at::DECLARATION, FLAG_PRESENT),
            o(3, at::ARTIFICIAL, FLAG_PRESENT),
            o(4, at::EXTERNAL, FLAG_PRESENT),
            o(5, at::ACCESSIBILITY, DATA1),
            o(6, at::OBJECT_POINTER, REF4),
        ],
    ),
    fam(
        "lexical_block",
        tag::LEXICAL_BLOCK,
        Yes,
        4,
        "WP-057",
        &[o(0, at::LOW_PC, ADDRX), o(0, at::HIGH_PC, DATA4), o(1, at::RANGES, SEC_OFFSET)],
    ),
    fam(
        "label",
        tag::LABEL,
        No,
        4,
        "WP-057",
        &[
            s(at::NAME, STRX),
            o(0, at::DECL_FILE, UDATA),
            o(0, at::DECL_LINE, UDATA),
            o(1, at::LOW_PC, ADDRX),
        ],
    ),
    // 40
    fam(
        "member (static, declaration)",
        tag::VARIABLE,
        No,
        16,
        "WP-058",
        &[
            s(at::NAME, STRX),
            s(at::TYPE, REF4),
            o(0, at::DECL_FILE, UDATA),
            o(0, at::DECL_LINE, UDATA),
            o(1, at::EXTERNAL, FLAG_PRESENT),
            s(at::DECLARATION, FLAG_PRESENT),
            o(2, at::ACCESSIBILITY, DATA1),
            o(3, at::CONST_VALUE, SDATA),
        ],
    ),
    fam(
        "variable (specification)",
        tag::VARIABLE,
        No,
        4,
        "WP-059",
        &[s(at::SPECIFICATION, REF4), o(0, at::LOCATION, EXPRLOC), o(1, at::LINKAGE_NAME, STRX)],
    ),
    fam(
        "variable (constant)",
        tag::VARIABLE,
        No,
        4,
        "WP-059",
        &[
            s(at::NAME, STRX),
            s(at::TYPE, REF4),
            o(0, at::EXTERNAL, FLAG_PRESENT),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            s(at::CONST_VALUE, SDATA),
        ],
    ),
    fam(
        "imported_module",
        tag::IMPORTED_MODULE,
        No,
        2,
        "WP-059",
        &[o(0, at::DECL_FILE, UDATA), o(0, at::DECL_LINE, UDATA), s(at::IMPORT, REF4)],
    ),
    fam(
        "imported_declaration",
        tag::IMPORTED_DECLARATION,
        No,
        4,
        "WP-059",
        &[
            o(0, at::DECL_FILE, UDATA),
            o(0, at::DECL_LINE, UDATA),
            s(at::IMPORT, REF4),
            o(1, at::NAME, STRX),
        ],
    ),
    // 45
    fam("friend", tag::FRIEND, No, 2, "WP-058", &[s(at::FRIEND, REF4)]),
    fam(
        "namespace",
        tag::NAMESPACE,
        Optional,
        8,
        "WP-053",
        &[o(0, at::NAME, STRX), o(1, at::EXPORT_SYMBOLS, FLAG_PRESENT)],
    ),
    fam(
        "variable (global)",
        tag::VARIABLE,
        No,
        32,
        "WP-053",
        &[
            s(at::NAME, STRX),
            s(at::TYPE, REF4),
            o(0, at::EXTERNAL, FLAG_PRESENT),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::ALIGNMENT, UDATA),
            o(3, at::LOCATION, EXPRLOC),
            o(4, at::LINKAGE_NAME, STRX),
        ],
    ),
    fam(
        "compile_unit",
        tag::COMPILE_UNIT,
        Optional,
        16,
        "WP-053",
        &[
            s(at::PRODUCER, STRX),
            s(at::LANGUAGE, DATA2),
            s(at::NAME, STRX),
            s(at::STR_OFFSETS_BASE, SEC_OFFSET),
            s(at::STMT_LIST, SEC_OFFSET),
            s(at::COMP_DIR, STRX),
            o(0, at::LOW_PC, ADDRX),
            o(0, at::HIGH_PC, DATA4),
            o(1, at::LOW_PC, ADDR),
            o(1, at::RANGES, RNGLISTX),
            o(2, at::ADDR_BASE, SEC_OFFSET),
            o(1, at::RNGLISTS_BASE, SEC_OFFSET),
        ],
    ),
    // 49: WP-058's general shapes of the type graph.
    fam(
        "base_type (any)",
        tag::BASE_TYPE,
        No,
        8,
        "WP-058",
        &[
            o(0, at::NAME, STRX),
            s(at::ENCODING, DATA1),
            s(at::BYTE_SIZE, UDATA),
            o(1, at::ENDIANITY, DATA1),
            o(2, at::BIT_SIZE, UDATA),
        ],
    ),
    // 50
    fam(
        "typedef (any)",
        tag::TYPEDEF,
        No,
        16,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            s(at::NAME, STRX),
            o(2, at::ALIGNMENT, UDATA),
            o(3, at::ACCESSIBILITY, DATA1),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "template_alias",
        tag::TEMPLATE_ALIAS,
        No,
        8,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            s(at::NAME, STRX),
            o(2, at::ACCESSIBILITY, DATA1),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
        ],
    ),
    fam(
        "member (any)",
        tag::MEMBER,
        No,
        256,
        "WP-058",
        &[
            o(0, at::NAME, STRX),
            s(at::TYPE, REF4),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::BIT_SIZE, UDATA),
            o(2, at::DATA_BIT_OFFSET, UDATA),
            o(3, at::ALIGNMENT, UDATA),
            o(4, at::DATA_MEMBER_LOCATION, UDATA),
            o(5, at::ACCESSIBILITY, DATA1),
            o(6, at::VIRTUALITY, DATA1),
            o(7, at::ARTIFICIAL, FLAG_PRESENT),
        ],
    ),
    fam(
        "inheritance (virtual)",
        tag::INHERITANCE,
        No,
        2,
        "WP-058",
        &[
            s(at::TYPE, REF4),
            s(at::DATA_MEMBER_LOCATION, EXPRLOC),
            o(0, at::ACCESSIBILITY, DATA1),
            s(at::VIRTUALITY, DATA1),
        ],
    ),
    fam("member (static, declaration, any)", tag::VARIABLE, No, 64, "WP-058", STATIC_ANY),
    // 55
    fam("member (static, DW_TAG_member, any)", tag::MEMBER, No, 64, "WP-058", STATIC_ANY),
    fam("structure_type (any)", tag::STRUCTURE_TYPE, Optional, 256, "WP-058", COMPOSITE_ANY),
    fam("class_type (any)", tag::CLASS_TYPE, Optional, 256, "WP-058", COMPOSITE_ANY),
    fam("union_type (any)", tag::UNION_TYPE, Optional, 256, "WP-058", COMPOSITE_ANY),
    fam(
        "structure_type (declaration, any)",
        tag::STRUCTURE_TYPE,
        Optional,
        8,
        "WP-058",
        DECLARATION_ANY,
    ),
    // 60
    fam("class_type (declaration, any)", tag::CLASS_TYPE, Optional, 8, "WP-058", DECLARATION_ANY),
    fam("union_type (declaration, any)", tag::UNION_TYPE, Optional, 8, "WP-058", DECLARATION_ANY),
    fam(
        "enumeration_type (any)",
        tag::ENUMERATION_TYPE,
        Optional,
        512,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            o(1, at::ENUM_CLASS, FLAG_PRESENT),
            o(2, at::NAME, STRX),
            o(3, at::BYTE_SIZE, UDATA),
            o(4, at::DECLARATION, FLAG_PRESENT),
            o(5, at::ACCESSIBILITY, DATA1),
            o(6, at::DECL_FILE, UDATA),
            o(6, at::DECL_LINE, UDATA),
            o(7, at::ALIGNMENT, UDATA),
        ],
    ),
    fam(
        "array_type (any)",
        tag::ARRAY_TYPE,
        Optional,
        16,
        "WP-058",
        &[
            o(0, at::GNU_VECTOR, FLAG_PRESENT),
            o(1, at::BYTE_SIZE, UDATA),
            s(at::TYPE, REF4),
            o(2, at::NAME, STRX),
        ],
    ),
    fam(
        "subroutine_type (any)",
        tag::SUBROUTINE_TYPE,
        Optional,
        64,
        "WP-058",
        &[
            o(0, at::TYPE, REF4),
            o(1, at::PROTOTYPED, FLAG_PRESENT),
            o(2, at::CALLING_CONVENTION, DATA1),
            o(3, at::REFERENCE, FLAG_PRESENT),
            o(4, at::RVALUE_REFERENCE, FLAG_PRESENT),
        ],
    ),
    // 65
    fam(
        "subprogram (member declaration)",
        tag::SUBPROGRAM,
        Optional,
        2048,
        "WP-058",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            o(1, at::NAME, STRX),
            o(2, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            o(3, at::TYPE, REF4),
            o(4, at::VIRTUALITY, DATA1),
            o(4, at::VTABLE_ELEM_LOCATION, EXPRLOC),
            s(at::DECLARATION, FLAG_PRESENT),
            o(5, at::ARTIFICIAL, FLAG_PRESENT),
            o(6, at::EXTERNAL, FLAG_PRESENT),
            o(7, at::ACCESSIBILITY, DATA1),
            o(8, at::EXPLICIT, FLAG_PRESENT),
            o(9, at::DELETED, FLAG_PRESENT),
            o(4, at::CONTAINING_TYPE, REF4),
        ],
    ),
    fam(
        "subprogram (member declaration, qualified)",
        tag::SUBPROGRAM,
        Optional,
        4096,
        "WP-058",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            o(1, at::NAME, STRX),
            o(2, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            o(3, at::TYPE, REF4),
            o(4, at::VIRTUALITY, DATA1),
            o(4, at::VTABLE_ELEM_LOCATION, EXPRLOC),
            s(at::DECLARATION, FLAG_PRESENT),
            o(5, at::EXTERNAL, FLAG_PRESENT),
            o(7, at::REFERENCE, FLAG_PRESENT),
            o(8, at::RVALUE_REFERENCE, FLAG_PRESENT),
            o(9, at::NORETURN, FLAG_PRESENT),
            o(6, at::ACCESSIBILITY, DATA1),
            o(10, at::DELETED, FLAG_PRESENT),
            o(4, at::CONTAINING_TYPE, REF4),
        ],
    ),
    fam(
        "variable (global, no name)",
        tag::VARIABLE,
        No,
        32,
        "WP-058",
        &[
            s(at::TYPE, REF4),
            o(0, at::EXTERNAL, FLAG_PRESENT),
            o(1, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::ALIGNMENT, UDATA),
            o(3, at::LOCATION, EXPRLOC),
            o(4, at::LINKAGE_NAME, STRX),
        ],
    ),
    fam(
        "base_type (array index)",
        tag::BASE_TYPE,
        No,
        1,
        "WP-058",
        &[s(at::NAME, STRX), s(at::BYTE_SIZE, UDATA), s(at::ENCODING, DATA1)],
    ),
    // 69: WP-057's subprogram definitions.
    fam(
        "subprogram (definition, any)",
        tag::SUBPROGRAM,
        Optional,
        512,
        "WP-057",
        &[
            s(at::LOW_PC, ADDRX),
            s(at::HIGH_PC, DATA4),
            s(at::FRAME_BASE, EXPRLOC),
            o(0, at::LINKAGE_NAME, STRX),
            o(1, at::NAME, STRX),
            o(2, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            o(3, at::PROTOTYPED, FLAG_PRESENT),
            o(4, at::TYPE, REF4),
            o(5, at::ARTIFICIAL, FLAG_PRESENT),
            o(6, at::EXTERNAL, FLAG_PRESENT),
            o(7, at::NORETURN, FLAG_PRESENT),
        ],
    ),
    fam(
        "subprogram (member definition)",
        tag::SUBPROGRAM,
        Optional,
        32,
        "WP-057",
        &[
            s(at::LOW_PC, ADDRX),
            s(at::HIGH_PC, DATA4),
            s(at::FRAME_BASE, EXPRLOC),
            o(0, at::OBJECT_POINTER, REF4),
            o(1, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            o(3, at::LINKAGE_NAME, STRX),
            s(at::SPECIFICATION, REF4),
        ],
    ),
    // 71: a label with a column (LLVM 21 and later).
    fam(
        "label (with column)",
        tag::LABEL,
        No,
        2,
        "WP-057",
        &[
            s(at::NAME, STRX),
            s(at::DECL_FILE, UDATA),
            s(at::DECL_LINE, UDATA),
            s(at::DECL_COLUMN, UDATA),
            o(0, at::LOW_PC, ADDRX),
        ],
    ),
    // 72 to 80: WP-085's abstract instances (`DW_AT_inline`, written as
    // `data1` where `llc` writes `implicit_const`, §6.7), the concrete
    // DIEs that name them by `DW_AT_abstract_origin`, and inlined
    // subroutines.
    fam(
        "subprogram (abstract)",
        tag::SUBPROGRAM,
        Optional,
        512,
        "WP-085",
        &[
            o(0, at::LINKAGE_NAME, STRX),
            o(1, at::NAME, STRX),
            o(2, at::DECL_FILE, UDATA),
            o(2, at::DECL_LINE, UDATA),
            o(3, at::PROTOTYPED, FLAG_PRESENT),
            o(4, at::TYPE, REF4),
            o(5, at::ARTIFICIAL, FLAG_PRESENT),
            o(6, at::EXTERNAL, FLAG_PRESENT),
            o(7, at::NORETURN, FLAG_PRESENT),
            s(at::INLINE, DATA1),
        ],
    ),
    fam(
        "subprogram (abstract member)",
        tag::SUBPROGRAM,
        Optional,
        32,
        "WP-085",
        &[
            o(0, at::DECL_FILE, UDATA),
            o(1, at::DECL_LINE, UDATA),
            o(2, at::LINKAGE_NAME, STRX),
            s(at::SPECIFICATION, REF4),
            s(at::INLINE, DATA1),
            o(3, at::OBJECT_POINTER, REF4),
        ],
    ),
    fam(
        "subprogram (of abstract)",
        tag::SUBPROGRAM,
        Optional,
        4,
        "WP-085",
        &[
            s(at::LOW_PC, ADDRX),
            s(at::HIGH_PC, DATA4),
            s(at::FRAME_BASE, EXPRLOC),
            o(0, at::OBJECT_POINTER, REF4),
            s(at::ABSTRACT_ORIGIN, REF4),
        ],
    ),
    fam(
        "inlined_subroutine",
        tag::INLINED_SUBROUTINE,
        Optional,
        16,
        "WP-085",
        &[
            s(at::ABSTRACT_ORIGIN, REF4),
            o(0, at::LOW_PC, ADDRX),
            o(0, at::HIGH_PC, DATA4),
            o(1, at::RANGES, SEC_OFFSET),
            s(at::CALL_FILE, UDATA),
            s(at::CALL_LINE, UDATA),
            o(2, at::CALL_COLUMN, UDATA),
        ],
    ),
    fam(
        "lexical_block (of abstract)",
        tag::LEXICAL_BLOCK,
        Yes,
        4,
        "WP-085",
        &[
            o(0, at::LOW_PC, ADDRX),
            o(0, at::HIGH_PC, DATA4),
            o(1, at::RANGES, SEC_OFFSET),
            s(at::ABSTRACT_ORIGIN, REF4),
        ],
    ),
    fam("lexical_block (abstract)", tag::LEXICAL_BLOCK, Yes, 1, "WP-085", &[]),
    fam(
        "formal_parameter (of abstract)",
        tag::FORMAL_PARAMETER,
        No,
        1,
        "WP-085",
        &[s(at::LOCATION, EXPRLOC), s(at::ABSTRACT_ORIGIN, REF4)],
    ),
    fam(
        "variable (of abstract)",
        tag::VARIABLE,
        No,
        1,
        "WP-085",
        &[s(at::LOCATION, EXPRLOC), s(at::ABSTRACT_ORIGIN, REF4)],
    ),
    fam(
        "label (of abstract)",
        tag::LABEL,
        No,
        1,
        "WP-085",
        &[s(at::ABSTRACT_ORIGIN, REF4), s(at::LOW_PC, ADDRX)],
    ),
];

/// The slots of the general static-member families (54, 55), in `llc`'s
/// order: one `const_value` form per sign (exclusive option bits 3, 4).
const STATIC_ANY: &[Slot] = &[
    s(at::NAME, STRX),
    s(at::TYPE, REF4),
    o(0, at::DECL_FILE, UDATA),
    o(0, at::DECL_LINE, UDATA),
    s(at::EXTERNAL, FLAG_PRESENT),
    s(at::DECLARATION, FLAG_PRESENT),
    o(1, at::ARTIFICIAL, FLAG_PRESENT),
    o(2, at::ACCESSIBILITY, DATA1),
    o(3, at::CONST_VALUE, UDATA),
    o(4, at::CONST_VALUE, SDATA),
    o(5, at::ALIGNMENT, UDATA),
];

/// The slots of the general structure, class and union families (56 to
/// 58), in `llc`'s order.
const COMPOSITE_ANY: &[Slot] = &[
    o(0, at::EXPORT_SYMBOLS, FLAG_PRESENT),
    o(1, at::CONTAINING_TYPE, REF4),
    o(2, at::CALLING_CONVENTION, DATA1),
    o(3, at::NAME, STRX),
    s(at::BYTE_SIZE, UDATA),
    o(4, at::ACCESSIBILITY, DATA1),
    o(5, at::DECL_FILE, UDATA),
    o(5, at::DECL_LINE, UDATA),
    o(6, at::ALIGNMENT, UDATA),
];

/// The slots of the general declaration families (59 to 61).
const DECLARATION_ANY: &[Slot] =
    &[o(0, at::NAME, STRX), s(at::DECLARATION, FLAG_PRESENT), o(1, at::ACCESSIBILITY, DATA1)];

/// The number of option bits of a family (not counting the children
/// bit).
pub const fn option_bits(f: &Family) -> u32 {
    let mut bits = 0;
    let mut i = 0;
    while i < f.slots.len() {
        if f.slots[i].bit as u32 > bits {
            bits = f.slots[i].bit as u32;
        }
        i += 1;
    }
    bits
}

const fn bases() -> [u32; FAMILIES.len()] {
    let mut out = [0; FAMILIES.len()];
    let mut next = 1;
    let mut i = 0;
    while i < FAMILIES.len() {
        let f = &FAMILIES[i];
        let needed = 1u32 << (option_bits(f) + (matches!(f.children, Optional) as u32));
        assert!(
            f.block.is_power_of_two() && needed <= f.block as u32,
            "a family's block is too small"
        );
        out[i] = next;
        next += f.block as u32;
        i += 1;
    }
    out
}

/// The first code of each family.
pub const BASES: [u32; FAMILIES.len()] = bases();

/// One past the last code of the table.
pub const CODE_END: u32 = BASES[FAMILIES.len() - 1] + FAMILIES[FAMILIES.len() - 1].block as u32;
// Every code takes at most two ULEB128 bytes.
const _: () = assert!(CODE_END <= 1 << 14);

/// The code of a DIE of family `family` with option `mask` and, for a
/// family with optional children, `children`.
#[inline]
pub(crate) const fn code(family: usize, mask: u32, children: bool) -> u32 {
    let f = &FAMILIES[family];
    let child = if matches!(f.children, Optional) && children { 1 << option_bits(f) } else { 0 };
    BASES[family] + (mask | child)
}

/// The codes an object used: a bit per code.
#[derive(Clone, Debug)]
pub(crate) struct Used(Vec<u64>);

impl Used {
    pub(crate) fn new() -> Self {
        Self(vec![0; (CODE_END as usize).div_ceil(64)])
    }

    #[inline]
    pub(crate) fn mark(&mut self, code: u32) {
        self.0[code as usize / 64] |= 1 << (code % 64);
    }

    #[inline]
    fn has(&self, code: u32) -> bool {
        self.0[code as usize / 64] & (1 << (code % 64)) != 0
    }
}

/// `.debug_abbrev`: the entries of the used codes, in code order, then
/// the terminating 0.
pub(crate) fn write_abbrev(used: &Used) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    for (i, f) in FAMILIES.iter().enumerate() {
        let bits = option_bits(f);
        for offset in 0..u32::from(f.block) {
            let code = BASES[i] + offset;
            if !used.has(code) {
                continue;
            }
            let mask = offset & ((1 << bits) - 1);
            let children = match f.children {
                No => false,
                Yes => true,
                Optional => offset >> bits != 0,
            };
            write_uleb(&mut out, u64::from(code));
            write_uleb(&mut out, u64::from(f.tag));
            out.push(u8::from(children));
            for slot in f.slots {
                if slot.bit == 0 || mask & (1 << (slot.bit - 1)) != 0 {
                    write_uleb(&mut out, u64::from(slot.at));
                    write_uleb(&mut out, u64::from(slot.form));
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.1: the codes of the families WP-053 writes are fixed; a change
    /// here renumbers what exists, which the table forbids.
    #[test]
    fn codes_are_fixed() {
        assert_eq!(BASES[fam::FORMAL_PARAMETER], 1);
        assert_eq!(BASES[fam::POINTER_TYPE], 121);
        assert_eq!(BASES[fam::BASE_TYPE], 129);
        assert_eq!(BASES[fam::NAMESPACE], 698);
        assert_eq!(BASES[fam::GLOBAL_VARIABLE], 706);
        assert_eq!(BASES[fam::COMPILE_UNIT], 738);
        // WP-058 appended families 49 to 67 after the end WP-053 left.
        assert_eq!(BASES[fam::BASE_TYPE_ANY], 754);
        // WP-057 appended families 69 to 71.
        assert_eq!(BASES[fam::SUBPROGRAM_ANY], 8733);
        // WP-085 appended families 72 to 80.
        assert_eq!(BASES[fam::SUBPROGRAM_ABSTRACT], 9279);
        assert_eq!(CODE_END, 9851);
        for (index, name) in [
            (fam::CONST_TYPE, "const_type"),
            (fam::TYPEDEF, "typedef"),
            (fam::INHERITANCE, "inheritance"),
            (fam::STRUCTURE_TYPE, "structure_type"),
            (fam::STRUCTURE_DECL, "structure_type (declaration)"),
            (fam::ENUMERATION_TYPE, "enumeration_type"),
            (fam::ENUMERATOR, "enumerator"),
            (fam::ARRAY_TYPE, "array_type"),
            (fam::SUBRANGE, "subrange_type"),
            (fam::SUBRANGE_VAR_COUNT, "subrange_type (variable count)"),
            (fam::SUBROUTINE_TYPE, "subroutine_type"),
            (fam::UNSPECIFIED_PARAMETERS, "unspecified_parameters"),
            (fam::PTR_TO_MEMBER_TYPE, "ptr_to_member_type"),
            (fam::UNSPECIFIED_TYPE, "unspecified_type"),
            (fam::VIRTUAL_DECL, "subprogram (virtual declaration)"),
            (fam::SPECIAL_DECL, "subprogram (special declaration)"),
            (fam::STATIC_MEMBER, "member (static, declaration)"),
            (fam::FRIEND, "friend"),
            (fam::BASE_TYPE_ANY, "base_type (any)"),
            (fam::STRUCTURE_ANY + 2, "union_type (any)"),
            (fam::STRUCTURE_DECL_ANY + 1, "class_type (declaration, any)"),
            (fam::METHOD, "subprogram (member declaration)"),
            (fam::GLOBAL_NAMELESS, "variable (global, no name)"),
        ] {
            assert_eq!(FAMILIES[index].name, name);
        }
        assert_eq!(FAMILIES[fam::NAMESPACE].tag, tag::NAMESPACE);
        assert_eq!(FAMILIES[fam::GLOBAL_VARIABLE].tag, tag::VARIABLE);
        assert_eq!(FAMILIES[fam::COMPILE_UNIT].tag, tag::COMPILE_UNIT);
        assert_eq!(FAMILIES[fam::SUBPROGRAM].tag, tag::SUBPROGRAM);
        assert_eq!(FAMILIES[fam::VARIABLE].tag, tag::VARIABLE);
        assert_eq!(FAMILIES[fam::MEMBER].tag, tag::MEMBER);
        assert_eq!(FAMILIES[fam::SUBPROGRAM_DECL].tag, tag::SUBPROGRAM);
    }

    /// Every abbreviation of the table is well formed: no attribute twice
    /// under one mask, option bits used without gaps.
    #[test]
    fn every_code_is_a_valid_abbreviation() {
        for f in FAMILIES {
            let bits = option_bits(f);
            for b in 1..=bits {
                assert!(
                    f.slots.iter().any(|s| u32::from(s.bit) == b),
                    "{}: bit {b} unused",
                    f.name
                );
            }
            for mask in 0..1u32 << bits {
                let mut seen = Vec::new();
                for s in f.slots {
                    if s.bit == 0 || mask & (1 << (s.bit - 1)) != 0 {
                        seen.push(s.at);
                    }
                }
                let n = seen.len();
                seen.sort_unstable();
                seen.dedup();
                // Exclusive alternatives (the unit's two `low_pc` forms,
                // the enumerator's two value forms) repeat an attribute
                // only under a mask that sets both, which is never
                // written.
                // So do the general static members' two `const_value`
                // forms (bits 3 and 4).
                let exclusive = ((f.tag == tag::COMPILE_UNIT || f.tag == tag::ENUMERATOR)
                    && mask & 0b11 == 0b11)
                    || (f.name.starts_with("member (static") && mask & 0b11000 == 0b11000);
                assert!(seen.len() == n || exclusive, "{}: mask {mask:#b} repeats", f.name);
            }
        }
        let mut all = Used::new();
        for c in 1..CODE_END {
            all.mark(c);
        }
        let table = write_abbrev(&all);
        // Every code at once (no object uses more than a few hundred).
        assert!(table.len() < 512 * 1024, "{}", table.len());
        assert_eq!(table.last(), Some(&0));
    }

    #[test]
    fn codes() {
        assert_eq!(code(fam::BASE_TYPE, 0, false), 129);
        assert_eq!(code(fam::NAMESPACE, 0b01, true), 698 + 0b101);
        assert_eq!(code(fam::COMPILE_UNIT, 0b110, true), 738 + 0b1110);
        let mut used = Used::new();
        used.mark(code(fam::BASE_TYPE, 0, false));
        // code 129 (two bytes), base_type, no children, three attributes.
        assert_eq!(
            write_abbrev(&used),
            [0x81, 0x01, 0x24, 0, 0x03, 0x1a, 0x3e, 0x0b, 0x0b, 0x0f, 0, 0, 0]
        );
    }
}
