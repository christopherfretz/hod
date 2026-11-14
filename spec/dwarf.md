# spec: hod_dwarf — debug information

`hod_dwarf` writes an object's DWARF 5 debug information at `-O0`: the line
table from what the code generator records per function (`spec/codegen.md`
§9.5), and the debugging information entries (DIEs) from `hod_ir`'s debug
tables (`spec/ir.md` §6) plus the same record. It depends on `hod_obj`,
and on `hod_ir` for the DIEs only: the line table is written from plain
rows of its own types, the DIEs straight from `hod_ir`'s `DebugInfo`
(decided by WP-053, §2), with the addresses the code generator placed
passed in as plain rows. It knows nothing of `hod_cg`. The bridge that
fills the rows from a module and its `FuncRecord`s belongs to the caller
(`hod_cg::emit_module`, §9).

This file is the design of the whole crate (§1 to §8) and the contract of
what is implemented: the line-table writer and its policy (§4, §5), and
the core of the DIE writer (§6.1 to §6.3, §6.5 to §6.7). Everything else
is marked *planned* and is split into work packages in §10.

External formats are cited as **DWARF 5** (section numbers of the DWARF
Debugging Information Format, Version 5); **AArch64 ELF** and the **x86-64
psABI** for relocations. Statements about what `llc` does were checked with
`llc` 19.1.7 and 22.1.8 at `-O0 -global-isel=0 -fast-isel=0
-relocation-model=pic` on `clang -g -O0` and kiln output, reading
`llvm-dwarfdump` and `llvm-readelf`; statements about gdb (16.3) and lldb
(19.1.7) were checked by running them. Where a statement rests on memory
of the DWARF 5 standard or of LLVM's sources rather than on a run, it says
"(from memory)".

## 1. What hod writes

### 1.1 Sections

What `llc -O0 -g` writes for a DWARF 5 Linux object, and therefore hod:

| section | contents | flags | relocations (AArch64 / x86-64) |
|---|---|---|---|
| `.debug_abbrev` | the abbreviation table (§6.1) | none | none |
| `.debug_info` | one `DW_UT_compile` unit per compile unit | none | `ABS32` / `R_X86_64_32` against the section symbols of `.debug_abbrev` (`abbr_offset`), `.debug_str_offsets` (`DW_AT_str_offsets_base`, addend 8), `.debug_line` (`DW_AT_stmt_list`), `.debug_addr` (`DW_AT_addr_base`, addend 8), `.debug_rnglists` (`DW_AT_rnglists_base`, addend 12); nothing else: the unit's `DW_AT_low_pc` is a `DW_FORM_addrx` when its code is in one section, and `DW_FORM_addr` 0 **without a relocation** when it has ranges (checked with `llc` 19 and 22 by WP-053) |
| `.debug_str` | DIE strings, NUL-terminated, pooled | `SHF_MERGE\|SHF_STRINGS`, entsize 1 | none |
| `.debug_str_offsets` | header (length, version 5, padding) and one 4-byte offset per string the unit names by `DW_FORM_strx*` | none | one `ABS32` / `R_X86_64_32` per entry against `.debug_str` |
| `.debug_addr` | header (length, version 5, address size 8, segment selector size 0) and one 8-byte address per `DW_FORM_addrx`/`DW_OP_addrx`/`DW_RLE_*x` use | none | one `ABS64` / `R_X86_64_64` per entry, against the **section symbol** of the code section plus the function's offset (a variable: against its symbol, or its section's symbol for a local one) |
| `.debug_rnglists` | the unit's range list (and a block's, when it is not contiguous) | none | none (they name `.debug_addr` entries) |
| `.debug_line` | the line table (§4, §5) | none | `ABS32` / `R_X86_64_32` per string of the header against `.debug_line_str`; `ABS64` / `R_X86_64_64` per `DW_LNE_set_address` against the section symbol of the code section plus offset |
| `.debug_line_str` | the line table's directory and file names, pooled | `SHF_MERGE\|SHF_STRINGS`, entsize 1 | none |

All have type `SHT_PROGBITS`, alignment 1, and none is `SHF_ALLOC`. The
format is 32-bit DWARF (`DW_FORM_sec_offset` and string offsets are 4
bytes); address size 8 on both targets.

Not written, with the reason:

- `.debug_loclists`: at `-O0` every variable `hod_cg` describes has one
  location for the whole function (`spec/codegen.md` §9.5, "`vars`"), which
  is an expression in the DIE (`DW_FORM_exprloc`). A location list is
  needed only for `#dbg_value` variables tracked over ranges, which
  hod writes since WP-162 (§6.23). `llc -O0` writes none for `clang -O0`
  output.
- `.debug_names`, `.debug_gnu_pubnames`/`pubtypes`: `llc` writes
  `.debug_names` when the unit's `nameTableKind` is `Default` and the GNU
  pair for `GNU`; clang on Linux and kiln write `nameTableKind: None`, and
  gdb and lldb index the DIEs themselves. hod writes no name index: a
  unit with `Default` or `GNU` gets none, a documented difference that
  costs only debugger start-up time. (Open question 1, §11.)
- `.debug_aranges`, `.debug_frame`: `llc` writes neither by default;
  debuggers unwind with `.eh_frame` (`spec/eh.md`).
- `.debug_types` / type units: only with `-fdebug-types-section`, which is
  not a module property.

**Compressed** (WP-061): with `debug_compression: Zlib` (`hod
--compress-debug-sections=zlib`, what `clang -gz=zlib` asks of LLVM's
object writer; `llc` has no such option) every `.debug_*` section that
shrinks is written as `SHF_COMPRESSED`: an `Elf64_Chdr`
(`ELFCOMPRESS_ZLIB`, the plain size, the plain alignment 1), a zlib
stream, `sh_addralign` 8, the other flags kept (`.debug_str` is `MSC`).
A section that would not shrink stays as above, as in LLVM. Relocations
are unchanged: their offsets are into the uncompressed bytes, which
linkers and debuggers decompress before applying them. The format, the
compressor and its limits are `spec/object.md` "Compressed sections";
zstd is not written.

### 1.2 Units

One `.debug_info` unit per compile unit listed in `!llvm.dbg.cu`
(`DebugInfo::compile_units()`), each with its own line-table unit in
`.debug_line` (`DW_AT_stmt_list` points at it) and its own contributions
to `.debug_str_offsets`, `.debug_addr` and `.debug_rnglists`; the string
sections are shared by all units. A function belongs to the unit of its
subprogram (`DiSubprogram.unit`). kiln and clang write one unit per
module; several appear only in linked IR (`llvm-link`). The line writer of
§4 writes one line-table unit per call; several units are several calls
appending to the same two sections (planned: the writer today creates the
sections, §4.6).

A unit without code is written by `llc` (`DwarfDebug::beginModule`)
when it lists a global (placed or not), an enumeration, a retained type
or an import whose scope is not a function's or a block's (a function's
imports are its children); a unit that lists none of them gets no debug
sections at all (checked with `llc` 19 by WP-053, and with `llc` 19 and
22 by WP-111: a compile-only test with only `using` declarations, or
only `-fno-eliminate-unused-debug-types` types). The bridge applies the
same rule (`crates/hod_cg/src/emit/lines.rs`, `lists_entities`);
`write_debug_info` itself writes whatever unit it is asked to.

### 1.3 Functions in COMDAT groups and in their own sections

`llc` keeps every debug section outside every group, and refers into a
group's code section from outside it:

- the line table has one sequence per contiguous run of functions in one
  section (it ends a sequence whenever emission switches sections, so a
  COMDAT function between two `.text` functions splits `.text` into two
  sequences), and each sequence starts with `DW_LNE_set_address` relocated
  against the section symbol of its code section;
- a subprogram's `DW_AT_low_pc` is a `DW_FORM_addrx` whose `.debug_addr`
  entry is relocated against the section symbol plus the function's
  offset, `DW_AT_high_pc` a `DW_FORM_data4` length;
- the unit has `DW_AT_low_pc 0` and `DW_AT_ranges` (`DW_FORM_rnglistx`):
  `DW_RLE_base_addressx` then `DW_RLE_offset_pair` per run of `.text`,
  and `DW_RLE_startx_length` per other section.

When a linker discards a duplicate group, the relocations from debug
sections into it resolve to a tombstone (GNU ld: 0; lld: 0 or −1 by
section (from memory)), and gdb and lldb ignore the sequences, ranges and
subprograms that start there. hod does exactly what `llc` does, so that
the linkers' existing handling applies; the L7 package links two objects
that share a COMDAT function and checks that the debugger reports one
location for a breakpoint in it (§8.4). The same holds with
`-function-sections`: every function is its own section and its own
sequence.

hod's choice for the line table (§5.6): **one sequence per function**,
whether or not the next function is in the same section. `llc`'s choice
differs only for consecutive functions of one section, where hod writes
an `end_sequence` and a `set_address` (14 bytes and one relocation) more.
Reason: a function's sequence then depends only on its own rows, so a
worker can encode it in the chunk that compiled the function and the merge
concatenates bytes (§7.1). For C++ the cost is small, since most
functions (templates, inline functions) are COMDAT members and are their
own sequences in `llc`'s output too. The writer of §4 takes sequences as
given, so it can also write `llc`'s grouping, which the oracle test uses.

## 2. Where each DIE comes from

`hod_ir` keeps, per kind, the record `spec/ir.md` §6.2 lists; `FuncRecord`
keeps the per-function facts that only exist after code generation
(`spec/codegen.md` §9.5). The DIE writer reads both, and the bridge maps
`DiId`s to DIE offsets (§6.3).

| DIE (M3) | from `hod_ir` | from the function's record | missing today |
|---|---|---|---|
| compile unit | `DiCompileUnit` (`producer`, `language`, `file` → `DW_AT_name`/`DW_AT_comp_dir`, `isOptimized`, `flags`, `runtimeVersion`, `emissionKind`, `enums`, `retainedTypes`, `globals`, `imports`, `nameTableKind`, `sysroot`, `sdk`) | the placed functions' sections and offsets (`DW_AT_ranges`) | nothing for `-O0` units; `splitDebugFilename` and `splitDebugInlining` drive split DWARF (§6.15), a `dwoId` is refused there; `macros` is `unsupported` (`spec/landed/ir/WP-005.md` field table) |
| line table | `DiFile` (`filename`, `directory`, `checksumkind`, `checksum`, `source`); `DiLocation` (`line`, `column`, `scope` → file) | `lines` (offset, `LocId`) | a block-start mark on a transition (§5.3, `crates/hod_cg/src/record.rs:119`) |
| subprogram | `DiSubprogram` (name, linkage name, scope, file, line, type, `scopeLine`, flags, `spFlags`, `declaration`, `templateParams`, `containingType`, `virtualIndex`), `FunctionDesc`'s subprogram | code size and placement (`DW_AT_low_pc`/`high_pc`), `FrameDesc::frame_base` (`DW_AT_frame_base`) | nothing |
| lexical block | `DiLexicalBlock` (`scope`, `file`, `line`, `column`), `DiLexicalBlockFile` | code ranges, derived from `lines`: the union of the transitions whose location's scope chain contains the block (§6.4) | nothing; the ranges are derived, not recorded |
| parameter, local variable | `DiLocalVariable` (`name`, `arg`, `scope`, `file`, `line`, `type`, `flags`, `align`); the record's `DIExpression` | `vars` (variable, expression, declared-at, `VarPlace`) | `VarRow` has no expression field yet (`crates/hod_cg/src/record.rs:139`) although `hod_ir` has `SlotVar.expr` now; `#dbg_value` variables get location lists (§6.23, WP-162) |
| label | `DiLabel` | `labels` | `Chunk` has no `labels` table yet (`crates/hod_cg/src/record.rs:225`) although `hod_ir` has `#dbg_label` now |
| type graph (WP-058, §6.8) | `DiBasicType`, `DiDerivedType` (pointers, references, typedefs, template aliases, cv, atomic, restrict, members, static members, inheritance, pointers to member, friends), `DiCompositeType` (classes, structs, unions, enums, arrays and vectors with `DiSubrange`s), `DiSubroutineType`, `DiEnumerator`, member function declarations (`DiSubprogram` without `DISPFlagDefinition`); a static member's constant from `Module`'s constant pool | nothing | nothing for C++ at `-O0`; Fortran-style bounds, strides, address spaces and annotations are `unsupported` (§6.8) |
| global variable | `DiGlobalVariableExpression` (`var`, `expr`) via `Module::global_exprs()`; `DiGlobalVariable` | the variable's symbol (address, TLS) | nothing |
| namespace | `DiNamespace` (`name`, `scope`, `exportSymbols`) | nothing | nothing |
| imported entity | `DiImportedEntity` (`using`, `using namespace`) | nothing | nothing |
| template parameters | `DiTemplateTypeParameter`, `DiTemplateValueParameter` (value a `DiConstant`, a symbol or a `DiTuple` for packs) | nothing | nothing |

What `hod_ir` lacks is an index, not data: a DIE tree is written parent
first, while `hod_ir` nodes name their parent (`scope`). The DIE writer
inverts the scope links once per unit (one pass over the nodes, a
counting sort by parent, §6.2).

**Where the input comes from (decided, WP-053).** The DIE writer reads
`hod_ir::DebugInfo` directly: `UnitPlan::new(&debug, unit)` decodes the
nodes it needs (`DebugInfo::node`) and borrows their strings, and no
intermediate row type mirrors the debug tables. Reasons: the tables are
already the frontend's data, interned and verified once (`spec/ir.md`
§6); a copy into `hod_dwarf`'s own types would be a second schema to keep
in step with every field later packages need (the type graph alone is
some forty fields), and a copy per node costs what the writer costs. What
only code generation knows (where each global's symbol is, where the
unit's code is) comes in as plain rows (`GlobalAddr`, `CodeSpan`), so the
writer depends on nothing in `hod_cg`. A frontend that serializes its own
format directly (kiln, without LLVM IR) fills the same `DebugInfo` with
its builder API (`reserve`, `define`, `DiNode::set`), as the tests do.

What `hod_ir` lacks for the DIE writer, found by WP-053 (none blocks the
core; `hod_ir` was not changed):

- a decoded node's strings are `&[u8]`, not the interned `StrId` they
  came from (`crates/hod_ir/src/debug/node.rs:64`, `DiFieldValue::Str`;
  the records of `node.rs:787` on), so the string pool hashes bytes where
  an id-keyed pool would index an array (§6.3);
- no per-unit index of nodes: the plan walks `cu.globals`, then
  `cu.enums` and `cu.retainedTypes` (WP-058), and follows links, which
  finds every DIE `llc` writes for them; no index is needed;
- a static member's constant (`extraData` of a static member) is a
  constant id, not a node, so its value needs the module's `ConstPool`
  and `TypeTable`: `UnitPlan::of_module` reads them, `UnitPlan::new`
  (tables only) refuses such a constant as `Unsupported` (WP-058);
- no TLS flag on a global: whether a variable's symbol is thread-local is
  the symbol's (`hod_obj::SymbolKind::Tls`), and `GlobalAddr` will carry
  it for `DW_OP_form_tls_address` (WP-059).

## 3. Interface (implemented)

```rust
pub struct LineTarget { .. }              // §4.2; AARCH64, X86_64, for_arch(arch)
pub struct FileEntry<'a> { pub name: &'a [u8], pub dir: u32, pub md5: Option<[u8; 16]>,
                          pub source: Option<&'a [u8]> }
pub struct LineHeader<'a> { pub dirs: &'a [&'a [u8]], pub files: &'a [FileEntry<'a>] }
pub struct Row { pub offset: u32, pub file: u32, pub line: u32, pub column: u16, pub flags: u8 }
pub mod flags { IS_STMT, PROLOGUE_END, EPILOGUE_BEGIN }
pub struct Sequence<'a> { pub order: u32, pub symbol: SymbolId, pub addend: i64,
                          pub size: u32, pub rows: &'a [Row] }
pub struct LineSections { pub line: SectionId, pub line_str: SectionId }
pub fn write_debug_line(obj: &mut Object, header: &LineHeader<'_>, seqs: &[Sequence<'_>])
    -> Result<LineSections, Error>;
// encoded sequences, and several units (§7.1, WP-056)
pub struct LineProgram { .. }    // new(arch), with_capacity(arch, rows, seqs), push(files, &Sequence),
                                 // append(&LineProgram), clear(), bytes(), sequence_count()
pub struct LineUnit<'a> { pub header: LineHeader<'a>, pub program: &'a LineProgram }
pub struct LineUnits { pub sections: LineSections, pub offsets: Vec<u32> }
pub fn write_debug_line_units(obj: &mut Object, units: &[LineUnit<'_>])
    -> Result<LineUnits, Error>;

// the policy (§5)
pub struct SourceLoc { pub file: u32, pub line: u32, pub column: u16 }
pub struct Transition { pub offset: u32, pub loc: Option<SourceLoc>, pub block_start: bool }
                                  // Transition::at(offset, loc): not a block start
pub struct FunctionLines<'a> { pub scope: SourceLoc, pub size: u32,
                               pub transitions: &'a [Transition], pub epilogue: bool }
pub fn function_rows(f: &FunctionLines<'_>, out: &mut Vec<Row>) -> Result<(), Error>;

// the file table (§4.4)
pub struct FileTable<'a> { .. }   // new(comp_dir, primary), add(directory, filename, md5),
                                  // new_with_source(.., source), add_with_source(.., source)

// DIEs (§6, §7.1)
pub struct UnitPlan<'a> { .. }    // new(&'a DebugInfo, unit: DiId) -> Result<Self, Error>;
                                  // unit(), files() -> &FileTable, file_index(DiId),
                                  // die_count(), die_of(DiId)
pub struct GlobalAddr { pub expr: DiId, pub symbol: SymbolId, pub addend: i64 }
pub struct CodeSpan { pub symbol: SymbolId, pub addend: i64, pub size: u64 }
pub struct UnitInput<'p, 'a> { pub plan: &'p UnitPlan<'a>, pub line_offset: u32,
                               pub globals: &'p [GlobalAddr], pub code: &'p [CodeSpan] }
pub struct DieSections { pub abbrev, pub info, pub str_offsets, pub str: SectionId,
                         pub addr: Option<SectionId>, pub rnglists: Option<SectionId> }
pub fn write_debug_info(obj: &mut Object, line: SectionId, units: &[UnitInput<'_, '_>])
    -> Result<DieSections, Error>;
pub struct DebugStrings<'a> { .. } // new(), begin_unit(), index(&'a [u8]) -> Result<u32, Error>,
                                   // unit_offsets(), data()
pub mod abbrev;                    // the fixed table: FAMILIES, BASES, CODE_END, tag, at, form
```

`UnitPlan::new` builds a unit's DIE tree from the tables alone (§6.2);
it is read-only afterwards and can be shared by every worker. Its
`files()` is the unit's `FileTable` (the unit's file, then every `DiFile`
in id order), whose `header()` is what `write_debug_line` takes, so the
`DW_AT_decl_file` numbers and the line table agree. `write_debug_info`
writes every unit's DIEs and its contributions to the string, address
and range sections, with all relocations, and returns the sections; its
output depends on `globals` and `code` as sets (§7.1).

`write_debug_line` writes one line-table unit into new `.debug_line` and
`.debug_line_str` sections with their relocations, and returns them. The
header is always written, even without sequences (a unit with no
functions still has a line table, as in `llc`). A sequence's rows are
relative to `symbol + addend`; a caller passes the section symbol of the
function's code section and the function's offset in it, as `llc` does.

`write_debug_line_units` writes several line-table units (one per
compile unit) whose sequences are already encoded: a `LineProgram` holds
the encoded bytes of the sequences pushed into it, in push order, with
their address fields, and `append` concatenates two programs. A
sequence's bytes depend only on its rows and the target (every sequence
starts from the initial state and ends with `end_sequence`), so programs
encoded per chunk and appended in module order are the bytes of one
program encoded in one go (§7.1). The units share one `.debug_line_str`
pool; `offsets` are the units' offsets in `.debug_line`, for
`DW_AT_stmt_list`. `write_debug_line` is `push` of every sequence in
`order` and one unit.

A file with source text (`DiFile.source`, `FileEntry::source`) adds the
`DW_LNCT_LLVM_source` (0x2001, `DW_FORM_line_strp`) column to the file
entry format when any file of the unit has one; files without get the
empty string, as `llc` writes it (byte for byte with LLVM 19 and 22). The
strings go to `.debug_line_str` in `llc`'s order: the directories, then
per file its name, then its source.

## 4. The line table (implemented)

### 4.1 Header

DWARF 5 §6.2.4, in this order: `unit_length` (4), `version` 5 (2),
`address_size` 8, `segment_selector_size` 0, `header_length` (4),
`minimum_instruction_length`, `maximum_operations_per_instruction` 1,
`default_is_stmt` 1, `line_base` −5, `line_range` 14, `opcode_base` 13,
`standard_opcode_lengths` `0 1 1 1 1 0 0 0 1 0 0 1`; then the directory
table and the file table:

- directories: format count 1, (`DW_LNCT_path`, `DW_FORM_line_strp`); the
  count (ULEB128); each directory's offset in `.debug_line_str`.
  Directory 0 is the compilation directory.
- files: format count 2, (`DW_LNCT_path`, `DW_FORM_line_strp`),
  (`DW_LNCT_directory_index`, `DW_FORM_udata`), and a third,
  (`DW_LNCT_MD5`, `DW_FORM_data16`), **when every file has a checksum**;
  the count (ULEB128); per file the name's offset, the directory index
  (ULEB128) and the 16 bytes of MD5. File 0 is the primary source file.

These are `llc`'s choices: the same forms, the same constants, and MD5 only
when all files have one (`llc` drops every checksum when one file has
none). Every string field is 4 bytes of zeros with an `ABS32` relocation
against the section symbol of `.debug_line_str`, addend the string's
offset.

### 4.2 The per-target table

| | AArch64 | x86-64 |
|---|---|---|
| `minimum_instruction_length` | 1 | 1 |
| address size | 8 | 8 |
| string-offset relocation | `R_AARCH64_ABS32` | `R_X86_64_32` |
| address relocation | `R_AARCH64_ABS64` | `R_X86_64_64` |

`line_base`, `line_range` and `opcode_base` are the same on both (§4.1).
AArch64 keeps a minimum instruction length of 1, as `llc` does, although
instructions are 4 bytes: address advances are in bytes, and a special
opcode then covers 17 bytes (4 instructions) of advance. A length of 4
would let one special opcode cover 17 instructions; it is not taken
because it would make every AArch64 table differ from `llc`'s (open
question 2).

### 4.3 Line strings

`.debug_line_str` holds the directory names, then the file names, in
table order, each once: a name equal to one already written reuses its
offset (no tail merging, which `llc` does not do either (from memory of
LLVM's `MCDwarfLineStr`; the byte-identity test of §8.2 holds it)). The pool is a
hash table keyed by the bytes, so the section is the same for any
insertion history that gives the same tables. A NUL inside a name is
`NulInName`.

### 4.4 The file table (`FileTable`)

The caller builds the header from `DiFile` nodes with `FileTable`, which
applies `llc`'s rules (verified on `llc` 19 and 22):

- `new(comp_dir, primary)`: directory 0 is the unit's
  `DW_AT_comp_dir` (the unit file's `directory`); file 0 is the unit's
  file (its `filename`, directory 0, its checksum).
- `add(directory, filename, md5) -> u32`: a file whose directory equals
  the compilation directory is split: the part of `filename` before its
  last `/` becomes its directory (`"./inc/util.h"` → directory `"./inc"`,
  name `"util.h"`), or directory 0 when there is none. Any other directory
  is a directory entry of its own and the name is kept whole
  (`"sub/util.h"` in `"/other/dir"` stays so). Directories and files are
  numbered in order of first `add`; adding an equal (directory, name) pair
  again returns the first index. `filename` of the primary file is also
  reachable by `add` and returns 0.

Which files are in the table and in what order is the bridge's decision
(§5.7): every `DiFile` node of the unit, in `DiId` order. `llc` numbers
files in order of first use while it writes, which a parallel writer
cannot know in advance, so hod's file indices (and so its tables' bytes)
differ from `llc`'s when the orders differ; the decoded rows name the
same files.

### 4.5 The program

For each sequence (in `order`) the state machine starts as DWARF 5
§6.2.2 says (file 1, line 1, column 0, `is_stmt` true) and each row is
written as `llc`'s `MCDwarfLineTable::emitOne` writes it:

1. `DW_LNS_set_file` (ULEB128) if the file differs from the state's;
   `DW_LNS_set_column` (ULEB128) if the column differs;
   `DW_LNS_negate_stmt` if `IS_STMT` differs from the state's;
   `DW_LNS_set_prologue_end` and `DW_LNS_set_epilogue_begin` for those
   flags. (So a sequence whose first row is in file 0 starts with
   `set_file 0`.)
2. For the first row: `DW_LNE_set_address` (`00 09 02`, 8 bytes with the
   address relocation against `symbol`, addend `addend + offset`), then
   the line advance with an address advance of 0. For the others, the
   line and address advance together.
3. The advance of line delta `l` and address delta `a` (bytes divided by
   `minimum_instruction_length`) is LLVM's `MCDwarfLineAddr::encode`:
   if `l − line_base` is not below `line_range`, `DW_LNS_advance_line`
   (SLEB128 `l`) and `l = 0`; then `DW_LNS_copy` if `l` and `a` are 0;
   else the special opcode `(l − line_base) + opcode_base + a ·
   line_range` if it is at most 255; else `DW_LNS_const_add_pc` and the
   special opcode for `a − 17` if that is at most 255; else
   `DW_LNS_advance_pc` (ULEB128 `a`) followed by `DW_LNS_copy` when the
   line was advanced separately, or by the special opcode for address 0.
4. After the last row: the advance to `size` (`DW_LNS_const_add_pc` when
   it is exactly 17, else `DW_LNS_advance_pc` when it is not 0), then
   `DW_LNE_end_sequence` (`00 01 01`).

Consecutive rows at one offset are each written (each is a row of the
matrix, DWARF 5 §6.2.5.1); a row needs no change of state to be written
(a row equal to the previous one is a `DW_LNS_copy` or a special opcode).
A sequence without rows writes nothing.

### 4.6 Errors, order and determinism

`write_debug_line` checks the whole input before it touches `obj`, and on
the first problem returns an error and leaves `obj` as it was. It never
panics on input.

| error | when |
|---|---|
| `NoPrimary` | `dirs` or `files` is empty (DWARF 5 requires entry 0 of both) |
| `BadDirectory { file }` | a file's `dir` is not an index of `dirs` |
| `NulInName` | a directory or file name contains a NUL |
| `BadFile { order, row }` | a row's `file` is not an index of `files` |
| `RowOutOfOrder { order, row }` | a row's offset is below the previous row's |
| `RowBeyondEnd { order, row }` | a row's offset is not below `size` |
| `Unaligned { order, row }` | an offset or `size` is not a multiple of `minimum_instruction_length` |
| `BadFlags { order, row }` | a flag outside `IS_STMT \| PROLOGUE_END \| EPILOGUE_BEGIN` |
| `DuplicateOrder { order }` | two sequences have the same `order` |
| `TooLarge` | a section would reach 4 GiB (32-bit DWARF) |
| `Object` | `hod_obj` refused a section |

Sequences are written in increasing `order`; the output depends only on
the set of sequences, not on the order of the slice. Relocations are
added in increasing offset order.

### 4.7 Size

The table is as small as `llc`'s for the same rows (it is the same
encoding). On top of the opcodes, one-sequence-per-function costs 14
bytes and one relocation per function that `llc` would have shared a
sequence with (§1.3). Not used, because `llc`'s readers and ours gain
nothing at `-O0` from them: `DW_LNS_fixed_advance_pc` (needed only by
assemblers that cannot compute deltas), `DW_LNS_set_isa`, discriminators
(`DILexicalBlockFile.discriminator`: optimized code only; open question
3), and `DW_LNS_set_basic_block`.

## 5. The line policy (implemented)

`spec/codegen.md` §9.5 records transitions: `(offset, LocId)` when the
location of the instruction being emitted differs from the previous
one's, the first emitting instruction always (possibly `LocId::NONE`),
and, as the last row, a row at the epilogue (the `ret`'s location if
exactly one `ret` was emitted, `LocId::NONE` otherwise). `function_rows`
turns one function's transitions into rows. The bridge maps each `LocId`
to a `SourceLoc` (the file index of the location's scope's file, its line
and column; `None` for `LocId::NONE`), passes the subprogram's file and
`scopeLine` (its `line` when `scopeLine` is 0) as `scope`, and sets
`epilogue` when the last transition is the epilogue row (always, for
`hod_cg`'s records).

### 5.1 The rows

Walking the transitions in order, with a current row and the last
non-zero line written (file and line, initially none):

1. **Start row.** Unless the first transition is at offset 0 with a
   non-zero line, a row at offset 0 with the `scope` location, column 0,
   `IS_STMT`. Its line becomes the last non-zero line. The entry code and
   the prologue precede the first transition (§9.5), so they are this
   row's. This is `llc`'s: it records the scope line at the function's
   first instruction unless the prologue is empty.
2. **No location** (`None`): no row; the instructions inherit the current
   row (§5.3). Except the epilogue row: it becomes a line-0 row (§5.4).
3. **Line 0** (an explicit `DILocation` with line 0): a row with line 0,
   its file and column, no `IS_STMT`, unless the current row's line is
   already 0 (`llc` does not repeat a line-0 row). Line 0 is "no source
   line" (DWARF 5 §6.2.5.1): debuggers attribute the code to nothing.
4. **A location**: a row with its file, line and column, unless it equals
   the current row in all three and carries no flag the current row
   lacks (a change of scope alone, which `llc` writes as a duplicate row).
5. A transition at or beyond `size` is dropped (an epilogue row of an
   empty epilogue). Rows at one offset are all kept: the start row and a
   line-0 or no-prologue row at offset 0 both stand, as in `llc`, and a
   consumer takes the last row at an address (DWARF 5 §6.2.5.1 makes
   each a row of the matrix).

### 5.2 Flags

- `IS_STMT` on a row with a non-zero line that differs from the last
  non-zero line written, or whose file differs from it; and on the
  start row and the `PROLOGUE_END` row. A return to the same line after a
  line-0 row is not a new statement. This is `llc`'s rule except for the
  file: `llc` compares lines only, so code of another file at the same
  line number (an `always_inline` call whose body is at the caller's line
  number) is not a statement in `llc`'s table, and gdb then cannot set a
  breakpoint at it. hod marks it.
- `PROLOGUE_END` on the first row from a transition with a non-zero line
  (the start row never has it unless it is that row). gdb (16.3) and lldb
  (19.1.7) put a breakpoint on a function name at the first
  `prologue_end` address when the function has one (both read the flag;
  without it they fall back to the second row of the function, the
  "second line" heuristic) (from memory for the fallback; the use of the
  flag was checked by breaking on functions in §8.3).
- `EPILOGUE_BEGIN` on the epilogue row when `epilogue` is set. `llc`
  writes it on AArch64 at the first instruction that tears the frame
  down, and on x86-64 only when that instruction is marked; hod's driver
  knows the epilogue's offset exactly, so it is on both targets. gdb uses
  it to recognize that a frame is being destroyed (its stack-frame-destroyed
  check, so that a software watchpoint does not fire on a dead frame)
  (from memory).

### 5.3 Instructions without a location

They inherit the current row: no row is made for `None`, so a
location-less instruction in the middle of a block, and the
location-less argument stores of the entry block, belong to the line
before them. `llc` does the same in the middle of a block. At the
**start of a block** (other than the entry block) whose first
instruction that emits code has no location, `llc` writes a line-0 row
instead, in the file and column of the location before it and without
`is_stmt`, so that the code is not attributed to the line of an
unrelated block laid out before it. hod does the same (WP-056): the
driver marks the first transition of such a block (`LineRow::block_start`,
`spec/codegen.md` §9.5) and rule 2 of §5.1 writes line 0 for a `None`
transition with `block_start` set, as for the epilogue. Checked against
`llc` on `crates/hod_a64/tests/data/lines/hand.ll` (`block_start`).

### 5.4 The epilogue row

With one `ret`, the epilogue row has its location, which is the line of
the `return` statement: `llc` attributes the epilogue to the `ret` too.
With none or several (`LocId::NONE`), it is a line-0 row: the shared
epilogue must not inherit the line of whichever block was emitted last
(§9.5's reason). Whether gdb's `next` stops there with "no line" or
steps through is open question 4; clang and kiln at `-O0` emit a single
`ret` per function, so the case comes only from other producers.

### 5.5 Columns

Columns are written as recorded (`DiLocation.column`, 16 bits; 0 for
none). `llc` writes them too, and lldb shows them; gdb ignores them. A
producer that writes no columns gets fewer rows (rule 4 merges rows that
differ only in a column).

### 5.6 Sequences

One sequence per function (§1.3): the function's section symbol and
offset, its code size as `size`, its rows. `end_sequence` is at the end
of the function's code, before any padding to the next function, as in
`llc`.

### 5.7 The bridge (implemented, WP-056)

In `emit_module` (`crates/hod_cg/src/emit/lines.rs`, §9): for each
compile unit that is written, its `UnitPlan`, whose `FileTable` (the
unit's file, then every `DiFile` in `DiId` order, §4.4) is the line
table's; for each function with a subprogram of that unit, as it is
placed, `function_rows` into one `Vec<Row>` reused across functions, and
the rows encoded at once as its own sequence into the unit's
`LineProgram`, relative to the section symbol of its code plus its
offset. A location's file is the file of its own scope (an inlined
location keeps its scope's file and line, as `llc` writes it); a scope
without a file is the unit's primary file.

A function **without a subprogram** gets no rows and no range, even when
its instructions have locations: `llc` writes neither (checked with LLVM
19 and 22 on `hand.ll`'s `no_subprogram`). A function of zero size gets
nothing.

**Permitted differences from `llc`** (rows compared per function modulo
addresses, `crates/hod_a64/tests/lines.rs`; stops compared under gdb
and lldb, `lines_debugger.rs`):

- P1: `llc` has a row for the location of an unconditional `br` that hod
  falls through (no code, so no row): the `}` of a loop body, the `for`
  of an `i = 0` initializer's branch to the condition, a `break` before
  the next case. A debugger stops at such a line in `llc`'s program
  only.
- P2: `llc` writes line-0 rows at the starts of machine blocks it makes
  itself (reloads after register allocation, switch lowering) that hod
  does not have.
- P3: where `llc`'s function has no frame (an empty prologue and
  epilogue), hod's prologue still has code: hod has a start row at the
  scope line `llc` lacks, and an `epilogue_begin` row `llc` lacks.
- With several `ret`s, hod's shared epilogue has one line-0 row with
  `epilogue_begin` (§5.4); `llc` has an epilogue per `ret`, each on its
  `ret`'s line.

Since WP-097 (the harness recognises each BY RULE from the IR both
programs were built from, `xtask/src/debug/permitted.rs`; nothing is
masked in general):

The instructions "at a pair's line" are, since WP-111, the function's
instructions with that line (any column) whose location's scope has the
pair's file (the scope's `DIFile`, joined as the line table joins it),
in the entry block or a block some `label %N` names: `llc` deletes
blocks without a predecessor even at `-O0` (`UnreachableBlockElim`), and
hod emits none (a terminate landing pad left behind by a folded
`invoke` has the line of the call it guarded).

- `load-placement`: a `(function, file, line)` pair one side lacks where the
  function's instructions at that line are loads and
  `getelementptr`s, at least one load. `llc`'s `SelectionDAG` merges a
  load with an earlier or later load of the same address in its block
  (the dead load of a trivial destructor's `n->~T()` keeps ITS line in
  `llc`'s program, `xtask/tests/data/debug_lines/dead_load.cpp`; a
  reload of an argument the call already loaded has no code), and folds
  a load of a call's argument into the call's sequence
  (`arg_load.c`); hod emits each load where the IR has it. Reproducing
  it would be load CSE, an optimisation. Either side.
- `post-indexed-load` (WP-111): hod's pair whose instructions at the
  line are all `getelementptr`s whose pointer operand a load of the
  function (with a location) also reads. `llc`'s AArch64 load/store
  optimizer merges the add of a constant offset into an earlier load of
  the same base as its post-increment (`ldr x8, [x9], #12`), which runs
  at that load's line, so the `getelementptr`'s line has no row of its
  own (libstdc++'s `_Sp_counted_base::_M_release_last_use`,
  `shared_ptr_base.h:187`, `&_M_weak_count` as the argument of an
  `always_inline` call; `xtask/tests/data/debug_post_index/post_index.cpp`,
  `llc` 19 and 22). Reproducing it would be an optimisation. hod's side
  only: the fold removes `llc`'s row.
- `is-constant`: hod's pair in a function that calls `llvm.is.constant`.
  `llc` folds the call to false even at `-O0` and deletes the blocks it
  guards (`LowerConstantIntrinsics`); hod does not emit the dead
  instructions (WP-084) but keeps the blocks and the branch, so their
  lines have rows (`is_constant.c`). Deleting them would need the
  branch folded and `mark_reachable` (`crates/hod_cg/src/driver.rs`) to
  know the target's folded calls, which it does not (the target decides
  in `begin_function`, after reachability): not free. Also §6.12 for DIE
  trees.
- `return-column` (sessions only): after `finish`, the stop is at a
  call's return address, which is in `llc`'s table the row of whatever
  its register allocator put after the `bl`: a `COPY` of the result out
  of `x0` carries the call's location when it is not coalesced, so
  `llc`'s row there is often the call's (`finish_column.c`: `int got =
  bump(&c, 1);` stops at column 13), and the next statement's when it is
  (the second call of the same test stops at the assignment's column, as
  hod's does for both). hod's return address has the location of the
  instruction after the call. The harness permits, in an lldb session,
  stops after `finish` (`stop reason = step out`) whose `frame #0:` line
  differs only in the column, `llc`'s column being that of a `call` or
  `invoke` at that line in the IR, with lldb's source caret moving with
  it; nothing else in the transcript may differ. Matching `llc` would
  mean modelling its register allocator.
- Columns of a line `llc` and hod both have (`cargo xtask debug columns`,
  `spec/harness.md` "debug", "Columns") are compared but do not fail the
  run: `llc`'s instruction selection folds instructions into others (a
  `zext` of a compare into the `cset`, the `and` of `x & m != 0` into
  `tst`, address arithmetic into a post-indexed load, a compare into
  its `b.cond`), and each fold moves code from one column to another
  inside a line. The measured classes are WP-097's
  (`spec/landed/dwarf/WP-097.md`).

### 5.8 Code emitted out of order (implemented, WP-097)

A lowering may emit the code of another instruction inside its own: a
deferred instruction (`spec/codegen.md` §4.6) is emitted where its user
asks for it. The driver records transitions per *segment*
(`crates/hod_cg/src/line_rows.rs`): each instruction opens one at its
first byte with its row location, and a deferred instruction asked for
(`FnCx::force`) switches to its own row location for its code and back
to the asker's after it (`FnCx::line_switch`). A segment that emitted
code is a transition (§9.5's rule: when its location differs from the
last one recorded, or it is the first code of a non-entry block without
a location). So the code of each IR instruction carries that
instruction's location wherever it was placed, as `llc`'s machine
instructions keep the location of the IR instruction they were selected
from. A compare FUSED into its user (the `cmp` of a `b.cond` or `csel`)
stays at the user's location: `llc`'s `DAGCombiner` turns `brcond
(setcc)` into `br_cc` and `select (setcc)` into `select_cc` at the
user's location (measured: switching fused compares to their own
location made 1 786 fewer functions of the `-g` set match `llc`'s
columns). On x86-64 (WP-149) the `cmp` of a branch fused with an integer
compare IS at the compare's location: `llc`'s x86-64 program keeps the
compare's instruction (`subl`/`cmpl`) at the `icmp`'s location and only
the `jcc` at the branch's (`crates/hod_x64/src/control.rs` `cond_br`
switches around `fused_icmp`). WP-157 extends it to every x86-64 fused
form whose compare `llc` keeps as an instruction of its own (§6.17). Measured on the `tests/debug` sources and
250 gcc torture sources at clang 22 `-O0`, x86-64: functions with `llc`'s
set of (line, column) positions 666 before, 732 after of 981; positions
only `llc` has 777 before, 320 after. With key instructions it decides
statements (§5.9): the compare and the branch are separate atoms.

The **row location** of an instruction is its location, except for two
whose code has none, so that it stays in the row before it (a line-0
row when it is the first code of its block, §5.3):

- `unreachable`: `llc` writes no code for it; hod's trap is no statement
  of its own, so the `unreachable`'s line (`__builtin_unreachable()`)
  has no row, as in `llc`'s table;
- a `ptradd` with offset 0 and no index (a zero-offset `getelementptr`,
  the first member of a struct): its value is its base's, so its code is
  a copy or reload of the base or the materialization of a frame or
  symbol address, which `llc` folds into the user or emits without a
  location (frame-index materializations, register-allocator reloads).

Nothing here changes code: the `.text` of the corpus's and the `-g`
set's modules is byte-identical before and after (WP-097). Making a
zero-offset `getelementptr` an alias of its base instead (no code at
all) was considered and not done: a deferred value may today only be
consumed by the NEXT instruction (§4.6), and every consumer of operands
(`use_reg`, `put`, `use_mem`, `release`, the `phi` stores, the call
arguments, the declaration rows) relies on that; an alias across
instructions would change all of them, and the line table is the same
either way.

### 5.9 Key instructions (implemented, WP-149)

clang 22 marks a function's subprogram `keyInstructions: true` and gives
its instructions' locations an **atom**: `atomGroup` (a source atom: a
statement, an assignment, a branch; unique in the function and its
inlined-at chain) and `atomRank` (1 the most important instruction of the
atom, 0 none). It does so at `-O1` and above (also with `-Xclang
-disable-llvm-passes`) and with `-gkey-instructions`; not at plain
`-O0`. kiln++ emits none at `-O0`, `-O1` or `-O2` and rejects
`-gkey-instructions`. `llc` 22 then writes `is_stmt` on the rows of the
atoms' KEY instructions only (`DwarfDebug::computeKeyInstructions`,
honoured at every optimisation level, including `-O0`), instead of on
every change of line (§5.2). Both readers keep the fields
(`spec/ll.md`, `spec/bc.md`, `spec/ir.md` §6.5); the rows are chosen in
`crates/hod_cg/src/key_rows.rs` and written by `function_rows`
(`hod_dwarf::Stmt`).

The rule, as `llc` applies it to its machine instructions in code order,
block by block (K1 to K6; the rows of a function whose subprogram has
`keyInstructions` are its "instructions", K6):

- K1. An instruction without a location or at line 0 is not considered.
- K2. The **buoy** is the first instruction of a run of instructions at
  one line in one block: a new block, a different line, or a call (K4)
  ends it, and the next considered instruction is the new buoy.
- K3. An instruction with an atom (group and rank not 0) whose group
  differs from the atom the buoy serves becomes the buoy itself. Then,
  per atom `(inlined at, group)`: a rank worse than the best seen is
  ignored; a better rank drops the atom's candidates; the buoy becomes a
  candidate (an equal rank drops none: `llc` 22's `remove_if` is not
  followed by an `erase`, measured), and the buoy now serves the atom.
- K4. A call (or tail call) makes its buoy a key instruction outright and
  ends the buoy. With an atom it drops the atom's candidates of a worse
  rank but is no candidate itself (the atom's best rank is 0 again when it
  has none left).
- K5. At the end of the function every candidate is a key instruction.
  `is_stmt` is on the key instructions' rows, and on the prologue end
  (§5.2) and the start row (§5.1) as before; on no other row. A key
  instruction's row is written even at the location of the row before.

Measured against `llc` 22's object (a model of K1 to K5 over `llc`'s final
machine code, `-stop-after=unpack-mi-bundles`, gives exactly `llc`'s
`is_stmt` rows in 3 160 of 3 160 AArch64 functions of 250 gcc torture
sources at `-O0 -gkey-instructions`, `-O1 -Xclang -disable-llvm-passes`,
`-O1` and `-O2`).

hod's rows map to `llc`'s instructions:

- K6. In a key-instructions function the driver records a row for every
  segment that emitted code (§5.8), not only for a change of location: a
  row stands for the machine instructions `llc` selects from one IR
  instruction. A row whose code ends with a call (`FnCx::call_returns_here`,
  and a `musttail` call's branch) is a call (K4), and the code after the
  call opens a row of its own, so the return address has one. The row of
  a tail exit, and the epilogue's row when the `ret` was not in the last
  block emitted, continue code emitted elsewhere and are no instruction.
  A call of `_Unwind_Resume` is no call: `llc` makes it from a `resume`
  without a location (`DwarfEHPrepare`).
- K7. Refused, by name, before any row is written: a function with
  locations of subprograms of both kinds (code inlined from a function
  compiled without key instructions, or the other way round:
  `debug:key-instructions-mixed`), and a coroutine body with key
  instructions (`debug:key-instructions-coroutine`: `llc` sees it only
  after `CoroSplit`).

Permitted differences (§5.7), recognised by `cargo xtask debug`:

- `prologue-end`: hod's `(function, file, line)` pair whose only
  `is_stmt` row is its prologue end, in a function with key instructions
  (`keyInstructions: true` in the IR). Both programs mark the prologue end
  a statement; where their first code differs (hod writes no code for
  `__builtin_prefetch`; `llc`'s x86-64 prologue of a variadic function
  saves the argument registers at the scope line) the line differs, and
  without key instructions the other line's change of line hides it.
- Differences inside one line run (an `is_stmt` row more or fewer on
  the line of the row before) are not pairs. `cargo xtask debug columns`
  counts the functions whose sequence of `is_stmt` rows equals `llc`'s
  (rows of unconditional branches only left out, P1); measured classes:
  an empty `asm` statement (`llc` writes a row for it, at the next
  instruction; hod has no code for it), and a prologue end at a line
  `llc` keys at another instruction. gdb's `step` and `next` stop at
  neither (gdb skips `is_stmt` rows of the line it is stepping).

Not done: `llc` places calls hod makes itself and `llc` does not (or the
other way round: `memcpy` for a copy hod inlines up to 64 bytes, the
x86-64 and AArch64 TLS descriptor calls that hod's targets emit
directly) by their own rules; such a call is a K4 call on one side only.
Measured: the TLS tests (`tls.c`, `tls_static.c`, `dwarf4_tls.c`) have
the same `is_stmt` sequence; `memcpy-1.c` and `memcpy-bi.c` (gcc
torture) differ only inside line runs (more `is_stmt` rows on a line
whose copy one side calls and the other inlines), the class above.

## 6. DIEs (core implemented, WP-053; the type graph, WP-058; functions, WP-057)

### 6.1 Abbreviations: a fixed table (implemented)

hod writes `.debug_abbrev` from a table built at compile time of hod
(`src/abbrev.rs`). Reasons: no hashing or interning per DIE; the codes
are known before any DIE is written, so per-function DIE runs can be
written in parallel and concatenated; and the table is the same for every
object.

**Families.** Every DIE shape is a *family*: a tag, whether it has
children (no, yes, or optional), and an ordered list of attribute slots.
A slot is either always present or controlled by one option bit; several
slots may share a bit (`decl_file` with `decl_line`). A family owns a
fixed block of codes, `base .. base + block`. A DIE's code is its family's
base plus the mask of its option bits, plus, when children are optional,
a children bit above them. So the code is a few ORs, and every
combination of options has its abbreviation without a table lookup.
`.debug_abbrev` holds only the codes the object used, in code order, and
ends with the 0 entry.

**Forms** are fixed per slot: strings `DW_FORM_strx` (a ULEB128 index,
variable length yet one form); addresses `DW_FORM_addrx`; references
`DW_FORM_ref4`; file, line, sizes and alignment `DW_FORM_udata`;
encodings and accessibility `DW_FORM_data1`; `DW_AT_language` `data2`
(as `llc`); flags `DW_FORM_flag_present` (an absent flag is a different
code); locations `DW_FORM_exprloc`; `DW_AT_high_pc` `data4`; the bases
`DW_FORM_sec_offset`. `llc` picks the smallest fixed form per value
(`strx1`, `data1`), which makes its abbreviation set depend on the
values; hod's ULEB forms are as small for small values and need no
choice.

**Extending it.** The table is append-only: a family's position and
block never change, so codes never renumber. A package that needs a
shape no family has appends a family at the end. The shapes of M3's later
packages are listed and reserved now, each with its owner; a reserved
family may have its slots refined by its owner within its block (a slot
added under a new bit, if the block has room). The unit tests
(`abbrev::tests`) pin the bases, check that every code in `1..CODE_END`
gives a valid abbreviation (no attribute twice, except the CU's two
exclusive `low_pc` forms), and pin the bytes of one entry.

| # | family | block | base | owner |
|---|---|---|---|---|
| 0 | `formal_parameter` | 16 | 1 | WP-057 |
| 1 | `variable` (local) | 8 | 17 | WP-057 |
| 2 | `member` | 32 | 25 | WP-058 |
| 3 | `subprogram` (declaration) | 32 | 57 | WP-058 |
| 4 | `subprogram` (definition) | 32 | 89 | WP-057 |
| 5 | `pointer_type` | 8 | 121 | WP-053 |
| 6 | `base_type` | 2 | 129 | WP-053 |
| 7–12 | `const`, `reference`, `rvalue_reference`, `volatile`, `restrict`, `atomic` types | 2 each | 131–141 | WP-058 |
| 13 | `typedef` | 8 | 143 | WP-058 |
| 14–18 | template type parameter, value parameter, value parameter (address), `GNU_template_parameter_pack`, `GNU_template_template_param` | 8, 16, 4, 4, 2 | 151–183 | WP-059 |
| 19 | `inheritance` | 4 | 185 | WP-058 |
| 20–25 | `structure`, `class`, `union` types, then their declarations | 32 each, 4 each | 189–293 | WP-058 |
| 26–34 | `enumeration_type`, `enumerator`, `array_type`, `subrange_type`, `subrange_type` (variable count), `subroutine_type`, `unspecified_parameters`, `ptr_to_member_type`, `unspecified_type` | 32, 4, 2, 8, 2, 4, 1, 2, 2 | 297–352 | WP-058 |
| 35 | `subprogram` (out-of-line definition) | 16 | 354 | WP-059 |
| 36–37 | `subprogram` (virtual declaration; special member declaration) | 32, 256 | 370, 402 | WP-058 |
| 38–39 | `lexical_block`, `label` | 4, 4 | 658, 662 | WP-057 |
| 40 | `member` (static, as `DW_TAG_variable` declaration) | 16 | 666 | WP-058 |
| 41–44 | `variable` (specification), `variable` (constant), `imported_module`, `imported_declaration` | 4 each, 2 | 682–692 | WP-059 |
| 45 | `friend` | 2 | 696 | WP-058 |
| 46 | `namespace`: `name` (bit 0), `export_symbols` (bit 1), children optional | 8 | 698 | WP-053 |
| 47 | `variable` (global): `name`, `type`, `external` (0), `decl_file`/`decl_line` (1), `alignment` (2), `location` (3), `linkage_name` (4) | 32 | 706 | WP-053 |
| 48 | `compile_unit`: `producer`, `language`, `name`, `str_offsets_base`, `stmt_list`, `comp_dir`, `low_pc` addrx + `high_pc` (0), `low_pc` addr + `ranges` + `rnglists_base` (1), `addr_base` (2), children optional | 16 | 738 | WP-053 |
| 49 | `base_type` (any: `bit_size` for `dataSize`, `endianity`) | 8 | 754 | WP-058 |
| 50–51 | `typedef` (any: `alignment`, accessibility, declaration), `template_alias` | 16, 8 | 762, 778 | WP-058 |
| 52–53 | `member` (any: bit fields, `alignment`, artificial, accessibility, declaration), `inheritance` (virtual: an `exprloc` offset and `virtuality`) | 256, 2 | 786, 1042 | WP-058 |
| 54–55 | static member (any), as `DW_TAG_variable` (DWARF 5) and as `DW_TAG_member` (DWARF 4 units) | 64, 64 | 1044, 1108 | WP-058 |
| 56–61 | `structure`, `class`, `union` types (any: `export_symbols`, `containing_type`, `calling_convention`, `alignment`, accessibility), then their declarations (any) | 256 each, 8 each | 1172–1956 | WP-058 |
| 62–64 | `enumeration_type`, `array_type` (`GNU_vector`, `byte_size`), `subroutine_type` (`prototyped`, `calling_convention`, `reference`, `rvalue_reference`), each any | 512, 16, 64 | 1964, 2476, 2492 | WP-058 |
| 65 | `subprogram` (member declaration): `linkage_name`, `name`, `decl_file`/`decl_line`, `type`, `virtuality` + `vtable_elem_location` + `containing_type`, `declaration`, `artificial`, `external`, accessibility, `explicit`, `deleted` | 2048 | 2556 | WP-058 |
| 66 | `subprogram` (member declaration, qualified): as 65 without `explicit`, with `reference`, `rvalue_reference`, `noreturn` before accessibility | 4096 | 4604 | WP-058 |
| 67 | `variable` (global, no name: a structured binding's hidden object) | 32 | 8700 | WP-058 |
| 68 | `base_type` (the array index type, `__ARRAY_SIZE_TYPE__`: `name`, `byte_size`, `encoding`, in `llc`'s order) | 1 | 8732 | WP-058 |
| 69 | `subprogram` (definition, any): `low_pc`, `high_pc`, `frame_base`, then `linkage_name` (0), `name` (1), `decl_file`/`decl_line` (2), `prototyped` (3), `type` (4), `artificial` (5), `external` (6), `noreturn` (7), children optional | 512 | 8733 | WP-057 |
| 70 | `subprogram` (member definition): `low_pc`, `high_pc`, `frame_base`, `object_pointer` (0), `decl_file` (1), `decl_line` (2), `linkage_name` (3), `specification`, children optional | 32 | 9245 | WP-057 |
| 71 | `label` (with `decl_column`, LLVM 21 and later), `low_pc` (0) | 2 | 9277 | WP-057 |
| 72 | `subprogram` (abstract instance): `linkage_name` (0), `name` (1), decl (2), `prototyped` (3), `type` (4), `artificial` (5), `external` (6), `noreturn` (7), `inline`, children optional | 512 | 9279 | WP-085 |
| 73 | `subprogram` (abstract member): `decl_file` (0), `decl_line` (1), `linkage_name` (2), `specification`, `inline`, `object_pointer` (3), children optional | 32 | 9791 | WP-085 |
| 74 | `subprogram` (of an abstract instance): `low_pc`, `high_pc`, `frame_base`, `object_pointer` (0), `abstract_origin`, children optional | 4 | 9823 | WP-085 |
| 75 | `inlined_subroutine`: `abstract_origin`, `low_pc`/`high_pc` (0), `ranges` (1), `call_file`, `call_line`, `call_column` (2), children optional | 16 | 9827 | WP-085 |
| 76 | `lexical_block` (of an abstract instance): `low_pc`/`high_pc` (0), `ranges` (1), `abstract_origin`, children | 4 | 9843 | WP-085 |
| 77 | `lexical_block` (abstract), children | 1 | 9847 | WP-085 |
| 78 | `formal_parameter` (of an abstract instance): `location`, `abstract_origin` | 1 | 9848 | WP-085 |
| 79 | `variable` (of an abstract instance): `location`, `abstract_origin` | 1 | 9849 | WP-085 |
| 80 | `label` (of an abstract instance): `abstract_origin`, `low_pc` | 1 | 9850 | WP-085 |
| 81 | `variable` (global, any): `name`, `type`, `external` (0), decl (1), `declaration` (2), `alignment` (3), `location` (4), `const_value` udata (5), sdata (6), `linkage_name` (7), children optional | 512 | 9851 | WP-059 |
| 82 | `variable` (specification, any): `specification`, `type` (0), `alignment` (1), `location` (2), `const_value` udata (3), sdata (4), `linkage_name` (5), children optional | 128 | 10363 | WP-059 |
| 83 | `template_value_parameter` (any): `type` (0), `name` (1), `default_value` (2), `const_value` udata (3), sdata (4), `location` (5) | 64 | 10491 | WP-059 |
| 84 | `GNU_template_template_param` (any): `name` (0), `default_value` (1), `GNU_template_name` | 4 | 10555 | WP-059 |
| 85 | `template_alias` (with parameters): 51's slots, children | 8 | 10559 | WP-059 |
| 86 | `subprogram` (specification, any): `low_pc`, `high_pc`, `frame_base`, `object_pointer` (0), `type` (1), `decl_file` (2), `decl_line` (3), `linkage_name` (4), `specification`, children optional | 64 | 10567 | WP-059 |
| 87 | `variable` (local, unnamed): `location` (0), decl (1), `type`, `artificial` (2) | 8 | 10631 | WP-087 |
| 88 | `variable` (local, aligned): `location` (0), `name`, `alignment`, decl (1), `type`, `artificial` (2) | 8 | 10639 | WP-087 |
| 89 | `subprogram` (line tables only, §6.13): `low_pc`, `high_pc`, `name` (0), `abstract_origin` (1), children optional | 8 | 10647 | WP-094 |
| 90 | `label` (any, §6.14): `name` (0), decl (1), `decl_column` (2), `artificial` (3), `LLVM_coro_suspend_idx` (4), `low_pc` (5) | 64 | 10655 | WP-098 |
| 91 | `skeleton_unit` (§6.15): `stmt_list`, `str_offsets_base`, `comp_dir`, `dwo_name`, `low_pc`, `high_pc` (0), `addr_base` (1) | 4 | 10719 | WP-062 |
| 92 | `skeleton_unit` (ranges): `stmt_list`, `str_offsets_base`, `comp_dir`, `dwo_name`, `low_pc`, `ranges`, `addr_base`, `rnglists_base` | 1 | 10723 | WP-062 |
| 93 | `skeleton_unit` (no split unit): `stmt_list`, `str_offsets_base`, `comp_dir`, `producer`, `language`, `name`, `low_pc`, `high_pc` (0), `addr_base` (1) | 4 | 10724 | WP-062 |
| 94 | `skeleton_unit` (no split unit, ranges): 93's fixed attributes, `low_pc`, `ranges`, `addr_base`, `rnglists_base` | 1 | 10728 | WP-062 |
| 95 | `compile_unit` (split, `.debug_info.dwo`): `producer`, `language`, `name`, `dwo_name`, children optional | 2 | 10729 | WP-062 |
| 96 | `subprogram` (definition, calls, §6.19): family 4 with `call_all_calls` after `frame_base` | 32 | 10731 | WP-113 |
| 97 | `subprogram` (any, calls): family 69 with `call_all_calls` | 512 | 10763 | WP-113 |
| 98 | `subprogram` (member definition, calls): family 70 with `call_all_calls` | 32 | 11275 | WP-113 |
| 99 | `subprogram` (specification, typed, calls): family 35 with `call_all_calls` after `object_pointer` | 16 | 11307 | WP-113 |
| 100 | `subprogram` (specification, any, calls): family 86 with `call_all_calls` after `object_pointer` | 64 | 11323 | WP-113 |
| 101 | `subprogram` (of an abstract instance, calls): family 74 with `call_all_calls` | 4 | 11387 | WP-113 |
| 102 | `call_site`: `call_origin` (0), `call_target_clobbered` (1), `call_tail_call` and `call_pc` (2), `call_return_pc` (3) | 16 | 11391 | WP-113 |
| 103 | `subprogram` (declaration, C): `linkage_name` (0), `name`, decl (1), `prototyped`, `type` (2), `declaration`, `external` (3), `noreturn` (4), children optional | 64 | 11407 | WP-113 |
| 107 | `formal_parameter` (location list, §6.23): family 0 with `location` a `sec_offset` | 16 | 11482 | WP-162 |
| 108 | `variable` (local, location list): family 1 with `location` a `sec_offset` | 8 | 11498 | WP-162 |
| 109 | `variable` (local, unnamed, location list): family 87 likewise | 8 | 11506 | WP-162 |
| 110 | `variable` (local, aligned, location list): family 88 likewise | 8 | 11514 | WP-162 |
| 111 | `formal_parameter` (of abstract, location list): `location` (0, a `sec_offset`), `abstract_origin` | 2 | 11522 | WP-162 |
| 112 | `variable` (of abstract, location list): likewise | 2 | 11524 | WP-162 |

Families 2 to 45 that WP-058 owns are used for the common shape of each
kind (they keep the codes the reserved shapes promised); the "any"
families 49 to 66 take every other combination, so the codes stay a few
ORs. Subroutine parameters use WP-057's `formal_parameter` family (its
`type` and `artificial` slots). WP-058 refined the slots of its own
reserved families 29 (`lower_bound` before `count`) and 36–37 to `llc`'s
order.

WP-057 uses its reserved families 0 (`formal_parameter`), 1 (local
`variable`), 4 (the common definition: no `prototyped`, `artificial` or
`noreturn`, a name), 38 (`lexical_block`; bit 0 chooses
`low_pc`/`high_pc` or `ranges` as `DW_FORM_sec_offset`) and 39 (`label`),
and appends 69 to 71 for the other shapes.

`CODE_END` is 11471 (754 after WP-053, 8733 after WP-058, 9279 after WP-057, 9851 after WP-085, 10631 after WP-059, 10647 after WP-087, 10655 after WP-094, 10719 after WP-098, 10731 after WP-062; 11526 after WP-162, whose families 107 to 112 follow WP-163's 106). Since the core families come last, their codes take
two ULEB bytes where `llc`'s take one (the cost is in §7.2; open question
7).

### 6.2 Writing straight from the tables (implemented)

No DIE tree of nodes is built. `UnitPlan::new` makes one pass over the
unit's `globals`, then its `enums`, then its `retainedTypes` (a
subprogram among them is WP-057's and is skipped), as `llc` does. Types
are made by the type graph (§6.8). For a global: for each global, its namespace chain from the outermost
(a cycle is `BadReference`), then the variable, then its type chain, each
node getting a DIE the first time it is reached. A DIE is a node id, a
parent index and a kind (twelve bytes); the node-to-DIE map is four bytes
per node of the table. The parents' runs of children come from a stable
counting sort of the DIEs by parent. This discovery order is `llc`'s, so
the trees come out in `llc`'s order (§8.6).

The writer walks the tree depth first with an explicit stack and writes
each DIE from its node's record into the output buffer, with a 0
terminator after each run of children (a DIE with no children has the
no-children code, so it gets no terminator). A `DW_FORM_ref4` to a DIE
already written is written at once; a forward one is recorded as a patch
`(position, DIE)` and filled in when the unit ends. The function DIEs
(WP-057) are DIEs of the same tree, added by `add_function` (§6.4) and
written by the same walk.

### 6.3 Strings (implemented)

`DebugStrings` keeps `.debug_str` pooled per object, so each distinct
string is stored once, and keeps `.debug_str_offsets` per unit, so each
distinct string the unit names has one entry, in order of first use by
the walk. The walk order is deterministic, so the indices are too. One
hash table keyed by the bytes serves both. A string's entry remembers the
last unit that named it and its index there, so a lookup is one probe,
whether the string is new to the object, new to the unit, or neither.

The hash takes eight bytes per multiply, Fx style, and ends with a mix.
Without that mix, the low bits of the product (the bucket) depend only on
the low bits of the last word. 65,536 names `v0`…`v65535` then shared 43
buckets; with it they fill 41,542 of 65,536, which is what a random
function gives (`strings::tests::hash_spreads`). A string with a NUL is
`NulInName`.

Planned optimization: `hod_ir`'s names are already interned, so a pool
keyed by `hod_ir`'s string id would index an array instead of hashing.
`hod_ir` does not expose that id from a decoded node yet (§2).

### 6.4 Subprograms, variables, blocks and labels (implemented, WP-057)

**Interface** (`src/func.rs`). `UnitPlan::for_functions(&module, unit)`
is `of_module`'s plan left open. The code generator adds each placed
function, in module order, with `add_function(&FunctionInput)`, then
calls `seal()`; `write_debug_info` takes only a sealed plan. A
`FunctionInput` is the `DISubprogram`, the `CodeSpan`, the DWARF register
of the frame base, the function's line-table transitions (`ScopeRow`:
code offset and `LocId`) and its `LocalRow`s. A `LocalRow` is a
`DILocalVariable` or `DILabel`, the location of its record, the
record's `DIExpression`, and a `LocalPlace`: `Frame(off)` (the variable
is `off` bytes from the frame base), `FrameIndirect(off)` (its address is
stored there), `Label(code offset)`, or `Refused(name)`, a stable name
the code generator gave for a row it cannot describe. Nothing is
allocated per DIE: a function's scopes, ranges and entities live in
scratch reused across functions, and the writer reads three side tables
sorted by DIE (functions, locals, block ranges).

**The subprogram DIE**, as `llc` writes it at `endFunction`:
- `low_pc` (`addrx` of the function's section symbol plus offset, §1.3),
  `high_pc` (the size, `data4`), `frame_base` (`DW_OP_reg<n>`, or
  `DW_OP_regx`; hod's is x29 on AArch64);
- a function that is not a member: `linkage_name`, `name`, `decl_file`/
  `decl_line`, `prototyped` (C only), `type` (the return type; none for
  `void`), `artificial`, `external` (unless `DISPFlagLocalToUnit`),
  `noreturn`. Its parent is its namespace (made as for a global) or the
  unit;
- a member function's definition (a `declaration:` whose scope is a
  class): the class is made first (the declaration is among its members),
  then the definition is a child of the unit with `specification` to the
  declaration, `object_pointer` to its `this` parameter, and
  `decl_file`, `decl_line` and `linkage_name` only where they differ from
  the declaration's (`llc`'s `applySubprogramDefinitionAttributes`);
- children in `llc`'s order: the parameters (by `arg:`), the locals (in
  record order), the labels, then the blocks; a variadic definition (its
  type's last element `null`) ends with `DW_TAG_unspecified_parameters`.

The return types are made at `seal()` (`finishSubprogramDefinitions`),
then the variables' types in DIE order (`finishEntityDefinitions`), so
the type DIEs come after every function, in `llc`'s order.

**Variables.** `name`, `decl_file`/`decl_line`, `type`, `artificial`
(the parameters' slot), and `location`: `DW_OP_fbreg off` (plus
`DW_OP_deref` for `FrameIndirect`) followed by the record's
`DIExpression`, each op written as itself. LLVM's own ops
(`DW_OP_LLVM_*`), `stack_value`, conversions, entry values and a
fragment are refused (§ below). `llc -O0` writes the same location for
a slot as `DW_OP_fbreg` from its own frame base, so locations are
compared by behaviour, under the debuggers (§6.7, §8.8).

**Retained variables** (WP-111, `retained_vars`): a `DILocalVariable`
in the subprogram's `retainedNodes` that no row of the function names
gets a DIE without `location` (the families' location bit clear), in
its own scope when that scope has code (else none), after the scope's
declared variables (a parameter in its number's place), its type made
after the function's other variables' (`llc`'s `collectEntityInfo`
makes the retained nodes' entities last). A `CoroSplit` ramp retains
`__coro_frame` and declares it only on a `phi`, which neither `llc` nor
hod describes (`spec/codegen.md` §9.5): gdb prints `<optimized out>`,
lldb `<no location, value may have been optimized out>`, under both. A
retained label without a record is not written (no producer emits one
at `-O0`); a function with an abstract instance and retained nodes is
refused (`debug:retained-nodes`, §6.9).

**Lexical blocks** (`scope_ranges`). `llc`'s `LexicalScopes` over the
function's transitions: for each transition with a location, the scopes
from its scope out to the first open one are opened (outermost first);
every open scope not on that chain is closed at the end of the last
located code. Code without a location belongs to whatever range is open
around it; a `DILexicalBlockFile` stands for its enclosing block. A
scope's ranges are the intervals it was open, so a block whose code is
interrupted by its parent's has several. A block with one range gets
`low_pc`/`high_pc`, one with several `DW_AT_ranges`, a list in
`.debug_rnglists` (`DW_RLE_base_addressx` of the function, then one
`DW_RLE_offset_pair` per range). Since WP-163 the attribute is a
`DW_FORM_rnglistx` index, as `llc` writes it in every DWARF 5 unit: the
unit's table has one offset per list, the blocks' and inlined
subroutines' lists numbered in DIE order and the unit's own (when its
code is several spans) last, and the unit DIE has
`DW_AT_rnglists_base` (with one span, family 106 "compile_unit (scope
lists)": `low_pc`, `high_pc`, `addr_base`, `rnglists_base`). The index
is a ULEB128 and needs no relocation: before WP-163 each list was a
four-byte `DW_FORM_sec_offset` with an `ABS32` against
`.debug_rnglists`. The unit's own index is the number of scope lists,
which the plan counts before the unit DIE is written
(`UnitPlan::scope_lists`; a writer that wrote another number is an
internal error). Families 38, 75 and 76 name `DW_FORM_rnglistx` for
`DW_AT_ranges` since WP-163 (the only change of an existing family's
form: no code moves, and DWARF 4, §6.20, writes `sec_offset` for it as
before).
Linear in the transitions plus the scopes opened; no recursion, so a
scope chain 20,000 deep costs what 20,000 siblings do (§8.8).

A block is written only when it has variables (`needToEmitLexicalScope`):
a block with only labels, or with nothing, is flattened, and its labels
and inner blocks go to its parent. A variable or label whose scope never
has code is dropped, as `llc` drops it.

**Labels**: `name`, `decl_file`/`decl_line`, `decl_column` when the
`DILabel` has a column (LLVM 21 and later write it; family 71), and
`low_pc` (`addrx` of the function's symbol plus the label's offset).

**Function-local types** reached from a variable are nested under the
subprogram (or block) DIE by the type graph (§6.8), since the function's
DIE exists by the time `seal()` makes them.

**Refused**, as `Unsupported { node, what }` with these names (the bridge
reports a `debug:*` name as itself and §6.5's unnamed shapes as
`debug:die-unsupported`, §9; the code generator's own names are in
`spec/codegen.md` §9.5):

| name | what |
|---|---|
| `debug:subprogram-not-definition` | a function whose `DISubprogram` is not a definition of this unit |
| `debug:subprogram-attribute` | flags beyond prototyped, artificial, noreturn, local-to-unit, optimized; thrown types |
| `debug:call-sites`, `debug:call-sites-line-tables`, `debug:call-site-*` | what of `DIFlagAllCallsDescribed` hod does not write: a unit that is not DWARF 5, a line-tables-only unit with inlined code, and the callees and calls §6.19 lists (call-site entries are written since WP-113) |
| `debug:subprogram-scope` | a definition scoped in something that is not a namespace or the unit |
| `debug:subprogram-twice` | two functions with one `DISubprogram` |
| `debug:out-of-line-definition` | a definition with a `declaration:` not scoped in a class (WP-059) |
| `debug:specification` | a member definition whose declaration is not among its class's elements (`llc` makes it on demand; member templates) |
| `debug:specification-type` | a definition whose return type differs from its declaration's |
| `debug:foreign-scope` | a variable or row in another function's scope |
| `debug:fragment` | a `DW_OP_LLVM_fragment` in a declared variable's expression |
| `debug:dbg-value-fragment`, `debug:dbg-value-expr` | a value record's expression with a fragment, or one §6.23 does not write (WP-162) |
| `debug:dbg-value-arglist`, `debug:dbg-value-coroutine` | a value record with a `DIArgList`; one in a coroutine body (`hod_cg`, §6.23) |
| `debug:expression` | any other op that is not a plain DWARF op |
| `debug:variable-without-type`, `debug:variable-twice` | as named |
| `debug:variable-alignment` | an `alignas` parameter, or an unnamed one; an `alignas` variable of an abstract instance (§6.9). A named local's `alignment` is written since WP-087 (family 88, in bytes, after `name`) |
| `debug:label-attribute` | no longer refused: every label is written since WP-098 (§6.14) |
| `debug:object-pointer` | two `this` parameters |
| `debug:local-type-scope` | a function-local type reached before its function was added, when the type is scoped in a block, or the function has no code in this module (in `types.rs`, `early.rs`; §6.12) |

Inlined code and functions' statics are §6.9's (WP-085), with its own
refusals. `LineTablesOnly` and `DebugDirectivesOnly` units are §6.10 (WP-061).

### 6.5 What the core writes (implemented)

- **The unit header**: `DW_UT_compile`, version 5, address size 8, 32-bit
  `abbrev_offset` against `.debug_abbrev`.
- **The compile unit DIE**, as `llc -O0` writes it:
  - `producer`, `language`, `name` and `comp_dir` (from the unit's
    `DIFile`), `str_offsets_base` and `stmt_list`;
  - for code in one section, `low_pc` (`addrx`) and `high_pc` (the
    length, `data4`);
  - for code in several sections, `low_pc` 0 (`DW_FORM_addr`, no
    relocation) and `ranges` (`rnglistx`, the last list of the unit's
    table, §6.4) with `rnglists_base`; for code in one section and
    scopes with range lists, `low_pc`, `high_pc`, `addr_base` and
    `rnglists_base` (family 106, WP-163);
  - `addr_base` when the unit has an address.
  The unit's range list has one `DW_RLE_startx_length` per merged span,
  where `llc` uses `base_addressx` and `offset_pair` for `.text` (§6.7).
- **Global variables**:
  - `name`, `type`, `external` (unless `isLocal`), `decl_file` and
    `decl_line` (when `line` is not 0), `alignment` in bytes (when
    `align` is set);
  - `location` as `DW_OP_addrx` of the variable's `.debug_addr` entry
    (when the caller gave its address; without one the variable was
    optimized out and has no location);
  - `linkage_name` (when set).
- **Namespaces**: `name` (absent for an anonymous one) and
  `export_symbols` (inline), with their children.
- **Base types**: `name`, `encoding`, `byte_size`.
- **Pointer types**: `type` (absent for `void *`), `name` (when set).
- **`.debug_addr`**, per unit: the globals' entries in DIE order, then
  one per merged code span. Each entry is relocated `ABS64` /
  `R_X86_64_64` against the symbol the caller gave (the variable's own,
  or its section's for an internal one, as `llc` does; a function's
  section symbol plus offset, §1.3). Spans of one symbol that touch or
  overlap are merged before numbering.
- **`.debug_rnglists`**: written only when some unit's code is in several
  sections.

All debug sections stay outside every group (§1.3).

Anything else the plan meets is `Unsupported { node, what }`, not a
wrong DIE:

- a unit whose `emissionKind` is `NoDebug` (the bridge gives the plan
  none; other reduced kinds are §6.10);
- a variable with several expressions, a non-empty expression, no
  type, a line without a file, or template parameters (a variable with
  no name is written without one since WP-058, as `llc` does);
- a declaration or a static member's definition;
- a global's scope other than a namespace or the unit, or a namespace
  inside anything else;
- the type shapes of §6.8's refusals.

These are the later packages' shapes (§10).

### 6.6 Errors (implemented)

`UnitPlan::new` fails with:
- `NotACompileUnit` if the node is not a unit;
- `BadReference { node, field }` if a link names no node, names a node of
  the wrong kind, or forms a namespace cycle;
- `MemberOutsideType { node }` for a member (a member, an inheritance, a
  friend, a static member, a member function) whose scope is not a class,
  struct or union (WP-058);
- `TypeCycle { node }` for a type that contains itself by value: a
  typedef, qualifier or array cycle not broken by a pointer, a
  reference or a class (WP-058);
- `MissingSize { node }` for a base type, a defined enumeration, a
  vector or a bit field of size 0 (WP-058);
- `Unsupported` (§6.5, §6.8).

`write_debug_info` fails with:
- `NoUnit` for no units;
- `GlobalNotInUnit` if an address names an expression that is not one of
  the unit's globals;
- `DuplicateGlobal` if an expression is given twice;
- `BadSymbol` if a symbol is not in the object;
- `BadSpan` for a negative start, or an end past `i64::MAX`;
- `NulInName`, `TooLarge` (a section of 4 GiB or more), or `Object`.

Every check runs before the object is touched: on an error the object is
unchanged (tested for each error). The writer panics only on its own
broken invariants (`unreachable!` where the plan made both a DIE's kind
and its node).

### 6.7 Permitted differences from `llc` (implemented)

The `llc` comparison (§8.6) decodes both objects with `llvm-dwarfdump
--debug-info` and compares the DIE trees after this normalization, which
is the whole list of permitted differences:

- **abbreviation codes and forms**: `strx`/`udata` against
  `strx1`/`data1`, and two-byte codes for the core families. The dump
  prints values, not forms, so these vanish;
- **the offset of a reference**: a reference is compared by the name its
  target prints; `DW_AT_count` and `DW_AT_friend`, which the dump prints
  as bare offsets, by the `DW_AT_name` of the DIE at that offset (WP-058);
- **string and address indices**: `DW_OP_addrx` is compared by the
  relocation target of its entry (section and offset of the symbol plus
  addend);
- **the radix of a constant** (`0x8` against `8`);
- **sibling and attribute order**: compared as sorted. hod in fact
  matches `llc`'s order on every compared object;
- **the encoding of the unit's range list** (startx_length for every
  span). No compared module has code, so this is not seen there; it is
  checked by `--verify` in `tests/dies.rs`;
- **`DW_AT_decl_file` numbers**: hod's file table holds every `DiFile` of
  the module, `llc`'s only the files it uses. The dump prints the file's
  name, which is equal;
- **not written**: Apple and sysroot attributes and name tables (§1.1);
- **the oracle input of the type graph** (WP-058, `tests/common/types.rs`):
  `hod_dwarf`'s own tests write no function DIEs (they have no code;
  WP-057's are compared in `hod_a64`, §8.8), and template parameters and
  imported entities are WP-059's, so a module with them is compared as a module
  that has none. Functions' bodies are deleted (`llvm-extract`), and the
  types of their locals and of the definitions join `retainedTypes`
  unless they reach something function-local. Globals that are
  declarations or static members' definitions, have an expression, or
  are scoped in a class are dropped, as are `imports` and function-local
  `enums` and `retainedTypes`. `private` globals become `internal`, and a
  unit left with no global gets `hod_oracle_anchor`, or `llvm-extract`
  would delete the unit. `llc` reads the result without `templateParams`.
  Both writers read the same module, so this narrows what is compared and
  changes nothing in it;
- **sizes**: hod's `.debug_info` is 1.07 to 1.16 times `llc`'s on the type
  graph (§7.2): every type family's code is two bytes, and a string index
  or a line from 128 to 255 is two ULEB bytes where `llc` uses `strx1`
  or `data1`;
- **function DIEs** (WP-057, §8.8), compared on hod's whole object
  against `llc`'s from the same IR:
  - `low_pc`, `high_pc`, `ranges` and `location` are masked in the tree
    comparison: hod's code is not `llc`'s, so addresses, sizes and frame
    offsets differ. A scope that is one range in one writer's code and
    several in the other's (`low_pc`/`high_pc` against `ranges`) is
    therefore equal (WP-098, §6.14; the unit's own are compared). They are compared by behaviour instead, under gdb and
    lldb (the sessions must be equal);
  - `frame_base`: hod's is `DW_OP_reg29` (x29) on AArch64; `llc` writes
    `DW_OP_reg29` or `DW_OP_reg31`;
  - `object_pointer` is compared as present, not by offset;
  - a block's range list was `DW_FORM_sec_offset` into
    `.debug_rnglists` where `llc` writes `DW_FORM_rnglistx` and a unit
    `rnglists_base` (until WP-163, which writes `llc`'s forms, §6.4); the
    unit's `rnglists_base` is still not compared;
  - the line table under function DIEs keeps §5.7's differences (P1),
    which the debugger sessions avoid by breaking at lines, not at
    `llc`'s branch rows.
- **inlined code** (WP-085, §6.9; `tests/dies_inline*.rs`):
  - `DW_AT_inline` is `data1` (1, `DW_INL_inlined`) where `llc` writes
    `DW_FORM_implicit_const`: the dump prints the value;
  - a block's `DW_AT_abstract_origin`, which the dump prints as a bare
    offset, is compared by the tag of its target (since WP-098 any
    reference printed bare, §6.14: the target of a nameless reference
    moves with the sizes before it);
  - `llc` 19 writes no `DW_AT_abstract_origin` on a concrete block
    (added in LLVM 21); hod always writes it, so `llc` 19's trees are
    compared with hod's blocks' attribute dropped, and its gdb sessions
    without the `<optimized out>` line gdb then adds for the abstract
    block's variable, which it cannot match to the concrete one;
  - the static of a function that is only inlined: `llc` 19 puts it
    under an empty `DW_TAG_subprogram` of its own, after the abstract
    instance (`llc` 22, and hod, under the abstract instance). `llc`
    19's tree of `statics.cpp` is not compared.
- **call-site entries** (WP-113, §6.19): `DW_AT_call_return_pc`,
  `DW_AT_call_pc` and `DW_AT_call_target_clobbered`'s register are
  masked (each address is checked against the disassembly instead);
  `llc` 19's `DW_AT_call_target` is compared as `llc` 22's
  `DW_AT_call_target_clobbered`;
- **what code generation changes in the tree** (WP-094, §6.13;
  `crates/hod_a64/tests/dies_decline.rs` holds each to exactly this
  shape, for clang 19, clang 22 and kiln++):
  - *an inlined call whose only code is a branch hod falls through*
    (§5.7 P1; `fallthrough.cpp`): `llc` keeps a
    `DW_TAG_inlined_subroutine` over one unconditional branch to the next
    instruction, with its abstract instance and the types only that
    instance names; hod has none of them. `llc`'s tree without them is
    hod's, the unit's children compared as a multiset (the abstract
    instance changes the order types are made in). A debugger user sees,
    under `llc`, `break category` resolve to the branch and `step` stop
    once in an inlined `category` frame; under hod `break category`
    finds no code and `step` goes on to the next call. The function had
    no code, so no value is lost;
  - *blocks under an `llvm.is.constant` `llc` folds to false*
    (`LowerConstantIntrinsics`, run even at `-O0`; `isconst.cpp`): hod's
    code generator keeps the never-run code, so hod keeps the block and
    its variables' DIEs; `llc` has neither. hod's tree without that one
    block is `llc`'s. A debugger user sees a breakpoint on a line of the
    dead block never hit under hod (its code never runs); under `llc` the
    line has no code, the breakpoint moves to the next line with code,
    and hits there;
  - *an `sret` result variable* (a named local returned by value through
    `x8`; `sret.cpp`): `llc` writes a location list,
    `[low+4, low+8) DW_OP_breg8; [low+8, high) DW_OP_breg9`, whose only
    gap is the function's first instruction (at the function's line,
    before `prologue_end`); hod writes `DW_OP_fbreg N, DW_OP_deref` (the
    slot holding the result's address) over the whole function, and
    writes no location lists, so its unit has no `DW_AT_loclists_base`.
    The trees are equal without that attribute. A debugger user stopped
    at the function's first instruction (`break *make`, `stepi` into the
    call) sees `print ret` give `<optimized out>` under `llc` and an
    unset value under hod, which reads the slot before the prologue has
    stored the address (the parameters there are unset under both).
    From `prologue_end`, where `break make` and `step` stop, both print
    the same;
  - *a slot of a stack protector's guarded block* (WP-183,
    `spec/codegen.md` §5.6.24 rule 2; class `stack protector`, only in a
    module naming `ssp`, `sspstrong` or `sspreq`): hod reaches an
    escaping local of a protected function through its pointer slot and
    writes `DW_OP_fbreg N, DW_OP_deref`; `llc` moves the object itself
    and writes `DW_OP_fbreg N`. Nothing else in the tree changes. The
    pointer slots are stored by the entry code (and by each coroutine
    stub) before `prologue_end`, so from where `break f` and `step` stop
    both programs print the same: `tests/debug/stack_protector.c`
    (arrays, a struct and scalars sharing a line with them; `info
    locals` / `frame variable` after every line, `print` of an element
    and of the whole object, `finish`) and `tests/debug/coroutine_ssp.cpp`
    (coroutine.cpp's session under `-fstack-protector-strong`) pass under
    gdb and lldb on x86-64 and AArch64, every linker, clang 19 and 22.
    Before the pointer slot is stored (`break *f`) hod's variable is
    unset, as the `sret` case above.
- **value records** (WP-162, §6.23): hod writes a location list for
  every variable of `#dbg_value` records that has a range, where `llc`
  may write a single location expression or `DW_AT_const_value`; and its
  lists have fewer entries (no ranges across blocks, none for register
  values). In sessions the class *value-range* (`xtask/src/debug/permitted.rs`
  `only_value_ranges`) permits a transcript line where hod says
  `<optimized out>`, `<variable not available>` or `<unavailable>` and
  `llc`'s line has a value there (each marker of hod's line matches a
  non-empty run of `llc`'s that is not a marker; the rest equal). Never
  the reverse, and never a different value. A debugger user sees fewer
  variables (`info locals`) under hod outside the proven ranges.

The relocations of `.debug_info`, `.debug_str_offsets` (by the string
each one points at) and `.debug_addr` must name the same targets.

### 6.8 The type graph (implemented, WP-058)

**Entry point.** `UnitPlan::reference_type(from, field, t)`
(`src/types.rs`, crate-internal) gives the DIE of type node `t` and makes
it first if it does not exist yet. `from` and `field` name the referring
node for a `BadReference`. The globals (§6.2) call it, and WP-057 (local
variables, parameters, subprogram types) and WP-059 (template
parameters, static members' definitions, imported entities) call it the
same way. A DIE index it returns is valid at once. A forward reference
is patched by the writer (§6.2). `UnitPlan::of_module(&module, unit)`
is the plan with the module's constant pool (static member constants,
§2).

**Order: `llc`'s `getOrCreateTypeDIE`.** For each type, the scope
(context) comes first: namespaces through their chain, classes as types.
Then, if the type has a DIE, that DIE is used. Otherwise its DIE is
created, appended to the context's children, and *constructed*: its
attributes are fixed and the types it names are reached in `llc`'s
order for its kind. The steps are tasks on one explicit work list
(`Ensure`, `Create`, `Construct { step }`), with no recursion, so depth
costs nothing (`tests/type_graph.rs`, 20,000 nested pointers). A DIE's
slot in the node-to-DIE map is set when it is created, before it is
constructed. So a cycle through a pointer, a reference or a class finds
the DIE and stops: cycles cost one map lookup. A cycle that does not
pass through one of those (a typedef, qualifier, atomic or array
contains itself by value) is `TypeCycle`. Members, inheritance, friends
and the vptr member are not in the map (as in `llc`'s
`constructMemberDIE`): each is written while its class is constructed,
which happens once. After the unit's
lists, the virtual member functions' `containing_type` is added if that
class has a DIE, as `llc` does at the end of the unit. Each node has one
DIE per unit.

**What each kind writes**, in `llc`'s attribute order:
- **base types**: name, encoding (every `DW_ATE`), byte size; `bit_size`
  for a `dataSize` that differs from the size (`_BitInt`, LLVM 22);
  `endianity`; `DW_TAG_unspecified_type` (`decltype(nullptr)`) with its
  name;
- **derived types**:
  - pointer, reference, rvalue reference, const, volatile, restrict and
    atomic: the type (absent for `void`);
  - typedef and template alias: name, decl, and alignment and
    accessibility (a class's member typedef);
  - pointer to member: the type and `containing_type`;
- **classes, structs and unions**: the elements in their order, each by
  its kind: member function declarations, friends, static members (`DW_TAG_variable` with
  `declaration`, `external`, and `const_value` for a constant: `udata` or
  `sdata` by `isUnsignedDIType`, a float as its bits), then members
  (`data_member_location`, or `bit_size` and `data_bit_offset` for a bit
  field), inheritance (an offset, or for a virtual base the `exprloc`
  `llc` writes, and `virtuality`), and the vptr member (`artificial`).
  After them `export_symbols`, `containing_type` (`vtableHolder`),
  `calling_convention` (`pass_by_value` or `pass_by_reference`), name,
  byte size (0 for an empty class, none for a declaration), declaration,
  accessibility, decl and alignment;
- **enumerations**: the base type, `enum_class`, name, byte size, decl;
  each enumerator's `const_value` is `udata` or `sdata` by the base type's
  signedness, 64 bits wide (`hod_ll` refuses a wider one as
  `debug:DIEnumerator.wide`);
- **arrays**: `GNU_vector` and the byte size for vectors, the element
  type, one subrange per dimension against the unit's index type
  `__ARRAY_SIZE_TYPE__` (made once, after the first element type);
  `lower_bound` unless it is the language's default; `count` unless it
  is -1 (an unknown bound); `upper_bound`; a count that is a variable is
  a reference to the variable's DIE (a VLA), or absent if the variable
  has none;
- **subroutine types**: the return type, one `formal_parameter` per
  parameter (`artificial` for an artificial type, `this`), and
  `unspecified_parameters` for a trailing null (varargs, or an
  unprototyped C function); `prototyped` for C languages;
  `calling_convention` unless normal; `reference` and `rvalue_reference`;
- **member function declarations**: linkage name, name, decl, type,
  `virtuality` and `vtable_elem_location` (`DW_OP_constu` index),
  `declaration`, `artificial`, `external`, then `reference`,
  `rvalue_reference` and `noreturn`, then accessibility, `explicit` and
  `deleted`; their parameters (the object pointer is artificial);
- **forward declarations**: name, `declaration`, and the byte size of a
  declared enumeration.

**Function-local types (implemented, WP-057).** A type whose scope is
a subprogram or a lexical block is placed under that scope's DIE. The
function's DIEs are made by `add_function` and the variables' types at
`seal()` (§6.4), so the context lookup finds the DIE and nests the type
there, as `llc` does. A local type reached while no DIE of its scope
exists (a global's type that names it, or a retained type of a function
that was not placed) is `Unsupported("debug:local-type-scope")`.

**Template parameters** are children at `llc`'s position in the
construct steps (WP-059, §6.11).

**Refused** (`Unsupported`, named):
- a pointer with an address space;
- a derived type with a name, size, line or accessibility its kind does
  not have;
- a composite with a runtime language, or of another tag (variant parts,
  Fortran or Objective-C shapes);
- a declared class with exported symbols, a vtable holder, a calling
  convention or an alignment;
- a subrange with a stride, a variable or expression bound, an
  expression count, a variable count together with a bound, or a count
  below -1;
- a vector with other than one subrange;
- a base type encoding beyond one byte;
- a static member constant wider than 64 bits, or one read without the
  module (`UnitPlan::new`);
- a member function definition, thrown types, or a declaration with
  `declaration` among a class's elements, or a combination of flags no
  family has;
- a function-local type (above).

### 6.9 Inlined subroutines, abstract instances and functions' statics (implemented, WP-085)

`src/inline.rs`, with one call each from `func.rs` (`add_function`,
`scope_ranges`, `collect_entities`, the writer's subprogram and local
arms) and `die.rs` (a global scoped in a function). A function that
inlines nothing pays one empty map lookup per entity.

**Scopes: `llc`'s `LexicalScopes`.** A function's scopes are keyed by
`(scope, inlinedAt)`, the scope being the location's non-file scope
(§6.4). The parent of a block is `(its scope, the same inlinedAt)`; the
parent of an inlined subprogram's scope is `(the call's scope, the
call's inlinedAt)`, the call being the `DILocation` `inlinedAt` names.
Every scope reached through an `inlinedAt` also gets an *abstract*
scope, keyed by the scope alone, and the abstract scope is made first
(`getOrCreateLexicalScope` calls `getOrCreateAbstractScope`), so the
abstract scopes are in `llc`'s creation order: a callee inlined into an
inlined caller is made before that caller, unless the caller was made
already. Ranges are §6.4's over these keys: an inlined scope whose code
is interrupted by its caller's has several, and `DW_AT_ranges`.

**Abstract instances** (`DW_AT_inline`, `DW_INL_inlined`): one per
inlined subprogram per unit, written by the first function that inlines
it, as `llc`'s `constructAbstractSubprogramScopeDIE` does at
`endFunction`, before that function's own DIE:
- first every entity of the function whose scope is in an inlined scope
  gets its abstract entity (`ensureAbstractEntityIsCreatedIfScoped`):
  a parameter or variable, a label; its type is made at once;
- then each abstract subprogram in creation order: the subprogram DIE
  (a function's attributes, §6.4, without code: linkage name, name,
  decl, prototyped, the return type, artificial, external, noreturn,
  `inline`), its return type made at once, and its children: arguments
  by number, locals, labels, then the abstract blocks that have
  variables (a block without is flattened into its parent, as in
  §6.4).
A subprogram that already has a DIE (an out-of-line definition added
earlier, or an early DIE, below) keeps it, and gets `DW_AT_inline`.

**Concrete DIEs** name their abstract one by `DW_AT_abstract_origin`,
decided when they are written (`llc` decides at the end of the unit,
`finishSubprogramDefinitions`/`finishEntityDefinitions`):
- a function with an abstract instance: `low_pc`, `high_pc`,
  `frame_base`, `object_pointer`, `abstract_origin`;
- `DW_TAG_inlined_subroutine`: `abstract_origin`, `low_pc`/`high_pc` or
  `ranges`, `call_file` (the file of the call's own scope), `call_line`,
  and `call_column` when it is not 0;
- a parameter or variable in an inlined scope, or of a function with an
  abstract instance: `location` and `abstract_origin` when its abstract
  entity has a DIE; otherwise (a variable the abstract instance's
  function did not have) written in full;
- a label: `abstract_origin`, `low_pc`;
- a block: `low_pc`/`high_pc` or `ranges`, then `abstract_origin`
  (`llc` 21 and later; §6.7 for 19).
Inlined subroutines are always written; inlined blocks under §6.4's
rule.

**Functions' statics** (`llc` 22, `beginModule`). A global scoped in a
subprogram that is a definition of this unit goes under that
subprogram's DIE, made when it is new: if the subprogram has code in
this module, an *early* concrete DIE (attributes empty, filled when the
function is added, so the statics come first among its children); if
not, its abstract instance. `seal()` refuses an early DIE no function
filled.

**Abbreviation families** (appended, §6.1): `SUBPROGRAM_ABSTRACT`,
`SUBPROGRAM_ABSTRACT_MEMBER` (a member function's: `specification`, and
`decl_file`/`decl_line`/linkage name where they differ from the
declaration's),
`SUBPROGRAM_OF_ABSTRACT`, `INLINED_SUBROUTINE`,
`LEXICAL_BLOCK_OF_ABSTRACT`, `LEXICAL_BLOCK_ABSTRACT`,
`FORMAL_PARAMETER_OF_ABSTRACT`, `VARIABLE_OF_ABSTRACT`,
`LABEL_OF_ABSTRACT`. No `Error` variant is added: the refusals are
`Unsupported` with these names:

| name | what |
|---|---|
| `debug:inlined-member` | an abstract instance of a member function whose declaration is WP-059's (not among its class's elements, or an out-of-line definition outside a class: `debug:specification` and kin, renamed) |
| `debug:inlined-discriminator` | a call located in a `DILexicalBlockFile` with a discriminator (`llc` writes `DW_AT_GNU_discriminator`) |
| `debug:inlined-foreign-unit` | code inlined from a subprogram of another unit |
| `debug:retained-nodes` | an inlined subprogram with `retainedNodes` (`llc` makes abstract entities no record names) |
| `debug:inlined-local-type` | a type local to a function with an abstract instance, or a variable a type names (a VLA bound) in one |
| `debug:local-static` | a static scoped in a block, or in a subprogram that is not a definition of this unit, or whose early DIE no function filled by `seal()`, or read without the module (`UnitPlan::new`) |

**`#dbg_value`** was refused (`debug:dbg-value`) until WP-162, which
writes location lists for it (§6.23). `-O0` input from clang 19, clang
22 and kiln++ has none; their `#dbg_declare` expressions are empty or
`DW_OP_deref`, so `debug:fragment` stays refused.

**Tests** (`crates/hod_a64/tests/`, data in `data/dies_inline/`):
- `dies_inline.rs`:
  - `inline_trees_match_llc`: `inline.cpp` (a member function, nesting,
    a block in an inlined function, a function out of line and inlined,
    one inlined twice, a function pointer, a label, a chain three deep)
    and `statics.cpp` (a static of a function only inlined, one with a
    block, statics before and after their function) by clang 19 (`llc`
    19 and 22; `statics.cpp` not on 19, §6.7), clang 22 and kiln++ (`llc`
    22), and `hand.ll` (19 and 22): both objects verify, the trees are
    equal (§6.7). 18 objects, 558 DIEs, 80 inlined subroutines, 43
    abstract instances. `hand.ll`'s shapes are checked one by one: an
    inlined call and its block with ranges, two instances of one
    abstract DIE (the second with column 0 and a variable the first
    lacks), a call inlined into an inlined call whose var-less block's
    label is the inlined subroutine's;
  - `refusals_are_named`: each name above but `debug:inlined-member`
    (which stands for WP-059's declarations), in `hand.ll` with that
    one construct;
  - `corpus_survey` (`HOD_DIES_CORPUS`): per function (each with the
    others' bodies `unreachable`) and per module, by refusal name.
- `dies_inline_debugger.rs` (AArch64): `debug.cpp` linked from hod's
  and `llc`'s objects by GNU ld, lld and mold, LLVM 19 and 22; gdb
  `break` at a line of an inlined function (both instances), `bt` with
  inlined frames, `info args`/`info locals` in them, `up`, `step` into
  and `finish` out of an inlined call; lldb the same with
  `frame variable`, `thread step-in`/`step-out`. 48 sessions (24
  pairs), hod's equal to `llc`'s.
- `dies_inline_throughput.rs`: 20,000 inlined calls, an inlining chain
  20,000 deep (counted at 2,000) and 2,000 functions of ten calls;
  `inlined_die_cost` (ignored) is §7.2's measurement.

### 6.10 Reduced units: `LineTablesOnly` and `DebugDirectivesOnly` (implemented, WP-061)

`crates/hod_dwarf/src/reduced.rs`. What `llc` 19 and 22 write, read from
their output (the two agree):

| `emissionKind` | sections of the unit | DIEs |
|---|---|---|
| `FullDebug` | §1.1 | §6 |
| `LineTablesOnly` (`-gline-tables-only`, `-g1`) | §1.1 | the unit DIE, attribute for attribute as for `FullDebug` (§6.5), and **no child** for a function without inlined code: `DwarfDebug::endFunctionImpl` returns before creating a subprogram when the function has no abstract scopes; the function's code is only in the unit's `low_pc`/`high_pc` or `DW_AT_ranges` |
| `DebugDirectivesOnly` (`-gline-directives-only`) | `.debug_line` and `.debug_line_str` only (no `.debug_info`, `.debug_abbrev`, strings, addresses or ranges) | none |
| `NoDebug` (`-g0` units in a linked module) | none: the unit is skipped, its functions get no rows | none |

The line table of a reduced unit is the one a `FullDebug` unit of the
same functions gets, byte for byte (`hod_a64/tests/reduced.rs` checks it
against hod's `-g` object of the same source). A module mixes kinds
freely: units are written in `!llvm.dbg.cu` order, each as its kind
says; the `DebugDirectivesOnly` and `NoDebug` units are left out of
`write_debug_info`'s inputs (`DebugLines::finish`), so a module whose
units are all `DebugDirectivesOnly` has no `.debug_info` at all.

A `LineTablesOnly` function with code inlined into it (a scope row with
`inlinedAt` that covers a byte) gets, from `llc`, a subprogram with only
`low_pc`, `high_pc` and `name`, an abstract subprogram per inlined
function (`name`, `DW_AT_inline`) and `DW_TAG_inlined_subroutine`
children (`abstract_origin`, `low_pc`/`high_pc`, `call_file`,
`call_line`, `call_column`). hod writes it since WP-094 (§6.13). A `DebugDirectivesOnly` function
with inlined code needs nothing more than its rows and is written.

Refusals (`pub const` in `reduced.rs`, re-exported by the crate):

| name | what |
|---|---|
| `debug:line-tables-inlined` | a `LineTablesOnly` function with inlined code whose subprogram has a declaration, template parameters, retained nodes or a subroutine type with elements (the plain shape is written since WP-094, §6.13) |
| `debug:line-tables-variables` | a `LineTablesOnly` function with variable or label records: no producer writes them, and `llc` 22 crashes on them |
| `debug:reduced-unit-entities` | a `LineTablesOnly` or `DebugDirectivesOnly` unit that lists globals, enumerations, retained types or imported entities: no producer writes them; `llc` 22 crashes on such a `LineTablesOnly` unit and writes a stray `.debug_addr` for such a `DebugDirectivesOnly` one (an empty list, `globals: !{}`, is accepted) |

The hooks: `UnitPlan::build` calls `reduced::check_unit` for a unit that
is not `FullDebug` (`die.rs`), and `UnitPlan::add_function` hands such a
unit's functions to `reduced::add_function` (`func.rs`), which adds no
DIE. `has_dies(kind)` tells the bridge which units go to
`write_debug_info`.

`debugInfoForProfiling: true` makes `llc` write a subprogram for every
function of a `LineTablesOnly` unit; `hod_ir` does not map the flag
(`crates/hod_ir/src/debug/map.rs`), so hod writes none. No producer of
hod's inputs sets it without `-fdebug-info-for-profiling`; open question
in §11.

### 6.11 Globals, templates, imports and specifications (implemented, WP-059)

What is outside function bodies, written as `llc` 19 and 22 write it
(`-O0 -global-isel=0 -fast-isel=0`), its order and attributes taken
from `llc`'s output (`llvm-dwarfdump` trees), not from its source. The
code is in new modules of `hod_dwarf`: `scope.rs` (the names of what is
refused, the plan's state), `global.rs`, `template.rs`, `import.rs` and
`spec.rs`. Each reaches the core through one call at the site that
refused before (`spec/landed/dwarf/WP-059.md` lists them).

**Globals** (`getOrCreateGlobalVariableDIE`). A plain global (a
definition, an empty expression, no declaration, no template
parameters, not thread-local) stays the core's (§6.5). Every other
global outside a function is made here, in `llc`'s order: its context
(namespaces), the variable, then
- a static member's definition (`declaration:`, a `DW_TAG_variable` or
  `DW_TAG_member` with `DIFlagStaticMember` in a class): the class (with
  its members), then the declaration if the class does not list it
  (appended), then the definition's type if it is not the
  declaration's. The definition is written in the unit with
  `DW_AT_specification`, then `type` (when it differs), `alignment`,
  the location or constant, and `linkage_name`;
- any other: `name`, `type`, `external`, decl, `declaration` (a
  declaration), `alignment`, its template parameters as children, the
  location or constant, `linkage_name`.

The location is `DW_OP_addrx` of the global's `.debug_addr` entry (§6.5).
A constant expression (`DW_OP_constu v, DW_OP_stack_value` or
`DW_OP_consts v, DW_OP_stack_value`) is `DW_AT_const_value`, `udata` or
`sdata` as the operator says, with no location. A thread-local
variable on AArch64 has no location: AArch64's `llc` writes none, and
gdb and lldb print it as `llc`'s. Its linkage name is kept. A
thread-local constant is written as a constant. Neither a constant nor
a thread-local variable takes a `.debug_addr` entry, even when the code
generator placed its symbol: the unit counts the entries of the
globals that write a location (`global_takes_address`), then the
template parameters' (below), then the functions'.

**Template parameters** (`addTemplateParams`), each a child DIE:
- `DW_TAG_template_type_parameter`: `type`, `name`, `default_value`;
- `DW_TAG_template_value_parameter`: `type`, `name`, `default_value`,
  then the value:
  - an integer is `DW_AT_const_value`, `udata` or `sdata` by
    `isUnsignedDIType` (an enumeration with a base type takes its base
    type's signedness, as in `llc` 19 and 22);
  - the address of a symbol is `DW_AT_location (DW_OP_addrx N,
    DW_OP_stack_value)`. It takes a `.debug_addr` entry, against the
    place the code generator gave the symbol (`UnitPlan::place_symbols`,
    called by the bridge after `seal`);
  - a float, a null pointer or an address with an offset has no value,
    as in `llc`;
- `DW_TAG_GNU_template_parameter_pack`: `name`; its parameters are
  children;
- `DW_TAG_GNU_template_template_param`: `name`, `default_value`,
  `DW_AT_GNU_template_name`.

hod is DWARF 5 only, so `default_value` is written whenever the node
says so. A parameter is a task of the type graph (§6.8,
`Task::Template`): its DIE, then its type's. Where the parameters go:
- the first children of a class, structure or union and of a member
  function declaration (before the return type and the parameters);
- after the base type of a template alias (its `extraData`);
- the children of a variable;
- the last children of a definition, after its variables, blocks and
  imports. They are made at `seal`, as `finishSubprogramDefinitions`
  does.

**Imported entities** (`DW_TAG_imported_module`, `DW_TAG_imported_declaration`)
are made at `seal`, before the functions' types, as `llc` emits them at
the end of the module:
- first the unit's `imports:`, each in its scope (the unit, a file or
  a namespace);
- then each function's (its `retainedNodes`), as the function's
  children.

For each, the scope comes first, then the entity's DIE if it has none
(a namespace, a type, a function declaration, a global without a
location), then the import: decl, `import`, `name`.

Checked against `llc` for every shape clang 19 and 22 write (WP-111,
`crates/hod_a64/tests/dies_followups.rs`
`imports_of_every_shape_match_llc`, `data/dies_followups/imports.cpp`):
using-directives at unit, namespace, function and nested block scope,
using-declarations of a function, a variable and a type at each of
those scopes, namespace aliases at unit and block scope, and the C
library's imports into `std` from `<cstdlib>`, `<cmath>`, `<cstring>`
and `<vector>`: 786 imports in 3 trees (clang 19 by `llc` 19 and 22,
clang 22 by `llc` 22), all equal, without a code change. kiln++ writes
no `DIImportedEntity`.

**Specifications** (`getOrCreateSubprogramDIE`,
`applySubprogramDefinitionAttributes`):
- a member function's declaration that its class does not list (a
  template's instance, an implicit member) is made when its definition
  is, and appended to the class;
- a definition whose declaration is in a namespace or the unit (an
  out-of-line definition such as `void ns::f() {}`) has its declaration
  made there, with the member declaration's attributes (§6.8);
- the definition goes to the unit with `DW_AT_specification`;
- a definition whose return type is not its declaration's (`auto`
  deduced) writes `DW_AT_type` after the frame base and before the decl
  (families 35 and 86). That type is made at `seal`.

**Families** (§6.1): WP-059 uses its reserved families
- 14 to 18;
- 35, refined to `object_pointer` (0), `type`, `decl_line` (1),
  `linkage_name` (2), `specification`;
- 41 and 42 (42's `const_value` is `udata`), 43 and 44.

It appends 81 to 86 for the other shapes.

**Refused** (`Unsupported`, named in `scope.rs`):

| name | what |
|---|---|
| `debug:global-expression` | a global's expression that is neither empty nor a constant |
| `debug:global-shape` | a static member's definition that is not a definition; a global without a name or type that is not a static member's definition; a line without a file |
| `debug:global-scope` | a global scoped outside the unit, a file or a namespace (a module) |
| `debug:static-member-declaration` | a `declaration:` that is not a static member of a class |
| `debug:tls-location` | a thread-local variable on x86-64 in a split unit, whose `llc` writes `DW_OP_GNU_const_index` of a `.debug_addr` entry (since WP-136; a full unit's is written, §6.17) |
| `debug:template-parameter` | a parameter of another tag; a pack with a type or a default; a template template parameter whose value is not a string; a value of the wrong kind |
| `debug:template-value` | an integer value wider than 64 bits |
| `debug:template-address` | the address of a symbol this object does not place (an undefined function or variable) |
| `debug:import` | an import of another tag, with `elements:`, an `imported_module` with a name, a line without a file |
| `debug:import-scope` | a unit's import scoped outside the unit, a file or a namespace; a function's import scoped elsewhere than its function |
| `debug:import-entity` | an import of a module, of another import, of a function's definition or of a function-local variable |
| `debug:declaration-scope` | a function declaration scoped outside a class, the unit, a file or a namespace, or virtual outside a class |

A function's `static` (`debug:local-static`), inlined subroutines,
`#dbg_value`, fragments and other location expressions are WP-085's. A
unit's `LineTablesOnly` and compressed sections are WP-061's.

### 6.12 Debug modules end to end (implemented, WP-087)

**`classify`** no longer refuses debug modules on AArch64: of its four
`debug:*` rows, `debug:module-flags`, `debug:subprogram`,
`debug:slot-variables` and `debug:locations` are supported there (x86-64
keeps them, its code generator being absent). One row is new and refused
on both: `debug:dwarf-version`, a compile unit that is not `NoDebug` in a
module whose `Dwarf Version` is not 5. No flag is LLVM's DWARF 4 (`llc`
22 writes version 4 then). Since WP-114 the flag 4 is written too
(§6.20); 2, 3 and no flag stay refused.

**Declines, not errors** (`crates/hod/src/back.rs`, the bridge): the
module's `ModuleEmitter` is made before any function is compiled, so
what the unit plans refuse (§6.4 to §6.11 names) is declined before code
generation; what the function DIEs refuse is found as functions are
added, except what depends only on metadata, which is checked before
code generation too since WP-094 (§6.13). What the DWARF writer finds wrong with the input is an
ordinary decline by name, never an internal error and never an object
without its DIEs:

| name | what |
|---|---|
| `debug:malformed-metadata` | a field naming a node of a kind it does not allow, a type cycle, a member outside a type, a sized type without a size, a NUL in a file name, a unit that is not a `DICompileUnit` |
| `debug:section-too-large` | a `.debug_*` section of 4 GiB or more |

What remains an internal error is the bridge's own bug (a line row, a
code span, a symbol or a global's address it made wrong).

**Shapes whole units need** (each compared with `llc` 19 and 22):
- a function-local type the unit retains (clang lists the typedef of an
  explicit cast in `retainedTypes`) reached before its function: the
  function's concrete DIE is made early, as for its statics (§6.9),
  when the module has its code. A type local to a block, or to a
  function without code here, stays `debug:local-type-scope` (`llc` 22
  puts the latter under the abstract instance, `llc` 19 under an empty
  subprogram);
- an unnamed local (a catch parameter without a name): a `variable`
  without `DW_AT_name` (family 87);
- an `alignas` local: `DW_AT_alignment` in bytes after `name` (family
  88);
- a `DIFlagThunk` definition (kiln++'s virtual thunks): `llc` writes
  nothing for the flag;
- an abstract instance of a function template: its template parameters
  are its first children, made before its return type
  (`applySubprogramDefinitionAttributes` at
  `constructAbstractSubprogramScopeDIE`); one whose return type differs
  from its declaration's is `debug:specification-type`;
- a static member of an enumeration type: `isUnsignedDIType` follows the
  enumeration's base type (signed without one).

**The census** (`crates/hod_a64/tests/dies_e2e.rs`, `debug_census`): 130
`-g -O0` modules (44 libc++ tests, a standard-library unit and four bench
sources, each by clang 19, clang 22 and kiln++, with libstdc++ or libc++).
hod's code generator builds 116; every built module is
`--verify` clean, 111 link and run with the same exit status and output
as `llc`'s, and 99 have `llc`'s DIE tree (tags, names, order). The
other 17 are known differences, none of them a wrong value:
- `llc` keeps the inlined subroutine of a call whose only code is an
  unconditional branch hod falls through (§5.7 P1), with its abstract
  instance; hod has neither (6 modules);
- `llc` deletes the blocks under a `llvm.is.constant` it folds to false
  (`LowerConstantIntrinsics`, even at `-O0`); hod keeps them, and their
  block and variable DIEs (8 modules);
- the order of two unit-level pointer and reference types around an
  on-demand member template declaration (5 modules).

On those modules 92% of the 239 416 functions' line tables are `llc`'s
under §5.7; the rest differ in rows `llc` gives instructions hod does
not emit (an empty `asm`, a branch), which the debuggers do not see
when they stop at lines.

### 6.13 The declines left after WP-087 (implemented, WP-094)

**Imports in lexical blocks.** An imported entity a function retains
whose `scope:` is one of that function's lexical blocks goes under the
block's DIE, as `llc` writes a scope's deferred local declarations
(`DeferredLocalDecls`, `createAndAddScopeChildren`). A block with an
import is written even if it has no variable (`skipLexicalScope`); an
import whose block has no code is not written (`llc` never visits the
scope). The imports are made at the end of the module (§6.11): each
function's own first, in `retainedNodes` order, then those of its blocks
in the order of the blocks' DIEs (depth first), each block's in
`retainedNodes` order. Still `debug:import-scope`: an import scoped in a
block file or in another function's block, and an import in a block of
a function that has an abstract instance (where `llc` puts it then
depends on what is inlined, §6.9). kiln++ writes no imported entities.

**The order of the unit's types.** Two rules of `llc`'s, seen in five
census modules:
- `finishEntityDefinitions` makes the types of the functions' variables
  (those of a function without an abstract instance) after the whole
  module, function by function, and within a function in the order of
  its variables' records (the frame table of `dbg_declare`s, then the
  `dbg_value`s: `ConcreteEntities`), not in the order of their DIEs. A
  catch parameter whose record comes before a block's variable gets its
  type first. hod records each function's variable DIEs in the order of
  `FunctionInput::vars` (`Inline::record_entities`) and makes their types
  in that order (`finish_entities`).
- `applySubprogramAttributes`: a definition's template parameters (and
  their types) are made before its return type, so `seal` adds them
  first.

**A function's own early DIE.** §6.12 makes a function's concrete DIE
early for a function-local type reached before its function. Such a type
may be reached by the function's *own* inlined code (a typedef local to
the function kept as a template argument of an inlined callee, as
libstdc++'s `_DistanceType` under kiln++): the early DIE is then the
function's DIE when its body is added. It was left over and refused as
`debug:local-static` (kiln's `regex-match-libstdcxx.cpp` at `-g`).

**A function's thread-local `static`** (a function's, a block's, a member
function's, an inlined function's; with or without a dynamic
initializer) goes where §6.9 puts a function's statics, with no
`DW_AT_location` on AArch64, as `llc` writes every thread-local variable
there (§6.11). hod wrote `DW_OP_addrx` of the variable's symbol, an
address that is not the thread's variable. gdb prints `<optimized out>`,
lldb `<no location, value may have been optimized out>`, for hod's
program as for `llc`'s. x86-64 stays `debug:tls-location` (written since
WP-136, §6.17).

**Line-tables-only units with inlined code** (`debug:line-tables-inlined`
narrowed). In a `LineTablesOnly` unit (`-gline-tables-only`, `-gmlt`)
`llc`'s `includeMinimalInlineScopes` writes a function only if it has
inlined code: a `DW_TAG_subprogram` under the unit whatever its scope,
with `low_pc`, `high_pc` and `name` (family 89), and its inlined
subroutines (`abstract_origin`, code, call file, line, column) as
children, nested as they are inlined. Each inlined function has an
abstract instance of only `name` and `DW_AT_inline`; a function that is
also out of line has `DW_AT_abstract_origin` in place of the name. No
types, parameters, variables or blocks. A function with
`DIFlagAllCallsDescribed` (`-O1 -gmlt`) was refused (as
`debug:call-sites` from WP-098, §6.14); since WP-113 a unit without
inlined code is written (`llc` writes no function DIE there either), and
one with inlined code is refused as `debug:call-sites-line-tables`
(§6.19). What no producer writes in such a unit stays
`debug:line-tables-inlined`: a function with inlined code whose
subprogram has a declaration, template parameters, retained nodes or a
subroutine type with elements.

**`debug:inlined-member` stays** (§6.9: an abstract instance of a member
function whose declaration is not among its class's elements, or an
out-of-line definition outside a class): no source reaches it through
clang 19, clang 22 or kiln++, which list every member function's
declaration among its class's elements and scope its definitions in
the class.

**Refusals before code generation.** `hod_dwarf::check_functions`, called
by the bridge after the `ModuleEmitter` is made (`crates/hod/src/back.rs`),
declines a module for each function-DIE refusal that depends only on
metadata, before any function is compiled, with the same names as the
writer's (`tests/early.rs`):

| name | what |
|---|---|
| `debug:subprogram-not-definition` | a function whose subprogram is not a definition |
| `debug:subprogram-attribute` | thrown types, `spFlags` or (without a declaration) flags hod does not write |
| `debug:call-sites-line-tables` | `DIFlagAllCallsDescribed` with inlined code (WP-113, §6.19) |
| `debug:variable-without-type`, `debug:variable-alignment` | a variable record's variable (§6.4, §6.12) |
| `debug:fragment`, `debug:expression` | a variable record's expression |
| `debug:dbg-value-fragment`, `debug:dbg-value-expr` | a value record's expression (WP-162, §6.23) |
| `debug:label-attribute` | no longer refused (WP-098, §6.14) |
| `debug:import-scope`, `debug:import` | a function's import with no scope, or scoped in a block file or another function; one in its own scope of a shape hod does not write |
| `debug:subprogram-twice` | two bodies with one subprogram |

What still needs the generated code or the plan's state, and is found as
the function's DIEs are written: a variable whose location the code
generator refused (its names, as `dbg-value-arglist`); the inlined-code refusals
that depend on which locations survive (`debug:retained-nodes`,
`debug:inlined-foreign-unit`, `debug:inlined-discriminator`,
`debug:inlined-local-type`, `debug:inlined-member`);
`debug:variable-twice` and `debug:object-pointer` (by the DIEs made);
the type graph's refusals (made at seal); `debug:local-type-scope`; an
import in a function with an abstract instance, `debug:import-entity`,
a unit's import scope; `debug:subprogram-scope` and
`debug:declaration-scope` (by the plan's DIEs); and a function whose
early DIE got no code (`debug:local-static`).

**The census** (§6.12's, regenerated, with four of kiln's libstdc++
units: 148 modules): 147 built (140 before), all `--verify` clean; 83
trees equal to `llc`'s (75 before). Of the other 64, 26 show only the
permitted differences above, 35 only a scope that is one address range
in hod's code and several in `llc`'s, and 3 only the bare offset of one
nameless reference. The one decline is `debug:label-attribute`
(coroutine suspend labels). `spec/landed/dwarf/WP-094.md` has the
numbers.

### 6.14 Labels of every shape; the census's own artefacts (implemented, WP-098)

**Labels.** Every `DILabel` is written; `debug:label-attribute` is no
longer refused anywhere. A label that is named, not artificial and not a
suspend point keeps WP-057's families (39, 71) and §6.9's abstract ones;
any other takes family 90 (`label (any)`), `llc`'s
`applyLabelAttributes` order:

| slot | attribute | written when |
|---|---|---|
| 0 | `DW_AT_name` (`strx`) | the name is not empty |
| 1 | `DW_AT_decl_file`, `DW_AT_decl_line` | the line is not 0 |
| 2 | `DW_AT_decl_column` | there is a decl and the column is not 0 |
| 3 | `DW_AT_artificial` | `isArtificial` |
| 4 | `DW_AT_LLVM_coro_suspend_idx` (0x3e0d, `udata`; `llc` writes `data1`, §6.7) | `coroSuspendIdx` |
| 5 | `DW_AT_low_pc` (`addrx`) | a concrete label (an abstract one, §6.9, has none) |

A label's concrete copy in inlined code stays family 80 (`abstract_origin`,
`low_pc`) whatever the label. The labels with a suspend index are the
artificial `__coro_resume_N` labels `CoroSplit` (LLVM 21 and later) puts
in a coroutine's `.resume`, `.destroy` and `.cleanup` clones; clang 19
writes none. kiln++ writes no `DILabel` for a `goto` target; it writes the
coroutine labels (its coroutines are split, as clang's at `-O0`).

**A variable's or label's scope.** A record's DIE goes under the
variable's or label's own `scope:` (inlined where the record's location
says), not under the scope of the record's location, as `llc`'s
`findLexicalScope` / `findInlinedScope` of `getScope()`. Both are the
same for every front end's own records; they differ after `CoroSplit`,
which moves the frame's `dbg_declare`s to a clone's entry (located in the
subprogram, the variable scoped in a block) and gives the suspend labels
the subprogram as scope with records located in a block. Only a record
without a scope falls back to its location's.

**Alignment under a byte.** `CoroSplit`'s frame types (`*.coro_frame_ty`)
give members `align: 4` (bits). `llc` writes `DW_AT_alignment` only when
`align / 8` is not 0; every writer of the attribute in hod (members,
composites, typedefs, globals, locals) tests `align >= 8` for the
abbreviation and the bytes alike. Before WP-098 the abbreviation had the
bit and the value was not written: the unit was unreadable.

**The tree comparison** (`crates/hod_a64/tests/dies_support/mod.rs`) adds
two normalizations, each a case §6.7 already permits: a reference the
dump prints as a bare offset is compared by the tag of the DIE at that
offset ("the offset of a reference": `name_bare_reference`); and a DIE
other than the unit's with `low_pc`/`high_pc` equals one with
`DW_AT_ranges` ("`low_pc`, `high_pc`, `ranges` are masked": `same_attrs`).
`permitted` classifies a difference by §6.7's code-generation classes
(the `sret` attribute, (a) a fall-through inlined call, (b) an
`is.constant` block, `llc` 19's blocks without `abstract_origin`) and
names the class; the census (`scripts/debug-census.sh`) counts trees
equal, permitted by class, and differing.

**Not written by `hod_cg` at WP-098, written since WP-111**
(`crates/hod_a64/tests/dies_label.rs`, no longer ignored): a unit with
imports but no code or globals (§1.2); a `dbg_declare` of the frame
pointer's `phi` in a coroutine's ramp (`spec/codegen.md` §9.5, §6.4
"Retained variables"). With it, clang 22's ramp no longer declares its
parameter twice: the second declaration was the `phi`'s.

**WP-099's findings** (`docs/status/debug.md`; each compared with `llc`
19 and 22 for clang 19, clang 22 and kiln++ in `dies_label.rs`, and a
`tests/debug/` test):
- *An `alignas` local of a function that is only inlined*
  (`align_inlined.cpp`): the abstract instance's variable is family 88
  without `location`, so `name`, `alignment`, decl, `type`, as `llc`
  writes it. An unnamed aligned variable stays `debug:variable-alignment`,
  as a concrete one does (§6.4).
- *A type local to a function that is only inlined* (the lambda of an
  `always_inline` member, `local_type_inlined.cpp`): reached once the
  function's abstract instance exists, the type goes under that instance
  (`getOrCreateContextDIE` finds `getAbstractScopeDIEs`; `llc` 19 and 22
  agree). In an abstract instance a variable's type is made before the
  variable's DIE (`constructVariableDIE` adds the variable to its scope
  after its attributes), so a local type comes before the variable that
  names it. A type reached before the instance exists stays
  `debug:local-type-scope`.
- *An optimized unit* (`optimized_unit.c`): `DISPFlagOptimized` writes
  nothing more off Apple targets (it was already accepted). clang marks
  every definition of such a unit `DIFlagAllCallsDescribed`, and for
  that flag `llc` writes `DW_AT_call_all_calls` and one `DW_TAG_call_site`
  per call (`call_origin`, `call_return_pc`), which needs the code's
  call return addresses. hod refused it as **`debug:call-sites`**
  (`hod_dwarf::DEBUG_CALL_SITES`, before code generation as well),
  narrowed from `debug:subprogram-attribute`, whether the definition has
  a declaration or not; WP-113 writes the entries (§6.19), and the name
  is left for a unit that is not DWARF 5. Before WP-098 a member definition (one with a
  declaration) with the flag was written without its call sites. The
  corpus's optimized modules also have `#dbg_value`
  (`debug:dbg-value` then; §6.23 since WP-162).

**DWARF 4** stayed `debug:dwarf-version` (§6.12) until WP-114 wrote it
(§6.20); the measured plan was `spec/landed/dwarf/WP-098.md`.

**The census** (`scripts/debug-census.sh`, regenerated: 123 modules, 44
libc++ tests, `stl.cpp` and four bench sources by clang 19, clang 22 and
kiln++, and five of kiln's libstdc++ units, two of them coroutines). All
123 build, where 121 did before WP-098, and both writers' objects are
all `--verify` clean. 105 trees equal `llc`'s; 14 show only permitted
differences (§6.7: (a) 6, `sret` and (a) 7, `llc` 19's blocks 1). The 4
left are not the DIE writer's: the two kiln++ generators' ramps keep a
frame variable `llc -O0` drops, and `range.range` has a unit with only
imports, which hod does not write (by clang 19 and by clang 22). The
measurement of `docs/status/debug.md` went from 12 declines among 212
modules to 4: `debug:call-sites` 2 and `debug:dwarf-version` 2
(`spec/landed/dwarf/WP-098.md`).

### 6.15 Split DWARF (implemented, WP-062)

`crates/hod_dwarf/src/split.rs`. `-gsplit-dwarf` reaches the backend as
`llc -split-dwarf-file=<name> [-split-dwarf-output=<file>]` and, in the
IR, as the unit's `splitDebugFilename` (clang and kiln write the name
they pass, `splitDebugInlining: false`). What `llc` 19 and 22 write for a
DWARF 5 unit, read from their output, and therefore hod:

| options | `llc` | hod (`hod_cg::Options::split_dwarf`) |
|---|---|---|
| neither | §6 | `SplitDwarf::Off`, §6 |
| `-split-dwarf-file` (`-gsplit-dwarf=single`) | skeleton in the object, the `.dwo` sections in the object too, `SHF_EXCLUDE` | `SplitDwarf::Single` |
| both (`-gsplit-dwarf`) | skeleton in the object, the `.dwo` sections in `<file>`; the file is written even when it is empty | `SplitDwarf::Separate`: the sections in a companion `Object` (`Object::set_dwo`, `spec/object.md`), which the facade writes to `<file>` (an empty object when there is no split unit) |
| `-split-dwarf-output` alone | an ordinary unit and an empty `.dwo` file | `option:split-dwarf-output` |

**The skeleton unit** (`.debug_info`, `DW_UT_skeleton`, the header's
DWO id at offset 12; `DW_TAG_skeleton_unit`, families 91 and 92):
`DW_AT_stmt_list`, `DW_AT_str_offsets_base`, `DW_AT_comp_dir`,
`DW_AT_dwo_name`, the unit's code (`DW_AT_low_pc` `addrx` and
`DW_AT_high_pc`, or `DW_AT_low_pc` 0 and a `DW_FORM_rnglistx` list),
`DW_AT_addr_base` and, with a list, `DW_AT_rnglists_base`. The object
keeps the line table, the unit's `.debug_addr` (every address of the
split unit is an index of it) and its `.debug_rnglists` (only the unit's
own list), and gets its own `.debug_abbrev`, `.debug_str` and
`.debug_str_offsets`, all with the relocations of §1.1.

**The split unit** (`.debug_info.dwo`, `DW_UT_split_compile`; family
95: `DW_AT_producer`, `DW_AT_language`, `DW_AT_name`, `DW_AT_dwo_name`)
holds every other DIE, written by the full writer's code
(`Writer::die`), so §6.4 to §6.13 hold unchanged. Its sections are
`.debug_str_offsets.dwo`, `.debug_str.dwo` (`SHF_MERGE|SHF_STRINGS`),
`.debug_info.dwo`, `.debug_abbrev.dwo` and, when a block has a range
list, `.debug_rnglists.dwo`, in that order, all `SHF_EXCLUDE`, with **no
relocation**: string offsets are resolved in place, `DW_AT_decl_file`
numbers are the skeleton's line table's, addresses are `.debug_addr`
indices. A block's list is written as in a full unit, a
`DW_FORM_rnglistx` index (§6.4), into the table of
`.debug_rnglists.dwo`, which holds the blocks' lists only (the unit's
own is the skeleton's). Before WP-163 the full unit wrote a four-byte
`DW_FORM_sec_offset` that the split writer turned into a padded
ULEB128 index; now both write the one-byte index `llc` writes. Any other attribute that would need a relocation is
refused (`Unsupported`, no `debug:` name: an internal error).

**A unit without a split unit**: a unit whose only DIE is its own (a
`-gline-tables-only` unit, §6.10) gets, as from `llc`, a skeleton with
`DW_AT_producer`, `DW_AT_language` and `DW_AT_name` in place of
`DW_AT_dwo_name` (families 93 and 94), DWO id 0, and no `.dwo` section.
clang writes no `splitDebugFilename` for it, which is then not needed.

**The DWO id.** `llc`'s is `DIEHash::computeCUSignature`, an MD5 over the
unit's DIEs under LLVM's type-signature rules; it includes the frame
offsets of locals, which hod lays out differently (§6.7), so hod cannot
reproduce it and does not try. hod's is `hod_dwarf::dwo_id` of the `.dwo`
name and the split unit's five sections (the info with a zero id):
each part hashed as its length and then 8-byte words through
`h = rotl((h ^ w) * K, 31)`, finished by MurmurHash3's `fmix64`, 0
replaced by 1. It is computed once and written into both headers, so
the two always agree; it depends only on the bytes, so it is
deterministic and the same for every thread count. Consumers only
compare the two (gdb, lldb, `llvm-dwp`).

**Permitted differences** (besides §6.7's):

- `llc` names the code of a function or a block that does not start its
  section by a range list in a split unit (`useRangesForSplitDwarf`,
  "minimize addresses"); hod keeps `low_pc`/`high_pc`, as in a full
  unit. Both are valid; the debuggers read both;
- `DW_AT_GNU_pubnames` (`nameTableKind: GNU`, clang's default with
  `-gsplit-dwarf`): hod writes no name index (§1.1);
- the DWO id's value (above);
- the empty `.dwo` file of a unit without split unit: `llc`'s has only
  `.strtab`, hod's is `hod_obj`'s empty object.

**Refusals** (`pub const` in `split.rs`, re-exported by the crate; the
two module-level ones are checked by the facade before code generation,
`crates/hod/src/back.rs`):

| name | what |
|---|---|
| `debug:DICompileUnit.dwoId` | a unit with a DWO id, split or not: a clang module's skeleton (`-gmodules`), for which `llc` writes `DW_AT_GNU_dwo_id`/`DW_AT_GNU_dwo_name` on an ordinary unit |
| `debug:split-several-units` | split DWARF for a module with several units with DIEs (linked IR): one `.dwo` would hold several split units |
| `debug:split-dwarf-inlining` | `splitDebugInlining: true` (LLVM's default when the field is omitted) and inlined code: `llc` then repeats the inlined subroutines in the skeleton |
| `debug:split-reduced-unit` | a `LineTablesOnly` unit with function DIEs in split mode (today refused earlier as `debug:line-tables-inlined`; kept for when that is lifted) |
| `debug:split-dwarf-name` | a unit with a split unit and no `splitDebugFilename`, or (facade) a name that is not `-split-dwarf-file`'s: hod writes the unit's name, `llc` the option's |
| `debug:split-dwarf-4` | (facade) split DWARF with `Dwarf Version` below 5 (GNU fission: `DW_AT_GNU_dwo_name`, `DW_FORM_GNU_addr_index`); since WP-114 version 4 is written (§6.20) and this name refuses it with split DWARF |

**Hooks** (one call each): `UnitPlan::build` calls `split::check_unit`
(`die.rs`); `DebugLines::finish` calls `write_unit_dies` with the
emitter's `split_dwarf` in place of `write_debug_info`
(`crates/hod_cg/src/emit/lines.rs`), set in `debug_tables`
(`emit/mod.rs`); the facade maps the options (`cg_options`), checks the
module (`check_split_dwarf`) and writes the companion (`write_dwo`,
compressed as the object's debug sections are).

Evidence: `crates/hod_a64/tests/dies_split.rs` (§8.8),
`tests/debug/split.cpp` (L7, §8.4), `spec/landed/dwarf/WP-062.md`.

### 6.16 Coroutine bodies (implemented, WP-108)

`crates/hod_dwarf/src/coro.rs` (the locations), `crates/hod_cg/src/coro_debug.rs`
(the rows). hod compiles a coroutine as one body and two stubs
(`spec/codegen.md` §7.6), where `llc` sees `CoroSplit`'s clones. The
debug information follows hod's shape, not the clones', by these rules.

**R1. One subprogram, the body only.** The body's `DISubprogram` is
written once, as for any function, and its `DW_AT_high_pc` ends where the
stubs `F.resume` and `F.destroy` start (as the ELF symbol `F` does,
§7.6): no DIE describes the stubs. A stub builds the machine frame
before it writes the frame word, so before its ready point no location
of R3 holds there, and a range that claimed them would show another
frame's bytes (gdb's `break F` resolves the stub's symbol and stops at
its entry). Described by no DIE, the stubs are what the `.await_suspend`
wrappers are: code with line rows and CFI, named by their local
`STT_FUNC` symbols (`count(int) [clone .resume]` in gdb), which is what
`break F.resume` (lldb `-n _Z1Fi.resume`) resolves. There is no
`F.cleanup`: hod never elides the frame's allocation. The unit's range
covers the stubs.

**R2. The stubs' rows.** Each stub gets two `is_stmt` rows at the
subprogram's `scopeLine`: at its entry, and with `prologue_end` at the
**ready point** (`FnCx::coro_stub_ready`, after the stub has set the mode
and frame words), so a breakpoint on the stub stops past its prologue
and a `step` into `handle.resume()` goes on into the body, where every
variable reads correctly. The body's own rows after the first stub are
dropped.

**R3. Locations on the frame word.** A variable's location is one
expression valid at every pc of the body, in the ramp and in a resumed
activation, built on the **frame word** (the machine-stack word holding
`coro.begin`'s result; the prologue stores 0 to it, `coro.begin` and the
stubs the frame's address):

| the variable's address | location |
|---|---|
| a heap slot or a value homed in the heap (`FrameRef::Coro`) | `DW_OP_fbreg word, DW_OP_deref, DW_OP_plus_uconst o` (LLVM's clone expression) |
| copied at `coro.begin` (a shadowed slot or value, the promise) | `fbreg word, deref, dup, bra H, drop, fbreg s, skip E, H: plus_uconst o, E:`: the shadow while the word is 0, the heap after |
| declared twice, on a machine-stack slot and on a heap slot (clang's parameter and its frame copy) | one DIE, the copied form above; the second declaration is dropped |
| a parameter on a machine-stack slot whose frame copy is declared as an artificial variable of the same name and scope on a heap slot (kiln) | both DIEs the copied form, with the parameter's slot and the copy's heap offset: the body's `n` is the copy ([dcl.fct.def.coroutine]), and gdb reads a parameter through the variable of its name |
| a parameter on a machine-stack slot only (`!coro.outside.frame`) | `fbreg mode, deref, bra Z, fbreg s, skip 1, Z: lit0`: the slot in the ramp, address 0 in a resumed activation (the debugger says it cannot read it, rather than showing bytes the ramp never wrote there) |
| any other machine-stack slot or home | §6.4 (`fbreg`), valid in the activation that wrote it, as any `-O0` local |

The word, not `x19`, is the base: it is 0 before `coro.begin`, so the
expression holds in the prologue and before the frame exists, needs no
location list, and does not depend on the register the code addresses
the heap through. A value homed in the heap (`indirect`) adds the
trailing `DW_OP_deref` of §6.4. The variable's `DIExpression` follows,
as in §6.4.

**Left out, with the reason** (no installed tool reads them; L7 shows
the sessions without them):

- `__coro_frame` and the `F.coro_frame_ty` structure type that
  `CoroSplit` synthesizes: libstdc++ installs no coroutine pretty
  printers; lldb 19's `std::coroutine_handle` formatter reads the
  handle's resume and destroy pointers and the promise through the
  handle's template argument, and both work on hod's programs;
- the `__coro_resume_N` labels with `DW_AT_LLVM_coro_suspend_idx` that
  `CoroSplit` adds to the clones (§6.14): pre-split IR has none, and hod
  does not synthesize them;
- one subprogram per clone (`linkageName` `F.resume`), R1; describing
  the code of a stub after its ready point would take `DW_AT_ranges` on
  the subprogram families (a follow-up), and gains only the variables at
  that one stop.

**Permitted differences** (L7 `pre-opt: checks`, `spec/harness.md`):
the frames' names (`count(int) [clone .resume]` against `count`), the
breakpoints' location counts, a stop in a stub showing no variables
(R1) where `llc`'s stops in its clone at the re-entry line, the order of `info locals`, a variable
`llc`'s clone has no DIE for (a ramp-only local is "optimized out" or
absent there, unreadable or readable in hod's), and one more stop of
`next`/`step` at the clone's return line when a suspend returns to the
resumer (hod's suspend path is in line-0 rows and the debuggers step
through it to the caller). The line comparison is not run on these
tests: the clones' functions are not hod's.

**Refusals.** `coro:debug-info` (WP-106) is lifted. The generic
refusals of §6.4 still apply (a variable whose frame place the frame
base does not reach is `debug:frame-place`).

**Hooks** (one call each): `FnCx::coro_vars` in `finish_vars`
(`crates/hod_cg/src/driver.rs`); `coro_ready` into the function's record
(`driver.rs`, `record.rs`, `emit/mod.rs`); `stub_rows` after
`function_rows` (`emit/lines.rs`); the target stores 0 to the frame word
in the prologue and calls `coro_stub_ready` in each stub
(`crates/hod_a64/src/coro_body.rs`, `spec/codegen.md` §7.6).

Evidence: `crates/hod_a64/tests/coro_debug.rs`, `tests/debug/coroutine.cpp`
(L7), `spec/landed/dwarf/WP-108.md`.

### 6.17 x86-64 (implemented, WP-136)

The same writer and the same bridge for x86-64; what the target changes
is in `crates/hod_dwarf/src/x86_64.rs`, and `classify` lets `-g` modules
through for x86-64 (the rows `debug:module-flags`, `debug:subprogram`,
`debug:slot-variables`, `debug:locations` are `ON` for both targets;
`debug:dwarf-version` stays AArch64's: versions other than 4 and 5 are
refused by name on x86-64 too; version 4 is written on both, §6.20).

**Register numbers** (System V psABI, "DWARF Register Number Mapping"):
`rax` 0, `rdx` 1, `rcx` 2, `rbx` 3, `rsi` 4, `rdi` 5, `rbp` 6, `rsp` 7,
`r8`..`r15` 8..15, the return address 16, `xmm0`..`xmm15` 17..32,
`st0`..`st7` 33..40, `mm0`..`mm7` 41..48 (`x86_64::{gpr, xmm, st}`: the
psABI's order of the first eight is not the encoding's). hod_x64 reports
`rbp` as every function's frame base (`FrameDesc::frame_base`,
`crates/hod_x64/src/frame.rs`), so the subprogram's `DW_AT_frame_base`
is `DW_OP_reg6`, as `llc`'s.

**Places**: a variable in a slot is `DW_OP_fbreg o`
(`frame::base_offset`: a local at `slot - 8n - FRAME`, below the `n`
saved registers and the frame, `codegen.md` §5.6.15; an incoming memory argument at `16 + n`);
one whose address is in a slot is `DW_OP_fbreg o, DW_OP_deref` (the sret
pointer, a dynamic alloca's address, a byval argument's, an over-aligned
slot's, which hod_x64 aligns dynamically instead of realigning `rsp`).
Memory arguments (the seventh integer and ninth floating argument on)
are copied into slots by hod and described there; `llc` describes them
in place (`DW_OP_fbreg +16`): the same operation, another offset.

**Relocations**: an offset into another debug section is
`R_X86_64_32`, an address `R_X86_64_64`, a thread-local's offset
`R_X86_64_DTPOFF64`. `R_X86_64_PC32` is `.eh_frame`'s (`spec/eh.md`);
no debug section of hod's or `llc`'s has one.

**Thread-local variables**: `llc` for x86-64 writes `DW_AT_location`
`DW_OP_const8u <x@DTPOFF>, DW_OP_GNU_push_tls_address` (gdb tuning, the
default of every ELF target; `DW_OP_form_tls_address` under lldb tuning,
which hod does not follow, §1.1), the offset relocated by
`R_X86_64_DTPOFF64` against the variable's own symbol (not a section
symbol). hod writes exactly that (`Writer::tls_location`, the
relocation `Target::Tls`), for a global and for a function's
thread-local static. In a split unit `llc` puts the offset in
`.debug_addr` (`DW_OP_GNU_const_index`), which hod does not write:
refused as `debug:tls-location`. AArch64 is unchanged (no location).

**The unit's code** (both targets): `llc` gives a unit `DW_AT_low_pc` /
`DW_AT_high_pc` when its code is one range, and extends a range over the
padding between consecutive functions of the same unit in the same
section (`DwarfCompileUnit::addRange`: a function that starts where the
previous one's section symbol is the same extends the last range to its
end). The bridge does the same (`emit/lines.rs`, `unit_span`): x86-64
pads functions to 16 bytes, so without it every unit had `DW_AT_ranges`
where `llc`'s has one range. AArch64 functions have no padding between
them; its objects are byte-identical before and after.

**Line table**: unchanged (§4, §5); `prologue_end` at the first row
after the prologue, as `llc`'s. The `(function, line)` comparison of
every debugger test on x86-64 has no difference but two P1 pairs.

**Fused compares** (WP-157; §5.8): the compare's instruction and the
moves that place its operands are at the compare's row location, the
consumer's instruction at the consumer's, for a `condbr` on an `icmp`,
on an `fcmp` of `float`/`double` and on an `x86_fp80` `fcmp`; a
`select` of class `Gpr` and a `zext`/`sext` on an `icmp`; a `select` of
`x86_fp80` on an `x86_fp80` `fcmp` (`control::at_compare`, called by
`control::cond_br`, `float::cond_br`, `x87::cond_br`, `x87::select`,
`int::select`, `int`'s casts). The consumer's instructions are the `jcc`,
the `cmovcc`, or a cast's `movzx` (and `neg` for a `sext`): the cast's
`setcc` is the compare's, as in `llc`, whatever uses the cast, a store
included (WP-163; WP-157 had put it at the cast's, a row difference
after a fused `zext` feeding a store). Classified, not fixed: a `zext
i1` to `i8` is no instruction for `llc` (`setcc` writes the byte), and a
store of it folds the `setcc` into the store (`setl (%rdx)`, at the
store's location), so `llc` has no row at the cast's line; hod widens
the byte with a `movzx` at the cast's line and stores the register. The
row is of an instruction hod emits for that line; matching it would mean
folding the `setcc` into the store, an instruction selection hod does
not do. Each switch is an ordinary transition
(`FnCx::line_switch`), a row the shrink pass moves; `is_stmt` and the
column follow §5.1, §5.2 (and §5.9 with key instructions: the compare
and its consumer are separate atoms). Not a `select` of class `Fpr` on
an `fcmp`: `llc` turns it into `minsd`/`maxsd` or a `cmpsd` mask at the
`select`'s location and has no row at the compare's line, nor does hod.
`llc` 19 and 22 agree on every form; a `switch` has no compare
instruction in the IR, and a compare with users other than its consumer
is not fused (its own lowering, its own location). AArch64 is unchanged
(§5.8). Evidence: `crates/hod_x64/tests/fused_lines.rs` (each row at
an instruction of its line after the `jcc`s were relaxed to `rel8`, on
hod's object and on `llc`'s), `xtask/tests/debug_post_index.rs`
`fused_compare_keeps_its_line_on_x86_64`,
`crates/hod_x64/tests/zext_lines.rs` (the `setcc` of a cast against
`llc` 19 and 22, the `i8` store fold).

**`.debug_frame`**: `llc` writes it only for functions that are
`nounwind` without `uwtable` (`-fno-asynchronous-unwind-tables`), on
either target; hod writes `.eh_frame` for every function and no
`.debug_frame` on either (`spec/eh.md`). Not seen in any compared
module; a follow-up of both targets, not of x86-64.

**Permitted differences on x86-64** (in addition to §6.7, all
recognised by `cargo xtask debug trees`, `spec/harness.md` "debug",
"Trees", each shown equal under the debuggers by
`tests/debug/x64_places.c`):

- `sret` also covers a dynamic alloca (a variable-length array): `llc`
  writes a location list of the saved address, hod `DW_OP_fbreg o,
  DW_OP_deref`; the array's `DW_AT_count` names the artificial
  `__vla_expr` variable in both;
- `byval`: a byval parameter `llc` places at its argument area
  (`DW_OP_fbreg +16`) and hod by the pointer to it hod saved
  (`DW_OP_fbreg o, DW_OP_deref`). The same bytes from `prologue_end`;
- `realigned frame`: in a function with an over-aligned local `llc`
  realigns `rsp` and places every variable by `DW_OP_breg7`; hod keeps
  `rbp`'s frame and places the over-aligned variable by its saved
  address. The same values from `prologue_end`;
- `return-column` mirrored (§5.7): after `finish` hod's return address
  can be the row of the call (hod copies the result out of `rax` with the
  call's location), where `llc`, which coalesced the copy, is at the
  assignment's column. The harness permits either side's column being
  the call's.
- `line 0 after a call` (WP-163, `tests/debug/key_multiline.c`, lldb
  `finish` out of `add`): `llc`'s instruction at the return address is
  its register allocator's spill of the result (`movl %eax, 0xc(%rsp)`),
  which has a line 0 row, so lldb reports the next row with a line, the
  following statement's (19:15); hod copies the result out of `rax`
  under the call's location and stops at the call (17:11). Both are
  correct stops for a return into the caller; the lines differ, so the
  `return-column` class (§5.7, columns only) does not cover it, and the
  test does not `finish` there on x86-64. Matching `llc` would mean
  modelling its spills.

**What qemu's gdb stub cannot show** (the debugger sessions on x86-64
run under `qemu-x86_64 -g` on an AArch64 host, `spec/harness.md` "debug",
"Other targets"): thread-local variables (no `qGetTLSAddr` in either
debugger; the location and its relocation are held by `debug trees`),
hardware watchpoints, lldb's shared-library list. The tests mark these
sessions `skip(remote, gdb)`; nothing is worked around.

**Hooks** (one call each): `crates/hod_dwarf/src/die.rs`
(`x86_64::reloc_kind` in `write_debug_info`), `global.rs`
(`tls_has_location`, `tls_location` in `scope_variable_die`), `split.rs`
(`refuse_split_tls` at the start of `split_unit`),
`crates/hod_cg/src/emit/mod.rs` (`lines::tls_by_symbol` in the place of
a global), `emit/lines.rs` (`unit_span`). hod_x64 is not changed: its
frame base and slot offsets were already reported (`frame.rs`
`FrameDesc`, `base_offset`).

Evidence: `xtask/tests/debug_x64.rs`, `tests/debug/x64_places.c`,
`spec/landed/dwarf/WP-136.md`, `docs/status/debug-x64.md`.

### 6.18 Follow-ups of WP-097 and WP-098 (implemented, WP-111)

- **A unit without code** is written when it lists globals,
  enumerations, retained types or imports not scoped in a function
  (§1.2; `crates/hod_cg/src/emit/lines.rs` `lists_entities`).
- **A `#dbg_declare` of a `phi`** has no row and its variable no
  location (`spec/codegen.md` §9.5); a retained variable without a row
  gets a DIE without a location (§6.4 "Retained variables").
- **`debug:variable-twice`** (clang 22's coroutine ramp): its second
  declaration of the parameter was the frame pointer's `phi`, now
  dropped, so the ramp's tree equals `llc`'s (clang 19 and 22, `llc` 19
  and 22). What is left of the refusal: two rows of one variable in one
  function or one inlined instance (`func.rs`, `first_of` and the
  variable's slot). `llc` keeps the first frame-index declaration and
  drops the rest; hod refuses. No module of the corpus (14) or the `-g`
  set (198) declines with it, so the rule is not written.
- **Imports** of every shape: §6.11.
- **The line classes** read from the IR count only the pair's file and
  blocks with a predecessor, and `post-indexed-load` is permitted
  (§5.7).
- **Not done, with the reason:**
  - `DW_AT_GNU_pubnames` and `.debug_gnu_pubnames`/`pubtypes` for a
    split unit (`nameTableKind: GNU`, clang's default with
    `-gsplit-dwarf`), and `.debug_names` for `Default`: still §6.15's
    permitted difference. What `llc` 22 writes: the flag on the
    skeleton, both sections in the object (version 2, the skeleton's
    offset and size, relocated), each entry the offset of a DIE of the
    split unit, a GDB index kind (`EXTERNAL`/`STATIC`,
    `FUNCTION`/`VARIABLE`/`TYPE`) and the qualified name (`ns::f`),
    sorted by offset; namespaces and enumerators are in `pubnames`,
    base types, typedefs and named composites in `pubtypes`. Matching it
    means `llc`'s `computeIndexValue` and `getParentContextString` over
    every DIE kind, a new pass over the split unit's offsets and two
    new relocated sections: more than a follow-up, and gdb and lldb
    index the DIEs themselves.
  - `DW_AT_ranges` on a coroutine's subprogram for its stubs after the
    ready point (§6.16 R1): `llc` writes a separate subprogram for each
    clone, so there is no tree of `llc`'s to compare a ranged body
    with, and the ranges would need a subprogram family with
    `DW_AT_ranges` beside each definition family that writes
    `low_pc`/`high_pc` (appended, §6.1), and the stubs' ready offsets passed through the bridge, which
    WP-136 is changing. What it would gain is §6.16's permitted
    differences at one stop (the frame's name and its variables there).

Evidence: `crates/hod_a64/tests/dies_followups.rs`,
`crates/hod_a64/tests/dies_label.rs`, `xtask/tests/debug_post_index.rs`,
`spec/landed/dwarf/WP-111.md`.

### 6.19 Call-site entries (implemented, WP-113)

A definition whose subprogram has `DIFlagAllCallsDescribed` (clang sets
it on every definition when optimizing: `-O1` and above, with
`-Xclang -disable-llvm-passes` too; kiln does not set it) describes its
calls, as `llc` 19 and 22 write them at `-O0`
(`DwarfDebug::constructCallSiteEntryDIEs`, DWARF 5 forms):

- **The flag.** `DW_AT_call_all_calls` (`flag_present`) on the
  subprogram, right after `DW_AT_frame_base` (and `DW_AT_object_pointer`
  when it has one), before the name: families 96 to 101 are families 4,
  69, 70, 35, 86 and 74 with that slot, same option bits
  (`call_sites::family`). An abstract instance has no flag; the
  concrete DIE of an inlined function that is also out of line has it.
- **One `DW_TAG_call_site` per call instruction** an IR `call` or
  `invoke` became (family 102), the subprogram's last children: after
  its parameters, variables, blocks, inlined subroutines and
  `unspecified_parameters`, before its template parameters. A call
  inside a block or an inlined subroutine is still a child of the
  subprogram. They are in the order the code generator lowered the
  calls (the IR's), which is not address order: a `musttail` call's
  branch is at its tail exit after the body (`spec/codegen.md` §5.4),
  and its entry keeps the call's place.
  - a direct call: `DW_AT_call_origin` (`ref4`), then
    `DW_AT_call_return_pc` (`addrx`): the address right after the call
    instruction (`bl`, `call`);
  - an indirect call: `DW_AT_call_target_clobbered`, `DW_OP_reg<n>` of
    the register the call goes through (`llc` 19 writes the attribute
    as `DW_AT_call_target`), then `DW_AT_call_return_pc`;
  - a `musttail` call: the origin or the target, `DW_AT_call_tail_call`,
    and `DW_AT_call_pc` (`addrx`), the address of the branch (`b`, `br`,
    `jmp`); no return address.
  No call site has parameters (`DW_TAG_call_site_parameter`): `llc` writes
  them only from `-O1` code (`-emit-call-site-info`), never at `-O0`.
- **Which calls have one** (`llc`'s rule: a call whose callee is a
  `GlobalAddress` with a `DISubprogram`, or a register): a call of a
  function of the module, defined or declared, that has a subprogram;
  every indirect call. None: a function without a subprogram, an alias,
  what the code generator calls on its own (the runtime, `memcpy`,
  `__tls_get_addr`, a libcall an intrinsic becomes: external symbols to
  `llc`), and an intrinsic with no call.
- **The origin** is `getOrCreateSubprogramDIE` of the callee:
  - a definition of this unit: its concrete DIE (never its abstract
    instance, which only an inlined scope finds). A callee defined later
    in the module gets its DIE at the call, in its scope (after a member
    function's declaration DIE), and its attributes when its own
    function is added (`call_sites::call_origin`, `Inline::mark_early`).
    A callee whose subprogram is a definition with no code in this unit,
    or in another unit, is refused (`debug:call-site-origin`);
  - a declaration: its declaration DIE, made in its scope at the first
    call (a namespace is made then, a class member is the class's
    declaration DIE). A C declaration has `DW_AT_prototyped` (family
    103: family 3's slots, the flag, and `DW_AT_noreturn`).

The code generator reports each call (`hod_cg::debug_calls`): the
engine arms a row before it asks the target for the call, and each
target says where the call returns (its `emit_call`, right after the
call instruction) and where a tail exit branches (right before the
branch). A row a target did not complete is an internal error: an
address is never guessed. `hod_dwarf` checks every address is inside the
function and that the returning calls are in code order
(`debug:call-site-pc` otherwise).

A tail exit of a function that describes its calls also gets a line
row at its start with the `musttail` call's location, as `llc` gives the
exit sequence the call's location: lldb shows a tail call's artificial
frame at the line of the row at its `DW_AT_call_pc` (line 0 before).

**Refused**, by name:

| name | what |
|---|---|
| `debug:call-sites` | the flag in a unit of version 2 or 3 (which `classify` refuses first); version 4 is written since WP-150 (§6.21) |
| `debug:call-sites-line-tables` | no longer returned: a `LineTablesOnly` unit's call sites are written since WP-150 (§6.21); the name is kept |
| `debug:call-site-origin` | a callee whose DIE `llc` would make in another unit, or a definition without code here |
| `debug:call-site-intrinsic` | a call of an address the lowering computed (a coroutine's resume or destroy, WP-105), and a coroutine body (`llc` describes `CoroSplit`'s clones) |
| `debug:call-site-pc` | an address outside the function, or returning calls out of order (a bug of the code generator, declined) |

**Permitted differences** (§6.7): the addresses and the target register
are masked in the tree comparison (hod's code is not `llc`'s; hod calls
through `x16` on AArch64 and `r11` on x86-64, `llc` through any
register); a `tail` call `llc` turns into a sibling call (only when
optimizing) is an ordinary call in hod's code, so its entry is an
ordinary one. hod writes no sibling call at `-O0`, and neither does
`llc`.

Evidence: `crates/hod/tests/dwarf_call_sites.rs` (the trees of
`tests/data/call_sites/` by clang 19, clang 22 and kiln++ against `llc`
19 and 22 on both targets, every address of both writers checked
against the disassembly), `crates/hod_dwarf/src/call_sites.rs`'s unit
tests, `tests/debug/call_sites.c` (tail calls, AArch64) and
`call_sites_direct.c` (both targets), `spec/landed/dwarf/WP-113.md`.

### 6.20 DWARF version 4 (implemented, WP-114)

A module whose flag `"Dwarf Version"` is `i32 4` (clang's and kiln++'s
`-gdwarf-4`) is written as `llc` 19 and 22 write it. Every other version
is refused by `classify` as `debug:dwarf-version`: 2 and 3. A module
with **no flag** is written as version 4, as `llc` writes it (the same
object as flag 4; a refusal until WP-150, §6.21). Split DWARF with version 4 (`-gsplit-dwarf -gdwarf-4`,
GNU fission) is refused as `debug:split-dwarf-4` (§6.15, the facade's
check and `hod_dwarf::split`).

**Design: a transcoder.** `UnitPlan::version` is 4 or 5
(`dwarf4::version_of`). The DIE writer is unchanged: a version 4 unit is
written by `Writer::unit` as DWARF 5 into scratch buffers, then
rewritten attribute by attribute (`crates/hod_dwarf/src/dwarf4.rs`,
`Out::unit`), driven by the abbreviation table of §6.1: the family of
each code (`BASES`) and its mask give the slots and their forms, so the
walk needs no knowledge of what the DIE means. Abbreviation **codes are
the same** as DWARF 5's; the version 4 table (written for the codes the
units use) gives each the version 4 tag and forms and omits the
attributes version 4 does not have. A family appended after
`fam::SPLIT_COMPILE_UNIT` is refused as `debug:dwarf-4-shape` until its
version 4 shape is checked against `llc` (WP-113's call sites, whose
version 4 form is `DW_TAG_GNU_call_site`, are such). DWARF 5 never
enters the module: the hooks are `die.rs` (`UnitPlan::version`, and
`check_dwarf5`, which refuses a version 4 plan in the version 5 writer),
`split.rs` (`write_unit_dies` hands version 4 units to
`write_debug_info4`), `types.rs` (the subrange's default lower bound)
and the bridge's `UnitPlan::line_program`.

**What changes against §6.2–§6.19** (each found by comparing `llc`'s
version 4 and version 5 trees of the same IR: `tests/debug`, the corpus
and the census, both targets):

| | DWARF 5 | DWARF 4 |
|---|---|---|
| unit header | 12 bytes: version 5, `DW_UT_compile`, address size, abbreviation offset | 11 bytes: version 4, abbreviation offset (`ABS32` against `.debug_abbrev`), address size 8 |
| strings | `DW_FORM_strx1..4` into `.debug_str_offsets` | `DW_FORM_strp` (`ABS32` against `.debug_str`, addend the string's offset); no `.debug_str_offsets` |
| addresses | `DW_FORM_addrx`, `DW_OP_addrx` into `.debug_addr` | `DW_FORM_addr`, `DW_OP_addr`, 8 bytes with the entry's relocation (`ABS64`; x86-64 TLS keeps `DW_OP_const8u` + `R_X86_64_DTPOFF64` + `DW_OP_GNU_push_tls_address`); no `.debug_addr` |
| ranges | `DW_FORM_rnglistx` into `.debug_rnglists` (WP-163; `sec_offset` before) | `DW_FORM_sec_offset` into `.debug_ranges` (`ABS32`); the transcoder reads each list by its index in the version 5 unit's table |
| unit attributes | `str_offsets_base`, `addr_base`, `rnglists_base` | none of the three |
| static data members | `DW_TAG_variable` (families 40, 54) | `DW_TAG_member`, same attributes |
| deleted member functions | `DW_AT_deleted` | not written |
| file numbers | `decl_file`, `call_file` and the line program's files from 0 | the same files from 1 (index + 1) |
| default lower bound | §6.8 (`default_lower_bound`) | -1 for languages `DW_LANG_OpenCL` (0x15) and above, so `llc` writes `DW_AT_lower_bound` wherever the IR has `lowerBound` (`dwarf4::default_lower_bound`) |

**`.debug_ranges`** (`Out::ranges_list`): a list is pairs of 8-byte
start and end, then 16 zero bytes. A block's list, in a unit with one
code span (the unit has `low_pc`), is offsets from that span's start
with no relocation; in a unit with several spans, every entry is an
absolute address with an `ABS64` relocation (the symbol of the span
plus the offset), as is the unit's own list (one pair per merged span,
§6.4). DIE expressions with `DW_OP_addrx` are rewritten to `DW_OP_addr`
and their relocations moved; an expression whose length changes and
that holds a branch (`DW_OP_skip`, `DW_OP_bra`) is refused as
`debug:dwarf-4-shape` (no producer writes one).

**The version 4 line table** (`line.rs`, `LineProgram::for_version`,
`dwarf4::write_line_header`): version 4, `header_length`,
`minimum_instruction_length` 1, `maximum_operations_per_instruction` 1,
`default_is_stmt` 1, `line_base` -5, `line_range` 14, `opcode_base` 13
and its 12 lengths; `include_directories` as NUL-terminated strings
(directories 1 and up of §4.4's table; directory 0 is the unit's
`comp_dir` and is not written), ended by an empty string; `file_names`
as name, ULEB directory, mtime 0, length 0, ended by a 0 byte; files are
numbered from 1 (`DW_LNS_set_file` is the file + 1). No MD5, no source,
no `.debug_line_str`: when every unit of the object is version 4,
`LineSections::line_str` is `.debug_line` itself, which no relocation
names.

**Permitted differences**: §6.7's, unchanged. Two show up differently in
version 4: the `sret` location list is in `.debug_loc` (`llc`'s unit has
no `loclists_base` and its `.debug_info` a relocation against
`.debug_loc`; the harness's `trees` recognises both,
`xtask/src/debug/trees.rs`); hod's file table holds every `DiFile`, so
its `file_names` can name files `llc`'s does not.

**Errors**: `debug:dwarf-4-shape` (`hod_dwarf::DWARF_4_SHAPE`), a family
or an expression whose version 4 shape is not checked;
`debug:split-dwarf-4` (`hod_dwarf::split::SPLIT_DWARF_4` and the
facade), split DWARF with version 4 (or no flag); `debug:dwarf-version`
(`classify`), versions 2 and 3.

**Speed**: DWARF 5 does not pass through the module (its objects are
byte-identical to before WP-114 on every `-g` module of the corpus, the
census and the test sources). Version 4 costs the version 5 writer plus
one linear pass over the unit's bytes, about 1.3 times version 5's cost
per DIE; see `spec/landed/dwarf/WP-114.md`.

Evidence: `xtask/tests/debug_dwarf4.rs` (trees, forms, line headers,
ranges, lower bounds, sessions on both targets), `tests/debug/dwarf4_*`
(the twins of nine debugger tests and `dwarf4_shapes.cpp`),
`crates/hod/tests/dwarf4.rs` (the facade's refusals and objects),
`spec/landed/dwarf/WP-114.md`.

### 6.21 Debug leftovers (implemented, WP-150)

What WP-113 to WP-115 left refused or unexplained, each against `llc`
19 and 22 at `-O0` on both targets:

- **No `Dwarf Version` flag** is version 4 (§6.20), as `llc` writes it:
  `dwarf4::version_of` is 5 only for the flag 5, and `classify` lets
  `None`, 4 and 5 through. A module without the flag gives the object of
  the same module with the flag 4 (`crates/hod/tests/debug_e2e.rs`).
  hod_dwarf's own tests now carry the flag. DWARF 5 output is unchanged.
- **DWARF 4 call sites** (§6.19's entries in a version 4 unit): the
  transcoder (`dwarf4.rs`) maps families 96 to 103 to the GNU forms
  `llc` writes there:
  - `DW_AT_call_all_calls` becomes `DW_AT_GNU_all_call_sites` (0x2117);
  - `DW_TAG_call_site` becomes `DW_TAG_GNU_call_site` (0x4109);
  - `DW_AT_call_origin` becomes `DW_AT_abstract_origin`;
  - `DW_AT_call_target_clobbered` becomes `DW_AT_GNU_call_site_target_clobbered`
    (0x2114);
  - `DW_AT_call_tail_call` becomes `DW_AT_GNU_tail_call` (0x2115);
  - `DW_AT_call_return_pc` and `DW_AT_call_pc` both become `DW_AT_low_pc`
    (`DW_FORM_addr`). For a returning call it is the return address. For
    a tail call it is the address right **after** the branch, not the
    branch. Since WP-163 the target reports it: it calls
    `FnCx::tail_branch_ends_here` right after the branch it wrote
    (`frame.rs` of `hod_a64` and `hod_x64`), and the end is mapped
    through the shrink pass's offsets like the branch's start
    (`spec/codegen.md` §5.6.15), into `DebugCallRow::end` and
    `CallSiteInput::end`. Before, `debug_calls::tail_branch_len` assumed
    the lengths (`jmp rel32` 5, `jmp *%reg` 2 or 3, `b`/`br` 4); a row
    whose end the target did not report is an internal error.
- **Call sites in a `LineTablesOnly` unit** (formerly
  `debug:call-sites-line-tables`), as `llc` 22 writes them:
  - the minimal subprogram (§6.10) of a function that describes its
    calls is `DW_AT_low_pc`, `DW_AT_high_pc`, `DW_AT_call_all_calls`,
    then its name or `DW_AT_abstract_origin` (family 104, "subprogram
    (line tables only, calls)");
  - its call sites are §6.19's;
  - a callee's declaration DIE is at the unit's level, as in a full unit;
  - a callee whose function has no DIE in the unit (a definition of the
    unit with no code, whose minimal subprogram is not written) gets an
    **empty** `DW_TAG_subprogram` at unit level, made at the call, with no
    attributes (family 105, "subprogram (empty)";
    `Calls::seal_call_sites`). Both families are version 4 as well.
- **Split DWARF 4** stays refused as `debug:split-dwarf-4`. GNU fission
  needs, none of which hod writes:
  - a skeleton unit with `DW_AT_GNU_dwo_name`, `DW_AT_GNU_dwo_id`,
    `DW_AT_GNU_addr_base`, `DW_AT_GNU_pubnames`, and the unit's ranges;
  - the `DW_FORM_GNU_str_index` and `DW_FORM_GNU_addr_index` forms, and
    `DW_OP_GNU_addr_index` in expressions;
  - `.debug_str_offsets.dwo` and `.debug_addr` without their version 5
    headers;
  - `.debug_gnu_pubnames` and `.debug_gnu_pubtypes`, which `llc` writes
    for every split version 4 unit.
  That is a second skeleton writer and a second set of forms, larger than
  the transcoder; no producer of the corpus uses it.
- **TLS call paths**: the model's runtime calls (`__tls_get_addr`,
  `__tlsdesc_*`) have no call site in any model (global-dynamic,
  local-dynamic, initial-exec, local-exec) on either target, as in
  `llc` (§6.19: an external symbol). kiln++ has no `-ftls-model`; its
  global-dynamic code is checked too
  (`crates/hod/tests/dwarf_call_sites4.rs`, `thread_local_models`).
- **Coroutine `DW_AT_ranges` and split units' `.debug_names`**: not
  written, for these reasons.
  - `llc` describes a coroutine through `CoroSplit`'s clones: one
    subprogram per clone (`.resume`, `.destroy`, `.cleanup`), each with
    its own `low_pc` and `high_pc`. hod lowers a coroutine as one function
    (`debug:call-site-intrinsic`, WP-105), so there is no tree of `llc`'s
    to compare a `DW_AT_ranges` with. Writing one would need range
    families for the subprogram and the clone offsets through the bridge.
  - `llc` 22 writes `.debug_names` (version 5) in the object for
    `-gsplit-dwarf -gpubnames`. hod writes no name index for any unit
    (§1.1), split or not, so the split case is not a gap of its own.
- **AArch64 `DW_OP_breg31` against `DW_OP_fbreg`**: the cause is a
  realigned frame (a local of alignment greater than 16, such as
  `_Alignas(64)`). `llc` then addresses the locals from `sp`
  (`DW_OP_breg31 WSP+N`, the frame base staying `x29`), and hod from the
  frame base (`DW_OP_fbreg`). Both are correct for the frame each wrote.
  This is x86-64's `breg7` class (§6.7), and the tree comparison accepts
  it as "realigned frame" on both targets (`xtask/src/debug/trees.rs`).
- **A `dbg_declare` of a slot plus a constant offset** (a `getelementptr`
  of an `alloca` named by `#dbg_declare`) is refused as
  `debug:declare-offset` (`hod_cg::policy`), no longer an internal error.

Evidence: `crates/hod/tests/dwarf_call_sites4.rs` (trees against `llc`
19 and 22 on both targets, version 4 and line tables only, TLS models),
`crates/hod_dwarf/src/call_sites.rs` and `dwarf4.rs` unit tests,
`crates/hod/tests/debug_e2e.rs`, `tests/debug/utf8_source.c`,
`spec/landed/dwarf/WP-150.md`. `.debug_frame` is `spec/eh.md` §10.

### 6.22 A declaration of `undef` or `poison` (implemented, WP-156)

A `#dbg_declare` whose address is `undef` (clang 19) or `poison` (clang
22) has no row (`hod_cg` `driver.rs` `finish_vars`,
`declares_undef`), so its variable has no DIE, unless it is a retained
node, which gets a DIE without a location (§6.4 "Retained variables").
That is what `llc` 19 and 22 write at `-O0` on both targets:
`SelectionDAGBuilder::visitDbgDeclare` drops the declaration of an
`UndefValue` address, and no other record names the variable. It is
the same decision as for a dropped address (`!{}`) and a `phi` (§6.18).

Where it comes from: the SysV x86-64 ABI passes no argument for an
empty record, so an empty-class parameter of an `always_inline` callee
(libstdc++'s `std::__distance(first, last, random_access_iterator_tag)`,
`std::__advance`) has no storage once clang's always-inliner has
inlined it, and clang declares the parameter at `undef`/`poison`. On
AArch64 the same parameter is an `i8` argument with a slot, so the
declaration names that slot and nothing changes there. Before WP-156
hod refused such a module as `debug:declare-address` on x86-64 only
(`containers`, `exceptions` and `stl_heavy` of `bench/src` at `-g`).

`debug:declare-address` keeps its meaning for every other address that
has no home: another constant (a null pointer, a symbol), or a value
the policy keeps in a register. No producer of the corpus, the `-g` set
or the benchmark inputs writes one at `-O0` on either target.

Evidence: `crates/hod/tests/declare_undef.rs` (both spellings, both
targets, trees equal to `llc` 22's), `tests/debug/empty_tag_inlined.cpp`
(gdb and lldb stop in the inlined callee and read its other parameters
and the caller's variables), `spec/landed/dwarf/WP-156.md`.

### 6.23 Variables described by `#dbg_value` (implemented, WP-162)

A variable that a function describes by `#dbg_value` records gets a
location list: `DW_AT_location` is `DW_FORM_sec_offset` into
`.debug_loclists` (`DW_FORM_loclistx` in a split unit, §6.15). Every
entry is a range hod can prove; outside the entries a debugger says
`<optimized out>`. hod never writes a location the value is not in.

**Rows** (`crates/hod_cg/src/dbg_value.rs`). `value_records` (in
`layout_slots`) finds the records of the function; a function without
any pays one flag test per block. As each block is emitted, a record
opens a range of its `(variable, inlinedAt)` and closes the previous
one:

- the range starts at the code of the instruction after the record. A
  record before a non-entry block's first instruction starts at the
  block's first byte (before the `phi` loads), unless its value is a
  `phi` of that block;
- it ends at the next record of the same variable in the block, or at
  the end of the block when the terminator is `br`, `switch`,
  `indirectbr` or `invoke` (the fused compare and branch emit the whole
  block's tail at the terminator), or at the start of the terminator for
  `ret`, any other terminator and a `musttail` call. Ranges never
  cross blocks;
- what the range holds (`ValueAt`): the value's frame home, when it has
  one that is written before the range starts (an argument, an earlier
  instruction of the block or an instruction of a dominating block) and
  its type is `i8`, `i16`, `i32`, `i64`, `ptr`, `float` or `double`; a
  constant, for an integer of at most 64 bits or `null`; otherwise
  nothing (a register value, `undef`, `poison`, a slot address, a
  floating constant), and the range gets no entry.

Ranges are code offsets: `move_rows` (`shrink.rs`) moves them with the
other tables when branches shrink (`crates/hod/tests/dbg_value.rs`
`ranges_follow_the_final_code` compares them with the final line rows
on x86-64). A record with a `DIArgList` is `debug:dbg-value-arglist`;
a value record in a coroutine body is `debug:dbg-value-coroutine`.

**Entries** (`crates/hod_dwarf/src/loclists.rs`). Each list is
`DW_LLE_base_addressx` (the function's address), one
`DW_LLE_offset_pair` per range with a location (adjacent ranges with the
same location merged), `DW_LLE_end_of_list`. Each unit's lists are one
`.debug_loclists` contribution, started on the first list; a unit with
none writes no section, so objects without value records do not change.
The expression of a record is checked before code generation:

- allowed: the empty expression, one ending in `DW_OP_stack_value`, or
  one ending in `DW_OP_deref` (a memory location: the value is the
  address, and the final deref is dropped), with the plain DWARF ops
  the `#dbg_declare` path writes in between;
- `debug:dbg-value-fragment`: a `DW_OP_LLVM_fragment`;
- `debug:dbg-value-expr`: any other op (`DW_OP_LLVM_convert`,
  `DW_OP_LLVM_arg`, entry values, `DW_OP_LLVM_tag_offset`, ...), an op
  after `DW_OP_stack_value`, or another last op.

A home with the empty expression is `DW_OP_fbreg n`; otherwise
`DW_OP_fbreg n`, `DW_OP_deref` (`DW_OP_deref_size k` for a value under
8 bytes), the record's ops. A value under 64 bits (a home or a
constant) takes only ops that keep the low bits (`plus_uconst`,
arithmetic, logic, `shl`, stack ops); with another (`shr`, `div`, a
deref) the range has no entry, since the value's high bits are not
known. A memory location needs an 8-byte value. A constant is
`DW_OP_consts` when the variable's type is a signed base type, else
`DW_OP_litN` or `DW_OP_constu`, then `DW_OP_stack_value` for the empty
expression.

**DIEs.** Families 107 to 110 (`FORMAL_PARAMETER_LIST`,
`VARIABLE_LIST`, `VARIABLE_UNNAMED_LIST`, `VARIABLE_ALIGNED_LIST`) are
families 0, 1, 87 and 88 with a `sec_offset` location; 111 and 112 are a
concrete parameter or variable of an abstract instance with one. A
variable of value records whose ranges all have no entry has no
location and keeps its base family. In an inlined instance the
variables of declarations come before those of value records, the
order of `llc`'s `collectVariableInfo`. hod always writes a list
where `llc` may write a single location or `DW_AT_const_value`; the
DIE trees differ there (§6.7).

**DWARF 4**: the list families are past `LAST_CHECKED` (§6.20), so a
version 4 unit with a located value variable is `debug:dwarf-4-shape`.
`.debug_loc` is a follow-up.

**Sessions**: `tests/debug/dbg_value.c` (`-O1`) prints variables inside
and outside their ranges on both targets. The harness class
`value-range` (§6.7) permits a line where hod says `<optimized out>`
(gdb), `<variable not available>` or `<unavailable>` (lldb) and `llc`
has a value; never the reverse, never another value.

Not done: ranges across blocks (they would give loop variables and
values defined before a loop a location over the loop body; today a
loop header's records start ranges only in the header); a home for
register values a record names (most of `-O1` code's records); slot
addresses and floating constants; `.debug_loc`. Evidence:
`spec/landed/dwarf/WP-162.md`.

## 7. Speed and memory

### 7.1 Parallel emission and determinism

The unit of work is the function. A function's line sequence (§5.6) and
its DIE run (§6.2) depend only on its rows and on unit-level numbering
fixed before any function is compiled (file indices from the `DiFile`
nodes, §4.4; abbreviation codes from the fixed table, §6.1; DIE offsets
of unit-level DIEs are patches). So a worker can encode them next to its
code in the chunk, with relocation offsets relative to the run's start;
the merge appends the runs in module order and rebases the relocations,
as it does for code. The bytes are then the same for any thread count
and any division into chunks, by construction; the tests compare the
object for 1 and N threads as `spec/codegen.md` §11 does for code.

The lines (WP-056): a sequence is encoded into a `LineProgram` (§3) as
soon as its function is placed, and `write_debug_line_units` takes the
encoded programs. The bridge encodes in the merge, in module order (the
merge places functions in that order), which makes the bytes the same
for any division into chunks and any thread count without the workers
encoding: the encoding is about 24 ns per row (§8.7), small next to code
generation. A worker can encode into a per-chunk `LineProgram` and the
merge `append` them, should the merge's share ever matter; the bytes
are the same by construction.

**The DIEs (implemented, WP-053): one function per phase.**

1. `UnitPlan::new(&debug, unit)`, once per unit, before or while code is
   generated. It needs only `hod_ir`. The plan is read-only from then on,
   so every worker can share it.
2. The workers record, per chunk, where things landed: a `GlobalAddr`
   per global they emitted (the expression its symbol names, the symbol,
   the addend) and a `CodeSpan` per function. These are plain rows, in
   any order.
3. `write_debug_info(obj, line, &[UnitInput])` at the merge. It takes
   each unit's rows as one slice: the chunks' rows concatenated in any
   order.

The writer makes the output independent of how the rows arrive. A
global's address-table entry is numbered by its DIE's place in the walk,
not by its place in the input. Spans are sorted by (symbol, start, size)
and merged before they are numbered. So the bytes depend on the rows as
sets. `tests/dies.rs` checks this by shuffling the globals and the spans,
cutting every span in two at a random point, repeating pieces, dealing
the rows into one to five chunks joined in random order, and building
plans and writing on other threads. Every variant gives the same bytes
on both targets.

Function DIEs are not encoded by the workers (WP-057, §9, open question
8): the unit writer writes them after the merge. Should per-function runs
be wanted later, a run needs its string and address indices fixed before
the merge. The two options:
- the plan assigns the indices of unit-level strings up front, and a
  run's own strings get per-run indices rebased at the merge (`strx` is a
  ULEB, so a rebase changes lengths: use `strx4` in runs, or patch before
  the lengths are fixed);
- a run carries its strings as `(position, string)` patches, resolved by
  the merge in run order.

### 7.2 Budget

For scale: `llc -O0` takes about twice as long with `-g` on a large
module. hod's budget for DWARF is **at most 25% of its own code
generation time** for the same module, of which the line table at most
5%. Measured (§8.5, on a shared host at load 25 on 24 cores, so
pessimistic): the policy costs about 6 ns per row and the writer about
12 ns per row (some 340 MB/s of `.debug_line`), so about 18 ns per row in
all. `hod_cg` records about one transition per 4 to 6 bytes of `-O0`
code, so the line table costs about 4 ns per byte of code; whether that
stays under 5% is to be measured against `hod_cg`'s own ns per byte with
the bridge (§10 row 1). Neither reserving more output nor encoding each
row into a stack buffer before appending it changed the writer's time.
It is spent in the data-dependent branches of the encoding. Memory: the line
writer allocates the two output buffers (reserved from the row count)
and one pending-relocation table; nothing per row or per sequence. The
policy appends into the caller's `Vec<Row>`, reused across functions.

The DIEs (WP-053, §8.6) were measured on the same shared host, best of
five runs, on 100,000 generated globals: 100,037 DIEs and 5.8 MB of DIE
sections (2.3 MB of them `.debug_str`), with 275,016 relocations.
- The plan costs about 38 ns per DIE (3.8 ms).
- The writer costs 350 to 450 ns per DIE (35 to 45 ms, 125 to 165 MB/s).
  Most of that is the string pool: each global brings two or three new
  strings (its name, its linkage name, and its namespaces' names the
  first time), and the pool alone costs 80 to 100 ns per string, 46 to
  60% of the writer's time. A table that does not fit in cache misses on
  nearly every new string, so the id-keyed pool of §6.3 is the next step.
- Before the hash's final mix (§6.3), the writer cost 2,900 ns per DIE.
- Reserving the table up front changed nothing.
- Size: on §8.6's `llc` modules (6 objects, 4,222 DIEs), hod's DIE
  sections are 1.020× `llc`'s (215,748 bytes against 211,560).
  `.debug_str`, `.debug_str_offsets` and `.debug_addr` are the same size.
  `.debug_abbrev` is smaller (884 bytes against 1,422).
  `.debug_info` is larger (67,438 against 62,712), about 1.1 bytes per
  DIE, nearly all of it the second byte of the core families' codes (open
  question 7).

The type graph (WP-058, `type_graph_throughput`, same host, release,
best of five) on the oracle input of `stl.cpp` (clang 19; `std::vector`,
`std::map`, `std::string`, `std::unique_ptr`, `std::shared_ptr`,
`std::function`, `<iostream>`), 6,393 DIEs:
- the plan costs 67 ns per DIE (0.43 ms), the writer 71 ns per DIE
  (0.46 ms), 138 ns per DIE in all. `llc`'s whole run of the same module
  (functions deleted) takes 61 ms;
- size: `.debug_info` is 45,225 bytes against `llc`'s 42,391 (1.067×);
  the five DIE sections are 159,748 against 158,038 (1.011×), because the
  fixed `.debug_abbrev` holds only the codes used. Over §8.6's
  comparisons, `.debug_info` is 1.066× (`stl.cpp`), 1.164× (`types.cpp`,
  a module made of small types), 1.122× (hand-written) and 1.086× (the
  corpus) `llc`'s: two-byte codes and two-byte ULEB values from 128 to 255
  (§6.7).

Function DIEs (WP-057, `dies_throughput`, same host, release, best of
five; the module emission timed with and without `FullDebug`):
- 20,000 locals in one function: 181 ns per DIE (3.66 ms); at 40,000,
  190 ns (ratio 1.05);
- one function with scopes nested 20,000 deep, a local in each (40,000
  DIEs): 166 ns per DIE; at 40,000 deep, 178 ns (ratio 1.07);
- 2,000 functions of ten locals in two blocks: 162 ns per DIE; at 4,000,
  163 (ratio 1.00).
So the cost per DIE stays flat from N to 2N on all three shapes: linear.
Inlined code (WP-085, `dies_inline_throughput`, release, best of five,
emission with DIEs minus without, per DIE):
- 20,000 inlined calls of one function: 248 ns per DIE; at 40,000, 261
  (ratio 1.05);
- an inlining chain 20,000 deep (as many abstract instances): 286 ns;
  at 40,000, 286 (ratio 1.00);
- 2,000 functions of ten inlined calls: 195 ns; at 4,000, 214 (ratio
  1.10).
An inlined subroutine costs more than a variable: it has a range or a
range list, a call file and line, and its key walk. `.debug_info`
against `llc` 22 on clang 22's IR: 1,167 bytes against 1,088
(`inline.cpp`), 278 against 265 (`statics.cpp`), 280 against 261
(`debug.cpp`): 1.05 to 1.07, the same two-byte codes and
`sec_offset` range lists as §8.8's.
Size against `llc` 22 on the same IR (§8.8's sources; hod's code is
larger, so the totals include more `.text`): `.debug_info` 999 bytes
against 955 (`vars.c`, clang 22; 991 against 956 by kiln++), 633 against
633 (`members.cpp`), 183 against 177 (`hand.ll`); all sections 5,176
against 4,634, 3,929 against 3,463, 1,205 against 951. The extra
`.debug_info` bytes were the block range lists' `sec_offset` (4 bytes
where `rnglistx` takes 1; `rnglistx` since WP-163) and the two-byte codes. `stl.cpp` cannot be
measured end to end yet: its functions are refused (§6.4:
`debug:inlined-subroutine` in 52 of 383, `debug:specification` in 43,
then `debug:local-type-scope`), as are 2 by the code generator
(`a64:atomic`) and its module `asm`.

## 8. Tests

### 8.1 Units (`tests/lines.rs`, implemented)

The encoding of every advance case (special opcode, `const_add_pc` with
special, `advance_pc` with special, `advance_line` with `copy` and with
`advance_pc`), the boundaries 17/18 and line deltas −5/8/9, `end_sequence`
advances of 0, 17 and more; the header with and without MD5; string
pooling; `FileTable`'s splitting rules; every error of §4.6 with the
object unchanged; output identical under permutations of the sequences;
the policy rules of §5.1 to §5.4 one by one.

### 8.2 `llc` (`tests/llc_oracle.rs`, implemented)

Modules (`tests/data/lines/*.ll` from `clang 19 -g -O0`, kiln's
`-g` modules when `HOD_DWARF_CORPUS` names the corpus), each compiled by
`llc` 19 and 22 for AArch64 and x86-64. The object's line table is
decoded with `llvm-dwarfdump --debug-line` and its relocations into a
`LineHeader` and `Sequence`s (`llc`'s grouping, `order` its position),
written with `write_debug_line` (also with the input reversed) and
spliced into the object. Asserted:

- **byte identity** of `.debug_line`, its relocations (same offsets,
  types and targets) and `.debug_line_str`;
- the decoded rows of the spliced object equal `llc`'s (address, file,
  line, column, flags) and `llvm-dwarfdump --verify` is clean;
- the bytes of one sequence per function (hod's grouping, §5.6) also
  verify clean, and are measured against `llc`'s;
- the **policy** against `llc`: each function's rows are turned back into
  transitions (the start row dropped, its line taken as `scope`; the
  `epilogue_begin` row as the epilogue), passed through `function_rows`,
  and must give `llc`'s rows, except the differences named in §5.2 (a
  file change at the same line is a statement) and §5.1 rule 4
  (duplicates merged), which the test counts and reports.

Result (WP-052, both majors alike): with the corpus, 18 objects with
164 sequences and 19,387 rows were byte-identical. Of 212 functions, 60
came out exactly as `llc` writes them. All 152 with differences are in the
generated module, and all of them are the named file-change statements
(1,476 rows). No duplicates were merged. Every clang and kiln function
matched exactly. One sequence per function took 102,211 bytes against
`llc`'s 103,031 (0.992x; `llc` ends a sequence at every section switch,
and the generated module switches often). Kiln modules that ask for
DWARF 4 are skipped with a note (open question 6).

### 8.3 Debuggers (`tests/debugger.rs`, implemented for the line table)

`tests/data/lines/` holds a two-file C++ program (a header with a
template and a static function; a second translation unit instantiating
the same template, so the linked program has a discarded COMDAT copy),
compiled by clang 19 at `-O0 -g` (AArch64 hosts only). `llc` 19 and 22
compile it. In both objects, `.debug_line`, `.rela.debug_line` and
`.debug_line_str` are replaced by hod's: the policy's rows (§5), one
sequence per function (§5.6). Linked with GNU ld, lld and mold, a gdb
batch session sets breakpoints by `file:line` in the header (two
locations, one of them the COMDAT copy), in `main.cpp` and in
`second.cpp`. It then runs, takes a `bt` and `info locals`, and goes
through `continue`, `next`, `step` and `finish`, with gdb printing the
frame and source line at each stop. The transcript must equal the
session on the original program, with addresses and pids masked. A
deliberately wrong table (every line shifted by one) must give a different
transcript, so the test can fail. lldb (found by `find_tool`, optional)
runs the same session with `breakpoint set --file --line`,
`thread step-over`, `step-in` and `step-out`.

Result (WP-052): on every pair of major and linker, gdb stopped 11 times
over 40 lines of transcript, and lldb stopped 20 times over 224 lines.
hod's transcripts were identical to the originals, and the wrong table's
differed every time.

### 8.4 L7 (`cargo xtask debug`, WP-060)

The contract of the command is `spec/harness.md` "debug". Four parts:

1. **Sessions** over `tests/debug/`: each program built from the same IR
   by hod and by `llc -O0 -global-isel=0 -fast-isel=0`, linked by every
   linker present (GNU ld, lld, mold; wild when installed), and the same
   gdb and lldb scripts run on both. The masked transcripts must be equal;
   what is masked is exactly the program's directory, `0x` numbers,
   process and thread ids, and code offsets (`<main+20>`, `` prog`main +
   20 ``). Lines, columns, values, names, frames and the number and order
   of stops are not masked. A debugger crash is its own outcome.
2. **kiln's debugger tests** (`tests/debuginfo` of kiln) compiled by
   kiln++ with hod (through the shim, strict) and with `llc`, judged by
   their own checks.
3. **`--verify`**: every `-g` module of the corpus hod builds is
   `llvm-dwarfdump --verify` clean (LLVM 22 and 19).
4. **`(function, line)`**: the set of `(function, file, line)` of the
   non-empty `is_stmt` rows with a line of each program, compared; a pair
   one side lacks is P1 or P3 of §5.7 or unclassified, and an
   unclassified pair fails.

**Seeds** (`tests/debug/`): `step.c` (breakpoints by line and function,
`step`, `next`, `finish`, `up`/`down`; lldb's step-in, -over and
-out), `places.c` (locals and parameters in every place hod puts them,
§6.4: integer and floating-point arguments spilled to their slots, a
small struct by value, a large one by reference to a copy, a struct
returned through memory, an over-aligned local, arrays, pointers,
shadowing in nested blocks, a variadic function), `globals.c` (globals, file statics, function
statics, a constant table, a pointer to a function), `classes.cpp`
(members, `this`, a base with a vtable, a class and a function template,
a static member, a scoped enumeration, nested namespaces, `ptype`),
`inlined.cpp` (a backtrace through an always-inline frame, `finish` out
of it), `exception.cpp` (a throw, the backtrace, the handler with the
caught reference after unwinding through hod's CFI), `tls.c`
(thread-local globals in two threads) and `comdat.cpp` with
`comdat_other.cpp` (the same inline function with `-g` in two objects,
one from hod and one from `llc`, in both orders: one copy kept, a
breakpoint on it, both callers in the backtrace). C tests run on clang
19 and 22; C++ on those and kiln++ (`classes.cpp` on clang only: kiln
describes its `Box` as incomplete with `llc` and hod alike).

**Result** (WP-060; AArch64, gdb 16.3, lldb 19; three linkers): 198
results over 12 tests, 360 sessions: 168 PASS and 30 XFAIL, every XFAIL
one of the findings below. `(function, line)`: 458
pairs, all the same, none unclassified. kiln's 4 debugger tests with gdb
and lldb: 8 of 8 pass with `llc` and 8 of 8 with hod; 7 of 8 transcripts
equal (the eighth is finding 4). Corpus: 14 `-g` modules, 10 built and
all clean, 4 declined (`debug:dbg-value` 2, `debug:dwarf-version` 2).

**Findings**, each reduced to a seed with an `xfail` naming it:

1. `tls_static.c` (gdb): a function's `_Thread_local` static gets
   `DW_AT_location (DW_OP_addrx <symbol>)`, as an ordinary static does,
   and gdb prints the word at the TLS offset as a value (wrong).
   `llc` writes no location for any thread-local variable on AArch64
   (gdb: `<optimized out>`). `discover_scope_global`
   (`crates/hod_dwarf/src/global.rs:51-58`) returns for a variable
   scoped in a function before the thread-local check at line 59, and the
   static path writes the address. Not a permitted difference: a wrong
   value. lldb shows neither program's value.
2. `sret_entry.c` (gdb, lldb): at a breakpoint on a function returning a
   struct in memory, `llc`'s location list of the result variable has a
   gap at the first line's address (x8 until the instruction before, the
   spilled pointer from the one after): `<optimized out>` /
   `<variable not available>`. hod's single location shows the caller's
   object. In hod's favour; not in §6.7's list, so XFAIL until §6.7 says
   so.
3. `gep_column.c` (lldb): the first instruction of `Pair p = {n * 10,
   3};` is, in hod's program, the field's `getelementptr` (column 12);
   `llc` folds it into the store and its first is the load of `n`
   (column 13). Same address and line; lldb prints the column.
   Instruction selection (`crates/hod_cg`).
4. `finish_column.c` (lldb), reduced from kiln's `step_finish.cpp`: the
   instruction after a call has the call's column in `llc`'s program and
   the assignment's in hod's; `finish` stops there and lldb prints it.
   Instruction selection.

Findings 3 and 4 are columns only (gdb shows none, and the `(function,
line)` sets are equal); a §5.7 entry that permits a different column at
a row's address, or code generation that matches `llc`'s first
instruction, would retire them.

### 8.5 Throughput (`tests/throughput.rs`, implemented)

`--release`, `--ignored`: 20,000 functions of 100 transitions shaped
like `-O0` code (4-byte instructions, a new location every 1 to 6
instructions, small line steps with occasional jumps and file changes,
columns changing at every row), passed through `function_rows`
(1,985,902 rows) and written as one sequence each. The output reports ns
per row for the policy and for the writer, MB/s of `.debug_line`, and
bytes per row, taking the best of five runs. WP-052 measured 6.1 ns/row
for the policy and 12.4 ns/row for the writer (336 MB/s; 4.16 bytes/row,
8.27 MB). The size relative to `llc` is measured on §8.2's
modules (hod writes the same bytes for the same sequences; with one
sequence per function the overhead is §4.7's).

### 8.6 DIEs (`tests/dies.rs`, `die_oracle.rs`, `die_debugger.rs`, `type_graph.rs`, `type_oracle.rs`, `type_debugger.rs`, implemented)

The parser does not read debug metadata yet (WP-054), so the tests fill
`hod_ir`'s tables with the builder API. `tests/common/meta.rs` reads
`!DI*` nodes, tuples, `!llvm.dbg.cu` and the `!dbg` of globals from LLVM
text into `reserve`, `define` and `DiNode::set`, skipping the fields
`hod_ir` does not map. One test builds its tables by hand.

The inputs are clang 19 `-g -O0` output in `tests/data/dies/`:
- `globals`: every C++ base type, pointers (with `void *` and `int **`),
  nested, inline and anonymous namespaces, a `static`, `alignas(32)`;
- `strings`: a 307-character name, names repeated across namespaces,
  `café`;
- `main`: a function only.

There is also a generator, `globals_module(n)`: namespaces up to three
deep whose inner names repeat under different parents, some internal and
some over-aligned globals, with 400 to 100,000 globals.

- **`tests/dies.rs`** (objects built with `hod_obj`: the globals in
  `.data`, the code in `.text`, more sections, and two COMDAT groups):
  - `llvm-dwarfdump --verify` of 19 and 22 is clean, together with the
    line table written from `plan.files().header()`, on both targets, for:
    - no globals (with no code, one section, or several);
    - the three data modules;
    - 3,000 generated globals with code in four sections, two of them
      COMDAT;
  - the dumps contain what was written;
  - `.debug_str` holds each string once;
  - the kinds, offsets and targets of every relocation on both targets.
    The unit's `low_pc` has no relocation when it has ranges. No debug
    section is in a group. The code entries are against each code
    section's symbol, COMDAT included;
  - the same bytes under shuffling, splitting, chunking and threads
    (§7.1);
  - every error of §6.6 leaves the object unchanged;
  - an unsupported shape (a pointer with an address space, a struct
    scope) is `Unsupported`.
- **`tests/die_oracle.rs`** (`llc` 19 and 22 × AArch64 and x86-64 on
  `globals`, `strings` and 2,000 generated globals):
  - hod's sections, written from the tables for `llc`'s symbols, are
    spliced into `llc`'s object, and both objects verify;
  - the DIE trees are equal after §6.7's normalization;
  - the relocation targets of `.debug_info`, `.debug_str_offsets` and
    `.debug_addr` are equal;
  - addresses four bytes off are caught.
  - Result (WP-053, both majors alike): 6 objects and 4,222 DIEs, equal,
    and in `llc`'s order in all 6; sizes in §7.2.
- **`tests/die_debugger.rs`** (AArch64 hosts):
  - `globals.o` from `llc` with hod's debug sections in place of
    `llc`'s, linked with `main.o` by GNU ld, lld and mold;
  - a gdb session runs `info variables -n`, `ptype` and `whatis` of 16
    globals, and `info address` of 2. It then breaks in `main` and
    `print`s 34 expressions: globals in nested, inline and anonymous
    namespaces, `**pp`, `*outer::inner::deep_ptr`, `&aligned % 32`,
    `sizeof(ld)`;
  - an lldb session runs `target variable` of 29 names (one per command:
    lldb stops a batch at its first error) and `expression` of 7
    dereferences;
  - stdout and stderr, masked, must equal the session on the original
    program, and an address four bytes off must differ.
  - Result (WP-053): identical on every pair of major and linker (104
    lines for gdb 16.3, 86 for lldb 19); the wrong table differs every
    time. lldb cannot find `outer::half` (a namespace member declared
    after a nested namespace) in `llc`'s program either, so it is
    skipped.
- **`tests/type_graph.rs`** (WP-058, modules read by `hod_ll`): each
  named error of §6.6 (a member scoped in a namespace; typedef,
  qualifier and array cycles; a base type, an enumeration and a bit field
  without a size); a struct whose members point back at it and a pointer
  to itself, with one DIE per node; a function-local type with no DIE
  of its scope refused as `debug:local-type-scope`; template parameters skipped; a 20,000-deep chain.
- **`tests/type_oracle.rs`** (WP-058; `llc` × AArch64 and x86-64; the
  oracle input of §6.7, read by `hod_ll`, the DIEs from
  `UnitPlan::of_module`): both objects verify, the trees are equal, the
  relocation targets are equal. The inputs and the results, all in
  `llc`'s order:
  - hand-written IR (`tests/data/types/hand.ll`, `hand_c.ll`,
    `hand22.ll`; 19 and 22, `hand22` 22 only): a friend, a template
    alias, every `DW_ATE`, endianity, calling conventions on subroutine
    types, a VLA count with and without its variable's DIE, explicit
    lower bounds and bound-only subranges, wide and signed enumerators,
    C's `prototyped` and default lower bound, `_BitInt` `dataSize`;
    10 objects, 332 DIEs;
  - `types.cpp` (every feature class reached from globals) by clang 19
    (`llc` 19 and 22), clang 22 and kiln++ (`llc` 22): 8 objects, 3,164
    DIEs;
  - `stl.cpp` by clang 19 and 22: 4 objects, 25,560 DIEs;
  - with `HOD_DWARF_CORPUS=.build/corpus`, every DWARF 5 `-g` module of
    the corpus by its own LLVM: 12 modules (the other two ask for DWARF
    4, open question 6), 24 objects, 668 DIEs.
- **`tests/type_debugger.rs`** (WP-058, AArch64 hosts): `types.cpp`'s
  object from `llc` 19 and 22, with hod's debug sections in place of
  `llc`'s, linked as a shared library by GNU ld, lld and mold. gdb runs
  `ptype` of 20 types, `ptype /o` and `print sizeof` of 8, `whatis` and
  `ptype` of 43 variables, `print` of 5 static member constants and
  `info types` twice. lldb runs `type lookup` of 20 types and `image
  lookup -t` of 8. Result: identical on every pair of major and linker
  (315 lines for gdb, 253 for lldb).
- **Throughput** (`tests/throughput.rs`, `die_throughput` and
  `type_graph_throughput`, ignored): §7.2.

### 8.7 The bridge (`crates/hod_a64/tests/lines*.rs`, WP-056)

- `lines.rs`: which units are written (none without `!llvm.dbg.cu` or
  for `NoDebug`, byte-identical to no debug information; the refusals),
  the rows of `data/lines/hand.ll` (block start, inlined, explicit line
  0, two returns, no subprogram), the relocations of the sequences and
  of the unit's addresses against section symbols with and without
  `-function-sections` (`--verify` clean), the `llc` oracle (LLVM 19 and
  22: per function, the rows modulo addresses and §5.7's differences; a
  table one line off is caught; `llc`'s file entries are hod's), and the
  same bytes for 1, 2, 3 and 8 chunks on one thread or several.
- `lines_debugger.rs`: `main.c` (gcc) with `prog.c`'s functions from
  hod's object and from `llc`'s, linked with GNU ld, lld and mold; gdb
  and lldb step from `main` through everything and the stops are
  compared (hod's are `llc`'s less P1's); a breakpoint by line, `bt`,
  `info line`, `next`. The COMDAT case: `tu1.cpp` and `tu2.cpp` share the
  inline `shared`; the linked program verifies, stepping reaches it from
  both units, a breakpoint in it has one location hit twice.
- `lines_throughput.rs` (ignored): 2 000 functions of 100 statements
  (206 000 rows): what the tables cost per row and function, peak
  memory, against `llc -O0`. Numbers in `spec/landed/dwarf/WP-056.md`.

### 8.8 Function DIEs (`crates/hod_a64/tests/dies*.rs`, WP-057)

Sources in `crates/hod_a64/tests/data/dies/`: `vars.c` (recursion,
shadowing in nested blocks, by-value structs in registers, on the stack
and by memory, `float`/`double`, a variadic function, a label),
`members.cpp` (a constructor, `const` and static member functions with
and without `this`, in a namespace; a function template and an inline
function, both COMDAT; a function-local struct), `hand.ll`
(a block with two ranges, a dropped variable, a label in a label-only
block, an unused parameter, a function without variables), with `main.c`,
`main.cpp` and `hand_main.c` to drive them. Shared support is
`tests/dies_support/mod.rs`: the tree of `llvm-dwarfdump --debug-info`,
normalized as §6.7 says.

- `dies.rs`:
  - `trees_match_llc`: `vars.c` and `members.cpp` by clang 19 (`llc` 19
    and 22), clang 22 and kiln++ (`llc` 22), and `hand.ll` (19 and 22):
    hod's object and `llc`'s verify (`--verify`) and their trees are
    equal, in `llc`'s order. Result: 20 objects, 664 DIEs, 96
    subprograms, 324 variables;
  - `hand_written_cases`: the block with two ranges has `DW_AT_ranges`;
    the dropped variable has no DIE; the label-only block is flattened;
  - `refusals_are_named`: `debug:dbg-value-arglist` (was `debug:dbg-value`), `debug:dbg-assign`,
    `debug:fragment`, `debug:declare-address`,
    `debug:out-of-line-definition`, each in a module with that construct
    only (`debug:inlined-subroutine` and `debug:local-static` went with
    WP-085, §6.9);
  - `function_pointer_global_is_written`: a global of function-pointer
    type, refused before `of_module`, now has its DIE;
  - `bytes_are_independent_of_chunks_and_threads`;
  - `corpus_survey` (`HOD_DIES_CORPUS`): every DWARF 5 `-g` module of the
    corpus compiled; those hod compiles verify and equal `llc`'s trees.
    Result: 14 modules with debug information, 2 DWARF 4 (not compared),
    8 equal (204 DIEs, 36 subprograms); refused: 2 `a64:vector-element`,
    2 `debug:die-unsupported` (both a global with a non-empty expression,
    WP-059).
- `dies_debugger.rs` (AArch64 hosts): each case linked from hod's object
  and from `llc`'s, by GNU ld, lld and mold, for LLVM 19 and 22. gdb runs
  `break`, `run`, `bt`, `info args`, `info locals`, `print`, `finish`,
  `up`/`down`; lldb `frame variable` and `bt`. The masked transcripts of
  hod's program and `llc`'s must be equal and contain the expected
  values. Result: 3 cases × 2 majors × 3 linkers × 2 debuggers, all
  equal.
- `dies_throughput.rs`: `twenty_thousand_scale` compiles 20,000 locals,
  a 20,000-deep scope chain and 2,000 functions and counts their DIEs
  (the deep one at 2,000: `llvm-dwarfdump` overflows its own stack at
  20,000); `function_die_cost` (ignored) is §7.2's measurement.

Mutations the tests catch (each made, run and reverted): dropping the
variadic `DW_TAG_unspecified_parameters` (`trees_match_llc`); frame
offsets 8 bytes off in `frame::base_offset` (every `dies_debugger`
session); writing label-only blocks (`trees_match_llc`, `hand.ll`).

## 9. The bridge in `hod_cg` (implemented: lines and the unit, WP-056; function DIEs, WP-057)

`emit_module` owns the mapping from `hod_ir` and `FuncRecord` to
`hod_dwarf`'s rows (§5.7, §6). It reads the subprogram of each function
(`FunctionDesc`), the location table (`DebugInfo::location`), and the
records. `hod_dwarf` never sees a `LocId` or a `DiId`.

`DebugLines` (`crates/hod_cg/src/emit/lines.rs`):

1. `new`, once, before any function is placed: a `UnitPlan` per compile
   unit of `!llvm.dbg.cu` whose `emissionKind` is not `NoDebug` (§6.10)
   (`UnitPlan::for_functions`, the open form of `of_module`, so the
   static member constants and the whole type graph are available). A
   module without `!llvm.dbg.cu`, or whose units are all `NoDebug`, gets
   `None` and nothing else is done or written: the same bytes as the
   module without any debug information (tested). A
   `DebugDirectivesOnly` unit gets its line table and no DIEs (§6.10); a unit
   the DIE core does not write (§6.5) as `debug:die-unsupported`, a
   `debug:*` name of §6.4 as itself.
2. `add_function`, as each function is placed (§5.7), in module order:
   its sequence; a `CodeSpan` (section symbol, offset, size) for the
   unit's ranges; and the `FunctionInput` of §6.4, made from the record:
   the line transitions as `ScopeRow`s, the `vars` rows as `LocalRow`s
   (a frame place is turned into a frame-base offset by the target's
   `Target::frame_base_offset`, `crates/hod_cg/src/target.rs`; a place
   the target cannot express is `debug:frame-place`), and the frame
   base register from the frame description.
3. `finish`, after the unwind tables: each plan is sealed; per unit with
   code or globals (in `!llvm.dbg.cu` order), its line-table unit
   (`write_debug_line_units`), then `write_debug_info` with
   `DW_AT_stmt_list`, the code spans and the placed globals.

The function DIEs are written by the one unit writer at `finish`, not
per chunk: the plan is filled serially as functions are placed, so every
string and address index is assigned in one order (open question 8).

**Reduced units** (WP-061, §6.10): every unit but a `NoDebug` one gets a
plan and a line table; `Unit::dies` (`has_dies`) says whether it goes to
`write_debug_info`. Since WP-087 `classify` lets debug modules through
on AArch64 (§6.12), so a `-gline-tables-only` build reaches this from
the command line.

**Compressed sections** (WP-061): with `Options::debug_compression`
`Zlib`, `ModuleEmitter::finish` compresses the `.debug_*` sections last
(§1.1, `spec/object.md` "Compressed sections").

**`classify`'s `debug:*` refusals** dropped on AArch64 with WP-087
(§6.12); `debug:dwarf-version` replaces them for units other than DWARF
5 and (since WP-114, §6.20) 4.

## 10. Follow-on work packages

Rows for `docs/BACKLOG.md` (ids to be assigned; "this" is WP-052):

| title | scope | dependencies |
|---|---|---|
| line tables in `emit_module` (WP-056, **landed**: `spec/landed/dwarf/WP-056.md`) | §5.7 and §9 in `hod_cg::emit`: `FileTable` from `DiFile` nodes, `LocId` → `SourceLoc`, `function_rows`, one `Sequence` per function; `encode_sequence` into the chunk and the merge of encoded runs (§7.1); `LineRow` gains a block-start bit (§5.3); `DW_LNCT_LLVM_source` for `DiFile.source`; gdb/lldb sessions on hod-compiled programs (§8.3 without `llc`); 1-vs-N-thread identity | this, WP-032 |
| DIE writer core (WP-053, **landed**: `spec/landed/dwarf/WP-053.md`) | §6.1 to §6.3, §6.5 to §6.7: the fixed abbreviation table, `.debug_info` unit header, compile unit DIE, `.debug_str`/`.debug_str_offsets`/`.debug_addr`/`.debug_rnglists`, the scope inversion, reference patching; `--verify` clean and compared with `llc` on modules with globals only | this |
| subprograms and variables (WP-057, **landed**: `spec/landed/dwarf/WP-057.md`) | subprogram DIEs (`low_pc`/`high_pc`, `frame_base`), parameters and locals from `vars` with `DIExpression` composed onto the frame-base offset (`VarRow` gains its expression), lexical blocks from `lines` (§6.4), labels (the `labels` table); gdb `info locals`/`print`/`bt` on hod-compiled programs | DIE writer core, line tables in `emit_module` |
| the type graph (WP-058, **landed**: `spec/landed/dwarf/WP-058.md`) | basic, derived (pointer, reference, rvalue reference, typedef, const/volatile, member, inheritance, pointer to member, friend), composite (class, struct, union, enum with enumerators, array with subranges), subroutine types, declarations and completion, `vtableHolder`; compared with `llc` by `llvm-dwarfdump` structure; gdb `ptype` | DIE writer core |
| globals, namespaces, templates (WP-059, **landed**: `spec/landed/dwarf/WP-059.md`) | global and static-member variables (`DW_OP_addrx`, TLS with `DW_OP_form_tls_address`), namespaces, imported entities, template type and value parameters (packs), `DW_AT_specification` for out-of-line definitions | subprograms and variables, the type graph |
| L7 harness (WP-060, **landed**: `spec/landed/dwarf/WP-060.md`) | §8.4: `xtask` gdb and lldb sessions, kiln's debugger tests, corpus `--verify`, `(function, line)` comparison with `llc`; the two-object COMDAT case | line tables in `emit_module`, subprograms and variables |
| line-tables-only and compressed sections (WP-061, **landed**: `spec/landed/dwarf/WP-061.md`) | §6.10 (`LineTablesOnly`, `DebugDirectivesOnly`, mixed units) and §1.1 (`SHF_COMPRESSED` with zlib, written in safe Rust by `hod_obj::deflate`) | DIE writer core |
| split DWARF (WP-062, **landed**: `spec/landed/dwarf/WP-062.md`) | §6.15: `-gsplit-dwarf`: skeleton unit, `.dwo` sections or file, `DW_UT_skeleton`/`split_compile`; `hod_ir` maps `splitDebugFilename`, `dwoId` and `splitDebugInlining` | DIE writer core |

## 11. Open questions

1. `.debug_names` for units with `nameTableKind: Default`: gdb and lldb
   build their own index; a name index speeds up their start on large
   programs. Write it when a measurement says the start-up time matters.
2. `minimum_instruction_length` 4 on AArch64 (§4.2): smaller tables
   (fewer `const_add_pc`/`advance_pc`), but every table then differs from
   `llc`'s byte for byte. Measure on the corpus with the bridge.
3. Discriminators (`DILexicalBlockFile.discriminator`): only optimized
   code and sample profiling use them; `hod_ir` keeps them. Not written.
4. Mostly answered (WP-056): the line-0 epilogue row of a function with
   several `ret`s (§5.4) is not a statement, and gdb and lldb stop by
   line only at statements; `hand.ll`'s `two_rets` has both return lines
   and one `epilogue_begin` row. A debugger session over it is left to
   the L7 harness.
5. Answered (WP-056): the block-start rule of §5.3 is implemented.
6. DWARF 4: a module flag `"Dwarf Version", i32 4` (one kiln corpus
   module, `-gdwarf-4`) makes `llc` write version 4 tables with
   `include_directories` strings inline and no `.debug_line_str`. hod
   writes DWARF 5 only and ignores the flag, which every current gdb and
   lldb read. Write version 4 only if a consumer that needs it shows up
   (`hod_ir` maps the flag as `ModuleProps::dwarf_version`,
   `crates/hod_ir/src/props.rs:140`, so the bridge can refuse or warn).
   Answered (WP-114): version 4 is written as `llc` writes it (§6.20).
7. Every code from 128 up takes two ULEB bytes, where `llc`'s codes take
   one. That covers the core families (698 to 753) and most type families
   (`base_type` at 129, and the cv-qualified, reference and composite
   types). On globals-only modules it costs about 1.1 bytes per DIE, 7.5%
   of `.debug_info` (§7.2). Codes mean something only inside one object,
   so they need to agree only between the writers of that object.
   Renumbering in a later version of hod changes no format and no
   consumer, only the pinned tests. "Append-only" (§6.1) keeps the
   packages from renumbering one another while M3 is being built. Once
   the corpus shows which shapes are frequent (subprograms, parameters,
   locals, members, cv types), one deliberate renumbering can put them
   below 128.
8. Answered for now (WP-057): neither. Function DIEs are not written per
   chunk; the bridge adds each placed function to the unit's plan in
   module order and the unit writer writes them all (§9), at 160 to 190
   ns per DIE (§7.2). Per-chunk DIE runs become worth it only if that
   shows up in a whole-module profile; the plan's side tables are by DIE,
   so either scheme can be added behind `add_function`.
9. `#dbg_value`: written as location lists since WP-162 (§6.23).
   Open: ranges across blocks, homes for register values, `.debug_loc`
   for DWARF 4.
10. `debugInfoForProfiling` (WP-061, §6.10): `llc` writes a subprogram
    for every function of a `LineTablesOnly` unit with the flag set;
    `hod_ir` drops the flag (`crates/hod_ir/src/debug/map.rs:72`). Map it
    and refuse such units, or write those subprograms, when an input with
    `-fdebug-info-for-profiling` shows up.
