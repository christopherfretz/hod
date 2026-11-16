# i386 join, part D: WP-316, WP-315 and WP-314 (Part 1)

Varargs, atomics and the dynamic stack (WP-316, `va.rs`, `atomic.rs`,
`dynstack.rs`), memory intrinsics, aggregates, overflow and small
intrinsics and integers of 33 to 63 bits by value (WP-315, `bytes.rs`,
`agg.rs`, `bits::small`) and exceptions (WP-314 Part 1, `eh.rs`,
`hod_eh::I386`) merged in that order onto wave 42 (`8e441c4b`) as
`wave/44`, and made to work together. Behaviour: `spec/codegen-x86.md`
§12 (WP-316), §13 (WP-315), §14 (WP-314); tests
`crates/hod/tests/i386_join_d.rs` (data `tests/data/i386/join_d/`).

## Merges and conflicts

- `cd5e96f3` (WP-316): no conflict.
- `8ce55e2c` (WP-315): `spec/codegen-x86.md` (two hunks: the section
  both packages numbered 12, and the §7 refusal table).
- `3ba79a8a` (WP-314 Part 1, the commit, not the branch head):
  `spec/codegen-x86.md` (two hunks: the section, the table),
  `crates/hod_x86/src/lib.rs` (the module list), `crates/hod_x86/src/target.rs`
  (two hunks: the `extractvalue`/`insertvalue` arms and the
  `va_arg`/`landingpad`/`resume` arms). The branch's `PARKED.md` was
  removed in the merge commit.

## Meeting points

- **Sections.** WP-316 keeps §12; WP-315's section is §13 (13.1 to
  13.5); WP-314's is §14. Every cross-reference was renumbered: the spec
  (§3, §5, §8, §11, the §7 table), `agg.rs`, `bits.rs`, `eh.rs`,
  `i386_agg.rs`, `i386_mem.rs`, `i386_eh.rs`,
  `data/i386/eh/callconv.ll`, `spec/landed/codegen/WP-315.md` and
  `WP-314.md` (each notes the old number).
- **Refusal table (§7).** The union: WP-316's varargs, probe-stack and
  atomic rows, WP-315's `x86:i64` (unreachable), `x86:aggregate` and
  `x86:frame-depth` rows, WP-314's `x86:eh:*` row. `x86:intrinsic` now
  names what is lowered (§13's intrinsics and `eh.typeid.for`, §14).
  The rows of `x86:eh`, `x86:dynamic-alloca` and plain `x86:atomic`,
  which no code raises any more, were dropped.
- **Dispatch (`target.rs`).** Order: ordered `load`/`store` and the
  `cmpxchg` `extractvalue` (`atomic`); x87's and `agg`'s own arms;
  dynamic `alloca`; `eh`'s `extractvalue`/`insertvalue` of the
  `{ ptr, i32 }` pad value (guarded by `eh::owns_aggregate`); the
  general `agg` arms; then `va_arg`, `landingpad`, `resume`. Each
  narrower arm has a guard the wider one does not shadow; the pad value's
  layout (`eax` at 0, `edx` at 4) is the same as `agg`'s, so a pad value
  stored, loaded or passed through a `phi` reads the same either way.
- **x87 barriers (§11).** The union: WP-315's memory intrinsics that
  `llc` makes libc calls (§13.1) and aggregate `select`s that branch
  (§13.2), WP-316's `atomicrmw` loops (§12.4, `atomic::barrier`, one arm
  in `x87::barrier`), and `invoke` (a terminator, §14). The §11 list now
  names all of them. The float `atomicrmw` added here is a loop and was
  already a barrier. The x87 counts are unchanged: 288 shapes equal `llc`'s
  and 72 equal the model; 190 integer-intrinsic shapes equal `llc`'s
  and 2 follow LLVM 22.
- **`cmpxchg` pair.** `atomic::cmpxchg` stores the value at 0 and the
  flag at `field_offset(ty, 1)`; `atomic::extract` and every `agg`
  lowering (aggregate `load`/`store`, `select`, `phi`, `insertvalue`)
  read the same layout. `join_d/cmpxchg.ll` reads the pair both ways at
  every width.

## Small work

- `atomicrmw fadd`/`fsub` of `float` and `double` are lowered as
  `llc`'s loop (§12.2); `x86:atomic:float-op` now covers `fmax`, `fmin`
  and the other floating operations (`llc` calls libm inside the loop).
  Test: `join_d/fatomic.ll`.
- `eh.typeid.for` is lowered by WP-314 (`eh.rs`) and no longer refused
  as `x86:intrinsic`.

## Bugs found where the packages meet

- `dynstack::alloca` (WP-316) refused a count of 33 to 64 bits with
  `x86:i64`, which WP-315 made unreachable everywhere else
  (`tests/ir/alloca_dynamic.ll` declined). The count now gives its
  slot's low word, as `llc` truncates it (`553a87b5`; test
  `join_d/alloca_wide.ll`, counts above 2^32 included).
- `Target::lower` grew to 110 lines with the three packages' arms
  (clippy's limit is 100). The `eh`-or-`agg` choice for
  `extractvalue`/`insertvalue` moved to two helpers, and the two dead
  `mem::ATOMIC` arms went (`2b1fdd3b`).
- `i386_atomic::what_stays_refused` expected `fadd` to be refused; it
  now expects `fmax` and compiles `fadd`/`fsub` (`d2b36066`).

No miscompilation was found: every join D program ran as `llc`'s at
every point in the matrix.

## Evidence

At `553a87b5`:

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D
  warnings`: clean.
- `HOD_REQUIRE_TOOLS=1 cargo test -p hod_x86 -p hod_eh -p hod_cg -p
  hod_ir -p xtask`: 907 passed, 0 failed.
- Every `i386*` test of `-p hod` (21 files) and `hod_x86`: 96 passed, 0
  failed. x87: 288 shapes equal `llc`'s, 72 equal the model; 190
  integer-intrinsic shapes equal `llc`'s, 2 follow LLVM 22; 224 barrier
  shape lines equal `llc`'s. `i386_join_d.rs`: 5 programs, 72 runs each
  (`llc` 19 and 22, `generic` and `i686`, static and PIE, GNU ld, lld,
  mold, plain and under `Stress::ALL`).
- 64-bit EH (`debug_frame`, `hardening`, `inlining`): 22 passed.
- `cargo xtask irdiff --target i686 --tier all`: LLVM 22, 123 PASS, 0
  FAIL, 0 ERROR, 81 UNSUPPORTED, 44 SKIP; LLVM 19, 123 PASS, 0 FAIL, 0
  ERROR, 78 UNSUPPORTED, 45 SKIP (wave 42: 9 PASS). Objects declined (LLVM 22):
  `coro:target` 5, `x86:intrinsic` 4, `x86:cf-protection` 3,
  `x86:vector` 3, `x86:abi:wide-arg` 2, `x86:float:cast` 2,
  `x86:stack-protector` 2, `x86:atomic:op` 1, `x86:constant` 1,
  `x86:float:fp128` 1 (LLVM 22 only), `x86:indirect-branch` 1,
  `x86:inline-asm:goto` 1, `attr:alignstack` 1. Gone since wave 42:
  `x86:eh`, `x86:i64`, `x86:aggregate`; `eh.typeid.for` is not among
  the `x86:intrinsic` ones (`coro/noop`, `drift_saturation`,
  `dwarf_cfa`, `frame_intrinsics`).
- `cargo xtask compare-binaries` against wave 42 over `tests/ir` and the
  kiln corpora (`corpus`, `corpus-x64`), `-j 4`: AArch64 6974 and
  x86-64 6912 comparisons, 0 different, 0 changed outcomes. i686 (111
  modules, plain and PIC): 88 changed outcomes are modules wave 42
  refused and head builds; 24 are refusals that moved to a later
  construct (`x86:eh` to `x86:intrinsic`/`x86:float:cast`/
  `x86:abi:wide-arg`, `x86:i64` to `x86:stack-protector`, `x86:aggregate`
  to `x86:constant`/`x86:float:fp128`, `x86:atomic` to `x86:atomic:op`,
  one `x86:intrinsic` to another); 8 differences (`exit_float`,
  `exit_int`, `exit_odd_widths`, `odd_width_store`) have the same
  `.text` and add `.eh_frame` (WP-314's unwind tables); no module wave
  42 built is refused.

## Found on the way

- `bitcast float to i32` (a floating `bitcast`) is still refused as
  `x86:float:cast` (§7); `fatomic.ll` reads the bits with a `load`.
- Dead code left: `frame.rs`'s `DYNAMIC_ALLOCA`/`dynamic_slot`
  (`target.rs` calls `dynstack::dynamic_slot`).
- No i686 corpus exists: `cargo xtask corpus` refuses `--target i686`
  (`xtask/src/corpus.rs` 124-127, kiln has no 32-bit target).

## What remains

- `atomicrmw fmax`/`fmin` (`x86:atomic:float-op`): a libm call inside
  the `cmpxchg` loop.
- A floating `bitcast` (`x86:float:cast`).
- The refusals of the census above.
- WP-314 Part 2 (debug information) is not in this join.
