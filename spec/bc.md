# spec: hod_bc — LLVM bitcode to hod_ir

## 1. What it is

`hod_bc` reads the bitcode LLVM 19 to 22 write (`llvm-as`, `clang -c
-emit-llvm`) into a `hod_ir::Module`. It is the second front door beside
`hod_ll` (`spec/ll.md`), and it replaces the `llvm-dis` round trip a
bitcode input otherwise costs.

The one rule above all others: for any bitcode file B, `read_module(B)`
delivers the module that `hod_ll::parse_module(llvm-dis(B))` delivers.
That covers the same symbols with the same fields, the same types and
constants, the same notes of unsupported constructs, the same module
lists, and the same refusals. Where the two could disagree, `hod_ll`'s
decision is the contract, and this file only says how the bitcode
reaches it. A decision `hod_ll` makes is cited as `ll.md §N`.

Stage 1 reads the module level and steps over function bodies (§7).
Stage 2 reads the bodies (§11) under the same rule: the bodies of B are
the bodies `hod_ll::parse_bodies` reads from `llvm-dis(B)`. Debug
information is a later stage (§6).

The rules for input:

- Input this reader does not take is an error with a stable name (§3.4),
  never a guess.
- Every error is a `BcError` carrying the bit position (§8), never a panic.
- No recursion follows the depth of the input.
- No allocation is sized by a number in the input before that number is
  checked against the bytes that remain.

## 2. The bitstream container

The format is LLVM's "LLVM Bitcode File Format" document: a little-endian
stream of bits, read in 32-bit words, least significant bit first.
`bitstream::Cursor` is the reader.

### 2.1 Fields

- **Fixed(w)** is w bits, 0 ≤ w ≤ 64.
- **VBR(w)** is a sequence of w-bit chunks, 2 ≤ w ≤ 32. Each chunk's top
  bit means "more follows". The value must fit 64 bits. A value that
  would pass 64 bits is Malformed.
- **Char6** is `a-z A-Z 0-9 . _`.
- Reading past the end of the data, or past the end of the current
  block, is Malformed.
- Fixed(0) and VBR(0) read as the literal 0, as LLVM reads them.

### 2.2 Blocks

ENTER_SUBBLOCK carries the block id (VBR8), the new abbreviation width
(VBR4) and, after a 32-bit alignment, the length in words (Fixed32).

- The declared length bounds the block. A read past it is Malformed,
  and so is a length that passes the end of the data.
- END_BLOCK re-aligns to 32 bits and must land exactly where the
  declared length ends.
- A block can be skipped by its length without being read, and that is
  how function blocks and the blocks of §3.3 are passed.
- Nesting is an explicit stack of at most `limits::MAX_BLOCK_DEPTH` (64)
  entries. One more is a Limit error. LLVM nests at most four deep.

### 2.3 Abbreviations

DEFINE_ABBREV lists operands: literal, Fixed, VBR, Array, Char6 and Blob.

- **Shapes that are refused as Malformed** (LLVM refuses the same ones):
  - an Array that is not second to last;
  - an Array whose element is an Array or a Blob;
  - a Blob that is not last;
  - an abbreviation with no operands;
  - a width over the limit of its encoding.
- An abbreviation id below 4 is a built-in id. An id above the defined
  ones is Malformed.
- Abbreviations defined in a block end with that block.
- **UNABBREV_RECORD** is a code (VBR6), a count (VBR6) and that many
  operands (VBR6).
- **Counts are checked before allocating.** An operand count, an array
  length or a blob length must fit in the bits that remain in the block,
  and is checked before anything is allocated (§8).
- At most `limits::MAX_ABBREV_OPS` abbreviation operands may be defined
  in one block together with its BLOCKINFO ones.
- **Compiled once.** The cursor compiles each abbreviation where it is
  defined: one op per operand, its kind resolved, an array's element
  folded into the array op, a Fixed field's mask computed. A record is
  read from the compiled ops. Every value, position and error is that
  of reading the definition field by field: a Fixed field of at most 57
  bits with a word of bits left is read from that word (no check of
  that read can fail); any other field is read with its checks.

### 2.4 BLOCKINFO

The BLOCKINFO block (id 0) holds SETBID followed by DEFINE_ABBREVs and
the optional name records.

- The abbreviations it defines for block id N take the first ids,
  starting at 4, of every later block N. The block's own abbreviations
  follow them.
- A DEFINE_ABBREV before any SETBID is Malformed.
- BLOCKINFO is read wherever it appears: at the top level, or inside the
  module block. `ModuleBits::block_info` hands it on (§7).

## 3. The first pass

`raw::walk` walks the file once. It interprets nothing. It keeps the
records the module level needs, as (code, position, operands) in one
`u64` pool, and the positions of the blocks it skips.

### 3.1 The file

- **The bitcode wrapper.** A file may start with the wrapper header
  (magic `0x0B17C0DE`, version, offset, size, CPU type). The wrapped
  range must lie inside the file, and only that range is read.
- **The magic.** What is read must start with `BC 0xC0DE`. Anything else
  is Malformed.
- **Top-level blocks:**
  - IDENTIFICATION (13): the producer and the epoch. An epoch other
    than 0 is `Unsupported("epoch")`.
  - MODULE (8).
  - STRTAB (23): its blob is the string table.
  - SYMTAB (25): skipped.
  - BLOCKINFO (0).
  - A second MODULE block is `Unsupported("multiple-modules")`.
  - Any other top-level block is `Unsupported("block")`.
- A file with no MODULE block is Malformed.

### 3.2 The MODULE block

**Records kept:**

- VERSION
- TRIPLE
- DATALAYOUT
- ASM
- SECTIONNAME
- GCNAME
- COMDAT
- GLOBALVAR
- FUNCTION
- ALIAS
- IFUNC
- SOURCE_FILENAME
- VSTOFFSET (read and checked, not used)
- HASH (ignored)

**Blocks read:**

- PARAMATTR_GROUP and PARAMATTR
- TYPE_NEW
- CONSTANTS
- METADATA_KIND
- METADATA (§3.3)
- OPERAND_BUNDLE_TAGS
- SYNC_SCOPE_NAMES

**Function blocks.** Each FUNCTION_BLOCK is skipped by its length. Its
`BlockSpan` (the bit position of its ENTER_SUBBLOCK, its abbreviation
width, and its end) is recorded in file order.

### 3.3 Skipped

These blocks are skipped by their declared length and never read, as
`llvm-dis` drops or rebuilds what they hold:

- VALUE_SYMTAB: names come from the string table, version 2.
- USELIST
- the module summaries: FULL_LTO_GLOBALVAL_SUMMARY and
  GLOBALVAL_SUMMARY.
- SYMTAB

**The METADATA block** is read record by record, but only these are
kept:

- STRINGS
- NAME paired with the NAMED_NODE after it
- NODE
- VALUE
- KIND
- INDEX_OFFSET and INDEX (skipped)
- GLOBAL_DECL_ATTACHMENT

- every debug-information record (§6.3), LOCATION, EXPRESSION,
  ASSIGN_ID and ARG_LIST.

Every record that defines an id takes the next metadata id, as
`md::defines_id` says; ARG_LIST does too, as LLVM's reader numbers it.
The first pass also notes whether any kept record is debug information
(`Raw::md_debug`): a module with none, and no `llvm.dbg.cu`, costs
nothing more (§6.1).

### 3.4 Unsupported, by name

Input this reader will not take is `BcErrorKind::Unsupported(name)`. The
names are stable:

| name | what |
|---|---|
| `module-version` | a MODULE VERSION other than 2 (relative value ids with a string table: LLVM 5 and later) |
| `no-strtab` | a version 2 module with no STRTAB block |
| `multiple-modules` | a second MODULE block in one file |
| `block` | a top-level block not listed in §3.1, or an unknown block inside MODULE |
| `record` | a record code not known in its block (TYPE, CONSTANTS, MODULE) |
| `epoch` | an IDENTIFICATION epoch other than 0 |
| `typed-pointers` | TYPE_CODE_POINTER or the pre-3.3 FUNCTION type |
| `attribute-list-old` | the pre-3.3 PARAMATTR entry |
| `attribute-encoding` | an attribute encoding other than 0, 1, 3 to 8 |
| `attribute-kind` | an attribute kind code past the table of §5.3 |
| `attribute-range-list-wide` | a range-list attribute over 64 bits |
| `typeless-type-attribute` | a type attribute (encoding 5) with no type |
| `alias-old` | the pre-3.8 ALIAS record |
| `linkage` | a linkage code LLVM does not write |
| `deplib` | the MODULE DEPLIB record |
| `metadata-string-old` | METADATA_STRING_OLD |
| `const-expr-binop`, `const-expr-cast` | a constant-expression opcode LLVM 19 to 22 no longer fold |
| `const-expr-select`, `const-expr-cmp`, `const-expr-unop`, `const-expr-gep-old` | constant expressions `hod_ll` refuses as well (`ll.md §3.4`) |
| `module-inline-asm-value` | an INLINEASM constant at module level |
| `debug-layout` | a debug-information record laid out as no LLVM from 19 to 22 writes it: more operands than LLVM 22 writes, a DIExpression version other than 3, a DIGlobalVariable or DISubrange version other than 2, the type references, subprogram and unit layouts of bitcode before LLVM 3.9, a DILocalVariable without its alignment (§6.3), a DEBUG_LOC of more than 7 operands or a debug record of more than its operands (§11.6) |

## 4. Record codes

`codes.rs` names every block id and record code the reader uses, after
LLVM's `LLVMBitCodes.h`. They were checked against `llvm-bcanalyzer
-dump` of LLVM 19 and 22 output and against the differential of §9.2.

The codes of LLVM 19 to 22 do not differ for anything this reader keeps.
There is one exception: attribute kind 105 (`nocreateundeforpoison`) is
new in 22.

## 5. The module build

`module::read_module` builds from the kept records, in the order
`hod_ll` builds, so that the notes, symbol numbering and refusals come
out the same.

### 5.1 Order

1. **Version and target.**
   - VERSION must be 2.
   - The target comes from TRIPLE, or from `ReadOptions::default_target`
     when there is none. With neither, the error is Target.
   - DATALAYOUT is parsed and checked as `ll.md §3.1` checks it.
2. **Module asm.** ASM is split into lines, each ending in `\n`.
3. **Types** (§5.2). The notes of named-struct definitions are recorded
   against no symbol.
4. **COMDATs** (§5.4).
5. **Attribute groups** (§5.3).
6. **Metadata kinds.**
7. **Symbols** (§5.5): every global is declared before any constant is
   built.
8. **Module metadata** (§5.8).
9. **The globals, in record order:**
   - variables (§5.7);
   - functions, together with their attribute lists;
   - aliases and IFUNCs, with their targets as constants (§5.6).
10. **Final checks.** The symbol checks of `ll.md §5.8`.

### 5.2 Types

TYPE_BLOCK_NEW builds `hod_ir` types.

- **Named structs.** Types are structural in `hod_ir`, so a named struct
  is its fields. A struct with no body (OPAQUE) is a fresh opaque struct.
- **Forward references.** A field may name a later entry, so the table
  is resolved by a walk with its own stack. A type that contains itself
  without a pointer between is a Type error.
- **NUMENTRY** must equal the number of type records in the block.
- **Types `hod_ir` lacks.** These are stood in for and noted, as
  `ll.md §5.1` does:

  | type | stand-in | note |
  |---|---|---|
  | `ppc_fp128` | fp128 | `type:ppc_fp128` |
  | `x86_mmx` | `<1 x i64>` | `type:x86_mmx` |
  | `x86_amx` | target type `x86_amx` | `type:x86_amx` |

- **Who carries a type's notes.** A use of a type carries the notes of
  the type as written in that place. A named struct contributes nothing
  at its use, since its definition was noted once.
- **Target extension types** take their name from the STRUCT_NAME
  before them. Their type and integer parameters are read and checked;
  `hod_ir` keeps only the name.

### 5.3 Attributes

**PARAMATTR_GROUP entries** are (group id, index, attributes). The
encodings are:

| encoding | meaning |
|---|---|
| 0 | a kind |
| 1 | a kind with an integer |
| 3 / 4 | a string key, without or with a value |
| 5 / 6 | a type attribute, with no type or with a type |
| 7 | a constant range: two integers of the given width, the wide form in active words |
| 8 | a constant range list, at most 64 bits wide |

- **Kind codes.** A kind code names an attribute through the table in
  `attrs::KIND_NAMES`. That table is LLVM's `AttributeKindCodes`,
  0 to 105.
- **Groups.** The group id must be unique, and the index is
  0xFFFFFFFF for a function, 0 for a return value, or 1+N for parameter
  N. PARAMATTR lists name groups by id.
- **Building.** From a function's list, function attributes, return
  attributes and parameter attributes are built exactly as `hod_ll`
  builds them from text (`ll.md §5.6`). That includes:
  - which attributes are kept, dropped or noted (`attr:<name>`);
  - the `elementtype` and memory-type handling;
  - the AArch64 branch-protection upgrade of `ll.md §7.3`.
- **Hand-off.** Attribute lists of call sites are a body matter and are
  handed on (`ModuleBits::attr_list`).

### 5.4 Strings, COMDATs, sections

- **Names come from the string table.** With version 2, every name
  (globals, COMDATs) is an (offset, size) pair into the STRTAB blob. A
  range outside the blob is Invalid.
- **COMDATs.** COMDAT is (name offset, name size, selection kind).
  Kinds 1 to 5 are any, exactly-match, largest, no-duplicates and
  same-size. A second COMDAT of one name is Redefined.
- **Section and GC names.** SECTIONNAME and GCNAME records are numbered
  from 1. A global's section or GC field names one of them, or is 0 for
  none.

### 5.5 Symbols and numbering

- **Every global value is declared first.** Variables, functions,
  aliases and IFUNCs are all declared before any constant, so forward
  references among globals resolve.
- **Value ids.** Globals take value ids in record order, and the
  module-level constants follow them.
- **Symbol ids.** Symbols are created in record order, which is LLVM's
  module order (variables, functions, aliases, IFUNCs): the order
  `ll.md` §5.7 gives the text, so both deliver the same `SymId`s. An
  unnamed global's symbol is created in its place too.
- **Unnamed globals.** A global with an empty name is numbered as
  `llvm-dis` numbers it: variables, then aliases, then IFUNCs, then
  functions, each group in record order. The order is
  `ModuleBits::numbered_globals`.
  - A local one (internal or private linkage) is an anonymous symbol.
  - A non-local one is named `__unnamed_K` (K counted from 1) and noted
    `symbol:anonymous-external`, as `ll.md §5.7` does.
- **The special arrays** (`llvm.global_ctors`, `llvm.global_dtors`,
  `llvm.used`, `llvm.compiler.used`) are not symbols. They are read into
  the module lists as in `ll.md §5.9`, and their notes go to no symbol.
  `llvm.global.annotations` in section `llvm.metadata` is dropped, as in
  `ll.md §5.9` (WP-095). Any other `llvm.` variable is noted
  `special-global:<name>`.

### 5.6 Values and constants

- **Built on demand.** Module-level constants are built when a global's
  initializer, an alias target, or a body asks for them. A CONSTANTS
  record may name a later value id, so each one is built by a walk with
  its own stack. A constant that holds itself is Invalid.
- **Typed by SETTYPE.** Each constant takes the type of the last SETTYPE.

The records:

- **NULL** follows its type's kind: an integer gives `int(0)`, a float
  type `float(0)`, a pointer `null`, a token `none`, and anything else
  gives `zero`.
- **UNDEF** and **POISON** are as named.
- **INTEGER** is a sign-rotated VBR, kept to the type's width.
- **WIDE_INTEGER** is in words.
- **FLOAT** follows the bits of its type.
- **AGGREGATE**, **STRING**, **CSTRING** and the **DATA** arrays are
  folded as `ll.md §5.3` folds the same text: bytes, splats and
  aggregates.
- **Expressions** are the cast, binary, GEP (with and without
  `inrange`), vector, `blockaddress`, `dso_local_equivalent`, `no_cfi`
  and `ptrauth` records, folded as `ll.md §5.5` folds them:
  - an address plus an offset;
  - the difference of two addresses;
  - an integer of an address.
  - A `ptrauth` constant is its pointer, noted `const-expr:ptrauth`.
  - The kinds `hod_ll` refuses are refused by name (§3.4).
- **blockaddress** must name a function with a body. Its block is the
  number in the record. Whether that block exists is a body matter.
- **Notes.** A constant's notes are the notes of the types it is written
  with and of the constants it holds. They are recorded once against the
  symbol whose initializer or target holds it.

### 5.7 Variables, functions, aliases, IFUNCs

The record fields are those of LLVM's `writeModuleInfo`:

- **Common fields:** linkage, visibility, DLL storage class, thread
  local mode, `unnamed_addr`, external initialization, `dso_local`,
  partition, code model, sanitizer metadata and attributes.
- Each field maps to the `hod_ir` field, or to the note, that `hod_ll`
  gives the same keyword (`ll.md §5.7`, `§5.8`).
- **Alignment** is stored as log2 + 1, with 0 for none.
- **Address space.** For a variable it is in the explicit-type bit
  field. For a function it is the address space of its pointer.
- **A function's type** is its FUNCTION record's type.
- **Body or declaration.** A function has a body when the record's
  `isproto` is 0. Its FUNCTION block is then the next unclaimed one, in
  order.
- **Personality** names a value id + 1, or 0 for none.

### 5.8 Metadata

Only what `hod_ll` uses is built. The rest takes an id and is never read.

- **Strings.** METADATA_STRINGS is a count, an offset and a blob: VBR6
  lengths, then the bytes. Both parts are checked against the blob.
- **Named metadata:**
  - `llvm.module.flags` gives the module flags of `ll.md §7.1`, and the
    known ones become `ModuleProps`.
  - `llvm.linker.options` and `llvm.dependent-libraries` give the string
    lists of `ll.md §7.2`.
  - `llvm.ident` and `llvm.errno.tbaa` are dropped, as `ll.md §3.3`
    drops them.
  - `llvm.dbg.cu` lists the compile units (§6.2).
  - Any other named node is noted `named-metadata:<name>`.
- **Nodes** are read without recursion. A flag's value is an integer, a
  string or a node. A node value is `Other`, as `hod_ll` keeps it.
- **Attachments.** GLOBAL_DECL_ATTACHMENT of a declaration gives the
  `global-metadata:<kind>` notes of `ll.md §5.7`.
  - A definition's own attachments live in its function block, so stage
    1 does not see them (§10). The body reader records their
    `global-metadata:<kind>` notes and keeps them as
    `BodyReader::attachment_notes()` until the next read; the body is
    still returned, as `hod_ll` returns it. The facade
    (`crates/hod/src/front.rs`) declines such a function by those names,
    where `hod_ll`'s same notes decline the module (WP-064; before, they
    were recorded after the module was judged and nothing refused them).

## 6. Debug information

The debug information of a module is read into `Module::debug`, the
tables `hod_ll` fills from text (`ll.md §7.4`), with the same fields,
defaults, checks and refusals: what `hod_ll` reads from `llvm-dis` of
the bitcode, `hod_bc` reads from the bitcode (§6.6 lists where the two
can part).

### 6.1 When

The module has debug information when its METADATA block holds a debug
record (§3.3) or it has `!llvm.dbg.cu`. Only then is §6.2 run; a module
without debug information allocates nothing for it and walks no
function block twice.

### 6.2 The passes

`module::Build::debug` runs after the named metadata (§5.8) and before
the items (§5.7):

1. **Reserve.** Over the module's metadata ids, in order: each node
   record and LOCATION reserves an id (`DebugInfo::reserve`,
   `reserve_loc`); each EXPRESSION is interned; ASSIGN_ID is kept as
   nothing (`ir.md §6.9`).
2. **Define.** Each node and location is defined from its record (§6.3);
   a reference names any id, so forward references and cycles need no
   order. A field that names a tuple as a single node reserves the tuple
   and defines it (uniqued) after the node; a list field reads the
   tuple's operands in place. Strings and constants are added
   (hash-consed). A reference to a location, an expression or an
   assign id where a node is expected, to a value of a function, or to
   a record that is not debug information is `debug:invalid` and stands
   as an empty tuple, as in `hod_ll`.
3. **Compile units.** `llvm.dbg.cu`'s operands, each a DICompileUnit
   (else `debug:invalid`), are listed (`add_compile_unit`).
4. **Functions.** Each FUNCTION block is walked once, in file order
   (`debug::scan_function`): its METADATA block is read (strings and
   records, ids numbered from the module's count) and its
   METADATA_ATTACHMENT block's even-length record gives the `!dbg` of
   the definition. Steps 1 and 2 run over the function's ids. What a
   body needs of each id is kept in `DebugBits` (`ModuleBits::debug`):
   the handle of a node, location or expression, a value as metadata
   (type and value id), `!{}`, or a DIArgList's operands.

Then the items: a variable's `!dbg` (GLOBAL_DECL_ATTACHMENT) names its
DIGlobalVariableExpressions (`attach_global_expr`); a function's names
its DISubprogram (`FunctionDesc::subprogram`), from the attachment of
a declaration or the function block of a definition. Anything else
there is `debug:invalid`, noted against the symbol.

After the items, as `hod_ll`'s `finish_debug`: `Module::finish_debug`,
and when the module uses its debug information (`llvm.dbg.cu`, a
global-variable expression or a subprogram) and the module flag `Debug
Info Version` is not 3 or `verify_debug` fails, `debug:invalid`.

### 6.3 Records

Operands are metadata id + 1 (0 for null) unless said otherwise. `op0`
holds `distinct` in bit 0 and the record's version or flags above it.
Every record's operand count is checked: fewer than LLVM 19 writes is
`Invalid`, more than LLVM 22 writes is `debug-layout`. The table is
`debug::layout`; each operand is a field `hod_ir` maps, drops or
refuses as `di_field` says (`ir.md §6.2`), or a flag read by hand.

| code | record | operands accepted | read by hand |
|---|---|---|---|
| 7 | DILocation | 5, 6 or 8: `distinct, line, column, scope (id), inlinedAt + 1[, implicit[, atomGroup, atomRank]]` | column of 16 bits or more is 0; `atomGroup, atomRank` kept (WP-149, `ll.md §11.9`) |
| 12 | GenericDINode | any | `DiOther` |
| 13 | DISubrange | 5 | version 2 (`op0 >> 1`); each bound a constant, a node or an expression |
| 14 | DIEnumerator | 3, or wide: `flags, width, name, words…` | sign-rotated words; a value that does not fit 64 bits is `debug:DIEnumerator.wide` |
| 15 | DIBasicType | 6 to 9 | 22: `op0 & 2`, size as metadata |
| 16 | DIFile | 3 to 6 | checksum kind and value, both or neither |
| 17 | DIDerivedType | 12 to 15 | 22: `op0 & 2`, size and offset as metadata; `dwarfAddressSpace` + 1; the packed pointer-authentication word |
| 18 | DICompositeType | 16 to 26 | `op0 & 2` required (`debug-layout`); 22: `op0 & 4`, size and offset as metadata; `enumKind` 0xFFFFFFFF is none |
| 19 | DISubroutineType | 3 to 4 | `op0 & 2` required |
| 20 | DICompileUnit | 14 to 23 | `op11` (old subprograms) must be 0; a DWARF 6 language name (`op1` bit 63) is `debug:DICompileUnit.sourceLanguageName` |
| 21 | DISubprogram | 18 to 21 | `op0 & 6 == 6` required; 22: `keyInstructions` (`op20`) |
| 22, 23 | DILexicalBlock, DILexicalBlockFile | 5, 4 | |
| 24 | DINamespace | 3 | `exportSymbols` from `op0 & 2` |
| 25, 26 | DITemplateTypeParameter, DITemplateValueParameter | 3 to 4, 5 to 6 | the 5-operand value form has no `defaulted` |
| 27 | DIGlobalVariable | 12 to 13 | version 2 |
| 28 | DILocalVariable | 9 to 10 | `op0 & 2` (alignment present) required |
| 29 | DIExpression | elements | version 3 (`op0 >> 1`) |
| 31 | DIImportedEntity | 6 to 8 | |
| 37 | DIGlobalVariableExpression | 3 | |
| 40 | DILabel | 5 to 7 | `isArtificial` from `op0 & 2`; `coroSuspendIdx` `u64::MAX` is none |
| 30, 32–34, 41, 44, 45, 48, 49 | DIObjCProperty, DIModule, DIMacro, DIMacroFile, DIStringType, DICommonBlock, DIGenericSubrange, DISubrangeType, DIFixedPointType | any | `DiOther`; `classify` refuses `debug:<kind>` |
| 46 | DIArgList | value ids as metadata (0-based) | a body's record value |
| 47 | DIAssignID | 0 | nothing kept |

### 6.4 Refusals

The names are `hod_ll`'s (`ll.md §3.4`): `debug:<kind>` (by
`classify`), `debug:<kind>.<field>` for a field `hod_ir` cannot hold at
a value other than its default, `debug:DIEnumerator.wide`,
`debug:#dbg_declare_value`, `debug:#dbg_assign.DIArgList` and
`debug:invalid`. They are module or body notes, never errors. A record
the reader cannot decode is an error: `Invalid` (cut short, an id that
names nothing, a field out of range) or `Unsupported("debug-layout")`.

### 6.5 Cost

Linear: each record is read once per pass and each function block is
walked once more (step 4). A tuple is defined once, when first named.
No recursion follows the input. `tests/debug.rs` and `examples/bcread.rs`
give the numbers (`landed/bc/WP-055.md`).

### 6.6 Where `hod_bc` and `hod_ll` can part

- **Function-local metadata in a module with no module-level debug
  record and no `llvm.dbg.cu`.** §6.1 does not run, so a body's
  `DEBUG_LOC` or record finds no node and notes `debug:invalid`.
  `hod_ll` reads the same nodes from the text. LLVM writes a
  subprogram's unit, file and types at module level, so clang and kiln
  never write such a module.
- **The `llvm.dbg.*` calls** (§11.6) are read as `hod_ll` reads them,
  but LLVM 19 to 22 write debug records, never the calls, so no tool
  makes the bitcode to compare.

### 6.7 Tests

`src/debug.rs`'s unit tests, over `tests/data/debug-llvm22.bc` (clang 22
`-O2 -g` of `hod_ll`'s `debug_c.c`, checked in):

- `the_layouts_agree_with_hod_ir`: each field of §6.3's table is one
  `hod_ir::di_field` maps, drops or refuses as the table says;
- `record_lengths_outside_the_layout_are_refused`: for every kind, one
  operand short is `Invalid` and one past the maximum is `debug-layout`;
- `unknown_versions_are_refused`: a DIExpression of version 4, a
  DIGlobalVariable or DISubrange of version 1 and a LOCATION of 9
  operands are `debug-layout`; an empty EXPRESSION and a LOCATION of 4
  are `Invalid`;
- `out_of_range_operands_are_errors`: a node or a location naming
  metadata that does not exist;
- `unknown_codes_are_not_debug_records`;
- `wide_enumerators_are_noted`: `debug:DIEnumerator.wide`.

`tests/debug.rs` compares `hod_bc` on bitcode with `hod_ll` on
`llvm-dis` of it, as `common::read_*_bodies` dump them (the debug dump
labels every node reachable from the compile units, the global-variable
expressions and the subprograms in the order a walk meets them, and
writes every location and debug record structurally); a difference
names its line:

- clang 19 and 22, `-O0` and `-O2`, on `debug_types.cpp`, `debug_c.c`
  and `stl.cpp`; clang 22 with `-gkey-instructions` and `-gdwarf-6`;
- kiln's `debug/kiln_types.ll`;
- `declare_value_is_refused_by_name`: record 66;
- `corpus_debug_modules_agree` (`HOD_BC_CORPUS`): every corpus module
  with `!llvm.dbg.cu` whose `llvm-dis` text `llvm-as` takes back.

`tests/differential.rs` and `tests/bodies.rs` count a module whose
`llvm-dis` text `llvm-as` does not take back as a tool failure: there is
no oracle (§10).

## 7. The interface to a body reader

`read_module` returns `BcModule { module, bits }`. `bits: ModuleBits`
borrows the input and carries what a body reader needs:

| item | what |
|---|---|
| `data()` | the bytes read |
| `block_info()` | the BLOCKINFO abbreviations, to start a cursor on a body |
| `functions()` | each definition: `FunctionBits { sym, ty, span }`, in FUNCTION record order. `span` is the body block's position. Open it with `bitstream::Cursor::enter_at` |
| `value_count()` | the number of module-level values: the first value id in a body |
| `global(id)`, `value_type(id)` | a module-level value's symbol (or none) and type |
| `constant(&mut module, id)` | a module-level constant, built on first use |
| `type_id(id)` | a bitcode type id's `hod_ir` type |
| `attr_list(index)` | a PARAMATTR list's group ids (call-site attributes) |
| `metadata_kinds()`, `sync_scopes()`, `bundle_tags()` | the tables of those names |
| `metadata_count()`, `metadata(id)` | the module metadata slot table: a string, a tuple (operands as id + 1, 0 for null), a value, or a skipped (debug-info) node; function-local metadata is numbered from `metadata_count()` |
| `debug` (crate-private) | §6.2's `DebugBits`: what each module and function-local metadata id became |
| `numbered_globals()`, `producer()` | the numbering of §5.5 and the producer string |

**Reading bodies in parallel** (WP-063). Bodies are independent of each
other given `ModuleBits`, which is shared read-only. Each thread's reader
(`BodyReader`, §11.1) keeps its own copy of the value table and reads
against a fork of a seed of the module (`spec/ir.md` §5.1): one copy
of its tables per parallel read, shared by every fork; the constants a body builds go to the fork,
and the calling thread adopts them into the module in file order. A
module-level constant built on first use (`constant`) is built in the
fork and adopted like the rest, so it gets the id the sequential reader
gives it.

## 8. Errors and limits

`BcError { kind, bit, message }`. `bit` is the position in the input
where the error was found.

| kind | when |
|---|---|
| `Malformed` | the bitstream is broken: magic, end of data or block, abbreviations (§2) |
| `Unsupported(name)` | §3.4 |
| `Invalid` | a record's contents make no sense: too few operands, an index or id that names nothing, a field out of range, a string-table range outside the blob |
| `Type` | a value of the wrong type, or a type that contains itself |
| `Redefined` | a second COMDAT or symbol of one name |
| `Limit` | `limits::MAX_BLOCK_DEPTH` or `limits::MAX_ABBREV_OPS` passed |
| `Target` | no triple and no default target |
| `Ir(kind)` | `hod_ir` refused what the bitcode describes |

**What the reader guarantees on any input:**

- Every read is bounds-checked.
- Every count that sizes an allocation is first checked against what
  remains of the input: an operand count, an array or blob length,
  NUMENTRY, or the count of metadata strings. The memory used is
  therefore linear in the input.
- No recursion follows the input: blocks, types, constants and metadata
  nodes are walked with explicit stacks.

## 9. Tests

### 9.1 Unit tests

`src/bitstream/tests.rs` (§2):

- fixed fields of every width and alignment
- reads past the end
- VBR
- blocks, records and every operand encoding
- local abbreviations ending with their block
- BLOCKINFO order
- malformed abbreviations
- zero-width fields
- counts checked before allocation
- a block that cannot read past its length
- nesting 100,000 deep as a Limit error
- compiled abbreviations against a field-by-field reference reader:
  every encoding at every offset, over arbitrary bits (values,
  positions, errors), and UNABBREV_RECORD operands cut short or over
  64 bits

### 9.2 The differential

`tests/differential.rs`, over the corpus that `xtask corpus` captures
(`HOD_BC_CORPUS`):

1. For each `.ll`, `llvm-as` of its major gives B.
2. `llvm-dis` of B gives T.
3. `hod_ll` reads T and `hod_bc` reads B.

**How the two are compared.** Comparing the `hod_ir` values directly
would compare ids, and those depend on the order things were created,
which the two readers need not share. So the comparison is of
`common::dump`, a sorted line dump:

- symbols by name, with every field;
- types and constants written out structurally;
- opaque structs written as `opaque`;
- an anonymous symbol named by its place in the numbering of §5.5;
- the notes;
- the module properties, flags, ctors and dtors, linker options,
  libraries, module asm and target.

Every byte of `hod_ir` that the module level writes appears in the dump.
Both refusing counts as agreement. Any other difference fails.

### 9.3 The handwritten module

`tests/oracle.rs` uses `tests/data/module.ll`. It runs the same
comparison on a module made to hold most of what §5 covers:

- COMDATs, sections and TLS;
- x86_fp80, fp128 and half;
- vectors, splats and wide integers;
- the folded expressions, `inrange`, `blockaddress`,
  `dso_local_equivalent`, `no_cfi` and `ptrauth`;
- aliases and an IFUNC;
- parameter and function attributes, calling conventions and
  personality;
- numbered globals;
- ctors, `llvm.used` and `llvm.compiler.used`;
- module flags, linker options and dependent libraries;
- module asm;
- the notes.

It also asserts that the dump holds each of these, so the agreement is
not vacuous.

### 9.4 Robustness

`tests/robustness.rs` uses `tests/data/module-llvm19.bc`,
`tests/data/bodies-llvm19.bc` and `tests/data/debug-llvm22.bc` (checked
in; the last holds the debug records of §6.3, `DEBUG_LOC`,
`DEBUG_LOC_AGAIN` and the debug records of §11.6). It checks four
things:

- the file reads;
- every truncation is an error;
- every single-byte corruption returns: eight values per position by
  default, and all 255 with `HOD_BC_ALL_BYTES=1`;
- 2,000 random inputs return, some behind a real magic and some as a
  real prefix with a random tail.

A read over 2 s fails as a hang. The process's peak resident memory
(`VmHWM`) may grow by at most 256 MiB over a run.

### 9.5 Cost

`examples/bcread.rs` gives the best of n reads of one file. Its modes
are:

- `bc`: `read_module`;
- `walk`: the first pass alone;
- `ll`: `hod_ll::parse_module` on text.

Peak memory is measured from outside with `/usr/bin/time -f %M`. The
numbers of each run are recorded in `landed/bc/`.

## 10. Known gaps

- **Definition attachments** are read by the body stage (§11.7): a
  reader that reads only the module level misses their notes.
- **blockaddress** does not check that the block exists (§5.6).
- **`hod_ll`'s nesting limit** on constants is not mirrored. Bitcode
  constants are walked with an explicit stack, without a depth limit.
- **Names.** A name that is not UTF-8 is read lossily where `hod_ir`
  takes a `&str`: target extension types and attribute string keys.
- **Debug information** parts from `hod_ll` where §6.6 says.
- **`llvm-dis` oracle.** `llvm-dis` 19 and 22 write a struct type named
  only in a `DITemplateValueParameter`'s value without defining it, so
  `llvm-as` refuses the text; such a module has no oracle.

## 11. Function bodies

Stage 2 (WP-046) reads each definition's FUNCTION block into a
`hod_ir::Body`. The rule is `hod_ll`'s: for every module, the body
`hod_bc` reads from bitcode is the body `hod_ll` reads from `llvm-dis`
of that bitcode, with the same unsupported names, or both refuse with
the same error kind. Every check and decision below is a port of the one
`hod_ll`'s body parser makes (`spec/ll.md` bodies), applied to records
instead of text.

### 11.1 The interface

| item | what |
|---|---|
| `BodyReader::new(&ModuleBits)` | a reader: a copy of the module value table and scratch tables, reused across bodies |
| `reader.read(&mut Module, &FunctionBits)` | `Ok(Some(body))`, `Ok(None)` when a construct is unsupported (the names in `reader.unsupported()`), or `Err(BcError)` |
| `reader.recycle(body)` | gives a body's storage back for the next |
| `read_bodies(&mut Module, &ModuleBits, each)` | every definition in order, `each(&Module, ReadBody { function, body, unsupported })`; the shape of `hod_ll::parse_bodies` |

| `read_bodies_on(host, module, &ModuleBits, threads, each)` | the same on up to `threads` threads (below) |

**In parallel** (WP-063). `read_bodies_on` reads through
`hod_ir::read_bodies` (`spec/ir.md` §5.1): one `BodyReader` per thread,
against a fork of a seed of the module, over the shared `ModuleBits` (§7).
`each(host, function, Ok(body) | Err(constructs))` gets the host back
(the module is `module(host)`) and returns whether to go on. Its promise
is determinism: the bodies delivered and their order, the unsupported
constructs and their record, the module's tables and every id in them,
and the error returned are the sequential reader's at any number of
threads, and with one thread it is the sequential reader, except that a
body with attachment notes (`BodyReader::attachment_notes`, WP-064) is
delivered as unsupported, those notes first, as the facade judges it.
The error is
the first in file order: a body that fails in a fork is read again in its
turn against the module itself, and nothing after it is delivered
(`tests/parallel.rs`: clang 22's C++ as bitcode with and without `-g`,
the checked-in files, and bytes flipped in two bodies, at 1, 2, 3, 8 and
24 threads).

### 11.2 Value numbering

A body's value ids, in order:

1. the module values (`value_count()`): globals, then module constants;
2. the arguments;
3. the function CONSTANTS block's constants (every record except
   SETTYPE), numbered after the arguments and built on first use like
   the module's (§5.6), over both tables;
4. each instruction whose type is not `void`, in record order.

**Operand encodings** (LLVM's relative ids, used from LLVM 3.3 on and
by every major read here):

- a value operand is relative: `abs = next − rel` (wrapping 32-bit),
  where `next` is the id the next instruction would define;
- a value-and-type operand (`VT` below) is followed by a type id only
  when it is a forward reference (`abs ≥ next`);
- `phi` incoming values are signed relative (`rel` sign-rotated:
  bit 0 is the sign);
- absolute: block ids, `alloca`'s count, `switch` case values, the
  CALLBR indirect targets.

**Forward references.** A reference to an instruction not yet read gets
a reserved value of the type the record gives. When the instruction is
read, its type must match (a `Type` error otherwise) and the reserved
value is resolved to it. A reference never defined is an error;
`hod_ir` reports an undefined value at `finish`, which is a cycle with
no code and gives the note `no-code-cycle`, as `hod_ll` does.

**Blocks.** DECLAREBLOCKS (once, before any instruction; count between
1 and what the block's length allows) declares the blocks; each
terminator ends one, and the next instruction begins the next. An
instruction after the last declared block is `Invalid`.

### 11.3 Records

`ty` is a type id, `V` a relative value, `VT` a value with its type when
forward. Trailing fields not listed are read and ignored (they carry
flags `hod_ir` does not keep: `nuw`/`nsw`/`exact`/`disjoint`/`nneg`,
fast-math flags, `samesign`, GEP no-wrap flags). LLVM 19 and 22 write
the same layouts; where a field appeared later it is optional and
ignored.

| code | record | layout | reads as |
|---|---|---|---|
| 2 | BINOP | `[VT lhs, V rhs, opcode, flags?]` | integer or float binary by opcode and operand type |
| 56 | UNOP | `[VT arg, opcode, flags?]` | `fneg` |
| 3 | CAST | `[VT arg, ty, opcode, flags?]` | a cast; LLVM's `castIsValid` checked |
| 43 | GEP | `[flags, ty source, VT base, VT index…]` | `PtrAdd`, constant indices folded (§11.4) |
| 26 | EXTRACTVAL | `[VT agg, index…]` | `ExtractValue` at the folded byte offset |
| 27 | INSERTVAL | `[VT agg, VT value, index…]` | `InsertValue` |
| 29 | VSELECT | `[VT true, V false, VT cond]` | `select` |
| 6 | EXTRACTELT | `[VT vector, VT index]` | |
| 7 | INSERTELT | `[VT vector, V value, VT index]` | |
| 8 | SHUFFLEVEC | `[VT first, V second, VT mask]` | mask a constant vector |
| 28 | CMP2 | `[VT lhs, V rhs, predicate, flags?]` | `icmp` (32–41) or `fcmp` (0–15) by operand type |
| 16 | PHI | `[ty, (signed V, block)…, flags?]` | a phi; an odd trailing field is fast-math flags |
| 19 | ALLOCA | `[ty, ty count, count (absolute), packed, addrspace?]` | a slot or `Alloca` (§11.4) |
| 20 | LOAD | `[VT ptr, ty, align, volatile]` | |
| 41 | LOADATOMIC | `[VT ptr, ty, align, volatile, ordering, scope]` | |
| 44 | STORE | `[VT ptr, VT value, align, volatile]` | |
| 45 | STOREATOMIC | `[VT ptr, VT value, align, volatile, ordering, scope]` | |
| 46 | CMPXCHG | `[VT ptr, VT expected, V new, volatile, success, scope, failure, weak, align?]` | result `{T, i1}` |
| 59 | ATOMICRMW | `[VT ptr, VT value, op, volatile, ordering, scope, align?]` | |
| 36 | FENCE | `[ordering, scope]` | |
| 23 | VAARG | `[ty list, V list, ty result]` | |
| 58 | FREEZE | `[VT arg]` | |
| 47 | LANDINGPAD | `[ty, cleanup, count, (catch=0/filter=1, VT clause)…]` | |
| 34 | CALL | `[attrs, ccinfo, fmf?, ty fn, VT callee, args…]` | §11.5 |
| 13 | INVOKE | `[attrs, ccinfo, normal, unwind, ty fn, VT callee, args…]` | §11.5 |
| 57 | CALLBR | `[attrs, ccinfo, default, count, indirect…, ty fn, VT callee, args…]` | §11.5 |
| 55 | OPERAND_BUNDLE | `[tag, VT input…]` | attached to the next call |
| 10 | RET | `[]` or `[VT value]` | |
| 11 | BR | `[block]` or `[true, false, V cond]` | |
| 12 | SWITCH | `[ty, V cond, default, (case value id, block)…]` | cases must be integer constants, no duplicates |
| 31 | INDIRECTBR | `[ty, V addr, block…]` | |
| 39 | RESUME | `[VT value]` | |
| 15 | UNREACHABLE | `[]` | |

**Funclets** (CLEANUPRET 48, CATCHRET 49, CATCHPAD 50, CLEANUPPAD 51,
CATCHSWITCH 52) are not read further: each gives `inst:<name>` and a
stand-in (a token value for the pads), so the body is unsupported, as
`hod_ll` names them.

**Ordering and scope.** Orderings are LLVM's codes (0 not atomic, 1
unordered, 2 monotonic, 4 acquire, 5 release, 6 acq_rel, 7 seq_cst; 3
is `Invalid`). A scope is a number into SYNC_SCOPE_NAMES: `""` is
`System`, `"singlethread"` `SingleThread`, any other name gives
`syncscope:<name>`. A module without the table has 0 singlethread and
1 system. Load and store orderings are checked as `hod_ll` checks them.

**Alignment** fields are log2 + 1, 0 for none; the default is the
type's ABI alignment (`alloca`: its preferred one).

### 11.4 Folding and slots

- **GEP**: the indices walk the source type as `hod_ll` walks it;
  constant indices fold into the byte offset, variable ones become
  `(index, scale)` pairs; vector GEPs splat. The unsupported cases are
  `hod_ll`'s (scalable types and the like), noted by its names.
- **EXTRACTVAL / INSERTVAL**: the index path is folded to the byte
  offset of the field.
- **ALLOCA**: `packed` is `align low 5 bits | inalloca << 5 |
  explicit-type << 6 | swifterror << 7 | align high 3 bits << 8`. Without
  the explicit-type bit (bitcode before LLVM 3.x) it is
  `Unsupported("record")`. `inalloca`, `swifterror` and scalable types
  give `alloca:inalloca`, `alloca:swifterror`, `alloca:scalable`. An
  alloca in the entry block, in address space 0, with a literal count
  is a stack slot of `count × alloc size`; any other is an `Alloca`.

### 11.5 Calls

`ccinfo`:

| record | calling convention | explicit type | other bits |
|---|---|---|---|
| CALL | `(ccinfo & 0x7ff) >> 1` | bit 15 | tail bit 0, musttail bit 14, notail bit 16, fast-math present bit 17 |
| INVOKE | `ccinfo & !(1 << 13)` | bit 13 | |
| CALLBR | `(ccinfo >> 1) & 0x3ff` | bit 15 | |

A call without the explicit function type (bitcode before LLVM 3.x) is
`Unsupported("record")`. The callee is a VT; a function constant that
is INLINEASM (code 30: `[fnty, flags, asmlen, asm…, conlen,
constraints…]`) gives an inline-asm callee, the older asm codes are
`Unsupported("record")`.

The rest follows `hod_ll`'s order:

1. a call of a debug intrinsic is dropped, as `hod_ll` drops it;
2. the arguments: one per parameter (a metadata parameter consumes one
   operand, a relative metadata id; a label parameter is a `Type`
   error), then VT operands for the variadic part. A metadata argument
   becomes a metadata operand as `hod_ll` makes one (`spec/ll.md`
   §11.6, WP-064): a METADATA_STRING, or a METADATA_NODE (not
   `distinct`) of strings, of the module or of the function's own
   METADATA block. That block is read by the module pass when the module
   has debug information (§6.2), and otherwise by the body reader, which
   then reads the block's strings and nodes only;
3. the attributes: `attrs` is a PARAMATTR list (§5.3); function flags
   come from its groups (each group decoded once per reader), the
   return and argument attributes as `hod_ll` keeps them, and the
   names the groups reject are notes;
4. an intrinsic's signature is checked as `hod_ll` checks it;
5. a `musttail` call in a variadic function forwards the function's
   variable arguments; the record does not say so and nothing is
   recorded for it, as in `hod_ll` (`spec/ll.md` §11.6);
6. the operand bundles, then the call is emitted; `invoke` and `callbr`
   end their block.

### 11.6 Skipped

- the function's VALUE_SYMTAB (local names: `hod_ir` keeps none), its
  METADATA block (read at module level, §6.2, or for metadata operands,
  §11.5 step 2), USELIST blocks and any
  other nested block;
- BLOCKADDR_USERS (60);
- metadata attachments of instructions other than `!dbg` (which
  bitcode writes as DEBUG_LOC).

**Debug information** is read as `hod_ll` reads it (`ll.md §11.9`):

- **DEBUG_LOC (35)** `[line, column, scope + 1, inlinedAt + 1,
  implicit]`, and in LLVM 22 `atomGroup, atomRank` (kept with the
  location, WP-149; a rank over 8 bits is `Invalid`): 4 to 7 operands. It is the location of the instructions the last record
  became (`set_loc_since`), interned (`DebugInfo::loc`). A scope that is
  not a node or an inlined-at that is not a location is `debug:invalid`.
  A scope of 0 is `Invalid`. **DEBUG_LOC_AGAIN (33)** repeats the last;
  without one it is `Invalid`.
- **Debug records** follow the instruction they stand before; after
  each instruction the builder places the records that follow before
  its first `hod_ir` instruction (`place_records_before`). Operands are
  metadata ids (0-based):
  - 61 VALUE and 62 DECLARE `[loc, var, expr, value]`;
  - 64 VALUE_SIMPLE `[loc, var, expr, value id (relative, and its type
    if a forward reference)]`;
  - 63 ASSIGN `[loc, var, expr, value, id, address expr, address]`;
  - 65 LABEL `[loc, label]`;
  - 66 DECLARE_VALUE notes `debug:#dbg_declare_value`.

  The location must be a DILocation (else `debug:invalid` and the
  record is dropped), the variable a DILocalVariable and the label a
  DILabel (else `debug:invalid`), the expression an expression
  (`Invalid`). A value is a value as metadata, `!{}` (none) or a
  DIArgList of values; anything else is `Invalid`. Then as `hod_ll`:
  a declaration of a DIArgList is `debug:invalid`; an assignment whose
  value or address is a DIArgList notes `debug:#dbg_assign.DIArgList`.
- **A call of `llvm.dbg.declare`, `.value`, `.assign` or `.label`**
  (the form before debug records): its arguments are metadata ids
  (relative, as values are), read as a record's operands, and the
  record waits for the call's DEBUG_LOC; without one it is
  `debug:invalid`. Another `llvm.dbg.*` notes `debug:<name>`.
- After the body is built, `Body::verify_debug` must pass, else
  `debug:invalid`.

### 11.7 Definition attachments

The function block's METADATA_ATTACHMENT block holds the definition's
own attachments in its one record of even length (`[kind, node]…`;
an odd-length record is an instruction's, skipped). Each kind other
than `dbg`, `type`, `vcall_visibility`, `prof`, `section_prefix` and
`annotation` gives the module note `global-metadata:<kind>` for the
definition, as `hod_ll` notes `define … !kind !N`. The body is still
delivered.

### 11.8 Refused

- the instruction codes of bitcode before LLVM 19's format: GEP_OLD
  (4), SELECT (5), CMP (9), STORE_OLD (24), INBOUNDS_GEP_OLD (30),
  CMPXCHG_OLD (37), ATOMICRMW_OLD (38), LANDINGPAD_OLD (40),
  STOREATOMIC_OLD (42), and any code not in §11.3:
  `Unsupported("record")`;
- a CONSTANTS block after an instruction, a second DECLAREBLOCKS,
  operand bundles before an instruction that is not a call, an id or
  block that names nothing, a field out of range: `Invalid`;
- a value of the wrong type: `Type`.

### 11.9 Tests

`tests/bodies.rs`:

- `handwritten_bodies_agree_on_every_major`: `tests/data/bodies.ll`,
  through `llvm-as` of each installed major, read by both readers; it
  holds every record of §11.3 and the notes of §11.3–11.7;
- `handwritten_bodies_are_delivered`: the expected unsupported names
  and attachment notes, so the agreement is not vacuous;
- `bodies_read_in_parallel_are_the_same`;
- `module_metadata_slots`: the slot table accessor of §7;
- `corpus_bodies_agree` (`HOD_BC_CORPUS`, `HOD_BC_EVERY`,
  `HOD_BC_DIFFS`): every corpus module, compared as `common::body_dump`
  (each instruction with its operands, types and constants written out
  structurally) plus the module dump of §9.2.

`tests/robustness.rs` (§9.4) reads the module and then every body, over
`tests/data/bodies-llvm19.bc` and `tests/data/debug-llvm22.bc` as well.
The debug tests are §6.7.

### 11.10 Cost

`examples/bcread.rs` modes `bc-bodies` and `ll-bodies` read the module
and every body and print the time per instruction. The numbers are in
`landed/bc/WP-046.md`.
