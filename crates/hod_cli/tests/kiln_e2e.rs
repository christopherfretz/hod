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

use hod_testutil::{LLVM_MAJORS, Outcome, Scratch, Target, run_with, tool, tools_required};
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

/// Exceptions through frames with destructors, a rethrow, and catches
/// by type (`llvm.eh.typeid.for`).
const EXCEPTIONS: &str = r#"
#include <cstdio>
#include <stdexcept>
#include <string>
#include <vector>

struct Guard {
    const char* name;
    ~Guard() { std::printf("unwind %s\n", name); }
};

static int depth(int n) {
    Guard guard{n % 2 ? "odd" : "even"};
    if (n == 0) throw std::runtime_error("bottom");
    return depth(n - 1) + 1;
}

static void noisy() {
    std::vector<int> v(3, 7);
    throw 42;
}

int main() {
    int caught = 0;
    try { depth(3); } catch (...) { ++caught; std::printf("caught one\n"); }
    try { noisy(); } catch (...) { ++caught; }
    try {
        try { noisy(); } catch (...) { std::printf("rethrow\n"); throw; }
    } catch (...) { ++caught; }
    try { depth(1); } catch (const std::exception& e) { std::printf("typed %s\n", e.what()); }
    try { throw 7; } catch (int v) { std::printf("int %d\n", v); }
    try {
        std::string s = "abc";
        std::printf("%c\n", s.at(10));
    } catch (const std::out_of_range&) { std::printf("range\n"); }
    std::printf("caught %d\n", caught);
    return 0;
}
"#;
const EXCEPTIONS_PRINTS: &str = "unwind even\nunwind odd\nunwind even\nunwind odd\ncaught one\n\
rethrow\nunwind even\nunwind odd\ntyped bottom\nint 7\nrange\ncaught 3\n";

/// Exceptions caught without a type: unwinding through destructors,
/// cleanups of a `std::vector`, a rethrow.
const UNWINDING: &str = r#"
#include <cstdio>
#include <stdexcept>
#include <vector>

struct Guard {
    const char* name;
    ~Guard() { std::printf("unwind %s\n", name); }
};

static int depth(int n) {
    Guard guard{n % 2 ? "odd" : "even"};
    if (n == 0) throw std::runtime_error("bottom");
    return depth(n - 1) + 1;
}

static void noisy() {
    std::vector<int> v(3, 7);
    throw 42;
}

int main() {
    int caught = 0;
    try { depth(3); } catch (...) { ++caught; std::printf("caught one\n"); }
    try { noisy(); } catch (...) { ++caught; }
    try {
        try { noisy(); } catch (...) { std::printf("rethrow\n"); throw; }
    } catch (...) { ++caught; }
    std::printf("caught %d\n", caught);
    return 0;
}
"#;
const UNWINDING_PRINTS: &str =
    "unwind even\nunwind odd\nunwind even\nunwind odd\ncaught one\nrethrow\ncaught 3\n";

/// Standard containers, algorithms, virtual calls through `unique_ptr`,
/// `std::function`, `double` and `long double` (fp128) arithmetic.
const CONTAINERS: &str = r#"
#include <algorithm>
#include <cstdio>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <vector>

struct Animal {
    virtual ~Animal() = default;
    virtual std::string name() const = 0;
};
struct Cat : Animal { std::string name() const override { return "cat"; } };
struct Dog : Animal { std::string name() const override { return "dog"; } };

int main() {
    std::vector<int> v{5, 3, 9, 1, 7};
    std::sort(v.begin(), v.end());
    for (int x : v) std::printf("%d ", x);
    std::printf("\n");
    std::map<std::string, int> m;
    for (const char* w : {"b", "a", "c", "a"}) ++m[w];
    for (auto& [k, c] : m) std::printf("%s=%d ", k.c_str(), c);
    std::printf("\n");
    std::vector<std::unique_ptr<Animal>> zoo;
    zoo.push_back(std::make_unique<Cat>());
    zoo.push_back(std::make_unique<Dog>());
    for (auto& a : zoo) std::printf("%s ", a->name().c_str());
    std::function<double(double)> f = [](double x) { return x * 1.5 + 0.25; };
    std::printf("\n%.3f %g\n", f(2.0), 1.0 / 3.0);
    long double ld = 1.0L / 7.0L;
    std::printf("%.10Lf\n", ld);
    return 0;
}
"#;
const CONTAINERS_PRINTS: &str =
    "1 3 5 7 9 \na=2 b=1 c=1 \ncat dog \n3.250 0.333333\n0.1428571429\n";

/// Why hod declines `MODULE_ASM`: the assembler of either target names
/// the directive (x86-64 reads file-scope `asm` since WP-204).
const MODULE_ASM_REASON: &str = "asm:directive/ident";

/// File-scope `asm` with a directive hod's assembler does not read
/// (`.ident`, `asm:directive/ident`, `spec/asm.md` §9): a module hod
/// declines, so the fallback path stays tested.
const MODULE_ASM: &str = r#"
#include <cstdio>
asm(".globl hod_e2e_marker\n.set hod_e2e_marker, 0x2a\n.ident \"hod e2e\"");
[[gnu::always_inline]] inline int twice(int value) { return value + value; }
int main() {
    std::printf("module asm %d\n", twice(21));
    return 0;
}
"#;

/// libstdc++'s `<iostream>` writes module assembly (`.globl
/// _ZSt21ios_base_library_initv`) whose reference links the library's
/// stream initializer: a static constructor that prints works only if
/// the streams are initialized first (WP-075, `spec/ir.md` §2.3).
const IOSTREAM_INIT: &str = r#"
#include <iostream>
struct Early {
    Early() { std::cout << "early " << 6 * 7 << '\n'; }
} early;
int main() {
    std::cout << "main" << std::endl;
    return 0;
}
"#;

/// File-scope `asm(".weak ...")` of a function nothing defines: without
/// the directive the program does not link (WP-075).
const WEAK_HOOK: &str = r#"
#include <cstdio>
asm(".weak hod_e2e_hook");
extern "C" int hod_e2e_hook();
int main() {
    int (*volatile hook)() = hod_e2e_hook;
    std::printf("hook %s\n", hook ? "present" : "absent");
    return 0;
}
"#;

/// Programs whose module assembly must also hold in a static link.
const STATIC_TOO: &[&str] = &["iostream-init", "weak-hook"];

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
    /* Called before the printf: the order in which a call's arguments are
       evaluated is unspecified, and gcc for x86-64 reads g_counter before
       it calls accumulate. */
    long sum = accumulate(1000);
    printf("%ld %d %u %u %ld %ld %ld\n", sum, prime_sum(xs, 7), collatz(27), collatz(97), p.a,
           p.b, g_counter);
    return 0;
}
"#;
const DRIVER_PRINTS: &str = "-96401 438 111 118 2 1 253\n";

/// The target kiln compiles for without `--target`.
fn host() -> Target {
    Target::host().unwrap_or(Target::Aarch64)
}

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
    // The shim's `llc` reads bitcode natively; a caller's converter must
    // not win.
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
    const FAMILIES: [&str; 13] = [
        "intrinsic:",
        "asm:",
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

// spec/harness.md "shim": kiln accepts the shim directory and compiles
// through hod; whatever hod does with each program (builds it, or hands
// it to the fallback with a construct's name), the program links under
// every linker and prints what the same source built by kiln with LLVM
// prints. One program is pinned to hod's code and one to the fallback, so
// both paths stay tested as the support matrix moves. Bitcode (after
// kiln's `opt`) is read by hod's own reader, never converted
// (spec/cli.md "Bitcode conversion", WP-068); spec/cli.md "Strict mode" surfaces
// as a failed compile.
#[test]
fn kiln_builds_and_runs_programs_through_the_shim() {
    let Some(kiln) = kiln() else { return };
    for llvm in llvm_installations() {
        eprintln!("note: kiln through a shim of '{}'", llvm.display());
        through_the_shim(&kiln, &llvm);
    }
}

/// Which path a program must take through hod.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// hod builds it.
    Hod,
    /// hod declines it with this reason; the fallback builds it.
    Fallback(&'static str),
    /// Either: the support matrix decides.
    Either,
}

/// One C++ program of the test.
struct Program {
    name: &'static str,
    source: &'static str,
    args: &'static [&'static str],
    /// What kiln hands `llc`: text, or bitcode after `opt`.
    form: &'static str,
    prints: &'static str,
    path: Route,
}

const HELLO_PRINTS: &str = "sides: 3\ncaught: too many sides\n";

const PROGRAMS: &[Program] = &[
    Program {
        name: "hello",
        source: HELLO,
        args: &[],
        form: "text",
        prints: HELLO_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "hello-o1",
        source: HELLO,
        args: &["-O1"],
        form: "bitcode",
        prints: HELLO_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "coro",
        source: COROUTINE,
        args: &["-std=c++20"],
        form: "bitcode",
        prints: "sum of squares: 30\n",
        path: Route::Either,
    },
    Program {
        name: "coro-o1",
        source: COROUTINE,
        args: &["-std=c++20", "-O1"],
        form: "bitcode",
        prints: "sum of squares: 30\n",
        path: Route::Either,
    },
    // An `always_inline` function, the common reason for bitcode in code
    // that uses the standard library: kiln runs `opt` at -O0.
    Program {
        name: "inline",
        source: ALWAYS_INLINE,
        args: &[],
        form: "bitcode",
        prints: "twice: 42\n",
        path: Route::Hod,
    },
    Program {
        name: "exceptions",
        source: EXCEPTIONS,
        args: &[],
        form: "bitcode",
        prints: EXCEPTIONS_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "exceptions-o1",
        source: EXCEPTIONS,
        args: &["-O1"],
        form: "bitcode",
        prints: EXCEPTIONS_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "unwinding",
        source: UNWINDING,
        args: &[],
        form: "bitcode",
        prints: UNWINDING_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "unwinding-o1",
        source: UNWINDING,
        args: &["-O1"],
        form: "bitcode",
        prints: UNWINDING_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "containers",
        source: CONTAINERS,
        args: &["-std=c++20"],
        form: "bitcode",
        prints: CONTAINERS_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "containers-nocontract",
        source: CONTAINERS,
        args: &["-std=c++20", "-ffp-contract=off", "-O1"],
        form: "bitcode",
        prints: CONTAINERS_PRINTS,
        path: Route::Either,
    },
    Program {
        name: "iostream-init",
        source: IOSTREAM_INIT,
        args: &[],
        form: "text",
        prints: "early 42\nmain\n",
        path: Route::Either,
    },
    Program {
        name: "weak-hook",
        source: WEAK_HOOK,
        args: &[],
        form: "text",
        prints: "hook absent\n",
        path: Route::Either,
    },
    Program {
        name: "module-asm",
        source: MODULE_ASM,
        args: &[],
        form: "bitcode",
        prints: "module asm 42\n",
        path: Route::Fallback(MODULE_ASM_REASON),
    },
];

/// One kiln, wrapped to use a shim, and where it works.
struct Shimmed {
    scratch: Scratch,
    compiler: PathBuf,
    work: PathBuf,
    kiln: PathBuf,
    llvm: PathBuf,
    toolchain: Option<linking::Toolchain>,
}

impl Shimmed {
    /// Compiles and links with `args` to `program` through kiln's own
    /// driver, runs the program and checks what it prints. Returns the
    /// coverage log of the compile.
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

    /// Compiles `source` with `args` to `object` through the shim (with
    /// the fallback, or in strict mode) and returns the coverage line.
    fn compile(&self, source: &str, args: &[&str], object: &str, strict: bool) -> Vec<String> {
        let log = self.scratch.join(format!("{object}.tsv"));
        let mut compile = command(&self.compiler);
        compile.args(args).args(["-c", source, "-o", object]).env("HOD_COVERAGE_LOG", &log);
        if strict {
            compile.env("HOD_STRICT", "1");
        }
        let compile = run_in(&self.work, &mut compile);
        assert!(compile.success(), "{object}: {}", compile.stderr_text());
        let mut lines = coverage(&log);
        assert_eq!(lines.len(), 1, "one translation unit, one line: {lines:?}");
        let line = lines.remove(0);
        assert_eq!(line.len(), 11, "{line:?}");
        assert!(line[5].contains("linux"), "the module's triple: {line:?}");
        line
    }

    /// The same source compiled by kiln with the LLVM installation itself.
    fn reference(&self, source: &str, args: &[&str], object: &str) -> PathBuf {
        let output = self.work.join(object);
        let mut compile = command(&self.kiln);
        compile.arg(format!("--llvm-dir={}", self.llvm.display()));
        let compile =
            run_in(&self.work, compile.args(args).args(["-c", source, "-o"]).arg(&output));
        assert!(compile.success(), "{object}: {}", compile.stderr_text());
        output
    }

    /// LLVM's tools read an object hod wrote for `target` without
    /// complaint.
    fn inspect(&self, object: &Path, shows: &str, target: Target) {
        let bin = self.llvm.join("bin");
        // `Machine:` names the object's architecture.
        let machine = match target {
            Target::Aarch64 => "AArch64",
            Target::X86_64 => "X86-64",
        };
        for (tool, args, expect) in [
            ("llvm-readelf", &["-h", "-S", "-r", "-s"][..], machine),
            ("llvm-objdump", &["-d", "-r"][..], shows),
            ("llvm-dwarfdump", &["--eh-frame"][..], "FDE"),
            ("llvm-dwarfdump", &["--verify"][..], "No errors."),
        ] {
            let mut read = Command::new(bin.join(tool));
            let read = run_in(&self.work, read.args(args).arg(object));
            let text = read.stdout_text();
            let what = format!("{tool} {args:?} {}", object.display());
            assert!(read.success() && read.stderr_text().is_empty(), "{what}: {read:?}");
            assert!(text.contains(expect), "{what}: {text}");
            assert!(!text.contains("<unknown>"), "{what}: {text}");
        }
    }

    /// Links `objects` under every linker present, runs each program and
    /// checks what it prints.
    /// `libraries` follow the objects (`-lstdc++`: the C driver links).
    fn links_and_prints(&self, what: &str, objects: &[&Path], libraries: &[&str], prints: &str) {
        self.links_with(what, &[], objects, libraries, prints);
    }

    /// [`Shimmed::links_and_prints`] with the linker driver's `flags`.
    fn links_with(
        &self,
        what: &str,
        flags: &[&str],
        objects: &[&Path],
        libraries: &[&str],
        prints: &str,
    ) {
        let Some(toolchain) = &self.toolchain else { return };
        Self::links_on(toolchain, &self.scratch, what, flags, objects, libraries, prints);
    }

    /// [`Shimmed::links_with`] with `toolchain`.
    fn links_on(
        toolchain: &linking::Toolchain,
        scratch: &Scratch,
        what: &str,
        flags: &[&str],
        objects: &[&Path],
        libraries: &[&str],
        prints: &str,
    ) {
        let mut inputs = objects.to_vec();
        inputs.extend(libraries.iter().map(Path::new));
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("{what}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, flags, &inputs, &program);
            assert!(ran.success(), "{what} {}: {ran:?}", linker.name);
            assert_eq!(ran.stdout_text(), prints, "{what} {}", linker.name);
        }
    }

    /// One program: through hod (built or handed on, as `program.path`
    /// allows), and by the LLVM route; both link and print the same.
    fn check(&self, program: &Program) {
        let source = format!("{}.cpp", program.name);
        std::fs::write(self.work.join(&source), program.source).unwrap();
        let object = format!("{}.o", program.name);
        let line = self.compile(&source, program.args, &object, false);
        let name = program.name;
        assert_eq!(line[2], program.form, "{name}: {line:?}");
        // The shim reads bitcode natively: nothing is converted (field 9).
        assert_eq!(&*line[8], "-", "{name}: {line:?}");
        match (&*line[0], program.path) {
            ("hod", Route::Hod | Route::Either) => {
                assert_eq!((&*line[1], &*line[7], &*line[9]), ("-", "-", "-"), "{name}: {line:?}");
                self.inspect(&self.work.join(&object), "<main>:", host());
            }
            ("fallback", Route::Fallback(_) | Route::Either) => {
                assert_construct(&line);
                if let Route::Fallback(reason) = program.path {
                    assert_eq!(line[1], reason, "{name}: {line:?}");
                }
                assert_eq!(line[9], "0", "the fallback succeeded: {line:?}");
            }
            _ => panic!("{name}: expected {:?}: {line:?}", program.path),
        }
        eprintln!("note: {name}: {} {}", line[0], line[1]);
        let reference = self.reference(&source, program.args, &format!("{name}-llvm.o"));
        let ours = self.work.join(&object);
        let flags = ["-lstdc++", "-lm"];
        self.links_and_prints(&format!("{name}-hod"), &[&ours], &flags, program.prints);
        self.links_and_prints(&format!("{name}-llvm"), &[&reference], &flags, program.prints);
        if STATIC_TOO.contains(&name) {
            let what = format!("{name}-hod-static");
            self.links_with(&what, &["-static"], &[&ours], &flags, program.prints);
        }
    }

    /// [`INTEGERS`] for `target` (`kiln --target`) in strict mode at -O0
    /// (text) and -O1 (bitcode): hod builds it, LLVM's tools read it, and
    /// with [`DRIVER`] it prints what the LLVM route's object prints. A
    /// target that is not the host links with the cross toolchain and runs
    /// under its emulator, so every host builds this program for both
    /// targets.
    fn hod_builds_integers(&self, target: Target) {
        let name = target.name();
        // A toolchain makes its linker directories in its scratch: the
        // cross one gets its own.
        let cross;
        let (toolchain, scratch) = if target.is_host() {
            (&self.toolchain, &self.scratch)
        } else {
            let scratch = Scratch::new(&format!("kiln-e2e-{name}"));
            cross = (linking::Toolchain::for_target(&scratch, target), scratch);
            (&cross.0, &cross.1)
        };
        let Some(toolchain) = toolchain else { return };
        let driver = toolchain.compile_c(scratch, &format!("driver-{name}"), DRIVER, &[]);
        let source = format!("integers-{name}.cpp");
        std::fs::write(self.work.join(&source), INTEGERS).unwrap();
        let triple = format!("--target={name}-unknown-linux-gnu");
        for (opt, form) in [("-O0", "text"), ("-O1", "bitcode")] {
            let args = [&*triple, opt];
            let object = format!("integers-{name}{opt}.o");
            let line = self.compile(&source, &args, &object, true);
            assert_eq!((&*line[0], &*line[1], &*line[2]), ("hod", "-", form), "{line:?}");
            assert!(line[5].starts_with(name), "the module's triple: {line:?}");
            let ours = self.work.join(&object);
            self.inspect(&ours, "<accumulate>:", target);
            let theirs = self.reference(&source, &args, &format!("integers-{name}-llvm{opt}.o"));
            for (route, input) in [("hod", &ours), ("llvm", &theirs)] {
                let what = format!("integers-{name}-{route}{opt}");
                let objects = [driver.as_path(), input];
                Self::links_on(toolchain, scratch, &what, &[], &objects, &[], DRIVER_PRINTS);
            }
        }
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
    let compiler = built.compiler.clone().expect("a kiln was given");
    let work = scratch.join("work");
    std::fs::create_dir(&work).unwrap();

    // The wrapper is kiln, and kiln took the shim as its LLVM.
    let version = run_in(&work, command(&compiler).arg("--version"));
    assert!(version.success(), "{}", version.stderr_text());
    let text = version.stdout_text();
    assert!(text.starts_with("kiln version"), "{text}");
    assert!(text.contains(&built.dir.display().to_string()), "kiln names its LLVM: {text}");

    let toolchain = linking::Toolchain::host(&scratch);
    if let Some(toolchain) = &toolchain {
        assert!(!toolchain.linkers.is_empty(), "no linker");
    }
    let shimmed = Shimmed {
        scratch,
        compiler,
        work,
        kiln: kiln.to_path_buf(),
        llvm: llvm.to_path_buf(),
        toolchain,
    };

    // kiln's own driver compiles through the shim and links.
    std::fs::write(shimmed.work.join("hello.cpp"), HELLO).unwrap();
    let log = shimmed.builds(&["hello.cpp"], "hello", HELLO_PRINTS);
    assert_eq!(coverage(&log).len(), 1, "one translation unit, one line");

    for program in PROGRAMS {
        shimmed.check(program);
    }

    // Code hod generates: strict mode succeeds.
    for target in Target::ALL {
        shimmed.hod_builds_integers(target);
    }

    // Strict mode on bitcode: hod's own reader declines it with the
    // construct's name, and nothing was converted.
    let line = shimmed.strict_fails("module-asm.cpp", "strict-asm.o");
    assert_eq!((&*line[1], &*line[2]), (MODULE_ASM_REASON, "bitcode"), "{line:?}");
    assert_eq!(&*line[8], "-", "no conversion ran: {line:?}");
}
