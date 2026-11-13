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
| `module` | 24 | 10 | 12 | 2 | 5 | 13 | 6 |
| `structor` | 2 | 2 | 0 | 0 | 0 | 2 | 0 |
| `debug` | 5 | 4 | 1 | 0 | 0 | 5 | 0 |
| `global` | 12 | 11 | 1 | 0 | 3 | 9 | 0 |
| `linkage` | 11 | 10 | 0 | 1 | 2 | 8 | 1 |
| `visibility` | 2 | 2 | 0 | 0 | 0 | 2 | 0 |
| `tls` | 4 | 4 | 0 | 0 | 0 | 4 | 0 |
| `comdat` | 5 | 2 | 0 | 3 | 0 | 2 | 3 |
| `init` | 10 | 6 | 3 | 1 | 0 | 10 | 0 |
| `function` | 6 | 6 | 0 | 0 | 2 | 4 | 0 |
| `cc` | 15 | 6 | 7 | 2 | 1 | 11 | 3 |
| `attr` | 57 | 23 | 31 | 3 | 14 | 35 | 8 |
| `type` | 15 | 11 | 1 | 3 | 2 | 11 | 2 |
| `inst` | 67 | 60 | 0 | 7 | 4 | 56 | 7 |
| `mem` | 3 | 3 | 0 | 0 | 0 | 3 | 0 |
| `atomicrmw` | 21 | 21 | 0 | 0 | 0 | 21 | 0 |
| `call` | 4 | 4 | 0 | 0 | 0 | 4 | 0 |
| `tail` | 3 | 3 | 0 | 0 | 2 | 1 | 0 |
| `bundle` | 10 | 0 | 5 | 5 | 0 | 5 | 5 |
| `landingpad` | 3 | 3 | 0 | 0 | 0 | 3 | 0 |
| `operand` | 11 | 8 | 3 | 0 | 2 | 9 | 0 |
| `const-expr` | 19 | 0 | 0 | 19 | 0 | 0 | 19 |
| `intrinsic` | 354 | 334 | 20 | 0 | 0 | 138 | 216 |
| total | 663 | 533 | 84 | 46 | 37 | 356 | 270 |

## `module`

Module properties (`ModuleProps`, §2.1) and module-level tables.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `module:pic` | supported | supported | `PIC Level` is not 0 |
| `module:pie` | supported | supported | `PIE Level` is not 0 |
| `module:code-model-tiny` | not-implemented | not-implemented | unplanned |
| `module:code-model-small` | supported | supported | the default of both targets: nothing changes |
| `module:code-model-kernel` | not-implemented | not-implemented | unplanned |
| `module:code-model-medium` | not-implemented | not-implemented | unplanned |
| `module:code-model-large` | not-implemented | not-implemented | unplanned |
| `module:large-data-threshold` | not-implemented | not-implemented | unplanned |
| `module:uwtable` | supported | supported | the module's default unwind-table kind is not `none` (WP-049) |
| `module:frame-pointer` | supported | supported | any `frame-pointer` value: hod always keeps a frame pointer (`spec/codegen.md` §5) |
| `module:branch-target-enforcement` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:guarded-control-stack` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address-all` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:sign-return-address-with-bkey` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:branch-protection-pauth-lr` | not-implemented | rejected | aarch64: unplanned; x86_64: an AArch64 property |
| `module:cf-protection-branch` | rejected | not-implemented | an x86 property |
| `module:cf-protection-return` | rejected | not-implemented | an x86 property |
| `module:asm` | not-implemented | not-implemented | hod_asm. module-level inline assembly an assembler must read |
| `module:asm-symbols` | supported | not-implemented | module-level assembly made only of symbol directives (`spec/ir.md` §2.3) |
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
| `debug:module-flags` | supported | not-implemented | `Dwarf Version` or `Debug Info Version` is set (WP-087) |
| `debug:dwarf-version` | not-implemented | not-implemented | T-DW. a compile unit with debug information and a `Dwarf Version` other than 5 (absent is LLVM's 4): `hod_dwarf` writes DWARF 5 only |
| `debug:subprogram` | supported | not-implemented | a function with a body and a `DISubprogram` (WP-087) |
| `debug:slot-variables` | supported | not-implemented | a stack slot that is a source variable (WP-087) |
| `debug:locations` | supported | not-implemented | a body whose instructions carry source locations (WP-087) |

## `global`

Symbols: what they are and what a definition carries.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `global:function-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:variable-declaration` | supported | supported | nothing is emitted for a declaration |
| `global:function-definition` | supported | supported | a function that gets a body |
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
| `linkage:available_externally` | supported | not-implemented | a function's body is discarded, or copied when `alwaysinline` and called directly (design 0001 point 6, WP-021) |
| `linkage:linkonce` | supported | not-implemented |  |
| `linkage:linkonce_odr` | supported | not-implemented |  |
| `linkage:weak` | supported | not-implemented |  |
| `linkage:weak_odr` | supported | not-implemented |  |
| `linkage:appending` | rejected | rejected | only LLVM's special arrays have it, and those are not symbols (§2.2) |
| `linkage:internal` | supported | supported |  |
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
| `tls:general-dynamic` | supported | not-implemented | `thread_local` |
| `tls:local-dynamic` | supported | not-implemented | `thread_local(localdynamic)` |
| `tls:initial-exec` | supported | not-implemented | `thread_local(initialexec)` |
| `tls:local-exec` | supported | not-implemented | `thread_local(localexec)` |

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
| `init:blockaddress` | supported | not-implemented | of a block of a function with an `indirectbr` (§9.6) |
| `init:sym-diff` | supported | not-implemented | the difference of two symbol addresses |
| `init:sym-diff-plt` | not-implemented | not-implemented | unplanned. the same through `dso_local_equivalent` (relative vtables) |
| `init:int-of-addr` | supported | not-implemented | an address in an integer field |
| `init:block-diff` | supported | not-implemented | a label difference `&&a - &&b` of two blocks of one function, a number (WP-095) |
| `init:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `init:no-cfi` | not-implemented | not-implemented | unplanned |
| `init:neg-addr` | rejected | not-implemented | no ELF relocation subtracts a symbol's address; `llc` refuses it too. a number minus an address, `sub (i64 4, i64 ptrtoint (ptr @a to i64))`, in data or in an aggregate operand's image |

## `function`

Properties of a function that has a body.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `function:parameters` | supported | supported | a body with parameters; per class of parameter |
| `function:variadic` | supported | not-implemented | a variadic function with a body |
| `function:may-unwind` | supported | supported | a body without `nounwind`: it needs unwind tables (WP-049) |
| `function:personality` | supported | not-implemented | a body with a personality function |
| `function:slots` | supported | not-implemented | a body with fixed stack objects (`Body::slots`) |
| `function:over-aligned-slot` | supported | not-implemented | a stack object aligned above 16 bytes |

## `cc`

Calling conventions, of a body's function and of a call.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `cc:ccc` | supported | supported |  |
| `cc:fastcc` | supported | not-implemented | assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:coldcc` | not-implemented | not-implemented | unplanned |
| `cc:ghccc` | not-implemented | not-implemented | unplanned |
| `cc:anyregcc` | not-implemented | not-implemented | unplanned |
| `cc:preserve_mostcc` | supported | not-implemented | assigned as `ccc`; the callee also saves `x9`..`x15` (WP-095) |
| `cc:preserve_allcc` | supported | not-implemented | assigned as `ccc`; the callee also saves `x9`..`x15` and `q8`..`q31` whole |
| `cc:swiftcc` | supported | not-implemented | assigned as `ccc` (`spec/abi-a64.md` §1.2) |
| `cc:cxx_fast_tlscc` | supported | not-implemented | assigned as `ccc` (`spec/abi-a64.md` §1.2) |
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
| `attr:zeroext` | supported | not-implemented |  |
| `attr:signext` | supported | not-implemented |  |
| `attr:inreg` | not-implemented | not-implemented | unplanned |
| `attr:nest` | supported | not-implemented |  |
| `attr:swiftself` | not-implemented | not-implemented | unplanned |
| `attr:swifterror` | not-implemented | not-implemented | unplanned |
| `attr:swiftasync` | not-implemented | not-implemented | unplanned |
| `attr:sret` | supported | not-implemented |  |
| `attr:byval` | supported | not-implemented |  |
| `attr:byref` | supported | not-implemented |  |
| `attr:inalloca` | rejected | rejected | a Windows construct |
| `attr:preallocated` | rejected | rejected | a Windows construct |
| `attr:elementtype` | not-implemented | not-implemented | unplanned. on an inline-assembly or intrinsic argument |
| `attr:align` | supported | supported | on a parameter or return: an assumption, no code |
| `attr:alignstack` | supported | not-implemented | on a parameter: placement of a stack block (abi-a64.md §3.3); on a function: no code, `sp` is always 16-aligned and an over-aligned slot aligns itself |
| `attr:uwtable` | supported | not-implemented | on a function with a body (WP-049) |
| `attr:noreturn` | supported | supported | a promise: nothing to do in the function itself |
| `attr:naked` | rejected | rejected | a naked function is its inline assembly, with no frame (`spec/codegen.md` §1) |
| `attr:nounwind` | supported | supported | its absence is `function:may-unwind` |
| `attr:returns_twice` | supported | not-implemented | on a body, a call, or a function called directly |
| `attr:alwaysinline` | supported | supported | an ordinary call; a body whose meaning depends on being inlined is declined (`spec/cli.md` "Always-inline", WP-021) |
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
| `attr:convergent` | supported | supported | constrains transformations only, and hod makes none (§5.7); `bundle:convergencectrl` and the `llvm.experimental.convergence.*` intrinsics stay refused |
| `attr:target-cpu` | supported | supported | the code is ARMv8.0 on AArch64 and the x86-64 base set on x86-64, which every CPU runs |
| `attr:target-features` | supported | supported | each feature judged by code generation (`spec/codegen.md` §5.5.2, §5.6.2) |
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
| `attr:min-legal-vector-width` | supported | supported | read by x86 only |
| `attr:no-jump-tables` | supported | supported | hod emits no jump table (`spec/codegen.md` §6.2) |
| `attr:no-builtins` | supported | supported | hod recognises no library call |
| `attr:no-builtin` | supported | supported | every `no-builtin-<function>`; hod recognises no library call |

## `type`

Type classes of the values instructions produce and consume.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `type:int` | supported | supported | integers of at most 64 bits |
| `type:i128` | supported | not-implemented | WP-040 |
| `type:int-wide` | supported | not-implemented | integers wider than 64 bits other than `i128` (WP-040) |
| `type:ptr` | supported | supported | pointers in address space 0 |
| `type:half` | supported | not-implemented | arithmetic in single precision |
| `type:bfloat` | supported | not-implemented | computed in single precision, rounded as `llc` without `+bf16` (WP-095) |
| `type:float` | supported | not-implemented |  |
| `type:double` | supported | not-implemented |  |
| `type:fp128` | supported | not-implemented | arithmetic, compares and conversions are calls |
| `type:x86_fp80` | rejected | not-implemented | not an AArch64 type |
| `type:vector` | supported | not-implemented | vectors of fixed length; what is lowered of them by element and shape: `spec/codegen.md` §5.5.1 (WP-079) |
| `type:scalable-vector` | rejected | rejected | scalable vectors are refused by design |
| `type:aggregate` | supported | not-implemented | structs and arrays as first-class values (WP-040) |
| `type:token` | not-implemented | not-implemented | T-XF (coroutine splitting). the results of the coroutine intrinsics |
| `type:target-ext` | rejected | rejected | no target extension type has a lowering |

## `inst`

Instructions.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `inst:add` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:sub` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:mul` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:udiv` | supported (int, i128, vector) | not-implemented | wider than 64 bits: up to 128 (WP-040) |
| `inst:sdiv` | supported (int, i128, vector) | not-implemented | wider than 64 bits: up to 128 (WP-040) |
| `inst:urem` | supported (int, i128, vector) | not-implemented | wider than 64 bits: up to 128 (WP-040) |
| `inst:srem` | supported (int, i128, vector) | not-implemented | wider than 64 bits: up to 128 (WP-040) |
| `inst:shl` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:lshr` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:ashr` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:and` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:or` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:xor` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:fadd` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fsub` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fmul` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fdiv` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:frem` | supported (half, bfloat, float, double, fp128, vector) | not-implemented | a call of `fmod` |
| `inst:fneg` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:icmp` | supported (int, i128, int-wide, ptr, vector) | not-implemented | vectors: masks of up to 64 lanes (WP-081) |
| `inst:fcmp` | supported (int, half, bfloat, float, double, fp128, vector) | not-implemented | vectors: masks of up to 64 lanes (WP-081) |
| `inst:trunc` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:zext` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:sext` | supported (int, i128, int-wide, vector) | not-implemented |  |
| `inst:fptrunc` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fpext` | supported (half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fptoui` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:fptosi` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:uitofp` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:sitofp` | supported (int, i128, int-wide, half, bfloat, float, double, fp128, vector) | not-implemented |  |
| `inst:ptrtoint` | supported (int, i128, int-wide, ptr, vector) | not-implemented |  |
| `inst:ptrtoaddr` | supported (int, i128, int-wide, ptr, vector) | not-implemented |  |
| `inst:inttoptr` | supported (int, i128, int-wide, ptr, vector) | not-implemented |  |
| `inst:bitcast` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector) | not-implemented | between integers and floats with WP-039 |
| `inst:addrspacecast` | rejected | rejected | it has a pointer in a non-zero address space on one side |
| `inst:load` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented | a plain load; see `mem:`; FP and vector values: WP-067 |
| `inst:store` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented | a plain store; see `mem:`; FP and vector values: WP-067 |
| `inst:alloca` | supported | not-implemented | a dynamic allocation; fixed ones are `function:slots` |
| `inst:fence` | supported | not-implemented | WP-074 |
| `inst:cmpxchg` | supported (int, i128, ptr, aggregate) | not-implemented | WP-074; the `{ T, i1 }` result is an aggregate |
| `inst:atomicrmw` | supported (int, i128, ptr, half, bfloat, float, double) | not-implemented | and the operation's `atomicrmw:` row; WP-074 |
| `inst:ptradd` | supported | not-implemented | LLVM's `getelementptr` (§5.4); not a vector of pointers |
| `inst:extractvalue` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented |  |
| `inst:insertvalue` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented |  |
| `inst:extractelement` | supported (int, i128, ptr, half, bfloat, float, double, fp128, vector) | not-implemented | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:insertelement` | supported (int, i128, ptr, half, bfloat, float, double, fp128, vector) | not-implemented | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:shufflevector` | supported (vector) | not-implemented | byte-sized elements, up to 256 lanes (WP-079) |
| `inst:phi` | supported | supported | for every class whose `type:*` row is supported |
| `inst:select` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented |  |
| `inst:freeze` | supported (int, i128, int-wide, ptr, half, bfloat, float, double, fp128, vector, aggregate) | not-implemented |  |
| `inst:va_arg` | rejected | rejected | `llc` miscompiles it on AArch64, so there is no oracle; frontends do not emit it |
| `inst:call` | supported | not-implemented | a call of anything but an intrinsic |
| `inst:landingpad` | supported | not-implemented |  |
| `inst:catchpad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanuppad` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:br` | supported | supported |  |
| `inst:condbr` | supported | not-implemented | LLVM's two-way `br` |
| `inst:switch` | supported (int, i128, int-wide) | not-implemented |  |
| `inst:indirectbr` | supported | not-implemented |  |
| `inst:ret` | supported | supported |  |
| `inst:unreachable` | supported | supported |  |
| `inst:callbr` | supported | not-implemented | inline assembly with labels, `asm goto` (WP-103, `spec/codegen.md` §5.5.3) |
| `inst:invoke` | supported | not-implemented |  |
| `inst:resume` | supported | not-implemented |  |
| `inst:catchswitch` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:catchret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |
| `inst:cleanupret` | rejected | rejected | Windows funclet exception handling; not an ELF construct |

## `mem`

Kinds of memory access.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `mem:volatile` | supported | not-implemented | a volatile `load`, `store`, `cmpxchg` or `atomicrmw` |
| `mem:atomic-load` | supported | not-implemented | WP-074; `fp128` and vectors: `a64:atomic-type` |
| `mem:atomic-store` | supported | not-implemented | WP-074; `fp128` and vectors: `a64:atomic-type` |

## `atomicrmw`

`atomicrmw` operations.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `atomicrmw:xchg` | supported | not-implemented | WP-074 |
| `atomicrmw:add` | supported | not-implemented | WP-074 |
| `atomicrmw:sub` | supported | not-implemented | WP-074 |
| `atomicrmw:and` | supported | not-implemented | WP-074 |
| `atomicrmw:nand` | supported | not-implemented | WP-074 |
| `atomicrmw:or` | supported | not-implemented | WP-074 |
| `atomicrmw:xor` | supported | not-implemented | WP-074 |
| `atomicrmw:max` | supported | not-implemented | WP-074 |
| `atomicrmw:min` | supported | not-implemented | WP-074 |
| `atomicrmw:umax` | supported | not-implemented | WP-074 |
| `atomicrmw:umin` | supported | not-implemented | WP-074 |
| `atomicrmw:fadd` | supported | not-implemented | WP-074 |
| `atomicrmw:fsub` | supported | not-implemented | WP-074 |
| `atomicrmw:fmax` | supported | not-implemented | WP-074 |
| `atomicrmw:fmin` | supported | not-implemented | WP-074 |
| `atomicrmw:uinc_wrap` | supported | not-implemented | WP-074 |
| `atomicrmw:udec_wrap` | supported | not-implemented | WP-074 |
| `atomicrmw:usub_cond` | supported | not-implemented | WP-074 |
| `atomicrmw:usub_sat` | supported | not-implemented | WP-074 |
| `atomicrmw:fmaximum` | supported | not-implemented | WP-074 |
| `atomicrmw:fminimum` | supported | not-implemented | WP-074 |

## `call`

Kinds of callee.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `call:indirect` | supported | not-implemented | the callee is a computed address |
| `call:inline-asm` | supported | not-implemented | the callee is inline assembly of instructions: `hod_asm` (WP-103, `spec/asm.md`) |
| `call:inline-asm-empty` | supported | not-implemented | the callee is inline assembly whose template assembles to nothing (`spec/ir.md` §2.3) |
| `call:variadic` | supported | not-implemented | a call through a variadic function type |

## `tail`

Tail-call markers.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `tail:tail` | supported | supported | a permission: ignored (`spec/codegen.md` §7.1) |
| `tail:notail` | supported | supported | hod makes no tail call of its own |
| `tail:musttail` | supported | not-implemented | and the `musttail:` rules of §8.6, which need the `StackArgs` hook (`hod_cg::abi::TargetStackArgs`) |

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
| `landingpad:cleanup` | supported | not-implemented |  |
| `landingpad:catch` | supported | not-implemented |  |
| `landingpad:filter` | supported | not-implemented |  |

## `operand`

Constant addresses as instruction operands.

| construct | aarch64 | x86_64 | work package / why |
|---|---|---|---|
| `operand:symbol` | supported | supported | a symbol's address as a value, with or without an offset |
| `operand:slot` | supported | not-implemented | the address of a stack slot |
| `operand:blockaddress` | supported | not-implemented | of a block of the same function |
| `operand:addr-of-int` | supported | supported | `inttoptr` of a constant number |
| `operand:int-of-addr` | supported | not-implemented | `ptrtoint` of a constant address (WP-072) |
| `operand:sym-diff` | supported | not-implemented | a difference of two symbol addresses (WP-072) |
| `operand:block-diff` | not-implemented | not-implemented | unplanned. a label difference as an operand (WP-095) |
| `operand:dso-local-equivalent` | not-implemented | not-implemented | unplanned |
| `operand:no-cfi` | not-implemented | not-implemented | unplanned |
| `operand:neg-addr` | supported | not-implemented | a number minus an address, alone (`init:neg-addr`) |
| `operand:address-in-aggregate` | supported | not-implemented | an aggregate or vector constant with an address (WP-072) |

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
| `intrinsic:llvm.memcpy` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.memcpy.inline` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.memmove` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.memset` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.memset.inline` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.va_start` | supported | not-implemented |  |
| `intrinsic:llvm.va_end` | supported | not-implemented |  |
| `intrinsic:llvm.va_copy` | supported | not-implemented |  |
| `intrinsic:llvm.stacksave` | supported | not-implemented |  |
| `intrinsic:llvm.stackrestore` | supported | not-implemented |  |
| `intrinsic:llvm.frameaddress` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.returnaddress` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.trap` | supported | not-implemented |  |
| `intrinsic:llvm.debugtrap` | supported | not-implemented |  |
| `intrinsic:llvm.ubsantrap` | supported (int) | not-implemented |  |
| `intrinsic:llvm.bswap` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.bitreverse` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.ctpop` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.ctlz` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.cttz` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.fshl` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.fshr` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.abs` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.smax` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.smin` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.umax` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.umin` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.scmp` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.ucmp` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.clmul` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.sadd.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented |  |
| `intrinsic:llvm.uadd.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented |  |
| `intrinsic:llvm.ssub.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented |  |
| `intrinsic:llvm.usub.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented |  |
| `intrinsic:llvm.smul.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented | wider than 2048 bits refused (`a64:wide-mul-overflow`) |
| `intrinsic:llvm.umul.with.overflow` | supported (int, i128, int-wide, aggregate) | not-implemented | wider than 2048 bits refused (`a64:wide-mul-overflow`) |
| `intrinsic:llvm.sadd.sat` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.uadd.sat` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.ssub.sat` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.usub.sat` | supported (int, i128, vector) | not-implemented |  |
| `intrinsic:llvm.fabs` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.sqrt` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.floor` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.ceil` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.trunc` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.round` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.roundeven` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.nearbyint` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.rint` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.copysign` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.fma` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.fmuladd` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.minnum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.maxnum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.minimum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.maximum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.minimumnum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.maximumnum` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.canonicalize` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.arithmetic.fence` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.pow` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.powi` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.ldexp` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.frexp` | supported (int, half, float, double, fp128, aggregate) | not-implemented |  |
| `intrinsic:llvm.sin` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.cos` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.tan` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.asin` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.acos` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.atan` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.atan2` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.sinh` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.cosh` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.tanh` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.exp` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.exp2` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.exp10` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.log` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.log2` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.log10` | supported (half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.lround` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.llround` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.lrint` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.llrint` | supported (int, half, float, double, fp128, vector) | not-implemented |  |
| `intrinsic:llvm.fptosi.sat` | supported (int, half, float, double, vector) | not-implemented |  |
| `intrinsic:llvm.fptoui.sat` | supported (int, half, float, double, vector) | not-implemented |  |
| `intrinsic:llvm.is.fpclass` | supported (int, half, float, double, fp128) | not-implemented |  |
| `intrinsic:llvm.eh.typeid.for` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.threadlocal.address` | supported (ptr) | not-implemented |  |
| `intrinsic:llvm.thread.pointer` | supported (ptr) | not-implemented |  |
| `intrinsic:llvm.prefetch` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.clear_cache` | supported (ptr) | not-implemented |  |
| `intrinsic:llvm.readcyclecounter` | supported (int) | not-implemented |  |
| `intrinsic:llvm.read_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.read_volatile_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.write_register` | supported (int) | not-implemented |  |
| `intrinsic:llvm.is.constant` | supported | not-implemented |  |
| `intrinsic:llvm.objectsize` | supported (int, ptr) | not-implemented |  |
| `intrinsic:llvm.ptrmask` | supported (int, ptr, vector) | not-implemented |  |
| `intrinsic:llvm.load.relative` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.instrprof.increment.step` | not-implemented | not-implemented | unplanned |
| `intrinsic:llvm.vector.reduce.add` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.mul` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.and` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.or` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.xor` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.smax` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.smin` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.umax` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.umin` | supported (int, vector) | not-implemented | masks of up to 64 lanes, lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmax` | supported (half, float, double, vector) | not-implemented | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmin` | supported (half, float, double, vector) | not-implemented | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fadd` | supported (half, float, double, vector) | not-implemented | lanes up to 256 (WP-081) |
| `intrinsic:llvm.vector.reduce.fmul` | supported (half, float, double, vector) | not-implemented | lanes up to 256 (WP-081) |
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
| `intrinsic:llvm.coro.resume` | supported (ptr) | not-implemented | in any function (WP-105) |
| `intrinsic:llvm.coro.destroy` | supported (ptr) | not-implemented | in any function (WP-105) |
| `intrinsic:llvm.coro.done` | supported (int, ptr) | not-implemented | in any function (WP-105) |
| `intrinsic:llvm.coro.promise` | supported (int, ptr) | not-implemented | in any function; a constant power-of-two alignment (WP-105) |
| `intrinsic:llvm.coro.frame` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.noop` | supported (ptr) | not-implemented | the noop frame (WP-105) |
| `intrinsic:llvm.coro.is_in_ramp` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.void` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.bool` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.coro.await.suspend.handle` | not-implemented | not-implemented | T-XF (coroutine splitting) |
| `intrinsic:llvm.sshl.sat` | supported (int) | not-implemented | up to 64 bits (WP-084) |
| `intrinsic:llvm.ushl.sat` | supported (int) | not-implemented | up to 64 bits (WP-084) |
| `intrinsic:llvm.aarch64.crc32b` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32h` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32w` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32x` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cb` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32ch` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cw` | supported (int) | rejected | x86_64: an AArch64 property |
| `intrinsic:llvm.aarch64.crc32cx` | supported (int) | rejected | x86_64: an AArch64 property |
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
| `cc:<number>` | a calling convention LLVM has no keyword for |
| `bundle:<tag>` | an operand bundle whose tag has no row |
| `<row>/<class>` | a supported row met with a value of a class it is not implemented for (§8.3), e.g. `inst:select/double` |
| `<producer record>` | a name a producer gave to `Module::record_unsupported` (§2.2), passed through unchanged |
