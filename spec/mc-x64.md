# `hod_mc_x64`: x86-64 instruction encoding

`crates/hod_mc_x64` turns an instruction and its operands into its bytes.
It knows nothing about the IR, object files or assembly text. Its consumer
is a single-pass code generator that calls it once per emitted instruction,
so the whole API is plain functions on small `Copy` values.

This document covers the base integer instruction set an `-O0` code
generator needs, in 64-bit mode. Scalar SSE floating point and vectors are
a later work package. The oracle and the crate assume the x86-64 baseline
plus the instructions named below that need a CPUID feature (`LZCNT`,
`TZCNT` (BMI1), `POPCNT`, `CMPXCHG16B`, `ENDBR64`); the crate does not check
features, the code generator does.

Instructions are cited by their Intel SDM names (Intel 64 and IA-32
Architectures Software Developer's Manual, Volume 2, "Instruction Set
Reference"), with the opcode column's notation: `/r` a ModRM byte whose
reg field is a register, `/digit` an opcode extension in the reg field,
`+r` a register in the opcode's low three bits, `ib`/`iw`/`id`/`io` an
immediate of 1, 2, 4 or 8 bytes, `cb`/`cd` a relative displacement.

## Shape of the API

- A **constructor** is an `#[inline] pub const fn` that returns an
  `Inst`: up to 15 bytes, their count, and a record of the instruction's
  PC-relative field ("Fixups and relocations"). `inst.bytes()` is the
  instruction to append to the section.
- A constructor allocates nothing, reads no state and writes none. There
  are no traits, trait objects or builders.
- Everything is re-exported from the crate root, so `hod_mc_x64::add_rr`
  and `hod_mc_x64::alu::add_rr` are the same function.
- An operand that selects among encodings of one instruction (operand size,
  condition, source size of an extension) is an enum operand, not a
  separate function. Where the operand *forms* of one SDM instruction
  differ in kind (register or memory, register or immediate), each form is
  its own constructor, named by a suffix that lists the operands in Intel
  order, as LLVM names its instruction records:

  | suffix | operands |
  |---|---|
  | `_rr` | register, register |
  | `_rm` | register destination, memory source |
  | `_mr` | memory destination, register source |
  | `_ri` / `_mi` | register / memory, immediate |
  | `_rcl` / `_mcl` | register / memory, shift count in `cl` |
  | `_rri`, `_rrcl`, `_mri`, `_mrcl`, `_rmi` | three operands, in order |
  | `_r` / `_m` | one register / one memory operand |

  A constructor with no suffix has one form.

### Checked and unchecked forms

A constructor whose operands are all valid by their types is infallible and
has one form.

A constructor `f` whose operands have a **precondition** (an immediate
that may not fit, a size the instruction does not exist in) has a twin
`try_f` with the same parameters:

- `try_f(..)` returns `Some(inst)` when the precondition holds and `None`
  exactly when it does not. It never panics.
- `f(..)` returns the same `inst` when the precondition holds. It checks
  the precondition with `debug_assert!` (message
  `"<f>: operand precondition violated"`); with debug assertions off, a
  violated precondition yields unspecified bytes. It is the form for a
  caller that has already established the precondition.

Both forms are generated from one statement of the precondition (the
`checked!` macro in `src/lib.rs`). In the tables below, a constructor has a
`try_` twin exactly when its "precondition" cell is not empty.

## Registers and operand kinds

| type | values | use |
|---|---|---|
| `Reg` | `Rax`, `Rcx`, `Rdx`, `Rbx`, `Rsp`, `Rbp`, `Rsi`, `Rdi`, `R8`..`R15` (numbers 0..15; `Reg::new(n)` is `None` for `n > 15`; `.code()` the number) | every register operand |
| `Size` | `B` (8), `W` (16), `D` (32), `Q` (64) | the operand size: which view of a register, how wide a memory access |
| `Index` | a `Reg` other than `rsp`: `reg.index()` is `None` for `Rsp`; `.reg()` goes back | a SIB index |
| `Scale` | `S1`, `S2`, `S4`, `S8` | the SIB scale |
| `Seg` | `Fs`, `Gs` | a segment override (`64`, `65`) of a memory operand |
| `Cond` | `O`, `No`, `B`, `Ae`, `E`, `Ne`, `Be`, `A`, `S`, `Ns`, `P`, `Np`, `L`, `Ge`, `Le`, `G` (the `cc` field 0..15, in that order; `.invert()` flips bit 0) | `Jcc`, `SETcc`, `CMOVcc` |

The size is not part of a register: `Reg::Rsi` with `Size::B` is `sil`,
with `Size::W` `si`. The 8-bit registers are `al`, `cl`, `dl`, `bl`, `spl`,
`bpl`, `sil`, `dil`, `r8b`..`r15b`; the high-byte registers `ah`, `ch`,
`dh`, `bh` are not provided ("Not provided"). A 32-bit operation zeroes
bits 63:32 of its destination register; 8- and 16-bit operations leave the
rest of it unchanged.

## Memory operands

A `Mem` is `seg:[base + index * scale + disp]` with a signed 32-bit `disp`.
It is built by one of:

| constructor | operand |
|---|---|
| `Mem::base(base: Reg, disp: i32)` | `[base + disp]` |
| `Mem::base_index(base: Reg, index: Index, scale: Scale, disp: i32)` | `[base + index*scale + disp]` |
| `Mem::index(index: Index, scale: Scale, disp: i32)` | `[index*scale + disp]`, no base |
| `Mem::abs(disp: i32)` | `[disp]`: the sign-extended absolute address (not RIP-relative) |
| `Mem::rip(disp: i32)` | `[rip + disp]`: `disp` from the end of the instruction |
| `m.with_seg(seg: Seg)` | the same operand with a segment override |

`get_base()` (a `Base`: `None`, `Reg(Reg)`, `Rip`), `get_index()`, `disp()`
and `seg()` read the parts back. Every value of `Mem` has an encoding:
`rsp` is not an `Index` and a RIP-relative operand has no index, by
construction. A segment override on `lea`'s operand is encoded and has no
effect.

**Encoding** (ModRM `mod`, `rm` and the SIB byte; SDM Vol. 2, 2.1.5 and
2.2.1):

- RIP-relative: `mod` 00, `rm` 101, `disp32`.
- No base: `mod` 00, `rm` 100, SIB with base 101, `disp32`; the SIB index
  is 100 ("none") for `Mem::abs`. `[index*1 + disp]` stays in this form; it
  is not rewritten as `[index + disp]`.
- A base: the shortest displacement: none when `disp == 0` and the base is
  not `rbp` or `r13` (whose low bits 101 mean "no base" or "RIP" at `mod`
  00), else `disp8` when `disp` is in `-128..=127`, else `disp32`. A SIB
  byte is present when there is an index or the base is `rsp` or `r12`
  (low bits 100), with index 100 in the second case. Base and index are
  never swapped.

This is exactly LLVM's choice for the same operand.

## Instruction layout

An instruction is, in this order: the segment override; `66` (16-bit
operand size); the lock prefix or the mandatory `F2`/`F3` prefix; REX; the
opcode (`0F` first for the two-byte map); ModRM; SIB; displacement;
immediate. This is the order LLVM emits (it puts `66` before `F0` and
before a mandatory `F3`).

REX is `0100WRXB`: `W` for a 64-bit operand size where the instruction's
default is not 64 bits; `R`, `X`, `B` the high bit of the reg field,
the SIB index and the base (or `rm`, or the `+r` register). A REX prefix
is emitted when any bit is set, and also, as `40`, when an 8-bit register
operand is `spl`, `bpl`, `sil` or `dil`, which exist only with REX.
`inst.has_rex()` says whether the instruction has one.

Instructions whose default operand size is 64 bits (`push`, `pop`, `jmp`
and `call` through a register or memory, `leave`) take no size and no
`REX.W`.

## Immediates

`Size::fits_imm(size, imm: i64)` is the precondition of every
immediate-taking constructor that has a size, except `mov_ri`:

| size | accepted `imm` |
|---|---|
| `B` | `-128..=255` |
| `W` | `-32768..=65535` |
| `D` | `-(1 << 31)..=(1 << 32) - 1` |
| `Q` | `-(1 << 31)..=(1 << 31) - 1`: a 64-bit instruction sign-extends an `imm32` |

An 8-, 16- or 32-bit immediate is accepted as a signed or an unsigned value
of the size's width; the field holds its low `size` bits, so `-1` and
`0xffff_ffff` are the same 32-bit immediate.

Where an instruction has a sign-extended `imm8` form (`83`, `6B`) it is
used when the immediate, truncated to the operand size, is the sign
extension of its low byte; there is none for 8-bit operands. Otherwise the
immediate has the operand size's width (`imm32` for 64 bits).

## Arithmetic and logic

The eight two-operand ALU instructions share one encoding scheme: `n` is
ADD 0, OR 1, ADC 2, SBB 3, AND 4, SUB 5, XOR 6, CMP 7. In each row `op` is
one of `add`, `or`, `adc`, `sbb`, `and`, `sub`, `xor`, `cmp`.

| constructor | encodes | precondition |
|---|---|---|
| `op_rr(size, dst: Reg, src: Reg)` | `op r/m, r` (`8n+0`/`8n+1 /r`), `dst` in `rm` | |
| `op_rm(size, dst: Reg, src: Mem)` | `op r, r/m` (`8n+2`/`8n+3 /r`) | |
| `op_mr(size, dst: Mem, src: Reg)` | `op r/m, r` (`8n+0`/`8n+1 /r`); lockable except `cmp` | |
| `op_ri(size, dst: Reg, imm: i64)` | `83 /n ib` if `imm` is a sign-extended byte (not for `B`); else, for `al`/`ax`/`eax`/`rax`, `8n+4 ib` / `8n+5 iw/id`; else `80 /n ib` / `81 /n iw/id` | `size.fits_imm(imm)` |
| `op_mi(size, dst: Mem, imm: i64)` | `83 /n ib` if a sign-extended byte (not for `B`), else `80 /n ib` / `81 /n iw/id`; lockable except `cmp` | `size.fits_imm(imm)` |
| `test_rr(size, a: Reg, b: Reg)` | TEST `84`/`85 /r`, `a` in `rm` | |
| `test_mr(size, a: Mem, b: Reg)` | TEST `84`/`85 /r` | |
| `test_ri(size, a: Reg, imm)` | TEST `A8 ib`/`A9 iw/id` for the accumulator, else `F6`/`F7 /0` (no `imm8` form) | `size.fits_imm(imm)` |
| `test_mi(size, a: Mem, imm)` | TEST `F6`/`F7 /0` | `size.fits_imm(imm)` |
| `inc_r`, `dec_r` `(size, r: Reg)` | INC/DEC `FE`/`FF /0`, `/1` | |
| `inc_m`, `dec_m` `(size, m: Mem)` | the same on memory; lockable | |
| `not_r`, `neg_r` `(size, r)` | NOT/NEG `F6`/`F7 /2`, `/3` | |
| `not_m`, `neg_m` `(size, m)` | the same on memory; lockable | |

In the 8-bit form of each opcode pair the first opcode is used, otherwise
the second, with `66` for `W` and `REX.W` for `Q`. `test` has no
register-memory form other than `test_mr` (TEST is symmetric). There is no
`op_rr` alternative through `8n+2`: a register-register ALU instruction is
always the `r/m, r` opcode, as LLVM encodes it. `xor_rr(Size::D, r, r)` is
the zero idiom.

## Multiply and divide

| constructor | encodes | precondition |
|---|---|---|
| `mul_r`, `imul_r`, `div_r`, `idiv_r` `(size, r: Reg)` | MUL/IMUL/DIV/IDIV `F6`/`F7 /4`, `/5`, `/6`, `/7`: the one-operand forms on `rdx:rax` (`ax` for `B`) | |
| `mul_m`, `imul_m`, `div_m`, `idiv_m` `(size, m: Mem)` | the same on memory | |
| `imul_rr(size, dst, src: Reg)` | IMUL `0F AF /r` | `size != B` |
| `imul_rm(size, dst, src: Mem)` | IMUL `0F AF /r` | `size != B` |
| `imul_rri(size, dst, src: Reg, imm: i64)` | IMUL `6B /r ib` for a sign-extended byte, else `69 /r iw/id` | `size != B`, `size.fits_imm(imm)` |
| `imul_rmi(size, dst, src: Mem, imm: i64)` | the same with a memory source | as `imul_rri` |

The sign extensions that set up a division are in "Moves and extensions"
(`cwd`, `cdq`, `cqo`).

## Moves and extensions

| constructor | encodes | precondition |
|---|---|---|
| `mov_rr(size, dst, src: Reg)` | MOV `88`/`89 /r`, `dst` in `rm` | |
| `mov_rm(size, dst: Reg, src: Mem)` | MOV `8A`/`8B /r` | |
| `mov_mr(size, dst: Mem, src: Reg)` | MOV `88`/`89 /r` | |
| `mov_ri(size, dst: Reg, imm: i64)` | `B0+r ib`, `B8+r iw`, `B8+r id`; for `Q`, `REX.W C7 /0 id` when `imm` is a sign-extended 32-bit value, else `REX.W B8+r io` (`movabs`) | `mov_imm_fits(size, imm)`: `size.fits_imm(imm)`, or any value for `Q` |
| `mov_mi(size, dst: Mem, imm: i64)` | MOV `C6 /0 ib`, `C7 /0 iw/id` (sign-extended for `Q`) | `size.fits_imm(imm)` |
| `movzx_rr(dst_size, dst, src_size, src: Reg)` | MOVZX `0F B6 /r` (8-bit source), `0F B7 /r` (16-bit) | `extends(dst_size, src_size)` |
| `movzx_rm(dst_size, dst, src_size, src: Mem)` | the same with a memory source | `extends(dst_size, src_size)` |
| `movsx_rr`, `movsx_rm` | MOVSX `0F BE /r`, `0F BF /r` | `extends(dst_size, src_size)` |
| `movsxd_rr(dst, src: Reg)`, `movsxd_rm(dst, src: Mem)` | MOVSXD `REX.W 63 /r`: 64-bit destination, 32-bit source | |
| `cbw()`, `cwde()`, `cdqe()` | `66 98`, `98`, `REX.W 98`: sign-extend `al`/`ax`/`eax` into `ax`/`eax`/`rax` | |
| `cwd()`, `cdq()`, `cqo()` | `66 99`, `99`, `REX.W 99`: sign-extend the accumulator into `dx:ax`/`edx:eax`/`rdx:rax` | |
| `lea(size, dst: Reg, src: Mem)` | LEA `8D /r` | `size != B` |

`extends(dst, src)` holds for `(W, B)`, `(D, B)`, `(Q, B)`, `(D, W)` and
`(Q, W)`: a source of 8 or 16 bits, narrower than the destination. A
32-to-64 zero extension is `mov_rr(Size::D, r, r)`; a sign extension is
`movsxd_rr`.

## Shifts

`op` is `rol` (`/0`), `ror` (`/1`), `rcl` (`/2`), `rcr` (`/3`), `shl` (`/4`;
SAL is the same instruction), `shr` (`/5`), `sar` (`/7`).

| constructor | encodes | precondition |
|---|---|---|
| `op_ri(size, r: Reg, n: u8)` | `D0`/`D1 /d` when `n == 1`, else `C0`/`C1 /d ib` | |
| `op_mi(size, m: Mem, n: u8)` | the same on memory | |
| `op_rcl(size, r: Reg)` | `D2`/`D3 /d`: by `cl` | |
| `op_mcl(size, m: Mem)` | the same on memory | |
| `shld_rri(size, dst, src: Reg, n: u8)`, `shrd_rri` | SHLD `0F A4 /r ib`, SHRD `0F AC /r ib`; `dst` in `rm`, `src` in reg | `size != B` |
| `shld_rrcl(size, dst, src)`, `shrd_rrcl` | SHLD `0F A5 /r`, SHRD `0F AD /r`: by `cl` | `size != B` |
| `shld_mri`, `shrd_mri`, `shld_mrcl`, `shrd_mrcl` `(size, dst: Mem, src: Reg, ..)` | the same with a memory destination | `size != B` |

The count is any `u8`; the processor masks it to 5 bits (6 for 64-bit
operands). The `D0`/`D1` form for a count of 1 behaves exactly as `C0`/`C1`
with 1, flags included, and is what LLVM's assembler chooses.

## Conditional

| constructor | encodes | precondition |
|---|---|---|
| `setcc_r(cond, dst: Reg)` | SETcc `0F 90+cc /0` on the 8-bit register | |
| `setcc_m(cond, dst: Mem)` | SETcc on a byte of memory | |
| `cmovcc_rr(cond, size, dst, src: Reg)` | CMOVcc `0F 40+cc /r` | `size != B` |
| `cmovcc_rm(cond, size, dst: Reg, src: Mem)` | the same with a memory source (the load happens either way) | `size != B` |

## Branches and calls

A displacement is the field's value: the signed distance from the **end**
of the instruction to the target, as the processor adds it.

| constructor | encodes |
|---|---|
| `jmp_short(disp: i8)` | JMP `EB cb` |
| `jmp_near(disp: i32)` | JMP `E9 cd` |
| `jcc_short(cond, disp: i8)` | Jcc `70+cc cb` |
| `jcc_near(cond, disp: i32)` | Jcc `0F 80+cc cd` |
| `call(disp: i32)` | CALL `E8 cd` |
| `jmp_r(target: Reg)`, `jmp_m(target: Mem)` | JMP `FF /4` |
| `call_r(target: Reg)`, `call_m(target: Mem)` | CALL `FF /2` |
| `ret()` | RET `C3` |
| `ret_imm(n: u16)` | RET `C2 iw` |

The choice between the short and the near form is the caller's: a
constructor never relaxes. The code generator emits the near form for a
branch whose target is not yet known and the short form only when the
distance is known to fit.

## Stack

| constructor | encodes |
|---|---|
| `push_r(r: Reg)`, `pop_r(r: Reg)` | PUSH `50+r`, POP `58+r` |
| `push_m(m: Mem)`, `pop_m(m: Mem)` | PUSH `FF /6`, POP `8F /0` |
| `push_i(imm: i32)` | PUSH `6A ib` for a sign-extended byte, else `68 id`; pushes 8 bytes |
| `leave()` | LEAVE `C9` |

## Bit counting

| constructor | encodes | precondition |
|---|---|---|
| `bsf_rr`, `bsf_rm` | BSF `0F BC /r` | `size != B` |
| `bsr_rr`, `bsr_rm` | BSR `0F BD /r` | `size != B` |
| `tzcnt_rr`, `tzcnt_rm` | TZCNT `F3 0F BC /r` (BMI1) | `size != B` |
| `lzcnt_rr`, `lzcnt_rm` | LZCNT `F3 0F BD /r` (LZCNT) | `size != B` |
| `popcnt_rr`, `popcnt_rm` | POPCNT `F3 0F B8 /r` (POPCNT) | `size != B` |
| `bswap(size, r: Reg)` | BSWAP `0F C8+r` | `size` is `D` or `Q` (the 16-bit form is undefined) |

Each `_rr` takes `(size, dst: Reg, src: Reg)` and each `_rm`
`(size, dst: Reg, src: Mem)`. On processors without the feature, `TZCNT`
and `LZCNT` execute as `BSF` and `BSR`; choosing them is the code
generator's business.

## Atomics

| constructor | encodes | precondition |
|---|---|---|
| `lock(inst: Inst)` | `inst` with `F0` inserted after its segment and `66` prefixes | `inst.is_lockable()` |
| `xchg_rr(size, a, b: Reg)` | see below | |
| `xchg_mr(size, m: Mem, r: Reg)` | XCHG `86`/`87 /r` (atomic without `lock`); lockable | |
| `xadd_mr(size, m: Mem, r: Reg)` | XADD `0F C0`/`C1 /r`; lockable | |
| `cmpxchg_mr(size, m: Mem, r: Reg)` | CMPXCHG `0F B0`/`B1 /r`; lockable | |
| `cmpxchg16b_m(m: Mem)` | CMPXCHG16B `REX.W 0F C7 /1`; lockable | |
| `mfence()`, `lfence()`, `sfence()` | `0F AE F0`, `0F AE E8`, `0F AE F8` | |
| `pause()` | PAUSE `F3 90` | |

An instruction is **lockable** when the SDM allows `LOCK` on it: the
memory-destination forms marked lockable in the tables above (`op_mr` and
`op_mi` except `cmp`, `inc_m`, `dec_m`, `not_m`, `neg_m`, `xchg_mr`,
`xadd_mr`, `cmpxchg_mr`, `cmpxchg16b_m`) and nothing else. A locked
instruction is not lockable again. `lock` moves the instruction's
PC-relative field one byte later.

`xchg_rr(size, a, b)`: when the size is 16, 32 or 64 bits and one operand
is the accumulator, `90+r` with the other register (`66 90+r`,
`REX.W 90+r`); except `xchg eax, eax`, which is `87 C0` (it zeroes bits
63:32 of `rax`, which `90` does not); and `xchg rax, rax`, which is `90`
alone, `nop`. Otherwise `86`/`87 /r` with `a` in reg and `b` in `rm`. This
is LLVM's choice; `xchg ax, ax` is `66 90`.

## System and padding

| constructor | encodes | precondition |
|---|---|---|
| `ud2()` | UD2 `0F 0B` | |
| `int3()` | INT3 `CC` | |
| `endbr64()` | ENDBR64 `F3 0F 1E FA` | |
| `nop(len: usize)` | one NOP of exactly `len` bytes | `nop_len_ok(len)`: `1..=15` |

`nop(len)` is, for 1 to 9 bytes, the SDM's recommended form (Vol. 2B,
"NOP", table 4-12): `90`; `66 90`; `0F 1F 00`; `0F 1F 40 00`;
`0F 1F 44 00 00`; `66 0F 1F 44 00 00`; `0F 1F 80` + 4 zero bytes;
`0F 1F 84 00` + 4; `66 0F 1F 84 00` + 4. For 10 bytes it is
`66 2E 0F 1F 84 00` + 4, and for 11 to 15 bytes that with one more `66`
per byte in front. This is LLVM's padding (`.nops len, len`).

## Fixups and relocations

An instruction has at most one **PC-relative field**: the `rel8`/`rel32` of
a branch or call, or the `disp32` of a RIP-relative memory operand. The
field holds the distance from the end of the instruction to the target.

```rust
pub enum Fixup { Rel8, Rel32 }
impl Fixup {
    pub const ALL: [Fixup; 2];
    pub const fn size(self) -> usize;                     // 1, 4
    pub const fn min(self) -> i64;                        // -128, -(1 << 31)
    pub const fn max(self) -> i64;                        // 127, (1 << 31) - 1
    pub const fn fits(self, disp: i64) -> bool;
    pub const fn patch(self, code: &mut [u8], at: usize, disp: i64) -> bool;
    pub const fn extract(self, code: &[u8], at: usize) -> Option<i64>;
}
pub struct Site { pub offset: usize, pub fixup: Fixup, pub addend: i64 }
impl Inst {
    pub const fn site(&self) -> Option<Site>;
    pub const fn with_disp(self, disp: i64) -> Option<Inst>;
    pub const fn has_rex(&self) -> bool;
    pub const fn is_lockable(&self) -> bool;
}
```

- `patch` writes `disp` little-endian into `code[at..at + size]` and
  returns `true`; it returns `false` and changes nothing when `!fits(disp)`
  or the field does not lie inside `code`. It touches no other byte and
  does not look at the opcode. `extract` is its inverse; `None` when the
  field does not lie inside `code`.
- `site()` is `None` for an instruction without a PC-relative field.
  `offset` is the field's offset from the start of the instruction and
  `addend` is `offset as i64 - len`: minus the bytes from the field to the
  end of the instruction (including an immediate after a RIP-relative
  operand). A relocation at `start + offset` with that addend computes
  `S + A - P` = the distance from the end of the instruction to `S`, so
  the same `Site` serves an ELF relocation and an internal fixup: to patch
  for a target at section offset `t`, write `t + addend - (start + offset)`.
- `with_disp(d)` is the instruction with its field set to `d`; `None` when
  it has no field or `d` does not fit. Each constructor with a displacement
  operand equals its zero-displacement form with that displacement:
  `jmp_near(d) == jmp_near(0).with_disp(d.into())`, and so on.

The relocations of the small PIC code model (x86-64 psABI "Relocation
Types"), at `start + site.offset` with addend `site.addend` (plus the
symbol offset):

| reference | instruction | relocation |
|---|---|---|
| direct call or tail call, conditional branch to a function | `call`, `jmp_near`, `jcc_near` | `R_X86_64_PLT32` |
| address or access of data or a function in this module | `lea`, or any constructor with `Mem::rip` | `R_X86_64_PC32` |
| load of a GOT entry | `mov_rm(Size::Q, ..)`, the ALU `_rm` forms, `test_mr` (GOT-relaxable by the linker) | `R_X86_64_REX_GOTPCRELX` when `has_rex()`, else `R_X86_64_GOTPCRELX` |
| call or jump through a GOT entry | `call_m`, `jmp_m` | `R_X86_64_GOTPCRELX` |
| initial-exec TLS offset | `mov_rm(Size::Q, ..)` or `add_rm(Size::Q, ..)` | `R_X86_64_GOTTPOFF` |

The code generator picks the relocation and gives it to the object writer
(`spec/object.md` "Relocations"); this crate names only the place and the
addend.

## Not provided

By decision of this WP's scope: the high-byte registers `ah`..`bh`; the
string instructions (`movs`, `stos`, `rep` prefixes); `bt`/`bts`/`btr`/
`btc`; `cmpxchg8b`; `xadd` and `cmpxchg` with a register destination;
`movabs` to and from the accumulator with a 64-bit address; `enter`; 16-bit
`push`/`pop`; far branches; `jrcxz` and `loop`; `cpuid`, `rdtsc`, `hlt` and
the other system instructions; the TLS access sequences that need fixed
padding prefixes (general and local dynamic: `data16 lea` / `data16 data16
rex64 call`) and the absolute `TPOFF32` field of local exec, which is not a
PC-relative site; scalar SSE and everything VEX- or EVEX-encoded (APX
included). Adding an instruction is a constructor, a row here, a row in
`tests/l1/table.rs` and a sweep.

## Test method

The tests are `crates/hod_mc_x64/tests/l1/` (L1 of `docs/VALIDATION.md`).
The oracle is LLVM's assembler and disassembler, target
`x86_64-unknown-linux-gnu`, Intel syntax (`.intel_syntax noprefix`,
`llvm-objdump -M intel`), and **every installed major** of
`hod_testutil::LLVM_MAJORS` (22 and 19): each batch is checked against each.
The tests that need the tools skip without them unless
`HOD_REQUIRE_TOOLS=1`.

**Sweeps.** One module per section above builds a batch of cases. A case is
a constructor call and the assembly text for the same instruction, written
by formatters that share nothing with the encoder (`tests/l1/text.rs`; a
memory operand is formatted from the parts it was built from). The
`memory-operands` sweep takes every form of "Memory operands" (each base
with each displacement class, each base with each index and scale, no
base, absolute, RIP-relative, both segments; about 1400) through a load and
a store with every register in the reg field and through `lea`. Each other
sweep covers, for every constructor: every register in every register
position, every size, every condition, a sample of memory operands that
hits each special case of the ModRM/SIB layout and each REX bit, the
boundary and interior values of every immediate, displacement and count,
and, for a checked form, operands just outside each bound, which must
return `None`.

**Checks.** For each batch (`*_match_llvm`, `*_matches_llvm`):

1. *Forward.* All the lines are assembled by one `llvm-mc` run, each after
   a label of its own; a branch's text names its target as an offset from
   that label, with `{disp32}` on the near forms. Each line's bytes, from
   its label to the next (`llvm-nm`, `llvm-objcopy`), must equal hod's. A
   mismatch is reported with its line, and a wrong length does not
   misalign the lines after it.
2. *Reverse.* hod's bytes, one `.byte` line per case, are disassembled by
   `llvm-objdump`; an instruction must start at each case's first byte and
   the next at its end, and its mnemonic (with a `lock` prefix joined to
   it) must be the instruction the table above says the constructor
   encodes (`tests/l1/table.rs`).
3. *Rejections.* Where a rejected operand combination has assembly text
   that LLVM does not silently re-express, `llvm-mc` must report an error
   on that line.

**Coverage.** `coverage::every_constructor_is_swept` reads the crate's
source for its public constructors (as `hod_mc_a64`'s does) and fails if
one has no row in the table, no case in any sweep, or (for a checked form)
no rejected operands, or if the sweeps together fall below 150,000
encodings.

**The harness itself.** `selftest` gives the harness batches that disagree
with LLVM (wrong bytes, a wrong length, a wrong instruction, bytes that
decode as two instructions, a wrong family member, text LLVM refuses, a
rejection LLVM accepts) and requires each to be reported, by every major.

**Fixups**, `fixups`: without tools, the ranges; `patch` and `extract` are
inverses, little-endian, touch only the field, and refuse out-of-range
displacements and fields outside the code; each instruction's site is at
the offset, of the kind and with the addend stated independently in the
test; constructors equal their zero form patched. With tools,
`relocation_sites_match_llvm`: for each reference of the table above
(68 instructions, among them every register for `lea`, byte loads and GOT
loads, an immediate after the field, a `lock` and a segment prefix before
it) the bytes equal LLVM's, and LLVM's relocation is at hod's site, with
hod's addend and the type the table gives, `has_rex` deciding between the
two GOTPCRELX kinds; and `a_laid_out_function_matches_llvm`: a function
with forward and backward short and near branches, a call, RIP-relative
operands with and without an immediate after the field, under `lock` and
a segment override, and padding, encoded with zero fields and patched
through its sites, equals LLVM's assembly of the same source.

### Where hod and LLVM's assembler differ

- LLVM's assembler truncates an 8-, 16- or 32-bit immediate that does not
  fit (`add al, 256` assembles as `add al, 0`) without a diagnostic. hod
  rejects it. Those rejections have no text LLVM refuses and are tested by
  hod alone; LLVM does refuse a 64-bit instruction's immediate that is not
  a sign-extended 32-bit value, and those are confirmed.
- LLVM's assembler accepts `lock` before any instruction. hod refuses it
  on an instruction that is not lockable; tested by hod alone.
- LLVM accepts `movsx r64, r32` as another spelling of MOVSXD; hod spells
  it `movsxd_rr` and `movsx_*` rejects that size pair; tested by hod alone.

Every encoding the crate produces is the one LLVM's assembler produces for
the same text, at both majors; there is no choice where they differ.
