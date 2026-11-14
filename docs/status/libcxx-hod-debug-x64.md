# libc++ test suite through hod with `-g` (x86-64)

WP-210, x86-64 leg of `docs/status/libcxx-hod-debug.md`: libc++'s test
suite built with debug information (`-g`) for `x86_64-unknown-linux-gnu`,
by kiln with `llc` (the control) and with hod in place of `llc` (hybrid:
refused modules go to `llc`; strict: no fallback), every test binary run
under `qemu-x86_64` on the AArch64 host. It is the recipe of
`docs/status/libcxx-hod-x64.md` (and its baseline,
`docs/status/libcxx-baseline-x64.md`) with `scripts/libcxx-debug-kiln.sh`
in front of the compiler chain, which adds `-g` to every invocation.
Status: **emulated** (design 0006 section 6.4).

The suite is `llvmorg-22.1.8` (`libcxx/test/std` and `libcxx/test/libcxx`,
9247 tests), LLVM is `/usr/lib/llvm-22` behind the shim, libc++ is the
baseline's amd64 22.1.8 static archives, the executor `qemu-x86_64 -L
/usr/x86_64-linux-gnu`. Every run is kiln's `cargo xtask libcxx` with
`--timeout 160 --run-timeout-long 1920 -j 4 --mem-limit 3072 --no-fail`,
one run at a time on a shared host (load average 25 to 30), off the
machine's jobserver by arrangement. kiln is `21b9110a4`; hod is `3b14f8f`
(main with WP-210's fixes), frozen for every run. `oom_kill` is read
before and after each run.

Two kinds of run:

- **Compile-only census** of the whole suite, hybrid: every module
  through the back end and every test linked, but `qemu-x86_64` replaced
  by `/bin/true` (`HOD_X64_QEMU=/bin/true`), so only compile and link
  outcomes and the coverage logs count. It names what hod refuses.
- **Shard 1/20 under qemu** (`--shard 1/20`, 463 tests), control,
  hybrid and strict, compared test by test.

| run | date | tests | wall | `oom_kill` |
|---|---|---:|---:|---|
| census (hybrid, not run) | 2026-11-14 | 9247 | 9026.5 s | 1, 1 |
| shard 1/20, control | 2026-11-14 | 463 | 741.1 s | 1, 1 |
| shard 1/20, hybrid | 2026-11-14 | 463 | 525.5 s | 1, 1 |
| shard 1/20, strict | 2026-11-14 | 463 | 574.3 s | 1, 1 |

`oom_kill` stood at 1 before the census began and did not move.

## Result (shard 1/20 under qemu)

| run | pass | fail | xfail | xpass | unsupported | timeout |
|---|---:|---:|---:|---:|---:|---:|
| control | 427 | 0 | 3 | 0 | 33 | 0 |
| hybrid | 427 | 0 | 3 | 0 | 33 | 0 |
| strict | 427 | 0 | 3 | 0 | 33 | 0 |

**Hybrid and strict against the control, test by test: 0 differences.**
Every one of the shard's 359 modules was built by hod in both runs (18
bitcode, 341 text); of the 359 tests built by hod alone, 357 pass, the
other two being the control's own XFAILs with objects.

`map/PR28469_undefined_behavior_segfault.sh.cpp`, the one module the
census found hod refusing for a reason of its own (below), is not in
shard 1/20. Run alone under qemu: control 2 of 2 PASS (20.7 s, 14.3 s);
hybrid 1 of 1 PASS (23.8 s), its module given to `llc`.
With hod `3b14f8f` it cannot build strict (the refusal); its strict run
with WP-225's fix follows when that lands.

## Coverage (census, hybrid, whole suite)

| | |
|---|---:|
| modules through the back end | 7226 (392 bitcode) |
| built by hod | 7224 |
| built by the fallback | 2 |
| tests that build an object | 7218 |
| all objects by hod | 7216 |
| coverage lines with outcome `error` | 0 |
| back end time, hod-built modules | 444.8 s |
| `llc` time, fallback modules | 0.2 s |

The one fallback that exited non-zero is `not_fn.nttp`'s, whose module
`llc` rejects as well. The census's compile and link outcomes: one test
fails to compile, `func.not_fn/not_fn.nttp.pass.cpp` (kiln's, below);
every other test that builds links. Its run outcomes (8600 pass, 14 fail,
45 xfail, 3 xpass, 585 unsupported) say nothing about the programs: the
13 other FAILs are `.sh.cpp` tests that compare a program's output
(`print.fun`, `iostream.objects`). The three XPASSes are front end and
harness outcomes, not back end ones: `variant.hash/hash.depr.verify.cpp`
and `setfill_wchar_max.pass.cpp` as in the baseline (the latter the
runner's host `target=` feature), and
`libcxx/depr/depr.rel_ops/rel_ops.depr_in_cxx20.verify.cpp`, a
`-fsyntax-only` test that is XFAIL in the baseline (kiln `bd60e1829`) and
XPASS with the newer kilns of WP-210 (`21b9110a4` here, `21caaf874` in
the native `-g` control too). Hence 45 xfail where the baseline has 46.

## Refusals (census) and what became of them

| first reason | modules | what it was | now |
|---|---:|---|---|
| `debug:dbg-value-fragment` | 1 | `map/PR28469_undefined_behavior_segfault.sh.cpp` (`-O2 -g`): `#dbg_value(ptr poison, ..., DW_OP_LLVM_fragment 0 8)` of an inlined reference parameter | written by WP-225 |
| `malformed-input` | 1 | `func.not_fn/not_fn.nttp.pass.cpp`: kiln names a struct type it never defines in a `DITemplateValueParameter` value | kiln's (WP-1424 there); `llc` rejects the module too |

These are the native run's refusals less the 55 wide static members and
the retained nodes WP-210 fixed (`docs/status/libcxx-hod-debug.md`
"Refusals"): the same two tests, nothing specific to x86-64. Natively
PR28469 stopped at `debug:dbg-value-expr`; on x86-64 the first value hod
meets is a fragment.

The modules are under `.build/findings/libcxx-debug/x64/` (not in git):
`debug-dbg-value-fragment/` (the module, a 44-line reduction that `llc`
accepts, hod's command and messages) and `malformed-input/`.

## Debug information quality

`cargo xtask debug trees` on 240 hod-built modules sampled across the
suite (`scripts/libcxx-debug-sample.sh`, of 7198 sampleable; every object
also passes `llvm-dwarfdump --verify`), hod `3b14f8f`:

| class (`spec/dwarf.md` §6.7, §6.21, x86-64 classes) | modules |
|---|---:|
| same as `llc` | 175 (399442 DIEs) |
| permitted: byval | 63 |
| permitted: sret | 7 |
| permitted: (b) is.constant block | 3 |
| differs | 1 |
| declined, failed (including `--verify`) | 0 |

A module can carry several permitted classes. The one difference is
`vector.capacity/resize_size_exceptions.pass.cpp`, and it is `llc`'s: a
lambda that captures a pack by reference (`[&a...]`) has one member per
element, all named after the pack, in a class with an ODR identifier.
LLVM's IR reader uniques a member of an identified class by its name and
scope alone, so the second member becomes the first, and `llc` writes
the first capture twice (`const int &` at offset 8, twice). hod writes
both members as kiln wrote them (`const int &` at 8, `int &` at 16).
clang 22 merges them the same way in memory. After `opt -S` the module
has one member left, and hod's tree then equals `llc`'s (permitted:
byval). Kept with a two-line source:
`.build/findings/libcxx-debug/x64/trees-differs/`.

## Repeating the run

```sh
# shim: cargo xtask shim --llvm /usr/lib/llvm-22 --hod <hod> --out <shim> --kiln <kiln++>
export HOD_DEBUG_TARGET=x86_64 HOD_JOBS=4
# census: links every test, runs none
HOD_X64_QEMU=/bin/true HOD_SHIM=<shim> scripts/libcxx-hod-debug.sh hybrid <census dir>
python3 scripts/libcxx-hod-report.py <census dir>/hybrid <census dir>/hybrid.tsv <census dir>/hybrid.tsv
KILN=scripts/libcxx-x64-kiln.sh HOD=<hod> scripts/libcxx-debug-refused.sh <census dir>/hybrid <out>
KILN=scripts/libcxx-x64-kiln.sh scripts/libcxx-debug-sample.sh <census dir>/hybrid <out> 240
cargo xtask debug trees -j 2 --hod <hod> <out>/*.ll
# one shard under qemu
scripts/libcxx-hod-debug.sh control <run dir> --shard 1/20
HOD_SHIM=<shim> scripts/libcxx-hod-debug.sh hybrid <run dir> --shard 1/20
HOD_SHIM=<shim> scripts/libcxx-hod-debug.sh strict <run dir> --shard 1/20
python3 scripts/libcxx-hod-report.py <run dir>/strict-1-of-20 <run dir>/strict-1-of-20.tsv <run dir>/control-1-of-20.tsv
# one test alone: add --filter <path substring> -j 1
```

`HOD_X64_KILN` must be unset for the refused and sample scripts: the
wrapper then calls kiln++ without hod.
