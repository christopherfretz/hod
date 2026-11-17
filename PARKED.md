# WP-342 parked (implementer hit the tool-call cap)

## Done (commit 813d240a)
- `crates/hod_dwarf/src/coro.rs`: `CoroAt::Passed { stack, deref, mode, heap }`
  (mode word 0: the incoming copy through `stack`; else frame word + heap),
  unit test.
- `crates/hod_cg/src/coro_debug.rs`: pre-pass in `coro_vars` merging a
  parameter passed by address (`Indirect(non-heap)` with an empty
  expression, or `Slot(non-heap)` under `DIExpression(DW_OP_deref)`) with
  its heap-slot copy (same variable: clang, one row; artificial same
  name/scope: kiln, both rows). Other expressions:
  `debug:coro-passed-expression`.
- `spec/dwarf.md` §6.16 (table row, paragraph, refusal) and the refusal
  table; `tests/debug/coro_byval.cpp`; `crates/hod/tests/coro_byval_debug.rs`.
- New program, debug lane: aarch64 30/30 PASS, x86_64 30/30 PASS (head).

- Full debug lanes, base f20f08b6 -> head: aarch64 1217 PASS/30 FAIL ->
  1247 PASS/0 FAIL; x86_64 1120 PASS/12 FAIL -> 1150 PASS/0 FAIL; every
  base FAIL is coro_byval.cpp, no PASS becomes anything else. i686, the new
  program only: 12 PASS, 1 SKIP (kiln has no i686).
- fmt, clippy (`-D warnings`), `cargo test -q -p` hod_dwarf, hod_cg, hod: clean.

## Remaining, in order
1. Not started (base binary kept for it:
   `/Volumes/AgentWork/hod-helper/.build/target-wp-342-base/debug/hod`): `cargo xtask compare-binaries --base <base hod> --head
   <head hod> --corpus tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln
   --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln -j 4 --out <abs dir>`;
   count `debug:variable-twice` refusals before/after.
2. Not written: `spec/landed/dwarf/WP-342.md` (findings: see the handback).
3. Delete `/Volumes/AgentWork/hod-helper/.build/target-wp-342-base` and
   `.build/target-wp-342/base-src` afterwards; remove this file.
