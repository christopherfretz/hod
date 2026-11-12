//! The `(function, line)` comparison of hod's program with `llc`'s.
//! Contract: `spec/harness.md` "debug", "Lines"; `spec/dwarf.md` §8.4.
//!
//! A pair is a function (the symbol whose range holds the address) and a
//! source position (file and line) where a debugger can stop: an
//! `is_stmt` row with a line other than 0 that covers at least one byte
//! of code (the next row of its sequence is at a higher address). The two
//! programs are built from the same IR, so the same pairs are expected;
//! each pair only one side has is classified under `spec/dwarf.md` §5.7.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::irdiff::process::{self, End, Limits};

/// A function and a source position.
pub type Pair = (String, String, u32);

/// Why a pair is on one side only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// §5.7 P1: `llc`'s row covers only unconditional branches, which hod
    /// falls through.
    P1,
    /// §5.7 P3: hod's prologue row at the function's first line, where
    /// `llc`'s function has no frame and no row there.
    P3,
    /// Not a permitted difference: a finding.
    Unclassified,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Self::P1 => "P1",
            Self::P3 => "P3",
            Self::Unclassified => "unclassified",
        }
    }
}

/// The comparison of one pair of programs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinesReport {
    /// Pairs both programs have.
    pub same: usize,
    /// Pairs only hod's program has, with their class.
    pub hod_only: Vec<(Pair, Class)>,
    /// Pairs only `llc`'s program has, with their class.
    pub llc_only: Vec<(Pair, Class)>,
    /// The rows with columns, function by function.
    pub columns: ColumnsReport,
}

impl LinesReport {
    /// The differences that are not permitted.
    pub fn unclassified(&self) -> usize {
        self.hod_only
            .iter()
            .chain(&self.llc_only)
            .filter(|(_, c)| *c == Class::Unclassified)
            .count()
    }

    /// Adds another report's counts and differences.
    pub fn add(&mut self, other: &Self) {
        self.same += other.same;
        self.hod_only.extend(other.hod_only.iter().cloned());
        self.llc_only.extend(other.llc_only.iter().cloned());
        self.columns.add(&other.columns);
    }

    /// One line: `same N, hod only N (P3 N, unclassified N), llc only ...`.
    pub fn summary(&self) -> String {
        let side = |list: &[(Pair, Class)]| {
            let count = |c| list.iter().filter(|(_, k)| *k == c).count();
            format!(
                "{} (P1 {}, P3 {}, unclassified {})",
                list.len(),
                count(Class::P1),
                count(Class::P3),
                count(Class::Unclassified)
            )
        };
        format!(
            "same {}, hod only {}, llc only {}",
            self.same,
            side(&self.hod_only),
            side(&self.llc_only)
        )
    }
}

/// The tools the comparison reads programs with.
#[derive(Clone, Debug)]
pub struct LineTools {
    pub dwarfdump: std::path::PathBuf,
    pub nm: std::path::PathBuf,
    pub objdump: std::path::PathBuf,
}

const LIMITS: Limits = Limits { time: Duration::from_mins(2), memory_kib: 4 << 20 };

fn tool_output(tool: &Path, args: &[&str], program: &Path) -> Result<String, String> {
    let configure = |c: &mut Command| {
        c.args(args).arg(program).env("LC_ALL", "C");
    };
    let output = process::run(tool, configure, b"", LIMITS)?;
    if output.end != End::Exit(0) || output.truncated {
        return Err(format!(
            "'{} {}' on '{}' failed: {}",
            tool.display(),
            args.join(" "),
            program.display(),
            output.first_error_line()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// One row of a line table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub address: u64,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub is_stmt: bool,
    pub end_sequence: bool,
}

/// The rows of `llvm-dwarfdump --debug-line`, each table's file indices
/// resolved to paths (`<include directory>/<name>`, or the name when it is
/// absolute).
pub fn parse_line_tables(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut dirs: BTreeMap<u32, String> = BTreeMap::new();
    let mut files: BTreeMap<u32, (String, u32)> = BTreeMap::new();
    let mut current: Option<u32> = None;
    let quoted = |s: &str| s.split('"').nth(1).unwrap_or("").to_owned();
    let index = |s: &str| {
        s.split('[').nth(1).and_then(|r| r.split(']').next()).and_then(|n| n.trim().parse().ok())
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("debug_line[") {
            dirs.clear();
            files.clear();
            current = None;
        } else if trimmed.starts_with("include_directories[") {
            if let Some(i) = index(trimmed) {
                dirs.insert(i, quoted(trimmed));
            }
        } else if trimmed.starts_with("file_names[") {
            current = index(trimmed);
        } else if let (Some(i), Some(name)) = (current, trimmed.strip_prefix("name:")) {
            files.entry(i).or_default().0 = quoted(name);
        } else if let (Some(i), Some(dir)) = (current, trimmed.strip_prefix("dir_index:")) {
            files.entry(i).or_default().1 = dir.trim().parse().unwrap_or(0);
        } else if trimmed.starts_with("0x") {
            let words: Vec<&str> = trimmed.split_whitespace().collect();
            let number = |i: usize| words.get(i).and_then(|w| w.parse::<u64>().ok());
            let (Ok(address), Some(line), Some(column), Some(file)) =
                (u64::from_str_radix(&words[0][2..], 16), number(1), number(2), number(3))
            else {
                continue;
            };
            let file = files.get(&(file as u32)).map_or_else(String::new, |(name, dir)| match dirs
                .get(dir)
            {
                Some(d) if !name.starts_with('/') => format!("{d}/{name}"),
                _ => name.clone(),
            });
            rows.push(Row {
                address,
                file,
                line: line as u32,
                column: column as u32,
                is_stmt: words.contains(&"is_stmt"),
                end_sequence: words.contains(&"end_sequence"),
            });
        }
    }
    rows
}

/// Function symbols from `llvm-nm -S --defined-only`: start, end, name,
/// sorted by start.
pub fn parse_symbols(text: &str) -> Vec<(u64, u64, String)> {
    let mut symbols: Vec<(u64, u64, String)> = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let start = u64::from_str_radix(words.next()?, 16).ok()?;
            let size = u64::from_str_radix(words.next()?, 16).ok()?;
            let kind = words.next()?;
            let name = words.next()?;
            (size > 0 && matches!(kind, "T" | "t" | "W" | "w"))
                .then(|| (start, start + size, name.to_owned()))
        })
        .collect();
    symbols.sort();
    symbols
}

/// Instructions from `llvm-objdump -d --no-show-raw-insn`: address and
/// mnemonic.
pub fn parse_instructions(text: &str) -> BTreeMap<u64, String> {
    let mut code = BTreeMap::new();
    for line in text.lines() {
        let Some((address, rest)) = line.split_once(':') else { continue };
        let address = address.trim();
        if address.is_empty() || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let (Ok(address), Some(mnemonic)) =
            (u64::from_str_radix(address, 16), rest.split_whitespace().next())
        else {
            continue;
        };
        code.insert(address, mnemonic.to_owned());
    }
    code
}

/// A program's pairs: for each, the address ranges of its rows; and the
/// line of each function's first row.
#[derive(Debug, Default)]
struct Pairs {
    ranges: BTreeMap<Pair, Vec<(u64, u64)>>,
    first_line: BTreeMap<String, (String, u32)>,
}

fn function_at(symbols: &[(u64, u64, String)], address: u64) -> Option<&str> {
    let i = symbols.partition_point(|(start, _, _)| *start <= address);
    let (start, end, name) = symbols.get(i.checked_sub(1)?)?;
    (*start <= address && address < *end).then_some(name.as_str())
}

fn pairs_of(rows: &[Row], symbols: &[(u64, u64, String)]) -> Pairs {
    let mut pairs = Pairs::default();
    for (row, next) in rows.iter().zip(rows.iter().skip(1)) {
        if row.end_sequence || next.address <= row.address {
            continue;
        }
        let Some(function) = function_at(symbols, row.address) else { continue };
        if row.line != 0 {
            pairs
                .first_line
                .entry(function.to_owned())
                .or_insert_with(|| (row.file.clone(), row.line));
        }
        if row.is_stmt && row.line != 0 {
            let key = (function.to_owned(), row.file.clone(), row.line);
            pairs.ranges.entry(key).or_default().push((row.address, next.address));
        }
    }
    pairs
}

fn is_unconditional_branch(mnemonic: &str) -> bool {
    matches!(mnemonic, "b" | "jmp")
}

/// Compares the pairs of two programs built from the same IR, and their
/// rows with columns ([`compare_function_rows`]).
pub fn compare(hod: &Path, llc: &Path, tools: &LineTools) -> Result<LinesReport, String> {
    type Read = (Pairs, BTreeMap<String, FunctionRows>);
    let read = |program: &Path| -> Result<Read, String> {
        let lines = tool_output(&tools.dwarfdump, &["--debug-line"], program)?;
        let symbols = tool_output(&tools.nm, &["-S", "--defined-only"], program)?;
        let (rows, symbols) = (parse_line_tables(&lines), parse_symbols(&symbols));
        Ok((pairs_of(&rows, &symbols), function_rows(&rows, &symbols)))
    };
    let ((ours, our_rows), (theirs, their_rows)) = (read(hod)?, read(llc)?);
    let code =
        parse_instructions(&tool_output(&tools.objdump, &["-d", "--no-show-raw-insn"], llc)?);
    let mut report = LinesReport::default();
    let keys: BTreeSet<&Pair> = ours.ranges.keys().chain(theirs.ranges.keys()).collect();
    for key in keys {
        match (ours.ranges.get(key), theirs.ranges.get(key)) {
            (Some(_), Some(_)) => report.same += 1,
            (Some(_), None) => {
                let first = ours.first_line.get(&key.0);
                let p3 = first.is_some_and(|(file, line)| *file == key.1 && *line == key.2);
                let class = if p3 { Class::P3 } else { Class::Unclassified };
                report.hod_only.push((key.clone(), class));
            }
            (None, Some(ranges)) => {
                let only_branches = ranges.iter().all(|&(start, end)| {
                    let mut inside = code.range(start..end).peekable();
                    inside.peek().is_some() && inside.all(|(_, m)| is_unconditional_branch(m))
                });
                let class = if only_branches { Class::P1 } else { Class::Unclassified };
                report.llc_only.push((key.clone(), class));
            }
            (None, None) => unreachable!("the key came from one of the maps"),
        }
    }
    report.columns = compare_function_rows(&our_rows, &their_rows, &code);
    Ok(report)
}

// ------------------------------------------------------------ columns ----

/// A source position with its column.
pub type Position = (String, u32, u32);

/// One function's rows for the column comparison (`spec/harness.md`
/// "debug", "Columns"): the rows that cover at least one byte, in address
/// order, without their addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionRows {
    /// `(file, line, column, is_stmt)` of each row, line 0 included; a
    /// row equal to the one before in all four is not repeated.
    pub sequence: Vec<(String, u32, u32, bool)>,
    /// The positions with a line other than 0, and for each the address
    /// ranges of its rows.
    pub positions: BTreeMap<Position, Vec<(u64, u64)>>,
}

/// Each function's rows.
pub fn function_rows(
    rows: &[Row],
    symbols: &[(u64, u64, String)],
) -> BTreeMap<String, FunctionRows> {
    let mut out: BTreeMap<String, FunctionRows> = BTreeMap::new();
    for (row, next) in rows.iter().zip(rows.iter().skip(1)) {
        if row.end_sequence || next.address <= row.address {
            continue;
        }
        let Some(function) = function_at(symbols, row.address) else { continue };
        let entry = out.entry(function.to_owned()).or_default();
        // A row that repeats the one before but for its flags (hod's
        // `epilogue_begin` row) is no new step.
        let step = (row.file.clone(), row.line, row.column, row.is_stmt);
        if entry.sequence.last() != Some(&step) {
            entry.sequence.push(step);
        }
        if row.line != 0 {
            let key = (row.file.clone(), row.line, row.column);
            entry.positions.entry(key).or_default().push((row.address, next.address));
        }
    }
    out
}

/// Why a position is in one function's rows on one side only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColumnClass {
    /// §5.7 P1: `llc`'s rows of the position cover only unconditional
    /// branches.
    P1,
    /// §5.7 P3: hod's start row (column 0) at the function's first line,
    /// which `llc`'s function without a frame lacks.
    P3,
    /// Not a permitted difference.
    Unclassified,
}

impl ColumnClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::P1 => "P1",
            Self::P3 => "P3",
            Self::Unclassified => "unclassified",
        }
    }
}

/// The column comparison of programs built from the same IR, per function
/// both programs have.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnsReport {
    /// Functions both programs have rows for.
    pub functions: usize,
    /// Of those, with exactly the same set of positions.
    pub same_set: usize,
    /// With the same set but for permitted differences.
    pub same_permitted: usize,
    /// With the same sequence of `(file, line, column, is_stmt)` rows.
    pub same_sequence: usize,
    /// Positions on one side only: function, position, class.
    pub hod_only: Vec<(String, Position, ColumnClass)>,
    pub llc_only: Vec<(String, Position, ColumnClass)>,
}

impl ColumnsReport {
    /// Adds another report's counts and differences.
    pub fn add(&mut self, other: &Self) {
        self.functions += other.functions;
        self.same_set += other.same_set;
        self.same_permitted += other.same_permitted;
        self.same_sequence += other.same_sequence;
        self.hod_only.extend(other.hod_only.iter().cloned());
        self.llc_only.extend(other.llc_only.iter().cloned());
    }

    /// The differences that are not permitted.
    pub fn unclassified(&self) -> usize {
        self.hod_only
            .iter()
            .chain(&self.llc_only)
            .filter(|(_, _, c)| *c == ColumnClass::Unclassified)
            .count()
    }

    /// One line of counts.
    pub fn summary(&self) -> String {
        let side = |list: &[(String, Position, ColumnClass)]| {
            let count = |c| list.iter().filter(|(_, _, k)| *k == c).count();
            format!(
                "{} (P1 {}, P3 {}, unclassified {})",
                list.len(),
                count(ColumnClass::P1),
                count(ColumnClass::P3),
                count(ColumnClass::Unclassified)
            )
        };
        format!(
            "functions {}, same (line, column) set {}, same but permitted {}, same sequence {}; \
             positions hod only {}, llc only {}",
            self.functions,
            self.same_set,
            self.same_permitted,
            self.same_sequence,
            side(&self.hod_only),
            side(&self.llc_only)
        )
    }
}

/// Compares the rows of two programs function by function, given each
/// one's rows and `llc`'s instructions (for P1).
pub fn compare_function_rows(
    ours: &BTreeMap<String, FunctionRows>,
    theirs: &BTreeMap<String, FunctionRows>,
    llc_code: &BTreeMap<u64, String>,
) -> ColumnsReport {
    let mut report = ColumnsReport::default();
    for (function, hod) in ours {
        let Some(llc) = theirs.get(function) else { continue };
        report.functions += 1;
        let mut permitted = true;
        let first = hod.sequence.iter().find(|(_, line, _, _)| *line != 0);
        for key in hod.positions.keys().filter(|k| !llc.positions.contains_key(*k)) {
            let p3 = key.2 == 0
                && first.is_some_and(|(file, line, column, _)| {
                    *file == key.0 && *line == key.1 && *column == 0
                });
            let class = if p3 { ColumnClass::P3 } else { ColumnClass::Unclassified };
            permitted &= class != ColumnClass::Unclassified;
            report.hod_only.push((function.clone(), key.clone(), class));
        }
        for (key, ranges) in llc.positions.iter().filter(|(k, _)| !hod.positions.contains_key(*k)) {
            let only_branches = ranges.iter().all(|&(start, end)| {
                let mut inside = llc_code.range(start..end).peekable();
                inside.peek().is_some() && inside.all(|(_, m)| is_unconditional_branch(m))
            });
            let class = if only_branches { ColumnClass::P1 } else { ColumnClass::Unclassified };
            permitted &= class != ColumnClass::Unclassified;
            report.llc_only.push((function.clone(), key.clone(), class));
        }
        let same = hod.positions.keys().eq(llc.positions.keys());
        report.same_set += usize::from(same);
        report.same_permitted += usize::from(!same && permitted);
        report.same_sequence += usize::from(hod.sequence == llc.sequence);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = "\
debug_line[0x00000000]
include_directories[  0] = \"/src\"
file_names[  0]:
           name: \"step.c\"
      dir_index: 0
file_names[  1]:
           name: \"/usr/include/x.h\"
      dir_index: 0

Address            Line   Column File   ISA Discriminator OpIndex Flags
------------------ ------ ------ ------ --- ------------- ------- -------------
0x0000000000000100      8      0      0   0             0       0  is_stmt
0x0000000000000110      9      7      0   0             0       0  is_stmt prologue_end
0x0000000000000114      9      9      1   0             0       0
0x0000000000000118     10      5      0   0             0       0  is_stmt
0x0000000000000118     11      5      0   0             0       0  is_stmt
0x0000000000000120      0      5      0   0             0       0  is_stmt
0x0000000000000124     12      1      0   0             0       0  is_stmt
0x0000000000000128     12      1      0   0             0       0  end_sequence
";

    // spec/harness.md "debug", "Lines": rows, files and the sequences.
    #[test]
    fn reads_line_tables() {
        let rows = parse_line_tables(DUMP);
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0].file, "/src/step.c");
        assert_eq!(rows[2].file, "/usr/include/x.h");
        assert!(rows[1].is_stmt && !rows[2].is_stmt && rows[7].end_sequence);
        let symbols = parse_symbols(
            "0000000000000100 0000000000000028 T fact\n0000000000000200 0000000000000000 t empty\n",
        );
        assert_eq!(symbols, vec![(0x100, 0x128, "fact".to_owned())]);
        let pairs = pairs_of(&rows, &symbols);
        let lines: Vec<u32> = pairs.ranges.keys().map(|(_, _, l)| *l).collect();
        // Line 10 covers no byte; line 0 is no pair; the end is no pair.
        assert_eq!(lines, vec![8, 9, 11, 12]);
        assert_eq!(pairs.first_line["fact"], ("/src/step.c".to_owned(), 8));
    }

    #[test]
    fn reads_instructions() {
        let code = parse_instructions(
            "00000000000007a8 <fact>:\n     7a8:      \tsub\tsp, sp, #0x20\n     7d0:      \tb\t0x7f8 <fact+0x50>\n",
        );
        assert_eq!(code.get(&0x7a8).map(String::as_str), Some("sub"));
        assert_eq!(code.get(&0x7d0).map(String::as_str), Some("b"));
        assert_eq!(code.len(), 2);
    }

    #[test]
    fn summary_counts_classes() {
        let pair = || ("f".to_owned(), "a.c".to_owned(), 1);
        let report = LinesReport {
            same: 3,
            hod_only: vec![(pair(), Class::P3)],
            llc_only: vec![(pair(), Class::P1), (pair(), Class::Unclassified)],
            ..LinesReport::default()
        };
        assert_eq!(report.unclassified(), 1);
        assert_eq!(
            report.summary(),
            "same 3, hod only 1 (P1 0, P3 1, unclassified 0), llc only 2 (P1 1, P3 0, unclassified 1)"
        );
    }

    // spec/harness.md "debug", "Columns": positions, sequences, classes.
    #[test]
    fn compares_columns() {
        let symbols = parse_symbols(
            "0000000000000100 0000000000000028 T fact\n0000000000000200 0000000000000010 T g\n",
        );
        let ours = function_rows(&parse_line_tables(DUMP), &symbols);
        let fact = &ours["fact"];
        // 10:5 covers no byte; the line-0 row is in the sequence only.
        assert_eq!(fact.sequence.len(), 6);
        let keys: Vec<(u32, u32)> = fact.positions.keys().map(|(_, l, c)| (*l, *c)).collect();
        // By file first: x.h after step.c.
        assert_eq!(keys, vec![(8, 0), (9, 7), (11, 5), (12, 1), (9, 9)]);
        let mut theirs = ours.clone();
        let report = compare_function_rows(&ours, &theirs, &BTreeMap::new());
        assert_eq!((report.functions, report.same_set, report.same_sequence), (1, 1, 1));
        // llc without hod's start row: P3; a column of its own: unclassified.
        let llc = theirs.get_mut("fact").unwrap();
        llc.positions.remove(&("/src/step.c".to_owned(), 8, 0));
        llc.positions.insert(("/src/step.c".to_owned(), 9, 8), vec![(0x110, 0x114)]);
        let report = compare_function_rows(&ours, &theirs, &BTreeMap::new());
        assert_eq!(report.same_set, 0);
        assert_eq!(report.hod_only[0].2, ColumnClass::P3);
        assert_eq!(report.llc_only[0].2, ColumnClass::Unclassified);
        assert_eq!(report.unclassified(), 1);
        assert_eq!(report.same_sequence, 1, "the sequences were not touched");
    }
}
