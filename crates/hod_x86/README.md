# hod_x86

32-bit x86 (i686 Linux) code generation: `hod_cg::Target` for the i386
System V psABI. Design `docs/design/0008-i386.md`; behaviour
`spec/codegen-x86.md`; convention `spec/abi-x86.md`.

## Module table

Each file has one owner. A package extends the functions named here and
removes the refusal names listed; it does not edit another package's file
except the dispatch arm in `target.rs` that sends its family to its
module (one line per arm). Shared tables (`REG_INFO`, the dispatch, the
refusal table of `spec/codegen-x86.md` §7) belong to WP-293's owner of
`target.rs`; a package adds its own row.

| file | owner | contents | extended by | functions it extends | refusals it removes |
|---|---|---|---|---|---|
| `target.rs` | WP-293 | `X86`, `REG_INFO`, `FnState`, the dispatch, the glue | every package (its arm only) | `Target::lower`, `class_of`, `materialize_mem` | — |
| `reg.rs` | WP-293 | registers, DWARF numbers | I9 (`st0`..`st7`), I12 | `dwarf` | — |
| `common.rs` | WP-293 | `put`, frame places, glue moves, symbols, branches | I8 (`sym_inst` through `pic`/`tls`) | `sym_inst` | — |
| `frame.rs` | WP-293 | prologue, epilogue, call instruction | I10 (FDE), I11 (`dynamic_slot`), I6 (tail exits) | `begin`, `end`, `dynamic_slot`, `emit_call` | `x86:dynamic-alloca`, `x86:abi:musttail` |
| `alu.rs` | WP-293 | `Gpr` integer operations | — | — | — |
| `mem.rs` | WP-293 | `load`, `store`, `ptradd` | I11 (atomics) | `load`, `store` | `x86:atomic` |
| `bytes.rs` | WP-293 | `memcpy`, `memmove`, `memset` | — | — | — |
| `control.rs` | WP-293 | terminators | — | — | — |
| `call.rs` | WP-293 | call checks, intrinsic dispatch | I10 (`check`), I11 (varargs), I13 (inline asm) | `check`, `lower` | `x86:eh`, `x86:varargs`, `x86:inline-asm` |
| `abi.rs` | **I6** (WP-297) | `assign` (`spec/abi-x86.md`), `check_call`, `callee_pop`, the `st0` result glue; I6 also wrote `frame.rs`'s tail exits and callee pop | I7, I9, I11 (their types' parts) | `assign`, `odd` | `x86:abi:cc`, `x86:abi:attr` and `x86:abi:musttail` but the shapes `spec/codegen-x86.md` §7 lists |
| `int.rs` | **I7** | wide integers (`i64` in two registers or memory) | — | new; `target.rs` arms for `Mem`-class integers | `x86:i64` |
| `pic.rs` | **I8** | GOT, `R_386_GOTPC`/`GOTOFF`/`GOT32X`, PLT calls from PIC | — | `check_static` (becomes the access choice) | `x86:pic` |
| `tls.rs` | **I8** | `%gs` TLS models | — | `address` | `x86:tls` |
| `x87.rs` | **I9** | floating point on the x87 stack | — | new; `target.rs` arms | `x86:float` |
| `hod_asm` (32-bit) | **I13** | inline assembly in 32-bit mode | — | `call::lower`'s asm arm | `x86:inline-asm` |

Outside the crate, `hod_cg` refuses `x86:debug-info` (`emit/mod.rs`,
removed by I10 with the unwind tables in `emit/unwind.rs`) and
`x86:cf-protection`.
