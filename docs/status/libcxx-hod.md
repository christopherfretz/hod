# libc++ test suite through hod (hybrid and strict)

## At the candidate a68a0d5d (release 1.1.0, RELEASE.md 4.3, 2026-11-17, strict, no `-g`)

The 1.1.0 release candidate `C`, hod `main` at `a68a0d5d` (frozen release
binary, sha256
`baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`;
`--version` prints `hod version 1.0.0`, the number moves after the
lanes), with kiln 1.2.0 at `K` = `8f93304ee`, LLVM `/usr/lib/llvm-22`
behind a shim made from the frozen binary. The command is RELEASE.md
4.3's, kiln's runner with `scripts/libcxx-hod-kiln.sh` as its
`--compiler`, `HOD_STRICT=1`, `--timeout 20 -j 3 --mem-limit 3072
--no-fail`, on the lanes' jobserver pool, the suite `llvmorg-22.1.8`.

The control is the one recorded at `K` and reused (kiln + `llc`, no hod
in it): `.build/libcxx-4/native-k12/control.tsv`, the same control as the
`cd6ef6bc` and `02fe2d02` sections below. `.build/libcxx-baseline/baseline.tsv`
(`docs/status/libcxx-baseline.md`) was recorded with kiln 1.0.1 at
`bd60e1829`, not at `K`, so it is not the control of this run; the
comparison with it is given below for reference.

| | recorded control at `K` (kiln + `llc`) | **strict at `a68a0d5d`** |
|---|---|---|
| date (EST) | 2026-11-15 04:25 to 05:35 | 2026-11-17 06:36:49 to 07:44:11 |
| total | 9,247 | 9,247 |
| pass | 8,608 | 8,608 |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 (24 driver) | 585 (24 driver) |
| timeout | 2 | 2 |
| killed, error | 0, 0 | 0, 0 |
| wall time, `-j 3` | 4,240.6 s | 4,041.9 s |
| `oom_kill` (cgroup `memory.events`) | 1 before, 1 after | 0 before, 0 after |
| modules by hod | - | 7,226 of 7,226; 0 refused, 0 fallback, 0 outcome error |

**Result: strict equals the control, test by test** (`libcxx-hod-report.py`:
0 changed outcomes). In strict, 7,218 tests have objects, all of them
built by hod, and 7,185 of those pass; the inputs are 392 bitcode and
6,834 text modules; hod's time summed over modules is 149.6 s (phases
152.7 s: parse 41.4, emission 33.6, write 28.6, bodies 22.7, read 17.0).
The timeouts (`lost_wakeup.pass.cpp`, `offset_range.pass.cpp`), the four
filesystem failures (`directory_iterator` and
`recursive_directory_iterator` constructors, `fs.op.remove`,
`fs.op.remove_all`) and the XPASSes (`rel_ops.depr_in_cxx20.verify.cpp`,
`variant.hash/hash.depr.verify.cpp`) are the same in both. The run
shared the host with a full gate, a fuzz run and the i686 libc++ lane.

Against the older `baseline.tsv` (kiln 1.0.1), two outcomes differ:
`rel_ops.depr_in_cxx20.verify.cpp` (XFAIL there, XPASS here, as in the
control at `K`: kiln's change, no code is generated for a `verify` test)
and `atomics.types.operations.wait/lost_wakeup.pass.cpp` (PASS there,
TIMEOUT here). `lost_wakeup` alone at `-j 1` and 20 s, three times per
mode (2026-11-17 07:45 to 07:51 EST): TIMEOUT in all six runs, with kiln
and `llc` and with hod strict; it times out in the control at `K` too.

Raw results: `.build/release-1.1.0/libcxx-a64/` (`strict.{log,tsv}`,
`strict/`, `report-strict.txt`, `report-strict-vs-baseline-bd60e.txt`,
`rerun-lw.log`, `lw-*`, `run-strict.sh`, `shim/`). Not covered here: the
native runners (4.5, the "Native ... runner" sections below are of an
earlier hosted run, not of `a68a0d5d`); hybrid was not run (strict
refused nothing).

## At the candidate cd6ef6bc (WP-233, 2026-11-15, strict, no `-g`)

The 1.0 release candidate, hod `main` at `cd6ef6bc` (release binary,
sha256 `80138ec1411506d5b4048f1eac49e8b377f9eeedb21e70ae43e3c81ab094929c`),
with kiln 1.2.0 at `8f93304ee`, strict against the same recorded control
as the `02fe2d02` section below, with the same command and settings.

| | recorded control (kiln + `llc`) | **strict at `cd6ef6bc`** |
|---|---|---|
| date (EST) | 2026-11-15 04:25 to 05:35 | 2026-11-15 18:41 to 20:47 |
| total | 9,247 | 9,247 |
| pass | 8,608 | 8,607 |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 | 585 |
| timeout | 2 | 3 |
| wall time, `-j 3` | 4,241 s | 7,538 s (a heavier host load) |
| `oom_kill` | 1 before, 1 after | 1 before, 1 after |
| modules by hod | - | 7,226 of 7,226, 0 fallback (hod 254.1 s summed over modules) |

**Result: strict equals the control.** The one changed outcome is
`ifstream.members/buffered_reads.pass.cpp` (PASS in the control, TIMEOUT
at the 20 s limit in strict), the load-dependent test described below;
alone at `-j 1` and 120 s it passes with hod (42.8 s).
`lost_wakeup.pass.cpp` and `offset_range.pass.cpp` time out in both
modes. The ten `std/strings/basic.string` tests that the intermediate
candidate `e98267c0` refused (`intrinsic:const-fold-nounwind-invoke`)
all pass. The failures (the four filesystem tests) and the XPASSes are
the same in both modes.

## At the candidate 02fe2d02 (WP-233, 2026-11-15, strict, no `-g`)

The 1.0 release candidate, hod `main` at `02fe2d02` (release binary,
sha256 `5c1e181de7f62e3a644a11b536495429674c76bb0be6203c5bf85a8c19c3633d`),
with kiln 1.2.0 at `8f93304ee`. Strict ran against the control recorded
on that kiln the same morning (below), with the same command and settings:
`scripts/libcxx-hod-debug.sh control|strict <dir>`, `HOD_DEBUG_FLAGS=`
(no `-g`), `HOD_JOBS=3` on the lanes' jobserver pool, `--timeout 20
--mem-limit 3072 --no-fail`.

| | recorded control (kiln + `llc`) | **strict at `02fe2d02`** |
|---|---|---|
| date (EST) | 2026-11-15 04:25 to 05:35 | 2026-11-15 06:08 to 08:24 |
| total | 9,247 | 9,247 |
| pass | 8,608 | **8,608** |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 | 585 |
| timeout | 2 | 2 |
| wall time, `-j 3` | 4,241 s | 8,164 s (a heavier host load) |
| `oom_kill` | 1 before, 1 after | 1 before, 1 after |
| modules by hod | - | 7,226 of 7,226, 0 fallback (hod 287.5 s summed over modules) |

**Result: strict equals the control.** Two tests swapped outcomes, both
at the 20 s limit: `ifstream.members/buffered_reads.pass.cpp` (PASS in
the control, TIMEOUT in strict) and
`atomics.types.operations.wait/lost_wakeup.pass.cpp` (TIMEOUT in the
control, PASS in strict). Alone at `-j 1` with the candidate, each timed
out twice in each mode at 20 s (26 to 32 s) and passed once in each mode
at 120 s (`buffered_reads` 42.9 s with `llc`, 45.8 s with hod;
`lost_wakeup` 31.9 s and 29.1 s). The failures (the four filesystem
tests), the timeout `offset_range.pass.cpp` and the XPASSes are the same
in both modes.

## Fourth measurement (WP-233, 2026-11-14, strict and control, no `-g`)

The whole suite natively on the AArch64 host, re-measured after about
105 packages had landed since the third measurement, to find any
regression. Strict hod (no fallback) and a control (kiln + `llc`) ran
one after the other on the same host, with the same kiln, so the control
is the reference, test by test. Evidence and commands:
`spec/landed/harness/WP-233.md`.

| | control (kiln + `llc`) | **strict (hod, no fallback)** |
|---|---|---|
| date | 2026-11-14, 16:51 to 18:23 EST | 2026-11-14, 18:23 to 19:49 EST |
| hod | - | `main` at `46076a33`, release, a frozen copy (sha256 `e96294c9...35d3`) |
| kiln | 1.1.0 at `21b9110a4`, release | the same |
| total | 9,247 | 9,247 |
| pass | 8,607 | **8,608** |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 | 585 |
| timeout | 3 | 2 |
| wall time, `-j 4` | 5,526 s | 5,146 s |
| `oom_kill` | 1 before, 1 after | 1 before, 1 after |
| modules by hod | - | 7,226 of 7,226 (0 refusals, 0 fallback; hod 249 s summed over modules) |

Setup: suite `llvmorg-22.1.8`, LLVM `/usr/lib/llvm-22` behind the shim,
target `aarch64-unknown-linux-gnu`, host load average 20 to 40, the
runner's own `-j 4` (`HOD_JOBSERVER_FIFO` set to a path that does not
exist), `--timeout 20 --mem-limit 3072 --no-fail`. The command is
`scripts/libcxx-hod-debug.sh control|strict <dir>` with `HOD_DEBUG_FLAGS=`
set and empty, which leaves `-g` out (WP-233 added that option; the
chain is otherwise WP-210's).

**Result: strict equals the control.** One test changed outcome:
`std/input.output/file.streams/fstreams/ifstream.members/buffered_reads.pass.cpp`,
TIMEOUT in the control and PASS in strict. Alone at `-j 1`, twice in each
mode, it timed out in both modes at the 20 s limit (26 to 30 s); with
`--timeout 120` it passes in both (28 s with `llc`, 37 s with hod). It is
a test that takes about the limit, not a hod difference. Both modes share
everything else:

- the four filesystem failures (`directory_iterator` and
  `recursive_directory_iterator` `ctor.pass.cpp`, `remove.pass.cpp`,
  `remove_all.pass.cpp`): the host's environment, as before;
- the timeouts `lost_wakeup.pass.cpp` (passes alone at 120 s in both
  modes, 47 s and 42 s) and `offset_range.pass.cpp` (times out alone at
  120 s in both modes);
- XPASS `libcxx/depr/depr.rel_ops/rel_ops.depr_in_cxx20.verify.cpp` and
  `std/utilities/variant/variant.hash/hash.depr.verify.cpp` (kiln's
  diagnostics, the same with either back end).

The kiln pin moved to 1.2.0 (`8f93304ee`) and hod `main` to `56a4695f`
right after this run. The x86-64 legs of WP-233
(`docs/status/libcxx-hod-x64.md`) ran on that pair. These native numbers
are therefore kiln 1.1.0 with hod `46076a33` until they are re-measured.

**Recorded control on kiln 1.2.0** (`8f93304ee`), for the release
candidate's strict run to be compared against: the same command,
`HOD_JOBS=3` on the lanes' jobserver pool, 2026-11-15 04:25 to 05:35 EST.
9,247 tests: 8,608 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 2
timeout; wall 4,241 s; `oom_kill` 1 before and after. It differs from the
kiln 1.1.0 control in one test only, `buffered_reads.pass.cpp` (TIMEOUT
then, PASS now, the test that takes about the limit). Its failures,
timeouts (`lost_wakeup`, `offset_range`) and XPASSes are the ones listed
above.

## Earlier measurements (WP-077, WP-088, WP-102)

libc++'s test suite with hod in the build: kiln is the front end, and hod's
shim stands where kiln runs `llc`. In **hybrid** mode hod builds every module
its support matrix accepts and hands the rest to the real `llc`. In
**strict** mode there is no fallback. The reference is the kiln + `llc` run
of `docs/status/libcxx-baseline.md`, not 100%. There have been three
measurements: WP-077 (the first), WP-088 (the second, after WP-072,
074, 075, 079, 080, 086 and 068) and WP-102 (the third, strict only,
after WP-081, 082, 083, 084, 095 and 021). In the sections after the third
measurement's, "current" means the second.

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

## Native AArch64 runner

Run [44078910283](https://github.com/christopherfretz/hod/actions/runs/44078910283)
of `.github/workflows/sweep.yml` (`spec/harness.md` "CI", rule 6),
2026-11-17, of exactly the 1.1.0 candidate `a68a0d5d`: kiln 1.2.0 at
`8f93304ee`, LLVM 22.1.8, libc++ 22.1.8, the locales generated; kiln's
`cargo xtask libcxx std libcxx --tag llvmorg-22.1.8 -j 4 --timeout 60
--mem-limit 3072 --no-fail` in six shards, once with hod strict
(`HOD_STRICT=1`, no fallback) and once with kiln++ and `llc` alone,
natively on the runner. Every test binary runs on the runner's own CPU,
no emulator.

| `ubuntu-24.04-arm` | total | pass | fail | xfail | xpass | unsupported | timeout | shards (s) | modules by hod |
|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| hod, strict | 9,247 | 8,749 | 4 | 46 | 2 | 446 | 0 | 323 to 372 | 7,365 of 7,365 |
| `llc` | 9,247 | 8,749 | 4 | 46 | 2 | 446 | 0 | 328 to 402 | - |

No test's outcome differs between hod and `llc`: the run's summary lists
none, and no test passes with `llc` and not with hod. The 4 failures are
therefore the same tests on both sides (the runner's environment, not
hod). The earlier hosted run of this section (43465227200, 2026-11-13,
one test refused by name) is in this file's history.

## Third measurement (WP-102, strict only)

`main` at `6e6bf9c` (after WP-081, 082, 083, 084, 095 and 021), release
`hod`, the same setup and command as above with `HOD_STRICT=1`. kiln and
LLVM are unchanged. There was no hybrid run and no time sample. The run took
5826 s at `-j 3`, with a load average of 18-24 and `oom_kill` 0 before and
after.

| strict | first (WP-077) | second (WP-088) | **third (WP-102)** |
|---|---:|---:|---:|
| pass (baseline 8609) | 6853 | 8446 | **8586** |
| fail | 1761 | 166 | 25 |
| timeout | 0 | 2 | 3 |
| pass rate | 79.1% | 97.5% | **99.1%** |
| modules built by hod (of 7226) | 5438 (75.3%) | 7063 (97.7%) | **7205 (99.7%)** |
| refused modules | 1788 | 163 | **21** |
| tests with all objects by hod (of 7218) | 5432 | 7055 | **7197 (99.7%)** |
| **hod-only** (passed strictly with an object) | 5430 | 7023 | **7163** (83.2% of 8609) |
| passing without an object | 1423 | 1423 | 1423 |
| hod back end, modules it built | 90.3 s | 138.2 s (hybrid) | 160.6 s (strict) |

Outcome by outcome: 9247 tests, 8586 PASS, 25 FAIL, 47 XFAIL, 1 XPASS,
585 UNSUPPORTED, 3 TIMEOUT. The 25 FAIL are 21 refused compile steps and the
baseline's own 4 failures. The 23 tests that the baseline passes and this run
does not are:

- the 21 refused tests below;
- 2 load timeouts that are not hod's (below).

**Every test whose objects hod built has the baseline's outcome**, except
for the 2 load timeouts. Of the 7197 such tests, 7163 pass. Of the other
34, 27 are XFAIL, 4 FAIL and 1 TIMEOUT (`offset_range.pass.cpp`), the
baseline's own outcomes. No coverage line has an outcome other than `hod` or
`error`, and there was no hod crash.

**The quick check came first.** It ran only the 163 tests whose modules the
second strict run refused (162 FAIL and 1 XFAIL). Those are the tests with an
`error` line in `.build/libcxx-hod-2/strict/cov/`, keyed through
`strict/args.tsv`. kiln's runner takes each path as a positional "directory"
(a file path selects that one test). 141 now pass and 1 is XFAIL as in the
baseline (`std/atomics/atomics.ref/notify_all.pass.cpp`). The other 21 are
refused, all for `inst:insertelement/i128`. The whole run confirms this.

### First refusals (third measurement)

There is one reason left, so this is the whole table:

| # | reason | modules (first) | modules (any) | tests | where | what | package |
|---:|---|---:|---:|---:|---|---|---|
| 1 | `inst:insertelement/i128` | 21 | 21 | 21 | `crates/hod_ir/src/classify/matrix.rs:1217` (the row has no `I128` class) | `std/experimental/simd` with 128-bit lanes (`simd.class` 8, `simd.mask.class` 6, `simd.reference` 6) and `std/algorithms/alg.nonmodifying/alg.find/find.pass.cpp` | WP-101 |

From the second measurement's seven reasons to now:

| reason (second run) | modules (second) | now | package |
|---|---:|---:|---|
| `inst:extractvalue/fp128` | 96 | 0 | WP-084 |
| `inst:icmp/vector` | 35 | 0 | WP-081 |
| `inst:insertelement/i128` | 17 | 21 (4 were behind other reasons) | WP-101 |
| `inst:sitofp/i128` | 6 | 0 | WP-084 |
| `intrinsic:llvm.umul.with.overflow/i128` | 3 | 0 | WP-084 |
| `linkage:available_externally` | 3 | 0 | WP-021 |
| `inst:fptosi/i128` | 3 | 0 | WP-084 |

### The two load timeouts

Both pass in the baseline and have a TIMEOUT here (the runner's 20 s limit
on the test binary). I reran each one alone (`--filter`, `-j 1`) twice
strictly through hod and twice with plain kiln + `llc` (`--compiler kiln++`)
at the same load (load average 18-34):

| test | strict run | hod alone | `llc` alone | 120 s limit, hod | 120 s limit, `llc` |
|---|---|---|---|---|---|
| `atomics.types.operations.wait/lost_wakeup.pass.cpp` | TIMEOUT 22.3 s | TIMEOUT 22.1, 22.7 s | TIMEOUT 21.9, 21.6 s | | |
| `ifstream.members/buffered_reads.pass.cpp` | TIMEOUT 22.1 s | TIMEOUT 24.9, 22.7 s | TIMEOUT 23.0 s, PASS 20.5 s | PASS 26.1, 30.9 s | PASS 33.3, 20.8 s |

`lost_wakeup` is the second measurement's load timeout again. `buffered_reads`
passed in the second strict run (14.6 s, its object built by hod). It times
out with `llc` too at this load. Given time, it passes both ways, and the
hod build is not slower. Neither is hod's, so nothing needed reducing and
`tests/libcxx-repro/` was still not created.

Raw results are in `.build/libcxx-hod-3/`:
- `refused-2.txt`: the quick check's list
- `quick.{log,tsv}` and `quick/`: the quick check
- `strict.{log,tsv}`, `strict/`, `strict.{start,end,oom-*,load-*}`: the whole run
- `report-strict.md` and `hod-only-strict-objs.txt`: the report and the
  tests with all objects by hod
- `buffered_reads-*`, `lost_wakeup-*` and `br-nolimit-*`: the reruns

The sections below are the second measurement's, as they were.

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
