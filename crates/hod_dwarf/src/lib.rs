//! DWARF 5 debug information writer (`spec/dwarf.md`).
//!
//! Implemented: the line table (§4) and the line policy (§5). The DIE
//! writer (§6) is planned. Like `hod_eh`, this crate depends on `hod_obj`
//! only: it writes sections and relocations into an `Object` from plain
//! rows of its own types, which the caller fills from `hod_ir`'s debug
//! tables and `hod_cg`'s per-function records.
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
//!     Transition { offset: 8, loc: at(4, 7) },
//!     Transition { offset: 16, loc: at(5, 3) },
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

mod leb;
mod line;
mod policy;
mod target;

pub use leb::{write_sleb, write_uleb};
pub use line::{
    FileEntry, FileTable, LineHeader, LineSections, Row, Sequence, flags, write_debug_line,
};
pub use policy::{FunctionLines, SourceLoc, Transition, function_rows};
pub use target::{
    AARCH64, LINE_BASE, LINE_RANGE, LineTarget, OPCODE_BASE, STANDARD_OPCODE_LENGTHS, X86_64,
    for_arch,
};

/// Why a table was not written (`spec/dwarf.md` §4.6). The object is
/// unchanged.
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
            Self::TooLarge => write!(f, "line table of 4 GiB or more"),
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
