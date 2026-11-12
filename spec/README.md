# Specifications

One contract per crate or cross-cutting topic. A spec is normative: the code
does what it says, and the tests named in it check that. The aim is that any
crate could be re-implemented from its spec and its tests alone.

| file | covers | status |
|---|---|---|
| `ir.md` | `hod_ir`: the IR model, builder API, printer, `classify` | design written; implementation WP-002..009 |
| `ll.md` | `hod_ll`: accepted text dialects, errors | planned (WP-007, WP-008) |
| `object.md` | `hod_obj`: object model, ELF64 writer | planned (WP-010, WP-011) |
| `mc-a64.md` | `hod_mc_a64`: instruction forms, fixups | planned (WP-012, WP-013) |
| `mc-x64.md` | `hod_mc_x64` | planned |
| `codegen.md` | `hod_cg`: walks, value locations, frame, `Target` trait, per-function output record | planned (WP-014) |
| `abi-a64.md` | how IR-level types are passed and returned on AArch64 | planned |
| `abi-x64.md` | the same for x86-64 SysV | planned |
| `eh.md` | `hod_eh`: CFI and LSDA | planned |
| `dwarf.md` | `hod_dwarf` | planned |
| `xform.md` | `hod_xform`: required lowerings | planned |
| `cli.md` | the `hod` command line, fallback, coverage log | planned (WP-015) |
| `harness.md` | `xtask`: test formats, differential runs, benchmarks | planned (WP-016..018) |

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
