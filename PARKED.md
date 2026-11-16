# WP-324 parked: what remains

Done, each committed with its tests: `x86:indirect-branch`,
`x86:stack-protector` (now with the stack arguments copied below the
guard), `x86:cf-protection`, `llvm.eh.unwind.init`. Record:
`spec/landed/codegen/WP-324.md`. The supervisor moved items 4 (below)
and `x86:blockaddress` out of this package.

## Next: `x86:constant` (a difference of two addresses)

- Seed `tests/ir/const_operands.ll`, function `main`, line 143: `store
  i64 sub (i64 ptrtoint @_ZL6squarei, i64 ptrtoint (gep @buf, 3))`. It is
  a `Const::SymDiff` of type `i64` and reaches `int::materialize_mem`
  (`crates/hod_x86/src/int.rs:151`), whose last arm refuses it. hod_x86
  has no `SymDiff` arm anywhere. A 32-bit one would reach
  `common::materialize_const` (`common.rs`, the `_ => CONSTANT` arm).
- `llc` 19 and 22, static and PIC alike, `-O0`: it puts each address in
  a register (`lea`/`mov $sym`, `@GOT`/`@GOTOFF` under PIC), then `sub`
  for the low word and `sbb` of a zero register for the high word (the
  two addresses zero-extended, then a 64-bit subtraction).
- Glue must keep the flags (`spec/codegen.md` §5.6.7: glue may run
  between a flag setter and its reader). Its scratch registers are `ecx`
  and `edx` (`target.rs` `REG_INFO`). `eax` is written by no glue.
- 32 bits (`materialize_const`): port `hod_x64::constaddr::difference`.
  `rd = a + addend` (`common::sym_address`), `t = b` in
  `common::other_scratch(rd)`, `not t`, `lea rd, [rd + t + 1]`. This is
  flag-free. If a caller can hold a value in `t`, bracket it with `push
  t` / `pop t`; body code is `ebp`-based, so no CFI changes.
- 64 bits (`int::materialize_mem`): the high word needs the borrow, which
  no flag-free sequence gives. Use `pushfd` (`mc::pushfq`, admitted in
  `Mode::X86`, used by `tls.rs`), `sym_address(ECX, a)`,
  `sym_address(EDX, b)`, `sub ecx, edx`, `sbb edx, edx`, `add ecx,
  lo(addend)`, `adc edx, hi(addend)`, `popfd`. Store both words only
  AFTER the `popfd`, because `dst` may be `Outgoing` (`esp`-relative).
- Test: the seed through irdiff i686 on LLVM 22 and 19, plus run cases of
  a difference across sections and a negative difference, static and
  PIE.

## Not this package

`llvm.fptosi.sat` / `fptoui.sat` on the x87 (`drift_saturation.ll`,
`x87.rs`'s). The 64-bit stack-argument weakness under a stack protector
is a finding in the record and needs a `hod_cg` package.
