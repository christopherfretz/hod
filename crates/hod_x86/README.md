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
| `int.rs` | **I7** (WP-298, WP-306) | integers of 33 to 64 bits, class `Mem`, half by half from their slots; libgcc division; their integer intrinsics (and `scmp`/`ucmp` of such operands, join C), `switch`, `ptradd` index | — | `target.rs` arms for `Mem`-class integers, `materialize_mem`; the wide arms of `call::lower`, `control::switch`, `mem::ptr_add` | `x86:i64` (arguments and results by I6); refuses `x86:i64:wide` |
| `bits.rs` | WP-309 | the integer intrinsics with a result of 1 to 32 bits (`bswap`, counts, `bitreverse`, `abs`, min/max, saturating add/sub/shift, funnel shifts) and `scmp`/`ucmp` of operands of 1 to 32 bits, in `Gpr` registers | — | the narrow arm of `call::lower` | `x86:intrinsic` for those (the rest stays) |
| `pic.rs` | **I8** (WP-299) | the access choice (`address`, `mem_inst`, `emit_call`), the GOT pointer, `R_386_GOTPC`/`GOTOFF`/`GOT32X`, PLT calls with `ebx` | I9 (constants through `mem_inst`) | — | `x86:pic` (removed) |
| `tls.rs` | **I8** (WP-299) | `%gs` TLS: all four models (general and local dynamic through `___tls_get_addr`), `llvm.thread.pointer` | — | — | `x86:tls`, `x86:tls:dynamic` (removed) |
| `x87.rs` | **I9** (WP-300) | `float`/`double`/`x86_fp80` on the x87 stack, compares, casts, libm calls, the 80-bit copies of a run, call results and returns at 80 bits, `double` folding (`spec/codegen-x86.md` §11); `FnState::x87` | I12 (SSE), any package lowering a call-like or branching instruction (`barrier`) | `target.rs` arms, `materialize_mem`, `barrier`, `call_result`/`store_call_result`, `ret_copy`/`load_ret` | `x86:float*` |
| `asm.rs`, `hod_asm::x86` | **I13** (WP-301) | inline assembly in 32-bit mode (`spec/asm.md` §22) | — | `call::lower`'s asm arm | `x86:inline-asm` (now `x86:inline-asm:<detail>`) |

Outside the crate, `hod_cg` refuses `x86:debug-info` (`emit/mod.rs`,
removed by I10 with the unwind tables in `emit/unwind.rs`) and
`x86:cf-protection`.
