//! C++ atomics through the whole pipeline (WP-074, `spec/codegen.md`
//! §5.5.1 "Atomics"): `std::atomic` of several types and orderings,
//! `std::atomic_flag` as a spinlock, `compare_exchange_weak` and
//! `_strong`, `std::shared_ptr` copied on four threads (its reference
//! counts), a function-local static (its guard), compiled to `.ll` by
//! kiln++ and by clang++ at `-O0` and `-O1`, built by `hod::compile`
//! (which refuses rather than falls back), linked under every linker and
//! run; every build prints what g++'s build of the same source prints.
//!
//! Needs `kiln++` (`$HOD_KILN`, or the kiln build beside this checkout's
//! target directory) and `clang++`; a missing one is skipped with a note,
//! or fails under `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const PROGRAM: &str = r#"
#include <atomic>
#include <cstdio>
#include <memory>
#include <thread>
#include <vector>

static std::atomic<int> counter{0};
static std::atomic<unsigned long long> big{0};
static std::atomic<short> small{0};
static std::atomic<unsigned char> tiny{0};
static std::atomic<double> real{0.0};
static std::atomic<float> single{0.0f};
static std::atomic_flag flag = ATOMIC_FLAG_INIT;
static long guarded = 0;
static std::atomic<long> casd{0};
static std::atomic<int> constructed{0};
static std::atomic<int *> pointer{nullptr};
static int cells[4] = {1, 2, 3, 4};

struct Thing {
  Thing() { constructed.fetch_add(1, std::memory_order_relaxed); }
  int value = 42;
};

static Thing &instance() {
  static Thing thing;
  return thing;
}

static void work(std::shared_ptr<int> shared, int tid, int n, long *sum) {
  for (int i = 0; i < n; i++) {
    counter.fetch_add(1);
    big.fetch_add(3, std::memory_order_relaxed);
    small.fetch_sub(1, std::memory_order_acq_rel);
    tiny.fetch_or((unsigned char)(1u << tid), std::memory_order_release);
    tiny.fetch_xor((unsigned char)(16u << tid), std::memory_order_seq_cst);
    real.fetch_add(1.0);
    single.fetch_sub(0.5f, std::memory_order_relaxed);
    while (flag.test_and_set(std::memory_order_acquire)) {
    }
    guarded++;
    flag.clear(std::memory_order_release);
    long seen = casd.load(std::memory_order_relaxed);
    while (!casd.compare_exchange_weak(seen, seen + 2, std::memory_order_acq_rel,
                                       std::memory_order_relaxed)) {
    }
    int *was = pointer.exchange(&cells[tid], std::memory_order_acq_rel);
    if (was && (*was < 1 || *was > 4)) {
      *sum -= 1000000;
    }
    std::shared_ptr<int> copy = shared;
    std::weak_ptr<int> weak = copy;
    *sum += *copy + instance().value + (weak.lock() ? 1 : 0);
  }
}

int main() {
  const int threads = 4, n = 3000;
  auto shared = std::make_shared<int>(1);
  std::vector<std::thread> pool;
  std::vector<long> sums(threads);
  for (int t = 0; t < threads; t++) pool.emplace_back(work, shared, t, n, &sums[t]);
  for (auto &t : pool) t.join();
  long total = 0;
  for (long s : sums) total += s;
  std::printf("counter %d big %llu small %d tiny %u real %.1f single %.1f\n", counter.load(),
              big.load(), (int)small.load(), (unsigned)tiny.load(), real.load(),
              (double)single.load());
  std::printf("guarded %ld cas %ld sum %ld constructed %d use_count %ld\n", guarded,
              casd.load(), total, constructed.load(), shared.use_count());
  int expected = threads * n;
  bool swapped = counter.compare_exchange_strong(expected, -1);
  int other = 7;
  bool failed = counter.compare_exchange_strong(other, 9);
  std::atomic_thread_fence(std::memory_order_seq_cst);
  std::atomic_signal_fence(std::memory_order_seq_cst);
  std::printf("strong %d failed %d saw %d now %d\n", (int)swapped, (int)failed, other,
              counter.exchange(5));
  return 0;
}
"#;

/// The `kiln++` to test with, or `None` to skip.
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
            eprintln!("note: no kiln++ (set HOD_KILN); skipping kiln's builds");
            None
        }
    }
}

fn run(command: &mut Command, what: &str) {
    let outcome = run_with(command, &[], Duration::from_mins(3));
    assert!(outcome.success(), "{what}: {:?}\n{}", outcome.status, outcome.stderr_text());
}

#[test]
fn cxx_atomics_build_and_run_like_gcc() {
    let scratch = Scratch::new("atomic-cxx");
    let Some(toolchain) = linking::Toolchain::host(&scratch) else { return };
    let source = scratch.write("atomics.cpp", PROGRAM);
    let Some(gxx) = tool("g++") else { return };
    let reference = scratch.join("gcc");
    run(
        Command::new(&gxx)
            .args(["-std=c++20", "-O0", "-pthread"])
            .arg(&source)
            .arg("-o")
            .arg(&reference),
        "g++",
    );
    let want = run_with(&mut Command::new(&reference), &[], Duration::from_mins(2));
    assert!(want.success(), "{want:?}");
    let want = want.stdout_text();
    assert!(want.contains("counter 12000 big 36000 small -12000 tiny 15"), "{want}");
    let mut fronts: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(kiln) = kiln() {
        fronts.push(("kiln", kiln));
    }
    match tool("clang++") {
        Some(clang) => fronts.push(("clang", clang)),
        None => assert!(!tools_required(), "no clang++ (HOD_REQUIRE_TOOLS=1)"),
    }
    let mut runs = 0;
    for (front, compiler) in &fronts {
        for opt in ["-O0", "-O1"] {
            let ll = scratch.join(format!("{front}{opt}.ll"));
            run(
                Command::new(compiler)
                    .args(["-std=c++20", opt, "-S", "-emit-llvm"])
                    .arg(&source)
                    .arg("-o")
                    .arg(&ll),
                front,
            );
            let text = std::fs::read(&ll).unwrap();
            assert!(
                text.windows(9).any(|w| w == b"atomicrmw")
                    && text.windows(7).any(|w| w == b"cmpxchg"),
                "{front}{opt}: no atomics in the module"
            );
            let options = Options {
                output_kind: OutputKind::Object,
                relocation_model: Some(RelocModel::Pic),
                ..Options::default()
            };
            let object = hod::compile(&text, &options)
                .unwrap_or_else(|e| panic!("{front}{opt}: hod refused: {e}"));
            let object = scratch.write(&format!("{front}{opt}.o"), object);
            for linker in &toolchain.linkers {
                let program = scratch.join(format!("{front}{opt}-{}", linker.name));
                let flags = ["-pthread"];
                let inputs = [object.as_path(), Path::new("-lstdc++"), Path::new("-lm")];
                for _ in 0..2 {
                    let ran = toolchain.link_and_run(linker, &flags, &inputs, &program);
                    assert!(ran.success(), "{front}{opt} {}: {ran:?}", linker.name);
                    assert_eq!(ran.stdout_text(), want, "{front}{opt} {}", linker.name);
                    runs += 1;
                }
            }
        }
    }
    eprintln!(
        "atomic cxx: {} front ends x 2 levels, {runs} runs, all print what g++'s build prints",
        fronts.len()
    );
}
