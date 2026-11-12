//! The LLVM oracle (`spec/interp.md` §10): generated programs, compiled by
//! `llc` and run natively, print what the interpreter says they do.
//!
//! Each batch of programs is one file of LLVM assembly (see `llvm_text`)
//! compiled twice, at `-O0` and through `opt -O2` and `llc -O2`, linked
//! with `gcc` and run. The output of both must be the interpreter's trace
//! of `sink` calls and result, program by program.
//!
//! `HOD_ORACLE_KEEP=<directory>` keeps every file compiled there.
//!
//! The tools are found with `hod_testutil::tool`: a missing one makes a
//! test skip with a note, or fail under `HOD_REQUIRE_TOOLS=1`. So does a
//! host the generator has no target for.

mod llvm_text;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use hod_interp::generate::{Generated, Options, generate};
use hod_ir::TargetSpec;
use hod_testutil::{Scratch, run_with, tool, tools_required};

/// The module target that is the machine the tests run on.
fn host_target() -> Option<TargetSpec> {
    if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        Some(TargetSpec::aarch64_linux_gnu())
    } else if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        Some(TargetSpec::x86_64_linux_gnu())
    } else {
        assert!(!tools_required(), "no native target for this host (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: skipping: the LLVM oracle runs natively on Linux only");
        None
    }
}

struct Tools {
    llc: PathBuf,
    opt: PathBuf,
    gcc: PathBuf,
}

fn tools() -> Option<Tools> {
    Some(Tools { llc: tool("llc")?, opt: tool("opt")?, gcc: tool("gcc")? })
}

/// Runs a tool that must succeed.
#[track_caller]
fn must(command: &mut Command, what: &str) -> Vec<u8> {
    let outcome = run_with(command, b"", Duration::from_mins(5));
    assert!(outcome.success(), "{what} failed ({:?}):\n{}", outcome.status, outcome.stderr_text());
    outcome.stdout
}

/// Compiles `text` at `-O0` and at `-O2`, runs both, and returns what
/// each printed.
fn compile_and_run(tools: &Tools, scratch: &Scratch, text: &str) -> [String; 2] {
    let source = scratch.write("batch.ll", text);
    let mut printed = Vec::new();
    for level in ["-O0", "-O2"] {
        let input = if level == "-O0" {
            source.clone()
        } else {
            let optimized = scratch.join("batch.opt.bc");
            must(Command::new(&tools.opt).arg(level).arg(&source).arg("-o").arg(&optimized), "opt");
            optimized
        };
        let object = scratch.join(format!("batch{level}.o"));
        must(
            Command::new(&tools.llc)
                .args([level, "-filetype=obj", "-relocation-model=pic", "-o"])
                .arg(&object)
                .arg(&input),
            "llc",
        );
        let binary = scratch.join(format!("batch{level}"));
        // `frem` is a call of `fmod`.
        must(Command::new(&tools.gcc).arg(&object).arg("-o").arg(&binary).arg("-lm"), "gcc");
        let ran = run_with(&mut Command::new(&binary), b"", Duration::from_mins(2));
        let mut output = ran.stdout_text();
        if !ran.success() {
            writeln!(output, "exited with {:?}", ran.status).expect("write to a string");
        }
        printed.push(output);
    }
    [printed.remove(0), printed.remove(0)]
}

/// The lines of `output` for each program: from a `p` line to the next.
fn split(output: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();
    for line in output.lines() {
        if line.starts_with("p ") || programs.is_empty() {
            programs.push(String::new());
        }
        let last = programs.last_mut().expect("just pushed");
        last.push_str(line);
        last.push('\n');
    }
    programs
}

/// Checks the programs of `seeds` under `options` against LLVM, `per_file`
/// to a file. Returns how many were checked.
fn check(options: &Options, seeds: std::ops::Range<u64>, per_file: usize) -> usize {
    let (Some(target), Some(tools)) = (host_target(), tools()) else { return 0 };
    let options = Options { target, ..options.clone() };
    let seeds: Vec<u64> = seeds.collect();
    let mut checked = 0;
    for chunk in seeds.chunks(per_file) {
        let programs: Vec<Generated> = chunk.iter().map(|&seed| generate(seed, &options)).collect();
        let mut file = llvm_text::File::new();
        let mut expected = Vec::new();
        for (&seed, program) in chunk.iter().zip(&programs) {
            let outcome = program.run();
            assert!(outcome.result.is_ok(), "seed {seed}: {:?}", outcome.result);
            file.add(seed, program);
            expected.push(llvm_text::expected(seed, &outcome));
        }
        let scratch = Scratch::new("llvm-oracle");
        let text = file.finish(options.target.triple());
        // For looking at what was compiled.
        if let Some(dir) = std::env::var_os("HOD_ORACLE_KEEP") {
            let name = format!("batch-{}-{}.ll", chunk[0], chunk.len());
            std::fs::write(PathBuf::from(dir).join(name), &text).expect("write");
        }
        let printed = compile_and_run(&tools, &scratch, &text);
        for (level, output) in ["-O0", "-O2"].into_iter().zip(&printed) {
            let got = split(output);
            for (at, want) in expected.iter().enumerate() {
                if got.get(at) == Some(want) {
                    continue;
                }
                // Keep the one program that disagrees.
                let seed = chunk[at];
                let mut alone = llvm_text::File::new();
                alone.add(seed, &programs[at]);
                let kept = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                    .join(format!("llvm-oracle-seed-{seed}.ll"));
                std::fs::write(&kept, alone.finish(options.target.triple())).expect("write");
                panic!(
                    "seed {seed} at {level}: LLVM and the interpreter disagree\n\
                     interpreter:\n{want}LLVM:\n{}\nprogram kept in {}",
                    got.get(at).map_or("(nothing)", String::as_str),
                    kept.display()
                );
            }
            assert_eq!(got.len(), expected.len(), "{level}: more output than programs");
        }
        checked += chunk.len();
    }
    checked
}

/// §10: the default options, a few dozen seeds.
#[test]
fn default_programs_agree_with_llvm() {
    let checked = check(&Options::default(), 0..48, 48);
    println!("{checked} programs agree with llc at -O0 and -O2");
}

/// §10: larger functions, and more of them.
#[test]
fn larger_programs_agree_with_llvm() {
    let checked = check(&Options::default().sized(8, 300), 1000..1006, 6);
    println!("{checked} programs agree with llc at -O0 and -O2");
}

/// §10: many seeds at several sizes. Minutes.
///
/// `HOD_ORACLE_SEEDS` sets how many seeds each size gets (default 2000).
#[test]
#[ignore = "minutes; run with --ignored"]
fn many_programs_agree_with_llvm() {
    let seeds: u64 =
        std::env::var("HOD_ORACLE_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(2000);
    let mut checked = 0;
    checked += check(&Options::default(), 10_000..10_000 + seeds, 100);
    checked += check(&Options::default().sized(2, 12), 20_000..20_000 + seeds, 250);
    checked += check(&Options::default().sized(6, 400), 30_000..30_000 + seeds / 10, 20);
    checked += check(&Options::default().sized(3, 3000), 40_000..40_000 + seeds / 50, 4);
    println!("{checked} programs agree with llc at -O0 and -O2");
}
