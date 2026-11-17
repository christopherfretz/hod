# Cutting a release candidate

How to produce the evidence of the release-candidate gate
(`docs/VALIDATION.md` "Gates", "Release candidate"; `docs/ROADMAP.md` M6)
for one commit of hod, and what to change when the owner accepts it. The
checklist with the evidence on `main` today is
[`docs/status/release.md`](status/release.md).

**1.0 is** (the owner's definition, 2026-11-14, with the decisions of
2026-11-15): the real projects of "the 1.0 set" (section 5) with no
fallback whose tests equal the kiln + `llc` control; libc++ strict on both
targets at the candidate commit; the fuzzing budget with no open findings;
the linker matrix with its three required linkers, GNU ld (bfd), lld and
mold; the status pages regenerated; the version bump. kiln's pin for 1.0
is `8f93304ee` (kiln 1.2.0) and does not move before the release.
**1.0 is not** conditional on the benchmark gate and the performance work,
the in-process bridge (both `docs/ROADMAP.md` M5, after 1.0), the 32-bit
x86 target, or wild: wild stays an optional linker of the test matrix,
used where it is installed (`linkers::WILD_REQUIRED` stays false), with
its exceptions listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`,
the debugger layer's XLINK outcome); hod's code generators and object
writer take no workaround for it.

**Nobody bumps the version or makes a tag without the owner's explicit
word.** Everything below up to "The version" only measures.

## 0. Fix the candidate

1. Choose the candidate: a commit `C` of hod `main` with no package in
   flight that changes code generation. Write `C` (8 hex digits) and the
   date (`date`) at the top of `docs/status/release.md`.
2. kiln's pin `K` is the one commit in all three places of
   `docs/VALIDATION.md` "Moving kiln's pin": the workspace's `kiln`
   submodule (its `target-kiln` release build is kiln++),
   `.github/kiln-revision`, and the kiln groups of both corpora (each
   entry's key). If they differ, finish the move first; every measurement
   below is made with `K`.
3. Build once and freeze the binary for every lane:
   `source scripts/env.sh && cargo build --release -p hod_cli`, then copy
   `<target dir>/release/hod` out of the build directory so no later
   build can replace it while a lane runs (as WP-160 did for its fuzz
   runs). Every result page names `C`, `K` and this binary.
4. Tools: the reference environment is `spec/harness.md` "CI" rule 2
   (Debian 13; LLVM 19 and 22, binutils, lld, mold, gdb, gdb-multiarch,
   lldb-19, `qemu-user`, csmith, the other architecture's cross
   toolchain), and wild 0.11.0 from the provisioning script (optional,
   section 2). Record each linker's version (`ld.bfd --version`,
   `ld.lld --version`, `mold --version`, and `wild --version` when it is
   installed) and the debuggers'
   (`gdb --version`, `lldb-19 --version`): `docs/VALIDATION.md`
   "Linker matrix" asks that status reports record them.

Times below are of the shared 16-core AArch64 development host under
load (average 20 to 30); a quiet machine is faster. Runs longer than a
minute go in the background. Every corpus-wide or suite-wide command
uses at most `-j 4`; real-project builds take the lanes' job pool
(`MAKEFLAGS=--jobserver-auth=fifo:/run/hod/lanes.fifo`, set by
`scripts/oss/oss-run.sh`).

## The candidate's order of work

Times are as measured where a record has them, on the shared AArch64
host; "not measured" where none has.

1. **Freeze** (section 0): choose `C`, check `K` = `8f93304ee`.
2. **One release build** of `C`, copied out of the build directory (its
   sha256 recorded); every step below uses that copy. A few minutes.
3. **Then in parallel**, each within its own job limit:
   - the full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci` (1.2): no whole
     run timed; the quick gate's part 90 to 125 min, then
     `corpus-aarch64` 45:21 and `corpus-x86_64` 56:06 (WP-251), the
     debug steps' `debug run` 4:24 and 1:50 (WP-244), the full `irdiff`
     steps not measured;
   - the fuzz budget (section 6),
     `cargo xtask fuzz --budget --seed 200001 -j 4 --hod <frozen hod> --out <dir>`:
     4 h 44 min for the first run, resumable;
   - libc++ strict on both targets (4.3, 4.4; 4.6 and 4.7 for `-g`):
     4241 to 5826 s on AArch64 at `-j 3`, about 11,400 s for x86-64
     under qemu; with the baselines 4.1 and 4.2 only if `K` or the suite
     moved since they were measured;
   - the hosted native sweep (4.5): about 14 min of wall time;
   - the hosted lanes (1.3) on `C`: the `test x64` job 17 min (natively,
     Xeon 6975P-C) to 20 min 40 s (EPYC 9V74, two programs under SDE);
   - the real projects by the rule of section 5: the strict builds and
     tests of the 1.0 set's rows in its order (LLVM and Clang, DuckDB,
     gRPC, Boost, abseil, then the rest), controls reused where `K` and
     the revision are unchanged; per row not measured as a whole, LLVM
     and Clang hours per tree.
4. **The pages** (section 7), each from its runs at `C`, and
   `docs/status/release.md` with every row's state.
5. **The version bump** (section 8), only on the owner's word, and the
   gates of section 1 on the bump commit.

## 1. The gates with every tool required

| | command | where | time | result | pass means |
|---|---|---|---|---|---|
| 1.1 quick gate | `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick` | AArch64 development host (x86-64 seeds under `qemu-x86_64`) | 90 to 125 min under load (mostly compiling the test binaries) | the log; no page | the log ends `ci: ok` and the exit status is 0 |
| 1.2 full gate | `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci` | the same host; it adds `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with LLVM 22 and 19, the whole-corpus `roundtrip` (10 to 20 min alone), then `bench-aarch64`, `bench-x86_64` (the counts gate of the benchmark, `cargo xtask bench --check --counts [--target x86_64]`, every input, about two minutes each), then `debug-x86_64`, `debug-aarch64` (every debugger test of that target, then `debug verify` of its corpus: 3.3, 3.4 without `debug kiln` and `debug trees`) and `corpus-aarch64`, `corpus-x86_64` (3.1, 3.2 with the workspace's hod) | no whole run timed yet; at least the quick gate plus two hours: measured steps `corpus-aarch64` 45:21 and `corpus-x86_64` 56:06 at `-j 4` (`spec/landed/harness/WP-251.md`), the `debug run` part of `debug-aarch64` 4:24 and of `debug-x86_64` 1:50 (`spec/landed/harness/WP-244.md`); `roundtrip` 10 to 20 min; the full `irdiff` steps not measured alone (the gate prints each step's wall time at its end) | the log; no page | `ci: ok`, exit status 0 |
| 1.3 hosted lanes | push `C` (it is on `main`) or open a pull request: `.github/workflows/ci.yml` calls `ci-lane.yml` on `ubuntu-24.04-arm` and `ubuntu-24.04` | GitHub Actions, both architectures natively, the other one under qemu | jobs in parallel, each `test` part about 15 min (`spec/harness.md` "CI" rule 5); a job is cut at 45 min | the run's page; `test x64`'s job summary has the AVX-512 block (`scripts/avx512-summary.sh`) | the fan-in jobs `check` and `check-x86_64` both succeed, on one run of `C` without a re-run; record the run id and whether the x86-64 runner had AVX-512 (the summary says what executed) |

The hosted lanes run the quick gate's steps (`xtask::ci::HOSTED_STEPS`)
without `roundtrip` (no corpus on a runner), so the x86-64 host's evidence
is the quick gate's steps; the full gate runs only on the development
host. `docs/VALIDATION.md` "Hosted CI" lists the tests that run on an
AArch64 host only.

## 2. Every linker

GNU ld (`bfd`), lld and mold are found by `hod_testutil` and are required
under `HOD_REQUIRE_TOOLS=1`: the gates of section 1 link every linking
test and every `irdiff` program with all three (the gate's log names them:
`linkers bfd lld mold`). wild (WP-239, 0.11.0) is a fourth, optional
linker: where it is installed the same tests link with it too and the log
names four linkers; a missing wild is a note, never a failure
(`linkers::WILD_REQUIRED` in `hod_testutil` is false and stays so). A case
wild cannot pass is listed in `hod_testutil::linkers::EXCEPTIONS` or, in
the debugger layer, reported as XLINK (`docs/VALIDATION.md` "Linker
matrix"); hod's code generators and object writer take no workaround for
it. wild is not a condition of 1.0. Since 1.1 mortar is a fifth linker
of the 64-bit matrix and is required under `HOD_REQUIRE_TOOLS=1`
(`linkers::MORTAR_REQUIRED` is true; WP-281, WP-310; its pin is in
`docs/status/linkers.md`), and the i686 matrix is GNU ld, lld and mold.
**Pass:** the gate logs of 1.1 and 1.2 name every required linker (bfd,
lld, mold and, on the 64-bit targets, mortar), and nothing is skipped for
a missing required linker.

## 3. Kiln's corpus through hod (L6) and debug information (L7)

| | command | target | time | page | pass means |
|---|---|---|---|---|---|
| 3.1 L6 strict | `cargo xtask corpus verify --strict -j 3 --hod <frozen hod>` | AArch64, native | 1,242 s (LLVM 22) and 1,451 s (LLVM 19) at `-j 4`, 45:21 for the gate step (`spec/landed/harness/WP-251.md`) | `<build>/corpus/verify-llvm<N>-strict.tsv`; summary into `docs/status/release.md` | exit status 0: no FAIL, no XPASS, no ERROR, every entry's expectation met with every linker; the only results that are not PASS are the rows of `docs/status/corpus-expected.tsv` for the target, listed as EXPECTED or UNDEFINED in the summary, and no row of it is stale |
| 3.2 L6 strict, x86-64 | `cargo xtask corpus verify --target x86_64 --strict -j 3 --hod <frozen hod>` | x86-64 under `qemu-x86_64` (baseline CPU) | 1,969 s (LLVM 22) and 1,352 s (LLVM 19) at `-j 4`, 56:06 for the gate step (WP-251) | `<build>/corpus/verify-x86_64-llvm<N>-strict.tsv`; then 4.3 fills `docs/status/reasons-x64.md` "What the built modules do" | as 3.1 |
| 3.3 L7 AArch64 | `cargo xtask debug` (sessions), `cargo xtask debug kiln`, `cargo xtask debug verify` | AArch64, native | minutes each | `docs/status/debug.md` (rewritten for `C`, `K`) | no FAIL, no XPASS, no crash, no stale `xfail`; kiln's debugger tests all pass; `--verify` clean for every object hod builds and nothing declined; line tables equal up to `spec/dwarf.md` §5.7; the `-g` corpus modules pass strict under every linker |
| 3.4 L7 x86-64 | `cargo xtask debug --target x86_64`, `cargo xtask debug trees` over the `-g` modules, `cargo xtask debug verify --target x86_64` | x86-64 under qemu's gdb stub | 1:18 for the sessions at `-j 4` (`docs/status/debug-x64.md`) | `docs/status/debug-x64.md` | as 3.3; SKIPs only for what the stub cannot do (`debug-x64.md` "What qemu's gdb stub cannot do") |

The full gate runs 3.1 to 3.4 as its `corpus-<arch>` and `debug-<arch>`
steps (with the workspace's development hod; `debug kiln` and `debug
trees` are not gate steps); run them separately with the frozen hod for
the pages.

## 4. libc++ (L8)

From the workspace root, with kiln++ built at `K`, the libc++ 22.1.8 tree
fetched, and a shim made from the frozen hod:
`cargo xtask shim --llvm /usr/lib/llvm-22 --hod <frozen hod> --out <shim> --kiln <kiln++>`.
Every run goes through kiln's runner (`cargo xtask libcxx` in the kiln
checkout); `scripts/libcxx-hod-kiln.sh`, `scripts/libcxx-hod-x64.sh` and
`scripts/libcxx-hod-debug.sh` are hod's wrappers. Read `oom_kill` from
`/sys/fs/cgroup/memory.events` before and after each run. The pages'
"Repeating the run" sections have the full command lines.

| | command | target | time | page | pass means |
|---|---|---|---|---|---|
| 4.1 baseline | kiln's `cargo xtask libcxx --tag llvmorg-22.1.8 std libcxx --timeout 20 -j 3 --mem-limit 3072 --results <dir>/baseline.tsv` | AArch64 | 3176 s | `docs/status/libcxx-baseline.md` | the run completes; its counts are the reference |
| 4.2 baseline x86-64 | the same with `--compiler scripts/libcxx-x64-kiln.sh` (tests under qemu) | x86-64 under qemu | about 11,800 s | `docs/status/libcxx-baseline-x64.md` | as 4.1 |
| 4.3 hod strict | `HOD_STRICT=1 HOD_LIBCXX_KILN=<shim>/kiln++-hod HOD_LIBCXX_LOGDIR=<dir>/strict cargo xtask libcxx --tag llvmorg-22.1.8 --compiler <hod>/scripts/libcxx-hod-kiln.sh std libcxx --timeout 20 -j 3 --mem-limit 3072 --no-fail --results <dir>/strict.tsv`, then `python3 -I scripts/libcxx-hod-report.py <dir>/strict <dir>/strict.tsv <baseline.tsv>` | AArch64 | 4241 to 5826 s at `-j 3` | `docs/status/libcxx-hod.md` | every test that passes in 4.1 passes, test by test (`libcxx-hod-report.py`); no module refused; a difference that is not hod's is shown with the control rerun alone at `K` |
| 4.4 hod strict x86-64 | `bash scripts/libcxx-hod-x64.sh strict <dir> --shard K/4` for K = 1..4, `bash scripts/libcxx-hod-x64.sh merge <dir> strict`, then the report against 4.2 | x86-64 under qemu | about 11,400 s | `docs/status/libcxx-hod-x64.md` | as 4.3 against 4.2 |
| 4.5 native, both runners | push `C` to a `sweep/**` branch, or run `.github/workflows/sweep.yml` by hand (inputs: kiln's commit `K`, strict) | GitHub's `ubuntu-24.04-arm` and `ubuntu-24.04`, natively | about 14 min of wall time, about 210 runner minutes | the `sweep-summary` artifact; its table into the "Native ... runner" sections of `libcxx-hod.md` and `libcxx-hod-x64.md` | the `summary` job (`scripts/libcxx-sweep.py`) succeeds: no test passes with `llc` and not with hod on the same runner, and hod built every module |
| 4.6 `-g`, AArch64 | `scripts/libcxx-hod-debug.sh control <dir>`, `HOD_SHIM=<shim> HOD_JOBS=4 scripts/libcxx-hod-debug.sh strict <dir>`, the report against the control | AArch64 | 6010 to 11214 s per run | `docs/status/libcxx-hod-debug.md` | as 4.3 against the `-g` control |
| 4.7 `-g`, x86-64 | `HOD_DEBUG_TARGET=x86_64`: the same three modes, in shards under qemu (`--shard K/N`, then `merge`) | x86-64 under qemu | about 9,000 s for a compile-only census; the whole suite executed in 20 shards at `-j 4`, 7,167 to 8,279 s summed over the shards per mode (about 2 h 05 m to 2 h 23 m of wall time per mode with the two modes side by side, at `cd6ef6bc`) | `docs/status/libcxx-hod-debug-x64.md` | as 4.6, over the whole suite (not one shard) |

kiln's own unpassing tests (the four filesystem tests as root, load
timeouts) fail in the control too; the comparison, not 100%, is the gate.
4.5 counts as the native run of both targets; 4.3 and 4.4 are the
development host's and are needed while 4.5 has no `-g` mode.

## 5. Real projects (L9)

| | command | target | time | page | pass means |
|---|---|---|---|---|---|
| 5.1 the lane | `source scripts/env.sh && scripts/oss/oss-run.sh <project>...` (each project of the 1.0 set below, both flavours `O0` and `O0g`, the three modes), then the page's command in section 7 | AArch64, native | per project, in the lanes' notes (three builds of each flavour, one at a time); build trees are pruned after each | `docs/status/oss.md` | every row: `fell back` 0, `not offered` 0, strict build `ok`, every object `hod` in "Strict builds", `against the control` `same` (or better), its `hod` and `kiln` cells naming the commits it was measured at. The one stated exception (the owner's decision of 2026-11-15): kiln never offers hod a translation unit compiled with `--coverage` (skip reason `coverage`; the gcov instrumentation runs in kiln's LLVM pipeline), so zlib's `infcover.c` is `not offered` and zlib has 36 of 37 objects by hod on both targets; not hod's fault, it does not block 1.0 (BACKLOG WP-279) |
| 5.2 the lane, x86-64 | `scripts/oss/oss-run.sh <project>... --target x86_64` (a new hod: `--modes strict --strict`, the control kept), then the page's command in section 7 | x86-64, cross-built, tests under qemu | as 5.1 and more | `docs/status/oss-x64.md` | as 5.1; a test that runs an x86-64 program outside ctest fails in the control too and is compared, not counted |
| 5.3 hosted | `.github/workflows/oss.yml` by hand (inputs: kiln's commit, the rows) or on an `oss/**` branch | both runners natively | per row | the run's artifacts | as 5.1, on both runners natively |
| 5.4 LLVM and Clang | `scripts/oss/llvm/` (native: `configure.sh`, `build.sh`, `strict.sh`, `unittests.sh`, `lit.sh`, `census-file.sh`) and `scripts/oss/llvm-x64/` (cross) | AArch64 native; x86-64 cross under qemu | hours per tree; about 21 GB of disk for a pair | `docs/status/oss-llvm.md` | every TU offered to hod and built by it in the strict build, and the tests identical per test with the kiln + `llc` control |

**The 1.0 set.** 1.0's real projects are the ones the lanes have measured
up to the candidate, not every row of kiln's `projects.toml`: a project
is in the set for a target and flavour when it has a row on
`docs/status/oss.md`, `docs/status/oss-x64.md` or `docs/status/oss-llvm.md`
at the freeze. At the freeze (the pages of the candidate `cd6ef6bc`,
2026-11-15): 24 projects on AArch64, 34 on x86-64, LLVM and Clang on
both:

| project | AArch64, native | x86-64, cross under qemu |
|---|---|---|
| fmt, googletest, abseil, benchmark, protobuf, spdlog, catch2, nlohmann_json, yaml_cpp, glog, leveldb, cli11, doctest, simdjson, zlib, zstd, mold | `O0`, `O0g` | `O0`, `O0g` |
| re2 | `O0`, `O0g` | `O0`, `O0g` (native, the hosted run's x86_64 runner) |
| magic_enum, tl_expected, CMake, OpenCV, DuckDB | `O0`, `O0g` | - |
| gRPC | `O0`, `O0g` (in a note: no control trees kept) | - |
| range_v3, Eigen, Boost (`boost_tmpl`), FlatBuffers, ctre, entt, cereal, scnlib, glaze, daw_json_link, mp_units, reflect_cpp, libunifex, stdexec | - | `O0`, `O0g` |
| highway, taskflow | - | `O0` (`O0g` not measured: WP-275 for taskflow) |
| LLVM 19.1.7 and Clang (`oss-llvm.md`) | native | cross (LLVM) |

On the AArch64 page, the rows of benchmark, re2, abseil, googletest,
fmt, protobuf, yaml_cpp, glog, nlohmann_json, doctest, spdlog, catch2,
cli11, simdjson and leveldb are the reports of the hosted real-projects
run 43779000922 of exactly `cd6ef6bc` (GitHub's aarch64 runner; the hod
cell says so).

After 1.0: every other row of `scripts/oss/projects.toml` (highway_avx2,
simdjson_haswell, capnproto, rocksdb), the flavours not measured above,
and every row of kiln's `projects.toml` that hod's has not
(`docs/status/release.md` lists them).

**The rule for a row at the candidate.** The candidate's re-measurement
has about 16 hours on one shared host, which does not hold every row of
the set, so:

1. A row's page cells name the hod commit and the kiln commit it was
   measured at (`hod`, `kiln`; the generator takes them from the reports
   and from `scripts/oss/provenance-<target>.tsv`).
2. A row measured at an earlier hod than the candidate says so (the page
   is generated with `--candidate C`: its `at the candidate` cell is
   `no: measured before the candidate`, or the carried-over text of
   rule 5); it is never presented as measured at the candidate.
3. The candidate re-runs the strict build and tests (`--modes strict
   --strict`) of as many rows as fit, in this order: LLVM and Clang,
   DuckDB, gRPC, Boost, abseil, then the rest.
4. A control (kiln + `llc`) is reused when kiln's pin and the project's
   revision are unchanged: it does not depend on hod.
5. A row measured at the previous candidate stands for the candidate
   (it is carried over) when all of these hold: the owner agreed to it
   for that move of the candidate; the row's result had no refusal, no
   decline, no link failure and no test difference against its control;
   the frozen hod binaries of the two candidates emit byte-identical
   objects over the whole corpus on both targets, plain and PIC (and,
   where a project's modules or objects of both builds exist, over
   those too); everything the change between the two commits touches is
   measured again at the candidate; and the row names the commit it was
   measured at and says that it is carried over (the page is generated
   with `--carried-from <previous candidate>`: its `at the candidate`
   cell is `carried over from <commit> (see release.md)`). This was used
   once, for the move from `e98267c0` to `cd6ef6bc` (the owner's
   decision of 2026-11-15); `docs/status/release.md` lists the rows and
   the evidence.

## 6. Random programs (L10)

The budget (seeds per target, profile, mode and LLVM major) is
`docs/VALIDATION.md` "Random programs (L10)", "The budget" (WP-241); the
one command, with the frozen hod and fresh seeds:
`cargo xtask fuzz --budget --seed <first seed> -j 4 --hod <frozen hod> --out <dir>`
(62 slices, 26,250 seeds; the first run took 4 h 44 min, `fuzz.md`; it
is resumable). Each candidate takes seeds no earlier run used: 1.0.0
started at 200001, 1.1.0 at 300001, the next candidate starts at 400001. Its harness is of `64b45de6` (WP-249) or later, which sets
apart the three classes of the first run on their own evidence
(`ub-float-cast`, `selector-split`, `llc19-int128-last-register`). **Page:**
`docs/status/fuzz.md`, rewritten for `C`. **Pass:** the budget's run on
`C` has no finding, and every finding of earlier runs is closed (fixed with
a seed test, or recorded as not hod's with its evidence).

## 7. The status pages, regenerated

"Regenerated" means each page below states `C` and `K` (or says why it
does not depend on them) and its numbers are of runs at `C`.

| page | how |
|---|---|
| `support.md` | `HOD_BLESS=1 cargo test -q -p hod_ir --test classify_support_doc`; the test fails while the page differs from `crates/hod_ir/src/classify/matrix.rs`, so a gated commit has it current |
| `corpus.md`, `corpus-x64-census.md` | `cargo xtask corpus census` and `cargo xtask corpus census --target x86_64` (generated; they change only when the corpus is captured again, that is, with `K`) |
| `corpus-x64.md`, `corpus-x64-differences.tsv` | hand-written record of the x86-64 capture (WP-139) and its later recaptures; rewrite when the corpus is recaptured |
| `reasons.md`, `reasons-x64.md` | `cargo xtask corpus reasons --llc --hod <frozen hod> -j 3 --report docs/status/reasons.md`, and with `--target x86_64 --report docs/status/reasons-x64.md` after 3.2 (the x86-64 page reads the hybrid `corpus verify --target x86_64` results); `--llc` also times `llc` on each module, without it the pages lose their timing tables. The generated pages name no commit, so `release.md` records their hod, kiln and date (regenerated for `02fe2d02` by WP-257, for `a68a0d5d` with release 1.1.0). Their times are of processes started from the build volume: the timing that counts is `perf.md` |
| `debug.md`, `debug-x64.md` | section 3 |
| `libcxx-baseline.md`, `libcxx-baseline-x64.md` | 4.1, 4.2 (only when `K` or the suite moved) |
| `libcxx-hod.md`, `libcxx-hod-x64.md`, `libcxx-hod-debug.md`, `libcxx-hod-debug-x64.md` | 4.3 to 4.7 |
| `oss.md` | after section 5.1, from the hod checkout (`B` is the build root): `source scripts/env.sh && B=$(dirname "$CARGO_TARGET_DIR") && python3 -I scripts/oss/oss-report.py status --root $B/oss --root $B/oss3 --root $B/oss4 --root $B/oss6 --root $H/aarch64 --provenance scripts/oss/provenance-aarch64.tsv --candidate C [--carried-from P] --out docs/status/oss.md` (one `--root` per native lane root; the newest report of a build wins). `$H` holds symlinks to a hosted `oss.yml` run's downloaded artifacts in the generator's layout, the artifacts unchanged: `$H/aarch64/build/<project>` to `oss-aarch64-<project>/build/<project>`, `$H/x86_64/x64-build/<project>` to `oss-x86_64-<project>/build/<project>`; at `cd6ef6bc`: run 43779000922 (`.build/findings/hosted-oss-43779000922/`), the 15 projects on aarch64, re2 on x86_64, `H=$CARGO_TARGET_DIR/hosted-roots`; the provenance file maps the job's binary `hod` to its commit |
| `oss-x64.md` | after section 5.2: `source scripts/env.sh && B=$(dirname "$CARGO_TARGET_DIR") && python3 -I scripts/oss/oss-report.py --target x86_64 status --root $B/oss --root $B/oss2 --root $B/oss3 --root $B/oss5 --root $B/oss6 --root $H/x86_64 --provenance scripts/oss/provenance-x86_64.tsv --candidate C [--carried-from P] --out docs/status/oss-x64.md` |
| `oss-llvm.md` | section 5.4, by hand from the LLVM lane's records |
| `fuzz.md` | section 6 |
| `perf.md` | not a condition of a release (`docs/VALIDATION.md` "Release candidate"); when it is rerun for a candidate (1.1.0: WP-357), hod, its inputs and its outputs are on the machine's own disk, not on the build volume, whose cost of starting a process is larger than hod's time on a small module |
| `release.md` | the checklist: each row's state moved to `met at C` with its page |

## 8. The version (only on the owner's word)

When `docs/status/release.md` has every row met at C, by a marked
carried-over row (section 5), or in the tagged commit, and the owner has
said, in so many words, to cut 1.0:

1. `Cargo.toml` (workspace root), `[workspace.package]`:
   `version = "0.0.0"` becomes the release's version. Every crate's
   manifest (`crates/*/Cargo.toml`, `xtask/Cargo.toml`, 19 in all) says
   `version.workspace = true` and needs no edit.
2. `Cargo.lock`: run `cargo update --workspace` (or any build) so the 19
   workspace entries carry the new version; commit it.
3. Nothing else pins the number: `hod --version` prints
   `hod version <CARGO_PKG_VERSION>` (`crates/hod_cli/src/driver.rs`), and
   its tests (`crates/hod_cli/tests/cli.rs`, the driver's unit test)
   compare against `env!("CARGO_PKG_VERSION")`. `docs/status/perf.md`
   records the version it measured (`hod version 0.0.0`) and is left as
   measured. `hod::EMBED_API` (`spec/cli.md` "Embedding") versions the
   embedding surface on its own and does not move with the release.
4. `README.md`: its "Status:" line says what the release is.
5. The gates of section 1 again on the bump commit (it changes only the
   version, so the evidence of `C` carries over to it).
6. The tag (`v<version>` on the bump commit) is made by the owner, or on
   the owner's word, and by no one else. (1.0.0: the hosted CI run of the
   bump commit found a defect of a test's driver, WP-280; its fix
   follows the bump commit and the tag is on that last commit, after
   the gates on it.)
