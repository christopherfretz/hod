# Release candidate 1.0: the checklist

The conditions of the release-candidate gate (`docs/VALIDATION.md`
"Gates", "Release candidate"; `docs/ROADMAP.md` M6) and of the owner's
definition of 1.0, each with the evidence it needs, the evidence on `main`
today and its state. How to produce each piece of evidence is
[`docs/RELEASE.md`](../RELEASE.md). Not conditions of 1.0: the benchmark
gate (`perf.md` stays as last measured), the in-process bridge, and the
32-bit x86 target.

| | |
|---|---|
| written | 2026-11-14 (21:36 EST), WP-243 |
| hod `main` | `56a4695f` (wave 24) |
| kiln pin of `main` | `21b9110a4` (kiln 1.1.0: `.github/kiln-revision`, the corpora's kiln groups); the move to `8f93304ee` (kiln 1.2.0) is WP-235, in the next wave |
| candidate | none chosen; no version bump, no tag (version `0.0.0`) |

States: **met at `<commit>`** (measured at that commit and nothing since
can change it); **re-measure** (met at an older commit: measure again at
the candidate); **partly** (part of the condition is measured or met);
**missing** (no evidence); **in progress** (a package is producing it).
Every commit named is hod's unless it says kiln. A hod commit in the pages
is written with 7 or 8 hex digits as the page has it.

## Gates and lanes

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the landing gate of wave 24 at `56a4695f`, 2026-11-14: `ci: ok`, exit 0, 348 test binaries; quick `roundtrip` 212 of 212 entries as expected; quick `irdiff` and the x86-64 seeds under qemu with linkers bfd, lld, mold; kiln `21b9110a4`. The log is the supervisor's, not a page | met at `56a4695f` (re-run at the candidate) |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with 22 and 19, the whole-corpus `roundtrip`, `debug` | no run of the full gate is recorded (the landing gates are the quick gate) | missing |
| gate on the x86-64 host | the gate's steps natively on x86-64 | only the hosted x86-64 lane (next row): the quick gate's steps without `roundtrip`; no x86-64 development host has a corpus, so no full gate runs there | partly |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 43662223396 of `56a4695f`: every job green but `x86_64 / test x64`, which died on a runner with AVX-512 (an LLVM 19 FastISel memory blow-up on the `zmm` grid; the fix is WP-238 part 1, next wave); its re-run reached the 45-minute limit. Run 43657241521 of `1d96122c` (wave 23): green on a re-run | partly (red at `56a4695f`; green at `1d96122c`) |
| linkers bfd, lld, mold | every linking test and `irdiff` program with all three, required | the gates above require and name `bfd lld mold`; versions recorded in `debug.md`: GNU ld 2.44, lld 22.1.8, mold 2.37.1 | met at `56a4695f` (re-run at the candidate) |
| linker wild | the same with wild, required, both targets, hosted lanes too | not on `main`: WP-239 (wild 0.11.0, pinned) is landing in the next wave; the hosted install step is unproven until that wave's hosted run; making it required (`linkers::WILD_REQUIRED`) is a follow-up | missing (landing in the next wave) |

## Kiln's corpus (L6) and debug information (L7)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | no strict `corpus verify` result on a page; `reasons.md` (measured at `9f410277`, wave 16; kiln `21b9110a4`): 3480 of 3483 modules (LLVM 22) and 3438 of 3441 (LLVM 19) compiled; first refusals `cc:win64cc` 1, `debug:dbg-value-arglist` 1, `intrinsic:llvm.aarch64.ldxr.p0` 1 per major | missing (compile-only counts at an older commit) |
| L6 strict, x86-64 | the same under qemu | no strict verify; `reasons-x64.md` (at `9f410277`): 3510 of 3513 (22), 3468 of 3471 (19) compiled; its "What the built modules do" has no numbers ("no `verify-x86_64-llvm22-hybrid.tsv`"); the only verify run, in `corpus-x64.md` (WP-139, kiln 1.0.1 `bd60e1829`), predates the x86-64 selector (hod built 0 of 3140 and 0 of 3104) | missing |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | WP-150's branch on `main` `3a98566`, kiln 1.0.1 `bd60e1829`, 2026-11-13: kiln's debugger tests 8 of 8; `tests/debug` met on both targets (AArch64 15 XFAIL; x86-64 22 SKIP; 0 FAIL); `--verify` 210 of 210 objects hod built, 2 corpus modules declined; line tables 12 of 12 corpus modules, 198 of 198 of the `-g` set; **L6 strict with `-g` not met: 12 of 14** (`debug:dbg-value-arglist`, which WP-230 has since taken on) | partly (re-measure; stale against `main` and kiln) |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | branch `wp/136-x64-debug` on `main` `73865eb`, debug build, 2026-11-13: 236 results, 210 PASS, 12 XFAIL, 14 SKIP, 0 FAIL, 0 XPASS, 0 crash; 384 sessions; bfd 70, lld 70, mold 70 PASS; the XFAILs are hod_x64 refusals (`attr:signext`, `attr:zeroext`, `attr:presplitcoroutine`). Not in the full gate (its `debug` step runs the host's target only) | re-measure |

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | 2026-11-11, kiln 1.0.1 `bd60e1829`: 9247 tests, 8609 pass, 4 fail, 47 xfail, 1 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure (kiln has moved twice since) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | 2026-11-12/13, kiln 1.0.1 `bd60e1829`: 9247, 8608 pass, 5 fail, 46 xfail, 2 xpass, 585 unsupported, 1 timeout, 99.4% | re-measure |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | native runner, run 43465227200, `wp/158-sweep` at `a67370e`, kiln `bd60e1829`, 2026-11-13: hod strict 8,748 pass, 5 fail, 7,364 of 7,365 modules; `llc` 8,749 pass, 4 fail; one difference, `alg.find/find.pass.cpp` (`abi:vector-element`, since built: WP-167). Development host, third measurement (WP-102) at `6e6bf9c`: 8586 against 8609 (21 refused, `inst:insertelement/i128`; 2 load timeouts), 7205 of 7226 modules | partly (one difference at `a67370e`, its cause fixed since; re-measure: WP-233 in progress) |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | native runner, run 43465227200, `a67370e`: hod strict 8,749 pass, 5 fail, 7,364 of 7,365 modules; `llc` 8,750 pass, 4 fail; one difference, `simd_unary.pass.cpp` (`x64:vector-wide-element`, since built: WP-161). Under qemu: hybrid only, `3a98566` merged into `ad92386`, 8,607 against the baseline's 8,608, 7,191 of 7,226 modules; strict not run (derived: 8,572) | partly (as AArch64; WP-233 in progress) |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | 2026-11-14: control kiln `21caaf874` 8606 pass; strict hod `3b14f8f`, kiln `21b9110a4`: 8605 pass, 6 fail, 7224 of 7226 modules; one difference, `PR28469_undefined_behavior_segfault.sh.cpp` (`debug:dbg-value-expr`, built since WP-225); the other refusal is kiln's (`not_fn.nttp`, `malformed-input`, fails in the control too) | partly (re-measure) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | hod `3b14f8f`, kiln `21b9110a4`, 2026-11-14: compile-only census of 9247 tests, 7224 of 7226 modules by hod; executed: shard 1/20 only (463 tests), control, hybrid and strict 427 pass each, 0 differences; PR28469 strict passes with hod `46076a3` (run alone) | partly (one shard of 20 executed) |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 43465227200 at `a67370e` (above): one difference per runner, so `summary` failed | re-measure |

## Real projects (L9)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | 19 projects, `O0` and `O0g` (38 rows): hod `hod-77f1f69`, `hod-7b127f48` and `hod-3b14f8ff`, kiln `21caaf874` (15 projects) and `21b9110a4` (simdjson, magic_enum, tl_expected, zlib, zstd). 36 rows: fell back 0, not offered 0, strict `ok`, tests `same` (abseil `O0g` one better). **zlib** (both flavours): 1 of 37 objects not offered by kiln (`coverage`), 1 object `llvm` in the strict build, the build marked `failed (0 TUs)`, tests 5/6 as the control. Objects outside hod with no fallback recorded: glog 45 of 47, simdjson 154 of 169 | partly (zlib not zero-fallback; rows at three older hod commits) |
| the lane, x86-64 | the same for x86-64 | no page; `spec/landed/harness/WP-191.md` (kiln `21caaf874`, hod main at wave 13 or 15, cross-built, tests under qemu): 10 projects strict with no refusal; protobuf's build fails in both (it runs the x86-64 `protoc`); re2 `O0` and `absl_demangle_test` worse than the control then, both explained as not miscompiles | partly (no status page; stale) |
| hosted (`oss.yml`) | both runners natively | no result recorded | missing |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | kiln `21caaf874`; the page names no hod commit (the records do: WP-178 `95cf7e9`, WP-180 `1bc3210`). LLVM 19.1.7 native: 2778 of 2779 TUs (1 not offered: `EntryExitInstrumenter.cpp`, `instrument-entry`), check-llvm at parity (lit 36338 tests, 30390 pass, 2 fail in both; 68 unit binaries, 8007 tests); Clang: 3621 of 3623 (2 not offered), 12460 lit tests at parity; LLVM for x86-64: 2794 of 2799 (4 assembled `.S`, 1 not offered), unit and lit slices identical | partly (re-measure with kiln `21b9110a4`, which no longer has the `instrument-entry` false positive: WP-222 in progress) |
| kiln's open-source project set (ROADMAP M6) | a row for each project of kiln's set | see the next table: 19 of kiln's 59 rows measured in `oss.md`, LLVM and Clang in `oss-llvm.md` | partly |

**kiln's set against hod's rows.** kiln's `scripts/oss/projects.toml` at
kiln `8f93304ee` has 59 rows (waves 1 to 5).

| | kiln rows |
|---|---|
| measured in `oss.md` (19) | fmt, nlohmann_json, spdlog, catch2, googletest, doctest, magic_enum, cli11, tl_expected, abseil, re2, benchmark, glog, yaml_cpp, simdjson, protobuf, leveldb, zlib, zstd |
| measured in `oss-llvm.md` (as hod's own LLVM recipe) | llvm_project and llvm_full (LLVM 19.1.7, check-llvm); llvm_clang in part (Clang with 12 directories of its lit tests; lld not built) |
| in hod's `projects.toml`, no row yet (packages in progress: WP-200, WP-211) | range_v3, eigen, rocksdb, opencv, grpc |
| in a package's scope, not yet in hod's `projects.toml` (WP-223) | duckdb, onetbb, folly |
| no row and no package | fmt_module, cmake_modules, c_demo, catch2_cxx23, catch2_werror, entt, taskflow, cereal, llvm_selfhost, z3, stdexec, libunifex, folly_coro, ctre, glaze, mp_units, scnlib, daw_json_link, reflect_cpp, boost_heavy, boost_meta, boost_compiled, cmake_app, bitcoin_core, jsoncpp_meson, hunspell_autotools, sqlite, lua, c_ares |

hod's lane drives CMake only (`scripts/oss/projects.toml`), so kiln's
meson (jsoncpp_meson), autotools (hunspell_autotools, sqlite) and make
(lua) rows, and the C++20-modules rows (fmt_module, cmake_modules), need
the lane extended first. hod's own rows that kiln does not have:
highway, highway_avx2, simdjson_haswell, boost_tmpl, flatbuffers, mold,
capnproto, cmake.

## Random programs (L10)

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | `fuzz.md` measured hod `6e300f5` (WP-154: seeds 1 to 2500, 0 findings) and `f4108bb` (WP-160, LLVM 22 only: 0 findings in `--mix` and whole runs; `--debug` seeds 1 to 500 one finding per target, seed 112, triaged to the harness, no permitted class yet). The budget is not yet stated in numbers | in progress (WP-241) |

## Status pages and release steps

| condition | evidence required | evidence on main today | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs`; `classify_support_doc` fails when it differs, so the gate of `56a4695f` checked it | met at `56a4695f` |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated 2026-11-14 for kiln 1.1.0 `21b9110a4`; the corpus is captured again for `8f93304ee` by WP-235 | re-measure (with the kiln pin) |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | hod `9f410277` (wave 16) | re-measure |
| the other status pages | each at the candidate | as in the rows above; `perf.md` (2026-11-13) is not a condition | re-measure |
| version bump | the workspace version moved, on the owner's word | `Cargo.toml` `[workspace.package]` `version = "0.0.0"`; no tag | missing (owner's word) |
| tag | made on the owner's word | none | missing (owner's word) |

## Summary

29 rows: 3 met at `56a4695f` (quick gate, linkers bfd/lld/mold, support
matrix; each to re-run at the candidate), 7 re-measure, 11 partly, 7
missing (2 of them the owner's: version and tag), 1 in progress. In the order they block 1.0: no full gate and no
strict L6 run is recorded; libc++ strict on both targets is one test short
at `a67370e` (both causes fixed since, the re-measurement is WP-233);
wild is landing; real projects have zlib's one object outside hod, no
x86-64 page, no hosted result, and 37 of kiln's 59 rows with no
measurement (llvm_clang only in part); the
fuzzing budget is WP-241.
