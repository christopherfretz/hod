# Backlog

One row per work package. Status: `todo`, `in-progress`, `review`, `done`.
Rows are added by the supervisor; the format of a WP is in `docs/AGENTS.md`.
Milestones and tracks are in `docs/ROADMAP.md`.

## M0: foundations

| WP | track | scope | depends on | status |
|---|---|---|---|---|
| WP-001 | T-H | Cargo workspace, lints (`forbid(unsafe_code)`, clippy pedantic), `rust-toolchain.toml`, `scripts/env.sh`, empty crates per `docs/ARCHITECTURE.md`, `xtask ci --quick` (fmt, clippy, test), GitHub Actions running it on aarch64 and x86_64 | - | done |
| WP-002 | T-IR | `hod_ir` types and constants per `spec/ir.md` §3-§4: the type table with layout, constant pool, address normal forms and folding, interning, sizes asserted | WP-001 | done |
| WP-003 | T-IR | `hod_ir` module level: globals, functions, aliases, IFUNCs, COMDATs, attributes, module flags; the builder API for producers | WP-002 | todo |
| WP-004 | T-IR | `hod_ir` functions: blocks, instructions, operand pools, call and landing-pad side tables; function-at-a-time delivery | WP-002 | todo |
| WP-005 | T-IR | `hod_ir` debug metadata: typed `DI*` nodes, `DILocation`, debug records | WP-003 | todo |
| WP-006 | T-IR | `hod_ir` printer (LLVM text, version-selectable) | WP-003, WP-004 | todo |
| WP-007 | T-IR | `hod_ll` lexer and the module-level parser (types, globals, declarations, attributes, metadata) | WP-003 | todo |
| WP-008 | T-IR | `hod_ll` function bodies: every instruction `hod_ir` models | WP-004, WP-007 | todo |
| WP-009 | T-IR | `hod_ir::classify`: the support matrix as code, with one named reason per rejected construct; `docs/status/support.md` generated from it | WP-004 | todo |
| WP-010 | T-OBJ | `spec/object.md` + `hod_obj` model and ELF64 writer: sections, symbols, RELA relocations, string tables, `.note.GNU-stack` | WP-001 | done |
| WP-011 | T-OBJ | `hod_obj` follow-ups to WP-010 (COMDAT groups landed there): link-and-run tests for TLS (local-exec, initial-exec, TLSDESC), `.init_array` constructors (and a helper for priority names), linker merging of mergeable sections; a `.note.gnu.property` helper (BTI/PAC, CET); `SHF_LINK_ORDER`/`sh_link` for caller sections; groups with debug members, `GroupKind::Plain`, references into a discarded group; x86-64 link-and-run under emulation | WP-010 | todo |
| WP-012 | T-A64 | `spec/mc-a64.md` + `hod_mc_a64`: base integer set (data processing, moves, loads/stores with all addressing modes, branches, system), fixups; L1 sweep harness against `llvm-mc` | WP-001 | done |
| WP-013 | T-A64 | `hod_mc_a64`: scalar floating point and the SIMD subset codegen needs for `float`/`double`/vectors-as-values: an FP/vector register type, FP/SIMD loads and stores (all addressing modes, pairs, literal), FP constant materialization next to `mov_imm`; the L1 harness run with `+fp-armv8,+neon` | WP-012 | todo |
| WP-014 | T-CG | `spec/codegen.md`: the `Target` trait, value-location policy v1, frame layout, the per-function output record. Design only; reviewed before T-CG work starts | WP-004, WP-010 | todo |
| WP-015 | T-H | `hod_cli`: `llc`-compatible command line, stdin input, `--fallback`, coverage log; the `--llvm-dir` shim directory for kiln | WP-001 | done |
| WP-016 | T-H | `xtask irdiff`: build with `llc` and with hod, link with each linker in the matrix, run under limits, compare; `tests/ir/` directive format | WP-015 | todo |
| WP-017 | T-H | `xtask corpus`: capture IR from kiln's executing tests and from clang on the GCC torture suite into `.build/corpus`, pinned and reproducible | WP-015 | todo |
| WP-018 | T-H | `xtask bench` and `bench/`: size classes, wall/RSS/size measurement for hod and `llc`, baselines, phase timings | WP-015 | todo |
| WP-019 | T-H | L2 gate: parse, print, `llvm-as` acceptance and behavioral equivalence through `llc` over the captured corpus | WP-006, WP-008, WP-017 | todo |
| WP-020 | T-H | Bitcode on the text path: `hod` converts bitcode input to text with a configured `llvm-dis` (`--bitcode-via=`, set by the shim) before parsing, instead of falling back; coverage log records the conversion time. Design 0001 point 6 | WP-015 | todo |
| WP-021 | T-IR | `available_externally` + `alwaysinline` bodies: producers mark them, `hod` emits a local copy and redirects the module's calls (design 0001 point 6); test with libstdc++'s extern-template cases | WP-003, WP-004 | todo |
| WP-022 | T-H | Coverage log: a ninth field with the fallback's exit status; per-function records (which functions hod compiled, which construct stopped it) for function-level coverage; measure the shim's per-TU `sh` exec and remove it if it shows | WP-015 | todo |

## M1 and later

Rows are written when the interfaces they depend on (WP-014 for T-CG, T-EH
and T-DW) are reviewed. Planned shape:

- T-CG: one WP per instruction group per target (integer arithmetic and
  compares; memory and GEP; control flow; calls and the argument assigner;
  varargs; floating point; aggregates as values; intrinsics by family).
- T-EH: CFI; LSDA.
- T-DW: line tables; unit and subprogram skeleton; types; variables;
  split DWARF.
- T-XF: coroutine splitting (design WP first).
- T-X64: encoder WPs mirroring WP-012/013.
- T-H: `xtask bisect`; L5 ABI grid generator; kiln corpus and libc++
  runners in hybrid and strict mode; fuzzing drivers.
