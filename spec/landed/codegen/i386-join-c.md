# i386 join, part C: WP-309 (with WP-306) and wave 41 with parts A and B

The integer intrinsics of 1 to 32 bits (WP-309, `bits.rs`) and the rest
of the integers of 33 to 64 bits (WP-306, in `int.rs`: their
intrinsics, `switch`, `ptradd` index) merged onto parts A and B
(`i386-join-a.md`, `i386-join-b.md`), then wave 41 (the irdiff seeds
made valid for i686, `;@ kept-off(i686...)`, the gate's i686 run), and
the items the packages left for the join closed. Behaviour:
`spec/codegen-x86.md` §7, §8, §11; tests `crates/hod/tests/i386_join_c.rs`
(data `tests/data/i386/join_c/`), `i386_x87.rs`'s integer-intrinsic
shapes, `i386_i64_rest.rs`'s `compare.ll`.

## Merges and conflicts

- `wp/309-i386-int-intrinsics` (`47c8942c`, containing WP-306):
  `spec/codegen-x86.md` §7: part B's `x86:float*` rows kept, the two
  `x86:i64` rows combined (WP-306's wording of `x86:i64:wide`, the
  removers of `x86:i64` from both sides). `crates/hod_x86/README.md`:
  WP-297's `abi.rs` row, WP-306's `int.rs` row, WP-309's `bits.rs` row.
  Section numbers needed no change (WP-309's text is §4.1, WP-306's in
  §8, both sides had the same §8 to §10). Nothing broke: `hod_x86`'s unit
  tests and the twelve `i386*` test binaries of `hod` passed.
- `6b89c011` (wave 41): no conflict.

## Items

- **irdiff for i686 after the merges** (full tier, rows are test x
  linker): LLVM 22: 112 tests, 248 results, 9 PASS, 0 FAIL, 195
  UNSUPPORTED, 0 ERROR, 44 SKIP; LLVM 19: 246 results, 9 PASS, 0 FAIL,
  192 UNSUPPORTED, 0 ERROR, 45 SKIP. Declined, per test (LLVM 22):
  `x86:eh` 35, `x86:intrinsic` 6, `coro:target` 5, `x86:i64` 4,
  `x86:aggregate` 3 (2 with LLVM 19), `x86:cf-protection` 3,
  `x86:vector` 3, `attr:alignstack`, `x86:abi:wide-arg`, `x86:atomic`,
  `x86:dynamic-alloca`, `x86:indirect-branch`, `x86:inline-asm:goto` 1
  each. Kept off: 25 tests (26 with LLVM 19).
- **x87 barriers for the integer intrinsics** (`x87::int_branch`, §11
  "Integer intrinsics"). The packages' claim, checked with `llc` 19 and
  22 at widths 1 to 64 (probe functions with a label inside the lowering):
  it holds, and the widths between the ones they measured follow the same
  rule (`ctlz` with zero defined and the min/max/saturating family from 2
  bits; `cttz` with zero defined from 32; the zero-poison counts and
  variable funnel shifts above 32). New: `llc`'s DAG folds, and does not
  branch for, a min/max of one value twice or with the type's extreme
  values, a saturating add of 0, a subtraction of a value from itself or
  of 0, `usub.sat` from all ones, a saturating shift of 0 or by 0, a
  funnel shift of one constant twice, and calls whose operands are all
  constants or folded results (also through a folded `add 0, 0`, which
  hod does not see: a barrier there for hod, none for `llc`). With `cmov`
  (`i686`, `pentiumpro`, `pentium4`) nothing branches. LLVM 19 branches
  for the saturating add/sub of `i1`, 22 does not: hod follows 22 (§11
  states no rule for a version difference). Shapes
  (`tests/data/i386/x87/int-shapes-{float,double}.ll`, `int_shapes.py`):
  24, of which 10 are barriers at `generic`; 2 types x 2 LLVM x 2 CPUs =
  192, 190 equal `llc`'s, 2 (`saddsat1_between`, LLVM 19, `generic`)
  follow 22. The old shapes: 288 equal `llc`'s, 72 the model (unchanged).
- **`fptoui` to `i64`** (§11 "Conversions"): `llc`'s sequence, the same
  in 19 and 22; a barrier without `cmov` (`llc` chooses the subtrahend
  with a branch and reads its operand rounded; with `cmov` an `fcmov`).
  Control word: the same save, `or 0xC00`, `fldcw`, `fistp qword`,
  restore as `fptosi`. `fptoui.c`: values at and around `2^63` and
  `2^64`, signed zeros, negatives, infinities, NaNs, subnormals, of
  `float`, `double`, `long double`, and products computed in the
  expression (their low bits differ at 53 and 64 bits; the CPU without
  `cmov` gives the rounded result).
- **`double` libm against `llc`** (`libm.c`, `-fno-math-errno`: `frem`,
  `llvm.fma`, `minnum`/`maxnum`, `sqrt`, `pow`, `atan2` and 21 more
  libm routines over 28 edge values, pairwise for the binary ones). Equal; nothing to fix.
  `i386_join_c.rs`: 2 programs x 2 LLVM x CPUs `i586`, `pentiumpro` = 8
  modules, 24836 lines of `llc`'s output, hod's and hod's under
  `Stress::ALL` equal.
- **`scmp`/`ucmp` of 33 to 64 bits** (`int::three_way`, §8): two slot
  compares, `setcc`, the difference. `i64_rest/compare.ll` (`gen.py`):
  widths 64, 57, 48, 33, results of 2, 8, 32, 33, 64 bits, run by
  `i386_i64_rest.rs` (now 6 programs, 48 modules, 144 runs). Of 128
  bits: `x86:i64:wide`.
- **`x86:i64:wide` for `i128`** in `abi::type_refusal`. What still
  reaches the plain `x86:i64`: an integer of 33 to 63 bits as an argument
  or result (also of a call, also variadic) and an `i64` length of
  `memcpy`/`memmove`/`memset` (`bytes.rs`). The four irdiff seeds that
  decline `x86:i64` are of those kinds (`drift_call_widths`: an `i33`
  argument; `mem_intrinsics`, `stack_protector`,
  `stack_protector_smash`: an `i64` memory-intrinsic length).
- **`llvm.threadlocal.address` of no global**: i686 already gave what
  x86-64 and AArch64 give (the same internal error, "of something that is
  no symbol"; a non-thread-local global, `null` or a constant expression
  compile on all three). A test keeps them equal.
- **`calls.c`'s struct initializer** (join B): clang writes
  `llvm.memset.p0.i64` for the padding; still `x86:i64` (memory
  intrinsics are a later package). The fields stay assigned one by one;
  the comment names the refusal.
- **`exit_float` kept off i686** (`x87-precision`): without the
  directive, `llc`'s program and hod's exit 103 (check 3: `6.5 / -0.375`
  compared unrounded) with LLVM 19 and 22 and every linker (bfd, lld,
  mold); the seed expects 7 (LLVM's semantics), so the oracle row is an
  ERROR. The directive stays.
- **The gate's i686 run**: the full tier for i686 costs 7.9 s with LLVM
  22 (6.5 s with 19) on the loaded machine: `xtask::ci::irdiff_runs` runs
  every test for i686 now (`spec/harness.md` "Tiers").

## Found on the way

- `i386_join_b.rs`'s CPU `generic` for its C program is clang's default
  for `i686-linux-gnu`, `"target-cpu"="i686"`, which has `cmov`: only its
  hand-written `join_b.ll` runs without `cmov`. `i386_join_c.rs` uses
  `-march=i586` for that.

## What remains

- Memory intrinsics with an `i64` length, integers of 33 to 63 bits by
  value (`x86:i64`), the `*.with.overflow` family and the other
  `x86:intrinsic`s; EH and debug info (I10); aggregates, varargs bodies,
  atomics (I11); SSE2 (I12); `module asm` (WP-307, merged by the
  supervisor).
