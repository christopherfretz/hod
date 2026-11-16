//! The i686 column of the cross runner, proven on the oracle
//! (`spec/harness.md` "Cross runs", design 0008 section 4): small C, C++
//! and IR programs compiled by clang 19 and 22 `--target=i686-linux-gnu
//! -O0` to IR, by `llc` to objects, linked by the cross driver with each
//! linker of the i686 matrix (GNU ld, lld, mold; wild does not link
//! i386) and run under `qemu-i386`. hod does not compile for i686 yet
//! (`Target::COMPILED_BY_HOD`): this is the lane its objects will enter,
//! and today it proves the tools, the system root, the link lines and the
//! runner.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, Target, linkers, run_with, tool};

const T: Target = Target::I686;

/// The tools of the lane, or `None` (after a note) where one is missing.
struct Lane {
    gcc: PathBuf,
    gxx: PathBuf,
    emulator: PathBuf,
    sysroot: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn lane() -> Option<Lane> {
    if !cfg!(target_os = "linux") {
        eprintln!("note: skipping: programs are linked and run on Linux only");
        return None;
    }
    let lane = Lane {
        gcc: T.tool("gcc")?,
        gxx: T.tool("g++")?,
        emulator: T.tool("emulator")?,
        sysroot: T.tool("sysroot")?,
        linkers: linkers::present(T),
    };
    let names: Vec<&str> = lane.linkers.iter().map(|(name, _)| *name).collect();
    assert!(!names.contains(&"wild"), "wild does not link i386: {names:?}");
    if hod_testutil::tools_required() {
        assert_eq!(names, ["bfd", "lld", "mold"]);
    }
    Some(lane)
}

/// How a program is built: the relocation model of clang and `llc`, and
/// the driver's link flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    /// clang's default for i686 (design 0008 section 5.3): PIC code in a
    /// position-independent executable.
    Pie,
    /// `-fno-pic` code linked `-static -no-pie`.
    Static,
}

impl Model {
    fn clang(self) -> &'static [&'static str] {
        match self {
            Self::Pie => &[],
            Self::Static => &["-fno-pic"],
        }
    }

    fn llc(self) -> &'static str {
        match self {
            Self::Pie => "-relocation-model=pic",
            Self::Static => "-relocation-model=static",
        }
    }

    fn link(self) -> &'static [&'static str] {
        match self {
            Self::Pie => &["-pie"],
            Self::Static => &["-static", "-no-pie"],
        }
    }

    /// The ELF type of the program: `ET_DYN` for a PIE, `ET_EXEC` else.
    fn elf_type(self) -> u16 {
        match self {
            Self::Pie => 3,
            Self::Static => 2,
        }
    }
}

/// A source program and what running it must give.
struct Program {
    name: &'static str,
    /// `c`, `cpp` or `ll`.
    language: &'static str,
    source: &'static str,
    stdout: &'static str,
    status: i32,
}

/// Checks that `output` of a child succeeded, with its output on failure.
fn ok(what: &str, outcome: &hod_testutil::Outcome) {
    assert!(
        outcome.success(),
        "{what}: {:?}\n{}{}",
        outcome.status,
        outcome.stdout_text(),
        outcome.stderr_text()
    );
}

fn run(command: &mut Command) -> hod_testutil::Outcome {
    run_with(command, b"", DEFAULT_TIMEOUT)
}

/// The IR of `program` for LLVM `major`: clang's for C and C++, the source
/// itself for IR. `None` when that clang is missing (after a note).
fn ir(dir: &Path, program: &Program, model: Model, major: u32) -> Option<PathBuf> {
    let source = dir.join(format!("{}.{}", program.name, program.language));
    std::fs::write(&source, program.source).unwrap();
    if program.language == "ll" {
        return Some(source);
    }
    let driver = if program.language == "cpp" { "clang++" } else { "clang" };
    let clang = tool(&format!("{driver}-{major}"))?;
    let out = dir.join(format!("{}-{major}.ll", program.name));
    let outcome = run(Command::new(&clang)
        .args(["--target=i686-linux-gnu", "-O0", "-S", "-emit-llvm"])
        .args(model.clang())
        .arg(&source)
        .arg("-o")
        .arg(&out));
    ok(&format!("{} {}", clang.display(), program.name), &outcome);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("target triple = \"i686-unknown-linux-gnu\""), "{text}");
    Some(out)
}

/// Builds `program` with LLVM `major` (the reference: `llc -O0` with
/// `SelectionDAG` alone, as irdiff's oracle), links it with every linker of
/// the lane and runs each under `qemu-i386`. Returns the linkers it ran
/// with.
fn check(lane: &Lane, program: &Program, model: Model, major: u32) -> Vec<&'static str> {
    let Some(llc) = tool(&format!("llc-{major}")) else { return Vec::new() };
    let scratch = Scratch::new(&format!("i686-{}-{model:?}-{major}", program.name));
    let Some(ir) = ir(scratch.path(), program, model, major) else { return Vec::new() };
    let object = scratch.join(format!("{}.o", program.name));
    let outcome = run(Command::new(&llc)
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(format!("-mtriple={}", T.triple()))
        .arg(model.llc())
        .arg(&ir)
        .arg("-o")
        .arg(&object));
    ok(&format!("llc-{major} {}", program.name), &outcome);
    let driver = if program.language == "cpp" { &lane.gxx } else { &lane.gcc };
    let mut ran = Vec::new();
    for (name, linker) in &lane.linkers {
        let exe = scratch.join(format!("{}-{name}", program.name));
        let outcome = run(Command::new(driver)
            .args(linkers::driver_flags(scratch.path(), name, linker))
            .args(model.link())
            .arg(&object)
            .arg("-o")
            .arg(&exe));
        let what = format!("{} {model:?} LLVM {major} {name}", program.name);
        ok(&format!("link {what}"), &outcome);
        let bytes = std::fs::read(&exe).unwrap();
        // ELFCLASS32, little-endian, EM_386, and the model's type.
        assert_eq!(&bytes[..6], b"\x7fELF\x01\x01", "{what}");
        assert_eq!(u16::from_le_bytes([bytes[16], bytes[17]]), model.elf_type(), "{what}");
        assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 3, "{what}: EM_386");
        let outcome =
            run(Command::new(&lane.emulator).args(T.emulator_args(&lane.sysroot)).arg(&exe));
        assert!(!outcome.timed_out(), "{what}: timed out");
        assert_eq!(outcome.stdout_text(), program.stdout, "{what}: {}", outcome.stderr_text());
        assert_eq!(outcome.status.and_then(|s| s.code()), Some(program.status), "{what}");
        ran.push(*name);
    }
    ran
}

/// Every LLVM major and model of `program`; at least one run under
/// `HOD_REQUIRE_TOOLS=1`.
fn check_all(program: &Program, models: &[Model]) {
    let Some(lane) = lane() else { return };
    let mut runs = 0;
    for major in LLVM_MAJORS {
        for &model in models {
            let ran = check(&lane, program, model, major);
            eprintln!("{} {model:?} LLVM {major}: ran with {ran:?}", program.name);
            runs += ran.len();
        }
    }
    if hod_testutil::tools_required() {
        assert_eq!(runs, LLVM_MAJORS.len() * models.len() * 3, "{}", program.name);
    }
}

const BOTH: [Model; 2] = [Model::Pie, Model::Static];

// Integer-only, no library call but libgcc's 64-bit division
// (`__divdi3`, `__moddi3`, `__udivdi3`, `__umoddi3`): i64 is a pair of
// registers on i386. The exit status is 42 when every identity holds.
#[test]
fn integers_run_under_qemu_i386() {
    const SOURCE: &str = r"
static volatile long long inputs[] = {-1000000000007LL, 7, 1234567890123LL, -3, 1LL << 40};
int main(void) {
    int bad = 0;
    for (int i = 0; i < 5; i++)
        for (int j = 0; j < 5; j++) {
            long long a = inputs[i], b = inputs[j];
            unsigned long long ua = (unsigned long long)a, ub = (unsigned long long)b;
            if ((a / b) * b + a % b != a) bad |= 1;
            if ((ua / ub) * ub + ua % ub != ua) bad |= 2;
            if (((ua << (j * 7)) >> (j * 7)) != (ua & (~0ULL >> (j * 7)))) bad |= 4;
        }
    unsigned u = 0xdeadbeefu;
    if ((u >> 28) != 0xd || (int)u >= 0) bad |= 8;
    return bad ? bad : 42;
}
";
    let program = Program { name: "ints", language: "c", source: SOURCE, stdout: "", status: 42 };
    check_all(&program, &BOTH);
}

// A `main` calling the C library, with a global, a string and a 64-bit
// argument: PIE (GOT and PLT through the base register) and static.
#[test]
fn a_main_calling_libc_runs_pie_and_static() {
    const SOURCE: &str = r#"
#include <stdio.h>
#include <string.h>
int counter = 40;
static const char *word = "i686";
int main(void) {
    counter += (int)strlen("ab");
    printf("hello %s %d %lld\n", word, counter, -1234567890123LL);
    return 3;
}
"#;
    let program = Program {
        name: "hello",
        language: "c",
        source: SOURCE,
        stdout: "hello i686 42 -1234567890123\n",
        status: 3,
    };
    check_all(&program, &BOTH);
}

// A C++ exception thrown and caught through libstdc++ and the unwinder:
// `.eh_frame`, the personality, typeinfo, linked by g++.
#[test]
fn a_cxx_exception_through_libstdcxx() {
    const SOURCE: &str = r#"
#include <cstdio>
#include <stdexcept>
struct Error { int value; };
static void thrower(int x) { if (x) throw Error{x}; }
int main() {
    try { thrower(7); } catch (const Error &e) { std::printf("caught %d\n", e.value); }
    try { throw std::runtime_error("boom"); }
    catch (const std::exception &e) { std::printf("caught %s\n", e.what()); return 0; }
    return 1;
}
"#;
    let program = Program {
        name: "eh",
        language: "cpp",
        source: SOURCE,
        stdout: "caught 7\ncaught boom\n",
        status: 0,
    };
    check_all(&program, &[Model::Pie]);
}

// Design 0008 milestone A as IR: `ret i32 42` with a call, the shape hod's
// first i686 objects take; static and PIE.
#[test]
fn ret_42_as_ir() {
    const SOURCE: &str = r#"
target triple = "i686-unknown-linux-gnu"
define internal i32 @half(i32 %x) {
  %r = sdiv i32 %x, 2
  ret i32 %r
}
define i32 @main() {
  %h = call i32 @half(i32 84)
  ret i32 %h
}
"#;
    let program = Program { name: "ret42", language: "ll", source: SOURCE, stdout: "", status: 42 };
    check_all(&program, &BOTH);
}
