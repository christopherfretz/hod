# WP-329 parked

Done: stage A (the i686 column runs: GNU ld, lld, mold; gdb-multiarch
and lldb-19 through qemu-i386's stub), one hod fix (i386 folded-branch
row, `crates/hod_x86/src/control.rs` `folded_br`), two plumbing / test
fixes. Lane at head: 445 results, 354 PASS, 34 FAIL, 57 SKIP (22 kept
off), 110 s at `-j 4`. Details and every open FAIL:
`spec/landed/harness/WP-329.md`.

Next, in order (stage B):
1. `dbg_value_arglist.c`: in `crates/hod_dwarf/src/loclists.rs`
   `fits_i386`, give a `ValueLoc::Args` range an entry when
   `arglist_width` is at most 32 (and check what operations the arglist
   expression writes: `DW_OP_deref` reads 4 bytes on i386); test in
   `hod_dwarf`; prove the 64-bit objects byte-identical.
2. `inlined.cpp`, `key_call.c`, `call_sites_direct.c`: dump hod's and
   llc's rows at the return address (`--keep`, `llvm-dwarfdump
   --debug-line`, `llvm-objdump -dl`) and decide bug or a rule.
3. mold `<.L.str>`: measure whether llc's object alone shows it under
   mold (witness), then an `xlink::EXPECTED` entry with a linker
   exception, or a finding.
4. Re-run when WP-324 / WP-327 land (coroutines, stack protector,
   probe-stack FAILs are their refusals).
Stage C: add `debug-i686` to `xtask/src/ci.rs` only once the lane is
green (110 s today); `debug::gate` would need to skip `verify` for
i686 (no corpus).
