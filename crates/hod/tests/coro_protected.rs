//! Stack-protected coroutine bodies on the 64-bit targets (WP-343,
//! `spec/codegen.md` §5.6.24 rules 6, 7 and 10, §7.6 "The stubs").
//!
//! The oracle is `opt -passes='default<O0>'` (which splits the
//! coroutine into ramp, resume, destroy and cleanup functions) and `llc
//! -O0 -global-isel=0 -fast-isel=0`. x86-64 programs run under
//! `qemu-x86_64` (unless the host is x86-64), AArch64 programs natively
//! (unless the host is not AArch64); both linked by the target's `g++`.
//! hod's objects: the facade's and the back end's under `Stress::ALL`.
//!
//! - `coro/protected.cpp` (clang 19 and 22, before LLVM's passes, at
//!   `-O0` with `-fstack-protector`, `-strong` and `-all`, and `-O1` with
//!   `-strong`; static and PIE): stack-passed arguments of a protected
//!   body, buffers in the heap frame (their scope holds a suspend),
//!   buffers on the machine stack (clang writes lifetime markers in a
//!   coroutine at `-O0` too; a buffer whose scope holds no suspend stays
//!   in the ramp or in a resumed activation, in `llc`'s split functions
//!   and in hod's body alike). Every run prints what `llc`'s prints and
//!   ends as it does.
//! - `coro/overflow.cpp`: the overflow of a buffer in the ramp and in a
//!   resumed activation ends in `__stack_chk_fail` (SIGABRT, "stack
//!   smashing detected") after the same output as `llc`'s.
//! - `coro/victim.ll`: a protected ramp that uses its ninth argument (on
//!   the stack on both targets) after writing its buffer; its overflow
//!   reaches the incoming argument area. It must abort as `llc`'s does
//!   (which read the argument at entry, below the guard) rather than
//!   store through the overwritten pointer (rule 10: before WP-343 hod
//!   read it in place and died of SIGSEGV on both targets).
#![cfg(target_os = "linux")]

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Outcome, Scratch, run_with, tools_required};

const TARGETS: [Target; 2] = [Target::X86_64, Target::Aarch64];

/// `SIGABRT`: how `__stack_chk_fail` ends a glibc program.
const SIGABRT: i32 = 6;

/// The tools that build and run one target's programs.
struct Lane {
    target: Target,
    cxx: PathBuf,
    /// The emulator, unless the target is the host.
    emulator: Option<PathBuf>,
}

/// A missing tool: `None` with a note, a failure under
/// `HOD_REQUIRE_TOOLS=1`.
fn missing<T>(what: &str) -> Option<T> {
    assert!(!tools_required(), "no {what} (HOD_REQUIRE_TOOLS=1)");
    eprintln!("note: no {what}; skipping its part");
    None
}

fn tool(name: &str) -> Option<PathBuf> {
    hod_testutil::tool(name).or_else(|| missing(name))
}

fn llvm_tool(major: u32, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if path.exists() { Some(path) } else { missing(&format!("{name} {major}")) }
}

fn lane(target: Target) -> Option<Lane> {
    let cxx = tool(&target.cxx_driver())?;
    let emulator = if target.is_host() { None } else { Some(tool(&target.emulator())?) };
    Some(Lane { target, cxx, emulator })
}

fn checked(what: &str, command: &mut Command) -> Outcome {
    let outcome = run_with(command, &[], DEFAULT_TIMEOUT);
    assert!(outcome.success(), "{what}: {}", outcome.stderr_text());
    outcome
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/coro").join(name)
}

fn layout(target: Target) -> &'static str {
    match target {
        Target::X86_64 => hod_ir::X86_64_LINUX_DATA_LAYOUT,
        _ => hod_ir::AARCH64_LINUX_DATA_LAYOUT,
    }
}

/// clang's module of C++ `source` before LLVM's passes for `target`.
fn pre_opt(
    scratch: &Scratch,
    clang: &Path,
    target: Target,
    source: &Path,
    flags: &[&str],
    name: &str,
) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(clang);
    command.arg(format!("--target={}", target.triple()));
    command.args(["-std=c++20", "-S", "-emit-llvm", "-Xclang", "-disable-llvm-passes"]);
    command.args(flags).arg("-o").arg(&out).arg(source);
    checked(&format!("clang {name}"), &mut command);
    std::fs::read_to_string(&out).unwrap()
}

/// The oracle's object of `text`: `opt -passes='default<O0>'`, then
/// `llc` with the reference options.
fn llc_object(
    scratch: &Scratch,
    major: u32,
    target: Target,
    text: &str,
    pic: bool,
    name: &str,
) -> Option<PathBuf> {
    let (opt, llc) = (llvm_tool(major, "opt")?, llvm_tool(major, "llc")?);
    let source = scratch.write(&format!("{name}.in.ll"), text);
    let lowered = scratch.join(format!("{name}.o0.ll"));
    let mut command = Command::new(opt);
    command.arg("-passes=default<O0>").arg("-S").arg(&source).arg("-o").arg(&lowered);
    checked(&format!("opt {name}"), &mut command);
    let out = scratch.join(format!("{name}-llc.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(format!("-mtriple={}", target.triple()));
    if pic {
        command.arg("-relocation-model=pic");
    }
    command.arg(&lowered).arg("-o").arg(&out);
    checked(&format!("llc {name}"), &mut command);
    Some(out)
}

/// The back end's object of `text` under `Stress::ALL`.
fn stressed_object(text: &str, target: Target, pic: bool) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let reloc_model = if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static };
    let options =
        hod_cg::Options { reloc_model, stress: hod_cg::Stress::ALL, ..hod_cg::Options::default() };
    match target {
        Target::X86_64 => hod_x64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("hod_x64: {e}")),
        _ => hod_a64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("hod_a64: {e}")),
    }
}

/// The three objects of `text`: `llc`'s, hod's, hod's stressed.
fn objects(
    scratch: &Scratch,
    major: u32,
    target: Target,
    text: &str,
    pic: bool,
    name: &str,
) -> Option<[(&'static str, PathBuf); 3]> {
    let llc = llc_object(scratch, major, target, text, pic, name)?;
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(target.triple().into()),
        relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
        ..Options::default()
    };
    let plain =
        hod::compile(text.as_bytes(), &options).unwrap_or_else(|e| panic!("hod {name}: {e}"));
    let stress = stressed_object(text, target, pic);
    Some([
        ("llc", llc),
        ("hod", scratch.write(&format!("{name}-hod.o"), plain)),
        ("stress", scratch.write(&format!("{name}-stress.o"), stress)),
    ])
}

fn link(lane: &Lane, object: &Path, pic: bool, exe: &Path) {
    let mut command = Command::new(&lane.cxx);
    command.arg(if pic { "-pie" } else { "-no-pie" }).arg(object).arg("-o").arg(exe);
    checked(&format!("link of {}", exe.display()), &mut command);
}

/// How a run ended: its exit code or its signal, and its output.
#[derive(Debug, PartialEq)]
struct Ended {
    code: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    smashed: bool,
}

fn run(lane: &Lane, exe: &Path, args: &[&str]) -> Ended {
    let mut command = match &lane.emulator {
        Some(emulator) => {
            let mut command = Command::new(emulator);
            command.args(lane.target.emulator_args(&lane.target.sysroot())).arg(exe);
            command
        }
        None => Command::new(exe),
    };
    command.args(args);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    let status = outcome.status.unwrap_or_else(|| panic!("{} timed out", exe.display()));
    Ended {
        code: status.code(),
        signal: status.signal(),
        stdout: outcome.stdout_text(),
        smashed: outcome.stderr_text().contains("stack smashing detected"),
    }
}

/// Links the three `objects` and runs each with every argument list of
/// `cases`; every hod run ends as `llc`'s. Returns `llc`'s ends, one per
/// case, and counts the runs.
fn compare(
    lane: &Lane,
    objects: &[(&str, PathBuf); 3],
    pic: bool,
    cases: &[&[&str]],
    label: &str,
    runs: &mut usize,
) -> Vec<Ended> {
    let exes: Vec<PathBuf> = objects
        .iter()
        .map(|(who, object)| {
            let exe = object.with_extension(format!("{who}.exe"));
            link(lane, object, pic, &exe);
            exe
        })
        .collect();
    let mut wants = Vec::new();
    for args in cases {
        let want = run(lane, &exes[0], args);
        for ((who, _), exe) in objects.iter().zip(&exes).skip(1) {
            let got = run(lane, exe, args);
            assert_eq!(want, got, "{label} {args:?}: {who}'s run against llc's");
            *runs += 1;
        }
        wants.push(want);
    }
    wants
}

/// `coro/protected.cpp` runs as `llc`'s build of it on both targets.
#[test]
fn protected_bodies_run_as_llc_s() {
    let variants: [(&str, &[&str], &str); 4] = [
        ("ssp", &["-O0", "-fstack-protector"], " ssp "),
        ("strong", &["-O0", "-fstack-protector-strong"], " sspstrong "),
        ("all", &["-O0", "-fstack-protector-all"], " sspreq "),
        ("strong-o1", &["-O1", "-fstack-protector-strong"], " sspstrong "),
    ];
    let scratch = Scratch::new("coro-protected");
    for target in TARGETS {
        let Some(lane) = lane(target) else { continue };
        let mut runs = 0;
        for major in LLVM_MAJORS {
            let Some(clang) = llvm_tool(major, "clang++") else { continue };
            for (variant, flags, attribute) in variants {
                for pic in [false, true] {
                    let label = format!(
                        "{}-{major}-{variant}-{}",
                        target.name(),
                        if pic { "pie" } else { "exe" }
                    );
                    let mut all = flags.to_vec();
                    all.push(if pic { "-fPIE" } else { "-fno-pic" });
                    let text =
                        pre_opt(&scratch, &clang, target, &data("protected.cpp"), &all, &label);
                    assert!(text.contains(attribute), "{label}: no{attribute}attribute");
                    assert!(text.contains("llvm.lifetime.start"), "{label}: lifetime markers");
                    let Some(objects) = objects(&scratch, major, target, &text, pic, &label) else {
                        continue;
                    };
                    let ends = compare(&lane, &objects, pic, &[&[]], &label, &mut runs);
                    assert_eq!(ends[0].code, Some(0), "{label}: llc's run: {:?}", ends[0]);
                    assert!(ends[0].stdout.contains("protected: "), "{label}: {:?}", ends[0]);
                }
            }
        }
        eprintln!("{}: {runs} protected coroutine runs equal llc's", target.name());
    }
}

/// `coro/overflow.cpp`: an overflow in the ramp and one in a resumed
/// activation abort in `__stack_chk_fail` after the output `llc`'s build
/// prints; in bounds, the program finishes.
#[test]
fn overflows_abort_where_llc_s_do() {
    let variants: [(&str, &[&str]); 4] = [
        ("ssp", &["-O0", "-fstack-protector"]),
        ("strong", &["-O0", "-fstack-protector-strong"]),
        ("all", &["-O0", "-fstack-protector-all"]),
        ("strong-o1", &["-O1", "-fstack-protector-strong"]),
    ];
    let cases: [&[&str]; 3] = [&["none"], &["ramp"], &["resume"]];
    let scratch = Scratch::new("coro-overflow");
    for target in TARGETS {
        let Some(lane) = lane(target) else { continue };
        let mut runs = 0;
        for major in LLVM_MAJORS {
            let Some(clang) = llvm_tool(major, "clang++") else { continue };
            for (variant, flags) in variants {
                let label = format!("{}-{major}-{variant}-overflow", target.name());
                let mut all = flags.to_vec();
                all.push("-fno-pic");
                let text = pre_opt(&scratch, &clang, target, &data("overflow.cpp"), &all, &label);
                let Some(objects) = objects(&scratch, major, target, &text, false, &label) else {
                    continue;
                };
                let ends = compare(&lane, &objects, false, &cases, &label, &mut runs);
                assert_eq!(ends[0].code, Some(0), "{label} none: {:?}", ends[0]);
                assert!(ends[0].stdout.ends_with("finished\n"), "{label} none: {:?}", ends[0]);
                for (end, wrote) in ends[1..].iter().zip(["ramp wrote", "resume wrote"]) {
                    assert_eq!(end.signal, Some(SIGABRT), "{label}: llc's overflow: {end:?}");
                    assert!(end.smashed, "{label}: llc's overflow: {end:?}");
                    assert!(end.stdout.contains(wrote), "{label}: llc's overflow: {end:?}");
                    assert!(!end.stdout.contains("finished"), "{label}: llc's overflow: {end:?}");
                }
            }
        }
        eprintln!("{}: {runs} coroutine overflow runs equal llc's", target.name());
    }
}

/// `coro/victim.ll` in LLVM `major`'s dialect, for `target`, with the
/// protector `attribute`.
fn victim(major: u32, target: Target, attribute: &str, pic: bool) -> String {
    let mut body = include_str!("data/coro/victim.ll").replace("ATTR", attribute);
    if major < 22 {
        body = body
            .replace("declare void @llvm.coro.end(", "declare i1 @llvm.coro.end(")
            .replace("call void @llvm.coro.end(", "call i1 @llvm.coro.end(")
            .replace("@llvm.lifetime.start.p0(ptr", "@llvm.lifetime.start.p0(i64 16, ptr")
            .replace("@llvm.lifetime.end.p0(ptr", "@llvm.lifetime.end.p0(i64 16, ptr")
            .replace("(i64 16, ptr)", "(i64, ptr)");
    }
    // A PIE's module says so, as clang's does: without `PIC Level`, `llc`
    // reads AArch64's `__stack_chk_guard` directly (§5.6.24 rule 5), an
    // object no PIE link accepts.
    let flags = if pic {
        "!llvm.module.flags = !{!0, !1}\n!0 = !{i32 8, !\"PIC Level\", i32 2}\n\
         !1 = !{i32 7, !\"PIE Level\", i32 2}\n"
    } else {
        ""
    };
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"{}\"\n{body}{flags}",
        layout(target),
        target.triple()
    )
}

/// `coro/victim.ll`: the ramp's stack-passed pointer is read at entry,
/// below the guard; the overflow that reaches it aborts as `llc`'s does.
#[test]
fn a_ramp_reads_its_stack_arguments_below_the_guard() {
    let scratch = Scratch::new("coro-victim");
    let cases: [&[&str]; 2] = [&[], &["overflow"]];
    for target in TARGETS {
        let Some(lane) = lane(target) else { continue };
        let mut runs = 0;
        for major in LLVM_MAJORS {
            for attribute in ["ssp", "sspstrong", "sspreq"] {
                for pic in [false, true] {
                    let label = format!(
                        "{}-{major}-{attribute}-{}",
                        target.name(),
                        if pic { "pie" } else { "exe" }
                    );
                    let text = victim(major, target, attribute, pic);
                    let Some(objects) = objects(&scratch, major, target, &text, pic, &label) else {
                        continue;
                    };
                    let ends = compare(&lane, &objects, pic, &cases, &label, &mut runs);
                    assert_eq!(ends[0].code, Some(0), "{label}: {:?}", ends[0]);
                    assert_eq!(
                        ends[0].stdout, "ramp wrote 16 (A)\nstored 28\nresumed 7 7\n",
                        "{label}"
                    );
                    assert_eq!(ends[1].signal, Some(SIGABRT), "{label}: {:?}", ends[1]);
                    assert!(ends[1].smashed, "{label}: {:?}", ends[1]);
                }
            }
        }
        eprintln!("{}: {runs} protected ramp runs equal llc's", target.name());
    }
}
