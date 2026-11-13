# The M3 gate measured: debug information

The gate of `docs/ROADMAP.md` M3: *"L7: kiln's debugger tests pass on hod's
objects under gdb and lldb; `llvm-dwarfdump --verify` is clean over the
corpus; line tables agree with `llc`'s on the corpus (same set of
`(function, line)` breakpoint locations). L6 strict with -g."* Each clause
measured first by WP-099, again by WP-111, and, as here, by WP-150. The
reference is `llc -O0 -global-isel=0 -fast-isel=0` (`docs/VALIDATION.md`
"Oracles"; x86-64 `-fast-isel=0 -relocation-model=pic`).

| | |
|---|---|
| date | 2026-11-13 |
| hod | WP-150's branch (`main` at `3a98566` and WP-150's commits), release build (`cargo build --release -p hod_cli`) |
| kiln | 1.0.1 at `bd60e1829` (the submodule's pin), release build |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb 16.3; lldb 19.1.7 (`lldb-22` is not installed) |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1 (`wild` is not installed) |
| targets | `aarch64-unknown-linux-gnu` (native); `x86_64-unknown-linux-gnu` for `tests/debug` (qemu-user's gdb stub) |

## Verdict

| clause | verdict | what stands in the way (modules stopped) |
|---|---|---|
| L7: kiln's debugger tests | **met** | nothing: 8 of 8 sessions pass with hod (one transcript differs in a column after `finish`, `spec/dwarf.md` §5.7 `return-column`) |
| L7: `tests/debug` | **met** on both targets (with known expected failures) | AArch64: 15 XFAIL (`sret_entry.c` 12, §6.7, permitted; `call_sites.c` 3, clang 22's key instructions); x86-64: 22 SKIP (TLS under qemu 18, tests for AArch64 only 4); nothing FAILs, nothing is stale |
| `--verify` clean | **met for every object hod builds** (210 of 210); not met as "the whole corpus" | 2 corpus modules declined: `debug:dbg-value` 2 |
| line tables agree | **met** on the corpus (12 of 12 modules equal) and on the `-g` set (198 of 198 modules without a pair §5.7 does not permit; 89 with equal sets) | nothing |
| L6 strict with `-g` | **not met**: 12 of 14 modules pass under every linker | `debug:dbg-value` 2 |

## The modules

1. **The corpus** (`.build/corpus`): 14 modules have a `!llvm.dbg.cu`, all
   kiln's (`kiln/llvm22` 7, `kiln/llvm19` 7, the same seven tests:
   `namespace-scope-debuginfo-run`, `expr.reflect/debuginfo-run`,
   `complex-int-debuginfo-run`, `class-type-argument-debuginfo-run`,
   `pattern-typedef-debuginfo-run`, `define-debuginfo-run` (DWARF 4),
   `lifetime-markers-O2-g-fpic-run` (an `-O2` unit)). No module of the
   gcc-torture source has debug information.
2. **The `-g` set** (`scripts/debug-gset.sh`): 198 modules from real
   sources at `-g -O0`: every 120th `*.pass.cpp` of libc++'s `test/std`
   against libstdc++, `stl.cpp`, `e2e.cpp`, the ten sources of
   `bench/src` and kiln's four libstdc++ units of WP-094 (kiln++ only).
   By producer: clang 19 67 (built by `llc` 19), clang 22 67 and kiln++
   64 (built by `llc` 22).

## `llvm-dwarfdump --verify`

hod's object and `llc`'s of each module, each checked by
`llvm-dwarfdump` 22 and 19.

| | modules | hod built | hod clean | `llc` clean | declined |
|---|---:|---:|---:|---:|---:|
| corpus, LLVM 22 | 7 | 6 | 6 | 7 | 1 |
| corpus, LLVM 19 | 7 | 6 | 6 | 7 | 1 |
| `-g` set, clang 19 | 67 | 67 | 67 | 67 | 0 |
| `-g` set, clang 22 | 67 | 67 | 67 | 67 | 0 |
| `-g` set, kiln++ | 64 | 64 | 64 | 64 | 0 |
| **all** | **212** | **210** | **210** | **212** | **2** |

No verify failure on either writer's objects. Declines by reason (the
first refusal of `hod --strict`):

| reason | modules | which | status |
|---|---:|---|---|
| `debug:dbg-value` | 2 | corpus `lifetime-markers-O2-g-fpic-run.cpp`, LLVM 19 and 22: an `-O2 -g` unit, whose variables are `#dbg_value`s | hod describes no variable by its values (`spec/dwarf.md` §6.4) |

WP-111's two other declines are gone: `define-debuginfo-run.cpp`
(`-gdwarf-4`) is built since WP-114, and `lifetime-markers`' call sites
(`debug:call-sites`) are written since WP-113, which leaves its
`#dbg_value`s as the first refusal.

Reproduce: `cargo xtask debug verify -j 4 --hod <release hod> -v` (the
corpus: `14 -g modules: 12 built (12 clean, 0 not clean), 2 declined`);
`scripts/debug-measure.py` for `llc`'s objects and the `-g` set (below).

## Line tables

The `(function, file, line)` pairs of every `is_stmt` row with a line
other than 0 and a non-empty range, by the rule of `xtask/src/debug/lines.rs`
(`spec/harness.md` "debug", "Lines"), on hod's and `llc`'s programs of each
module both build. `scripts/debug-measure.py` links each object into a PIE
without start files and with undefined symbols allowed, so that modules
without `main` (most libc++ tests are `main`-only, `stl.cpp` has none) get
one address per function. The classes are `spec/dwarf.md` §5.7's,
recognised from the IR (`xtask/src/debug/permitted.rs`).

| | modules | identical sets | no unclassified pair | pairs the same | P1 | P3 | load-placement | post-indexed-load | is-constant | unclassified |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus (both majors) | 12 | 12 | 12 | 96 | 0 | 0 | 0 | 0 | 0 | 0 |
| `-g` set, clang 19 | 67 | 30 | 67 | 64 347 | 819 | 33 | 20 | 0 | 12 | 0 |
| `-g` set, clang 22 | 67 | 30 | 67 | 64 335 | 819 | 33 | 20 | 4 | 12 | 0 |
| `-g` set, kiln++ | 64 | 29 | 64 | 65 999 | 952 | 33 | 20 | 0 | 16 | 0 |
| **`-g` set** | **198** | **89** | **198** | **194 681** | **2 590** | **99** | **60** | **4** | **40** | **0** |

Pairs only hod's program has: 163 (P3 99, `load-placement` 20,
`post-indexed-load` 4, `is-constant` 40); only `llc`'s: 2 630 (P1 2 590,
`load-placement` 40). The `-g` set's counts are WP-111's, pair for pair.

Reproduce:

```
scripts/debug-gset.sh <dir>/gset
scripts/debug-measure.py --hod <release hod> --work <dir>/work -j 4 .build/corpus > corpus.tsv
scripts/debug-measure.py --hod <release hod> --work <dir>/gwork -j 4 <dir>/gset > gset.tsv
```

## L6 strict with `-g`

`cargo xtask irdiff --llvm <N> --hod <release hod> <the 7 -g modules>`
(strict is `irdiff`'s default), every linker found:

| LLVM | linker | PASS | FAIL (declined) |
|---|---|---:|---:|
| 22 | bfd, lld, mold | 6 each | 1 each |
| 19 | bfd, lld, mold | 6 each | 1 each |

The FAILs are the decline above: `debug:dbg-value`
(`lifetime-markers-O2-g-fpic-run.cpp`). Every module hod builds runs as
`llc`'s does under the three linkers.

## kiln's debugger tests and `tests/debug`

`cargo xtask debug kiln --hod <release hod>`: kiln's four debugger tests
(`globals_lambda.cpp`, `locals.cpp`, `reflection.cpp`, `step_finish.cpp`)
under gdb and lldb 19, linked by kiln++'s own driver: `llc` 8 of 8 pass,
hod 8 of 8 pass; 7 of 8 transcripts equal. The eighth, `step_finish.cpp`
under lldb, shows column 7 (hod) against 15 (`llc`) after `finish`:
`spec/dwarf.md` §5.7 `return-column`.

`cargo xtask debug run -j 4 --hod <release hod>` (and `--target x86_64`):
34 tests, front ends clang 19, clang 22 and kiln++, linkers bfd, lld and
mold, gdb and lldb 19.

| target | results | PASS | XFAIL | SKIP | sessions | `(function, file, line)` pairs |
|---|---:|---:|---:|---:|---:|---|
| AArch64 | 522 | 507 | 15 | 0 | 972 | 1 331: 1 327 the same, 4 `llc`'s only (P1) |
| x86-64 | 502 | 480 | 0 | 22 | 888 | 1 293: 1 289 the same, 4 `llc`'s only (P1) |

No FAIL, XPASS, ERROR or crash on either target. The expected failures
and skips:

| test | results | reason | status |
|---|---:|---|---|
| `sret_entry.c` | 12 (AArch64) | `llc`'s location list of the `sret` variable has a gap at the first line's row; hod's shows the value | permitted by §6.7 since WP-094, still compared as a difference |
| `call_sites.c` | 3 (AArch64, clang 22, gdb) | clang 22's `-O1` IR has key instructions, which `llc` 22 uses for `is_stmt`; hod ignores `atomGroup`, so gdb's `finish` stops mid-line | equal under `-gno-key-instructions` |
| `tls.c`, `tls_static.c`, `dwarf4_tls.c` | 18 SKIP (x86-64) | qemu-user's gdb stub has no `qGetTLSAddr` | `spec/harness.md` "debug", "Other targets" |
| `call_sites.c`, `sret_entry.c` | 4 SKIP (x86-64) | the tests are for AArch64 only (`target:`) | |

`optimized_unit.c`'s expected failures (`debug:call-sites`) are gone
since WP-113; `debug_frame.c` (WP-150: a backtrace, `up` and `finish`
through functions whose CFI is in `.debug_frame` only) and
`utf8_source.c` (WP-150) pass on both targets.

## Other observations

- GNU binutils (2.44: `addr2line`, `objdump -l`, `ld`'s diagnostics) print
  `DWARF error: mangled line number section (bad file number)` on
  single-file DWARF 5 line tables, `llc`'s and hod's alike. On kiln's
  modules `llc`'s table has a second file entry (`<stdin>`) and is quiet,
  hod's is not. gdb, lldb and the LLVM tools are not affected; not part of
  the gate.
- The corpus holds only 14 `-g` modules: the line clause is met there
  and not on real sources, so the `-g` set is the measure that matters.
