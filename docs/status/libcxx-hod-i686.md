# libc++ test suite on i686 through hod

## WP-339 full run (2026-11-16, control and strict, no `-g`)

All 9,247 tests of libc++'s suite (`std libcxx`, the full listing) for
`i686-unknown-linux-gnu`, run in control and strict mode with the same
setup as the WP-336 sample below (same hod binary `436a3755`, sha256
`ba2de921...`, not rebuilt; same clang 22, suite, archives, emulator and
runner flags), in 20 shards per mode. Hybrid was not run: it is to be
rerun with a newer hod against this control. Evidence and commands:
`spec/landed/harness/WP-339.md`.

Two runner changes came first (WP-339):

- hybrid's fallback `llc` now gets the control's instruction selection
  (`-global-isel=0 -fast-isel=0`), so a module hod declines is built
  exactly as the control builds it, and a hybrid-versus-control
  difference can only come from hod. On a refused module the hybrid
  object is byte-identical to the control's (the old fallback's,
  with fast-isel, is not). On the ten-test set, control and hybrid
  agree (10 PASS each; one module went to the fallback).
- strict now runs with `HOD_CENSUS=1`: the coverage log lists every
  reason of a declined module and the answer is unchanged, so strict
  alone gives the full census.

### Results

| outcome | control | strict | sample control | sample strict |
|---|---:|---:|---:|---:|
| PASS | 8,619 | 8,426 | 947 | 918 |
| FAIL | 8 | 201 | 1 | 30 |
| TIMEOUT | 1 | 1 | 0 | 0 |
| XFAIL | 33 | 33 | 26 | 26 |
| XPASS | 1 | 1 | 0 | 0 |
| UNSUPPORTED | 585 | 585 | 52 | 52 |
| total | 9,247 | 9,247 | 1,026 | 1,026 |
| tests with a refused module | - | 195 (193 FAIL, 2 XFAIL) | - | 31 |
| modules by hod | - | 7,028 of 7,223 | - | 800 of 831 |
| wall time, `-j 4`, 20 shards | 5,056 s (1 h 24 min) | 5,239 s (1 h 27 min) | | |
| back end, summed over modules | `llc` 835.7 s (7,223 modules) | hod 281.3 s (7,028 built) + 12.7 s (195 declined); longest 1.06 s | | |
| `oom_kill` | 0 | 0 | | |

Of the 9,247 tests, 7,215 compile at least one module (7,223 modules:
four tests have two, two have three). **hod builds every module of
7,020 of the 7,215 tests (97.3 %).**

**Tests that pass in the control and fail with a module hod built:
none.** Every test whose strict outcome differs from the control's
(193, all PASS to FAIL) has a module hod refused. Every test whose
modules hod built has the control's outcome.

**hod crashes and timeouts: none.** No hod step ended by a signal or a
panic (every hod exit was 0, built, or 2, refused), none took 60 s or
more (the longest took 1.06 s), and no compile timed out.

The control's failures, the same in strict:

- `std/modules/std.pass.cpp`, `std.compat.pass.cpp`: compile ("module
  not found"; the runner does not build the standard library modules).
- `std/algorithms/alg.nonmodifying/alg.find/find.pass.cpp`: clang's IR
  step fails, so no module reaches a back end.
- five `std/input.output/filesystems` tests (`copy_file_procfs`,
  `remove`, `remove_all`, the two iterator `ctor` tests) fail at run
  time under `qemu-i386`, and `ifstream.members/offset_range.pass.cpp`
  times out at run time. hod built their modules in strict; the
  outcome is the control's.
- `std.manip/setfill_wchar_max.pass.cpp` is an XPASS in both.

### Refusal census (full run)

195 tests have a declined module, each module exactly one reason (with
`HOD_CENSUS=1`), so "tests blocked" equals "blocked only by this name".
"In progress" names are being implemented by WP-337 and WP-338.

| reason | tests blocked | blocked only by it | what the module needs | state | saved module |
|---|---:|---:|---|---|---|
| `x86:intrinsic` | 139 | 139 | `llvm.is.fpclass`: `.f32` in 92 modules, `.f64` in 46, `.f80` in 1 (`stold.pass.cpp`); no other intrinsic | in progress (WP-337) | sample's `is-fpclass-{f32,f64,f80}.ll` |
| `x86:vector:no-sse2` | 49 | 49 | vector values with no SSE2: `__num_get::__atoms_offset` (13), `experimental::simd` storage and mask operations (20), `__mismatch_vectorized` (11), `__find_vectorized` (4), `<N x i32>` allocas in `is_always_lock_free` (1); counted by the first refused function | in progress (WP-338) | sample's `vector-no-sse2-v4i32.ll` |
| `intrinsic:llvm.modf.f64` | 2 | 2 | `llvm.modf.f64` (`cmath.pass.cpp`, `depr.c.headers/math_h.pass.cpp`) | in progress (WP-337) | sample's `modf-f64.ll` |
| `x86:atomic:libatomic` | 2 | 2 | `cmpxchg i64` at `align 4` (`atomic_ref<long long>`); both XFAIL in all modes | in progress (WP-337) | sample's |
| `x86:float:fp80-cast` | 1 | 1 | `bitcast x86_fp80 to i80` (`signbit.pass.cpp`) | in progress (WP-337) | sample's |
| `x86:i64:wide` | 1 | 1 | integers wider than 64 bits: `i65` from `zext`/`sext` and `llvm.sadd.with.overflow.i65` in `num_get::__do_get_integral<long long>` (`locale.num.get/user_defined_char_type.pass.cpp`) | open | `i64-wide-zext-i65.ll`, `i64-wide-sadd-overflow-i65.ll` |
| `x86:inline-asm:operand-type` | 1 | 1 | an `i64` operand to an inline-asm `"r\|m"` constraint (`DoNotOptimize<unsigned long long>`, `murmur2_or_cityhash_ubsan_unsigned_overflow_ignored.pass.cpp`) | open | `inline-asm-operand-type-i64-reg-or-mem.ll` |

Behind `x86:intrinsic`, the first refused function is `__math::isfinite`
in 85 tests, `isinf` in 30, `isnan` in 23 and `isnormal` in 1; by area,
`numerics` 54, `utilities` 32, `time` 23, `input.output` 17, the rest
under 5 each. The new modules are reduced by hand, one function each;
hod refuses each under its name and `llc -O0 -global-isel=0
-fast-isel=0` builds each. They are kept outside the repository, with
the full run's tables, in `.build/findings/libcxx-i686-full/`.

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

The full run was made by WP-339 (above). The question left open here,
whether hybrid's fallback should get `-fast-isel=0 -global-isel=0`, was
decided yes; the sample's hybrid column predates that change.
