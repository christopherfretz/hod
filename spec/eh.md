# spec: hod_eh — unwind tables

`hod_eh` writes an object's unwind tables from what the code generator
records per function (`spec/codegen.md` §9.3, §9.4): the call-frame
information in `.eh_frame` (§1 to §8 here) and the language-specific
data areas in `.gcc_except_table` (§9). It depends on `hod_obj`
only: it writes sections and relocations into an `Object` and knows nothing
of IR or of `hod_cg`'s types.

External formats are cited as: **DWARF 5** (§6.4 "Call Frame Information",
§7.6 LEB128, §7.24 the call-frame instruction encodings); **LSB**, the
Linux Standard Base Core Specification 5.0, "Exception Frames" (the
`.eh_frame` format, its augmentations and the `DW_EH_PE_*` pointer
encodings); **AArch64 ELF** and the **x86-64 psABI** for the relocations
and DWARF register numbers; the **Itanium C++ ABI** exception-handling
chapter and LLVM's `EHStreamer` for the LSDA, which no standard specifies.

The reference is `llc` at `-O0 -relocation-model=pic` (LLVM 19 and 22, which
agree on everything here). Where `hod_eh` writes the same rules with
different bytes, this spec says so (§4.3).

## 1. Interface

```rust
pub enum Op { DefCfa { reg: u16, offset: i32 }, DefCfaOffset(i32),
              DefCfaRegister(u16), Offset { reg: u16, cfa_offset: i32 },
              Restore { reg: u16 } }
pub struct Row { pub offset: u32, pub op: Op }
pub struct Lsda { pub symbol: SymbolId, pub addend: i64 }
pub struct Fde<'a> {
    pub order: u32,            // the function's position in the module
    pub start: SymbolId,       // what the initial location is relative to
    pub start_addend: i64,
    pub size: u32,             // the function's code size
    pub rows: &'a [Row],       // in code order
    pub personality: Option<SymbolId>,  // the DW.ref slot (§2.2)
    pub lsda: Option<Lsda>,
}
pub fn write_eh_frame(obj: &mut Object, fdes: &[Fde<'_>])
    -> Result<Option<SectionId>, Error>;
pub fn personality_ref(obj: &mut Object, personality: SymbolId)
    -> Result<SymbolId, Error>;
pub struct CfiTarget { .. }    // §2.1; AARCH64, X86_64, for_arch(arch)

pub const NO_PAD: u32;
pub struct CallSite { pub start: u32, pub len: u32, pub pad: u32, pub landing: u32 }
pub struct Pad { pub block: u32, pub offset: u32, pub cleanup: bool,
                 pub clauses: Range<u32> }
pub enum Clause { Catch(u32), Filter(Range<u32>) }
pub struct FuncLsda<'a> {
    pub order: u32, pub name: &'a str, pub section: SectionId, pub size: u32,
    pub call_sites: &'a [CallSite], pub pads: &'a [Pad], pub clauses: &'a [Clause],
    pub type_ids: &'a [Option<SymbolId>], pub filter_ids: &'a [u32],
}
pub struct LsdaOptions { pub function_sections: bool, pub stubs: Option<SectionId> }
pub fn write_lsdas(obj: &mut Object, funcs: &[FuncLsda<'_>], options: LsdaOptions)
    -> Result<Vec<Lsda>, Error>;                          // §9
```

`Op` and `Row` are `hod_cg`'s `CfiOp` and `CfiRow` (`spec/codegen.md` §9.3)
one for one: registers are DWARF numbers, offsets are bytes (not yet divided
by the data alignment factor), and a row holds from its code offset, counted
from the function's first byte, on. `write_eh_frame` writes one `.eh_frame`
for the whole object, with its relocations, and returns the section, or
`None` (and adds nothing) when `fdes` is empty. The encoding primitives
(`write_uleb`, `write_sleb`, `uleb_len`, `write_advance`, `write_op`) are
public for `hod_dwarf`'s `.debug_frame`, should it come, and for tests.

### 1.1 Filling `Fde` from `FuncRecord`

The caller (`hod_cg::emit_module`, `spec/codegen.md` §9.7) makes one `Fde`
per function that needs an unwind-table entry, after the merge has placed
it:

| field | from |
|---|---|
| `order` | the function's `SymId` index (module order) |
| `start`, `start_addend` | the section symbol of the function's section and the function's offset in it, as `llc` does it (a COMDAT function's section included, `spec/object.md` "References into a group from outside it") |
| `size` | `func.code.len()` |
| `rows` | `chunk.cfi[func.cfi]`, each `CfiOp` mapped to the `Op` of the same name |
| `personality` | when `func.has_pads`: the slot `personality_ref` made for the function's IR personality (`hod_ir` `SymbolKind::Function { personality }`), one slot per personality per object; `None` otherwise |
| `lsda` | when `func.has_pads`: the function's entry of `write_lsdas`' result (§9.1); `None` otherwise |

Which functions get an entry is LLVM's rule (`needsUnwindTableEntry`): a
function has an FDE unless it is `nounwind`, has no `uwtable` and no
personality. A known personality (the C and C++ ones) is used only by a
function with landing pads, so a function with a personality and no pads
gets the CIE without one, as `llc` does. `FuncRecord` carries neither the
personality nor the attributes; the caller reads them from the module.

## 2. CIE

### 2.1 The per-target table

Everything target-specific is in one `CfiTarget` per architecture:

| | AArch64 | x86-64 |
|---|---|---|
| code alignment factor | 1 | 1 |
| data alignment factor | -4 | -8 |
| return-address register | 30 (`x30`) | 16 (`rip`) |
| initial instructions | `DW_CFA_def_cfa sp(31), 0` | `DW_CFA_def_cfa rsp(7), 8`; `DW_CFA_offset rip(16), -8` |
| FDE pointer encoding (`R`) | `pcrel\|sdata4` (0x1b) | 0x1b |
| personality encoding (`P`) | `indirect\|pcrel\|sdata8` (0x9c) | `indirect\|pcrel\|sdata4` (0x9b) |
| LSDA encoding (`L`) | `pcrel\|sdata8` (0x1c) | `pcrel\|sdata4` (0x1b) |
| initial-location relocation | `R_AARCH64_PREL32` | `R_X86_64_PC32` |
| personality and LSDA relocation | `R_AARCH64_PREL64` | `R_X86_64_PC32` |
| `.eh_frame` `sh_type` | `SHT_PROGBITS` | `SHT_X86_64_UNWIND` |

These are `llc`'s choices for Linux PIC. AArch64 keeps the code alignment
factor at 1 although instructions are 4 bytes, so an advance is in bytes.

### 2.2 Layout

A CIE is: length (4 bytes, of what follows), CIE id 0 (4), version 1,
augmentation string, code alignment factor (ULEB128), data alignment factor
(SLEB128), return-address register (1 byte, version 1), augmentation data
length (ULEB128), augmentation data, initial instructions, `DW_CFA_nop`
padding to a multiple of 4 (§3.4).

- Without a personality: augmentation `"zR"`, data `R`.
- With one: `"zPLR"`, data `P`, the personality pointer (`pointer_size`
  bytes of zeros with the personality-and-LSDA relocation against the
  personality's slot, addend 0), `L`, `R`.

The personality pointer is indirect: it addresses a slot holding the
routine's address. `personality_ref` makes the slot as `llc` does: an
8-byte zero word in a writable section `.data.DW.ref.<routine>` (alignment
8), with an absolute 64-bit relocation (`R_AARCH64_ABS64`, `R_X86_64_64`)
against the routine, defined by a weak hidden object symbol
`DW.ref.<routine>` of size 8, which is also the signature of the COMDAT
group holding the section (so the linker keeps one slot for the program).

## 3. FDE

### 3.1 Layout

length (4), CIE pointer (4: the distance from this field back to the CIE's
first byte), initial location (4, `R`: relocated, §3.2), address range (4,
the function's `size`), augmentation data length (ULEB128: 0 under a `zR`
CIE, `pointer_size` under `zPLR`), the LSDA pointer under `zPLR` (§3.3),
the instructions (§4), `DW_CFA_nop` padding (§3.4).

### 3.2 Initial location

A PC-relative 32-bit relocation at the field against `start` with addend
`start_addend`.

### 3.3 LSDA pointer

Under a `zPLR` CIE every FDE has the field (`pointer_size` bytes). With
`lsda`, it carries the personality-and-LSDA relocation against
`lsda.symbol` with `lsda.addend`; without, it is zero and unrelocated.
libgcc and LLVM's libunwind both read a raw zero as "no LSDA" (they add the
PC-relative base only to a non-zero value), and the C++ personality then
unwinds the frame without stopping. `llc` never writes this form, and §1.1
never asks for it (a function with a personality has pads and so an LSDA);
it is tested for its encoding only. `lsda` without `personality` is `LsdaWithoutPersonality`.

### 3.4 Padding

Every CIE and FDE is padded with `DW_CFA_nop` (0) so that it ends at a
multiple of 4 from the section's start; the last FDE of the section is
padded to the section's alignment, 8, so the section's size is a multiple
of it. Both are `llc`'s. The padding is inside the entry's length.

### 3.5 Errors

`write_eh_frame` checks each FDE and, on the first problem, returns an
error naming the FDE's `order` (and the row's index) and leaves `obj` as it
was. It never panics on input:

| error | when |
|---|---|
| `RowOutOfOrder` | a row's offset is below the previous row's |
| `RowBeyondEnd` | a row's offset exceeds `size` (a row at `size` is accepted: it holds for no instruction) |
| `Unfactored` | an offset the encoding must divide by the data alignment factor (a saved register's place, a negative CFA offset) is not a multiple of it, or a code delta not one of the code alignment factor |
| `LsdaWithoutPersonality` | §3.3 |
| `DuplicateOrder` | two FDEs of one CIE have the same `order`, or two personalities' first users do (either would make the output depend on the input order, §5) |
| `TooLarge` | the section would reach 4 GiB, beyond 32-bit CIE pointers |
| `Object` | `hod_obj` refused the section |

## 4. Instructions

For each row in order: the advance from the previous row's offset (the
first from 0), then the row's instruction.

### 4.1 Advance

The delta, divided by the code alignment factor, as the shortest of: none
for 0; `DW_CFA_advance_loc` (delta in the low 6 bits) below 0x40;
`DW_CFA_advance_loc1` up to 0xff; `DW_CFA_advance_loc2` up to 0xffff;
`DW_CFA_advance_loc4` beyond. Rows at one offset share the advance.

### 4.2 Forms

`llc`'s choices (LLVM `MCDwarf`), extended to cases it does not produce:

| row | encoding |
|---|---|
| `DefCfa { reg, offset }` | `DW_CFA_def_cfa` ULEB `reg`, ULEB `offset`; `DW_CFA_def_cfa_sf` ULEB `reg`, SLEB `offset / daf` for a negative offset |
| `DefCfaOffset(o)` | `DW_CFA_def_cfa_offset` ULEB `o`; `DW_CFA_def_cfa_offset_sf` SLEB `o / daf` for a negative one |
| `DefCfaRegister(reg)` | `DW_CFA_def_cfa_register` ULEB `reg` |
| `Offset { reg, cfa_offset }`, `f = cfa_offset / daf` | `f >= 0`, `reg < 64`: `DW_CFA_offset \| reg`, ULEB `f`; `f >= 0`, `reg >= 64`: `DW_CFA_offset_extended` ULEB `reg`, ULEB `f`; `f < 0`: `DW_CFA_offset_extended_sf` ULEB `reg`, SLEB `f` |
| `Restore { reg }` | `DW_CFA_restore \| reg` below 64; `DW_CFA_restore_extended` ULEB `reg` |

(`daf` is the data alignment factor.)

### 4.3 Remembered state

hod's rows have no `DW_CFA_remember_state`/`DW_CFA_restore_state`: a target
states the body's rules again after an exit sequence that is not the last
code (`spec/codegen.md` §9.3). `llc` uses the pair in that place, so for
such a function `hod_eh`'s bytes differ from `llc`'s while the rules at
every offset are the same. The difference is deliberate: it keeps the rows
a flat list a target appends to and the encoder stateless.

### 4.4 LEB128

DWARF 5 §7.6: unsigned, 7 bits per byte, low first, high bit set on all
but the last; signed likewise, ending when the remaining value is all sign
bits and the last byte's bit 6 agrees.

## 5. Order, sharing and determinism

FDEs are grouped by CIE, one CIE per distinct personality and one for none,
each CIE written once, immediately before its FDEs. The CIE without a
personality comes first, then one per personality in the order of the
lowest `order` that uses it; within a CIE, FDEs are in `order`. For an
object with at most one personality this is `llc`'s order (it sorts its
frames by CIE, the one without a personality first, stably in function
order); with several, `llc` orders them by an internal pointer, which the
tests found to be first use as well.

The output depends only on the set of `Fde`s, never on the order of the
slice: a caller may pass functions in whatever order its workers finished.

## 6. Section

`.eh_frame`, `sh_type` per §2.1, `sh_flags` `SHF_ALLOC`, alignment 8,
`sh_entsize` 0, one per object. Its relocations are added in increasing
offset order (so `hod_obj` writes them without sorting, `spec/object.md`
"Relocation sections"). There is no terminator: the linker adds one.

## 7. Tests

### 7.1 `llc` round trip (`tests/llc_oracle.rs`)

A table of modules (leaf and non-leaf functions, synchronous and
asynchronous tables, callee-saved general and vector registers, frame
pointer on and off, frames of 70,000 and 20,000,000 bytes, dynamic
`alloca`, functions with landing pads sharing a CIE, a COMDAT function, a
function with a personality and no pads, two personalities, bodies long
enough for each advance opcode, sixty functions sharing one CIE), each
compiled by `llc` 19 and 22 for AArch64 and x86-64. The object's
`.eh_frame` is decoded with `llvm-dwarfdump --eh-frame` and its
relocations; each FDE becomes an `Fde` (rows from the instructions, with
`DW_CFA_restore_state` restated as §4.3 says; `order` from the position of
the function's `define`; `start`, personality and LSDA from the
relocations), encoded with `write_eh_frame` (also with the input reversed),
and spliced into the object. Asserted:

- the section's type, flags and alignment are `llc`'s;
- decoded equality: the same entries in the same order, each CIE with the
  same augmentation, factors, return-address register and encodings, each
  FDE under the same CIE with the same range and the same rules at every
  code offset (`llvm-dwarfdump`'s unwind table);
- the relocations mean the same: per entry, field and type, the same
  function or symbol and addend;
- **byte identity** of the section and its relocations wherever `llc` used
  no remembered state, which is every case but the functions with landing
  pads on AArch64. Byte identity is asserted there because the advance
  opcodes, instruction forms, CIE layout and padding are all deterministic
  choices `hod_eh` makes as `llc` does; it is not asserted for the others
  because of §4.3.

`personality_slot_as_llc_makes_it` compares `personality_ref`'s slot with
`llc`'s (section type, flags, alignment and size; symbol binding, type,
visibility and value; relocation; the COMDAT group it signs).

### 7.2 Real unwinding (`tests/unwind.rs`)

`tests/data/unwind.ll` (clang 19 `-O0` from `tests/data/unwind.cpp`) throws
from a leaf through a 70,000-byte frame, a frame with dynamic allocation, a
COMDAT template frame, frames with cleanups and a catch that rethrows, to a
catch in `main`, three times, and once does not throw. `llc` 19 and 22
compile it; its `.eh_frame` and `.rela.eh_frame` are replaced by `hod_eh`'s
(§7.1) by appending the new contents to the file and pointing the two
section headers at them, every other byte unchanged. Linked with `g++` and
GNU ld, lld and mold, with `--eh-frame-hdr` and without, the replacement
must print what the original prints and exit as it does. With the header
the original catches every exception. Without it, this libgcc finds no FDE
(it looks them up through `PT_GNU_EH_FRAME`), so the original terminates at
the first throw, and the replacement must do the same.

### 7.3 Units (`tests/encoder.rs`)

LEB128 (DWARF's examples and each length's edges); each advance opcode at
its boundaries (0x3f/0x40, 0xff/0x100, 0xffff/0x10000); every instruction
form of §4.2 including the `_sf` and `_extended` ones and unfactorable
offsets; CIE sharing and order with two personalities interleaved;
padding of every entry and of the section; an FDE with a personality and no
LSDA (§3.3); output identical under forty
permutations of the input; each error of §3.5, with the object unchanged.

## 8. Performance

Encoding is one pass over the rows, linear in their number: one output
buffer per object (reserved from the row count), one table of pending
relocations, the FDE order computed by a counting sort over CIEs (a
comparison sort only for input not already in module order), and each FDE
assembled in a stack buffer and appended in one copy. Measured by
`tests/throughput.rs` (`--release`, `--ignored`): see
`spec/landed/eh/WP-042.md`.

## 9. LSDA (`.gcc_except_table`)

One language-specific data area per function with landing pads, read by
the C++ personality routine (`__gxx_personality_v0`) through the FDE's
LSDA pointer (§3.3). `hod_eh` writes it as `llc` does for Linux PIC
(LLVM `EHStreamer::emitExceptionTable`), byte for byte: LLVM 19 and 22
agree on everything here.

### 9.1 Interface

`CallSite`, `Pad` and `Clause` are `hod_cg`'s `CallSiteRow`, `PadRow` and
`ClauseRow` (`spec/codegen.md` §9.4) field for field, as `Op`/`Row` are
its CFI rows (§1; `hod_eh` does not depend on `hod_cg`, and WP-049 unifies
the types). `Pad::block` is the `BlockId`'s index, carried but not used.
`FuncLsda` is one function's ranges of the chunk's tables, so every index
in them is relative to the function:

| field | from (`hod_cg::emit_module`) |
|---|---|
| `order` | the function's `SymId` index, as `Fde::order` |
| `name` | the function's symbol name (it names its own section, §9.6) |
| `section` | the section the merge put the function's code in |
| `size` | `func.code.len()` |
| `call_sites`, `pads`, `clauses` | `chunk.call_sites[func.call_sites]` etc., each row mapped to the type of the same name (`NO_PAD` is `hod_cg::NO_PAD`) |
| `type_ids` | `chunk.type_ids[func.type_ids]`, each `SymId` mapped to its `hod_obj::SymbolId` (the arithmetic of `spec/codegen.md` §9.7) |
| `filter_ids` | `chunk.filter_ids[func.filter_ids]` |

`write_lsdas` takes every function of the object with `has_pads` at once
and returns, in the order of `funcs`, each one's `Lsda` (the section
symbol of its table's section and its offset there), which is the
`Fde::lsda` of the same function: the caller writes the LSDAs first and
`.eh_frame` second. `LsdaOptions::stubs` is the object's `.data`, to which
the type-info stubs are appended (§9.5); `None` makes a `.data` of its own
when there are stubs. An empty `funcs` adds nothing.

### 9.2 Header

All offsets are from the LSDA's first byte, which is at a multiple of 4.

| field | bytes |
|---|---|
| `@LPStart` encoding | `DW_EH_PE_omit` (0xff): landing pads are relative to the function's start (the FDE's initial location) |
| `@TType` encoding | `DW_EH_PE_omit` when the function has no type ids and no filter; otherwise the target's personality encoding (§2.1): AArch64 `indirect\|pcrel\|sdata8` (0x9c), x86-64 `indirect\|pcrel\|sdata4` (0x9b) |
| `@TType` base offset | only with a type table: ULEB128, the distance from the end of this field to the end of the type table (the "TType base") |
| call-site encoding | `DW_EH_PE_uleb128` (0x01) |
| call-site table length | ULEB128, in bytes |

The TType base offset's size depends on the padding before the type table
(§9.5), which depends on that size. `hod_eh` settles it as the assembler
does (its LEB fragments start empty and only grow): starting from size 0,
compute the value with the current size, and while the value needs more
bytes than the current size, take that many and compute again; the value
is then written in exactly the final size, padded with `0x80`
continuation bytes if it needs fewer (`ttype_base_offset_at_every_size`
checks the field against the table at every size up to 24 types and 48
call sites; `llc`'s 24-type case is byte-identical).

### 9.3 Call-site table

Each entry is four ULEB128 values: start, length, landing pad (0 for
none) and action (§9.4; 0 for none), offsets from the function's start.
Entries are in address order. From the rows (`EHStreamer::
computeCallSiteTable`, with hod's rows in place of LLVM's `EH_LABEL`
pairs and calls):

1. Rows must be in code order and not overlap; a row covers
   `[start, start + len)`. A row with a pad lands at its `landing`, or at
   its pad's `offset` when `landing` is 0; its action is its pad's first
   action.
2. A row with a pad whose landing address and action equal those of the
   entry just written, when that entry also came from a row with a pad,
   extends that entry to the row's end (across any code between them that
   has no call).
3. A row without a pad (`NO_PAD`) writes nothing itself. When a row with
   a pad follows one or more rows without, an entry with no landing pad
   and no action covering from the end of the last entry with a pad (or
   the function's start) to the start of the row with a pad is written
   first, and rule 2 does not merge across it. When rows without a pad
   follow the last row with a pad, a final such entry covers from there to
   the function's end.

So a call that can throw and has no pad is covered by an entry without a
landing pad (the personality unwinds through it), and code with no call
in it is covered only where it lies inside a merged range. A call covered
by no entry makes the personality call `std::terminate`; with hod's rows
that is only a call the code generator did not record.

`llc` decides "a call without a pad" by whether the callee is `nounwind`
(`callToNoUnwindFunction`): a call to a `nounwind` function leaves a gap.
`CallSiteRow` does not say whether its callee can unwind (§9.9), so every
plain call is a row without a pad, and `hod_eh` writes an entry where
`llc` writes none (or splits an entry `llc` merges). The tables then
differ in bytes, not in what any call that can unwind does; a `nounwind`
callee that unwinds anyway (undefined behaviour) is unwound through
rather than terminated. §9.8's oracle builds its rows with `llc`'s rule,
which is what makes it byte-identical.

### 9.4 Action table

The action table follows the call-site table. A record is two SLEB128
values: the type filter (a type id > 0 is a `catch` of that type; 0 is a
cleanup; < 0 is an exception specification, the negated byte offset,
biased by one, of its list in §9.5's filter table) and the displacement,
from the start of this second field, of the next record of the chain (0:
the chain ends). A call site's action is the offset of its chain's first
record, plus one.

The records and their sharing are LLVM's (`computeActionsTable`), which
the oracle confirms byte for byte:

1. Each pad's list, in LLVM's order: 0 if the pad is a cleanup with at
   least one clause, then its clauses **last to first**, a `Catch(t)` as
   `t`, a `Filter` as its filter id (§9.5). A cleanup with no clause has
   the empty list: action 0, which the personality treats as a cleanup.
   The chain written for a list runs from its last element to its first,
   so it visits the clauses in source order and the cleanup last.
2. Pads are taken in the lexicographic order of their lists (signed, so
   filters before cleanups before catches; ties in pad order). Each pad
   shares with the pad before it the longest common prefix of their lists:
   those records (the tail of its chain) are the previous pad's, and only
   the rest are appended, the first of them pointing at the last shared
   record. A pad whose list equals the previous one's has the same action;
   the empty list has action 0.
3. Records are appended in that order, each displacement computed from
   the records' actual positions (backwards, so negative).

This shares only with the neighbour in sorted order, not every common
tail; it is `llc`'s scheme, kept for byte identity, and the unwinder reads
any sharing the same way.

### 9.5 Type table and filters

Only with type ids or filters. After the action table, zero bytes to a
multiple of 4; then the type table, the entry of type id `k` at
`TTBase - k * pointer_size` (so the table is written last type id first),
`pointer_size` 8 on AArch64 and 4 on x86-64; the TType base; the filter
table; zero bytes to a multiple of 4 ending the LSDA (also without a type
table).

- A catch-all (`None`) entry is zero, unrelocated.
- Any other entry is indirect and PC-relative: the address of a stub
  word holding the type-info's address. The field is zero with the
  personality-and-LSDA relocation of §2.1 (`R_AARCH64_PREL64`,
  `R_X86_64_PC32`) against the stub section's section symbol, addend the
  stub's offset. A stub is 8 zero bytes in `LsdaOptions::stubs` (aligned
  to 8 before the first, the section's alignment raised to 8) with an
  absolute 64-bit relocation (`R_AARCH64_ABS64`, `R_X86_64_64`) against
  the type-info's symbol: one per distinct type-info symbol in the object,
  all functions' together, ordered by symbol name (then by symbol id),
  as `llc`'s `.L<name>.DW.stub` words in `.data` are. The stub keeps the
  read-only table free of dynamic relocations.
- The filter table: for each distinct filter, its type ids as ULEB128
  and a 0. Filters are numbered as LLVM numbers them
  (`MachineFunction::getFilterIDFor`): processing pads in `pads` order and
  each pad's clauses last to first, a filter whose list equals the end of
  a filter already in the table (an empty one: any filter's terminator)
  reuses that position; otherwise it is appended. Its filter id is
  `-(1 + index of its first entry)` and its record value
  `-(1 + byte offset of that entry)`. `llc` registers pads in reverse
  post-order of the CFG, hod in `pads` order (block order); the two can
  number a function's filters differently when it has several distinct
  filters in different pads. The lists then differ in order only, which
  the personality reads the same; no case of §9.8 differed.

### 9.6 Sections

The section is `SHT_PROGBITS`, `SHF_ALLOC`, alignment 4, entry size 0,
with no `SHF_LINK_ORDER` (`llc` sets it only for binutils ≥ 2.36, which
its default does not assume). A function's table is in

- `.gcc_except_table.<name>` of its own when the function's section is
  in a group (the table joins that group, so the linker keeps or drops it
  with the function) or `LsdaOptions::function_sections` is set, as `llc`
  does (`TargetLoweringObjectFileELF::getSectionForLSDA` with unique
  section names);
- otherwise `.gcc_except_table`, one for the object, the tables one after
  another in `order`.

Relocations are added in increasing offset order per section. `llc` also
defines a local `GCC_except_table<n>` label at each LSDA; nothing refers
to it (the FDE uses the section symbol), and `hod_eh` does not write it.

### 9.7 Order, determinism and cost

The output depends only on the set of `FuncLsda`s: tables are placed by
`order` and stubs by name, so a caller may pass functions in any order
(`DuplicateOrder` otherwise). Encoding is one pass over each function's
rows into one buffer for the object, reusing a set of scratch buffers
from function to function: no allocation per function. The sizes are
computed first and the bytes written into a zeroed region, so padding and
type-table fields cost nothing. Measured by `tests/throughput.rs`: see
`spec/landed/eh/WP-047.md`.

### 9.8 Errors

`write_lsdas` checks every function before it touches the object and, on
the first problem (functions in `order`), returns an error naming the
function's `order` and the row, pad or clause, with `obj` unchanged. It
never panics on input. (An `Object` error, which only `hod_obj`'s limits
cause, may leave the sections added so far.)

| error | when |
|---|---|
| `CallSiteOutOfOrder` | a row starts before the previous row ends |
| `CallSiteBeyondEnd` | a row ends beyond `size` (or overflows) |
| `BadPad` | a row's `pad` is neither `NO_PAD` nor an index of `pads` |
| `LandingBeyondEnd` | a row's landing address (§9.3 rule 1) is not below `size` |
| `BadClauses` | a pad's clause range is reversed or beyond `clauses` |
| `EmptyPad` | a pad has no clause and is not a cleanup |
| `BadTypeId` | a `Catch` names 0 or more than `type_ids.len()` |
| `BadFilter` | a `Filter` range is beyond `filter_ids`, or one of its ids is not a type id |
| `BadName` | a function that gets its own section has a NUL in its name |
| `DuplicateOrder` | two functions have the same `order` |
| `TooLarge` | the tables would reach 4 GiB |
| `Object` | `hod_obj` refused a section |

### 9.9 Tests

- **`llc` round trip** (`tests/lsda_oracle.rs`, `tests/common/lsda.rs`):
  C++-shaped modules (catches of several types with `llvm.eh.typeid.for`
  dispatch, catch-all, nested `try` whose inner pad's chain shares the
  outer's tail, cleanups only, `throw(int, long)` and `throw()`
  specifications and a catch beside a filter, a rethrow from a handler
  with a cleanup and a terminate pad, a COMDAT template next to a plain
  function, `nounwind` and throwing plain calls between invokes, and 48
  invokes over 6 pads and 24 types for two-byte ULEB128 fields and a TType
  base beyond 127), compiled by `llc` 19 and 22 for AArch64 and x86-64,
  with and without `-function-sections`, at `-O0 -global-isel=0
  -fast-isel=0 -relocation-model=pic`. Each LSDA llc wrote (found through
  `.eh_frame`'s relocations) is decoded with its relocations and the stub
  words they reach; it becomes hod's rows: llc's type table, one pad per
  landing address with the clauses of its chain, a row per call that can
  throw (from `.text`'s call relocations, a callee being `nounwind` by its
  IR declaration), each padded entry split at every such call so that hod
  must merge it again, every other row with landing 0. `write_lsdas`
  writes them (also with the input reversed) into an object with llc's
  code sections and groups. Asserted: the same sections (name, type,
  flags, alignment, group); the same stubs; each function's LSDA in the
  section and at the offset llc put it; decoded equality (encodings, type
  table, every call site's range, landing pad and action chain by
  meaning); and **byte identity** of every section and its relocations,
  which holds in every case. A wrong landing address or a wrong catch type
  in the rows fails the comparison (`wrong_rows_fail_the_round_trip`).
- **Real unwinding** (`tests/lsda_unwind.rs`): `tests/data/lsda.ll`
  (clang 19 `-std=c++14 -O0` from `tests/data/lsda.cpp`) catches by type
  after a non-matching handler, falls through non-matching inner handlers
  past a cleanup to an outer `catch (Base &)`, rethrows out of a handler
  with a cleanup to a catch-all, passes an exception through a satisfied
  `throw(Error)`, has two pads in one function, and on request violates a
  `throw(Error)` (`std::unexpected`, then `std::terminate`) or a
  `noexcept`. `llc` 19 and 22 compile it; its LSDA sections are replaced by
  `write_lsdas`' and its `.eh_frame` by `write_eh_frame`'s with every
  FDE's LSDA from `write_lsdas`' result (§7.2's splice). Linked with `g++`
  and GNU ld, lld and mold with `--eh-frame-hdr`, each of the three runs
  prints, writes to stderr and exits exactly as the original (the
  violations abort with SIGABRT in both). A wrong table (the first call
  of `two_pads` pointed at the other pad, whose handler does not match)
  makes the program abort instead of printing what the original prints.
- **Units** (`tests/lsda_units.rs`): exact bytes of a cleanup-only table
  and of a catch, catch-all and catch chain with its type table and stubs;
  filters sharing a tail and `throw()` sharing a terminator; every merge
  and gap rule of §9.3 and a function whose calls all lack pads; the TType
  base offset at every size; sections and groups with and without function
  sections; identical output under forty permutations of the input; each
  error of §9.8 with the object unchanged.

### 9.10 What the core's rows do not say

- Whether a call can unwind (§9.3): `CallSiteRow` (`hod_cg` `record.rs`,
  `CallSiteRow`) has no flag for a `nounwind` callee, so hod's tables cover
  calls `llc` leaves uncovered. Harmless for correct programs; a flag
  (or dropping such rows in the core) would make them `llc`'s.
- The function's name, section and the object's `.data`: the caller
  supplies them after the merge, as it does `Fde::start` (§1.1).
