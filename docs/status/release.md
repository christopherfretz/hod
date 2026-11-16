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
| written | 2026-11-15 20:17 EST, WP-257d for the candidate `cd6ef6bc` (first written 2026-11-14 against `56a4695f`, WP-243; then WP-256; WP-257 for the candidate `02fe2d02`; WP-257b for the candidate `e98267c0`; WP-257c, phase A for `cd6ef6bc`) |
| candidate `C` | hod `cd6ef6bc` (frozen 2026-11-15 15:55 EST; `main` is moved to exactly this commit when its full gate is green). The tagged commit is `cd6ef6bc` plus status pages, records, harness-side report scripts and, as its last commit, the version number: no change to any crate's code besides the version. It replaces the candidate `e98267c0` of 14:50 EST, which replaced `02fe2d02` of 06:07 EST. Since `02fe2d02`, hod's code changed by WP-261 (the dead arm of an `llvm.is.constant`/`llvm.objectsize` branch is not compiled), WP-262 (a coroutine suspend in an unreachable block), WP-267 (`llvm.x86.tpause`, `umwait`, `umonitor`) and WP-269 (the constant fold never refuses a function: where WP-261 refused one with an `intrinsic:const-fold-*` name, hod keeps the instruction as written; `spec/codegen.md` 5.6.8), the harness by WP-259, WP-260, WP-263, WP-266 and WP-268. `cd6ef6bc` is `e98267c0` plus WP-269 and one commit to `docs/BACKLOG.md` |
| kiln `K` | `8f93304ee` (kiln 1.2.0) in all three places (the submodule, `.github/kiln-revision`, the corpora's kiln groups); it does not move before the release |
| frozen binary | `cargo build --release -p hod_cli` of the tree of `cd6ef6bc` (15:54 to 15:55 EST), copied out of the build directory: `hod-cd6ef6bc`, sha256 `80138ec1411506d5b4048f1eac49e8b377f9eeedb21e70ae43e3c81ab094929c`, `hod version 0.0.0` |
| full gate | `HOD_REQUIRE_TOOLS=1 cargo xtask ci` on exactly `cd6ef6bc`, with the workspace's development hod, 2026-11-15 15:54 to 19:12 EST: `ci: ok`, exit 0, 3:16:30 (fmt 9.2 s, clippy 39.1 s, test 1:03:57, irdiff 4:22, roundtrip 30:11, debug-x86_64 4:52, debug-aarch64 6:34, corpus-aarch64 38:32, corpus-x86_64 47:10). The log is the supervisor's, not a page |
| quick gate | no separate quick gate on `cd6ef6bc`: the full gate above contains every step of the quick gate |
| gate on the tag | the tagged commit's own full gate (`HOD_REQUIRE_TOOLS=1 cargo xtask ci` on exactly that commit) is run before the tag is made, and the tag exists only if it was green |
| tools | Debian 13.7, AArch64 host; LLVM 22.1.8 and 19.1.7; GNU ld 2.44 (`ld.bfd` and `x86_64-linux-gnu-ld.bfd`), lld 22.1.8, mold 2.37.1, wild 0.11.0 (optional); gdb 16.3 (and `gdb-multiarch` 16.3), lldb 19.1.7; qemu-x86_64 10.0.13; rustc 1.95.0 |

States: **met at `<commit>`** (measured at that commit and nothing since
can change it; re-run at the candidate where the row says so); **carried over from `e98267c0`**
(a real-project row measured at the previous candidate with zero
refusals; it stands for `cd6ef6bc`, section "Pages fed by the lanes");
**met in the tagged commit** (the version bump, the tag and the tag's own
gate, true of the commit that carries the tag); **re-measure** (met at an older commit or kiln: measure again at the
candidate); **partly** (part of the condition is measured or met);
**missing** (no evidence); **not a condition** (listed for the record).
Every commit named is hod's unless it says kiln. A hod commit in the pages
is written with 7 or 8 hex digits as the page has it.

## Gates and lanes

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the full gate on `cd6ef6bc` (next row), which contains every step of the quick gate: each passed, kiln `8f93304ee`, linkers bfd, lld, mold and wild. The tagged commit's own full gate is run before the tag (header) | met at `cd6ef6bc` |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: the quick gate's steps, `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with 22 and 19, the whole-corpus `roundtrip`, `debug-x86_64`, `debug-aarch64`, `corpus-aarch64`, `corpus-x86_64` | exactly `cd6ef6bc`, 2026-11-15 15:54 to 19:12 EST, `ci: ok`, exit 0, 3:16:30 (step times in the header); kiln `8f93304ee`; `irdiff` AArch64 strict 364 PASS of 387 with LLVM 22 and 19 (4 UNSUPPORTED, 19 SKIP, 0 FAIL), hybrid 368, x86-64 372 PASS of 393 with 22 and 19 (4 UNSUPPORTED, 17 SKIP, 0 FAIL); `roundtrip` 6468 entries, 6468 as expected, 0 worse, 0 stale. The tagged commit's own full gate is run before the tag, and the tag exists only if it was green (header) | met at `cd6ef6bc` |
| gate on the x86-64 host | the gate's steps natively on x86-64 | the hosted x86-64 lane of run 43778976284 of `cd6ef6bc` (next row): its eight `x86_64 /` jobs (lints, kiln, irdiff, test front, test a64, test harness, test rest, test x64) green natively on an x86-64 runner, that is the quick gate's steps without `roundtrip`; no x86-64 development host has a corpus, so no full gate runs there: the corpus and debugger steps for x86-64 run under qemu on the AArch64 host (the L6 and L7 x86-64 rows) | met at `cd6ef6bc` (the quick gate's steps natively; the corpus steps under qemu) |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 43778976284 of exactly `cd6ef6bc` (2026-11-15 19:12 to 19:35 EST): all 18 jobs green, `check` and `check-x86_64` among them; `x86_64 / test x64` 21 min 21 s on an AMD EPYC 9V74 with `HOD_CI_SDE=1` (Intel SDE 10.13.1, `-spr`): its AVX-512 block says 19 programs executed, 17 natively and the 2 AVX512-FP16 programs under SDE, 0 failed, 1 skipped (`abi zmm_grid 17 knl`: neither the EPYC nor SDE `-spr` has `avx512er`/`avx512pf`). A runner with no AVX-512 is not measured. Earlier: run 43688918672 of `9e25d3f1` and run 43754685470 of the previous candidate `e98267c0`, both green | met at `cd6ef6bc` |
| linkers bfd, lld, mold | every linking test and `irdiff` program with all three, required | the full gate on `cd6ef6bc` under `HOD_REQUIRE_TOOLS=1`: each of its nine `irdiff`/`corpus verify` runs names `linkers bfd lld mold wild`, and the log has no test skipped for a missing linker (its SKIPs are qemu's gdb stub and the UNDEFINED `strncmp.c`); versions GNU ld 2.44, lld 22.1.8, mold 2.37.1 (header) | met at `cd6ef6bc` |
| linker wild | none: wild is optional | wild 0.11.0 (WP-239; linked in the full gate's runs above) is in the matrix where installed (`linkers::WILD_REQUIRED` false); its cases are listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`, the debugger layer's XLINK, WP-248); hod's code generators and object writer take no workaround for it | not a condition (decided 2026-11-15: GNU ld, lld and mold are the required linkers) |

## Kiln's corpus (L6) and debug information (L7)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | **3.1 with the frozen binary `hod-cd6ef6bc`** (WP-257c, `corpus verify --strict -j 3`, 2026-11-15 18:06 to 18:57 EST, exit 0): LLVM 22 13,012 PASS of 13,012, hod built 3,253 of 3,253; LLVM 19 12,860 of 12,860, 3,215 of 3,215; 0 FAIL, 0 XPASS, 0 ERROR; 0 refused, 0 undefined, 0 list problems; and the full gate's `corpus-aarch64` on `cd6ef6bc` (the workspace's development hod, 38:32), kiln `8f93304ee`: LLVM 22 13,012 PASS of 13,012, hod built 3,253 of 3,253; LLVM 19 12,860 of 12,860, 3,215 of 3,215; four linkers; `docs/status/corpus-expected.tsv` has no AArch64 row, 0 list problems. The same counts at the earlier candidates `02fe2d02` (WP-257, `hod-02fe2d02`) and `e98267c0` (its code's full gate on `014a64f8`) | met at `cd6ef6bc` (the frozen binary and the gate's development hod) |
| L6 strict, x86-64 | the same under qemu | **3.2 with the frozen binary `hod-cd6ef6bc`** (WP-257c, `corpus verify --target x86_64 --strict -j 3`, 2026-11-15 18:57 to 20:02 EST, exit 0): LLVM 22 13,004 results, 12,996 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, 0 XPASS, 0 ERROR, hod built 3,250 of 3,251; LLVM 19 12,852 results, 12,844 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, 3,212 of 3,213; 1 refused test (`cc-run.cpp`, EXPECTED), 1 undefined (`strncmp.c`), 0 list problems. The hybrid run with the same binary (16:13 to 17:22 EST, exit 0): 13,000 PASS of 13,004 and 12,848 of 12,852, `cc-run.cpp` built by `llc`, the 4 SKIP of each `strncmp.c` (UNDEFINED). And the full gate's `corpus-x86_64` on `cd6ef6bc` (the development hod, 47:10), kiln `8f93304ee`: LLVM 22 13,004 results, 12,996 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, hod built 3,250 of 3,251; LLVM 19 12,852 results, 12,844 PASS, 4 UNSUPPORTED, 4 SKIP, 0 FAIL, 3,212 of 3,213; the one test hod does not build is `cc-run.cpp` (`cc:x86_regcallcc`, by design: EXPECTED), `builtins/strncmp.c` UNDEFINED (the test's own undefined behaviour), 0 list problems. The same strict counts at the earlier candidates `02fe2d02` (WP-257, `hod-02fe2d02`) and `e98267c0` (its code's full gate on `014a64f8`); at `02fe2d02` hybrid 13,000 PASS of 13,004 and 12,848 of 12,852, the rest `strncmp.c` | met at `cd6ef6bc` (the frozen binary and the gate's development hod, with the one expected row) |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | **with the frozen binary `hod-cd6ef6bc`** (WP-257c, 2026-11-15 16:00 to 16:13 EST, `docs/status/debug.md` "At the candidate"), kiln `8f93304ee`: `debug run` 993 results, 973 PASS, 16 XFAIL, 3 XLINK (wild with gdb, WP-248), 1 SKIP, 0 FAIL, 0 XPASS, 0 ERROR, 1888 sessions, exit 0; lines: 0 unclassified (2 `selector-split` pairs, set apart since WP-250); `debug verify` 18 of 18 `-g` modules clean, 0 declined; `debug kiln` 17 sessions, `llc` 17 pass, hod 17 pass, 15 with equal transcripts, exit 0 (the two-unit test measured since WP-259); `debug trees` 8 same (822 DIEs), 10 permitted, 0 differ, exit 0 (the `-O2` unit's 2 permitted since WP-260). The full gate's `debug-aarch64` on `cd6ef6bc` (the development hod, 6:34): the same `debug run` and `debug verify` counts, passed. L6 strict with `-g` is part of 3.1 (row above). The same counts at the previous candidate `e98267c0` (WP-257b) | met at `cd6ef6bc` (sessions, verify, lines, kiln's tests, trees, L6 with `-g` in 3.1) |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | **with the frozen binary `hod-cd6ef6bc`** (WP-257c, 16:07 to 16:13 EST, `docs/status/debug-x64.md` "At the candidate"): `debug run --target x86_64` 937 results, 896 PASS, 8 XFAIL, 33 SKIP (the stub's), 0 FAIL, 0 XPASS, 0 ERROR, 1712 sessions, exit 0; lines 0 unclassified; `debug verify --target x86_64` 18 of 18 clean, 0 declined; `debug trees` 16 same (1722 DIEs), 2 permitted, 0 differ, exit 0 (WP-260). The full gate's `debug-x86_64` on `cd6ef6bc` (4:52): the same `debug run` and `debug verify` counts, passed. The same counts at the previous candidate `e98267c0` (WP-257b) | met at `cd6ef6bc` (sessions, verify, lines, trees, L6 with `-g` in 3.2) |

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | at `K` (WP-233): the libc++ lane's recorded control, kiln 1.2.0 `8f93304ee` + `llc`, 2026-11-15 04:25 to 05:35 EST, the baseline's command and settings (`--timeout 20 -j 3`): 9247 tests, 8608 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 2 timeout; recorded in `libcxx-hod.md` ("At the candidate cd6ef6bc") and `spec/landed/harness/WP-233.md`. `libcxx-baseline.md` itself still shows the earlier run (2026-11-11, kiln 1.0.1 `bd60e1829`: 8609 pass, 4 fail, 47 xfail, 1 xpass, 585 unsupported, 1 timeout) | met at `K` (the lane's control; the page `libcxx-baseline.md` is not rewritten for `K`) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | at `K` (WP-233): the lane's recorded control, kiln 1.2.0 `8f93304ee` + `llc` under qemu, 20 shards, 2026-11-14 23:36 to 2026-11-15 04:24 EST: 9247 tests, 8607 pass, 5 fail, 45 xfail, 3 xpass, 585 unsupported, 2 timeout; recorded in `libcxx-hod-x64.md` ("At the candidate cd6ef6bc", "WP-233 measurement"). `libcxx-baseline-x64.md` itself still shows the earlier run (2026-11-12/13, kiln `bd60e1829`: 8608 pass, 5 fail, 46 xfail, 2 xpass, 585 unsupported, 1 timeout) | met at `K` (the lane's control; the page `libcxx-baseline-x64.md` is not rewritten for `K`) |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | **4.3 with the frozen binary `hod-cd6ef6bc`** (WP-233), kiln `8f93304ee`, natively, 2026-11-15 18:41 to 20:47 EST: 9247 tests, 8607 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 3 timeout; 7226 of 7226 modules by hod, 0 refusals, 0 fallback. Against the control at `K` (row above) one changed outcome, `ifstream.members/buffered_reads.pass.cpp`: PASS in the control, TIMEOUT at the 20 s limit in strict under a heavier host load; alone at `-j 1` and 120 s it passes with hod (42.8 s). `lost_wakeup.pass.cpp` and `offset_range.pass.cpp` time out in both modes; failures and XPASSes are the same. Natively on the runner: the sweep row below | met at `cd6ef6bc` (the one difference is a time limit under load, passing alone) |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | **4.4 with the frozen binary `hod-cd6ef6bc`**, kiln `8f93304ee`, under qemu, the whole suite executed (20 shards merged): 9247 tests, 8608 pass, 5 fail, 45 xfail, 3 xpass, 585 unsupported, 1 timeout; 7226 of 7226 modules by hod, 0 refusals, 0 fallback. Against the lane's kiln + `llc` control at `K` (8607 pass, 2 timeout, otherwise the same) one changed outcome, `unord.multimap/local_iterators.pass.cpp`, TIMEOUT in the control and PASS with hod; no test passes with `llc` and not with hod. Natively: the sweep row below (8750 = 8750). Page: `libcxx-hod-x64.md` "At the candidate cd6ef6bc" | met at `cd6ef6bc` |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | 2026-11-14: control 8606 pass; strict hod `3b14f8f`, kiln `21b9110a4`: 8605, 7224 of 7226 modules; the one difference `PR28469_undefined_behavior_segfault.sh.cpp` (`debug:dbg-value-expr`, built since WP-225); the other refusal is kiln's (fails in the control too) | partly (awaiting: 4.6 with `hod-cd6ef6bc` at `K`) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | hod `3b14f8f`, kiln `21b9110a4`: compile-only census 7224 of 7226 modules; executed: shard 1 of 20, 0 differences | partly (one shard of 20 executed; awaiting: 4.7 with `hod-cd6ef6bc`, the whole suite) |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 43778998227 of exactly `cd6ef6bc` (2026-11-15 19:12 to 19:26 EST), kiln `8f93304ee`, strict, every job green and `summary` green. x86-64 runner: hod 9247 tests, 8750 pass, 4 fail, 45 xfail, 2 xpass, 446 unsupported, `llc` the same; AArch64 runner: hod 8749 pass, 4 fail, 46 xfail, 2 xpass, 446 unsupported, `llc` the same; 7365 of 7365 modules by hod on each; no test whose outcome differs on either runner. Earlier: run 43465227200 at `a67370e` had one difference per runner (both causes fixed since) | met at `cd6ef6bc` |

## Real projects (L9)

The 1.0 set is the projects the lanes have measured up to the
candidate (`docs/RELEASE.md` section 5, which has the list and the rule:
each row names its hod and kiln commits, a row measured before the
candidate is marked so, the candidate re-runs as many strict rows as
fit in the order LLVM and Clang, DuckDB, gRPC, Boost, abseil, then the
rest, and reuses a control when kiln and the revision are unchanged).

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row of the 1.0 set: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | 44 rows (22 projects; gRPC `-O0 -g` running), every one measured before the candidate (hod `77f1f69`, `7b127f48`, `3b14f8ff`, `77873543`, `46076a33`, `56a4695f`; kiln `21caaf874`, `21b9110a4`, `8f93304ee`). Open: zlib (1 object not offered by kiln, `coverage`); mold's kiln is not in its reports. Final lane (WP-223, DuckDB, kiln `8f93304ee`): `-O0` strict at `cd6ef6bc`, 1901 of 1901 objects by hod, tests 4615 of 4629 against the control's 4614 of 4629; `-O0 -g` strict at `e98267c0`, the same counts (carried over from `e98267c0`) | partly (awaiting: the native lanes' strict re-runs at `cd6ef6bc` in section 5's order, then `oss.md` regenerated with `--candidate cd6ef6bc`) |
| the lane, x86-64 (`oss-x64.md`) | the same for x86-64 | new page: 34 rows (18 projects; re2 and Boost `-O0 -g` running), cross-built, tests under qemu, every one measured before the candidate (hod `3b14f8ff`, `686ff25b`, `ee8e0e7b`, `56a4695f`, `9e25d3f1`). Boost's strict build that stopped at `asm:section` at `56a4695f` (built since WP-255) was measured again: see the second set below. Final lane (WP-253, the fifth set, kiln `8f93304ee`, `spec/landed/harness/WP-253.md`): strict at `cd6ef6bc` against the recorded controls, every object by hod, none declined or not offered, links and tests as the control: scnlib, mp_units, reflect_cpp, ctre, entt, cereal, taskflow `-O0`, glaze, stdexec, and daw_json_link (159 of 159 objects, 158 of 158 links, 147 of 147 tests); libunifex strict at `e98267c0` (114 of 114 objects, tests 91 of 94 as the control; carried over from `e98267c0`); taskflow `-O0 -g` not run (WP-275). Second set (lane `wp/200`, final at `4e760c95`, kiln `8f93304ee`, rows in the lane's census; strict, every object by hod, tests as the control): at `cd6ef6bc` Boost (`boost_tmpl`) `-O0` (631 of 631 objects, tests 534 of 536, the 2 failing in every mode), Eigen `-O0` and `-O0 -g` (240 of 240 objects, 240 of 240 tests; the `-O0` control was built with kiln `21b9110a4`); carried over from `e98267c0`: highway `-O0` (40 of 40 objects, 28 of 28 tests), range_v3 `-O0` and `-O0 -g` (255 of 255 objects, 254 of 255 tests as the control), Boost `-O0 -g` (631 of 631, 534 of 536) | partly (awaiting: the x86-64 lanes' strict re-runs at `cd6ef6bc`, then `oss-x64.md` regenerated with `--candidate cd6ef6bc`) |
| hosted (`oss.yml`) | both runners natively | the candidate's run: run 43779000922 of exactly `cd6ef6bc`, kiln `8f93304ee`, green, completed 2026-11-15 20:41 EST: both builds, 15 projects on each of x86_64 and aarch64 (benchmark, re2, abseil, googletest, fmt, protobuf, yaml_cpp, glog, nlohmann_json, doctest, spdlog, catch2, cli11, simdjson, leveldb), both summaries green. The tagged tree's `scripts/oss/oss-build.sh` differs from `cd6ef6bc`'s (58a0defb, `spec/landed/harness/WP-223.md`; and the x86-64 cross-build link flags of WP-191), so the hosted real-projects workflow is run again on the tagged commit before the tag is made, and the tag exists only if that run was green | met at `cd6ef6bc`; the run on the tagged commit: met in the tagged commit (the tag exists only after it was green) |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | kiln `21caaf874` (native) and `21b9110a4` (x86-64, hod `3b14f8ff`, WP-222): LLVM 19.1.7 native 2778 of 2779 TUs (1 not offered), check-llvm at parity; Clang 3621 of 3623, 12460 lit tests at parity; LLVM for x86-64 2795 of 2795 C/C++ TUs, 0 declined | partly (awaiting: the LLVM lane's strict build and tests with `hod-cd6ef6bc` at kiln `8f93304ee`, both trees; first in the candidate's order) |
| the 1.0 set (ROADMAP M6) | a row on a page for each project of the set, both flavours where measured | the set is named (`docs/RELEASE.md` section 5): 22 projects on AArch64, 18 on x86-64, LLVM and Clang on both; the other rows of `scripts/oss/projects.toml` and of kiln's `projects.toml` (59 rows at `8f93304ee`) come after 1.0 | partly (awaiting: the lanes' rows above; Boost and DuckDB to re-measure) |

## Random programs (L10)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | **the budget's run with the frozen binary `hod-cd6ef6bc`**, seeds 200001 to 226250 (26,250), 2026-11-15 15:56 to 21:00 EST, exit 0: 62 of 62 slices `error 0 finding 0`, 0 refused; set apart, each on its evidence and none hod's wrong code: 29 seeds `llc19-int128-last-register` (x86-64 LLVM 19 `abi`), and in the debug slices 2 `selector-split` pairs (the oracle's own selectors disagreeing among themselves) and 2 `dead-store` pairs (WP-266); `fuzz.md` "At the candidate cd6ef6bc". Earlier: the first full run of the budget (WP-241) on a frozen `df415453`: 26,250 seeds (100001 to 126250), 4 h 44 min, 0 refused, 0 errors, 49 findings, **none hod's wrong code**: three classes, each now set apart by its own evidence (`ub-float-cast`, a sanitizer's report; `selector-split`, `llc`'s own configurations differing; `llc19-int128-last-register`, gcc and `llc` 22 agreeing with hod, WP-249: its 21 findings re-run came out set apart) | met at `cd6ef6bc` (the runs with `hod-02fe2d02` and `hod-e98267c0`, the latter stopped at 15:56 EST partial, are earlier candidates' and count for nothing here) |

## Status pages and release steps

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs`; `classify_support_doc` fails when it differs, so every gated commit has it current; the full gate's `test` step on `cd6ef6bc` passed; `classify_support_doc` (and `classify_matrix`) passed on this branch after `cd6ef6bc` was merged into it (WP-257c, 16:15 EST), so the page is current at `cd6ef6bc` | met at `cd6ef6bc` |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated for kiln 1.2.0 `8f93304ee` (they change only with the corpus, that is, with the pin) | met at kiln `8f93304ee` |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | regenerated at `cd6ef6bc` (WP-257c, `corpus reasons --llc -j 3 --hod hod-cd6ef6bc` over the corpora of kiln `K` = `8f93304ee`, 2026-11-15: `reasons.md` 17:22 to 17:46 EST, `reasons-x64.md` 17:46 to 18:06 EST, after the hybrid `corpus verify --target x86_64` with the same binary, 16:13 to 17:22 EST). The generated pages carry no commit line of their own; this row is their provenance. AArch64: every module compiled (3,487 and 3,445, LLVM 22 and 19); x86-64: every module but `cc-run.cpp`'s (`cc:x86_regcallcc`; 3,516 and 3,474 compiled), "What the built modules do" 13,000 PASS of 13,004 and 12,848 of 12,852, the 4 FAIL of each being `builtins/strncmp.c`, UNDEFINED. Every count is the one of the earlier candidate `02fe2d02` (WP-257); only the pages' timing tables changed | met at `cd6ef6bc` |
| the other status pages | each at the candidate | at `cd6ef6bc` (WP-257c): `debug.md`, `debug-x64.md` ("At the candidate"); `reasons.md`, `reasons-x64.md` (the row above). Independent of hod: `corpus.md`, `corpus-x64-census.md` (generated, kiln `8f93304ee` per group), `corpus-x64.md` (the capture's record; says which recapture the candidate used), `linkers.md` (the linkers' versions, as in the header), `libcxx-baseline.md`, `libcxx-baseline-x64.md` (kiln + `llc` only; still kiln `bd60e1829`, awaiting 4.1, 4.2 at `K`). Not a condition: `perf.md` (generated 2026-11-13, left as measured). Fed by the lanes, still of earlier commits (section "Pages fed by the lanes" below): `oss.md`, `oss-x64.md`, `oss-llvm.md`, `libcxx-hod.md`, `libcxx-hod-x64.md`, `libcxx-hod-debug.md`, `libcxx-hod-debug-x64.md`, `fuzz.md` | partly (awaiting: the lanes' pages) |
| version bump | the workspace version moved, on the owner's word | the owner's word was given on 2026-11-15: the release is made when every condition row is met at the final candidate. The bump (`Cargo.toml` `[workspace.package]` `version`, and `Cargo.lock`, `docs/RELEASE.md` section 8) is the last commit of the tagged commit's history, on top of these pages, and changes no crate's code besides the version | met in the tagged commit (the owner's word of 2026-11-15) |
| tag | made on the owner's word | the owner's word of 2026-11-15; the tag `v<version>` is made on the bump commit after that commit's own full gate is green, and only then | met in the tagged commit (it exists only after its gate was green) |

## Pages fed by the lanes

Each is rewritten from its lane's results with `hod-cd6ef6bc` at kiln
`8f93304ee` (`docs/RELEASE.md` sections 4 to 7); until then it names the
commits it was measured at.

**Rows carried over from `e98267c0`** (the owner's decision of
2026-11-15): a real-project result measured at the previous candidate
`e98267c0` with zero refusals stands for the candidate; its row names
`e98267c0` and says it is carried over because `cd6ef6bc` emits
byte-identical objects. The evidence: the two frozen binaries
`hod-e98267c0` and `hod-cd6ef6bc` over the whole corpus, both targets,
plain and with `-relocation-model=pic`, 25,864 comparisons: 24,404
objects identical, 0 different, 0 with a changed outcome, 1,460 identical
failures (the 730 modules neither compiles: concatenated
`module.interface` entries and the gcc-torture `builtins` refusals); and
a real project's own objects: LLVM for x86-64 rebuilt strict at
`cd6ef6bc` in the same tree, all 2,798 objects byte-identical to the
`e98267c0` build. The limit: the corpus is not the projects' own
translation units. The difference between the two commits is WP-269 (four
refusal exits of the constant fold removed, `spec/landed/codegen/WP-269.md`),
and everything it touches was measured again at `cd6ef6bc`: the full gate,
libc++ on both targets, daw_json_link, the hosted `ci.yml` and
`sweep.yml` runs, the debug and corpus measurements, and every lane step
that had not started; the fuzz budget and the hosted `oss.yml` run are
measured again at `cd6ef6bc`: see their rows. The rule is
`docs/RELEASE.md` section 5, rule 5; on the generated pages such a row's
`at the candidate` cell reads `carried over from e98267c0 (see
release.md)` (`--carried-from e98267c0`). Nothing measured
at the earlier candidate `02fe2d02` is carried over: a row whose only
measurement is at `02fe2d02` says so and is not counted as met.

| page | regenerated by | waits for |
|---|---|---|
| `oss.md` | `source scripts/env.sh && B=$(dirname "$CARGO_TARGET_DIR") && python3 -I scripts/oss/oss-report.py status --root $B/oss --root $B/oss3 --root $B/oss4 --provenance scripts/oss/provenance-aarch64.tsv --candidate cd6ef6bc --carried-from e98267c0 --out docs/status/oss.md` | the native lanes' strict re-runs at `cd6ef6bc` (5.1), their provenance rows |
| `oss-x64.md` | the same with `--target x86_64 status --root $B/oss --root $B/oss2 --root $B/oss3 --root $B/oss5 --provenance scripts/oss/provenance-x86_64.tsv --candidate cd6ef6bc --carried-from e98267c0 --out docs/status/oss-x64.md` | the x86-64 lanes' strict re-runs at `cd6ef6bc` (5.2) |
| `oss-llvm.md` | by hand from the LLVM lane's records (5.4) | LLVM and Clang strict builds and tests with `hod-cd6ef6bc` at `K`, both trees |
| `libcxx-hod.md` | 4.3, then `python3 -I scripts/libcxx-hod-report.py <dir>/strict <dir>/strict.tsv <baseline.tsv>`; the native section from 4.5 | 4.1 at `K` and 4.3 with a shim of `hod-cd6ef6bc`; a `sweep.yml` run of `cd6ef6bc` |
| `libcxx-hod-x64.md` | 4.4 (`scripts/libcxx-hod-x64.sh strict <dir> --shard K/4`, then `merge`), the report against 4.2; 4.5 | 4.2 at `K`, the four shards |
| `libcxx-hod-debug.md` | 4.6 (`scripts/libcxx-hod-debug.sh control`, then `strict`, the report) | both runs at `K` with `hod-cd6ef6bc` |
| `libcxx-hod-debug-x64.md` | 4.7 (`HOD_DEBUG_TARGET=x86_64`, shards, `merge`) | the whole suite executed |
| `fuzz.md` | section 6: `cargo xtask fuzz --budget --seed 200001 -j 4 --hod hod-cd6ef6bc --out <dir>`, the page rewritten from its summary | at `cd6ef6bc`: the budget run 2026-11-15 15:56 to 21:00 EST (`.build/candidate/fuzz-cd6ef6bc`), section "At the candidate cd6ef6bc" |

## Summary

As of 2026-11-15 20:20 EST (WP-257d), for the candidate `cd6ef6bc` at
kiln `8f93304ee`: 29 rows, 28 of them conditions (wild is listed and is
not one). 13 met at the candidate: the full gate on exactly `cd6ef6bc`
(green, 3:16:30) and with it the quick gate's steps, linkers bfd/lld/mold
and the support matrix; the hosted `ci.yml` run 43778976284 (both lanes
green, the x86-64 lane natively) and the `sweep.yml` run 43778998227 (hod
equal to `llc` on both runners); with the frozen binary `hod-cd6ef6bc`,
L6 strict on both targets, L7 on both targets, the reasons pages and
libc++ strict on x86-64 under qemu (its page follows with the lane's
branch); the corpus pages are met at kiln `8f93304ee`. 2 met in the
tagged commit (the version bump and the tag, on the owner's word of
2026-11-15). 1 met before the candidate and awaiting its run (the fuzz
budget at `df415453`, its run with `hod-cd6ef6bc` in progress since 15:56
EST). 2 re-measure (the two libc++ baselines at `K`), 8 partly, 1 missing
(the hosted `oss.yml` run of `cd6ef6bc`, in progress). The corpus and
debug counts measured at `cd6ef6bc` equal those of the earlier candidates
`e98267c0` and `02fe2d02`. Still open, in the order they block 1.0:
libc++ strict on AArch64 and both `-g` runs (the libc++ lanes at `K`);
real projects: the lanes' strict re-runs (LLVM and Clang, the first,
second, third and x86-64 sets) and their pages, with the rows at
`e98267c0` carried over; the fuzz budget's run; the `oss.yml` run.
