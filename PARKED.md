# WP-317 (design 0008 I12) parked after session 4

Branch `wp/317-i386-sse2`, worktree `.worktrees/wp-317`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-317`. Base of the group
`31400b11`.

## Done

- Sessions 1-3: scalar SSE (§12), `half` with SSE2, pentium3 fix.
- Session 4, step C (vectors), §13 and the record "Fourth session":
  - Measured (llc 19 = 22 at `pentium4`): vectors of <= 16 bytes, >= 2
    lanes of 8..64 bits in one `xmm`, packed (widened); result `xmm0`;
    args `xmm0`-`xmm2` (fastcc, inreg the same), then 16-byte stack
    slots aligned 16; a variadic call puts every vector on the stack.
    `<1 x T>` scalarized, `<N x i1>` promoted, wide split: refused at
    the boundary. No SSE2: llc scalarizes: refused. llc does not
    realign for 16-byte slots; hod's `esp` is 16-aligned after the
    prologue, so `frame.rs` is unchanged.
  - (a) done: `sse::vector_place` + `sse::load_xmm`/`store_xmm`,
    hooks in `abi.rs` (arg, ret) and `target.rs` (`load`/`store` of
    any `xmm`, dispatch, `materialize_mem`).
  - (b) done, (c) done, (d) partly: `crates/hod_x86/src/vector.rs`
    (all Mem, lane by lane): load/store/extract/insert (variable
    index)/shuffle/bitcast/freeze, int and float arithmetic, i64
    add/sub/bitwise, i1 bitwise, icmp/fcmp to masks, select, casts,
    integer and i1 reductions.
  - Tests: `crates/hod/tests/i386_vector.rs` (8 runs equal llc's incl.
    the boundary both ways; 10 refusals by name).
  - Evidence: fmt, clippy `-D warnings` clean; hod_x86/hod_cg/
    hod_mc_x64 pass; every `i386*` test passes (x87 288/72, 190+2);
    irdiff i686 22 and 19: 0 FAIL, 0 ERROR.

## Remaining, in order

1. compare-binaries against `31400b11` (not run): base build as
   session 3 did (`git archive 31400b11` into `<build>/base-src`,
   `CARGO_TARGET_DIR=/Volumes/AgentWork/hod-helper/.build/target-wp-317-base`),
   corpora `tests/ir`, `.build/corpus/kiln`, `.build/corpus-x64/kiln`;
   classify every i686 changed outcome; delete the base dir.
2. The three seeds (`vectors.ll`, `vector_masks.ll`,
   `drift_lanes.ll`) now refuse `x86:eh` (I10's), so they do not run
   on i686 yet; after the join with I10, run them (they use `<8 x
   i32>` bodies, `<5 x i8>`, `<3 x i1>`, reductions) and fix what
   differs.
3. Still refused: `uitofp`/`fptoui` with 32-bit lanes, i64-lane
   mul/div/shift, `frem` (libm per lane: would need an `x87::barrier`
   check), float reductions and other vector intrinsics, vector
   `ptradd` (plain `x86:vector`). Add tests per family as in
   `i386_vector.rs`.
4. README row for WP-317; remove `PARKED.md` in the last commit.
