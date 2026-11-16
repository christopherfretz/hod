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
