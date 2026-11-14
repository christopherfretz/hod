# libc++ test suite through hod with `-g` (native AArch64)

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
