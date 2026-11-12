# Backlog

One row per work package. Status: `todo`, `in-progress`, `review`, `done`.
Rows are added by the supervisor; the format of a WP is in `docs/AGENTS.md`.
Milestones and tracks are in `docs/ROADMAP.md`.

## M0: foundations

| WP | track | scope | depends on | status |
|---|---|---|---|---|
| WP-001 | T-H | Cargo workspace, lints (`forbid(unsafe_code)`, clippy pedantic), `rust-toolchain.toml`, `scripts/env.sh`, empty crates per `docs/ARCHITECTURE.md`, `xtask ci --quick` (fmt, clippy, test), GitHub Actions running it on aarch64 and x86_64 | - | done |
| WP-002 | T-IR | `hod_ir` types and constants per `spec/ir.md` §3-§4: the type table with layout, constant pool, address normal forms and folding, interning, sizes asserted | WP-001 | done |
| WP-003 | T-IR | `hod_ir` module level: globals, functions, aliases, IFUNCs, COMDATs, attributes, module flags; the builder API for producers | WP-002 | in-progress |
| WP-004 | T-IR | `hod_ir` functions: blocks, instructions, operand pools, call and landing-pad side tables; function-at-a-time delivery | WP-002 | in-progress |
| WP-005 | T-IR | `hod_ir` debug metadata: typed `DI*` nodes, `DILocation`, debug records | WP-003 | todo |
| WP-006 | T-IR | `hod_ir` printer (LLVM text, version-selectable) | WP-003, WP-004 | todo |
| WP-007 | T-IR | `hod_ll` lexer and the module-level parser (types, globals, declarations, attributes, metadata) | WP-003 | todo |
| WP-008 | T-IR | `hod_ll` function bodies: every instruction `hod_ir` models | WP-004, WP-007 | todo |
| WP-009 | T-IR | `hod_ir::classify`: the support matrix as code, with one named reason per rejected construct; `docs/status/support.md` generated from it | WP-004 | todo |
| WP-010 | T-OBJ | `spec/object.md` + `hod_obj` model and ELF64 writer: sections, symbols, RELA relocations, string tables, `.note.GNU-stack` | WP-001 | done |
| WP-011 | T-OBJ | `hod_obj` follow-ups to WP-010 (COMDAT groups landed there): link-and-run tests for TLS (local-exec, initial-exec, TLSDESC), `.init_array` constructors (and a helper for priority names), linker merging of mergeable sections; a `.note.gnu.property` helper (BTI/PAC, CET); `SHF_LINK_ORDER`/`sh_link` for caller sections; groups with debug members, `GroupKind::Plain`, references into a discarded group; x86-64 link-and-run under emulation | WP-010 | in-progress |
| WP-012 | T-A64 | `spec/mc-a64.md` + `hod_mc_a64`: base integer set (data processing, moves, loads/stores with all addressing modes, branches, system), fixups; L1 sweep harness against `llvm-mc` | WP-001 | done |
| WP-013 | T-A64 | `hod_mc_a64`: scalar floating point and the SIMD subset codegen needs for `float`/`double`/vectors-as-values: an FP/vector register type, FP/SIMD loads and stores (all addressing modes, pairs, literal), FP constant materialization next to `mov_imm`; the L1 harness run with `+fp-armv8,+neon` | WP-012 | done |
| WP-014 | T-CG | `spec/codegen.md`: the `Target` trait, value-location policy v1, frame layout, the per-function output record. Design only; reviewed before T-CG work starts | WP-004, WP-010 | review |
| WP-015 | T-H | `hod_cli`: `llc`-compatible command line, stdin input, `--fallback`, coverage log; the `--llvm-dir` shim directory for kiln | WP-001 | done |
| WP-016 | T-H | `xtask irdiff`: build with `llc` and with hod, link with each linker in the matrix, run under limits, compare; `tests/ir/` directive format | WP-015 | in-progress |
| WP-017 | T-H | `xtask corpus`: capture IR from kiln's executing tests and from clang on the GCC torture suite into `.build/corpus`, pinned and reproducible | WP-015 | todo |
| WP-018 | T-H | `xtask bench` and `bench/`: size classes, wall/RSS/size measurement for hod and `llc`, baselines, phase timings | WP-015 | todo |
| WP-019 | T-H | L2 gate: parse, print, `llvm-as` acceptance and behavioral equivalence through `llc` over the captured corpus | WP-006, WP-008, WP-017 | todo |
| WP-020 | T-H | Bitcode on the text path: `hod` converts bitcode input to text with a configured `llvm-dis` (`--bitcode-via=`, set by the shim) before parsing, instead of falling back; coverage log records the conversion time. Design 0001 point 6 | WP-015 | todo |
| WP-021 | T-IR | `available_externally` + `alwaysinline` bodies: producers mark them, `hod` emits a local copy and redirects the module's calls (design 0001 point 6); test with libstdc++'s extern-template cases | WP-003, WP-004 | todo |
| WP-022 | T-H | Coverage log: a ninth field with the fallback's exit status; per-function records (which functions hod compiled, which construct stopped it) for function-level coverage; measure the shim's per-TU `sh` exec and remove it if it shows | WP-015 | todo |

## M1: C (aarch64)

Interfaces: `spec/codegen.md`. Rows WP-035 and later are split by
instruction group and can run in parallel once WP-033 has landed.

| WP | track | scope | depends on | status |
|---|---|---|---|---|
| WP-030 | T-CG | `hod_cg` core per `spec/codegen.md` §2-§6, §8-§11: `Target` trait and `FnCx` in Rust, location policy v1, frame allocator, labels and fixups with far-branch retry, reachability, the `phi` protocol, the call sequence on top of `assign`, `Chunk`/`FuncRecord`; the test target and the policy tests of §12 | WP-004, WP-014, WP-031 | todo |
| WP-031 | T-H | `hod_interp`: a reference interpreter for `hod_ir` bodies (integers, pointers into a flat memory, calls between bodies and to a few host functions), and the body generator of `hod_ir`'s tests made reusable; unsupported constructs are named, not guessed | WP-003, WP-004 | todo |
| WP-032 | T-CG | `hod_cg::emit_module` per `spec/codegen.md` §7.3, §7.4, §9.7: symbols up front, sections for functions and variables as `llc` chooses them, `flatten` fixups to relocations, aliases, IFUNCs, constructors, runtime-function symbols, chunk merge; `sym_access` tested against `llc`'s relocations | WP-003, WP-030 | todo |
| WP-033 | T-A64 | `hod_a64` skeleton per `spec/codegen.md` §5.5: `RegInfo`, `class_of`, prologue/epilogue/CFI rows, glue, constant and symbol-address materialization, `Ret`, `Br`, `Unreachable`; a builder-made module that returns 42 links with every linker and runs | WP-030, WP-032 | todo |
| WP-034 | T-A64 | `spec/abi-a64.md` and `assign` for AArch64 (scalars, `sret`, `byval`, aggregates and `i128` as values, variadic calls and callees, `musttail`); the L5 grid generator against clang in both directions | WP-030 | todo |
| WP-035 | T-A64 | integer arithmetic, shifts, division, compares, casts between integers and pointers, `select`, `freeze`; compare-and-branch fusion | WP-033 | todo |
| WP-036 | T-A64 | memory: `Load`/`Store` of every `Gpr` width incl. odd widths, stack slots, `PtrAdd` with fusion into addressing modes, dynamic `Alloca`, over-aligned slots, `stacksave`/`stackrestore` | WP-033 | todo |
| WP-037 | T-A64 | control flow: `CondBr`, `Switch`, `phi`, `IndirectBr` and block addresses | WP-033 | todo |
| WP-038 | T-A64 | calls: direct and indirect, results, `sret`/`byval`, stack arguments, `noreturn`; variadic callees and `VaArg`/`va_*` | WP-033, WP-034 | todo |
| WP-039 | T-A64 | floating point: arithmetic, compares, conversions, constants, loads and stores, FP arguments | WP-013, WP-033 | todo |
| WP-040 | T-A64 | `Mem`-class values: `i128` arithmetic, first-class aggregates (`ExtractValue`, `InsertValue`, loads, stores, `select`, `phi`), overflow intrinsics | WP-035, WP-036 | todo |
| WP-041 | T-A64 | the intrinsics C code produces: `memcpy`/`memmove`/`memset`, bit manipulation, `abs`/min/max, `trap`, `expect`-class no-ops, `frameaddress`/`returnaddress` | WP-035, WP-036 | todo |
| WP-042 | T-EH | `spec/eh.md` (CFI part) and `hod_eh`: `.eh_frame` from `FuncRecord` frame descriptions, CIE sharing; checked with `llvm-dwarfdump --eh-frame` and by unwinding through hod frames | WP-030 | todo |
| WP-043 | T-H | wire code generation into `hod::compile` (parse with `hod_ll`, `classify`, compile, emit), per-construct reasons in `spec/cli.md`, `docs/status/support.md` | WP-008, WP-009, WP-033 | todo |

## M1 and later

Rows are written when the interfaces they depend on are reviewed. Planned
shape:

- T-CG: the M2 instruction groups (exceptions, TLS, atomics, vectors,
  `musttail`), then the same groups for x86-64.
- T-EH: LSDA.
- T-DW: line tables; unit and subprogram skeleton; types; variables;
  split DWARF.
- T-XF: coroutine splitting (design WP first).
- T-X64: encoder WPs mirroring WP-012/013.
- T-H: `xtask bisect`; L5 ABI grid generator; kiln corpus and libc++
  runners in hybrid and strict mode; fuzzing drivers.
