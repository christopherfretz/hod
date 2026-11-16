# WP-317 (design 0008 I12) parked after session 3

Branch `wp/317-i386-sse2`, worktree `.worktrees/wp-317`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-317`. Base of the group
`31400b11`.

## Done

- Sessions 1-2: scalar SSE (`sse.rs`, §12), the call-result views.
- Session 3: the pentium3 finding fixed (`x87::push`, `x87::cast`
  bodies read `sse` views; test un-ignored; I9 counts 288/72, 190+2;
  112 x87 objects identical to base). `half` with SSE2 (§12 "`half`
  with SSE2", record "Third session"); `half` without SSE2 stays
  `x86:float:half` (libgcc's `__truncsfhf2` returns only in `xmm0`);
  new name `x86:float:half:intrinsic`. `bfloat`, `fp128`: refused,
  measured only.

## Remaining, in order

1. Step C, vectors (`x86:vector`), not started. Measure first: `llc`
   22 and 19 `-O0 -global-isel=0 -fast-isel=0
   -mtriple=i686-unknown-linux-gnu` at `pentium4`, `pentium3` and
   `generic` (legal IR, llc splits it). Start with the shapes of the
   three seeds of `tests/ir` kept off i686 / refused under
   `x86:vector` (`grep -rn "kept-off(i686" tests/ir`; irdiff census
   counts 3), then 128-bit integer and float vectors at `pentium4`.
   Plan (supervisor-approved): class `Mem`, operations through
   `xmm6`/`xmm7`, `hod_x64`'s lane code (`crates/hod_x64/src/lanes.rs`)
   as the model; arguments `xmm0`-`xmm2` and the `xmm0` return through
   `sse::half_place`'s sibling in `sse.rs`, called from
   `abi::assign_for` (the features already reach it). Wider vectors,
   MMX, vectors without SSE2 may stay refused under narrower names.
2. Evidence after each vector step (list in the brief) and
   compare-binaries against `31400b11` at the end (base build:
   `git archive 31400b11 | tar -x -C <build dir>/base-src`, `cargo
   build --release -p hod_cli` with
   `CARGO_TARGET_DIR=/Volumes/AgentWork/hod-helper/.build/target-wp-317-base`;
   delete that directory by its literal path when done).

## Ruled out

- `Class::Fpr` for SSE scalars; reading `w`'s home in `fptrunc (fpext
  w)` (homes of non-operands may be reused).
- `half` at `generic`/`pentium3` "as llc": its code reads `ax` after
  libgcc writes `xmm0` (and libgcc uses SSE2): it cannot run correctly.
