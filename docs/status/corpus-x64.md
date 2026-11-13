# The x86-64 corpus (WP-139)

This is the corpus of `spec/harness.md` "corpus", captured for x86-64 on the
AArch64 reference machine (`cargo xtask corpus capture --target x86_64`,
"Other targets"). It holds the same two suites with the same two LLVM
majors as the AArch64 corpus. Each entry's expectations are what the
reference build made of its module: `llc -O0 -global-isel=0 -fast-isel=0
-relocation-model=pic -mtriple=x86_64-unknown-linux-gnu`, linked by the
cross driver and run under `qemu-x86_64 -cpu qemu64,-sse3,-cx16,-lahf-lm`
(x86-64-v1). Admission was the host's, with the `-O2` witness.

Inputs:
- kiln 1.0.1 at `bd60e1829`
- the GCC tests at `6afcc4f6d`
- clang and `llc` 22.1.8 and 19.1.7
- the cross GCC 14 driver, with its `ld.bfd`, lld and mold (wild is not
  installed)
- QEMU from the machine's packages

The corpus is in `.build/corpus-x64`. Its generated report, with the
constructs counted, is [corpus-x64-census.md](corpus-x64-census.md).

## Counts

| group | tests | captured | skipped | entry MB | capture s (`-j 3`) | AArch64 captured |
|---|---:|---:|---:|---:|---:|---:|
| `gcc-torture/llvm22` | 1812 | 1723 | 89 | 56.5 | 836 | 1709 |
| `gcc-torture/llvm19` | 1812 | 1726 | 86 | 56.5 | 558 | 1711 |
| `kiln/llvm22` | 1540 | 1417 | 123 | 46.2 | 1200 | 1434 |
| `kiln/llvm19` | 1540 | 1378 | 162 | 45.8 | 511 | 1396 |
| all | 6704 | **6244** | 460 | **205.1** | 3105 | 6250 |

The corpus keeps no executables, and its size is the AArch64 corpus's
(205 MB on disk in both). The capture times are of a shared machine
under load. The first group, `kiln/llvm22`, ran beside the workspace's
test suites.

## Capture failures by reason

| reason | torture 22 | torture 19 | kiln 22 | kiln 19 | for x86-64 |
|---|---:|---:|---:|---:|---|
| `clang-rejects` | 66 | 63 | | | GNU C clang does not take (nested functions, VLAs in structs, GCC-only builtins); 20 (21) fewer than AArch64's 86 (84), whose clang also rejects `__builtin_setjmp`/`__builtin_longjmp` and the x87 `=t` constraint |
| `special-flags` | 8 | 8 | | | 5 more than AArch64's 3: x86 `dg-options` the capture does not translate (`-mno-mmx`, `-mpc64`, `-mno-stv`, `-mtune=i686`) |
| `program-fails` | 12 | 12 | 6 | 6 | the 11 torture failures AArch64 has too (all `abort`), plus `pr64242.c` (an `llc` bug, below); the 6 kiln ones are the `target.x86/` tests of instructions beyond x86-64-v1 (SIGILL, below) |
| `link-fails` | 2 | 2 | | | as on AArch64 (`link_error` that only GCC's optimiser removes) |
| `not-reproducible` | 1 | 1 | | | `return-addr.c`, as on AArch64 |
| `requires` | | | 49 | 90 | `requires: target-aarch64` (24, of which 19 are captured on AArch64) in place of AArch64's `requires: target-x86_64` (10: 3 captured here (1 with 19), 6 SIGILL, 1 `link-flags`) and `host-sve` (2), and the `clang` version lines as on AArch64 |
| `needs-toolchain` | | | 33 | 33 | as on AArch64 |
| `libc++` | | | 14 | 13 | as on AArch64 |
| `marked-xfail` | | | 8 | 8 | as on AArch64 |
| `link-flags` | | | 5 | 4 | `-latomic`, `-fuse-ld=lld`; the x86-64 variant of `alignment-warning` in place of the AArch64 one |
| `needs-env`, `opt-first-lto`, `multi-file`, `compile-fails` | | | 8 | 8 | as on AArch64 (`compile-fails` is `builtins-srcloc-path-style.cpp`, whose `static_assert` wants an absolute `__FILE__`) |

**The tests that cannot be captured for x86-64 but can for AArch64**,
with their reasons:
- The 19 `requires: target-aarch64` kiln tests (NEON, ACLE, AAPCS64
  over-alignment, multiversioning, `fp-contract`). They name the target
  they test.
- The 5 torture tests with x86-only `dg-options`: `20010129-1.c`,
  `20050316-2.c`, `20050604-1.c`, `990127-2.c` and `pr97073.c`.
- `pr64242.c`, whose reference program crashes (the `llc` bug below).
- The 6 kiln `target.x86/` run tests that use AVX2/FMA/F16C, MMX,
  SSE4.1 non-temporal loads, SSE4.2/AES (Westmere) or `target`
  attributes for those. On x86-64-v1 they get SIGILL (an emulation
  artefact of the baseline CPU, class (b)).
  - A future `;@ cpu:` line in their entries could admit them. The
    capture does not add one: hod's baseline is x86-64-v1.
  - These tests skip on AArch64 too (`requires: target-x86_64`).

## Cross-check against AArch64

`cargo xtask corpus compare --target x86_64` gives the following.

| group | x86-64 | AArch64 | in both | same exit and stdout | different |
|---|---:|---:|---:|---:|---:|
| `gcc-torture/llvm19` | 1726 | 1711 | 1706 | 1706 | 0 |
| `gcc-torture/llvm22` | 1723 | 1709 | 1704 | 1704 | 0 |
| `kiln/llvm19` | 1378 | 1396 | 1376 | 1376 | 0 |
| `kiln/llvm22` | 1417 | 1434 | 1414 | 1414 | 0 |
| all | 6244 | 6250 | **6200** | **6200** | **0** |

No test with an entry in both corpora differs in exit status or
standard output. Both suites check themselves, so a
behaviour that depends on the target shows as a test captured on one side
only. Each such test is classified below. The table of explained
differences, `docs/status/corpus-x64-differences.tsv`, therefore has no
rows. `compare` exits 1 as soon as a recapture brings a difference it
does not explain.

| tests captured on one side only | LLVM 22 | LLVM 19 | class | cause |
|---|---:|---:|---|---|
| torture, x86-64 only: `__builtin_setjmp`/`__builtin_longjmp` (`built-in-setjmp.c`, `pr84521.c`, 16 `builtins/*-chk.c`; `pr60003.c` with 19, which clang 22 rejects on both for a pointer type) | 18 | 19 | (a) target | clang supports the GCC builtins on x86 only |
| torture, x86-64 only: `990413-2.c` | 1 | 1 | (a) target | x87 `=t` asm constraint |
| torture, AArch64 only: x86 `dg-options` | 5 | 5 | (a) target | options for x86 targets only |
| kiln, x86-64 only: `requires: target-x86_64` | 3 | 1 | (a) target | the test names its target |
| kiln, AArch64 only: `requires: target-aarch64` | 19 | 19 | (a) target | likewise |
| kiln: `alignment-warning-x86_64-run` / `-run` / `-compat19-run` | `link-flags` either side | | (a) target | one variant per target, all need `-latomic` |
| kiln, neither: 6 `target.x86/` beyond x86-64-v1 | 6 | 6 | (b) emulation | SIGILL on the baseline CPU model |
| torture, neither: `pr64242.c` | 1 | 1 | (c) **llc** | below |

**Suspected bugs (class c).** There is one, in `llc` and not in kiln or
clang, with a reduction in
`.build/target-wp-139/findings/pr64242/` (`README.md`, `reduced.ll`, and
the test with clang-22's module of it).
- The bug is x86-64 SelectionDAG's expansion of `llvm.eh.sjlj.longjmp`
  in a function with a frame pointer when the buffer is a local of that
  frame. It loads `%rbp` from the buffer first, then the target and
  `%rsp` relative to the `%rbp` it has just replaced.
- LLVM 22 and 19 are affected, and the program gets SIGSEGV with any
  `-cpu`. FastISel on the original module takes the address into a
  register first and passes.
- For hod: its lowering of that intrinsic must not do the same.

No kiln bug was found. Every kiln test that kiln++ compiles for both
targets behaves the same on both.

## Emulation times

`TIMES.tsv` records the wall time of each program's run during the
capture, under `qemu-x86_64`, on a loaded machine:

| group | programs | median s | p90 s | p99 s | max s | over 1 s | over 10 s | timed out |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `gcc-torture/llvm19` | 1739 | 0.029 | 0.031 | 0.053 | 0.269 | 0 | 0 | 0 |
| `gcc-torture/llvm22` | 1736 | 0.054 | 0.130 | 0.236 | 1.142 | 1 | 0 | 0 |
| `kiln/llvm19` | 1384 | 0.041 | 0.209 | 0.513 | 0.865 | 0 | 0 | 0 |
| `kiln/llvm22` | 1423 | 0.289 | 0.550 | 0.896 | 5.858 | 13 | 0 | 0 |
| all | 6282 | 0.041 | 0.332 | 0.680 | 5.858 | 14 | 0 | 0 |

The slowest 20 runs, all of which exited normally:

| group | test | s |
|---|---|---:|
| kiln 22 | `tests/stress/bool-vector-sieve.cpp` | 5.86 |
| kiln 22 | `conformance/over.over/template-id-with-non-template.cpp` | 3.79 |
| kiln 22 | `conformance/except.ctor/run-lifetime-extended-unwind.cpp` | 2.76 |
| kiln 22 | `conformance/except.ctor/run-unwind-destroy.cpp` | 2.36 |
| kiln 22 | `conformance/coro.generator/libstdcxx-generator-elements-of-run.cpp` | 1.48 |
| kiln 22 | `conformance/conv/n3323-probe.cpp` | 1.34 |
| kiln 22 | `conformance/expr.await/symmetric_transfer.cpp` | 1.28 |
| kiln 22 | `conformance/expr.ref/member-access.cpp` | 1.16 |
| kiln 22 | `conformance/dcl.init.general/paren-init-dmi-srcloc-subscript-run.cpp` | 1.14 |
| torture 22 | `memcpy-a2.c` | 1.14 |
| kiln 22 | `conformance/basic.start.dynamic/module-init-run.cpp` | 1.12 |
| kiln 22 | `conformance/ext.gnu.stmt.expr/temporary-before-stmt-expr-run.cpp` | 1.10 |
| kiln 22 | `conformance/dcl.fct.def.coroutine/promise_alloc.cpp` | 1.08 |
| kiln 22 | `conformance/expr.call/run-discarded-member-call-chain-order.cpp` | 1.08 |
| kiln 22 | `conformance/expr.call/p1-parenthesized-member-callee-run.cpp` | 0.97 |
| torture 22 | `memclr.c` | 0.91 |
| kiln 22 | `conformance/builtin.carry/carry-uint128-run.cpp` | 0.90 |
| kiln 22 | `conformance/class.base.init/qualified-dependent-initializer.cpp` | 0.88 |
| kiln 19 | `conformance/ext.gnu.typeof/run.cpp` | 0.86 |
| kiln 22 | `conformance/lex.phases/p1-invalid-utf8-narrow-literal-run.cpp` | 0.86 |

The `kiln/llvm22` numbers are load, not emulation. That group ran while
the workspace's test suites did. Rerun alone, every one of its slowest
programs takes 0.02-0.8 s.

**Whether ×6 holds.** As a *limit*, it does with a wide margin. The
emulated limit is 6 × 10 s = 60 s, and the slowest program of the whole
corpus took 5.9 s under load, 10 times less, with no timeout.

As a *ratio* of emulated to native time, it does not. A sample (the
slowest 20 and 20 drawn at random, `.build/target-wp-139/ratio/`)
compared the AArch64 entry built natively with the x86-64 entry under
`qemu-x86_64`, best of 3 runs each:
- 37 programs were measured. The ratio was 5.8-16× (median about 11×),
  and 56× for `memclr.c`.
- Most programs run in 2-5 ms natively and about 20-30 ms emulated, so
  QEMU's start-up dominates.
- The programs that compute for longer still show 7-12×: `memcpy-a2.c`
  7.1× (44 ms native), `symmetric_transfer.cpp` 12× (65 ms).

So a program that runs for longer than about 5-8 s natively could time
out under emulation where it passes natively. No corpus program comes
near that. The factor is `EMULATION_SLOWDOWN` in
`xtask/src/irdiff/engine.rs`, and raising it to 12-15 would cover the
measured ratios if such a test ever appears.

## Verify and reasons

(filled in below)

## Package census (design 0006 section 7.2, redone)

This is the method of `docs/design/0006-data/corpus/packages.py`,
unchanged, with its table imported by `scripts/x64-packages.py`. It is
run on every module of every entry of the corpus: 6713 modules in the
6244 entries. Each module is counted as freed once every package it
needs has landed, in design 7.2's order. The second-to-last column is
the 70-module random sample of the design, for comparison.

| package | modules needing it | frees | cumulative | % of 6713 | sample (of 70) |
|---|---:|---:|---:|---:|---:|
| X-int (+ S0) | 5623 | 10 | 10 | 0.1 | 0 |
| X-mem | 6482 | 120 | 130 | 1.9 | 5 |
| X-control | 4577 | 72 | 202 | 3.0 | 5 |
| X-call | 6324 | 3063 | 3265 | 48.6 | 26 |
| X-float | 767 | 350 | 3615 | 53.8 | 30 |
| X-aggr | 634 | 187 | 3802 | 56.6 | 35 |
| X-intrinsic | 2638 | 2430 | 6232 | 92.8 | 69 |
| X-x87 | 120 | 98 | 6330 | 94.3 | 69 |
| X-atomic | 37 | 33 | 6363 | 94.8 | 69 |
| X-tls | 26 | 26 | 6389 | 95.2 | 69 |
| X-vector | 152 | 148 | 6537 | 97.4 | 70 |
| X-asm | 176 | 176 | 6713 | 100 | 70 |
| C7-coro | 0 | 0 | 6713 | 100 | 70 |

With the core packages (int, mem, control, call) alone, 3265 modules are
freed (48.6%).
- core + float frees 3615.
- core + aggr frees 3416.
- core + intrinsic frees 5089.
- core + float + aggr + intrinsic frees 6232 (92.8%).

By entry, an entry is freed when all its modules are. 2992 of the 6244
entries are freed by the core alone, and 5769 (92.4%) after
X-intrinsic. Run `--by-entry` for the full table.

The full corpus confirms the sample's order and its two large steps,
X-call and X-intrinsic, with the same shares to within a few points (the
sample had 37% after X-call and 99% after X-intrinsic). The tail is
larger than in the sample: 7% of the modules need x87, atomics, TLS,
vectors or inline asm. Those come mostly from gcc-torture and kiln's
`target.x86/`, `atomics` and `ext.*` tests.

C7-coro frees nothing because kiln lowers coroutines before the IR. The
counts per group are in the script's output.

Reproduce:

    python3 -I scripts/x64-packages.py --by-entry .build/corpus-x64

## The design sample's capture failures

Design 0006's samples (`docs/design/0006-data/corpus/capture.tsv`,
`targeted-capture.tsv`) reported 5 capture failures. All 5 are tests of
several translation units:
- `c/interop/cxx-calls-c.cpp`
- `module.interface/image-bodies-run.cpp` and `image-name-clash.cpp`
- `temp.over.link/unresolved-nested-qualified-two-units-run.cpp`
- `basic.stc.thread/module-tls-run.cpp`

The sample's script gave kiln++ the header's shell command line as
arguments (`kiln++: error: no such file or directory: '&&'`). They were
failures of the script, not of x86-64 or of kiln. The xtask capture
builds each unit in its own step (`kiln::plan`), and all 5 are entries
here with both majors.

## How to repeat

```sh
cargo xtask corpus capture --target x86_64 --source kiln --llvm 22 -j 3   # and the other three groups
cargo xtask corpus compare --target x86_64          # exit 1 on an unexplained difference
cargo xtask corpus verify  --target x86_64 -j 3     # hybrid: hod refuses, llc builds, qemu runs
cargo xtask corpus reasons --target x86_64 -j 3
cargo xtask corpus census  --target x86_64          # docs/status/corpus-x64-census.md
python3 -I scripts/x64-packages.py --by-entry .build/corpus-x64
```
