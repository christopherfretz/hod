# WP-317 (design 0008 I12) parked after session 1

Branch `wp/317-i386-sse2`, worktree `.worktrees/wp-317`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-317`.

## Done

- Step A: measured, written first in `spec/landed/codegen/WP-317.md`.
- Step B (scalar SSE): `crates/hod_x86/src/sse.rs`; dispatch in
  `target.rs` (`lower` arm `sse::owns` before `x87::owns`; the
  `materialize_mem` test). `x87.rs` and `abi.rs` unchanged. Spec
  `spec/codegen-x86.md` §12 appended, §7 row `x86:float:sse` removed.
  Tests `crates/hod/tests/i386_sse.rs` (+ `data/i386/sse/`).
- Design decision: SSE values stay class `Mem` (no `Fpr` values on this
  target); every operation goes through `xmm6` (`xmm4`, `xmm5` for
  `minnum`/`maxnum`). The x87 glue (args, `st0` returns and call
  results) is reused unchanged. `fpext`/`fptrunc` read §11's 80-bit copy
  through `x87::ret_copy`/`load_ret` (call results of SSE type, `double`
  at `pentium3`): llc's DAG folds an exact `fp_round` of a call result.

## Remaining, in order

1. `half`/`bfloat` with SSE2 (Step A: `__extendhfsf2` result in `st0`,
   `__truncsfhf2`/`__truncsfbf2` result in `xmm0`; a `half` return in
   `xmm0`). Needs: `abi.rs` to return `half` in `xmm0` for SSE2
   functions (a function of `sse.rs` that `abi::assign` calls at one
   point; `assign` has no function context, so the `X86` value must
   carry the feature set: `Target::for_function` exists in `hod_cg` for
   exactly this, `X86` has only `cpu` today: adding a field is
   `target.rs`'s, ask the supervisor), plus `Target::load`/`store` of
   `xmm0` in `target.rs`.
2. Step C (vectors), as the brief says. Same `for_function` question for
   the vector ABI (`xmm0`-`xmm2` arguments only with SSE).
   Proposal: vectors stay class `Mem` like scalars (the 64-bit back
   end's lane code as the model, operations through `xmm6`/`xmm7`), and
   `abi.rs` gets the `xmm` places from a function of `sse.rs`.
3. Evidence not yet gathered: irdiff i686 full tier (`cargo xtask irdiff
   --target i686 --llvm 22` and `--llvm 19`) before/after; compare-binaries
   against `31400b11` (base built into
   `/Volumes/AgentWork/hod-helper/.build/target-wp-317-base`).

## Ruled out

- `Class::Fpr` for SSE scalars: needs `class_of` per function (it has
  only the types) and xmm glue for args/returns that the x87 path
  already does through memory; no speed is gained that matters here.
- A refusal of `fptrunc` of a computed `double` at `pentium3`: not
  needed, the x87 copy is readable through `ret_copy`/`load_ret`.
