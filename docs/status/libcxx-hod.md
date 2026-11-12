# libc++ test suite through hod (hybrid and strict)

libc++'s test suite with hod in the build: kiln is the front end, and hod's
shim stands where kiln runs `llc`. In **hybrid** mode hod builds every module
its support matrix accepts and hands the rest to the real `llc`. In
**strict** mode there is no fallback. The reference is the kiln + `llc` run
of `docs/status/libcxx-baseline.md`, not 100%. There have been two
measurements: WP-077 (the first) and WP-088 (the current one, after WP-072,
074, 075, 079, 080, 086 and 068).

| | first (WP-077) | **current (WP-088)** |
|---|---|---|
| date | 2026-11-12 | 2026-11-12 |
| hod | `main` at `6a94d46`, release | `main` at `c4ecc07` (after WP-086), release |
| bitcode | via the shim's `llvm-dis` | hod's own reader (`HOD_BITCODE_VIA=native`, WP-068) |
| wall time, `-j 3` | hybrid 5564 s, strict 4241 s | hybrid 5175 s, strict 5659 s |
| host load average | 22-27 | 16-27 |
| `oom_kill` during runs | not recorded | 0 before and after each run |

Both measurements use the same setup. The suite is `llvmorg-22.1.8`
(`libcxx/test/std` and `libcxx/test/libcxx`), built against the baseline's
libc++ 22.1.8 tree. kiln is 1.0.1 at `bd60e1829` (the baseline's), LLVM is
`/usr/lib/llvm-22` behind the shim, and the target is
`aarch64-unknown-linux-gnu`. The command is kiln's
`cargo xtask libcxx --tag llvmorg-22.1.8 --compiler scripts/libcxx-hod-kiln.sh std libcxx --timeout 20 -j 3 --mem-limit 3072 --no-fail`.
The hybrid run sets `HOD_CENSUS=1`, the strict run `HOD_STRICT=1`. The
baseline took 3176 s.

## Result

| run | total | pass | fail | xfail | xpass | unsupported | timeout | pass rate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline (kiln + `llc`) | 9247 | 8609 | 4 | 47 | 1 | 585 | 1 | 99.4% |
| hybrid, first | 9247 | 8609 | 4 | 47 | 1 | 585 | 1 | 99.4% |
| **hybrid, current** | 9247 | **8608** | 4 | 47 | 1 | 585 | 2 | 99.4% |
| strict, first | 9247 | 6853 | 1761 | 47 | 1 | 585 | 0 | 79.1% |
| **strict, current** | 9247 | **8446** | 166 | 47 | 1 | 585 | 2 | 97.5% |

**Hybrid against the baseline, test by test: 0 differences caused by hod.**
Only one test changed outcome:
`std/atomics/atomics.types.operations/atomics.types.operations.wait/lost_wakeup.pass.cpp`
(PASS in the baseline, TIMEOUT now). It is a load timeout, not a hod
difference:

- The runner's 20 s limit is tight for this test. It took 17.5 s in the
  baseline and 21.6 s in the first hybrid run.
- Run alone, it timed out 3 times out of 3 in hybrid (22.4-23.5 s, its one
  module built by hod).
- Run alone with plain kiln + `llc` (the baseline's configuration, no
  shim), it also timed out 2 times out of 2 (22.3 s, 22.7 s) at the same
  load (average 20-25).
- Built by hand both ways and run without a limit, both binaries pass
  (exit 0). The hod-built one is not slower: 30.5 s and 31.1 s, against
  37.3 s and 33.9 s for the `llc`-built one.
- The `seq_cst` and acquire atomic loads in its `__cxx_atomic_load` are
  the same instructions (`ldar`) as `llc`'s.

Nothing needed reducing, so `tests/libcxx-repro/` was not created. There
was no hod crash and no internal error: no coverage line has outcome
`error`, and every fallback exited 0. No module that hod built in the first
run went back to the fallback.

Strict: 166 failures. 162 are compile steps that hod refused (163 refused
modules), and 4 are the baseline's own failures. The 2 timeouts are the
baseline's `offset_range.pass.cpp` and `lost_wakeup` again. **Every test
whose objects hod built in the hybrid run passes in strict mode too**, with
two kinds of exception that are not hod's. Of the 7055 such tests, 7023
pass. The other 32 are the baseline's own outcomes: 26 XFAIL, 4 FAIL and
1 TIMEOUT. The last one is `lost_wakeup`, the load timeout above.

## Coverage

| | first | **current** |
|---|---:|---:|
| modules through the back end | 7226 | 7226 |
| built by hod | 5438 (75.3%) | **7063 (97.7%)** |
| built by the fallback | 1788 | 163 |
| bitcode modules (coroutines, `always_inline`) | 392 (70 by hod) | 392, read natively |
| tests that build an object | 7218 | 7218 |
| all objects by hod | 5432 | **7055 (97.7%)** |
| some object by the fallback | 1786 | 163 |
| **hod-only**: passed in strict mode with an object | **5430** | **7023** (81.6% of the 8609 passing tests) |
| passing in strict mode without an object (compile-only and `verify` tests) | 1423 | 1423 |

Per test (first compile module), from the first run to the current one:
5420 stayed hod, 1616 moved from the fallback to hod, 163 stayed fallback,
0 moved from hod to the fallback. The hod-only list is `hod-only.txt` of
the run directory. It is not in git, and the recipe regenerates it.

## First refusals

These are the current run's 163 refused modules, by the reason hod declined
first (strict coverage log, field 2). "Any" counts the modules whose census
(the hybrid run's field 11) has the reason. "Tests" counts the tests whose
refused module has this first reason (one per test). How many of them only
this reason keeps from being hod-only is in the family table below, because
one construct can carry several names (`inst:icmp/vector` and
`intrinsic:llvm.vector.reduce.or`). There are only 7 distinct first
reasons, so the table is the whole list.

| # | reason | modules (first) | modules (any) | tests | where | what | package |
|---:|---|---:|---:|---:|---|---|---|
| 1 | `inst:extractvalue/fp128` | 96 | 107 | 96 | `crates/hod_ir/src/classify/matrix.rs:1082` (row's `fp128` class) | `long double` (`std::complex<long double>` 31+, atomics, `<bit>`) | WP-084 |
| 2 | `inst:icmp/vector` | 35 | 36 | 35 | `matrix.rs:853` | vector compares (libc++'s vectorised `find`/`mismatch`, `<locale>`) | WP-081 |
| 3 | `inst:insertelement/i128` | 17 | 21 | 17 | `matrix.rs:1126` | `std/experimental/simd` with 128-bit lanes | none |
| 4 | `inst:sitofp/i128` | 6 | 10 | 6 | `matrix.rs:952` | `<random>` | WP-084 |
| 5 | `intrinsic:llvm.umul.with.overflow/i128` | 3 | 3 | 3 | `matrix.rs:1485` | `<charconv>`, `<numeric>` | WP-084 |
| 6 | `linkage:available_externally` | 3 | 3 | 3 | `matrix.rs:482` | `<locale>` | WP-021 |
| 7 | `inst:fptosi/i128` | 3 | 8 | 3 | `matrix.rs:926` | `experimental/simd` | WP-084 |

Reasons that occur in a census but are never first:
`intrinsic:llvm.vector.reduce.or` (26 modules), `.and` (10),
`inst:extractelement/i128` (20), `inst:insertelement/fp128` and
`inst:extractelement/fp128` (13 each), `inst:fptoui/i128` (8),
`inst:uitofp/i128` (7), and `inst:fcmp/vector` (1).

By family (hybrid census; the tests that only this family keeps from being
hod-only):

| family | tests | modules (any) | package |
|---|---:|---:|---|
| `fp128` operations | 96 | 109 | WP-084 |
| vectors (`icmp`/`fcmp` on vectors, `vector.reduce.*`, element rows) | 34 | 36 | WP-081 |
| `i128` operations (conversions, `umul.with.overflow`, `i128` lanes) | 16 | 30 | WP-084; lanes unplanned |
| `linkage:available_externally` | 3 | 3 | WP-021 |

**WP-084 (`fp128` and `i128`) frees the most: about 112 of the 163
remaining tests, and WP-081 frees about 35.** Together they would leave
only the `available_externally` modules and some of the 17 `i128`-lane simd
modules.

The first measurement's first refusals are kept for reference. Each family
named there has since landed or shrunk:

| reason (first run) | modules (first) | now |
|---|---:|---|
| `attr:convergent` | 1055 | accepted (WP-086) |
| `mem:atomic-load`, `inst:atomicrmw`, `mem:atomic-store`, `inst:cmpxchg`, `inst:fence` | 230 + 176 + 37 + 8 + 2 | accepted (WP-074) |
| `call:inline-asm` | 97 | accepted (WP-075) |
| `operand:address-in-aggregate`, `operand:int-of-addr` | 83 + 11 | accepted (WP-072) |
| `inst:extractvalue/fp128` | 51 | 96 first (it was behind other reasons before) |
| `type:vector`, `inst:insertelement` | 30 + 1 | the base and lanes accepted (WP-079, WP-080); masks remain |
| `linkage:available_externally`, `llvm.umul.with.overflow/i128`, `inst:sitofp/i128` | 3 + 3 + 1 | unchanged |

## Time

The back end alone, from the coverage logs (wall microseconds per module,
measured inside the suite runs under the same load):

| | first | **current** |
|---|---:|---:|
| hod, modules it built | 5438 modules, 2878 MB, 90.3 s | 7063 modules, 4563 MB, 138.2 s |
| hod, before declining | 1788 modules, 62.2 s (32.4 s of it `llvm-dis`) | 163 modules, 4.9 s with census, 3.9 s in the strict run |
| fallback `llc` | 1788 modules, 435.5 s | 163 modules (488 MB), 72.7 s |
| **hybrid back end, total** | **588 s** | **216 s** |

`scripts/libcxx-hod-time.sh` regenerates sampled modules with kiln
(`-S -emit-llvm`, the test's own command line) and times
`llc -O0 -relocation-model=pic -filetype=obj` (kiln's invocation) against
`hod` (`HOD_THREADS=1 HOD_STRICT=1`, no census, no coverage log), best of 3
each, one process at a time, on the same text. The sample is the first
run's 198 modules (by test, `SAMPLE=`) plus every 10th of the 1616 tests
newly built by hod (162). The load average was 16-20.

| sample | modules | IR | `llc` | hod | total ratio | median ratio | range | hod slower |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| first run's 198, first measurement | 198 | 167 MB | 21.6 s | 6.1 s | 3.5x | 2.3x | 0.65-7.9x | 20 |
| first run's 198, now | 198 | 167 MB | 19.9 s | 5.9 s | 3.4x | 2.4x | 0.58-9.1x | 39 |
| newly built 162 | 162 | 275 MB | 32.9 s | 6.8 s | 4.9x | 2.7x | 0.57-10.4x | 4 |
| **all 360** | 360 | 442 MB | 52.8 s | 12.6 s | **4.2x** | **2.5x** | 0.57-10.4x | 43 |

The per-module medians are 55.9 ms for `llc` and 25.0 ms for hod. **The
modules where hod is slower are all small (1-40 kB, 16-37 ms), and the
cause is where the binary lives, not code generation.** The script runs
hod from the workspace bind mount (`.build/target-*/release/hod`) and `llc`
from the container's local `/usr`. On the 1.2 kB `main`-only module of
`contiguous_iterators.pass.cpp`:

| | time |
|---|---:|
| hod from the bind mount | 11-13 ms (19-24 ms through `env` at a higher load) |
| the same hod binary, copied to local disk | 3.2 ms |
| `llc` | 15-18 ms |
| hod's own phases (`HOD_TIME_PHASES`) | 7.6 ms total |

This is the start-up cost on the mount that `spec/landed/cli/WP-068.md`
measured (5.6 ms on the mount at a lower load; there no module was slower,
with input on stdin). The first run's 198 modules give the same ratio
(3.4x against 3.5x), so this sample shows no regression.

A plain-`llc` back end for the whole suite would have needed about 650 s:
hod's 138 s scaled by the sample's 4.2x, plus the fallback's 73 s. hybrid
needs 216 s, about 3x less. hod now spends 4-5 s declining modules, where
it spent 62 s before.

## Repeating the run

From the wrapper repository root, with kiln built
(`.build/target-kiln/release/kiln++`) and the libc++ tree fetched
(`scripts/fetch-libcxx22.sh`):

```sh
R=$PWD; W=$R/hod/.worktrees/wp-088   # or the hod checkout
L=$R/.build/libcxx-hod-2; T=$R/.build/target-$(basename $W)
(cd $W && source scripts/env.sh && cargo build --release -p hod_cli &&
 cargo xtask shim --llvm /usr/lib/llvm-22 --hod $T/release/hod \
   --out $T/llvm-22-shim --kiln $R/.build/target-kiln/release/kiln++)
export HOD_LIBCXX_KILN=$T/llvm-22-shim/kiln++-hod
# hybrid; for strict: HOD_STRICT=1 instead of HOD_CENSUS=1, another log dir
grep oom_kill /sys/fs/cgroup/memory.events   # before and after each run
(cd $R/kiln && source scripts/env.sh &&
 HOD_LIBCXX_LOGDIR=$L/hybrid HOD_CENSUS=1 cargo xtask libcxx --tag llvmorg-22.1.8 \
   --compiler $W/scripts/libcxx-hod-kiln.sh std libcxx --timeout 20 -j 3 \
   --mem-limit 3072 --no-fail --results $L/hybrid.tsv)
python3 -I $W/scripts/libcxx-hod-report.py $L/hybrid $L/hybrid.tsv $R/.build/libcxx-baseline/baseline.tsv
python3 -I $W/scripts/libcxx-hod-report.py $L/hybrid --hod-only   # all objects by hod
# a single test alone: add --filter <path substring> -j 1 (with or without --compiler)
SAMPLE=$L/sample-all.txt HOD=$T/release/hod KILN=$R/.build/target-kiln/release/kiln++ \
  sh $W/scripts/libcxx-hod-time.sh $L/hybrid $T/time 400 > $L/time-sample.tsv
```

`SAMPLE` is a file of test paths (relative to `libcxx/test`). Without it,
the script takes every 10th hod-built module. kiln's runner cannot resume a
run, so a run that is interrupted starts over.

How the shim is put in front of kiln without changing kiln: kiln's runner
sets `KILN_LLVM_DIR` for every step, and `kiln++-hod` passes `--llvm-dir=`
first, which beats the variable (`spec/harness.md` "shim").
`scripts/libcxx-hod-kiln.sh` is the `--compiler`. It runs `kiln++-hod`.
With `HOD_LIBCXX_LOGDIR` set, it keys each invocation by its scratch
directory (the runner's working directory for a test's steps), appends the
command line to `args.tsv`, and points `HOD_COVERAGE_LOG` and
`HOD_TIME_PHASES` at `cov/<key>.tsv` and `phases/<key>.tsv`, so every
coverage line maps to its test. Raw results stay in the run directories:
`.build/libcxx-hod/` (first) and `.build/libcxx-hod-2/` (current:
`hybrid.{log,tsv}`, `strict.{log,tsv}`, `hybrid/`, `strict/`,
`report-{hybrid,strict}.md`, `time-sample.tsv`, `sample-*.txt`,
`hod-only.txt`, `lw-*` for the lost_wakeup reruns, and
`hybrid-interrupted/`, the partial run that a container restart stopped).

## What a later package should automate

kiln's runner stays as it is. hod's side is setup and reading, which fits a
`cargo xtask libcxx-hod` command with three verbs:

- `setup`: build the release `hod`, the shim (`xtask::shim::build`) and the
  wrapper, and print the kiln command to run. hod cannot run kiln's xtask
  for you without a second cargo in another repository, and printing the
  command keeps the ownership line clean.
- `report <logdir> <results.tsv> <baseline.tsv>`: `libcxx-hod-report.py`
  in Rust, reusing `irdiff`'s coverage-line reader. It would write the
  generated tables of this page between markers, as kiln's `--report`
  does. It would fail when the pass list differs from the baseline (the
  hybrid gate) or when a test in the given hod-only list fails strictly
  (the strict gate). It should also compare against a previous run's
  coverage (no module from hod back to the fallback).
- `hod-only <logdir>`: the list for the strict gate, whose regressions are
  the code generation bugs this suite can find.

A load-sensitive test like `lost_wakeup` needs a rerun rule in that gate:
a TIMEOUT counts only when the test also passes with plain `llc` at the
same moment.
