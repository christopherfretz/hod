//! kiln with hod as its code generator, end to end: `spec/harness.md`
//! "shim" with `spec/cli.md` "Fallback protocol" and "Bitcode conversion".
//!
//! Needs a built `kiln++`: `$HOD_KILN`, or the kiln build beside this
//! checkout's target directory (`<build root>/target-kiln/release/kiln++`,
//! where the workspace that holds both projects puts it). Without one the
//! test skips with a note, or fails under `HOD_REQUIRE_TOOLS=1`. It also
//! needs a complete LLVM installation (`opt`, `llc`, `clang++`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use hod_testutil::{LLVM_MAJORS, Outcome, Scratch, run_with, tool, tools_required};
use xtask::shim;

/// A C++ program with a virtual call, an exception and iostream.
const HELLO: &str = r#"
#include <iostream>
#include <stdexcept>

struct Shape {
    virtual ~Shape() = default;
    virtual int sides() const = 0;
};

struct Triangle : Shape {
    int sides() const override { return 3; }
};

static int checked(const Shape& shape, int limit) {
    if (shape.sides() > limit) {
        throw std::runtime_error("too many sides");
    }
    return shape.sides();
}

int main() {
    Triangle triangle;
    const Shape& shape = triangle;
    std::cout << "sides: " << checked(shape, 4) << '\n';
    try {
        checked(shape, 2);
    } catch (const std::exception& e) {
        std::cout << "caught: " << e.what() << '\n';
    }
    return 0;
}
"#;

/// A coroutine: kiln lowers it with `opt` and hands `llc` bitcode.
const COROUTINE: &str = r#"
#include <coroutine>
#include <cstdio>

struct Generator {
    struct promise_type {
        int current = 0;
        Generator get_return_object() {
            return Generator{std::coroutine_handle<promise_type>::from_promise(*this)};
        }
        std::suspend_always initial_suspend() noexcept { return {}; }
        std::suspend_always final_suspend() noexcept { return {}; }
        std::suspend_always yield_value(int value) noexcept {
            current = value;
            return {};
        }
        void return_void() noexcept {}
        void unhandled_exception() noexcept {}
    };
    std::coroutine_handle<promise_type> handle;
    explicit Generator(std::coroutine_handle<promise_type> h) : handle(h) {}
    Generator(const Generator&) = delete;
    ~Generator() { handle.destroy(); }
    bool next() {
        handle.resume();
        return !handle.done();
    }
    int value() const { return handle.promise().current; }
};

static Generator squares(int count) {
    for (int i = 1; i <= count; ++i) {
        co_yield i * i;
    }
}

int main() {
    int sum = 0;
    Generator g = squares(4);
    while (g.next()) {
        sum += g.value();
    }
    std::printf("sum of squares: %d\n", sum);
    return 0;
}
"#;

/// A function that must be inlined: kiln runs `opt` for it at `-O0` and
/// hands `llc` bitcode (`docs/design/0001-input.md`, point 6).
const ALWAYS_INLINE: &str = r#"
#include <cstdio>

[[gnu::always_inline]] inline int twice(int value) { return value + value; }

int main() {
    std::printf("twice: %d\n", twice(21));
    return 0;
}
"#;

/// Integer code inside what hod generates code for, with no call: loops,
/// a switch, globals, a constant table, pointers and a struct.
const INTEGERS: &str = r#"
long g_counter = 3;
static const int primes[] = {2, 3, 5, 7, 11, 13};
extern "C" long accumulate(long n) {
    long sum = 0;
    for (long i = 0; i < n; ++i) {
        switch (i % 4) {
        case 0: sum += i; break;
        case 1: sum -= 2 * i; break;
        case 2: sum ^= i; break;
        default: sum += g_counter++;
        }
    }
    return sum;
}
extern "C" int prime_sum(const int* first, int count) {
    int total = 0;
    for (int i = 0; i < count; ++i) total += first[i] * primes[i % 6];
    return total;
}
extern "C" unsigned collatz(unsigned x) {
    unsigned steps = 0;
    while (x != 1) { x = (x & 1) ? 3 * x + 1 : x / 2; ++steps; }
    return steps;
}
struct Pair { long a; long b; };
extern "C" void swap_pair(Pair* p) { long t = p->a; p->a = p->b; p->b = t; }
"#;

/// The C program that calls [`INTEGERS`], and what it prints.
const DRIVER: &str = r#"
#include <stdio.h>
struct Pair { long a, b; };
long accumulate(long);
int prime_sum(const int*, int);
unsigned collatz(unsigned);
void swap_pair(struct Pair*);
extern long g_counter;
int main(void) {
    int xs[] = {1, -2, 3, 40, 5, 6, 7};
    struct Pair p = {1, 2};
    swap_pair(&p);
    printf("%ld %d %u %u %ld %ld %ld\n", accumulate(1000), prime_sum(xs, 7), collatz(27),
           collatz(97), p.a, p.b, g_counter);
    return 0;
}
"#;
const DRIVER_PRINTS: &str = "-96401 438 111 118 2 1 253\n";

/// The `kiln++` to test with, or `None` to skip.
fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        assert!(path.is_file(), "HOD_KILN names '{}', which is not a file", path.display());
        return Some(path);
    }
    // `<build root>/target-<checkout>/tmp` -> `<build root>/target-kiln/...`.
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping the kiln end-to-end test");
            None
        }
    }
}

/// Whether `dir` has what kiln requires of an LLVM installation.
fn complete(dir: &Path) -> bool {
    shim::REQUIRED_TOOLS.iter().all(|t| dir.join("bin").join(t).is_file())
}

/// The LLVM installations to test with: the one that holds the `llc` the
/// tests use, which must be complete, and then every other supported LLVM
/// installed in `/usr/lib/llvm-<N>` (kiln supports each of
/// `LLVM_MAJORS`, and bitcode is read only by the LLVM that wrote it).
fn llvm_installations() -> Vec<PathBuf> {
    let Some(llc) = tool("llc").and_then(|llc| std::fs::canonicalize(llc).ok()) else {
        return Vec::new();
    };
    let Some(dir) = llc.parent().and_then(Path::parent).map(Path::to_path_buf) else {
        return Vec::new();
    };
    if !complete(&dir) {
        assert!(
            !tools_required(),
            "'{}' is not a complete LLVM installation (HOD_REQUIRE_TOOLS=1)",
            dir.display()
        );
        eprintln!("note: '{}' has no opt or clang++; skipping", dir.display());
        return Vec::new();
    }
    let mut found = vec![dir];
    for major in LLVM_MAJORS {
        let other = std::fs::canonicalize(format!("/usr/lib/llvm-{major}"));
        if let Ok(other) = other
            && complete(&other)
            && !found.contains(&other)
        {
            found.push(other);
        }
    }
    found
}

/// A command that inherits none of hod's settings from the test's own
/// environment.
fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    for var in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"] {
        command.env_remove(var);
    }
    // The shim's `llc` names its own converter; a caller's must not win.
    command.env("HOD_BITCODE_VIA", "/nonexistent/llvm-dis");
    // kiln's own harness sets this; the wrapper's flag must win over it.
    command.env("KILN_LLVM_DIR", "/nonexistent/llvm");
    command
}

fn run_in(dir: &Path, command: &mut Command) -> Outcome {
    let outcome = run_with(command.current_dir(dir), b"", Duration::from_mins(3));
    assert!(!outcome.timed_out(), "timed out: {command:?}");
    outcome
}

/// Whether `reason` is one hod gives for a decline: a name of
/// `classify`'s vocabulary or of one of its families (`spec/ir.md` §8.5),
/// or a code generator's (`spec/cli.md` "The back half"). Which one is the
/// support matrix's business, and moves as code generation lands, so a
/// test of real programs checks only the shape.
fn is_construct(reason: &str) -> bool {
    const FAMILIES: [&str; 12] = [
        "intrinsic:",
        "module-flag:",
        "cc:",
        "bundle:",
        "a64:",
        "abi:",
        "attr:",
        "call:",
        "limit:",
        "object:",
        "option:",
        "eh:",
    ];
    hod_ir::CONSTRUCT_NAMES.contains(&reason)
        || FAMILIES.iter().any(|f| reason.starts_with(f))
        || reason.split_once('/').is_some_and(|(row, _)| hod_ir::CONSTRUCT_NAMES.contains(&row))
}

/// Asserts that a coverage line's reason is a construct hod names.
fn assert_construct(line: &[String]) {
    assert!(is_construct(&line[1]), "a construct hod names: {line:?}");
}

/// The coverage log as rows of fields.
fn coverage(log: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines().map(|line| line.split('\t').map(str::to_owned).collect()).collect()
}

/// Asserts that `log` holds the one line of a bitcode module that hod
/// converted to text, declined as text is, and handed to
/// the fallback, which succeeded (`spec/cli.md` "Coverage log").
fn assert_converted_then_fell_back(log: &Path) {
    let lines = coverage(log);
    assert_eq!(lines.len(), 1, "one translation unit, one line: {lines:?}");
    let line = &lines[0];
    assert_eq!(line.len(), 11, "{line:?}");
    assert_eq!((&*line[0], &*line[2]), ("fallback", "bitcode"), "{line:?}");
    assert_construct(line);
    assert!(line[3].parse::<u64>().is_ok_and(|bytes| bytes > 0), "the bitcode's size: {line:?}");
    assert!(line[5].contains("linux"), "the triple, known from the converted text: {line:?}");
    let micros = |field: &str| field.parse::<u64>().unwrap_or_else(|_| panic!("{line:?}"));
    assert!(micros(&line[8]) > 0, "the conversion ran: {line:?}");
    assert!(micros(&line[6]) >= micros(&line[8]), "hod's time includes it: {line:?}");
    assert!(micros(&line[7]) > 0, "the fallback ran: {line:?}");
    assert_eq!(line[9], "0", "the fallback's exit status: {line:?}");
}

// spec/harness.md "shim": kiln accepts the shim directory, compiles through
// hod, and hod's fallback produces working programs; the bitcode kiln hands
// over after `opt` is converted with the installation's own llvm-dis
// (spec/cli.md "Bitcode conversion"); spec/cli.md "Strict mode" surfaces as
// a failed compile.
#[test]
fn kiln_builds_and_runs_programs_through_the_shim() {
    let Some(kiln) = kiln() else { return };
    for llvm in llvm_installations() {
        eprintln!("note: kiln through a shim of '{}'", llvm.display());
        through_the_shim(&kiln, &llvm);
    }
}

/// One kiln, wrapped to use a shim, and where it works.
struct Shimmed {
    scratch: Scratch,
    compiler: PathBuf,
    work: PathBuf,
}

impl Shimmed {
    /// Compiles and links with `args` to `program`, runs the program and
    /// checks what it prints. Returns the coverage log of the compile.
    fn builds(&self, args: &[&str], program: &str, prints: &str) -> PathBuf {
        let log = self.scratch.join(format!("{program}.tsv"));
        let compile = run_in(
            &self.work,
            command(&self.compiler).args(args).args(["-o", program]).env("HOD_COVERAGE_LOG", &log),
        );
        assert!(compile.success(), "{program}: {}", compile.stderr_text());
        let ran = run_in(&self.work, &mut command(&self.work.join(program)));
        assert!(ran.success(), "{program}: {ran:?}");
        assert_eq!(ran.stdout_text(), prints, "{program}");
        log
    }

    /// Compiles `source` to `object` in strict mode, which must fail with
    /// hod's diagnostic and no object. Returns the one coverage line.
    fn strict_fails(&self, source: &str, object: &str) -> Vec<String> {
        let log = self.scratch.join(format!("{object}.tsv"));
        let compile = run_in(
            &self.work,
            command(&self.compiler)
                .args(["-c", source, "-o", object])
                .env("HOD_COVERAGE_LOG", &log)
                .env("HOD_STRICT", "1"),
        );
        assert!(!compile.success(), "strict mode must fail on a module hod declines");
        let stderr = compile.stderr_text();
        assert!(!self.work.join(object).exists());
        let mut lines = coverage(&log);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = lines.remove(0);
        assert_eq!(line.len(), 11, "{line:?}");
        assert_eq!(line[0], "error", "{line:?}");
        assert_construct(&line);
        // The diagnostic names the construct, and the function for a
        // body's reason.
        let construct = format!("hod: error: unsupported construct '{}'", line[1]);
        assert!(stderr.contains(&construct), "{stderr}");
        assert!(stderr.contains("(strict mode: no fallback)"), "{stderr}");
        assert_eq!(line[4], object, "{line:?}");
        assert_eq!(line[9], "-", "no fallback ran: {line:?}");
        line
    }
}

/// The test, with the LLVM installation `llvm`.
fn through_the_shim(kiln: &Path, llvm: &Path) {
    let scratch = Scratch::new("kiln-e2e");
    let built = shim::build(&shim::Request {
        llvm: llvm.to_path_buf(),
        hod: PathBuf::from(env!("CARGO_BIN_EXE_hod")),
        out: scratch.join("llvm"),
        kiln: Some(kiln.to_path_buf()),
    })
    .unwrap_or_else(|e| panic!("cannot build the shim: {e}"));
    assert!(built.real_llvm_dis.is_some(), "the installation has an llvm-dis: {built:?}");
    let compiler = built.compiler.clone().expect("a kiln was given");
    let work = scratch.join("work");
    std::fs::create_dir(&work).unwrap();

    // The wrapper is kiln, and kiln took the shim as its LLVM.
    let version = run_in(&work, command(&compiler).arg("--version"));
    assert!(version.success(), "{}", version.stderr_text());
    let text = version.stdout_text();
    assert!(text.starts_with("kiln version"), "{text}");
    assert!(text.contains(&built.dir.display().to_string()), "kiln names its LLVM: {text}");

    std::fs::write(work.join("hello.cpp"), HELLO).unwrap();
    std::fs::write(work.join("coro.cpp"), COROUTINE).unwrap();
    std::fs::write(work.join("inline.cpp"), ALWAYS_INLINE).unwrap();
    let shimmed = Shimmed { scratch, compiler, work };
    let hello = "sides: 3\ncaught: too many sides\n";

    // A program with a virtual call, an exception and iostream: text.
    let log = shimmed.builds(&["hello.cpp"], "hello", hello);
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1, "one translation unit, one line: {lines:?}");
    assert_eq!(lines[0].len(), 11, "{lines:?}");
    assert_eq!((&*lines[0][0], &*lines[0][2]), ("fallback", "text"), "{lines:?}");
    assert_construct(&lines[0]);
    assert!(lines[0][5].contains("linux"), "the module's triple: {lines:?}");
    assert_eq!(lines[0][8..], ["-", "0", "-"], "no conversion; the fallback succeeded: {lines:?}");

    // A coroutine: kiln runs `opt` first, so `llc` (hod) is handed bitcode.
    let log = shimmed.builds(&["-std=c++20", "coro.cpp"], "coro", "sum of squares: 30\n");
    assert_converted_then_fell_back(&log);

    // An `always_inline` function, the common reason for bitcode in code
    // that uses the standard library: again `opt` first, bitcode to hod.
    let log = shimmed.builds(&["inline.cpp"], "inline", "twice: 42\n");
    assert_converted_then_fell_back(&log);

    // Optimization: at -O1 every translation unit goes through `opt`.
    let log = shimmed.builds(&["-O1", "hello.cpp"], "hello-o1", hello);
    assert_converted_then_fell_back(&log);

    // Strict mode on bitcode: hod's decline is the text path's, which shows
    // that the module was converted and looked at as text.
    let line = shimmed.strict_fails("inline.cpp", "strict-inline.o");
    assert_eq!(line[2], "bitcode", "{line:?}");
    assert!(line[8].parse::<u64>().is_ok(), "the conversion ran: {line:?}");

    // Strict mode on text: the compile fails with hod's diagnostic.
    let line = shimmed.strict_fails("hello.cpp", "strict.o");
    assert_eq!(line[2], "text", "{line:?}");
    assert_eq!(line[8], "-", "no conversion: {line:?}");

    // Code hod generates: strict mode succeeds, and the object links and
    // runs under every linker as the LLVM route's does.
    shimmed.hod_builds(kiln, llvm);
}

impl Shimmed {
    /// Compiles [`INTEGERS`] in strict mode at -O0 (text) and -O1
    /// (bitcode), checks the object with LLVM's tools, links it with
    /// [`DRIVER`] under every linker present, runs it, and compares with
    /// the same source compiled by kiln with `llvm` itself.
    fn hod_builds(&self, kiln: &Path, llvm: &Path) {
        let scratch = Scratch::new("kiln-e2e-link");
        let Some(toolchain) = linking::Toolchain::host(&scratch) else { return };
        assert!(!toolchain.linkers.is_empty(), "no linker");
        let driver = toolchain.compile_c(&scratch, "driver", DRIVER, &[]);
        std::fs::write(self.work.join("integers.cpp"), INTEGERS).unwrap();
        let bin = llvm.join("bin");
        for (opt, form) in [("-O0", "text"), ("-O1", "bitcode")] {
            let object = format!("integers{opt}.o");
            let log = self.scratch.join(format!("{object}.tsv"));
            let compile = run_in(
                &self.work,
                command(&self.compiler)
                    .args([opt, "-c", "integers.cpp", "-o", &object])
                    .env("HOD_COVERAGE_LOG", &log)
                    .env("HOD_STRICT", "1"),
            );
            assert!(compile.success(), "{object}: {}", compile.stderr_text());
            let lines = coverage(&log);
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert_eq!((&*lines[0][0], &*lines[0][1], &*lines[0][2]), ("hod", "-", form));
            let ours = self.work.join(&object);
            // LLVM's tools read the object without complaint.
            for (tool, args, shows) in [
                ("llvm-readelf", &["-h", "-S", "-r", "-s"][..], "AArch64"),
                ("llvm-objdump", &["-d", "-r"][..], "<accumulate>:"),
                ("llvm-dwarfdump", &["--eh-frame"][..], "FDE"),
            ] {
                let mut read = Command::new(bin.join(tool));
                let read = run_in(&self.work, read.args(args).arg(&ours));
                let text = read.stdout_text();
                assert!(read.success() && read.stderr_text().is_empty(), "{tool}: {read:?}");
                assert!(text.contains(shows), "{tool} {object}: {text}");
                assert!(!text.contains("<unknown>"), "{tool} {object}: {text}");
            }
            let theirs = self.work.join(format!("llvm{opt}.o"));
            let mut reference = command(kiln);
            reference.arg(format!("--llvm-dir={}", llvm.display()));
            let reference =
                run_in(&self.work, reference.args([opt, "-c", "integers.cpp", "-o"]).arg(&theirs));
            assert!(reference.success(), "{}", reference.stderr_text());
            for linker in &toolchain.linkers {
                for (route, input) in [("hod", &ours), ("llvm", &theirs)] {
                    let program = scratch.join(format!("{route}{opt}-{}", linker.name));
                    let ran = toolchain.link_and_run(linker, &[], &[&driver, input], &program);
                    assert!(ran.success(), "{route} {opt} {}: {ran:?}", linker.name);
                    assert_eq!(ran.stdout_text(), DRIVER_PRINTS, "{route} {opt} {}", linker.name);
                }
            }
        }
    }
}
