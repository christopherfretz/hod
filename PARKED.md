# WP-314 parked (session 2 of 3)

Branch `wp/314-i386-eh-debug`, worktree `.worktrees/wp-314`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-314`. Base binary of
`31400b11` (a `hod_cli` build): `/Volumes/AgentWork/hod-helper/.build/target-wp-314-base/release/hod`;
base sources `.build/target-wp-314/base-src`. Keep both until Part 2's
compare-binaries, then delete each by one literal absolute path.
NOTE: the `hod` binary is the `hod_cli` package: `cargo build --release
-p hod_cli` (`-p hod` builds the library only and leaves a stale binary).

## Done

- Part 1 (exceptions) complete, with evidence in
  `spec/landed/codegen/WP-314.md` (irdiff i686 22/19: 0 FAIL, 0 ERROR;
  compare-binaries: AArch64/x86-64 0 different, 0 changed outcome).
- `hod_cg/src/emit/mod.rs:1383` structor entries of the address's size
  (supervisor-authorized; own commit).
- New tests in `crates/hod/tests/i386_eh.rs`: `callconv.ll` runs (sret
  callee pop, i64, fastcc, x87 across an invoke and inside a pad),
  stdcall/fastcall invoke refusal by name, `lsdas_decode_as_llc_s`.
- Spec `spec/codegen-x86.md` §12 (x87 measurement, static-model
  deviation, tests).

## Part 2 (debug information): measured, designed, not started in code

Measured (`clang --target=i686-linux-gnu -O0 -g`, `llc` 22 and 19,
`llvm-dwarfdump`; sample in `.build/target-wp-314/meas/dbg.c`): DWARF 5,
CU `addr_size = 4`; `.debug_addr` entries 4 bytes, header address size
4; every relocation in `.debug_info`, `.debug_str_offsets`,
`.debug_addr`, `.debug_line` is `R_386_32`; `DW_AT_frame_base
DW_OP_reg5 EBP`; parameters `DW_OP_fbreg +8, +12` (hod's
`hod_x86::frame::base_offset` gives `Incoming` the same base), locals
`fbreg -N`. Thread-local variable: `DW_OP_const4u 0 (R_386_TLS_LDO_32),
DW_OP_GNU_push_tls_address`. `gdb-multiarch` against `qemu-i386 -g
<port>` on llc's static program works (break, bt through 3 frames, info
args/locals, up, print) — see the command in the next steps.

Next, in order (keep each edit local; WP-305/308/311/318 also edit
`hod_dwarf`):
1. `hod_dwarf/src/target.rs`: field `arch: Arch` -> `machine: Machine`;
   add `I386` (`address_size` 4, `offset_reloc` and `address_reloc`
   `I386Reloc::Abs32`); `for_machine(Machine)`; keep `for_arch`.
2. `line.rs`: `LineProgram::new/for_version/with_capacity` take `impl
   Into<Machine>` (callers passing `Arch` still compile); `arch()` ->
   `machine()`; lines 383, 678 compare `.machine`; 647, 653, 675 use
   `obj.machine()`/`for_machine`. `dwarf4.rs:138` `line_program` the
   same.
3. `die.rs`: `Writer` gets `addr_size: u8` (set in `write_debug_info`
   from `for_machine(obj.machine())`; 8 in `dwarf4.rs:822` and
   `split.rs:202`, which i386 refuses); sites: 910 (unit header), 1007
   (`DW_AT_low_pc` 0), 1082/1085/1092 (`.debug_addr` length, header,
   entries), 1107 (rnglists header); `loclists.rs:873` (loclists
   header); 1286 `for_machine`. `dwarf4.rs:931`, `split.rs:472`
   `for_machine` too (they panic on `obj.arch()` otherwise).
4. `split.rs` `write_unit_dies`: on `Machine::I386` refuse a DWARF 4 unit
   (`debug:dwarf-4-i386`) and split DWARF (`debug:split-dwarf-i386`)
   before dispatch: stable names, `debug:` prefix so `hod_cg`'s
   `dwarf_err` keeps them.
5. `x86_64.rs:88` `tls_offset_reloc(target.arch)` -> by machine. TLS
   location on i386 (const4u + `R_386_TLS_LDO_32`): optional; else
   leave `tls_has_location` false for i386 (no location, as AArch64)
   and record the gap.
6. `hod_cg/src/emit/mod.rs:423-428`: `DebugLines::new(module, machine,
   ...)` for every machine; remove `X86_DEBUG_INFO` use (keep the
   constant only if referenced). `emit/lines.rs:146,167` take
   `Machine`.
7. Remove `x86:debug-info` from `spec/codegen-x86.md` §7 row 179 and
   README line 37; `crates/hod/tests/inlining.rs:1029` comment names
   it (not this package's file: report).
8. Evidence: `llvm-dwarfdump --verify` on i386 objects of the debug
   seeds (`tests/debug/`), irdiff i686 22/19, compare-binaries (64-bit
   byte-identical), a gdb test: build `meas/dbg.c` by hod (static, `-g`),
   then `qemu-i386 -g <port> ./prog &` and `gdb-multiarch -batch -ex
   "file prog" -ex "target remote :<port>" -ex "break add" -ex continue
   -ex bt -ex "info args" -ex "info locals" -ex up -ex "print x"`.
   The xtask debug lane refuses `--target i686` (`xtask/src/debug.rs:74`
   `NO_I686_LANE`; `debug/cross.rs` has the qemu/gdb cross runner for
   x86-64): adding the column is xtask work beyond this package; park.
