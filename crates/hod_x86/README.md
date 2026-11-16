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
| `frame.rs` | WP-293 | prologue, epilogue, call instruction | I10 (FDE), I11 (`dynamic_slot`), I6 (tail exits) | `begin`, `end`, `dynamic_slot`, `emit_call` | `x86:abi:musttail` (`x86:dynamic-alloca` removed by I11b) |
| `alu.rs` | WP-293 | `Gpr` integer operations | — | — | — |
| `mem.rs` | WP-293 | `load`, `store`, `ptradd` | I11 (atomics) | `load`, `store` | `x86:atomic` (unreachable since I11b: ordered accesses reach `atomic.rs` first) |
| `bytes.rs` | WP-293 | `memcpy`, `memmove`, `memset` | — | — | — |
| `control.rs` | WP-293 | terminators | — | — | — |
| `call.rs` | WP-293 | call checks, intrinsic dispatch | I10 (`check`), I11 (varargs), I13 (inline asm) | `check`, `lower` | `x86:call`, `x86:intrinsic` (`x86:eh` removed by I10, `x86:varargs` narrowed by I11b, `x86:inline-asm` now `asm.rs`'s) |
| `abi.rs` | **I6** (WP-297) | `assign` (`spec/abi-x86.md`), `check_call`, `callee_pop`, the `st0` result glue; I6 also wrote `frame.rs`'s tail exits and callee pop | I7, I9, I11 (their types' parts) | `assign`, `odd` | `x86:abi:cc`, `x86:abi:attr` and `x86:abi:musttail` but the shapes `spec/codegen-x86.md` §7 lists |
| `int.rs` | **I7** (WP-298, WP-306) | integers of 33 to 64 bits, class `Mem`, half by half from their slots; libgcc division; their integer intrinsics (and `scmp`/`ucmp` of such operands, join C), `switch`, `ptradd` index | — | `target.rs` arms for `Mem`-class integers, `materialize_mem`; the wide arms of `call::lower`, `control::switch`, `mem::ptr_add` | `x86:i64` (arguments and results by I6); refuses `x86:i64:wide` |
| `bits.rs` | WP-309 | the integer intrinsics with a result of 1 to 32 bits (`bswap`, counts, `bitreverse`, `abs`, min/max, saturating add/sub/shift, funnel shifts) and `scmp`/`ucmp` of operands of 1 to 32 bits, in `Gpr` registers | — | the narrow arm of `call::lower` | `x86:intrinsic` for those (the rest stays) |
| `pic.rs` | **I8** (WP-299) | the access choice (`address`, `mem_inst`, `emit_call`), the GOT pointer, `R_386_GOTPC`/`GOTOFF`/`GOT32X`, PLT calls with `ebx` | I9 (constants through `mem_inst`) | — | `x86:pic` (removed) |
| `tls.rs` | **I8** (WP-299) | `%gs` TLS: all four models (general and local dynamic through `___tls_get_addr`), `llvm.thread.pointer` | — | — | `x86:tls`, `x86:tls:dynamic` (removed) |
| `x87.rs` | **I9** (WP-300) | `float`/`double`/`x86_fp80` on the x87 stack, compares, casts, libm calls, the 80-bit copies of a run, call results and returns at 80 bits, `double` folding (`spec/codegen-x86.md` §11); `FnState::x87` | I12 (SSE), any package lowering a call-like or branching instruction (`barrier`) | `target.rs` arms, `materialize_mem`, `barrier`, `call_result`/`store_call_result`, `ret_copy`/`load_ret` | `x86:float*` |
| `va.rs`, `atomic.rs`, `dynstack.rs` | **I11b** (WP-316) | variadic bodies (`va_start`, `va_copy`, `va_end`, the IR `va_arg`); ordered loads and stores, `atomicrmw`, `cmpxchg` (and `extractvalue` of its result), `fence` (8 bytes through x87 and `cmpxchg8b`); dynamic `alloca`, over-aligned slots, `stacksave`/`stackrestore` (`spec/codegen-x86.md` §12); `atomic::barrier` | — | `x87::barrier` (one arm), `frame.rs`'s `dynamic_slot` glue (now `dynstack::dynamic_slot`), `call::lower` (the `va_*` and stack arms), `begin_function` (`dynstack::check`) | `x86:atomic`, `x86:dynamic-alloca`, `x86:varargs` (narrowed to `va_start` outside a variadic function); new: `x86:varargs:va-arg-type`, `x86:atomic:{no-cx8,libatomic,float-op,op,type}`, `x86:probe-stack` |
| `asm.rs`, `hod_asm::x86` | **I13** (WP-301) | inline assembly in 32-bit mode (`spec/asm.md` §22) | — | `call::lower`'s asm arm | `x86:inline-asm` (now `x86:inline-asm:<detail>`) |
| `agg.rs` (and `bytes.rs`, `bits::small`) | **I11** (WP-315) | aggregates in frame slots (`load`, `store`, `select`, `extractvalue`, `insertvalue`, constants, results in memory), the `*.with.overflow` intrinsics at 1 to 64 bits; in `bytes.rs` lengths of 33 to 64 bits and `llc`'s libc-call rule (`calls_libc`, the x87 barrier); in `bits.rs` the small intrinsics (`spec/codegen-x86.md` §13) | — | the `agg::owns` arm and the extract/insert arms of `Target::lower`, `materialize_mem`; `call::lower`'s overflow and small arms; `control::ret`; `x87::barrier`'s memory-intrinsic and aggregate-`select` arms | `x86:aggregate` (but arguments of more than 16 leaves), `x86:i64`, `x86:intrinsic` for the overflow family and the small intrinsics |
| `eh.rs` | **I10** (WP-314) | `landingpad` (`eax`/`edx`), `resume`, `llvm.eh.typeid.for`, `extractvalue`/`insertvalue` of `{ ptr, i32 }` (its arms come before `agg`'s general ones; same layout); `invoke` is `hod_cg`'s call sequence, the tables `hod_eh::I386` (`spec/codegen-x86.md` §14) | — | `target.rs` arms, `call::lower`'s `eh.typeid.for` arm | `x86:eh` (removed; `x86:eh:<detail>` remain) |
| `ibr.rs` | **WP-324** | `indirectbr`, `blockaddress` of the function's own block in a register (`spec/codegen-x86.md` §15) | — | the `IndirectBr` arm of `Target::lower`, `common::materialize_const`'s `BlockAddr` arm | `x86:indirect-branch` |

Outside the crate, `hod_cg` refuses `x86:debug-info` (`emit/mod.rs`,
removed by I10 with the unwind tables in `emit/unwind.rs`) and
`x86:cf-protection`.
