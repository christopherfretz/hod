//! The i686 permitted classes that are checked against the linked
//! programs' line tables (WP-352; `spec/harness.md` "Permitted
//! differences of sessions", `spec/dwarf.md` §5.7 `return-row` and
//! `entry-row`).
//!
//! - **return-row**: a stop after `finish` may differ in line and column
//!   when hod's stop is at a position (line, column) at which hod's
//!   program has a `call` whose return address is in a row of that same
//!   position: the stop is then the return address, still in the call
//!   statement's row (on i386 the instruction there releases the
//!   outgoing argument area, which `llc` reserves in the prologue).
//! - **entry-row**: lldb's breakpoint at `F + ?` of one program against
//!   one at `F` of the other, at the same
//!   position, when in both programs that position is F's entry row (its
//!   first `prologue_end` row): the two prologues differ (`llc` builds a
//!   leaf without a frame), the breakpoint is at the same statement.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::lines::{self, LineTools, Row};

/// What the classes read from one linked program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramRows {
    /// `(line, column)` of each `call` whose row and whose return
    /// address's row have that same position (line other than 0).
    pub return_rows: BTreeSet<(u32, u32)>,
    /// Each function's entry row: the position of its first
    /// `prologue_end` row.
    pub entries: BTreeMap<String, (u32, u32)>,
}

impl ProgramRows {
    /// Reads `program` with `tools`.
    ///
    /// # Errors
    /// A tool that fails.
    pub fn read(program: &Path, tools: &LineTools) -> Result<ProgramRows, String> {
        let table = lines::tool_output(&tools.dwarfdump, &["--debug-line"], program)?;
        let symbols = lines::tool_output(&tools.nm, &["-S", "--defined-only"], program)?;
        let code = lines::tool_output(&tools.objdump, &["-d", "--no-show-raw-insn"], program)?;
        Ok(ProgramRows::of(
            &lines::parse_line_tables(&table),
            &lines::parse_symbols(&symbols),
            &lines::parse_instructions(&code),
        ))
    }

    /// The facts of a program's rows, symbols and instructions.
    pub fn of(
        rows: &[Row],
        symbols: &[(u64, u64, String)],
        code: &BTreeMap<u64, String>,
    ) -> ProgramRows {
        let mut sorted: Vec<&Row> = rows.iter().collect();
        sorted.sort_by_key(|r| r.address);
        // The row whose range holds `address`: the last row at or before
        // it, unless that row ends a sequence.
        let covering = |address: u64| {
            let at = sorted.partition_point(|r| r.address <= address);
            let row = sorted.get(at.checked_sub(1)?)?;
            (!row.end_sequence).then_some((row.line, row.column))
        };
        let mut out = ProgramRows::default();
        for ((&address, mnemonic), (&next, _)) in code.iter().zip(code.iter().skip(1)) {
            if !mnemonic.starts_with("call") {
                continue;
            }
            if let (Some(call), Some(back)) = (covering(address), covering(next))
                && call == back
                && call.0 != 0
            {
                out.return_rows.insert(call);
            }
        }
        for (start, end, name) in symbols {
            let entry = sorted.iter().find(|r| {
                r.prologue_end && !r.end_sequence && r.address >= *start && r.address < *end
            });
            if let Some(r) = entry {
                out.entries.insert(name.clone(), (r.line, r.column));
            }
        }
        out
    }

    /// The lines of [`ProgramRows::return_rows`].
    fn return_lines(&self) -> BTreeSet<u32> {
        self.return_rows.iter().map(|&(line, _)| line).collect()
    }
}

/// The commands of a gdb script after which gdb prints where it
/// stopped.
fn gdb_stops(command: &str) -> bool {
    let verb = command.split_whitespace().next().unwrap_or("");
    matches!(
        verb,
        "run" | "r" | "continue" | "c" | "step" | "s" | "next" | "n" | "finish" | "fin" | "until"
    )
}

/// A gdb stop line: `[Breakpoint N, ][0x? in ]F (args) at file:LINE`;
/// gives the text before the line number (without `0x? in `) and the
/// line.
fn gdb_stop(line: &str) -> Option<(&str, u32)> {
    if line.starts_with('#') || line.contains(" from ") {
        return None;
    }
    let (before, number) = line.rsplit_once(':')?;
    let number = number.parse().ok()?;
    if !before.contains(") at ") {
        return None;
    }
    Some((before.strip_prefix("0x? in ").unwrap_or(before), number))
}

/// The `return-row` rule for gdb (module doc): two masked gdb
/// transcripts of as many lines whose differing pairs are each the stop
/// line of a stop made by a `finish` of `commands` (the k-th stop line
/// of `llc`'s transcript is the k-th stopping command's), with the same
/// text but for `0x? in ` and the line, hod's line being one of
/// `hod`'s return rows, and the source line after it.
pub fn gdb_only_return_rows(
    ours: &str,
    theirs: &str,
    commands: &[String],
    hod: &ProgramRows,
) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a.len() != b.len() || a == b {
        return false;
    }
    let stops: Vec<&String> = commands.iter().filter(|c| gdb_stops(c)).collect();
    let lines = hod.return_lines();
    let mut stop = 0;
    let mut source: Option<(u32, u32)> = None;
    for (x, y) in a.iter().zip(&b) {
        let theirs_stop = gdb_stop(y);
        if theirs_stop.is_some() {
            stop += 1;
        }
        let expected_source = source.take();
        if x == y {
            continue;
        }
        if let Some((la, lb)) = expected_source
            && x.starts_with(&format!("{la}\t"))
            && y.starts_with(&format!("{lb}\t"))
        {
            continue;
        }
        let (Some((pa, la)), Some((pb, lb))) = (gdb_stop(x), theirs_stop) else { return false };
        let finish = stops
            .get(stop - 1)
            .is_some_and(|c| matches!(c.split_whitespace().next(), Some("finish" | "fin")));
        if pa != pb || !finish || !lines.contains(&la) {
            return false;
        }
        source = Some((la, lb));
    }
    true
}

/// The `return-row` rule for lldb (module doc): two masked lldb
/// transcripts of as many lines whose differing pairs are each in a stop
/// whose `stop reason` is `step out`, from its `frame #0:` line (the
/// same text but for line and column, hod's position one of `hod`'s
/// return rows) to the next prompt (the source listing around it).
pub fn lldb_only_return_rows(ours: &str, theirs: &str, hod: &ProgramRows) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a.len() != b.len() || a == b {
        return false;
    }
    let mut step_out = false;
    let mut moved = false;
    for (x, y) in a.iter().zip(&b) {
        if y.contains("stop reason = ") {
            step_out = y.contains("stop reason = step out");
            moved = false;
        }
        if y.starts_with("(lldb) ") {
            moved = false;
        }
        if x == y {
            continue;
        }
        if !step_out {
            return false;
        }
        if moved {
            continue;
        }
        // `frame_position`'s text ends with `:LINE`; the file before it
        // is the same, the line may differ.
        let file =
            |text: &str, line: u32| text.strip_suffix(&format!(":{line}")).map(str::to_owned);
        match (super::permitted::frame_position(x), super::permitted::frame_position(y)) {
            (Some((pa, la, ca)), Some((pb, lb, _)))
                if file(pa, la).is_some()
                    && file(pa, la) == file(pb, lb)
                    && y.trim_start().starts_with("frame #0:")
                    && hod.return_rows.contains(&(la, ca)) =>
            {
                moved = true;
            }
            _ => return false,
        }
    }
    true
}

/// lldb's breakpoint line (`Breakpoint N: where = prog`, a backtick,
/// `F[ + ?] at file:L:C, address = 0x?`): the line without ` + ?`, F,
/// L and C.
fn breakpoint_where(line: &str) -> Option<(String, &str, u32, u32)> {
    let rest = line.strip_prefix("Breakpoint ")?;
    let (_, place) = rest.split_once(": where = ")?;
    let (_, function) = place.split_once('`')?;
    let function = function.split([' ', '(']).next()?;
    let position = place.split(" at ").nth(1)?.split(',').next()?;
    let mut parts = position.rsplit(':');
    let column = parts.next()?.parse().ok()?;
    let number = parts.next()?.parse().ok()?;
    Some((line.replacen(" + ?", "", 1), function, number, column))
}

/// The `entry-row` rule (module doc), alone or after `return-row`: two
/// masked lldb transcripts of as many lines whose differing pairs are
/// either breakpoint lines equal but for ` + ?` after the function,
/// at a position that is the function's entry row in both programs, or
/// pairs `rest` permits (it is given the two transcripts with those
/// breakpoint lines made equal).
pub fn lldb_entry_rows(
    ours: &str,
    theirs: &str,
    hod: &ProgramRows,
    llc: &ProgramRows,
    rest: impl Fn(&str, &str) -> bool,
) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a.len() != b.len() || a == b {
        return false;
    }
    let mut changed = false;
    let mut merged: Vec<&str> = Vec::with_capacity(a.len());
    for (x, y) in a.iter().zip(&b) {
        if x != y
            && let (Some((tx, fx, lx, cx)), Some((ty, fy, ly, cy))) =
                (breakpoint_where(x), breakpoint_where(y))
            && tx == ty
            && fx == fy
            && (lx, cx) == (ly, cy)
            && hod.entries.get(fx) == Some(&(lx, cx))
            && llc.entries.get(fy) == Some(&(ly, cy))
        {
            changed = true;
            merged.push(y);
        } else {
            merged.push(x);
        }
    }
    if !changed {
        return false;
    }
    let merged = merged.join("\n");
    merged.lines().eq(b.iter().copied()) || rest(&merged, theirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(address: u64, line: u32, column: u32, prologue_end: bool) -> Row {
        Row {
            address,
            file: "/t/a.c".to_owned(),
            line,
            column,
            is_stmt: true,
            prologue_end,
            end_sequence: false,
        }
    }

    fn program() -> ProgramRows {
        // f: entry row 3:5 at 0x13; a call at 0x20 in row 5:3 whose
        // return address 0x25 is still in it; a call at 0x30 in row 6:3
        // whose return address starts the row 7:1.
        let mut rows = vec![
            row(0x10, 2, 0, false),
            row(0x13, 3, 5, true),
            row(0x1c, 5, 3, false),
            row(0x2a, 6, 3, false),
            row(0x35, 7, 1, false),
            row(0x40, 7, 1, false),
        ];
        rows.last_mut().unwrap().end_sequence = true;
        let symbols = vec![(0x10, 0x40, "f".to_owned())];
        let code: BTreeMap<u64, String> =
            [(0x10, "pushl"), (0x20, "calll"), (0x25, "addl"), (0x30, "calll"), (0x35, "movl")]
                .into_iter()
                .map(|(a, m)| (a, m.to_owned()))
                .collect();
        ProgramRows::of(&rows, &symbols, &code)
    }

    #[test]
    fn reads_return_rows_and_entries() {
        let p = program();
        assert_eq!(p.return_rows, BTreeSet::from([(5, 3)]));
        assert_eq!(p.entries.get("f"), Some(&(3, 5)));
    }

    #[test]
    fn gdb_return_rows() {
        let p = program();
        let commands: Vec<String> =
            ["break g", "run", "finish", "continue"].iter().map(|s| (*s).to_owned()).collect();
        let llc = "Breakpoint 1, g () at a.c:9\n9\tx;\nf () at a.c:6\n6\t  y();\nend";
        let hod = "Breakpoint 1, g () at a.c:9\n9\tx;\n0x? in f () at a.c:5\n5\t  g();\nend";
        assert!(gdb_only_return_rows(hod, llc, &commands, &p));
        // Not a row of a call's return address.
        let other = "Breakpoint 1, g () at a.c:9\n9\tx;\n0x? in f () at a.c:6\n6\t  y();\nend";
        assert!(!gdb_only_return_rows(
            other,
            "Breakpoint 1, g () at a.c:9\n9\tx;\nf () at a.c:7\n7\tz;\nend",
            &commands,
            &p
        ));
        // Not after `finish`.
        let steps: Vec<String> =
            ["break g", "run", "next", "continue"].iter().map(|s| (*s).to_owned()).collect();
        assert!(!gdb_only_return_rows(hod, llc, &steps, &p));
    }

    #[test]
    fn lldb_return_rows() {
        let p = program();
        let head = "(lldb) finish\nProcess ? stopped\n* thread #1, stop reason = step out\n";
        let hod = format!("{head}    frame #0: 0x? prog`f at a.c:5:3\n-> 5 g();\n(lldb) next\nx");
        let llc = format!("{head}    frame #0: 0x? prog`f at a.c:6:3\n-> 6 y();\n(lldb) next\nx");
        assert!(lldb_only_return_rows(&hod, &llc, &p));
        let off = format!("{head}    frame #0: 0x? prog`f at a.c:6:3\n-> 6 y();\n(lldb) next\nx");
        let llc7 = format!("{head}    frame #0: 0x? prog`f at a.c:7:1\n-> 7 z;\n(lldb) next\nx");
        assert!(!lldb_only_return_rows(&off, &llc7, &p));
        // A difference after the next prompt is not permitted.
        let late = format!("{head}    frame #0: 0x? prog`f at a.c:5:3\n-> 5 g();\n(lldb) next\ny");
        assert!(!lldb_only_return_rows(&late, &llc, &p));
    }

    #[test]
    fn lldb_entry_rows_of_both_programs() {
        let p = program();
        let hod = "Breakpoint 2: where = prog`f + ? at a.c:3:5, address = 0x?\nok";
        let llc = "Breakpoint 2: where = prog`f at a.c:3:5, address = 0x?\nok";
        assert!(lldb_entry_rows(hod, llc, &p, &p, |_, _| false));
        let elsewhere = "Breakpoint 2: where = prog`f + ? at a.c:4:5, address = 0x?\nok";
        let llc4 = "Breakpoint 2: where = prog`f at a.c:4:5, address = 0x?\nok";
        assert!(!lldb_entry_rows(elsewhere, llc4, &p, &p, |_, _| false));
        assert!(!lldb_entry_rows(hod, &llc.replace("ok", "no"), &p, &p, |_, _| false));
    }
}
