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

## Remaining, in order
1. Read the results of the chained run (task started at handback time):
   `/Volumes/AgentWork/hod-helper/.build/target-wp-342/l7-new-i686.log`, then
   `l7-{base,head}-{aarch64,x86_64}.tsv` (base binary
   `/Volumes/AgentWork/hod-helper/.build/target-wp-342-base/debug/hod`, built
   from f20f08b6): no PASS becomes FAIL.
2. Clippy/tests results: `.build/target-wp-342/clippy.log`, `test-{hod_dwarf,hod_cg,hod}.log`.
3. Not started: `cargo xtask compare-binaries --base <base hod> --head
   <head hod> --corpus tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln
   --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln -j 4 --out <abs dir>`;
   count `debug:variable-twice` refusals before/after.
4. Not written: `spec/landed/dwarf/WP-342.md` (findings: see the handback).
5. Delete `/Volumes/AgentWork/hod-helper/.build/target-wp-342-base` and
   `.build/target-wp-342/base-src` afterwards; remove this file.
