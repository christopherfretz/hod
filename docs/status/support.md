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

## Summary

| group | constructs | aarch64 supported | not-implemented | rejected | x86_64 supported | not-implemented | rejected |
|---|---|---|---|---|---|---|---|
| `module` | 23 | 8 | 13 | 2 | 2 | 15 | 6 |
| `structor` | 2 | 2 | 0 | 0 | 0 | 2 | 0 |
| `debug` | 4 | 0 | 4 | 0 | 0 | 4 | 0 |
| `global` | 12 | 11 | 1 | 0 | 2 | 10 | 0 |
| `linkage` | 11 | 9 | 1 | 1 | 1 | 9 | 1 |
| `visibility` | 2 | 2 | 0 | 0 | 0 | 2 | 0 |
| `tls` | 4 | 0 | 4 | 0 | 0 | 4 | 0 |
| `comdat` | 5 | 2 | 0 | 3 | 0 | 2 | 3 |
| `init` | 8 | 4 | 4 | 0 | 0 | 8 | 0 |
| `function` | 6 | 1 | 5 | 0 | 0 | 6 | 0 |
| `cc` | 15 | 1 | 12 | 2 | 0 | 12 | 3 |
| `attr` | 56 | 11 | 42 | 3 | 9 | 39 | 8 |
| `type` | 15 | 4 | 8 | 3 | 0 | 13 | 2 |
| `inst` | 67 | 39 | 20 | 8 | 0 | 59 | 8 |
| `mem` | 3 | 0 | 3 | 0 | 0 | 3 | 0 |
| `atomicrmw` | 21 | 0 | 21 | 0 | 0 | 21 | 0 |
| `call` | 3 | 0 | 3 | 0 | 0 | 3 | 0 |
| `tail` | 3 | 2 | 1 | 0 | 2 | 1 | 0 |
| `bundle` | 10 | 0 | 5 | 5 | 0 | 5 | 5 |
| `landingpad` | 3 | 0 | 3 | 0 | 0 | 3 | 0 |
| `operand` | 9 | 2 | 7 | 0 | 0 | 9 | 0 |
| `const-expr` | 19 | 0 | 0 | 19 | 0 | 0 | 19 |
| `intrinsic` | 133 | 0 | 133 | 0 | 0 | 133 | 0 |
| total | 434 | 98 | 290 | 46 | 16 | 363 | 55 |

## `module`

Module properties (`ModuleProps`, §2.1) and module-level tables.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `module:pic` | supported | not-implemented | `PIC Level` is not 0 |
| `module:pie` | supported | not-implemented | `PIE Level` is not 0 |
| `module:code-model-tiny` | not-implemented | not-implemented | unplanned |
| `module:code-model-small` | supported | supported | the default of both targets: nothing changes |
| `module:code-model-kernel` | not-implemented | not-implemented | unplanned |
| `module:code-model-medium` | not-implemented | not-implemented | unplanned |
| `module:code-model-large` | not-implemented | not-implemented | unplanned |
| `module:large-data-threshold` | not-implemented | not-implemented | unplanned |
| `module:uwtable` | not-implemented | not-implemented | WP-042. the module's default unwind-table kind is not `none` |
| `module:frame-pointer` | supported | supported | any `frame-pointer` value: hod always keeps a frame pointer (`spec/codegen.md` §5) |
| `module:branch-target-enforcement` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:guarded-control-stack` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address-all` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address-with-bkey` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:branch-protection-pauth-lr` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:cf-protection-branch` | rejected | not-implemented | an x86 property |
| `module:cf-protection-return` | rejected | not-implemented | an x86 property |
| `module:asm` | not-implemented | not-implemented | hod_asm. module-level inline assembly |
| `module:linker-options` | supported | not-implemented | `!llvm.linker.options` |
| `module:dependent-libraries` | supported | not-implemented | `!llvm.dependent-libraries` |
| `module:global-ctors` | supported | not-implemented | an entry of `llvm.global_ctors` |
| `module:global-dtors` | supported | not-implemented | an entry of `llvm.global_dtors` |

## `structor`

Constructor and destructor entries.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `structor:priority` | supported | not-implemented | a constructor or destructor priority other than 65535 |
| `structor:associated` | supported | not-implemented | a constructor or destructor with an associated symbol |

## `debug`

Debug information.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `debug:module-flags` | not-implemented | not-implemented | T-DW. `Dwarf Version` or `Debug Info Version` is set |
| `debug:subprogram` | not-implemented | not-implemented | T-DW. a function with a body and a `DISubprogram` |
| `debug:slot-variables` | not-implemented | not-implemented | T-DW. a stack slot that is a source variable |
| `debug:locations` | not-implemented | not-implemented | T-DW. a body whose instructions carry source locations |

## `global`

Symbols: what they are and what a definition carries.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `global:function-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:variable-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:function-definition` | supported | not-implemented | a function that gets a body |
| `global:variable-definition` | supported | not-implemented | a variable with an initializer |
| `global:alias` | supported | not-implemented |  |
| `global:alias-offset` | supported | not-implemented | an alias of a symbol plus a non-zero offset |
| `global:alias-absolute` | not-implemented | not-implemented | unplanned. an alias of a number |
| `global:ifunc` | supported | not-implemented |  |
| `global:section` | supported | not-implemented | a definition with an explicit section |
| `global:align` | supported | not-implemented | a definition with a stated alignment |
| `global:unnamed-addr` | supported | not-implemented | `unnamed_addr` on a definition: mergeable sections |
| `global:used` | supported | not-implemented | a definition in `llvm.used` (`SHF_GNU_RETAIN`) |

## `linkage`

Linkages, on declarations and definitions.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `linkage:external` | supported | supported | by itself; a definition also meets its `global:` row |
| `linkage:available_externally` | not-implemented | not-implemented | WP-021. design 0001, point 6 |
| `linkage:linkonce` | supported | not-implemented |  |
| `linkage:linkonce_odr` | supported | not-implemented |  |
| `linkage:weak` | supported | not-implemented |  |
| `linkage:weak_odr` | supported | not-implemented |  |
| `linkage:appending` | rejected | rejected | only LLVM's special arrays have it, and those are not symbols (§2.2) |
| `linkage:internal` | supported | not-implemented |  |
| `linkage:private` | supported | not-implemented |  |
| `linkage:extern_weak` | supported | not-implemented |  |
| `linkage:common` | supported | not-implemented |  |

## `visibility`

Visibilities other than `default`.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `visibility:hidden` | supported | not-implemented |  |
| `visibility:protected` | supported | not-implemented |  |

## `tls`

Thread-local models of variables and aliases.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `tls:general-dynamic` | not-implemented | not-implemented | M2 (TLS). `thread_local` |
| `tls:local-dynamic` | not-implemented | not-implemented | M2 (TLS). `thread_local(localdynamic)` |
| `tls:initial-exec` | not-implemented | not-implemented | M2 (TLS). `thread_local(initialexec)` |
| `tls:local-exec` | not-implemented | not-implemented | M2 (TLS). `thread_local(localexec)` |

## `comdat`

COMDAT selection kinds of a definition's COMDAT.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `comdat:any` | supported | not-implemented |  |
| `comdat:exactmatch` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |
| `comdat:largest` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |
| `comdat:nodeduplicate` | supported | not-implemented |  |
| `comdat:samesize` | rejected | rejected | ELF has `any` and `nodeduplicate` only; `llc` refuses it too |

## `init`

What a variable's initializer contains.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `init:data` | supported | not-implemented | any initializer: numbers, zero, `undef`, aggregates, bytes |
| `init:symbol` | supported | not-implemented | a symbol's address, with or without an offset |
| `init:blockaddress` | not-implemented | not-implemented | WP-037 |
| `init:sym-diff` | supported | not-implemented | the difference of two symbol addresses |
| `init:sym-diff-plt` | not-implemented | not-implemented | unplanned. the same through `dso_local_equivalent` (relative vtables) |
| `init:int-of-addr` | supported | not-implemented | an address in an integer field |
| `init:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `init:no-cfi` | not-implemented | not-implemented | unplanned |

## `function`

Properties of a function that has a body.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `function:parameters` | supported | not-implemented | a body with parameters; per class of parameter |
| `function:variadic` | not-implemented | not-implemented | WP-038. a variadic function with a body |
| `function:may-unwind` | not-implemented | not-implemented | WP-042. a body without `nounwind`: it needs unwind tables |
| `function:personality` | not-implemented | not-implemented | M2 (exceptions). a body with a personality function |
| `function:slots` | not-implemented | not-implemented | WP-036. a body with fixed stack objects (`Body::slots`) |
| `function:over-aligned-slot` | not-implemented | not-implemented | WP-036. a stack object aligned above 16 bytes |

## `cc`

Calling conventions, of a body's function and of a call.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `cc:ccc` | supported | not-implemented |  |
| `cc:fastcc` | not-implemented | not-implemented | WP-038. assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:coldcc` | not-implemented | not-implemented | unplanned |
| `cc:ghccc` | not-implemented | not-implemented | unplanned |
| `cc:anyregcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_mostcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_allcc` | not-implemented | not-implemented | unplanned |
| `cc:swiftcc` | not-implemented | not-implemented | WP-038. assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:cxx_fast_tlscc` | not-implemented | not-implemented | WP-038. assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:tailcc` | not-implemented | not-implemented | unplanned |
| `cc:cfguard_checkcc` | rejected | rejected | a Windows construct |
| `cc:swifttailcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_nonecc` | not-implemented | not-implemented | unplanned |
| `cc:aarch64_vector_pcs` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 convention |
| `cc:aarch64_sve_vector_pcs` | rejected | rejected | aarch64: scalable vectors are refused by design; x86_64: an AArch64 convention |

## `attr`

Kept attributes (§5.7) of parameters, returns, functions and calls.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `attr:zeroext` | not-implemented | not-implemented | WP-038 |
| `attr:signext` | not-implemented | not-implemented | WP-038 |
| `attr:inreg` | not-implemented | not-implemented | unplanned |
| `attr:nest` | not-implemented | not-implemented | WP-038 |
| `attr:swiftself` | not-implemented | not-implemented | unplanned |
| `attr:swifterror` | not-implemented | not-implemented | unplanned |
| `attr:swiftasync` | not-implemented | not-implemented | unplanned |
| `attr:sret` | not-implemented | not-implemented | WP-038 |
| `attr:byval` | not-implemented | not-implemented | WP-038 |
| `attr:byref` | not-implemented | not-implemented | WP-038 |
| `attr:inalloca` | rejected | rejected | a Windows construct |
| `attr:preallocated` | rejected | rejected | a Windows construct |
| `attr:elementtype` | not-implemented | not-implemented | unplanned. on an inline-assembly or intrinsic argument |
| `attr:align` | supported | supported | on a parameter or return: an assumption, no code |
| `attr:alignstack` | not-implemented | not-implemented | unplanned. on a parameter and on a function |
| `attr:uwtable` | not-implemented | not-implemented | WP-042. on a function with a body |
| `attr:noreturn` | supported | supported | a promise: nothing to do in the function itself |
| `attr:naked` | rejected | rejected | a naked function is its inline assembly, with no frame (`spec/codegen.md` §1) |
| `attr:nounwind` | supported | supported | its absence is `function:may-unwind` |
| `attr:returns_twice` | not-implemented | not-implemented | WP-038. on a body, a call, or a function called directly |
| `attr:alwaysinline` | supported | supported | by itself; see `linkage:available_externally` |
| `attr:ssp` | not-implemented | not-implemented | spec/codegen.md §13. open question: accept and ignore, or implement |
| `attr:sspreq` | not-implemented | not-implemented | spec/codegen.md §13 |
| `attr:sspstrong` | not-implemented | not-implemented | spec/codegen.md §13 |
| `attr:presplitcoroutine` | not-implemented | not-implemented | T-XF (coroutine splitting) |
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
| `attr:target-cpu` | supported | not-implemented | AArch64: the code is ARMv8.0, which every CPU runs |
| `attr:target-features` | not-implemented | not-implemented | WP-033. the target has to read it |
| `attr:tune-cpu` | supported | supported | tuning only: hod does not tune |
| `attr:frame-pointer` | supported | supported | any value: hod always keeps a frame pointer |
| `attr:patchable-function-entry` | not-implemented | not-implemented | unplanned |
| `attr:patchable-function-prefix` | not-implemented | not-implemented | unplanned |
| `attr:function-instrument` | not-implemented | not-implemented | unplanned |
| `attr:branch-target-enforcement` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `attr:sign-return-address` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `attr:sign-return-address-key` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `attr:branch-protection-pauth-lr` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `attr:guarded-control-stack` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `attr:stack-probe-size` | not-implemented | not-implemented | unplanned |
| `attr:probe-stack` | not-implemented | not-implemented | unplanned |
| `attr:split-stack` | not-implemented | not-implemented | unplanned |
| `attr:zero-call-used-regs` | not-implemented | not-implemented | unplanned |
| `attr:stack-protector-buffer-size` | supported | supported | a parameter of `attr:ssp`, read with it |
| `attr:min-legal-vector-width` | supported | not-implemented | read by x86 only |
| `attr:no-jump-tables` | not-implemented | not-implemented | WP-037 |
| `attr:no-builtins` | supported | supported | hod recognises no library call |
| `attr:no-builtin` | supported | supported | every `no-builtin-<function>`; hod recognises no library call |

## `type`

Type classes of the values instructions produce and consume.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `type:int` | supported | not-implemented | integers of at most 64 bits |
| `type:i128` | not-implemented | not-implemented | WP-040 |
| `type:int-wide` | not-implemented | not-implemented | unplanned. integers wider than 64 bits other than `i128` |
| `type:ptr` | supported | not-implemented | pointers in address space 0 |
| `type:half` | not-implemented | not-implemented | unplanned |
| `type:bfloat` | not-implemented | not-implemented | unplanned |
| `type:float` | supported | not-implemented |  |
| `type:double` | supported | not-implemented |  |
| `type:fp128` | not-implemented | not-implemented | unplanned |
| `type:x86_fp80` | rejected | not-implemented | not an AArch64 type |
| `type:vector` | not-implemented | not-implemented | M2 (vectors). vectors of fixed length |
| `type:scalable-vector` | rejected | rejected | scalable vectors are refused by design |
| `type:aggregate` | not-implemented | not-implemented | WP-040. structs and arrays as first-class values |
| `type:token` | not-implemented | not-implemented | T-XF (coroutine splitting). the results of the coroutine intrinsics |
| `type:target-ext` | rejected | rejected | no target extension type has a lowering |

## `inst`

Instructions.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `inst:add` | supported (int) | not-implemented |  |
| `inst:sub` | supported (int) | not-implemented |  |
| `inst:mul` | supported (int) | not-implemented |  |
| `inst:udiv` | supported (int) | not-implemented |  |
| `inst:sdiv` | supported (int) | not-implemented |  |
| `inst:urem` | supported (int) | not-implemented |  |
| `inst:srem` | supported (int) | not-implemented |  |
| `inst:shl` | supported (int) | not-implemented |  |
| `inst:lshr` | supported (int) | not-implemented |  |
| `inst:ashr` | supported (int) | not-implemented |  |
| `inst:and` | supported (int) | not-implemented |  |
| `inst:or` | supported (int) | not-implemented |  |
| `inst:xor` | supported (int) | not-implemented |  |
| `inst:fadd` | supported (half, float, double) | not-implemented |  |
| `inst:fsub` | supported (half, float, double) | not-implemented |  |
| `inst:fmul` | supported (half, float, double) | not-implemented |  |
| `inst:fdiv` | supported (half, float, double) | not-implemented |  |
| `inst:frem` | supported (float, double) | not-implemented | a call of `fmod` |
| `inst:fneg` | supported (half, float, double) | not-implemented |  |
| `inst:icmp` | supported (int, ptr) | not-implemented |  |
| `inst:fcmp` | supported (int, half, float, double) | not-implemented |  |
| `inst:trunc` | supported (int) | not-implemented |  |
| `inst:zext` | supported (int) | not-implemented |  |
| `inst:sext` | supported (int) | not-implemented |  |
| `inst:fptrunc` | supported (half, float, double) | not-implemented |  |
| `inst:fpext` | supported (half, float, double) | not-implemented |  |
| `inst:fptoui` | supported (int, half, float, double) | not-implemented |  |
| `inst:fptosi` | supported (int, half, float, double) | not-implemented |  |
| `inst:uitofp` | supported (int, half, float, double) | not-implemented |  |
| `inst:sitofp` | supported (int, half, float, double) | not-implemented |  |
| `inst:ptrtoint` | supported (int, ptr) | not-implemented |  |
| `inst:ptrtoaddr` | supported (int, ptr) | not-implemented |  |
| `inst:inttoptr` | supported (int, ptr) | not-implemented |  |
| `inst:bitcast` | supported (int, ptr) | not-implemented | between integers and floats with WP-039 |
| `inst:addrspacecast` | rejected | rejected | it has a pointer in a non-zero address space on one side |
| `inst:load` | not-implemented | not-implemented | WP-036. a plain load; see `mem:` |
| `inst:store` | not-implemented | not-implemented | WP-036. a plain store; see `mem:` |
| `inst:alloca` | not-implemented | not-implemented | WP-036. a dynamic allocation; fixed ones are `function:slots` |
| `inst:fence` | not-implemented | not-implemented | M2 (atomics) |
| `inst:cmpxchg` | not-implemented | not-implemented | M2 (atomics) |
| `inst:atomicrmw` | not-implemented | not-implemented | M2 (atomics). and the operation's `atomicrmw:` row |
| `inst:ptradd` | not-implemented | not-implemented | WP-036. LLVM's `getelementptr` (§5.4) |
| `inst:extractvalue` | not-implemented | not-implemented | WP-040 |
| `inst:insertvalue` | not-implemented | not-implemented | WP-040 |
| `inst:extractelement` | not-implemented | not-implemented | M2 (vectors) |
| `inst:insertelement` | not-implemented | not-implemented | M2 (vectors) |
| `inst:shufflevector` | not-implemented | not-implemented | M2 (vectors) |
| `inst:phi` | not-implemented | not-implemented | WP-037 |
| `inst:select` | supported (int, ptr) | not-implemented |  |
| `inst:freeze` | supported (int, ptr) | not-implemented |  |
| `inst:va_arg` | rejected | rejected | `llc` miscompiles it on AArch64, so there is no oracle; frontends do not emit it |
| `inst:call` | not-implemented | not-implemented | WP-038. a call of anything but an intrinsic |
| `inst:landingpad` | not-implemented | not-implemented | M2 (exceptions) |
| `inst:catchpad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanuppad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:br` | supported | not-implemented |  |
| `inst:condbr` | not-implemented | not-implemented | WP-037. LLVM's two-way `br` |
| `inst:switch` | not-implemented | not-implemented | WP-037 |
| `inst:indirectbr` | not-implemented | not-implemented | WP-037 |
| `inst:ret` | supported | not-implemented |  |
| `inst:unreachable` | supported | not-implemented |  |
| `inst:callbr` | rejected | rejected | its only use is inline assembly with labels (`spec/codegen.md` §1) |
| `inst:invoke` | not-implemented | not-implemented | M2 (exceptions) |
| `inst:resume` | not-implemented | not-implemented | M2 (exceptions) |
| `inst:catchswitch` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:catchret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanupret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |

## `mem`

Kinds of memory access.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `mem:volatile` | not-implemented | not-implemented | WP-036. a volatile `load`, `store`, `cmpxchg` or `atomicrmw` |
| `mem:atomic-load` | not-implemented | not-implemented | M2 (atomics) |
| `mem:atomic-store` | not-implemented | not-implemented | M2 (atomics) |

## `atomicrmw`

`atomicrmw` operations.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `atomicrmw:xchg` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:add` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:sub` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:and` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:nand` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:or` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:xor` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:max` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:min` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:umax` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:umin` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fadd` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fsub` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fmax` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fmin` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:uinc_wrap` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:udec_wrap` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:usub_cond` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:usub_sat` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fmaximum` | not-implemented | not-implemented | M2 (atomics) |
| `atomicrmw:fminimum` | not-implemented | not-implemented | M2 (atomics) |

## `call`

Kinds of callee.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `call:indirect` | not-implemented | not-implemented | WP-038. the callee is a computed address |
| `call:inline-asm` | not-implemented | not-implemented | hod_asm. the callee is inline assembly |
| `call:variadic` | not-implemented | not-implemented | WP-038. a call through a variadic function type |

## `tail`

Tail-call markers.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `tail:tail` | supported | supported | a permission: ignored (`spec/codegen.md` §7.1) |
| `tail:notail` | supported | supported | hod makes no tail call of its own |
| `tail:musttail` | not-implemented | not-implemented | M2 (musttail). and the `musttail:` rules of §8.6 |

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
| `landingpad:cleanup` | not-implemented | not-implemented | M2 (exceptions) |
| `landingpad:catch` | not-implemented | not-implemented | M2 (exceptions) |
| `landingpad:filter` | not-implemented | not-implemented | M2 (exceptions) |

## `operand`

Constant addresses as instruction operands.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `operand:symbol` | supported | not-implemented | a symbol's address as a value, with or without an offset |
| `operand:slot` | not-implemented | not-implemented | WP-036. the address of a stack slot |
| `operand:blockaddress` | not-implemented | not-implemented | WP-037. of a block of the same function |
| `operand:addr-of-int` | supported | not-implemented | `inttoptr` of a constant number |
| `operand:int-of-addr` | not-implemented | not-implemented | unplanned. `ptrtoint` of a constant address |
| `operand:sym-diff` | not-implemented | not-implemented | unplanned |
| `operand:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `operand:no-cfi` | not-implemented | not-implemented | unplanned |
| `operand:address-in-aggregate` | not-implemented | not-implemented | unplanned. an aggregate or vector constant with an address |

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
| `intrinsic:llvm.memcpy` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.memcpy.inline` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.memmove` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.memset` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.memset.inline` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.va_start` | not-implemented | not-implemented | WP-038 |
| `intrinsic:llvm.va_end` | not-implemented | not-implemented | WP-038 |
| `intrinsic:llvm.va_copy` | not-implemented | not-implemented | WP-038 |
| `intrinsic:llvm.stacksave` | not-implemented | not-implemented | WP-036 |
| `intrinsic:llvm.stackrestore` | not-implemented | not-implemented | WP-036 |
| `intrinsic:llvm.frameaddress` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.returnaddress` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.trap` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.debugtrap` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ubsantrap` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.bswap` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.bitreverse` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.ctpop` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.ctlz` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.cttz` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.fshl` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.fshr` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.abs` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.smax` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.smin` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.umax` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.umin` | not-implemented | not-implemented | WP-041 |
| `intrinsic:llvm.scmp` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ucmp` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.clmul` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sadd.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.uadd.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.ssub.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.usub.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.smul.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.umul.with.overflow` | not-implemented | not-implemented | WP-040 |
| `intrinsic:llvm.sadd.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.uadd.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ssub.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.usub.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.fabs` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sqrt` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.floor` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ceil` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.trunc` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.round` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.roundeven` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.nearbyint` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.rint` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.copysign` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.fma` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.fmuladd` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.minnum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.maxnum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.minimum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.maximum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.minimumnum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.maximumnum` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.canonicalize` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.arithmetic.fence` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.pow` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.powi` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ldexp` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.frexp` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sin` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.cos` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.tan` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.asin` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.acos` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.atan` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.atan2` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sinh` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.cosh` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.tanh` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.exp` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.exp2` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.exp10` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.log` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.log2` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.log10` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.lround` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.llround` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.lrint` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.llrint` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.fptosi.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.fptoui.sat` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.is.fpclass` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.eh.typeid.for` | not-implemented | not-implemented | M2 (exceptions) |
| `intrinsic:llvm.threadlocal.address` | not-implemented | not-implemented | M2 (TLS) |
| `intrinsic:llvm.thread.pointer` | not-implemented | not-implemented | M2 (TLS) |
| `intrinsic:llvm.prefetch` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.clear_cache` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.readcyclecounter` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.is.constant` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.objectsize` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.ptrmask` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.load.relative` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment.step` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.vector.reduce.add` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.mul` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.and` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.or` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.xor` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.smax` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.smin` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.umax` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.umin` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.fmax` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.fmin` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.fadd` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.reduce.fmul` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.extract` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.vector.insert` | not-implemented | not-implemented | M2 (vectors) |
| `intrinsic:llvm.coro.id` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.alloc` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.begin` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.end` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.free` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.size` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.align` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.save` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.suspend` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.resume` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.destroy` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.done` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.promise` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.frame` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.noop` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.is_in_ramp` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.void` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.bool` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.handle` | not-implemented | not-implemented | T-XF (coroutine splitting) |

## Structural rules

Refused whatever the matrix says (`spec/ir.md` §8.6), on both targets.

| construct | when |
|---|---|
| `tls:direct-operand` | a thread-local symbol as an operand of anything but the first argument of `llvm.threadlocal.address` |
| `tls:address-in-constant` | the address of a thread-local symbol inside a constant: an initializer, a constant operand, a landing-pad clause |
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
| `cc:<number>` | a calling convention LLVM has no keyword for |
| `bundle:<tag>` | an operand bundle whose tag has no row |
| `<row>/<class>` | a supported row met with a value of a class it is not implemented for (§8.3), e.g. `inst:select/double` |
| `<producer record>` | a name a producer gave to `Module::record_unsupported` (§2.2), passed through unchanged |
