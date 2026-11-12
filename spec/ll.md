# spec: hod_ll — LLVM textual IR to hod_ir

Status: **current** for the lexer and the module level (WP-007) and for
function bodies (WP-008, §11, which builds on §6). Normative for `crates/hod_ll`.
Background: `docs/design/0001-input.md`; the IR it produces is
`spec/ir.md`.

## 1. What it is

`hod_ll` reads LLVM's textual IR (LLVM Language Reference, "Module
Structure" and what it refers to) and builds a `hod_ir::Module` through
`hod_ir`'s builder API. The text is the whole file in memory, as bytes;
nothing in it has to be UTF-8.

It is a producer in the sense of `spec/ir.md` §1, with three verdicts for
what it reads:

- **Delivered**: built into the module.
- **Dropped**: read and not delivered, because `spec/ir.md` says no
  consumer reads it (§3.3). Nothing else is dropped.
- **Unsupported**: read, recorded with `Module::record_unsupported` under
  a stable name (§3.4), and parsing goes on. An unsupported construct is
  never an error and never silent.

Anything that is not LLVM IR, or that `hod_ir` refuses, is an error with
a position (§8). The parser never panics on input, never loops without
consuming input and never recurses without a bound.

This document covers the lexer (§2), the module level (§3 to §5, §7) and
function bodies (§11). The module level finds each body, steps over it,
and hands it on as a byte span with the tables a body parser needs (§6);
the body parser reads it into a `hod_ir::Body`.

## 2. Lexer

`Lexer::new(source: &[u8])`, `next_token() -> Token { kind: Tok, start:
u32, end: u32 }`. A token is a kind and a byte span of the source; nothing
is copied and nothing is allocated. Names and strings are unescaped only
by whoever needs their bytes (`unescape`, §2.7). The rules are those of
LLVM's `LLLexer`, including the ones that surprise.

After `Tok::Eof` every call returns it again. `Tok::Error` is a span of at
least one byte (so a caller that goes on makes progress) and
`Lexer::error()` says why. `pos()` and `set_pos(offset)` move the lexer to
any token boundary; `text(token)` is the token's bytes.

### 2.1 Names with a sigil

| token | form |
|---|---|
| `Global`, `GlobalQuoted`, `GlobalId` | `@name`, `@"name"`, `@42` |
| `Local`, `LocalQuoted`, `LocalId` | `%name`, `%"name"`, `%42` |
| `Comdat`, `ComdatQuoted` | `$name`, `$"name"` |
| `Meta` | `!name`: `!dbg`, `!llvm.module.flags`, `!DILocation`; a `\` and what follows belong to the name |
| `Exclaim` | `!` before anything else: `!{`, `!"s"`, `!42` |
| `AttrGroup` | `#42` |
| `HashWord` | `#name`: the head of a debug record, `#dbg_value` |
| `Hash` | `#` alone |
| `SummaryId` | `^42` |

A name is a run of `[-a-zA-Z$._0-9]`; one that starts with a digit is a
number and ends at the first non-digit, so `%1x` is `%1` and `x`. A quoted
name ends at the first `"`.

### 2.2 Words and integer types

`Word` is a letter or `_` followed by `[a-zA-Z0-9_]*`: every keyword, and
every bare word of specialized metadata (`DW_TAG_member`). A word ends at
the first other character, so `a.b` is a word and a stray `.`. Two
exceptions, both LLVM's: `i` followed by digits is `IntType` even when
letters follow (`i1x` is `i1` and `x`), and `cc` followed by a digit is
the word `cc` (`cc10` is `cc` and `10`).

### 2.3 Labels

A run of label characters (`[-a-zA-Z$._0-9]`) that ends in `:` is a label
whatever else it looks like: `Label` (`entry:`, `i32:`, `-1:`, `$x:`),
`LabelId` (`42:`), `LabelQuoted` (`"a b":`). The span includes the colon.
The check costs one look at each run however many tokens the run is cut
into (a run such as `-1-1-1…` is otherwise quadratic).

### 2.4 Numbers

| token | form |
|---|---|
| `Int` | `[-]?[0-9]+`, of any length |
| `HexInt` | `u0x…`, `s0x…` |
| `Float` | `[-+]?[0-9]+[.][0-9]*([eE][-+]?[0-9]+)?`; a `+` must be followed by this form |
| `HexDouble` | `0x` and hexadecimal digits |
| `HexFp80`, `HexFp128`, `HexPpcFp128`, `HexHalf`, `HexBFloat` | `0xK…`, `0xL…`, `0xM…`, `0xH…`, `0xR…` |

A number without a point is an integer: `1e5` is `1` and the word `e5`.

### 2.5 Strings and punctuation

`Str` is `"…"` up to the first `"`, quotes included in the span; a quote
inside a string is written `\22`. `c"…"` is the word `c` and a string.
Punctuation: `=` `,` `*` `[` `]` `{` `}` `<` `>` `(` `)` `|` `:` and
`...` (`DotDotDot`).

### 2.6 White space and comments

Space, tab, line feed, carriage return and the zero byte are white space
(LLVM reads a zero byte inside a file so). `;` starts a comment that ends
at the line's end. `/* … */` is a comment and does not nest; one that
does not end is an error.

### 2.7 Escapes

`unescape(raw, out)` appends what the contents of a string or quoted name
stand for: `\\` is a backslash, `\xx` with two hexadecimal digits is that
byte, any other backslash is itself.

## 3. Accepted input

### 3.1 Top-level entities

| entity | what becomes of it |
|---|---|
| `target triple = "…"` | the module's `TargetSpec` (`spec/ir.md` §2.1); without one, `ParseOptions::default_target` |
| `target datalayout = "…"` | checked against the target's (`TargetSpec::check_data_layout`) |
| `source_filename = "…"` | dropped |
| `module asm "…"` | `append_module_asm`, in file order |
| `%name = type …`, `%42 = type …` | §5.2 |
| `$name = comdat kind` | `comdat_named`, `set_comdat_kind` |
| `@name = … global/constant …` | a variable, §5.7; or one of the special arrays, §5.9 |
| `@name = … alias …`, `… ifunc …` | §5.8 |
| `declare …`, `define … { … }` | a function symbol, §5.8; the body is not read, §6 |
| `attributes #N = { … }` | an attribute group, §5.6 |
| `!name = !{…}` | named metadata, §7 |
| `!N = [distinct] !{…}`, `!N = [distinct] !Kind(…)` | a metadata node, §7 |
| `uselistorder …`, `uselistorder_bb …` | dropped, unread |
| `^N = …` | a module summary entry: dropped, unread |

Entities may come in any order, and anything may be used before it is
defined, as in LLVM. Line ends mean nothing.

### 3.2 Dialects

`hod_ll` reads one dialect: the union of what LLVM 19 through LLVM 22
read. There is no version switch; a file is not asked which LLVM wrote
it. Where the versions differ:

| construct | LLVM 19 | LLVM 22 | hod_ll |
|---|---|---|---|
| `/* … */` comments | no | yes | read |
| `ptrtoaddr` constant expression | no | yes | read |
| `captures(…)`, `dead_on_return`, `nodivergencesource`, `sanitize_realtime`, `sanitize_realtime_blocking`, `sanitize_alloc_token`, `sanitize_type` | no | yes | read; classed by `hod_ir`'s attribute table |
| `mul` constant expression | yes | no | read |
| `x86_mmx` type | yes | no | read as `<1 x i64>`, which is what LLVM 20 and later make of it, and recorded (`type:x86_mmx`) |
| a struct that contains itself, `%T = type { %T }` | accepted | refused | refused |
| an initializer of an opaque type | accepted | refused | refused |
| a wrong signature on an intrinsic declaration | verifier refuses | accepted | accepted |
| AArch64 branch-protection attributes implied by module flags | as written | upgraded on reading | upgraded, §7.3 |
| `i1` global alignment on AArch64 | 1 | 4, that of `i8` | `hod_ir`'s data layout: LLVM 22's |

Both versions still read the pointer types of LLVM 14 and before, `T*`
and `T addrspace(N)*`, as `ptr`; so does `hod_ll`, with LLVM's check that
`T` is not `void`, `label`, `metadata` or `token`. A named type may hold
a pointer to itself written `%T*`; the other old pointers to a type under
definition (`[2 x %T]*`) are refused, where LLVM accepts them.

Both versions refuse `shl` and every other constant expression LLVM has
removed; so does `hod_ll`. The constant expressions read are in §5.5.

### 3.3 Dropped

Read and not delivered, each because `spec/ir.md` says so:

- `source_filename`; comments (including `; ModuleID`); `uselistorder`
  and `uselistorder_bb`; module summary entries.
- `!llvm.ident`, `!llvm.errno.tbaa`.
- On a global value: `dso_preemptable`, `default` visibility (they are
  the defaults); `externally_initialized`; `no_sanitize_address`,
  `no_sanitize_hwaddress`, `sanitize_address_dyninit`; the attachments
  `!type`, `!vcall_visibility`, `!prof`, `!section_prefix`, `!annotation`
  (`spec/ir.md` §2.2).
- Every attribute `hod_ir`'s table classes as dropped (`spec/ir.md`
  §5.7), with its arguments.
- In constant expressions: `inbounds`, `nuw`, `nusw` and `inrange(…)` on
  `getelementptr`, `nuw` and `nsw` on `add`, `sub`, `mul`: they change
  no constant's value.
- The names of struct types (`hod_ir` types are structural), the
  parameters of a target extension type (`hod_ir` keeps its name), and
  the names of function parameters in a header (§6).

### 3.4 Unsupported, by name

Each is recorded with `record_unsupported(name, symbol)`: against the
symbol it belongs to, or against the module (`None`) where the table says
"module". Parsing continues; what stands in for the construct, if
anything has to, is given.

| name | construct | stands in |
|---|---|---|
| `attr:<name>` | an attribute `hod_ir`'s table rejects or does not have, keyword or string; on a function, a parameter, a return value, a variable or through a group | nothing |
| `attr:elementtype-with-<other>` | `elementtype` beside `sret`, `byval`, `byref`, `inalloca` or `preallocated` | the other one |
| `dll-storage` | `dllimport`, `dllexport` | nothing |
| `partition` | `partition "…"` | nothing |
| `gc` | `gc "…"` | nothing |
| `prefix-data`, `prologue-data` | `prefix`, `prologue` on a function | nothing |
| `personality-not-a-symbol` | a personality that is not the address of a symbol | no personality |
| `global-code-model` | `code_model "…"` on a variable | nothing |
| `global-sanitize-memtag` | `sanitize_memtag` on a variable | nothing |
| `global-metadata:<kind>` | an attachment on a global value other than `!dbg` and the dropped ones | nothing |
| `global-attributes` | kept attributes on a variable, in place or through a group | nothing |
| `special-global:<name>` | a variable named `llvm.…` other than the four arrays of §5.9 (`llvm.global.annotations`) | the variable, as written |
| `named-metadata:<name>` | module: named metadata other than those of §7 | nothing |
| `debug-info` | module: `!llvm.dbg.cu` is present | the index, §7 |
| `type:ppc_fp128` | the type | `fp128` |
| `type:x86_amx` | the type | a target extension type named `x86_amx` |
| `type:x86_mmx` | the type (§3.2) | `<1 x i64>` |
| `const-expr:ptrauth` | `ptrauth (…)` | its pointer operand |
| `blockaddress:unlabelled-block` | a `blockaddress` of a number in a function whose unnamed values the scan of §5.5 cannot count (hand-written text with two statements on a line) | a null pointer |
| `symbol:anonymous-external` | a numbered global that is not `internal` or `private` (§5.7) | a symbol named `__unnamed_K` |
| `syncscope:<name>` | body: a `syncscope("…")` other than `singlethread` (§11.5) | the system scope |
| `alloca:inalloca`, `alloca:swifterror`, `alloca:scalable` | body: such an `alloca` | the `alloca` without it |
| `getelementptr:scalable`, `getelementptr:vector-element-not-bytes` | body: a step of a `getelementptr` with no fixed size in bytes | poison |
| `aggregate:no-fixed-offset`, `insertvalue:offset-over-4GiB` | body: an `extractvalue` or `insertvalue` of a field without a fixed offset, or past 4 GiB | poison |
| `inst:catchswitch`, `inst:catchpad`, `inst:cleanuppad`, `inst:catchret`, `inst:cleanupret` | body: funclet exception handling (§11.4) | the opcode alone |
| `metadata-arg:<callee>`, `metadata-arg:indirect` | body: a call that would be emitted with a `metadata` argument (§11.6) | poison |
| `bundle:metadata-input` | body: a `metadata` operand of an operand bundle | nothing |
| `invoke:no-code-intrinsic` | body: an `invoke` or `callbr` of an intrinsic that generates no code (§11.7) | a branch to the normal destination |
| `no-code-cycle` | body: no-code calls each of which stands for the next (§11.7) | nothing |

A construct met several times in one top-level entity is recorded once
for it. A body construct is recorded against the function, and the body
that has one is not delivered (§11.1).

### 3.5 What is not checked

`hod_ll` is not a verifier. Of what LLVM's parser or verifier refuse, it
accepts:

- At the module level, anything inside a function body (§6), a
  `uselistorder` or a summary entry: stepped over unread. What the body
  parser does not check is in §11.8.
- Metadata that the module level does not read (§7): the operands of a
  node, references from one node to another, the fields of specialized
  nodes.
- The arguments of dropped and rejected attributes
  (`dereferenceable(0)`, `memory(bogus)`), and where such an attribute
  stands (a parameter attribute on a function): `hod_ir`'s table has
  neither. A kept attribute in the wrong place is an error.
- What `spec/ir.md` §2.2 lists as not checked (a personality on a
  declaration, the fit of parameter attributes to parameter types), the
  signatures of intrinsics, and that a `dso_local_equivalent` names a
  function.
- A `getelementptr` struct index of an integer type other than `i32`;
  `%U = type %T` where `%T` is a struct (LLVM leaves `%U` undefined); a
  `blockaddress` of a numbered block after the function's definition.

And it refuses what LLVM accepts where `hod_ir` has no place for it: a
target other than AArch64 or x86-64 Linux, a data layout that is not the
target's, a symbol with an empty name (`@"" = …`), a calling convention
above 1023, a PIC level above 2, an attribute group number above
2^32 - 1. `tests/syntax` holds each of these differences as a row with
its reason, checked against `llvm-as` 19 and 22.

## 4. Parsing

`parse_module(source, &ParseOptions) -> Result<ParsedModule, LlError>`
works in two steps: an index scan that finds every top-level entity
without reading it (§4.1, §4.2), then phases over the index, each reading
the entities of one kind (§4.3). Forward references are the reason:
attribute groups and metadata come after the functions that use them,
types and globals are used before they are defined. With the index, each
phase finds what it depends on already built, nothing is read twice, and
function bodies are never lexed at all. §9 has the measurements.

### 4.1 The index scan

One pass over the source. An entity begins at a name followed by `=`
(`@g =`, `%T =`, `$c =`, `!name =`, `!7 =`, `^3 =`) or at one of the
keywords `define`, `declare`, `target triple`, `target datalayout`,
`source_filename`, `module`, `attributes`, `uselistorder`,
`uselistorder_bb`, and ends where the next one begins. Tokens outside
brackets are lexed; a group in `( )`, `[ ]` or `{ }` is stepped over
(§4.2). The scan records, per kind and in file order: each type
definition's name and extent; each COMDAT's name and kind; each attribute
group's number and the inside of its braces; each global value's name,
extent and, for a definition, the insides of its parameter parentheses
and of its body braces; each metadata node's number, kind, `distinct`
mark and inside; each named metadata's name and inside; the triple, the
data layout and the `module asm` strings.

The `{` of a function body is the first one outside brackets after the
parameter list that does not belong to the typed constant of a `prefix`,
`prologue` or `personality`.

`scan_only(source) -> Result<(usize, usize), LlError>` runs the scan
alone and returns the number of entities and the bytes of bodies: for
measurements.

### 4.2 Skipping

Between an opening bracket and its partner only five bytes matter: the
bracket pair itself, and the three that start a region in which brackets
do not count: `"` (a string or a quoted name, to the next `"`), `;` (a
comment, to the line's end) and `/` when `*` follows (to `*/`). The
skipper looks for these eight bytes of source at a time, with word
arithmetic, and counts only the one bracket pair it was called for; every
other pair is balanced inside it in well-formed text. A group, string or
comment that does not end is an error at its opening.

This is exact for LLVM text: a string has no way to contain a `"`, and a
`}` in a string, a quoted name or a comment inside a body does not end
the body.

### 4.3 Phases

In this order, each over its entries of the index:

1. Target: the triple (or the default), then the data layout check.
2. `module asm`.
3. Type definitions (§5.2).
4. COMDATs.
5. Attribute groups (§5.6).
6. The metadata index, then named metadata (§7).
7. Numbered globals get their symbols (§5.7).
8. Global values in file order: variables, aliases, IFUNCs and function
   headers (§5.7, §5.8), with their constants (§5.3).
9. The end checks: a global that was used and never defined is an error
   at its first use; then `Module::verify_symbols`.

## 5. Entities

### 5.1 Types

Every type of `spec/ir.md` §3: `void`, `iN`, `half`, `bfloat`, `float`,
`double`, `x86_fp80`, `fp128`, `ptr`, `ptr addrspace(N)`, `label`,
`metadata`, `token`, `[N x T]`, `<N x T>`, `<vscale x N x T>`, `{ … }`,
`<{ … }>`, function types `R (A, B, ...)`, `target("name", …)`, `opaque`
(in a definition), and named types. What a type may be made of is
`hod_ir`'s to refuse, and the refusal is an error at the type.

### 5.2 Named types

`%name = type …` and `%42 = type …`, numbered ones in any order. A name
defined twice is an error. Definitions are resolved in an order in which
each comes after the types it holds, wherever they stand in the file; the
walk that finds the order keeps its own stack, so a chain of types each
holding the next may be as long as the file. A type that holds itself, at
any distance, is an error at the name that closes the circle.

`hod_ir` types are structural: two named struct types with the same
fields are one `TypeId`, and the name is not kept. Each `opaque` is a
type of its own. A name may stand for any type (`%I = type i32`).

### 5.3 Constants

A constant is read with the type its context gives and built through
`hod_ir`'s constant pool: integers of any width (decimal, negative,
`u0x…`, `s0x…`; a decimal is taken modulo 2^N as LLVM takes it), `true`
and `false` (of type `i1` only), floating-point literals (§5.4), `null`,
`zeroinitializer`, `undef`, `poison`, `none` (of type `token`), `c"…"`,
`[ … ]`, `{ … }`, `<{ … }>`, `< … >`, `splat (T v)`, the address of a
global (§5.5) and constant expressions (§5.5). The bracket form must be
the one the type calls for, and a struct constant packed exactly when its
type is. Everything else about a constant's fit (element count, element
types, integer range of `iN`) is `hod_ir`'s to refuse.

### 5.4 Floating-point literals

By LLVM's rules:

- A decimal literal is the nearest `double`. It may be given to `double`,
  and to `float`, `half` or `bfloat` if the value is exactly one of
  theirs (`float 0.1` is an error); never to a wider type.
- `0x` and hexadecimal digits are the bits of a `double`, with any number
  of leading zeros, under the same rule.
- `0xK` is an `x86_fp80`: up to 20 digits, sign and exponent first. LLVM
  reads the pattern as a value and writes the value back, which changes
  the two kinds of pattern that are not values; `hod_ll` gives the same
  bits.
- `0xL` is an `fp128` and `0xM` a `ppc_fp128`: up to 32 digits, the low
  half first. `0xH` is a `half` and `0xR` a `bfloat`: the low 16 bits.
- A NaN keeps its sign, its payload and whether it signals when it
  narrows; a payload that does not fit is an error.

### 5.5 Addresses and constant expressions

`@name` as a constant is `Addr { sym, 0 }`; in a pointer type of another
address space it is that address cast there. The constant expressions
read are `getelementptr`, `ptrtoint`, `ptrtoaddr`, `inttoptr`, `bitcast`,
`addrspacecast`, `trunc`, `add`, `sub`, `mul`, `xor`, `extractelement`,
`insertelement`, `shufflevector`, `blockaddress`, `dso_local_equivalent`,
`no_cfi` and `ptrauth` (§3.4). Each goes through `hod_ir`'s folding
function (`fold_gep`, `fold_cast`, `fold_binary`; `spec/ir.md` §4.2), so
an address always arrives as symbol plus addend and `hod_ll` holds no
arithmetic of its own. An expression's type must be the type its context
gives.

`blockaddress(@f, %b)` is `block_addr(f, BlockId)`, where the `BlockId`
is the block's position in the function's text: the entry block is 0, and
a block's number is the number of terminator instructions before its
label (`ret`, `br`, `switch`, `indirectbr`, `invoke`, `resume`,
`unreachable`, `cleanupret`, `catchret`, `catchswitch`, `callbr`: each
block has exactly one, and these words are nothing else outside
parentheses in a body). The function must be defined in the module. The
labels of a function are read at the first `blockaddress` that names it,
once.

A block without a label in the text takes the next number of the
function's unnamed values, as in LLVM, and the same scan counts them: the
unnamed parameters (a parameter is named when it ends in a local name
that is not its only token), then, in text order, the entry block and
every other block without a label, a written number (`%N =`, `N:`, a
parameter `%N`) from which the count continues, and each unassigned
instruction with a result (every instruction but `store`, `fence`, the
terminators without a result and a call of a `void` type). The scan reads
one statement per line, outside brackets; a line that starts with `to`,
`unwind`, `catch`, `filter` or `cleanup` continues the one before. When it
cannot be sure of the count it gives up, and a number it has not counted
is recorded (`blockaddress:unlabelled-block`): an instruction word inside
a statement that does not start a constant expression (a second
statement on the line), a written number below the count, a line it
cannot read. A name, or a counted number, that is no block of the
function is `Undefined` at the label, as `llvm-as` refuses it ("referenced
value is not a basic block", "use of undefined value"). Tested against the
blocks that `llvm-as | llvm-dis` names (§10).

### 5.6 Attributes

Every attribute goes through `hod_ir`'s table (`attr_info`, `spec/ir.md`
§5.7): a kept one is stored (`ParamAttrs`, `FnAttrs`), a dropped one is
stepped over with its arguments, a rejected one is recorded as
`attr:<name>`. A string attribute that the table does not have is
rejected like any other; a bare word that the table does not have is not
an attribute, and ends the list.

- Parameter and return attributes: the flags and typed attributes of
  `ParamAttrs`, `align N`, `align(N)`, `alignstack(N)`.
- Function attributes: the flags of `FnAttrs`, `uwtable`, `uwtable(sync)`,
  `uwtable(async)`, `alignstack(N)`, the strings of `FnStr`; in a group
  also `align=N` and `alignstack=N`.
- An attribute group, `attributes #N = { … }`, holds function attributes
  and is interned once (`intern_fn_attrs`). It may not be empty. Of two
  definitions of one number the later one is the group, as in LLVM. A
  group that is used and never defined is empty.
- A function's set is its own attributes with those of its groups laid
  over them in the order written (a group's `"frame-pointer"` replaces
  the function's own). A group's `align` is the function's alignment and
  wins over `align N` in the header.
- The rejected attributes of a group are recorded against each function
  that names the group.

### 5.7 Variables

`@name = [linkage] [preemption] [visibility] [dll storage]
[thread_local[(model)]] [unnamed_addr] [addrspace(N)]
[externally_initialized] global|constant T [init] {, property} {attribute}`,
with every field of `VariableDesc`. The properties are `section "…"`,
`comdat`, `comdat($name)`, `align N`, `partition "…"`, `code_model "…"`,
the sanitizer marks and metadata attachments; a `!dbg` attachment is kept
for §6. A variable with a written `external` or `extern_weak` linkage has
no initializer; any other has one.

**Numbered globals.** `@42 = …`, `declare void @42()` and
`define void @42() { … }` take their symbols before anything else is
read, so that a use may come first. The numbers must ascend through the
file, as LLVM requires. One that is `internal` or `private` is
`Module::anonymous_symbol`. Any other needs a name in the object:
`hod_ir` has no anonymous symbol that is not local, so it is named
`__unnamed_K`, `K` counting the module's numbered globals from 1 in file
order, and recorded (`symbol:anonymous-external`), because LLVM's own `K`
follows the order in which it emits them.

**Order.** Symbols are numbered in LLVM's module order (`ir.md` §2.2):
every variable, then every function, then every alias, then every IFUNC,
each kind in the order of its definitions and declarations in the text.
Which global refers to which first does not matter: LLVM keeps one list
per kind and appends a global to its list where it is defined or
declared, and `llvm-dis` prints, the bitcode writer numbers, and `llc`
lays out `.text` and `.data` in the order of those lists. All symbols are
created by this one pass over the index (§4.3) before anything else is
read. The rule is the same as `bc.md` §5.5's record order, so the text
and bitcode of one module give the same `SymId`s; `hod_bc`'s differential
test checks it (`symbol order`), and `tests/syntax`
(`symbols_are_numbered_in_llvms_module_order`) checks it against
`llvm-dis` 19 and 22.

### 5.8 Functions, aliases, IFUNCs

`declare|define [linkage] [preemption] [visibility] [dll storage] [cc]
{return attribute} T @name(parameters) [unnamed_addr] [addrspace(N)]
{function attribute | #N} [section "…"] [partition "…"] [comdat[($name)]]
[align N] [gc "…"] [prefix T c] [prologue T c] [personality T c]
{!kind !N}`, with every field of `FunctionDesc`. A calling convention is a
keyword (`call_conv_keyword`) or `cc N`. A parameter is `T {attribute}
[%name]`; the list may end in `...`. Trailing parameters without kept
attributes are not stored. `subprogram` is not set: the `!dbg` attachment
is kept for §6 until `hod_ir` has debug nodes. A definition is delivered
with `has_body: true` and its text recorded (§6).

`@name = [linkage] … alias T, target` and `… ifunc T, resolver`, with
every field of `AliasDesc` and `IFuncDesc`. The target is a typed
constant; LLVM writes it without its type when it begins with
`getelementptr`, `bitcast`, `addrspacecast` or `inttoptr`, and it is read
so. An IFUNC's resolver is the address of a symbol.

A name described twice is an error at the second. The rules between
symbols (alias targets, resolvers, declarations' linkage) are
`verify_symbols`'s.

### 5.9 The special arrays

Not symbols (`spec/ir.md` §2.2):

- `@llvm.global_ctors`, `@llvm.global_dtors`: each entry `{ i32
  priority, ptr function [, ptr data] }` is `add_ctor` / `add_dtor`. An
  entry whose function is `null`, or that is `zeroinitializer`, is
  skipped.
- `@llvm.used`, `@llvm.compiler.used`: each member is `retain(sym,
  Retained::Used | CompilerUsed)`; a member that is not the address of a
  symbol is an error.

An array that is `zeroinitializer` or `undef` is empty. Each may appear
once. Any other variable named `llvm.…` is delivered as an ordinary
variable and recorded (`special-global:<name>`).

## 6. The interface to a body parser

`parse_module` returns

```rust
pub struct ParsedModule<'a> {
    pub module: hod_ir::Module,
    pub text: ModuleText<'a>,
}
impl<'a> Deref for ParsedModule<'a> { type Target = ModuleText<'a>; }

impl<'a> ModuleText<'a> {
    pub fn source(&self) -> &'a [u8];
    pub fn functions(&self) -> &[FunctionText];
    pub fn named_type(&self, name: &[u8]) -> Option<TypeId>;
    pub fn numbered_type(&self, number: u32) -> Option<TypeId>;
    pub fn numbered_global(&self, number: u32) -> Option<SymId>;
    pub fn attr_group(&self, id: u32) -> Option<&AttrGroup>;
    pub fn attr_groups(&self) -> &[AttrGroup];
    pub fn metadata(&self) -> &MetadataIndex;
    pub fn symbol_dbg(&self) -> &[(SymId, MdRef)];
}

pub struct FunctionText {
    pub sym: SymId,
    pub header: Span,          // after `define`, up to the `{`
    pub params: Span,          // inside the parameter parentheses
    pub body: Span,            // inside the body braces
    pub dbg: Option<MdRef>,    // the definition's `!dbg`
}
pub struct AttrGroup {
    pub id: u32,
    pub attrs: FnAttrsId,
    pub align: Option<u64>,
    pub rejected: Vec<Box<str>>,
}
pub struct Span { pub start: u32, pub end: u32 }
impl Span { pub fn of<'a>(&self, source: &'a [u8]) -> &'a [u8]; }
```

`ModuleText` is read-only, and it and `ParsedModule` are `Send` and
`Sync`. The two fields are separate so that a caller can do either of the
things `spec/ir.md` §5.1 allows: share `&module` and `&text` among
workers that parse bodies in parallel against frozen tables, or keep
`&mut module` (to add the types, constants and symbols a body mentions)
while any number of readers hold `&text`.

What a body parser finds there, without scanning the file again:

- **Where the text is.** `functions()` lists the definitions in file
  order; each `body` span is exactly what stands between the braces, and
  a `Lexer` set to `body.start` reads it. Each function's symbol is
  already described (`has_body: true`); declarations, variables, aliases
  and IFUNCs are complete. A body is an independent unit of work.
- **Parameter names.** The module level does not keep them. `params` is
  the text of the parameter list; the body parser reads `T {attribute}
  [%name]` from it to name the arguments (unnamed ones take the numbers
  from 0, and the entry block the next number, as in LLVM). Types and
  attributes are already in the symbol.
- **Named types**: `named_type` (by unescaped name) and `numbered_type`.
- **Globals**: `Module::find_symbol` by unescaped name; `numbered_global`
  for `@42`. A global used in a body is always a top-level entity, so the
  lookup cannot miss in well-formed text.
- **Attribute groups** for call sites: `attr_group(N)` gives the
  interned kept set, the group's alignment and the names of its rejected
  attributes, which whoever applies the group records as `attr:<name>`
  against the function.
- **Metadata**: `metadata()` is the index of §7. `MdRef` is a node by
  number (`Node(id)`) or written in place (`Inline(kind, span)`).
  `symbol_dbg()` lists the `!dbg` attachments of global values in file
  order, for the work package that delivers debug nodes.
- **Block numbers.** A body parser must number a function's blocks in the
  order of the text, the entry block 0: that is the `BlockId` a
  `blockaddress` constant already holds (§5.5).

`call_conv_keyword(word) -> Option<u32>` maps a calling-convention
keyword to LLVM's number, for call sites.

## 7. Metadata

The module level indexes all metadata and reads little of it.

```rust
impl MetadataIndex {
    pub fn node(&self, id: u32) -> Option<&MdNode>;
    pub fn nodes(&self) -> &[MdNode];            // file order
    pub fn named(&self) -> &[NamedMetadata];     // file order
    pub fn has_debug_info(&self) -> bool;        // `!llvm.dbg.cu` exists
}
pub struct MdNode { pub id: u32, pub kind: MdKind, pub distinct: bool, pub span: Span }
pub struct NamedMetadata { pub name: Span, pub span: Span }
pub enum MdRef { Node(u32), Inline(MdKind, Span) }
```

A node's `span` is the source between its braces or parentheses, and
`kind` is `Tuple` for `!{…}` or the specialized kind named after the `!`
(`MdKind::from_name`; `Other` for a name the list does not have). Node
numbers may come in any order; a number defined twice is an error. A
node is found by number in constant time when the numbers are dense, as
they are in anything a compiler wrote. Nothing inside a node is read
unless the list below says so; a later work package reads nodes from
their spans.

Named metadata:

| name | what becomes of it |
|---|---|
| `!llvm.module.flags` | §7.1 |
| `!llvm.linker.options`, `!llvm.dependent-libraries` | §7.2 |
| `!llvm.ident`, `!llvm.errno.tbaa` | dropped |
| `!llvm.dbg.cu` | `has_debug_info()`, and `debug-info` is recorded: debug nodes are indexed, not delivered |
| any other | recorded, `named-metadata:<name>` |

An attachment on a global value is `!kind` and a node, by number (which
must be defined) or in place: §3.3 and §3.4 say what becomes of each
kind.

### 7.1 Module flags

Each operand of `!llvm.module.flags` is a tuple `!{i32 behavior,
!"name", value}` and becomes `Module::set_module_flag(behavior, name,
value)`: an integer constant is `FlagValue::Int`, a metadata string
`FlagValue::Str`, anything else `FlagValue::Other`. What a flag means,
and which are refused, is `spec/ir.md` §2.1.

### 7.2 Linker options and dependent libraries

Each operand of `!llvm.linker.options` is a tuple of strings and becomes
`add_linker_option`; each operand of `!llvm.dependent-libraries` is a
tuple of one string and becomes `add_dependent_library`.

### 7.3 The branch-protection upgrade

LLVM 22's reader rewrites the function attributes of an AArch64 module
whose branch-protection module flags are in the form LLVM 19 wrote, so
that each function carries its own policy. `hod_ll` does the same to
every definition of an AArch64 module, whichever LLVM wrote it, so that
one text means one thing. The six flags are `branch-target-enforcement`,
`branch-protection-pauth-lr`, `guarded-control-stack`,
`sign-return-address`, `sign-return-address-all` and
`sign-return-address-with-bkey`. If any of them has the value 2 the
module is in the current form and nothing is changed. Otherwise, on each
definition:

- If `sign-return-address` is 1, a function without the
  `"sign-return-address"` attribute gets it (`"all"` if
  `sign-return-address-all` is 1, else `"non-leaf"`), and one without
  `"sign-return-address-key"` gets that (`"b_key"` if
  `sign-return-address-with-bkey` is 1, else `"a_key"`). If it is not 1,
  `"sign-return-address"="none"` is removed together with the key.
- For each of `"branch-target-enforcement"`,
  `"branch-protection-pauth-lr"` and `"guarded-control-stack"`: the
  value `"false"` removes the attribute, `"true"` leaves it without a
  value, and a function without it gets it, without a value, if the
  module flag is 1.

Declarations are not changed, and no other target is.

## 8. Errors and limits

```rust
pub struct LlError { pub kind: LlErrorKind, pub offset: usize, pub line: u32, pub column: u32, pub message: String }
#[non_exhaustive]
pub enum LlErrorKind { Lex, Syntax, Undefined, Redefined, Type, Limit, Target, Ir(hod_ir::IrErrorKind) }
```

`offset` is the byte offset of the token the failure was met at; `line`
and `column` are those of the offset, from 1, the column in bytes.
`Display` is `line:column: message`. The first failure ends the parse;
there is no recovery.

| kind | what |
|---|---|
| `Lex` | bytes that are no token: a stray character, a string, quoted name or `/* */` comment that does not end, a malformed number |
| `Syntax` | a token the grammar does not allow where it stands |
| `Undefined` | a use of a type, COMDAT, metadata node or global that the module does not define; a `blockaddress` in a function that is only declared |
| `Redefined` | a second definition of a type, COMDAT, metadata node or global value |
| `Type` | a value of the wrong type for its place, a literal its type cannot hold, a type that contains itself |
| `Limit` | one of the limits below |
| `Target` | the module names no target and the caller gave none |
| `Ir(kind)` | `hod_ir` refused what the text describes; the message is `hod_ir`'s, the position that of the entity or operand it refused |

Limits (`hod_ll::limits`):

| limit | value | why |
|---|---|---|
| `MAX_SOURCE_BYTES` | 2^32 - 1 | offsets are 32 bits |
| `MAX_NESTING` | 200 | types and constants are read by recursive descent; each aggregate type, function type, aggregate constant and constant expression inside another counts one |
| `MAX_INT_LITERAL_DIGITS` | 65,536 | a decimal literal is converted in time quadratic in its digits |

Named types do not count toward `MAX_NESTING` (§5.2), and nothing else
recurses: brackets in text that is stepped over, metadata, attribute
arguments and bodies are not nested calls.

For every input, of any bytes: the lexer and the parser return; they do
not panic; time is linear in the input apart from the integer literals
above and `hod_ir`'s own costs; memory beyond the module is proportional
to the number of top-level entities. The module itself can be larger
than the text in one way, which is LLVM's too: an integer constant of a
wide type takes its type's size, so `i8388607 1` is a megabyte.

## 9. Cost

Measured by `tests/perf.rs` (printed, not gated; `cargo test --release -p
hod_ll --test perf -- --nocapture`), optimized build, best of five, on
the shared development host. The clang texts are `tests/data/stl.cpp`
compiled for x86-64 at `-O0`; the synthetic one is 2000 functions shaped
like clang's C++ output.

| text | size | bodies | lexing every token | index scan | module-level parse | stepping over bodies | peak memory | `llvm-as` |
|---|---|---|---|---|---|---|---|---|
| clang 22 | 3.65 MiB | 75% | 649 MiB/s, 1.47 ns/byte | 2296 MiB/s, 0.42 ns/byte | 827 MiB/s, 1.15 ns/byte (4.4 ms) | 4954 MiB/s | 1.3 MiB | 103 ms |
| clang 22 `-g` | 9.68 MiB | 35% | 602 MiB/s, 1.58 ns/byte | 1248 MiB/s, 0.76 ns/byte | 892 MiB/s, 1.07 ns/byte (10.9 ms) | 5368 MiB/s | 3.3 MiB | 298 ms |
| clang 19 | 3.59 MiB | 74% | 652 MiB/s, 1.46 ns/byte | 2259 MiB/s, 0.42 ns/byte | 832 MiB/s, 1.15 ns/byte (4.3 ms) | 4886 MiB/s | 1.3 MiB | 100 ms |
| clang 19 `-g` | 9.52 MiB | 35% | 580 MiB/s, 1.64 ns/byte | 1232 MiB/s, 0.77 ns/byte | 862 MiB/s, 1.11 ns/byte (11.1 ms) | 5319 MiB/s | 3.3 MiB | 284 ms |
| synthetic | 18.9 MiB | 96% | 591 MiB/s, 1.61 ns/byte | 5110 MiB/s, 0.19 ns/byte | 2957 MiB/s, 0.32 ns/byte (6.4 ms) | 6056 MiB/s | 1.2 MiB | |
| synthetic, `-g` | 20.6 MiB | 94% | 555 MiB/s, 1.72 ns/byte | 4755 MiB/s, 0.20 ns/byte | 2894 MiB/s, 0.33 ns/byte (7.1 ms) | 6121 MiB/s | 1.3 MiB | |

"Lexing every token" is the lexer run over the whole text and nothing
else. "Stepping over bodies" is §4.2 on the bodies alone, concatenated.
"Peak memory" is how far the resident memory of a process rises during
`parse_module`, the text itself not counted: 0.34 to 0.37 bytes per byte
of clang's text, 0.06 to 0.07 of the synthetic one. `llvm-as` is the wall
time of the process on the same text; it also parses the bodies, verifies
the module and writes bitcode, so it is the cost of the reader `hod_ll`
replaces a part of, not a like-for-like figure.

Why two steps. A single left-to-right pass has to lex every token of the
file to find where entities end, which alone costs more (5.6 ms for the
3.65 MiB text) than the whole two-step parse (4.4 ms), and it still meets
attribute groups and metadata after the functions that need them. The
index scan lexes only the tokens outside brackets and steps over the rest
at about 5 GiB/s; it takes 1.6 ms of those 4.4 ms. The tokens the phases
then lex are the ones outside bodies (1.0 ms for that text, measured as
"lexing outside bodies"); the remaining 1.8 ms is building 2351 symbols
with their types, constants and attributes, so the module level is
bounded by the builder and not by the lexer. With debug information the
scan is slower per byte because most of the text is 54,000 small
entities, and the parse then leaves them unread.

Stepping over a body costs 0.16 to 0.20 ns per byte and is what lets a
body be handed to another thread unread.

## 10. Tests

| section | test | oracle |
|---|---|---|
| §2.1 to §2.7 | `src/lexer.rs` unit tests: every token form and the edge cases (`\22` in names, `-0.0`, `0xH`, `i8388607`, `%1x`, `cc10`, labels) | the rules of `LLLexer` |
| §4.2 | `src/skip.rs` unit tests: nesting, strings, quoted names, both comment forms, every alignment of the special bytes against a byte-at-a-time scan | |
| §5.2 | `src/names.rs` unit test | |
| §5.4 | `src/float.rs` unit tests | LLVM's `APFloat` results, as values |
| §10 (acceptance) | `differential::accepts_everything_llvm_accepts` | everything `llvm-as` 19 and 22 accept in the corpus parses, as written and as `llvm-dis` prints it; every unsupported construct in clang's output is in a list by name |
| §5.3 to §5.9, §7.2 | `differential::objects::symbols_and_initializers_match_llc_objects` | `llc`'s object for the same text: every variable's bytes and relocations against `ConstPool::flatten`; every symbol's name, kind, binding, visibility, section, COMDAT, alignment, size and TLS-ness; aliases and IFUNCs; `.init_array` and `.fini_array`; `.linker-options` and `.deplibs` |
| (the object reader) | `differential::objects::the_object_reader_agrees_with_llvm_readelf` | `llvm-readelf` |
| §5.6, §5.8, §7.3 | `differential::canonical::function_headers_match_llvm_canonical_text` | `llvm-as \| llvm-dis` of the newest LLVM: linkage, visibility, `dso_local`, calling convention, return and parameter attributes, the merged and upgraded attribute sets, section, COMDAT, alignment, personality |
| §4, §5, §7 | `differential::canonical::the_canonical_text_delivers_the_same_module` | the module as written and as `llvm-dis` prints it deliver the same symbols, images and properties |
| §4.2, §6 | `differential::canonical::body_spans_are_exactly_the_bodies` | a line scan of text that has one header per line |
| §6, §7 | `differential::canonical::the_body_parser_interface_matches_the_text` | line scans of the canonical text: named types, groups, every metadata node's number, kind and span, `!dbg` attachments |
| §6 | `differential::canonical::bodies_are_read_from_several_threads_while_the_module_grows` | one reader |
| §7.1 | `differential::flags::module_properties_match_the_clang_options` | a table of what each clang option means |
| §3, §8 | `syntax::verdicts_and_error_positions` | a table of some 490 snippets: accepted, or the error's kind, line and column |
| §3.2, §3.5 | `syntax::llvm_as_agrees_or_the_difference_is_named` | `llvm-as` 19 and 22 on each snippet: the verdicts agree or the row says why not |
| §3.4 | `syntax::unsupported_constructs_are_recorded_by_name` | the names, per snippet |
| §5.5 | `syntax::blockaddress_of_a_block_without_a_label_is_llvms` | the blocks of `blockaddress` constants of blocks without labels, against those of the same text through `llvm-as \| llvm-dis` 19 and 22 |
| §8 | `syntax::a_module_without_a_target_needs_a_default`, `nesting_has_a_limit`, `integer_literals_have_a_limit` | |
| §8 | `robustness`: every prefix of the hand-written modules, seeded mutations of them and of clang's output, and some 55 inputs built to be deep, long or repetitive | each returns, within a time a quadratic loop would exceed |
| §9 | `perf` | printed |

The corpus (`tests/common`): `tests/data/*.c` and `*.cpp` compiled at
test time by clang 19 and clang 22 for `aarch64-linux-gnu` and
`x86_64-linux-gnu` under several option sets (with and without `-g`,
`-fPIC`, sanitizers, relative vtables, branch protection), and the
hand-written `tests/data/*.ll` for what clang does not write; 62 modules,
28 MiB. Each is paired with `llvm-as | llvm-dis` of its own LLVM and of
the newest, and with `llc`'s object. Tests that need LLVM tools skip with
a note when the tools are absent, and fail instead under
`HOD_REQUIRE_TOOLS=1`.

Where `hod_ll` and `llc` differ and the test says so by name: an
initializer that is a difference of block addresses has no image in
`hod_ir` yet (`labels.relative`); a discardable `unnamed_addr` constant
that only holds a symbol's address is replaced by `llc` with a GOT
entry, which is the backend's choice and not the parser's.

## 11. Function bodies

The body parser (WP-008) reads the text of one function body, a span of
§6, into a `hod_ir::Body` (`spec/ir.md` §5) through `FunctionBuilder`.
It is the same `Parser` as the module level's, in body mode: types,
constants, attributes and metadata references are read by the code of
§5 and §7, and the names they refer to resolve through `ModuleText`
(§6) and `Module::find_symbol` instead of the module level's own
tables.

### 11.1 Interface

```rust
pub struct BodyParser<'t, 'a> { /* … */ }
impl<'t, 'a> BodyParser<'t, 'a> {
    pub fn new(text: &'t ModuleText<'a>) -> Self;
    pub fn parse(&mut self, module: &mut Module, function: &FunctionText)
        -> Result<Option<Body>, LlError>;
    pub fn unsupported(&self) -> impl ExactSizeIterator<Item = &str>;
    pub fn debug(&self) -> &BodyDebug;
    pub fn recycle(&mut self, body: Body);
}
pub fn parse_bodies<E: From<LlError>>(module: &mut Module, text: &ModuleText<'_>,
    each: impl FnMut(&Module, ParsedBody<'_>) -> Result<(), E>) -> Result<(), E>;
pub struct ParsedBody<'p> { pub function: &'p FunctionText, pub body: Option<&'p Body>,
    pub unsupported: &'p [Cow<'static, str>], pub debug: &'p BodyDebug }
pub struct BodyDebug { pub locations: Vec<(u32, MdRef)>, pub records: Vec<DebugRecordText> }
pub struct DebugRecordText { pub name: Span, pub args: Span, pub before: u32 }
```

- `parse` adds to `module` the types, constants and inline-assembly
  callees the body mentions, and the body is valid against that module.
  It never adds a symbol.
- `Ok(None)`: the body has a construct of §3.4 marked "body". Each is
  named by `unsupported()` (once) and recorded against the function's
  symbol. The rest of the body is still read and checked, with the
  stand-in the table gives, so an error after an unsupported construct
  is still an error.
- A parser keeps its tables from one function to the next; with
  `recycle`, the next body is built in the storage of the last one
  (`spec/ir.md` §5.1), so peak memory follows the largest function.
  `parse_bodies` does that for every function, in file order.
- **Parallel parsing.** `spec/ir.md` §5.1 has bodies parsed in parallel
  against a frozen `&Module`. That is not implementable on `hod_ir` as it
  is: there is no lookup-only access to the type and constant tables,
  and `Value` has no immediate integers, so almost every body interns
  something. The deviation: each worker parses with its own **clone** of
  the module (`Module: Clone`), with a `BodyParser` of its own over the
  shared `&ModuleText`. Every id present in the module before the clone
  means the same in every clone, so a body is the same body against any
  clone that has what it interned; a consumer that needs one module
  re-parses, or runs one sequential pass first (after which the bodies'
  ids are all present). The unsupported names come back from the parser
  as well as in the clone's record, for merging. This is a gap of
  `hod_ir` (`spec/landed/ll/WP-008.md`).

### 11.2 Locals

`%name`, `%"name"` and `%N` are the function's own: arguments, results
and blocks. Arguments are named from the parameter list of the header
(§6): unnamed ones take the numbers from 0, then the entry block, if it
has no label, the next number. Each unnamed result that is not `void`
and each unlabelled block takes the next number; a written number may
skip numbers (LLVM 19 and later) but never go back. A name used before
its definition is a `FunctionBuilder::reserve` placeholder, resolved at
the definition; the type it was used with must be the type it is defined
with (`Type`); a name used and never defined is `Undefined` at its first
use. Numbers are kept in a dense table (up to 4096 past the highest
met) and a map beyond; names in an open-addressed table emptied by
generation, so neither costs an allocation per function.

### 11.3 Blocks

`BlockId` is the block's ordinal in the text, the entry block 0: the
number of terminators before the block. That is the rule of §5.5, so a
`blockaddress` built at the module level names the same block. A use of
a block name not yet met triggers one scan of the labels from the
current statement to the end of the body (terminators and labels outside
parentheses, as §5.5). A use of a *number* not found by that scan is a
block without a label further on: the body is then read twice more, a
numbering pass that records the number and ordinal of each unlabelled
block (whether an unnamed instruction takes a number depends on its
type, so this needs the real grammar), then the real pass. A
`blockaddress` inside a body resolves the function through
`text.functions()`, by symbol, and numbers blocks without a label by the
scan of §5.5.

### 11.4 Instructions

Every instruction of LLVM 19 to 22 is read. What `hod_ir` keeps
(`spec/ir.md` §5.4):

- Flags (`nuw`, `nsw`, `exact`, `disjoint`, `samesign`, `nneg`,
  `inbounds`, `nusw`, `inrange`, fast-math flags) are read and dropped.
- `getelementptr` becomes `ptr_add`: constant indices folded into the
  offset, the others as `(value, stride)` pairs.
- An `alloca` in the entry block with a literal count, in address space
  0, becomes a slot of `size × count` bytes at its alignment (the
  preferred one when none is written); any other is the `Alloca`
  instruction.
- Default alignments: `load` and `store` the ABI alignment of the type;
  `cmpxchg` and `atomicrmw` the store size rounded up to a power of two.
- The funclet instructions are read, emitted as bare opcodes and noted
  (`inst:*`): the body is unsupported.

The checks LLVM's parser makes on operand types are made (`Type`), with
LLVM's messages where it has one; `castIsValid` is reproduced for casts.

### 11.5 Memory and atomics

`load atomic` and `store atomic` take an ordering LLVM allows for them,
or the text is an error. `syncscope("singlethread")` is kept; any other
scope is noted `syncscope:<name>`.

### 11.6 Calls

`call`, `invoke` and `callbr`: `[tail | musttail | notail] call
[fast-math] [cc] [return attributes] [addrspace(N)] type callee(args)
[function attributes] [bundles]`, then the successors.

- `type` is a function type, checked against the arguments (count and
  types), or the return type alone, in which case the function type is
  made of the arguments' types (the callee's own when it is the same).
- The callee is a symbol (a direct call; so is a constant expression
  that is a symbol's address), a local, inline assembly (`asm
  [sideeffect] [alignstack] [inteldialect] [unwind] "…", "…"`, interned
  with the call's function type) or any other pointer constant.
- Of the call-site function attributes, in place or through `#N`, only
  `noreturn`, `nounwind` and `returns_twice` are kept (`spec/ir.md`
  §5.7); the callee's own are not merged in. A rejected attribute is
  noted `attr:<name>`, also through a group.
- Argument and return attributes are those of §5.6.
- Operand bundles are kept with their tags and inputs.
- A callee named `llvm.*` is resolved once per symbol: first
  `hod_ir::no_code_intrinsic`, then `resolve_intrinsic` with the
  declaration's type. It must be called with exactly the declaration's
  function type: anything else is a `Type` error, which LLVM's verifier
  also refuses ("Intrinsic called with incompatible signature").
- `...` at the end of the arguments forwards the variable arguments of
  the function; it is allowed in a `musttail` call only, and there only
  in a variadic function, where it is required (LLVM's parser: "unexpected
  ellipsis in argument list for non-musttail call", "… for musttail call
  in non-varargs function", "expected '...' at end of argument list for
  musttail call in varargs function"). Nothing is recorded for it: a
  `musttail` call in a variadic function always forwards them, and the
  printer writes the `...` back (`spec/ir.md` §9.4). `hod_ir`'s verifier
  also refuses variable arguments of the call itself before the `...`,
  which LLVM's accepts (`spec/ir.md` §5.8).
- A `metadata` argument is read (`!"…"`, `!N`, `!{…}`, `!DIKind(…)`, or a
  value as metadata) and, if the call would be emitted, noted
  `metadata-arg:<callee>`: `hod_ir` has no metadata operand.

### 11.7 Calls that generate no code

By `spec/ir.md` §5.5: a call of a `Drop` intrinsic leaves no
instruction, and its result, if any, is poison; a `FirstArg` intrinsic's
result is its first argument (whose type must be the result type); a
`DebugRecord` intrinsic (`llvm.dbg.*`) is a debug record in the old
form: its argument list is stepped over and handed to §11.9. An `invoke`
or `callbr` of one is noted `invoke:no-code-intrinsic`. A ring of
`FirstArg` calls that stand for each other leaves values that never
resolve: noted `no-code-cycle`.

### 11.8 What is not checked

As at the module level, the body parser is not a verifier. It does not
check dominance, that phi incomings match the predecessors, where
landing pads and funclet pads stand, inline-assembly constraints, the
rules of `musttail`, the operands of a debug record, or the metadata of
an attachment beyond that the node exists. A body that parses need not
pass `Body::verify`, though every body of the corpus does (§11.11).

### 11.9 Debug information

`hod_ir`'s debug nodes are not delivered yet (WP-005). The body parser
keeps where the text has them (`BodyParser::debug`): the `!dbg`
attachment of each instruction that left one in the body, by its
position, and each debug record (`#dbg_value(…)` and kin, and calls of
`llvm.dbg.*`) as the spans of its name and arguments with the position
of the instruction it stands before. Every other attachment is checked
to name a node and dropped. Both pass through one seam in `body.rs`
(`SEAM(WP-005)`), where the work package that delivers debug nodes
turns them into `FunctionBuilder` calls.

### 11.10 Cost

Measured by `tests/perf.rs` as in §9 ("module and bodies", "bodies
alone") and by `tests/corpus.rs` over the captured corpus. Nothing is
recursive or quadratic in the size of a body: the robustness inputs
include a block of a million instructions, a 100,000-way `switch`, a phi
of 100,000 incomings, 100,000 forward references and 100,000 unlabelled
blocks referred to ahead (`tests/robustness.rs`). Figures:
`spec/landed/ll/WP-008.md`.

### 11.11 Tests

| section | test | oracle |
|---|---|---|
| §11.2 to §11.7 | `syntax::verdicts_and_error_positions`, `llvm_as_agrees_or_the_difference_is_named` (the rows after "Bodies") | `llvm-as` 19 and 22 |
| §3.4 (body names) | `syntax::unsupported_constructs_are_recorded_by_name` | the names, per snippet |
| §11 | `roundtrip::printed_programs_read_back_run_the_same` (and an `#[ignore]`d larger batch) | `hod_interp` on generated programs, against the same programs printed as LLVM text and read back |
| §11, §11.3 | `differential::bodies::every_body_parses_verifies_and_has_llvms_blocks` | `Body::verify`; the text as written and as `llvm-dis` prints it give the same shapes; block counts against `llvm-dis`'s labels |
| §11.1 | `differential::bodies::bodies_parsed_on_clones_in_parallel_are_the_sequential_ones` | one sequential parser |
| §11 | `corpus::every_corpus_body_parses_and_verifies` (with `HOD_LL_CORPUS`) | `Body::verify` over the captured corpus |
| §8, §11.10 | `robustness` (prefixes and mutations now parse bodies too; body inputs) | each returns in time |
| §11.10 | `perf` | printed |
