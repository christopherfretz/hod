//! kiln with hod as its code generator, end to end: `spec/harness.md`
//! "shim" and `spec/cli.md` "Fallback protocol" together.
//!
//! Needs a built `kiln++`: `$HOD_KILN`, or the kiln build beside this
//! checkout's target directory (`<build root>/target-kiln/release/kiln++`,
//! where the workspace that holds both projects puts it). Without one the
//! test skips with a note, or fails under `HOD_REQUIRE_TOOLS=1`. It also
//! needs a complete LLVM installation (`opt`, `llc`, `clang++`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Outcome, Scratch, run_with, tool, tools_required};
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

/// The LLVM installation that holds the `llc` the tests use, if it is a
/// complete one.
fn llvm_installation() -> Option<PathBuf> {
    let llc = std::fs::canonicalize(tool("llc")?).ok()?;
    let dir = llc.parent()?.parent()?.to_path_buf();
    let complete = shim::REQUIRED_TOOLS.iter().all(|t| dir.join("bin").join(t).is_file());
    if !complete {
        assert!(
            !tools_required(),
            "'{}' is not a complete LLVM installation (HOD_REQUIRE_TOOLS=1)",
            dir.display()
        );
        eprintln!("note: '{}' has no opt or clang++; skipping", dir.display());
        return None;
    }
    Some(dir)
}

/// A command that inherits none of hod's settings from the test's own
/// environment.
fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    for var in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"] {
        command.env_remove(var);
    }
    // kiln's own harness sets this; the wrapper's flag must win over it.
    command.env("KILN_LLVM_DIR", "/nonexistent/llvm");
    command
}

fn run_in(dir: &Path, command: &mut Command) -> Outcome {
    let outcome = run_with(command.current_dir(dir), b"", Duration::from_mins(3));
    assert!(!outcome.timed_out(), "timed out: {command:?}");
    outcome
}

/// The coverage log as rows of fields.
fn coverage(log: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines().map(|line| line.split('\t').map(str::to_owned).collect()).collect()
}

// spec/harness.md "shim": kiln accepts the shim directory, compiles through
// hod, and hod's fallback produces working programs; spec/cli.md "Strict
// mode" surfaces as a failed compile.
#[test]
fn kiln_builds_and_runs_programs_through_the_shim() {
    let (Some(kiln), Some(llvm)) = (kiln(), llvm_installation()) else { return };
    let scratch = Scratch::new("kiln-e2e");
    let built = shim::build(&shim::Request {
        llvm,
        hod: PathBuf::from(env!("CARGO_BIN_EXE_hod")),
        out: scratch.join("llvm"),
        kiln: Some(kiln),
    })
    .unwrap_or_else(|e| panic!("cannot build the shim: {e}"));
    let compiler = built.compiler.as_deref().expect("a kiln was given");
    let work = scratch.join("work");
    std::fs::create_dir(&work).unwrap();

    // The wrapper is kiln, and kiln took the shim as its LLVM.
    let version = run_in(&work, command(compiler).arg("--version"));
    assert!(version.success(), "{}", version.stderr_text());
    let text = version.stdout_text();
    assert!(text.starts_with("kiln version"), "{text}");
    assert!(text.contains(&built.dir.display().to_string()), "kiln names its LLVM: {text}");

    // A program with a virtual call, an exception and iostream.
    std::fs::write(work.join("hello.cpp"), HELLO).unwrap();
    let log = scratch.join("hello.tsv");
    let compile = run_in(
        &work,
        command(compiler).args(["hello.cpp", "-o", "hello"]).env("HOD_COVERAGE_LOG", &log),
    );
    assert!(compile.success(), "{}", compile.stderr_text());
    let ran = run_in(&work, &mut command(&work.join("hello")));
    assert!(ran.success(), "{ran:?}");
    assert_eq!(ran.stdout_text(), "sides: 3\ncaught: too many sides\n");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1, "one translation unit, one line: {lines:?}");
    assert_eq!(lines[0].len(), 8, "{lines:?}");
    assert_eq!(lines[0][..3], ["fallback", "codegen-not-implemented", "text"], "{lines:?}");
    assert!(lines[0][5].contains("linux"), "the module's triple: {lines:?}");

    // A coroutine: kiln runs `opt` first, so `llc` (hod) is handed bitcode.
    std::fs::write(work.join("coro.cpp"), COROUTINE).unwrap();
    let log = scratch.join("coro.tsv");
    let compile = run_in(
        &work,
        command(compiler)
            .args(["-std=c++20", "coro.cpp", "-o", "coro"])
            .env("HOD_COVERAGE_LOG", &log),
    );
    assert!(compile.success(), "{}", compile.stderr_text());
    let ran = run_in(&work, &mut command(&work.join("coro")));
    assert!(ran.success(), "{ran:?}");
    assert_eq!(ran.stdout_text(), "sum of squares: 30\n");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0][..3], ["fallback", "bitcode-input", "bitcode"], "{lines:?}");

    // Strict mode: the compile fails with hod's diagnostic and no object.
    let log = scratch.join("strict.tsv");
    let compile = run_in(
        &work,
        command(compiler)
            .args(["-c", "hello.cpp", "-o", "strict.o"])
            .env("HOD_COVERAGE_LOG", &log)
            .env("HOD_STRICT", "1"),
    );
    assert!(!compile.success(), "strict mode must fail while hod generates no code");
    let stderr = compile.stderr_text();
    assert!(
        stderr.contains(
            "hod: error: unsupported construct 'codegen-not-implemented' (strict mode: no fallback)"
        ),
        "{stderr}"
    );
    assert!(!work.join("strict.o").exists());
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0][..3], ["error", "codegen-not-implemented", "text"], "{lines:?}");
    assert_eq!(lines[0][4], "strict.o", "{lines:?}");
}
