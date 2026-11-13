# The M3 gate measured: debug information

The gate of `docs/ROADMAP.md` M3: *"L7: kiln's debugger tests pass on hod's
objects under gdb and lldb; `llvm-dwarfdump --verify` is clean over the
corpus; line tables agree with `llc`'s on the corpus (same set of
`(function, line)` breakpoint locations). L6 strict with -g."* Each clause
measured first by WP-099 and again, as here, by WP-111. The reference is
`llc -O0 -global-isel=0 -fast-isel=0` (`docs/VALIDATION.md` "Oracles").

| | |
|---|---|
| date | 2026-11-13 |
| hod | WP-111's branch (`main` at `73865eb` and WP-111's commits), release build (`cargo build --release -p hod_cli`) |
| kiln | 1.0.1 at `bd60e1829` (the submodule's pin), release build |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb 16.3; lldb 19.1.7 (`lldb-22` is not installed) |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1 (`wild` is not installed) |
| target | `aarch64-unknown-linux-gnu` |

## Verdict

| clause | verdict | what stands in the way (modules stopped) |
|---|---|---|
| L7: kiln's debugger tests | **met** | nothing: 8 of 8 sessions pass with hod (one transcript differs in a column after `finish`, `spec/dwarf.md` §5.7 `return-column`) |
| L7: `tests/debug` | **met** (with known expected failures) | 14 XFAIL: `sret_entry.c` 12 (§6.7, permitted), `optimized_unit.c` 2 (`debug:call-sites`); nothing FAILs, nothing is stale |
| `--verify` clean | **met for every object hod builds** (208 of 208); not met as "the whole corpus" | 4 corpus modules declined: `debug:call-sites` 2, `debug:dwarf-version` 2 |
| line tables agree | **met** on the corpus (10 of 10 modules equal) and on the `-g` set (198 of 198 modules without a pair §5.7 does not permit; 89 with equal sets) | nothing |
| L6 strict with `-g` | **not met**: 10 of 14 modules pass under every linker | `debug:call-sites` 2, `debug:dwarf-version` 2 |

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
| corpus, LLVM 22 | 7 | 5 | 5 | 7 | 2 |
| corpus, LLVM 19 | 7 | 5 | 5 | 7 | 2 |
| `-g` set, clang 19 | 67 | 67 | 67 | 67 | 0 |
| `-g` set, clang 22 | 67 | 67 | 67 | 67 | 0 |
| `-g` set, kiln++ | 64 | 64 | 64 | 64 | 0 |
| **all** | **212** | **208** | **208** | **212** | **4** |

No verify failure on either writer's objects. Declines by reason (the
first refusal of `hod --strict`):

| reason | modules | which | status |
|---|---:|---|---|
| `debug:call-sites` | 2 | corpus `lifetime-markers-O2-g-fpic-run.cpp`, LLVM 19 and 22: `DIFlagAllCallsDescribed` (an `-O2 -g` unit) | `spec/dwarf.md` §6.14; `tests/debug/optimized_unit.c` |
| `debug:dwarf-version` | 2 | corpus `define-debuginfo-run.cpp` (`-gdwarf-4`), LLVM 19 and 22 | known, WP-098 (DWARF 4 planned) |

WP-099's other declines are gone: `debug:label-attribute` (WP-098),
`debug:local-type-scope` and `debug:variable-alignment` (lifted since
WP-099); `debug:subprogram-attribute` is now reached as
`debug:call-sites` on the same modules.

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
one address per function. The classes are `spec/dwarf.md` §5.7's,
recognised from the IR (`xtask/src/debug/permitted.rs`).

| | modules | identical sets | no unclassified pair | pairs the same | P1 | P3 | load-placement | post-indexed-load | is-constant | unclassified |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| corpus (both majors) | 10 | 10 | 10 | 86 | 0 | 0 | 0 | 0 | 0 | 0 |
| `-g` set, clang 19 | 67 | 30 | 67 | 64 347 | 819 | 33 | 20 | 0 | 12 | 0 |
| `-g` set, clang 22 | 67 | 30 | 67 | 64 335 | 819 | 33 | 20 | 4 | 12 | 0 |
| `-g` set, kiln++ | 64 | 29 | 64 | 65 999 | 952 | 33 | 20 | 0 | 16 | 0 |
| **`-g` set** | **198** | **89** | **198** | **194 681** | **2 590** | **99** | **60** | **4** | **40** | **0** |

Pairs only hod's program has: 163 (P3 99, `load-placement` 20,
`post-indexed-load` 4, `is-constant` 40); only `llc`'s: 2 630 (P1 2 590,
`load-placement` 40). WP-099's five causes of unclassified pairs are
fixed or classified by WP-097; its last four (`_Sp_counted_base::_M_release_last_use`
in four clang 22 modules: `stl.cpp`, `bench/src/stl_heavy.cpp`, libc++'s
`directory_iterator.members/copy_assign.pass.cpp` and
`re.regex.construct/ptr_size_flg.pass.cpp`) are `post-indexed-load`
since WP-111, which also counts only the pair's file and blocks with a
predecessor (`xtask/tests/debug_post_index.rs`).

Reproduce:

```
scripts/debug-gset.sh <dir>/gset
scripts/debug-measure.py --hod <release hod> --work <dir>/work -j 4 .build/corpus > corpus.tsv
scripts/debug-measure.py --hod <release hod> --work <dir>/gwork -j 4 <dir>/gset > gset.tsv
```

## L6 strict with `-g`

`cargo xtask irdiff --llvm <N> --hod <release hod>` (strict is
`irdiff`'s default) on the 7 `-g` modules of each major, every linker
found:

| LLVM | linker | PASS | FAIL (declined) |
|---|---|---:|---:|
| 22 | bfd, lld, mold | 5 each | 2 each |
| 19 | bfd, lld, mold | 5 each | 2 each |

The FAILs are the declines above: `debug:dwarf-version`
(`define-debuginfo-run.cpp`) and `debug:call-sites`
(`lifetime-markers-O2-g-fpic-run.cpp`). Every module hod builds runs as
`llc`'s does under the three linkers.

## kiln's debugger tests and `tests/debug`

`cargo xtask debug kiln --hod <release hod>`: kiln's four debugger tests
(`globals_lambda.cpp`, `locals.cpp`, `reflection.cpp`, `step_finish.cpp`)
under gdb and lldb 19, linked by kiln++'s own driver: `llc` 8 of 8 pass,
hod 8 of 8 pass; 7 of 8 transcripts equal. The eighth, `step_finish.cpp`
under lldb, shows column 7 (hod) against 15 (`llc`) after `finish`:
`spec/dwarf.md` §5.7 `return-column`.

`cargo xtask debug run -j 4 --hod <release hod>`: 19 tests, front ends
clang 19, clang 22 and kiln++, linkers bfd, lld and mold, gdb and lldb 19:
284 results, 270 PASS, 14 XFAIL, no FAIL, XPASS, ERROR or crash, 528
sessions; `(function, file, line)` 697 pairs, 695 the same, 2 `llc`'s
only (P1). The expected failures:

| test | results | reason | status |
|---|---:|---|---|
| `sret_entry.c` | 12 (gdb, lldb) | `llc`'s location list of the `sret` variable has a gap at the first line's row; hod's shows the value | permitted by §6.7 since WP-094, still compared as a difference |
| `optimized_unit.c` | 2 | `DIFlagAllCallsDescribed`: `llc` writes call-site entries, hod declines them | `debug:call-sites`, §6.14 |

`coro_labels.cpp`'s clang 22 results (6) were expected failures
(`debug:variable-twice`) until WP-111; they pass.

## Other observations

- GNU binutils (2.44: `addr2line`, `objdump -l`, `ld`'s diagnostics) print
  `DWARF error: mangled line number section (bad file number)` on
  single-file DWARF 5 line tables, `llc`'s and hod's alike. On kiln's
  modules `llc`'s table has a second file entry (`<stdin>`) and is quiet,
  hod's is not. gdb, lldb and the LLVM tools are not affected; not part of
  the gate.
- The corpus holds only 14 `-g` modules: the line clause is met there
  and not on real sources, so the `-g` set is the measure that matters.
