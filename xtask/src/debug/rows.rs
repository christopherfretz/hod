//! The i686 permitted classes that are checked against the linked
//! programs' line tables (WP-352, WP-354; `spec/harness.md` "Permitted
//! differences of sessions", `spec/dwarf.md` §5.7 `return-line0` and
//! `entry-row`).
//!
//! - **return-line0** (gdb): a stop after `finish` at the return address
//!   of a call to the same callee from the same function in both
//!   programs, where `llc`'s row starting at that address is line 0 and
//!   hod's address starts a row. Part 1: the stop line may differ in
//!   `0x? in ` and the line, and the source line under it. Part 2, when
//!   the lines differ and the next stopping command is `step` or `next`:
//!   `llc`'s first row after the line-0 row has the file and line of
//!   hod's row at the return address, and `llc`'s report of that command
//!   is the location hod reported at the `finish`; hod's own report of
//!   that one command (it is one statement ahead) is not compared. On
//!   i386 PIC `llc`'s return address is its reload of the PIC base,
//!   which it gives no location.
//! - **entry-row**: lldb's breakpoint at `F + ?` of one program against
//!   one at `F` of the other, at the same
//!   position, when in both programs that position is F's entry row (its
//!   first `prologue_end` row): the two prologues differ (`llc` builds a
//!   leaf without a frame), the breakpoint is at the same statement.
//! - **local-label**: a value symbolized `0x? <.L…>` in `llc`'s session
//!   and `0x?` in hod's, when that `.L…` name is a local symbol of
//!   `llc`'s linked program and no symbol of hod's: on i386 `llc`'s
//!   object keeps a local `.L.str` for each `R_386_GOTOFF` into a
//!   merged-string section, hod's relocates against the section symbol
//!   (both resolve to the same string), and mold keeps
//!   local `.L` symbols that GNU ld and lld drop.

use std::collections::BTreeMap;
use std::path::Path;

use super::lines::{self, LineTools, Row};

/// What the classes read from one linked program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramRows {
    /// Each direct `call`'s return site, in address order.
    pub return_sites: Vec<ReturnSite>,
    /// Each function's entry row: the position of its first
    /// `prologue_end` row.
    pub entries: BTreeMap<String, (u32, u32)>,
    /// Every defined symbol's name (`nm --defined-only`), the local ones
    /// (a lower-case type letter) flagged.
    pub symbols: BTreeMap<String, bool>,
}

/// A direct `call` of a program, read at its return address (the next
/// instruction's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReturnSite {
    /// The function holding the call.
    pub caller: String,
    /// The called function (`call 0x? <callee>`).
    pub callee: String,
    /// The line of the row covering the call instruction.
    pub call_line: Option<u32>,
    /// `(file, line)` of the row starting at the return address (the
    /// last one there); `None` when the address is inside a row.
    pub starts: Option<(String, u32)>,
    /// `(file, line)` of the first row after the one starting at the
    /// return address, at a higher address.
    pub after: Option<(String, u32)>,
}

/// The direct calls of `llvm-objdump -d`: the call instruction's address
/// and its callee, from `call… 0x? <callee>` (not `<f+0x…>`).
pub fn parse_call_targets(text: &str) -> BTreeMap<u64, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((address, rest)) = line.split_once(':') else { continue };
        let Ok(address) = u64::from_str_radix(address.trim(), 16) else { continue };
        let mut words = rest.split_whitespace();
        if !words.next().is_some_and(|m| m.starts_with("call")) {
            continue;
        }
        let Some(target) = rest.split_once('<').and_then(|(_, t)| t.split_once('>')) else {
            continue;
        };
        if !target.0.contains('+') {
            out.insert(address, target.0.to_owned());
        }
    }
    out
}

/// The names `nm --defined-only` lists, each with whether it is local
/// (its type letter is lower case).
fn parse_defined(text: &str) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_), Some(kind), Some(name)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let local = kind.chars().all(|c| c.is_ascii_lowercase());
        // A name defined twice is local only if every definition is.
        let entry = out.entry(name.to_owned()).or_insert(local);
        *entry &= local;
    }
    out
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
        let defined = lines::tool_output(&tools.nm, &["--defined-only"], program)?;
        let mut out = ProgramRows::of(
            &lines::parse_line_tables(&table),
            &lines::parse_symbols(&symbols),
            &lines::parse_instructions(&code),
            &parse_call_targets(&code),
        );
        out.symbols = parse_defined(&defined);
        Ok(out)
    }

    /// The facts of a program's rows, symbols, instructions and direct
    /// calls (`parse_call_targets`).
    pub fn of(
        rows: &[Row],
        symbols: &[(u64, u64, String)],
        code: &BTreeMap<u64, String>,
        calls: &BTreeMap<u64, String>,
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
        let function = |address: u64| {
            symbols
                .iter()
                .find(|(start, end, _)| address >= *start && address < *end)
                .map(|(_, _, name)| name.clone())
        };
        for ((&address, mnemonic), (&next, _)) in code.iter().zip(code.iter().skip(1)) {
            let (true, Some(called), Some(holder)) =
                (mnemonic.starts_with("call"), calls.get(&address), function(address))
            else {
                continue;
            };
            // The last row starting at the return address, and the first
            // row at a higher address.
            let at = sorted.partition_point(|r| r.address <= next);
            let start = at
                .checked_sub(1)
                .and_then(|i| sorted.get(i))
                .filter(|r| r.address == next && !r.end_sequence);
            let after = start.and(sorted.get(at)).filter(|r| !r.end_sequence);
            out.return_sites.push(ReturnSite {
                caller: holder,
                callee: called.clone(),
                call_line: covering(address).map(|(line, _)| line),
                starts: start.map(|r| (r.file.clone(), r.line)),
                after: after.map(|r| (r.file.clone(), r.line)),
            });
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

    /// The return sites of the calls from `caller` to `callee`, in
    /// address order.
    fn sites(&self, from: &str, to: &str) -> Vec<&ReturnSite> {
        self.return_sites.iter().filter(|s| s.caller == from && s.callee == to).collect()
    }
}

/// A gdb command's verb.
fn verb(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or("")
}

/// The commands of a gdb script after which gdb prints where it
/// stopped.
fn gdb_stops(command: &str) -> bool {
    matches!(
        verb(command),
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

/// A gdb source line `N<tab>text`: N.
fn gdb_source(line: &str) -> Option<u32> {
    line.split_once('\t')?.0.parse().ok()
}

/// The function and the file of a stop line's text before its line
/// (`[Breakpoint N, ]F (args) at FILE`).
fn function_and_file(text: &str) -> Option<(&str, &str)> {
    let text = match text.split_once(", ") {
        Some((head, rest)) if head.starts_with("Breakpoint ") => rest,
        _ => text,
    };
    let function = text.split([' ', '(']).next().filter(|f| !f.is_empty())?;
    Some((function, text.rsplit_once(") at ")?.1))
}

/// The same source file: equal paths, or equal names (the two programs'
/// line tables may spell the directory differently).
fn same_file(a: &str, b: &str) -> bool {
    a == b || Path::new(a).file_name() == Path::new(b).file_name()
}

/// The `return-line0` rule for gdb (module doc; `spec/harness.md`
/// "Permitted differences of sessions"): `Some(two)` when two masked gdb
/// transcripts differ only as it permits, `two` telling whether part 2
/// (one step ahead) was applied; `None` otherwise.
///
/// Stops are counted on `llc`'s side by their source lines (each stop
/// prints one), so a script with `list` is refused. At the first line of
/// a differing pair both lines are stop lines of a `finish` (the k-th
/// stopping command, k the stops so far), equal but for `0x? in ` and
/// the line; the callee is the function of `llc`'s last stop line before
/// it, the caller and file are the stop's. Among the calls from caller to
/// callee (the same number in both programs, paired in address order)
/// one pair has: `llc`'s row starting at the return address of line 0,
/// its call in a row of `llc`'s reported line; hod's return address
/// starting a row of hod's reported line, in that file. The next pair of
/// lines are the two reported lines' source lines. Part 2, when the
/// lines differ and the next stopping command is `step` or `next`:
/// `llc`'s first row after its line-0 row has hod's row's file and line,
/// and `llc`'s report of that command (an optional stop line, then a
/// source line, after the lines both print for the `finish`) equals
/// hod's `finish` report (stop line without `0x? in `, source line);
/// hod's report of it (an optional stop line, then a source line) is
/// skipped. Everything else is compared line by line.
#[expect(
    clippy::many_single_char_names,
    reason = "two transcripts walked in step: lines a, b, x, y at i, j; sites h, l"
)]
pub fn gdb_return_line0(
    ours: &str,
    theirs: &str,
    commands: &[String],
    hod: &ProgramRows,
    llc: &ProgramRows,
) -> Option<bool> {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a == b || commands.iter().any(|c| matches!(verb(c), "list" | "l")) {
        return None;
    }
    let stops: Vec<&str> = commands.iter().filter(|c| gdb_stops(c)).map(|c| verb(c)).collect();
    let (mut i, mut j, mut done) = (0, 0, 0);
    let mut function: Option<&str> = None;
    let (mut used, mut ahead) = (false, false);
    // Part 2 pending: hod's `finish` report (stop text, line, source line).
    let mut pending: Option<(&str, u32, &str)> = None;
    loop {
        let (x, y) = match (a.get(i), b.get(j)) {
            (None, None) => break,
            (Some(x), Some(y)) => (*x, *y),
            _ => return None,
        };
        if let Some((text, line, source)) = pending
            && (gdb_stop(y).is_some() || gdb_source(y).is_some())
        {
            pending = None;
            // `llc`'s report of the stepping command: hod's `finish` one.
            let mut theirs_end = j;
            if let Some((t, l)) = gdb_stop(y) {
                if (t, l) != (text, line) {
                    return None;
                }
                function = function_and_file(t).map(|(f, _)| f);
                theirs_end += 1;
            }
            if b.get(theirs_end) != Some(&source) {
                return None;
            }
            // hod's report of it, one statement further: not compared.
            let mut ours_end = i + usize::from(gdb_stop(x).is_some());
            a.get(ours_end).and_then(|l| gdb_source(l))?;
            ours_end += 1;
            (i, j, done, ahead) = (ours_end, theirs_end + 1, done + 1, true);
            continue;
        }
        if x == y {
            if let Some((t, _)) = gdb_stop(y) {
                function = function_and_file(t).map(|(f, _)| f);
            }
            done += usize::from(gdb_source(y).is_some());
            (i, j) = (i + 1, j + 1);
            continue;
        }
        // Part 1.
        let (Some((pa, la)), Some((pb, lb))) = (gdb_stop(x), gdb_stop(y)) else { return None };
        if pa != pb || pending.is_some() || !matches!(stops.get(done), Some(&("finish" | "fin"))) {
            return None;
        }
        let (Some((holder, file)), Some(called)) = (function_and_file(pb), function) else {
            return None;
        };
        let (ours_sites, theirs_sites) = (hod.sites(holder, called), llc.sites(holder, called));
        if ours_sites.len() != theirs_sites.len() {
            return None;
        }
        let (h, l) = ours_sites.iter().zip(&theirs_sites).find(|(h, l)| {
            l.starts.as_ref().is_some_and(|(_, line)| *line == 0)
                && l.call_line == Some(lb)
                && h.starts.as_ref().is_some_and(|(f, line)| *line == la && same_file(f, file))
        })?;
        let (Some(sx), Some(sy)) = (a.get(i + 1), b.get(j + 1)) else { return None };
        if gdb_source(sx) != Some(la) || gdb_source(sy) != Some(lb) || (la == lb && sx != sy) {
            return None;
        }
        (used, function) = (true, Some(holder));
        (i, j, done) = (i + 2, j + 2, done + 1);
        if la != lb && matches!(stops.get(done), Some(&("next" | "n" | "step" | "s"))) {
            let (Some((hf, hl)), Some((lf, ll))) = (h.starts.as_ref(), l.after.as_ref()) else {
                return None;
            };
            if hl != ll || !same_file(hf, lf) {
                return None;
            }
            pending = Some((pa, la, *sx));
        }
    }
    (used && pending.is_none()).then_some(ahead)
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

/// The `entry-row` rule (module doc), alone or after `return-column`: two
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

/// `line` without each ` <NAME>` or ` <NAME+?>` whose NAME `drop`
/// accepts; `None` if nothing was dropped.
fn without_labels(line: &str, drop: impl Fn(&str) -> bool) -> Option<String> {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut dropped = false;
    while let Some(at) = rest.find(" <") {
        let after = &rest[at + 2..];
        let Some(close) = after.find('>') else { break };
        let name = after[..close].split('+').next().unwrap_or("");
        out.push_str(&rest[..at]);
        if drop(name) {
            dropped = true;
        } else {
            out.push_str(&rest[at..at + 3 + close]);
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    dropped.then_some(out)
}

/// The `local-label` rule (module doc): two masked transcripts of as
/// many lines whose differing pairs are each `llc`'s line equal to hod's
/// once every ` <.L…>` symbolization is dropped from it whose name is a
/// LOCAL symbol of `llc`'s program and no symbol of hod's.
pub fn only_local_labels(ours: &str, theirs: &str, hod: &ProgramRows, llc: &ProgramRows) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a.len() != b.len() || a == b {
        return false;
    }
    let label = |name: &str| {
        name.starts_with(".L")
            && llc.symbols.get(name) == Some(&true)
            && !hod.symbols.contains_key(name)
    };
    a.iter().zip(&b).all(|(x, y)| x == y || without_labels(y, label).as_deref() == Some(*x))
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
        // f: entry row 3:5 at 0x13; a call of g at 0x20 in row 5:3 whose
        // return address 0x25 is inside it; a call of g at 0x30 in row
        // 6:3 whose return address starts the row 0:3, then 7:1; an
        // indirect call at 0x37.
        let mut rows = vec![
            row(0x10, 2, 0, false),
            row(0x13, 3, 5, true),
            row(0x1c, 5, 3, false),
            row(0x2a, 6, 3, false),
            row(0x35, 0, 3, false),
            row(0x37, 7, 1, false),
            row(0x40, 7, 1, false),
        ];
        rows.last_mut().unwrap().end_sequence = true;
        let symbols = vec![(0x10, 0x40, "f".to_owned())];
        let text = "  20:  calll 0x50 <g>\n  30:  calll 0x50 <g>\n  37:  calll *%eax\n";
        let code: BTreeMap<u64, String> = [
            (0x10, "pushl"),
            (0x20, "calll"),
            (0x25, "addl"),
            (0x30, "calll"),
            (0x35, "movl"),
            (0x37, "calll"),
            (0x39, "retl"),
        ]
        .into_iter()
        .map(|(a, m)| (a, m.to_owned()))
        .collect();
        ProgramRows::of(&rows, &symbols, &code, &parse_call_targets(text))
    }

    #[test]
    fn reads_return_sites_and_entries() {
        let p = program();
        let site = |line, starts: Option<u32>, after: Option<u32>| ReturnSite {
            caller: "f".to_owned(),
            callee: "g".to_owned(),
            call_line: Some(line),
            starts: starts.map(|l| ("/t/a.c".to_owned(), l)),
            after: after.map(|l| ("/t/a.c".to_owned(), l)),
        };
        assert_eq!(p.return_sites, vec![site(5, None, None), site(6, Some(0), Some(7))]);
        assert_eq!(p.entries.get("f"), Some(&(3, 5)));
    }

    /// One return site of `main` calling `callee`.
    fn site(callee: &str, call: u32, starts: Option<u32>, after: Option<u32>) -> ReturnSite {
        ReturnSite {
            caller: "main".to_owned(),
            callee: callee.to_owned(),
            call_line: Some(call),
            starts: starts.map(|l| ("/src/a.c".to_owned(), l)),
            after: after.map(|l| ("/src/a.c".to_owned(), l)),
        }
    }

    /// `key_call.c`'s two programs on i386 PIC: `llc`'s return addresses
    /// in line-0 rows, hod's starting rows (after `p` on line 16 a row
    /// of 16, after `c` the next statement's row).
    fn key_call() -> (ProgramRows, ProgramRows) {
        let llc = ProgramRows {
            return_sites: vec![
                site("p", 16, Some(0), Some(16)),
                site("c", 16, Some(0), Some(17)),
                site("p", 17, Some(0), Some(17)),
                site("c", 17, Some(0), Some(18)),
            ],
            ..ProgramRows::default()
        };
        let hod = ProgramRows {
            return_sites: vec![
                site("p", 16, Some(16), None),
                site("c", 16, Some(17), None),
                site("p", 17, None, None),
                site("c", 17, Some(18), None),
            ],
            ..ProgramRows::default()
        };
        (hod, llc)
    }

    fn script(stepping: &str) -> Vec<String> {
        ["break a.c:16", "run", "step", "finish", "step", "finish", stepping, "continue"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    const LLC: &str = "Breakpoint 1, main () at a.c:16\n16\t  c(p(x));\n\
        p (x=4) at a.c:10\n10\tint p(int x) { return x * 3; }\n\
        0x? in main () at a.c:16\n16\t  c(p(x));\nValue returned is $1 = 12\n\
        c (v=12) at a.c:12\n12\tvoid c(int v) { seen = v; }\n\
        0x? in main () at a.c:16\n16\t  c(p(x));\n17\t  c(p(x) + 1);\n\
        [Inferior 1 (process ?) exited normally]";

    /// `LLC` with hod's stops: the first `finish` without `0x? in `, the
    /// second at line 17, and the `next` at line 18.
    fn hod_session() -> String {
        LLC.replacen("0x? in main () at a.c:16", "main () at a.c:16", 1).replace(
            "0x? in main () at a.c:16\n16\t  c(p(x));\n17\t  c(p(x) + 1);",
            "main () at a.c:17\n17\t  c(p(x) + 1);\n18\t  return 0;",
        )
    }

    #[test]
    fn return_line0_the_stop_alone() {
        let (hod, llc) = key_call();
        // Only the first `finish` differs, in `0x? in `: part 1.
        let ours = LLC.replacen("0x? in main () at a.c:16", "main () at a.c:16", 1);
        assert_eq!(gdb_return_line0(&ours, LLC, &script("next"), &hod, &llc), Some(false));
        // The second `finish` a statement ahead, then `continue`: part 1.
        let theirs = LLC.replace("17\t  c(p(x) + 1);\n", "");
        let ours = hod_session().replace("18\t  return 0;\n", "");
        assert_eq!(gdb_return_line0(&ours, &theirs, &script("continue"), &hod, &llc), Some(false));
        assert_eq!(gdb_return_line0(LLC, LLC, &script("next"), &hod, &llc), None);
    }

    #[test]
    fn return_line0_one_step_ahead() {
        let (hod, llc) = key_call();
        let ours = hod_session();
        assert_eq!(gdb_return_line0(&ours, LLC, &script("next"), &hod, &llc), Some(true));
        assert_eq!(gdb_return_line0(&ours, LLC, &script("step"), &hod, &llc), Some(true));
    }

    #[test]
    fn return_line0_refused() {
        let (hod, llc) = key_call();
        let ours = hod_session();
        let commands = script("next");
        let with = |f: &dyn Fn(&mut ProgramRows, &mut ProgramRows)| {
            let (mut h, mut l) = key_call();
            f(&mut h, &mut l);
            gdb_return_line0(&ours, LLC, &commands, &h, &l)
        };
        // `llc`'s row at the return address is not line 0.
        assert_eq!(with(&|_, l| l.return_sites[1].starts = Some(("/src/a.c".into(), 16))), None);
        // hod's return address is inside a row.
        assert_eq!(with(&|h, _| h.return_sites[1].starts = None), None);
        // Another callee in hod's program.
        assert_eq!(with(&|h, _| h.return_sites[1].callee = "d".to_owned()), None);
        // `llc`'s next row is another line than hod's row.
        assert_eq!(with(&|_, l| l.return_sites[1].after = Some(("/src/a.c".into(), 19))), None);
        // `llc`'s report of the `next` is not where hod's `finish` was.
        let theirs = LLC.replace("17\t  c(p(x) + 1);", "17\t  c(p(x) + 2);");
        assert_eq!(gdb_return_line0(&ours, &theirs, &commands, &hod, &llc), None);
        // A later command still differs.
        let late = ours.replace("exited normally", "exited with code 01");
        assert_eq!(gdb_return_line0(&late, LLC, &commands, &hod, &llc), None);
        // The command after the `finish` is not `next` or `step`.
        assert_eq!(gdb_return_line0(&ours, LLC, &script("continue"), &hod, &llc), None);
        // Not after a `finish`.
        let steps: Vec<String> = script("next")
            .into_iter()
            .map(|c| if c == "finish" { "next".to_owned() } else { c })
            .collect();
        assert_eq!(gdb_return_line0(&ours, LLC, &steps, &hod, &llc), None);
    }

    #[test]
    fn local_labels_of_llc_only() {
        let nm = "000009f3 r .L.str\n000009e0 r .LC0\n00001000 T main\n";
        let llc = ProgramRows { symbols: parse_defined(nm), ..ProgramRows::default() };
        let hod = ProgramRows {
            symbols: parse_defined("000009e8 r .LC0\n00001000 T main\n"),
            ..ProgramRows::default()
        };
        assert_eq!(llc.symbols.get(".L.str"), Some(&true));
        assert_eq!(llc.symbols.get("main"), Some(&false));
        let ours = "f = {code = 50, what = 0x? \"too big\"}\nend";
        let theirs = "f = {code = 50, what = 0x? <.L.str> \"too big\"}\nend";
        assert!(only_local_labels(ours, theirs, &hod, &llc));
        assert!(only_local_labels(ours, &theirs.replace(".str>", ".str+?>"), &hod, &llc));
        // A label hod's program has too, a global one, another text.
        assert!(!only_local_labels(ours, &theirs.replace(".L.str", ".LC0"), &hod, &llc));
        assert!(!only_local_labels(ours, &theirs.replace(".L.str", "main"), &hod, &llc));
        assert!(!only_local_labels(ours, &theirs.replace("big", "small"), &hod, &llc));
        assert!(!only_local_labels(ours, ours, &hod, &llc));
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
