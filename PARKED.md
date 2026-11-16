# WP-323 (i386 join E) parked after stage 2's merge

Branch `wave/45`, worktree `.worktrees/wave-45`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wave-45` (base binary
`bin-base/hod` = `f492182b`, kept for the comparisons; `bin-s1/hod` =
stage 1's head, deletable). Record: `spec/landed/codegen/i386-join-e.md`.

## Done

- Stage 1 (complete, evidence in the record): merge `5f873503` of
  `wp/314-i386-eh-debug` at `e61710bd`; tests and record `49c205e2`.
- Stage 2, merge only: `28994373` merges `wp/317-i386-sse2` at
  `0714a2ec`. Conflicts resolved (target.rs dispatch through
  `sse_family_owns`/`sse_family` after `atomic`'s arms and before
  `x87`'s/`agg`'s; `materialize_mem` agg, vector, sse, x87; lib.rs;
  README rows; spec §16 scalar SSE, §17 vectors, §7 union). fmt and
  `clippy --workspace --all-targets -D warnings` clean at `28994373`.
  NOTHING else of stage 2 has run yet (no tests at all after the merge).
- Stage 2 test programs written and checked against `llc` 19 and 22
  only (not yet against hod): `crates/hod/tests/data/i386/join_e/`
  `sse_eh.ll` (1), `sse_va.c` (2), `sse_agg.c` (3) and (5),
  `sse_atomic.ll` (4), `sse_dbg.c` (6; clang needs `-msse2` for
  `_Float16` on i686, `-march=pentium4` alone refuses it). Each gives the
  same output under `llc` 19 / 22, `generic` / `i686`, static / PIE.

## Remaining, in order

1. Append the drafted tests to `crates/hod/tests/i386_join_e.rs` from
   `/Volumes/AgentWork/hod-helper/.build/target-wave-45/parked/join_e_part2.rs`
   (needs `use std::fmt::Write as _;` and `llc_object` in the
   `i386_support` import; the `.ll` programs carry
   `"target-cpu"="pentium4"`, so they also run at `-mcpu=i686` and the
   stress object honours the attribute). Update the file's `//!` list.
2. Build the release binary; run the stage checks:
   `/Volumes/AgentWork/hod-helper/.build/target-wave-45/check.sh s2`
   in the background (unit tests, every `i386*` test by name incl.
   `i386_sse` and `i386_vector` — add them to the script's list with
   `EXTRA_TESTS="--test i386_sse --test i386_vector"` — 64-bit debug
   tests, irdiff i686 22 and 19, both counts gates). Expect x87 288 / 72
   and 190 + 2. Fix bugs between packages, one commit each with a line
   in the record.
3. Prove the dispatch does not shadow `atomic`/`agg` (tests (3) and (4)
   cover it; also a `refusal`-style check that an atomic load of a
   vector still names `x86:atomic:type`).
4. compare-binaries `bin-base/hod` vs head (`--out` absolute under the
   build dir): 64-bit 0 different, 0 changed; classify i686 outcomes;
   check whether the 112 i686 objects at CPUs without SSE stay
   identical (explain any difference).
5. irdiff census of refusal names (LLVM 22); the vector seeds
   (`vectors.ll`, `vector_masks.ll`, `drift_lanes.ll`) have no
   `target-cpu`, so they stay `x86:vector:no-sse2` on i686.
6. Record stage 2 in `i386-join-e.md`; commit; remove this file.
7. Stage 3 (only with budget): `bitcast` float<->i32 / double<->i64 on
   the x87 path (WP-317's `sse::bitcast` already covers SSE functions).
   Measured `llc` 22: at `generic` `bitcast (fadd float) to i32` is
   `fstps (%esp); movl (%esp), %eax` (the rounded value, no branch: not
   a barrier); `bitcast i64 to double` is `fldl` of the argument.
   Then the vector edge-value lane tests and one `x86_fp80` across
   vector code.

## Facts measured on the way

- `llc` 19 / 22 at `pentium4`: an 8-byte atomic load is `movq (%eax),
  %xmm0` (SSE2), hod's `atomic.rs` uses `fild`/`fistp` also with SSE2:
  one 8-byte access either way (values to be compared by test (4)).
  A `seq_cst` store in `llc` is `lock orl $0, (%esp)`; hod's
  `full_fence` uses `mfence` with SSE2: both full fences.
