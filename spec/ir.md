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
  entries; `Max` or `Min` behavior with a value that is not an integer;
  `Require` (3), `Append` (5) or `AppendUnique` (6) with a value that is
  not a metadata node (`FlagValue::Other`), and `wchar_size` or
  `SemanticInterposition` (other than as a `Require`) with one that is
  not an integer, as LLVM's verifier refuses (`tests/verify_llvm.rs`);
  a known flag with a bad value.
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

**Order.** `SymId`s are given in creation order, and `symbols()` walks
them so. The producers create every symbol first and in LLVM's module
order: the variables, then the functions, then the aliases, then the
IFUNCs, each kind in the order its module lists them (the text's order
of definitions and declarations, which is the bitcode's record order).
That is the order `llc` lays the sections' contents out in, and the
order in which one module read from its text and from its bitcode
delivers the same ids (`ll.md` §5.7, `bc.md` §5.5).

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

Parameter and return attributes fit their types and positions, as §5.7
says (`InvalidAttribute`, from `add_function` and `set_function`).

**Not checked**, deliberately: that a declaration has no personality (LLVM refuses one;
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

### 2.3 Assembly text that needs no assembler

hod has no assembler. Two kinds of assembly text do nothing an assembler
would have to do, and `hod_ir` (`src/asm_text.rs`, WP-075) reads them so
that a target need not:

- **Module-level assembly of symbol directives.** `read_module_asm(arch,
  text, &mut Vec<AsmDirectives>)` reads statements separated by newlines
  and `;`, with comments (`/* */` at the start of a statement; `//`
  anywhere and `#` at the start of a statement on AArch64, `#` anywhere on
  x86-64) and blank statements. A statement is one of `.globl`/`.global`,
  `.weak`, `.hidden`, `.protected` (each with a comma list of names) or
  `.type name, @function|%function|STT_FUNC|@object|%object|STT_OBJECT|
  @notype|%notype|STT_NOTYPE`. A name is plain (`[A-Za-z_.$][A-Za-z0-9_.$]*`)
  and not an assembler-local `.L` name. The result has one entry per name,
  in the order of first appearance, with its binding, visibility (the last
  directive wins) and type. Everything else is an `AsmRefusal { what,
  statement }`: a label, an instruction, an assignment (`.set`, `=`), any
  other directive (`.size`, `.symver`, `.internal`, section directives
  among them: none was seen in real input with an effect `llc` writes
  without assembling), a quoted name, an unterminated comment, and two
  directives that disagree (`.globl` and `.weak` of one name, two types).
- `module_asm_effects(module, &mut Vec<AsmSymbolEffect>)` reads the
  module's text and checks each name against the module, giving what the
  object must change; it refuses (`what = "binding" | "type" | "symbol"`)
  what `llc` warns about, fails on, or settles by its assembler's
  precedence:

  | directive | name the module does not have | declaration | definition |
  |---|---|---|---|
  | `.globl` | a new undefined global symbol | nothing; `extern_weak` refused (`llc` warns) | nothing; weak or `linkonce` refused (`llc` warns) |
  | `.weak` | a new undefined weak symbol | weak | nothing if weak or `linkonce`; refused if strong (`llc` fails) |
  | `.hidden`, `.protected` | the visibility, on a new undefined global symbol | the visibility, unless the module gives one other than default (it wins) | likewise |
  | `.type` | the type, on a new undefined global symbol | the type, but on a thread-local variable (refused) | nothing if it is the definition's own (function, or object for a variable); otherwise refused |

  A symbol of `internal`, `private`, `appending` or
  `available_externally` linkage, an unresolved symbol, and a binding
  directive on a `common` variable are refused. A declaration the text
  names is kept in the symbol table even if nothing refers to it, as
  `llc` keeps it.
- **Empty inline-assembly templates.** `asm_template_is_empty(arch,
  template)` is true for a template with no `$` (no operand, no `$$`)
  whose statements are all blank or comments. Such a call is an
  optimization barrier: its effect is its constraints' (a target's,
  `spec/codegen.md` §5.5.1 "Assembly that needs no assembler").

`classify` (§8) reports module assembly that `module_asm_effects` accepts
as `module:asm-symbols` and any other as `module:asm`; a call of inline
assembly with an empty template as `call:inline-asm-empty` and any other
as `call:inline-asm`. An `elementtype` on an argument of inline
assembly is the operand's type, which the target reads, and has no row
of its own there; elsewhere it is `attr:elementtype`.

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
- `BlockAddr { function, block }`. The pool keeps a list of them as
  they are interned, which is the address-taken mark on blocks (WP-064):
  `ConstPool::block_addresses()` in id order, `is_address_taken(func,
  block)`, `has_address_taken_block(func)`. A block is address-taken if
  any `blockaddress` of it was interned, used or not.
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
- Two expressions LLVM can emit are deliberately left as `Expr`: a
  number minus an address (`sub (i64 4, ptrtoint @a)`), and an address
  in an integer whose width is not 8, 16, 32 or 64 bits. The second is
  declined by `classify` (its `const-expr:<op>`); neither has been seen
  from a C++ front end.
- A number minus an address stays an `Expr`, and
  `ConstPool::neg_addr(types, c)` recognizes one (WP-101): an `i64`
  expression of only numbers, `add`, `sub` and one `ptrtoint` to `i64`
  of an `Addr`, in which the address is subtracted, returning `NegAddr {
  sym, addend }` whose value is `addend - sym` modulo 2^64 (the
  `Addr`'s own offset is folded into `addend`). `classify` names it
  `operand:neg-addr` as an operand on its own, and `init:neg-addr` in
  an initializer or inside an aggregate operand, whose image has no
  relocation for it (`llc`: "Cannot represent a difference across
  sections"); a thread-local symbol is `tls:address-in-constant`. Any
  other shape (two addresses subtracted, another width) keeps
  `const-expr:<op>`.
- `fold_cast` also folds a `bitcast` of a fixed vector of at most 128
  bits whose elements are all numbers to an integer or float: the
  vector's image (§4.3), as `llc` folds it (WP-095).
- A label difference stays an `Expr` (`trunc (sub (ptrtoint
  blockaddress(@f, %a)), (ptrtoint blockaddress(@f, %b)))`): its value is
  known only once the function is laid out. `ConstPool::block_diff`
  recognizes one: an integer expression of only `ptrtoint` of a
  `blockaddress`, numbers, `add`, `sub` and `trunc` that adds one block
  and subtracts another of the same function, returning `BlockDiff {
  func, a, b, addend }`. Arithmetic modulo 2^n commutes with truncation,
  so its value is the low bits of `a - b + addend` (WP-095).

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
  `Diff { a, b }` and `PltDiff { a, b }` for `SymDiff`; `BlockDiff {
  func, a, b }` for an `Expr` that is a label difference (§4.2, WP-095),
  whose addend is the `BlockDiff`'s (a number once the function is laid
  out: the code generator writes it, no relocation). The field's value
  is target + addend, truncated to `size` bytes (8 for a pointer; 1, 2, 4
  or 8 for an integer).
- A `Diff { a, b }` can be written only when `a` and `b` are defined in
  one section, or `b` is defined in the section that holds the field.
  `flatten` cannot know sections; whoever turns fixups into relocations
  checks this and reports the variable as unsupported otherwise.
- No image (`UnsupportedInitializer`): an `Expr` other than a label
  difference, the `none` token, an
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

**Reading bodies in parallel: a seed and forks** (WP-063). A
reader adds to the module the types, constants, inline-assembly callees,
function attribute sets, debug expressions and locations a body uses.
To read bodies on several threads without a copy of the module per
thread:

- The layer is a type parameter of the tables and of `Module`
  (`Layering`, sealed): `NoLayer`, zero-sized, is the default and the
  only kind a producer or the one-thread reader sees (`Module` is
  `Module<NoLayer>`), so with one thread the table code is plain vectors
  and hash tables, with no base pointer and no branch on one. `Layered`
  is for forks only. The body readers are generic over it and are
  instantiated twice; the `Layered` instantiation runs only when bodies
  are read on more than one thread.
- `Module::seed` copies those tables once, into a `Module<Layered>` whose
  tables are a shared base (an `Arc`) with an empty layer each, ids
  counted on from the base's end; the module itself is not changed and
  stays a `Module<NoLayer>`. The symbol, COMDAT, flag, constructor and
  linker tables are always shared, behind one `Arc`.
- `seed.fork()` shares every table of the seed and copies only the
  properties and a few words of debug counts. A lookup in a fork probes
  the base, then its own layer: what the module has, the fork finds,
  with the module's id. What it adds goes to its layer, with ids after
  the base's.
- `fork.mark()` and `fork.delta(mark)` give what a body added, owned; the
  delta is `None` if the fork changed what it cannot hand over (a symbol,
  a COMDAT, a property or flag, a debug node, a location in place).
  `module.adopt(&delta, &mut remap)` interns the delta's entries into the
  module in the fork's order, translating each id, and extends the
  `Remap` from fork ids to module ids; `Body::remap` translates a body
  read in the fork. `Module::stamp` changes when the module changes what
  forks read and do not add to.
- Interning is by content, so adopting the deltas of bodies in file
  order makes every new entry, and so every id, what reading those
  bodies in order against the module makes.

`hod_ir::read_bodies` is the driver over any `BodySource` (`hod_ll`,
`hod_bc`): the calling thread and up to `threads - 1` scoped workers
(no global pool) take runs of consecutive bodies of about
`CHUNK_WEIGHT` (16 KiB of text, by `BodySource::weight`; smaller when
that would not give each thread four runs) in order from a shared
counter, each against its own fork, at most four runs per thread ahead
of the last delivered. The calling thread adopts and delivers body by
body in file order, and reads runs itself while the next is not ready.
A lock is taken once per run, and a thread is woken only when it waits
(the calling thread for the next run, the others for room ahead). A body whose read fails, or whose
delta is `None`, is read again by the calling thread against the module
itself in its turn, and the worker that met it starts a new fork; if
that read changed the module's stamp, the forks are stale and the rest
is read sequentially. So the bodies delivered, the module's tables and
ids, the unsupported record and the error (the first in file order) are
the sequential reading's at any thread count, and one thread is the
sequential reading. `ReadStats` says how many bodies were read again and
whether the reading fell back.

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
whole function: `SlotVar { slot, var: DiId, loc: LocId, expr: ExprId }`,
recorded by `FunctionBuilder::dbg_declare` for a `#dbg_declare` of a
static `alloca` (§6.6), with the declaration's `DIExpression`
(`ExprId::EMPTY` when the variable is simply at the slot's address;
`set_slot_var(slot, var, loc)` records that case). It is a separate list,
in recording order, so that a body without debug information pays nothing
and several variables can share a slot: the variables of an inlined call
and of its caller, or the pieces of one variable, each with a fragment
expression.

Two facts about slots and their lifetimes are kept for the coroutine plan
(`spec/xform.md` §3); neither is code, and neither is a use:

- `Slot::outside_frame()`: the `alloca` carried `!coro.outside.frame`
  (set with `FunctionBuilder::set_slot_outside_frame`).
- `Body::lifetime_markers()`: one `LifetimeMarker { before, ptr, start }`
  per `llvm.lifetime.start` (`start`) or `llvm.lifetime.end` call, which
  is still not built (§5.5): `ptr` is its pointer operand and `before`
  the instruction appended after it. `FunctionBuilder::lifetime_marker`
  records one; `lifetime_intrinsic(name)` tells a producer which names
  these are. The verifier checks that each stands before a non-`phi`
  instruction, in position order, with a valid pointer.

### 5.3 Values

A `Value` is 4 bytes: a 3-bit tag and a 29-bit index. Tags: instruction
result, argument, constant, symbol address, stack slot address, inline
assembly (callee position only), metadata (an argument of an intrinsic
call only, below), and "none" (`Value::NONE`: the value of `ret void`,
the callee of an intrinsic call). `Value::kind()` decodes it to a
`ValueKind`. Blocks are `BlockId`s and appear only in terminators,
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

**Metadata operands** (WP-064). `FunctionBuilder::metadata(node,
strings)` adds a metadata operand to the body and returns its `Value`
(`ValueKind::Meta(MetaId)`, type `metadata`): a string `!"s"` (`node`
false, exactly one string) or a node of strings `!{!"a", ...}` (`node`
true, any number, the empty node included). Nothing else LLVM writes as
metadata (numbers, values, nested or `distinct` nodes, debug nodes) has a
form; a producer refuses it as `metadata-arg:<callee>`. The operand
belongs to the body (`Body::metadata(id)` reads it back: `is_node`,
`strings`, `single_node_string`), so bodies stay independent of each
other and of the module. It is allowed only as an argument of a call
whose callee is an intrinsic, in a parameter of type `metadata` (the
builder checks the first, `verify` both, §5.8); the producers make one
only for an intrinsic on hod's list (§5.5).

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
| other | `Phi`, `Select`, `Freeze`, `VaArg` | `VaArg` exists so `classify` can reject it (`inst:va_arg`): `llc` 19 and 22 accept the instruction for AArch64 Linux and miscompile it, so there is nothing to agree with. C and C++ front ends expand `va_arg` themselves |
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
`nounwind`, `returns_twice`, `convergent`, the `ParamAttrs` of the return
value and of each argument (§5.7; stored only for calls that have any), and operand
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

The list (354; each with every overload LLVM allows):

- memory: `memcpy memcpy.inline memmove memset memset.inline`
- variable arguments and stack: `va_start va_end va_copy stacksave
  stackrestore frameaddress returnaddress`
- traps: `trap debugtrap ubsantrap`
- bits: `bswap bitreverse ctpop ctlz cttz fshl fshr abs smax smin umax
  umin scmp ucmp clmul`
- checked and saturating arithmetic: `{s,u}{add,sub,mul}.with.overflow`,
  `{s,u}{add,sub,shl}.sat`
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
- AArch64 (WP-084, WP-083; the variants are `Aarch64` and the name's
  parts in camel case): `aarch64.crc32{b,h,w,x,cb,ch,cw,cx}`,
  `aarch64.{cls,cls64,dmb,dsb,isb,hint}`, the `aarch64.crypto.*` of AES,
  SHA1, SHA256, SHA512 and SHA3, and the `aarch64.neon.*` and
  `aarch64.sisd.*` that clang 22 or 19 emits for `<arm_neon.h>` (178 of
  them; `bfcvt`, `bfcvtn` and `bfcvtn2` are LLVM 19's, which LLVM 22
  upgrades to other IR on reading) but the SM3/SM4, FP8, `famax`/`famin`
  and `vluti` ones.
- named registers (WP-064): `read_register read_volatile_register
  write_register`, whose first argument is a metadata operand (§5.3)
  naming the register. Which names a target reads or writes is the
  target's to judge. The list in
  `crates/hod_ir/src/intrinsic.rs` is the reference.

Not on the list, and therefore `Other`: the other target-specific
intrinsics (`llvm.aarch64.*` not above, `llvm.x86.*`), the other
intrinsics with metadata arguments (the constrained floating-point family,
whose producers refuse the call as `metadata-arg:<callee>`: no input in
the corpus has one), the masked
vector family (its signatures differ between LLVM 19 and 22), and anything
no frontend in scope emits at `-O0`.

**The no-code rule.** Intrinsics that generate no code have no
representation. `no_code_intrinsic(name)` is the one place that says which
and what a producer does with the call:

| names | rule |
|---|---|
| `llvm.lifetime.start/end`, `llvm.invariant.start/end`, `llvm.assume`, `llvm.donothing`, `llvm.var.annotation`, `llvm.experimental.noalias.scope.decl` | `NoCode::Drop`: omit the call. (The `{}*` result of `invariant.start` can only be used by `invariant.end`.) A lifetime call is recorded as a lifetime marker (§5.2). |
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
  an `available_externally` body and which bodies decline a pre-opt
  module: design 0001, point 6, `spec/cli.md` "Always-inline"), `ssp`, `sspreq`,
  `sspstrong`, `presplitcoroutine`, and the sanitizers
  `sanitize_address`, `sanitize_alloc_token`, `sanitize_hwaddress`,
  `sanitize_memory`, `sanitize_memtag`, `sanitize_numerical_stability`,
  `sanitize_realtime`, `sanitize_realtime_blocking`, `sanitize_thread`,
  `sanitize_type`, `convergent` (below), `noredzone` (below),
  `nocf_check` (below); with a
  value: `uwtable`
  (`UwTable`: `sync` or `async`) and `alignstack(N)`.
- On functions, as strings (`FnStr`, 22 keys): `target-cpu`,
  `target-features`, `tune-cpu`, `frame-pointer`,
  `patchable-function-entry`, `patchable-function-prefix`,
  `function-instrument`, `branch-target-enforcement`,
  `sign-return-address`, `sign-return-address-key`,
  `branch-protection-pauth-lr`, `guarded-control-stack`,
  `stack-probe-size`, `probe-stack`, `split-stack`,
  `zero-call-used-regs`, `stack-protector-buffer-size`,
  `min-legal-vector-width`, `no-jump-tables`, `no-builtins`, every
  name that starts with `no-builtin-` (the one prefix entry of the
  table), and `disable-tail-calls` (below).
- On calls: `noreturn`, `nounwind`, `returns_twice`, `convergent`, the
  tail kind and the calling convention (the call instruction's own
  fields, §5.4).

  Five of the strings were not in the first version of this list:
  `branch-protection-pauth-lr` and `guarded-control-stack` are the two
  missing members of the `sign-return-address` family;
  `stack-protector-buffer-size` decides which functions `ssp` protects
  and is on every function clang emits; `min-legal-vector-width` bounds
  the vector registers an x86 function may use and is on every x86
  function clang emits; `no-jump-tables` forbids a code shape.

  `convergent` (WP-086) is kept although it changes no code hod
  generates, on declarations, definitions and call sites, and `classify`
  accepts it on every target (`attr:convergent`, row `ON`). The LLVM
  Language Reference ("Function Attributes", `convergent`) defines it as
  a restriction on *transformations*: a convergent call may not be made
  control-dependent on additional values, so an optimizer must not, for
  example, duplicate it into the arms of a branch, sink or hoist it across
  control flow, or unswitch a loop around it; "Convergent Operations"
  adds that, absent convergence control tokens, the set of threads that
  execute the call together is left to the implementation and is only
  constrained relative to the input. hod makes none of those
  transformations: code generation is one walk over each body in layout
  order that emits every call once, in its block (`spec/codegen.md` §1),
  and its only reordering, deferral (§4.6), delays a result to the
  instruction immediately after it, never past a terminator, so no call
  changes the branches it depends on, and the "converged-with" relation
  of the output is the input's, which is all the attribute asks. It is kept rather than dropped so that a
  module hod prints (§9) forbids what the input forbade, should anything
  downstream transform it. On targets without SIMT execution (AArch64,
  x86-64) it has no further meaning. What would need real support stays
  refused by name: the `convergencectrl` operand bundle
  (`bundle:convergencectrl`, LangRef "Convergence Control Tokens"), whose
  token fixes *which* dynamic instances converge, and the intrinsics
  that produce those tokens, `llvm.experimental.convergence.entry`,
  `.anchor` and `.loop` (`intrinsic:llvm.experimental.convergence.*`,
  `Intrinsic::Other`, and `type:token` for their result). Clang and kiln
  put `convergent` on every intrinsic LLVM declares convergent, notably
  `declare i1 @llvm.is.constant.*` (`__builtin_constant_p`, all over
  libc++), and on calls in CUDA, OpenCL and HIP code; no CPU front end
  emits the bundle.

  `"disable-tail-calls"` (WP-179; `"true"` or `"false"`, the value kept
  as written) is LLVM's `TargetLowering` rule that in such a function no
  call is a tail call except a `musttail` one; `"false"` is the
  default. It is not in `Attributes.td` (LLVM reads it by name), so the
  lists of `crates/hod_ir/tests/data` do not have it. clang and kiln put
  it on a function with `__attribute__((disable_tail_calls))`
  (googletest's `GTEST_NO_TAIL_CALL_`). hod makes no tail call but a
  `musttail` one in any function (`spec/codegen.md` §7.1 "Tail kinds"),
  so the code is the code without it; `classify` accepts it on every
  target (`attr:disable-tail-calls`, row `ON`).

  `noredzone` (WP-179) forbids keeping data below the stack pointer;
  clang puts it on every function with `-mno-red-zone`. hod keeps none
  there on either target (`spec/codegen.md` §5.6 "The red zone is not
  used"; on AArch64 nothing is addressed below `sp` either), so it
  changes no code; `classify` accepts it on every target
  (`attr:noredzone`, row `ON`).

  `nocf_check` (WP-184; clang's `__attribute__((nocf_check))`), a
  function flag (`FnAttrs::NO_CF_CHECK`, bit 21) and a call-site flag
  (`CallDesc::nocf_check`, `CallSite::is_nocf_check`, printed after the
  call as ` nocf_check`). On a function it drops the entry `endbr64` of
  a `cf-protection-branch` module; on an indirect call it asks for
  `notrack` (`spec/codegen.md` §5.6.25). `classify` accepts it on every
  target (`attr:nocf_check`, row `ON`); it changes no AArch64 code, as
  in `llc`.

  `"probe-stack"` and `"stack-probe-size"` (WP-186; clang's
  `-fstack-clash-protection` writes `"probe-stack"="inline-asm"` on every
  function, `-mstack-probe-size=N` the size) are read together by
  `FnAttrsView::stack_probe` (`hod_ir::StackProbe`): no `"probe-stack"`
  is `None` (a `"stack-probe-size"` alone changes nothing, as in `llc`);
  `"inline-asm"` is `Inline(P)`, `P` the size or 4096 without one; any
  other `"probe-stack"` value names a probe function to call
  (`__probestack`-style), refused as `attr:probe-stack-function`; a size
  that is not a decimal multiple of 4096 in `[4096, 2^24)` is refused as
  `attr:stack-probe-size-value` (`llc` takes others, and some of them do
  not probe: 0 on x86-64 allocates the whole frame at once). Both rows
  are `ON` (`attr:probe-stack`, `attr:stack-probe-size`); what the
  targets build is `spec/codegen.md` §5.6.27. kiln accepts
  `-fstack-clash-protection` and writes neither attribute.
  `"zero-call-used-regs"="skip"` zeroes nothing and is accepted (the
  classifier skips its row); every other value is refused as
  `attr:zero-call-used-regs` (WP-186).

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
  `unsafe-fp-math`, and `no-trapping-math`;
- `sample-profile-suffix-elision-policy`, a string LLVM defines where it
  is read (sample-profile loading), not in its attribute list; and
  `fmv-features`, likewise (WP-095).

  Fifteen of these were added when the table was built against LLVM's
  attribute list and clang's output, each because clang emits it in an
  ordinary build or because it is the twin of a name already here, and
  two later, because LLVM 22 modules carry them:

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
  | `"sample-profile-suffix-elision-policy"` | steers how the sample-profile loader matches symbol names with suffixes (`"selected"`); no effect on code generation; on functions of LLVM 22 bitcode modules (WP-029) |
  | `"fmv-features"` | the feature set of a function-multiversioning version (`target_version`/`target_clones`), read by the optimizer's static resolution of calls to versions (`getFeatureMask`); the resolver and the version names are already in the IR. `llc` 22 writes the same object with it removed or with a value (`"dotprod,sve2"`); on functions of clang 22 with `-fmv` (WP-095) |

**Rejected**: anything else. An attribute that is in neither list is a
`classify` failure naming the attribute (`attr:<name>`, which the
producer records with `record_unsupported`, §2). It is never silently
dropped. Of LLVM 19's and 22's attribute names these are:
`coro_elide_safe`, `coro_only_destroy_when_complete`,
`fn_ret_thunk_extern`, `hybrid_patchable`, `jumptable`,
`nodivergencesource`, `noduplicate`, `noimplicitfloat`, `nonlazybind`,
`optdebug`, `optforfuzzing`, `safestack`, `shadowcallstack`,
`speculative_load_hardening`, `strictfp`, and the strings
`allow_direct_access_in_hot_patch_function`, `denormal-fp-math`,
`denormal-fp-math-f32`, `loader-replaceable`,
`marked_for_windows_hot_patching`, `no-inline-line-tables`,
`profile-sample-accurate`, `use-sample-profile`. Some of them change
code (`noimplicitfloat`, `nonlazybind`,
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
  `CallConv::keyword` is LLVM 22's keyword for the number (`x86_64_sysvcc`,
  `riscv_vls_cc(128)`), `None` where LLVM has none (it prints `ccN` and
  reads `cc N`); `keyword_since` is the first major whose text has it (19
  for numbers below 112, 22 above: `riscv_vls_cc(N)`,
  `amdgpu_gfx_whole_wave`, the CHERIoT conventions); `from_keyword` is the
  inverse, for any dialect 19 to 22. The table was checked against
  `llvm-dis` 19 and 22 over every number (`hod_bc` `tests/x86_64.rs`
  `calling_convention_keywords_are_llvms`, WP-141).
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
- LLVM's rules for which attributes fit (`verifyParameterAttrs`,
  `verifyFunctionAttrs`) are checked where the attributes are given: a
  function's by `Module::add_function` and `set_function`, a call's by
  `Body::verify` (§5.8), each an `InvalidAttribute`. Per value: `zeroext`
  and `signext` on an integer; the typed attributes, `nest`, `swifterror`
  and `align` on a pointer or vector of pointers; `byval`, `byref`,
  `inalloca` and `preallocated` of a sized type; at most one of the typed
  ones but `elementtype`, `sret` or `inreg`, and `nest`; `elementtype`
  only on a call of an intrinsic or inline assembly, never on a
  function. On a return value no typed attribute, `nest`, Swift attribute
  or `alignstack`. Positions: `sret` once, on the first or second
  parameter, never on a variable argument; `nest` once; `inalloca` on the
  last parameter (on a call's variable arguments, the last argument);
  `swiftself`, `swiftasync` and `swifterror` each once.
  `FnAttrs::set` refuses `sanitize_realtime` with
  `sanitize_realtime_blocking`.

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
| operands, ids, pool ranges, side-table links and counts are in range and allowed in their position; atomic orderings (§5.4); `switch` cases; call argument counts against the function type; an inline-assembly value only as a callee; a metadata operand (§5.3) only as an argument of an intrinsic call in a `metadata` parameter, naming an operand of this body whose strings are in range and a string operand with one string | `InvalidOperand` |
| types that must agree do: operands with each other and with the result, call arguments with the function type's parameters, the result with its return type, `ret` with the function's, `resume` and landing pads, a known intrinsic's overload types with its function type | `TypeMismatch` |
| a type is of the class the instruction needs (integer, float, pointer, vector, aggregate, a valid cast, an atomic type) | `InvalidType` |
| a loaded, stored or `va_arg` type is sized | `UnsizedType` |
| an aggregate offset names a field of that type; a shuffle mask entry selects an element | `BadIndex` |
| the recorded use facts are the recomputed ones (§5.6) | `UseFactsMismatch` |
| a use is dominated by its definition (below) | `NotDominated` |
| `callbr` calls inline assembly that does not unwind (LLVM 19 refuses another callee; LLVM 22's verifier crashes on one, which the tests count as a refusal), with as many `!i` label constraints as indirect destinations; label constraints on `callbr` only; `invoke` calls no intrinsic but LLVM's invokable ones (`donothing`, the coroutine `resume`/`destroy`/`await.suspend.*`, `seh.try.*`, `seh.scope.*`, `wasm.throw`, `wasm.rethrow`, `fake.use`, `experimental.patchpoint.*`, `experimental.gc.statepoint.*`); a tail marker on `call` only; `musttail`'s conditions (below); an intrinsic's `immarg` arguments are constant integers or floats, not `undef` or `poison`; a bundle LLVM knows (`deopt`, `funclet`, `gc-transition`, `cfguardtarget`, `preallocated`, `gc-live`, `ptrauth`, `kcfi`, `clang.arc.attachedcall`, `convergencectrl`) at most once per call; `kcfi` has one `i32` constant | `InvalidCall` |
| a call's parameter and return attributes fit their types and positions, as a function's do (§5.7) | `InvalidAttribute` |

**Dominance**, as LLVM's verifier defines it. A use in a block that is
not reachable from the entry is always dominated; a definition in an
unreachable block dominates nothing reachable. Within a block, a
definition dominates the instructions after it, so an instruction never
dominates itself, and a self-use in a reachable block is refused (a
`phi` naming itself through a back edge is not a self-use: see next).
A `phi`'s operand for the edge from block `P` is used at the end of `P`:
its definition must dominate `P`'s terminator. An `invoke`'s result is
defined on its normal edge, not at the end of its block: a use is
dominated if it is the normal destination's `phi` operand for that edge,
or the normal destination dominates the use and the edge dominates the
destination (the invoke's block appears once among the destination's
predecessors and the destination dominates every other predecessor).
So the result is not available on the unwind edge, nor in a normal
destination it reaches over a critical edge. A `callbr`'s result is
defined at the end of its block, as LLVM 19 to 22 verify it. The dominator
tree is built only for a body with a use across blocks or a use before
its definition in one block (iterative Lengauer–Tarjan, then preorder
intervals: each query is two compares); the cost is in
`spec/landed/ir/WP-026.md`.

**`musttail`** (LLVM's `verifyMustTailCall`): the callee is not inline
assembly; the call and the function agree in variadic-ness, return type
and calling convention (variable arguments of the call before a
variadic function's forwarded ones are allowed, as LLVM allows them,
WP-064); the next
instruction is `ret`, or a `bitcast` of the result and then `ret`, which
returns nothing, the result (or its cast), `undef` or `poison`. With
`tailcc` or `swifttailcc` no parameter of the function or the call has
`inalloca`, `inreg`, `swifterror`, `preallocated` or `byref` and the
function is not variadic; otherwise the parameter types match (but for an
intrinsic callee) and each parameter has the same ABI attributes in the
call and the function (`sret`, `byval`, `inalloca`, `inreg`,
`alignstack`, `swiftself`, `swiftasync`, `swifterror`, `preallocated`,
`byref`, and `align` with `byval` or `byref`). The function's attributes
are its symbol's; a body whose symbol is not a function of the module is
checked as a `ccc` function without attributes.

`verify` returns the first violation. It is the definition of "well
formed" that code generation may assume. Tests run it on every body; a
producer under test runs it on every body it hands over; a release build
need not run it at all.

An inline-assembly callee names an entry of the module's table
(`InvalidOperand`). What `verify` leaves to LLVM is listed in §9.8.

### 5.9 Reading a body

Code generation reads a body through borrowed views. Every accessor is an
array lookup and none allocates.

| question | API |
|---|---|
| the function | `Body::symbol`, `fn_type`, `ret_type(&m)`, `arg_count`, `arg_types(&m)`, `arg_uses(i)` |
| blocks in layout order | `Body::blocks()` (`Block`s), `layout()` (ids), `entry()`, `block(id)`, `block_count()` |
| a block | `Block::id`, `inst_range`, `insts()`, `has_phis()`, `phis()`, `non_phis()`, `terminator()`, `successors()` |
| an instruction | `Body::inst(id)`, `insts()`; `Inst::id`, `opcode`, `ty`, `result`, `uses`, `loc`, `operands()` (the value operands as one slice, whatever the opcode), `is_terminator`, `successors()`, `call()`, and `kind()`, an `InstKind` with one variant per instruction shape and named fields |
| lists | `PtrAdd::{base, offset, indices()}`, `Phi::{values(), blocks(), incoming(), value_for(pred)}` (the first value listed for `pred`, WP-064), `Switch::{cond, default, cases()}` (its `Debug` writes the cases as typed `(constant, block)` pairs, WP-064), `InstKind::ShuffleVector { mask }`, `InstKind::IndirectBr { targets }` |
| calls | `Body::calls()`, `call_count()`; `CallSite::{inst, opcode, ty, callee, callee_value, intrinsic, overloads, fn_type, args, cc, tail, is_noreturn, is_nounwind, returns_twice, is_convergent, has_param_attrs, ret_attrs, arg_attrs(i), bundles(), normal_dest, unwind_dest, indirect_dests}` |
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

`Module::debug` is a `DebugInfo`: the module's debug nodes, expressions,
source locations and compile units. The nodes are typed: one Rust record
per kind hod writes DWARF for, with fields named after LLVM's. Everything
a debugger needs from LLVM's metadata has a field; what LLVM can write and
`hod_ir` cannot represent is named, by the field table (§6.9), as a
construct `classify` reports. The mapping is recorded in
`spec/landed/ir/WP-005.md`.

### 6.1 Tables, and what no debug information costs

`DebugInfo` follows the crate's rules: `u32` ids into append-only arenas,
strings and expressions interned, no allocation per node.

- Nodes are `DiId`s, `0..node_count()`. `DiId::NONE` is "no node" as an
  element of a node list (the `void` that starts a subroutine type's
  `types`); a single reference is `Option<DiId>`.
- Expressions are `ExprId`s, `0..expr_count()`; `ExprId::EMPTY` (id 0) is
  `!DIExpression()` and always exists.
- Locations are `LocId`s, `0..loc_count()`. They are a module table, not
  a body's: code generation keeps `LocId`s after a body is dropped, and a
  location names module nodes. A body's per-instruction location array
  (§5.9) holds `LocId`s, `LocId::NONE` for none.
- `compile_units()` lists the compile units, in the order of
  `!llvm.dbg.cu`.

A module without debug information has an empty `DebugInfo`
(`is_empty()`), which has allocated nothing, and a body without debug
records has two empty `Vec`s more than before (§6.6). Nothing else is
paid.

The tables are append-only, with two exceptions that happen before
anything reads the node: a reserved node or location is defined once, and
a forward-declared composite type is completed once (§6.3).

### 6.2 Nodes

`DiKind` names the kinds; `DiNode` is a node, one variant per kind, each a
record with public fields. A producer fills a record and hands it to the
table; a reader gets the same record back from `node(id)`, its strings
(`&[u8]`) and lists (`&[DiId]`) borrowed from the table. Stored, a node is
an 8-byte head (where its cells start, its kind, its distinctness, its
DWARF tag) and a mask of the fields that are not at their default followed
by those fields' 4-byte cells: a field at its default costs nothing.
`kind`, `tag`, `is_distinct` and `node` read a node; `subprogram_of` and
`enclosing_block_scope` walk scopes.

The kinds, the record of each, and each field's LLVM name, Rust type and
the kinds of node a reference may name (a "scope" is LLVM's `DIScope`, a
"local scope" a subprogram or a block, a "type" a basic, derived,
composite or subroutine type, or an unknown kind):

| LLVM kind | record | fields |
|---|---|---|
| `DICompileUnit` | `DiCompileUnit` | `language`: `u16`; `file`: `Option<DiId>` → file; `producer`: `&[u8]`; `isOptimized`: `bool`; `flags`: `&[u8]`; `runtimeVersion`: `u32`; `emissionKind`: `EmissionKind`; `enums`: `&[DiId]` → composite; `retainedTypes`: `&[DiId]` → type | subprogram; `globals`: `&[DiId]` → global expr; `imports`: `&[DiId]` → imported; `nameTableKind`: `NameTableKind`; `sysroot`: `&[u8]`; `sdk`: `&[u8]` |
| `DIFile` | `DiFile` | `filename`: `&[u8]`; `directory`: `&[u8]`; `checksumkind`: `ChecksumKind`; `checksum`: `&[u8]`; `source`: `&[u8]` |
| `DISubprogram` | `DiSubprogram` | `name`: `&[u8]`; `linkageName`: `&[u8]`; `scope`: `Option<DiId>` → scope; `file`: `Option<DiId>` → file; `line`: `u32`; `type`: `Option<DiId>` → subroutine; `scopeLine`: `u32`; `containingType`: `Option<DiId>` → type; `virtualIndex`: `u32`; `flags`: `DiFlags`; `spFlags`: `SpFlags`; `unit`: `Option<DiId>` → unit; `templateParams`: `&[DiId]` → template param; `declaration`: `Option<DiId>` → subprogram; `retainedNodes`: `&[DiId]`; `thrownTypes`: `&[DiId]` → type |
| `DILexicalBlock` | `DiLexicalBlock` | `scope`: `Option<DiId>` → local scope; `file`: `Option<DiId>` → file; `line`: `u32`; `column`: `u16` |
| `DILexicalBlockFile` | `DiLexicalBlockFile` | `scope`: `Option<DiId>` → local scope; `file`: `Option<DiId>` → file; `discriminator`: `u32` |
| `DINamespace` | `DiNamespace` | `name`: `&[u8]`; `scope`: `Option<DiId>` → scope; `exportSymbols`: `bool` |
| `DILocalVariable` | `DiLocalVariable` | `name`: `&[u8]`; `arg`: `u16`; `scope`: `Option<DiId>` → local scope; `file`: `Option<DiId>` → file; `line`: `u32`; `type`: `Option<DiId>` → type; `flags`: `DiFlags`; `align`: `u32` |
| `DIGlobalVariable` | `DiGlobalVariable` | `name`: `&[u8]`; `linkageName`: `&[u8]`; `scope`: `Option<DiId>` → scope; `file`: `Option<DiId>` → file; `line`: `u32`; `type`: `Option<DiId>` → type; `isLocal`: `bool`; `isDefinition`: `bool`; `templateParams`: `&[DiId]` → template param; `declaration`: `Option<DiId>` → derived; `align`: `u32` |
| `DIGlobalVariableExpression` | `DiGlobalVariableExpression` | `var`: `Option<DiId>` → global; `expr`: `ExprId` |
| `DILabel` | `DiLabel` | `scope`: `Option<DiId>` → local scope; `name`: `&[u8]`; `file`: `Option<DiId>` → file; `line`: `u32`; `column`: `u32`; `isArtificial`: `bool`; `coroSuspendIdx`: `Option<u32>` |
| `DIImportedEntity` | `DiImportedEntity` | tag (default `0`); `name`: `&[u8]`; `scope`: `Option<DiId>` → scope; `entity`: `Option<DiId>`; `file`: `Option<DiId>` → file; `line`: `u32`; `elements`: `&[DiId]` |
| `DIBasicType` | `DiBasicType` | tag (default `DW_TAG_BASE_TYPE`); `name`: `&[u8]`; `size`: `u64`; `align`: `u32`; `encoding`: `u16`; `flags`: `DiFlags`; `dataSize`: `u32` |
| `DIDerivedType` | `DiDerivedType` | tag (default `0`); `name`: `&[u8]`; `scope`: `Option<DiId>` → scope; `file`: `Option<DiId>` → file; `line`: `u32`; `baseType`: `Option<DiId>` → type; `size`: `u64`; `align`: `u32`; `offset`: `u64`; `flags`: `DiFlags`; `extraData`: `Option<DiId>`; `dwarfAddressSpace`: `Option<u32>` |
| `DICompositeType` | `DiCompositeType` | tag (default `0`); `name`: `&[u8]`; `scope`: `Option<DiId>` → scope; `file`: `Option<DiId>` → file; `line`: `u32`; `baseType`: `Option<DiId>` → type; `size`: `u64`; `align`: `u32`; `offset`: `u64`; `flags`: `DiFlags`; `elements`: `&[DiId]`; `runtimeLang`: `u16`; `vtableHolder`: `Option<DiId>` → type; `templateParams`: `&[DiId]` → template param; `identifier`: `&[u8]` |
| `DISubroutineType` | `DiSubroutineType` | `flags`: `DiFlags`; `cc`: `u16`; `types`: `&[DiId]` → type |
| `DISubrange` | `DiSubrange` | `count`: `DiBound`; `lowerBound`: `DiBound`; `upperBound`: `DiBound`; `stride`: `DiBound` |
| `DIEnumerator` | `DiEnumerator` | `name`: `&[u8]`; `value`: `i64`; `isUnsigned`: `bool` |
| `DITemplateTypeParameter` | `DiTemplateTypeParameter` | `name`: `&[u8]`; `type`: `Option<DiId>` → type; `defaulted`: `bool` |
| `DITemplateValueParameter` | `DiTemplateValueParameter` | tag (default `DW_TAG_TEMPLATE_VALUE_PARAMETER`); `name`: `&[u8]`; `type`: `Option<DiId>` → type; `defaulted`: `bool`; `value`: `Option<DiId>` |
| `(constant)` | `DiConstant` | `value`: `ConstId` |
| `(string)` | `DiString` | `text`: `&[u8]` |
| `(tuple)` | `DiTuple` | `elements`: `&[DiId]` |
| `(other)` | `DiOther` | `kind`: `&[u8]` |

Four kinds are not LLVM node kinds: `DiConstant` is `ValueAsMetadata` of
a constant (a template value parameter's value, a bound), `DiString` an
`!"..."` operand, `DiTuple` a `!{...}` list, and `DiOther` a node of a
kind `hod_ir` does not represent, kept with its kind's name so that
`classify` reports `debug:<kind>` (§6.9). The fields' vocabulary (tags,
languages, encodings, `DiFlags`, `SpFlags`, `EmissionKind`,
`NameTableKind`, `ChecksumKind`) is in `debug::names`, with LLVM's
spellings.

`DiNode::new(kind)` is a node with every field at its default;
`DiNode::llvm_default(kind)` is a node as LLVM's parser starts one, which
differs in two places: a global variable is a definition, and so is a
subprogram (`spFlags: DISPFlagDefinition`: LLVM's `isDefinition` is
`true` when neither it nor `spFlags` is written).
`DiNode::set(field, value)` sets a field by its LLVM name, for a producer
that holds LLVM's names; it also takes the older spellings of a
subprogram's flags (`isLocal`, `isDefinition`, `isOptimized`,
`virtuality`).

For a reader of LLVM's text and for the printer, each field says what it
holds and gives its value back by name: `DiKind::field_shape(field)` is
the field's `DiShape` (a number, a signed number, an optional number, a
flag, a string, a node, a list, an expression, a constant, a bound, a
flag set of `DiFlags` or `SpFlags`, or one of the three enumerations),
which says what to make of the text written there; and
`DiNode::each_field(visit)` calls `visit(name, DiFieldOut)` for the tag
of a tagged kind and then for every field not at its default, in the
order of the table above, `DiFieldOut` being the field's value as
stored (`UInt`, `Int`, `Bool`, `Str`, `Node`, `List`, `Expr`, `Const`,
`Bound`, `Flags`, `SpFlags`, `EmissionKind`, `NameTableKind`,
`ChecksumKind`).

### 6.3 Building

- `add(node)` is a node LLVM would unique: it hash-conses, and an equal
  node added before (same kind, same fields) is returned again.
- `add_distinct(node)` is a `distinct` node: never equal to another id.
- `reserve()` hands out an id whose node `define(id, node)` or
  `define_distinct(id, node)` gives later, once. This is how cycles are
  built (a class names its members, each member names the class) and how
  a producer refers to `!7` before it has read it. A reserved node is
  never the answer to a later `add`.
- A compile unit, and a subprogram that is a definition, must be distinct
  (`DiNode::must_be_distinct`); `add` and `define` refuse them.
- `complete(id, node)` replaces a forward-declared composite type (with
  `DIFlagFwdDecl` and an `identifier`) by its definition with the same
  identifier, in place: every reference to `id` then names the
  definition. There is no uniquing by `identifier` otherwise (no ODR
  uniquing): equal ids mean one node; equal content does not mean equal
  ids.
- `add_compile_unit(unit)` appends to `!llvm.dbg.cu`; a unit that is not
  listed is not emitted.
- A node may name only ids that exist (reserved or defined):
  `UndefinedDebugNode` otherwise. Defining twice is `DebugRedefined`.
- `Module::finish_debug()` closes the tables: the producer calls it once,
  after the last node and before the first body is compiled. It checks
  that every reserved node and location was defined (two compares when
  that holds) and records with `Module::record_unsupported` the construct
  of each `DiOther` kind and each unknown expression operation (§6.9).

### 6.4 Expressions

An expression (`DIExpression`) is stored as LLVM stores it, a list of
64-bit elements, operation codes followed by their operands, and read as
typed operations: `ops(id)` yields `DiOp`s, `expr_elements(id)` the
elements. `expr(&[DiOp])` and `expr_from_elements(&[u64])` intern: equal
expressions have equal ids. `DiOp` has a member for each operation in
`DI_OPS` (the field table, §6.9, lists them with their codes and operand
counts); any other element is `DiOp::Other(element)`, kept so that
nothing is dropped, and reported as `debug:DW_OP_<name or number>`. A
known operation without its operands is `InvalidDebugInfo`.

Placement rules, checked by `verify` (§6.8) as LLVM's verifier checks
them: `DW_OP_LLVM_entry_value` is first (after a leading
`DW_OP_LLVM_arg 0`) and covers one operation; `DW_OP_LLVM_fragment` is
last; `DW_OP_stack_value` is last or just before the fragment;
`DW_OP_swap` is not alone.

### 6.5 Locations

`DiLocation { line, column, scope, inlined_at }` is LLVM's `DILocation`:
line 0 for none, a 16-bit column (LLVM stores a larger column as 0, and so
must a producer), the innermost local scope, and, for code inlined from
another function, the location of the call it was inlined at
(`LocId::NONE` otherwise).

- `loc(l)` interns: one hash of four words and usually one probe.
- `distinct_loc(l)` is LLVM's `distinct !DILocation`, which `clang` uses
  for an inlined-at call so that two calls on one line stay two inlined
  instances; also the cheapest way in for a producer whose locations are
  unique already (one push, no hash).
- `reserve_loc()` and `define_loc(id, l)` or `define_distinct_loc(id,
  l)`: for `!dbg !7` met before `!7`. A defined reserved location is not
  interned (a later `loc` of the same location is another id), and keeps
  whether it was defined `distinct`, which `is_distinct_loc(id)` answers
  for every location, so that the printer writes it as it was read.
- `location(id)` is one array read; `function_of(id)` is the subprogram
  of the scope at the end of the inlined-at chain: the function the
  instruction is in.

A location's scope and inlined-at must exist when it is made
(`UndefinedDebugNode`).

**Atoms** (WP-149, `dwarf.md §5.9`): a location may carry LLVM's Key
Instructions fields, `DiAtom { group, rank }` (`group` below 2^61, `rank`
at most 7, as LLVM stores them; 0 for none; otherwise
`InvalidDebugInfo`). `loc_in_atom`, `distinct_loc_in_atom` and
`define_loc_in_atom` make one; the atom is part of the location's
identity (two locations that differ only in it are two ids, as in LLVM).
`atom(id)` answers for every location. A location without an atom costs
nothing: the record stays 16 bytes, a flag bit of its state says that a
side table, keyed by location and only allocated when a module has an
atom, holds the atom (`has_atoms()`).

### 6.6 Variables, values and labels in a body

A body carries its debug records in three tables, each empty without
debug information.

- `Body::slot_vars()`: `SlotVar { slot, var, loc, expr }`, a variable
  that lives in a stack slot for the whole function (§5.2).
- `Body::declares()`: `DbgDeclare { address, var, expr, loc }`, a
  variable that lives for the whole function at an address that is not a
  slot: an argument, an instruction result, a constant or a symbol, or
  `Value::NONE` for a declaration whose address was dropped
  (`#dbg_declare(!{}, ...)`).
- `Body::dbg_records()`: the positioned records, a `DbgRecord` each, in
  position order (by `before`, and among the records before one
  instruction in the order given), so that a consumer walking the
  instructions in layout order walks this beside them.
  `DbgRecordKind::Value` is `#dbg_value`: from `before` on, the variable
  has the value `expr` makes from `values`. `Assign` is `#dbg_assign`:
  the same, and the variable's memory is at `address` with
  `address_expr`; a consumer that does not track assignments reads it as
  a value record. `Label` is `#dbg_label`: the label `var` is at
  `before`, and `values` is empty.

A record has one value, which the expression starts from, or a list
(`arg_list`, LLVM's `DIArgList`) that the expression names by
`DW_OP_LLVM_arg`. A value may be `Value::NONE` (LLVM's empty metadata) or
`undef` or `poison`: the variable has no location from there on. A
record's `loc` says, by its scope and inlined-at, which instance of the
variable or label it is.

The builder (`FunctionBuilder`) records them, and each also takes the
older intrinsic-call spelling (`llvm.dbg.declare` ...):

- `dbg_declare(address, var, expr, loc)`: a slot's address becomes a
  `SlotVar`, anything else a `DbgDeclare`. It has no position and needs no
  open block. `set_slot_var(slot, var, loc)` is `dbg_declare` of the
  slot's address with the empty expression.
- `dbg_value(value, var, expr, loc)`, `dbg_value_list(values, var, expr,
  loc)` (at most 65535 values), `dbg_assign(value, var, expr, address,
  address_expr, loc)` (one value: a `DIArgList` there is unsupported,
  §6.9; the `DIAssignID` is not kept), `dbg_label(label, loc)`: before the
  next instruction appended to the open block (`BadBlock` without one).
- `set_loc(loc)` gives the instructions appended from then on their
  location; `set_loc_since(first, loc)` gives it to those appended since
  the builder held `first` of them (`inst_count()` then) and to no
  others, for a producer that learns an instruction's location after the
  instructions it became, as LLVM's text, which writes `!dbg` last. These
  three tables and the location array are the whole of a body's debug
  information: the form `hod_ll` fills from text and the bitcode reader
  from records.

A record or declaration does not use the value it names: use counts and
the two facts about uses (§5.6) are the same with and without debug
information. A name may be a reserved value; it is rewritten to what the
value was defined as, and a declaration whose address turns out to be a
slot becomes a `SlotVar`. The one exception is
`UseFacts::named_by_declare()`: the instruction results and arguments a
`DbgDeclare` names say so, because such a value needs a home for the
whole function (`spec/codegen.md` §4.1). That bit takes one bit of the use
count, which saturates at 2^29 − 1; `only_use_is_next` ignores it.

### 6.7 Attachments

- A function symbol's optional subprogram (§2.2, `FunctionDesc`) is its
  `!dbg`. The subprogram of a function with a body is distinct, and is no
  other such function's.
- A variable's `!dbg` attachments are global-variable expressions:
  `Module::attach_global_expr(sym, expr)`, one per fragment or per
  variable that shares the storage. `global_exprs()` lists every
  attachment in the order made (the DWARF writer reads it once);
  `global_exprs_of(sym)` scans it for one symbol. The symbol's kind is
  checked by `verify_debug`, because a producer may attach before it has
  described the symbol.
- An instruction's `!dbg` is its entry in the body's location array
  (§5.9).

### 6.8 Verification

`DebugInfo::verify()` checks the tables in one pass:

- everything reserved was defined;
- every reference names a node of a kind its field allows (§6.2);
- every listed compile unit is one;
- a location's scope is a local scope whose chain of blocks ends at a
  subprogram, and its inlined-at chain ends;
- an expression's operations are placed as §6.4 says.

`Module::verify_debug()` runs it, and checks that a `DiConstant` names a
constant of the pool, that a function's subprogram is a subprogram, that
the subprogram of a function with a body is distinct and no other such
function's, and that a global-variable expression is attached to a
variable. Like `verify_symbols` it is for tests and for producers that
want their output checked; code generation does not need it to have run.

`Body::verify` (§5.8) checks a body's debug records, and
`Body::verify_debug(module)` checks those rules alone, for a producer
that hands out only bodies whose debug information LLVM's verifier would
keep (`spec/ll.md` §11.9):

- always: declarations and records name values of this body (or
  nothing); a declaration names no slot; records are in position order,
  each before an instruction of the body that is not a `phi`; a label has
  no values and a value record without a list one; "named by a declare"
  is recomputed with the other use facts;
- and, whether or not the module has debug tables (an id into an empty
  table names nothing, so a module without debug information has no
  valid location or variable): every instruction location exists,
  leads to a subprogram and, when the function has one, to the
  function's; the variable of a slot variable, declaration or record is a
  `DILocalVariable` and a label a `DILabel`, in the subprogram of the
  record's location; its expressions exist and name only arguments the
  record has. A function without a subprogram may have locations in any
  function, as in LLVM.

Errors are `UndefinedDebugNode` (an id that does not exist or was never
defined), `DebugRedefined`, and `InvalidDebugInfo` for every other rule.
LLVM's verifier checks more (required fields, tag validity, flag
consistency, fragment bounds, ...); the rules not checked are listed in
`spec/landed/ir/WP-005.md`, and a consumer treats every optional
reference as possibly absent.

### 6.9 The field table, and the producer's duty

`debug::map` is data saying what becomes of every node kind, field and
debug record LLVM 19 to 22 can write: **mapped** (`hod_ir` has a field
for it), **dropped** (it cannot change the DWARF hod writes, with the
reason), or **unsupported** (it would, and `hod_ir` cannot represent it:
the construct name). A producer looks each one up (`di_kind`, `di_field`,
`di_record`) and:

- sets a mapped field (`DiNode::set`);
- ignores a dropped one;
- for an unsupported field with a value other than its default, records
  the construct (`debug:<kind>.<field>`) with
  `Module::record_unsupported`;
- makes an unsupported or unknown kind a `DiOther` (`debug:<kind>`), and
  records an unknown field as `debug:<kind>.<field>`.

`finish_debug` (§6.3) records the `DiOther` kinds and unknown expression
operations. `classify` then refuses the module by those names: a module
whose debug information hod cannot represent is unsupported, never
silently degraded. `di_field_table()` lists every row; the landed file
prints the table, and a test keeps it current. `tests/debug_survey.rs`
holds what `clang` 19 and 22 and kiln write against it.

### 6.10 Sizes and speed

A node head is 8 bytes and a node 8 + 4·(1 + stored cells); a location
16; `SlotVar`, `DbgDeclare` 16; a debug record 28 bytes plus 4 per value;
`ExprId` 4. `stats()` (`DebugStats`) reports the nodes, expressions,
locations and strings held and their bytes. The tests assert the record
sizes and that an empty table has allocated nothing; the example
`debug_throughput` measures building, reading and verifying, with the
numbers in `spec/landed/ir/WP-005.md`.

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

`classify` (`crates/hod_ir/src/classify/`) is the single place where
"supported" is decided:

- the driver calls it before code generation, and a rejection becomes the
  fallback reason in the coverage log (`spec/cli.md`);
- code generation may assume everything `classify` accepts, and reaching
  an unhandled case there is a bug, reported as an internal error, never a
  guess;
- `docs/status/support.md` is generated from it (§8.10).

`classify` starts by rejecting nearly everything. A work package that
implements a construct flips its row and adds the tests in the same
change.

### 8.1 API and outcomes

- `classify(&Module)`: the module pass (§8.8) under `Support::shipped` for
  the module's target. `classify_body(&Module, &Body)`: the body pass for
  one function. A driver classifies every body it compiles: the
  function-level checks (convention, parameters, attributes) are made in
  the body pass, not the module pass.
- `census(&Module, bodies)`: every finding of both passes (§8.9).
- `Classifier` is the general form: `Classifier::new(&Support)`,
  `with_stack_args(&dyn StackArgs)` (the hook the `musttail` rules need,
  §8.6), `support()`, `module`, `body`, `census_module`, `census_body`.
- A refusal is `Rejection { kind, construct, symbol, inst }`: `kind` is
  `RejectionKind::Unsupported` (a matrix row that is not supported, or a
  structural rule) or `RejectionKind::Invalid` (malformed input, §8.7);
  `construct` is the stable name (§8.5); `symbol` is the symbol in the
  module pass and the function in the body pass, `None` for a property of
  the module; `inst` is the instruction in the body pass, `None` for the
  function's signature and attributes.
- `classify` does not panic, for any module and any body a
  `FunctionBuilder` finished, including a body classified against another
  module than its own: everything it cannot trust is `Invalid`.

### 8.2 The matrix and `Support`

The matrix is data (`matrix.rs`, `SUPPORT_ROWS`): one `SupportRow` per
construct, with a `SupportCell` per target whose status is `supported`
(for a set of type classes where the row operates on values, §8.3),
`not-implemented(<what>)` (the work package, milestone or track that
flips it, or `unplanned`), or `rejected(<why>)` (by design).

`Support` is the matrix as a value a walk consults, for one target:

- `Support::shipped(arch)`: the matrix as checked in; what `classify`
  uses.
- `Support::everything_implemented(arch)`: every row that is not
  `rejected` supported for every class. What it still refuses is refused
  by design.
- `with(name)`, `with_classes(row, classes)`, `without(name)` switch a
  row, or one class of it (`row/class`); `with` of a `rejected` row is
  `None`. `supports(name)` and `classes(row)` read it.

Flipping a construct is a one-line change in `matrix.rs` plus a test.

### 8.3 Type classes and subjects

`TypeClass` has 15 members: `int` (at most 64 bits), `i128`, `int-wide`,
`ptr` (address space 0), `half`, `bfloat`, `float`, `double`, `fp128`,
`x86_fp80`, `vector` (fixed length; element kinds are not examined),
`scalable-vector`, `aggregate` (a struct or array as a first-class
value), `token`, `target-ext`. Each has a row `type:<class>`.

The **subjects** of an instruction are its result type, unless `void`,
and the type of every value operand. Pointee types, aggregate member
types and global value types are not examined. For each subject:

- the class's `type:` row is not supported: `type:<class>`;
- otherwise the instruction's row is supported, but not for this class:
  the pair `<row>/<class>` (e.g. `inst:select/double`);
- a pointer in a non-zero address space: `addrspace:pointer`
  (`addrspace:alloca` for the result of an `alloca`).

Results of `cmpxchg`, `landingpad` and the `*.with.overflow` intrinsics
are of class `aggregate`, and wait for `type:aggregate`.

### 8.4 Groups

A row's group is its name up to the colon. `SUPPORT_GROUPS` lists the 23
groups in the order of `docs/status/support.md`: `module`, `structor`,
`debug`, `global`, `linkage`, `visibility`, `tls`, `comdat`, `init`,
`function`, `cc`, `attr`, `type`, `inst`, `mem`, `atomicrmw`, `call`,
`tail`, `bundle`, `landingpad`, `operand`, `const-expr`, `intrinsic`.
There are 674 rows (`docs/status/support.md`, "Summary").

### 8.5 Vocabulary

Every name has the form `group:name`, in lower case, with LLVM's spelling
after the colon where LLVM has one (`intrinsic:llvm.memcpy`,
`cc:swifttailcc`, `attr:preallocated`, `inst:catchswitch`).
`CONSTRUCT_NAMES` lists every fixed name: the rows, then the structural
rules (§8.6), then the malformed-input names (§8.7). `CONSTRUCT_FAMILIES`
lists the six families of names built from the input:
`intrinsic:<name>` (`Intrinsic::Other`, always refused),
`module-flag:<name>`, `cc:<keyword>` (a convention without a row, by
LLVM's keyword, `cc:arm_apcscc`, or by its number where LLVM has no
keyword, `cc:42`; before WP-141 every such convention was named by its
number), `bundle:<tag>`, `<row>/<class>`, and
a producer's record (`Module::record_unsupported`, §2.2), passed through
unchanged.

The `abi:*` names of a target's argument assignment and the facade's
`Construct` names (`spec/cli.md`) share this namespace: no two mean
different things.

### 8.6 Structural rules

Reasons no `Support` takes away (`STRUCTURAL_RULES`):

| name | when |
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

Also refused on both targets, as matrix rows whose status is `rejected`:
`attr:naked`, `inst:va_arg`, the 19 `const-expr:*` rows (constant
expressions without a normal form, §4.2), and the calling conventions
`cc:cfguard_checkcc`, `cc:win64cc` (Windows), `cc:x86_vectorcallcc`
(Windows on x86-64), `cc:x86_stdcallcc`, `cc:x86_fastcallcc`,
`cc:x86_thiscallcc` (32-bit x86: `llc` treats them as `ccc` on x86-64,
and no x86-64 front end emits them), `cc:x86_intrcc` and
`cc:x86_regcallcc` (WP-141).

Calling conventions (WP-141): a convention is supported, `unplanned`, or
refused by design on each target; one of the other target's (the x86
conventions on AArch64, `aarch64_*` on x86-64) is refused by design,
since `llc` for that target aborts on it. `x86_64_sysvcc` is supported on
x86-64 and assigned as `ccc`: it is the System V convention, which is
x86-64 Linux's C convention (`llc` 19 and 22 emit identical code for the
two, `-O0` with and without fast instruction selection, SSE2, AVX2 and
AVX-512, over arguments of every class, on the stack, `byval`, `sret`,
`x86_fp80`, `fp128`, `i128` and variadic calls). `coldcc`, `ghccc`,
`anyregcc`, `tailcc`, `swifttailcc` and `preserve_nonecc` are `unplanned`
on both targets; `intel_ocl_bicc`, `hhvmcc` and `hhvm_ccc` on x86-64. `inst:callbr` was one until
WP-103; it is supported on AArch64 (`spec/codegen.md` §5.5.3).

- Producer records (`Module::unsupported()`) are reported unchanged.
- A module flag that is neither known nor ignorable (§2.1), or any
  `Require` flag, is `module-flag:<name>`.
- `returns_twice` (`attr:returns_twice`) is checked on the body's
  function, on the call and on a direct callee's declaration.
- A direct call through a function type other than the callee's is not
  an error: the call's own function type and attributes govern; the
  callee's declaration is not consulted (beyond `returns_twice`), nor are
  a declaration's attributes. A direct call through the callee's own
  type is walked with its effective attributes (`spec/codegen.md` §7.1:
  the call site's merged with the declaration's, `call_arg_attrs`), so
  that `classify` sees what the call sequence passes; where the two sides
  conflict (`call:attribute-conflict`, refused by the call sequence) both
  sides' rows are reported.
- The constant walk has a depth limit of 64 and no memo: a shared
  sub-constant is walked at each use.

### 8.7 Malformed input

Outcomes of kind `Invalid` (`INVALID_RULES`): what `Body::verify` and the
builders cannot see, mostly because it needs the module.

| name | when |
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

`invalid:initializer` and the personality case of `invalid:symbol` are
defensive: the current builders cannot produce them. The first four
stop the body pass (nothing else in the body can be trusted). Whether a
malformed input goes to the fallback is `spec/cli.md`'s rule, not this
crate's.

### 8.8 Order

The first reason is deterministic: the first finding of the walk, which
visits, in order:

- **Module pass.** `target:support-mismatch`; the module properties
  (`module:pic`, `module:pie`, code model, large-data threshold,
  `module:uwtable`, `module:frame-pointer`, the branch-protection bits in
  order, `debug:module-flags`); the other module flags in table order;
  module assembly, linker options, dependent libraries; constructors,
  then destructors, entry by entry (the entry, its priority, its
  associated symbol, then the symbols it names); producer records; then
  every symbol in id order: what it is (declaration or definition of a
  variable or function, alias, IFUNC), linkage, visibility, TLS model,
  address space; for a definition `unnamed_addr`, section, COMDAT,
  alignment, `used`; then a variable's initializer, a function's
  attribute set and personality, an alias's target, an IFUNC's resolver.
- **Body pass.** `target:support-mismatch`; the four stopping
  malformed-input checks (§8.7); the function's convention, variadic,
  parameters and their types, return and parameter attributes, function
  attributes (flags in order, `function:may-unwind`, `uwtable`, `alignstack`,
  string attributes), personality, debug subprogram, stack slots and
  over-aligned slots, slot variables, debug locations; then every
  instruction in layout order. For one instruction: its row; then (by
  opcode) memory access, atomic operation, landing-pad clauses or switch
  cases; its result's subject; its operands in order. For a call: its
  row (a plain call of a known intrinsic is the intrinsic's row only),
  the callee (indirect, inline assembly, variadic, convention), the tail
  marker (`musttail`'s rules before its row), `returns_twice`, bundles by
  tag, return attributes, the result, the callee operand, each argument
  then its attributes, the bundle inputs.

A structural rule or a malformed-input outcome comes where the walk meets
it, not first. `findings_come_in_the_specified_order` and
`a_rejection_names_where_it_was_found` in `tests/classify_matrix.rs` pin
this order.

Under `Support::shipped` today every body is refused first at `cc:ccc`,
and every module with a definition at `global:function-definition` or
`global:variable-definition`; a module of external declarations only is
accepted. `debug:*` rows wait for the debug-information track, so a
module with the `Dwarf Version` or `Debug Info Version` flags is refused
at `debug:module-flags`.

### 8.9 Census

`Census` collects every finding instead of the first: one `CensusEntry {
construct, kind, occurrences, symbols }` per construct, in the order of
first occurrence, where `symbols` counts the distinct symbols it was met
at. Its first entry is the first reason. It is for coverage reports, not
for the compile path.

A census is the findings of one `Support`, so a pair `<row>/<class>` is
in it only where §8.3 makes the pair a finding: while `type:<class>` is
not supported, the finding at that subject is `type:<class>`, and the
pair appears only under a `Support` with that type row switched on.
Classifying again and again, switching on each first reason, therefore
meets such a pair after its `type:` row, where the census of the first
`Support` has the type row alone (WP-027: the `token` parameter of
`classify_random`'s seed 360 under WP-033's matrix; pinned by
`a_pair_is_found_only_behind_its_type_row`).

### 8.10 `docs/status/support.md`

`support_markdown()` writes the matrix as a document: a per-group
summary of the counts per target and status, then one table per group,
then the structural rules, the malformed-input names and the families.
The checked-in file is that text; `tests/classify_support_doc.rs` fails
when it is stale, and `HOD_BLESS=1 cargo test -p hod_ir --test
classify_support_doc` rewrites it.

### 8.11 Cost

The accept path does not allocate and does not format: a walk reads the
tables once and consults an array of bit sets per row. A short way
(`Walker::accepts`) decides the ordinary instruction with ordinary
operands; everything else goes the long way (`Walker::inst`), which is
the definition, and debug builds check on every instruction that the
short way never accepts what the long way refuses.

The short way reads one word per opcode (`Support::short`, built with
the support masks): whether the short way may decide the op, whether it
is a call or memory op, its inline operand count, and its row's accepted
classes. A direct callee accepted with no declared attributes is
remembered for the body (8 slots, keyed by symbol and function type).
The constant walk does not descend into leaf elements of an aggregate
below the nesting limit.

Measured with callgrind (release, strict, one thread, both targets; see
`spec/landed/ir/WP-027.md`). On the bench inputs, classify is a small
fraction of a whole run: 0.6 to 2.3% of the instructions (0.9 to 3.3%
before WP-027). In isolation (`examples/classify_throughput.rs` with `build` and `classify`),
classifying an accepted body costs 92 instructions per IR instruction,
against 183 to build it. That is half of building, down from 89% before
WP-027. The rest is operand iteration and the per-operand value and
class match.

## 9. Printer

`Module::print` writes LLVM textual IR (dialect selected by an LLVM major,
19 to 22) for the module and the bodies it is given. Guarantees:

- `llvm-as` of the dialect's major accepts it and `opt -passes=verify`
  verifies it, for a module that passes `Module::verify_symbols` with
  bodies that pass `Body::verify` and keep the rules of §9.8;
- compiled by `llc`, it behaves as the IR says (checked by the
  differential harness, which is also how producers are validated:
  `input → producer → print → llc` against `input → llc`);
- it is deterministic (§9.7);
- where LLVM's text cannot say what hod means, the printer returns an
  error naming the construct (§9.5, §9.6). It never prints text that
  means something else.

Uses: debugging, reduced reproducers, handing hod's IR to LLVM's tools in
tests, and splitting a module into "functions for hod" and "functions for
`llc`" for bisection.

### 9.1 Interface

- `Dialect`: `LLVM19`, `LLVM20`, `LLVM21`, `LLVM22`, `ALL`;
  `Dialect::new(major)` (any other major is `Unsupported`
  `dialect:llvm-<major>`), `major()`.
- `Module::print(out, dialect, bodies)`: the whole module. The module does
  not own bodies: `bodies` (any `IntoIterator<Item = &Body>`, any order)
  supplies them, and a function with none is written as a declaration.
- `Module::print_part(out, dialect, part, bodies)`: one `Part` (§9.6).
  `print` is `print_part` with `Part::Whole`.
- `Module::print_body(out, dialect, body)`: one `define` as it stands in
  the module's text and nothing else, a fragment for reading. It names
  the module's symbols, attribute groups and intrinsics without
  declaring them, and checks nothing that needs the other bodies.
- `out` is any `fmt::Write` (`String`, ...). `IoSink::new(w)` makes an
  `io::Write` one; `IoSink::finish` gives the writer back, or the
  `io::Error` that stopped the printer. The printer makes many small
  writes: give it a buffered writer.
- `PrintError`: `Unsupported { construct, symbol }` (the construct names
  of §9.5 and §9.6 are stable; `symbol` is the one being printed);
  `Malformed { what, symbol }` (an id that names nothing, a body for a
  symbol that is not a function with a body or whose function type is
  not the symbol's, two bodies for one function, a `blockaddress` of a
  block the function lacks); `Write` (the sink refused the text). On an
  error the sink may hold part of the text.

### 9.2 Dialects

The text is the same for every major but for these differences (checked
against LLVM 19.1.7 and 22.1.8; 20 and 21 are specified by this table and
not tested, their tools not being installed):

| construct | 19 | 20 | 21 | 22 |
|---|---|---|---|---|
| `ptrtoaddr` (instruction and constant expression) | `ptrtoint`, where the address is the whole pointer; else `inst:ptrtoaddr` / `const-expr:ptrtoaddr` | as 19 | as 19 | `ptrtoaddr` |
| `llvm.thread.pointer` | no suffix (only `ptr`; else `intrinsic-overload:`) | as 19 | as 19 | `.p0` |
| function attributes `sanitize_realtime`, `sanitize_realtime_blocking`, `sanitize_type` | `attr:<name>` | yes | yes | yes |
| function attribute `sanitize_alloc_token` | `attr:<name>` | `attr:<name>` | `attr:<name>` | yes |
| `atomicrmw usub_cond`, `usub_sat` | `atomicrmw:<op>` | yes | yes | yes |
| `atomicrmw fmaximum`, `fminimum` | `atomicrmw:<op>` | `atomicrmw:<op>` | yes | yes |
| intrinsics `minimumnum`, `maximumnum`, `atan2` | `intrinsic:<name>` | yes | yes | yes |
| intrinsics `clmul`, `coro.is_in_ramp` | `intrinsic:<name>` | `intrinsic:<name>` | `intrinsic:<name>` | yes |
| constant expression `mul` | yes | yes | `const-expr:mul` | `const-expr:mul` |
| module flag `frame-pointer` value 4 | `module-flag:frame-pointer=4` | same | same | yes |
| AArch64 branch-protection module flags (`branch-target-enforcement`, ...) | value 1 | value 2 | 2 | 2 |
| AArch64 Linux data layout | LLVM 19's string, without `p270`..`p272` | the target's | the target's | the target's |
| an `i1` global, or an array of them, without an alignment | `, align` of §3 written | none written | none | none |

The constant expressions `zext`, `sext`, `and`, `or` and `shl` are gone
from every dialect (`const-expr:<op>`). On LLVM 19's AArch64 layout a
pointer in address space 270 to 272 has another width than §3 gives it:
`type:ptr addrspace(N)`. LLVM 19 aligns an `i1` global to 1 and later
majors (and §3) to `i8`'s preferred alignment, hence the explicit
alignment. From LLVM 20 on, branch-protection value 2 says that each
function states its own protection, as the attribute sets do; 1 would
make LLVM add it to functions that say nothing.

### 9.3 Module-level text

In this order:

1. `target datalayout` (§9.2) and `target triple`.
2. `module asm`, one per line of the module assembly, quoted (§9.3's
   escaping).
3. `%hod.opaque.<n> = type opaque` per opaque struct. Other struct types
   are written as literal types (`{ i8, i64 }`, `<{ ... }>`): no names
   are kept.
4. `$name = comdat <kind>` per COMDAT, in id order.
5. Every symbol, in id order:
   - a variable: `@name = [linkage] [dso_local] [visibility]
     [thread_local(...)] [unnamed_addr] [addrspace(N)] global|constant
     <type> [<initializer>][, section "s"][, comdat($c)][, align A]`. It
     is defined when it has an initializer (§9.6 for `Part::Rest`); a
     declaration says `external` (or `extern_weak`).
   - a function: `define` when its body is supplied, else `declare`;
     `[linkage] [dso_local] [visibility] [cc] [return attributes] <ret>
     @name(<type> [attributes] [%a<n>], ... [, ...])
     [unnamed_addr] [addrspace(N)] [#<set>] [section "s"]
     [comdat($c)] [align A] [personality ptr @p] { body }`. A `declare`
     keeps the calling convention, the parameter and return attributes
     and the attribute set. `cc` is the keyword (`fastcc`, `coldcc`, ...)
     when the dialect has one (`CallConv::keyword_since`), else `cc <n>`.
   - an alias: `@name = [linkage] ... alias <type>, <aliasee>`; an IFUNC:
     `@name = ... ifunc <type>, ptr @resolver`.
   - `dso_local` is written only where LLVM does not imply it (a local
     linkage, or a visibility other than default on a symbol that is not
     `extern_weak`).
6. `@llvm.used` and `@llvm.compiler.used`: the retained symbols this
   text defines, in id order, `section "llvm.metadata"`. An anonymous
   member in `Part::Whole` is `<list>:anonymous-symbol` (LLVM wants them
   named).
7. `@llvm.global_ctors` and `@llvm.global_dtors`: `{ i32 priority, ptr
   f, ptr associated-or-null }` in the order they were added.
8. A `declare` for every intrinsic the printed bodies call, by name and
   overloads, with the function type of the calls, unless the module has
   a symbol of that name. One name called with two function types is
   `intrinsic-signature:<name>`.
9. `attributes #<id> = { ... }` for each attribute set a printed
   function names (`#<id>` is the set's id): the flags in
   `FnAttrs::FLAG_NAMES` order, `uwtable` / `uwtable(sync)`,
   `alignstack=N`, then the strings in key order, `"key"="value"`,
   `"key"` for an empty value, and `"no-builtin-<value>"`.
10. The module flags as `!N = !{i32 behavior, !"name", value}`: `PIC
    Level` (8), `PIE Level` (7), `Code Model` (1), `Large Data Threshold`
    (1, always `i64`), `uwtable` (7), `frame-pointer` (7), `Dwarf
    Version` (7), `Debug Info Version` (2), the branch-protection flags
    (8, §9.2), then the other flags in the order they were set. An
    integer value is `i32` when it fits, else `i64`; a string value is
    `!"..."`; `FlagValue::Other` is `module-flag:<name>`. Then
    `!llvm.module.flags`, one node of strings per linker option
    (`!llvm.linker.options`) and per dependent library
    (`!llvm.dependent-libraries`). Nodes are numbered from 0 in that
    order.

Names: `@name`, `$name` are bare when the name is LLVM identifier
characters (`[A-Za-z0-9$._-]`) not starting with a digit, else quoted.
In a quoted string or name, printable ASCII but `"` and `\` is written as
it is, and every other byte as `\XX`. An anonymous symbol is `@<id>`
(`@__hod_anon.<id>` in a split, §9.6); LLVM renumbers them, so the
number is a name for reading only.

Constants: integers as signed decimals (`i1` as `true` / `false`),
integers wider than 64 bits as `u0x<hex>`; `half` `0xH`, `bfloat` `0xR`,
`float` as the hexadecimal `double` of the same value (a NaN keeps its
payload and signaling bit, as LLVM's printer does), `double` `0x`,
`fp128` `0xL` (low word first), `x86_fp80` `0xK`; `null`,
`zeroinitializer`, `undef`, `poison`, `none`; aggregates in braces,
brackets or angles; `splat (...)`; bytes as `c"..."`. A symbol's address
plus an addend is `getelementptr (i8, ptr @s, i64 addend)` (the bare
`@s` for 0); `blockaddress(@f, %b<n>)`; a symbol difference as
`sub (i64 ptrtoint ..., i64 ptrtoint ...)`, with `add` for an addend,
`trunc` for a narrower type and `dso_local_equivalent` for a PLT
reference; `ptrtoint (...)`, `inttoptr (i64 N to ptr)`,
`dso_local_equivalent @s`, `no_cfi @s`; the expressions of §4.2 that
have no normal form as LLVM's constant expressions. An initializer means
to LLVM what `flatten` (§4.3) makes of it, byte for byte and relocation
for relocation. The address of a symbol in an address space other than
0 is `address-of-symbol-in-address-space` (hod types it `ptr`, LLVM
`ptr addrspace(N)`); a target extension type is `type:target("name")`
(its parameters are not kept). A `blockaddress` needs the function's
body in the same text (`blockaddress-of-declaration`) and a block other
than the entry (`blockaddress-of-entry-block`).

### 9.4 Bodies

- Blocks are `b<id>:` in layout order (labels `%b<id>`), values `%v<id>`,
  arguments `%a<n>`.
- Stack slots are `%s<n> = alloca [size x i8], align A` at the top of
  the entry block, in slot order.
- `PtrAdd` is a chain of `getelementptr i8` steps: one for the constant
  offset (or a zero offset when there are no indices), then one per
  index, the index first brought to the pointer's index width
  (`sext`/`trunc`, `.c<k>`) and multiplied by its scale (`mul`, `.m<k>`).
  The last step has the instruction's name, the ones before it
  `%v<id>.g<n>`. A vector index makes the later steps vectors. An index
  width outside 1 to 64 bits is `ptradd:index-width`.
- An aggregate access by byte offset (`extractvalue`, `insertvalue`)
  prints the index path that `aggregate_field` maps back to that offset
  and type: depth first, a struct's fields in ascending order, the first
  field with the type at the offset.
- A `phi` without incoming values (its block has no predecessor) is
  `freeze <ty> poison`.
- Calls: tail markers on `call` only (`invoke:<tail>`, `callbr:<tail>`
  otherwise); the full function type is written for a variadic callee; a
  `musttail` call in a variadic function ends its arguments with `...`;
  `noreturn`, `nounwind`, `returns_twice`, `convergent` call attributes, operand
  bundles as `[ "tag"(...) ]`, `to label ... unwind label ...` for
  `invoke`, the indirect labels for `callbr`; inline assembly as `asm
  sideeffect "template", "constraints"`.
- A metadata operand is written in place: `metadata !"s"` or `metadata
  !{!"a", !"b"}`, each string quoted as a string constant's name is.
- An intrinsic's name is `Intrinsic::mangled_name`'s: the base name and
  one suffix per overload type (§9.2 for `llvm.thread.pointer`), and
  `Other(name)` as it is. A dialect without the intrinsic is
  `intrinsic:<name>`; an overload type without a spelling in a name is
  `intrinsic-overload:<name>`.
- A scalable `shufflevector` mask is `shufflevector:scalable-mask`
  (LLVM's text has only `zeroinitializer` and `poison` for one).
- Constants of an index width are written as signed numbers.

### 9.5 Instructions without text

`catchswitch`, `catchpad`, `cleanuppad`, `catchret` and `cleanupret` are
`inst:<opcode>`: `hod_ir` keeps none of their operands (§5.4). Also an
error in a body: `atomicrmw:<op>` and `inst:ptrtoaddr` (§9.2).

### 9.6 Parts and the split

`Part::Whole` is the module. `Part::Main` and `Part::Rest` split it into
two texts: printed as `Main` with some bodies and as `Rest` with the
others, their objects link to what the whole module's object is.

- In both: a function is defined when its body is supplied; a symbol of
  local linkage becomes external and `hidden`, so that the other text
  can name it; anonymous symbols are `@__hod_anon.<id>` in both.
- `Main` also defines every variable with an initializer and has the
  module assembly, the constructors and destructors, the linker options
  and dependent libraries. `Rest` declares every variable and has none
  of these.
- An alias or IFUNC is defined where what it names is defined; elsewhere
  it is a declaration (a function's with the convention and attributes
  of the function it names, else `global` of its type).
- The retained lists hold the symbols the text defines.
- A COMDAT is kept in a text that defines all its members. Elsewhere a
  member that the linker may see twice anyway (`linkonce`, `weak`, or
  local before the split) leaves it; any other member is `split:comdat`.
- A function whose blocks are addressed by a `blockaddress` in a
  variable must be defined in `Main` (`blockaddress-of-declaration`).

Module-level errors, checked before any text: the first construct a
producer recorded with `Module::record_unsupported` is returned as it was
recorded (the object would lack what it stands for), and a symbol
neither defined nor declared is `unresolved-symbol`.

The constructs, all told: `dialect:llvm-<major>`, `unresolved-symbol`,
the recorded ones, `type:target("name")`, `type:ptr addrspace(N)`,
`module-flag:<name>`, `module-flag:frame-pointer=4`,
`address-of-symbol-in-address-space`, `<list>:anonymous-symbol`,
`split:comdat`, `blockaddress-of-declaration`,
`blockaddress-of-entry-block`, `const-expr:<op>`, `attr:<name>`,
`intrinsic:<name>`, `intrinsic-overload:<name>`,
`intrinsic-signature:<name>`, `invoke:<tail>`, `callbr:<tail>`,
`atomicrmw:<op>`, `inst:<opcode>`, `ptradd:index-width`,
`shufflevector:scalable-mask`.

### 9.7 Determinism, normal form, debug information

The same module and bodies give the same text, byte for byte, whatever
the order of the bodies and whatever the sink. The text is a normal
form, not a round trip of what a producer read: `PtrAdd` as `i8`
`getelementptr`s, slots as `alloca`s, numbered values and blocks,
literal struct types, and what the IR dropped stays dropped.

Debug information (`crates/hod_ir/src/print/debug.rs`) is numbered
before every other metadata: node `DiId(i)` is `!i` and location `l` is
`!(node_count + l)`; strings (`!"…"`), constants (`i32 7`), tuples a
field names as a list (`!{…}`) and expressions (`!DIExpression(…)`)
are written in place, so not every number is used. A node is written
with its tag first and then the fields `each_field` gives (§6.2), DWARF
constants by name (`DW_TAG_*`, `DW_ATE_*`, `DW_LANG_*`, `DW_CC_*`),
flag sets as `A | B` with the multi-bit fields first and any bits
without a name as a number; a field LLVM's parser requires, and a field
whose default there is not hod's (a subprogram's `spFlags`, a global
variable's `isDefinition`), is written even at hod's default. A field
LLVM's text has only from a later major (`DILabel`'s `column`,
`isArtificial`, `coroSuspendIdx`, `DIBasicType`'s `dataSize`, from 21)
is `Unsupported("debug:<kind>.<field>")` in an older dialect when it is
not at its default. Then the locations, then `!llvm.dbg.cu`, then the
module flags and the rest.

` !dbg !N` stands on a `define` with its subprogram, and on a `declare`
whose subprogram is not distinct; a variable gets `, !dbg !N` for each
global-variable expression. In a body, `#dbg_declare` records (in every
dialect: LLVM 19's parser takes records) follow the entry block's
`alloca`s, one per slot variable and per declaration whose address is
not an instruction result; a declaration of an instruction's result
follows that instruction (or the `alloca`s, for a `phi` or a
terminator). The positioned records stand before their instruction, and
`, !dbg !N` follows an instruction with a location. An assign record is
`Unsupported("debug:#dbg_assign")`: without its `DIAssignID` the text
could not say what it says. A `DiOther` node is
`Unsupported("debug:<kind>")`.

### 9.8 LLVM's rules that `hod_ir` does not check

The printer prints what `Body::verify` and `Module::verify_symbols`
accept, and LLVM 19 and 22 verify all of it but what is listed here. The
rules LLVM's verifier has that `hod_ir` once left to producers
(dominance, `callbr` and `invoke` callees, tail markers and `musttail`,
`immarg`, bundles, parameter attributes, the realtime sanitizer pair,
module-flag shapes) are checked since WP-026 (§2.1, §5.7, §5.8). What
remains, each a rule no producer of hod's has met and a test of LLVM's
behaviour would need a construct `hod_ir` does not model:

- `swifterror` values: LLVM allows a `swifterror` argument or `alloca`
  only as a load or store address or a `swifterror` call argument.
- `preallocated`: the `"preallocated"` bundle's token and the
  `llvm.call.preallocated.*` protocol, and `musttail` with it.
- An intrinsic's own `immarg` value ranges (`llvm.prefetch`'s locality
  0 to 3, ...): `verify` checks that the argument is a constant, not its
  value.
- The shapes of bundles other than `kcfi` (`deopt`, `gc-live`,
  `funclet`, `ptrauth`, `clang.arc.attachedcall`, `convergencectrl`):
  at most one of each is checked, not their operands.
- `elementtype` on an indirect inline-assembly operand: LLVM requires it
  on each `=*` or `*` constraint; `verify` allows it there and does not
  require it.
- `musttail` in a body whose symbol is not a function of the module: the
  function's calling convention and parameter attributes are taken as
  `ccc` and none.
- In an ELF object, each linker option is a key and a value: `llc`
  refuses another option ("invalid llvm.linker.options"), but LLVM's
  verifier accepts it and so does a Mach-O or COFF object. It is a rule of
  the object writer, not of the IR, so `verify` does not check it; the
  ELF writer refuses such an option (WP-032's `hod_cg::emit_module`).
- An `atomicrmw` operation LLVM 19 does not have (`fmaximum`, `fminimum`,
  `usub_cond`, `usub_sat`) is not printed for LLVM 19 (§9.2): not a
  verifier rule, a dialect difference.

What the model does not keep: a symbol's address is typed `ptr` whatever
its address space; branch-protection value 1 versus 2, the behaviors and
integer widths of the known module flags, and `TargetExt` parameters are
not kept.

### 9.9 Tests

- Unit (`print/mod.rs`, `print/body.rs`): dialect numbers, `IoSink`
  errors, `float` spelling, name quoting, intrinsic names against
  `Intrinsic::mangled_name`, the index path against `aggregate_field`.
- Random bodies (`print/random_tests.rs`): 1500 generator bodies, each
  verified by `Body::verify`, printed for LLVM 19 and 22 in batches of
  modules; every printed body passes `llvm-as` and `opt -passes=verify`
  (no "parsed only" bucket), at least 1000 per major, covering every
  opcode but the funclets'; a body that is not printed holds a construct
  its dialect lacks (for LLVM 19, the four `atomicrmw` operations of
  §9.8: about 200 of the 1500).
- LLVM's verifier rules (`tests/verify_llvm.rs`): each rule of §5.8's
  `NotDominated`, `InvalidCall` and `InvalidAttribute` rows, of §5.7's
  attribute rules and of §2.1's flag shapes, built through `hod_ir` with
  its nearest valid variant; hod's verdict, then `llvm-as` (with its
  verifier) 19 and 22 on the printed text agree. Where `hod_ir` refuses
  the construct before it can be printed (a function's signature, a
  module flag, a tail marker on `invoke`), LLVM is given the accepted
  text with the line edited.
- Initializers (`tests/print_data.rs`): every kind of initializer, both
  targets, LLVM 19 and 22: `llc`'s `.data` bytes and relocations equal
  `flatten`'s.
- Every instruction (`tests/print_bodies.rs`): every opcode, predicate,
  `RmwOp` and `TailKind` printed and accepted; the funclet opcodes are
  errors; the dialect differences of §9.2 for bodies.
- Programs (`tests/print_run.rs`): a program of many functions compiled
  by `llc` 19 and 22 at `-O0` and `-O2`, linked and run; the same
  program split (§9.6) into two objects; landing pads catching a C++
  exception.
- The module (`tests/print_module.rs`): one module with every kind of
  symbol, linkage, visibility, thread-local model, COMDAT, alias, IFUNC,
  attribute, parameter attribute, retained list, constructor, module
  flag, linker option and quoted name: accepted by LLVM 19 and 22; LLVM's
  own text (`opt -S`) has the line hod means for each; the object `llc`
  makes has each symbol's type, binding, visibility, section and size,
  the COMDAT groups, linker options, dependent libraries and string
  bytes; each module-level error of §9.6; determinism and `IoSink`.
- Speed (`tests/print_perf.rs`): a printed measurement, not a gate.

## 10. Planned extensions

Each needs a measurement before it is specified (design 0001, "Beyond LLVM
IR").

- **Call shapes from the producer.** Argument and return locations
  assigned by the frontend's ABI layer, so the target does not re-derive
  them from IR types.
- **Tables that grow during parallel code generation**, for a producer
  that streams bodies to workers while it is still lowering. The frozen
  tables and forks of §5.1 (WP-063) are the reading side of this: a
  layered table whose base is shared. A producer that lowers on several
  threads could hand its bodies over as forks' deltas the same way.
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
  verify (and which LLVM verifies, §9.9), build identically in recycled storage and not finish with a
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
- LLVM's verifier rules kept by `verify` (§5.8 `NotDominated`,
  `InvalidCall`, `InvalidAttribute`; §5.7's attribute rules; §2.1's flag
  shapes), against `llvm-as` 19 and 22, each case with its nearest valid
  variant (`tests/verify_llvm.rs`); the dominator tree against the
  definition on random graphs (`dom` module); verify's cost with and
  without uses across blocks (`examples/body_throughput.rs`).
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
- classify (§8): every row, rule and malformed-input name, the subject rule and the order (`tests/classify_matrix.rs`, `tests/classify_rules.rs`); consistency and no panic on random tame and wild programs (`tests/classify_random.rs`); `docs/status/support.md` is the generated text (`tests/classify_support_doc.rs`); the cost, not a gate (`examples/classify_throughput.rs`).

## 12. Producer kit

`hod_ir::kit` (WP-092, design 0004 H3) is the part of the mapping from
LLVM's constructs to `hod_ir` that every producer makes the same way.
`hod_ll` and `hod_bc` call it; a third producer (a front end lowering
into `hod_ir` directly) that calls it gets exactly what the readers get,
refusals included. Normative: each helper promises what is written here,
and the readers' byte equality over the corpus (WP-092's record) is
checked against it. What stays in a producer is reading its own syntax:
lexing or records, positions, its error kinds and messages for malformed
input (the kit returns an error value with LLVM's message where LLVM has
one, and the producer reports it at its own position).

The kit maps a construct as written; it applies none of the upgrades
LLVM's reader makes to IR of older versions (renamed or removed
intrinsics), except the branch-protection upgrade (`BranchUpgrade`).
The readers' byte equality is therefore between the text and the
bitcode of the same module, the bitcode written by the `llvm-as` of the
text's version (`bc.md` §9.2, WP-172).

Cost: no boxing and no allocation per call on the body path (the index
list of a `getelementptr` is the caller's scratch vector); the body-path
helpers are `#[inline]` and generic over the `Layering`. Strings are
allocated only to name a refused construct.

**Results.** `Lowered` is what a lowering made: `Built(value)`, or
`Refused { construct, stand_in }`, for which the producer records
`construct` against the function and uses `stand_in` in a body that will
not be kept. `stand_in(module, ty)` is no value for `void`, `none` for
`token`, `poison` of `ty` otherwise.

**`getelementptr`** (`Gep`, §5.4). `Gep::new(types, source, base_ty)`:
`BaseNotPointer` unless the base (or each element of a vector base) is a
pointer, then `SourceUnsized` unless `source` is sized. Per index, first
`index_type(types, ty)` (returns the width; `IndexNotInteger`,
`VectorLength` when a vector index's length differs from the base's or
an earlier index's), then `index(types, value, constant, indices)` with
the index's constant value if it is one (`gep_index_value`: an integer
sign-extended from its width, a wide integer's low word, zero, a splat,
or a vector whose elements all have one value; else `None`). The first
index scales by the source's allocation size; a struct index must be a
constant in range (else `InvalidIndices`) and adds the field's offset; an
array index scales by the element's allocation size; a vector index by
the element stride, and a vector whose elements are not whole bytes makes
the address `getelementptr:vector-element-not-bytes`; anything scalable
makes it `getelementptr:scalable`; an index into any other type is
`InvalidIndices`. Constant contributions are summed into the offset with
wrapping 64-bit arithmetic; a variable index is pushed with its scale, in
order. `build(module, b, base, indices)` returns the result type (the
base's pointer type, a vector of it if the base or an index is one) and
the `PtrAdd`, or the refusal. `inbounds`, `nuw`, `nusw` are not read.

**`alloca`** (`lower_alloca(module, b, &AllocaDesc)`, §5.2). In order:
`InvalidType` for an unsized type or `token`; the result type (`ptr`, or
`ptr addrspace(N)`); refused `alloca:inalloca`, then `alloca:swifterror`,
then `alloca:scalable`. The alignment is the written one or the type's
preferred one. A **static** `alloca` (in the entry block, address space
0, a count that is absent or an integer constant, and `count * size` not
overflowing) is a slot of that size; any other is the `Alloca`
instruction with the count (`i32 1` when absent). The count's type is the
producer's to check (an integer).

**Attributes** (§5.7). `string_attr(fa, name, value)`: a dropped string
is `Dropped`; a kept one on a function (`fa` given) goes into `fa`
(`no-builtin-<f>` keeps the name's rest) and is `Kept`; any other
(unknown, rejected, or kept on a value) is `Rejected`.
`fn_keyword(fa, name, class)` for a function keyword other than
`uwtable`, `alignstack`, `align` (whose arguments the producer reads):
a flag of `FnAttrs` is set and `Kept`, any other is
`AttrUse::of_class(class)`: `Dropped`, `Rejected`, or `Misplaced` for a
kept attribute that does not apply there (malformed). `value_keyword`
names the parameter and return keywords hod keeps: the seven
`ParamAttrs` flags, the six typed attributes, `align`, `alignstack`; any
other is `Other`, used as its class says. `add_typed(attrs, mem, ty)`:
the first typed attribute is kept; `elementtype` beside another keeps the
other and returns `attr:elementtype-with-<other>` to record; two others
are `TwoTyped` (malformed). A rejected attribute is recorded as
`attr_construct(name)` (`attr:<name>`). `merge_fn_attrs(out, view)` adds
an interned set to `out`: flags, the unwind-table kind unless none, the
stack alignment, the strings (the merge of two groups on one function).
`CALL_SITE_FLAGS` are the call-site function attributes kept (`noreturn`,
`nounwind`, `returns_twice`, `convergent`, `alwaysinline`, and
`noinline` as `CALL_SITE_NOINLINE`, a bit above every `FnAttrs` flag:
a function's `noinline` is dropped, a call site's is kept, WP-188);
`set_call_site_flags` sets `CallDesc`'s fields from them. A call site
with both `alwaysinline` and `noinline` is malformed, as in LLVM's
verifier ("'noinline and alwaysinline' are incompatible"); the printer
writes them in place after `convergent`. The text reader takes
`noinline` from a group (`AttrGroup::noinline`) or in place; the
bitcode reader from a call-site group's kind 14.

**Special globals and attachments** (§2.2). `SpecialGlobal::from_name`
(without `@`): `llvm.global_ctors`, `llvm.global_dtors`, `llvm.used`,
`llvm.compiler.used`, `llvm.global.annotations`; none of them is a
symbol. `deliver_special(module, which, init)`: a zero, `undef` or
`poison` array has no members; otherwise the initializer is an array
(`NotAnArray`); a member of the two `used` lists is a symbol's address
(`NotASymbol`, `NullUsed` for null) and is retained; a structor entry is
`{ i32, ptr }` or `{ i32, ptr, ptr }` (`BadEntry`; an all-zero entry is
skipped), its priority an integer constant (`BadPriority`, truncated to
32 bits), its function a symbol or null (skipped), its data a symbol or
null. `llvm.global.annotations` delivers nothing;
`annotations_construct(section)` is
`special-global:llvm.global.annotations` unless the section is
`llvm.metadata`. `GlobalAttachment::of(kind)`: `!dbg` is debug
information; `!type`, `!vcall_visibility`, `!prof`, `!section_prefix`,
`!annotation` are dropped; any other is recorded as
`global-metadata:<kind>`. Which other `llvm.*` variable names a producer
records as `special-global:<name>` is lexical and stays the producer's.

**Named metadata and module flags** (§2.1). `NamedMetadataKind::of(name)`:
`llvm.module.flags`, `llvm.linker.options`, `llvm.dependent-libraries`,
`llvm.dbg.cu`; `llvm.ident` and `llvm.errno.tbaa` are dropped; any other
is recorded against no symbol as `named-metadata:<name>`.
`module_flag(module, behavior, name, int, text, upgrade)`: the value is
`Int` when the operand is an integer constant, else `Str` when it is a
string, else `Other`; a behavior over `u32::MAX` becomes `u32::MAX`
(refused by `set_module_flag`); an integer flag is fed to
`BranchUpgrade::observe`. `BranchUpgrade` is LLVM's reader upgrade of
AArch64 definitions whose branch-protection flags are in the old form
(`spec/ll.md` §7.3): start from `Some(default)`, a flag value 2 ends it,
value 1 sets the flag's field; `apply(module, id, cache)` rewrites a
definition's set (`sign-return-address=none` dropped unless signing,
`"false"` values dropped and `"true"` made empty for the three boolean
strings, the flags' defaults added where the set has none), once per set.
It applies to AArch64 only, to definitions only.

**Debug fields** (§6, `spec/dwarf.md` §5.9). `di_node(kind, name,
fields)` is `DiNode::llvm_default(kind)` with each read field set in order
(`DiNode::set`'s errors); a kind `hod_ir` does not represent (`None`) is
`Other` named `name`. `define_di_node(debug, id, node, distinct)` defines
a reserved node distinct or uniqued; `define_di_tuple(debug, id,
elements)` a reserved tuple (`DiId::NONE` for `null`).
`invalid_di_ref(debug)` is what stands for a reference LLVM's verifier
refuses (a location, an expression, an assign id or a function's value
where a node is named): the empty tuple, and the producer records
`DEBUG_INVALID` (`debug:invalid`), against the function in a body, the
module otherwise. `unsupported_field(module, kind, field, default)`
records a field `hod_ir` cannot represent unless its value is the
default (what LLVM writes as no field), by `di_field`'s construct, else
`debug:<kind>.<field>`; an enumerator wider than 64 bits is
`WIDE_ENUMERATOR`, read as 0. `di_atom(group, rank)` is a location's
key-instruction atom (WP-149), `None` for a rank over 8 bits (malformed;
a group or rank over what LLVM stores is refused when the location is
defined); `add_location(debug, loc, atom, distinct)` adds a location
written in place. `compile_unit(module, unit)` lists an operand of
`!llvm.dbg.cu` that is a compile unit; `function_subprogram(module,
node)` is a function's `!dbg` when it is a subprogram (of several, the
last); `variable_expression(module, sym, node)` attaches a variable's
`!dbg` when it is a global-variable expression; anything else is
`DEBUG_INVALID`. `finish_debug(module, listed_units)` runs after the
items: nothing without debug information; else the tables are completed
and, when used (`!llvm.dbg.cu`, or a function or variable that names its
debug information), a failed `verify_debug` or a `Debug Info Version`
other than 3 is `DEBUG_INVALID`. The debug records: `DebugRecordKind`
(`of_record` for `#dbg_declare`, `#dbg_value`, `#dbg_assign`,
`#dbg_label`; `of_intrinsic` for `llvm.dbg.*`; any other intrinsic is
`debug_intrinsic_construct`, `debug:<name>`; a record LLVM knows that hod
does not keep is `unsupported_record_construct`), `variable_kind` (the
kind its variable must be), `DebugRecord` (`new(kind)`: no variable, the
empty expressions, no address), and `emit_debug_record(b, record, values,
loc)`, which hands it to the builder, except a `declare` of a `DIArgList`
(false: `DEBUG_INVALID`); an assignment's `DIArgList` is
`ASSIGN_ARG_LIST`. What stays in each producer: reading the fields
(names and `DW_*` keywords in text, operand layouts and versions in
bitcode), its error kinds and messages, and where it records.

Tests: `tests/kit.rs` (each promise above); byte equality of both
readers' objects over the corpus and the seed tests, both targets, text
and bitcode, before and after (WP-092's record).
