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
            let (Ok(address), Some(line), Some(file)) =
                (u64::from_str_radix(&words[0][2..], 16), number(1), number(3))
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

/// Compares the pairs of two programs built from the same IR.
pub fn compare(hod: &Path, llc: &Path, tools: &LineTools) -> Result<LinesReport, String> {
    let read = |program: &Path| -> Result<Pairs, String> {
        let lines = tool_output(&tools.dwarfdump, &["--debug-line"], program)?;
        let symbols = tool_output(&tools.nm, &["-S", "--defined-only"], program)?;
        Ok(pairs_of(&parse_line_tables(&lines), &parse_symbols(&symbols)))
    };
    let (ours, theirs) = (read(hod)?, read(llc)?);
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
    Ok(report)
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
        };
        assert_eq!(report.unclassified(), 1);
        assert_eq!(
            report.summary(),
            "same 3, hod only 1 (P1 0, P3 1, unclassified 0), llc only 2 (P1 1, P3 0, unclassified 1)"
        );
    }
}
