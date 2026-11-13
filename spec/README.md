# Specifications

One contract per crate or cross-cutting topic. A spec is normative: the code
does what it says, and the tests named in it check that. The aim is that any
crate could be re-implemented from its spec and its tests alone.

| file | covers | status |
|---|---|---|
| `ir.md` | `hod_ir`: the IR model, builder API, printer, `classify` | §2..§5 implemented (WP-002, WP-003, WP-004); debug metadata, the printer and `classify` are WP-005, WP-006, WP-009 |
| `ll.md` | `hod_ll`: accepted text dialects, errors | current for the lexer, the module level (WP-007) and function bodies (WP-008) |
| `bc.md` | `hod_bc`: LLVM bitcode — the bitstream container, the module level and function bodies read as `hod_ll` reads text | current for the module level (WP-028), function bodies (WP-046) and debug information (§6, WP-055) |
| `object.md` | `hod_obj`: object model, ELF64 writer, linked sections, ELF convention helpers | current (WP-010, WP-011); compressed sections and the zlib compressor current (WP-061) |
| `mc-a64.md` | `hod_mc_a64`: register model, instruction constructors, logical and floating-point immediates, constant materialization, fixups | current: base integer set (WP-012), scalar floating point, FP/SIMD loads and stores and a SIMD subset (WP-013) |
| `mc-x64.md` | `hod_mc_x64`: register and memory-operand model, instruction constructors, PC-relative sites, fixups and the small PIC relocations | current: base integer set (WP-050); scalar SSE, bit tests, absolute fields and TLS sequences (WP-051) |
| `codegen.md` | `hod_cg`: the walk, value-location policy, frame, `Target` trait, per-function output record | design written (WP-014); core and test target landed (WP-030), `sym_access` and `emit_module` landed (WP-032, `spec/landed/codegen/`), unwind tables in `emit_module` (WP-049); targets to come |
| `abi-a64.md` | `hod_a64::abi::assign`: how IR-level signatures are passed and returned on AArch64 Linux as LLVM does it, extension duties, variadic functions, what is unsupported | current (WP-034); the code generator's duties of §8 are implemented by WP-038 |
| `abi-x64.md` | the same for x86-64 SysV | v1 (WP-118) |
| `eh.md` | `hod_eh`: CFI and LSDA | current: `.eh_frame` (WP-042) and `.gcc_except_table` (§9, WP-047), both byte-identical to `llc` 19 and 22; written by `emit_module` from `FuncRecord` (§1.1, WP-049); evidence in `spec/landed/eh/` and `spec/landed/codegen/WP-049.md` |
| `dwarf.md` | `hod_dwarf`: DWARF 5 for M3 — sections, COMDAT, DIE sources, the line table and its policy, DIEs, speed, tests, follow-on rows | design written; the line table (`.debug_line`, `.debug_line_str`) and its policy current (WP-052); the DIE writer core current (WP-053: the fixed abbreviation table, the unit, globals with base and pointer types, namespaces, the string, address and range sections); the type graph current (WP-058, §6.8); line tables and unit DIEs written by `emit_module` (§5.7, §9, WP-056); subprograms, parameters and locals, lexical blocks, labels and function-local types current (WP-057, §6.4); inlined subroutines, abstract instances and functions' statics current (WP-085, §6.9); `LineTablesOnly`, `DebugDirectivesOnly` and mixed units current (WP-061, §6.10); compressed `.debug_*` sections (§1.1, WP-061); globals of every shape, template parameters, imported entities, on-demand and out-of-line declarations current (WP-059, §6.11); whole `-g` modules through `classify` and the facade, DWARF 4 refused as `debug:dwarf-version`, the writer's refusals declined by name before code generation (WP-087, §6.12); imports in lexical blocks, `llc`'s order of variables' types, functions' thread-local statics, line-tables-only units with inlined code, the function-DIE refusals that need no code checked before code generation, three code-generation differences permitted in §6.7 (WP-094, §6.13); labels of every shape (artificial, nameless, coroutine suspend points), sub-byte alignments, the census's own artefacts normalized, DWARF 4 planned and still refused (WP-098, §6.14); split DWARF (`-gsplit-dwarf`: skeleton unit, `.dwo` sections or file) current (WP-062, §6.15); coroutine bodies (one subprogram over the body and its stubs, variables on the frame word, the stubs' rows) current (WP-108, §6.16); each instruction's code at its own location wherever it is emitted, and the permitted classes the harness recognises from the IR (WP-097, §5.7, §5.8); x86-64 (psABI register numbers, the `DW_OP_reg6` frame base, slot places, thread-local locations by `R_X86_64_DTPOFF64`, `llc`'s unit ranges over padding, three permitted location classes) current, its sessions through qemu's gdb stub (WP-136, §6.17); units without code that list entities, a `phi`'s declaration dropped, retained variables without a location, imports of every shape checked, the classes by file and reachable block and `post-indexed-load` (WP-111, §6.18); call-site entries for `DIFlagAllCallsDescribed` (`DW_AT_call_all_calls`, direct, indirect and tail calls) on both targets current (WP-113, §6.19); DWARF version 4 (`-gdwarf-4`: strp strings, inline addresses, `.debug_ranges`, the version 4 line table, by a transcoder of the version 5 unit; split DWARF 4 refused) current (WP-114, §6.20); evidence in `spec/landed/dwarf/` |
| `xform.md` | `hod_xform`: required lowerings and analyses | current: the coroutine plan (§3, WP-104, `spec/landed/xform/WP-104.md`) |
| `cli.md` | the `hod` command line, the `hod` facade's `compile`, fallback, coverage log | current (WP-015); reasons grow with code generation |
| `harness.md` | `xtask`: the kiln shim, test formats, differential runs, benchmarks | current: "ci", "shim" (WP-015), "irdiff" and "Test directives" (WP-016), "corpus" (WP-017), "bench" (WP-018), "Jobserver" (WP-024); `bisect` planned |
| `asm.md` | `hod_asm`: assembly text in IR — statements, expressions, template substitution, AArch64 operands and mnemonics, inline assembly in code, refusals | current (WP-103): inline-assembly templates of AArch64 instructions (§1..§8), the lowering in `codegen.md` §5.5.3; module-level assembly beyond symbol directives planned (§9) |
| `interp.md` | `hod_interp`: the reference interpreter (values, vectors, memory, traps, unsupported reasons, host functions, limits), the program generator, the LLVM oracle | current (WP-044; vectors WP-078, `spec/landed/interp/WP-078.md`) |

## Conventions

- A spec states behavior, not history. What each work package did is
  recorded in `landed/<spec>/WP-NNN.md`, one file per WP, so that parallel
  branches never edit the same file.
- External formats are cited by their stable names: the LLVM Language
  Reference section, the ELF gABI and processor supplement, the DWARF 5
  section number, the Itanium C++ ABI section, AAPCS64, the SysV x86-64
  psABI.
- Every "must" has a test. The test names the spec section.
- Where hod deliberately differs from LLVM's output (it does not have to be
  byte-identical, only correct), the spec says so and says why.
