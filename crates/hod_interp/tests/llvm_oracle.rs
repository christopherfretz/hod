//! The LLVM oracle (`spec/interp.md` §10): generated programs, compiled by
//! `llc` and run natively, print what the interpreter says they do.
//!
//! Each batch of programs is one file of LLVM assembly (see `llvm_text`)
//! compiled twice, at `-O0` (by the selector of §10.1) and through
//! `opt -O2` and `llc -O2`, linked with `gcc` and run. The output of both
//! must be the interpreter's trace of `sink` calls and result, program by
//! program.
//!
//! Programs that throw (§5.9) are linked with `g++` and a C++ shim that
//! defines `hod_throw` and `hod_catch` over the Itanium ABI, and are
//! checked with the `llc` of each LLVM major installed (§10.2).
//!
//! `HOD_ORACLE_KEEP=<directory>` keeps every file compiled there.
//!
//! The tools are found with `hod_testutil::tool`: a missing one makes a
//! test skip with a note, or fail under `HOD_REQUIRE_TOOLS=1`. So does a
//! host the generator has no target for.

mod eh_cases;
mod llvm_text;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use hod_interp::generate::{Generated, Options, generate};
use hod_ir::TargetSpec;
use hod_testutil::{LLVM_MAJORS, Scratch, find_tool, run_with, tool, tools_required};

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
    /// The C compiler that links, or the C++ one with `shim`.
    cc: PathBuf,
    /// The C++ exception shim's object, linked into every program.
    shim: Option<PathBuf>,
}

fn tools() -> Option<Tools> {
    Some(Tools { llc: tool("llc")?, opt: tool("opt")?, cc: tool("gcc")?, shim: None })
}

/// `hod_throw` and `hod_catch` (`spec/interp.md` §5.9) over the Itanium
/// C++ ABI.
const SHIM: &str = r#"#include <cstdint>
#include <typeinfo>
#include <cxxabi.h>
extern "C" void hod_throw(const std::type_info *type, int64_t payload) {
  void *object = __cxxabiv1::__cxa_allocate_exception(sizeof(int64_t));
  *static_cast<int64_t *>(object) = payload;
  __cxxabiv1::__cxa_throw(object, const_cast<std::type_info *>(type), nullptr);
}
extern "C" int64_t hod_catch(void *exception) {
  int64_t payload = *static_cast<int64_t *>(__cxxabiv1::__cxa_begin_catch(exception));
  __cxxabiv1::__cxa_end_catch();
  return payload;
}
"#;

/// The tools of LLVM `major` with `g++` and the shim built in `scratch`,
/// or `None` (with a note) if that major is not installed.
fn eh_tools(major: u32, scratch: &Scratch) -> Option<Tools> {
    let versioned = |name: &str| {
        let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
        if installed.is_file() {
            return Some(installed);
        }
        find_tool(&format!("{name}-{major}")).filter(|path| path.is_file())
    };
    let (Some(llc), Some(opt)) = (versioned("llc"), versioned("opt")) else {
        eprintln!("note: LLVM {major} is not installed; skipping it");
        return None;
    };
    let cxx = tool("g++")?;
    let source = scratch.write("shim.cc", SHIM);
    let shim = scratch.join("shim.o");
    must(Command::new(&cxx).args(["-c", "-O1"]).arg(&source).arg("-o").arg(&shim), "g++ shim");
    Some(Tools { llc, opt, cc: cxx, shim: Some(shim) })
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
        let mut llc = Command::new(&tools.llc);
        llc.args([level, "-filetype=obj", "-relocation-model=pic"]);
        if level == "-O0" {
            // The selector `-O2` uses. For AArch64 `llc -O0` has two of
            // its own, and each miscompiles a correct program
            // (`spec/interp.md` §10.1).
            llc.args(["-global-isel=0", "-fast-isel=0"]);
        }
        must(llc.arg("-o").arg(&object).arg(&input), "llc");
        let binary = scratch.join(format!("batch{level}"));
        // `frem` is a call of `fmod`.
        let mut link = Command::new(&tools.cc);
        link.arg(&object).args(&tools.shim);
        must(link.arg("-o").arg(&binary).arg("-lm"), "link");
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
    check_with(&tools, &options, seeds, per_file)
}

/// As [`check`], with `tools`.
fn check_with(
    tools: &Tools,
    options: &Options,
    seeds: std::ops::Range<u64>,
    per_file: usize,
) -> usize {
    let seeds: Vec<u64> = seeds.collect();
    let mut checked = 0;
    for chunk in seeds.chunks(per_file) {
        let programs: Vec<Generated> = chunk.iter().map(|&seed| generate(seed, options)).collect();
        checked += check_programs(tools, options.target.triple(), chunk, &programs);
    }
    checked
}

/// Checks `programs`, labelled `labels`, as one file. Returns how many
/// were checked.
fn check_programs(tools: &Tools, triple: &str, labels: &[u64], programs: &[Generated]) -> usize {
    let chunk = labels;
    let mut file = llvm_text::File::new();
    let mut expected = Vec::new();
    for (&seed, program) in chunk.iter().zip(programs) {
        let outcome = program.run();
        assert!(outcome.result.is_ok(), "seed {seed}: {:?}", outcome.result);
        file.add(seed, program);
        expected.push(llvm_text::expected(seed, &outcome));
    }
    let scratch = Scratch::new("llvm-oracle");
    let text = file.finish(triple);
    // For looking at what was compiled.
    if let Some(dir) = std::env::var_os("HOD_ORACLE_KEEP") {
        let name = format!("batch-{}-{}.ll", chunk[0], chunk.len());
        std::fs::write(PathBuf::from(dir).join(name), &text).expect("write");
    }
    let printed = compile_and_run(tools, &scratch, &text);
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
            std::fs::write(&kept, alone.finish(triple)).expect("write");
            panic!(
                "seed {seed} at {level}: LLVM and the interpreter disagree\n\
                     interpreter:\n{want}LLVM:\n{}\nprogram kept in {}",
                got.get(at).map_or("(nothing)", String::as_str),
                kept.display()
            );
        }
        assert_eq!(got.len(), expected.len(), "{level}: more output than programs");
    }
    labels.len()
}

/// For each LLVM major installed: `check` with its tools and the C++
/// shim. Returns the count per major.
fn per_major(mut check: impl FnMut(&Tools, &TargetSpec) -> usize) -> Vec<(u32, usize)> {
    let Some(target) = host_target() else { return Vec::new() };
    let scratch = Scratch::new("llvm-oracle-eh");
    let mut counts = Vec::new();
    for major in LLVM_MAJORS {
        if let Some(tools) = eh_tools(major, &scratch) {
            counts.push((major, check(&tools, &target)));
        }
    }
    assert!(!tools_required() || !counts.is_empty(), "no LLVM major (HOD_REQUIRE_TOOLS=1)");
    counts
}

/// §5.5, §5.9, §10.2: the hand-written programs that unwind or branch
/// through block addresses agree with LLVM.
#[test]
fn unwinding_cases_agree_with_llvm() {
    let counts = per_major(|tools, target| {
        let cases = eh_cases::cases(target);
        let labels: Vec<u64> = (0..cases.len() as u64).collect();
        check_programs(tools, target.triple(), &labels, &cases)
    });
    for (major, count) in counts {
        println!("LLVM {major}: {count} unwinding and computed-goto cases agree with llc at -O0 and -O2");
    }
}

/// §5.9, §10.2: an exception without a handler: the interpreter's
/// `uncaught-exception` is `std::terminate`, which aborts before any
/// cleanup runs.
#[test]
fn uncaught_is_terminate() {
    let counts = per_major(|tools, target| {
        let program = eh_cases::uncaught(target);
        let outcome = program.run();
        assert_eq!(
            outcome.result,
            Err(hod_interp::Stop::Trap(hod_interp::Trap::UncaughtException))
        );
        let mut file = llvm_text::File::new();
        file.add(0, &program);
        let scratch = Scratch::new("llvm-oracle");
        // What is printed before the abort must reach the pipe: flush
        // after every line.
        let printf = |format: &str| format!("@printf(ptr @fmt_{format}, i64 ");
        let mut text = file.finish(target.triple());
        for format in ["p", "s"] {
            let at = text.find(&printf(format)).expect("a printf");
            let end = at + text[at..].find('\n').expect("a line");
            text.insert_str(end + 1, "  call i32 @fflush(ptr null)\n");
        }
        for output in compile_and_run(tools, &scratch, &text) {
            // SIGABRT, and no `s` line: no cleanup ran.
            assert_eq!(output, "p 0\nexited with Some(ExitStatus(unix_wait_status(6)))\n");
        }
        1
    });
    for (major, _) in counts {
        println!("LLVM {major}: an uncaught exception aborts without cleanups");
    }
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
