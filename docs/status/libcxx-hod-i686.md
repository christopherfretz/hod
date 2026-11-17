# libc++ test suite on i686 through hod (sample)

## WP-336 sample (2026-11-16, control, hybrid and strict, no `-g`)

A sample of 1,026 tests of libc++'s suite for `i686-unknown-linux-gnu`,
run three ways under `qemu-i386` on the AArch64 host (design 0008, step
I14). Evidence and commands: `spec/landed/harness/WP-336.md`.

### What was run

| | |
|---|---|
| date (EST) | 2026-11-16 19:34 to 20:12 |
| hod | `436a3755` (wave 45, "join E"), release binary, sha256 `ba2de92155bbc7f9b5884b8b17d141d6c32c1f044e7ab221d8eeccf9e8826a55` |
| IR producer | clang 22 (`clang++ --target=i686-linux-gnu -S -emit-llvm`, the test's own flags). kiln at its current pin refuses `--target=i686`, so this lane does not use kiln. |
| control back end | `llc` 22 `-O0 -global-isel=0 -fast-isel=0` |
| suite | `llvmorg-22.1.8`, `libcxx/test` |
| libc++ | the i686 libc++, libc++abi and libunwind archives of `scripts/build-libcxx-i686.sh` (built by clang 22) |
| emulator | `qemu-i386 -cpu pentium2,-mmx -L /usr/i686-linux-gnu -R 0x40000000` (clang's default `i686`: x87, no SSE) |
| runner | `scripts/libcxx-hod-i686.sh <control\|hybrid\|strict> <dir> --shard K/4` (kiln's `cargo xtask libcxx` in its clang mode, `-j 4` from the lanes pool, `--timeout 160 --run-timeout-long 1920 --mem-limit 3072 --no-fail`) |
| tables | `scripts/libcxx-i686-table.py <run dir> <out dir>` |

The sample (777 selection lines) is every 12th test of the full listing
(9,247 tests, `std libcxx`) plus six whole directories:
`std/language.support/support.exception`, `support.coroutines`,
`std/atomics`, `std/numerics/c.math`, `std/utilities/charconv` and
`std/containers/sequences/vector`. After overlap that is 1,026 tests.

The three modes:

- **control**: the same IR, `llc` alone. It says whether a test can pass
  on this setup at all.
- **hybrid**: hod builds every module it accepts. A module it declines
  goes to `llc` (hod's `--fallback`), and with `HOD_CENSUS=1` every reason
  is recorded.
- **strict**: hod alone (`--strict`). A declined module fails the
  compile, and its first reason is recorded.

Which back end built a module is read from the first field of hod's
coverage log (`hod`, `fallback`, `error`), not from an exit status.

### Results

| outcome | control | hybrid | strict |
|---|---:|---:|---:|
| PASS | 947 | 946 | 918 |
| FAIL | 1 | 2 | 30 |
| XFAIL | 26 | 26 | 26 |
| UNSUPPORTED | 52 | 52 | 52 |
| total | 1,026 | 1,026 | 1,026 |
| tests with a refused module | - | 31 (built by `llc`) | 31 (29 FAIL, 2 XFAIL) |
| modules by hod | - | 800 of 831 | 800 of 831 |
| wall time, `-j 4` (sum of 4 shards) | 1,011 s | 669 s | 557 s |
| back end, summed over modules | `llc` 140.2 s | hod 20.9 s (800 modules); hod 1.2 s plus `llc` 13.9 s for the 31 fallbacks | hod 19.4 s (800 modules) |
| `oom_kill` | 0 | 0 | 0 |

Of the 1,026 tests, 831 compile one module each. The other 195 compile
nothing that reaches a back end (`.compile.pass.cpp` and `.verify.cpp`
tests, and tests the runner marks unsupported). **hod builds every
module of 800 of the 831 tests (96.3 %)**, and each of those 800 tests
has the same outcome in strict as in the control.

**Tests that pass in the control and fail with a module hod built: none.**
No test in hybrid or strict fails where hod built a module.

The other failures:

- `std/modules/std.compat.pass.cpp` fails to compile in all three modes
  ("module not found"). The runner does not build the standard library
  modules for this lane.
- `std/utilities/charconv/charconv.msvc/test.pass.cpp` fails in hybrid
  only ("round-trip failed"). hod declined that module (`x86:intrinsic`),
  so `llc` built it as the fallback. The fallback runs `llc` without
  `-fast-isel=0`, so it differs from the control's `llc`. The test seeds
  itself from `random_device`. That fallback build, rebuilt and run
  again, passed (102,400 cases). The failure is not in a module hod
  built, and has not been explained further.
- The 29 strict failures are exactly the tests with a refused module,
  listed under the census below. Two more refused tests are XFAIL in all
  three modes (`std/atomics/atomics.ref/compare_exchange_strong` and
  `compare_exchange_weak`: the link needs libatomic, which the i686
  sysroot provides only as `libatomic.so.1`).

The control's other failures are XFAILs that libc++'s annotations
expect. In the sample, no test that libc++ marks unsupported on 32-bit
targets ran and failed (the runner's `target=` lit feature stays
`aarch64`).

### Refusal census

There are 31 tests with a declined module, and each module has exactly
one reason, so "tests blocked" equals "blocked only by this name". The
counts are from the hybrid census and match strict's first refusals
test for test.

| reason | tests blocked | blocked only by it | what the module needs | saved module |
|---|---:|---:|---|---|
| `x86:intrinsic` | 20 | 20 | `llvm.is.fpclass` on x87 types: `.f32` first in 15 modules, `.f64` first in 5. `.f80` is refused too (probe), and is declared in 16 of the 31 modules | `is-fpclass-f32.ll`, `is-fpclass-f64.ll`, `is-fpclass-f80.ll` |
| `x86:vector:no-sse2` | 7 | 7 | vector values with no SSE2 (`__mismatch_vectorized`, `experimental::simd`, `__num_get::__atoms_offset`, `<N x i32>` allocas in `is_always_lock_free`) | `vector-no-sse2-v4i32.ll` |
| `x86:atomic:libatomic` | 2 | 2 | `cmpxchg i64` at `align 4` (`atomic_ref<long long>`); both tests are XFAIL in all modes | `atomic-libatomic-cmpxchg-i64-align4.ll` |
| `intrinsic:llvm.modf.f64` | 1 | 1 | `llvm.modf.f64` (`std/numerics/c.math/cmath.pass.cpp`) | `modf-f64.ll` |
| `x86:float:fp80-cast` | 1 | 1 | `bitcast x86_fp80 to i80` (`signbit(long double)`) | `fp80-cast-bitcast-i80.ll` |

The saved modules are reduced by hand, each one function. hod refuses
each of them under the name shown, and `llc` builds each.

The tests behind `x86:intrinsic` are `std::isnan`, `isinf`, `isfinite`,
`isnormal`, `lerp`, `hermite`, the complex `asin` and `sin`, `stod`,
`from_chars` (two charconv tests), two Poisson distribution tests, two
`format` tests, `print.sh.cpp`, `compare_weak_order_fallback`, and three
`chrono` time-zone or clock tests. The full per-test table holds the
control, hybrid and strict outcome of every test and its refusal.

**Landing in wave 45, not seen here.** `indirectbr`, the stack
protector, cf-protection and `llvm.eh.unwind.init` (wp/324), and
coroutine handle operations and the two-register pool (wp/331), never
occur in this sample. Coroutine bodies (WP-335) cannot occur on this
lane: clang's `-O0` pipeline splits coroutines before it writes IR, so
the modules have `.resume`/`.destroy` functions and no `llvm.coro.*`
calls. All 25 `support.coroutines` tests pass in all three modes, with
hod building each module. Nor did the sample show `coro:target`,
`x86:float:fp128`, `x86:abi:wide-arg`, `x86:atomic:op` or `x86:constant`.

### The full run (9,247 tests)

- The same runner with no `HOD_I686_SELECTION` (that is `std libcxx`),
  in about 20 shards per mode. At about 0.6 s of wall time per test at
  `-j 4` (control 1.0 s, because of slow randomized tests), each mode
  should take about 1.5 to 2.5 hours, so 5 to 7 hours for all three.
- Disk: the run directories are small (27 MB for the sample's three
  modes). The runner's scratch directories are removed per test.
- Expected: the same five names, with other x87 and vector cases among
  the tests the sample skipped. `x86:intrinsic` is almost all
  `llvm.is.fpclass`, so supporting it on x87 should remove most of
  hybrid's fallbacks.
- To be decided first: whether hybrid's fallback should pass
  `-fast-isel=0 -global-isel=0` to `llc`, so that a fallback module is
  built the way the control builds it.
