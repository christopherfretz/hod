# Release candidate 1.0: the checklist

The conditions of the release-candidate gate (`docs/VALIDATION.md`
"Gates", "Release candidate"; `docs/ROADMAP.md` M6) and of the owner's
definition of 1.0, each with the evidence it needs, the evidence on `main`
today and its state. How to produce each piece of evidence, and the
candidate's order of work, is [`docs/RELEASE.md`](../RELEASE.md). Not
conditions of 1.0: the benchmark gate (`perf.md` stays as last measured),
the in-process bridge, the 32-bit x86 target, and wild (an optional
linker of the test matrix).

| | |
|---|---|
| written | 2026-11-15 15:20 EST, WP-257b for the candidate `e98267c0` (first written 2026-11-14 against `56a4695f`, WP-243; then WP-256; WP-257 for the previous candidate `02fe2d02`) |
| candidate `C` | hod `e98267c0` (`main`, frozen 2026-11-15 14:50 EST); no version bump, no tag (version `0.0.0`). It replaces the candidate `02fe2d02` of 06:07 EST; since that one, hod's code changed by WP-261 (the dead arm of an `llvm.is.constant`/`llvm.objectsize` branch is not compiled), WP-262 (a coroutine suspend in an unreachable block) and WP-267 (`llvm.x86.tpause`, `umwait`, `umonitor`), the harness by WP-259, WP-260, WP-263, WP-266 and WP-268 |
| kiln `K` | `8f93304ee` (kiln 1.2.0) in all three places (the submodule, `.github/kiln-revision`, the corpora's kiln groups); it does not move before the release |
| frozen binary | `cargo build --release -p hod_cli` of the tree of `faf4714d` (14:26 to 14:27 EST; `e98267c0` is `faf4714d` plus one commit to `docs/BACKLOG.md`, so the same code), copied out of the build directory: `hod-e98267c0`, sha256 `e076c68e7c988cdbe504a8f97e2521feabdc94341f1418cf3baf70a485b9cbdc`, `hod version 0.0.0` |
| full gate | `HOD_REQUIRE_TOOLS=1 cargo xtask ci` on `014a64f8`, 2026-11-15 12:48 to 14:50 EST: `ci: ok`, exit 0, 2:00:34 (fmt 8.2 s, clippy 42.2 s, test 48:15, irdiff 2:08, roundtrip 18:42, debug-x86_64 2:18, debug-aarch64 3:08, corpus-aarch64 18:38, corpus-x86_64 26:30), with the workspace's development hod. `git diff --stat 014a64f8 e98267c0`: `.github/workflows/oss.yml`, `docs/BACKLOG.md`, `scripts/oss/` (`oss-build.sh`, `oss-report-selftest.py`, `oss-report.py`, `oss-run.sh`, `projects.py`), `spec/harness.md`, `spec/landed/harness/WP-268.md`; nothing under `crates/` or `xtask/`. So the full gate ran on `C`'s code, not on `C` itself nor with the frozen binary. The log is the supervisor's, not a page |
| quick gate | `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick` on `faf4714d` (`C` without its `docs/BACKLOG.md` commit), 2026-11-15 13:50 to 14:39 EST: `ci: ok`, exit 0, 48:14 (fmt 8.1 s, clippy 41.8 s, test 46:25, irdiff 32.9 s, roundtrip 26.0 s) |
| gate on the tag | the tagged commit gets its own full gate: pending |
| tools | Debian 13.7, AArch64 host; LLVM 22.1.8 and 19.1.7; GNU ld 2.44 (`ld.bfd` and `x86_64-linux-gnu-ld.bfd`), lld 22.1.8, mold 2.37.1, wild 0.11.0 (optional); gdb 16.3 (and `gdb-multiarch` 16.3), lldb 19.1.7; qemu-x86_64 10.0.13; rustc 1.95.0 |

States: **met at `<commit>`** (measured at that commit and nothing since
can change it; re-run at the candidate where the row says so);
**re-measure** (met at an older commit or kiln: measure again at the
candidate); **partly** (part of the condition is measured or met);
**missing** (no evidence); **not a condition** (listed for the record).
Every commit named is hod's unless it says kiln. A hod commit in the pages
is written with 7 or 8 hex digits as the page has it.

## Gates and lanes

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the quick gate on `faf4714d` (`C` without its `docs/BACKLOG.md` commit), 13:50 to 14:39 EST: `ci: ok`, exit 0, 48:14; and the full gate on `014a64f8` (`C`'s code, header), which contains every step of the quick gate: each passed, kiln `8f93304ee`, linkers bfd, lld, mold and wild. The gate of the tagged commit: pending | met at `faf4714d` (`C`'s tree but `docs/BACKLOG.md`) |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: the quick gate's steps, `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with 22 and 19, the whole-corpus `roundtrip`, `debug-x86_64`, `debug-aarch64`, `corpus-aarch64`, `corpus-x86_64` | `014a64f8`, 2026-11-15 12:48 to 14:50 EST, `ci: ok`, exit 0, 2:00:34 (step times in the header); kiln `8f93304ee`; `roundtrip` 6468 entries, 6468 as expected, 0 worse, 0 stale. Before it, for the previous candidate: `b6e9b8c2` (04:08 to 06:05 EST, 1:56:03). The full gate of the tagged commit: pending | met at `C`'s code (`014a64f8`; `C` differs from it only in `scripts/oss/`, `oss.yml`, specs and `docs/BACKLOG.md`, header) |
| gate on the x86-64 host | the gate's steps natively on x86-64 | the hosted x86-64 lane (next row): the quick gate's steps without `roundtrip`; no x86-64 development host has a corpus, so no full gate runs there | partly (awaiting: the hosted lanes' run of `e98267c0`) |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 43688918672 of `9e25d3f1`: every job of both lanes green; `test x64` 17 min on a Xeon 6975P-C, 19 AVX-512 programs natively, `knl`'s machine skipped. With `HOD_CI_SDE=1` (set since) its re-run took 20 min 40 s on an EPYC 9V74: 17 natively, the 2 AVX512-FP16 programs under SDE, `knl` built and compared but not executed. A runner with no AVX-512 is not measured yet | met at `9e25d3f1` (awaiting: one run of `e98267c0` with `check` and `check-x86_64` green, its run id and the AVX-512 block) |
| linkers bfd, lld, mold | every linking test and `irdiff` program with all three, required | the full gate on `014a64f8` (`C`'s code) under `HOD_REQUIRE_TOOLS=1`: each of its nine `irdiff`/`corpus verify` runs names `linkers bfd lld mold wild`, both debug steps name the four, and the log has no test skipped for a missing linker; versions GNU ld 2.44, lld 22.1.8, mold 2.37.1 (header) | met at `C`'s code (`014a64f8`) |
| linker wild | none: wild is optional | wild 0.11.0 (WP-239; linked in the full gate's runs above) is in the matrix where installed (`linkers::WILD_REQUIRED` false); its cases are listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`, the debugger layer's XLINK, WP-248); hod's code generators and object writer take no workaround for it | not a condition (decided 2026-11-15: GNU ld, lld and mold are the required linkers) |

## Kiln's corpus (L6) and debug information (L7)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | the full gate's `corpus-aarch64` on `014a64f8` (`C`'s code, the workspace's development hod), kiln `8f93304ee`: LLVM 22 13,012 PASS of 13,012, hod built 3,253 of 3,253; LLVM 19 12,860 of 12,860, 3,215 of 3,215; four linkers; `docs/status/corpus-expected.tsv` has no AArch64 row, 0 list problems. **3.1 with the frozen binary `hod-e98267c0`**: being measured at `e98267c0` (WP-257b). At the previous candidate `02fe2d02` (WP-257, `hod-02fe2d02`): the same counts as the gate, exit 0 | met at `C`'s code (`014a64f8`); with the frozen binary: being measured at `e98267c0` |
| L6 strict, x86-64 | the same under qemu | the full gate's `corpus-x86_64` on `014a64f8` (`C`'s code, the development hod), kiln `8f93304ee`: LLVM 22 13,004 results, 12,996 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, hod built 3,250 of 3,251; LLVM 19 12,852 results, 12,844 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, 3,212 of 3,213; the one test hod does not build is `cc-run.cpp` (`cc:x86_regcallcc`, by design: EXPECTED), `builtins/strncmp.c` UNDEFINED (the test's own undefined behaviour), 0 list problems. **3.2 and the hybrid run with the frozen binary `hod-e98267c0`**: being measured at `e98267c0` (WP-257b). At the previous candidate `02fe2d02` (WP-257, `hod-02fe2d02`): strict the same counts as the gate, exit 0; hybrid 13,000 PASS of 13,004 and 12,848 of 12,852, the rest `strncmp.c` | met at `C`'s code (`014a64f8`, with the one expected row); with the frozen binary: being measured at `e98267c0` |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | the full gate's `debug-aarch64` on `014a64f8` (`C`'s code, the development hod), kiln `8f93304ee`: 993 results, 973 PASS, 16 XFAIL, 3 XLINK (wild with gdb, WP-248), 1 SKIP, 0 FAIL, 0 XPASS, 1888 sessions; lines: 0 unclassified (2 `selector-split` pairs, set apart since WP-250); `debug verify` 18 of 18 `-g` modules clean, 0 declined; L6 strict with `-g` in `corpus-aarch64`. Not in the gate: `debug kiln`, `debug trees`. **With the frozen binary `hod-e98267c0`** (WP-257b, 2026-11-15 15:02 to 15:15 EST, `docs/status/debug.md` "At the candidate"): `debug run` the same counts, exit 0; `debug verify` 18 of 18 clean, 0 declined; `debug kiln` 17 sessions, `llc` 17 pass, hod 17 pass, 15 with equal transcripts, exit 0 (the two-unit test measured since WP-259); `debug trees` 8 same (822 DIEs), 10 permitted, 0 differ, exit 0 (the `-O2` unit's 2 permitted since WP-260) | met at `e98267c0` (sessions, verify, lines, kiln's tests, trees); L6 with `-g` in 3.1 being measured at `e98267c0` |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | the full gate's `debug-x86_64` on `014a64f8`: 937 results, 896 PASS, 8 XFAIL, 33 SKIP (the stub's), 0 FAIL, 1712 sessions; lines 0 unclassified; `debug verify --target x86_64` 18 of 18 clean, 0 declined. **With the frozen binary `hod-e98267c0`** (WP-257b, 15:09 to 15:15 EST, `docs/status/debug-x64.md` "At the candidate"): the same `debug run` counts, exit 0; `debug verify` 18 of 18 clean; `debug trees` 16 same (1722 DIEs), 2 permitted, 0 differ, exit 0 (WP-260) | met at `e98267c0` (sessions, verify, lines, trees); L6 with `-g` in 3.2 being measured at `e98267c0` |

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | 2026-11-11, kiln 1.0.1 `bd60e1829`: 9247 tests, 8609 pass, 4 fail, 47 xfail, 1 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (kiln is at `8f93304ee` since; awaiting: 4.1 at `K`) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | 2026-11-12/13, kiln `bd60e1829`: 9247, 8608 pass, 5 fail, 46 xfail, 2 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (awaiting: 4.2 at `K`) |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | native runner, run 43465227200, `a67370e`, kiln `bd60e1829`: hod strict 8,748 pass, `llc` 8,749; one difference, `alg.find/find.pass.cpp` (`abi:vector-element`, built since WP-167). Development host (WP-102) at `6e6bf9c`: 8586 against 8609 | partly (its one cause fixed; awaiting: 4.3 with `hod-e98267c0` at `K`, the libc++ lane) |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | native runner, run 43465227200, `a67370e`: hod strict 8,749, `llc` 8,750; one difference, `simd_unary.pass.cpp` (`x64:vector-wide-element`, built since WP-161). Under qemu: hybrid only, 8,607 against 8,608 | partly (awaiting: 4.4 with `hod-e98267c0`, executed shards 1 to 4 merged, the libc++ lane) |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | 2026-11-14: control 8606 pass; strict hod `3b14f8f`, kiln `21b9110a4`: 8605, 7224 of 7226 modules; the one difference `PR28469_undefined_behavior_segfault.sh.cpp` (`debug:dbg-value-expr`, built since WP-225); the other refusal is kiln's (fails in the control too) | partly (awaiting: 4.6 with `hod-e98267c0` at `K`) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | hod `3b14f8f`, kiln `21b9110a4`: compile-only census 7224 of 7226 modules; executed: shard 1 of 20, 0 differences | partly (one shard of 20 executed; awaiting: 4.7 with `hod-e98267c0`, the whole suite) |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 43465227200 at `a67370e`: one difference per runner (both causes fixed since), so `summary` failed | re-measure (awaiting: a `sweep.yml` run of `e98267c0` with kiln `8f93304ee`, strict) |

## Real projects (L9)

The 1.0 set is the projects the lanes have measured up to the
candidate (`docs/RELEASE.md` section 5, which has the list and the rule:
each row names its hod and kiln commits, a row measured before the
candidate is marked so, the candidate re-runs as many strict rows as
fit in the order LLVM and Clang, DuckDB, gRPC, Boost, abseil, then the
rest, and reuses a control when kiln and the revision are unchanged).

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row of the 1.0 set: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | 44 rows (22 projects; gRPC `-O0 -g` running), every one measured before the candidate (hod `77f1f69`, `7b127f48`, `3b14f8ff`, `77873543`, `46076a33`, `56a4695f`; kiln `21caaf874`, `21b9110a4`, `8f93304ee`). Open: zlib (1 object not offered by kiln, `coverage`); DuckDB `-O0` strict stopped at `alwaysinline:stacksave-escapes` at `3b14f8ff` (built since WP-237); DuckDB `-O0 -g` hybrid failed `test_random_uuid.test` with an allocation failure twice while strict equals the control (under investigation); mold's kiln is not in its reports | partly (awaiting: the native lanes' strict re-runs at `e98267c0` in section 5's order, then `oss.md` regenerated with `--candidate e98267c0`) |
| the lane, x86-64 (`oss-x64.md`) | the same for x86-64 | new page: 34 rows (18 projects; re2 and Boost `-O0 -g` running), cross-built, tests under qemu, every one measured before the candidate (hod `3b14f8ff`, `686ff25b`, `ee8e0e7b`, `56a4695f`, `9e25d3f1`). Open: Boost's strict build stopped at `asm:section` at `56a4695f` (built since WP-255; to re-measure) | partly (awaiting: the x86-64 lanes' strict re-runs at `e98267c0`, then `oss-x64.md` regenerated with `--candidate e98267c0`) |
| hosted (`oss.yml`) | both runners natively | no result recorded | missing (awaiting: an `oss.yml` run of `e98267c0`) |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | kiln `21caaf874` (native) and `21b9110a4` (x86-64, hod `3b14f8ff`, WP-222): LLVM 19.1.7 native 2778 of 2779 TUs (1 not offered), check-llvm at parity; Clang 3621 of 3623, 12460 lit tests at parity; LLVM for x86-64 2795 of 2795 C/C++ TUs, 0 declined | partly (awaiting: the LLVM lane's strict build and tests with `hod-e98267c0` at kiln `8f93304ee`, both trees; first in the candidate's order) |
| the 1.0 set (ROADMAP M6) | a row on a page for each project of the set, both flavours where measured | the set is named (`docs/RELEASE.md` section 5): 22 projects on AArch64, 18 on x86-64, LLVM and Clang on both; the other rows of `scripts/oss/projects.toml` and of kiln's `projects.toml` (59 rows at `8f93304ee`) come after 1.0 | partly (awaiting: the lanes' rows above; Boost and DuckDB to re-measure) |

## Random programs (L10)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | the first full run of the budget (WP-241) on a frozen `df415453`: 26,250 seeds (100001 to 126250), 4 h 44 min, 0 refused, 0 errors, 49 findings, **none hod's wrong code**: three classes, each now set apart by its own evidence (`ub-float-cast`, a sanitizer's report; `selector-split`, `llc`'s own configurations differing; `llc19-int128-last-register`, gcc and `llc` 22 agreeing with hod, WP-249: its 21 findings re-run came out set apart) | met at `df415453` (awaiting: the budget's run with `hod-e98267c0` from seed 200001, being measured at `e98267c0` since 2026-11-15 14:55 EST; the earlier run with `hod-02fe2d02` is the previous candidate's and counts for nothing here) |

## Status pages and release steps

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs`; `classify_support_doc` fails when it differs, so every gated commit has it current; the full gate's `test` step on `014a64f8` (`C`'s code) passed, and `classify_support_doc` passed again on this branch after `e98267c0` was merged into it (WP-257b) | met at `C`'s code (`014a64f8`) |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated for kiln 1.2.0 `8f93304ee` (they change only with the corpus, that is, with the pin) | met at kiln `8f93304ee` |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | being measured at `e98267c0` (WP-257b, `corpus reasons --llc -j 3 --hod hod-e98267c0` over the corpora of kiln `K` = `8f93304ee`, after the hybrid verify above). The generated pages carry no commit line of their own; this row is their provenance. At the previous candidate `02fe2d02` (WP-257, `hod-02fe2d02`): AArch64 every module compiled (3,487 and 3,445, LLVM 22 and 19); x86-64 every module but `cc-run.cpp`'s (`cc:x86_regcallcc`), "What the built modules do" 13,000 and 12,848 PASS, the 4 FAIL of each being `builtins/strncmp.c`, UNDEFINED | being measured at `e98267c0` |
| the other status pages | each at the candidate | at `e98267c0` (WP-257b): `debug.md`, `debug-x64.md` ("At the candidate"); `reasons.md`, `reasons-x64.md` being measured (the row above). Independent of hod: `corpus.md`, `corpus-x64-census.md` (generated, kiln `8f93304ee` per group), `corpus-x64.md` (the capture's record; says which recapture the candidate used), `linkers.md` (the linkers' versions, as in the header), `libcxx-baseline.md`, `libcxx-baseline-x64.md` (kiln + `llc` only; still kiln `bd60e1829`, awaiting 4.1, 4.2 at `K`). Not a condition: `perf.md` (generated 2026-11-13, left as measured). Fed by the lanes, still of earlier commits (section "Pages fed by the lanes" below): `oss.md`, `oss-x64.md`, `oss-llvm.md`, `libcxx-hod.md`, `libcxx-hod-x64.md`, `libcxx-hod-debug.md`, `libcxx-hod-debug-x64.md`, `fuzz.md` | partly (awaiting: the lanes' pages) |
| version bump | the workspace version moved, on the owner's word | `Cargo.toml` `[workspace.package]` `version = "0.0.0"`; no tag | missing (owner's word) |
| tag | made on the owner's word | none | missing (owner's word) |

## Pages fed by the lanes

Each is rewritten from its lane's results with `hod-e98267c0` at kiln
`8f93304ee` (`docs/RELEASE.md` sections 4 to 7); until then it names the
commits it was measured at. Results of the previous candidate
`02fe2d02` are not carried over as the candidate's.

| page | regenerated by | waits for |
|---|---|---|
| `oss.md` | `source scripts/env.sh && B=$(dirname "$CARGO_TARGET_DIR") && python3 -I scripts/oss/oss-report.py status --root $B/oss --root $B/oss3 --root $B/oss4 --provenance scripts/oss/provenance-aarch64.tsv --candidate e98267c0 --out docs/status/oss.md` | the native lanes' strict re-runs at `e98267c0` (5.1), their provenance rows |
| `oss-x64.md` | the same with `--target x86_64 status --root $B/oss --root $B/oss2 --root $B/oss5 --provenance scripts/oss/provenance-x86_64.tsv --candidate e98267c0 --out docs/status/oss-x64.md` | the x86-64 lanes' strict re-runs at `e98267c0` (5.2) |
| `oss-llvm.md` | by hand from the LLVM lane's records (5.4) | LLVM and Clang strict builds and tests with `hod-e98267c0` at `K`, both trees |
| `libcxx-hod.md` | 4.3, then `python3 -I scripts/libcxx-hod-report.py <dir>/strict <dir>/strict.tsv <baseline.tsv>`; the native section from 4.5 | 4.1 at `K` and 4.3 with a shim of `hod-e98267c0`; a `sweep.yml` run of `e98267c0` |
| `libcxx-hod-x64.md` | 4.4 (`scripts/libcxx-hod-x64.sh strict <dir> --shard K/4`, then `merge`), the report against 4.2; 4.5 | 4.2 at `K`, the four shards |
| `libcxx-hod-debug.md` | 4.6 (`scripts/libcxx-hod-debug.sh control`, then `strict`, the report) | both runs at `K` with `hod-e98267c0` |
| `libcxx-hod-debug-x64.md` | 4.7 (`HOD_DEBUG_TARGET=x86_64`, shards, `merge`) | the whole suite executed |
| `fuzz.md` | section 6: `cargo xtask fuzz --budget --seed 200001 -j 4 --hod hod-e98267c0 --out <dir>`, the page rewritten from its summary | the budget run started 2026-11-15 14:55 EST (`.build/candidate/fuzz-e98267c0`) |

## Summary

SUMMARY-PENDING (WP-257b rewrites this paragraph when the L6 and reasons
runs with `hod-e98267c0` have ended).
