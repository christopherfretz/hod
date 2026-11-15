# Cutting a release candidate

How to produce the evidence of the release-candidate gate
(`docs/VALIDATION.md` "Gates", "Release candidate"; `docs/ROADMAP.md` M6)
for one commit of hod, and what to change when the owner accepts it. The
checklist with the evidence on `main` today is
[`docs/status/release.md`](status/release.md).

**1.0 is** (the owner's definition, 2026-11-14): real projects with no
fallback whose tests equal the kiln + `llc` control; libc++ strict on both
targets at the candidate commit; the fuzzing budget with no open findings;
wild in the linker matrix; the status pages regenerated; the version bump.
**1.0 is not** conditional on the benchmark gate and the performance work,
the in-process bridge (both `docs/ROADMAP.md` M5, after 1.0), or the 32-bit
x86 target.

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
   toolchain) plus wild once `docs/BACKLOG.md` WP-239 has landed. Record
   each linker's version (`ld.bfd --version`, `ld.lld --version`,
   `mold --version`, `wild --version`) and the debuggers'
   (`gdb --version`, `lldb-19 --version`): `docs/VALIDATION.md`
   "Linker matrix" asks that status reports record them.

Times below are of the shared 16-core AArch64 development host under
load (average 20 to 30); a quiet machine is faster. Runs longer than a
minute go in the background. Every corpus-wide or suite-wide command
uses at most `-j 4`; real-project builds take the lanes' job pool
(`MAKEFLAGS=--jobserver-auth=fifo:/run/hod/lanes.fifo`, set by
`scripts/oss/oss-run.sh`).

## 1. The gates with every tool required

| | command | where | time | result | pass means |
|---|---|---|---|---|---|
| 1.1 quick gate | `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick` | AArch64 development host (x86-64 seeds under `qemu-x86_64`) | 90 to 125 min under load (mostly compiling the test binaries) | the log; no page | the log ends `ci: ok` and the exit status is 0 |
| 1.2 full gate | `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci` | the same host; it adds `irdiff --witness` with LLVM 22 and 19, `irdiff --hybrid`, `irdiff --target x86_64` with LLVM 22 and 19, the whole-corpus `roundtrip` (10 to 20 min alone), and `debug` (every debugger test of the host's target, then `debug verify`) | not yet timed (`docs/status/release.md`: no full gate is recorded); at least the quick gate plus an hour | the log; no page | `ci: ok`, exit status 0 |
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
`linkers bfd lld mold`). wild is the fourth (WP-239): once it lands, the
same gates name four linkers, and a follow-up makes wild required
(`linkers::WILD_REQUIRED` in `hod_testutil`); until then a missing wild is a
note, not a failure, and the condition is not met. **Pass:** the gate logs
of 1.1 and 1.2 name every one of the four linkers, and nothing is skipped
for a missing linker.

## 3. Kiln's corpus through hod (L6) and debug information (L7)

| | command | target | time | page | pass means |
|---|---|---|---|---|---|
| 3.1 L6 strict | `cargo xtask corpus verify --strict -j 3 --hod <frozen hod>` | AArch64, native | about the hybrid verify's (1291 to 1366 s per LLVM major on x86-64 under qemu, `docs/status/corpus-x64.md`) | `<build>/corpus/verify-llvm<N>-strict.tsv`; summary into `docs/status/release.md` | exit status 0: no FAIL, no XPASS, no ERROR, every entry's expectation met with every linker |
| 3.2 L6 strict, x86-64 | `cargo xtask corpus verify --target x86_64 --strict -j 3 --hod <frozen hod>` | x86-64 under `qemu-x86_64` (baseline CPU) | about 1300 s per LLVM major | `<build>/corpus/verify-x86_64-llvm<N>-strict.tsv`; then 4.3 fills `docs/status/reasons-x64.md` "What the built modules do" | as 3.1 |
| 3.3 L7 AArch64 | `cargo xtask debug` (sessions), `cargo xtask debug kiln`, `cargo xtask debug verify` | AArch64, native | minutes each | `docs/status/debug.md` (rewritten for `C`, `K`) | no FAIL, no XPASS, no crash, no stale `xfail`; kiln's debugger tests all pass; `--verify` clean for every object hod builds and nothing declined; line tables equal up to `spec/dwarf.md` §5.7; the `-g` corpus modules pass strict under every linker |
| 3.4 L7 x86-64 | `cargo xtask debug --target x86_64`, `cargo xtask debug trees` over the `-g` modules, `cargo xtask debug verify --target x86_64` | x86-64 under qemu's gdb stub | 1:18 for the sessions at `-j 4` (`docs/status/debug-x64.md`) | `docs/status/debug-x64.md` | as 3.3; SKIPs only for what the stub cannot do (`debug-x64.md` "What qemu's gdb stub cannot do") |

The full gate's `debug` step runs the host's target only, so 3.4 is a
separate run.

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
| 4.6 `-g`, AArch64 | `scripts/libcxx-hod-debug.sh control <dir>`, `HOD_SHIM=<shim> HOD_JOBS=4 scripts/libcxx-hod-debug.sh strict <dir>`, the report against the control | AArch64 | 7478 to 11214 s per run | `docs/status/libcxx-hod-debug.md` | as 4.3 against the `-g` control |
| 4.7 `-g`, x86-64 | `HOD_DEBUG_TARGET=x86_64`: the same three modes, in shards under qemu (`--shard K/N`, then `merge`) | x86-64 under qemu | about 9,000 s for a compile-only census; the whole suite executed is several times that | `docs/status/libcxx-hod-debug-x64.md` | as 4.6, over the whole suite (not one shard) |

kiln's own unpassing tests (the four filesystem tests as root, load
timeouts) fail in the control too; the comparison, not 100%, is the gate.
4.5 counts as the native run of both targets; 4.3 and 4.4 are the
development host's and are needed while 4.5 has no `-g` mode.

## 5. Real projects (L9)

| | command | target | time | page | pass means |
|---|---|---|---|---|---|
| 5.1 the lane | `source scripts/env.sh && scripts/oss/oss-run.sh <project>...` (each project of `scripts/oss/projects.toml`, both flavours `O0` and `O0g`, the three modes), then `python3 scripts/oss/oss-report.py status --out docs/status/oss.md` | AArch64, native | not recorded per project (three builds of each flavour, one at a time); build trees are pruned after each | `docs/status/oss.md` | every row: `fell back` 0, `not offered` 0, strict build `ok`, every object `hod` in "Strict builds", `against the control` `same` (or better), at `C` and `K` |
| 5.2 the lane, x86-64 | `scripts/oss/oss-run.sh <project>... --target x86_64`, then `python3 scripts/oss/oss-report.py --target x86_64 census --out <findings>/X64-CENSUS.md` | x86-64, cross-built, tests under qemu | as 5.1 and more | today only `spec/landed/harness/WP-191.md`; the release needs it in `docs/status/` | as 5.1; a test that runs an x86-64 program outside ctest fails in the control too and is compared, not counted |
| 5.3 hosted | `.github/workflows/oss.yml` by hand (inputs: kiln's commit, the rows) or on an `oss/**` branch | both runners natively | per row | the run's artifacts | as 5.1, on both runners natively |
| 5.4 LLVM and Clang | `scripts/oss/llvm/` (native: `configure.sh`, `build.sh`, `strict.sh`, `unittests.sh`, `lit.sh`, `census-file.sh`) and `scripts/oss/llvm-x64/` (cross) | AArch64 native; x86-64 cross under qemu | hours per tree; about 21 GB of disk for a pair | `docs/status/oss-llvm.md` | every TU offered to hod and built by it in the strict build, and the tests identical per test with the kiln + `llc` control |

The project set is kiln's (`kiln/scripts/oss/projects.toml`, rows copied
unchanged into `scripts/oss/projects.toml`); `docs/status/release.md`
lists which rows have a measurement.

## 6. Random programs (L10)

The budget (seeds per target, profile, mode and LLVM major, with fresh
seed ranges for each candidate) and the one command that runs it are
WP-241's to define in `docs/VALIDATION.md` "Random programs (L10)"; until
then the unit is `cargo xtask fuzz --seed <S> --count <M> --target <T>
[--llvm 19] [--profile <P>] [--mix | --debug] -j 4` per cell. Run with
the frozen hod on both targets and both LLVM majors. **Page:**
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
| `reasons.md`, `reasons-x64.md` | `cargo xtask corpus reasons --hod <frozen hod> -j 3 --report docs/status/reasons.md`, and with `--target x86_64 --report docs/status/reasons-x64.md` after 3.2 |
| `debug.md`, `debug-x64.md` | section 3 |
| `libcxx-baseline.md`, `libcxx-baseline-x64.md` | 4.1, 4.2 (only when `K` or the suite moved) |
| `libcxx-hod.md`, `libcxx-hod-x64.md`, `libcxx-hod-debug.md`, `libcxx-hod-debug-x64.md` | 4.3 to 4.7 |
| `oss.md`, `oss-llvm.md` | section 5 |
| `fuzz.md` | section 6 |
| `perf.md` | not a 1.0 condition: stays as last measured (`docs/VALIDATION.md` "Release candidate") |
| `release.md` | the checklist: each row's state moved to `met at C` with its page |

## 8. The version (only on the owner's word)

When `docs/status/release.md` has every row `met at C` and the owner has
said, in so many words, to cut 1.0:

1. `Cargo.toml` (workspace root), `[workspace.package]`:
   `version = "0.0.0"` becomes the release's version. Every crate's
   manifest (`crates/*/Cargo.toml`, `xtask/Cargo.toml`, 18 in all) says
   `version.workspace = true` and needs no edit.
2. `Cargo.lock`: run `cargo update --workspace` (or any build) so the 18
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
   the owner's word, and by no one else.
