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
returned, stays valid and its entry never moves. The entry of a type or a
constant never changes; a symbol's description changes only as §2.2
allows.

| table | id | contents | reached through |
|---|---|---|---|
| types | `TypeId` | §3 | the field `module.types` |
| constants | `ConstId` | §4 | the field `module.consts` |
| symbols | `SymId` | functions, variables, aliases, IFUNCs: one id space (§2.2) | `symbol_named`, `set_*`, `symbol` |
| COMDATs | `ComdatId` | name and selection kind | `comdat_named`, `set_comdat_kind`, `find_comdat`, `comdat`, `comdat_count` |
| inline assembly | `AsmId` | template, constraints, flags, function type | `inline_asm`, `asm`, `asm_count` |
| function attribute sets | `FnAttrsId` | the kept attributes of a function (§5.7) | `intern_fn_attrs`, `fn_attrs`, `fn_attrs_count` |
| debug | `DiId`, `LocId` | §6 | |

Every method that can refuse its input returns `IrResult`. A method that
has nothing to refuse returns a plain value and panics only for an id
that is not this module's.

- **COMDATs** are a name space of their own (LLVM's `$name`), so a COMDAT
  and a symbol may share a name and usually do. `comdat_named(name) ->
  IrResult<ComdatId>` returns the COMDAT of that name, creating it with
  kind `any`; `set_comdat_kind(id, ComdatKind)` sets one of the five kinds
  (`Any`, `ExactMatch`, `Largest`, `NoDeduplicate`, `SameSize`);
  `comdat(id) -> Comdat { name, kind }`. Membership is the `comdat` field
  of a symbol. A COMDAT entry is 12 bytes.
- **Inline assembly.** `inline_asm(&InlineAsm { template, constraints,
  flags, ty }) -> IrResult<AsmId>` interns a callee; equal callees are one
  id. `flags` is an OR of `SIDE_EFFECT`, `ALIGN_STACK`, `INTEL_DIALECT`,
  `UNWIND`. `ty` must be a function type (`InvalidType`). The template and
  the constraint string are bytes that only a target reads. An entry is
  16 bytes.
- **Module-level assembly.** `append_module_asm(text)` appends one
  `module asm` string, and a newline if the text does not end in one;
  `module_asm()` is the whole text.
- **Linker options.** `add_linker_option(&[strings])` appends one operand
  of `!llvm.linker.options` (a list of strings), `add_dependent_library(
  name)` one of `!llvm.dependent-libraries`; `linker_options()` and
  `dependent_libraries()` return them in order (`StringLists`). Both
  change the object file (§7).
- **What a producer could not represent.** `record_unsupported(construct,
  symbol)` records that the input had something `hod_ir` has no field
  for, under the stable name `classify` (§8) will report; `unsupported()`
  lists the records. This is how a rejected attribute (`attr:<name>`,
  §5.7) and the constructs of §2.2 "Not representable" reach `classify`.
  A producer never drops such a construct silently and never stops at it:
  it records and goes on, so that one pass reports everything.

### 2.1 Target

`Module::new(TargetSpec)`. A `TargetSpec` is an architecture and OS/ABI
(`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`) plus the parsed
data layout: pointer size and alignment, integer and float alignments,
aggregate alignment, stack alignment, endianness. A producer reading LLVM
text checks the module's `target datalayout` string against the spec's and
reports `IrErrorKind::DataLayoutMismatch` on a difference that changes
layout.

**Module properties** are the public field `module.props: ModuleProps`
(40 bytes). A producer that has no LLVM module flags (the bridge) assigns
the fields; one that has them calls `set_module_flag` for each.

| field | type | LLVM module flag | value |
|---|---|---|---|
| `pic` | `PicLevel` (`None`, `Small`, `Big`) | `PIC Level` | 0 to 2 |
| `pie` | `PicLevel` | `PIE Level` | 0 to 2 |
| `code_model` | `Option<CodeModel>` (`Tiny`, `Small`, `Kernel`, `Medium`, `Large`) | `Code Model` | 0 to 4 |
| `large_data_threshold` | `Option<u64>` | `Large Data Threshold` | any |
| `uwtable` | `UwTable` (`None`, `Sync`, `Async`) | `uwtable` | 0 to 2 |
| `frame_pointer` | `FramePointer` (`None`, `NonLeaf`, `All`, `Reserved`, `NonLeafNoReserve`) | `frame-pointer` | 0 to 4 |
| `branch_protection` | `BranchProtection`, a set of the bits below | | |
| — `BTI` | | `branch-target-enforcement` | on if not 0 |
| — `GCS` | | `guarded-control-stack` | on if not 0 |
| — `PAC_RET` | | `sign-return-address` | on if not 0 |
| — `PAC_RET_ALL` | | `sign-return-address-all` | on if not 0 |
| — `PAC_B_KEY` | | `sign-return-address-with-bkey` | on if not 0 |
| — `PAC_PAUTH_LR` | | `branch-protection-pauth-lr` | on if not 0 |
| — `CET_IBT` | | `cf-protection-branch` | on if not 0 |
| — `CET_SHSTK` | | `cf-protection-return` | on if not 0 |
| `dwarf_version` | `Option<u16>` | `Dwarf Version` | fits `u16` |
| `debug_info_version` | `Option<u32>` | `Debug Info Version` | fits `u32` |

The module properties are defaults and facts about the module as a whole
(what the linker is told, what code hod synthesizes itself is built
with). The policy of one function is in that function's attributes
(§5.7); a producer copies nothing from one to the other.

`set_module_flag(behavior, name, value) -> IrResult<()>` takes one entry
of `!llvm.module.flags`: LLVM's behavior number, the name, and
`FlagValue::Int(u64)`, `Str(bytes)` or `Other` (any other metadata).

- The sixteen names in the table are the **known** flags
  (`KNOWN_MODULE_FLAGS`). A known flag sets its field and is not kept.
  Its value must be an integer in the range given; anything else is
  `InvalidModuleFlag`. (LLVM's verifier is laxer here; hod cannot map a
  value it has no meaning for.)
- Any other flag is **kept by name**, with its behavior and value, and
  listed by `other_flags()`. Each is marked `ignorable` if its name is on
  the **ignorable** list (`IGNORABLE_MODULE_FLAGS`): `wchar_size` and
  `min_enum_size` (they go into ARM EABI build attributes only),
  `probe-stack` and `stack-probe-size` (defaults for functions LLVM itself
  synthesizes; every function a producer delivers has the attribute).
  `classify` refuses a module with a kept flag that is not ignorable, as
  `module-flag:<name>`. `module_flag_class(name)` gives the class of a
  name.
- A flag with behavior `Require` (3) is a statement about another flag.
  It is always kept, never ignorable, and never sets a property; hod does
  not evaluate requirements.
- Errors (`InvalidModuleFlag`), after which nothing has changed: a
  behavior outside 1 to 8; a name set twice, not counting `Require`
  entries; `Max` or `Min` behavior with a value that is not an integer; a
  known flag with a bad value.
- The behavior of a known flag is otherwise not looked at: behaviors say
  how to merge modules, and hod gets one module.

The known and ignorable lists were built from what `clang -O0 -g -S
-emit-llvm` (19 and 22) writes for C and C++ on both targets with no
further option and with each of `-fPIC`, `-fPIE`,
`-mbranch-protection=standard`, `-fcf-protection`,
`-fno-omit-frame-pointer`, `-funwind-tables`, `-gdwarf-4`: `Debug Info
Version`, `Dwarf Version`, `PIC Level`, `PIE Level`, `uwtable`,
`frame-pointer`, `wchar_size`, and with the protection options
`branch-target-enforcement`, `guarded-control-stack`,
`sign-return-address`, `sign-return-address-all`,
`sign-return-address-with-bkey`, `cf-protection-branch`,
`cf-protection-return`. `Code Model` and `Large Data Threshold` come
with `-mcmodel=`, `branch-protection-pauth-lr` with
`-mbranch-protection=pac-ret+pc`, the two stack-probe flags with
`-fstack-clash-protection`; `min_enum_size` is written for 32-bit ARM
only and is listed because it is `wchar_size`'s twin. A flag clang writes
only for other options (`RtLibUseGOT`, `direct-access-external-data`,
`SemanticInterposition`, `kcfi`, `override-stack-alignment`,
`stack-protector-guard`, ...) is unknown until a work package gives it a
meaning. `tests/clang_survey.rs` runs the survey for the options named
first.

Two things a producer of LLVM IR must know:

- LLVM upgrades `branch-target-enforcement`, `sign-return-address` and the
  rest when their value is 1 (clang 19 and earlier wrote 1; clang 22
  writes 2): it copies them onto every function as string attributes.
  A producer that reads IR without LLVM's reader applies that upgrade
  itself, because hod reads a function's protection from the function.
- `uwtable` and `frame-pointer` values that LLVM's verifier accepts and
  no LLVM version defines (3, 5, ...) are errors here.

### 2.2 Symbols

One `SymId` space, because code refers to all of them the same way: by
address. A symbol entry is 48 bytes; names are bytes in one pool,
interned.

**Names.** `symbol_named(name) -> IrResult<SymId>` returns the symbol of
that name, creating it if the module has none: one name is one symbol,
whatever it turns out to be. A name is not empty and has no zero byte
(`InvalidSymbol`). `anonymous_symbol()` creates a symbol with no name
(LLVM's `@0`); it can only become an `internal` or `private` symbol.
`find_symbol(name) -> Option<SymId>` looks a name up without creating it.

**States.** A symbol is in one of three states:

- *unresolved*: it has an id, which constants and instructions may use,
  and nothing else. This is what `symbol_named` and `anonymous_symbol`
  create, and how a producer refers to a symbol before it has seen its
  declaration.
- *declaration*: a variable without an initializer or a function for
  which no body will be delivered.
- *definition*: a variable with an initializer, a function with a body,
  an alias, an IFUNC.

| operation | on | result |
|---|---|---|
| `set_variable`, `set_function`, `set_alias`, `set_ifunc` `(sym, &desc)` | an unresolved symbol or a declaration of any kind | the symbol is what `desc` says, entirely; only the id, the name and the retained marks stay |
| the same | a definition | `SymbolRedefined` |
| `set_initializer(sym, init)` | a declared variable | a definition (`InvalidSymbol` if not a variable, `SymbolRedefined` if defined) |
| `set_has_body(sym)` | a declared function | a definition (likewise) |
| `discard_definition(sym)` | a defined variable or function | an `external` declaration: no initializer or body, no COMDAT, no personality, no subprogram (LLVM's `convertToDeclaration`) |
| the same | an alias or IFUNC | unresolved |
| the same | a declaration, an unresolved symbol | nothing |
| `retain(sym, Retained::Used \| CompilerUsed)` | any named symbol | the mark is set; marks are never cleared and survive every operation above |

`add_variable`, `add_function`, `add_alias`, `add_ifunc` `(name, &desc)
-> IrResult<SymId>` are `symbol_named` followed by the `set_*` method.
If the description is refused the name stays, unresolved.

The rules this fixes, where LLVM's text format has no equivalent because
a name is written once there:

1. **Declaring again is allowed, and the last description wins.**
   Descriptions are not merged. A producer that learns more about a
   symbol (the bridge meets a callee before its attributes) describes it
   again in full.
2. **A declaration may be described again as another kind.** A function
   declared at its first call may turn out to be an alias or an IFUNC.
   Nothing that refers to a symbol depends on its kind or type: pointers
   are opaque, and a call carries its own function type.
3. **Defining twice is an error** (`SymbolRedefined`), also when the two
   definitions are equal. `discard_definition` is the one way back.
4. **A symbol that is still unresolved when the module is finished is an
   error** (`UndefinedSymbol`): the producer referred to a name and never
   said what it is.
5. **A refused operation changes nothing.**

**Common fields** (every kind; `Symbol` has an accessor for each):
`linkage` (`Linkage`, the eleven LLVM linkages), `visibility`,
`dso_local`, `unnamed_addr` (`None`, `Local`, `Global`), and for
variables and functions `section` (bytes; empty is none), `comdat`,
`align` (a power of two up to 2^32, or none) and `addr_space`. DLL
storage classes are not represented.

`dso_local` is stored as LLVM implies it: a symbol with local linkage, or
with a visibility other than `default` unless it is `extern_weak`, is
`dso_local` whether the producer said so or not. (`tests/module_llc.rs`:
this is exactly when `llc` reaches a symbol without the GOT in PIC code,
with one target rule on top: AArch64 uses the GOT for every `extern_weak`
symbol.)

**Per kind**, as the description structs have them (`VariableDesc`,
`FunctionDesc`, `AliasDesc`, `IFuncDesc`; each has `new` with LLVM's
defaults and public fields) and as `Symbol::kind() -> SymbolKind` returns
them:

- **variable**: value type, `constant`, thread-local model (`TlsModel`),
  address space, initializer (`ConstId`; none for a declaration).
- **function**: function type, calling convention, return and parameter
  attributes (`ParamAttrs`; a list shorter than the parameter list means
  no attributes on the rest), the function attribute set (`FnAttrsId`),
  personality (`Option<SymId>`), subprogram (`Option<DiId>`, §6), address
  space, `has_body`.
- **alias**: value type, thread-local model, and the target, a pointer
  constant that is an `Addr`, an `AddrOfInt` or an `Expr` (§4.2).
  `classify` decides which targets are supported.
- **IFUNC**: value type and the resolver, a `SymId`.

Address spaces are kept (0 to `MAX_ADDR_SPACE`) so that `classify` can
refuse the non-zero ones by name; nothing else reads them.

**Checked when a symbol is described** (`InvalidSymbol` unless another
kind is named):

- An anonymous symbol has local linkage.
- A symbol with local linkage has `default` visibility.
- Alignment is a power of two up to 2^32 (`InvalidOperand`).
- A variable's value type is not `void`, `label`, `metadata`, `token`, a
  function type or scalable (`InvalidType`); an `appending` variable is an
  array; an initializer has the value type (`TypeMismatch`).
- A function's type is a function type (`InvalidType`); a function is
  not `common` or `appending`; it has no more parameter attributes than
  parameters (`InvalidOperand`).
- A definition is not `extern_weak`, and not `available_externally` in a
  COMDAT. A defined variable has a sized type (`UnsizedType`). A `common`
  variable is not constant, is in no COMDAT, and has a zero initializer.
- An alias is not `extern_weak`, `common` or `appending`; its target is a
  pointer (`TypeMismatch`) of one of the three forms above. An IFUNC is
  not `extern_weak`, `common`, `appending` or `available_externally`.

**Checked at the end** by `verify_symbols()`, which a producer calls
once after the last symbol and constant, because these depend on symbols
described later:

- No symbol is unresolved, and every `SymId` in a constant is a symbol of
  the module (`UndefinedSymbol`).
- A declaration is `external` or `extern_weak` and in no COMDAT.
- An alias's target symbol is a definition; it is
  `available_externally` exactly when the alias is; if it is itself an
  alias, that alias is not `weak` or `linkonce` (the ODR forms are
  fine); aliases do not form a cycle.
- An IFUNC's resolver is a defined function, reached directly or through
  aliases at offset zero, that is not `available_externally` and returns
  a pointer.

**Not checked**, deliberately: that parameter attributes fit their
parameter types; that a declaration has no personality (LLVM refuses one;
hod never reads it); that an earlier and a later description agree.

These rules are LLVM's wherever both have one: `tests/symbols_as.rs`
gives each case to `llvm-as` 19 and 22 as text and to the builder, and
the verdicts agree on every case except the ones the test lists by name:
the redeclaration rules 1 and 2, the declaration personality, anonymous
symbols with non-local linkage (LLVM accepts them and `llc` invents
`__unnamed_N`; hod wants the producer to name what it exports), and a
definition of an unsized type, which LLVM 19 lets through.

**Not representable.** A producer that meets one of these calls
`record_unsupported` with the name given: a DLL storage class
(`dll-storage`), prefix or prologue data (`prefix-data`,
`prologue-data`), `gc` (`gc`), `partition` (`partition`), a variable's
own `code_model` (`global-code-model`), `sanitize_memtag` on a variable
(`global-sanitize-memtag`), a personality that is not a symbol
(`personality-not-a-symbol`), a rejected attribute (`attr:<name>`), and
`elementtype` beside another typed attribute (§5.7,
`attr:elementtype-with-<other>`). `externally_initialized` and the
`no_sanitize_*` marks of a variable are dropped: the first does not
change where `llc` puts the variable, the others are read by
instrumentation passes only.

Metadata attached to a symbol, other than `!dbg` (§6): `!type`,
`!vcall_visibility`, `!prof`, `!section_prefix` and `!annotation` are
dropped (they are read by link-time or profile-guided passes, or only
move a function between sections). Any other kind is recorded as
`global-metadata:<kind>`: `!associated`, `!absolute_symbol` and
`!kcfi_type` change the object, and a kind hod does not know may.

**The special LLVM arrays are not symbols.** `llvm.global_ctors` and
`llvm.global_dtors` become `add_ctor` / `add_dtor(priority, function,
associated)` and are read as `ctors()` / `dtors() -> &[Structor {
priority: u16, func, associated: Option<SymId> }]`, in the order added.
The priority is LLVM's `i32` read as unsigned and clamped to 65535, as
`llc` does (`tests/module_llc.rs`): 65535 is the plain `.init_array`
section, any lower `N` is `.init_array.N`. An entry whose function is
`null` is skipped by the producer. `llvm.used` and `llvm.compiler.used`
become `retain`.

Symbols can be created at any time before they are referred to, so a
producer that discovers callees while lowering a body creates them as it
goes.

Cost (`tests/symbol_perf.rs`, a printed measurement, not a gate): for
100,000 symbols with names of 115 bytes on average, an optimized build
takes about 0.2 µs to create a symbol by name, 0.03 µs to describe it
and 0.07 µs to find it again.

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

LLVM attributes fall in three classes. The implementation has them as one
table, `ATTRIBUTES: [AttrInfo { name, syntax, class, prefix }]`, that
`hod_ll` and `classify` both read through `attr_class(syntax, name) ->
AttrClass` (`Kept`, `Dropped`, `Rejected`) and `attr_info`. `syntax` is
`AttrSyntax::Keyword` for an attribute written bare (`nounwind`,
`align 8`, `memory(none)`) and `String` for one written in quotes
(`"target-cpu"="x"`); the two are separate name spaces. A name the table
does not have is `Rejected`. The lists below are the table, in full.

**Kept**, because they change the code.

- On parameters and returns (`ParamAttrs`): `zeroext`, `signext`,
  `inreg`, `nest`, `swiftself`, `swifterror`, `swiftasync`, `align`,
  `alignstack`, and the typed ones `sret`, `byval`, `byref`, `inalloca`,
  `preallocated`, `elementtype` (inline assembly).
- On functions, as keywords (`FnAttrs` flags): `noreturn`, `naked`,
  `nounwind`, `returns_twice`, `alwaysinline` (it decides what happens to
  an `available_externally` body: design 0001, point 6), `ssp`, `sspreq`,
  `sspstrong`, `presplitcoroutine`, and the sanitizers
  `sanitize_address`, `sanitize_alloc_token`, `sanitize_hwaddress`,
  `sanitize_memory`, `sanitize_memtag`, `sanitize_numerical_stability`,
  `sanitize_realtime`, `sanitize_realtime_blocking`, `sanitize_thread`,
  `sanitize_type`; with a value: `uwtable` (`UwTable`: `sync` or `async`)
  and `alignstack(N)`.
- On functions, as strings (`FnStr`, 21 keys): `target-cpu`,
  `target-features`, `tune-cpu`, `frame-pointer`,
  `patchable-function-entry`, `patchable-function-prefix`,
  `function-instrument`, `branch-target-enforcement`,
  `sign-return-address`, `sign-return-address-key`,
  `branch-protection-pauth-lr`, `guarded-control-stack`,
  `stack-probe-size`, `probe-stack`, `split-stack`,
  `zero-call-used-regs`, `stack-protector-buffer-size`,
  `min-legal-vector-width`, `no-jump-tables`, `no-builtins`, and every
  name that starts with `no-builtin-` (the one prefix entry of the
  table).
- On calls: `noreturn`, `nounwind`, `returns_twice`, the tail kind and
  the calling convention (the call instruction's own fields, §5.4).

  Five of the strings were not in the first version of this list:
  `branch-protection-pauth-lr` and `guarded-control-stack` are the two
  missing members of the `sign-return-address` family;
  `stack-protector-buffer-size` decides which functions `ssp` protects
  and is on every function clang emits; `min-legal-vector-width` bounds
  the vector registers an x86 function may use and is on every x86
  function clang emits; `no-jump-tables` forbids a code shape.

**Dropped**, because only an optimizer (or a pass that runs before any
backend) reads them:

- on values: `noundef`, `nonnull`, `dereferenceable`,
  `dereferenceable_or_null`, `noalias`, `nocapture`, `captures`,
  `readnone`, `readonly`, `writeonly`, `nofpclass`, `range`, `writable`,
  `dead_on_unwind`, `dead_on_return`, `initializes`, `returned`,
  `immarg`, `allocalign`, `allocptr`, `noext`;
- on functions and calls: `memory`, `nofree`, `nosync`, `willreturn`,
  `mustprogress`, `noinline`, `optnone`, `minsize`, `optsize`, `cold`,
  `hot`, `speculatable`, `allocsize`, `allockind`, `nocallback`,
  `nomerge`, `nobuiltin`, `builtin`, `norecurse`, `inlinehint`,
  `null_pointer_is_valid`, `nocreateundeforpoison`, `vscale_range`,
  `disable_sanitizer_instrumentation`, `nosanitize_bounds`,
  `nosanitize_coverage`, `noprofile`, `skipprofile`;
- the fast-math strings `approx-func-fp-math`, `less-precise-fpmad`,
  `no-infs-fp-math`, `no-nans-fp-math`, `no-signed-zeros-fp-math`,
  `unsafe-fp-math`, and `no-trapping-math`.

  Fifteen of these were added when the table was built against LLVM's
  attribute list and clang's output, each because clang emits it in an
  ordinary build or because it is the twin of a name already here:

  | attribute | why only an optimizer reads it |
  |---|---|
  | `builtin` | on a call, undoes `nobuiltin` (dropped) for library-call recognition; on every `operator new` and `delete` call clang emits |
  | `norecurse` | an inferred fact for interprocedural optimization; clang puts it on `main` |
  | `inlinehint` | an inliner threshold |
  | `allocalign`, `allocptr` | parameter roles in the optimizer's model of allocation functions, like `allocsize` and `allockind` |
  | `noext` | says that neither `zeroext` nor `signext` applies, which their absence already says; read by a verification mode of targets that demand one of the three |
  | `null_pointer_is_valid` | withdraws an assumption that only optimizations make |
  | `nocreateundeforpoison` | a fact about an intrinsic for poison analysis; on intrinsic declarations in clang 22 output |
  | `vscale_range` | bounds on `vscale` for the optimizer; code must be right for every `vscale`; on every function clang emits with `+sve` |
  | `disable_sanitizer_instrumentation`, `nosanitize_bounds`, `nosanitize_coverage` | read by the sanitizer instrumentation passes, which run before a backend; clang emits the first without any sanitizer enabled |
  | `noprofile`, `skipprofile` | read by the profile instrumentation passes |
  | `"no-trapping-math"` | on every function clang emits; without `strictfp` (rejected) the default floating-point environment is assumed either way |

**Rejected**: anything else. An attribute that is in neither list is a
`classify` failure naming the attribute (`attr:<name>`, which the
producer records with `record_unsupported`, §2). It is never silently
dropped. Of LLVM 19's and 22's attribute names these are: `convergent`,
`coro_elide_safe`, `coro_only_destroy_when_complete`,
`fn_ret_thunk_extern`, `hybrid_patchable`, `jumptable`, `nocf_check`,
`nodivergencesource`, `noduplicate`, `noimplicitfloat`, `nonlazybind`,
`noredzone`, `optdebug`, `optforfuzzing`, `safestack`, `shadowcallstack`,
`speculative_load_hardening`, `strictfp`, and the strings
`allow_direct_access_in_hot_patch_function`, `denormal-fp-math`,
`denormal-fp-math-f32`, `loader-replaceable`,
`marked_for_windows_hot_patching`, `no-inline-line-tables`,
`profile-sample-accurate`, `use-sample-profile`. Some of them change
code (`noredzone`, `noimplicitfloat`, `nonlazybind`, `nocf_check`,
`shadowcallstack`, ...) and move to "kept" with the work package that
implements them; some look optimizer-only (`nodivergencesource`,
`noduplicate`, `optdebug`, `optforfuzzing`, the profile strings) and
stay rejected until someone shows an input that has them. A string
attribute that LLVM's attribute list does not define (LLVM accepts any
string) is rejected like any other unknown name.

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
- `FnAttrs` is the kept attribute set of one function, as a producer
  fills it in: `set(flags)` with the flag constants (`FnAttrs::NORETURN`,
  ...; `flag_named(keyword)` maps LLVM's keyword to its flag),
  `set_uwtable(UwTable)`, `set_stack_align(bytes)`, `set_string(FnStr,
  value)`. String values are kept as LLVM's bytes; each target parses the
  ones it reads. `FnStr::from_name(name)` maps an attribute name to its
  key; for `no-builtin-<function>` the key is `FnStr::NoBuiltin`, the
  value is the function's name, and a set may hold several.
  `clear()` empties a set for reuse without freeing its storage.
- `Module::intern_fn_attrs(&FnAttrs) -> IrResult<FnAttrsId>` interns a
  set: equal sets are one id whatever order they were filled in, and
  `FnAttrsId::EMPTY` is the empty set in every module. A translation unit
  has a handful of distinct sets, so a function symbol carries four
  bytes, and a target parses `target-features` once per set, not once per
  function. `Module::fn_attrs(id) -> FnAttrsView` reads a set back with
  the accessors `FnAttrs` has (`has`, `flags`, `uwtable`, `stack_align`,
  `string`, `strings`). A set's fixed part is 12 bytes.
- The parameter attributes of a function symbol are interned as a list
  per distinct signature (`Symbol::kind()` returns the slice).

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

`!llvm.linker.options` and `!llvm.dependent-libraries` are kept: they
change the object file (§2). So is every module flag (§2.1).

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
- Layout: §3's grid against `llc` and `opt`, both targets, LLVM 19 and 22.
- Constant folding: §4.2's reductions against the assembly `llc` writes
  for the same expressions as initializers (`symbol + addend` has no
  folded form in IR text that `opt` could print).
- Initializer images: §4.3 against `llc`'s `.data` bytes and relocations.
- Attribute table: every attribute name in LLVM 19's and 22's
  `Attributes.td` list is in exactly one class, the class this spec
  gives it (the two lists are checked in as test data with their source
  version, `tests/data/attributes-llvm{19,22}.txt`, and checked against
  what `llvm-as` 19 and 22 parse).
- Symbols and module flags: §2.2's and §2.1's rules against `llvm-as` 19
  and 22, case by case, with the differences listed by name; implied
  `dso_local` and constructor priorities against `llc`.
- What clang emits: no module flag or attribute in clang 19 and 22 output
  for §2.1's option list is unknown or rejected.
- Printer: §9's guarantees over the IR corpus.
