# i386 join, part E: WP-314 (Part 2) and WP-317

Debug information (WP-314 Part 2, `hod_dwarf` at address size 4) and
scalar SSE / SSE2 with 128-bit vectors (WP-317, `sse.rs`, `vector.rs`)
merged in that order onto `wave/44` (`f492182b`: join D and three
performance packages) as `wave/45`, and made to work together.
Behaviour: `spec/codegen-x86.md` §15 (debug information), §16 (scalar
SSE), §17 (vectors); tests `crates/hod/tests/i386_join_e.rs` (data
`tests/data/i386/join_e/`).

## Stage 1: WP-314 Part 2 (`e61710bd`)

### Merge and meeting points

- Merge `5f873503`. One conflict: `spec/codegen-x86.md`'s §7 table.
  `git` merged `crates/hod_dwarf`, `crates/hod_cg/src/emit/lines.rs`
  and `emit/mod.rs` (WP-320's and WP-321's decode call sites beside the
  branch's address size, relocation kind, register numbers and
  `DebugLines::new`'s machine) and `crates/hod_x86/src/target.rs` (the
  branch's `emit_call` beside the base's `Target::lower` helpers)
  without a conflict; the proof is below (byte-identical 64-bit objects,
  the counts gate).
- **Sections.** Exceptions stay §14 (the base's text; Part 2 did not
  edit it); debug information, the branch's §13, is §15. Its "as in §12"
  (unwind tables) now reads §14. References moved to §15:
  `spec/dwarf.md:48`, `crates/hod_dwarf/src/target.rs:38`,
  `split.rs:143`, `loclists.rs:623`, `crates/hod_cg/src/emit/mod.rs:420`,
  `crates/hod_x86/README.md:39`, `crates/hod/tests/i386_debug.rs:2`
  (comments only).
- **Refusal table (§7).** `x86:debug-info` replaced by
  `debug:dwarf-4-i386` and `debug:split-dwarf-i386`; the base's rows of
  `x86:aggregate`, `x86:frame-depth` and `x86:eh:*` kept.

### New with the join

- Debug information of join D's constructs:
  `tests/data/i386/join_e/dbg_eh.cpp` (exceptions through `invoke` and
  landing pads, `std::atomic` of 4 and 8 bytes, a struct of `long
  long`, `int` and `float` returned through an `invoke`),
  `dbg_va.cpp` (variadic functions with `va_copy`, `va_arg` of `int`,
  `long long`, `double` and a pointer, structs by value and as
  results), `dbg_vla.cpp` (a VLA in a loop with a `double` and a `long
  long` live across it), by clang 19 and 22 at `-O0 -g`, static and
  PIC: hod's objects pass `llvm-dwarfdump --verify` of each major, and
  run as `llc`'s (GNU ld, lld, mold; plain and `Stress::ALL`).
  Tests: `i386_join_e.rs` `debug_info_of_*`: 4 objects verified and 36
  runs equal `llc`'s per program. No bug was found between the packages
  in stage 1.

### Evidence (stage 1, at the merge plus the tests)

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --
  -D warnings`: clean.
- `HOD_REQUIRE_TOOLS=1 cargo test -p hod_x86 -p hod_eh -p hod_dwarf -p
  hod_cg`: 235 passed, 0 failed.
- Every `i386*` test of `-p hod` (23 files, `i386_debug.rs` and
  `i386_join_e.rs` new): 73 passed, 0 failed. x87: 288 shapes equal
  `llc`'s, 72 equal the model; 190 integer-intrinsic shapes equal
  `llc`'s, 2 follow LLVM 22; 224 barrier shape lines equal `llc`'s.
  `i386_debug`: 12 objects verified, trees and rows equal `llc`'s (the
  epilogue-row rule of §15 for `eh.cpp`); 2 objects of `-O1` IR.
- 64-bit debug tests (`debug_frame`, `debug_e2e`, `inlining`,
  `debug_unit_ranges`): 24 passed.
- `cargo xtask irdiff --target i686`: LLVM 22, 126 PASS, 0 FAIL, 0
  ERROR, 81 UNSUPPORTED, 44 SKIP; LLVM 19, 126 PASS, 0 FAIL, 0 ERROR,
  78 UNSUPPORTED, 45 SKIP (base 123: the seed
  `tests/ir/debug_info_locals.ll` adds 3). Declined (LLVM 22):
  `coro:target` 5, `x86:intrinsic` 4, `x86:cf-protection` 3,
  `x86:vector` 3, `x86:abi:wide-arg` 2, `x86:float:cast` 2,
  `x86:stack-protector` 2, `x86:atomic:op` 1, `x86:constant` 1,
  `x86:float:fp128` 1, `x86:indirect-branch` 1, `x86:inline-asm:goto`
  1, `attr:alignstack` 1 (as join D).
- `cargo xtask compare-binaries` base `f492182b` vs head `5f873503`
  over `tests/ir`, `corpus/kiln`, `corpus-x64/kiln`, `-j 4`: 6833
  modules, 14114 comparisons; AArch64 6976 and x86-64 6914 comparisons,
  0 different, 0 changed outcomes. i686: 2 changed outcomes, both
  `tests/ir/debug_info_locals.ll` (plain, PIC): refused `x86:debug-info`
  before, compiles now; 0 different.
- Counts gate: `cargo xtask bench --check --counts` 4712.4 M (base
  4711.9 M, +0.01%), `--target x86_64` 4832.5 M (base 4831.8 M,
  +0.01%): 0 REGRESSED, 0 LOST, each against the committed baseline.

## Stage 2: WP-317 (`0714a2ec`)

### Merge

- Merge `28994373`. Conflicts resolved: `target.rs`'s dispatch (through
  `sse_family_owns` / `sse_family`, after `atomic`'s arms and before
  `x87`'s and `agg`'s), `materialize_mem` (agg, vector, sse, x87),
  `lib.rs`, the README rows, `spec/codegen-x86.md` §16 (scalar SSE), §17
  (vectors), §7 (the union of both tables).

### New with the join (tests `i386_join_e.rs`, data `join_e/`)

- `sse_eh.ll`: an `invoke` of SSE results whose landing pads use them;
  `sse_va.c`: varargs of SSE values and vectors; `sse_agg.c`: aggregates
  with SSE and vector leaves, `memcpy`, a dynamic `alloca`;
  `sse_atomic.ll`: `atomicrmw fadd`/`fsub`/`xchg`, atomic loads and
  stores of `float` / `double` in SSE2 functions; `sse_dbg.c` (`-g`,
  SSE2, `_Float16`): hod's DIE tree equals `llc`'s (4 trees). Runs
  equal `llc`'s (19 and 22, default CPU and `-mcpu=i686` for the `.ll`
  programs whose functions carry `pentium4`, static and PIE, GNU ld,
  lld, mold, plain and `Stress::ALL`): 72, 36, 36, 72, 6.
- What WP-317's evidence left thin: `sse_lanes.ll` (edge values through
  every lane: integer grids of zero, one, minus one, the extremes, a bit
  pattern, a small negative and a power of two at 8, 16, 32 and 64 bits;
  float grids of both zeros, one, minus one, the largest finite, the
  smallest normal, subnormals, both infinities, a quiet and a signalling
  NaN; every operation and compare predicate, `select`, reductions,
  casts; an `x86_fp80` live across vector code): 4544 output lines, 72
  runs equal `llc`'s. `sse_atomic_bits.ll`: an 8-byte atomic load is
  `movq` in `llc` at SSE2 and `fild`/`fistp` in hod; both are one 8-byte
  access and give the same value for every pattern (`fild` of a 64-bit
  integer is exact in the 64-bit significand, `fistp` of an integral
  value is exact in every rounding mode); the test runs zero, one, all
  ones, both extremes, `double` signalling and quiet NaNs of both signs,
  infinities, subnormals: 72 runs equal `llc`'s. hod keeps the x87 pair.
- The dispatch does not shadow `atomic` / `agg` (`sse_atomic.ll`,
  `sse_agg.c`); an atomic load of a vector is still `x86:atomic:type`.
- No bug was found between the packages in stage 2.

## Stage 3: `bitcast` on the x87 path

- `x87::bitcast` (§11 "`bitcast`"): `float` <-> `i32`, `double` <->
  `i64` through the homes; the bits of the rounded value (measured `llc`
  22 at `generic`: `fstps (%esp); movl (%esp), %eax` of a product, no
  branch: not a barrier); no bits through the x87 stack (signalling NaN
  patterns kept). `x86_fp80` <-> `i80`: `x86:float:fp80-cast` (new); a
  vector side: the vector's refusal. x87 shape `bitcast_between` (float
  and double); `x87_bitcast.ll`: 72 runs equal `llc`'s.
- **Bug found once `bitcast` compiled** (`tests/ir/casts.ll` FAILed,
  `f32=` of `uitofp i64 to float`): `x87::from_i64` kept an 80-bit copy
  of an unsigned conversion; `llc` 19 and 22 round it at once
  (`fstps`/`flds`, `fstpl`/`fldl`). Fixed: an unsigned result is not
  wide. `casts.ll` now passes.

## Evidence (head `746a9235`)

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --
  -D warnings`: clean.
- `HOD_REQUIRE_TOOLS=1 cargo test -q -p hod_x86 -p hod_cg -p hod_dwarf
  -p hod_eh -p hod_obj -p xtask`: 741 passed, 0 failed.
- Every `i386*` test of `-p hod` (25 files with `i386_sse`,
  `i386_vector`): 94 passed, 0 failed. x87 shapes 296 equal `llc`'s, 72
  equal the model (46 shapes); 190 integer-intrinsic shapes, 2 follow
  LLVM 22; 224 barrier shape lines; 276 SSE shapes; vector lanes 8 runs.
- `debug_e2e`, `debug_frame`, `split_dwarf`, `object_file`: 12 passed.
- `cargo xtask irdiff --target i686`: LLVM 22 129 PASS, 0 FAIL, 0 ERROR,
  78 UNSUPPORTED, 44 SKIP; LLVM 19 129 PASS, 0 FAIL, 0 ERROR, 75
  UNSUPPORTED, 45 SKIP (`casts.ll` passes; `x86:float:cast` declines
  nothing). Declined (LLVM 22): `coro:target` 5, `x86:intrinsic` 4,
  `x86:vector:no-sse2` 4, `x86:cf-protection` 3, `x86:abi:wide-arg` 2,
  `x86:stack-protector` 2, `x86:atomic:op` 1, `x86:constant` 1,
  `x86:float:fp128` 1, `x86:indirect-branch` 1, `x86:inline-asm:goto`
  1, `attr:alignstack` 1. The vector seeds `vectors.ll`,
  `vector_masks.ll`, `drift_lanes.ll` and now `vector_arith.ll` have no
  `target-cpu`: `x86:vector:no-sse2` (as `llc`, which scalarizes).
- `cargo xtask compare-binaries` base `f492182b` vs head, `tests/ir`,
  `corpus/kiln`, `corpus-x64/kiln`, `-j 4`: 14114 comparisons; AArch64
  and x86-64 0 different, 0 changed outcomes; i686 0 different, 16
  changed outcomes: compiles now `casts.ll`, `debug_info_locals.ll`;
  narrower names `x86:vector` -> `x86:vector:no-sse2` (`vectors.ll`,
  `vector_masks.ll`, `drift_lanes.ll`, `vector_lanes.ll`),
  `x86:float:cast` -> `x86:vector:no-sse2` (`vector_arith.ll`),
  `x86:float` -> `x86:float:half` (`half_fp128.ll`); each plain and
  PIC; none to an error or a panic.
- Counts: `--counts` 4712.4 M, `--target x86_64` 4832.3 M: 0 REGRESSED,
  0 LOST (both 1.8% under the committed baseline: re-record).

## What remains

- Vectors: `x86:vector:no-sse2` (no `target-cpu`: `llc` scalarizes),
  the `x86:vector:*` refusals of §17 (`i64-op`, `cast`, `frem`,
  `intrinsic`, `wide`, `abi`); which of them the corpus reaches first
  was not measured.
- The debug lane's i686 column (running kiln-built `-g` programs under a
  debugger) is not part of this join.
