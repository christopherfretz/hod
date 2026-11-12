# The M3 gate measured: debug information

The gate of `docs/ROADMAP.md` M3: *"L7: kiln's debugger tests pass on hod's
objects under gdb and lldb; `llvm-dwarfdump --verify` is clean over the
corpus; line tables agree with `llc`'s on the corpus (same set of
`(function, line)` breakpoint locations). L6 strict with -g."* Each clause
measured once, on `main` as it was (WP-099). The reference is `llc -O0
-global-isel=0 -fast-isel=0` (`docs/VALIDATION.md` "Oracles").

| | |
|---|---|
| date | 2026-11-12 |
| hod | `main` at `8f7c5f9` (no change under `crates/` up to `cc9f7c3`), release build (`cargo build --release -p hod_cli`) |
| kiln | 1.0.1 at `bd60e1829` (the submodule's pin), release build |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb 16.3; lldb 19.1.7 (`lldb-22` is not installed) |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1 (`wild` is not installed) |
| target | `aarch64-unknown-linux-gnu` |

## Verdict

| clause | verdict | what stands in the way (modules stopped) |
|---|---|---|
| L7: kiln's debugger tests | **met** | nothing: 8 of 8 sessions pass with hod (one transcript differs in a column, which the checks do not judge) |
| L7: `tests/debug` | **met** (with known expected failures) | 24 XFAIL, 3 findings of WP-060; nothing FAILs, nothing is stale |
| `--verify` clean | **met for every object hod builds** (200 of 200); not met as "the whole corpus" | 12 modules declined: `debug:label-attribute` 3, `debug:local-type-scope` 3, `debug:dwarf-version` 2, `debug:subprogram-attribute` 2, `debug:variable-alignment` 2 |
| line tables agree | **met on the corpus** (10 of 10 modules equal); **not met** on the `-g` set (42 of 190 modules have pairs §5.7 does not permit) | `unreachable` 37, `gep-placement` 23, `load-placement` 18, `is-constant` 13, `compare-at-user` 10 |
| L6 strict with `-g` | **not met**: 10 of 14 modules pass under every linker | `debug:dwarf-version` 2, `debug:subprogram-attribute` 2 |

## The modules

1. **The corpus** (`.build/corpus`, 6250 entries): 14 modules have a
   `!llvm.dbg.cu`, all kiln's (`kiln/llvm22` 7, `kiln/llvm19` 7, the same
   seven tests: `namespace-scope-debuginfo-run`, `expr.reflect/debuginfo-run`,
   `complex-int-debuginfo-run`, `class-type-argument-debuginfo-run`,
   `pattern-typedef-debuginfo-run`, `define-debuginfo-run` (DWARF 4),
   `lifetime-markers-O2-g-fpic-run` (an `-O2` unit)). No module of the
   gcc-torture source has debug information.
2. **The `-g` set** (`scripts/debug-gset.sh`): 198 modules from real
   sources at `-g -O0`, in the spirit of WP-087's and WP-094's census
   (whose libc++ list was not recorded): every 120th `*.pass.cpp` of
   libc++'s `test/std` against libstdc++ (66 chosen; 55 compile with clang,
   53 with kiln++), `stl.cpp`, `e2e.cpp`, the ten sources of `bench/src`
   and kiln's four libstdc++ units of WP-094 (kiln++ only). By producer:
   clang 19 67, clang 22 67, kiln++ 64 (LLVM 19 IR built by `llc` 19,
   the rest by `llc` 22).

## `llvm-dwarfdump --verify`

hod's object and `llc`'s of each module, each checked by
`llvm-dwarfdump` 22 and 19.

| | modules | hod built | hod clean | `llc` clean | declined |
|---|---:|---:|---:|---:|---:|
| corpus, LLVM 22 | 7 | 5 | 5 | 7 | 2 |
| corpus, LLVM 19 | 7 | 5 | 5 | 7 | 2 |
| `-g` set, clang 19 | 67 | 65 | 65 | 67 | 2 |
| `-g` set, clang 22 | 67 | 64 | 64 | 67 | 3 |
| `-g` set, kiln++ | 64 | 61 | 61 | 64 | 3 |
| **all** | **212** | **200** | **200** | **212** | **12** |

No verify failure on either writer's objects. Declines by reason (the
first refusal of `hod --strict`):

| reason | modules | which | status |
|---|---:|---|---|
| `debug:label-attribute` | 3 | coroutine suspend labels: libc++'s `coroutine.handle.prom/promise.pass.cpp` (clang 22, kiln++), kiln's `libstdcxx-generator-int-run.cpp` | known, WP-098 |
| `debug:local-type-scope` | 3 | libc++'s `thread.jthread/assign.move.pass.cpp` (clang 19, 22, kiln++): a lambda local to libstdc++'s `always_inline` `__atomic_semaphore::_M_acquire` | finding: `tests/debug/local_type_inlined.cpp` |
| `debug:dwarf-version` | 2 | corpus `define-debuginfo-run.cpp` (`-gdwarf-4`), LLVM 19 and 22 | known, WP-098 |
| `debug:subprogram-attribute` | 2 | corpus `lifetime-markers-O2-g-fpic-run.cpp`, LLVM 19 and 22: `DISPFlagOptimized` (an `-O2 -g` unit; it also has `#dbg_value`, which is `debug:dbg-value` once this is lifted) | finding: `tests/debug/optimized_unit.c` |
| `debug:variable-alignment` | 2 | libc++'s `atomics.types.float/exchange.pass.cpp` (clang 19, 22): an `alignas` local of libstdc++'s `always_inline` atomic `exchange` (abstract instance) | finding: `tests/debug/align_inlined.cpp` |

Reproduce: `cargo xtask debug verify -j 4 --hod <release hod> -v` (the
corpus: `14 -g modules: 10 built (10 clean, 0 not clean), 4 declined`);
`scripts/debug-measure.py` for `llc`'s objects and the `-g` set (below).

## Line tables

The `(function, file, line)` pairs of every `is_stmt` row with a line
other than 0 and a non-empty range, by the rule of `xtask/src/debug/lines.rs`
(`spec/harness.md` "debug", "Lines"), on hod's and `llc`'s programs of each
module both build. `scripts/debug-measure.py` links each object into a PIE
without start files and with undefined symbols allowed, so that modules
without `main` (most libc++ tests are `main`-only, `stl.cpp` has none) get
one address per function.

| | modules | identical sets | no unclassified pair | pairs the same | P1 | P3 | unclassified |
|---|---:|---:|---:|---:|---:|---:|---:|
| corpus (both majors) | 10 | 10 | 10 | 86 | 0 | 0 | 0 |
| `-g` set, clang 19 | 65 | 30 | 51 | 61 457 | 810 (P1 + P3) | | 181 |
| `-g` set, clang 22 | 64 | 29 | 50 | 61 366 | 810 (P1 + P3) | | 181 |
| `-g` set, kiln++ | 61 | 28 | 47 | 64 078 | 947 (P1 + P3) | | 217 |
| **`-g` set** | **190** | **87** | **148** | **186 901** | **2 473** | **94** | **579** |

The 579 unclassified pairs (42 modules) are of five causes, read from the
IR by `scripts/debug-pairs-ir.py`; each is reduced to a source under
`xtask/tests/data/debug_lines/` with an ignored test in
`xtask/tests/debug_m3.rs` (`tests/debug` cannot hold them: an unclassified
pair fails `cargo xtask debug` whatever its `xfail` says). Under gdb and
lldb, a breakpoint at the line stops on another line (or never) in one of
the programs, under every linker; no value or frame differs. In order of
modules stopped:

| cause | side | pairs | modules (only this cause) | what | reduced | responsible |
|---|---|---:|---:|---|---|---|
| `unreachable` | hod | 223 | 37 (11) | hod writes a trap for `unreachable` with its location (libstdc++'s `__builtin_unreachable()`); `llc` writes no code, so no row | `unreachable_line.c` | `crates/hod_a64/src/term.rs:57` |
| `gep-placement` | hod 213, `llc` 33 | 246 | 23 (0) | a `getelementptr` on a line of its own: hod has code for a zero-offset one `llc` folds into its user (`this->_M_impl` in `_M_realloc_append`), and folds one `llc` computes at its line (`_M_node + 1` in `deque::_M_push_back_aux`) | `gep_member.cpp`, `gep_offset.c` | `crates/hod_a64/src/mem.rs:563` (deferred only when the use is the next instruction) |
| `load-placement` | `llc` 40, hod 20 | 60 | 18 (2) | `llc` merges a load whose value is unused (a trivial destructor's `n->~T()`) with the next load of the same address and keeps the first's line, hod drops it; hod's load of a call argument on its own line has a row where `llc` places it with the call | `dead_load.cpp`, `arg_load.c` | `crates/hod_cg/src/driver.rs:541` (dead), `:575` (rows) |
| `is-constant` | hod | 40 | 13 (3) | `llc` folds `llvm.is.constant` and deletes the blocks it guards, even at `-O0`; hod drops the instructions but keeps the blocks (`std::__advance`) | `is_constant.c` | `crates/hod_a64/src/tail.rs:459` (`begin`) |
| `compare-at-user` | `llc` | 10 | 10 (0) | an `i1` compare returned by the next instruction: `llc` fuses the `and` into it at the compare's line, hod's compare is deferred to its user's line (`bitset::_Unchecked_test`) | `compare_user.cpp` | `crates/hod_cg/src/policy.rs:488` (`may_defer`), `crates/hod_a64/src/target.rs:236` |

All five are where code generation puts (or omits) instructions, not
errors of the line program: the rows hod writes are its instructions'
locations. Each needs either a code-generation change or a class in
`spec/dwarf.md` §5.7; `spec/dwarf.md` §6.12 already names `is-constant`
for DIE trees. `unreachable` alone would leave 31 of 42 modules still
differing; with `unreachable`, `is-constant` and both `load-placement`
directions, 23 (the `gep-placement` modules).

Reproduce:

```
scripts/debug-gset.sh <dir>/gset
scripts/debug-measure.py --hod <release hod> --work <dir>/work -j 4 .build/corpus > corpus.tsv
scripts/debug-measure.py --hod <release hod> --work <dir>/gwork -j 4 <dir>/gset > gset.tsv
scripts/debug-pairs-ir.py <dir>/gwork > unclassified.tsv
cargo test -p xtask --test debug_m3 -- --ignored   # the seven reductions; all fail today
```

## L6 strict with `-g`

`cargo xtask irdiff --llvm <N> --hod <release hod>` (strict is
`irdiff`'s default; `corpus verify --strict` runs every module, so the
`-g` modules are given as paths) on the 7 `-g` modules of each major, every
linker found:

| LLVM | linker | PASS | FAIL (declined) |
|---|---|---:|---:|
| 22 | bfd, lld, mold | 5 each | 2 each |
| 19 | bfd, lld, mold | 5 each | 2 each |

The FAILs are the declines above: `debug:dwarf-version`
(`define-debuginfo-run.cpp`) and `debug:subprogram-attribute`
(`lifetime-markers-O2-g-fpic-run.cpp`). Every module hod builds runs as
`llc`'s does under the three linkers.

## kiln's debugger tests and `tests/debug`

`cargo xtask debug kiln --hod <release hod>`: kiln's four debugger tests
(`globals_lambda.cpp`, `locals.cpp`, `reflection.cpp`, `step_finish.cpp`)
under gdb and lldb 19, linked by kiln++'s own driver (not per linker):
`llc` 8 of 8 pass, hod 8 of 8 pass; 7 of 8 transcripts equal. The eighth,
`step_finish.cpp` under lldb, shows column 7 (hod) against 15 (`llc`) after
`finish`: WP-060's `finish_column.c`, being worked by WP-097.

`cargo xtask debug run -j 4 --hod <release hod>`: 15 tests, front ends
clang 19, clang 22 and kiln++, linkers bfd, lld and mold, gdb and lldb 19:
198 results, 174 PASS, 24 XFAIL, no FAIL, XPASS, ERROR or crash, 360
sessions in 21 s; `(function, line)` 458 pairs, all the same. The
expected failures, all of WP-060:

| test | results | reason | status |
|---|---:|---|---|
| `sret_entry.c` | 12 (gdb, lldb) | `llc`'s location list of the `sret` variable has a gap at the first line's row; hod's shows the value | permitted by §6.7 since WP-094, still compared as a difference; the session prints the caller's uninitialized object |
| `gep_column.c` | 6 (lldb) | hod's first instruction of the line is a `getelementptr` `llc` folds into the store | WP-097 (columns) |
| `finish_column.c` | 6 (lldb) | the return address of a call has the assignment's column in hod's program | WP-097 (columns) |

Nothing is stale. This package adds three tests whose module hod
declines (above); with them the run has 211 results, 7 more XFAIL
(`optimized_unit.c` 2, `align_inlined.cpp` 2, `local_type_inlined.cpp` 3).

## Other observations

- GNU binutils (2.44: `addr2line`, `objdump -l`, `ld`'s diagnostics) print
  `DWARF error: mangled line number section (bad file number)` on
  single-file DWARF 5 line tables, `llc`'s and hod's alike. On kiln's
  modules `llc`'s table has a second file entry (`<stdin>`) and is quiet,
  hod's is not. gdb, lldb and the LLVM tools are not affected; not part of
  the gate.
- The corpus holds only 14 `-g` modules: the line clause is met there
  and not on real sources, so the `-g` set is the measure that matters.
