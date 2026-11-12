//! What is made from the walk and the matrix for people to read
//! (`spec/ir.md` §8.9, §8.10): the census and `docs/status/support.md`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::RejectionKind;
use super::matrix::{
    CONSTRUCT_FAMILIES, ConstructInfo, INVALID_RULES, STRUCTURAL_RULES, SUPPORT_GROUPS,
    SUPPORT_ROWS, SupportCell, SupportRow, SupportStatus, TypeClass, TypeClasses,
};
use super::walk::{At, Finding, Flow, Sink};
use crate::ids::SymId;

/// One construct of a [`Census`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CensusEntry {
    /// The construct's name (§8.5).
    pub construct: String,
    /// Whether it is unsupported or malformed input.
    pub kind: RejectionKind,
    /// How many times it was met.
    pub occurrences: u64,
    /// How many distinct symbols it was met at: for a construct of a
    /// body, the functions affected. Findings on the module itself count
    /// in `occurrences` only.
    pub symbols: u64,
}

/// Every reason a module and its bodies are not compiled, counted (§8.9).
///
/// Where `classify` stops at the first reason, the census goes on: it
/// answers "what would have to be implemented for this input", which the
/// first reason alone does not.
#[derive(Clone, Debug, Default)]
pub struct Census {
    entries: Vec<CensusEntry>,
    index: HashMap<String, u32>,
    /// `(entry, symbol)` pairs already counted in `symbols`.
    seen: HashSet<(u32, SymId)>,
}

impl Census {
    /// An empty census.
    pub fn new() -> Census {
        Census::default()
    }

    /// The constructs, in the order each was first met (§8.8).
    pub fn entries(&self) -> &[CensusEntry] {
        &self.entries
    }

    /// The entry of the construct called `construct`.
    pub fn get(&self, construct: &str) -> Option<&CensusEntry> {
        self.index.get(construct).map(|&index| &self.entries[index as usize])
    }

    /// Whether nothing was found: `classify` accepts what was counted.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of distinct constructs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Sink for Census {
    fn report(&mut self, finding: Finding<'_>, at: At) -> Flow {
        let (kind, construct) = finding.name();
        let index = if let Some(&index) = self.index.get(construct.as_ref()) {
            index
        } else {
            let index = self.entries.len() as u32;
            let construct = construct.into_owned();
            self.index.insert(construct.clone(), index);
            self.entries.push(CensusEntry { construct, kind, occurrences: 0, symbols: 0 });
            index
        };
        let entry = &mut self.entries[index as usize];
        entry.occurrences += 1;
        if let Some(symbol) = at.symbol
            && self.seen.insert((index, symbol))
        {
            entry.symbols += 1;
        }
        Flow::Continue(())
    }
}

// ---- docs/status/support.md ----

/// What every x86-64 `not-implemented` waits for unless its row says
/// otherwise; left out of the last column.
const X64_DEFAULT: &str = "T-X64";

fn cell_text(cell: SupportCell) -> String {
    match cell.status {
        SupportStatus::Supported if cell.classes == TypeClasses::ALL => "supported".to_owned(),
        SupportStatus::Supported => {
            let classes: Vec<&str> = cell.classes.iter().map(TypeClass::name).collect();
            format!("supported ({})", classes.join(", "))
        }
        SupportStatus::NotImplemented(_) => "not-implemented".to_owned(),
        SupportStatus::Rejected(_) => "rejected".to_owned(),
    }
}

/// Why a cell is not supported: the work package or the reason.
fn cell_why(cell: SupportCell) -> &'static str {
    match cell.status {
        SupportStatus::Supported => "",
        SupportStatus::NotImplemented(why) | SupportStatus::Rejected(why) => why,
    }
}

fn row_why(row: &SupportRow) -> String {
    let (a, x) = (cell_why(row.aarch64), cell_why(row.x86_64));
    let default_x = matches!(row.x86_64.status, SupportStatus::NotImplemented(X64_DEFAULT));
    let mut out = String::new();
    if a == x || default_x || x.is_empty() {
        out.push_str(a);
    } else if a.is_empty() {
        let _ = write!(out, "x86_64: {x}");
    } else {
        let _ = write!(out, "aarch64: {a}; x86_64: {x}");
    }
    if !row.note.is_empty() {
        if !out.is_empty() {
            out.push_str(". ");
        }
        // The first letter of a note that starts the column is not
        // capitalised: notes are fragments.
        out.push_str(row.note);
    }
    out
}

fn count(rows: &[&SupportRow], cell: fn(&SupportRow) -> SupportCell) -> [usize; 3] {
    let mut counts = [0; 3];
    for row in rows {
        counts[match cell(row).status {
            SupportStatus::Supported => 0,
            SupportStatus::NotImplemented(_) => 1,
            SupportStatus::Rejected(_) => 2,
        }] += 1;
    }
    counts
}

fn names_table(out: &mut String, head: &str, list: &[ConstructInfo]) {
    let _ = writeln!(out, "| {head} | when |\n|---|---|");
    for info in list {
        let _ = writeln!(out, "| `{}` | {} |", info.name, info.what);
    }
}

/// The text of `docs/status/support.md`: the support matrix, one table
/// per group, then the names that are not rows (§8.10). The file is
/// checked in; `tests/classify_support_doc.rs` fails when it differs
/// from this, and rewrites it when run with `HOD_BLESS=1`.
pub fn support_markdown() -> String {
    let mut out = String::new();
    out.push_str(
        "# Support matrix\n\
         \n\
         <!-- Generated from crates/hod_ir/src/classify/matrix.rs. Do not edit: run\n     \
         `HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc` instead. -->\n\
         \n\
         What `hod_ir::classify` accepts, per construct and target (`spec/ir.md` §8).\n\
         A module or function with a construct that is not `supported` is not\n\
         compiled by hod: the driver reports the construct's name and hands the\n\
         input to the fallback when one is configured (`spec/cli.md`).\n\
         \n\
         - `supported`: code generation implements it. `supported (int, ptr)` means\n  \
         for values of those type classes only (`spec/ir.md` §8.3).\n\
         - `not-implemented`: it will be; the last column names the work package,\n  \
         milestone or track (`docs/BACKLOG.md`, `docs/ROADMAP.md`). An x86_64\n  \
         `not-implemented` waits for the x86-64 target (`T-X64`) unless the row\n  \
         says otherwise.\n\
         - `rejected`: refused by design; the last column says why.\n\
         \n\
         ## Summary\n\
         \n\
         | group | constructs | aarch64 supported | not-implemented | rejected \
         | x86_64 supported | not-implemented | rejected |\n\
         |---|---|---|---|---|---|---|---|\n",
    );
    let mut total = [0; 7];
    for (group, _) in SUPPORT_GROUPS {
        let rows: Vec<&SupportRow> =
            SUPPORT_ROWS.iter().filter(|row| row.group() == group).collect();
        let a = count(&rows, |row| row.aarch64);
        let x = count(&rows, |row| row.x86_64);
        let line = [rows.len(), a[0], a[1], a[2], x[0], x[1], x[2]];
        let _ = write!(out, "| `{group}` |");
        for (sum, value) in total.iter_mut().zip(line) {
            *sum += value;
            let _ = write!(out, " {value} |");
        }
        out.push('\n');
    }
    out.push_str("| total |");
    for value in total {
        let _ = write!(out, " {value} |");
    }
    out.push('\n');

    for (group, what) in SUPPORT_GROUPS {
        let _ = write!(
            out,
            "\n## `{group}`\n\n{what}.\n\n\
             | construct | aarch64 | x86_64 | work package / why |\n|---|---|---|---|\n"
        );
        for row in SUPPORT_ROWS.iter().filter(|row| row.group() == group) {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                row.name,
                cell_text(row.aarch64),
                cell_text(row.x86_64),
                row_why(row)
            );
        }
    }

    out.push_str(
        "\n## Structural rules\n\n\
         Refused whatever the matrix says (`spec/ir.md` §8.6), on both targets.\n\n",
    );
    names_table(&mut out, "construct", &STRUCTURAL_RULES);
    out.push_str(
        "\n## Malformed input\n\n\
         Not a missing feature: the module or body is not well formed \
         (`spec/ir.md` §8.7).\nNo matrix accepts it; the producer of the input has a bug.\n\n",
    );
    names_table(&mut out, "construct", &INVALID_RULES);
    out.push_str(
        "\n## Names built from the input\n\n\
         Always refused (`spec/ir.md` §8.5), except a pair, which a work package\n\
         removes by widening its row.\n\n",
    );
    names_table(&mut out, "form", &CONSTRUCT_FAMILIES);
    out
}
