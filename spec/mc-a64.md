# `hod_mc_a64`: AArch64 instruction encoding

`crates/hod_mc_a64` turns an instruction and its operands into the 32-bit
instruction word. It knows nothing about the IR, object files or assembly
text. Its consumer is a single-pass code generator that calls it once per
emitted instruction, so the whole API is plain functions on small `Copy`
values.

This document covers the base integer instruction set. Floating-point and
SIMD (the `fp` and `simd` modules, empty today) are WP-013 and will extend
it.

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

`SysReg` is `Nzcv`, `Fpcr`, `Fpsr`, `TpidrEl0`. `Barrier` is the twelve
named options (`Sy`, `St`, `Ld`, `Ish`, `Ishst`, `Ishld`, `Nsh`, `Nshst`,
`Nshld`, `Osh`, `Oshst`, `Oshld`). The hint-space instructions execute as
NOP on a core without the feature, which is why they are encoded as hints.

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
| `Literal19` | `imm19`, bits 23:5 | LDR (literal), LDRSW (literal) | 4 | `-(1 << 20) ..= (1 << 20) - 4` |

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

## Test method

The tests are `crates/hod_mc_a64/tests/l1/` (L1 of `docs/VALIDATION.md`). The
oracle is LLVM 22's assembler and disassembler, target
`aarch64-unknown-linux-gnu` with `+lse,+bti,+pauth`. The tests that need the
tools skip without them unless `HOD_REQUIRE_TOOLS=1`.

**Sweeps.** One module per section above builds a batch of cases. A case is
a constructor call and the assembly text for the same instruction, written
by formatters that share nothing with the encoder (`tests/l1/text.rs`). Each
sweep covers, for every constructor: every register number in every
position, including 31 under both of its names; both sizes; every condition,
shift, extension, access width and ordering; the boundary and interior
values of every immediate; and, for a checked form, operands just outside
each bound and misaligned ones, which must return `None`.

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
or if the sweeps together fall below 30,000 encodings.

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
- Fixups, `fixups`: ranges and masks against the table above; `patch` and
  `extract` are inverses and touch only the field, for every field value of
  the short fields and at the boundaries of all; out-of-range and
  misaligned displacements return `None`. With tools: words patched for a
  given distance equal what LLVM resolves for a label at that distance, at
  the boundaries; and a function laid out with forward and backward labels
  and padding, encoded with zero displacements and patched, equals LLVM's
  assembly of the same source.

### Where hod and LLVM's assembler differ

hod is stricter in two places. Each is an operand combination the Arm ARM
makes CONSTRAINED UNPREDICTABLE and LLVM 22 assembles without a diagnostic,
so these rejections are tested by hod alone:

- `LDXP`/`LDAXP` with `rt == rt2`;
- post-indexed `LDPSW` whose base is one of the loaded registers (LLVM
  diagnoses the pre-indexed form, and both forms of LDP).

In the other direction, LLVM's assembler rewrites some operands hod's
constructors reject into a different instruction (a negative ADD immediate
into SUB, a negative or misaligned scaled offset into LDUR/STUR). hod leaves
that choice to the caller; those rejections have no assembly text to
confirm them and are likewise tested by hod alone.
