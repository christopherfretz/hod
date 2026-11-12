# WP-023 parked (irdiff follow-ups, part 1)

Parked on the supervisor's instruction (token quota), mid-package. This file
is for whoever resumes: read it top to bottom, then the brief. Delete this
file in the final commit of the package.

Branch `wp/023-irdiff-reference`, worktree
`/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-023`, base `0db8525`.
Before any cargo command: `cd <worktree> && source scripts/env.sh`.
Everything below is in the one WIP commit on top of the base. Nothing is
pushed.

## State in one paragraph

All four items of the brief are implemented in code, on the `irdiff` side
and on the `corpus capture` side, with fake-tool tests. What is missing is:
running the new real-tool tests, the real capture (it was stopped during its
first group), `corpus verify`, the regenerated `docs/status/corpus.md`, the
plain-`-O0` experiment, every spec and doc edit, the landed file, and the
gate. No spec file has been touched yet, so `spec/harness.md` does NOT agree
with the code right now.

## What last passed, and what was never run

Passed (last run after all source edits except the ones marked below):

- `cargo test -p xtask --lib`: 89 passed.
- `cargo test -p xtask --test irdiff_fakes`: 24 passed.
- `cargo test -p xtask --test corpus_fakes`: 6 passed.
- `cargo fmt --all --check`: clean. `cargo check -p xtask --tests`: clean
  (after removing one `super::` qualification in `capture.rs`, a
  one-token edit made after the test runs above).

Never run:

- `cargo test -p xtask --test irdiff_real`. The file was edited after the
  runs above: five new tests were added and they have only been
  type-checked. The run was started and stopped before any result. Expect
  to adjust assertions in them (see "Half-done" below).
- `cargo test -p xtask --test corpus_real` since the capture changes. It was
  not edited; it may need its expectations updated (for example the
  manifest's `llc_args` value and the new `delivered` column).
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo xtask ci --quick` (the gate), with or without `HOD_REQUIRE_TOOLS=1`.
- `corpus verify`, `corpus census`, and a complete `corpus capture`.

## State of `/Volumes/AgentWork/hod-helper/.build/corpus-wp023`

Empty directory. The first real capture was stopped while admitting the
first group (kiln, LLVM 22): it had captured 1434 candidates of 1540 kiln
tests and had written nothing to the corpus yet. Its work directory
`/Volumes/AgentWork/hod-helper/.build/corpus-wp023.work` was emptied. No
numbers exist for entries gained or lost against the shared corpus's 5,760.
The shared corpus `/Volumes/AgentWork/hod-helper/.build/corpus` was never
written to.

Scratch `/Volumes/AgentWork/hod-helper/.build/target-wp-023/`: `exp/` and
`tmp/` were emptied; the small logs `build0.log`, `unit.log`, `fakes.log`,
`cfakes.log`, `capture1.log`, `real.log` remain and can be deleted. The
jobserver pool showed 12 of 12 tokens idle after the stop.

## Done (code and fake-tool tests)

Item 1, the reference compiler:

- `xtask/src/irdiff/engine.rs`: `REFERENCE_SELECTOR = ["-global-isel=0",
  "-fast-isel=0"]` is added to the oracle's `llc -O0` on every target; hod
  still gets plain `-O0 ...`. In Hybrid mode the harness writes a
  `<work>/reference-llc` wrapper and passes it as hod's `--fallback=`, so
  the fallback builds as the reference does.
- Witness: `Session.witness`, `Config.witness`, option `--witness`
  (`xtask/src/irdiff.rs`), summary suffix `; witness`. The witness is
  `llc -O2`; it is linked and run once per test, with the first linker
  whose oracle program is what the directives say. A disagreement, a
  witness build failure or a witness link failure is an ERROR whose detail
  starts with `witness: ` (`engine::WITNESS`), for that linker and every
  later one. `xtask/src/main.rs` `irdiff_gate` turns it on for the full
  (non-quick) gate.
- Seed tests `tests/ir/selector_narrow_load.ll`,
  `tests/ir/selector_i1_compare.ll` (the two `spec/interp.md` §10.1
  reproducers) and `tests/ir/two_modules.ll`. The seed corpus is now 32
  tests.
- Capture admission runs with the witness; a refused candidate whose ERROR
  detail starts with `witness: ` is skipped with the new reason
  `oracle-unconfirmed` (`xtask/src/corpus/capture.rs`, `Refused`,
  `admitted`). The kiln capture script (`xtask/src/corpus/kiln.rs`
  `CAPTURE_SCRIPT`) forwards to the real `llc` with the selector arguments.

Item 2, directives (`xtask/src/irdiff/directives.rs`):

- `;@ llc-args: <args>`: allow-listed arguments that the oracle, the
  witness and hod all get. Allow-list `LLC_ARGS`: `-code-model=`,
  `-data-sections`, `-debugger-tune=`, `-fp-contract=`,
  `-function-sections`, `-mcpu=`, `-x86-branches-within-32B-boundaries`.
- `;@ link: ... allow-warnings`: the link is done without
  `-Wl,--fatal-warnings`. Capture sets it only after a first admission pass
  is refused for an oracle link failure and a second pass with the flag is
  admitted.
- `;@ stdout-no-newline: <text>`: the end of an output that has no final
  newline; at most one, and no `stdout` after it.
- `read_coverage` and its unit test `coverage_log_reading` are untouched
  (WP-020 owns them).

Item 3, bitcode: `capture.rs` `module_of_call` converts a module that
reached `llc` as bitcode with the group's `llvm-dis` (`Group.llvm_dis`,
found with `Search::versioned("llvm-dis", major)`, output written to
`<job>/module.ll` because of the 16 MiB output cap). The entry carries
`; delivered: bitcode`, the manifest a `delivered` column. Failure is the
new skip reason `llvm-dis-fails`; without the tool the test keeps its old
`opt-first-*` reason. `opt-first-lto` is unchanged.

Item 4, several modules:

- Directive `;@ module: <name>` starts a module; everything before the
  first one is directives only (`directives::modules`, `Module`). The
  engine compiles each module (`<dir>/modules/<name>.ll`, `.oracle.o`,
  `.witness.o`, `.hod.o`, `.coverage.tsv`), links the objects in file
  order, runs once. One declined module settles the test as a decline.
- Capture: kiln's two-step build is replayed (`kiln::Plan::two_step`,
  `capture.rs` `kiln_calls`) for `module:` and `unit:` tests; kiln's
  one-command multi-input builds are captured when the objects' `<n>-`
  prefix gives the link order (`kiln::in_link_order`); torture `builtins/`
  tests are captured as `test`, `lib`, `main` (`torture.rs`, `TEST_MODULE`).
  Still skipped as `multi-file` by design: `pch:` tests, modules given
  different llc arguments, module name clashes, unknown link order.
- Corpus format is now `hod-corpus-2`; `MANIFEST.tsv` has 12 columns
  (`delivered` after `llc_args`); the old 11-column header is still read.
- Census (`xtask/src/corpus/census.rs`, `census/report.rs`): each module of
  an entry is scanned on its own; `REASONS` has 33 rows, `FAILURES` 9; new
  table "Modules of an entry".

## Half-done

`xtask/tests/irdiff_real.rs`: five new tests, compiled, never run:
`the_reference_is_not_plain_o0`,
`the_witness_confirms_the_reference_and_catches_plain_o0`,
`llc_args_reach_every_compiler`,
`a_link_that_warns_and_an_output_without_a_newline`,
`a_real_program_of_two_modules`. Helpers added: `selecting_hod`,
`seed_test`, `llvm_without`, `report`, `printed`, `contains`. Things most
likely to need a fix on the first run:

- `a_real_program_of_two_modules` asserts the summary contains
  `hod built 2 of 2 objects`; the coverage line may only be printed in
  Hybrid mode or may count differently. Check `report.rs` line ~315.
- `llc_args_reach_every_compiler` and the kept-directory lookups assume a
  failing run keeps `oracle.o`, `witness.o`, `hod.o` at the top of the
  test's directory, and that the witness object exists when hod's side
  FAILs.
- `a_link_that_warns_...` relies on glibc's link-time warning for `tmpnam`:
  checked by hand that `ld.bfd` fails it under `--fatal-warnings` and
  `mold` links it silently; `lld` was not checked.

## What remains, in order

1. Run `cargo test -p xtask --test irdiff_real` (background, with a log);
   fix the new tests.
2. `xtask/tests/corpus_real.rs`: run it; then add a `builtins/` program
   (`builtins/x.c`, `builtins/x-lib.c`, `builtins/lib/main.c`), a program
   whose output has no final newline, and the `tmpnam` program (it should
   be admitted with `link: allow-warnings` because only `ld.bfd` refuses
   it).
3. `cargo clippy --workspace --all-targets -- -D warnings`.
4. Real capture, in the background, about 4 to 5 minutes per group without
   the witness and more with it:
   `cargo xtask corpus capture --out /Volumes/AgentWork/hod-helper/.build/corpus-wp023 -j 3`.
   Read the per-group counts and the skip reasons; look hard at the
   two-step (`module:`/`unit:`) tests, the bitcode entries, the `builtins/`
   entries, `oracle-unconfirmed` and `allow-warnings`, because none of that
   has met the real kiln yet. Code changes do not change cache keys: use
   `--force` or `--retry-skipped` to recapture after a fix.
5. `cargo xtask corpus verify <that dir> -j 3` (background), then
   `cargo xtask corpus census <that dir>` to regenerate
   `docs/status/corpus.md`.
6. The one-off experiment for the landed file: over a COPY of the entries
   in scratch, run `irdiff --hybrid --hod <stand-in>` where the stand-in
   ignores `--fallback`, builds with plain `llc -O0` and writes a
   `fallback` coverage line; `-j 3`, background. A FAIL is a program that
   behaves differently between plain `-O0` and the reference. List them by
   name, with a one-line diagnosis for the first few.
7. Specs and docs (nothing written yet): `spec/harness.md` sections
   "irdiff" (Options `--witness`, "What a test does", the oracle and the
   witness, Outcomes, the Hybrid wrapper, work directory, "In the gate",
   seed corpus table with three new rows), "Test directives" (`llc-args`,
   `stdout-no-newline`, `module`, `link: allow-warnings`, several modules,
   canonical form; the first `llvm` block must still equal
   `tests/ir/main_args.ll`'s directives) and "corpus" (capture script,
   Admission with witness and the warning retry, Skip reasons, Entries with
   `; delivered:`, bitcode, several translation units, manifest column and
   format, census table). `xtask/src/corpus.rs` USAGE if it needs a line.
   `docs/VALIDATION.md` "Oracles": replace the sentence "(WP-023 moves
   `xtask irdiff` to this; until then it runs plain `-O0`)".
   `spec/landed/harness/WP-023.md`: what landed per item, the x86-64
   evidence, the experiment's list, capture and verify numbers, gained and
   lost against 5,760 by reason.
8. Gate, in the background:
   `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`
   with log `/Volumes/AgentWork/hod-helper/.build/wp023-gate-impl.log`;
   then `scripts/jobserver.sh status` must show every token idle.
9. Delete this file, commit (message ends with the attribution line of the
   brief), empty the scratch, hand back in the brief's format.

## Decisions and why

- x86-64 gets the same `-global-isel=0 -fast-isel=0` as AArch64. No
  miscompile is known there, but FastISel is the same `-O0`-only design,
  SelectionDAG is what the `-O2` witness uses, and `-global-isel=0` is a
  no-op on x86-64. Evidence gathered: the 31 single-module seed tests
  behaved identically under `qemu-x86_64` with LLVM 22 and 19.
- The reference needs BOTH arguments. On AArch64 with LLVM 22 and 19:
  the narrow load prints `-5408` with plain `-O0` (GlobalISel) and `60128`
  with SelectionDAG and `-O2`; the i1 compare prints `0` (right) with
  GlobalISel, `1` with `-global-isel=0` alone (FastISel), `0` with
  SelectionDAG and `-O2`. So plain `-O0` fails only the first reproducer;
  the second is failed by `-global-isel=0` alone. Say this exactly in the
  handback: the brief's "plain `-O0` would have failed them" holds for one
  of the two.
- A witness disagreement is its own ERROR detail and never a hod failure;
  directives are checked first, so a wrong test is reported as a wrong
  test.
- The real-tool tests for the selector bugs accept two results on AArch64:
  FAIL with the known wrong value, or PASS plus a note on stderr (written
  past the test harness's capture) that the workaround can be reconsidered.
- `entry::delivered` writes one word per module, so the census can count
  entries of several modules from the manifest alone.
- `corpus verify` does not use the witness (admission already did).

## Ruled out, and open questions for the handback

- No real program was found whose `-O0` and `-O2` builds legitimately
  differ without undefined behaviour; the real-tool witness test instead
  uses an oracle wrapper that drops the selector arguments.
- The real shim and kiln fallback (WP-020's files, not this package's)
  still run plain `llc -O0` and so inherit the selector bugs.
- The shared corpus (format `hod-corpus-1`) is readable by the new code,
  but every entry would be recaptured under the new format.
