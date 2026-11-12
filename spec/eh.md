# spec: hod_eh — unwind tables

`hod_eh` writes an object's unwind tables from what the code generator
records per function (`spec/codegen.md` §9.3, §9.4): the call-frame
information in `.eh_frame` (§1 to §8 here) and, later, the language-specific
data areas in `.gcc_except_table` (§9, planned). It depends on `hod_obj`
only: it writes sections and relocations into an `Object` and knows nothing
of IR or of `hod_cg`'s types.

External formats are cited as: **DWARF 5** (§6.4 "Call Frame Information",
§7.6 LEB128, §7.24 the call-frame instruction encodings); **LSB**, the
Linux Standard Base Core Specification 5.0, "Exception Frames" (the
`.eh_frame` format, its augmentations and the `DW_EH_PE_*` pointer
encodings); **AArch64 ELF** and the **x86-64 psABI** for the relocations
and DWARF register numbers.

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
| `lsda` | when `func.has_pads`: where the LSDA package put the function's LSDA (§9); `None` otherwise |

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

## 9. LSDA (planned)

`.gcc_except_table` from `call_sites`, `pads`, `clauses`, `type_ids` and
`filter_ids` (`spec/codegen.md` §9.4). Its result is the `Lsda` of each
function with pads.
