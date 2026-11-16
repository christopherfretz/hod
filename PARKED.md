# WP-314 parked (session 3 of 3, Part 2 code done, evidence partly in)

Branch `wp/314-i386-eh-debug`, worktree `.worktrees/wp-314`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-314`. The `hod` binary is
`cargo build --release -p hod_cli`. Base for the comparison = the merge
commit `1742ae7e` (wave/42 `8e441c4b` merged, no conflict), built into
`/Volumes/AgentWork/hod-helper/.build/target-wp-314-base/release/hod`
from `.build/target-wp-314/base-src`. Delete both (one literal path each,
`find <dir> -mindepth 1 -delete`) once the numbers are in the record.

## Post-merge state (Part 1 lands in it)

fmt clean; clippy `-D warnings` clean; unit tests of hod_x86 hod_eh
hod_dwarf hod_cg 229 passed / 0 failed / 11 ignored; the 15 `i386*`
binaries of `-p hod`: 49 passed, 0 failed; x87 shapes 288 / 72,
integer-intrinsic shapes 190 + 2 = 192. irdiff i686 full: LLVM 22 93
PASS / 0 FAIL / 111 UNSUPPORTED / 0 ERROR (31 of 68 objects built); LLVM
19 93 PASS / 0 FAIL / 108 UNSUPPORTED / 0 ERROR (31 of 67).

## Done in code (commit 06430652)

- `hod_dwarf`: `target.rs` `LineTarget.machine`, `I386`, `for_machine`;
  `line.rs` constructors take `impl Into<Machine>`, `machine()`;
  `die.rs` `Writer.addr_size` (unit header, ranges `low_pc` 0,
  `.debug_addr`, rnglists header); `loclists.rs` loclists header and
  `fits_i386` (no value-list entry needing a 64-bit DWARF stack);
  `x86_64.rs` TLS location `DW_OP_const4u` + `R_386_TLS_LDO_32`;
  `split.rs` `write_unit_dies` refuses `debug:dwarf-4-i386`,
  `debug:split-dwarf-i386` on i386.
- `hod_cg`: `emit/mod.rs:420` `DebugLines::new(module, machine, ..)` for
  every machine, `X86_DEBUG_INFO` deleted; `emit/lines.rs:146,167`
  `Machine`; `emit/lines.rs:519` `tls_by_symbol` also i386.
- `crates/hod/tests/inlining.rs:1029` comment updated.
- New `crates/hod/tests/i386_debug.rs` + `tests/data/i386/debug/`.
  Its first run failed on two test bugs, both fixed in the commit but
  NOT re-run: the split name (clang's `vals.dwo`) and `llc`'s line-0
  rows (§5.7 P2) excluded from the row-subset check.

## Measured by hand (build dir `meas/p2`, scripts `run.sh`, `cmp.py`,
`rows.py`, `gdb.sh`)

- `llvm-dwarfdump --verify` (19 and 22) clean on hod's objects of
  vals.c, tlsg.c (static and PIC), eh.cpp (static): 10 objects.
- DIE trees (name, type, location with fbreg by operation, frame base
  `DW_OP_reg5 EBP`, low/high_pc, ranges) equal `llc`'s (reference
  options) in all 10, except the `byval` parameter `b`: hod `fbreg o,
  deref`, llc `fbreg +8` (spec/dwarf.md §6.17 rule). TLS `const4u 0x0,
  GNU_push_tls_address` and globals `addrx` equal; parameters described
  in hod's slots (fbreg -N) where llc describes them in place (+8): the
  §6.17 "memory arguments" rule.
- Line rows: is_stmt and prologue_end rows equal for vals/tlsg; eh.cpp
  has one extra is_stmt `epilogue_begin` row (line 6 col 3) after the
  pad, identical on x86-64 (a 64-bit behaviour; no explicit spec rule
  found: report as a finding). hod's (line, col) set is a superset of
  llc's (§5.7 columns).
- gdb-multiarch on `qemu-i386 -g` of dbg.c (static), hod vs llc: the
  same transcript after normalising addresses and uninitialised `s`/`t`
  (break add, bt 3 frames, info args/locals, up, print x, print g).

## Next, in order

1. DONE: compare-binaries base 1742ae7e vs head 06430652's code over
   `tests/ir`, `.build/corpus/kiln`, `.build/corpus-x64/kiln`, `-j 4`
   (`.build/target-wp-314/cmp-p2/summary.md`): AArch64 3487 + 3487,
   x86-64 3456 + 3456, i686 111 + 111 comparisons: 0 different, 0
   changed outcome (no i686 module of those corpora has debug info), 159
   identical failures, PASS. Re-run only if hod_dwarf / hod_cg change.
2. `cargo test -q -p hod --test i386_debug -- --nocapture`; then clippy
   workspace, unit tests of hod_dwarf hod_cg, all `i386*`, and
   `debug_frame debug_e2e inlining debug_unit_ranges`; irdiff i686 22/19.
3. Docs: `spec/codegen-x86.md` §7 row `x86:debug-info` -> remove, add
   rows `debug:dwarf-4-i386` (needs dwarf4.rs at address size 4:
   `.debug_ranges`/`.debug_loc` 4-byte pairs, `DW_OP_addr` 4) and
   `debug:split-dwarf-i386` (needs split.rs at address size 4 and the
   `.dwo` path); append §13 (debug information) with the measurements
   above; `spec/dwarf.md` §1 line 48, §4.1, §4.2 table (i386 column),
   §6.4 unit header line 1232: "address size 8 (4 on i386)";
   `crates/hod_x86/README.md:37`; record Part 2 in
   `spec/landed/codegen/WP-314.md` (incl. the xtask debug lane brief:
   `xtask/src/debug.rs:74` `NO_I686_LANE`: needs gdb-multiarch +
   `qemu-i386 -g` in `debug/cross.rs`, clang `--target=i686-linux-gnu`,
   the i686 sysroot linkers; transcripts differ by design where
   parameters are in hod's slots only in `info frame`-style output).
4. Delete the base build dir and base-src; remove this file.
