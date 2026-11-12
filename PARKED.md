# WP-020 parked (bitcode on the text path)

Parked on the supervisor's instruction (token quota), 2026-11-12. Branch
`wp/020-bitcode`, worktree `/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-020`.
The work is functionally complete; what is missing is one clean run of
the gate, a final read of the documents, scratch cleanup and the handback.
Delete this file in the commit that finishes the package.

## Done (code, tests, documents all written)

- `crates/hod_cli/src/convert.rs` (new): `run(llvm_dis, bitcode)` runs
  `llvm-dis -o - -` over pipes; `Failure` has the five reasons
  `bitcode-converter-unavailable|killed|failed|stderr|output`.
- `crates/hod_cli/src/args.rs`: `--bitcode-via=<llvm-dis>`.
- `crates/hod_cli/src/driver.rs`: `HOD_BITCODE_VIA`, `SETTINGS`, the
  conversion step in `Job::run` (only when `compile` declines with
  `bitcode-input` and a converter is set), `fallback_end`.
- `crates/hod_cli/src/coverage.rs`: fields 9 (conversion microseconds)
  and 10 (fallback status: code, `signal:<N>`, `unknown`, `-`).
- `xtask/src/shim.rs`: `bin/llc` sets `HOD_BITCODE_VIA` to the
  installation's `llvm-dis` (empty when it has none).
- `xtask/src/irdiff/engine.rs`: `read_coverage` accepts eight fields or
  more; unit test `coverage_log_reading` updated.
- Tests: `crates/hod_cli/tests/cli.rs` (20 tests), `kiln_e2e.rs` (bitcode
  cases, every installed LLVM), unit tests in `convert.rs`, `driver.rs`,
  `coverage.rs`, `args.rs`, `shim.rs`.
- Documents: `spec/cli.md` (new section "Bitcode conversion", ten-field
  coverage log), `spec/harness.md` "shim", `spec/landed/cli/WP-020.md`
  (decisions and all measurements), `docs/design/0001-input.md` point 6.

## Last test and gate results

- `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick` was run ONCE and FAILED in
  the `test` step (fmt and clippy passed): `cli.rs`
  `bitcode_is_converted_and_goes_on_as_text` saw `HOD_REQUIRE_TOOLS=1` in
  the fake converter's recorded environment. Log:
  `/Volumes/AgentWork/hod-helper/.build/wp020-gate-impl.log`.
- Fixed afterwards: `FakeDis::new` in `cli.rs` now records only hod's own
  five variables. After the fix, `HOD_REQUIRE_TOOLS=1 cargo test -p hod_cli
  --test cli` passed (20 passed). `cargo fmt --all` was run after the fix.
- NOT re-run after that fix: clippy, the rest of the workspace tests
  (xtask integration tests `irdiff_fakes`, `irdiff_real`, `corpus_fakes`,
  `jobserver` were never reached by the failed gate), and the irdiff step.
- Before the gate, these passed: `cargo clippy --workspace --all-targets
  -- -D warnings`; `cargo test -p hod_cli` (30 + 20 + 1); `cargo test -p
  xtask --lib` (85).

## What remains, in order

1. `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-020 && source
   scripts/env.sh`, then in the background:
   `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick >
   /Volumes/AgentWork/hod-helper/.build/wp020-gate-impl.log 2>&1`.
   It must end with `ci: ok`. Fix whatever it reports.
2. One wording fix in `spec/landed/cli/WP-020.md`, bullet "A warning is a
   failure": "in the cases LLVM has today, dropped something" is too
   strong; say that LLVM strips debug info it finds invalid with such a
   warning, and hod cannot tell what a warning means.
3. Empty the scratch by explicit path (never a glob `rm`):
   `find /Volumes/AgentWork/hod-helper/.build/target-wp-020/scratch -mindepth 1 -delete`
   then `rmdir` it, and `rm -f
   /Volumes/AgentWork/hod-helper/.build/target-wp-020/build-release.log`.
   KEEP `/Volumes/AgentWork/hod-helper/.build/target-wp-020/CACHEDIR.TAG`
   (without it test scratch directories land in `/tmp`). The raw
   measurement rows are `scratch/rows22.tsv` and `rows19.tsv`; their
   summary is already in `WP-020.md`.
4. `git rm PARKED.md`, commit (message ends with the `Co-Authored-By:
   Claude Fable 5.2 <noreply@anthropic.com>` line), do not push, do not
   merge `main`.
5. Handback: see "For the handback" below.

## Decisions made, and why (details in spec/landed/cli/WP-020.md)

- hod runs the converter itself over pipes; no temporary file.
- Conversion only when `compile` answers `bitcode-input`: no change to
  `crates/hod/src/lib.rs` was needed.
- `llvm-dis` exit 0 with anything on standard error is a failure
  (`bitcode-converter-stderr`): fall back with the original bitcode.
- Output must start with `; ModuleID = ` and end with a newline.
- No time limit on the converter; it is always waited for.
- The converter child gets no hod settings and `HOD_IN_FALLBACK=1`
  (reused as the loop guard; no second variable).
- Field 10 is `signal:<N>` for a signal, not `128+N`.
- `read_coverage` accepts >= 8 fields, NOT >= 10: `DECLINE_SCRIPT` in
  `xtask/src/corpus/capture.rs` and the fake hods in `xtask/tests/*.rs`
  write eight fields and are not this package's files. An earlier edit to
  `xtask/tests/irdiff_fakes.rs` was reverted for the same reason.
- The shim's `sh` entry was measured (+0.3 ms min, +0.8 to +1.2 ms
  median per invocation) and LEFT: `cargo xtask corpus capture` relies on
  the script exporting `HOD_FALLBACK_LLC` to its stand-in for hod.

## Ruled out

- Converting in the shim script (cannot give the fallback the original
  bytes, not in the coverage log).
- `bin/llc` as a symbolic link to hod (breaks corpus capture, see above).
- Requiring ten fields in `read_coverage` (breaks corpus admission).

## For the handback

- Headline measurement: conversion has an 8 to 10 ms floor (starting
  `llvm-dis`); it is 55% to 83% of an entire `llc -O0` run on modules
  under 64 KB of bitcode and 22% to 65% on standard-library-heavy ones
  (150 KB to 3 MB). The native bitcode reader is urgent for speed.
- Non-bitcode overhead unchanged: hod's own time min 156 -> 148 us,
  median 358 -> 360 us.
- Until code generation is wired in (WP-043) the shim's conversion is
  pure cost on every bitcode translation unit; there is no switch to
  turn it off through the shim. Open question for the supervisor.
- Follow-ups: `xtask/src/irdiff/engine.rs:61` (`HOD_VARIABLES` lacks
  `HOD_BITCODE_VIA`); eight-field stand-ins at
  `xtask/src/corpus/capture.rs:58`, `xtask/tests/irdiff_fakes.rs:70`,
  `xtask/tests/irdiff_real.rs:176`, `xtask/tests/jobserver.rs:51`,
  `xtask/tests/corpus_fakes.rs:60`; the corpus could now admit
  `opt-first-*` units as text.
