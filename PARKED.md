# WP-324 parked (context budget)

Done, each committed with its tests: item 1 (`x86:indirect-branch`),
item 2 (`x86:stack-protector`), item 3 (`x86:cf-protection`), item 5 in
part (`llvm.eh.unwind.init`; `frame_intrinsics.ll` passes). Record:
`spec/landed/codegen/WP-324.md`.

## Item 4, `x86:constant` (not done)

- Seed `tests/ir/const_operands.ll`, function `main`: `store i64 sub
  (i64 ptrtoint A, i64 ptrtoint B)` (lines 138, 139, 143), a
  `Const::SymDiff` of an `i64` type, reaching `int::materialize_mem`
  (`crates/hod_x86/src/int.rs` ~150), whose last arm refuses it.
- `llc` 19 and 22 (static and PIC alike, `-O0`): each address in a
  register (`lea`/`mov $sym`, `@GOT`/`@GOTOFF` under PIC), `sub` for the
  low word, `sbb` of a zero register for the high word (the two
  addresses zero-extended, then a 64-bit subtraction).
- Next step: in `int::materialize_mem`, an arm for `Const::SymDiff { a,
  b, addend, .. }`: `common::sym_address(ECX, a, 0)`,
  `common::sym_address(EDX, b, 0)`, `sub ecx, edx`, `sbb edx, edx`,
  then `add ecx, lo(addend)`, `adc edx, hi(addend)`, store both words.
  The function promises "flags kept": wrap in `pushfd`/`popfd`
  (`hod_mc_x64::pushfq`/`popfq` are the 64-bit names; check `Mode::X86`
  admits them) or prove no flag is live there. A `SymDiff` of 32 bits
  or less in `common::materialize_const` likewise (`sub` only). Test:
  the seed through irdiff i686 (LLVM 19, 22), plus run cases of a
  difference across sections and a negative difference under static
  and PIE.

## Item 5, rest

`drift_saturation.ll` stops at `llvm.fptosi.sat.*` / `fptoui.sat.*`
(function `convs`, `float`/`double` sources). `llc` lowers them inline
on the x87 (`fist`/`fistp` plus clamping compares). Not in `bits.rs` at
any width; it is `x87.rs`'s (the join's file), so it was left alone.

## Not run yet

- compare-binaries `bin-base/hod` against head over `tests/ir`,
  `.build/corpus/kiln`, `.build/corpus-x64/kiln` (`-j 4`, absolute
  `--out` under the build dir);
- the full `i386*` test list of `-p hod` and `stack_probe` at head
  (`i386_ibr`, `i386_ssp`, `i386_cet`, `cf_protection`, `hardening`
  pass at the item 3 commit).
