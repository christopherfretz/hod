//! C++ `thread_local` objects through `hod::compile` (WP-073,
//! `spec/codegen.md` §5.5.1 "Thread-local storage and symbol offsets
//! (WP-073)"): dynamic initialization and destructors
//! (`__cxa_thread_atexit`, the `_ZTW`/`_ZTH` wrappers), an `extern
//! thread_local` with and without a dynamic initializer (a weak `_ZTH`
//! that is not defined), a thread-local in a `try` with a landing pad,
//! thread-local addresses selected and passed to calls. Each translation
//! unit is written by kiln++ and by clang++ as `-O0` IR, compiled by hod
//! (strict: nothing falls back), linked under bfd, lld and mold, run with
//! several threads, and compared with clang++'s own build.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use hod::{OutputKind, RelocModel};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const A: &str = r#"
#include <cstdio>
#include <pthread.h>
#include <string>

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static int constructed, destroyed;
static void count(int* what) {
    pthread_mutex_lock(&lock);
    ++*what;
    pthread_mutex_unlock(&lock);
}
int compute(int k);

struct Tracker {
    int value;
    std::string name;
    Tracker() : value(compute(1)), name("tracker") { count(&constructed); }
    ~Tracker() { count(&destroyed); }
};

thread_local Tracker tracker;
thread_local int plain = 5;
static thread_local std::string local_name = "local-" + std::to_string(compute(2));
extern thread_local int ext_dyn;
extern thread_local int ext_const;

[[gnu::noinline]] static int deref(int* p) { return *p; }

[[gnu::noinline]] static int* pick(bool c) { return c ? &plain : &ext_const; }

static int in_try(int throw_it) {
    try {
        if (throw_it) throw tracker.value;
        return plain;
    } catch (int v) {
        return v + tracker.value + plain;
    }
}

struct Line { char text[160]; };

static void* work(void* arg) {
    long k = (long)arg;
    Line* line = new Line;
    tracker.value += k;
    plain += k;
    ext_dyn += k;
    ext_const += k;
    local_name += std::to_string(k);
    int a = in_try(0), b = in_try(1);
    int c = deref(pick(true)) + deref(pick(false)) + deref(&ext_dyn);
    std::snprintf(line->text, sizeof line->text, "thread %ld: %d %s %d %d %d %d %d %s", k,
                  tracker.value, tracker.name.c_str(), plain, ext_dyn, ext_const, a, b + c,
                  local_name.c_str());
    return line;
}

int main() {
    std::printf("main: %d %d %d %d %s\n", tracker.value, plain, ext_dyn, ext_const,
                local_name.c_str());
    pthread_t threads[4];
    for (long t = 0; t < 4; t++) pthread_create(&threads[t], nullptr, work, (void*)(t + 1));
    for (long t = 0; t < 4; t++) {
        void* out;
        pthread_join(threads[t], &out);
        std::printf("%s\n", static_cast<Line*>(out)->text);
        delete static_cast<Line*>(out);
    }
    std::printf("after: %d %d %d %d; constructed %d destroyed %d\n", tracker.value, plain,
                ext_dyn, ext_const, constructed, destroyed);
    return 0;
}
"#;

const B: &str = r"
int compute(int k) { return 40 + k; }
thread_local int ext_dyn = compute(10);
thread_local int ext_const = 7;
";

fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its half");
            None
        }
    }
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}", ran.stderr_text());
    ran.stdout_text()
}

/// `front -S -emit-llvm -O0` of `source`, compiled by hod.
fn hod_object(dir: &Scratch, front: &Path, name: &str, source: &str) -> PathBuf {
    let cpp = dir.write(&format!("{name}.cpp"), source);
    let ll = dir.join(format!("{name}.ll"));
    let mut command = Command::new(front);
    command.args(["-S", "-emit-llvm", "-O0"]).arg(&cpp).arg("-o").arg(&ll);
    run_ok(&mut command, name);
    let text = std::fs::read(&ll).unwrap();
    let options = hod::Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..hod::Options::default()
    };
    let object = hod::compile(&text, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
    dir.write(&format!("{name}.o"), object)
}

#[test]
fn thread_local_objects_run_as_clang_builds_them() {
    let dir = Scratch::new("hod-tls-cxx");
    let (Some(clang), Some(gxx)) = (tool("clang++"), tool("g++")) else {
        assert!(!tools_required(), "clang++ and g++ are required (HOD_REQUIRE_TOOLS=1)");
        return;
    };
    let Some(toolchain) = linking::Toolchain::host(&dir) else { return };
    // The reference: clang++'s own build.
    let a = dir.write("a.cpp", A);
    let b = dir.write("b.cpp", B);
    let reference = dir.join("reference");
    let mut command = Command::new(&clang);
    command.args(["-O0", "-pthread"]).arg(&a).arg(&b).arg("-o").arg(&reference);
    run_ok(&mut command, "clang++");
    let expected = run_ok(&mut Command::new(&reference), "the reference");
    assert!(expected.contains("constructed 5 destroyed 4"), "{expected}");

    let mut fronts = vec![("clang", clang)];
    fronts.extend(kiln().map(|k| ("kiln", k)));
    for (who, front) in &fronts {
        let a_obj = hod_object(&dir, front, &format!("a-{who}"), A);
        let b_obj = hod_object(&dir, front, &format!("b-{who}"), B);
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("p-{who}-{}", linker.name));
            let mut link = Command::new(&gxx);
            link.arg(format!("-fuse-ld={}", linker.name))
                .arg(format!("-B{}", dir.join(format!("use-{}", linker.name)).display()))
                .args(["-pthread", "-pie"])
                .arg(&a_obj)
                .arg(&b_obj)
                .arg("-o")
                .arg(&exe);
            run_ok(&mut link, &format!("{who} {} link", linker.name));
            let got = run_ok(&mut Command::new(&exe), &format!("{who} {}", linker.name));
            assert_eq!(got, expected, "{who} {}", linker.name);
        }
    }
}
