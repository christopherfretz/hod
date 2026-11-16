# i386 join, part A: WP-298, WP-301 and WP-299 together

Design 0008's I7 (integers of 33 to 64 bits, `int.rs`, WP-298), I13
(inline assembly in 32-bit mode, `asm.rs` and `hod_asm::x86`, WP-301)
and I8 (PIC, PIE and the four TLS models, `pic.rs` and `tls.rs`,
WP-299) were written in parallel on the same base and merged here, in
that order, into one branch. Behaviour: `spec/codegen-x86.md` §8, §9,
§10 (the interactions in each). Tests: `crates/hod/tests/i386_join.rs`.

## Merges and conflicts

- WP-298: no conflict.
- WP-301: `spec/codegen-x86.md` (both appended a §8): wide integers stay
  §8, inline assembly becomes §10; its reference in the §7 refusal table
  and in `spec/landed/asm/WP-301.md` follow. `PARKED.md` (add/add):
  deleted, its open items below.
- WP-299: `spec/codegen-x86.md` (a third §8): PIC and TLS become §9 (§9.1
  to §9.4), placed between the two; references in `pic.rs`
  (`mem_inst`'s contract, §9.2) and `spec/landed/codegen/WP-299.md`
  follow. `crates/hod_x86/README.md`: WP-298's `int.rs` row and
  WP-299's `pic.rs`/`tls.rs` rows kept. The §7 table merged without a
  conflict (each package's deletions and rewordings kept);
  `crates/hod_cg/src/sym.rs` kept all five `RuntimeFn` entries
  (`UDivDi3`, `DivDi3`, `UModDi3`, `ModDi3`, `TlsGetAddrRegparm`);
  `target.rs`, `call.rs`, `Cargo.lock` merged without a conflict.

## What the interaction tests found

- **64-bit division under PIC** (fixed): `int::call_pushed` called
  `frame::emit_call`, so `__divdi3`, `__udivdi3`, `__moddi3`,
  `__umoddi3` were called through the PLT (`R_386_PLT32`) without the
  GOT pointer in `ebx`. With libgcc's static archive the linkers bind
  the call directly and nothing shows; linked against `libgcc_s`
  (`wide.c`, a PIE, GNU ld) the PLT entry reads `ebx` and hod's program
  died with a signal before its first line of output (llc's printed
  it). The call is now `pic::emit_call`, as every symbol call is
  (§9.2).
- Not a bug, made a contract: `int` never builds an instruction through
  `pic::mem_inst` (a symbol's address is taken into `eax` by
  `common::sym_address` first, then words move through `ecx`/`edx`), so
  `mem_inst`'s `eax` rule is not met by 64-bit code (§8, §9.2).
- `A` (`edx:eax`) with 64-bit operands works as input, output, early
  clobber and tied (`+A`); memory and pointer operands that are globals
  or thread-locals of any model work under every relocation model
  (§10).
- Module-level assembly with code or data stays refused for i386 as
  `module:asm` (`hod_cg::emit::asm::check`): not miscompiled; text of
  symbol directives only compiles. Wiring it needs, besides `hod_asm`, a
  change of `hod_cg/src/emit/module_asm.rs` beyond a hook (below), so it
  was not done.
- `tls::TLS` and `tls::DYNAMIC`, refusal names no lowering returns any
  more and no test names, are deleted.

Evidence: `i386_join.rs`: 3 programs (`wide.c`, `asm.c` by clang;
`join.ll` by hand) × LLVM 19, 22 × CPU generic, pentium4 × program, PIE,
shared library = 36 programs; 108 runs (GNU ld, lld, mold), each of
`llc`'s, hod's and hod's under `Stress::ALL` objects, all equal `llc`'s;
every link against a shared library of the cross gcc (`ext.c`) and
`libgcc_s`.

## Remaining

- i386 module-level assembly: `hod_asm::module` needs a 32-bit entry
  (`run` with `Mode::X86` through `x86::statement`, the 32-bit
  `.long`/`.quad` and relocation kinds), and
  `hod_cg/src/emit/module_asm.rs` an `I386Reloc` mapping in
  `reloc_kind` (today a `bool` x86-64/AArch64) and `asm::check`'s
  target list.
- From WP-298: the `i64` intrinsics (`*.with.overflow` need I11's
  `extractvalue`; bit counts, min/max, funnel shifts), `switch` on an
  `i64`, an `i64` `ptradd` index, `i128` (`x86:i64:wide`); `i64`
  arguments and results (I6).
- From WP-301: several direct asm outputs (`extractvalue`, I11); `t`/`u`
  (x87, I9); `%h` and the high-byte registers.
- I6 (ABI), I9 (x87), I10 (exceptions, debug information), I11
  (atomics, varargs bodies, dynamic `alloca`), I12 (SSE).
- kiln's end-to-end integers test (`crates/hod_cli/tests/kiln_e2e.rs`)
  does not try i686: kiln refuses the target itself.
