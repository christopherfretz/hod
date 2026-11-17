# WP-343 parked (stack-protected coroutine bodies, 64-bit)

## Done (committed)
- Tests `crates/hod/tests/coro_protected.rs` with `crates/hod/tests/data/coro/`
  (`protected.cpp`, `overflow.cpp`, `victim.ll`); all 3 pass on this host:
  protected.cpp 32 runs per target, overflow.cpp 48 per target, victim.ll 48
  per target, every hod / stress run equal to llc's.
- Fix: `crates/hod_cg/src/driver.rs` `enter_arguments`: the WP-333 copy of
  stack-passed arguments below the guard now applies to coroutine bodies too
  (the `self.coro.is_none()` condition is gone).
- Spec: `spec/codegen.md` §5.6.24 rule 10 (coroutine sentence), new rule 11
  (llc's measured rule for coroutines).

## Before the change (measured by hand with a hod CLI built at f20f08b6)
- victim.ll overflow, x86-64 and AArch64: hod SIGSEGV (store through the
  overwritten 9th argument, read in place above the guard); llc SIGABRT
  ("stack smashing detected"). In bounds equal.
- overflow.cpp (ramp and resumed-activation overflows): already equal to llc
  on both targets (stubs already write the guard, `guard::stub_enter`).

## Remaining, in order
1. Before-state as a counted test run: put `&& self.coro.is_none()` back in
   `enter_arguments` temporarily, `cargo test -q -p hod --test coro_protected`,
   record the failures (expect only `a_ramp_reads_its_stack_arguments_below_the_guard`),
   revert.
2. Base build (source already extracted at
   `/Volumes/AgentWork/hod-helper/.build/target-wp-343/base-src`, = f20f08b6):
   `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-343 && source scripts/env.sh && cd /Volumes/AgentWork/hod-helper/.build/target-wp-343/base-src && cargo build -q -p hod_cli --target-dir /Volumes/AgentWork/hod-helper/.build/target-wp-343-base`
   and head: `cargo build -q -p hod_cli` in the worktree.
3. `cargo xtask compare-binaries --base /Volumes/AgentWork/hod-helper/.build/target-wp-343-base/debug/hod --head /Volumes/AgentWork/hod-helper/.build/target-wp-343/debug/hod --corpus tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln -j 4 --out /Volumes/AgentWork/hod-helper/.build/target-wp-343/cmp`
   (background). Expect no corpus object to change (the kiln corpus has no
   coroutine at all); differences only in tests/ir modules with a protected
   coroutine having a stack-passed argument (probably none). i686: 0.
4. `cargo xtask irdiff` x86_64 and aarch64, LLVM 22 and 19: 0 FAIL / 0 ERROR,
   counts equal to base (480 / 470 with LLVM 22); no tests/ir file was added.
5. `cargo clippy --workspace --all-targets -- -D warnings`;
   `cargo test -q -p hod_cg`, `-p hod` (at least `--test coro_protected
   --test stack_protector --test stack_protector_stack_args --test coro_preopt`).
6. `spec/landed/codegen/WP-343.md` (the record: finding, llc's rule, change,
   evidence); then delete the base target dir by its literal path and
   `base-src`, and this file.

## Decisions
- No refusal added: nothing found that could not be made equal.
- `ssp` plain is in the matrix: llc guards every split function there too.
- Tests' host-conditional parts: none use `cfg!(target_arch)`; the lane picks
  native or emulator by `Target::is_host()`.
