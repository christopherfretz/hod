# spec: hod_dwarf — debug information

`hod_dwarf` writes an object's DWARF 5 debug information at `-O0`: the line
table from what the code generator records per function (`spec/codegen.md`
§9.5), and the debugging information entries (DIEs) from `hod_ir`'s debug
tables (`spec/ir.md` §6) plus the same record. Like `hod_eh` it depends on
`hod_obj` only: it writes sections and relocations into an `Object` from
plain input rows of its own types, and knows nothing of `hod_cg` or of
`hod_ir`. The bridge that fills those rows from a module and its
`FuncRecord`s belongs to the caller (`hod_cg::emit_module`, §9).

This file is the design of the whole crate (§1 to §8) and the contract of
what is implemented: the line-table writer and its policy (§4, §5).
Everything else is marked *planned* and is split into work packages in
§10.

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
| `.debug_info` | one `DW_UT_compile` unit per compile unit | none | `ABS32` / `R_X86_64_32` against the section symbols of `.debug_abbrev` (`abbr_offset`), `.debug_str_offsets` (`DW_AT_str_offsets_base`, addend 8), `.debug_line` (`DW_AT_stmt_list`), `.debug_addr` (`DW_AT_addr_base`, addend 8), `.debug_rnglists` (`DW_AT_rnglists_base`, addend 12); `ABS64` / `R_X86_64_64` for the unit's `DW_AT_low_pc` (`DW_FORM_addr`) |
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
  needed only for `#dbg_value` variables tracked over ranges, which policy
  v1 drops (§9.5). `llc -O0` writes none for `clang -O0` output either.
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
| compile unit | `DiCompileUnit` (`producer`, `language`, `file` → `DW_AT_name`/`DW_AT_comp_dir`, `isOptimized`, `flags`, `runtimeVersion`, `emissionKind`, `enums`, `retainedTypes`, `globals`, `imports`, `nameTableKind`, `sysroot`, `sdk`) | the placed functions' sections and offsets (`DW_AT_ranges`) | nothing for `-O0` units; split DWARF (`splitDebugFilename`, `dwoId`) and `macros` are `unsupported` (`spec/landed/ir/WP-005.md` field table) |
| line table | `DiFile` (`filename`, `directory`, `checksumkind`, `checksum`, `source`); `DiLocation` (`line`, `column`, `scope` → file) | `lines` (offset, `LocId`) | a block-start mark on a transition (§5.3, `crates/hod_cg/src/record.rs:119`) |
| subprogram | `DiSubprogram` (name, linkage name, scope, file, line, type, `scopeLine`, flags, `spFlags`, `declaration`, `templateParams`, `containingType`, `virtualIndex`), `FunctionDesc`'s subprogram | code size and placement (`DW_AT_low_pc`/`high_pc`), `FrameDesc::frame_base` (`DW_AT_frame_base`) | nothing |
| lexical block | `DiLexicalBlock` (`scope`, `file`, `line`, `column`), `DiLexicalBlockFile` | code ranges, derived from `lines`: the union of the transitions whose location's scope chain contains the block (§6.4) | nothing; the ranges are derived, not recorded |
| parameter, local variable | `DiLocalVariable` (`name`, `arg`, `scope`, `file`, `line`, `type`, `flags`, `align`); the record's `DIExpression` | `vars` (variable, expression, declared-at, `VarPlace`) | `VarRow` has no expression field yet (`crates/hod_cg/src/record.rs:139`) although `hod_ir` has `SlotVar.expr` now; `#dbg_value` variables are dropped by policy v1 |
| label | `DiLabel` | `labels` | `Chunk` has no `labels` table yet (`crates/hod_cg/src/record.rs:225`) although `hod_ir` has `#dbg_label` now |
| type graph | `DiBasicType`, `DiDerivedType` (pointers, references, typedefs, cv, members, inheritance, pointers to member, friends), `DiCompositeType` (classes, structs, unions, enums, arrays with `DiSubrange`s), `DiSubroutineType`, `DiEnumerator` | nothing | nothing for C++ at `-O0`; Fortran-style bounds and annotations are `unsupported` |
| global variable | `DiGlobalVariableExpression` (`var`, `expr`) via `Module::global_exprs()`; `DiGlobalVariable` | the variable's symbol (address, TLS) | nothing |
| namespace | `DiNamespace` (`name`, `scope`, `exportSymbols`) | nothing | nothing |
| imported entity | `DiImportedEntity` (`using`, `using namespace`) | nothing | nothing |
| template parameters | `DiTemplateTypeParameter`, `DiTemplateValueParameter` (value a `DiConstant`, a symbol or a `DiTuple` for packs) | nothing | nothing |

What `hod_ir` lacks is an index, not data: a DIE tree is written parent
first, while `hod_ir` nodes name their parent (`scope`). The DIE writer
inverts the scope links once per unit (one pass over the nodes, a
counting sort by parent, §6.2).

## 3. Interface (implemented)

```rust
pub struct LineTarget { .. }              // §4.2; AARCH64, X86_64, for_arch(arch)
pub struct FileEntry<'a> { pub name: &'a [u8], pub dir: u32, pub md5: Option<[u8; 16]> }
pub struct LineHeader<'a> { pub dirs: &'a [&'a [u8]], pub files: &'a [FileEntry<'a>] }
pub struct Row { pub offset: u32, pub file: u32, pub line: u32, pub column: u16, pub flags: u8 }
pub mod flags { IS_STMT, PROLOGUE_END, EPILOGUE_BEGIN }
pub struct Sequence<'a> { pub order: u32, pub symbol: SymbolId, pub addend: i64,
                          pub size: u32, pub rows: &'a [Row] }
pub struct LineSections { pub line: SectionId, pub line_str: SectionId }
pub fn write_debug_line(obj: &mut Object, header: &LineHeader<'_>, seqs: &[Sequence<'_>])
    -> Result<LineSections, Error>;

// the policy (§5)
pub struct SourceLoc { pub file: u32, pub line: u32, pub column: u16 }
pub struct Transition { pub offset: u32, pub loc: Option<SourceLoc> }
pub struct FunctionLines<'a> { pub scope: SourceLoc, pub size: u32,
                               pub transitions: &'a [Transition], pub epilogue: bool }
pub fn function_rows(f: &FunctionLines<'_>, out: &mut Vec<Row>) -> Result<(), Error>;

// the file table (§4.4)
pub struct FileTable<'a> { .. }   // new(comp_dir, primary), add(directory, filename, md5)
```

`write_debug_line` writes one line-table unit into new `.debug_line` and
`.debug_line_str` sections with their relocations, and returns them. The
header is always written, even without sequences (a unit with no
functions still has a line table, as in `llc`). A sequence's rows are
relative to `symbol + addend`; a caller passes the section symbol of the
function's code section and the function's offset in it, as `llc` does.

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
**start of a block** `llc` writes a line-0 row instead (so that code is
not attributed to the line of an unrelated block that happened to be laid
out before it). hod cannot do that yet: `hod_cg`'s transitions do not say
which instruction starts a block (`crates/hod_cg/src/record.rs:119`).
With clang and kiln output at `-O0` every instruction outside the entry
code has a location, so the difference has not been seen; the follow-up
is a block-start bit on `LineRow` and rule 2 becomes "line 0 at a block
start, inherit elsewhere".

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

### 5.7 The bridge (planned, §10 row 1)

In `emit_module`, after the merge has placed the functions: for each
compile unit, a `FileTable` from its `DiFile` nodes in `DiId` order; for
each function with a subprogram of that unit, `function_rows` into one
`Vec<Row>` reused across functions, then a `Sequence`. A function without
a subprogram but with locations (possible in LLVM) gets its rows in the
unit of the locations' subprogram. A function without debug information
has no sequence.

## 6. DIEs (planned)

### 6.1 Abbreviations: a fixed table

hod writes `.debug_abbrev` from a table built at compile time of hod: one
abbreviation per (tag, attribute set, form choice) that the DIE writer
can produce, numbered once. Reasons: no hashing or interning per DIE, the
abbreviation codes are known before any DIE is written (so per-function
DIE runs can be written in parallel and concatenated), and the table is
the same for every object (a few hundred bytes to a few kilobytes,
against `llc`'s computed table of 0.2 to 2 KB). Forms are fixed per
attribute: strings `DW_FORM_strx` (a ULEB128 index: variable-length, yet
one form per abbreviation); addresses
`DW_FORM_addrx`; references `DW_FORM_ref4`; line and file numbers
`DW_FORM_udata`; sizes `DW_FORM_udata`; flags `DW_FORM_flag_present`
(an absent flag is a different abbreviation). `llc` chooses the smallest
fixed form (`data1`, `strx1`) per value, which makes its abbreviation set
depend on the values; hod's ULEB forms are as small for small values and
need no choice. The cost is a few bytes per DIE against `llc`'s table,
measured by the DIE package.

### 6.2 Writing straight from the tables

No DIE tree is built. Per unit: one pass over the nodes counting children
per parent scope (namespaces, composite types, subprograms, blocks) and a
prefix sum giving each parent's run of children; then a depth-first walk
that writes each DIE from its node's record directly into the output
buffer. References (`DW_FORM_ref4`) to a DIE not written yet are patched
when it is: a table `DiId → offset` (4 bytes per node) and a list of
patches. Per-function DIEs (the subprogram's children: parameters,
variables, blocks, labels) come from the function's `vars` and `lines`
and can be written by the worker that compiled the function into a
buffer of its own, with references to unit-level DIEs (types) as patches
resolved at the merge.

### 6.3 Strings

`.debug_str` is pooled per object with one hash table keyed by bytes;
`.debug_str_offsets` gets one entry per distinct string a unit names, in
order of first use by the walk (deterministic, because the walk order is).
Names come from `hod_ir` already interned, so a pool keyed by `hod_ir`'s
string id avoids hashing the bytes again (planned optimization).

### 6.4 Lexical blocks

A block's ranges are derived from the function's transitions: each
transition's location names a scope; the intervals between transitions
are attributed to every block on that scope's chain. A block with one
interval gets `DW_AT_low_pc`/`DW_AT_high_pc`, one with several a range
list in `.debug_rnglists`, as `llc` does.

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

Part 1 (this WP) encodes all sequences in one call; the per-chunk API
(`encode_sequence` into a caller's buffer, and `write_debug_line` taking
encoded runs) is row 1 of §10.

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

### 8.4 L7 (planned)

`xtask` sessions over kiln's debugger tests on hod's objects, gdb and
lldb; `llvm-dwarfdump --verify` over the corpus; the line tables of hod
and `llc` compared on the set of `(function, line)` breakpoint locations
(the addresses at which gdb would stop for each line: the first
`is_stmt` row of each line in each function).

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

## 9. The bridge in `hod_cg` (planned)

`emit_module` owns the mapping from `hod_ir` and `FuncRecord` to
`hod_dwarf`'s rows (§5.7, §6). It reads the subprogram of each function
(`FunctionDesc`), the location table (`DebugInfo::location`), and the
records. `hod_dwarf` never sees a `LocId` or a `DiId`.

## 10. Follow-on work packages

Rows for `docs/BACKLOG.md` (ids to be assigned; "this" is WP-052):

| title | scope | dependencies |
|---|---|---|
| line tables in `emit_module` | §5.7 and §9 in `hod_cg::emit`: `FileTable` from `DiFile` nodes, `LocId` → `SourceLoc`, `function_rows`, one `Sequence` per function; `encode_sequence` into the chunk and the merge of encoded runs (§7.1); `LineRow` gains a block-start bit (§5.3); `DW_LNCT_LLVM_source` for `DiFile.source`; gdb/lldb sessions on hod-compiled programs (§8.3 without `llc`); 1-vs-N-thread identity | this, WP-032 |
| DIE writer core | §6.1 to §6.3: the fixed abbreviation table, `.debug_info` unit header, compile unit DIE, `.debug_str`/`.debug_str_offsets`/`.debug_addr`/`.debug_rnglists`, the scope inversion, reference patching; `--verify` clean and compared with `llc` on modules with globals only | this |
| subprograms and variables | subprogram DIEs (`low_pc`/`high_pc`, `frame_base`), parameters and locals from `vars` with `DIExpression` composed onto the frame-base offset (`VarRow` gains its expression), lexical blocks from `lines` (§6.4), labels (the `labels` table); gdb `info locals`/`print`/`bt` on hod-compiled programs | DIE writer core, line tables in `emit_module` |
| the type graph | basic, derived (pointer, reference, rvalue reference, typedef, const/volatile, member, inheritance, pointer to member, friend), composite (class, struct, union, enum with enumerators, array with subranges), subroutine types, declarations and completion, `vtableHolder`; compared with `llc` by `llvm-dwarfdump` structure; gdb `ptype` | DIE writer core |
| globals, namespaces, templates | global and static-member variables (`DW_OP_addrx`, TLS with `DW_OP_form_tls_address`), namespaces, imported entities, template type and value parameters (packs), `DW_AT_specification` for out-of-line definitions | subprograms and variables, the type graph |
| L7 harness | §8.4: `xtask` gdb and lldb sessions, kiln's debugger tests, corpus `--verify`, `(function, line)` comparison with `llc`; the two-object COMDAT case | line tables in `emit_module`, subprograms and variables |
| line-tables-only and compressed sections | `emissionKind: LineTablesOnly` (CU and subprogram DIEs only), `SHF_COMPRESSED` with zlib written in safe Rust (no dependency) | DIE writer core |
| split DWARF | `-gsplit-dwarf`: skeleton unit, `.dwo` sections, `DW_UT_skeleton`/`split_compile`; `hod_ir` must first map `splitDebugFilename` and `dwoId` | DIE writer core |

## 11. Open questions

1. `.debug_names` for units with `nameTableKind: Default`: gdb and lldb
   build their own index; a name index speeds up their start on large
   programs. Write it when a measurement says the start-up time matters.
2. `minimum_instruction_length` 4 on AArch64 (§4.2): smaller tables
   (fewer `const_add_pc`/`advance_pc`), but every table then differs from
   `llc`'s byte for byte. Measure on the corpus with the bridge.
3. Discriminators (`DILexicalBlockFile.discriminator`): only optimized
   code and sample profiling use them; `hod_ir` keeps them. Not written.
4. The line-0 epilogue row of a function with several `ret`s (§5.4):
   whether gdb's and lldb's `next` stop there. To be checked by the line
   package's debugger tests on a hand-written module with two `ret`s.
5. The block-start rule of §5.3 needs `hod_cg` to mark block starts;
   until then hod inherits where `llc` writes line 0.
6. DWARF 4: a module flag `"Dwarf Version", i32 4` (one kiln corpus
   module, `-gdwarf-4`) makes `llc` write version 4 tables with
   `include_directories` strings inline and no `.debug_line_str`. hod
   writes DWARF 5 only and ignores the flag, which every current gdb and
   lldb read. Write version 4 only if a consumer that needs it shows up
   (`hod_ir` maps the flag as `ModuleProps::dwarf_version`,
   `crates/hod_ir/src/props.rs:140`, so the bridge can refuse or warn).
