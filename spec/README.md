# Specifications

One contract per crate or cross-cutting topic. A spec is normative: the code
does what it says, and the tests named in it check that. The aim is that any
crate could be re-implemented from its spec and its tests alone.

| file | covers | status |
|---|---|---|
| `ir.md` | `hod_ir`: the IR model, builder API, printer, `classify` | §2..§5 implemented (WP-002, WP-003, WP-004); debug metadata, the printer and `classify` are WP-005, WP-006, WP-009 |
| `ll.md` | `hod_ll`: accepted text dialects, errors | current for the lexer, the module level (WP-007) and function bodies (WP-008) |
| `bc.md` | `hod_bc`: LLVM bitcode — the bitstream container, the module level read as `hod_ll` reads text, function blocks handed on as spans | current for the module level (WP-028); function bodies and debug information are later stages |
| `object.md` | `hod_obj`: object model, ELF64 writer, linked sections, ELF convention helpers | current (WP-010, WP-011) |
| `mc-a64.md` | `hod_mc_a64`: register model, instruction constructors, logical and floating-point immediates, constant materialization, fixups | current: base integer set (WP-012), scalar floating point, FP/SIMD loads and stores and a SIMD subset (WP-013) |
| `mc-x64.md` | `hod_mc_x64`: register and memory-operand model, instruction constructors, PC-relative sites, fixups and the small PIC relocations | current: base integer set (WP-050); scalar SSE planned |
| `codegen.md` | `hod_cg`: the walk, value-location policy, frame, `Target` trait, per-function output record | design written (WP-014); core and test target landed (WP-030, `spec/landed/codegen/`); `emit_module` and targets to come |
| `abi-a64.md` | `hod_a64::abi::assign`: how IR-level signatures are passed and returned on AArch64 Linux as LLVM does it, extension duties, variadic functions, what is unsupported | current (WP-034); the code generator's duties of §8 are implemented by WP-038 |
| `abi-x64.md` | the same for x86-64 SysV | planned |
| `eh.md` | `hod_eh`: CFI and LSDA | current: `.eh_frame` (WP-042) and `.gcc_except_table` (§9, WP-047), both byte-identical to `llc` 19 and 22; evidence in `spec/landed/eh/` |
| `dwarf.md` | `hod_dwarf` | planned |
| `xform.md` | `hod_xform`: required lowerings | planned |
| `cli.md` | the `hod` command line, the `hod` facade's `compile`, fallback, coverage log | current (WP-015); reasons grow with code generation |
| `harness.md` | `xtask`: the kiln shim, test formats, differential runs, benchmarks | current: "ci", "shim" (WP-015), "irdiff" and "Test directives" (WP-016), "corpus" (WP-017), "bench" (WP-018), "Jobserver" (WP-024); `bisect` planned |
| `interp.md` | `hod_interp`: the reference interpreter (values, memory, traps, unsupported reasons, host functions, limits), the program generator, the LLVM oracle | current (WP-031) |

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
