# libc++ test suite through hod with `-g` (native AArch64)

## At the candidate a68a0d5d (release 1.1.0, RELEASE.md 4.6, 2026-11-17, strict, `-g`)

The 1.1.0 release candidate `C`, hod `main` at `a68a0d5d` (frozen release
binary, sha256
`baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`;
`--version` prints `hod version 1.0.0`, the number moves after the
lanes), with kiln 1.2.0 at `K` = `8f93304ee`, LLVM `/usr/lib/llvm-22`
behind a shim made from the frozen binary. Strict is
`scripts/libcxx-hod-debug.sh` at `a68a0d5d` with its defaults
(`--timeout 20 --mem-limit 3072 --no-fail`), `HOD_JOBS=3`, on the lanes'
jobserver pool (`HOD_JOBSERVER_FIFO=/run/hod/lanes.fifo`),
`HOD_DEBUG_FLAGS` unset, so every compile carries `-g` (the log's header
says `flags '-g'`):

```
env -u HOD_DEBUG_FLAGS HOD_JOBS=3 HOD_JOBSERVER_FIFO=/run/hod/lanes.fifo HOD_SHIM=$S scripts/libcxx-hod-debug.sh strict $D
python3 -I scripts/libcxx-hod-report.py $D/strict $D/strict.tsv .build/libcxx-5/native-g/control.tsv
```

The control is the `-g` control recorded at `K` for the `cd6ef6bc`
section below and reused (kiln + `llc`, no hod in it, not re-run):
`.build/libcxx-5/native-g/control.tsv`.

| | `-g` control at `K` (kiln + `llc`) | **strict at `a68a0d5d`** |
|---|---|---|
| date (EST) | 2026-11-15 20:10:09 to 22:07:18 | 2026-11-17 07:52:08 to 09:09:56 |
| total | 9,247 | 9,247 |
| pass | 8,608 | 8,608 |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 (24 driver) | 585 (24 driver) |
| timeout | 2 | 2 |
| killed, error | 0, 0 | 0, 0 |
| wall time, `-j 3` | 7,027.3 s | 4,667.7 s |
| `oom_kill` (cgroup `memory.events`) | 1 before, 1 after | 0 before, 0 after |
| modules by hod | - | 7,226 of 7,226; 0 refused, 0 fallback, 0 outcome error |

In strict, 7,218 tests have objects, all of them built by hod, and 7,185
of those pass; the inputs are 392 bitcode and 6,834 text modules; hod's
time summed over modules is 237.1 s (phases 240.8 s: parse 76.8, write
47.3, emission 46.7, bodies 27.5, read 27.2, object 8.7). The run shared
the host with a full gate, a fuzz run and the i686 libc++ lane; the wall
times are not a timing comparison.

**Result: strict equals the `-g` control.** Two outcomes differ, the
same two tests and the same directions as at `cd6ef6bc`, both at the
20 s limit:

- `std/atomics/atomics.types.operations/atomics.types.operations.wait/lost_wakeup.pass.cpp`:
  PASS in the control (23.10 s), TIMEOUT in strict (22.41 s). Run alone
  at `HOD_JOBS=1`, `-g`, 20 s, three times per mode (2026-11-17 09:10 to
  09:13 EST): TIMEOUT in all six runs (control 21.33 s, 22.40 s,
  21.78 s; strict 21.60 s, 21.73 s, 21.72 s). The test runs past the
  20 s limit in both modes.
- `ifstream.members/buffered_reads.pass.cpp`: TIMEOUT in the control
  (23.96 s), PASS in strict (19.24 s).

`offset_range.pass.cpp` times out in both modes. The four filesystem
failures (`directory_iterator` and `recursive_directory_iterator`
constructors, `fs.op.remove`, `fs.op.remove_all`) and the XPASSes
(`rel_ops.depr_in_cxx20.verify.cpp`, `variant.hash/hash.depr.verify.cpp`)
are the same in both modes.

Not covered: hybrid was not run (strict refused nothing); the debug
information quality (`cargo xtask debug trees`) was not sampled at the
candidate; only `-O0 -g` as the suite builds it. Raw results:
`.build/release-1.1.0/libcxx-a64/g/` (`strict.{log,tsv}`, `strict/`,
`report-strict.txt`, `rerun-lw.log`, `rerun-lw-*`).

## At the candidate cd6ef6bc (WP-271, 2026-11-15, control and strict, `-g`)

The 1.0 release candidate, hod `main` at `cd6ef6bc` (release binary,
frozen copy, sha256
`80138ec1411506d5b4048f1eac49e8b377f9eeedb21e70ae43e3c81ab094929c`),
with kiln 1.2.0 at `8f93304ee`, LLVM `/usr/lib/llvm-22` behind the shim.
Both runs are `scripts/libcxx-hod-debug.sh` at `cd6ef6bc` with its
defaults (`--timeout 20 --mem-limit 3072 --no-fail`), `-j 3`
(`HOD_JOBS=3`), off the machine's jobserver (`HOD_JOBSERVER_FIFO` set to
a path that does not exist), `HOD_DEBUG_FLAGS` unset, so every compile
carries `-g` (each log's header says `flags -g`):

```
env -u HOD_DEBUG_FLAGS HOD_JOBS=3 HOD_JOBSERVER_FIFO=<no such path> scripts/libcxx-hod-debug.sh control $D
env -u HOD_DEBUG_FLAGS HOD_JOBS=3 HOD_JOBSERVER_FIFO=<no such path> HOD_SHIM=$S scripts/libcxx-hod-debug.sh strict $D
python3 -I scripts/libcxx-hod-report.py $D/strict $D/strict.tsv $D/control.tsv
```

`$S` is a shim naming the frozen hod and kiln above. The suite is
`llvmorg-22.1.8`, 9,247 tests.

| | control (kiln + `llc`) | **strict at `cd6ef6bc`** |
|---|---|---|
| date (EST) | 2026-11-15 20:10:09 to 22:07:18 | 2026-11-15 20:47:25 to 22:27:36 |
| total | 9,247 | 9,247 |
| pass | 8,608 | 8,608 |
| fail | 4 | 4 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 (24 driver) | 585 (24 driver) |
| timeout | 2 | 2 |
| killed, error | 0, 0 | 0, 0 |
| wall time, `-j 3` | 7,027.3 s | 6,009.7 s |
| `oom_kill` (cgroup `memory.events`) | 1 before, 1 after | 1 before, 1 after |
| modules by hod | - | 7,226 of 7,226; 0 refused, 0 fallback, 0 outcome error |

The two runs overlapped with each other and with the x86-64 runs on a
shared host; the wall times are not a timing comparison. In strict, 7,218
tests have objects, all of them built by hod, and 7,185 of those pass;
the inputs are 392 bitcode and 6,834 text modules; hod's time summed over
modules is 291.4 s.

**Result: strict equals the `-g` control.** Two outcomes differ:

- `std/atomics/atomics.types.operations/atomics.types.operations.wait/lost_wakeup.pass.cpp`:
  PASS in the control (23.10 s), TIMEOUT in strict (22.79 s) at the 20 s
  limit. Run alone at `HOD_JOBS=1`, twice per mode (2026-11-15 22:28 to
  22:32 EST): at the 20 s limit, TIMEOUT in all four runs (control 21.80 s
  and 21.44 s, strict 21.42 s and 21.23 s); at 120 s (`HOD_TIMEOUT=120`),
  PASS in all four (control 25.35 s and 29.90 s, strict 24.97 s and
  26.42 s). The test runs past the 20 s limit in both modes.
- `ifstream.members/buffered_reads.pass.cpp`: TIMEOUT in the control,
  PASS in strict.

`offset_range.pass.cpp` times out in both modes. The failures (the four
filesystem tests: `directory_iterator` and `recursive_directory_iterator`
constructors, `fs.op.remove`, `fs.op.remove_all`) and the XPASSes
(`rel_ops.depr_in_cxx20.verify.cpp`, `variant.hash/hash.depr.verify.cpp`)
are the same in both modes. The two special modules of the 2026-11-14
measurement below now pass in both modes:
`map/PR28469_undefined_behavior_segfault.sh.cpp` (refused then,
`debug:dbg-value-expr`) and `func.not_fn/not_fn.nttp.pass.cpp` (kiln's
invalid `-g` module, FAIL in the control then).

Not covered: hybrid was not run (strict refused nothing); the debug
information quality (`cargo xtask debug trees`, below) was not sampled
again at the candidate; only `-O0 -g` as the suite builds it. Record:
`spec/landed/harness/WP-271.md`.

## 2026-11-14 measurement (WP-210)

WP-210: libc++'s test suite built with debug information (`-g`), the
configuration of a development build, by kiln with `llc` (the control) and
with hod in place of `llc` (hybrid: refused modules go to `llc`; strict: no
fallback). It is the recipe of `docs/status/libcxx-hod.md` with
`scripts/libcxx-debug-kiln.sh` in front of the compiler chain, which adds
`-g` to every invocation. The x86-64 leg is
`docs/status/libcxx-hod-debug-x64.md`.

The suite is `llvmorg-22.1.8` (`libcxx/test/std` and `libcxx/test/libcxx`,
9247 tests), LLVM is `/usr/lib/llvm-22` behind the shim, the target is
`aarch64-unknown-linux-gnu`, and every run is kiln's `cargo xtask libcxx`
with `--timeout 20 -j 4 --mem-limit 3072 --no-fail`, one run at a time on a
shared host. `oom_kill` is read before and after each run.

| run | date | kiln | hod | wall | `oom_kill` |
|---|---|---|---|---:|---|
| control | 2026-11-14 | `21caaf874` | - | 11214 s | 0, 0 |
| hybrid | 2026-11-14 | `21caaf874` | `b5545a9` | 7633 s | 0, 1 (no test killed) |
| strict | 2026-11-14 | `21b9110a4` | `3b14f8f` (with WP-210's fixes) | 7478 s | 1, 1 |

## Result

| run | pass | fail | xfail | xpass | unsupported | timeout |
|---|---:|---:|---:|---:|---:|---:|
| control | 8606 | 5 | 46 | 2 | 585 | 3 |
| hybrid | 8607 | 5 | 46 | 2 | 585 | 2 |
| strict | 8605 | 6 | 46 | 2 | 585 | 3 |

The control's 5 failures are the four filesystem tests that fail on this
host without `-g` too (`directory_iterator` and `recursive_directory_iterator`
`ctor.pass.cpp`, `remove.pass.cpp`, `remove_all.pass.cpp`) and
`func.not_fn/not_fn.nttp.pass.cpp`, whose `-g` module kiln writes invalid
(below). Its timeouts are `lost_wakeup.pass.cpp`, `offset_range.pass.cpp`
and `buffered_reads.pass.cpp`, load timeouts of the 20 s limit.

**Hybrid against the control, test by test: 0 differences caused by hod.**
The one changed test, `ifstream.members/buffered_reads.pass.cpp` (TIMEOUT
in the control, PASS in hybrid), was run alone twice in each mode: 2 of 2
passes each, its module built by hod.

**Strict against the control, test by test: one difference, not a
refusal of WP-210's scope.**
`map/PR28469_undefined_behavior_segfault.sh.cpp` (PASS in the control,
FAIL in strict) is the one module hod refuses: built at its own `-O2 -g`,
it has a `#dbg_value` expression hod does not write
(`debug:dbg-value-expr`, a follow-up). Its control on kiln `21b9110a4`,
run alone, passes. The other strict outcomes are the control's: the same
five failures (`not_fn.nttp` still kiln's on `21b9110a4`, control alone:
FAIL) and the same three timeouts. `buffered_reads.pass.cpp` timed out in
strict alone too (23.2 s, 24.0 s against the 20 s limit, load average
29); with a 120 s limit, alternating, it passes in both modes (control
35.6 s and 26.7 s, strict 34.8 s and 29.0 s).

The strict run is on a newer pair than the control (kiln `21b9110a4`, hod
`3b14f8f`). Every test whose strict outcome equals the control's is
vouched for by that equality; the two tests whose outcomes depend on kiln
(PR28469 and `not_fn.nttp`) were rerun in the control on the new kiln.

| strict coverage | |
|---|---:|
| modules through the back end | 7226 |
| built by hod | 7224 |
| refused (no fallback) | 2: `debug:dbg-value-expr` (PR28469), `malformed-input` (`not_fn.nttp`) |
| tests that build an object | 7218 |
| all objects by hod | 7216 |
| of those, passing | 7182 |
| back end time, hod | 444.6 s |

Of the 7216 tests built by hod alone, the 34 that do not pass are the
control's own outcomes (XFAIL, the four filesystem failures, the
timeouts).

## Coverage (hybrid)

| | |
|---|---:|
| modules through the back end | 7226 (392 bitcode) |
| built by hod | 7169 (99.2%) |
| built by the fallback | 57 |
| tests that build an object | 7218 |
| all objects by hod | 7161 |
| of those, passing | 7128 |
| back end time, hod-built modules | 268.2 s |
| `llc` time, fallback modules | 62.8 s |

No coverage line has outcome `error`; the one fallback that exited non-zero
is `not_fn.nttp`'s, whose module `llc` rejects as well.

## Refusals (hybrid) and what became of them

| first reason | modules | what it was | now |
|---|---:|---|---|
| `debug:die-unsupported` | 55 | a static member whose constant is wider than 64 bits (in every case a static member named `__min`; filesystems 12, bit 10, simd 10, format 6, rand 5, numeric.ops 5, support.limits 3, charconv 2, time.clock 1, alg.nonmodifying 1) | written (WP-210, `spec/dwarf.md` §6.28); all 55 build strict |
| `debug:retained-nodes` | 1 | `map/PR28469_undefined_behavior_segfault.sh.cpp` (`-O2 -g`): retained variables of an inlined function | written (WP-210, §6.29); the module then stops at `debug:dbg-value-expr` (a follow-up) |
| `malformed-input` | 1 | `func.not_fn/not_fn.nttp.pass.cpp`: kiln names a struct type it never defines in a `DITemplateValueParameter` value | kiln's (WP-1424 there); `llc` rejects the module too |

The reduced modules are under `.build/findings/libcxx-debug/` (not in git):
`debug-die-unsupported/wide-static-member/`, `debug-retained-nodes/` and
`kiln-undef-type/` (a 7-line C++ source and its command).
`scripts/libcxx-debug-refused.sh` regenerates every refused module of a
run with hod's detail.

## Debug information quality

`cargo xtask debug trees` on 240 hod-built modules sampled across the
suite (`scripts/libcxx-debug-sample.sh`; every object also passes
`llvm-dwarfdump --verify`), hod at `dd90c23a`:

| class (`spec/dwarf.md` §6.7, §6.21) | modules |
|---|---:|
| same as `llc` | 26 (16004 DIEs) |
| permitted: realigned frame | 213 |
| permitted: sret | 8 |
| permitted: (b) is.constant block | 5 |
| differs | 1 |
| declined, failed (including `--verify`) | 0 |

A module can carry several permitted classes. The one difference is
`support.coroutines/end.to.end/fullexpr-dtor.pass.cpp`: kiln declares a
split coroutine's `__coro_frame` and `__promise` on a `phi`, which `llc` 22
describes by location lists and hod does not (WP-221). The 55 wide-member
modules: 1 same, 54 permitted, 0 differ.

## Repeating the run

```sh
# shim: cargo xtask shim --llvm /usr/lib/llvm-22 --hod <hod> --out <shim> --kiln <kiln++>
scripts/libcxx-hod-debug.sh control <run dir>
HOD_SHIM=<shim> HOD_JOBS=4 scripts/libcxx-hod-debug.sh hybrid <run dir>
HOD_SHIM=<shim> HOD_JOBS=4 scripts/libcxx-hod-debug.sh strict <run dir>
python3 scripts/libcxx-hod-report.py <run dir>/hybrid <run dir>/hybrid.tsv <run dir>/control.tsv
KILN=<kiln++> HOD=<hod> scripts/libcxx-debug-refused.sh <run dir>/hybrid <out>
KILN=<kiln++> scripts/libcxx-debug-sample.sh <run dir>/hybrid <out> 240
cargo xtask debug trees -j 2 --hod <hod> <out>/*.ll
# one test alone: add --filter <path substring> -j 1
```
