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
| written | 2026-11-15 06:15 EST, WP-257 (first written 2026-11-14 against `56a4695f`, WP-243; then WP-256) |
| candidate `C` | hod `02fe2d02` (`main`, frozen 2026-11-15 06:07 EST); no version bump, no tag (version `0.0.0`) |
| kiln `K` | `8f93304ee` (kiln 1.2.0) in all three places (the submodule, `.github/kiln-revision`, the corpora's kiln groups); it does not move before the release |
| frozen binary | `cargo build --release -p hod_cli` of `C`, copied out of the build directory: `hod-02fe2d02`, sha256 `5c1e181de7f62e3a644a11b536495429674c76bb0be6203c5bf85a8c19c3633d`, `hod version 0.0.0` |
| full gate | `HOD_REQUIRE_TOOLS=1 cargo xtask ci` on `b6e9b8c2`, 2026-11-15 04:08 to 06:05 EST: `ci: ok`, exit 0, 1:56:03 (fmt 7.8 s, clippy 51.5 s, test 45:32, irdiff 1:57, roundtrip 13:07, debug-x86_64 2:39, debug-aarch64 3:26, corpus-aarch64 20:41, corpus-x86_64 27:37), with the workspace's development hod. `C` is `b6e9b8c2` merged into `main` (`7a98bffb`, the same tree) plus rows of `docs/BACKLOG.md` only: the gate ran on `C`'s code, not on `C` itself nor with the frozen binary. The log is the supervisor's, not a page |
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
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the full gate on `b6e9b8c2` (`C`'s code, above) contains every step of the quick gate (fmt, clippy, test, irdiff with LLVM 22): each passed, kiln `8f93304ee`, linkers bfd, lld, mold and wild. Before it: the landing gate of wave 26 at `9e25d3f1` | met at `C`'s code (`b6e9b8c2`, the full gate's run) |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: the quick gate's steps, `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with 22 and 19, the whole-corpus `roundtrip`, `debug-x86_64`, `debug-aarch64`, `corpus-aarch64`, `corpus-x86_64` | the first whole run: `b6e9b8c2`, 2026-11-15 04:08 to 06:05 EST, `ci: ok`, exit 0, 1:56:03 (step times in the header); kiln `8f93304ee`; `roundtrip` 6468 entries as expected, 0 worse, 0 stale | met at `C`'s code (`b6e9b8c2`; `C` differs only in `docs/BACKLOG.md`) |
| gate on the x86-64 host | the gate's steps natively on x86-64 | the hosted x86-64 lane (next row): the quick gate's steps without `roundtrip`; no x86-64 development host has a corpus, so no full gate runs there | partly (awaiting: the hosted lanes' run of `02fe2d02`) |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 43688918672 of `9e25d3f1`: every job of both lanes green; `test x64` 17 min on a Xeon 6975P-C, 19 AVX-512 programs natively, `knl`'s machine skipped. With `HOD_CI_SDE=1` (set since) its re-run took 20 min 40 s on an EPYC 9V74: 17 natively, the 2 AVX512-FP16 programs under SDE, `knl` built and compared but not executed. A runner with no AVX-512 is not measured yet | met at `9e25d3f1` (awaiting: one run of `02fe2d02` with `check` and `check-x86_64` green, its run id and the AVX-512 block) |
| linkers bfd, lld, mold | every linking test and `irdiff` program with all three, required | the full gate on `b6e9b8c2` (`C`'s code) under `HOD_REQUIRE_TOOLS=1`: each of its nine `irdiff`/`corpus verify` runs names `linkers bfd lld mold wild`, both debug steps name the four, and the log has no test skipped for a missing linker; versions GNU ld 2.44, lld 22.1.8, mold 2.37.1 (header) | met at `C`'s code (`b6e9b8c2`) |
| linker wild | none: wild is optional | wild 0.11.0 (WP-239; linked in the full gate's runs above) is in the matrix where installed (`linkers::WILD_REQUIRED` false); its cases are listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`, the debugger layer's XLINK, WP-248); hod's code generators and object writer take no workaround for it | not a condition (decided 2026-11-15: GNU ld, lld and mold are the required linkers) |

## Kiln's corpus (L6) and debug information (L7)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | the full gate's `corpus-aarch64` on `b6e9b8c2` (`C`'s code, the workspace's development hod), kiln `8f93304ee`: LLVM 22 13,012 PASS of 13,012, hod built 3,253 of 3,253; LLVM 19 12,860 of 12,860, 3,215 of 3,215; four linkers; `docs/status/corpus-expected.tsv` has no AArch64 row, 0 list problems. Before: `d8d0b53f` (WP-251), the same counts | met at `C`'s code (`b6e9b8c2`); awaiting: 3.1 with the frozen binary (`--hod hod-02fe2d02`) |
| L6 strict, x86-64 | the same under qemu | the full gate's `corpus-x86_64` on `b6e9b8c2` (`C`'s code, the development hod), kiln `8f93304ee`: LLVM 22 13,004 results, 12,996 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, hod built 3,250 of 3,251; LLVM 19 12,852 results, 12,844 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, 3,212 of 3,213; the one test hod does not build is `cc-run.cpp` (`cc:x86_regcallcc`, by design: EXPECTED), `builtins/strncmp.c` UNDEFINED (the test's own undefined behaviour), 0 list problems. Before: `d8d0b53f` (WP-251) | met at `C`'s code (`b6e9b8c2`); awaiting: 3.2 with the frozen binary |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | the full gate's `debug-aarch64` on `b6e9b8c2` (`C`'s code, the development hod), kiln `8f93304ee`: 993 results, 973 PASS, 16 XFAIL, 3 XLINK (wild with gdb, WP-248), 1 SKIP, 0 FAIL, 0 XPASS, 1888 sessions; lines: 0 unclassified (the `declare_fragment.c:18` pair of `e7e473af` is set apart as `selector-split` since WP-250; 2 such pairs); `debug verify` 18 of 18 `-g` modules clean, 0 declined; L6 strict with `-g` in `corpus-aarch64`. Not in the gate: `debug kiln`, `debug trees`. **With the frozen binary** (WP-257, 2026-11-15, `docs/status/debug.md` "At the candidate"): `debug run` the same counts, exit 0; `debug verify` 18 of 18 clean; `debug kiln` 17 sessions, `llc` 15 pass, hod 15 pass, none passing with `llc` only, exit 0; the 2 others (`class_decl_two_units.cpp`) fail with both writers because the harness hands kiln++ the unsubstituted `%{test_dir}` path, so that test is not measured; `debug trees` 8 same, 8 permitted, 2 differ (`lifetime-markers-O2`, unchanged from WP-244's record), exit 1 | partly: met at `02fe2d02` for sessions, verify and lines; open: kiln's two-unit test unmeasured (harness), the 2 `debug trees` differences of an `-O2` unit (known, WP-244); L6 with `-g` in 3.1 |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | the full gate's `debug-x86_64` on `b6e9b8c2`: 937 results, 896 PASS, 8 XFAIL, 33 SKIP (the stub's), 0 FAIL, 1712 sessions; lines 0 unclassified; `debug verify --target x86_64` 18 of 18 clean, 0 declined. **With the frozen binary** (WP-257, `docs/status/debug-x64.md` "At the candidate"): the same `debug run` counts, exit 0; `debug verify` 18 of 18 clean; `debug trees` 16 same, 2 differ (`lifetime-markers-O2`, unchanged from WP-244's record), exit 1 | partly: met at `02fe2d02` for sessions, verify and lines; open: the 2 `debug trees` differences (known, WP-244); L6 with `-g` in 3.2 |

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | 2026-11-11, kiln 1.0.1 `bd60e1829`: 9247 tests, 8609 pass, 4 fail, 47 xfail, 1 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (kiln is at `8f93304ee` since; awaiting: 4.1 at `K`) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | 2026-11-12/13, kiln `bd60e1829`: 9247, 8608 pass, 5 fail, 46 xfail, 2 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (awaiting: 4.2 at `K`) |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | native runner, run 43465227200, `a67370e`, kiln `bd60e1829`: hod strict 8,748 pass, `llc` 8,749; one difference, `alg.find/find.pass.cpp` (`abi:vector-element`, built since WP-167). Development host (WP-102) at `6e6bf9c`: 8586 against 8609 | partly (its one cause fixed; awaiting: 4.3 with `hod-02fe2d02` at `K`, the libc++ lane) |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | native runner, run 43465227200, `a67370e`: hod strict 8,749, `llc` 8,750; one difference, `simd_unary.pass.cpp` (`x64:vector-wide-element`, built since WP-161). Under qemu: hybrid only, 8,607 against 8,608 | partly (awaiting: 4.4 with `hod-02fe2d02`, executed shards 1 to 4 merged, the libc++ lane) |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | 2026-11-14: control 8606 pass; strict hod `3b14f8f`, kiln `21b9110a4`: 8605, 7224 of 7226 modules; the one difference `PR28469_undefined_behavior_segfault.sh.cpp` (`debug:dbg-value-expr`, built since WP-225); the other refusal is kiln's (fails in the control too) | partly (awaiting: 4.6 with `hod-02fe2d02` at `K`) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | hod `3b14f8f`, kiln `21b9110a4`: compile-only census 7224 of 7226 modules; executed: shard 1 of 20, 0 differences | partly (one shard of 20 executed; awaiting: 4.7 with `hod-02fe2d02`, the whole suite) |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 43465227200 at `a67370e`: one difference per runner (both causes fixed since), so `summary` failed | re-measure (awaiting: a `sweep.yml` run of `02fe2d02` with kiln `8f93304ee`, strict) |

## Real projects (L9)

The 1.0 set is the projects the lanes have measured up to the
candidate (`docs/RELEASE.md` section 5, which has the list and the rule:
each row names its hod and kiln commits, a row measured before the
candidate is marked so, the candidate re-runs as many strict rows as
fit in the order LLVM and Clang, DuckDB, gRPC, Boost, abseil, then the
rest, and reuses a control when kiln and the revision are unchanged).

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row of the 1.0 set: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | 44 rows (22 projects; gRPC `-O0 -g` running), every one measured before the candidate (hod `77f1f69`, `7b127f48`, `3b14f8ff`, `77873543`, `46076a33`, `56a4695f`; kiln `21caaf874`, `21b9110a4`, `8f93304ee`). Open: zlib (1 object not offered by kiln, `coverage`); DuckDB `-O0` strict stopped at `alwaysinline:stacksave-escapes` at `3b14f8ff` (built since WP-237); DuckDB `-O0 -g` hybrid failed `test_random_uuid.test` with an allocation failure twice while strict equals the control (under investigation); mold's kiln is not in its reports | partly (awaiting: the native lanes' strict re-runs at `02fe2d02` in section 5's order, then `oss.md` regenerated with `--candidate 02fe2d02`) |
| the lane, x86-64 (`oss-x64.md`) | the same for x86-64 | new page: 34 rows (18 projects; re2 and Boost `-O0 -g` running), cross-built, tests under qemu, every one measured before the candidate (hod `3b14f8ff`, `686ff25b`, `ee8e0e7b`, `56a4695f`, `9e25d3f1`). Open: Boost's strict build stopped at `asm:section` at `56a4695f` (built since WP-255; to re-measure) | partly (awaiting: the x86-64 lanes' strict re-runs at `02fe2d02`, then `oss-x64.md` regenerated with `--candidate 02fe2d02`) |
| hosted (`oss.yml`) | both runners natively | no result recorded | missing (awaiting: an `oss.yml` run of `02fe2d02`) |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | kiln `21caaf874` (native) and `21b9110a4` (x86-64, hod `3b14f8ff`, WP-222): LLVM 19.1.7 native 2778 of 2779 TUs (1 not offered), check-llvm at parity; Clang 3621 of 3623, 12460 lit tests at parity; LLVM for x86-64 2795 of 2795 C/C++ TUs, 0 declined | partly (awaiting: the LLVM lane's strict build and tests with `hod-02fe2d02` at kiln `8f93304ee`, both trees; first in the candidate's order) |
| the 1.0 set (ROADMAP M6) | a row on a page for each project of the set, both flavours where measured | the set is named (`docs/RELEASE.md` section 5): 22 projects on AArch64, 18 on x86-64, LLVM and Clang on both; the other rows of `scripts/oss/projects.toml` and of kiln's `projects.toml` (59 rows at `8f93304ee`) come after 1.0 | partly (awaiting: the lanes' rows above; Boost and DuckDB to re-measure) |

## Random programs (L10)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | the first full run of the budget (WP-241) on a frozen `df415453`: 26,250 seeds (100001 to 126250), 4 h 44 min, 0 refused, 0 errors, 49 findings, **none hod's wrong code**: three classes, each now set apart by its own evidence (`ub-float-cast`, a sanitizer's report; `selector-split`, `llc`'s own configurations differing; `llc19-int128-last-register`, gcc and `llc` 22 agreeing with hod, WP-249: its 21 findings re-run came out set apart) | met at `df415453` (awaiting: the budget's run with `hod-02fe2d02` from seed 200001, running since the freeze) |

## Status pages and release steps

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs`; `classify_support_doc` fails when it differs, so every gated commit has it current; the full gate's `test` step on `b6e9b8c2` (`C`'s code) passed | met at `C`'s code (`b6e9b8c2`) |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated for kiln 1.2.0 `8f93304ee` (they change only with the corpus, that is, with the pin) | met at kiln `8f93304ee` |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | hod `9f410277` (wave 16): stale, they still name refusals built since | re-measure (awaiting: `corpus reasons` with `hod-02fe2d02`, both targets: WP-257) |
| the other status pages | each at the candidate | as in the rows above; `oss.md` and `oss-x64.md` regenerable by one command each (`docs/RELEASE.md` section 7); `perf.md` (2026-11-13) is not a condition | re-measure (awaiting: the lanes' pages above) |
| version bump | the workspace version moved, on the owner's word | `Cargo.toml` `[workspace.package]` `version = "0.0.0"`; no tag | missing (owner's word) |
| tag | made on the owner's word | none | missing (owner's word) |

## Summary

29 rows, 28 of them conditions (wild is listed and is not one): 8 met
(quick gate, hosted CI and linkers bfd/lld/mold at `9e25d3f1`; L6 strict
on both targets at `d8d0b53f`; the fuzz budget at `df415453`; the support
matrix; the corpus pages at kiln `8f93304ee`; each to re-run at the
candidate where it says so), 5 re-measure, 12 partly, 3 missing (2 of them
the owner's: version and tag). In the order they block 1.0: no whole full
gate is recorded and L7 exits 1 on one unclassified line pair; libc++
strict on both targets is one test short at `a67370e` (both causes fixed
since; WP-233 re-measures); real projects: every row predates the
candidate, Boost's x86-64 strict build and DuckDB's `-O0` strict build to
re-measure, DuckDB's `-O0 -g` hybrid test under investigation, zlib's one
object outside hod, no hosted result, LLVM and Clang to re-measure at the
pin.
