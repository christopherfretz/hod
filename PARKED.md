# WP-314 parked (session 1 of 2)

Branch `wp/314-i386-eh-debug`, worktree `.worktrees/wp-314`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-314` (base binary built
in `/Volumes/AgentWork/hod-helper/.build/target-wp-314-base/release/hod`,
base sources in `.build/target-wp-314/base-src`: delete both when done,
one literal path per command).

## Done (Part 1, exceptions)

- `hod_eh`: `CfiTarget` field `arch` became `machine: Machine`, new
  fields `address_size` (8 / 4) and `abs32_reloc`; `I386` table,
  `for_machine(machine, large)`; `write_eh_frame`, `write_debug_frame`,
  `personality_ref`, `write_lsdas` take the table by machine, and the
  `DW.ref` slot, stubs and `.debug_frame` addresses are `address_size`
  bytes (64-bit tables keep 8: byte-identical by construction; prove it
  with compare-binaries, below).
- `hod_cg/src/emit/unwind.rs`: the `EM_386` early return removed;
  `write_fdes` uses `hod_eh::for_machine`.
- `hod_x86`: `eh.rs` (landingpad, resume, typeid.for, pad-value
  extract/insert); `call.rs` no longer refuses `x86:eh`; `target.rs`
  arms. Folded invokes now go through the call sequence as on 64-bit.
- Tests `crates/hod/tests/i386_eh.rs` + `tests/data/i386/eh/` (3 tests,
  all pass: 48 program runs x3 builders, 6 shared-library runs, 6
  objects' tables verify + decode as llc's).
- Spec: `spec/codegen-x86.md` §12 + §3 + §7 rows, `spec/eh.md` §2.1,
  README row.

## Measured in session 1

- irdiff i686 full tier: LLVM 22 base 9 PASS / 195 UNSUPPORTED / 0 FAIL,
  head 90 PASS / 111 UNSUPPORTED / 3 FAIL (hod built 3 -> 31 of 68
  objects; `x86:eh` 35 -> 0; newly reached: x86:intrinsic +4,
  x86:float:cast +2, x86:abi:wide-arg +1). LLVM 19: 9 -> 90 PASS, 192 ->
  108 UNSUPPORTED, 3 FAIL. The 3 FAIL rows are `tests/ir/global_ctors.ll`
  (bfd, lld, mold): NOT an EH bug, a hod_cg bug the EH refusal hid:
  `crates/hod_cg/src/emit/mod.rs:1383` `append_zeros(section, 8)` writes
  8-byte `.init_array`/`.fini_array` entries on i386; it should be
  `u64::from(arch.address_bytes())` (8 on 64-bit: byte-identical). Not
  fixed: hod_cg outside authorization; supervisor to decide.
- compare-binaries base 31400b11 vs head: aarch64 and x86_64 0 different,
  0 changed-outcome (3487 + 3456 modules x2 modes); i686: 64 refused ->
  compiles, 18 refused under another name, 8 different (objects now with
  `.eh_frame`), none compiles -> error.
- `cargo clippy --workspace --all-targets -D warnings`: one error in
  `crates/hod/tests/i386_module_asm.rs:367` (too_many_lines), present at
  base, not this package's file. fmt clean.
- Unit tests `-p hod_x86 -p hod_eh -p hod_dwarf -p hod_cg`: all ok.
  `-p hod --test 'i386*' --test debug_frame --test hardening --test
  debug_e2e`: 10 result lines all ok, but the glob may not have selected
  every i386 test: rerun them by name and read the x87 shape counts.

## Remaining, in order

1. irdiff i686 full tier, LLVM 22 and 19, base vs head (command running
   when parked; results in `.build/target-wp-314/irdiff-{base,head}-{22,19}.log`):
   `cargo xtask irdiff --target i686 --llvm /usr/lib/llvm-22 -j 4 --hod <bin> --results <dir>`.
   Remove `;@ kept-off(i686...)` directives of `tests/ir` seeds that now
   build and pass (grep `kept-off(i686` in `tests/ir`, look for class
   `x86:eh`); then `cargo test -q -p xtask --test irdiff_real`.
2. compare-binaries base vs head over `tests/ir`,
   `.build/corpus/kiln`, `.build/corpus-x64/kiln`, `-j 4`, `--out`
   absolute under the build dir: 0 different on AArch64 and x86-64.
3. `cargo fmt --all --check`; `cargo clippy --workspace --all-targets
   -- -D warnings` (hod_eh tests may construct `CfiTarget { arch: .. }`:
   fix if so); `HOD_REQUIRE_TOOLS=1 cargo test -q -p hod_x86 -p hod_eh
   -p hod_dwarf -p hod_cg`; all `i386*` tests of `-p hod` (i386_x87 shape
   counts must stay 288/72/192); `-p hod` EH and debug tests of the
   64-bit targets (`debug_frame`, `debug_e2e`, `hardening`, `inlining`).
   `crates/hod/tests/i386_abi.rs:998` and `hardening.rs:43` mention
   `x86:eh` in comments only; `inlining.rs:1029` names
   `x86:debug-info` (Part 2).
4. Record `spec/landed/codegen/WP-314.md` with the numbers.
5. Not yet tested explicitly: an `invoke` of a callee-pop (`stdcall`,
   `fastcall`) callee and an `sret` `invoke` (add to `live.ll`).

## Part 2 (debug information), not started

- `hod_cg/src/emit/mod.rs:423-428` refuses `x86:debug-info`
  (`machine.arch()` is `None` for i386): `DebugLines::new` takes an
  `Arch`. hod_dwarf's 18 `address_size` sites (design 0008 §1.6) need a
  target parameter (address size 4, `R_386_32`, i386 register numbers
  `reg.rs::dwarf`, frame base `DW_OP_reg5`, parameters `fbreg +8`).
  Keep edits local: WP-313 and WP-305 are editing `hod_dwarf`.
- Measured so far: `llc` 22 i386 `.debug_frame` = CIE version 4,
  address size 4, `7c` data align, RA 8, `0c 04 04 88 01`, entries padded
  to 4, FDE 4-byte location (`R_386_32` to `.text`) and range, CIE
  pointer `R_386_32` to `.debug_frame` (already implemented in hod_eh).
- `gdb-multiarch` exists at `/usr/bin/gdb-multiarch`; `lldb-19` too.
