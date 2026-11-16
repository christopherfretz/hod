# WP-300 (I9, x87) parked at the context budget (second session)

Branch `wp/300-i386-x87`, base `d9833483`. Read `.build/briefs/general.md`,
`.build/briefs/i386-group.md`, the WP-300 row, then this file and the
module comment of `crates/hod_x86/src/x87.rs` (the design and the rule).

## Done

- `x87.rs` lowering (commit 286dcea0), then (e43a3219 and the commit after it):
  carried results with up to 4 uses (one stack copy per operand,
  `fld st0` at the definition), `freeze` and `fptrunc (fpext x)` pass 80
  bits on, `select` is `fcmov` on the stack with `cmov`, `candidate`
  counts only computed results (loads no longer block carrying),
  minnum/maxnum (float `fminf`/`fmaxf`, x86_fp80 `fminl`/`fmaxl`; double
  refused `x86:float:intrinsic`: hod_cg has no `RuntimeFn::Fmin`/`Fmax`,
  a follow-up in `crates/hod_cg/src/sym.rs` ~line 1006), asin..tanh,
  atan2, exp10 as libm calls.
- Rounding rule MEASURED (hand-written shapes, qemu-i386; llc 19 == llc 22
  on every shape, float and double): llc keeps a computed result at 80 bits
  for EVERY use in its block before the next call (any number of uses, any
  order, any distance); a use after a call or in another block reads it
  rounded; `freeze`, `fpext`, `fptrunc(fpext x)` (DAG fold) and `select`
  with cmov (`fcmov`) pass 80 bits; without cmov `select` rounds (branch);
  with 6+ simultaneously live x87 values llc rounds SOME uses (depends on
  its fast allocator: not computable from the IR). `x86_fp80`: nothing to
  round.
- Tests `crates/hod/tests/i386_x87.rs` (4 tests, all pass): arith.c (12
  runs), fcmp module (12 runs, 16 preds x 3 types x 121 pairs, zext/select/
  br/fp select/fneg operand), shapes-{float,double}.ll + shapes.c (156 lines
  equal llc, 140 equal the model), `x86:float:sse` refusal. Each run: llc,
  hod, Stress::ALL; llc 19 and 22; generic and pentiumpro; GNU ld.
- `cargo fmt`, `clippy -p hod_x86 -p hod --all-targets -D warnings` clean;
  `cargo test -p hod_x86`, `-p hod --test i386 --test i386_run` pass.

## DECISION for the supervisor (put at the top of the hand-back)

hod rounds, where llc keeps 80 bits, in these shapes (tests compare them
with a model): a use after another carried result defined later is still
live (non-LIFO: `twice` when the first result's store comes after the
second use, `nonlifo`, `sqrt_between`), uses more than 12 instructions
away (`far_*`), more than 4 copies on the stack (`many*`, where llc itself
rounds some, allocator-dependent), a value with uses both before and after
a call or also in another block (`call_between`, `block_and_local`), and
`select i1 true` (llc folds it) without cmov. Fix for all but the
allocator part: keep an 80-bit copy in a 12-byte frame slot (`fstp tword`)
next to the rounded one and `fld tword` it for every same-block use before
a call. That needs a per-function map InstId -> FrameRef, i.e. ONE field
in `FnState` (`crates/hod_x86/src/target.rs:44`, outside ownership).
Approve that edit, or accept the documented divergence.

## Remaining, in order

1. Evidence: `cargo xtask irdiff --target i686 --quick` base (`d9833483`,
   own target dir) vs head, counts per class and refusal name;
   `cargo xtask compare-binaries` base vs head over `.build/corpus/kiln`
   and `.build/corpus-x64/kiln` (`-j 4`, absolute `--out`): 0 different,
   0 changed-outcome. Run in the background.
2. Spec: new last section §9 of `spec/codegen-x86.md` (the measured llc
   rule and hod's rule above with the divergent shapes, compare flags
   per CPU, control word sequence around `fistp`, libm symbols per type,
   refusals `x86:float:{half,bfloat,fp128,sse,i64-cast,intrinsic,cast}`;
   `x86:float` remains only for floating arguments/returns (I6)); §7
   rows; `crates/hod_x86/README.md` I9 row; `spec/landed/codegen/WP-300.md`
   (no `x86:pic:float-constant`: constants are `mov` immediates). Then
   delete this file.
3. Not this package: i64 casts wait for WP-298; SSE is I12.

## Notes

- `hod --mcpu=pentiumpro` does not change `X86Features::of_function` (only
  the function's `target-cpu` attribute does); tests use attributes.
- `arith.c` setup was rewritten once with a python script (against the
  edit rule; content reviewed, test data only).
- Measurement scratch: `.build/target-wp-300/measure/` (delete when done).
