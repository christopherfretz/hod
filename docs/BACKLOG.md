# Backlog

One row per work package. Status: `todo`, `in-progress`, `review`, `done`.
Rows are added by the supervisor; the format of a WP is in `docs/AGENTS.md`.
Milestones and tracks are in `docs/ROADMAP.md`.

## M0: foundations

| WP | track | scope | depends on | status |
|---|---|---|---|---|
| WP-001 | T-H | Cargo workspace, lints (`forbid(unsafe_code)`, clippy pedantic), `rust-toolchain.toml`, `scripts/env.sh`, empty crates per `docs/ARCHITECTURE.md`, `xtask ci --quick` (fmt, clippy, test), GitHub Actions running it on aarch64 and x86_64 | - | done |
| WP-002 | T-IR | `hod_ir` types and constants per `spec/ir.md` §3-§4: the type table with layout, constant pool, address normal forms and folding, interning, sizes asserted | WP-001 | done |
| WP-003 | T-IR | `hod_ir` module level: globals, functions, aliases, IFUNCs, COMDATs, attributes, module flags; the builder API for producers | WP-002 | done |
| WP-004 | T-IR | `hod_ir` functions: blocks, instructions, operand pools, call and landing-pad side tables; function-at-a-time delivery | WP-002 | done |
| WP-005 | T-IR | `hod_ir` debug metadata: typed `DI*` nodes, `DILocation`, debug records. What `spec/codegen.md` §4.1 and §9.5 read: several variables per stack slot, each with its debug expression (`SlotVar` has none yet); the per-value fact "named by a `#dbg_declare`" for a value that is not a slot; `#dbg_label` positions | WP-003, WP-004 | done |
| WP-006 | T-IR | `hod_ir` printer (LLVM text, version-selectable) | WP-003, WP-004 | done |
| WP-007 | T-IR | `hod_ll` lexer and the module-level parser (types, globals, declarations, attributes, metadata) | WP-003 | done |
| WP-008 | T-IR | `hod_ll` function bodies: every instruction `hod_ir` models, on the interface of `spec/ll.md` §6 (bodies as spans, parallel over functions, blocks numbered in text order with the entry 0, parameter names from `FunctionText::params`, rejected attributes of a call-site group recorded as `attr:<name>`); closes `blockaddress:unlabelled-block` | WP-004, WP-007 | in-progress |
| WP-009 | T-IR | `hod_ir::classify`: the support matrix as code, with one named reason per rejected construct; `docs/status/support.md` generated from it. Rejects what `spec/codegen.md` §1 lists (a thread-local symbol as an operand of anything but `llvm.threadlocal.address`, cross-function `blockaddress`, `naked`, `callbr`, the `musttail` cases), `inst:va_arg`, scalable-vector atomics, non-zero address spaces, everything in `Module::unsupported()` and every non-ignorable entry of `other_flags()`. Also the module-level checks `Body::verify` cannot make yet: the body's symbol is a function of the body's type, and every symbol and inline-assembly operand exists | WP-004 | done |
| WP-010 | T-OBJ | `spec/object.md` + `hod_obj` model and ELF64 writer: sections, symbols, RELA relocations, string tables, `.note.GNU-stack` | WP-001 | done |
| WP-011 | T-OBJ | `hod_obj` follow-ups to WP-010 (COMDAT groups landed there): link-and-run tests for TLS (local-exec, initial-exec, TLSDESC), `.init_array` constructors (and a helper for priority names), linker merging of mergeable sections; a `.note.gnu.property` helper (BTI/PAC, CET); `SHF_LINK_ORDER`/`sh_link` for caller sections; groups with debug members, `GroupKind::Plain`, references into a discarded group; x86-64 link-and-run under emulation | WP-010 | done |
| WP-012 | T-A64 | `spec/mc-a64.md` + `hod_mc_a64`: base integer set (data processing, moves, loads/stores with all addressing modes, branches, system), fixups; L1 sweep harness against `llvm-mc` | WP-001 | done |
| WP-013 | T-A64 | `hod_mc_a64`: scalar floating point and the SIMD subset codegen needs for `float`/`double`/vectors-as-values: an FP/vector register type, FP/SIMD loads and stores (all addressing modes, pairs, literal), FP constant materialization next to `mov_imm`; the L1 harness run with `+fp-armv8,+neon` | WP-012 | done |
| WP-014 | T-CG | `spec/codegen.md`: the `Target` trait, value-location policy v1, frame layout, the per-function output record. Design only; reviewed before T-CG work starts | WP-004, WP-010 | done |
| WP-015 | T-H | `hod_cli`: `llc`-compatible command line, stdin input, `--fallback`, coverage log; the `--llvm-dir` shim directory for kiln | WP-001 | done |
| WP-016 | T-H | `xtask irdiff`: build with `llc` and with hod, link with each linker in the matrix, run under limits, compare; `tests/ir/` directive format | WP-015 | done |
| WP-017 | T-H | `xtask corpus`: capture IR from kiln's executing tests and from clang on the GCC torture suite into `.build/corpus`, pinned and reproducible | WP-015 | done |
| WP-018 | T-H | `xtask bench` and `bench/`: size classes, wall/RSS/size measurement for hod and `llc`, baselines, phase timings | WP-015 | done |
| WP-019 | T-H | L2 gate: parse, print, `llvm-as` acceptance and behavioral equivalence through `llc` over the captured corpus | WP-006, WP-008, WP-017 | todo |
| WP-020 | T-H | Bitcode on the text path: `hod` converts bitcode input to text with a configured `llvm-dis` (`--bitcode-via=`, set by the shim) before parsing, instead of falling back; coverage log records the conversion time, and (from WP-022) a field with the fallback's exit status. Design 0001 point 6. The corpus then captures the modules kiln hands `llc` as bitcode (97 tests per LLVM major run `opt` first: `-O1` and up, coroutines, `always_inline` library functions; `xtask/src/corpus/kiln.rs` `opt_first_reason`) | WP-015 | done |
| WP-021 | T-IR | `available_externally` + `alwaysinline` bodies: producers mark them, `hod` emits a local copy and redirects the module's calls (design 0001 point 6); test with libstdc++'s extern-template cases | WP-003, WP-004 | todo |
| WP-022 | T-H | Coverage log: per-function records (which functions hod compiled, which construct stopped it) for function-level coverage; measure the shim's per-TU `sh` exec and remove it if it shows | WP-020, WP-043 | todo |
| WP-023 | T-H | `irdiff` follow-ups, part 1: the reference compiler (plain `llc -O0` miscompiles on AArch64 with GlobalISel and with FastISel, `spec/interp.md` §10.1: the oracle becomes SelectionDAG at `-O0`, `-global-isel=0 -fast-isel=0`, with `-O2` as a second witness; a disagreement between the two is an `oracle` error, not a hod failure; the corpus is re-verified under it); directives from the corpus (WP-017): `llc-args:` so the `-debugger-tune=gdb` entries are built as their producer builds them, a per-test opt-out of `--fatal-warnings` (9 entries lost to linker warnings), a multi-module test form (119 kiln and 59 torture tests are several translation units) and its capture; `stdout` forms for output without a final newline | WP-016, WP-017, WP-024 | in-progress |
| WP-025 | T-H | `irdiff` follow-ups, part 2: emulated cross runs (x86-64 through a cross driver and `qemu-x86_64 -L <sysroot>`); the full gate also runs `--hybrid` and `--target x86_64`; seed corpus gaps (bit and floating-point intrinsics, vectors, `indirectbr`, `byval`/`sret`, `half`/`fp128`); wild once provisioned | WP-023 | todo |
| WP-024 | T-H | Jobserver client for the harness, ported from kiln (`xtask/src/jobs.rs` and its `spec/harness.md` "Jobserver"): every child process started by `xtask` (`irdiff`, `shim`, `ci` steps) and by `hod_testutil::run_with` holds a token of the pool named by `MAKEFLAGS` (`--jobserver-auth=fifo:`), with the implicit token, release on drop, kill-children-then-return-tokens on SIGINT/SIGTERM, `cargo xtask jobserver status|refill`, and the fallback to `-j` without a pool | WP-016 | done |
| WP-026 | T-IR | `hod_ir` verification catches what LLVM's verifier refuses: the cases WP-006's printer tests found (`spec/landed/ir/WP-006.md`: a use not dominated by its definition, self-use in a reachable block, `invoke`/`callbr` results used off the normal edge, `callbr` of a non-asm callee, `invoke` of an intrinsic, tail-marker and `musttail` rules, non-constant `immarg`, `"kcfi"` bundle arity, `signext` on a non-integer, `sret` position, incompatible function attributes, module-flag value shapes, linker-option arity), and debug ids checked whether or not the module has debug tables (the tests that make ids up are fixed); the printer's random test then requires `llvm-as` to accept every body `hod_ir` verifies | WP-005, WP-006 | todo |
| WP-027 | T-IR | `classify` cost: an accepted body costs about half of building it (`spec/landed/ir/WP-009.md`); profile the call path (`accepts_call`) and inlining, target a small fraction of build cost, numbers on a quiet machine | WP-009 | todo |
| WP-028 | T-IR | `spec/bc.md` + `hod_bc`, stage 1: the bitstream container, abbreviations, the module-level blocks (types, globals, constants, attributes, metadata kinds) into `hod_ir` for the bitcode LLVM 19 to 22 write, differential against `hod_ll` over `llvm-dis` of the same file. Starting `llvm-dis` costs 8 to 10 ms a file, 22 to 65% of a whole `llc -O0` run on standard-library-heavy modules (`spec/landed/cli/WP-020.md`), so this is on the speed path. Function blocks are a second row, on WP-008's body-building interface | WP-007, WP-008 | todo |

## M1: C (aarch64)

Interfaces: `spec/codegen.md`. Rows WP-035 and later are split by
instruction group and can run in parallel once WP-033 has landed.

| WP | track | scope | depends on | status |
|---|---|---|---|---|
| WP-030 | T-CG | `hod_cg` core per `spec/codegen.md` §2-§6, §8-§11: `Target` trait and `FnCx` in Rust, location policy v1, frame allocator, labels and fixups with far-branch retry, reachability, the `phi` protocol, the call sequence on top of `assign`, `Chunk`/`FuncRecord`; the test target and the policy tests of §12 | WP-004, WP-014, WP-031 | in-progress |
| WP-031 | T-H | `hod_interp`: a reference interpreter for `hod_ir` bodies (integers, pointers into a flat memory, calls between bodies and to a few host functions), and the body generator of `hod_ir`'s tests made reusable; unsupported constructs are named, not guessed | WP-003, WP-004 | done |
| WP-032 | T-CG | `hod_cg::emit_module` per `spec/codegen.md` §7.3, §7.4, §9.7: symbols up front, sections for functions and variables as `llc` chooses them, `flatten` fixups to relocations, aliases, IFUNCs, constructors, runtime-function symbols, chunk merge; variables in the `llvm.metadata` section are not emitted (`hod_ll` delivers them as ordinary variables); `sym_access` tested against `llc`'s relocations | WP-003, WP-030 | todo |
| WP-033 | T-A64 | `hod_a64` skeleton per `spec/codegen.md` §5.5: `RegInfo`, `class_of`, prologue/epilogue/CFI rows, glue, constant and symbol-address materialization, `Ret`, `Br`, `Unreachable`; a builder-made module that returns 42 links with every linker and runs | WP-030, WP-032 | todo |
| WP-034 | T-A64 | `spec/abi-a64.md` and `assign` for AArch64 (scalars, `sret`, `byval`, aggregates and `i128` as values, variadic calls and callees, `musttail`); the L5 grid generator against clang in both directions | WP-030 | done |
| WP-035 | T-A64 | integer arithmetic, shifts, division, compares, casts between integers and pointers, `select`, `freeze`; compare-and-branch fusion | WP-033 | todo |
| WP-036 | T-A64 | memory: `Load`/`Store` of every `Gpr` width incl. odd widths, stack slots, `PtrAdd` with fusion into addressing modes, dynamic `Alloca`, over-aligned slots, `stacksave`/`stackrestore` | WP-033 | todo |
| WP-037 | T-A64 | control flow: `CondBr`, `Switch`, `phi`, `IndirectBr` and block addresses | WP-033 | todo |
| WP-038 | T-A64 | calls: direct and indirect, results, `sret`/`byval`, stack arguments, `noreturn`; variadic callees and `va_start`/`va_end`/`va_copy` (the `va_arg` instruction is rejected: `llc` miscompiles it on this target, so there is no oracle) | WP-033, WP-034 | todo |
| WP-039 | T-A64 | floating point: arithmetic, compares, conversions, constants, loads and stores, FP arguments | WP-013, WP-033 | todo |
| WP-040 | T-A64 | `Mem`-class values: `i128` arithmetic, first-class aggregates (`ExtractValue`, `InsertValue`, loads, stores, `select`, `phi`), overflow intrinsics | WP-035, WP-036 | todo |
| WP-041 | T-A64 | the intrinsics C code produces: `memcpy`/`memmove`/`memset`, bit manipulation, `abs`/min/max, `trap`, `expect`-class no-ops, `frameaddress`/`returnaddress` | WP-035, WP-036 | todo |
| WP-042 | T-EH | `spec/eh.md` (CFI part) and `hod_eh`: `.eh_frame` from `FuncRecord` frame descriptions, CIE sharing; checked with `llvm-dwarfdump --eh-frame` and by unwinding through hod frames | WP-030 | todo |
| WP-043 | T-H | wire code generation into `hod::compile` (parse with `hod_ll`, `classify`, compile, emit), per-construct reasons in `spec/cli.md`, `docs/status/support.md` | WP-008, WP-009, WP-033 | todo |
| WP-050 | T-X64 | `spec/mc-x64.md` + `hod_mc_x64`: the base integer set an `-O0` code generator needs (registers of every width with REX, every ModRM/SIB/RIP-relative memory form, moves and extensions, `lea`, arithmetic and logic with register, memory and immediate operands, multiply and divide, shifts, `setcc`/`cmovcc`, branches and calls with near and short forms, stack, bit counting, `lock` forms and `cmpxchg`/`xadd`/`xchg`, `ud2`, `endbr64`, multi-byte `nop`), fixups for the ELF relocations of the small PIC code model; L1 sweep harness against `llvm-mc`. Scalar SSE floating point is the follow-up row, as WP-013 was for AArch64 | WP-001 | in-progress |

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
