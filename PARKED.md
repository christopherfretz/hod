# WP-300 (I9, x87) parked at the context budget

Branch `wp/300-i386-x87`, base `d9833483`. Read `.build/briefs/general.md`,
`.build/briefs/i386-group.md`, the WP-300 row, then this file and
`crates/hod_x86/src/x87.rs` (its module comment is the design).

## Done (commit 286dcea0)

- `crates/hod_x86/src/x87.rs`: all of the lowering. `float`/`double`/
  `x86_fp80` are `Mem` (class_of already said so). Loads, stores, phis
  (hod_cg's `Target::copy`), `freeze`, `select`, `copysign`: integer
  copies (bit-exact). Constants: `materialize_mem` writes the encoding
  with `mov m, imm` pieces (no constant pool, no relocation, so nothing
  to refuse under PIC: `x86:pic:float-constant` is NOT needed; say so).
  fadd/fsub/fmul/fdiv/fneg, frem (fmodf/fmod/fmodl), fcmp all 16
  predicates (`fucompp`+`fnstsw`+`sahf` without CMOV, `fucomip` with it,
  via `hod_ir::X86Features::of_function`), fpext/fptrunc, sitofp/uitofp
  and fptosi/fptoui for i1..i32 (control word around `fistp`; unsigned
  i32 through 64-bit `fild`/`fistp`), sqrt (`fsqrt`), fabs, copysign,
  fmuladd (product unrounded into the sum, as llc without FMA), libm
  calls of `routine_op` (sin..fma) through `fp_routine(.., X86_64)`.
- Excess precision (point 3 of the brief): **llc -O0 does NOT round after
  each IR instruction**; it keeps a result in an x87 register for a use
  in the same block (verified: `(a*b)+c`, a=b=1+2^-30, c=-(1+2^-29),
  llc 22 prints 2^-60 = `0 3c300000`; rounding each step gives 0). hod
  matches with "carried" results (`carried`, `carry_user`, `neutral`,
  `consumes`): a pure function of the IR, no state. Multi-use in-block
  results are still rounded (llc keeps them unrounded): a known
  divergence to write into the spec.
- `reg.rs`: `st(n)` = PReg 16+n, DWARF 11..18 (`dwarf` body, test).
- `target.rs`: one first arm `_ if crate::x87::owns(cx, inst) =>
  crate::x87::lower(cx, inst)` (target.rs:136), `materialize_mem` body
  calls `crate::x87::materialize_mem` (target.rs:~245).
- Smoke-verified by hand (clang 22 `-mno-sse -march=pentiumpro
  -ffp-contract=off`, and the same module with target-cpu/features
  stripped = generic): chain, float chain with division, long double
  div/neg, fcmp with NaN, conversions, fromi, fptrunc fp80->double->float,
  select: hod's output == llc 22's, both CPUs. No test file yet.

## Remaining, in order

1. `crates/hod/tests/i386_x87.rs` (`mod i386_support;`), programs in
   `crates/hod/tests/data/i386/x87/`: arith.c, conv.c, libm.c
   (`-fno-math-errno`), and an IR module generated in the test for the 16
   fcmp predicates x 3 types (C cannot spell ueq/ugt/...). Variants per
   program: clang flags `-fno-pic -fno-pie -mno-sse -march=pentiumpro`
   (+ `-ffp-contract=off` in one variant so fmul/fadd chains are seen,
   default in the other so `llvm.fmuladd` is seen) and "generic" = the
   module text with `"target-cpu"=..`, `"target-features"=..`,
   `"tune-cpu"=..` removed. Each: hod, `stressed_object`, llc 19 and 22;
   link `-no-pie -lm`; outputs equal. Print an FNV hash of result bytes
   (only 10 bytes of a long double) plus the 2^-60 chain line, and
   assert that line in Rust. Random operands: LCG in the program; set the
   quiet bit of any NaN pattern (SNaN through a copy differs from llc's
   `fld`/`fstp`); long double with the integer bit set; to-int inputs only
   in range (out of range is poison and llc uses `fistp word` for i16).
   Assert `x86:float:sse` for a module with clang's default pentium4.
2. Run `cargo test -q -p hod --test i386_x87`, `i386`, `i386_run`;
   `cargo xtask irdiff --target i686 --quick` before (base worktree build
   dir) and after; `cargo xtask compare-binaries` base vs head over both
   corpora (`-j 4`, absolute `--out`).
3. `spec/codegen-x86.md`: new last section (classes, carried-result rule
   and its divergence, compare flags table, control word sequence, libm
   symbols, refusal names) and the §7 rows: `x86:float` now only "a
   floating argument/result (I6)"; add `x86:float:{half,bfloat,fp128,
   sse,i64-cast,intrinsic,cast}`. `crates/hod_x86/README.md` I9 row.
   `spec/landed/codegen/WP-300.md`.
4. i64 casts: refused `x86:float:i64-cast` (no i64 value can exist before
   I7 merges); after I7: `fild qword`/`fistp qword` on the i64 slot.

## Decisions

- Constants by immediates, not a `.rodata` pool: same bits, no
  relocation, PIC-neutral (deviation from the brief, on purpose).
- libm calls are emitted in x87.rs (args copied to `[esp+..]` at 4/8/12
  bytes, area rounded to 16, `st0` result); no register displacement is
  needed: the pool (ebx, esi, edi) is callee-saved.
- SSE: a `float` op under SSE or a `double` op under SSE2 is refused
  `x86:float:sse` (llc would use xmm; I12).
