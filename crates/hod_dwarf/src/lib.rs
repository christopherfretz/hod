//! DWARF 5 debug information writer (`spec/dwarf.md`).
//!
//! Implemented: the line table (§4), the line policy (§5), and the core
//! of the DIE writer (§6.1 to §6.3): the fixed abbreviation table, the
//! compile unit, `.debug_str`/`.debug_str_offsets`/`.debug_addr`/
//! `.debug_rnglists`, and the global variables, namespaces, base and
//! pointer types of a unit. The line writer takes plain rows of its own
//! types; the DIE writer reads `hod_ir`'s debug tables directly
//! ([`UnitPlan`]), with the addresses the code generator placed given as
//! symbol references ([`GlobalAddr`], [`CodeSpan`]).
//!
//! ```
//! use hod_dwarf::{FileTable, FunctionLines, Sequence, SourceLoc, Transition};
//! use hod_dwarf::{function_rows, write_debug_line};
//! use hod_obj::{Arch, Object, SectionKind};
//!
//! let mut obj = Object::new(Arch::AArch64);
//! let text = obj.add_section(b".text", SectionKind::Text, 4)?;
//! obj.append(text, &[0; 24]);
//! let text_sym = obj.section_symbol(text);
//!
//! let files = FileTable::new(b"/src", b"main.cpp", None);
//! let at = |line, column| Some(SourceLoc { file: 0, line, column });
//! let transitions = [
//!     Transition::at(8, at(4, 7)),
//!     Transition::at(16, at(5, 3)),
//! ];
//! let lines = FunctionLines {
//!     scope: SourceLoc { file: 0, line: 3, column: 0 },
//!     size: 24,
//!     transitions: &transitions,
//!     epilogue: true,
//! };
//! let mut rows = Vec::new();
//! function_rows(&lines, &mut rows)?;
//! assert_eq!(rows.len(), 3); // the scope line, the body, the epilogue
//!
//! let seq = Sequence { order: 0, symbol: text_sym, addend: 0, size: 24, rows: &rows };
//! let sections = write_debug_line(&mut obj, &files.header(), &[seq])?;
//! assert_eq!(obj.relocation_count(sections.line), 3); // two names, one address
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

use std::fmt;

pub mod abbrev;
mod die;
mod early;
mod func;
mod global;
mod import;
mod inline;
mod leb;
mod line;
mod policy;
mod reduced;
pub mod scope;
mod spec;
mod split;
mod strings;
mod target;
mod template;
mod types;

pub use die::{CodeSpan, DieSections, GlobalAddr, UnitInput, UnitPlan, write_debug_info};
pub use early::{LOCAL_TYPE_SCOPE, check_functions};
pub use func::{FunctionInput, LocalPlace, LocalRow, ScopeRow};
pub use inline::{
    DEBUG_INLINED_DISCRIMINATOR, DEBUG_INLINED_FOREIGN_UNIT, DEBUG_INLINED_LOCAL_TYPE,
    DEBUG_INLINED_MEMBER, DEBUG_LOCAL_STATIC, DEBUG_RETAINED_NODES,
};
pub use reduced::{LINE_TABLES_INLINED, LINE_TABLES_VARIABLES, REDUCED_UNIT_ENTITIES, has_dies};
pub use split::{
    DWO_ID, SPLIT_DWARF_4, SPLIT_INLINING, SPLIT_NAME, SPLIT_REDUCED, SPLIT_SEVERAL_UNITS,
    SplitDwarf, SplitSections, dwo_id, write_split_debug_info, write_unit_dies,
};
pub use strings::DebugStrings;

pub use leb::{write_sleb, write_uleb};
pub use line::{
    FileEntry, FileTable, LineHeader, LineProgram, LineSections, LineUnit, LineUnits, Row,
    Sequence, flags, write_debug_line, write_debug_line_units,
};
pub use policy::{FunctionLines, SourceLoc, Transition, function_rows};
pub use target::{
    AARCH64, LINE_BASE, LINE_RANGE, LineTarget, OPCODE_BASE, STANDARD_OPCODE_LENGTHS, X86_64,
    for_arch,
};

/// Why a table was not written (`spec/dwarf.md` §4.6, §6.6). The object
/// is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The header has no directory or no file: DWARF 5 requires entry 0
    /// of both.
    NoPrimary,
    /// A file's directory is not an index of the header's directories.
    BadDirectory {
        /// The file's index.
        file: u32,
    },
    /// A directory or file name contains a NUL byte.
    NulInName,
    /// A row's file is not an index of the header's files.
    BadFile {
        /// The sequence's `order`.
        order: u32,
        /// The row's index.
        row: u32,
    },
    /// A row's offset is below the previous row's.
    RowOutOfOrder {
        /// The sequence's `order`.
        order: u32,
        /// The row's index.
        row: u32,
    },
    /// A row's offset is not below the sequence's size.
    RowBeyondEnd {
        /// The sequence's `order`.
        order: u32,
        /// The row's index.
        row: u32,
    },
    /// An offset or a size is not a multiple of the minimum instruction
    /// length.
    Unaligned {
        /// The sequence's `order`.
        order: u32,
        /// The row's index; the row count for the sequence's size.
        row: u32,
    },
    /// A row has a flag outside [`flags::ALL`].
    BadFlags {
        /// The sequence's `order`.
        order: u32,
        /// The row's index.
        row: u32,
    },
    /// Two sequences have the same `order`, which would make the output
    /// depend on the input's order.
    DuplicateOrder {
        /// The `order`.
        order: u32,
    },
    /// A transition's offset is below the previous one's (§5).
    TransitionOutOfOrder {
        /// The transition's index.
        index: u32,
    },
    /// A section would reach 4 GiB, beyond 32-bit DWARF.
    TooLarge,
    /// [`write_debug_info`] was given no unit.
    NoUnit,
    /// A unit is not a `DICompileUnit` node.
    NotACompileUnit {
        /// The node.
        node: u32,
    },
    /// A node's field names no node or a node of a kind the field does
    /// not allow, or a namespace's scopes form a cycle.
    BadReference {
        /// The node.
        node: u32,
        /// LLVM's name of the field.
        field: &'static str,
    },
    /// A construct the DIE writer does not write yet (`spec/dwarf.md`
    /// §6.5), by description.
    Unsupported {
        /// The node.
        node: u32,
        /// What it is.
        what: &'static str,
    },
    /// An address names a global-variable expression that is not one of
    /// the unit's `globals`.
    GlobalNotInUnit {
        /// The `DIGlobalVariableExpression` node.
        expr: u32,
    },
    /// Two addresses name one global-variable expression.
    DuplicateGlobal {
        /// The `DIGlobalVariableExpression` node.
        expr: u32,
    },
    /// A symbol that is not a symbol of the object.
    BadSymbol {
        /// The symbol's index.
        symbol: u32,
    },
    /// A code span with a negative addend, or whose end overflows.
    BadSpan {
        /// The span's index in the unit's `code`.
        index: u32,
    },
    /// A member, inheritance, friend, static member or member function
    /// whose `scope` names a node that is not a type (§6.8).
    MemberOutsideType {
        /// The member.
        node: u32,
    },
    /// A type that would be infinite: a cycle through the `baseType` of
    /// typedefs, template aliases, qualifiers or arrays, or through the
    /// `scope` of types (§6.8).
    TypeCycle {
        /// A type on the cycle.
        node: u32,
    },
    /// A base type, enumeration, vector or bit field with no size
    /// (§6.8).
    MissingSize {
        /// The node.
        node: u32,
    },
    /// An encoded line program is for another architecture than the
    /// object or the program it is appended to.
    ArchMismatch,
    /// `hod_obj` refused a section.
    Object(hod_obj::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPrimary => write!(f, "line table without a directory 0 or a file 0"),
            Self::BadDirectory { file } => write!(f, "file {file}: no such directory"),
            Self::NulInName => write!(f, "a line-table name contains a NUL byte"),
            Self::BadFile { order, row } => write!(f, "sequence {order}, row {row}: no such file"),
            Self::RowOutOfOrder { order, row } => {
                write!(f, "sequence {order}, row {row}: offset below the previous row's")
            }
            Self::RowBeyondEnd { order, row } => {
                write!(f, "sequence {order}, row {row}: offset at or beyond the sequence's end")
            }
            Self::Unaligned { order, row } => write!(
                f,
                "sequence {order}, row {row}: not a multiple of the minimum instruction length"
            ),
            Self::BadFlags { order, row } => write!(f, "sequence {order}, row {row}: bad flags"),
            Self::DuplicateOrder { order } => write!(f, "two sequences with order {order}"),
            Self::TransitionOutOfOrder { index } => {
                write!(f, "transition {index}: offset below the previous transition's")
            }
            Self::TooLarge => write!(f, "a debug section of 4 GiB or more"),
            Self::NoUnit => write!(f, "no compile unit to write"),
            Self::NotACompileUnit { node } => write!(f, "debug node {node} is not a compile unit"),
            Self::BadReference { node, field } => {
                write!(f, "debug node {node}: `{field}` names no node of a kind it allows")
            }
            Self::Unsupported { node, what } => {
                write!(f, "debug node {node}: {what} is not written yet")
            }
            Self::GlobalNotInUnit { expr } => {
                write!(f, "debug node {expr}: an address for a global the unit does not list")
            }
            Self::DuplicateGlobal { expr } => {
                write!(f, "debug node {expr}: two addresses for one global")
            }
            Self::BadSymbol { symbol } => write!(f, "symbol {symbol} is not in the object"),
            Self::BadSpan { index } => {
                write!(f, "code span {index}: a negative or overflowing range")
            }
            Self::MemberOutsideType { node } => {
                write!(f, "debug node {node}: a member whose scope is not a type")
            }
            Self::TypeCycle { node } => {
                write!(f, "debug node {node}: a type that contains itself")
            }
            Self::MissingSize { node } => write!(f, "debug node {node}: a type without a size"),
            Self::ArchMismatch => write!(f, "a line program of another architecture"),
            Self::Object(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<hod_obj::Error> for Error {
    fn from(e: hod_obj::Error) -> Self {
        Self::Object(e)
    }
}
