# WP-346 parked (implementer at the context cap)

Head: see `git log -1` (code commit `a1ab662e`, then README row + this file).

## Done
- `crates/hod_x86/src/wideint.rs`: i65..i128 (3/4 limbs) add/sub/mul/and/or/xor,
  icmp, select, freeze, casts, load/store, constants, six with.overflow.
- Dispatch: `int.rs` entry points, `agg.rs` overflow/check_type/insert_value,
  `abi.rs` type_refusal. Spec §22 + §7/§8/§21 text. README row.
- `crates/hod/tests/i386_wide_int.rs`: 36 + 36 runs equal llc's; the refusal test
  was fixed afterwards (srem i96 -> i128, odd width is the classifier's
  `inst:srem/int-wide`). Re-run it.
- `crates/hod/tests/i386_i64.rs:118-125`: i128 add -> udiv, asserts `x86:i64:wide:div`.

## Result of the i386 run (t2.log)
14 binaries ok, then `i386_i64_rest.rs` `wider_than_64_bits_is_refused_by_name`
failed (panic at :176): its last two cases (`getelementptr` with an `i128` / `i96`
index) now COMPILE by design (low limb, as llc). Fix: drop those two cases from
the list in `crates/hod/tests/i386_i64_rest.rs:154-172` (or move them to an
"index compiles" assertion). cargo stopped at that binary: re-run step 1 with
`--no-fail-fast`; the other 18 binaries have not run.

## Remaining, in order (each: `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-346 && source scripts/env.sh && export CARGO_TARGET_DIR=/Volumes/AgentWork/hod-helper/.build/target-wp-346 &&` ...)
1. Every i386 test binary (a run was started; its log is
   `/Volumes/AgentWork/hod-helper/.build/target-wp-346/t2.log`, last line `exit N`):
   `HOD_REQUIRE_TOOLS=1 cargo test -q -p hod $(ls crates/hod/tests | grep -E '^i386.*\.rs$' | sed 's/\.rs$//;s/^/--test /')`
2. `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
   `cargo test -q -p hod_x86 -p hod_ir` (abi.rs unit test changed: i128 -> `x86:i64:wide:abi`, i200 -> `:width`).
3. `cargo xtask irdiff --target i686 --llvm 22` and `--llvm 19`: 0 FAIL / 0 ERROR (base 171 PASS).
4. compare-binaries: `git archive b9d4aad4 | tar -x -C /Volumes/AgentWork/hod-helper/.build/target-wp-346-base-src`,
   build hod_cli there with `--target-dir /Volumes/AgentWork/hod-helper/.build/target-wp-346-base`, then
   `cargo xtask compare-binaries --base <base>/debug/hod --head /Volumes/AgentWork/hod-helper/.build/target-wp-346/debug/hod --corpus tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln --out /Volumes/AgentWork/hod-helper/.build/target-wp-346/cmp`.
   Expect 64-bit targets 0 different; i686 only "refused x86:i64:wide -> compiles / narrower name".
   Delete both base dirs by literal path afterwards.
5. Record `spec/landed/codegen/WP-346.md` (measurements are in spec §22).

## Decisions
- Bits above N in a slot unspecified (as §8); memory image = store size, zero above N (llc's).
- Odd-width overflow = top limb not its own sign/zero extension (llc: sext_inreg compare).
- mul via an exact 2L-limb product in a frame area; llc 19/22 emit no call or branch
  for wide mul / mul.with.overflow (measured generic and i686), so no x87 barrier.
- `x86:i64:wide:index` not needed: a wide ptradd index uses the low limb (as llc).
