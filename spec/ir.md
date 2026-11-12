# spec: hod_ir — the IR hod consumes

Status: **design** (supervisor-owned; WP-002 to WP-005 and WP-009 implement
it and replace each "Signatures" note with the real API). Normative for
`crates/hod_ir`. Background: `docs/design/0001-input.md`.

## 1. What it is

`hod_ir` is the only input the code generator reads. It has **LLVM IR's
semantics** and is **not LLVM IR**: it is a normal form, lower than LLVM's,
in which everything a producer can resolve once has been resolved, and
everything that only an optimizer would read has been dropped. A producer
(the text parser `hod_ll`, the bitcode reader `hod_bc`, kiln's in-process
bridge) does that work while it builds the module; the code generator then
never has to.

Consequences that the rest of this spec spells out:

- Type layout (size, alignment, field offsets) is computed when a type is
  interned, for the module's data layout, and is an array lookup afterwards.
- Address arithmetic is `PtrAdd`: a base, a constant byte offset, and
  scaled indices. There is no typed `getelementptr` (§5.4).
- A function's fixed stack objects are a list in its header, not `alloca`
  instructions (§5.2).
- Each instruction result carries its use facts, maintained by the builder
  as operands are appended (§5.6). The code generator has no analysis walk
  to do for them.
- Constant addresses are `symbol + addend` (§4.2).
- Value names, optimization hints and optimization metadata are not
  represented (§7).
- The printer (§9) writes LLVM text that *behaves* like the input. It does
  not reproduce the input text.

Design rules, shared with the rest of hod: index arenas with `u32` newtype
ids and no pointer graphs; interning where equality matters; errors, not
panics, from every builder method (`IrError { kind, message }`); panics only
for an id from another module; `#![forbid(unsafe_code)]`; no dependencies.

## 2. Module

A `Module` owns the tables below. They are append-only: an id, once
returned, stays valid and its entry never moves.

| table | id | contents |
|---|---|---|
| types | `TypeId` | §3 |
| constants | `ConstId` | §4 |
| symbols | `SymId` | functions, variables, aliases, IFUNCs: one id space (§2.2) |
| COMDATs | `ComdatId` | name and selection kind |
| inline assembly | `AsmId` | template, constraints, flags, function type |
| debug | `DiId`, `LocId` | §6 |

### 2.1 Target

`Module::new(TargetSpec)`. A `TargetSpec` is an architecture and OS/ABI
(`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`) plus the parsed
data layout: pointer size and alignment, integer and float alignments,
aggregate alignment, stack alignment, endianness. A producer reading LLVM
text checks the module's `target datalayout` string against the spec's and
reports `IrErrorKind::DataLayoutMismatch` on a difference that changes
layout.

Module properties that affect code: PIC level, PIE level, code model,
default unwind-table kind, frame-pointer policy, branch protection
(BTI, pointer authentication, CET), DWARF version, and the module-level
assembly strings. LLVM module flags map onto these; a flag hod does not
know is kept by name so `classify` (§8) can reject it if it is not on the
"ignorable" list.

### 2.2 Symbols

One `SymId` space, because code refers to all of them the same way: by
address. A symbol has a name (bytes, interned), a kind, and:

- common: linkage (the eleven LLVM linkages), visibility, `dso_local`, DLL
  storage is not represented, `unnamed_addr`, section name, COMDAT,
  alignment, "retained" (member of `llvm.used` / `llvm.compiler.used`);
- **variable**: value type, constant?, thread-local model, initializer
  (`ConstId`, or none for a declaration), address space (0 only);
- **function**: function type, calling convention, the attributes of §5.7,
  personality symbol, and whether a body will be delivered;
- **alias** / **IFUNC**: the target (a constant address, §4.2) or resolver.

The special LLVM arrays are not symbols. `llvm.global_ctors` and
`llvm.global_dtors` become `Module::ctors` / `dtors`: lists of
`(priority, function, associated symbol)`. `llvm.used` and
`llvm.compiler.used` become the "retained" bit.

Symbols can be declared at any time before they are referenced, so a
producer that discovers callees while lowering a body can declare them as
it goes.

## 3. Types

Interned in the module's type table. Kinds: `void`; integers of any width
from 1 to 2^23 bits; `half`, `bfloat`, `float`, `double`, `fp128`,
`x86_fp80`; pointer (address space kept, only 0 supported); fixed vector;
scalable vector; array; struct (field list, packed?); function (return,
parameters, variadic?); `label`, `token`, `metadata` (accepted where LLVM
requires them, never as storage); target extension type (by name).

- Structural types are hash-consed. LLVM's named ("identified") structs are
  interned by structure too: two named structs with the same body are one
  `TypeId`. Names are not kept. An opaque struct is a distinct unsized type.
- Fixed ids exist for the common types (`TypeId::{VOID, PTR, I1, I8, I16,
  I32, I64, I128, FLOAT, DOUBLE, ...}`).
- **Layout is part of the type.** For every sized type the table stores
  store size, alloc size and ABI alignment, and for a struct the byte
  offset of each field, computed at interning from the data layout by
  LLVM's rules (LangRef "Data Layout"; `StructLayout`). `size_of`,
  `align_of` and `field_offset` are O(1). Scalable vectors have a known
  minimum size and a "scalable" flag.
- Tested by: layout of a generated grid of types against the sizes,
  alignments and offsets `llc` gives them (the target's own data layout)
  and against `opt` (custom data layouts, which `llc` ignores), on both
  targets, with LLVM 19 and 22. Most LLVM types have no C declaration, so
  a C compiler is not the oracle.

## 4. Constants

Interned per module.

### 4.1 Data

Integers (a value up to 64 bits is stored inline; wider values are words
in a pool), floats (bit pattern), `null`, zero of any type, `undef`,
`poison`, the `none` token, arrays, structs, vectors (with a splat form),
and byte strings. All-zero aggregates are canonicalized to the zero
constant. `undef` and `poison` are distinct constants (the printer needs
the difference) and identical for code generation: any value, here zero.

### 4.2 Addresses

- `Addr { sym, addend }`: the address of a symbol plus a signed byte
  offset. This is the normal form of every LLVM constant expression that
  denotes an address: a symbol, `getelementptr` constant expressions over
  it (folded with §3 layout), and the pointer casts LLVM still has as
  expressions, in any nesting.
- `BlockAddr { function, block }`.
- `SymDiff { ty, a, b, addend, plt }`: `a - b + addend` as a value of
  integer type `ty`. The normal form of `trunc (sub (ptrtoint a),
  (ptrtoint b))` and its variants, used by relative tables. `plt` says
  that `a` was named through `dso_local_equivalent` (relative vtables).
  `a - a` without `plt` is the integer `addend`.
- `IntOfAddr` / `AddrOfInt`: `ptrtoint` and `inttoptr` of a constant, when
  the result type differs in kind from the operand.
- `dso_local_equivalent` and `no_cfi` wrap a function symbol.
- `Expr`: any other constant expression, kept as an operator and operand
  ids. Its only use is to let `classify` name what was rejected.

A producer passes every constant expression through the folding functions
`hod_ir` provides (`fold_gep`, `fold_cast`, `fold_binary`) and hands on
the result, so that all producers reduce the same way. What the functions
reduce is the definition of "reducible": address arithmetic whose result
is again a number, a symbol plus a number, or a symbol minus a symbol plus
a number, and integer arithmetic on numbers. Everything else stays `Expr`.

- An addend is reduced to the signed range of the field it will be
  written to, so that equal values are one constant. LLVM keeps the
  unreduced sum; the two agree modulo the field's width.
- Two expressions LLVM can emit are deliberately left as `Expr`, and so
  are declined by `classify`: a number minus an address (`sub (i64 4,
  ptrtoint @a)`), and an address in an integer whose width is not 8, 16,
  32 or 64 bits. Neither has been seen from a C++ front end; a normal
  form is added when one is.

### 4.3 Initializer images

`ConstPool::flatten(types, c, bytes, fixups)` appends the image of
constant `c` to `bytes` and a `Fixup { offset, size, target, addend }` to
`fixups` for each address in it. Exactly `alloc_size_of(type of c)` bytes
are appended; fixup offsets are positions in `bytes`, in increasing order;
on error both buffers are left as they were.

- Little-endian targets only (`UnsupportedTarget` otherwise).
- An integer or float occupies its store size, low byte first; bits above
  an integer's width are zero. `x86_fp80` is ten bytes.
- Padding, `undef` and `poison` are zero, with one exception for identity
  with LLVM: the tail padding of a vector of `i8`, `i16`, `i32`, `i64`,
  `half`, `bfloat`, `float` or `double` numbers whose bytes are all one
  byte repeats that byte.
- A struct field is at its field offset; array element `i` at `i` times
  the element's alloc size; element `i` of a vector whose elements are
  whole bytes at `i` times the element's width in bytes. A vector whose
  elements are not whole bytes is one little-endian integer with element 0
  in its lowest bits.
- An address leaves zero bytes and a fixup: `Sym(s)` for `Addr` and
  `no_cfi`; `Plt(s)` for `dso_local_equivalent`; `Block { func, block }`;
  `Diff { a, b }` and `PltDiff { a, b }` for `SymDiff`. The field's value
  is target + addend, truncated to `size` bytes (8 for a pointer; 1, 2, 4
  or 8 for an integer).
- A `Diff { a, b }` can be written only when `a` and `b` are defined in
  one section, or `b` is defined in the section that holds the field.
  `flatten` cannot know sections; whoever turns fixups into relocations
  checks this and reports the variable as unsupported otherwise.
- No image (`UnsupportedInitializer`): `Expr`, the `none` token, an
  address in an integer whose width is not 8, 16, 32 or 64 bits, an
  address in a vector whose elements are not whole bytes. `UnsizedType`
  and `InvalidType` (scalable) for types without a fixed size.
- A writable variable belongs in `.bss` exactly when
  `ConstPool::is_zero_or_undef` holds for its initializer.
- Tested by: `tests/flatten_diff.rs` against `llc`'s `.data` and
  relocations, both targets, LLVM 19 and 22.

## 5. Functions

### 5.1 Delivery

A function body is built with `FunctionBuilder::new(&module, sym, fn_ty)`
for a function symbol and its function type, and finished into a `Body`, a
value that owns all per-function storage. The module does not keep bodies.
The consumer may compile a `Body` and drop it before the next one is built,
so peak memory follows the largest function. Everything a body refers to
(types, constants, symbols, debug nodes) must be in the module tables
before the instruction that refers to it is appended.

The builder reads the module only in `new` (for the parameter count). After
that it holds no borrow of it: every append states the result type, and the
builder looks no type up. One module can feed builders on many threads, and
a `Body` is `Send`.

While bodies are compiled on worker threads the module tables are
read-only. (A producer that appends to the tables while earlier bodies are
still being compiled needs snapshot semantics; that is a planned extension,
§10.)

Building, in order:

1. `create_block()` returns a `BlockId`; ids count up in creation order.
   A block may be created long before it is placed, so that branches can
   name it.
2. `begin_block(b)` places `b` next in the layout and opens it. The first
   block begun is the entry block. Only one block is open at a time.
3. Append methods, one per instruction (§5.4), add to the open block and
   return the instruction's result as a `Value` (for an instruction
   without a result, a value that must not be used). Appending a
   terminator closes the block.
4. `finish()` returns the `Body`, or fails: `BadBlock` if no block was
   begun, a created block was never begun or the last one is still open;
   `UndefinedValue` for a reserved value (§5.3) that was never resolved.

In a finished body instruction ids are positions in layout order: the
instructions of a block are the consecutive ids `first..end`, its `phi`s
come first, its terminator is `end - 1`, and the blocks' ranges follow each
other in layout order without gaps. So `id + 1` is the next instruction of
the block unless `id` is its terminator. `Body::layout` lists the block ids
in layout order.

**Recycling.** `FunctionBuilder::reusing(&module, sym, fn_ty, storage)`
builds into the tables of a `Body` the consumer is done with (or of one
that `abandon()` returned from a builder given up on). Once the tables have
grown to the largest function, building allocates nothing. The body built
is equal (`Body: Eq`, every table compared) to the one built in fresh
storage. A failed append leaves the builder as it was; a failed `finish`
drops the storage.

Limits, all reported as `TableFull`: 2^28 instructions and 2^28 reserved
values per body, 2^29 arguments, slots, constants and symbols nameable by a
`Value`, 2^32 - 1 entries in any pool.

### 5.2 Stack objects

`Body::slots` lists the function's fixed stack objects, each a `Slot` of a
size in bytes (zero allowed) and a power-of-two alignment.
`FunctionBuilder::add_slot(size, align)` returns a `SlotId`, and
`slot(id)` its address as a `Value`: a constant offset from the frame
pointer once the frame is laid out. An LLVM `alloca` in the entry block
with a constant size (LLVM's "static alloca") is a slot; the producer
multiplies the element size by the count. Any other `alloca` is the
`Alloca` instruction: a dynamic stack allocation of `count * elem_size`
bytes with an alignment.

A slot is not typed. Use facts (§5.6) are not kept for slots: a slot's
address is a constant, never a value that needs a location.

`Body::slot_vars` lists the source variables that live in a slot for the
whole function: `SlotVar { slot, var: DiId, loc: LocId }`, recorded with
`set_slot_var` for a `#dbg_declare` of a static `alloca`. It is a separate
list, in recording order, so that a body without debug information pays
nothing and several variables can share a slot. The declaration's
`DIExpression` is the attachment point left for §6: `SlotVar` gains the
expression when expressions exist, and until then a producer records only
declarations whose expression is empty.

### 5.3 Values

A `Value` is 4 bytes: a 3-bit tag and a 29-bit index. Tags: instruction
result, argument, constant, symbol address, stack slot address, inline
assembly (callee position only), and "none" (`Value::NONE`: the value of
`ret void`, the callee of an intrinsic call). `Value::kind()` decodes it
to a `ValueKind`. Blocks are `BlockId`s and appear only in terminators,
`phi` and `BlockAddr`; they are never `Value`s.

Values may be referred to before they are defined (a `phi` operand, or a
block laid out before its dominator). `reserve(ty)` returns a placeholder
`Value` that can be used as an operand at once; `resolve(placeholder,
value)` gives it its definition, which may be any operand, another
placeholder included. `finish` rewrites every operand that named a
placeholder, so a finished body has none, and fails with `UndefinedValue`
if any placeholder, used or not, was never resolved (or only through a
cycle of placeholders). The type given to `reserve` is what
`FunctionBuilder::value_type` answers for the placeholder; it is not
checked against the definition.

The builder cannot hand out the final instruction id ahead of time, because
ids are layout positions (§5.1) and a producer that meets a use first does
not know how many instructions come before the definition.

### 5.4 Instructions

An instruction is 20 bytes (asserted): an opcode, a one-byte sub-code
(predicate, `atomicrmw` operation), 16 bits of flags, a result type and
three inline operand words. Longer operand lists (call arguments, `phi`
inputs, `PtrAdd` indices, `switch` cases, shuffle masks) live in the body's
pools, and the inline words give their position and length. Blocks are in
layout order and each ends in exactly one terminator.

| group | instructions | notes |
|---|---|---|
| integer | `add sub mul udiv sdiv urem srem shl lshr ashr and or xor` | `nuw`/`nsw`/`exact`/`disjoint` are not kept |
| float | `fadd fsub fmul fdiv frem fneg` | fast-math flags are not kept, except `contract` only if a later spec section gives it meaning |
| compare | `icmp` (10 predicates), `fcmp` (16) | `samesign` is not kept |
| cast | `trunc zext sext fptrunc fpext fptoui fptosi uitofp sitofp ptrtoint ptrtoaddr inttoptr bitcast addrspacecast` | LLVM's `castIsValid`; `ptrtoaddr` (LLVM 22) yields exactly the address (index) width of the pointer's address space |
| memory | `Load`, `Store` (`MemFlags`: alignment, volatile, atomic ordering and synchronization scope), `Alloca` (dynamic only), `Fence`, `CmpXchg` (two orderings, weak), `AtomicRmw` (21 operations: LLVM 22's) | atomic rules below |
| address | `PtrAdd { base, offset, [(index, scale)] }` | replaces `getelementptr`: `base + offset + Σ sext_or_trunc(index) * scale`, computed in the pointer's index width. A producer folds all constant indices into `offset`. With a vector base or index it is element-wise and yields a vector of pointers. `inbounds`/`nuw`/`nusw` are not kept |
| aggregate | `ExtractValue`, `InsertValue` (byte offset of the field and its type), `ExtractElement`, `InsertElement`, `ShuffleVector` (constant mask, `SHUFFLE_UNDEF` for an undefined element) | `aggregate_field(types, agg, path)` resolves an LLVM index path to `(offset, field type)` |
| other | `Phi`, `Select`, `Freeze`, `VaArg` | |
| call | `Call`, `Invoke`, `CallBr` (callee, function type, calling convention, tail kind, the call attributes of §5.7, operand bundles by name) | side table below; intrinsics: §5.5 |
| exception | `LandingPad` (cleanup flag, catch and filter clauses), `Resume` | the funclet instructions (`catchswitch`, `catchpad`, `catchret`, `cleanuppad`, `cleanupret`) exist as opcodes only, appended with `funclet(op, ty)` and carrying no operands, so `classify` can reject them |
| terminator | `Br`, `CondBr`, `Switch`, `IndirectBr`, `Ret`, `Unreachable`, `Resume`, `Invoke`, `CallBr`, and the funclet terminators | `Opcode::is_terminator` is LLVM's set |

Rules that are LLVM's and are checked against LLVM 22 (§11):

- **Atomic orderings.** `load`: not `release` or `acq_rel`. `store`: not
  `acquire` or `acq_rel`. `atomicrmw` and `cmpxchg` success: not
  `unordered`. `cmpxchg` failure: not `unordered`, `release` or `acq_rel`
  (it may be stronger than the success ordering). `fence`: `acquire` or
  stronger. `not_atomic` is the absence of an ordering in `MemFlags`.
- **Atomic types.** A type accessed atomically is an integer, pointer or
  float, or a vector of them, whose size in bits is a power of two and at
  least 8 (a scalable vector counts with its minimum size). `atomicrmw
  xchg` takes an integer, float or pointer; the float operations take a
  float or a fixed vector of floats; the others an integer. `cmpxchg`
  takes an integer or pointer and its result is `{ T, i1 }`.
- **Aggregate paths.** `extractvalue`/`insertvalue` paths are not empty,
  index struct fields and array elements only (never into a vector), and
  each index is in range. hod stores the resulting byte offset; `verify`
  accepts an offset and field type only if some path leads to that type at
  that offset. An `InsertValue` offset is at most 2^32 - 1.
- **`switch`.** Case values are distinct integer literals of the
  condition's type.
- **`phi`.** One incoming entry per predecessor *edge* (a predecessor
  that branches to the block twice is listed twice, with the same value),
  and no entry for a block that is not a predecessor.
- **Landing pads.** A `landingpad` is the first instruction after the
  `phi`s of a block that is entered only by the unwind edges of
  `invoke`s; every unwind destination starts with one; all landing pads
  of a function have one type; the entry block has no predecessors.

**Calls.** `call`, `invoke` and `callbr` take a `CallDesc`: the callee (a
`Value`, or an `Intrinsic` with its overload types), the function type the
call is made with, arguments, `CallConv`, `TailKind`, `noreturn`,
`nounwind`, `returns_twice`, the `ParamAttrs` of the return value and of
each argument (§5.7; stored only for calls that have any), and operand
bundles (a tag and input values each). The instruction record points into
the **call side table**, which holds one record per call in layout order.
`Body::calls()` iterates it without walking instructions, which is how a
target finds the largest outgoing-argument area; `Inst::call()` goes from
an instruction to its `CallSite`. An inline-assembly callee is
`Value::asm(id)`. A direct call is a call whose callee is a symbol value;
any other callee value is an indirect call.

**Landing pads** have their own side table (`Body::landing_pads()`,
`Body::has_landing_pads()`): the instruction, the cleanup flag, and the
clauses, each `Clause::Catch(constant)` (a type-information pointer, or
`null` for catch-all) or `Clause::Filter(constant array)`.

### 5.5 Intrinsics

A call to an LLVM intrinsic is `Call` (or `Invoke`) with an `Intrinsic`
callee: a member of a closed enum for the intrinsics hod knows, with the
overload types as type operands of the call (`CallSite::overloads`), or
`Intrinsic::Other(name)` for any other `llvm.*` name, which `classify`
rejects by name. The enum is the list of intrinsics hod recognises; adding
one means adding a variant, its `classify` entry and its lowering in each
target. Being on the list says nothing about whether a target lowers it.

**One name mapping, shared by all producers.**
`resolve_intrinsic(types, name, fn_ty)` maps an LLVM name and function
type to an `IntrinsicRef`: the member and its overload types. It answers a
known member only if `name` is exactly the name LLVM gives the intrinsic
for the overload types read from `fn_ty` (LLVM's suffix mangling: `i32`,
`f64`, `p0`, `v4i32`, `nxv2f32`, ...); anything else that starts with
`llvm.` is `Other(name)`. LLVM itself renames a declaration with a wrong
suffix, so a producer reading LLVM's output never sees one. One exception:
`llvm.thread.pointer` without a suffix, which LLVM 19 writes, resolves to
the member. `Intrinsic::mangled_name(types, overloads)` is the inverse, for
the printer.

The list (133; each with every overload LLVM allows):

- memory: `memcpy memcpy.inline memmove memset memset.inline`
- variable arguments and stack: `va_start va_end va_copy stacksave
  stackrestore frameaddress returnaddress`
- traps: `trap debugtrap ubsantrap`
- bits: `bswap bitreverse ctpop ctlz cttz fshl fshr abs smax smin umax
  umin scmp ucmp clmul`
- checked and saturating arithmetic: `{s,u}{add,sub,mul}.with.overflow`,
  `{s,u}{add,sub}.sat`
- floating point: `fabs sqrt floor ceil trunc round roundeven nearbyint
  rint copysign fma fmuladd minnum maxnum minimum maximum minimumnum
  maximumnum canonicalize arithmetic.fence pow powi ldexp frexp sin cos tan
  asin acos atan atan2 sinh cosh tanh exp exp2 exp10 log log2 log10 lround
  llround lrint llrint fptosi.sat fptoui.sat is.fpclass`
- C++ runtime support: `eh.typeid.for threadlocal.address thread.pointer
  prefetch clear_cache readcyclecounter is.constant objectsize ptrmask
  load.relative instrprof.increment instrprof.increment.step`
- vectors: `vector.reduce.{add,mul,and,or,xor,smax,smin,umax,umin,fmax,
  fmin,fadd,fmul}`, `vector.extract`, `vector.insert`
- coroutines: `coro.id coro.alloc coro.begin coro.end coro.free coro.size
  coro.align coro.save coro.suspend coro.resume coro.destroy coro.done
  coro.promise coro.frame coro.noop coro.is_in_ramp
  coro.await.suspend.{void,bool,handle}`

Not on the list, and therefore `Other`: target-specific intrinsics
(`llvm.aarch64.*`, `llvm.x86.*`), intrinsics with metadata arguments (the
constrained floating-point family, `llvm.read_register`), the masked
vector family (its signatures differ between LLVM 19 and 22), and anything
no frontend in scope emits at `-O0`.

**The no-code rule.** Intrinsics that generate no code have no
representation. `no_code_intrinsic(name)` is the one place that says which
and what a producer does with the call:

| names | rule |
|---|---|
| `llvm.lifetime.start/end`, `llvm.invariant.start/end`, `llvm.assume`, `llvm.donothing`, `llvm.var.annotation` | `NoCode::Drop`: omit the call. (The `{}*` result of `invariant.start` can only be used by `invariant.end`.) |
| `llvm.expect`, `llvm.expect.with.probability`, `llvm.annotation`, `llvm.ptr.annotation`, `llvm.launder.invariant.group`, `llvm.strip.invariant.group` | `NoCode::FirstArg`: omit the call and use its first argument as its result |
| `llvm.dbg.*` | `NoCode::DebugRecord`: a debug record (§6), not a call |

A producer asks `no_code_intrinsic` before `resolve_intrinsic`. A body
that still calls one of these names carries it as `Other`, and `classify`
rejects it.

### 5.6 Use facts

For each instruction result and each argument the body records `UseFacts`
(4 bytes), maintained by the builder whenever an operand is appended:

- the number of uses (`count`);
- whether any use is in a different block from the definition
  (`used_outside_block`);
- for instruction results, whether the only use is by the instruction
  immediately after the definition (`only_use_is_next`).

Definitions:

- A **use** is one operand position of one instruction. An instruction
  that names a value twice uses it twice. The callee of a call and the
  inputs of its operand bundles are uses. A constant, symbol or slot
  operand is not counted anywhere.
- A use is **located** in the block of the instruction that has the
  operand, with one exception: an incoming value of a `phi`, `[ %v, %pred
  ]`, is located at the end of `%pred`, where the copy for that edge is
  made. It is "in a different block" exactly when `%pred` is not the block
  that defines `%v`. So a value defined in a loop block and fed to that
  block's own `phi` along the back edge is not used outside its block, and
  a value defined in one block and fed along an edge from a second block
  to a `phi` in a third is.
- An **argument** is defined in the entry block: its `used_outside_block`
  says whether any use is located outside the entry block.
  `only_use_is_next` is never true for an argument.
- `only_use_is_next` is true exactly when the count is 1, the use is not
  by a `phi`, and the user is the instruction with the next id in the
  same block. A `phi` use never satisfies it, even when the `phi` has the
  next id (the first instruction of the next block in layout).

`Body::uses(inst)`, `Inst::uses()` and `Body::arg_uses(index)` read them.
These are exactly what value-location policy v1 needs
(`spec/codegen.md`). They are derived data: `Body::verify` recomputes them
from the instructions and fails with `UseFactsMismatch` on any difference,
and the tests run it on every body they build.

Uses through a placeholder (§5.3) are attributed when `finish` resolves
it, with the same rules.

### 5.7 Attributes

LLVM attributes fall in three classes, listed exhaustively in the
implementation as one table (attribute name → class) that `hod_ll` and
`classify` both read:

- **kept**, because they change the code: on parameters and returns `sret`
  (with type), `byval` (type, alignment), `byref`, `zeroext`, `signext`,
  `inreg`, `nest`, `swiftself`/`swifterror`/`swiftasync`, `alignstack`,
  `preallocated`, `inalloca`, `elementtype` (inline assembly); on functions
  `noreturn`, `naked`, `nounwind`, `uwtable`, `returns_twice`,
  `alwaysinline` (it decides what happens to an `available_externally`
  body: design 0001, point 6),
  `alignstack`, the stack-protector levels, `sanitize_*`,
  `presplitcoroutine`, and the string attributes `target-cpu`,
  `target-features`, `tune-cpu`, `frame-pointer`,
  `patchable-function-entry`/`-prefix`, `function-instrument`,
  `branch-target-enforcement`, `sign-return-address*`,
  `stack-probe-size`, `probe-stack`, `split-stack`,
  `zero-call-used-regs`, `no-builtins` class; on calls `noreturn`,
  `nounwind`, `returns_twice`, the tail kind and the calling convention.
- **dropped**, because only an optimizer reads them: `noundef`, `nonnull`,
  `dereferenceable*`, `noalias`, `nocapture`/`captures`, `readonly` and the
  `memory(...)` family, `nofree`, `nosync`, `willreturn`, `mustprogress`,
  `noinline`, `optnone`,
  `minsize`, `optsize`, `cold`, `hot`, `speculatable`, `allocsize`,
  `allockind`, `nofpclass`, `range`, `writable`, `dead_on_unwind`,
  `dead_on_return`, `initializes`, `returned`, `immarg`, `nocallback`,
  `nomerge`, `nobuiltin`, and the fast-math string attributes.
- **rejected**: anything else. An attribute that is in neither list is a
  `classify` failure naming the attribute. It is never silently dropped.

Shared representation (`attrs` module, used by function symbols and by
call instructions):

- `CallConv` is LLVM's calling-convention number (0 to 1023); every number
  is representable and `classify` names the unsupported ones.
- `ParamAttrs` holds the kept attributes of one parameter or of the return
  value in eight bytes: the flags (`zeroext`, `signext`, `inreg`, `nest`,
  the three `swift*`), `align`, `alignstack`, and at most one typed
  attribute (`ParamMem`: `sret`, `byval`, `byref`, `inalloca`,
  `preallocated`, `elementtype`) with its type. LLVM allows `elementtype`
  beside another typed attribute on an inline-assembly operand; that is
  not representable, and a producer that meets it reports the function as
  unsupported.

### 5.8 What is checked, and when

The builder checks, on every append, what keeps the body's tables sound.
It needs no type table for it and costs a few compares per operand:

| check | error |
|---|---|
| a block is open; the block being begun exists, is not open and was not begun before; a `phi` is appended before any other instruction of its block; `finish` rules of §5.1 | `BadBlock` |
| every operand names something that exists in this body (an instruction already appended or a placeholder, an argument, a slot); a block operand was created; an alignment is a power of two up to 2^32; `resolve` and `add_incoming` are given a placeholder and a `phi` of this builder; a call's attribute list is empty or has one entry per argument; an intrinsic callee carries the number of overload types its member has | `InvalidOperand` |
| a count or offset fits its field (§5.1 limits) | `TableFull`, `TypeTooLarge` |
| `FunctionBuilder::new` is given a function type | `InvalidType` |
| a placeholder is resolved by `finish` | `UndefinedValue` |

`Body::verify(&module)` checks everything else that can be decided from
the body and the module, and re-checks the above from the finished tables,
so that it also judges a body that did not come from the builder:

| check | error |
|---|---|
| the tables are consistent: block ranges tile the instructions in layout order, each block ends in its only terminator, `phi`s first, landing pads placed as in §5.4, `phi` lists match predecessor edges | `BadBlock` |
| operands, ids, pool ranges, side-table links and counts are in range and allowed in their position; atomic orderings (§5.4); `switch` cases; call argument counts against the function type; an inline-assembly value only as a callee | `InvalidOperand` |
| types that must agree do: operands with each other and with the result, call arguments with the function type's parameters, the result with its return type, `ret` with the function's, `resume` and landing pads, a known intrinsic's overload types with its function type | `TypeMismatch` |
| a type is of the class the instruction needs (integer, float, pointer, vector, aggregate, a valid cast, an atomic type) | `InvalidType` |
| a loaded, stored or `va_arg` type is sized | `UnsizedType` |
| an aggregate offset names a field of that type; a shuffle mask entry selects an element | `BadIndex` |
| the recorded use facts are the recomputed ones (§5.6) | `UseFactsMismatch` |

`verify` returns the first violation. It is the definition of "well
formed" that code generation may assume. Tests run it on every body; a
producer under test runs it on every body it hands over; a release build
need not run it at all.

Not checked: that a definition dominates its uses (LLVM allows anything in
unreachable blocks, and no decision in code generation rests on it), and
that a symbol or inline-assembly id exists, which becomes checkable when
the module has those tables (§2.2).

### 5.9 Reading a body

Code generation reads a body through borrowed views. Every accessor is an
array lookup and none allocates.

| question | API |
|---|---|
| the function | `Body::symbol`, `fn_type`, `ret_type(&m)`, `arg_count`, `arg_types(&m)`, `arg_uses(i)` |
| blocks in layout order | `Body::blocks()` (`Block`s), `layout()` (ids), `entry()`, `block(id)`, `block_count()` |
| a block | `Block::id`, `inst_range`, `insts()`, `has_phis()`, `phis()`, `non_phis()`, `terminator()`, `successors()` |
| an instruction | `Body::inst(id)`, `insts()`; `Inst::id`, `opcode`, `ty`, `result`, `uses`, `loc`, `operands()` (the value operands as one slice, whatever the opcode), `is_terminator`, `successors()`, `call()`, and `kind()`, an `InstKind` with one variant per instruction shape and named fields |
| lists | `PtrAdd::{base, offset, indices()}`, `Phi::{values(), blocks(), incoming()}`, `Switch::{cond, default, cases()}`, `InstKind::ShuffleVector { mask }`, `InstKind::IndirectBr { targets }` |
| calls | `Body::calls()`, `call_count()`; `CallSite::{inst, opcode, ty, callee, callee_value, intrinsic, overloads, fn_type, args, cc, tail, is_noreturn, is_nounwind, returns_twice, has_param_attrs, ret_attrs, arg_attrs(i), bundles(), normal_dest, unwind_dest, indirect_dests}` |
| exceptions | `Body::landing_pads()`, `has_landing_pads()`; `LandingPad::{inst, is_cleanup, clauses}` |
| the frame | `Body::slots()`, `slot_vars()` |
| types of values | `Body::value_type(&m, value)` |
| source locations | `Body::has_locs()`, `loc(id)`; the array exists only if the builder was ever given a location (`set_loc`), §6 |
| cost | `Body::inst_count()`, `table_bytes()`, `allocated_bytes()` |

`Block::phis()` and `has_phis()` exist because a branch to a block with
`phi`s must place the edge's copies, and every branch asks.

A body costs 20 bytes per instruction, 4 for its use facts, 4 for its
location when there is debug information, 16 per block, 40 per call plus 4
for the callee and each argument, and 4 or 8 per entry of the other
lists.
`examples/body_throughput.rs` prints bytes and time per instruction for an
unoptimized-C++ instruction mix; the figures of record are in
`spec/landed/ir/WP-004.md`.

## 6. Debug information

Present only when the producer was asked for it. Typed, not generic
name-and-field nodes: one Rust type per kind hod emits DWARF for.

- `LocId`: an interned `(line, column, scope, inlined-at)`. Each
  instruction has a `LocId` in a side array of the body that exists only
  with debug info (`LocId::NONE` for "no location").
- `DiId` nodes: compile unit, file, subprogram, lexical block (and block
  file), namespace, local variable, global variable (with expression),
  label, imported entity, basic type, derived type, composite type,
  subroutine type, subrange, enumerator, template type and value
  parameters. Fields are the LLVM fields that DWARF emission reads.
- A function symbol has an optional subprogram; a variable symbol has its
  global-variable expressions.
- Variable locations: a stack slot (§5.2) carries `(variable, expression,
  declaration location)` when a `#dbg_declare` (or `llvm.dbg.declare`)
  names its address. A declare of anything else, and every `#dbg_value`,
  is a `Body::dbg_records` entry positioned before an instruction.
- `DIExpression` is a sequence of typed operations (`Deref`, `PlusUconst`,
  `Constu`, `StackValue`, `Fragment`, ..., and `Other(opcode)` for
  `classify` to name).

Signatures: WP-005, which also owns `spec/landed/ir/WP-005.md` listing the
LLVM field → hod field mapping.

## 7. Not represented

Dropped by producers; a module that differs only in these produces the
same `hod_ir`: value and block names; `; comments`; the `source_filename`;
optimization attributes (§5.7); instruction flags that only license
optimizations; instruction metadata other than `!dbg` (`!tbaa`, `!range`,
`!prof`, `!llvm.loop`, `!nonnull`, `!noundef`, `!align`, `!srcloc`,
`!annotation`, ...); `!llvm.ident`; use-list orders; module summaries.

`!llvm.linker.options` is kept: it changes the object file.

## 8. classify

`classify(&Module)` and `classify_body(&Module, &Body)` return
`Ok(())` or `Unsupported { construct, symbol }`, where `construct` is a
stable kebab-case name (`intrinsic:llvm.coro.id`, `type:x86_fp80`,
`inst:catchswitch`, `attr:preallocated`, `cc:swifttailcc`,
`const-expr:...`, `module-flag:...`). It is the single place where
"supported" is decided:

- the driver calls it before code generation, and a rejection becomes the
  fallback reason in the coverage log;
- code generation may assume everything `classify` accepts, and reaching
  an unhandled case there is a bug, reported as an internal error, never a
  guess;
- `docs/status/support.md` is generated from it.

`classify` starts by rejecting nearly everything. A work package that
implements a construct flips its entry and adds the tests in the same
change.

## 9. Printer

`Module::print` writes LLVM textual IR (dialect selected by an LLVM major,
19 to 22) for the module and for a body. Guarantees:

- `llvm-as` accepts it and `opt -passes=verify` verifies it;
- compiled by `llc`, it behaves as the producer's input did (checked by
  the differential harness, which is also how producers are validated:
  `input → producer → print → llc` against `input → llc`);
- it is deterministic.

It is not a round trip of the input text: `PtrAdd` prints as an `i8`
`getelementptr` plus explicit index arithmetic, slots print as entry-block
`alloca`s, values and blocks are numbered, dropped information stays
dropped. Uses: debugging, reduced reproducers, and splitting a module into
"functions for hod" and "functions for `llc`" for bisection.

## 10. Planned extensions

Each needs a measurement before it is specified (design 0001, "Beyond LLVM
IR").

- **Call shapes from the producer.** Argument and return locations
  assigned by the frontend's ABI layer, so the target does not re-derive
  them from IR types.
- **Tables that grow during parallel code generation**, for a producer
  that streams bodies to workers while it is still lowering.
- **A serialized form**, if a use appears for one (caching, distributed
  builds). In-process delivery needs none.

## 11. Tests

- Unit: interning, the size assertions, every `IrErrorKind`, forward
  references, use facts against `verify`.
- Function bodies (§5): each instruction group built and read back
  through §5.9; placeholders; the use-fact rules of §5.6, one test per
  rule, including the block-boundary and `phi` cases; recycling
  (`tests/body_build.rs`). A deterministic generator builds random
  well-formed bodies over every opcode but the funclets', which must
  verify, build identically in recycled storage and not finish with a
  placeholder unresolved; and damages finished bodies one table entry at
  a time, which `verify` must refuse with the kind of §5.8 (`random`
  module).
- LLVM's rules in §5.4 and §5.5, against `llvm-as`, `llvm-dis` and `llc`
  22 (`tests/body_llvm.rs`): hod and LLVM accept the same cases of a grid
  of casts, of atomic accesses (orderings, types, operations) and of
  aggregate index paths, whose byte offsets are compared with the ones
  `llc` computes; an opcode ends a block in LLVM exactly when
  `is_terminator`; LLVM keeps the mangled name of every listed intrinsic
  and renames a wrong one; `llc -O0` emits the same assembly with a
  no-code intrinsic call as with what §5.5's rule makes of it.
- Layout: §3's grid against `llc` and `opt`, both targets, LLVM 19 and 22.
- Constant folding: §4.2's reductions against the assembly `llc` writes
  for the same expressions as initializers (`symbol + addend` has no
  folded form in IR text that `opt` could print).
- Initializer images: §4.3 against `llc`'s `.data` bytes and relocations.
- Attribute table: every attribute name in LLVM 19's and 22's
  `Attributes.td` list is in exactly one class (the list is checked in as
  test data with its source version).
- Printer: §9's guarantees over the IR corpus.
