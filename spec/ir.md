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
- Tested by: layout of a generated grid of types against `clang`'s
  `sizeof`/`offsetof` for the equivalent C declarations and against
  `opt`'s constant folding of `getelementptr` offsets, on both targets.

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
- `SymDiff { a, b, addend, width }`: `a - b + addend`, truncated to
  `width` bits. The normal form of `trunc (sub (ptrtoint a), (ptrtoint
  b))` and its variants, used by relative tables.
- `IntOfAddr` / `AddrOfInt`: `ptrtoint` and `inttoptr` of a constant, when
  the result type differs in kind from the operand.
- `dso_local_equivalent` and `no_cfi` wrap a function symbol.
- `Expr`: any other constant expression, kept as an operator and operand
  ids. Its only use is to let `classify` name what was rejected.

A producer must reduce to the first forms whenever the expression is
reducible; `hod_ir` provides the folding functions so that all producers
reduce the same way.

## 5. Functions

### 5.1 Delivery

A function body is built with `FunctionBuilder`, obtained from the module
for a declared function symbol, and finished into a `Body`, a value that
owns all per-function storage. The module does not keep bodies. The
consumer may compile a `Body` and drop it before the next one is built, so
peak memory follows the largest function. Everything a body refers to
(types, constants, symbols, debug nodes) must be in the module tables
before the instruction that refers to it is appended.

While bodies are compiled on worker threads the module tables are
read-only. (A producer that appends to the tables while earlier bodies are
still being compiled needs snapshot semantics; that is a planned extension,
§10.)

### 5.2 Stack objects

`Body::slots` lists the function's fixed stack objects: size, alignment,
and an optional debug variable (§6). `Value::slot(n)` is the address of
slot `n`: a constant offset from the frame pointer once the frame is laid
out. An LLVM `alloca` in the entry block with a constant size (LLVM's
"static alloca") is a slot. Any other `alloca` is the `Alloca`
instruction: a dynamic stack allocation with a size operand and an
alignment.

### 5.3 Values

A `Value` is 4 bytes: a tag and an index. Tags: instruction result,
argument, constant, symbol address, stack slot address, inline assembly
(callee position only), and "none". Blocks are `BlockId`s and appear only
in terminators, `phi` and `BlockAddr`.

Values may be referred to before they are defined (a `phi` operand, or a
block laid out before its dominator): the builder hands out instruction ids
ahead of definition with `reserve`, and `finish` fails with
`UndefinedValue` if one was never defined.

### 5.4 Instructions

An instruction is an opcode, small flags, a result type and up to three
inline operands; longer operand lists live in the body's operand pool. The
size is asserted (target: 20 bytes; at most 24). Blocks are in layout
order and each ends in exactly one terminator.

| group | instructions | notes |
|---|---|---|
| integer | `add sub mul udiv sdiv urem srem shl lshr ashr and or xor` | `nuw`/`nsw`/`exact`/`disjoint` are not kept |
| float | `fadd fsub fmul fdiv frem fneg` | fast-math flags are not kept, except `contract` only if a later spec section gives it meaning |
| compare | `icmp` (10 predicates), `fcmp` (16) | |
| cast | `trunc zext sext fptrunc fpext fptoui fptosi uitofp sitofp ptrtoint inttoptr bitcast addrspacecast` | |
| memory | `Load`, `Store` (alignment, volatile, atomic ordering), `Alloca` (dynamic only), `Fence`, `CmpXchg`, `AtomicRmw` (17 operations) | |
| address | `PtrAdd { base, offset, [(index, scale)] }` | replaces `getelementptr`: `base + offset + Σ sext_or_trunc(index) * scale`, computed in pointer width. A producer folds all constant indices into `offset`. Vector GEPs are element-wise `PtrAdd`s on vectors of pointers |
| aggregate | `ExtractValue`, `InsertValue` (index path resolved to a byte offset and a field type), `ExtractElement`, `InsertElement`, `ShuffleVector` | |
| other | `Phi`, `Select`, `Freeze`, `VaArg` | |
| call | `Call`, `Invoke` (callee value, function type, calling convention, tail kind, the call attributes of §5.7, operand bundles by name) | intrinsics: §5.5 |
| exception | `LandingPad` (cleanup flag, catch and filter clauses), `Resume` | the funclet instructions (`catchswitch` and the rest) exist as opcodes only so `classify` can reject them |
| terminator | `Br`, `CondBr`, `Switch`, `IndirectBr`, `Ret`, `Unreachable`, `CallBr` | |

### 5.5 Intrinsics

A call to an LLVM intrinsic is `Call` with an `Intrinsic` callee: a member
of a closed enum for the intrinsics hod knows (with the overload types as
type operands), or `Intrinsic::Other(name)` for any other `llvm.*` name,
which `classify` rejects by name. The enum is the list of intrinsics hod
handles; adding an intrinsic means adding a variant, its `classify` entry
and its lowering in each target.

Intrinsics that generate no code are dropped by producers and have no
representation: `llvm.lifetime.*`, `llvm.invariant.*`, `llvm.assume`,
`llvm.dbg.*` (debug records go to §6), `llvm.donothing`, and the
annotation intrinsics (which return their first argument).
`llvm.expect*` is replaced by its first argument.

### 5.6 Use facts

For each instruction result the body records, maintained by the builder
whenever an operand is appended:

- the number of uses;
- whether any use is in a different block from the definition;
- whether the only use is by the instruction immediately after it.

These are exactly what value-location policy v1 needs (`spec/codegen.md`).
They are derived data: `Body::verify` recomputes and compares them, and the
tests run it on every body they build.

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
- Layout: §3's grid against clang and `opt`, both targets.
- Constant folding: §4.2's reductions against `opt`'s output for the same
  expressions.
- Attribute table: every attribute name in LLVM 19's and 22's
  `Attributes.td` list is in exactly one class (the list is checked in as
  test data with its source version).
- Printer: §9's guarantees over the IR corpus.
