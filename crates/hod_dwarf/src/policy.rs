//! The line policy: from a function's recorded location transitions to
//! line-table rows (`spec/dwarf.md` §5).

use crate::Error;
use crate::line::{Row, flags};

/// A source position: a file of the line-table header, a line (0 for "no
/// source line") and a column (0 for none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceLoc {
    /// An index of the header's files.
    pub file: u32,
    /// The line.
    pub line: u32,
    /// The column.
    pub column: u16,
}

/// From `offset` on, the instructions are at `loc`; `None` for an
/// instruction without a location (`LocId::NONE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    /// The code offset from the function's first byte.
    pub offset: u32,
    /// The location.
    pub loc: Option<SourceLoc>,
    /// The instruction is the first that emits code in a block other
    /// than the entry block (§5.3): without a location it is then a
    /// line-0 row, not an inherited one.
    pub block_start: bool,
    /// How the row's `is_stmt` is decided (§5.9).
    pub stmt: Stmt,
}

/// How a transition's row gets `is_stmt` (`spec/dwarf.md` §5.2, §5.9).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stmt {
    /// §5.2: when its line differs from the last line written.
    #[default]
    Line,
    /// §5.9: the row's code is a key instruction of a function with key
    /// instructions: `is_stmt` (on a line other than 0), and a row of its
    /// own even at the current row's location.
    Key,
    /// §5.9: another row of such a function: no `is_stmt`, except as the
    /// prologue end.
    NotKey,
}

impl Transition {
    /// A transition that is not at a block start.
    pub const fn at(offset: u32, loc: Option<SourceLoc>) -> Self {
        Self { offset, loc, block_start: false, stmt: Stmt::Line }
    }
}

/// One function's recorded transitions (`spec/codegen.md` §9.5) with
/// what the policy needs besides.
#[derive(Clone, Copy, Debug)]
pub struct FunctionLines<'a> {
    /// The subprogram's file and `scopeLine` (its `line` when
    /// `scopeLine` is 0); the column is not used.
    pub scope: SourceLoc,
    /// The function's code size.
    pub size: u32,
    /// The transitions, in code order.
    pub transitions: &'a [Transition],
    /// Whether the last transition is the epilogue row.
    pub epilogue: bool,
}

/// Appends the rows of one function's sequence to `out` (`spec/dwarf.md`
/// §5.1 to §5.4), with offsets from the function's first byte. Nothing
/// else in `out` is read or changed.
///
/// # Errors
/// [`Error::TransitionOutOfOrder`]; `out` is then as it was.
pub fn function_rows(f: &FunctionLines<'_>, out: &mut Vec<Row>) -> Result<(), Error> {
    let ts = f.transitions;
    if let Some(i) = ts.windows(2).position(|w| w[1].offset < w[0].offset) {
        return Err(Error::TransitionOutOfOrder { index: i as u32 + 1 });
    }
    if f.size == 0 {
        return Ok(());
    }
    let base = out.len();
    out.reserve(ts.len() + 1);
    let real = |t: &Transition| t.loc.is_some_and(|l| l.line != 0);
    // The last non-zero line written: (file, line).
    let mut last_line = None;
    let mut prologue_done = false;
    // §5.1 rule 1: the start row.
    if !ts.first().is_some_and(|t| t.offset == 0 && real(t)) {
        let s = f.scope;
        let stmt = if s.line != 0 { flags::IS_STMT } else { 0 };
        out.push(Row { offset: 0, file: s.file, line: s.line, column: 0, flags: stmt });
        if s.line != 0 {
            last_line = Some((s.file, s.line));
        }
    }
    let last = ts.len().wrapping_sub(1);
    for (i, t) in ts.iter().enumerate() {
        if t.offset >= f.size {
            break; // rule 5: no instruction there
        }
        let epilogue = f.epilogue && i == last;
        let current = out[base..].last().copied();
        let (file, line, column) = match (t.loc, current) {
            (Some(l), _) => (l.file, l.line, l.column),
            // Rule 2: inherit. The epilogue row and the start of a block
            // instead are a line-0 row (§5.3, §5.4), keeping the file and
            // column of the current row.
            (None, Some(c)) if epilogue || t.block_start => (c.file, 0, c.column),
            (None, _) => continue,
        };
        let mut row_flags = 0;
        if line != 0 {
            if !prologue_done {
                row_flags |= flags::PROLOGUE_END | flags::IS_STMT;
                prologue_done = true;
            }
            let changed = last_line != Some((file, line));
            if t.stmt == Stmt::Key || (t.stmt == Stmt::Line && changed) {
                row_flags |= flags::IS_STMT;
            }
            last_line = Some((file, line));
        }
        if epilogue {
            row_flags |= flags::EPILOGUE_BEGIN;
        }
        let row = Row { offset: t.offset, file, line, column, flags: row_flags };
        if let Some(c) = current {
            // Rule 3: no repeated line-0 row; rule 4: no row that says
            // nothing new.
            let new_flags = row_flags & !c.flags != 0;
            if line == 0 && c.line == 0 && !new_flags {
                continue;
            }
            let key = t.stmt == Stmt::Key && line != 0;
            if (c.file, c.line, c.column) == (file, line, column) && !new_flags && !key {
                continue;
            }
        }
        out.push(row);
    }
    Ok(())
}
