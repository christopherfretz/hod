# The M3 gate measured: debug information

The gate of `docs/ROADMAP.md` M3: *"L7: kiln's debugger tests pass on hod's
objects under gdb and lldb; `llvm-dwarfdump --verify` is clean over the
corpus; line tables agree with `llc`'s on the corpus (same set of
`(function, line)` breakpoint locations). L6 strict with -g."* Each clause
measured first by WP-099, again by WP-111 and WP-150, and, as here, by
WP-244 (the corpus, `tests/debug` and the verdict; the `-g` set's rows
are WP-150's, not measured again). The reference is `llc -O0
-global-isel=0 -fast-isel=0` (`docs/VALIDATION.md` "Oracles"; x86-64
`-fast-isel=0 -relocation-model=pic`).

| | |
|---|---|
| date | 2026-11-15 16:00 to 16:13 EST (WP-257c, the candidate: "At the candidate" below); the sections after it 2026-11-15 (WP-244); the `-g` set 2026-11-13 (WP-150) |
| hod | the candidate `C` = `cd6ef6bc`, its frozen release binary `hod-cd6ef6bc` (sha256 `80138ec1411506d5b4048f1eac49e8b377f9eeedb21e70ae43e3c81ab094929c`, `hod version 0.0.0`) for "At the candidate"; the sections after it: `e7e473af` (wave 25: `main` at `56a4695f` with kiln's pin at 1.2.0, wild and WP-238 part 1), release build (`cargo build --release -p hod_cli`); the `-g` set: WP-150's branch on `3a98566` |
| kiln | 1.2.0 at `8f93304ee` (the submodule's pin), release build; the corpus captured with it; the `-g` set: 1.0.1 at `bd60e1829` |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb 16.3; lldb 19.1.7 (`lldb-22` is not installed) |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1, wild 0.11.0 |
| targets | `aarch64-unknown-linux-gnu` (native); `x86_64-unknown-linux-gnu` for `tests/debug` (qemu-user's gdb stub) |

## At the candidate

`C` = `cd6ef6bc`, kiln `K` = `8f93304ee`, the frozen binary above, every
command with `HOD_REQUIRE_TOOLS=1 -j 3 --hod hod-cd6ef6bc` on the AArch64
host, 2026-11-15 16:00 to 16:13 EST (WP-257c). x86-64: `debug-x64.md`.

| command | result | exit | wall |
|---|---|---|---|
| `cargo xtask debug run` | 993 results: 973 PASS, 16 XFAIL (`sret_entry.c`, §6.7), 3 XLINK (wild with gdb, WP-248), 1 SKIP, 0 FAIL, 0 XPASS, 0 ERROR; 1888 sessions; lines: same 1801, hod only 11 (P3 9, `selector-split` 2), `llc` only 18 (P1 18), **0 unclassified** | 0 | 7:34 |
| `cargo xtask debug verify` (the corpus) | 18 `-g` modules: 18 built, 18 clean, 0 declined, 0 hod failures | 0 | 0:14 |
| `cargo xtask debug kiln` | kiln's `tests/debuginfo`: 17 sessions, `llc` 17 pass, hod 17 pass, 15 with equal transcripts (`step_finish.cpp` and `template_names_anon_union.cpp` under lldb-19 pass with both, transcripts differ); no test passes with `llc` and not with hod. The two-unit test `class_decl_two_units.cpp` (gdb and lldb-19) is measured since WP-259 (the harness expands kiln's `%{test_dir}`) and passes with both writers | 0 | 0:22 |
| `cargo xtask debug trees` (the corpus) | 18 `-g` modules: 8 same (822 DIEs), 10 permitted (8 `realigned frame`; 2 `unit range lists, value records`: `lifetime-markers-O2-g-fpic-run.cpp`, LLVM 22 and 19, an `-O2` unit, classified by WP-260), 0 differ, 0 declined, 0 failed | 0 | 0:14 |

At the previous candidate `e98267c0` (WP-257b, `hod-e98267c0`, 15:02 to
15:15 EST) every count above was the same. At the candidate before it,
`02fe2d02` (WP-257, 06:14 to 06:29 EST), the
same `debug run` and `debug verify` counts; `debug kiln` 15 of 17 (the
two-unit test unmeasured before WP-259) and `debug trees` 2 differ, exit
1 (the `-O2` unit, before WP-260's classes).

L6 strict with `-g`: the corpus's 18 `-g` entries are part of 3.1
(`docs/status/release.md`).

## Verdict

The table below is WP-244's at `e7e473af`; at the candidate (above) the
`tests/debug` clause has no unclassified pair (the `declare_fragment.c`
pair is set apart as `selector-split` since WP-250) and the run exits 0,
kiln's debugger tests were run again (17 of 17 sessions with both
writers, above), and `debug trees` has no difference left (exit 0).

| clause | verdict | what stands in the way (modules stopped) |
|---|---|---|
| L7: kiln's debugger tests | **met** at WP-150 (8 of 8 sessions pass with hod); not run again by WP-244 | nothing known |
| L7: `tests/debug` | **not met** on either target: no session FAILs, but the lines comparison has an unclassified pair, which fails the run (exit 1) | `declare_fragment.c` (clang 19 and 22 on AArch64, clang 22 on x86-64): a row for line 18 (`j--`) in `sort` only hod's program has; `llc` merges that `add` into the loop's compare at line 17. AArch64 also: 3 ERROR under wild and gdb, `llc`'s session failing the same check as hod's (`stack_protector.c` 2, `hardening_a64.cpp` 1); 16 XFAIL (`sret_entry.c`, §6.7); nothing is stale |
| `--verify` clean | **met** on the corpus: 18 of 18 `-g` modules built and clean; the `-g` set 198 of 198 (WP-150) | nothing |
| line tables agree | **met** on the corpus (18 of 18 modules without a pair §5.7 does not permit; 16 with equal sets) and on the `-g` set (WP-150) | nothing |
| L6 strict with `-g` | **met**: 18 of 18 modules pass under every linker, both targets | nothing |

## The modules

1. **The corpus** (`.build/corpus`, captured with kiln 1.2.0): 18 modules
   have a `!llvm.dbg.cu`, all kiln's (`kiln/llvm22` 9, `kiln/llvm19` 9,
   the same nine tests: `namespace-scope-debuginfo-run`,
   `expr.reflect/debuginfo-run`, `complex-int-debuginfo-run`,
   `class-type-argument-debuginfo-run`,
   `class-type-argument-local-debuginfo-run`,
   `pattern-typedef-debuginfo-run`,
   `definition-in-reopened-namespace-run`, `define-debuginfo-run` (DWARF
   4), `lifetime-markers-O2-g-fpic-run` (an `-O2` unit)). No module of the
   gcc-torture source has debug information. The x86-64 corpus
   (`.build/corpus-x64`) has the same 18.
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
| corpus, LLVM 22 | 9 | 9 | 9 | 9 | 0 |
| corpus, LLVM 19 | 9 | 9 | 9 | 9 | 0 |
| `-g` set, clang 19 (WP-150) | 67 | 67 | 67 | 67 | 0 |
| `-g` set, clang 22 (WP-150) | 67 | 67 | 67 | 67 | 0 |
| `-g` set, kiln++ (WP-150) | 64 | 64 | 64 | 64 | 0 |
| **all** | **216** | **216** | **216** | **216** | **0** |

No verify failure on either writer's objects, and no decline: the
corpus's last one, `lifetime-markers-O2-g-fpic-run.cpp`
(`debug:dbg-value-arglist`), is built since WP-230 (`DIArgList` value
records) and verifies clean.
The x86-64 corpus's 18 `-g` modules are all built and clean too
(`cargo xtask debug verify --target x86_64 ... .build/corpus-x64`).

Reproduce: `cargo xtask debug verify -j 4 --hod <release hod> -v` (the
corpus: `18 -g modules: 18 built (18 clean, 0 not clean), 0 declined`);
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
| corpus (both majors) | 18 | 16 | 18 | 236 | 0 | 2 | 0 | 0 | 0 | 0 |
| `-g` set, clang 19 | 67 | 30 | 67 | 64 347 | 819 | 33 | 20 | 0 | 12 | 0 |
| `-g` set, clang 22 | 67 | 30 | 67 | 64 335 | 819 | 33 | 20 | 4 | 12 | 0 |
| `-g` set, kiln++ | 64 | 29 | 64 | 65 999 | 952 | 33 | 20 | 0 | 16 | 0 |
| **`-g` set** | **198** | **89** | **198** | **194 681** | **2 590** | **99** | **60** | **4** | **40** | **0** |

Pairs only hod's program has: 163 (P3 99, `load-placement` 20,
`post-indexed-load` 4, `is-constant` 40); only `llc`'s: 2 630 (P1 2 590,
`load-placement` 40). The `-g` set's counts are WP-111's, pair for pair
(WP-150; not measured again by WP-244). The corpus row is WP-244's: the
two pairs only hod's program has are P3, both in
`lifetime-markers-O2-g-fpic-run.cpp`.

Reproduce:

```
scripts/debug-gset.sh <dir>/gset
scripts/debug-measure.py --hod <release hod> --work <dir>/work -j 4 .build/corpus > corpus.tsv
scripts/debug-measure.py --hod <release hod> --work <dir>/gwork -j 4 <dir>/gset > gset.tsv
```

## L6 strict with `-g`

The `-g` entries' results in `cargo xtask corpus verify --strict` (and
`--target x86_64` over `.build/corpus-x64`), every linker:

| target | LLVM | linker | PASS | FAIL |
|---|---|---|---:|---:|
| AArch64 | 22, 19 | bfd, lld, mold, wild | 9 each | 0 |
| x86-64 | 22, 19 | bfd, lld, mold, wild | 9 each | 0 |

Every `-g` module is built by hod and runs as `llc`'s does under the
four linkers (`spec/landed/harness/WP-244.md`).

## kiln's debugger tests and `tests/debug`

WP-150's measurement (not run again by WP-244):
`cargo xtask debug kiln --hod <release hod>`: kiln's four debugger tests
(`globals_lambda.cpp`, `locals.cpp`, `reflection.cpp`, `step_finish.cpp`)
under gdb and lldb 19, linked by kiln++'s own driver: `llc` 8 of 8 pass,
hod 8 of 8 pass; 7 of 8 transcripts equal. The eighth, `step_finish.cpp`
under lldb, shows column 7 (hod) against 15 (`llc`) after `finish`:
`spec/dwarf.md` §5.7 `return-column`.

WP-244: `cargo xtask debug run -j 4 --hod <release hod>` (and `--target
x86_64`) with `HOD_REQUIRE_TOOLS=1`: 56 tests, front ends clang 19, clang
22 and kiln++, linkers bfd, lld, mold and wild, gdb and lldb 19.

| target | results | PASS | XFAIL | ERROR | SKIP | sessions | `(function, file, line)` pairs | wall |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| AArch64 | 977 | 957 | 16 | 3 | 1 | 1 856 | 1 798: 1 773 the same, 11 hod's only (P3 9, **unclassified 2**), 14 `llc`'s only (P1) | 4:24 |
| x86-64 | 921 | 880 | 8 | 0 | 33 | 1 680 | 1 748: 1 733 the same, 1 hod's only (**unclassified 1**), 14 `llc`'s only (P1) | 1:50 |

No session FAILs, XPASSes or crashes on either target, but both runs end
with exit status 1: the unclassified pairs are one line of one test,
`declare_fragment.c:18` (`j--`) in `sort` (WP-230's test, clang `-O1`),
by clang 19 and 22 on AArch64 and clang 22 on x86-64. hod gives the
`add nsw i32 %j, -1` of line 18 a row of its own; `llc` computes it
with the loop's compare in one `subs` at line 17, so its table has no
row for line 18. No §5.7 class covers it.

The three ERRORs (AArch64) are wild with gdb, where `llc`'s session also
fails the check that hod's fails: `stack_protector.c` by clang 19 and 22
(`b = "hello\000"`) and `hardening_a64.cpp` by clang 22 (the frame in
`guarded`). The same tests pass under bfd, lld and mold, so wild's
output and gdb disagree there whatever the code generator.

The expected failures and skips:

| test | results | reason | status |
|---|---:|---|---|
| `sret_entry.c` | 16 XFAIL (AArch64) | `llc`'s location list of the `sret` variable has a gap at the first line's row; hod's shows the value | permitted by §6.7 since WP-094, still compared as a difference |
| `key_call.c` | 8 XFAIL (x86-64, gdb) | `llc`'s x86-64 code shows `produce (x=0)` after `step` (the parameter before its spill); hod's shows x=4 | |
| `tls.c`, `tls_static.c`, `dwarf4_tls.c` | 24 SKIP (x86-64, gdb) | qemu-user's gdb stub has no `qGetTLSAddr` | `spec/harness.md` "debug", "Other targets" |
| `call_sites.c`, `sret_entry.c`, `hardening_a64.cpp`, `local_static.ll`, `local_type_no_code.ll` | 9 SKIP (x86-64) | for AArch64 only (`target:`) | |
| `hardening_x64.cpp` | 1 SKIP (AArch64) | for x86-64 only | |

No XFAIL passes: no expectation is stale. WP-150's `call_sites.c`
expected failures (clang 22's key instructions) are gone.

## Other observations

- GNU binutils (2.44: `addr2line`, `objdump -l`, `ld`'s diagnostics) print
  `DWARF error: mangled line number section (bad file number)` on
  single-file DWARF 5 line tables, `llc`'s and hod's alike. On kiln's
  modules `llc`'s table has a second file entry (`<stdin>`) and is quiet,
  hod's is not. gdb, lldb and the LLVM tools are not affected; not part of
  the gate.
- The corpus holds only 14 `-g` modules: the line clause is met there
  and not on real sources, so the `-g` set is the measure that matters.

## i686 (WP-329)

`cargo xtask debug --target i686 -j 4` on the AArch64 host, 2026-11-16,
hod at WP-329's head (qemu-i386's gdb stub; gdb-multiarch 16.3,
lldb-19; GNU ld, lld, mold): 445 results, 354 PASS, 34 FAIL, 57 SKIP
(22 of them kept off: 20 `debug:dwarf-4-i386`, 2
`debug:split-dwarf-i386`), 738 sessions, 110 s wall. Not in the gate
yet: the FAILs are listed in `spec/landed/harness/WP-329.md`.
