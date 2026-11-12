# `hod_obj`: object model and ELF64 relocatable writer

`hod_obj` holds one relocatable object file in memory and serializes it. It
knows nothing about IR or code generation. Its consumer is the code
generator, which adds thousands of functions per object, so every call on
the building path is a few stores into flat tables.

The crate has two parts:

- the **model** (`Object` and the types around it): sections, symbols,
  relocations, section groups and names, addressed by `u32` ids;
- the **ELF writer** (`hod_obj::elf`): an ELF64 little-endian relocatable
  file (`ET_REL`) for AArch64 (`EM_AARCH64`) or x86-64 (`EM_X86_64`).

External formats are cited as: **gABI**, the System V ABI generic ELF
specification, by chapter title; **AArch64 ELF**, "ELF for the Arm 64-bit
Architecture (AArch64)"; **x86-64 psABI**, "System V ABI, AMD64 Architecture
Processor Supplement".

The model has no ELF layout in it and is meant to serve other container
formats later. Two things in it are ELF-shaped on purpose: relocation kinds
are the psABI relocation types, because that is what the code generator
selects, and `SectionKind::Elf` is a raw escape. A writer for another format
maps or rejects them.

## Model

### Ids

`SectionId`, `SymbolId` and `GroupId` are `u32` newtypes. Each table numbers
its entries from zero in the order they are added, so the n-th section added
has `index() == n`; `from_index` builds an id from a position. `StrId` names
a stored symbol name; `StrId::EMPTY` is the empty name.

An id is valid only for the object that issued it. A method that takes an id
as an argument **panics** when the id is out of range for the object; this
is a caller bug, not an input error. Ids stored in data (`Symbol::name`,
`Symbol::place`) are checked by the writer and reported as errors.

### `Object`

```rust
Object::new(arch: Arch) -> Object            // Arch::AArch64 | Arch::X86_64
fn arch(&self) -> Arch
fn set_header_flags(&mut self, flags: u32)   // e_flags; default 0
fn set_stack_note(&mut self, note: StackNote)
fn reserve(&mut self, sections: usize, symbols: usize, relocations: usize, string_bytes: usize)
```

`reserve` only pre-sizes tables; it never changes behavior.

### Strings

```rust
fn add_string(&mut self, name: &[u8]) -> Result<StrId, Error>
fn string(&self, id: StrId) -> &[u8]
```

`add_string` stores a symbol name and returns its id. Names are arbitrary
bytes without NUL. **Equal names are not merged**: each call stores the
bytes again. A caller that uses one name for many symbols (AArch64 mapping
symbols `$x` and `$d`, for example) adds it once and reuses the `StrId`. The
empty name returns `StrId::EMPTY` and stores nothing. Errors: `NulInName`;
`TooLarge(StringBytes)` when the names would pass 4 GiB.

`string` returns the bytes of a stored name. It never panics: for an id
the object did not issue the result is unspecified bytes of the name arena
(empty when the id is past its end).

### Sections

```rust
fn add_section(&mut self, name: &[u8], kind: SectionKind, align: u64) -> Result<SectionId, Error>
fn section_count(&self) -> u32
fn section_name(&self, section: SectionId) -> &[u8]
fn section_kind(&self, section: SectionId) -> SectionKind
fn section_align(&self, section: SectionId) -> u64
fn raise_section_align(&mut self, section: SectionId, align: u64)
fn section_size(&self, section: SectionId) -> u64
fn section_group(&self, section: SectionId) -> Option<GroupId>
```

A section has a name, a kind, an alignment (a power of two, in bytes) and
contents. **Several sections may have the same name**; they stay separate
sections (`-ffunction-sections` with COMDAT produces many sections named
`.text.<symbol>`, and groups may repeat a name). `add_section` errors:
`NulInName`; `BadAlignment` when `align` is not a power of two (zero is
not); `BadEntrySize { section: None }` for a mergeable kind with a zero
entry size; `ReservedSectionType` and `ReservedSectionFlags` (see
`SectionKind::Elf`); `TooLarge`. A rejected call adds nothing.

`raise_section_align` sets the alignment to the larger of the current one
and `align`. An `align` that is not a power of two changes nothing and is
reported by the writer as `BadAlignment` (see "Deferred errors").

#### Section kinds

| `SectionKind` | meaning | `sh_type` | `sh_flags` | `sh_entsize` |
|---|---|---|---|---|
| `Text` | code | `SHT_PROGBITS` | `ALLOC`, `EXECINSTR` | 0 |
| `Data` | writable data | `SHT_PROGBITS` | `ALLOC`, `WRITE` | 0 |
| `ReadOnlyData` | read-only data | `SHT_PROGBITS` | `ALLOC` | 0 |
| `Bss` | zero-initialized data | `SHT_NOBITS` | `ALLOC`, `WRITE` | 0 |
| `Tls` | initialized thread-local data | `SHT_PROGBITS` | `ALLOC`, `WRITE`, `TLS` | 0 |
| `TlsBss` | zero-initialized thread-local data | `SHT_NOBITS` | `ALLOC`, `WRITE`, `TLS` | 0 |
| `InitArray` | pointers to initializers | `SHT_INIT_ARRAY` | `ALLOC`, `WRITE` | 0 |
| `FiniArray` | pointers to finalizers | `SHT_FINI_ARRAY` | `ALLOC`, `WRITE` | 0 |
| `Note` | a loaded note | `SHT_NOTE` | `ALLOC` | 0 |
| `Debug` | debug information, not loaded | `SHT_PROGBITS` | none | 0 |
| `MergeableStrings { char_size }` | read-only NUL-terminated strings | `SHT_PROGBITS` | `ALLOC`, `MERGE`, `STRINGS` | `char_size` |
| `MergeableConstants { entry_size }` | read-only fixed-size constants | `SHT_PROGBITS` | `ALLOC`, `MERGE` | `entry_size` |
| `Elf { sh_type, sh_flags, sh_entsize }` | anything else | as given | as given | as given |

The types and flags are those of gABI "Sections". A section in a group also
gets `SHF_GROUP`. `sh_link` and `sh_info` of a caller's section are zero.

`SectionKind::Elf` is the escape hatch (a mergeable `.debug_str`, a section
with `SHF_GNU_RETAIN`, `SHT_X86_64_UNWIND`, ...). The constants are public
in `hod_obj::elf` (`SHT_*`, `SHF_*`). It may not claim what the writer
generates or what needs a link the model cannot express: `sh_type` of
`SHT_NULL`, `SHT_SYMTAB`, `SHT_RELA`, `SHT_REL`, `SHT_HASH`, `SHT_DYNAMIC`,
`SHT_DYNSYM`, `SHT_GROUP` or `SHT_SYMTAB_SHNDX` is `ReservedSectionType`;
`SHF_GROUP`, `SHF_INFO_LINK` or `SHF_LINK_ORDER` in `sh_flags` is
`ReservedSectionFlags` (carrying the offending bits).

`SectionKind::is_nobits()` is true for `Bss`, `TlsBss` and `Elf` with
`SHT_NOBITS`: the section occupies no file space and has a size only.

#### Section contents

```rust
fn append(&mut self, section: SectionId, bytes: &[u8]) -> u64
fn append_owned(&mut self, section: SectionId, bytes: Vec<u8>) -> u64
fn append_zeros(&mut self, section: SectionId, len: u64) -> u64
fn align_to(&mut self, section: SectionId, align: u64, fill: u8) -> u64
fn reserve_data(&mut self, section: SectionId, additional: usize)
fn data(&self, section: SectionId) -> &[u8]
fn data_mut(&mut self, section: SectionId) -> &mut Vec<u8>
```

- `append` copies bytes to the end of the section and returns the offset
  they start at.
- `append_owned` does the same with an owned buffer. When the section is
  empty it **takes the buffer over without copying** (the section's bytes
  are then at the buffer's address).
- `append_zeros` appends `len` zero bytes and returns the offset they start
  at. For a section without file space it only grows the size.
- `align_to` pads the section with `fill` bytes to a multiple of `align`,
  raises the section's alignment to at least `align`, and returns the new
  size. For a section without file space only the size changes. An `align`
  that is not a power of two changes nothing, returns the current size, and
  is reported by the writer as `BadAlignment`.
- `data_mut` gives the section's buffer, so that a producer can build
  contents in place (no intermediate buffer) or patch bytes already there.
- `section_size` is the buffer's length, or the tracked size of a section
  without file space.

Bytes stored in a section without file space (through `append`,
`append_owned` or `data_mut`) are an error the writer reports:
`DataInNobits`.

### Symbols

```rust
pub struct Symbol {
    pub name: StrId,
    pub value: u64,
    pub size: u64,
    pub place: SymbolPlace,     // Undefined | Absolute | Common | Section(SectionId)
    pub binding: Binding,       // Local | Global | Weak
    pub kind: SymbolKind,       // NoType | Func | Object | Section | File | Tls | GnuIfunc
    pub visibility: Visibility, // Default | Hidden | Protected
    pub other: u8,
}
Symbol::undefined(name: StrId) -> Symbol
Symbol::defined(name: StrId, kind: SymbolKind, section: SectionId, value: u64, size: u64) -> Symbol
fn with_binding(self, binding: Binding) -> Symbol
fn with_visibility(self, visibility: Visibility) -> Symbol

fn add_symbol(&mut self, symbol: Symbol) -> SymbolId
fn symbol_count(&self) -> u32
fn symbol(&self, symbol: SymbolId) -> &Symbol
fn symbol_mut(&mut self, symbol: SymbolId) -> &mut Symbol
fn section_symbol(&mut self, section: SectionId) -> SymbolId
fn add_file_symbol(&mut self, name: &[u8]) -> Result<SymbolId, Error>
fn retain_symbol(&mut self, symbol: SymbolId)
fn is_referenced(&self, symbol: SymbolId) -> bool
```

A `Symbol` is plain data, 32 bytes. `value` is the offset in the section for
`Section`, the value for `Absolute`, and the alignment for `Common` (whose
`size` is the block's size). `other` holds processor-specific `st_other`
bits, such as `elf::STO_AARCH64_VARIANT_PCS` (AArch64 ELF "Symbol Table");
its low two bits, the visibility field, must be zero.

`Symbol::undefined` is global, default visibility, `NoType`.
`Symbol::defined` is global, default visibility.

The intended pattern for the code generator is to add every symbol of the
module up front as undefined, which fixes the ids, and to define each one
through `symbol_mut` when its code or data is placed.

- `section_symbol` returns the symbol that stands for a section itself
  (`SymbolKind::Section`, local, unnamed, value 0). It is created on the
  first call for a section and the same id is returned afterwards.
- `add_file_symbol` adds the `SymbolKind::File` symbol: local, absolute,
  named after the source file.
- **A symbol is referenced** when a relocation names it, when it is a
  group's signature, or after `retain_symbol`. The writer leaves out an
  undefined symbol that is not referenced, so declarations that are never
  used cost nothing in the output and create no link-time dependency. Every
  other symbol is written.

Constraints the writer checks on every symbol it writes (`BadSymbol` with a
`SymbolProblem`): `name` is a string of this object (`UnknownName`); a
`Section` place names an existing section (`UnknownSection`); an undefined
symbol is not local (`UndefinedLocal`); a `Section` symbol is local and
placed in a section (`SectionSymbolShape`); a `File` symbol is local and
absolute (`FileSymbolShape`); `other & 3 == 0` (`OtherOverlapsVisibility`).
Nothing else is checked: a symbol's value may lie outside its section.

### Relocations

```rust
fn add_relocation(&mut self, section: SectionId, offset: u64,
                  kind: impl Into<RelocKind>, symbol: SymbolId, addend: i64)
fn relocation_count(&self, section: SectionId) -> u32
fn relocations(&self, section: SectionId) -> impl Iterator<Item = Relocation>
pub struct Relocation { pub offset: u64, pub kind: RelocKind, pub symbol: SymbolId, pub addend: i64 }
```

A relocation belongs to a section and says: patch the place at `offset` in
this section with a value computed, as `kind` says, from `symbol` and
`addend`. Relocations always carry an explicit addend (RELA); the bytes at
the place are whatever the caller appended, and should be the instruction or
datum with a zero field.

Relocations of different sections may be added in any interleaving, and
within a section in any order. `relocations` yields a section's relocations
in the order they were added. Adding a relocation marks its symbol as
referenced.

`RelocKind` is `A64(A64Reloc)` or `X64(X64Reloc)`, with `From` for both, so
a caller passes `A64Reloc::Call26` directly. A kind whose architecture is
not the object's is not added and is reported by the writer as
`RelocationArchMismatch`.

Each kind enum has one variant per supported relocation and `Other(u32)`
for any other type number, and:

```rust
const NAMED: &[Self]                          // every named variant
const fn elf_type(self) -> u32                // the ELF relocation type number
const fn from_elf_type(number: u32) -> Self   // the named variant if any, else Other
const fn elf_name(self) -> Option<&'static str>   // "R_AARCH64_CALL26"; None for Other
```

`RelocKind` has `arch`, `elf_type`, `elf_name` and
`from_elf_type(arch, number)`. `Other(n)` with the number of a named variant
is written identically to that variant but does not compare equal to it;
`from_elf_type` gives the canonical form.

`A64Reloc` (AArch64 ELF "Relocation types"; the variant is the name after
`R_AARCH64_` in camel case):

| group | variants (ELF number) |
|---|---|
| data | `None` 0, `Abs64` 257, `Abs32` 258, `Abs16` 259, `Prel64` 260, `Prel32` 261, `Prel16` 262, `Plt32` 314, `GotPcrel32` 315 |
| `MOVZ`/`MOVK` | `MovwUabsG0` 263, `MovwUabsG0Nc` 264, `MovwUabsG1` 265, `MovwUabsG1Nc` 266, `MovwUabsG2` 267, `MovwUabsG2Nc` 268, `MovwUabsG3` 269 |
| PC-relative | `LdPrelLo19` 273, `AdrPrelLo21` 274, `AdrPrelPgHi21` 275, `AdrPrelPgHi21Nc` 276, `AddAbsLo12Nc` 277, `Ldst8AbsLo12Nc` 278, `Ldst16AbsLo12Nc` 284, `Ldst32AbsLo12Nc` 285, `Ldst64AbsLo12Nc` 286, `Ldst128AbsLo12Nc` 299 |
| branches | `TstBr14` 279, `CondBr19` 280, `Jump26` 282, `Call26` 283 |
| GOT | `AdrGotPage` 311, `Ld64GotLo12Nc` 312 |
| TLS general dynamic | `TlsgdAdrPage21` 513, `TlsgdAddLo12Nc` 514 |
| TLS initial exec | `TlsieMovwGottprelG1` 539, `TlsieMovwGottprelG0Nc` 540, `TlsieAdrGottprelPage21` 541, `TlsieLd64GottprelLo12Nc` 542, `TlsieLdGottprelPrel19` 543 |
| TLS local exec | `TlsleMovwTprelG2` 544, `TlsleMovwTprelG1` 545, `TlsleMovwTprelG1Nc` 546, `TlsleMovwTprelG0` 547, `TlsleMovwTprelG0Nc` 548, `TlsleAddTprelHi12` 549, `TlsleAddTprelLo12` 550, `TlsleAddTprelLo12Nc` 551, `TlsleLdst{8,16,32,64}TprelLo12` 552, 554, 556, 558 and their `Nc` forms 553, 555, 557, 559, `TlsleLdst128TprelLo12` 570, `TlsleLdst128TprelLo12Nc` 571 |
| TLS descriptors | `TlsdescLdPrel19` 560, `TlsdescAdrPrel21` 561, `TlsdescAdrPage21` 562, `TlsdescLd64Lo12` 563, `TlsdescAddLo12` 564, `TlsdescOffG1` 565, `TlsdescOffG0Nc` 566, `TlsdescLdr` 567, `TlsdescAdd` 568, `TlsdescCall` 569 |

`X64Reloc` (x86-64 psABI "Relocation Types"):

| variants (ELF name after `R_X86_64_`, number) |
|---|
| `None` (`NONE`, 0), `Abs64` (`64`, 1), `Pc32` (`PC32`, 2), `Plt32` (`PLT32`, 4), `GotPcrel` (`GOTPCREL`, 9), `Abs32` (`32`, 10), `Abs32S` (`32S`, 11), `Abs16` (`16`, 12), `Pc16` (`PC16`, 13), `Abs8` (`8`, 14), `Pc8` (`PC8`, 15), `DtpOff64` (`DTPOFF64`, 17), `TpOff64` (`TPOFF64`, 18), `TlsGd` (`TLSGD`, 19), `TlsLd` (`TLSLD`, 20), `DtpOff32` (`DTPOFF32`, 21), `GotTpOff` (`GOTTPOFF`, 22), `TpOff32` (`TPOFF32`, 23), `Pc64` (`PC64`, 24), `GotOff64` (`GOTOFF64`, 25), `GotPc32` (`GOTPC32`, 26), `GotPc32Tlsdesc` (`GOTPC32_TLSDESC`, 34), `TlsdescCall` (`TLSDESC_CALL`, 35), `GotPcrelX` (`GOTPCRELX`, 41), `RexGotPcrelX` (`REX_GOTPCRELX`, 42) |

The writer does not interpret kinds: it does not know a relocation's width
and does not check that the place lies inside the section, beyond
`RelocationOutOfBounds` when an offset is greater than the section's size.

### Section groups

```rust
fn add_group(&mut self, signature: SymbolId, kind: GroupKind) -> GroupId   // Comdat | Plain
fn group_count(&self) -> u32
fn add_group_member(&mut self, group: GroupId, section: SectionId) -> Result<(), Error>
fn group_signature(&self, group: GroupId) -> SymbolId
fn group_members(&self, group: GroupId) -> impl Iterator<Item = SectionId>
```

A group is a set of sections the linker keeps or discards together (gABI
"Section Groups"). Of all `Comdat` groups with the same signature symbol
name across the link, the linker keeps one. The signature is a symbol of
the object, usually the one the group defines; `add_group` marks it
referenced. `add_group_member` adds a section; a section is in at most one
group (`SectionAlreadyInGroup`, which changes nothing). A section's
relocations belong to its group with it. `group_members` yields members in
the order added. An empty group is allowed.

### Stack marker

`StackNote::NonExecutable` (the default) makes the writer emit an empty
`.note.GNU-stack` section, which tells GNU-compatible linkers the object
does not need an executable stack. `Executable` emits it with
`SHF_EXECINSTR`. `Omit` emits none, and the linker's default applies.

### Deferred errors

Calls on the hot path return ids or offsets, not `Result`. When such a call
cannot do what was asked, it records the error in the object, has no other
effect, and the writer returns the **first** recorded error before writing
anything. This applies to: `add_relocation` with a kind of another
architecture; `align_to` and `raise_section_align` with a bad alignment;
`add_symbol`, `add_relocation` and `add_group` past the 32-bit index space
(`TooLarge`).

## ELF writer

```rust
hod_obj::elf::write<W: io::Write>(obj: &Object, out: &mut W) -> Result<u64, WriteError>
hod_obj::elf::to_vec(obj: &Object) -> Result<Vec<u8>, Error>
hod_obj::elf::file_size(obj: &Object) -> Result<u64, Error>
```

All three first check the object and compute the complete layout; they
fail with the same `Error` for the same object, before producing any
output. `write` then emits the file **front to back** with `write_all`,
never seeking, and returns the number of bytes written, which equals
`file_size`. It does not flush `out`. `to_vec` returns the same bytes in a
vector allocated once at the final size. `WriteError` is `Invalid(Error)`
(nothing was written) or `Io(io::Error)` (the output is incomplete).

`write` batches small pieces in a 64 KiB buffer, so an unbuffered `File` is
a fine sink; a piece of 64 KiB or more goes to the sink directly.

Writing does not change the object; it may be written again or extended and
written again.

### File header

gABI "ELF Header". `e_ident`: `ELFCLASS64`, `ELFDATA2LSB`, `EV_CURRENT`;
`EI_OSABI` is `ELFOSABI_NONE`, or `ELFOSABI_GNU` when a written symbol is
`GnuIfunc` or a section has `SHF_GNU_RETAIN`; `EI_ABIVERSION` 0. `e_type`
`ET_REL`; `e_machine` `EM_AARCH64` (183) or `EM_X86_64` (62); `e_version` 1;
`e_entry`, `e_phoff`, `e_phentsize`, `e_phnum` 0; `e_flags` as set
(default 0); `e_ehsize` 64; `e_shentsize` 64; `e_shoff`, `e_shnum`,
`e_shstrndx` per the layout below and "Large section counts".

### Section header order

1. the null section;
2. one `.group` section per group, in `GroupId` order;
3. the caller's sections, in `SectionId` order;
4. `.note.GNU-stack`, unless omitted;
5. one `.rela<name>` section per caller's section that has relocations, in
   `SectionId` order;
6. `.symtab`;
7. `.symtab_shndx`, only when needed;
8. `.strtab`;
9. `.shstrtab`.

So the ELF index of `SectionId(n)` is `1 + group_count + n`, and every
group's header precedes its members' (gABI "Section Groups" requires it).

### File layout

In file order: the file header; the caller's sections' contents in
`SectionId` order; the group sections; the relocation sections; `.symtab`;
`.symtab_shndx`; `.strtab`; `.shstrtab`; the section header table, which
ends the file.

- The contents of a section that occupies file space start at a file
  offset that is a multiple of its alignment, padded with zeros, whether or
  not the section is empty.
- A section without file space takes no bytes and **no padding**; its
  `sh_offset` is the offset the next contents would have, not aligned.
- Group sections start at a multiple of 4; relocation sections, `.symtab`
  and the section header table at a multiple of 8.
- `sh_addr` is 0 everywhere.

An alignment so large that the file would pass 2^64 bytes is
`TooLarge(FileBytes)`. Below that the padding is written as asked: a
section aligned to 1 GiB makes a file of at least 1 GiB.

### Section names

`.shstrtab` (`SHT_STRTAB`, no flags, alignment 1) holds: a NUL; `.symtab`,
`.strtab`, `.shstrtab`; then `.symtab_shndx`, `.group` and
`.note.GNU-stack`, each only when such a section is written; then each
**distinct** caller's section name once, in the order first used. When any
section of a name has relocations the name is stored as `.rela<name>`: the
relocation section's `sh_name` points at it and the section's `sh_name`
five bytes further. Sections with equal names have equal `sh_name`.

### Symbol table

`.symtab` (`SHT_SYMTAB`, `sh_entsize` 24, alignment 8, `sh_link` =
`.strtab`), gABI "Symbol Table". Entries, in order:

1. the null symbol;
2. local `File` symbols, in `SymbolId` order (gABI: a file symbol precedes
   the other local symbols);
3. the other local symbols, in `SymbolId` order;
4. global and weak symbols, in `SymbolId` order.

`sh_info` is the index of the first entry of part 4 (one past the last
local). Unreferenced undefined symbols are not written (see "Symbols").
Relocations and group signatures use the entries' new indices.

Fields: `st_name` is the name's offset in `.strtab`; `st_info` is
`binding << 4 | type` with bindings `STB_LOCAL` 0, `STB_GLOBAL` 1,
`STB_WEAK` 2 and types `STT_NOTYPE` 0, `STT_OBJECT` 1, `STT_FUNC` 2,
`STT_SECTION` 3, `STT_FILE` 4, `STT_TLS` 6, `STT_GNU_IFUNC` 10; `st_other`
is `other | visibility` with `STV_DEFAULT` 0, `STV_HIDDEN` 2,
`STV_PROTECTED` 3; `st_shndx` is `SHN_UNDEF`, `SHN_ABS`, `SHN_COMMON`, or
the section's ELF index; `st_value` and `st_size` as given.

`.strtab` (`SHT_STRTAB`, no flags, alignment 1) is a NUL followed by every
name passed to `add_string`, in call order, each NUL-terminated, including
names only omitted symbols use. `st_name` of the empty name is 0.

### Relocation sections

For each caller's section with at least one relocation: a section named
`.rela<name>` of type `SHT_RELA`, flags `SHF_INFO_LINK` (plus `SHF_GROUP`
when the section is in a group), `sh_link` = `.symtab`, `sh_info` = the
section's ELF index, `sh_entsize` 24, alignment 8 (gABI "Relocation").
Entries are `Elf64_Rela`: `r_offset`; `r_info` = symbol table index `<< 32`
| type number; `r_addend`.

Entries are written in **non-decreasing `r_offset` order**, entries with
equal offsets in the order they were added, whatever order they were added
in. (Linkers read `.rela.eh_frame` assuming this order, and the x86-64
psABI TLS sequences need their relocation pairs adjacent.) A section whose
relocations were added in order is written without sorting.

### Section groups

For each group: a section named `.group` of type `SHT_GROUP`, no flags,
`sh_link` = `.symtab`, `sh_info` = the signature's symbol table index,
`sh_entsize` 4, alignment 4. Its contents are 32-bit words: `GRP_COMDAT`
(1) for `Comdat` or 0 for `Plain`; then, for each member in the order
added, the member's ELF index followed by its relocation section's ELF
index if it has one. Members and their relocation sections have
`SHF_GROUP`.

### Large section counts

`-ffunction-sections` on a large C++ translation unit passes 65 280
(`SHN_LORESERVE`, 0xff00) sections, where the 16-bit fields run out. The
writer uses the escapes of gABI "ELF Header" and "Symbol Table":

- section count ≥ 0xff00: `e_shnum` is 0 and the count is in `sh_size` of
  section header 0;
- `.shstrtab` index ≥ 0xff00: `e_shstrndx` is `SHN_XINDEX` (0xffff) and
  the index is in `sh_link` of section header 0;
- a written symbol in a section whose ELF index is ≥ 0xff00: its
  `st_shndx` is `SHN_XINDEX`, and the object has a `.symtab_shndx` section
  (`SHT_SYMTAB_SHNDX`, `sh_link` = `.symtab`, `sh_entsize` 4, alignment 4)
  with one 32-bit word per `.symtab` entry: the section index for those
  symbols, 0 for all others.

Each escape is used exactly when needed; otherwise header 0 is all zeros
and there is no `.symtab_shndx`. `sh_link`, `sh_info` and group members are
32-bit and need no escape.

## Errors

`Error` is `Copy + Eq`, implements `std::error::Error`, and names the id
involved.

| error | raised by | when |
|---|---|---|
| `NulInName` | `add_string`, `add_section`, `add_file_symbol` | the name contains a NUL |
| `BadAlignment { align }` | `add_section`; writer (deferred from `align_to`, `raise_section_align`) | not a power of two |
| `BadEntrySize { section: None }` | `add_section` | mergeable kind with entry size 0 |
| `BadEntrySize { section: Some }` | writer | a `MergeableStrings`/`MergeableConstants` section's size is not a multiple of its entry size |
| `ReservedSectionType { sh_type }` | `add_section` | see "Section kinds" |
| `ReservedSectionFlags { sh_flags }` | `add_section` | see "Section kinds" |
| `SectionAlreadyInGroup { section }` | `add_group_member` | the section is in a group |
| `RelocationArchMismatch { section, kind }` | writer (deferred from `add_relocation`) | kind of another architecture |
| `DataInNobits { section }` | writer | bytes stored in a section without file space |
| `RelocationInNobits { section }` | writer | relocations in a section without file space |
| `RelocationOutOfBounds { section, offset }` | writer | the section's largest relocation offset is greater than its size |
| `BadSymbol { symbol, problem }` | writer | see "Symbols" |
| `TooLarge(Limit)` | `add_string`, `add_section`, writer | `StringBytes`, `SectionNameBytes`: more than 4 GiB of names; `Sections`, `Symbols`, `Relocations`, `Groups`: the 32-bit index space is exhausted; `FileBytes`: the file would pass 2^64 bytes or cannot be allocated by `to_vec` |

No call panics on any input other than an id that the object did not issue
(see "Ids"). Internal invariants (the layout's sizes against the bytes
emitted, a relocation chain against its count) are `debug_assert!`s whose
messages start with `layout:`, `model:` or `record:`.

## Determinism

The bytes written depend only on the sequence of calls made on the object
and their arguments: not on addresses, hashing order, time or the sink.
`reserve` and `reserve_data` do not affect them. `write` and `to_vec`
produce identical bytes.

## Performance

These are properties of the design, checked by `record_sizes` in
`src/model.rs` and by the tests named under "Testing"; measurements are in
`landed/object/WP-010.md`.

- **No allocation per symbol, relocation or string.** Symbols are 32-byte
  records in one vector, with one bit each in a side bitmap. Relocations
  are 32-byte records in **one vector for the whole object**; a section's
  relocations are an index chain through it, so sections need no vectors of
  their own and may be interleaved freely. Symbol names are appended to one
  byte arena that is already the image of `.strtab`.
- **Per section:** one record (about 100 bytes), and one heap buffer for
  its contents once it has any. Section names are interned in one arena
  with an open-addressing index (a fixed, non-random hash); adding a
  section hashes its name once.
- **Section contents are copied at most once by the writer**: `to_vec`
  copies them into the output vector; `write` copies a piece smaller than
  64 KiB into its buffer and hands larger pieces to the sink as they are.
  `append_owned` and `data_mut` let a producer avoid the copy into the
  model as well.
- **The writer's own memory** is: 8 bytes per section (content offsets) and
  4 bytes per section (relocation section indices); 4 bytes per symbol (new
  indices); 5 bytes per distinct section name; the 64 KiB buffer (`write`)
  or the output (`to_vec`); and, only for a section whose relocations were
  added out of order, 4 bytes per relocation of the largest such section.
  The symbol table, relocation entries, string tables and section headers
  are generated straight into the output, never materialized.
- The writer makes one pass over sections and two over symbols to lay out,
  then one pass over each table to emit (three over symbols, for the three
  parts of `.symtab`).

## Differences from LLVM's output

hod's objects do not have to equal `llc`'s byte for byte, only to mean the
same. Deliberate differences:

- Section headers are ordered groups, sections, relocation sections,
  tables; LLVM puts `.strtab` first and each relocation section after its
  section. This order makes the ELF index of a section a constant offset
  from its id.
- `.strtab` and `.shstrtab` are not tail-merged or deduplicated beyond what
  "Section names" and "Strings" say: suffix merging costs time and hod's
  job is to finish.
- Relocations are sorted by offset only; LLVM's order within equal offsets
  differs for some targets.
- Sections without file space are not padded to their alignment in the
  file.
- AArch64 mapping symbols (`$x`, `$d`) are not synthesized; the caller adds
  them as ordinary local symbols if it wants them. Linking and running do
  not need them.

## Deliberately unsupported

- ELF32, big-endian, executables and shared objects, `SHT_REL` relocation
  sections, architectures other than AArch64 and x86-64.
- `SHF_LINK_ORDER` and any caller's section that needs `sh_link` or
  `sh_info` (`SectionKind::Elf` rejects it).
- `STB_GNU_UNIQUE`, symbol versioning, `STT_COMMON`.
- Compressing sections: `SHF_COMPRESSED` may be set through
  `SectionKind::Elf`, with the caller providing the compressed bytes and
  their header.
- Removing or renaming anything once added, and merging two objects.
- Mach-O and COFF output.

## Testing

All in `crates/hod_obj`. `cargo test -p hod_obj` runs everything;
`HOD_REQUIRE_TOOLS=1` turns a missing external tool from a skip into a
failure.

| file | layer | covers |
|---|---|---|
| `src/model.rs`, `src/names.rs`, `src/reloc.rs` (unit tests) | L0 | "Model": ids, strings, contents, symbols, relocation chains, groups, deferred errors, record sizes, name interning, relocation numbering |
| `tests/common/mod.rs` | - | test support: an independent ELF64 reader that checks, for every object it parses, the header, the escapes, that contents are aligned and do not overlap, the symbol order against `sh_info`, every relocation section's link, info, flags, name and order, and every group's link, signature, members and flags |
| `tests/structure.rs` | L3 | "ELF writer", "Errors", "Determinism", through the reader: each section kind; shared names; symbol reordering and relocation remapping; omitted symbols; section and file symbols; sorted relocations; group contents; stack marker; `write` = `to_vec` through sinks of any granularity; every error; the three escapes exactly at 0xff00; an object with 70 000 sections |
| `tests/tools.rs` | L3 | the same objects read by `llvm-readelf` and binutils `readelf` (`--all -g -W`, no diagnostics allowed) with sections, symbols, relocations and groups compared; **every named relocation kind** written once and its name read back from both tools; the 70 000 section object; an x86-64 object through `llvm-readelf` and `llvm-objdump -d -r`; the AArch64 program through `llvm-objdump -d -r` |
| `tests/link.rs` | L3 | link and run, AArch64 Linux hosts only |
| `examples/throughput.rs` | - | a timing and memory report, not a gate: `cargo run --release -p hod_obj --example throughput [functions [relocations-per-function]]`, and with `HOD_OBJ_MODE=one-text` for a single `.text` |

### Link-and-run tests

Each program is hand-encoded AArch64 code, linked through the C compiler
driver with each of GNU ld, lld and mold, with linker warnings fatal, and
run:

- `main` returns 42;
- `main` calls `puts` on a string in `.rodata` (`ADR_PREL_PG_HI21`,
  `ADD_ABS_LO12_NC`, `CALL26`);
- two objects each define the COMDAT group `answer` (a function and the
  data it loads through `ADR_PREL_PG_HI21`/`LDST32_ABS_LO12_NC`, with
  different data in each object): the link succeeds, with the function weak
  and with it global, and the program returns the value of whichever object
  comes first; the control test shows the same two global definitions
  without a group fail to link;
- a pointer in `.data` initialized with an address (`ABS64` against a
  global symbol, and against a section symbol plus addend), read by `main`;
- a 4096-aligned variable in `.bss`, placed between sections with
  contents, is zero and aligned;
- the object with 70 000 function sections, where `main` and the function
  it jumps to are in sections whose indices need the escapes.

The linker is selected with `gcc -B<dir> -fuse-ld=<bfd|lld|mold>`, where
`<dir>` is a scratch directory holding one symbolic link named `ld.<name>`
to the linker `hod_testutil::tool` found. `gcc -fuse-ld=<name>` runs the
program `ld.<name>` from its program search path and `-B` puts a directory
at the front of that path, so this works wherever the linker is installed
and whatever it is called (`ld.lld` is `/usr/bin/ld.lld-22` or
`/usr/lib/llvm-22/bin/ld.lld` here, neither of which plain `-fuse-ld=lld`
finds). Each test then checks the program's `.comment` section to confirm
the intended linker produced it.
