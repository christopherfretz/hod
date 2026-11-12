# `hod_mc_a64`: AArch64 instruction encoding

`crates/hod_mc_a64` turns an instruction and its operands into the 32-bit
instruction word. It knows nothing about the IR, object files or assembly
text. Its consumer is a single-pass code generator that calls it once per
emitted instruction, so the whole API is plain functions on small `Copy`
values.

This document covers the base integer instruction set, scalar floating
point, FP/SIMD loads and stores, and the subset of Advanced SIMD a code
generator needs to treat vectors as values. The oracle and the crate assume
FEAT_FP and FEAT_AdvSIMD (`+fp-armv8,+neon`), without FEAT_FP16.

Instructions are cited by their Arm Architecture Reference Manual names
(Arm ARM, A-profile, "A64 Base Instruction Descriptions"), for example
"ADD (immediate)"; each group names its encoding class from "A64 instruction
set encoding" the same way. Where a constructor is an alias, the table gives
the instruction it encodes; the alias conditions are the Arm ARM's.

## Shape of the API

- A **constructor** is an `#[inline] pub const fn` that returns the
  instruction word as a `u32` in the value's natural (host) order. The
  caller writes it to the section little-endian.
- A constructor allocates nothing, reads no state and writes none. There are
  no traits, trait objects or builders.
- Everything is re-exported from the crate root, so `hod_mc_a64::add_imm`
  and `hod_mc_a64::arith::add_imm` are the same function.
- An operand that selects among encodings of one instruction (operand size,
  shift kind, memory access width, ordering) is an enum operand, not a
  separate function.

### Checked and unchecked forms

A constructor whose operands are all valid by their types is infallible and
has one form.

A constructor `f` whose operands have a **precondition** (an immediate that
may not fit, a shift amount, a constraint between registers) has a twin
`try_f` with the same parameters:

- `try_f(..)` returns `Some(word)` when the precondition holds and `None`
  exactly when it does not. It never panics.
- `f(..)` returns the same `word` when the precondition holds. It checks the
  precondition with `debug_assert!` (message
  `"<f>: operand precondition violated"`); with debug assertions off, a
  violated precondition yields an unspecified word. It is the form for a
  caller that has already established the precondition.

Both forms are generated from one statement of the precondition (the
`checked!` macro in `src/lib.rs`), so they cannot drift apart. In the tables
below, a constructor has a `try_` twin exactly when its "precondition" cell
is not empty.

A precondition is at least what the architecture needs for the encoding to
exist and to be predictable: operand combinations the Arm ARM calls
CONSTRAINED UNPREDICTABLE are rejected, not encoded.

## Registers and operand kinds

Register field value 31 is the stack pointer in some operand positions and
the zero register in others. The types make the caller say which.

| type | values | use |
|---|---|---|
| `Reg` | `X0`..`X30` (`Reg::X0`.., `Reg::FP` = X29, `Reg::LR` = X30; `Reg::new(n)` is `None` for `n > 30`) | a position that accepts neither, or a caller-side register number |
| `RegZr` | a `Reg`, or `RegZr::ZR` | a position where 31 is the zero register |
| `RegSp` | a `Reg`, or `RegSp::SP` | a position where 31 is the stack pointer |

`Reg::zr()` and `Reg::sp()` (and `From<Reg>`) convert; `RegZr::new(code)` and
`RegSp::new(code)` build from a field value `0..=31`; `.reg()` goes back
(`None` for ZR/SP); `.code()` is the 5-bit field. There is no conversion
between `RegZr` and `RegSp`: 31 does not mean the same register.

The operand size is not part of a register. Constructors that exist in both
sizes take `Size::W` (32-bit, `sf` = 0) or `Size::X` (64-bit) as their first
operand. A 32-bit operation zeroes the upper half of the destination, as the
architecture says. Constructors with one fixed size (`smulh`, `sxtw`, `adr`,
...) take no size.

| type | values |
|---|---|
| `Size` | `W`, `X`; `bits()` is 32 or 64 |
| `Cond` | `Eq Ne Hs Lo Mi Pl Vs Vc Hi Ls Ge Lt Gt Le Al Nv`, encodings 0..=15 in that order; `invert()` flips the low encoding bit; `is_always()` is true for `Al` and `Nv` |
| `Shift` | `Lsl Lsr Asr Ror`, encodings 0..=3 |
| `Extend` | `Uxtb Uxth Uxtw Uxtx Sxtb Sxth Sxtw Sxtx`, encodings 0..=7 |
| `MemSize` | `B H W X`: an access of 1, 2, 4, 8 bytes |

Each has an `ALL` array in encoding order.

### FP/SIMD registers

`VReg` is one of the 32 FP/SIMD registers, `VReg::V0`..`VReg::V31`
(`VReg::new(n)` is `None` for `n > 31`; `.index()` is the number). There is
no register 31 with a second meaning, so there is one type.

As with `Size`, which view of the register an instruction uses is not part
of the register. `B0`, `H0`, `S0`, `D0`, `Q0` and `V0.4S` are all
`VReg::V0`; the constructor's other operands say which:

| type | values | says |
|---|---|---|
| `FpSize` | `S`, `D`; `bits()` is 32 or 64 | the precision of a scalar floating-point operation |
| `FpFormat` | `H`, `S`, `D`; `bits()` is 16, 32 or 64 | a floating-point format, where half precision exists without FEAT_FP16: conversions and constants |
| `VecBits` | `V64`, `V128`; `bits()` is 64 or 128 | the width of a vector whose element size does not matter (bitwise operations, byte permutes) |
| `Elem` | `B`, `H`, `S`, `D`; `bits()` is 8..64; `lanes()` is how many fit in 128 bits (16, 8, 4, 2) | an element width, for element access |
| `Arr` | `B8 B16 H4 H8 S2 S4 D2`; `elem()`, `vec_bits()`, `lanes()` | a vector arrangement (`8B`, `16B`, ... in assembly) |
| `FpArr` | `S2 S4 D2`; `arr()`, `fp_size()` | an arrangement of floating-point elements |
| `VMemSize` | `B H S D Q`; `bytes()` is 1..16 | the scalar view a load or store transfers |
| `VPairSize` | `S D Q`; `bytes()` is 4, 8, 16 | the same, for pairs and literal loads |

Each has an `ALL` array. There is no `1D` arrangement: the instructions
that would use it here are the scalar `D` forms. An instruction that writes
a scalar view or a 64-bit vector zeroes the rest of the 128-bit register, as
the architecture says. The only constructors that leave part of the
destination as it was are the element inserts: `ins_elem`, `ins_gpr` and
`fmov_gpr_to_hi`.

## Immediates

`is_arith_imm(imm: u64) -> bool`: `imm` is an add/subtract immediate, that
is `imm < 4096`, or `imm` is a multiple of 4096 below `1 << 24`. The
constructors choose the shifted form (`sh` = 1) only when the unshifted one
does not apply, so zero encodes with `sh` = 0.

`encode_bitmask(sz: Size, value: u64) -> Option<u16>`: the 13-bit field
`N:immr:imms` of a logical immediate.

- A value is encodable exactly when the architecture's `DecodeBitMasks`
  produces it for some field: a rotation of a run of ones, neither empty nor
  full, in an element of 2, 4, 8, 16, 32 or 64 bits, replicated to the
  operand size. There are 5334 such 64-bit values and 1302 32-bit ones.
- For `Size::W` the value must have its upper 32 bits clear (a value with
  bits above bit 31 is not a 32-bit immediate, whatever its low half is),
  and `N` is 0.
- The field returned is the canonical one: `immr` is below the element
  size. It is the field LLVM's assembler produces.

`decode_bitmask(sz: Size, encoding: u16) -> Option<u64>`: `DecodeBitMasks`.
`None` for a reserved field (and for any `encoding >= 0x2000`). Bits of
`immr` above the element size are ignored, so several fields may decode to
one value; `encode_bitmask(sz, decode_bitmask(sz, e)?)` is the canonical one
among them.

Neither function allocates or uses a table.

`encode_fp_imm8(sz: FpSize, bits: u64) -> Option<u8>`: the `imm8` field of
an FMOV immediate for the floating-point constant with bit pattern `bits`.

- A pattern is encodable exactly when the architecture's `VFPExpandImm`
  produces it for some field: the values `±(16 + m) / 16 × 2^e` for
  `m` in `0..=15` and `e` in `-3..=4`, that is 256 values per precision,
  from ±0.125 to ±31.0. Zero, infinities, NaNs and denormals are not among
  them.
- The pattern is the IEEE 754 encoding at the precision `sz`. For
  `FpSize::S` it is in the low 32 bits and the upper 32 must be clear.
- A pattern has at most one field.

`decode_fp_imm8(sz: FpSize, imm8: u8) -> u64`: `VFPExpandImm`; for
`FpSize::S` the result is in the low 32 bits. Every field is valid.

## Arithmetic

Classes: Add/subtract (immediate), Add/subtract (shifted register),
Add/subtract (extended register), Add/subtract (with carry),
Data-processing (3 source), Data-processing (2 source).

`Z` is `RegZr`, `S` is `RegSp`. `sz` is `Size`.

| constructor | precondition | encodes |
|---|---|---|
| `add_imm(sz, rd: S, rn: S, imm: u64)` | `is_arith_imm(imm)` | ADD (immediate) |
| `sub_imm(sz, rd: S, rn: S, imm: u64)` | same | SUB (immediate) |
| `adds_imm(sz, rd: Z, rn: S, imm: u64)` | same | ADDS (immediate) |
| `subs_imm(sz, rd: Z, rn: S, imm: u64)` | same | SUBS (immediate) |
| `cmp_imm(sz, rn: S, imm: u64)` | same | CMP (immediate): SUBS with `rd` = ZR |
| `cmn_imm(sz, rn: S, imm: u64)` | same | CMN (immediate): ADDS with `rd` = ZR |
| `mov_sp(sz, rd: S, rn: S)` | | MOV (to/from SP): ADD (immediate) with `imm` = 0 |
| `add`, `adds`, `sub`, `subs` `(sz, rd: Z, rn: Z, rm: Z)` | | the (shifted register) form with `LSL #0` |
| `cmp`, `cmn` `(sz, rn: Z, rm: Z)` | | SUBS / ADDS (shifted register) with `rd` = ZR |
| `neg`, `negs` `(sz, rd: Z, rm: Z)` | | SUB / SUBS (shifted register) with `rn` = ZR |
| `add_shifted`, `adds_shifted`, `sub_shifted`, `subs_shifted` `(sz, rd: Z, rn: Z, rm: Z, shift: Shift, amount: u32)` | `shift != Ror` and `amount < sz.bits()` | ADD, ADDS, SUB, SUBS (shifted register) |
| `cmp_shifted`, `cmn_shifted` `(sz, rn: Z, rm: Z, shift, amount)` | same | with `rd` = ZR |
| `neg_shifted`, `negs_shifted` `(sz, rd: Z, rm: Z, shift, amount)` | same | with `rn` = ZR |
| `add_ext`, `sub_ext` `(sz, rd: S, rn: S, rm: Z, ext: Extend, amount: u32)` | `amount <= 4` | ADD, SUB (extended register) |
| `adds_ext`, `subs_ext` `(sz, rd: Z, rn: S, rm: Z, ext, amount)` | same | ADDS, SUBS (extended register) |
| `cmp_ext`, `cmn_ext` `(sz, rn: S, rm: Z, ext, amount)` | same | with `rd` = ZR |
| `adc`, `adcs`, `sbc`, `sbcs` `(sz, rd: Z, rn: Z, rm: Z)` | | ADC, ADCS, SBC, SBCS |
| `madd`, `msub` `(sz, rd: Z, rn: Z, rm: Z, ra: Z)` | | MADD (`ra + rn * rm`), MSUB (`ra - rn * rm`) |
| `mul`, `mneg` `(sz, rd: Z, rn: Z, rm: Z)` | | MADD / MSUB with `ra` = ZR |
| `smaddl`, `smsubl`, `umaddl`, `umsubl` `(rd: Z, rn: Z, rm: Z, ra: Z)` | | SMADDL, SMSUBL, UMADDL, UMSUBL (`Xd`, `Wn`, `Wm`, `Xa`) |
| `smull`, `umull` `(rd: Z, rn: Z, rm: Z)` | | SMADDL / UMADDL with `ra` = ZR |
| `smulh`, `umulh` `(rd: Z, rn: Z, rm: Z)` | | SMULH, UMULH |
| `udiv`, `sdiv` `(sz, rd: Z, rn: Z, rm: Z)` | | UDIV, SDIV |
| `crc32(kind: Crc32, rd: Z, rn: Z, rm: Z)` | `rm` is an `X` register for `X`, `Cx`, else `W` | CRC32B/H/W/X, CRC32CB/CH/CW/CX (FEAT_CRC32, WP-084) |

- The shifted-register forms cannot name SP. An add or subtract that reads
  or writes SP with a register operand is the extended-register form:
  `add_ext(.., Extend::Uxtx, 0)` at 64 bits, `Extend::Uxtw` at 32. (That is
  what an assembler emits for `add sp, sp, x1`.)
- In the extended-register forms `rm` is read as a `W` register unless `ext`
  is `Uxtx` or `Sxtx`. Every `Extend` is accepted at both sizes.
- The immediate is the value, not a field: `add_imm(.., 0x5000)` encodes
  `#5, lsl #12`. A negative amount is the caller's `sub_imm`.

## Logical

Classes: Logical (immediate), Logical (shifted register).

| constructor | precondition | encodes |
|---|---|---|
| `and_imm`, `orr_imm`, `eor_imm` `(sz, rd: S, rn: Z, imm: u64)` | `encode_bitmask(sz, imm).is_some()` | AND, ORR, EOR (immediate) |
| `ands_imm(sz, rd: Z, rn: Z, imm: u64)` | same | ANDS (immediate) |
| `tst_imm(sz, rn: Z, imm: u64)` | same | TST (immediate): ANDS with `rd` = ZR |
| `mov_bitmask(sz, rd: S, imm: u64)` | same | MOV (bitmask immediate): ORR (immediate) with `rn` = ZR |
| `and`, `bic`, `orr`, `orn`, `eor`, `eon`, `ands`, `bics` `(sz, rd: Z, rn: Z, rm: Z)` | | the (shifted register) form with `LSL #0` |
| `and_shifted`, `bic_shifted`, `orr_shifted`, `orn_shifted`, `eor_shifted`, `eon_shifted`, `ands_shifted`, `bics_shifted` `(sz, rd: Z, rn: Z, rm: Z, shift: Shift, amount: u32)` | `amount < sz.bits()` | AND, BIC, ORR, ORN, EOR, EON, ANDS, BICS (shifted register) |
| `mov(sz, rd: Z, rm: Z)` | | MOV (register): ORR (shifted register) with `rn` = ZR |
| `mvn(sz, rd: Z, rm: Z)`, `mvn_shifted(sz, rd: Z, rm: Z, shift, amount)` | (`amount < sz.bits()`) | MVN: ORN (shifted register) with `rn` = ZR |
| `tst(sz, rn: Z, rm: Z)`, `tst_shifted(sz, rn: Z, rm: Z, shift, amount)` | (`amount < sz.bits()`) | TST (shifted register): ANDS with `rd` = ZR |

- `mov` cannot name SP; `mov_sp` (Arithmetic) can.
- `mov_bitmask` is always the ORR form, including for a value MOVZ could
  express (an assembler's `mov` prefers MOVZ/MOVN there; `mov_imm` below
  makes that choice).

## Moves and constant materialization

Class: Move wide (immediate).

| constructor | precondition | encodes |
|---|---|---|
| `movz(sz, rd: Z, imm16: u16, shift: u32)` | `shift` is a multiple of 16 below `sz.bits()` | MOVZ (`rd = imm16 << shift`) |
| `movn(sz, rd: Z, imm16: u16, shift: u32)` | same | MOVN (`rd = !(imm16 << shift)`) |
| `movk(sz, rd: Z, imm16: u16, shift: u32)` | same | MOVK (replace 16 bits of `rd`) |

### `mov_imm`

```rust
pub const fn mov_imm(sz: Size, rd: Reg, value: u64) -> MovSeq;

pub struct MovSeq { /* [u32; 4] and a length */ }
impl MovSeq {
    pub const fn as_slice(&self) -> &[u32];   // 1..=4 words, in execution order
    pub const fn len(&self) -> usize;
}
```

`mov_imm` returns instructions that leave `value` in `rd` and change nothing
else. For `Size::W` the constant is the low 32 bits of `value`; the upper 32
bits are ignored.

**Guarantee.** A *materialization sequence* is one of MOVZ, MOVN or MOV
(bitmask immediate) writing `rd`, followed by zero or more MOVK on `rd`, all
at size `sz`. `mov_imm` returns a materialization sequence for the constant
that is no longer than any other materialization sequence for it. So the
length is at most 4 at `Size::X` and at most 2 at `Size::W`, and it is 1
exactly when a single MOVZ, MOVN or ORR (immediate) can express the
constant.

Among materialization sequences of that minimal length the result is fixed:
MOVZ-first if one exists, else MOVN-first, else ORR-first; MOVKs go from the
low chunk to the high one. The same inputs always give the same words.

The guarantee is about this family of sequences only. Sequences that use
other instructions (a second logical immediate, a bitfield copy of one half
onto the other) can be shorter for some constants and are not produced.

`rd` is a `Reg`: encoding 31 would be ZR for the wide moves and SP for ORR
(immediate), so neither is offered.

`mov_imm` does not allocate; `MovSeq` is `Copy`.

## Shifts and bitfields

Classes: Bitfield, Extract, Data-processing (2 source), Data-processing
(1 source).

| constructor | precondition | encodes |
|---|---|---|
| `ubfm`, `sbfm`, `bfm` `(sz, rd: Z, rn: Z, immr: u32, imms: u32)` | `immr < sz.bits()` and `imms < sz.bits()` | UBFM, SBFM, BFM |
| `lsl_imm(sz, rd: Z, rn: Z, amount: u32)` | `amount < sz.bits()` | LSL (immediate): UBFM `#(-amount mod bits), #(bits - 1 - amount)` |
| `lsr_imm(sz, rd: Z, rn: Z, amount: u32)` | same | LSR (immediate): UBFM `#amount, #(bits - 1)` |
| `asr_imm(sz, rd: Z, rn: Z, amount: u32)` | same | ASR (immediate): SBFM `#amount, #(bits - 1)` |
| `ubfx`, `sbfx`, `bfxil` `(sz, rd: Z, rn: Z, lsb: u32, width: u32)` | `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb` | UBFX, SBFX, BFXIL: UBFM, SBFM, BFM `#lsb, #(lsb + width - 1)` |
| `ubfiz`, `sbfiz`, `bfi` `(sz, rd: Z, rn: Z, lsb: u32, width: u32)` | same | UBFIZ, SBFIZ, BFI: UBFM, SBFM, BFM `#(-lsb mod bits), #(width - 1)` |
| `sxtb`, `sxth` `(sz, rd: Z, rn: Z)` | | SXTB, SXTH: SBFM `#0, #7` / `#0, #15` |
| `sxtw(rd: Z, rn: Z)` | | SXTW: 64-bit SBFM `#0, #31` |
| `uxtb`, `uxth` `(rd: Z, rn: Z)` | | UXTB, UXTH: 32-bit UBFM `#0, #7` / `#0, #15` |
| `extr(sz, rd: Z, rn: Z, rm: Z, lsb: u32)` | `lsb < sz.bits()` | EXTR |
| `ror_imm(sz, rd: Z, rs: Z, amount: u32)` | `amount < sz.bits()` | ROR (immediate): EXTR with `rn` = `rm` = `rs` |
| `lslv`, `lsrv`, `asrv`, `rorv` `(sz, rd: Z, rn: Z, rm: Z)` | | LSLV, LSRV, ASRV, RORV (the shift by a register amount) |
| `rbit`, `rev16`, `rev`, `clz`, `cls` `(sz, rd: Z, rn: Z)` | | RBIT, REV16, REV, CLZ, CLS |
| `rev32(rd: Z, rn: Z)` | | REV32 (64-bit only) |

`rev` reverses all the bytes of the register at either size (the encoding
differs between the sizes; the constructor picks it).

## Conditional

Classes: Conditional select, Conditional compare (register), Conditional
compare (immediate).

| constructor | precondition | encodes |
|---|---|---|
| `csel`, `csinc`, `csinv`, `csneg` `(sz, rd: Z, rn: Z, rm: Z, cond: Cond)` | | CSEL, CSINC, CSINV, CSNEG (`rd = cond ? rn : f(rm)`) |
| `cset`, `csetm` `(sz, rd: Z, cond)` | `!cond.is_always()` | CSET, CSETM: CSINC / CSINV of ZR, ZR with `cond.invert()` |
| `cinc`, `cinv` `(sz, rd: Z, rn: Reg, cond)` | same | CINC, CINV: CSINC / CSINV of `rn`, `rn` with `cond.invert()` |
| `cneg(sz, rd: Z, rn: Z, cond)` | same | CNEG: CSNEG of `rn`, `rn` with `cond.invert()` |
| `ccmp`, `ccmn` `(sz, rn: Z, rm: Z, nzcv: u32, cond)` | `nzcv < 16` | CCMP, CCMN (register) |
| `ccmp_imm`, `ccmn_imm` `(sz, rn: Z, imm: u32, nzcv: u32, cond)` | `imm < 32` and `nzcv < 16` | CCMP, CCMN (immediate) |

`cinc` and `cinv` take a `Reg` because with ZR they are CSET and CSETM.

## Branches and addresses

Classes: Unconditional branch (immediate), Conditional branch (immediate),
Compare and branch (immediate), Test and branch (immediate), Unconditional
branch (register), PC-rel. addressing.

A displacement is an `i64` byte distance from the instruction's own address
to the target (see "Fixups" for the range and unit of each field).

| constructor | precondition | encodes |
|---|---|---|
| `b(disp)`, `bl(disp)` | `Fixup::Jump26.fits(disp)` | B, BL |
| `b_cond(cond: Cond, disp)` | `Fixup::Branch19.fits(disp)` | B.cond |
| `cbz`, `cbnz` `(sz, rt: Z, disp)` | `Fixup::Branch19.fits(disp)` | CBZ, CBNZ |
| `tbz`, `tbnz` `(rt: Z, bit: u32, disp)` | `bit < 64` and `Fixup::Test14.fits(disp)` | TBZ, TBNZ |
| `br`, `blr`, `ret` `(rn: Z)` | | BR, BLR, RET |
| `adr(rd: Z, disp)` | `Fixup::Adr21.fits(disp)` | ADR |
| `adrp(rd: Z, page_disp)` | `Fixup::Adrp21.fits(page_disp)` | ADRP |

- `tbz`/`tbnz` take no size: `bit` selects it (`b5` is `bit >> 5`).
- `ret` always takes its register; `ret(Reg::LR.zr())` is the usual one.
- `adrp`'s `page_disp` is the distance between pages,
  `(target & !0xfff) - (pc & !0xfff)`, a multiple of 4096.
- The usual pattern is to encode with displacement 0 and patch later; 0
  fits every field.

## Loads and stores

Classes: Load/store register (unsigned immediate), (unscaled immediate),
(immediate pre-indexed), (immediate post-indexed), (register offset); Load
register (literal); Load/store register pair (offset), (pre-indexed),
(post-indexed).

What is loaded is a `Load`; what is stored is a `MemSize` (a store has no
extension, and `rt` is read at the access width).

| `Load` | instruction | | `Load` | instruction |
|---|---|---|---|---|
| `B` | LDRB `Wt` | | `SbW` | LDRSB `Wt` |
| `H` | LDRH `Wt` | | `ShW` | LDRSH `Wt` |
| `W` | LDR `Wt` | | `SbX` | LDRSB `Xt` |
| `X` | LDR `Xt` | | `ShX` | LDRSH `Xt` |
| | | | `SwX` | LDRSW `Xt` |

`Load::mem_size()` is the access width. Below, `width` is that width in
bytes, `rt: Z`, `rn: S` (the base), offsets are `i64` bytes.

| constructor | precondition | encodes |
|---|---|---|
| `ldr_off(op: Load, rt, rn, offset)`, `str_off(ms: MemSize, rt, rn, offset)` | `offset` is a multiple of `width` in `0..=4095 * width` | LDR*/STR* (immediate), unsigned offset |
| `ldur(op, rt, rn, offset)`, `stur(ms, rt, rn, offset)` | `-256 <= offset <= 255` | LDUR*/STUR* |
| `ldr_pre`, `ldr_post` `(op, rt, rn, offset)`; `str_pre`, `str_post` `(ms, rt, rn, offset)` | `-256 <= offset <= 255`, and `rt` is not the base register | LDR*/STR* (immediate), pre- and post-index |
| `ldr_reg(op, rt, rn, rm: Z, index: Index, scaled: bool)`, `str_reg(ms, rt, rn, rm: Z, index, scaled)` | | LDR*/STR* (register) |
| `ldr_lit(op: LitLoad, rt, disp)` | `Fixup::Literal19.fits(disp)` | LDR (literal) for `LitLoad::W`, `X`; LDRSW (literal) for `SwX` |
| `ldp_off(sz, rt, rt2: Z, rn, offset)` | `offset` is a multiple of the register width in `-64..=63` widths; `rt != rt2` | LDP, signed offset |
| `ldp_pre`, `ldp_post` `(sz, rt, rt2, rn, offset)` | as `ldp_off`, and neither register is the base | LDP, pre- and post-index |
| `ldpsw_off`, `ldpsw_pre`, `ldpsw_post` `(rt, rt2, rn, offset)` | as LDP with a 4-byte width | LDPSW |
| `stp_off(sz, rt, rt2, rn, offset)` | the offset rule of `ldp_off` | STP, signed offset |
| `stp_pre`, `stp_post` `(sz, rt, rt2, rn, offset)` | the offset rule, and neither register is the base | STP, pre- and post-index |

- "Is not the base register" compares register field values and is always
  true when the base is SP: `rt` = ZR and `rn` = SP are both 31 and are
  different registers. `rt != rt2` likewise compares field values, so two
  ZRs are rejected for a load pair.
- The constructors never pick between the scaled and the unscaled form: an
  offset `ldr_off` rejects (negative, misaligned) is the caller's `ldur`.
- `Index` is the extension of the index register: `Uxtw`, `Sxtw` (a `W`
  index), `Lsl`, `Sxtx` (an `X` index). With `scaled` the index is shifted
  left by log2 of the access width, otherwise not at all. For a 1-byte
  access `scaled` sets the `S` bit and shifts by 0; both values are valid
  encodings of the same operation, and `scaled = false` is the usual one.

## Atomics

Classes: Load/store exclusive register, Load/store exclusive pair,
Load/store ordered, Compare and swap, Compare and swap pair, Atomic memory
operations. The last three need FEAT_LSE.

`ms: MemSize` selects the `B`, `H`, 32-bit or 64-bit instruction; `rn: S`
is the address; there is no offset.

| constructor | precondition | encodes |
|---|---|---|
| `ldxr`, `ldaxr`, `ldar` `(ms, rt: Z, rn)` | | LDXR*, LDAXR*, LDAR* |
| `stlr(ms, rt: Z, rn)` | | STLR* |
| `stxr`, `stlxr` `(ms, rs: Z, rt: Z, rn)` | the status register `rs` is neither `rt` nor the base | STXR*, STLXR* |
| `ldxp`, `ldaxp` `(sz, rt: Z, rt2: Z, rn)` | `rt != rt2` | LDXP, LDAXP |
| `stxp`, `stlxp` `(sz, rs: Z, rt: Z, rt2: Z, rn)` | `rs` is none of `rt`, `rt2`, the base | STXP, STLXP |
| `clrex()` | | CLREX (`CRm` = 15) |
| `cas(ms, order: Order, rs: Z, rt: Z, rn)` | | CAS, CASA, CASL, CASAL and `B`/`H` forms |
| `casp(sz, order, rs: Reg, rt: Reg, rn)` | `rs` and `rt` are even-numbered | CASP, CASPA, CASPL, CASPAL |
| `swp(ms, order, rs: Z, rt: Z, rn)` | | SWP family |
| `ldadd`, `ldclr`, `ldeor`, `ldset`, `ldsmax`, `ldsmin`, `ldumax`, `ldumin` `(ms, order, rs: Z, rt: Z, rn)` | | LDADD, LDCLR, LDEOR, LDSET, LDSMAX, LDSMIN, LDUMAX, LDUMIN families |

- Register comparisons are on field values, with the base exempt when it is
  SP, as for loads and stores.
- `Order` is the memory ordering suffix: `Plain` (none), `A` (acquire), `L`
  (release), `Al` (both).
- `casp` names the first register of each pair; the second is the next one.
- In the atomic memory operations `rs` is the operand and `rt` receives the
  old value; `rt` = ZR is the `ST*` alias (STADD, ...).

## System

Classes: Hints, Barriers, System register move, Exception generation, and
the permanently undefined UDF.

| constructor | precondition | encodes |
|---|---|---|
| `nop()` | | NOP (`HINT #0`) |
| `hint(imm: u32)` | `imm < 128` | HINT |
| `bti_c()`, `bti_j()`, `bti_jc()` | | BTI c, j, jc (`HINT #34`, `#36`, `#38`) |
| `paciasp()`, `pacibsp()`, `autiasp()`, `autibsp()` | | PACIASP, PACIBSP, AUTIASP, AUTIBSP (`HINT #25`, `#27`, `#29`, `#31`) |
| `brk(imm16: u16)`, `svc(imm16: u16)` | | BRK, SVC |
| `udf(imm16: u16)` | | UDF |
| `mrs(rt: Z, reg: SysReg)`, `msr(reg: SysReg, rt: Z)` | | MRS, MSR (register) |
| `dmb(option: Barrier)`, `dsb(option: Barrier)` | | DMB, DSB |
| `isb()` | | ISB (`CRm` = 15) |

`SysReg` is `Nzcv`, `Fpcr`, `Fpsr`, `TpidrEl0`, and `CntvctEl0` (read
only: `msr` to it is not an instruction; WP-084). `Barrier` is the twelve
named options (`Sy`, `St`, `Ld`, `Ish`, `Ishst`, `Ishld`, `Nsh`, `Nshst`,
`Nshld`, `Osh`, `Oshst`, `Oshld`). The hint-space instructions execute as
NOP on a core without the feature, which is why they are encoded as hints.

## Scalar floating point

Classes: Floating-point data-processing (1 source), (2 source), (3 source);
Floating-point compare; Floating-point conditional compare; Floating-point
conditional select; Floating-point immediate; Conversion between
floating-point and integer.

`V` is `VReg`, `Z` is `RegZr`. `sz`/`fsz` is `FpSize` and selects the `S` or
`D` view of every `V` operand; a `Size` selects `W` or `X` for a
general-purpose operand.

| constructor | precondition | encodes |
|---|---|---|
| `fmov`, `fabs`, `fneg`, `fsqrt` `(sz, rd: V, rn: V)` | | FMOV (register), FABS, FNEG, FSQRT (scalar) |
| `frintn`, `frintp`, `frintm`, `frintz`, `frinta`, `frintx`, `frinti` `(sz, rd: V, rn: V)` | | FRINTN (nearest, ties to even), FRINTP (toward +inf), FRINTM (toward -inf), FRINTZ (toward zero), FRINTA (nearest, ties away), FRINTX (current mode, signalling inexact), FRINTI (current mode) |
| `fadd`, `fsub`, `fmul`, `fdiv`, `fnmul` `(sz, rd: V, rn: V, rm: V)` | | FADD, FSUB, FMUL, FDIV, FNMUL (scalar) |
| `fmax`, `fmin`, `fmaxnm`, `fminnm` `(sz, rd: V, rn: V, rm: V)` | | FMAX, FMIN (a NaN operand gives NaN), FMAXNM, FMINNM (a quiet NaN operand gives the other operand) |
| `fmadd`, `fmsub`, `fnmadd`, `fnmsub` `(sz, rd: V, rn: V, rm: V, ra: V)` | | FMADD (`ra + rn * rm`), FMSUB (`ra - rn * rm`), FNMADD (`-ra - rn * rm`), FNMSUB (`-ra + rn * rm`), each with one rounding |
| `fcmp`, `fcmpe` `(sz, rn: V, rm: V)` | | FCMP, FCMPE (FCMPE also signals on a quiet NaN) |
| `fcmp_zero`, `fcmpe_zero` `(sz, rn: V)` | | FCMP, FCMPE with `#0.0` |
| `fccmp`, `fccmpe` `(sz, rn: V, rm: V, nzcv: u32, cond: Cond)` | `nzcv < 16` | FCCMP, FCCMPE |
| `fcsel(sz, rd: V, rn: V, rm: V, cond: Cond)` | | FCSEL (`rd = cond ? rn : rm`) |
| `fcvt(to: FpFormat, rd: V, from: FpFormat, rn: V)` | `to != from` | FCVT between half, single and double |
| `fmov_imm(sz, rd: V, bits: u64)` | `encode_fp_imm8(sz, bits).is_some()` | FMOV (scalar, immediate) |
| `scvtf`, `ucvtf` `(fsz, rd: V, sz: Size, rn: Z)` | | SCVTF, UCVTF (scalar, integer): `W` or `X` to `S` or `D` |
| `fcvtzs`, `fcvtzu` `(sz: Size, rd: Z, fsz, rn: V)` | | FCVTZS, FCVTZU (scalar, integer): toward zero |
| `fcvtns`, `fcvtnu`, `fcvtas`, `fcvtau`, `fcvtps`, `fcvtpu`, `fcvtms`, `fcvtmu` `(sz: Size, rd: Z, fsz, rn: V)` | | the same conversion to nearest (ties to even), to nearest (ties away), toward +inf, toward -inf; `s` signed, `u` unsigned |
| `fmov_fp_to_gpr(sz: Size, rd: Z, rn: V)` | | FMOV (general): `Wd` from `Sn`, or `Xd` from `Dn`, bits unchanged |
| `fmov_gpr_to_fp(sz: Size, rd: V, rn: Z)` | | FMOV (general): `Sd` from `Wn`, or `Dd` from `Xn` |
| `fmov_hi_to_gpr(rd: Z, rn: V)` | | FMOV `Xd, Vn.D[1]`: the upper 64 bits |
| `fmov_gpr_to_hi(rd: V, rn: Z)` | | FMOV `Vd.D[1], Xn`: replaces the upper 64 bits, keeps the lower |

- Every conversion to an integer saturates and returns 0 for a NaN, as the
  architecture says; the constructor name is the rounding.
- `fmov_gpr_to_fp` with `rn` = ZR writes +0.0 and is what an assembler
  emits for `fmov d0, #0.0`. `fmov_const` uses `MOVI` for zero instead (see
  below).
- Half precision appears only where base floating point has it: `fcvt`
  (`FpFormat::H`), loads and stores of an `H` register, the constant
  materializer, and `fcvtl`/`fcvtn` between `4H` and `4S`. Arithmetic,
  compares, register moves and immediates on `H` registers are FEAT_FP16
  and are not provided: there is no `FpSize::H`. A half-precision value is
  moved as a single (`fmov(FpSize::S, ..)`, which copies bits 31:0) or
  through `fmov_fp_to_gpr`/`fmov_gpr_to_fp` at `Size::W`.

### `fmov_const`

```rust
pub const fn fmov_const(fmt: FpFormat, rd: VReg, scratch: Reg, bits: u64) -> FmovSeq;

pub struct FmovSeq { /* [u32; 5] and a length */ }
impl FmovSeq {
    pub const fn as_slice(&self) -> &[u32];   // 1..=5 words, in execution order
    pub const fn len(&self) -> usize;
}
```

`fmov_const` returns instructions that leave the floating-point constant
with bit pattern `bits` in the `fmt` view of `rd` and zero the rest of the
register. The constant is the low `fmt.bits()` bits of `bits`; the bits
above are ignored. The pattern is taken as given: a NaN keeps its payload
and its signalling bit, and no instruction of the result can raise a
floating-point exception.

**Algorithm.** The result is the first of these that applies:

1. the constant is all zero bits (+0.0): `MOVI <Dd>, #0`, one instruction;
2. `fmt` is `S` or `D` and the constant is an FMOV immediate
   (`encode_fp_imm8`): `FMOV <Sd|Dd>, #imm`, one instruction;
3. otherwise `mov_imm` of the pattern into `scratch` (at `Size::X` for `D`,
   at `Size::W` for `S` and `H`), then `FMOV <Dd>, <Xscratch>` or
   `FMOV <Sd>, <Wscratch>`: two to five instructions for `D`, two or three
   for `S` and `H`.

**Guarantee.** The result has one instruction exactly when the constant is
zero or an FMOV immediate at that format; otherwise its length is
`1 + mov_imm(..).len()`, with `mov_imm`'s own minimality guarantee. So the
general-purpose part is never longer than any other MOVZ/MOVN/ORR + MOVK
materialization. The same inputs always give the same words.

`scratch` is written only in case 3, where it is left holding the pattern
(zero-extended). The caller provides a register it can spare; it is a `Reg`
for the reason `mov_imm`'s destination is. Flags and memory are untouched in
every case.

The guarantee is about these three shapes only. Not produced: a literal-pool
load (which needs a pool and a fixup; `ldr_fp_lit` is there for a caller
that keeps one), the byte-mask and replicated forms of MOVI/MVNI for
constants other than zero, and FMOV immediate plus FNEG and similar
two-instruction FP-only sequences. Negative zero is case 3: two
instructions at every format.

Zero is `MOVI <Dd>, #0` at every format, not `FMOV <Sd>, WZR`: it has no
dependency on a general-purpose register and is the zeroing idiom cores
recognize. Half precision has no FMOV immediate without FEAT_FP16, so every
non-zero half constant is case 3.

`fmov_const` does not allocate; `FmovSeq` is `Copy`.

## FP/SIMD loads and stores

Classes: the `V` = 1 halves of Load/store register (unsigned immediate),
(unscaled immediate), (immediate pre-indexed), (immediate post-indexed),
(register offset); Load register (literal); Load/store register pair
(offset), (pre-indexed), (post-indexed).

`sz` is `VMemSize` (`B H S D Q`: the scalar view transferred, 1 to 16
bytes) or, for pairs and literals, `VPairSize` (`S D Q`). `width` is its
size in bytes; `rt: V`, `rn: S` (the base), offsets are `i64` bytes. A load
zeroes the rest of the register.

| constructor | precondition | encodes |
|---|---|---|
| `ldr_fp_off`, `str_fp_off` `(sz: VMemSize, rt, rn, offset)` | `offset` is a multiple of `width` in `0..=4095 * width` | LDR/STR (immediate, SIMD&FP), unsigned offset |
| `ldur_fp`, `stur_fp` `(sz: VMemSize, rt, rn, offset)` | `-256 <= offset <= 255` | LDUR/STUR (SIMD&FP) |
| `ldr_fp_pre`, `ldr_fp_post`, `str_fp_pre`, `str_fp_post` `(sz: VMemSize, rt, rn, offset)` | `-256 <= offset <= 255` | LDR/STR (immediate, SIMD&FP), pre- and post-index |
| `ldr_fp_reg`, `str_fp_reg` `(sz: VMemSize, rt, rn, rm: Z, index: Index, scaled: bool)` | | LDR/STR (register, SIMD&FP) |
| `ldr_fp_lit(sz: VPairSize, rt, disp)` | `Fixup::Literal19.fits(disp)` | LDR (literal, SIMD&FP) |
| `ldp_fp_off`, `ldp_fp_pre`, `ldp_fp_post` `(sz: VPairSize, rt, rt2: V, rn, offset)` | `offset` is a multiple of `width` in `-64..=63` widths; `rt != rt2` | LDP (SIMD&FP) |
| `stp_fp_off`, `stp_fp_pre`, `stp_fp_post` `(sz: VPairSize, rt, rt2: V, rn, offset)` | the offset rule | STP (SIMD&FP) |

- The names put `fp` after the mnemonic; otherwise the forms, the offset
  rules, `Index` and `scaled` are those of "Loads and stores". With
  `scaled` the index is shifted left by log2 of `width` (4 for `Q`).
- Writeback has no register precondition: the base is a general-purpose
  register and cannot be the register transferred.
- A load pair into one register twice is CONSTRAINED UNPREDICTABLE and is
  rejected; a store pair of one register twice is fine.
- The literal load's field is `Fixup::Literal19`, the same field as the
  integer literal load, so one fixup kind serves both. There is no literal
  load of a `B` or `H` register in the architecture.
- As for integers, the constructors never pick between the scaled and the
  unscaled form.

## SIMD

Classes: Advanced SIMD copy, scalar copy, modified immediate, three same,
two-register miscellaneous, across lanes, shift by immediate, permute,
extract, table lookup.

This is the subset a code generator needs to treat vectors as values: move
them, build and take them apart, and do each elementwise operation of the
IR in one instruction. It is not the whole of Advanced SIMD (see "Not
provided").

**Names.** A constructor is named by its mnemonic. Where the crate already
has a scalar constructor of that name, the vector one ends in `_vec`
(`add_vec`, `fadd_vec`, `neg_vec`, `rev16_vec`, ...); a compare with zero
ends in `_zero`. Everything is re-exported at the crate root, so names are
unique across the crate.

`V` is `VReg`, `Z` is `RegZr`; `arr` is `Arr` unless stated; `bits` is
`VecBits`, written `<8B|16B>` in assembly; `elem` is `Elem`.

### Moves and element access

| constructor | precondition | encodes |
|---|---|---|
| `mov_vec(bits, rd: V, rn: V)` | | MOV (vector): ORR (vector, register) with both sources `rn` |
| `dup_gpr(arr, rd: V, rn: Z)` | | DUP (general): `Wn` (`Xn` for `D2`) to every element |
| `dup_elem(arr, rd: V, rn: V, index: u32)` | `index < arr.elem().lanes()` | DUP (element), vector: element `index` of `rn` to every element |
| `dup_scalar(elem, rd: V, rn: V, index: u32)` | `index < elem.lanes()` | DUP (element), scalar (`MOV <Sd>, <Vn>.S[i]`): one element to the scalar view of `rd` |
| `ins_elem(elem, rd: V, rd_index: u32, rn: V, rn_index: u32)` | both indices `< elem.lanes()` | INS (element): `rd[rd_index] = rn[rn_index]` |
| `ins_gpr(elem, rd: V, index: u32, rn: Z)` | `index < elem.lanes()` | INS (general): `rd[index] = Wn` (`Xn` for `Elem::D`) |
| `umov(elem, rd: Z, rn: V, index: u32)` | `index < elem.lanes()` | UMOV: an element zero-extended to `Wd` (`Xd` for `Elem::D`) |
| `smov(sz: Size, elem, rd: Z, rn: V, index: u32)` | `elem.bits() < sz.bits()` and `index < elem.lanes()` | SMOV: an element sign-extended to `Wd` or `Xd` |

An element index counts from the low end of the 128-bit register and may
address any of its `elem.lanes()` elements, whatever the width of the
vector being built.

### Immediates

| constructor | precondition | encodes |
|---|---|---|
| `movi(arr, rd: V, imm8: u8, shift: u32)` | `shift` is 0 for byte elements; 0 or 8 for `H`; 0, 8, 16 or 24 for `S`; no `D2` | MOVI, 8-bit and shifted 16- and 32-bit forms: every element is `imm8 << shift` |
| `mvni(arr, rd: V, imm8: u8, shift: u32)` | as `movi`, and not byte elements | MVNI, shifted forms: every element is `!(imm8 << shift)` |
| `movi_mask(bits, rd: V, mask: u64)` | every byte of `mask` is `0x00` or `0xff` | MOVI, 64-bit forms: `MOVI <Dd>, #mask` for `V64`, `MOVI <Vd>.2D, #mask` for `V128` |
| `fmov_imm_vec(arr: FpArr, rd: V, bits: u64)` | `encode_fp_imm8(arr.fp_size(), bits).is_some()` | FMOV (vector, immediate): every element is the constant |

`movi_mask(bits, rd, 0)` clears a register. All ones is
`movi_mask(.., u64::MAX)`.

### Integer and bitwise

| constructor | precondition | encodes |
|---|---|---|
| `add_vec`, `sub_vec` `(arr, rd: V, rn: V, rm: V)` | | ADD, SUB (vector) |
| `mul_vec`, `smax`, `smin`, `umax`, `umin` `(arr, rd, rn, rm)` | `arr != D2` | MUL (vector), SMAX, SMIN, UMAX, UMIN |
| `cmeq`, `cmgt`, `cmge`, `cmhi`, `cmhs`, `cmtst` `(arr, rd, rn, rm)` | | CMEQ, CMGT, CMGE (signed), CMHI, CMHS (unsigned) (register), CMTST |
| `sshl`, `ushl` `(arr, rd, rn, rm)` | | SSHL, USHL: shift by the signed low byte of each element of `rm`; negative is a right shift |
| `abs`, `neg_vec` `(arr, rd: V, rn: V)` | | ABS, NEG (vector) |
| `cmeq_zero`, `cmgt_zero`, `cmge_zero`, `cmlt_zero`, `cmle_zero` `(arr, rd, rn)` | | CMEQ, CMGT, CMGE, CMLT, CMLE (zero), signed |
| `clz_vec`, `rev64`, `uaddlp` `(arr, rd, rn)` | `arr != D2` | CLZ (vector); REV64; UADDLP (`arr` is the source; the result has elements twice as wide) |
| `rev32_vec(arr, rd, rn)` | byte or 16-bit elements | REV32 (vector) |
| `rev16_vec`, `cnt`, `not`, `rbit_vec` `(bits, rd, rn)` | | REV16 (vector), CNT, NOT (`MVN`), RBIT (vector), all on bytes |
| `and_vec`, `bic_vec`, `orr_vec`, `orn_vec`, `eor_vec` `(bits, rd, rn, rm)` | | AND, BIC, ORR, ORN (vector, register), EOR (vector) |
| `bsl`, `bit`, `bif` `(bits, rd, rn, rm)` | | BSL (`rd = rd ? rn : rm`, bitwise), BIT (insert `rn` where `rm` is set), BIF (where `rm` is clear) |

- A compare sets each element to all ones where it holds and to zero where
  it does not. The missing directions are the same instruction with the
  operands exchanged: `a < b` is `cmgt(arr, rd, b, a)`, unsigned
  `a <= b` is `cmhs(arr, rd, b, a)`.
- A select is a compare, then `bsl` with the mask in `rd` (or `bit`/`bif`
  with the mask in `rm`).
- A shift by a variable amount is `sshl`/`ushl`; a variable right shift
  negates the amounts first (`neg_vec`).
- Population count of wider elements is `cnt` followed by `uaddlp` steps;
  of a whole register, `cnt` then `addv`/`uaddlv`.

### Shifts, widening, narrowing

| constructor | precondition | encodes |
|---|---|---|
| `shl(arr, rd: V, rn: V, amount: u32)` | `amount < arr.elem().bits()` | SHL |
| `sshr`, `ushr` `(arr, rd, rn, amount)` | `1 <= amount <= arr.elem().bits()` | SSHR, USHR |
| `sshll`, `ushll` `(from: Elem, rd, rn, amount)` | `from != D` and `amount < from.bits()` | SSHLL, USHLL: the lower half of `rn`, extended to elements twice as wide, shifted left |
| `sxtl`, `uxtl` `(from: Elem, rd, rn)` | `from != D` | SXTL, UXTL: SSHLL, USHLL by zero |
| `xtn(to: Elem, rd, rn)` | `to != D` | XTN: each element truncated to half its width, `to`; the upper half of `rd` is zeroed |

A right shift by zero is not an encoding (it is a move); a left shift by
the element width is not one either.

### Permutes, tables, reductions

| constructor | precondition | encodes |
|---|---|---|
| `uzp1`, `uzp2`, `trn1`, `trn2`, `zip1`, `zip2` `(arr, rd: V, rn: V, rm: V)` | | UZP1, UZP2, TRN1, TRN2, ZIP1, ZIP2 |
| `ext(bits, rd, rn, rm, index: u32)` | `index < bits.bits() / 8` | EXT: the bytes of `rm:rn` from byte `index` of `rn` up |
| `tbl1`, `tbl2` `(bits, rd, rn, rm)` | | TBL with a table of one register (`rn`) or two (`rn` and the next, `V0` after `V31`); `rm` holds the byte indices |
| `addv`, `smaxv`, `sminv`, `umaxv`, `uminv` `(arr, rd, rn)` | `arr` is `B8`, `B16`, `H4`, `H8` or `S4` | ADDV, SMAXV, SMINV, UMAXV, UMINV: the result is in the scalar view of the element width |
| `uaddlv(arr, rd, rn)` | same | UADDLV: the unsigned sum, in the scalar view twice the element width |

- An arbitrary shuffle of one or two vectors is `tbl1`/`tbl2` with a
  constant index vector; the fixed permutes and `ext` cover the common
  ones without a constant.
- The across-lanes instructions do not exist for two elements. The sum of
  a `2S` or `2D` vector is not provided as an instruction (see "Not
  provided"); it can be done with `umov`/`dup_scalar` and scalar adds.

### Floating point

`arr` is `FpArr` (`2S`, `4S`, `2D`).

| constructor | precondition | encodes |
|---|---|---|
| `fadd_vec`, `fsub_vec`, `fmul_vec`, `fdiv_vec` `(arr, rd: V, rn: V, rm: V)` | | FADD, FSUB, FMUL, FDIV (vector) |
| `fmax_vec`, `fmin_vec`, `fmaxnm_vec`, `fminnm_vec` `(arr, rd, rn, rm)` | | FMAX, FMIN, FMAXNM, FMINNM (vector) |
| `fmla`, `fmls` `(arr, rd, rn, rm)` | | FMLA (`rd + rn * rm`), FMLS (`rd - rn * rm`) (vector), one rounding |
| `fcmeq`, `fcmge`, `fcmgt` `(arr, rd, rn, rm)` | | FCMEQ, FCMGE, FCMGT (register); false when unordered |
| `fabs_vec`, `fneg_vec`, `fsqrt_vec` `(arr, rd: V, rn: V)` | | FABS, FNEG, FSQRT (vector) |
| `frintn_vec`, `frintm_vec`, `frintp_vec`, `frintz_vec`, `frinta_vec`, `frintx_vec`, `frinti_vec` `(arr, rd, rn)` | | FRINTN, FRINTM, FRINTP, FRINTZ, FRINTA, FRINTX, FRINTI (vector) |
| `fcmeq_zero`, `fcmgt_zero`, `fcmge_zero`, `fcmlt_zero`, `fcmle_zero` `(arr, rd, rn)` | | FCMEQ, FCMGT, FCMGE, FCMLT, FCMLE (zero) |
| `scvtf_vec`, `ucvtf_vec` `(arr, rd, rn)` | | SCVTF, UCVTF (vector, integer): integer elements to floating point of the same width |
| `fcvtzs_vec`, `fcvtzu_vec` `(arr, rd, rn)` | | FCVTZS, FCVTZU (vector, integer): toward zero, saturating |
| `fcvtl(wide: FpSize, rd, rn)` | | FCVTL: the lower half of `rn` to the next wider format, `wide` (`4H` to `4S`, or `2S` to `2D`) |
| `fcvtn(wide: FpSize, rd, rn)` | | FCVTN: each element of format `wide` to the next narrower (`4S` to `4H`, or `2D` to `2S`); the upper half of `rd` is zeroed |

As for the integer compares, `a < b` is `fcmgt(arr, rd, b, a)`. An
"unordered or" predicate is the complement (`not`) of the opposite ordered
one.

## Fixups

A code generator emits a PC-relative instruction before it knows the
distance, and patches it when it does. `Fixup` names the field:

| `Fixup` | field | instructions | unit | range of the displacement |
|---|---|---|---|---|
| `Jump26` | `imm26`, bits 25:0 | B, BL | 4 | `-(1 << 27) ..= (1 << 27) - 4` |
| `Branch19` | `imm19`, bits 23:5 | B.cond, CBZ, CBNZ | 4 | `-(1 << 20) ..= (1 << 20) - 4` |
| `Test14` | `imm14`, bits 18:5 | TBZ, TBNZ | 4 | `-(1 << 15) ..= (1 << 15) - 4` |
| `Adr21` | `immlo` bits 30:29, `immhi` bits 23:5 | ADR | 1 | `-(1 << 20) ..= (1 << 20) - 1` |
| `Adrp21` | the same bits | ADRP | 4096 | `-(1 << 32) ..= (1 << 32) - 4096` |
| `Literal19` | `imm19`, bits 23:5 | LDR (literal), LDRSW (literal), LDR (literal, SIMD&FP) | 4 | `-(1 << 20) ..= (1 << 20) - 4` |

The displacement is `target - pc` in bytes, where `pc` is the address of the
instruction holding the field; for `Adrp21` it is the page distance
`(target & !0xfff) - (pc & !0xfff)`.

```rust
impl Fixup {
    pub const ALL: [Fixup; 6];
    pub const fn mask(self) -> u32;                       // the field's bits
    pub const fn min(self) -> i64;
    pub const fn max(self) -> i64;
    pub const fn fits(self, disp: i64) -> bool;           // in range and a multiple of the unit
    pub const fn patch(self, word: u32, disp: i64) -> Option<u32>;
    pub const fn extract(self, word: u32) -> i64;
}
```

- `patch` returns `None` exactly when `!fits(disp)`. Otherwise it returns
  `word` with the field replaced and every bit outside `mask()` unchanged.
  It does not look at the opcode: applying a kind to the wrong instruction
  is the caller's error.
- `extract` is the inverse: `kind.extract(kind.patch(w, d)?) == d`, and
  `kind.patch(w, kind.extract(w)) == Some(w)` for every word.
- Each constructor with a displacement operand is its zero-displacement
  word patched: `b(d) == Fixup::Jump26.patch(b(0), d)?`, and so on. Its
  precondition is the field's `fits`.

Relocations against symbols (`R_AARCH64_*`, including the `:lo12:` low-bits
forms) are the object writer's business and are not fixups in this sense.

## Not provided

Base integer instructions the crate does not encode, by decision of this
WP's scope: the CRC32 family, the pointer-authentication instructions other
than the four hint-space ones, `MSR` (immediate) and the `SYS`/`SYSL`
aliases, system registers other than the four listed, `HVC`/`SMC`/`HLT` and
the debug exceptions, non-temporal pairs (`LDNP`/`STNP`), unprivileged
loads and stores, prefetch (`PRFM`), `LDAPR`, the load/store-acquire
"LO" forms, the `ST*` atomic aliases as separate names, `NGC`/`NGCS`,
`SMNEGL`/`UMNEGL`, `BFC`, and everything from architecture extensions after
Armv8.1 other than BTI. Adding one is a constructor, a row here, a row in
`tests/l1/table.rs` and a sweep.

Floating point and SIMD the crate does not encode. SVE, SME, the
cryptographic and dot-product extensions, and the structure loads and
stores (`LD1`..`LD4`, `ST1`..`ST4`, `LD1R`) are outside this crate's scope
so far. The rest is listed with where it would go; each is an entry next to
an existing one, using the helper named:

| not provided | follow-up |
|---|---|
| Half-precision scalar arithmetic, compares, `FMOV` to, from and between `H` registers, `FMOV <Hd>, #imm`, conversions between `H` and integers (all FEAT_FP16) | an `H` member of `FpSize`, `crates/hod_mc_a64/src/reg.rs:353` |
| Half-precision vector elements (`4H`, `8H` floating point; FEAT_FP16) | `FpArr`, `crates/hod_mc_a64/src/reg.rs:550` |
| Fixed-point conversions (`SCVTF`/`UCVTF`/`FCVTZS`/`FCVTZU` with `#fbits`), `FJCVTZS` | `fp_int`, `crates/hod_mc_a64/src/fp.rs:58` |
| `FRINT32X`/`FRINT32Z`/`FRINT64X`/`FRINT64Z` (Armv8.5) | `fp_data1`, `crates/hod_mc_a64/src/fp.rs:13` |
| Non-temporal FP pairs (`LDNP`/`STNP`) | `vpair`, `crates/hod_mc_a64/src/fpmem.rs:115` |
| Scalar forms of the SIMD instructions (`ADD <Dd>`, `CMEQ <Dd>`, `SSHL <Dd>`, `SHL <Dd>`, `FCMEQ <Sd>`, `FABD <Sd>`, ...) | `three_same`, `crates/hod_mc_a64/src/simd.rs:15` |
| `MLA`, `MLS`, `ADDP`, `SABD`/`UABD`, `PMUL`, the saturating and halving arithmetic (`SQADD`, `UQSUB`, `SHADD`, `URHADD`, ...), `SQDMULH`, the rounding and saturating register shifts | the `int3` family, `crates/hod_mc_a64/src/simd.rs:21` |
| `CLS`, `SQABS`, `SQNEG`, `SADDLP`, `SADALP`/`UADALP`, `URECPE`/`URSQRTE` | the `int2` family, `crates/hod_mc_a64/src/simd.rs:45` |
| `FABD`, `FMULX`, `FACGE`/`FACGT`, `FRECPS`/`FRSQRTS`, the pairwise `FADDP`/`FMAXP`/`FMINP` | the `fp3` family, `crates/hod_mc_a64/src/simd.rs:33` |
| Vector conversions in the other rounding modes (`FCVTNS`, `FCVTAS`, `FCVTMS`, `FCVTPS` and unsigned), vector fixed-point conversions, `FRECPE`/`FRSQRTE`, `FCVTXN` | the `fp2` family, `crates/hod_mc_a64/src/simd.rs:57` |
| `SADDLV`; the floating-point reductions `FMAXV`/`FMINV`/`FMAXNMV`/`FMINNMV`; the two-element reductions, which are the scalar pairwise `ADDP <Dd>, <Vn>.2D` and `FADDP`/`FMAXP`/`FMINP <Sd|Dd>, <Vn>.<2S|2D>` | `across`, `crates/hod_mc_a64/src/simd.rs:63` |
| `MOVI`/`MVNI` with a shifting-ones (`MSL`) immediate, `ORR`/`BIC` (vector, immediate) | `modified_imm`, `crates/hod_mc_a64/src/simd.rs:88` |
| The other shifts by immediate: rounding and accumulating (`SRSHR`, `URSHR`, `SSRA`, `USRA`, ...), inserting (`SLI`, `SRI`), narrowing (`SHRN`, `RSHRN`, `SQSHRN`, ...), saturating (`SQSHL`, `UQSHL`, `SQSHLU`) | `shift_imm`, `crates/hod_mc_a64/src/simd.rs:94` |
| The upper-half forms `SSHLL2`, `USHLL2`, `SXTL2`, `UXTL2`, `XTN2`, `FCVTL2`, `FCVTN2`, and `SHLL` | `shll_fields`, `crates/hod_mc_a64/src/simd.rs:115` |
| The saturating narrows `SQXTN`, `UQXTN`, `SQXTUN` | `two_misc`, `crates/hod_mc_a64/src/simd.rs:39` |
| `TBX`, and `TBL` with a table of three or four registers | `table`, `crates/hod_mc_a64/src/simd.rs:100` |
| The widening and narrowing three-register arithmetic (`SMULL`, `UMULL`, `SADDL`, `UADDW`, `ADDHN`, ...) and every by-element form (`MUL`, `FMUL`, `FMLA` ... with an indexed operand): two encoding classes with no helper yet | the head of `crates/hod_mc_a64/src/simd.rs:1` |

The upper-half forms are a shorter way to say something the crate can
already say: to widen the upper half of a vector, bring it down with `ext`
and use the lower-half form; to narrow into an upper half, narrow and then
`ins_elem` the `D` element.

## Test method

The tests are `crates/hod_mc_a64/tests/l1/` (L1 of `docs/VALIDATION.md`). The
oracle is LLVM 22's assembler and disassembler, target
`aarch64-unknown-linux-gnu` with `+lse,+bti,+pauth,+fp-armv8,+neon`. Without
`+fullfp16`, half-precision arithmetic is an error to the oracle, as it is
absent from the crate. The tests that need the tools skip without them
unless `HOD_REQUIRE_TOOLS=1`.

**Sweeps.** One module per section above builds a batch of cases. A case is
a constructor call and the assembly text for the same instruction, written
by formatters that share nothing with the encoder (`tests/l1/text.rs`). Each
sweep covers, for every constructor: every register number in every
position, including 31 under both of its names; both sizes; every condition,
shift, extension, access width and ordering; the boundary and interior
values of every immediate; and, for a checked form, operands just outside
each bound and misaligned ones, which must return `None`. The FP and SIMD
sweeps do the same over every `VReg` in every position, every precision,
format, arrangement and element width, every element index, and every
shift amount.

**Checks.** For each batch (`*_match_llvm`, `*_matches_llvm`):

1. *Forward.* All the assembly lines are assembled by one `llvm-mc` run; the
   `.text` bytes, extracted with `llvm-objcopy`, must equal hod's words one
   for one. A mismatch is reported with its assembly line.
2. *Reverse.* hod's words are disassembled by `llvm-objdump -M no-aliases`;
   the mnemonic printed for each must be the instruction the table above
   says the constructor encodes (`tests/l1/table.rs`). LLVM prints `mov`
   and the bitfield aliases whatever the alias setting; the table lists, for
   those constructors, each name its words can print as.
3. *Rejections.* Where a rejected operand combination has assembly text that
   LLVM does not silently turn into a different instruction, `llvm-mc` must
   report an error on that line.

**Coverage.** `coverage::every_constructor_is_swept` reads the crate's
source for its public constructors and fails if one has no row in the
table, no case in any sweep, or (for a checked form) no rejected operands,
or if the sweeps together fall below 150,000 encodings. It recognizes a
constructor by how the source writes one: `pub const fn` at the start of a
line, or `pub fn` indented once, which is an entry of the `checked!` or
`family!` macro (with a checked form when ` / try_name` follows the name).
It also fails if a name is defined twice.

**The harness itself.** `selftest` gives the harness batches that disagree
with LLVM (a wrong word, a wrong instruction, a wrong family member, text
LLVM refuses, a rejection LLVM accepts) and requires each to be reported.

**Without tools.**

- Logical immediates, `bitmask`: all 5334 64-bit and 1302 32-bit patterns
  (generated from the definition, not from the encoder) encode, decode back
  through the crate's decoder and through an independent transcription of
  `DecodeBitMasks`, and are canonical; all 8192 fields decode as the
  reference says; values that are not patterns (every replicated 16-bit
  value, every pattern with one bit flipped, a random sample, the corner
  cases) are rejected.
- The materializer, `movimm`: for a sample of constants weighted toward the
  shapes each strategy handles, an interpreter of MOVZ/MOVN/MOVK/ORR runs
  the sequence and must get the constant; the length must equal the minimum
  found by exhaustive search over every logical immediate; the tie-break
  must be the specified one. With tools, LLVM's `mov Rd, #imm` must accept
  exactly the constants `mov_imm` does in one instruction, and produce the
  same word.
- FMOV immediates, `fpimm`: all 256 fields at each precision decode to the
  value the definition gives (computed in floating point, not by bit
  manipulation) and encode back; nothing else encodes: every pattern whose
  low fraction bits are zero is tried (8192 single, 65536 double; exactly
  256 of each encode), every encodable value with one more fraction bit
  set, 100,000 random patterns per precision, and single patterns with bits
  above bit 31.
- The FP materializer, `fpimm`: for about 20,000 patterns per format (zeros,
  extremes, infinities, quiet and signalling NaNs with payloads, denormals,
  everyday constants, a seeded random sample, the 256 immediates) and for
  every one of the 65536 half-precision patterns, a model that shares
  nothing with the encoder (it recognizes `MOVI <Dd>, #0`, FMOV immediate
  and FMOV from a general-purpose register by their bits, evaluates the
  immediate from the definition, and runs the general-purpose prefix
  through the `movimm` interpreter) must get the constant; the sequence must
  have the specified shape and length and touch the scratch register only
  in case 3; bits above the format's width must not matter. With tools,
  group `fp-constant-materialization`: every one-instruction result and
  both instructions of every two-instruction result equal LLVM's assembly
  of the same text, so LLVM's `fmov <reg>, #value` and `mov <reg>, #imm`
  agree about which constants take one instruction; and LLVM's `fmov`
  accepts exactly the values `try_fmov_imm` does, over 1000 random
  single-precision values and every immediate with each one of its 32 bits
  flipped, at both precisions.
- Fixups, `fixups`: ranges and masks against the table above; `patch` and
  `extract` are inverses and touch only the field, for every field value of
  the short fields and at the boundaries of all; out-of-range and
  misaligned displacements return `None`. With tools: words patched for a
  given distance equal what LLVM resolves for a label at that distance, at
  the boundaries; and a function laid out with forward and backward labels
  and padding, encoded with zero displacements and patched, equals LLVM's
  assembly of the same source.

### Where hod and LLVM's assembler differ

hod is stricter in three places, so these rejections are tested by hod
alone. The first two are operand combinations the Arm ARM makes CONSTRAINED
UNPREDICTABLE and LLVM 22 assembles without a diagnostic:

- `LDXP`/`LDAXP` with `rt == rt2`;
- post-indexed `LDPSW` whose base is one of the loaded registers (LLVM
  diagnoses the pre-indexed form, and both forms of LDP, including the
  FP/SIMD ones);
- `EXT` with an index beyond the vector: LLVM accepts any index and keeps
  its low bits (`ext v0.8b, v1.8b, v2.8b, #9` assembles as `#1`). hod
  rejects `index >= 8` (or 16).

In the other direction, LLVM's assembler rewrites some operands hod's
constructors reject into a different instruction (a negative ADD immediate
into SUB, a negative or misaligned scaled offset into LDUR/STUR). hod leaves
that choice to the caller; those rejections have no assembly text to
confirm them and are likewise tested by hod alone.

Floating-point constants:

- LLVM's assembler reads the operand of `fmov <reg>, #value` as a decimal
  number and rounds it to single precision before asking whether it is an
  immediate, at both precisions: `fmov d1, #1.0000000000000002` assembles
  as 1.0. hod takes the bit pattern and is exact: that double is not an
  immediate. The tests only ask LLVM about values that are exact in single
  precision.
- `fmov d0, #0.0` is accepted by LLVM as an alias of `fmov d0, xzr`; it is
  not an FMOV immediate and `try_fmov_imm` rejects zero. `#-0.0` is an
  error to LLVM too.
- For a constant that is neither zero nor an FMOV immediate, LLVM's code
  generator (`llc -O0`) emits a literal-pool load (`adrp` + `ldr d0,
  [x8, :lo12:..]`) for most doubles and `mov w8, #..; fmov s0, w8` for
  singles. `fmov_const` never uses a pool; it produces the second shape at
  every format. For zero both produce `movi d0, #0`.
