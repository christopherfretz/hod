//! What a benchmark run says to a person: the summary on standard output
//! and `docs/status/perf.md`. Contract: `spec/harness.md` "bench",
//! "Summary" and "The report".

use std::fmt::Write as _;

use super::check::{mib, millis, percent, table, times};
use super::record::{Ratio, Record, Results, Status, Tool};

/// Bytes as KiB, one decimal.
pub fn kib(bytes: u64) -> String {
    format!("{}.{}", bytes / 1024, bytes % 1024 * 10 / 1024)
}

/// Bytes per second as MB/s (10^6 bytes), two decimals.
pub fn mb_per_s(bytes_per_s: u64) -> String {
    format!("{}.{:02}", bytes_per_s / 1_000_000, bytes_per_s % 1_000_000 / 10_000)
}

fn cell<T>(value: Option<T>, show: impl Fn(T) -> String) -> String {
    value.map_or_else(|| "-".to_owned(), show)
}

fn ratio(value: Option<Ratio>) -> String {
    cell(value, |r| times(r.0))
}

fn spread(record: &Record) -> String {
    cell(record.spread_bp(), |bp| percent(Some(bp as i64)).trim_start_matches('+').to_owned())
}

/// The inputs of a run, in its order, each once.
fn inputs(results: &Results) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for record in &results.records {
        if !names.contains(&record.input.as_str()) {
            names.push(&record.input);
        }
    }
    names
}

/// What hod in strict mode did with an input, in words. A decline is not
/// a time: it is named, with its reason, and what declining cost.
pub fn strict_outcome(record: Option<&Record>) -> String {
    let Some(record) = record else { return "-".to_owned() };
    match record.status {
        Status::Ok => "built".to_owned(),
        Status::Fallback => "fallback".to_owned(),
        Status::Error => "ERROR".to_owned(),
        Status::Declined => {
            let cost = match (record.wall_us_min, record.rss_kib) {
                (Some(us), Some(kib)) => format!(" (in {} ms, {} MiB)", millis(us), mib(kib)),
                (Some(us), None) => format!(" (in {} ms)", millis(us)),
                _ => String::new(),
            };
            format!(
                "declined: {}{cost}",
                if record.detail.is_empty() { "?" } else { &record.detail }
            )
        }
    }
}

/// The header and rows of the table of `llc`'s numbers.
fn llc_table(results: &Results) -> (Vec<String>, Vec<Vec<String>>) {
    let header = [
        "input",
        "class",
        "IR KiB",
        "functions",
        "ms",
        "spread",
        "MiB",
        "object KiB",
        "IR MB/s",
        "functions/s",
    ];
    let mut rows = Vec::new();
    for record in results.records.iter().filter(|r| matches!(r.tool, Tool::Llc | Tool::Generate)) {
        let failed = record.status == Status::Error;
        rows.push(vec![
            record.input.clone(),
            record.class.name().to_owned(),
            cell(record.ir_bytes, kib),
            cell(record.functions, |n| n.to_string()),
            if failed { "ERROR".to_owned() } else { cell(record.wall_us_min, millis) },
            spread(record),
            cell(record.rss_kib, mib),
            cell(record.object_bytes, kib),
            cell(record.ir_bytes_per_s(), mb_per_s),
            cell(record.functions_per_s(), |n| n.to_string()),
        ]);
    }
    (header.iter().map(|&name| name.to_owned()).collect(), rows)
}

/// The header and rows of the table of hod's numbers.
fn hod_table(results: &Results) -> (Vec<String>, Vec<Vec<String>>) {
    let header = [
        "input",
        "hod ms",
        "x llc",
        "MiB",
        "x llc",
        "object",
        "hod+fallback ms",
        "x llc",
        "MiB",
        "x llc",
        "hod",
    ];
    let mut rows = Vec::new();
    for input in inputs(results) {
        let strict = results.find(input, Tool::Hod);
        let hybrid = results.find(input, Tool::HodFallback);
        if strict.is_none() && hybrid.is_none() {
            continue;
        }
        // Only an object hod built itself is a time of hod's.
        let built = strict.filter(|record| record.status == Status::Ok);
        let produced = hybrid.filter(|record| record.produced());
        let mut row = vec![input.to_owned()];
        row.extend([
            cell(built.and_then(|r| r.wall_us_min), millis),
            ratio(built.and_then(|r| r.wall_ratio)),
            cell(built.and_then(|r| r.rss_kib), mib),
            ratio(built.and_then(|r| r.rss_ratio)),
            ratio(built.and_then(|r| r.size_ratio)),
        ]);
        row.extend([
            match hybrid {
                Some(record) if record.status == Status::Error => "ERROR".to_owned(),
                _ => cell(produced.and_then(|r| r.wall_us_min), millis),
            },
            ratio(produced.and_then(|r| r.wall_ratio)),
            cell(produced.and_then(|r| r.rss_kib), mib),
            ratio(produced.and_then(|r| r.rss_ratio)),
        ]);
        row.push(strict_outcome(strict));
        rows.push(row);
    }
    (header.iter().map(|&name| name.to_owned()).collect(), rows)
}

/// The sum of the fastest runs of `tool` and of `llc`, over the inputs
/// both produced an object for.
fn totals(results: &Results, tool: Tool) -> Option<(u64, u64)> {
    let mut sums = None;
    for record in results.records.iter().filter(|r| r.tool == tool && r.produced()) {
        let llc = results.find(&record.input, Tool::Llc).filter(|llc| llc.status == Status::Ok);
        if let Some((own, reference)) = record.wall_us_min.zip(llc.and_then(|llc| llc.wall_us_min))
        {
            let (a, b) = sums.unwrap_or((0, 0));
            sums = Some((a + own, b + reference));
        }
    }
    sums
}

/// The middle and the largest spread of `llc`'s times, with the input of
/// the largest: what this machine's timing was worth during the run.
pub fn noise(results: &Results) -> Option<(u64, u64, &str)> {
    let mut spreads: Vec<(u64, &str)> = results
        .records
        .iter()
        .filter(|record| record.tool == Tool::Llc)
        .filter_map(|record| Some((record.spread_bp()?, record.input.as_str())))
        .collect();
    spreads.sort_unstable();
    let &(largest, input) = spreads.last()?;
    Some((spreads[spreads.len() / 2].0, largest, input))
}

fn plain_percent(bp: u64) -> String {
    percent(Some(bp as i64)).trim_start_matches('+').to_owned()
}

/// How the run was measured, one fact per item.
fn method(results: &Results) -> Vec<String> {
    let get = |key: &str| results.get(key).filter(|value| !value.is_empty());
    let mut facts = Vec::new();
    if let Some(runs) = get("runs") {
        facts.push(format!("times are the fastest of {runs} runs, memory the middle one"));
    }
    if let Some(floor) = get("floor_us").and_then(|us| us.parse().ok()) {
        facts.push(format!(
            "every time includes {} ms of starting a measured process",
            millis(floor)
        ));
    }
    match get("rss_error_kib") {
        Some(error) => {
            facts.push(format!("peak memory is off by {error} KiB on a 64 MiB allocation"));
        }
        None if get("usage") == Some("none") => {
            facts.push("no memory or CPU figures (no GNU time)".to_owned());
        }
        None => {}
    }
    if let Some(load) = get("load") {
        facts.push(format!("load average {load}"));
    }
    facts
}

/// The summary of a run for standard output.
pub fn summary(results: &Results, verbose: bool) -> String {
    let get = |key: &str| results.get(key).unwrap_or("?");
    let mut text = format!(
        "bench: {} inputs on {} ({} cores); inputs by {}\n",
        inputs(results).len(),
        get("machine"),
        get("cores"),
        get("producer")
    );
    let facts = method(results);
    if !facts.is_empty() {
        let _ = writeln!(text, "bench: method: {}", facts.join("; "));
    }
    let options = results.get("llc_options").map(|o| format!(" {o}")).unwrap_or_default();
    let target = results.get("target").map(|t| format!(" for {t}")).unwrap_or_default();
    let _ =
        writeln!(text, "bench: llc -O0 -filetype=obj{options} ({}){target}", get("llc_version"));
    let (header, rows) = llc_table(results);
    text.push_str(&table(&header, &rows, &[0, 1]));
    if results.records.iter().any(|r| matches!(r.tool, Tool::Hod | Tool::HodFallback)) {
        let _ = writeln!(text, "bench: hod ({}, {})", get("hod_version"), get("hod_build"));
        let (header, rows) = hod_table(results);
        text.push_str(&table(&header, &rows, &[0, 10]));
    }
    if verbose {
        for record in results.records.iter().filter(|record| !record.phases.is_empty()) {
            let phases: Vec<String> = record
                .phases
                .iter()
                .map(|(name, us)| format!("{name} {} ms", millis(*us)))
                .collect();
            let _ = writeln!(
                text,
                "bench: phases: {} {}: {}",
                record.input,
                record.tool.name(),
                phases.join(", ")
            );
        }
    }
    for record in results.records.iter().filter(|record| record.status == Status::Error) {
        let _ = writeln!(
            text,
            "bench: ERROR {} {}: {}",
            record.input,
            record.tool.name(),
            record.detail
        );
    }
    for (tool, name) in [(Tool::Hod, "hod"), (Tool::HodFallback, "hod+fallback")] {
        if let Some((own, llc)) = totals(results, tool) {
            let _ = writeln!(
                text,
                "bench: sum of the fastest runs: {name} {} ms, llc {} ms: {}",
                millis(own),
                millis(llc),
                ratio(Ratio::of(own, llc))
            );
        }
    }
    if let Some((hod, llc, n)) = instruction_rows(results).1 {
        let _ = writeln!(
            text,
            "bench: instructions under callgrind, over the {n} inputs hod built: hod {} M, llc {} M: {}",
            millions(hod),
            millions(llc),
            ratio(Ratio::of(hod, llc))
        );
    }
    if let Some((median, largest, input)) = noise(results) {
        let _ = writeln!(
            text,
            "bench: spread of llc's times (middle run over fastest): median {}, largest {} ({input})",
            plain_percent(median),
            plain_percent(largest)
        );
    }
    let count = |tool: Tool, status: Status| {
        results.records.iter().filter(|r| r.tool == tool && r.status == status).count()
    };
    let errors = results.records.iter().filter(|r| r.status == Status::Error).count();
    let _ = writeln!(
        text,
        "bench: llc built {}; hod built {}, declined {}; hod+fallback built {}, fell back {}; {errors} errors",
        count(Tool::Llc, Status::Ok),
        count(Tool::Hod, Status::Ok),
        count(Tool::Hod, Status::Declined),
        count(Tool::HodFallback, Status::Ok),
        count(Tool::HodFallback, Status::Fallback),
    );
    text
}

/// A table as Markdown; the columns named in `text` are left-aligned,
/// the others right-aligned.
fn markdown_table(header: &[String], rows: &[Vec<String>], text: &[usize]) -> String {
    let escape = |cell: &String| cell.replace('|', "\\|");
    let mut out = format!("| {} |\n", header.iter().map(escape).collect::<Vec<_>>().join(" | "));
    let rules: Vec<&str> =
        (0..header.len()).map(|i| if text.contains(&i) { "---" } else { "---:" }).collect();
    let _ = writeln!(out, "|{}|", rules.join("|"));
    for row in rows {
        let _ = writeln!(out, "| {} |", row.iter().map(escape).collect::<Vec<_>>().join(" | "));
    }
    out
}

/// `docs/status/perf.md`: the numbers of a results file, then those of
/// each cross run's (`cross`), each in a section of its own.
pub fn perf_md(results: &Results, cross: &[Results]) -> String {
    let mut text = String::from(
        "# Performance\n\n\
         <!-- Generated by `cargo xtask bench --report` from a results file. Do not edit: \
         run the benchmark and generate it again (spec/harness.md \"bench\", \"The report\"). -->\n\n\
         What `cargo xtask bench` measured: the time, peak memory and output size of turning each\n\
         benchmark input (`bench/manifest`) into an object file, with `llc -O0` and with hod.\n\n\
         **One machine.** These are the numbers of a single development host, which runs other work\n\
         while it is measured. They say what to expect there and how hod compares with `llc` on the\n\
         same input in the same run; they are not portable, and the gate (`cargo xtask bench --check`)\n\
         compares against a baseline recorded on the machine it runs on.\n\n",
    );
    body(results, "##", &mut text);
    for other in cross {
        let triple = other.get("target").unwrap_or("?");
        let arch = triple.split('-').next().unwrap_or(triple);
        let _ = write!(
            text,
            "\n## {arch} objects\n\n\
             A cross run: the same inputs compiled for `{triple}` and built into objects for it, on\n\
             the same machine, by the same tools.\n\n"
        );
        body(other, "###", &mut text);
    }
    text
}

/// One results file's sections, under headings of `level`.
fn body(results: &Results, level: &str, text: &mut String) {
    let get = |key: &str| results.get(key).unwrap_or("?");
    let _ = write!(text, "{level} Where and how\n\n");
    let options = results.get("llc_options").map(|o| format!(" {o}")).unwrap_or_default();
    let mut facts = vec![
        format!("measured: {}", get("date")),
        format!(
            "machine: `{}` ({}, {} cores, {})",
            get("machine"),
            get("cpu"),
            get("cores"),
            get("arch")
        ),
    ];
    if let Some(target) = results.get("target") {
        facts.push(format!("target: `{target}` (a cross run)"));
    }
    facts.extend([
        format!(
            "reference: `llc -O0 -filetype=obj -relocation-model=pic{options}`, {}",
            get("llc_version")
        ),
        format!("hod: {} ({})", get("hod_version"), get("hod_build")),
        format!("inputs produced by: {}", get("producer")),
    ]);
    facts.extend(method(results));
    if let Some((median, largest, input)) = noise(results) {
        facts.push(format!(
            "spread of `llc`'s times, middle run over fastest: median {}, largest {} (`{input}`)",
            plain_percent(median),
            plain_percent(largest)
        ));
    }
    for fact in facts {
        let _ = writeln!(text, "- {fact}");
    }

    let _ = write!(
        text,
        "\n{level} `llc` baselines\n\n\
         What hod has to beat. Time is the fastest run, in milliseconds; spread is how far the middle\n\
         run was above it; memory is the peak resident set; throughput is IR bytes (and function\n\
         definitions) per second of the fastest run. Inputs named `-g` carry debug information.\n\n"
    );
    let (header, rows) = llc_table(results);
    text.push_str(&markdown_table(&header, &rows, &[0, 1]));

    hod_section(results, level, text);
    instructions_section(results, level, text);
    phases_and_errors(results, level, text);
}

/// Instructions as millions, one decimal.
fn millions(count: u64) -> String {
    format!("{}.{}", count / 1_000_000, count % 1_000_000 / 100_000)
}

/// hod's and `llc`'s instructions summed over the inputs hod built, and
/// how many those are.
type InstructionSums = Option<(u64, u64, usize)>;

/// The rows of the instruction counts: per input, `llc`'s and strict
/// hod's, with their ratio; and the sums over the inputs both have.
fn instruction_rows(results: &Results) -> (Vec<Vec<String>>, InstructionSums) {
    let mut rows = Vec::new();
    let mut sums: InstructionSums = None;
    for input in inputs(results) {
        let count = |tool| results.find(input, tool).and_then(|record| record.instructions);
        let (llc, hod) = (count(Tool::Llc), count(Tool::Hod));
        if llc.is_none() && hod.is_none() {
            continue;
        }
        let strict = results.find(input, Tool::Hod);
        let both = llc.zip(hod.filter(|_| strict.is_some_and(Record::produced)));
        if let Some((llc, hod)) = both {
            let (a, b, n) = sums.unwrap_or((0, 0, 0));
            sums = Some((a + hod, b + llc, n + 1));
        }
        rows.push(vec![
            input.to_owned(),
            cell(llc, millions),
            cell(hod, millions),
            ratio(both.and_then(|(llc, hod)| Ratio::of(hod, llc))),
            strict_outcome(strict),
        ]);
    }
    (rows, sums)
}

/// The instruction counts, if the run counted any (`--instructions`).
fn instructions_section(results: &Results, level: &str, text: &mut String) {
    let (rows, sums) = instruction_rows(results);
    if rows.is_empty() {
        return;
    }
    let _ = write!(
        text,
        "\n{level} Instructions\n\n\
         What one run under valgrind's callgrind executed, in millions of instructions: a count\n\
         that does not depend on the machine's load, for comparing two versions of hod. hod runs\n\
         in strict mode with one thread (`HOD_THREADS=1`); a decline's count is what declining\n\
         cost and has no ratio.\n\n"
    );
    let header: Vec<String> =
        ["input", "llc M", "hod M", "x llc", "hod"].iter().map(|&name| name.to_owned()).collect();
    text.push_str(&markdown_table(&header, &rows, &[0, 4]));
    if let Some((hod, llc, n)) = sums {
        let _ = writeln!(
            text,
            "\nSum over the {n} inputs hod built: {} M instructions against {} M for `llc` ({}).",
            millions(hod),
            millions(llc),
            ratio(Ratio::of(hod, llc))
        );
    }
}

/// The `## hod` section of the report, if hod was measured.
fn hod_section(results: &Results, level: &str, text: &mut String) {
    if results.records.iter().any(|r| matches!(r.tool, Tool::Hod | Tool::HodFallback)) {
        let _ = write!(
            text,
            "\n{level} hod\n\n\
             `hod` is hod in strict mode: what it builds by itself. A module it does not support yet is\n\
             a decline, reported with its reason and what declining cost, never as a time for building\n\
             nothing. `hod+fallback` is hod with `llc` as its fallback, what a hybrid build runs: its\n\
             ratio to `llc` is the price of going through hod for a module hod hands on. Ratios are to\n\
             `llc` on the same input in the same run.\n\n"
        );
        let (header, rows) = hod_table(results);
        text.push_str(&markdown_table(&header, &rows, &[0, 10]));
        text.push('\n');
        let count = |tool: Tool, status: Status| {
            results.records.iter().filter(|r| r.tool == tool && r.status == status).count()
        };
        let strict = results.records.iter().filter(|r| r.tool == Tool::Hod).count();
        let _ = writeln!(
            text,
            "hod built {} of {strict} inputs itself and declined {}.",
            count(Tool::Hod, Status::Ok),
            count(Tool::Hod, Status::Declined)
        );
        for (tool, name) in [(Tool::Hod, "`hod`"), (Tool::HodFallback, "`hod+fallback`")] {
            if let Some((own, llc)) = totals(results, tool) {
                let _ = writeln!(
                    text,
                    "Sum of the fastest runs, over the inputs {name} produced an object for: {} ms \
                     against {} ms for `llc` ({}).",
                    millis(own),
                    millis(llc),
                    ratio(Ratio::of(own, llc))
                );
            }
        }
    }
}

/// The `## Phases` and `## Errors` sections, when there are any.
fn phases_and_errors(results: &Results, level: &str, text: &mut String) {
    let with_phases: Vec<&Record> =
        results.records.iter().filter(|record| !record.phases.is_empty()).collect();
    if !with_phases.is_empty() {
        let _ = write!(
            text,
            "\n{level} Phases\n\n\
             Where hod's time went in its fastest run, as hod reports it (spec/cli.md \"Phase report\"),\n\
             in milliseconds.\n\n"
        );
        let header: Vec<String> =
            ["input", "tool", "phases"].iter().map(|&name| name.to_owned()).collect();
        let rows: Vec<Vec<String>> = with_phases
            .iter()
            .map(|record| {
                let phases: Vec<String> = record
                    .phases
                    .iter()
                    .map(|(name, us)| format!("{name} {}", millis(*us)))
                    .collect();
                vec![record.input.clone(), record.tool.name().to_owned(), phases.join(", ")]
            })
            .collect();
        text.push_str(&markdown_table(&header, &rows, &[0, 1, 2]));
    }

    let errors: Vec<&Record> =
        results.records.iter().filter(|record| record.status == Status::Error).collect();
    if !errors.is_empty() {
        let _ = write!(
            text,
            "\n{level} Errors\n\nThese were not measured; the run is not a complete picture.\n\n"
        );
        for record in errors {
            let _ = writeln!(
                text,
                "- `{}` with {}: {}",
                record.input,
                record.tool.name(),
                record.detail
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::record::Class;

    fn run() -> Results {
        let mut results = Results::default();
        for (key, value) in [
            ("date", "2026-11-11T10:00:00Z"),
            ("machine", "aarch64-x-24c"),
            ("cpu", "x"),
            ("cores", "24"),
            ("arch", "aarch64"),
            ("llc_version", "LLVM version 22.1.8"),
            ("hod_version", "hod version 0.0.0"),
            ("hod_build", "workspace, release"),
            ("producer", "clang version 22.1.8"),
            ("runs", "3"),
            ("usage", "gnu-time (/usr/bin/time -f '%M %U %S')"),
            ("floor_us", "3400"),
            ("rss_error_kib", "88"),
            ("load", "13.50 14.00 15.00"),
        ] {
            results.set(key, value);
        }
        let numbers = |tool: Tool, status: Status, input: &str, wall: u64, rss: u64| Record {
            ir_bytes: Some(1_161_604),
            functions: Some(643),
            ir_hash: Some("fnv1a64:0000000000000001".to_owned()),
            runs: 3,
            wall_us_min: Some(wall),
            wall_us_median: Some(wall + wall / 10),
            wall_us_max: Some(wall * 2),
            cpu_us: Some(wall),
            rss_kib: Some(rss),
            rss_kib_min: Some(rss),
            rss_kib_max: Some(rss),
            object_bytes: matches!(status, Status::Ok | Status::Fallback).then_some(402_912),
            ..Record::blank(status, input, Class::Typical, input.ends_with("-g"), tool)
        };
        let llc = numbers(Tool::Llc, Status::Ok, "containers", 170_000, 70_184);
        let mut declined = numbers(Tool::Hod, Status::Declined, "containers", 1_520, 1_948);
        declined.detail = "codegen-not-implemented".to_owned();
        declined.phases = vec![("read".to_owned(), 410), ("total".to_owned(), 520)];
        let mut hybrid =
            numbers(Tool::HodFallback, Status::Fallback, "containers", 173_400, 70_300);
        hybrid.detail = "codegen-not-implemented".to_owned();
        hybrid.set_ratios(&llc);
        let llc_g = numbers(Tool::Llc, Status::Ok, "containers-g", 240_000, 82_472);
        let mut built = numbers(Tool::Hod, Status::Ok, "containers-g", 24_000, 8_247);
        built.set_ratios(&llc_g);
        let mut broken =
            Record::blank(Status::Error, "containers-g", Class::Typical, true, Tool::HodFallback);
        broken.detail = "hod+fallback exited with status 1: boom".to_owned();
        results.records = vec![llc, declined, hybrid, llc_g, built, broken];
        results
    }

    // spec/harness.md "bench", "Summary": llc's absolutes and throughput;
    // hod as absolutes and ratios; a decline named, not timed as a build;
    // an error named, not a zero.
    #[test]
    fn summary_text() {
        assert_eq!(
            summary(&run(), true),
            "\
bench: 2 inputs on aarch64-x-24c (24 cores); inputs by clang version 22.1.8
bench: method: times are the fastest of 3 runs, memory the middle one; every time includes 3.40 ms of \
starting a measured process; peak memory is off by 88 KiB on a 64 MiB allocation; load average 13.50 14.00 15.00
bench: llc -O0 -filetype=obj (LLVM version 22.1.8)
input         class    IR KiB  functions      ms  spread   MiB  object KiB  IR MB/s  functions/s
containers    typical  1134.3        643  170.00   10.0%  68.5       393.4     6.83         3782
containers-g  typical  1134.3        643  240.00   10.0%  80.5       393.4     4.84         2679
bench: hod (hod version 0.0.0, workspace, release)
input         hod ms  x llc  MiB  x llc  object  hod+fallback ms  x llc   MiB  x llc  hod
containers         -      -    -      -       -           173.40  1.02x  68.6  1.00x  declined: codegen-not-implemented (in 1.52 ms, 1.9 MiB)
containers-g   24.00  0.10x  8.0  0.10x   1.00x            ERROR      -     -      -  built
bench: phases: containers hod: read 0.41 ms, total 0.52 ms
bench: ERROR containers-g hod+fallback: hod+fallback exited with status 1: boom
bench: sum of the fastest runs: hod 24.00 ms, llc 240.00 ms: 0.10x
bench: sum of the fastest runs: hod+fallback 173.40 ms, llc 170.00 ms: 1.02x
bench: spread of llc's times (middle run over fastest): median 10.0%, largest 10.0% (containers-g)
bench: llc built 2; hod built 1, declined 1; hod+fallback built 0, fell back 1; 1 errors
"
        );
        assert!(!summary(&run(), false).contains("bench: phases:"));
    }

    // "The report": docs/status/perf.md is a function of the results file.
    #[test]
    fn perf_report() {
        let text = perf_md(&run(), &[]);
        assert_eq!(text, perf_md(&Results::parse(&run().to_text()).unwrap(), &[]));
        for expected in [
            "# Performance\n",
            "**One machine.**",
            "- measured: 2026-11-11T10:00:00Z\n",
            "- machine: `aarch64-x-24c` (x, 24 cores, aarch64)\n",
            "- reference: `llc -O0 -filetype=obj -relocation-model=pic`, LLVM version 22.1.8\n",
            "- times are the fastest of 3 runs, memory the middle one\n",
            "- peak memory is off by 88 KiB on a 64 MiB allocation\n",
            "| input | class | IR KiB | functions | ms | spread | MiB | object KiB | IR MB/s | functions/s |\n\
             |---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n\
             | containers | typical | 1134.3 | 643 | 170.00 | 10.0% | 68.5 | 393.4 | 6.83 | 3782 |\n",
            "| containers | - | - | - | - | - | 173.40 | 1.02x | 68.6 | 1.00x | \
             declined: codegen-not-implemented (in 1.52 ms, 1.9 MiB) |\n",
            "| containers-g | 24.00 | 0.10x | 8.0 | 0.10x | 1.00x | ERROR | - | - | - | built |\n",
            "hod built 1 of 2 inputs itself and declined 1.\n",
            "## Phases\n",
            "| containers | hod | read 0.41, total 0.52 |\n",
            "## Errors\n",
            "- `containers-g` with hod+fallback: hod+fallback exited with status 1: boom\n",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        // Without hod, phases or errors those sections are not there.
        let mut llc_only = run();
        llc_only.records.retain(|record| record.tool == Tool::Llc);
        let text = perf_md(&llc_only, &[]);
        assert!(
            !text.contains("## hod") && !text.contains("## Phases") && !text.contains("## Errors"),
            "{text}"
        );
    }

    // "The report", "Cross runs", "Instructions": a cross run's results
    // are a section of their own, with its target and llc's options; the
    // instruction counts are a table, with a ratio only for a build.
    #[test]
    fn cross_sections_and_instructions() {
        let mut cross = run();
        cross.set("target", "x86_64-unknown-linux-gnu");
        cross.set("llc_options", "-mtriple=x86_64-unknown-linux-gnu -fast-isel=0");
        for record in &mut cross.records {
            record.instructions = match (record.input.as_str(), record.tool) {
                (_, Tool::Llc) => Some(400_000_000),
                ("containers-g", Tool::Hod) => Some(100_000_000),
                ("containers", Tool::Hod) => Some(2_500_000),
                _ => None,
            };
        }
        let text = perf_md(&run(), std::slice::from_ref(&cross));
        let (host, other) = text.split_once("\n## x86_64 objects\n").expect(&text);
        assert!(!host.contains("Instructions") && !host.contains("target:"), "{host}");
        for expected in [
            "A cross run: the same inputs compiled for `x86_64-unknown-linux-gnu`",
            "### Where and how\n",
            "- target: `x86_64-unknown-linux-gnu` (a cross run)\n",
            "- reference: `llc -O0 -filetype=obj -relocation-model=pic \
             -mtriple=x86_64-unknown-linux-gnu -fast-isel=0`, LLVM version 22.1.8\n",
            "### `llc` baselines\n",
            "### hod\n",
            "### Instructions\n",
            "| input | llc M | hod M | x llc | hod |\n",
            "| containers | 400.0 | 2.5 | - | declined: codegen-not-implemented (in 1.52 ms, 1.9 MiB) |\n",
            "| containers-g | 400.0 | 100.0 | 0.25x | built |\n",
            "Sum over the 1 inputs hod built: 100.0 M instructions against 400.0 M for `llc` (0.25x).\n",
            "### Phases\n",
            "### Errors\n",
        ] {
            assert!(other.contains(expected), "missing {expected:?} in:\n{other}");
        }
        let said = summary(&cross, false);
        assert!(
            said.contains(
                "bench: llc -O0 -filetype=obj -mtriple=x86_64-unknown-linux-gnu -fast-isel=0 \
                 (LLVM version 22.1.8) for x86_64-unknown-linux-gnu\n"
            ),
            "{said}"
        );
        assert!(
            said.contains(
                "bench: instructions under callgrind, over the 1 inputs hod built: \
                 hod 100.0 M, llc 400.0 M: 0.25x\n"
            ),
            "{said}"
        );
        assert!(!summary(&run(), false).contains("instructions"));
    }

    #[test]
    fn units() {
        assert_eq!(
            (kib(0), kib(1536), kib(10_757)),
            ("0.0".to_owned(), "1.5".to_owned(), "10.5".to_owned())
        );
        assert_eq!((mb_per_s(0), mb_per_s(9_205_200)), ("0.00".to_owned(), "9.20".to_owned()));
        assert_eq!(strict_outcome(None), "-");
        let results = run();
        assert_eq!(noise(&results), Some((1000, 1000, "containers-g")));
        assert_eq!(noise(&Results::default()), None);
    }
}
