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
| written | 2026-11-15, WP-256 (first written 2026-11-14 against `56a4695f`, WP-243) |
| hod `main` | `9e25d3f1` (wave 26); waves 27 and 28 (to `c8880ccc`: WP-249 to WP-255) stacked on it |
| kiln pin | `8f93304ee` (kiln 1.2.0) in all three places (the submodule, `.github/kiln-revision`, the corpora's kiln groups); it does not move before the release |
| candidate | none chosen yet; the freeze is 2026-11-15 about 12:00 EST; no version bump, no tag (version `0.0.0`) |

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
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the landing gate of wave 26 at `9e25d3f1`, 2026-11-15: `ci: ok`, exit 0, 354 test binaries; linkers bfd, lld, mold (and wild); kiln `8f93304ee`. The log is the supervisor's, not a page | met at `9e25d3f1` (re-run at the candidate) |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: the quick gate's steps, `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with 22 and 19, the whole-corpus `roundtrip`, `debug-x86_64`, `debug-aarch64`, `corpus-aarch64`, `corpus-x86_64` | its steps exist (WP-251, in wave 28); the two corpus steps run alone at `d8d0b53f`, exit 0 (`corpus-aarch64` 45:21, `corpus-x86_64` 56:06, `spec/landed/harness/WP-251.md`); no whole run recorded | partly |
| gate on the x86-64 host | the gate's steps natively on x86-64 | the hosted x86-64 lane (next row): the quick gate's steps without `roundtrip`; no x86-64 development host has a corpus, so no full gate runs there | partly |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 43688918672 of `9e25d3f1`: every job of both lanes green; `test x64` 17 min on a Xeon 6975P-C, 19 AVX-512 programs natively, `knl`'s machine skipped. With `HOD_CI_SDE=1` (set since) its re-run took 20 min 40 s on an EPYC 9V74: 17 natively, the 2 AVX512-FP16 programs under SDE, `knl` built and compared but not executed. A runner with no AVX-512 is not measured yet | met at `9e25d3f1` (re-run at the candidate) |
| linkers bfd, lld, mold | every linking test and `irdiff` program with all three, required | the gates above require and name `bfd lld mold`; versions in `debug.md`: GNU ld 2.44, lld 22.1.8, mold 2.37.1 | met at `9e25d3f1` (re-run at the candidate) |
| linker wild | none: wild is optional | wild 0.11.0 (WP-239) is in the matrix where installed (`linkers::WILD_REQUIRED` false); its cases are listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`, the debugger layer's XLINK, WP-248); hod's code generators and object writer take no workaround for it | not a condition (decided 2026-11-15: GNU ld, lld and mold are the required linkers) |

## Kiln's corpus (L6) and debug information (L7)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | `corpus-aarch64` at `d8d0b53f`, kiln `8f93304ee` (WP-251): LLVM 22 13,012 PASS of 13,012, hod built 3,253 of 3,253; LLVM 19 12,860 of 12,860, 3,215 of 3,215; four linkers; `docs/status/corpus-expected.tsv` has no AArch64 row (win64cc and the exclusive intrinsics have landed) | met at `d8d0b53f` (re-run at the candidate: a full-gate step) |
| L6 strict, x86-64 | the same under qemu | `corpus-x86_64` at `d8d0b53f` (WP-251): LLVM 22 12,992 PASS, 8 UNSUPPORTED, 4 SKIP, 0 FAIL, hod built 3,249 of 3,251; LLVM 19 12,840 PASS, 0 FAIL, 3,211 of 3,213. Since WP-254 (x86-64 `asm goto` with a template) and module assembly with its own sections (WP-255), ONE corpus test is not built by hod: `cc-run.cpp`, `cc:x86_regcallcc`, by design; `builtins/strncmp.c` is UNDEFINED (the test's own undefined behaviour) | met at `d8d0b53f` (re-run at the candidate, with the one expected row) |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | `e7e473af`, kiln `8f93304ee` (WP-244): 977 results, 957 PASS, 16 XFAIL, 3 ERROR (wild with gdb), 0 FAIL, 0 XPASS; `--verify` 18 of 18 corpus `-g` modules; L6 strict with `-g` 18 of 18; exit status 1 on one unclassified line pair (`declare_fragment.c:18`). In the full gate (`debug-aarch64`) | partly (the unclassified pair; re-measure) |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | `e7e473af` (WP-244): 921 results, 880 PASS, 8 XFAIL, 33 SKIP, 0 FAIL; `--verify` and L6 strict with `-g` 18 of 18; exit 1 on the same pair. In the full gate (`debug-x86_64`) | partly (as AArch64) |

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | 2026-11-11, kiln 1.0.1 `bd60e1829`: 9247 tests, 8609 pass, 4 fail, 47 xfail, 1 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (kiln is at `8f93304ee` since) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | 2026-11-12/13, kiln `bd60e1829`: 9247, 8608 pass, 5 fail, 46 xfail, 2 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | native runner, run 43465227200, `a67370e`, kiln `bd60e1829`: hod strict 8,748 pass, `llc` 8,749; one difference, `alg.find/find.pass.cpp` (`abi:vector-element`, built since WP-167). Development host (WP-102) at `6e6bf9c`: 8586 against 8609 | partly (its one cause fixed; re-measure: WP-233 in progress) |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | native runner, run 43465227200, `a67370e`: hod strict 8,749, `llc` 8,750; one difference, `simd_unary.pass.cpp` (`x64:vector-wide-element`, built since WP-161). Under qemu: hybrid only, 8,607 against 8,608 | partly (WP-233 in progress: executed shards) |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | 2026-11-14: control 8606 pass; strict hod `3b14f8f`, kiln `21b9110a4`: 8605, 7224 of 7226 modules; the one difference `PR28469_undefined_behavior_segfault.sh.cpp` (`debug:dbg-value-expr`, built since WP-225); the other refusal is kiln's (fails in the control too) | partly (re-measure) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | hod `3b14f8f`, kiln `21b9110a4`: compile-only census 7224 of 7226 modules; executed: shard 1 of 20, 0 differences | partly (one shard of 20 executed) |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 43465227200 at `a67370e`: one difference per runner (both causes fixed since), so `summary` failed | re-measure |

## Real projects (L9)

The 1.0 set is the projects the lanes have measured up to the
candidate (`docs/RELEASE.md` section 5, which has the list and the rule:
each row names its hod and kiln commits, a row measured before the
candidate is marked so, the candidate re-runs as many strict rows as
fit in the order LLVM and Clang, DuckDB, gRPC, Boost, abseil, then the
rest, and reuses a control when kiln and the revision are unchanged).

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row of the 1.0 set: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | 44 rows (22 projects; gRPC `-O0 -g` running), every one measured before the candidate (hod `77f1f69`, `7b127f48`, `3b14f8ff`, `77873543`, `46076a33`, `56a4695f`; kiln `21caaf874`, `21b9110a4`, `8f93304ee`). Open: zlib (1 object not offered by kiln, `coverage`); DuckDB `-O0` strict stopped at `alwaysinline:stacksave-escapes` at `3b14f8ff` (built since WP-237); DuckDB `-O0 -g` hybrid failed `test_random_uuid.test` with an allocation failure twice while strict equals the control (under investigation); mold's kiln is not in its reports | partly |
| the lane, x86-64 (`oss-x64.md`) | the same for x86-64 | new page: 34 rows (18 projects; re2 and Boost `-O0 -g` running), cross-built, tests under qemu, every one measured before the candidate (hod `3b14f8ff`, `686ff25b`, `ee8e0e7b`, `56a4695f`, `9e25d3f1`). Open: Boost's strict build stopped at `asm:section` at `56a4695f` (built since WP-255; to re-measure) | partly |
| hosted (`oss.yml`) | both runners natively | no result recorded | missing |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | kiln `21caaf874` (native) and `21b9110a4` (x86-64, hod `3b14f8ff`, WP-222): LLVM 19.1.7 native 2778 of 2779 TUs (1 not offered), check-llvm at parity; Clang 3621 of 3623, 12460 lit tests at parity; LLVM for x86-64 2795 of 2795 C/C++ TUs, 0 declined | partly (re-measure at kiln `8f93304ee`: WP-222's native stage in progress; first in the candidate's order) |
| the 1.0 set (ROADMAP M6) | a row on a page for each project of the set, both flavours where measured | the set is named (`docs/RELEASE.md` section 5): 22 projects on AArch64, 18 on x86-64, LLVM and Clang on both; the other rows of `scripts/oss/projects.toml` and of kiln's `projects.toml` (59 rows at `8f93304ee`) come after 1.0 | partly (rows running; Boost and DuckDB to re-measure) |

## Random programs (L10)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | the first full run of the budget (WP-241) on a frozen `df415453`: 26,250 seeds (100001 to 126250), 4 h 44 min, 0 refused, 0 errors, 49 findings, **none hod's wrong code**: three classes, each now set apart by its own evidence (`ub-float-cast`, a sanitizer's report; `selector-split`, `llc`'s own configurations differing; `llc19-int128-last-register`, gcc and `llc` 22 agreeing with hod, WP-249: its 21 findings re-run came out set apart) | met at `df415453` (re-run at the candidate from seed 200001) |

## Status pages and release steps

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs`; `classify_support_doc` fails when it differs, so every gated commit has it current | met at `9e25d3f1` |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated for kiln 1.2.0 `8f93304ee` (they change only with the corpus, that is, with the pin) | met at kiln `8f93304ee` |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | hod `9f410277` (wave 16): stale, they still name refusals built since | re-measure |
| the other status pages | each at the candidate | as in the rows above; `oss.md` and `oss-x64.md` regenerable by one command each (`docs/RELEASE.md` section 7); `perf.md` (2026-11-13) is not a condition | re-measure |
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
