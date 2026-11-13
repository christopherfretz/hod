//! `spec/dwarf.md` §5.9 (WP-149): the line-table policy for functions
//! with key instructions. A `Stmt::Key` row is a statement and a row of
//! its own even at the current location; a `Stmt::NotKey` row is not a
//! statement, whatever its line, except as the prologue end.

use hod_dwarf::{FunctionLines, Row, SourceLoc, Stmt, Transition, flags, function_rows};

const STMT: u8 = flags::IS_STMT;
const PE: u8 = flags::PROLOGUE_END;

fn row(offset: u32, line: u32, column: u16, flags: u8) -> Row {
    Row { offset, file: 0, line, column, flags }
}

fn t(offset: u32, line: u32, column: u16, stmt: Stmt) -> Transition {
    Transition { offset, loc: Some(SourceLoc { file: 0, line, column }), block_start: false, stmt }
}

fn policy(transitions: &[Transition], size: u32) -> Vec<Row> {
    let f = FunctionLines {
        scope: SourceLoc { file: 0, line: 3, column: 0 },
        size,
        transitions,
        epilogue: false,
    };
    let mut out = Vec::new();
    function_rows(&f, &mut out).unwrap();
    out
}

/// A change of line on a `NotKey` row writes a row without `is_stmt`; a
/// `Key` row on the line already current is a statement row of its own.
#[test]
fn key_and_not_key_rows() {
    use Stmt::{Key, NotKey};
    let ts = [
        t(4, 4, 7, NotKey), // the prologue end: a statement anyway
        t(8, 5, 3, NotKey), // a new line, not key
        t(12, 5, 3, Key),   // the same location, key: a row of its own
        t(16, 6, 2, NotKey),
        t(20, 0, 0, Key), // line 0 is never a statement
    ];
    assert_eq!(
        policy(&ts, 24),
        [
            row(0, 3, 0, STMT),
            row(4, 4, 7, STMT | PE),
            row(8, 5, 3, 0),
            row(12, 5, 3, STMT),
            row(16, 6, 2, 0),
            row(20, 0, 0, 0),
        ]
    );
}

/// `Stmt::Line` (functions without key instructions) is unchanged: a
/// statement where the line changes, and no row that says nothing new.
#[test]
fn line_rows_are_unchanged() {
    let ts = [t(0, 4, 7, Stmt::Line), t(4, 4, 7, Stmt::Line), t(8, 5, 1, Stmt::Line)];
    assert_eq!(policy(&ts, 12), [row(0, 4, 7, STMT | PE), row(8, 5, 1, STMT)]);
}
