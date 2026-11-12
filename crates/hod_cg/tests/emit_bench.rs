//! Module emission's time and peak memory against `llc` (`spec/codegen.md`
//! §9.7, §11), on the most data-heavy modules of the corpus with their
//! bodies stripped. A measurement, not a gate: `#[ignore]`d, and run
//! optimized with the modules' paths in `HOD_CG_BENCH` (colon-separated):
//!
//! ```text
//! HOD_CG_BENCH=a.ll:b.ll cargo test --release -p hod_cg --test emit_bench -- --ignored --nocapture
//! ```
//!
//! Time is the fastest of several runs: `llc`'s process under GNU `time`,
//! `hod`'s `emit_module` plus the ELF writer in this process (the parse
//! is reported apart). Peak memory is each process's peak resident set:
//! `llc`'s, and a child of this test that parses and emits once, less a
//! child that only parses (what emission adds).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::{Options, RelocModel, emit_module};
use hod_ir::Dialect;
use hod_testutil::{Scratch, run_with};

const RUNS: usize = 5;

fn major(path: &Path) -> u32 {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find_map(|part| part.strip_prefix("llvm")?.parse().ok())
        .unwrap_or(22)
}

/// The module's text with every function body stripped.
fn stripped(path: &Path) -> String {
    let source = std::fs::read(path).unwrap();
    let parsed = hod_ll::parse_module(&source, &hod_ll::ParseOptions::default()).unwrap();
    let mut text = String::new();
    parsed.module.print(&mut text, Dialect::new(major(path)).unwrap(), []).unwrap();
    text
}

fn options() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

/// Peak KiB from GNU `time -f "%M"` on the last stderr line.
fn peak_kib(stderr: &str) -> u64 {
    stderr.lines().last().and_then(|line| line.trim().parse().ok()).unwrap_or(0)
}

/// Wall seconds and peak KiB of `command` under GNU `time`.
fn timed(command: &Command) -> (f64, u64) {
    let mut timed = Command::new("/usr/bin/time");
    timed.args(["-f", "%M"]).arg(command.get_program()).args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => timed.env(key, value),
            None => timed.env_remove(key),
        };
    }
    let start = Instant::now();
    let outcome = run_with(&mut timed, &[], Duration::from_mins(5));
    let wall = start.elapsed().as_secs_f64();
    assert!(outcome.success(), "{}", outcome.stderr_text());
    (wall, peak_kib(&outcome.stderr_text()))
}

/// The child: parse (and unless `HOD_CG_BENCH_PARSE_ONLY`) emit and write
/// the module `HOD_CG_BENCH_CHILD` names.
#[test]
#[ignore = "a child process of `emit_time_and_memory`"]
fn emit_child() {
    let Some(path) = std::env::var_os("HOD_CG_BENCH_CHILD") else { return };
    let source = std::fs::read(&path).unwrap();
    let parsed = hod_ll::parse_module(&source, &hod_ll::ParseOptions::default()).unwrap();
    if std::env::var_os("HOD_CG_BENCH_PARSE_ONLY").is_none() {
        let emitted = emit_module(&parsed.module, &options(), &[]).unwrap();
        let bytes = hod_obj::elf::to_vec(&emitted.object).unwrap();
        assert!(!bytes.is_empty());
    }
}

#[test]
#[ignore = "a measurement: run with HOD_CG_BENCH"]
fn emit_time_and_memory() {
    let Some(list) = std::env::var_os("HOD_CG_BENCH") else {
        eprintln!("skipped: HOD_CG_BENCH is not set");
        return;
    };
    let scratch = Scratch::new("emit-bench");
    let me = std::env::current_exe().unwrap();
    for (index, path) in std::env::split_paths(&list).enumerate() {
        let major = major(&path);
        let text = stripped(&path);
        let input = scratch.write(&format!("m{index}.ll"), &text);
        let output = scratch.join(format!("m{index}.o"));
        // hod, in process.
        let mut parse = f64::INFINITY;
        let mut emit = f64::INFINITY;
        let mut size = 0;
        for _ in 0..RUNS {
            let start = Instant::now();
            let parsed =
                hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).unwrap();
            let parsed_at = Instant::now();
            let emitted = emit_module(&parsed.module, &options(), &[]).unwrap();
            let bytes = hod_obj::elf::to_vec(&emitted.object).unwrap();
            let end = Instant::now();
            parse = parse.min((parsed_at - start).as_secs_f64());
            emit = emit.min((end - parsed_at).as_secs_f64());
            size = bytes.len();
        }
        let child = |parse_only: bool| {
            let mut command = Command::new(&me);
            command
                .args(["--ignored", "--exact", "emit_child", "--test-threads=1"])
                .env("HOD_CG_BENCH_CHILD", &input);
            if parse_only {
                command.env("HOD_CG_BENCH_PARSE_ONLY", "1");
            }
            (0..3).map(|_| timed(&command_clone(&command)).1).min().unwrap()
        };
        let rss_all = child(false);
        let rss_parse = child(true);
        // llc.
        let mut llc_time = f64::INFINITY;
        let mut llc_rss = u64::MAX;
        for _ in 0..RUNS {
            let mut command = Command::new(PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc")));
            command
                .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"])
                .args(["-filetype=obj"])
                .arg(&input)
                .arg("-o")
                .arg(&output);
            let (seconds, kib) = timed(&command);
            llc_time = llc_time.min(seconds);
            llc_rss = llc_rss.min(kib);
        }
        let llc_size = std::fs::metadata(&output).map_or(0, |m| m.len());
        let name = path.file_name().unwrap().to_string_lossy();
        eprintln!(
            "emit_bench: {name} (LLVM {major}, {} KiB of text): hod emit+write {:.2} ms (parse {:.2} ms), peak {} KiB (+{} KiB over the parse); llc {:.0} ms, peak {} KiB; object {} vs {} bytes",
            text.len() / 1024,
            emit * 1e3,
            parse * 1e3,
            rss_all,
            rss_all.saturating_sub(rss_parse),
            llc_time * 1e3,
            llc_rss,
            size,
            llc_size
        );
    }
}

/// `Command` is not `Clone`.
fn command_clone(command: &Command) -> Command {
    let mut copy = Command::new(command.get_program());
    copy.args(command.get_args());
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            copy.env(key, value);
        }
    }
    copy
}
