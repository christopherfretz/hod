//! `cargo xtask corpus compare`: the corpus of one architecture against
//! the corpus of the other. Contract: `spec/harness.md` "corpus",
//! "compare".
//!
//! An entry's expectations are what the reference build of its module did
//! on its architecture. The same test captured for x86-64 and for AArch64
//! must do the same thing unless what it prints depends on the target
//! (`sizeof(long double)`, `char`'s signedness, `va_list`, code the
//! preprocessor selects); every other difference is a bug of a producer's
//! lowering for one target, or of `llc`, or an artefact of the emulator.
//! The command finds the differences; a table kept in the repository says
//! what each one is, so that a new one shows.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::capture::{self, TimeRow};
use super::manifest::{Manifest, Skips};
use crate::irdiff::directives::{self, Arch, Exit};

/// The classes of a difference ("compare").
pub const CLASSES: [(&str, &str); 6] = [
    ("target", "target-dependent by nature: the program prints or does what its target decides"),
    ("emulation", "an artefact of the emulator (timing, signals, the CPU model)"),
    ("kiln", "suspected kiln lowering bug for one target (a finding for the kiln agents)"),
    ("clang", "suspected clang bug for one target"),
    ("llc", "suspected llc bug for one target"),
    ("unknown", "not yet explained"),
];

/// What a test's program did in one corpus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviour {
    pub exit: Exit,
    pub stdout: Vec<u8>,
}

/// One explained difference: the test (`<source> <test>`), its class of
/// [`CLASSES`] and the cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explained {
    pub class: String,
    pub cause: String,
}

/// Reads the table of explained differences: lines `<source> <test>⇥
/// <class>⇥<cause>`; blank lines and lines starting with `#` are comments.
pub fn parse_differences(text: &str) -> Result<BTreeMap<String, Explained>, String> {
    let mut found = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [test, class, cause] = fields[..] else {
            return Err(format!("line {}: expected 3 tab-separated fields", index + 1));
        };
        if !CLASSES.iter().any(|(name, _)| *name == class) {
            return Err(format!("line {}: unknown class '{class}'", index + 1));
        }
        let explained = Explained { class: class.to_owned(), cause: cause.to_owned() };
        if found.insert(test.to_owned(), explained).is_some() {
            return Err(format!("line {}: '{test}' is explained twice", index + 1));
        }
    }
    Ok(found)
}

/// The behaviour an entry file records.
fn behaviour(path: &Path) -> Result<Behaviour, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("'{}': {e}", path.display()))?;
    let expect = directives::parse(&text).map_err(|errors| {
        let first = errors.first().map(ToString::to_string).unwrap_or_default();
        format!("'{}': {first}", path.display())
    })?;
    Ok(Behaviour { exit: expect.exit, stdout: expect.stdout })
}

fn exit_text(exit: Exit) -> String {
    match exit {
        Exit::Code(code) => format!("exit {code}"),
        Exit::Signal(signal) => format!("signal {signal}"),
    }
}

/// What differs between `ours` and `theirs`, in one line: the exit, then
/// the first line of output that differs.
pub fn describe(ours: &Behaviour, theirs: &Behaviour) -> String {
    let mut parts = Vec::new();
    if ours.exit != theirs.exit {
        parts.push(format!("{} here, {} there", exit_text(ours.exit), exit_text(theirs.exit)));
    }
    if ours.stdout != theirs.stdout {
        let a: Vec<&[u8]> = ours.stdout.split(|b| *b == b'\n').collect();
        let b: Vec<&[u8]> = theirs.stdout.split(|b| *b == b'\n').collect();
        let at = (0..a.len().max(b.len())).find(|i| a.get(*i) != b.get(*i)).unwrap_or(0);
        let show = |line: Option<&&[u8]>| {
            line.map_or_else(
                || "(none)".to_owned(),
                |line| {
                    let text = String::from_utf8_lossy(line);
                    let cut: String = text.chars().take(60).collect();
                    format!("`{}`", cut.replace('`', "'").replace('|', "\\|"))
                },
            )
        };
        parts.push(format!(
            "stdout line {}: {} here, {} there",
            at + 1,
            show(a.get(at)),
            show(b.get(at))
        ));
    }
    parts.join("; ")
}

/// One test of both corpora whose programs differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// `<source> <test>`.
    pub test: String,
    /// The LLVM majors it differs with.
    pub llvm: Vec<u32>,
    pub what: String,
}

/// What the comparison found.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    /// Per group (`<source>/llvm<N>`): entries here, there, in both, the
    /// same.
    pub groups: BTreeMap<String, [usize; 4]>,
    pub differences: Vec<Difference>,
    /// Tests with an entry in one corpus only: (group, test, the other
    /// corpus's skip reason or `-`), here and there.
    pub only_here: Vec<(String, String, String)>,
    pub only_there: Vec<(String, String, String)>,
}

/// Compares the corpus `here` with the corpus `there`.
pub fn compare(here: &Path, there: &Path) -> Result<Comparison, String> {
    let (ours, theirs) = (Manifest::read(here)?, Manifest::read(there)?);
    let (our_skips, their_skips) = (Skips::read(here)?, Skips::read(there)?);
    let key = |row: &super::manifest::Row| (row.source.clone(), row.llvm, row.test.clone());
    let their_rows: BTreeMap<_, _> = theirs.rows.values().map(|row| (key(row), row)).collect();
    let our_keys: BTreeSet<_> = ours.rows.values().map(key).collect();
    let mut comparison = Comparison::default();
    let mut differing: BTreeMap<String, (Vec<u32>, String)> = BTreeMap::new();
    let reason = |skips: &Skips, slot: &(String, u32, String)| {
        skips.rows.get(slot).map_or_else(|| "-".to_owned(), |skip| skip.reason.clone())
    };
    for row in ours.rows.values() {
        let group = format!("{}/llvm{}", row.source, row.llvm);
        let counts = comparison.groups.entry(group.clone()).or_default();
        counts[0] += 1;
        let slot = key(row);
        let Some(other) = their_rows.get(&slot) else {
            comparison.only_here.push((group, row.test.clone(), reason(&their_skips, &slot)));
            continue;
        };
        counts[2] += 1;
        let a = behaviour(&here.join(&row.entry))?;
        let b = behaviour(&there.join(&other.entry))?;
        if a == b {
            counts[3] += 1;
        } else {
            let test = format!("{} {}", row.source, row.test);
            let found = differing.entry(test).or_insert_with(|| (Vec::new(), describe(&a, &b)));
            found.0.push(row.llvm);
        }
    }
    for row in theirs.rows.values() {
        let group = format!("{}/llvm{}", row.source, row.llvm);
        comparison.groups.entry(group.clone()).or_default()[1] += 1;
        let slot = key(row);
        if !our_keys.contains(&slot) {
            comparison.only_there.push((group, row.test.clone(), reason(&our_skips, &slot)));
        }
    }
    comparison.differences = differing
        .into_iter()
        .map(|(test, (mut llvm, what))| {
            llvm.sort_unstable_by(|a, b| b.cmp(a));
            Difference { test, llvm, what }
        })
        .collect();
    Ok(comparison)
}

/// The value at quantile `q` of sorted `values`.
fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let at = ((values.len() - 1) as f64 * q).round() as usize;
    values[at.min(values.len() - 1)]
}

/// The report's part on how long the programs ran ([`capture::TIMES`]).
pub fn render_times(rows: &[TimeRow], limit_seconds: f64) -> String {
    let mut text = String::new();
    let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for row in rows {
        groups.entry(format!("{}/llvm{}", row.source, row.llvm)).or_default().push(row.seconds);
    }
    let _ = writeln!(
        text,
        "| group | programs | median s | p90 s | p99 s | max s | over 1 s | over 10 s | timed out |"
    );
    let _ = writeln!(text, "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    let mut all: Vec<f64> = Vec::new();
    let line = |text: &mut String, name: &str, values: &mut Vec<f64>, out: usize| {
        values.sort_by(f64::total_cmp);
        let over = |limit: f64| values.iter().filter(|value| **value > limit).count();
        let _ = writeln!(
            text,
            "| {name} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {} | {} | {out} |",
            values.len(),
            quantile(values, 0.5),
            quantile(values, 0.9),
            quantile(values, 0.99),
            values.last().copied().unwrap_or(0.0),
            over(1.0),
            over(10.0),
        );
    };
    let timeouts = |group: Option<&str>| {
        rows.iter()
            .filter(|row| row.ended == "timeout")
            .filter(|row| group.is_none_or(|g| format!("{}/llvm{}", row.source, row.llvm) == g))
            .count()
    };
    for (group, mut values) in groups {
        all.extend(&values);
        line(&mut text, &format!("`{group}`"), &mut values, timeouts(Some(&group)));
    }
    line(&mut text, "all", &mut all, timeouts(None));
    let mut slow: Vec<&TimeRow> = rows.iter().collect();
    slow.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then(a.test.cmp(&b.test)));
    let _ =
        writeln!(text, "\nThe slowest 20 (the limit under emulation is {limit_seconds:.0} s):\n");
    let _ = writeln!(text, "| group | test | seconds | ended |");
    let _ = writeln!(text, "| --- | --- | ---: | --- |");
    for row in slow.iter().take(20) {
        let _ = writeln!(
            text,
            "| `{}/llvm{}` | `{}` | {:.2} | {} |",
            row.source, row.llvm, row.test, row.seconds, row.ended
        );
    }
    text
}

/// The report: counts, differences with their explanations, the tests of
/// one corpus only, and the times.
pub fn render(
    comparison: &Comparison,
    explained: &BTreeMap<String, Explained>,
    times: &[TimeRow],
    names: (&str, &str),
    limit_seconds: f64,
) -> String {
    let (here, there) = names;
    let mut text = String::new();
    let _ = writeln!(text, "### Entries of both corpora\n");
    let _ = writeln!(
        text,
        "| group | {here} entries | {there} entries | in both | same behaviour | different |"
    );
    let _ = writeln!(text, "| --- | ---: | ---: | ---: | ---: | ---: |");
    let mut total = [0; 4];
    for (group, counts) in &comparison.groups {
        for (sum, count) in total.iter_mut().zip(counts) {
            *sum += count;
        }
        let [a, b, both, same] = counts;
        let _ = writeln!(text, "| `{group}` | {a} | {b} | {both} | {same} | {} |", both - same);
    }
    let [a, b, both, same] = total;
    let _ = writeln!(text, "| all | {a} | {b} | {both} | {same} | {} |", both - same);

    let _ = writeln!(text, "\n### Differences\n");
    let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
    for difference in &comparison.differences {
        let class = explained.get(&difference.test).map_or("unknown", |e| e.class.as_str());
        *by_class.entry(class).or_default() += 1;
    }
    let _ = writeln!(text, "| class | tests | meaning |");
    let _ = writeln!(text, "| --- | ---: | --- |");
    for (class, meaning) in CLASSES {
        if let Some(count) = by_class.get(class) {
            let _ = writeln!(text, "| `{class}` | {count} | {meaning} |");
        }
    }
    let _ = writeln!(text, "\n| test | LLVM | what differs ({here} first) | class | cause |");
    let _ = writeln!(text, "| --- | --- | --- | --- | --- |");
    for difference in &comparison.differences {
        let (class, cause) = explained
            .get(&difference.test)
            .map_or(("unknown", ""), |e| (e.class.as_str(), e.cause.as_str()));
        let llvm: Vec<String> = difference.llvm.iter().map(u32::to_string).collect();
        let _ = writeln!(
            text,
            "| `{}` | {} | {} | `{class}` | {} |",
            difference.test,
            llvm.join(", "),
            difference.what,
            cause.replace('|', "\\|")
        );
    }

    for (title, list, missing_in) in [
        (format!("Entries of the {here} corpus only"), &comparison.only_here, there),
        (format!("Entries of the {there} corpus only"), &comparison.only_there, here),
    ] {
        let _ = writeln!(text, "\n### {title}\n");
        let mut reasons: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for (group, _, reason) in list {
            *reasons.entry((group.as_str(), reason.as_str())).or_default() += 1;
        }
        let _ = writeln!(text, "| group | why the {missing_in} corpus has none | tests |");
        let _ = writeln!(text, "| --- | --- | ---: |");
        for ((group, reason), count) in &reasons {
            let _ = writeln!(text, "| `{group}` | `{reason}` | {count} |");
        }
        if !list.is_empty() {
            let _ = writeln!(text, "\n<details><summary>{} tests</summary>\n", list.len());
            for (group, test, reason) in list {
                let _ = writeln!(text, "- `{group}` `{test}`: `{reason}`");
            }
            let _ = writeln!(text, "\n</details>");
        }
    }
    if !times.is_empty() {
        let _ = writeln!(text, "\n### How long the {here} programs ran\n");
        text.push_str(&render_times(times, limit_seconds));
    }
    text
}

/// The differences no line of the table explains.
pub fn unexplained<'a>(
    comparison: &'a Comparison,
    explained: &BTreeMap<String, Explained>,
) -> Vec<&'a Difference> {
    comparison.differences.iter().filter(|d| !explained.contains_key(&d.test)).collect()
}

/// The table of explained differences when none is given.
pub fn default_differences(target: Option<Arch>) -> PathBuf {
    let name = match super::other_target(target) {
        Some(Arch::X86_64) => "corpus-x64-differences.tsv",
        Some(Arch::Aarch64) => "corpus-a64-differences.tsv",
        None => "corpus-differences.tsv",
    };
    crate::irdiff::workspace_root().join("docs").join("status").join(name)
}

/// `cargo xtask corpus compare`. Exit status: 0; 1 when a difference is
/// not explained; 2 for a usage error or a corpus that cannot be read.
pub fn command(args: &[String], build_dir: &Path, target: Option<Arch>) -> u8 {
    let mut here = super::default_dir_for(build_dir, target);
    let mut there = super::default_dir(build_dir);
    let mut table = default_differences(target);
    let (mut given, mut index) = (false, 0);
    let usage = |message: &str| {
        eprint!("xtask corpus: {message}\n{}", super::USAGE);
        2
    };
    while index < args.len() {
        let arg = args[index].as_str();
        let mut next = || {
            index += 1;
            args.get(index).map(|value| super::absolute(Path::new(value)))
        };
        match arg {
            "--against" => match next() {
                Some(dir) => there = dir,
                None => return usage("--against needs a value"),
            },
            "--differences" => match next() {
                Some(file) => table = file,
                None => return usage("--differences needs a value"),
            },
            other if other.starts_with('-') => return usage(&format!("unknown option '{other}'")),
            other if !given => {
                here = super::absolute(Path::new(other));
                given = true;
            }
            other => return usage(&format!("more than one corpus directory: '{other}'")),
        }
        index += 1;
    }
    let explained = match fs::read_to_string(&table) {
        Ok(text) => match parse_differences(&text) {
            Ok(explained) => explained,
            Err(message) => {
                eprintln!("xtask corpus: '{}': {message}", table.display());
                return 2;
            }
        },
        Err(_) => BTreeMap::new(),
    };
    let comparison = match compare(&here, &there) {
        Ok(comparison) => comparison,
        Err(message) => {
            eprintln!("xtask corpus: {message}");
            return 2;
        }
    };
    let name = |dir: &Path| dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let limit = capture::Options::new(PathBuf::new()).run_limits.time.as_secs_f64()
        * f64::from(crate::irdiff::engine::EMULATION_SLOWDOWN);
    let times = capture::read_times(&here);
    print!("{}", render(&comparison, &explained, &times, (&name(&here), &name(&there)), limit));
    let open = unexplained(&comparison, &explained);
    for difference in &open {
        eprintln!("xtask corpus: not explained in '{}': {}", table.display(), difference.test);
    }
    u8::from(!open.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "corpus", "compare": the table of explanations.
    #[test]
    fn differences_table() {
        let text = "# comment\n\nkiln tests/a.cpp\ttarget\tsizeof(long double)\n";
        let table = parse_differences(text).unwrap();
        assert_eq!(table["kiln tests/a.cpp"].class, "target");
        assert!(parse_differences("t\tbogus\tx\n").is_err());
        assert!(parse_differences("t\ttarget\n").is_err());
        assert!(parse_differences("t\ttarget\tx\nt\tkiln\ty\n").is_err());
    }

    // "compare": what a difference says.
    #[test]
    fn descriptions() {
        let b = |code, out: &str| Behaviour { exit: Exit::Code(code), stdout: out.into() };
        assert_eq!(describe(&b(0, "a\nb\n"), &b(1, "a\nb\n")), "exit 0 here, exit 1 there");
        assert_eq!(
            describe(&b(0, "a\n16\n"), &b(0, "a\n8\n")),
            "stdout line 2: `16` here, `8` there"
        );
        assert_eq!(describe(&b(0, "a\n"), &b(0, "a\nb\n")), "stdout line 2: `` here, `b` there");
        assert_eq!(quantile(&[1.0, 2.0, 3.0], 0.5), 2.0);
        assert_eq!(quantile(&[], 0.5), 0.0);
    }
}
