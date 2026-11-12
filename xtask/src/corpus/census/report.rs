//! The census's report: `docs/status/corpus.md`. Contract:
//! `spec/harness.md` "corpus", "census".

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::{APPROXIMATIONS, D, GroupCensus, T, count_corpus};
use crate::corpus::capture::{self, KILN};
use crate::corpus::kiln::harness_owned;
use crate::corpus::manifest::{Manifest, Skips};
use crate::irdiff::directives::llc_arg_allowed;

/// The skip reasons and what each means, in the order the report lists
/// them (`spec/harness.md` "corpus", "Skip reasons"). A reason that is not
/// here is listed after these, without a meaning.
pub const REASONS: [(&str, &str); 33] = [
    ("header-error", "the test's `//@` header does not parse"),
    ("marked-skip", "the test's header says `skip:`"),
    ("marked-xfail", "the test's header says `xfail:`: kiln's own suite expects it to fail"),
    (
        "multi-file",
        "a test of several translation units whose build the capture does not replay (`pch:` \
         in the header; modules that were given different `llc` arguments or would have one \
         name; objects whose link order is not known)",
    ),
    ("needs-env", "the test needs an environment or input files the capture does not set up"),
    ("other-target", "the test names a target that is not the host"),
    ("requires", "a `requires:` line that this host or this LLVM does not meet"),
    (
        "needs-toolchain",
        "`args:` use a placeholder the capture does not substitute (`%{gcc_toolchain}` and \
         the like: a toolchain kiln's own suite locates)",
    ),
    ("libc++", "`-stdlib=libc++`: the entry's link (`link: c++`) is against libstdc++"),
    (
        "link-flags",
        "a link option the entry's link cannot repeat (`-l...`, `-fuse-ld=`, `-static`)",
    ),
    ("opt-first-lto", "`-flto`: kiln++ gives llc nothing; the link runs the code generator"),
    (
        "opt-first-sanitizer",
        "`-fsanitize=`: kiln++ runs `opt` first and gives llc bitcode (skipped only where no \
         `llvm-dis` is installed, as the next four)",
    ),
    ("opt-first-coverage", "coverage or profiling instrumentation: `opt` first, bitcode to llc"),
    ("opt-first-optimized", "`-O1` or higher: `opt` first, bitcode to llc"),
    ("opt-first-coroutine", "coroutines are lowered by `opt`: bitcode to llc"),
    (
        "opt-first-always-inline",
        "an `always_inline` function (most often a standard library one) makes kiln++ run \
         `opt`: bitcode to llc",
    ),
    ("llvm-dis-fails", "`llvm-dis` does not write the bitcode llc was given as text"),
    ("no-module", "kiln++ succeeded without running llc"),
    ("module-not-text", "llc was given something that is neither bitcode nor text IR"),
    ("llc-args-unexpected", "llc's command line is not the shape kiln's driver writes"),
    ("compile-fails", "the producer fails before any module reaches llc"),
    ("clang-rejects", "clang does not accept the test (GNU C that only GCC compiles)"),
    ("special-flags", "the test asks for options the capture does not translate for clang"),
    ("llc-fails", "the real llc rejects or fails on the module"),
    ("link-fails", "the module compiles but the program does not link"),
    ("program-fails", "the program built with the real llc does not exit with status 0 in time"),
    ("expectation-mismatch", "the program built with the real llc does not do what the test says"),
    (
        "stdout-not-expressible",
        "the program's output cannot be written as `;@ stdout:` lines, or is too large",
    ),
    ("stdin-not-expressible", "the test's input cannot be written as a `;@ stdin:` line"),
    ("args-not-expressible", "a program argument cannot be written on a `;@ args:` line"),
    (
        "not-reproducible",
        "the entry does not pass `irdiff` with every linker when llc alone builds it \
         (see \"Admission\")",
    ),
    (
        "oracle-unconfirmed",
        "the program `llc -O2` builds from the entry does not behave as the oracle's: one of \
         the two is wrong, or the program depends on how it is compiled (see \"Admission\")",
    ),
    ("capture-broken", "the capture itself failed for this test (a tool could not be run)"),
];

/// The reasons that are a failure of the test with the real tools, not a
/// limit of the capture.
const FAILURES: [&str; 9] = [
    "compile-fails",
    "llvm-dis-fails",
    "llc-fails",
    "link-fails",
    "program-fails",
    "expectation-mismatch",
    "no-module",
    "not-reproducible",
    "oracle-unconfirmed",
];

/// `text` as a Markdown code span that is safe in a table cell.
fn code(text: &str) -> String {
    let text = text.replace('|', "\\|").replace(['\n', '\t'], " ");
    if text.contains('`') { format!("`` {text} ``") } else { format!("`{text}`") }
}

/// `text` as plain text in a list item or a table cell.
fn plain(text: &str, limit: usize) -> String {
    let mut text: String = text
        .chars()
        .map(|c| match c {
            '`' => '\'',
            '|' | '<' | '>' | '*' | '_' | '[' | ']' | '\\' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    if text.len() > limit {
        let mut cut = limit;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...");
    }
    text
}

/// A skip's detail, a tool's own words, as a code span of bounded length.
fn quoted(text: &str, limit: usize) -> String {
    let text: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if text.len() <= limit {
        return code(&text);
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    code(&format!("{}...", &text[..cut]))
}

fn row(cells: &[String]) -> String {
    format!("| {} |\n", cells.join(" | "))
}

fn header(cells: &[String], numeric_from: usize) -> String {
    let rule: Vec<String> = (0..cells.len())
        .map(|at| if at >= numeric_from { "---:".to_owned() } else { "---".to_owned() })
        .collect();
    format!("{}{}", row(cells), row(&rule))
}

/// A number with thin groups: `1234567` is `1,234,567`.
fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A group: a source and an LLVM major. In the report's order: sources by
/// name, of each the newest LLVM first.
type Group = (String, Reverse<u32>);

fn group_name(group: &Group) -> String {
    format!("{}/llvm{}", group.0, group.1.0)
}

fn majors_of(majors: &BTreeSet<Reverse<u32>>) -> String {
    let list: Vec<String> = majors.iter().map(|major| major.0.to_string()).collect();
    format!("LLVM {}", list.join(", "))
}

/// The report's "Capture" part: what the tables of the corpus say.
#[expect(
    clippy::too_many_lines,
    reason = "one case after another; splitting it would not make it clearer"
)]
fn capture_part(dir: &Path, manifest: &Manifest, skips: &Skips, groups: &[Group]) -> String {
    let mut text = String::from("## Capture\n\n");
    let names: Vec<String> = groups.iter().map(group_name).collect();

    // Per group.
    let mut cells = vec!["group".to_owned()];
    cells.extend(
        ["tests seen", "captured", "skipped", "entry bytes", "producer", "source revision"]
            .map(str::to_owned),
    );
    let head = header(&cells, 1);
    // The last two columns are text.
    text.push_str(&head.replacen("---: | ---: |\n", "--- | --- |\n", 1));
    let mut total = (0u64, 0u64, 0u64, 0u64);
    for group in groups {
        let rows: Vec<_> = manifest
            .rows
            .values()
            .filter(|row| row.source == group.0 && row.llvm == group.1.0)
            .collect();
        let skipped = skips
            .rows
            .values()
            .filter(|skip| skip.source == group.0 && skip.llvm == group.1.0)
            .count() as u64;
        let bytes: u64 = rows.iter().map(|row| row.bytes).sum();
        let producers: BTreeSet<&str> = rows.iter().map(|row| row.producer.as_str()).collect();
        let revisions: BTreeSet<&str> = rows.iter().map(|row| row.revision.as_str()).collect();
        let captured = rows.len() as u64;
        total =
            (total.0 + captured + skipped, total.1 + captured, total.2 + skipped, total.3 + bytes);
        text.push_str(&row(&[
            code(&group_name(group)),
            number(captured + skipped),
            number(captured),
            number(skipped),
            number(bytes),
            producers.iter().map(|p| plain(p, 80)).collect::<Vec<_>>().join("; "),
            revisions.iter().map(|r| code(r)).collect::<Vec<_>>().join(" "),
        ]));
    }
    text.push_str(&row(&[
        "all".to_owned(),
        number(total.0),
        number(total.1),
        number(total.2),
        number(total.3),
        String::new(),
        String::new(),
    ]));

    let runs = capture::read_runs(dir);
    if !runs.is_empty() {
        text.push_str(
            "\nThe last capture that made each group anew, nothing reused (from `CAPTURE.tsv`; \
             times are of one run on one shared machine):\n\n",
        );
        let cells = ["group", "tests", "captured", "reused", "skipped", "seconds", "jobs"]
            .map(str::to_owned);
        text.push_str(&header(&cells, 1));
        for group in groups {
            for run in runs.iter().filter(|run| run.source == group.0 && run.llvm == group.1.0) {
                text.push_str(&row(&[
                    code(&group_name(group)),
                    number(run.tests as u64),
                    number(run.captured as u64),
                    number(run.reused as u64),
                    number(run.skipped as u64),
                    format!("{:.1}", run.seconds),
                    run.jobs.to_string(),
                ]));
            }
        }
    }

    // Skip reasons.
    let mut by_reason: BTreeMap<&str, BTreeMap<&Group, u64>> = BTreeMap::new();
    for skip in skips.rows.values() {
        if let Some(group) = groups.iter().find(|g| g.0 == skip.source && g.1.0 == skip.llvm) {
            *by_reason.entry(&skip.reason).or_default().entry(group).or_default() += 1;
        }
    }
    let mut order: Vec<&str> =
        REASONS.iter().map(|(reason, _)| *reason).filter(|r| by_reason.contains_key(r)).collect();
    order.extend(
        by_reason.keys().filter(|reason| !REASONS.iter().any(|(known, _)| known == *reason)),
    );
    text.push_str("\n### Skipped tests by reason\n\n");
    if order.is_empty() {
        text.push_str("No test was skipped.\n");
    } else {
        let mut cells = vec!["reason".to_owned()];
        cells.extend(names.iter().map(|name| code(name)));
        cells.push("meaning".to_owned());
        let mut head = header(&cells, 1);
        // The last column is text.
        head = head.replacen("---: |\n", "--- |\n", 1);
        text.push_str(&head);
        for reason in &order {
            let mut cells = vec![code(reason)];
            for group in groups {
                let count = by_reason[reason].get(group).copied().unwrap_or(0);
                cells.push(if count == 0 { String::new() } else { number(count) });
            }
            let meaning = REASONS.iter().find(|(known, _)| known == reason).map_or("", |(_, m)| m);
            cells.push(meaning.to_owned());
            text.push_str(&row(&cells));
        }
    }

    // llc's arguments.
    let mut by_args: BTreeMap<&str, BTreeMap<&Group, u64>> = BTreeMap::new();
    for entry in manifest.rows.values() {
        if let Some(group) = groups.iter().find(|g| g.0 == entry.source && g.1.0 == entry.llvm) {
            *by_args.entry(&entry.llc_args).or_default().entry(group).or_default() += 1;
        }
    }
    // Repeated: every argument is the harness's own in its own way, or
    // one the entry's `llc-args` directive holds.
    let expressible = |args: &str| {
        args.split(' ').all(|arg| {
            arg == "-O0" || (!arg.starts_with("-O") && harness_owned(arg)) || llc_arg_allowed(arg)
        })
    };
    text.push_str(
        "\n### `llc` arguments\n\nThe arguments the producer's driver gave `llc` (without `-o \
         <object>` and the input). `irdiff` builds an entry with `-O0 -filetype=obj \
         -relocation-model=pic` or `static` (by the entry's `link:` line), the module's own \
         triple, and the arguments of the entry's `llc-args` directive: those of the \
         producer's that are on the directive's list. Anything else (an optimization level \
         above `-O0`, an argument that names a file) is recorded in the manifest and in the \
         entry's header comment, and is not repeated by `irdiff`.\n\n",
    );
    let mut cells = vec!["llc arguments".to_owned()];
    cells.extend(names.iter().map(|name| code(name)));
    cells.push("repeated by irdiff".to_owned());
    text.push_str(&header(&cells, 1));
    let mut sets: Vec<(&str, u64)> =
        by_args.iter().map(|(args, counts)| (*args, counts.values().sum())).collect();
    sets.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut beyond = 0;
    for (args, count) in sets {
        let mut cells = vec![code(args)];
        for group in groups {
            let count = by_args[args].get(group).copied().unwrap_or(0);
            cells.push(if count == 0 { String::new() } else { number(count) });
        }
        cells.push(if expressible(args) { "yes" } else { "no" }.to_owned());
        if !expressible(args) {
            beyond += count;
        }
        text.push_str(&row(&cells));
    }
    let _ = writeln!(
        text,
        "\n{} of {} entries were built by their producer with `llc` arguments `irdiff` does \
         not repeat.",
        number(beyond),
        number(manifest.rows.len() as u64)
    );

    // How the modules reached llc, and how many an entry has.
    text.push_str(
        "\n### Modules of an entry\n\nAn entry of several modules is a test of several \
         translation units, each compiled on its own and linked into one program. A module \
         that reached `llc` as bitcode (`opt` ran first) is in the entry as the text \
         `llvm-dis` writes for it.\n\n",
    );
    let mut cells = vec!["entries".to_owned()];
    cells.extend(names.iter().map(|name| code(name)));
    text.push_str(&header(&cells, 1));
    // The manifest has one word per module.
    let kinds: [(&str, fn(&str) -> bool); 2] = [
        ("of several modules", |delivered| delivered.contains(' ')),
        ("with a module delivered as bitcode", |delivered| delivered.contains("bitcode")),
    ];
    for (label, is) in kinds {
        let mut cells = vec![label.to_owned()];
        for group in groups {
            let count = manifest
                .rows
                .values()
                .filter(|row| row.source == group.0 && row.llvm == group.1.0 && is(&row.delivered))
                .count() as u64;
            cells.push(if count == 0 { String::new() } else { number(count) });
        }
        text.push_str(&row(&cells));
    }
    text
}

/// The tests skipped for one reason, of one source: each with the LLVM
/// majors it was skipped for and a detail.
type Tests<'a> = BTreeMap<&'a str, (BTreeSet<Reverse<u32>>, &'a str)>;

/// The report's lists of skipped tests.
fn skipped_part(skips: &Skips, sources: &BTreeSet<&str>) -> String {
    let mut text = String::from(
        "## Tests that are not in the corpus\n\nEvery test the capture saw and did not capture, \
         by source and reason, with the LLVM majors it was skipped for.\n",
    );
    let mut lists: BTreeMap<(&str, &str), Tests<'_>> = BTreeMap::new();
    for skip in skips.rows.values() {
        let tests = lists.entry((&skip.source, &skip.reason)).or_default();
        let slot = tests.entry(&skip.test).or_insert((BTreeSet::new(), &skip.detail));
        slot.0.insert(Reverse(skip.llvm));
    }

    if sources.contains(KILN) {
        text.push_str(
            "\n### kiln tests that fail without hod\n\nThe capture builds each of kiln's \
             executing tests (`expect: run`) with plain `kiln++` and the real `llc`, honouring \
             the test's header (`args:`, `run-args:`, `run-stdin:`, `exit:`, `stdout:`, \
             `requires:`, `xfail:`). A test that then does not build, link, run as its header \
             says, or pass `irdiff` with every linker is a failure that has nothing to do with \
             hod. These are all of them:\n\n",
        );
        let mut any = false;
        for ((source, reason), tests) in &lists {
            if *source != KILN || !FAILURES.contains(reason) {
                continue;
            }
            for (test, (majors, detail)) in tests {
                any = true;
                let _ = writeln!(
                    text,
                    "- {} ({}): {}: {}",
                    code(test),
                    majors_of(majors),
                    code(reason),
                    quoted(detail, 240)
                );
            }
        }
        if !any {
            text.push_str("None.\n");
        }
        text.push_str(
            "\nA runner that does *not* honour the header fails on more: every test under \
             `multi-file`, `needs-toolchain`, `needs-env`, `marked-xfail` and most under \
             `requires` below fails to build or run when its file alone is given to `kiln++` \
             with the header's arguments verbatim, and a test with a `run-stdin:` line \
             fails without its input. A C++ test also fails to link when the compiler is run \
             under the name `kiln` instead of `kiln++` (the name selects the C++ driver and \
             with it the C++ standard library at the link). Measured once over the 1540 run \
             tests of kiln `bd60e182`, such a runner fails 176 of them, by cause in \
             `spec/landed/harness/WP-017.md`.\n",
        );
    }

    for source in sources {
        let _ = writeln!(text, "\n### {source}: skipped tests");
        let mut reasons: Vec<&str> = REASONS
            .iter()
            .map(|(reason, _)| *reason)
            .filter(|reason| lists.contains_key(&(*source, *reason)))
            .collect();
        reasons.extend(
            lists
                .keys()
                .filter(|(s, reason)| {
                    s == source && !REASONS.iter().any(|(known, _)| known == reason)
                })
                .map(|(_, reason)| *reason),
        );
        if reasons.is_empty() {
            text.push_str("\nNone.\n");
        }
        for reason in reasons {
            let tests = &lists[&(*source, reason)];
            let _ = writeln!(text, "\n{} ({}):\n", code(reason), tests.len());
            for (test, (majors, detail)) in tests {
                let _ = writeln!(
                    text,
                    "- {} ({}): {}",
                    code(test),
                    majors_of(majors),
                    quoted(detail, 160)
                );
            }
        }
    }
    text
}

/// The report's "Census" part.
fn census_part(groups: &[Group], counts: &BTreeMap<String, GroupCensus>) -> String {
    let empty = GroupCensus::default();
    let names: Vec<String> = groups.iter().map(group_name).collect();
    let of: Vec<&GroupCensus> =
        names.iter().map(|name| counts.get(name).unwrap_or(&empty)).collect();
    let mut text = String::from("## Census\n\n");
    text.push_str(APPROXIMATIONS);

    text.push_str("\n### Totals\n\n");
    let mut cells = vec!["total".to_owned()];
    cells.extend(names.iter().map(|name| code(name)));
    text.push_str(&header(&cells, 1));
    let mut cells = vec!["modules".to_owned()];
    cells.extend(of.iter().map(|census| number(u64::from(census.modules))));
    text.push_str(&row(&cells));
    for (title, d) in [
        ("module text, bytes", D::ModuleBytes),
        ("function definitions", D::Definitions),
        ("function declarations", D::Declarations),
        ("global variables", D::GlobalVariables),
        ("basic blocks", D::BlocksPerModule),
        ("instructions", D::InstructionsPerModule),
        ("numbered metadata nodes", D::MetadataNodes),
    ] {
        let mut cells = vec![title.to_owned()];
        cells.extend(of.iter().map(|census| number(census.summary(d).sum)));
        text.push_str(&row(&cells));
    }

    text.push_str("\n### Sizes\n\nMedian / 90th percentile / maximum.\n\n");
    let mut cells = vec!["size".to_owned()];
    cells.extend(names.iter().map(|name| code(name)));
    text.push_str(&header(&cells, 1));
    for d in D::ALL {
        let mut cells = vec![d.title().to_owned()];
        for census in &of {
            let summary = census.summary(d);
            cells.push(if summary.n == 0 {
                String::new()
            } else {
                format!(
                    "{} / {} / {}",
                    number(summary.median),
                    number(summary.p90),
                    number(summary.max)
                )
            });
        }
        text.push_str(&row(&cells));
    }

    for table in T::ALL {
        let (title, what) = table.title();
        let _ = writeln!(text, "\n### {title}\n\n{what}\n");
        let mut keys: BTreeSet<&str> = BTreeSet::new();
        for census in &of {
            keys.extend(census.keys(table));
        }
        if keys.is_empty() {
            text.push_str("None in the corpus.\n");
            continue;
        }
        let mut rows: Vec<(u64, &str)> = keys
            .into_iter()
            .map(|key| (of.iter().map(|census| census.count(table, key)).sum(), key))
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        let mut cells = vec!["row".to_owned(), "all".to_owned()];
        cells.extend(names.iter().map(|name| code(name)));
        text.push_str(&header(&cells, 1));
        let cap = table.cap();
        for (total, key) in rows.iter().take(cap) {
            let mut cells = vec![code(key), number(*total)];
            for census in &of {
                let cell = census.cell(table, key);
                cells.push(if cell.count == 0 {
                    String::new()
                } else {
                    format!("{} ({})", number(cell.count), number(u64::from(cell.modules)))
                });
            }
            text.push_str(&row(&cells));
        }
        if rows.len() > cap {
            let rest: u64 = rows[cap..].iter().map(|(total, _)| total).sum();
            let _ = writeln!(
                text,
                "\n{} more rows, {} occurrences together.",
                number((rows.len() - cap) as u64),
                number(rest)
            );
        }
    }
    text
}

/// The whole report of the corpus in `dir`, counted `jobs` modules at a
/// time. The same corpus gives the same text.
pub fn render(dir: &Path, jobs: usize) -> Result<String, String> {
    let manifest = Manifest::read(dir)?;
    let skips = Skips::read(dir)?;
    if manifest.rows.is_empty() && skips.rows.is_empty() {
        return Err(format!(
            "no corpus in '{}': run `cargo xtask corpus capture` first",
            dir.display()
        ));
    }
    let mut groups: BTreeSet<Group> = BTreeSet::new();
    groups.extend(manifest.rows.values().map(|row| (row.source.clone(), Reverse(row.llvm))));
    groups.extend(skips.rows.values().map(|skip| (skip.source.clone(), Reverse(skip.llvm))));
    let groups: Vec<Group> = groups.into_iter().collect();
    let sources: BTreeSet<&str> = groups.iter().map(|group| group.0.as_str()).collect();
    let entries: Vec<(String, String)> = manifest
        .rows
        .iter()
        .map(|(entry, row)| (format!("{}/llvm{}", row.source, row.llvm), entry.clone()))
        .collect();
    let counts = count_corpus(dir, &entries, jobs)?;

    let mut text = String::from(
        "# The corpus: what hod has to compile\n\n<!-- Generated by `cargo xtask corpus census` \
         from the captured corpus. Do not edit: run the command again. -->\n\nThe corpus is the \
         LLVM IR that hod's front ends really produce, captured by `cargo xtask corpus capture` \
         (`spec/harness.md`, \"corpus\") as `irdiff` tests: every module here was compiled by \
         the real `llc`, linked and run, and the entry records what the program did. This \
         report says what was captured and what was not, and counts the constructs of the \
         captured modules. Counts are `occurrences (modules with at least one)`; a group is a \
         source and the LLVM major whose tools produced and built it.\n\n",
    );
    text.push_str(&capture_part(dir, &manifest, &skips, &groups));
    text.push('\n');
    text.push_str(&census_part(&groups, &counts));
    text.push('\n');
    text.push_str(&skipped_part(&skips, &sources));
    Ok(text)
}

/// Counts the corpus in `dir` and writes the report to `report`. Returns
/// the line the command prints.
pub fn write_report(dir: &Path, report: &Path) -> Result<String, String> {
    let jobs =
        std::thread::available_parallelism().map_or(1, usize::from).min(capture::DEFAULT_JOBS);
    let text = render(dir, jobs)?;
    if let Some(parent) = report.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    fs::write(report, &text).map_err(|e| format!("cannot write '{}': {e}", report.display()))?;
    let modules = Manifest::read(dir)?.rows.len();
    Ok(format!("corpus census: {modules} modules counted; report in '{}'\n", report.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells() {
        assert_eq!(code("a|b"), "`a\\|b`");
        assert_eq!(code("a`b"), "`` a`b ``");
        assert_eq!(plain("a `b` <c>\td", 100), "a 'b'  c  d");
        assert_eq!(plain("abcdef", 3), "abc...");
        assert_eq!(quoted("a_b[0] == '/'\n", 100), "`a_b[0] == '/' `");
        assert_eq!(quoted("a|b `c`", 100), "`` a\\|b `c` ``");
        assert_eq!(quoted("abcdef", 3), "`abc...`");
        assert_eq!(number(0), "0");
        assert_eq!(number(999), "999");
        assert_eq!(number(1000), "1,000");
        assert_eq!(number(1_234_567), "1,234,567");
        assert_eq!(header(&["a".to_owned(), "b".to_owned()], 1), "| a | b |\n| --- | ---: |\n");
    }

    #[test]
    fn reasons_are_distinct() {
        let names: BTreeSet<&str> = REASONS.iter().map(|(reason, _)| *reason).collect();
        assert_eq!(names.len(), REASONS.len());
        for failure in FAILURES {
            assert!(names.contains(failure), "{failure}");
        }
    }
}
