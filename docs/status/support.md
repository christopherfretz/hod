# Support matrix

<!-- Generated from crates/hod_ir/src/classify/matrix.rs. Do not edit: run
     `HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc` instead. -->

What `hod_ir::classify` accepts, per construct and target (`spec/ir.md` §8).
A module or function with a construct that is not `supported` is not
compiled by hod: the driver reports the construct's name and hands the
input to the fallback when one is configured (`spec/cli.md`).

- `supported`: code generation implements it. `supported (int, ptr)` means
  for values of those type classes only (`spec/ir.md` §8.3).
- `not-implemented`: it will be; the last column names the work package,
  milestone or track (`docs/BACKLOG.md`, `docs/ROADMAP.md`). An x86_64
  `not-implemented` waits for the x86-64 target (`T-X64`) unless the row
  says otherwise.
- `rejected`: refused by design; the last column says why.

The x86_64 rows noted `AVX-512 (WP-...)` are lowered and checked against
`llc` 19 and 22 by dataflow and against `llvm-mc` by their bytes; this page
records no execution of them. They execute only on an x86-64 processor
with the features: in the hosted x86-64 lane when GitHub's runner has
them (a mixed fleet), whose `test x64` job summary states the processor,
its AVX-512 flags and how many programs executed and how many were
skipped (`docs/design/0007-avx512.md` §4.4).

## Summary

| group | constructs | aarch64 supported | not-implemented | rejected | x86_64 supported | not-implemented | rejected |
|---|---|---|---|---|---|---|---|
| `module` | 25 | 18 | 5 | 2 | 22 | 3 | 0 |
| `structor` | 2 | 2 | 0 | 0 | 2 | 0 | 0 |
| `debug` | 5 | 4 | 1 | 0 | 4 | 1 | 0 |
| `global` | 12 | 11 | 1 | 0 | 11 | 1 | 0 |
| `linkage` | 11 | 10 | 0 | 1 | 10 | 0 | 1 |
| `visibility` | 2 | 2 | 0 | 0 | 2 | 0 | 0 |
| `tls` | 4 | 4 | 0 | 0 | 4 | 0 | 0 |
| `comdat` | 5 | 2 | 0 | 3 | 2 | 0 | 3 |
| `init` | 10 | 6 | 3 | 1 | 6 | 3 | 1 |
| `function` | 6 | 6 | 0 | 0 | 6 | 0 | 0 |
| `cc` | 26 | 8 | 6 | 12 | 6 | 11 | 9 |
| `attr` | 64 | 38 | 23 | 3 | 37 | 24 | 3 |
| `type` | 15 | 12 | 0 | 3 | 13 | 0 | 2 |
| `inst` | 67 | 60 | 0 | 7 | 60 | 0 | 7 |
| `mem` | 3 | 3 | 0 | 0 | 3 | 0 | 0 |
| `atomicrmw` | 21 | 21 | 0 | 0 | 21 | 0 | 0 |
| `call` | 4 | 4 | 0 | 0 | 4 | 0 | 0 |
| `tail` | 3 | 3 | 0 | 0 | 3 | 0 | 0 |
| `bundle` | 10 | 0 | 5 | 5 | 0 | 5 | 5 |
| `landingpad` | 3 | 3 | 0 | 0 | 3 | 0 | 0 |
| `operand` | 11 | 8 | 3 | 0 | 8 | 3 | 0 |
| `const-expr` | 19 | 0 | 0 | 19 | 0 | 0 | 19 |
| `intrinsic` | 1032 | 373 | 11 | 648 | 782 | 10 | 240 |
| total | 1360 | 598 | 58 | 704 | 1009 | 61 | 290 |

## `module`

Module properties (`ModuleProps`, §2.1) and module-level tables.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `module:pic` | supported | supported | `PIC Level` is not 0 |
| `module:pie` | supported | supported | `PIE Level` is not 0 |
| `module:code-model-tiny` | not-implemented | not-implemented | unplanned |
| `module:code-model-small` | supported | supported | the default of both targets: nothing changes |
| `module:code-model-kernel` | not-implemented | not-implemented | unplanned. not built: the large model does not cover it (`spec/codegen.md` §5.6.36) |
| `module:code-model-medium` | not-implemented | not-implemented | unplanned. not built: the large model does not cover it (`spec/codegen.md` §5.6.36) |
| `module:code-model-large` | supported | supported | x86-64: every reference 64-bit, `.ltext`/`.ldata`/`.lrodata`/`.lbss`; AArch64: `movz`/`movk` addresses outside PIC, 8-byte unwind tables (WP-209) |
| `module:code-model-large-pic` | not-implemented | supported | unplanned. the large model with `PIC Level` not 0: AArch64's is refused by name (`spec/codegen.md` §5.6.36) |
| `module:large-data-threshold` | supported | supported | ignored, as `llc` ignores it (it reads the threshold from its command line only; WP-209) |
| `module:uwtable` | supported | supported | the module's default unwind-table kind is not `none` (WP-049) |
| `module:frame-pointer` | supported | supported | any `frame-pointer` value: hod always keeps a frame pointer (`spec/codegen.md` §5) |
| `module:branch-target-enforcement` | supported | supported | AArch64: the BTI bit of `.note.gnu.property`; x86-64: ignored, as `llc` (WP-185) |
| `module:guarded-control-stack` | supported | supported | AArch64: the GCS bit of `.note.gnu.property`, no code change; x86-64: ignored (WP-185) |
| `module:sign-return-address` | supported | supported | AArch64: the PAC bit of `.note.gnu.property`; x86-64: ignored, as `llc` (WP-185) |
| `module:sign-return-address-all` | supported | supported | read with `sign-return-address` by `hod_ll`'s upgrade; x86-64: ignored (WP-185) |
| `module:sign-return-address-with-bkey` | supported | supported | read with `sign-return-address` by `hod_ll`'s upgrade; x86-64: ignored (WP-185) |
| `module:branch-protection-pauth-lr` | not-implemented | supported | unplanned. AArch64: `paciasppc` and the PC-modifier forms (FEAT_PAuth_LR); x86-64: ignored (WP-185) |
| `module:cf-protection-branch` | rejected | supported | an x86 property. `endbr64` at every function entry but a `nocf_check` one, at address-taken blocks, landing pads, the `llvm.eh.sjlj.setjmp` resume point, coroutine entry stubs and after direct calls of `returns_twice` functions, then the IBT bit in `.note.gnu.property` (WP-184); `llc` 19 and 22 turn it on for any value, 0 included (WP-143), as `hod_ir` reads it (WP-144) |
| `module:cf-protection-return` | rejected | supported | an x86 property. the shadow-stack bit in `.note.gnu.property`; the code does not change (WP-143); on for any value, 0 included, as `llc` (WP-144) |
| `module:asm` | supported | supported | module-level assembly of code and data: `hod_asm` (WP-112 on AArch64, WP-204 on x86-64 with `.symver`, `spec/asm.md` §9, §14), sections of the text's own name on both (WP-255, §9.1); what it does not read is refused by an `asm:*` name |
| `module:asm-symbols` | supported | supported | module-level assembly made only of symbol directives (`spec/ir.md` §2.3) |
| `module:linker-options` | supported | supported | `!llvm.linker.options` |
| `module:dependent-libraries` | supported | supported | `!llvm.dependent-libraries` |
| `module:global-ctors` | supported | supported | an entry of `llvm.global_ctors` |
| `module:global-dtors` | supported | supported | an entry of `llvm.global_dtors` |

## `structor`

Constructor and destructor entries.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `structor:priority` | supported | supported | a constructor or destructor priority other than 65535 |
| `structor:associated` | supported | supported | a constructor or destructor with an associated symbol |

## `debug`

Debug information.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `debug:module-flags` | supported | supported | `Dwarf Version` or `Debug Info Version` is set (WP-087) |
| `debug:dwarf-version` | not-implemented | not-implemented | T-DW. a compile unit with debug information and a `Dwarf Version` other than 4 and 5: `hod_dwarf` writes DWARF 5, since WP-114 4, and since WP-150 4 for no flag (LLVM's 4) |
| `debug:subprogram` | supported | supported | a function with a body and a `DISubprogram` (WP-087) |
| `debug:slot-variables` | supported | supported | a stack slot that is a source variable (WP-087); in a coroutine body, on the frame word (WP-108, `spec/dwarf.md` §6.16) |
| `debug:locations` | supported | supported | a body whose instructions carry source locations (WP-087) |

## `global`

Symbols: what they are and what a definition carries.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `global:function-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:variable-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:function-definition` | supported | supported | a function that gets a body |
| `global:variable-definition` | supported | supported | a variable with an initializer |
| `global:alias` | supported | supported |  |
| `global:alias-offset` | supported | supported | an alias of a symbol plus a non-zero offset |
| `global:alias-absolute` | not-implemented | not-implemented | unplanned. an alias of a number |
| `global:ifunc` | supported | supported |  |
| `global:section` | supported | supported | a definition with an explicit section |
| `global:align` | supported | supported | a definition with a stated alignment |
| `global:unnamed-addr` | supported | supported | `unnamed_addr` on a definition: mergeable sections |
| `global:used` | supported | supported | a definition in `llvm.used` (`SHF_GNU_RETAIN`) |

## `linkage`

Linkages, on declarations and definitions.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `linkage:external` | supported | supported | by itself; a definition also meets its `global:` row |
| `linkage:available_externally` | supported | supported | a function's body is discarded, or copied when `alwaysinline` and called directly (design 0001 point 6, WP-021) |
| `linkage:linkonce` | supported | supported |  |
| `linkage:linkonce_odr` | supported | supported |  |
| `linkage:weak` | supported | supported |  |
| `linkage:weak_odr` | supported | supported |  |
| `linkage:appending` | rejected | rejected | only LLVM's special arrays have it, and those are not symbols (§2.2) |
| `linkage:internal` | supported | supported |  |
| `linkage:private` | supported | supported |  |
| `linkage:extern_weak` | supported | supported |  |
| `linkage:common` | supported | supported |  |

## `visibility`

Visibilities other than `default`.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `visibility:hidden` | supported | supported |  |
| `visibility:protected` | supported | supported |  |

## `tls`

Thread-local models of variables and aliases.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `tls:general-dynamic` | supported | supported | `thread_local` |
| `tls:local-dynamic` | supported | supported | `thread_local(localdynamic)` |
| `tls:initial-exec` | supported | supported | `thread_local(initialexec)` |
| `tls:local-exec` | supported | supported | `thread_local(localexec)` |

## `comdat`

COMDAT selection kinds of a definition's COMDAT.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `comdat:any` | supported | supported |  |
| `comdat:exactmatch` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |
| `comdat:largest` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |
| `comdat:nodeduplicate` | supported | supported |  |
| `comdat:samesize` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |

## `init`

What a variable's initializer contains.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `init:data` | supported | supported | any initializer: numbers, zero, `undef`, aggregates, bytes |
| `init:symbol` | supported | supported | a symbol's address, with or without an offset |
| `init:blockaddress` | supported | supported | of a block of a function with an `indirectbr` (§9.6) |
| `init:sym-diff` | supported | supported | the difference of two symbol addresses |
| `init:sym-diff-plt` | not-implemented | not-implemented | unplanned. the same through `dso_local_equivalent` (relative vtables) |
| `init:int-of-addr` | supported | supported | an address in an integer field |
| `init:block-diff` | supported | supported | a label difference `&&a - &&b` of two blocks of one function, a number (WP-095, WP-174) |
| `init:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `init:no-cfi` | not-implemented | not-implemented | unplanned |
| `init:neg-addr` | rejected | rejected | no ELF relocation subtracts a symbol's address; `llc` refuses it too. a number minus an address, `sub (i64 4, i64 ptrtoint (ptr @a to i64))`, in data or in an aggregate operand's image |

## `function`

Properties of a function that has a body.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `function:parameters` | supported | supported | a body with parameters; per class of parameter |
| `function:variadic` | supported | supported | a variadic function with a body |
| `function:may-unwind` | supported | supported | a body without `nounwind`: it needs unwind tables (WP-049) |
| `function:personality` | supported | supported | a body with a personality function |
| `function:slots` | supported | supported | a body with fixed stack objects (`Body::slots`) |
| `function:over-aligned-slot` | supported | supported | a stack object aligned above 16 bytes |

## `cc`

Calling conventions, of a body's function and of a call.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `cc:ccc` | supported | supported |  |
| `cc:fastcc` | supported | supported | assigned as `ccc` (`spec/abi-a64.md` §1.2, `spec/abi-x64.md` §1.2) |
| `cc:coldcc` | not-implemented | not-implemented | unplanned |
| `cc:ghccc` | not-implemented | not-implemented | unplanned |
| `cc:anyregcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_mostcc` | supported | supported | assigned as `ccc`; the callee also saves `x9`..`x15` (WP-095); x86-64: `rax`, `rcx`, `rdx`, `rsi`, `rdi`, `r8`..`r10` but its result's (WP-181) |
| `cc:preserve_allcc` | supported | supported | assigned as `ccc`; the callee also saves `x9`..`x15` and `q8`..`q31` whole; x86-64: `preserve_mostcc`'s and `xmm0`..`xmm15` but its result's (WP-181) |
| `cc:swiftcc` | supported | not-implemented | assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:cxx_fast_tlscc` | supported | not-implemented | assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:tailcc` | not-implemented | not-implemented | unplanned |
| `cc:cfguard_checkcc` | rejected | rejected | a Windows construct |
| `cc:swifttailcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_nonecc` | not-implemented | not-implemented | unplanned |
| `cc:x86_stdcallcc` | rejected | rejected | aarch64: an x86 convention; x86_64: a 32-bit x86 convention |
| `cc:x86_fastcallcc` | rejected | rejected | aarch64: an x86 convention; x86_64: a 32-bit x86 convention |
| `cc:x86_thiscallcc` | rejected | rejected | aarch64: an x86 convention; x86_64: a 32-bit x86 convention |
| `cc:intel_ocl_bicc` | rejected | not-implemented | aarch64: an x86 convention; x86_64: unplanned. OpenCL built-ins |
| `cc:x86_64_sysvcc` | rejected | supported | an x86 convention. assigned as `ccc`: the System V convention is x86-64 Linux's C convention (`llc` 19 and 22 emit the same code) |
| `cc:win64cc` | supported | supported | AArch64: assigned as `ccc` but a variadic call's `float`s and `double`s in general registers; the callee also keeps `x18` (`spec/abi-a64.md` §13, WP-247); x86-64: four positions in `rcx`/`xmm0`..`r9`/`xmm3`, a 32-byte shadow area; the callee also keeps `rsi`, `rdi`, `xmm6`..`xmm15` (`spec/abi-x64.md` §13) |
| `cc:x86_vectorcallcc` | rejected | rejected | aarch64: an x86 convention; x86_64: a Windows construct. `__vectorcall` |
| `cc:hhvmcc` | rejected | not-implemented | aarch64: an x86 convention; x86_64: unplanned |
| `cc:hhvm_ccc` | rejected | not-implemented | aarch64: an x86 convention; x86_64: unplanned |
| `cc:x86_intrcc` | rejected | rejected | aarch64: an x86 convention; x86_64: interrupt and exception handlers are refused by design |
| `cc:x86_regcallcc` | rejected | rejected | aarch64: an x86 convention; x86_64: Intel's `__regcall` is refused by design |
| `cc:aarch64_vector_pcs` | supported | rejected | x86_64: an AArch64 convention. assigned as `ccc`; the callee also keeps `q8`..`q23` whole (`spec/abi-a64.md` §13, WP-247) |
| `cc:aarch64_sve_vector_pcs` | rejected | rejected | aarch64: scalable vectors are refused by design; x86_64: an AArch64 convention |

## `attr`

Kept attributes (§5.7) of parameters, returns, functions and calls.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `attr:zeroext` | supported | supported |  |
| `attr:signext` | supported | supported |  |
| `attr:inreg` | not-implemented | not-implemented | unplanned |
| `attr:nest` | supported | not-implemented |  |
| `attr:swiftself` | not-implemented | not-implemented | unplanned |
| `attr:swifterror` | not-implemented | not-implemented | unplanned |
| `attr:swiftasync` | not-implemented | not-implemented | unplanned |
| `attr:sret` | supported | supported |  |
| `attr:byval` | supported | supported |  |
| `attr:byref` | supported | supported |  |
| `attr:inalloca` | rejected | rejected | a Windows construct |
| `attr:preallocated` | rejected | rejected | a Windows construct |
| `attr:elementtype` | not-implemented | not-implemented | unplanned. on an inline-assembly or intrinsic argument |
| `attr:align` | supported | supported | on a parameter or return: an assumption, no code |
| `attr:alignstack` | supported | not-implemented | on a parameter: placement of a stack block (abi-a64.md §3.3); on a function: no code, `sp` is always 16-aligned and an over-aligned slot aligns itself |
| `attr:uwtable` | supported | supported | on a function with a body (WP-049) |
| `attr:probe-stack-function` | not-implemented | not-implemented | unplanned. a `probe-stack` value other than `inline-asm`: a probe function to call |
| `attr:stack-probe-size-value` | not-implemented | not-implemented | unplanned. a `stack-probe-size` other than a multiple of 4096 below 2^24 (WP-186) |
| `attr:noreturn` | supported | supported | a promise: nothing to do in the function itself |
| `attr:naked` | rejected | rejected | a naked function is its inline assembly, with no frame (`spec/codegen.md` §1) |
| `attr:nounwind` | supported | supported | its absence is `function:may-unwind` |
| `attr:returns_twice` | supported | supported | on a body, a call, or a function called directly |
| `attr:alwaysinline` | supported | supported | an ordinary call; a body whose meaning depends on being inlined is declined (`spec/cli.md` "Always-inline", WP-021); a variable-length array's own `stacksave`/`stackrestore` pair is not (WP-237) |
| `attr:ssp` | supported | supported | a guard above the escaping slots, checked at every exit (`spec/codegen.md` §5.6.24, WP-183) |
| `attr:sspreq` | supported | supported | as `attr:ssp`, every function protected |
| `attr:sspstrong` | supported | supported | as `attr:ssp` |
| `attr:presplitcoroutine` | supported | supported | one body, the frame in the heap; what the plan refuses is refused by its `coro:*` name (WP-106); with `-g`, its variables on the frame word (WP-108); a pre-opt module with one is accepted in place of `opt` + `llc`; with this row off, it is declined by its module level (WP-109, `spec/cli.md` "Pre-opt modules") |
| `attr:sanitize_address` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_alloc_token` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_hwaddress` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_memory` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_memtag` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_numerical_stability` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_realtime` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_realtime_blocking` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_thread` | not-implemented | not-implemented | unplanned |
| `attr:sanitize_type` | not-implemented | not-implemented | unplanned |
| `attr:convergent` | supported | supported | constrains transformations only, and hod makes none (§5.7); `bundle:convergencectrl` and the `llvm.experimental.convergence.*` intrinsics stay refused |
| `attr:noredzone` | supported | supported | hod keeps no data below the stack pointer (`spec/codegen.md` §5) |
| `attr:nocf_check` | supported | supported | on x86-64 under `cf-protection-branch`: no `endbr64` at the function's entry; an indirect call with it is refused as `x64:nocf-check-indirect-call` (no `notrack` encoding); no code elsewhere, as `llc` (WP-184) |
| `attr:willreturn` | supported | supported | read only by the constant fold's `invoke` rewrite (`spec/codegen.md` §5.6.8, WP-270); no code otherwise |
| `attr:null_pointer_is_valid` | supported | supported | read only by the constant fold: an access through `null` does not end its block (`spec/codegen.md` §5.6.8, WP-270); no code otherwise |
| `attr:target-cpu` | supported | supported | the code is ARMv8.0 on AArch64 and the x86-64 base set on x86-64, which every CPU runs |
| `attr:target-features` | supported | supported | each feature judged by code generation (`spec/codegen.md` §5.5.2, §5.6.2) |
| `attr:tune-cpu` | supported | supported | tuning only: hod does not tune |
| `attr:frame-pointer` | supported | supported | any value: hod always keeps a frame pointer |
| `attr:patchable-function-entry` | not-implemented | not-implemented | unplanned |
| `attr:patchable-function-prefix` | not-implemented | not-implemented | unplanned |
| `attr:function-instrument` | not-implemented | not-implemented | unplanned |
| `attr:branch-target-enforcement` | supported | supported | AArch64: `bti c` at the entry, `bti j` where an indirect branch lands (`spec/codegen.md` §5.6.26); x86-64: ignored, as `llc` (WP-185) |
| `attr:sign-return-address` | supported | supported | AArch64: `paciasp`/`autiasp` with `DW_CFA_AARCH64_negate_ra_state`, `non-leaf` and `all` alike (every hod frame saves `x30`); x86-64: ignored (WP-185) |
| `attr:sign-return-address-key` | supported | supported | AArch64: `b_key` signs with `pacibsp`/`autibsp` under a CIE with `B`; x86-64: ignored (WP-185) |
| `attr:branch-protection-pauth-lr` | not-implemented | supported | unplanned. AArch64: FEAT_PAuth_LR; x86-64: ignored (WP-185) |
| `attr:guarded-control-stack` | supported | supported | no code change: hod returns only to the address its call left in `x30` (WP-185) |
| `attr:stack-probe-size` | supported | supported | a parameter of `attr:probe-stack`, read with it (WP-186) |
| `attr:probe-stack` | supported | supported | `inline-asm`: frames and `alloca`s probed page by page (`spec/codegen.md` §5.6.27, WP-186); other values are `attr:probe-stack-function` |
| `attr:split-stack` | not-implemented | not-implemented | unplanned |
| `attr:zero-call-used-regs` | not-implemented | not-implemented | unplanned. `skip` accepted: it zeroes nothing (WP-186) |
| `attr:stack-protector-buffer-size` | supported | supported | a parameter of `attr:ssp`, read with it |
| `attr:min-legal-vector-width` | supported | supported | read by x86 only |
| `attr:no-jump-tables` | supported | supported | hod emits no jump table (`spec/codegen.md` §6.2) |
| `attr:no-builtins` | supported | supported | hod recognises no library call |
| `attr:no-builtin` | supported | supported | every `no-builtin-<function>`; hod recognises no library call |
| `attr:disable-tail-calls` | supported | supported | either value: hod makes no tail call but a `musttail` one (`spec/codegen.md` §7.1) |

## `type`

Type classes of the values instructions produce and consume.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `type:int` | supported | supported | integers of at most 64 bits |
| `type:i128` | supported | supported | WP-040 |
| `type:int-wide` | supported | supported | integers wider than 64 bits other than `i128` (WP-040) |
| `type:ptr` | supported | supported | pointers in address space 0 |
| `type:half` | supported | supported | arithmetic in single precision |
| `type:bfloat` | supported | supported | computed in single precision, rounded as `llc` without `+bf16` (WP-095) |
| `type:float` | supported | supported |  |
| `type:double` | supported | supported |  |
| `type:fp128` | supported | supported | arithmetic, compares and conversions are calls |
| `type:x86_fp80` | rejected | supported | not an AArch64 type. in a 16-byte slot, computed on the x87 stack; calls of the C library's `l` functions (WP-131) |
| `type:vector` | supported | supported | vectors of fixed length; what is lowered of them by element and shape: `spec/codegen.md` §5.5.1 (WP-079) |
| `type:scalable-vector` | rejected | rejected | scalable vectors are refused by design |
| `type:aggregate` | supported | supported | structs and arrays as first-class values (WP-040) |
| `type:token` | supported | supported | the results and operands of the coroutine intrinsics (WP-106) |
| `type:target-ext` | rejected | rejected | no target extension type has a lowering |

## `inst`

Instructions.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `inst:add` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:sub` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:mul` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:udiv` | supported (int, i128, vector) | supported (int, i128, vector) | wider than 64 bits: up to 128 (WP-040) |
| `inst:sdiv` | supported (int, i128, vector) | supported (int, i128, vector) | wider than 64 bits: up to 128 (WP-040) |
| `inst:urem` | supported (int, i128, vector) | supported (int, i128, vector) | wider than 64 bits: up to 128 (WP-040) |
| `inst:srem` | supported (int, i128, vector) | supported (int, i128, vector) | wider than 64 bits: up to 128 (WP-040) |
| `inst:shl` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:lshr` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:ashr` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:and` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:or` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:xor` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:fadd` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:fsub` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:fmul` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:fdiv` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:frem` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) | a call of `fmod` |
| `inst:fneg` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:icmp` | supported (int, i128, int-wide, ptr, vector) | supported (int, i128, int-wide, ptr, vector) | vectors: masks of up to 64 lanes (WP-081) |
| `inst:fcmp` | supported (int, half, bfloat, float, double, fp128, vector) | supported (int, half, bfloat, float, double, fp128, x86_fp80, vector) | vectors: masks of up to 64 lanes (WP-081) |
| `inst:trunc` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:zext` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:sext` | supported (int, i128, int-wide, vector) | supported (int, i128, int-wide, vector) |  |
| `inst:fptrunc` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:fpext` | supported (half, bfloat, float, double, fp128, vector) | supported (half, bfloat, float, double, fp128, x86_fp80, vector) |  |
| `inst:fptoui` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | supported (int, i128, int-wide, half, bfloat, float, double, fp128, x86_fp80, vector) | an integer wider than 128 bits, or one wider than 64 with `bfloat`, is refused by the target (`a64:wide-fp-cast`, `x64:wide-fp-cast`) |
| `inst:fptosi` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | supported (int, i128, int-wide, half, bfloat, float, double, fp128, x86_fp80, vector) | an integer wider than 128 bits, or one wider than 64 with `bfloat`, is refused by the target (`a64:wide-fp-cast`, `x64:wide-fp-cast`) |
| `inst:uitofp` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | supported (int, i128, int-wide, half, bfloat, float, double, fp128, x86_fp80, vector) | an integer wider than 128 bits, or one wider than 64 with `bfloat`, is refused by the target (`a64:wide-fp-cast`, `x64:wide-fp-cast`) |
| `inst:sitofp` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | supported (int, i128, int-wide, half, bfloat, float, double, fp128, x86_fp80, vector) | an integer wider than 128 bits, or one wider than 64 with `bfloat`, is refused by the target (`a64:wide-fp-cast`, `x64:wide-fp-cast`) |
| `inst:ptrtoint` | supported (int, i128, int-wide, ptr, vector) | supported (int, i128, int-wide, ptr, vector) |  |
| `inst:ptrtoaddr` | supported (int, i128, int-wide, ptr, vector) | supported (int, i128, int-wide, ptr, vector) |  |
| `inst:inttoptr` | supported (int, i128, int-wide, ptr, vector) | supported (int, i128, int-wide, ptr, vector) |  |
| `inst:bitcast` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector) | between integers and floats with WP-039 |
| `inst:addrspacecast` | rejected | rejected | it has a pointer in a non-zero address space on one side |
| `inst:load` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) | a plain load; see `mem:`; FP and vector values: WP-067 |
| `inst:store` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) | a plain store; see `mem:`; FP and vector values: WP-067 |
| `inst:alloca` | supported | supported | a dynamic allocation; fixed ones are `function:slots` |
| `inst:fence` | supported | supported | WP-074; x86-64: WP-133 |
| `inst:cmpxchg` | supported (int, i128, ptr, aggregate) | supported (int, i128, ptr, aggregate) | WP-074; the `{ T, i1 }` result is an aggregate; x86-64 `i128`: `lock cmpxchg16b` with `+cx16` (WP-133), else libatomic's `__atomic_compare_exchange_16` (WP-144) |
| `inst:atomicrmw` | supported (int, i128, ptr, half, bfloat, float, double) | supported (int, i128, ptr, half, bfloat, float, double) | and the operation's `atomicrmw:` row; WP-074; x86-64 `i128`: a `cmpxchg16b` loop with `+cx16` (WP-133), else libatomic's `__atomic_*_16` (WP-144) |
| `inst:ptradd` | supported | supported | LLVM's `getelementptr` (§5.4); not a vector of pointers |
| `inst:extractvalue` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) | x86-64: floating-point members with WP-144; `x86_fp80` with WP-131 |
| `inst:insertvalue` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) | x86-64: floating-point members with WP-144; `x86_fp80` with WP-131 |
| `inst:extractelement` | supported (int, i128, ptr, half, bfloat, float, double, fp128, vector) | supported (int, i128, ptr, half, bfloat, float, double, fp128, x86_fp80, vector) | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:insertelement` | supported (int, i128, ptr, half, bfloat, float, double, fp128, vector) | supported (int, i128, ptr, half, bfloat, float, double, fp128, x86_fp80, vector) | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:shufflevector` | supported (vector) | supported (vector) | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:phi` | supported | supported | for every class whose `type:*` row is supported |
| `inst:select` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) |  |
| `inst:freeze` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, x86_fp80, vector, aggregate) |  |
| `inst:va_arg` | rejected | rejected | `llc` miscompiles it on AArch64, so there is no oracle; frontends do not emit it |
| `inst:call` | supported | supported | a call of anything but an intrinsic |
| `inst:landingpad` | supported | supported |  |
| `inst:catchpad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanuppad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:br` | supported | supported |  |
| `inst:condbr` | supported | supported | LLVM's two-way `br` |
| `inst:switch` | supported (int, i128, int-wide) | supported (int, i128, int-wide) |  |
| `inst:indirectbr` | supported | supported |  |
| `inst:ret` | supported | supported |  |
| `inst:unreachable` | supported | supported |  |
| `inst:callbr` | supported | supported | inline assembly with labels, `asm goto` (WP-103, `spec/codegen.md` §5.5.3; x86-64: an empty template, WP-181, §5.6.23, a template, WP-254, `spec/asm.md` §21) |
| `inst:invoke` | supported | supported |  |
| `inst:resume` | supported | supported |  |
| `inst:catchswitch` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:catchret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanupret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |

## `mem`

Kinds of memory access.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `mem:volatile` | supported | supported | a volatile `load`, `store`, `cmpxchg` or `atomicrmw` |
| `mem:atomic-load` | supported | supported | WP-074, WP-133; `fp128` and vectors: `a64:atomic-type`, `x64:atomic-type`; x86-64 `i128` without `+cx16`: libatomic (WP-144) |
| `mem:atomic-store` | supported | supported | WP-074, WP-133; `fp128` and vectors: `a64:atomic-type`, `x64:atomic-type`; x86-64 `i128` without `+cx16`: libatomic (WP-144) |

## `atomicrmw`

`atomicrmw` operations.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `atomicrmw:xchg` | supported | supported | WP-074, WP-133 |
| `atomicrmw:add` | supported | supported | WP-074, WP-133 |
| `atomicrmw:sub` | supported | supported | WP-074, WP-133 |
| `atomicrmw:and` | supported | supported | WP-074, WP-133 |
| `atomicrmw:nand` | supported | supported | WP-074, WP-133 |
| `atomicrmw:or` | supported | supported | WP-074, WP-133 |
| `atomicrmw:xor` | supported | supported | WP-074, WP-133 |
| `atomicrmw:max` | supported | supported | WP-074, WP-133 |
| `atomicrmw:min` | supported | supported | WP-074, WP-133 |
| `atomicrmw:umax` | supported | supported | WP-074, WP-133 |
| `atomicrmw:umin` | supported | supported | WP-074, WP-133 |
| `atomicrmw:fadd` | supported | supported | WP-074, WP-133; x86-64 `half`: WP-144 |
| `atomicrmw:fsub` | supported | supported | WP-074, WP-133; x86-64 `half`: WP-144 |
| `atomicrmw:fmax` | supported | supported | WP-074; x86-64: WP-144 |
| `atomicrmw:fmin` | supported | supported | WP-074; x86-64: WP-144 |
| `atomicrmw:uinc_wrap` | supported | supported | WP-074, WP-133 |
| `atomicrmw:udec_wrap` | supported | supported | WP-074, WP-133 |
| `atomicrmw:usub_cond` | supported | supported | WP-074, WP-133 |
| `atomicrmw:usub_sat` | supported | supported | WP-074, WP-133 |
| `atomicrmw:fmaximum` | supported | supported | WP-074; x86-64: WP-144 |
| `atomicrmw:fminimum` | supported | supported | WP-074; x86-64: WP-144 |

## `call`

Kinds of callee.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `call:indirect` | supported | supported | the callee is a computed address |
| `call:inline-asm` | supported | supported | the callee is inline assembly of instructions: `hod_asm` (WP-103, `spec/asm.md`; x86-64: WP-187) |
| `call:inline-asm-empty` | supported | supported | the callee is inline assembly whose template assembles to nothing (`spec/ir.md` §2.3) |
| `call:variadic` | supported | supported | a call through a variadic function type |

## `tail`

Tail-call markers.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `tail:tail` | supported | supported | a permission: ignored (`spec/codegen.md` §7.1) |
| `tail:notail` | supported | supported | hod makes no tail call of its own |
| `tail:musttail` | supported | supported | and the `musttail:` rules of §8.6, which need the `StackArgs` hook (`hod_cg::abi::TargetStackArgs`) |

## `bundle`

Operand bundles, by tag.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `bundle:deopt` | not-implemented | not-implemented | unplanned |
| `bundle:funclet` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `bundle:gc-transition` | rejected | rejected | garbage-collection statepoints are refused by design |
| `bundle:gc-live` | rejected | rejected | garbage-collection statepoints are refused by design |
| `bundle:cfguardtarget` | rejected | rejected | a Windows construct |
| `bundle:preallocated` | rejected | rejected | a Windows construct |
| `bundle:clang.arc.attachedcall` | not-implemented | not-implemented | unplanned |
| `bundle:ptrauth` | not-implemented | not-implemented | unplanned |
| `bundle:kcfi` | not-implemented | not-implemented | unplanned |
| `bundle:convergencectrl` | not-implemented | not-implemented | unplanned |

## `landingpad`

Landing-pad clauses.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `landingpad:cleanup` | supported | supported |  |
| `landingpad:catch` | supported | supported |  |
| `landingpad:filter` | supported | supported |  |

## `operand`

Constant addresses as instruction operands.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `operand:symbol` | supported | supported | a symbol's address as a value, with or without an offset |
| `operand:slot` | supported | supported | the address of a stack slot |
| `operand:blockaddress` | supported | supported | of a block of the same function |
| `operand:addr-of-int` | supported | supported | `inttoptr` of a constant number |
| `operand:int-of-addr` | supported | supported | `ptrtoint` of a constant address (WP-072, WP-147) |
| `operand:sym-diff` | supported | supported | a difference of two symbol addresses (WP-072) |
| `operand:block-diff` | not-implemented | not-implemented | unplanned. a label difference as an operand (WP-095) |
| `operand:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `operand:no-cfi` | not-implemented | not-implemented | unplanned |
| `operand:neg-addr` | supported | supported | a number minus an address, alone (`init:neg-addr`) |
| `operand:address-in-aggregate` | supported | supported | an aggregate or vector constant with an address (WP-072) |

## `const-expr`

Constant expressions without a normal form, in initializers and operands.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `const-expr:trunc` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:zext` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:sext` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:ptrtoint` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:ptrtoaddr` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:inttoptr` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:bitcast` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:addrspacecast` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:getelementptr` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:add` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:sub` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:mul` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:xor` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:and` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:or` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:shl` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:extractelement` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:insertelement` | rejected | rejected | a constant expression with no normal form (§4.2) |
| `const-expr:shufflevector` | rejected | rejected | a constant expression with no normal form (§4.2) |

## `intrinsic`

Intrinsics (§5.5).

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `intrinsic:llvm.memcpy` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.memcpy.inline` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.memmove` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.memset` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.memset.inline` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.va_start` | supported | supported |  |
| `intrinsic:llvm.va_end` | supported | supported |  |
| `intrinsic:llvm.va_copy` | supported | supported |  |
| `intrinsic:llvm.stacksave` | supported | supported |  |
| `intrinsic:llvm.stackrestore` | supported | supported |  |
| `intrinsic:llvm.frameaddress` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.returnaddress` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.eh.dwarf.cfa` | rejected | supported (int, ptr) | `llc` 19 and 22 for AArch64 return the CFA less 16 (the frame pointer), not the CFA their own unwind tables and GCC define |
| `intrinsic:llvm.eh.return.i64` | rejected | supported (int, ptr) | `llc` 19 and 22 for AArch64 emit nothing for it: the code falls through into what follows |
| `intrinsic:llvm.eh.unwind.init` | supported | supported |  |
| `intrinsic:llvm.sponentry` | supported (ptr) | rejected | x86_64: `llc` 19 and 22 for x86-64 abort in instruction selection; clang has no builtin for it there |
| `intrinsic:llvm.addressofreturnaddress` | supported (ptr) | supported (ptr) |  |
| `intrinsic:llvm.trap` | supported | supported |  |
| `intrinsic:llvm.debugtrap` | supported | supported |  |
| `intrinsic:llvm.ubsantrap` | supported (int) | supported (int) |  |
| `intrinsic:llvm.bswap` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.bitreverse` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.ctpop` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.ctlz` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.cttz` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.fshl` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.fshr` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.abs` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.smax` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.smin` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.umax` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.umin` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.scmp` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.ucmp` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.clmul` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sadd.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) |  |
| `intrinsic:llvm.uadd.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) |  |
| `intrinsic:llvm.ssub.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) |  |
| `intrinsic:llvm.usub.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) |  |
| `intrinsic:llvm.smul.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) | wider than 2048 bits refused (`a64:wide-mul-overflow`) |
| `intrinsic:llvm.umul.with.overflow` | supported (int, i128, int-wide, aggregate) | supported (int, i128, int-wide, aggregate) | wider than 2048 bits refused (`a64:wide-mul-overflow`) |
| `intrinsic:llvm.sadd.sat` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.uadd.sat` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.ssub.sat` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.usub.sat` | supported (int, i128, vector) | supported (int, i128, vector) |  |
| `intrinsic:llvm.fabs` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.sqrt` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.floor` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.ceil` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.trunc` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.round` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.roundeven` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.nearbyint` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.rint` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.copysign` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.fma` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.fmuladd` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.minnum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.maxnum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.minimum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.maximum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.minimumnum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.maximumnum` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.canonicalize` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.arithmetic.fence` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.pow` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.powi` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.ldexp` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.frexp` | supported (int, half, float, double, fp128, aggregate) | supported (int, half, float, double, fp128, x86_fp80, aggregate) |  |
| `intrinsic:llvm.sin` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.cos` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.tan` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.asin` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.acos` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.atan` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.atan2` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.sinh` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.cosh` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.tanh` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.exp` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.exp2` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.exp10` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.log` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.log2` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.log10` | supported (half, float, double, fp128, vector) | supported (half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.lround` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.llround` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.lrint` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.llrint` | supported (int, half, float, double, fp128, vector) | supported (int, half, float, double, fp128, x86_fp80, vector) |  |
| `intrinsic:llvm.fptosi.sat` | supported (int, half, float, double, vector) | supported (int, half, float, double, x86_fp80, vector) |  |
| `intrinsic:llvm.fptoui.sat` | supported (int, half, float, double, vector) | supported (int, half, float, double, x86_fp80, vector) |  |
| `intrinsic:llvm.is.fpclass` | supported (int, half, float, double, fp128) | supported (int, half, float, double, fp128, x86_fp80) |  |
| `intrinsic:llvm.eh.typeid.for` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.eh.sjlj.setjmp` | rejected | supported | `llc` for AArch64 compiles `setjmp` to 0 and `longjmp` to nothing; clang refuses the builtins there |
| `intrinsic:llvm.eh.sjlj.longjmp` | rejected | supported | `llc` for AArch64 compiles `setjmp` to 0 and `longjmp` to nothing; clang refuses the builtins there |
| `intrinsic:llvm.threadlocal.address` | supported (ptr) | supported (ptr) |  |
| `intrinsic:llvm.thread.pointer` | supported (ptr) | supported (ptr) |  |
| `intrinsic:llvm.prefetch` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.clear_cache` | supported (ptr) | supported (ptr) |  |
| `intrinsic:llvm.readcyclecounter` | supported (int) | supported (int) |  |
| `intrinsic:llvm.read_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.read_volatile_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.write_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.is.constant` | supported | supported |  |
| `intrinsic:llvm.objectsize` | supported (int, ptr) | supported (int, ptr) |  |
| `intrinsic:llvm.ptrmask` | supported (int, ptr, vector) | supported (int, ptr, vector) |  |
| `intrinsic:llvm.load.relative` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment.step` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.vector.reduce.add` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.mul` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.and` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.or` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.xor` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.smax` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.smin` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.umax` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.umin` | supported (int, vector) | supported (int, vector) | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmax` | supported (half, float, double, vector) | supported (half, float, double, vector) | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmin` | supported (half, float, double, vector) | supported (half, float, double, vector) | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fadd` | supported (half, float, double, vector) | supported (half, float, double, vector) | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmul` | supported (half, float, double, vector) | supported (half, float, double, vector) | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.extract` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.insert` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.coro.id` | supported (int, ptr, token) | supported (int, ptr, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.alloc` | supported (int, token) | supported (int, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.begin` | supported (ptr, token) | supported (ptr, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.end` | supported (int, ptr, token) | supported (int, ptr, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.free` | supported (ptr, token) | supported (ptr, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.size` | supported (int) | supported (int) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.align` | supported (int) | supported (int) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.save` | supported (ptr, token) | supported (ptr, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.suspend` | supported (int, token) | supported (int, token) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.resume` | supported (ptr) | supported (ptr) | in any function (WP-105) |
| `intrinsic:llvm.coro.destroy` | supported (ptr) | supported (ptr) | in any function (WP-105) |
| `intrinsic:llvm.coro.done` | supported (int, ptr) | supported (int, ptr) | in any function (WP-105) |
| `intrinsic:llvm.coro.promise` | supported (int, ptr) | supported (int, ptr) | in any function; a constant power-of-two alignment (WP-105) |
| `intrinsic:llvm.coro.frame` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.noop` | supported (ptr) | supported (ptr) | the noop frame (WP-105) |
| `intrinsic:llvm.coro.is_in_ramp` | supported (int) | supported (int) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.await.suspend.void` | supported (ptr) | supported (ptr) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.await.suspend.bool` | supported (int, ptr) | supported (int, ptr) | in a coroutine body (WP-106) |
| `intrinsic:llvm.coro.await.suspend.handle` | supported (ptr) | supported (ptr) | in a coroutine body; symmetric transfer in constant stack (WP-107) |
| `intrinsic:llvm.sshl.sat` | supported (int) | supported (int) | up to 64 bits (WP-084) |
| `intrinsic:llvm.ushl.sat` | supported (int) | supported (int) | up to 64 bits (WP-084) |
| `intrinsic:llvm.aarch64.crc32b` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32h` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32w` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32x` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cb` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32ch` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cw` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cx` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.x86.sse2.pause` | rejected | supported | an x86 property. `pause` (WP-161) |
| `intrinsic:llvm.aarch64.cls` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.cls64` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.aesd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.aese` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.aesimc` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.aesmc` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.bcaxs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.bcaxu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.eor3s` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.eor3u` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.rax1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1c` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1h` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1m` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1p` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1su0` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha1su1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha256h` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha256h2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha256su0` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha256su1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha512h` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha512h2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha512su0` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sha512su1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.xar` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.dmb` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.dsb` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.hint` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.isb` | supported (int) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.abs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.addp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.bfcvt` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfcvtn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfcvtn2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfdot` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfmlalb` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfmlalt` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.bfmmla` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-101 |
| `intrinsic:llvm.aarch64.neon.cls` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fabd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.facge` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.facgt` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.faddp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.faddv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtas` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtau` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtms` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtmu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtns` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtnu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtps` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtpu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtxn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtzs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fcvtzu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmax` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmaxnm` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmaxnmp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmaxnmv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmaxp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmaxv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmin` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fminnm` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fminnmp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fminnmv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fminp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fminv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmlal` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmlal2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmlsl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmlsl2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.fmulx` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frecpe` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frecps` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frecpx` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frint32x` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frint32z` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frint64x` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frint64z` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frsqrte` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.frsqrts` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld1x2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld1x3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld1x4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld2lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld2r` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld3lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld3r` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld4lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ld4r` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.pmul` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.pmull` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.pmull64` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.raddhn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.rshrn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.rsubhn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sabd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.saddlp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.saddlv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.saddv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.scalar.sqxtn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.scalar.sqxtun` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.scalar.uqxtn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sdot` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.shadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.shsub` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smax` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smaxp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smaxv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smin` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sminp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sminv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smmla` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.smull` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqabs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqdmulh` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqdmulh.lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqdmulh.laneq` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqdmull` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqdmulls.scalar` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqneg` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrdmlah` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrdmlsh` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrdmulh` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrdmulh.lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrdmulh.laneq` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrshrn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqrshrun` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqshlu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqshrn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqshrun` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqsub` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqxtn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sqxtun` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.srhadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.srshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.sshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st1x2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st1x3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st1x4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st2lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st3lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.st4lane` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.suqadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbl1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbl2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbl3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbl4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbx1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbx2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbx3` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.tbx4` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uabd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uaddlp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uaddlv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uaddv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.udot` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uhadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uhsub` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.umax` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.umaxp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.umaxv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.umin` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uminp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uminv` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ummla` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.umull` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqrshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqrshrn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqshrn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqsub` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.uqxtn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.urecpe` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.urhadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.urshl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ursqrte` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.usdot` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.ushl` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.usmmla` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.usqadd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcadd.rot270` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcadd.rot90` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcmla.rot0` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcmla.rot180` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcmla.rot270` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcmla.rot90` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvtfp2fxs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvtfp2fxu` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvtfp2hf` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvtfxs2fp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvtfxu2fp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vcvthf2fp` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vsli` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.neon.vsri` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.sisd.fabd` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.sisd.fcvtxn` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-083 |
| `intrinsic:llvm.aarch64.crypto.sm3partw1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3partw2` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3ss1` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3tt1a` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3tt1b` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3tt2a` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm3tt2b` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm4e` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.crypto.sm4ekey` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.frint32x` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.frint32z` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.frint64x` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.frint64z` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.fjcvtzs` | supported (int, ptr, half, float, double, vector, aggregate) | rejected | x86_64: an AArch64 property. WP-205 |
| `intrinsic:llvm.aarch64.ldxr` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.ldaxr` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.stxr` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.stlxr` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.ldxp` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.ldaxp` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.stxp` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.stlxp` | supported (int, ptr, aggregate) | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.aarch64.clrex` | supported | rejected | x86_64: an AArch64 property. WP-245 |
| `intrinsic:llvm.x86.sse.cmp.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comieq.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comige.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comigt.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comile.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comilt.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.comineq.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomieq.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomige.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomigt.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomile.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomilt.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ucomineq.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.cvtss2si` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.cvtss2si64` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.cvttss2si` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.cvttss2si64` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.max.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.max.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.min.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.min.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.movmsk.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.sfence` | rejected | supported (int, ptr, vector) | an x86 property. SSE |
| `intrinsic:llvm.x86.sse.ldmxcsr` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse.rcp.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse.rcp.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse.rsqrt.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse.rsqrt.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse.stmxcsr` | rejected | supported (int, ptr, vector) | an x86 property. SSE; WP-181 |
| `intrinsic:llvm.x86.sse2.cmp.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comieq.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comige.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comigt.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comile.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comilt.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.comineq.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomieq.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomige.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomigt.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomile.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomilt.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.ucomineq.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtpd2dq` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtpd2ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtps2dq` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtsd2si` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtsd2si64` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvtsd2ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvttpd2dq` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvttps2dq` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvttsd2si` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.cvttsd2si64` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.lfence` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.mfence` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.clflush` | rejected | supported (int, ptr, vector) | an x86 property. SSE2; WP-181 |
| `intrinsic:llvm.x86.sse2.maskmov.dqu` | rejected | supported (int, ptr, vector) | an x86 property. SSE2; WP-181 |
| `intrinsic:llvm.x86.sse2.max.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.max.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.min.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.min.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.movmsk.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.packssdw.128` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.packsswb.128` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.packuswb.128` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pavg.b` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pavg.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pmadd.wd` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pmovmskb.128` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pmulh.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pmulhu.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psad.bw` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psll.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psll.q` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psll.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pslli.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pslli.q` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.pslli.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psra.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psra.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrai.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrai.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrl.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrl.q` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrl.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrli.d` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrli.q` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse2.psrli.w` | rejected | supported (int, ptr, vector) | an x86 property. SSE2 |
| `intrinsic:llvm.x86.sse3.addsub.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.addsub.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.hadd.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.hadd.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.hsub.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.hsub.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.sse3.ldu.dq` | rejected | supported (int, ptr, vector) | an x86 property. SSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phadd.d.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phadd.sw.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phadd.w.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phsub.d.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phsub.sw.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.phsub.w.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.pmadd.ub.sw.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.pmul.hr.sw.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.pshuf.b.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.psign.b.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.psign.d.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.ssse3.psign.w.128` | rejected | supported (int, ptr, vector) | an x86 property. SSSE3 (WP-173) |
| `intrinsic:llvm.x86.flags.read.u64` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.flags.write.u64` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.fxsave` | rejected | supported (int, ptr, aggregate) | an x86 property. FXSR (WP-190) |
| `intrinsic:llvm.x86.fxsave64` | rejected | supported (int, ptr, aggregate) | an x86 property. FXSR (WP-190) |
| `intrinsic:llvm.x86.fxrstor` | rejected | supported (int, ptr, aggregate) | an x86 property. FXSR (WP-190) |
| `intrinsic:llvm.x86.fxrstor64` | rejected | supported (int, ptr, aggregate) | an x86 property. FXSR (WP-190) |
| `intrinsic:llvm.x86.rdtsc` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.rdtscp` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.rdpmc` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.addcarry.32` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.addcarry.64` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.subborrow.32` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.subborrow.64` | rejected | supported (int, ptr, aggregate) | an x86 property. baseline (WP-190) |
| `intrinsic:llvm.x86.xgetbv` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.xsetbv` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.rdrand.16` | rejected | supported (int, ptr, aggregate) | an x86 property. RDRND (WP-190) |
| `intrinsic:llvm.x86.rdrand.32` | rejected | supported (int, ptr, aggregate) | an x86 property. RDRND (WP-190) |
| `intrinsic:llvm.x86.rdrand.64` | rejected | supported (int, ptr, aggregate) | an x86 property. RDRND (WP-190) |
| `intrinsic:llvm.x86.rdseed.16` | rejected | supported (int, ptr, aggregate) | an x86 property. RDSEED (WP-190) |
| `intrinsic:llvm.x86.rdseed.32` | rejected | supported (int, ptr, aggregate) | an x86 property. RDSEED (WP-190) |
| `intrinsic:llvm.x86.rdseed.64` | rejected | supported (int, ptr, aggregate) | an x86 property. RDSEED (WP-190) |
| `intrinsic:llvm.x86.xsave` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.xsave64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.xrstor` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.xrstor64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVE (WP-190) |
| `intrinsic:llvm.x86.xsaveopt` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVEOPT (WP-190) |
| `intrinsic:llvm.x86.xsaveopt64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVEOPT (WP-190) |
| `intrinsic:llvm.x86.xsavec` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVEC (WP-190) |
| `intrinsic:llvm.x86.xsavec64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVEC (WP-190) |
| `intrinsic:llvm.x86.xsaves` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVES (WP-190) |
| `intrinsic:llvm.x86.xsaves64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVES (WP-190) |
| `intrinsic:llvm.x86.xrstors` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVES (WP-190) |
| `intrinsic:llvm.x86.xrstors64` | rejected | supported (int, ptr, aggregate) | an x86 property. XSAVES (WP-190) |
| `intrinsic:llvm.x86.clflushopt` | rejected | supported (int, ptr, aggregate) | an x86 property. CLFLUSHOPT (WP-190) |
| `intrinsic:llvm.x86.clwb` | rejected | supported (int, ptr, aggregate) | an x86 property. CLWB (WP-190) |
| `intrinsic:llvm.x86.bmi.bextr.32` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI (WP-190) |
| `intrinsic:llvm.x86.bmi.bextr.64` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI (WP-190) |
| `intrinsic:llvm.x86.bmi.bzhi.32` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.bmi.bzhi.64` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.bmi.pdep.32` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.bmi.pdep.64` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.bmi.pext.32` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.bmi.pext.64` | rejected | supported (int, ptr, aggregate) | an x86 property. BMI2 (WP-190) |
| `intrinsic:llvm.x86.sse42.crc32.32.8` | rejected | supported (int, ptr, aggregate) | an x86 property. CRC32 (WP-190) |
| `intrinsic:llvm.x86.sse42.crc32.32.16` | rejected | supported (int, ptr, aggregate) | an x86 property. CRC32 (WP-190) |
| `intrinsic:llvm.x86.sse42.crc32.32.32` | rejected | supported (int, ptr, aggregate) | an x86 property. CRC32 (WP-190) |
| `intrinsic:llvm.x86.sse42.crc32.64.64` | rejected | supported (int, ptr, aggregate) | an x86 property. CRC32 (WP-190) |
| `intrinsic:llvm.x86.rdfsbase.32` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.rdfsbase.64` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.rdgsbase.32` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.rdgsbase.64` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.wrfsbase.32` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.wrfsbase.64` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.wrgsbase.32` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.wrgsbase.64` | rejected | supported (int, ptr, aggregate) | an x86 property. FSGSBASE (WP-190) |
| `intrinsic:llvm.x86.xbegin` | rejected | supported (int, ptr, aggregate) | an x86 property. RTM (WP-190) |
| `intrinsic:llvm.x86.xend` | rejected | supported (int, ptr, aggregate) | an x86 property. RTM (WP-190) |
| `intrinsic:llvm.x86.xabort` | rejected | supported (int, ptr, aggregate) | an x86 property. RTM (WP-190) |
| `intrinsic:llvm.x86.xtest` | rejected | supported (int, ptr, aggregate) | an x86 property. RTM (WP-190) |
| `intrinsic:llvm.x86.rdpid` | rejected | supported (int, ptr, aggregate) | an x86 property. RDPID (WP-190) |
| `intrinsic:llvm.x86.rdpkru` | rejected | supported (int, ptr, aggregate) | an x86 property. PKU (WP-190) |
| `intrinsic:llvm.x86.wrpkru` | rejected | supported (int, ptr, aggregate) | an x86 property. PKU (WP-190) |
| `intrinsic:llvm.x86.serialize` | rejected | supported (int, ptr, aggregate) | an x86 property. SERIALIZE (WP-190) |
| `intrinsic:llvm.x86.incsspd` | rejected | supported (int, ptr, aggregate) | an x86 property. SHSTK (WP-190) |
| `intrinsic:llvm.x86.incsspq` | rejected | supported (int, ptr, aggregate) | an x86 property. SHSTK (WP-190) |
| `intrinsic:llvm.x86.rdsspd` | rejected | supported (int, ptr, aggregate) | an x86 property. SHSTK (WP-190) |
| `intrinsic:llvm.x86.rdsspq` | rejected | supported (int, ptr, aggregate) | an x86 property. SHSTK (WP-190) |
| `intrinsic:llvm.x86.tpause` | rejected | supported (int, ptr, aggregate) | an x86 property. WAITPKG (WP-267) |
| `intrinsic:llvm.x86.umwait` | rejected | supported (int, ptr, aggregate) | an x86 property. WAITPKG (WP-267) |
| `intrinsic:llvm.x86.umonitor` | rejected | supported (int, ptr, aggregate) | an x86 property. WAITPKG (WP-267) |
| `intrinsic:llvm.x86.sse41.round.ps` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.round.pd` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.round.ss` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.round.sd` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.blendvps` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.blendvpd` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.pblendvb` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.dpps` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.dppd` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.insertps` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.mpsadbw` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.packusdw` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.phminposuw` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.ptestz` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.ptestc` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse41.ptestnzc` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.1 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistrm128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistri128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistria128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistric128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistrio128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistris128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpistriz128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestrm128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestri128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestria128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestric128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestrio128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestris128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.sse42.pcmpestriz128` | rejected | supported (int, ptr, vector) | an x86 property. SSE4.2 (WP-202) |
| `intrinsic:llvm.x86.aesni.aesenc` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.aesni.aesenclast` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.aesni.aesdec` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.aesni.aesdeclast` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.aesni.aesimc` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.aesni.aeskeygenassist` | rejected | supported (int, ptr, vector) | an x86 property. AES (WP-202) |
| `intrinsic:llvm.x86.pclmulqdq` | rejected | supported (int, ptr, vector) | an x86 property. PCLMUL (WP-202) |
| `intrinsic:llvm.x86.sha1rnds4` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha1nexte` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha1msg1` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha1msg2` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha256msg1` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha256msg2` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.sha256rnds2` | rejected | supported (int, ptr, vector) | an x86 property. SHA (WP-202) |
| `intrinsic:llvm.x86.avx.addsub.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.addsub.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.max.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.max.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.min.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.min.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.hadd.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.hadd.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.hsub.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.hsub.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.blendv.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.blendv.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.dp.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cmp.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cmp.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cvt.pd2.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cvt.ps2dq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cvt.pd2dq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cvtt.pd2dq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.cvtt.ps2dq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.rcp.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.rsqrt.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.round.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.round.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vpermilvar.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vpermilvar.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vpermilvar.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vpermilvar.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestz.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestz.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestz.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestz.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestc.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestc.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestc.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestc.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestnzc.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestnzc.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestnzc.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vtestnzc.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.ptestz.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.ptestc.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.ptestnzc.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.movmsk.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.movmsk.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.ldu.dq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskload.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskstore.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskload.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskstore.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskload.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskstore.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskload.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.maskstore.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vzeroall` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx.vzeroupper` | rejected | supported (int, ptr, vector) | an x86 property. AVX (WP-202) |
| `intrinsic:llvm.x86.avx2.packssdw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.packsswb` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.packusdw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.packuswb` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pavg.b` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pavg.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pblendvb` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phadd.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phadd.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phadd.sw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phsub.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phsub.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.phsub.sw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmadd.ub.sw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmadd.wd` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmovmskb` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmul.hr.sw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmulh.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pmulhu.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psad.bw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pshuf.b` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psign.b` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psign.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psign.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psll.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psll.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psll.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pslli.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pslli.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.pslli.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrl.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrl.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrl.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrli.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrli.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrli.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psra.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psra.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrai.w` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrai.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psllv.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psllv.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psllv.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psllv.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrlv.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrlv.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrlv.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrlv.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrav.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.psrav.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.permd` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.permps` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.mpsadbw` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskload.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskstore.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskload.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskstore.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskload.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskstore.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskload.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.avx2.maskstore.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 (WP-202) |
| `intrinsic:llvm.x86.fma.vfmaddsub.ps` | rejected | supported (int, ptr, vector) | an x86 property. FMA (WP-202) |
| `intrinsic:llvm.x86.fma.vfmaddsub.pd` | rejected | supported (int, ptr, vector) | an x86 property. FMA (WP-202) |
| `intrinsic:llvm.x86.fma.vfmaddsub.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. FMA (WP-202) |
| `intrinsic:llvm.x86.fma.vfmaddsub.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. FMA (WP-202) |
| `intrinsic:llvm.x86.vcvtps2ph.128` | rejected | supported (int, ptr, vector) | an x86 property. F16C (WP-202) |
| `intrinsic:llvm.x86.vcvtps2ph.256` | rejected | supported (int, ptr, vector) | an x86 property. F16C (WP-202) |
| `intrinsic:llvm.x86.vgf2p8affineqb.128` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.vgf2p8affineqb.256` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.vgf2p8affineinvqb.128` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.vgf2p8affineinvqb.256` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.vgf2p8mulb.128` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.vgf2p8mulb.256` | rejected | supported (int, ptr, vector) | an x86 property. GFNI (WP-202) |
| `intrinsic:llvm.x86.avx2.gather.d.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.d.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.d` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.q` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.ps` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.pd` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.x86.avx2.gather.q.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX2 gather (WP-207) |
| `intrinsic:llvm.masked.load` | not-implemented | supported (int, ptr, vector) | unplanned. lane by lane (WP-208) |
| `intrinsic:llvm.masked.store` | not-implemented | supported (int, ptr, vector) | unplanned. lane by lane (WP-208) |
| `intrinsic:llvm.masked.expandload` | not-implemented | supported (int, ptr, vector) | unplanned. lane by lane (WP-208) |
| `intrinsic:llvm.masked.compressstore` | not-implemented | supported (int, ptr, vector) | unplanned. lane by lane (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.rndscale.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.max.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.min.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.sf.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rcp14.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rsqrt14.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vfmaddsub.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.vcvtps2ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.compress` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtps2dq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2dq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2udq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.si.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psll.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pslli.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psllv.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psra.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrai.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrl.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrli.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrlv.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.d.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qb.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qb.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.db.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.db.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.db.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qb.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qb.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rcp14.pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rcp14.sd` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rsqrt14.pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rsqrt14.sd` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psra.q.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrai.q.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.q.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.q.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.q.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.df.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rcp14.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rsqrt14.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.ps.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.ps.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2udq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2udq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2udq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.d.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.d.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.di.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psra.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrai.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.q.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.rndscale.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.max.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.min.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.df.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rcp14.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.rsqrt14.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vfmaddsub.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtpd2ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.getexp.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qw.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qw.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qw.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qw.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtpd2dq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2dq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2udq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2udq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovs.qd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.pmovus.qd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.d.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.di.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psll.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pslli.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psllv.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psra.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrai.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrl.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrli.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrlv.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pternlog.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.q.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.dbpsadbw.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.hi.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psllv.w.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.w.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrlv.w.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.hi.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.dbpsadbw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.packssdw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.packusdw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pavg.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.hi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pmul.hr.sw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pmulh.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pmulhu.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psll.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pslli.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psllv.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psra.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrai.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrl.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrli.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrlv.w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.hi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.packsswb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.packuswb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pavg.b.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.pshuf.b.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.dbpsadbw.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.hi.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psllv.w.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrav.w.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psrlv.w.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.vpermi2var.hi.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.psad.bw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.ps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtpd2qq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2qq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2uqq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2qq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2uqq.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtqq2ps.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtuqq2ps.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.ps.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtpd2qq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2qq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2uqq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2qq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2uqq.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.fpclass.ps.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvtpd2qq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2qq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttpd2uqq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2qq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.mask.cvttps2uqq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-208) |
| `intrinsic:llvm.x86.avx512.permvar.qi.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.permvar.qi.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.permvar.qi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.vpermi2var.qi.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.vpermi2var.qi.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.vpermi2var.qi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.pmultishift.qb.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.pmultishift.qb.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.pmultishift.qb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.vgf2p8affineqb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.vgf2p8mulb.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.aesni.aesenc.256` | rejected | supported (int, ptr, vector) | an x86 property. VAES (WP-220) |
| `intrinsic:llvm.x86.aesni.aesenclast.256` | rejected | supported (int, ptr, vector) | an x86 property. VAES (WP-220) |
| `intrinsic:llvm.x86.aesni.aesdec.256` | rejected | supported (int, ptr, vector) | an x86 property. VAES (WP-220) |
| `intrinsic:llvm.x86.aesni.aesdeclast.256` | rejected | supported (int, ptr, vector) | an x86 property. VAES (WP-220) |
| `intrinsic:llvm.x86.aesni.aesenc.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.aesni.aesenclast.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.aesni.aesdec.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.aesni.aesdeclast.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.pclmulqdq.256` | rejected | supported (int, ptr, vector) | an x86 property. VPCLMULQDQ (WP-220) |
| `intrinsic:llvm.x86.pclmulqdq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512bf16.cvtne2ps2bf16.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512bf16.cvtne2ps2bf16.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512bf16.cvtne2ps2bf16.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512bf16.cvtneps2bf16.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.vcvtneps2bf16128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.vcvtneps2bf16256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.fpclass.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.fpclass.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.fpclass.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.getexp.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.getexp.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.getexp.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rcp.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rcp.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rcp.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rndscale.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rndscale.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rndscale.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rsqrt.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rsqrt.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.rsqrt.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtpd2ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtpd2ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtpd2ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2pd.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2pd.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2pd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2w.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2w.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvtph2w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2uw.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2uw.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2uw.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2w.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2w.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.mask.vcvttph2w.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.max.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.max.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.max.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.min.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.min.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.min.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.vfmaddsub.ph.128` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.vfmaddsub.ph.256` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512fp16.vfmaddsub.ph.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-220) |
| `intrinsic:llvm.x86.avx512.mask.gather.dpd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.dps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.qpd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.qps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.dpq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.dpi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.qpq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather.qpi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div2.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div2.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div4.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div4.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div4.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div4.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div8.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3div8.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv2.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv2.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv4.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv4.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv4.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv4.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv8.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.gather3siv8.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.dpd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.dps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.qpd.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.qps.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.dpq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.dpi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.qpq.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatter.qpi.512` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv2.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv2.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv4.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv4.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv4.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv4.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv8.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scatterdiv8.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv2.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv2.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv4.df` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv4.di` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv4.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv4.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv8.sf` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |
| `intrinsic:llvm.x86.avx512.mask.scattersiv8.si` | rejected | supported (int, ptr, vector) | an x86 property. AVX-512 (WP-224) |

## Structural rules

Refused whatever the matrix says (`spec/ir.md` §8.6), on both targets.

| construct | when |
|---|---|
| `tls:direct-operand` | a thread-local symbol as the callee of a call |
| `tls:address-in-constant` | the address of a thread-local symbol inside a constant other than a plain address operand (`@t`, `getelementptr (@t, ...)`): an initializer, an aggregate or expression operand, a landing-pad clause |
| `blockaddress:other-function` | a `blockaddress` in a body that names a block of another function |
| `musttail:callee-pop` | a `musttail` call with a convention whose callee pops its arguments (`tailcc`, `swifttailcc`) |
| `musttail:stack-args-grow` | a `musttail` call whose stack-argument area is larger than its caller's |
| `musttail:stack-args-unknown` | a `musttail` call whose stack-argument areas are not known: no `StackArgs` was given, or it had no answer |
| `atomic:scalable-vector` | an atomic access to a scalable vector |
| `addrspace:pointer` | a value whose type is a pointer in a non-zero address space |
| `addrspace:global` | a variable, alias or IFUNC in a non-zero address space |
| `addrspace:function` | a function in a non-zero address space |
| `addrspace:alloca` | an `alloca` in a non-zero address space |
| `limit:constant-nesting` | a constant nested more than 64 aggregates deep |
| `target:support-mismatch` | the `Support` given is for another architecture than the module's |

## Malformed input

Not a missing feature: the module or body is not well formed (`spec/ir.md` §8.7).
No matrix accepts it; the producer of the input has a bug.

| construct | when |
|---|---|
| `invalid:body-symbol` | a body's symbol is not a symbol of the module |
| `invalid:body-not-function` | a body's symbol is not a function |
| `invalid:body-type` | a body's function type is not its symbol's |
| `invalid:body` | a body's tables disagree: its argument count is not its type's, or an operand names an instruction, parameter, stack slot or record the body does not have |
| `invalid:symbol` | a symbol id in an operand, a constant, a personality, a resolver or a constructor entry names no symbol; a COMDAT id names no COMDAT |
| `invalid:unresolved-symbol` | a symbol was named and never described |
| `invalid:inline-asm` | an inline-assembly callee names no entry of the module's table, or inline assembly is an operand other than a callee |
| `invalid:constant` | a constant id names no constant |
| `invalid:type` | a value's type is no type, or a type without values; a call's function type is not one |
| `invalid:blockaddress` | a `blockaddress` names a symbol that is not a function with a body, or a block its function does not have |
| `invalid:initializer` | an initializer contains the `none` token, or an alias target is not an address |
| `invalid:function-attributes` | a function's attribute set is no set of the module |

## Names built from the input

Always refused (`spec/ir.md` §8.5), except a pair, which a work package
removes by widening its row.

| form | when |
|---|---|
| `intrinsic:<name>` | a call of `Intrinsic::Other(name)`: an `llvm.*` name hod does not know, the no-code names (§5.5) included |
| `module-flag:<name>` | a module flag that is neither known nor ignorable (§2.1), or any `Require` flag |
| `cc:<keyword>` | a calling convention without a row, by LLVM's keyword (`cc:riscv_vector_cc`), or by its number if LLVM has none (`cc:42`) |
| `bundle:<tag>` | an operand bundle whose tag has no row |
| `<row>/<class>` | a supported row met with a value of a class it is not implemented for (§8.3), e.g. `inst:select/double` |
| `<producer record>` | a name a producer gave to `Module::record_unsupported` (§2.2), passed through unchanged |
