# WP-352 parked (the i686 debug lane to green)

Tree `hod/.worktrees/wp-345`, branch `wp/345-debug-lane-i686-b`; build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-345`. Every cargo command:
`cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-345 && source scripts/env.sh && ...`

## Done (committed)

- Class A: `crates/hod_cg/src/dbg_value.rs` `value_at`: pointer home size and
  null constant width from `module.target().data_layout().pointer(0).bits`.
  `dbg_value_arglist.c` i686: 6/6 PASS.
- Class B: `crates/hod_x86/src/mem.rs` `ptr_add`: a `ptradd` of a guarded-block
  slot (`guarded_slot`) at constant indices emits under `LocId::NONE` (no row
  of its own, like `llc`'s frame-index materialization); spec `spec/dwarf.md`
  §5.8. `stack_protector.c` i686: 12/12 PASS. Instruction bytes unchanged by
  construction (only `line_switch`), NOT yet proven by compare-binaries.
- Class C/4: `xtask/src/debug/rows.rs` (`ProgramRows`, `return-row` gdb and
  lldb, `entry-row` lldb), wired in `sessions.rs` for i686 only; spec
  `spec/harness.md` "Permitted differences of sessions", `spec/dwarf.md` §5.7.
  Unit tests `debug::rows::tests` (4) pass. Measured: inlined.cpp gdb (6) PASS
  `return-row`; key_call.c clang-22 gdb (3) PASS `return-row`;
  call_sites_direct.c lldb (3) PASS `entry-row`.
- Class E: `x86:probe-stack` in `sessions::KEPT_OFF_I686` (SKIP, named);
  spec harness i686 paragraph.

## Still failing

- key_call.c lldb (6, both clangs): the `finish` stop itself passes
  `return-row` (16:3 is a return row), but the following `next` stops at
  17:19 in hod against 18:10 in `llc` (hod's `next` starts from line 16).
  Per the supervisor's rule 2 a later stop is not covered: left FAILING.
  What is missing: either a codegen change (reserve the i386 outgoing
  argument area in the prologue, as `llc`) or a decision to permit the
  stop that follows a permitted `return-row` stop.
- Class D (3, mold gdb `<.L.str>`): no witness yet.

## Remaining, in order

1. Clippy and tests:
   `cargo clippy --workspace --all-targets -- -D warnings`;
   `cargo fmt --all --check`;
   `cargo test -q -p hod_cg -p hod_x86 -p xtask`.
2. Full i686 lane (background):
   `cargo xtask debug --target i686 -j 4 --results /Volumes/AgentWork/hod-helper/.build/target-wp-345/head-results.tsv > /Volumes/AgentWork/hod-helper/.build/target-wp-345/head.log 2>&1`;
   compare with `base-results.tsv` (384/34/57): no PASS becomes FAIL;
   expected remaining FAILs: key_call.c lldb 6, class D 3.
3. 64-bit lanes at head: `cargo xtask debug --target x86_64 -j 4`, then
   `--target aarch64`; totals equal to those in `spec/landed/dwarf/WP-342.md`
   (or of base `f20f08b6` if not recorded there).
4. compare-binaries: build base `f20f08b6` into
   `/Volumes/AgentWork/hod-helper/.build/target-wp-345-base` (own target dir,
   `git worktree`-free: `git archive f20f08b6 | tar -x -C <dir under the build dir>`),
   then `cargo xtask compare-binaries -j 4 --out <ABSOLUTE path>` head vs base over
   `tests/ir`, `/Volumes/AgentWork/hod-helper/.build/corpus/kiln`,
   `/Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln`: 64-bit 0 different;
   i686 differences only in debug sections (pointer `DIArgList` modules; guarded
   slot rows), `.text` identical; `llvm-dwarfdump --verify` on changed i686
   objects. Delete the base dir by its literal path afterwards.
5. Class D witness: `cargo xtask debug --target i686 -j 4 --keep --linker mold tests/debug/exception.cpp`;
   in the kept dir compare `llvm-readelf -s` of `m0.hod.o` and `m0.llc.o`
   (does `llc` keep a local `.L.str` symbol, or a section symbol hod lacks) and
   of the linked `hod-mold/prog` / `llc-mold/prog` (and bfd/lld: do they drop it).
   Fix in hod if hod lacks a local symbol by rule; else a rule with the witness.
6. Gate step `debug-i686` in `xtask ci` only when the lane shows 0 FAIL
   (`git grep -n "debug-x86_64" xtask/src .github`); not reached.
7. Record `spec/landed/harness/WP-352.md` (census before/after, the rules,
   the evidence).
