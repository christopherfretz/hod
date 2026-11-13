# `hod_mc_x64`: x86-64 instruction encoding

`crates/hod_mc_x64` turns an instruction and its operands into its bytes.
It knows nothing about the IR, object files or assembly text. Its consumer
is a single-pass code generator that calls it once per emitted instruction,
so the whole API is plain functions on small `Copy` values.

This document covers the base integer instruction set, the scalar
SSE/SSE2 floating point and the packed SSE2 forms an `-O0` code generator
needs, in 64-bit mode, and the fixed forms of the thread-local storage
accesses. The oracle and the crate assume the x86-64 baseline (which
includes SSE2 and `CMPXCHG8B`) plus the instructions named below that need
a CPUID feature (`LZCNT`, `TZCNT` (BMI1), `POPCNT`, `CMPXCHG16B`,
`ENDBR64`, `RDTSCP`); the crate does not check features, the code
generator does. Nothing is VEX-encoded: hod's x86-64 code is SSE2 only.

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
  | `_xr` / `_rx` | an `xmm` and a general register / the reverse, where the kinds would otherwise share a suffix |
  | `_rri`, `_rmi` (packed) | two registers / a register and memory, then an `imm8` (a shuffle order, a lane) |

  An `xmm` register is an `r` in every other suffix: `adds_rr` takes two
  `xmm` registers, `cvtsi2s_rr` an `xmm` and a general register, and the
  parameter types say which.

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
| `Xmm` | `Xmm0`..`Xmm15` (numbers 0..15; `Xmm::new(n)` is `None` for `n > 15`; `.code()` the number) | every SSE register operand |
| `FpSize` | `S` (single, 32 bits), `D` (double, 64 bits); `.bits()` | the precision of a scalar operation, the element type of a packed logical one |
| `FpCmp` | `Eq`, `Lt`, `Le`, `Unord`, `Neq`, `Nlt`, `Nle`, `Ord` (the `cmpss` `imm8` 0..7, in that order; `.invert()` flips bit 2, the negation including the unordered case) | `cmps_rr`, `cmps_rm` |

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
| `m.with_abs32() -> Option<Mem>` | the same operand with its displacement an **absolute field** ("Fixups and relocations"); `None` for a RIP-relative operand, whose displacement is already the PC-relative field |

`get_base()` (a `Base`: `None`, `Reg(Reg)`, `Rip`), `get_index()`, `disp()`,
`seg()` and `is_abs32()` read the parts back; `has_field()` says whether
the displacement is a relocatable field (RIP-relative or `with_abs32`). Every value of `Mem` has an encoding:
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
- An absolute field: as above, except that with a base the displacement
  is always `disp32` (`mod` 10), even for `disp == 0` or a `disp8` value;
  with no base it is the `disp32` it always is. Either way that `disp32` is
  the instruction's site.

This is exactly LLVM's choice for the same operand (with `{disp32}` for an
absolute field, which is also what LLVM writes for `[base + sym]`).

## Instruction layout

An instruction is, in this order: the segment override; `66` (16-bit
operand size); the lock prefix or the mandatory `F2`/`F3` prefix; REX; the
opcode (`0F` first for the two-byte map); ModRM; SIB; displacement;
immediate. This is the order LLVM emits (it puts `66` before `F0` and
before a mandatory `F3`).

An SSE instruction's mandatory prefix (`66`, `F2` or `F3`) takes the place
of `66` or `F2`/`F3` in that order, so it precedes REX and follows a
segment override.

REX is `0100WRXB`: `W` for a 64-bit operand size where the instruction's
default is not 64 bits; `R`, `X`, `B` the high bit of the reg field,
the SIB index and the base (or `rm`, or the `+r` register). A REX prefix
is emitted when any bit is set, and also, as `40`, when an 8-bit register
operand is `spl`, `bpl`, `sil` or `dil`, which exist only with REX.
`inst.has_rex()` says whether the instruction has one. An `xmm` register
never needs a REX prefix of its own; `xmm8`..`xmm15` set `R` or `B` like
`r8`..`r15`.

`rep_movs` and `rep_stos` are the exception to this order: LLVM emits
`REP` as an instruction of its own, so `F3` comes first, before `66`
("String instructions").

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
| `pushfq()`, `popfq()` | PUSHFQ `9C`, POPFQ `9D`: `rflags` as 8 bytes (WP-144) |
| `push_i(imm: i32)` | PUSH `6A ib` for a sign-extended byte, else `68 id`; pushes 8 bytes |
| `push_i32(imm: i32)` | PUSH `68 id` always; the `id` is the site ("Relocatable immediates") |
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

## Bit tests

`op` is `bt`, `bts`, `btr`, `btc`: each copies the selected bit to CF;
`bts`, `btr`, `btc` then set, clear or complement it.

| constructor | encodes | precondition |
|---|---|---|
| `op_rr(size, dst: Reg, bit: Reg)` | BT `0F A3 /r`, BTS `0F AB /r`, BTR `0F B3 /r`, BTC `0F BB /r`; `dst` in `rm`, `bit` in reg | `size != B` |
| `op_mr(size, dst: Mem, bit: Reg)` | the same with a memory operand; lockable except `bt` | `size != B` |
| `op_ri(size, dst: Reg, bit: u8)` | `0F BA /4 ib` (BT), `/5` (BTS), `/6` (BTR), `/7` (BTC) | `size != B` |
| `op_mi(size, dst: Mem, bit: u8)` | the same with a memory operand; lockable except `bt` | `size != B` |

An immediate bit index is any `u8`, taken modulo the operand size. A
register bit index with a memory operand is a signed offset in bits from
the operand's address, not reduced: the instruction may access memory
outside the operand.

## Atomics

| constructor | encodes | precondition |
|---|---|---|
| `lock(inst: Inst)` | `inst` with `F0` inserted after its segment and `66` prefixes | `inst.is_lockable()` |
| `xchg_rr(size, a, b: Reg)` | see below | |
| `xchg_mr(size, m: Mem, r: Reg)` | XCHG `86`/`87 /r` (atomic without `lock`); lockable | |
| `xadd_mr(size, m: Mem, r: Reg)` | XADD `0F C0`/`C1 /r`; lockable | |
| `cmpxchg_mr(size, m: Mem, r: Reg)` | CMPXCHG `0F B0`/`B1 /r`; lockable | |
| `cmpxchg8b_m(m: Mem)` | CMPXCHG8B `0F C7 /1`: `edx:eax` against the 8 bytes at `m`, `ecx:ebx` stored if equal; lockable | |
| `cmpxchg16b_m(m: Mem)` | CMPXCHG16B `REX.W 0F C7 /1`; lockable | |
| `mfence()`, `lfence()`, `sfence()` | `0F AE F0`, `0F AE E8`, `0F AE F8` | |
| `pause()` | PAUSE `F3 90` | |

`lock` with a memory-destination `add`, `sub`, `and`, `or`, `xor`, `xadd`,
`inc`, `dec`, `not` or `neg` is what the code generator writes for an
`atomicrmw` whose result is unused; each is swept under `lock` at every
size over every memory form ("Test method").

An instruction is **lockable** when the SDM allows `LOCK` on it: the
memory-destination forms marked lockable in the tables above (`op_mr` and
`op_mi` except `cmp`, `inc_m`, `dec_m`, `not_m`, `neg_m`, `xchg_mr`,
`xadd_mr`, `cmpxchg_mr`, `cmpxchg8b_m`, `cmpxchg16b_m`, and `bts`, `btr`,
`btc` in their `_mr` and `_mi` forms) and nothing else. A locked
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
| `ud1_eax(disp: u8)` | UD1 `67 0F B9 /r`: `ud1 eax, [eax + disp]` with 32-bit addressing, `00` for 0, `40 ib` up to 127, else `80 id` (`llc`'s `llvm.ubsantrap`, WP-144) | |
| `int3()` | INT3 `CC` | |
| `endbr64()` | ENDBR64 `F3 0F 1E FA` | |
| `nop(len: usize)` | one NOP of exactly `len` bytes | `nop_len_ok(len)`: `1..=15` |
| `rdtsc()` | RDTSC `0F 31`: the time-stamp counter into `edx:eax` (`llvm.readcyclecounter`) | |
| `rdtscp()` | RDTSCP `0F 01 F9`: the same and `IA32_TSC_AUX` into `ecx`; needs the RDTSCP feature | |
| `cpuid()` | CPUID `0F A2`: leaf `eax`, subleaf `ecx`, into `eax`, `ebx`, `ecx`, `edx` | |

`nop(len)` is, for 1 to 9 bytes, the SDM's recommended form (Vol. 2B,
"NOP", table 4-12): `90`; `66 90`; `0F 1F 00`; `0F 1F 40 00`;
`0F 1F 44 00 00`; `66 0F 1F 44 00 00`; `0F 1F 80` + 4 zero bytes;
`0F 1F 84 00` + 4; `66 0F 1F 84 00` + 4. For 10 bytes it is
`66 2E 0F 1F 84 00` + 4, and for 11 to 15 bytes that with one more `66`
per byte in front. This is LLVM's padding (`.nops len, len`).

## String instructions

| constructor | encodes |
|---|---|
| `cld()` | CLD `FC`: clears the direction flag (the psABI guarantees it clear at calls and returns) |
| `rep_movs(size)` | `F3` then MOVSB/MOVSW/MOVSD/MOVSQ `[66] [REX.W] A4`/`A5`: copies `rcx` elements of `size` from `[rsi]` to `[rdi]`, advancing both; `rcx` ends at 0 |
| `rep_stos(size)` | `F3` then STOSB/W/D/Q `[66] [REX.W] AA`/`AB`: stores `al`/`ax`/`eax`/`rax` to `rcx` elements at `[rdi]`; `rcx` ends at 0 |

The `F3` is first, before the `66` of the 16-bit forms (`F3 66 A5`), as
LLVM writes `rep` (an instruction of its own to its assembler). The
operands are implicit; there is no segment override and no form without
`rep`. hod does not use them in v1 (a large `memcpy` is a call); they are
here for an inline copy or fill when one is wanted.

## Scalar floating point

SSE and SSE2 in their legacy encodings: the `0F` map with a mandatory
prefix. `X` is an `Xmm`, `fp` an `FpSize`. For a precision-taking
constructor the SDM instruction is the name with the precision letter put
back: `adds` is ADDSS (`fp = S`, prefix `F3`) or ADDSD (`D`, `F2`); `xorp`
is XORPS (no prefix) or XORPD (`66`); `cvtts2si` is CVTTSS2SI or
CVTTSD2SI. A scalar memory operand is 32 bits for `S`, 64 for `D`; a
128-bit one (`movap`, `movup`, `movdq*`, the packed logical forms) must be
16-byte aligned except for `movup_*` and `movdqu_*`, or the instruction
faults.

| constructor | encodes | precondition |
|---|---|---|
| `movs_rr(fp, dst: X, src: X)` | MOVSS/MOVSD `F3`/`F2 0F 10 /r`, `dst` in reg: the low element, the rest of `dst` kept | |
| `movs_rm(fp, dst: X, src: Mem)` | the same, a load: the rest of `dst` zeroed | |
| `movs_mr(fp, dst: Mem, src: X)` | MOVSS/MOVSD `F3`/`F2 0F 11 /r`: a store | |
| `movap_rr(fp, dst: X, src: X)` | MOVAPS/MOVAPD `[66] 0F 28 /r`, `dst` in reg: all 128 bits | |
| `movap_rm(fp, dst: X, src: Mem)`, `movap_mr(fp, dst: Mem, src: X)` | `[66] 0F 28 /r`, `[66] 0F 29 /r` | |
| `movup_rm(fp, dst: X, src: Mem)`, `movup_mr(fp, dst: Mem, src: X)` | MOVUPS/MOVUPD `[66] 0F 10 /r`, `[66] 0F 11 /r` | |
| `movdqa_rr(dst: X, src: X)`, `movdqa_rm(dst: X, src: Mem)`, `movdqa_mr(dst: Mem, src: X)` | MOVDQA `66 0F 6F /r` (register and load), `66 0F 7F /r` (store) | |
| `movdqu_rm(dst: X, src: Mem)`, `movdqu_mr(dst: Mem, src: X)` | MOVDQU `F3 0F 6F /r`, `F3 0F 7F /r` | |
| `movd_xr(size, dst: X, src: Reg)` | MOVD/MOVQ `66 [REX.W] 0F 6E /r`: the general register's 32 or 64 bits into the low element, the rest zeroed | `size` is `D` or `Q` |
| `movd_rx(size, dst: Reg, src: X)` | MOVD/MOVQ `66 [REX.W] 0F 7E /r`, the `xmm` in reg: the low 32 or 64 bits | `size` is `D` or `Q` |
| `adds_rr`, `subs_rr`, `muls_rr`, `divs_rr`, `sqrts_rr` `(fp, dst: X, src: X)` | ADD/SUB/MUL/DIV/SQRT SS/SD `F3`/`F2 0F 58`, `5C`, `59`, `5E`, `51 /r` | |
| `adds_rm`, `subs_rm`, `muls_rm`, `divs_rm`, `sqrts_rm` `(fp, dst: X, src: Mem)` | the same with a memory source | |
| `andp_rr`, `andnp_rr`, `orp_rr`, `xorp_rr` `(fp, dst: X, src: X)` | AND/ANDN/OR/XOR PS/PD `[66] 0F 54`, `55`, `56`, `57 /r`: the bitwise operations on 128 bits (`andnp`: `dst = !dst & src`) for absolute value, negation, copysign and the zero idiom `xorp_rr(S, x, x)` | |
| `andp_rm`, `andnp_rm`, `orp_rm`, `xorp_rm` `(fp, dst: X, src: Mem)` | the same with a 128-bit memory source | |
| `ucomis_rr(fp, a: X, b: X)`, `ucomis_rm(fp, a: X, b: Mem)` | UCOMISS/UCOMISD `[66] 0F 2E /r`: the low elements into ZF, PF, CF (all three set when unordered), OF, SF, AF cleared; an exception only for a signaling NaN | |
| `comis_rr(fp, a: X, b: X)`, `comis_rm(fp, a: X, b: Mem)` | COMISS/COMISD `[66] 0F 2F /r`: the same, signaling on any NaN | |
| `cmps_rr(pred: FpCmp, fp, dst: X, src: X)`, `cmps_rm(pred, fp, dst: X, src: Mem)` | CMPSS/CMPSD `F3`/`F2 0F C2 /r ib`, `ib` the predicate: the low element of `dst` all ones where it holds, else zero | |
| `cvtsi2s_rr(fp, dst: X, size, src: Reg)`, `cvtsi2s_rm(fp, dst: X, size, src: Mem)` | CVTSI2SS/CVTSI2SD `F3`/`F2 [REX.W] 0F 2A /r`: the signed 32- or 64-bit integer, rounded as MXCSR says; the rest of `dst` kept | `size` is `D` or `Q` |
| `cvtts2si_rr(size, dst: Reg, fp, src: X)`, `cvtts2si_rm(size, dst: Reg, fp, src: Mem)` | CVTTSS2SI/CVTTSD2SI `F3`/`F2 [REX.W] 0F 2C /r`: toward zero; `1 << (size.bits() - 1)` (the "integer indefinite") when NaN or out of range | `size` is `D` or `Q` |
| `cvts2si_rr(size, dst: Reg, fp, src: X)`, `cvts2si_rm(size, dst: Reg, fp, src: Mem)` | CVTSS2SI/CVTSD2SI `F3`/`F2 [REX.W] 0F 2D /r`: rounded as MXCSR says (`lrint`, `llrint`); the integer indefinite when NaN or out of range | `size` is `D` or `Q` |
| `mins_rr`, `maxs_rr` `(fp, dst: X, src: X)`, `mins_rm`, `maxs_rm` `(fp, dst: X, src: Mem)` | MIN/MAX SS/SD `F3`/`F2 0F 5D`, `5F /r`: `dst < src ? dst : src` (`>` for max), so the **second** operand when either is NaN or both are zeros of either sign; `minnum`/`fmin` need a NaN fix-up around it | |
| `movup_rr(fp, dst: X, src: X)` | MOVUPS/MOVUPD `[66] 0F 10 /r`, `dst` in reg | |
| `movdqu_rr(dst: X, src: X)` | MOVDQU `F3 0F 6F /r` | |
| `movq_rr(dst: X, src: X)` | MOVQ `F3 0F 7E /r`: the low 64 bits, the high 64 zeroed | |
| `movd_rm(size, dst: X, src: Mem)` | MOVD `66 0F 6E /r` (`D`) / MOVQ `F3 0F 7E /r` (`Q`): a load into the low element, the rest zeroed | `size` is `D` or `Q` |
| `movd_mr(size, dst: Mem, src: X)` | MOVD `66 0F 7E /r` (`D`) / MOVQ `66 0F D6 /r` (`Q`): a store of the low element | `size` is `D` or `Q` |
| `cvtss2sd_rr(dst: X, src: X)`, `cvtss2sd_rm(dst: X, src: Mem)` | CVTSS2SD `F3 0F 5A /r`: exact | |
| `cvtsd2ss_rr(dst: X, src: X)`, `cvtsd2ss_rm(dst: X, src: Mem)` | CVTSD2SS `F2 0F 5A /r`: rounded as MXCSR says | |

Register-register moves use the load opcode with `dst` in reg (`0F 10`,
`0F 28`, `66 0F 6F`), as LLVM encodes them. The conversions' operand order
follows `movzx`: destination, then the source and its size or precision.
An unsigned conversion, a 64-bit unsigned one in particular, is a sequence
the code generator writes from these. The 64-bit `movq` memory forms are
LLVM's: `F3 0F 7E` to load and `66 0F D6` to store, not `66 REX.W 0F
6E`/`7E`, which LLVM reserves for the general-register forms.

## Packed SSE2

SSE and SSE2 on all 128 bits of an `xmm`, legacy-encoded: the `0F` map,
mandatory prefix `66` for the integer forms, none or `66` for the single
and double floating-point forms (an `fp` operand, as in "Scalar floating
point": `addp` is ADDPS or ADDPD). The mandatory prefix precedes REX.
Every memory operand is 128 bits and must be 16-byte aligned, except
where a row names a narrower width (those have no alignment requirement).
A `_rm` form is its `_rr` form with the second register replaced by the
memory operand; both exist for every row with "/r".

**Integer, `66 0F op /r`**, constructors `name_rr(dst: X, src: X)` and
`name_rm(dst: X, src: Mem)`:

| instructions | `op` |
|---|---|
| `paddb`, `paddw`, `paddd`, `paddq` | `FC`, `FD`, `FE`, `D4` |
| `psubb`, `psubw`, `psubd`, `psubq` | `F8`, `F9`, `FA`, `FB` |
| `paddsb`, `paddsw`, `paddusb`, `paddusw` (saturating) | `EC`, `ED`, `DC`, `DD` |
| `psubsb`, `psubsw`, `psubusb`, `psubusw` (saturating) | `E8`, `E9`, `D8`, `D9` |
| `pmullw`, `pmulhw`, `pmulhuw`, `pmuludq`, `pmaddwd` | `D5`, `E5`, `E4`, `F4`, `F5` |
| `pand`, `pandn` (`!dst & src`), `por`, `pxor` | `DB`, `DF`, `EB`, `EF` |
| `pcmpeqb`, `pcmpeqw`, `pcmpeqd` | `74`, `75`, `76` |
| `pcmpgtb`, `pcmpgtw`, `pcmpgtd` (signed `dst > src`) | `64`, `65`, `66` |
| `pminsw`, `pmaxsw`, `pminub`, `pmaxub` | `EA`, `EE`, `DA`, `DE` |
| `pavgb`, `pavgw`, `psadbw` | `E0`, `E3`, `F6` |
| `psllw`, `pslld`, `psllq` (count in the low 64 bits of `src`) | `F1`, `F2`, `F3` |
| `psrlw`, `psrld`, `psrlq` | `D1`, `D2`, `D3` |
| `psraw`, `psrad` | `E1`, `E2` |
| `punpcklbw`, `punpcklwd`, `punpckldq`, `punpcklqdq` | `60`, `61`, `62`, `6C` |
| `punpckhbw`, `punpckhwd`, `punpckhdq`, `punpckhqdq` | `68`, `69`, `6A`, `6D` |
| `packsswb`, `packssdw`, `packuswb` | `63`, `6B`, `67` |

A shift count of at least the element width gives 0 (`psra*`: the sign
in every bit).

**Shifts by an immediate**, `name_ri(dst: X, n: u8)`, `66 0F op /digit ib`
with `dst` in `rm`:

| instructions | encoding |
|---|---|
| `psrlw`, `psraw`, `psllw` | `71 /2`, `71 /4`, `71 /6` |
| `psrld`, `psrad`, `pslld` | `72 /2`, `72 /4`, `72 /6` |
| `psrlq`, `psrldq`, `psllq`, `pslldq` | `73 /2`, `73 /3`, `73 /6`, `73 /7` |

`psrldq`/`pslldq` shift the whole register by `n` **bytes** (all zero for
`n > 15`).

**Shuffles and the rest:**

| constructor | encodes |
|---|---|
| `pshufd_rri(dst: X, src: X, order: u8)`, `pshufd_rmi(dst, src: Mem, order)` | PSHUFD `66 0F 70 /r ib`: element `i` of `dst` is element `(order >> 2i) & 3` of `src` |
| `pshuflw_rri`, `pshuflw_rmi` | PSHUFLW `F2 0F 70 /r ib`: the same on the low four words, the high 64 bits copied |
| `pshufhw_rri`, `pshufhw_rmi` | PSHUFHW `F3 0F 70 /r ib`: the same on the high four words |
| `shufp_rri(fp, dst: X, src: X, order: u8)`, `shufp_rmi(fp, dst, src: Mem, order)` | SHUFPS/SHUFPD `[66] 0F C6 /r ib`: the low half from `dst`, the high half from `src` |
| `addp`, `subp`, `mulp`, `divp`, `sqrtp`, `minp`, `maxp` `_rr(fp, dst: X, src: X)` / `_rm(fp, dst, src: Mem)` | ADD/SUB/MUL/DIV/SQRT/MIN/MAX PS/PD `[66] 0F 58`, `5C`, `59`, `5E`, `51`, `5D`, `5F /r`; `minp`/`maxp` per element as `mins`/`maxs` |
| `unpcklp_rr`, `unpckhp_rr` `(fp, dst, src: X)`, `_rm` | UNPCKLPS/PD `[66] 0F 14 /r`, UNPCKHPS/PD `[66] 0F 15 /r` |
| `cmpp_rr(pred: FpCmp, fp, dst: X, src: X)`, `cmpp_rm(pred, fp, dst, src: Mem)` | CMPPS/CMPPD `[66] 0F C2 /r ib`, `ib` the predicate: each element all ones where it holds |
| `andp_*`, `andnp_*`, `orp_*`, `xorp_*` | in "Scalar floating point" (they are packed already) |
| `movap_*`, `movup_*`, `movdqa_*`, `movdqu_*` | in "Scalar floating point" |
| `movlhps_rr(dst: X, src: X)`, `movhlps_rr(dst, src)` | MOVLHPS `0F 16 /r` (low half of `src` to high half of `dst`), MOVHLPS `0F 12 /r` (high to low) |
| `movlp_rm(fp, dst: X, src: Mem)`, `movhp_rm(fp, dst, src)` | MOVLPS/PD `[66] 0F 12 /r`, MOVHPS/PD `[66] 0F 16 /r`: a 64-bit load into the low / high half, the other half kept |
| `movlp_mr(fp, dst: Mem, src: X)`, `movhp_mr(fp, dst, src)` | `[66] 0F 13 /r`, `[66] 0F 17 /r`: the low / high half stored (64 bits) |
| `cvtdq2ps_*` | CVTDQ2PS `0F 5B /r` |
| `cvtps2dq_*`, `cvttps2dq_*` | CVTPS2DQ `66 0F 5B /r` (MXCSR rounding), CVTTPS2DQ `F3 0F 5B /r` (toward zero) |
| `cvtdq2pd_*` | CVTDQ2PD `F3 0F E6 /r`: the low two `i32`; the memory form reads 64 bits |
| `cvtpd2dq_*`, `cvttpd2dq_*` | CVTPD2DQ `F2 0F E6 /r`, CVTTPD2DQ `66 0F E6 /r`: into the low half, the high half zeroed |
| `cvtps2pd_*` | CVTPS2PD `0F 5A /r`: the low two `f32`; the memory form reads 64 bits |
| `cvtpd2ps_*` | CVTPD2PS `66 0F 5A /r`: into the low half, the high half zeroed |
| `pmovmskb_rr(dst: Reg, src: X)` | PMOVMSKB `66 0F D7 /r`, `dst` (32 bits) in reg: the 16 byte sign bits, zero-extended |
| `movmskp_rr(fp, dst: Reg, src: X)` | MOVMSKPS/PD `[66] 0F 50 /r`, `dst` (32 bits) in reg: the element sign bits |
| `pextrw_rri(dst: Reg, src: X, lane: u8)` | PEXTRW `66 0F C5 /r ib`, `dst` (32 bits) in reg: word `lane & 7`, zero-extended |
| `pinsrw_rri(dst: X, src: Reg, lane: u8)`, `pinsrw_rmi(dst: X, src: Mem, lane)` | PINSRW `66 0F C4 /r ib`: the low 16 bits of `src` (the memory form reads 16 bits) into word `lane & 7` |

Each `cvt*` row is `_rr(dst: X, src: X)` and `_rm(dst: X, src: Mem)`. The
`imm8` operands take every `u8` (the instruction uses the bits it needs).
A packed-form instruction has a site exactly when its memory operand is
RIP-relative or marked `with_abs32`; an `imm8` after a RIP-relative
displacement is part of the addend, as for `cmps_rm`.

Not provided: anything from SSE3 and later (`pshufb`, `pminsd`,
`pmulld`, `pcmpeqq`, `blendv*`, `round*`, `ptest`, `pextrb/d/q`,
`pinsrb/d/q`, `movddup`), `maskmovdqu`, the non-temporal stores, and
MMX.

## Relocatable immediates

The non-PIC address materialisations: the immediate itself is the
instruction's site, with addend 0, an **absolute field** like a
`with_abs32` displacement. Each constructor always writes the full-width
immediate, whatever the value, so the field is where the relocation
expects it.

| constructor | encodes | site | relocation | precondition |
|---|---|---|---|---|
| `movabs_ri(dst: Reg, imm: i64)` | MOVABS `REX.W B8+r io` (10 bytes) | `Abs64`, offset 2 | `R_X86_64_64` | |
| `mov_ri32s(dst: Reg, imm: i32)` | MOV `REX.W C7 /0 id`: sign-extended into 64 bits | `Abs32`, the last 4 bytes | `R_X86_64_32S` | |
| `mov_ri32(dst: Reg, imm: u32)` | MOV `B8+r id` (`41 B8+r` for `r8`..): zero-extended into 64 bits | `Abs32U`, the last 4 bytes | `R_X86_64_32` | |
| `mov_mi32s(dst: Mem, imm: i32)` | MOV `REX.W C7 /0 id` on memory | `Abs32`, the last 4 bytes | `R_X86_64_32S` | `!dst.has_field()`: not RIP-relative and not `with_abs32` |
| `push_i32(imm: i32)` | PUSH `68 id` | `Abs32`, offset 1 | `R_X86_64_32S` | |

An instruction has one site, so `mov_mi32s` refuses a memory operand that
is a field itself (`Mem::has_field`); the code generator puts such an
address in a register first. `mov_ri(Size::Q, ..)`, `mov_ri(Size::D, ..)`
and `push_i` remain the forms for a known constant: they choose the
shortest encoding and have no site.

## Fixups and relocations

An instruction has at most one **relocatable field**, its *site*: the
**PC-relative field**, the `rel8`/`rel32` of a branch or call or the
`disp32` of a RIP-relative memory operand, which holds the distance from
the end of the instruction to the target; or an **absolute field**, the
`disp32` of a memory operand marked `with_abs32`, which holds a value (a
thread-pointer offset, a `sym` in `[base + sym]`), or the immediate of a
form of "Relocatable immediates". A RIP-relative operand cannot be
marked, and `mov_mi32s` refuses either, so no instruction has two.

```rust
pub enum Fixup { Rel8, Rel32, Abs32, Abs32U, Abs64 }
impl Fixup {
    pub const ALL: [Fixup; 5];
    pub const fn is_pc_relative(self) -> bool;            // Rel8 and Rel32 only
    pub const fn size(self) -> usize;                     // 1, 4, 4, 4, 8
    pub const fn min(self) -> i64;                        // -128, -(1 << 31), -(1 << 31), 0, i64::MIN
    pub const fn max(self) -> i64;                        // 127, (1 << 31) - 1, (1 << 31) - 1, (1 << 32) - 1, i64::MAX
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
- `site()` is `None` for an instruction without a relocatable field.
  `offset` is the field's offset from the start of the instruction. For
  a PC-relative field `addend` is `offset as i64 - len`: minus the bytes
  from the field to the end of the instruction (including an immediate
  after a RIP-relative operand). For an absolute field (`Abs32`,
  `Abs32U`, `Abs64`) it is 0: the relocation
  computes the value itself (`S + A`, `S + A - TP`, ...). A relocation at `start + offset` with that addend computes
  `S + A - P` = the distance from the end of the instruction to `S`, so
  the same `Site` serves an ELF relocation and an internal fixup: to patch
  for a target at section offset `t`, write `t + addend - (start + offset)`.
- `with_disp(d)` is the instruction with its field set to `d` (for
  `Abs32`, the value); `None` when it has no field or `d` does not fit. Each constructor with a displacement
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
| general-dynamic TLS: the `tls_index` and the call | `tls_gd_lea`, `tls_gd_call` | `R_X86_64_TLSGD`, `R_X86_64_PLT32` (against `__tls_get_addr`) |
| local-dynamic TLS: the module's `tls_index` | `lea(Size::Q, Reg::Rdi, Mem::rip(0))` | `R_X86_64_TLSLD` |
| TLS descriptor | `lea(Size::Q, Reg::Rax, Mem::rip(0))` | `R_X86_64_GOTPC32_TLSDESC` |
| offset of a variable in its module's TLS block | an absolute field | `R_X86_64_DTPOFF32` |
| offset of a variable from the thread pointer | an absolute field | `R_X86_64_TPOFF32` |
| absolute address in the low or high 2 GiB (non-PIC) | an absolute field: a `with_abs32` displacement, `mov_ri32s`, `mov_mi32s`, `push_i32` | `R_X86_64_32S` |
| absolute address in the low 4 GiB (non-PIC) | `mov_ri32` | `R_X86_64_32` |
| absolute 64-bit address (any code model) | `movabs_ri` | `R_X86_64_64` |

The code generator picks the relocation and gives it to the object writer
(`spec/object.md` "Relocations"); this crate names only the place and the
addend.

## Thread-local storage

The access sequences of the x86-64 psABI ("Thread-Local Storage") and
Drepper's *ELF Handling For Thread-Local Storage*, §4.3. A linker that
links the access into an executable rewrites ("relaxes") it into a
cheaper model by recognizing its exact bytes, so each dynamic model has a
fixed form. `x` is the variable; the relocations are those of the table
above, at each instruction's site.

| model | sequence | result |
|---|---|---|
| general dynamic | `tls_gd_lea()`, `tls_gd_call()`: 16 bytes | `rax` = `&x` |
| local dynamic | `lea(Size::Q, Reg::Rdi, Mem::rip(0))` (`TLSLD`), `call(0)` (`PLT32` against `__tls_get_addr`): 12 bytes; then any number of accesses `[rax + x@dtpoff]` through an absolute field (`DTPOFF32`) | `rax` = the module's TLS block |
| initial exec | `mov_rm(Size::Q, r, Mem::rip(0))` or `add_rm(Size::Q, r, Mem::rip(0))` (`GOTTPOFF`), with the thread pointer `mov_rm(Size::Q, r, Mem::abs(0).with_seg(Seg::Fs))` | `r` = `x`'s offset from the thread pointer, or added to it |
| local exec | the thread pointer as above, then `lea(Size::Q, r, Mem::base(r, 0).with_abs32())` (`TPOFF32`); or any access to `fs:[x@tpoff]`, `Mem::abs(0).with_seg(Seg::Fs).with_abs32()` (`TPOFF32`) | `&x`, or `x` itself |

| constructor | encodes |
|---|---|
| `tls_gd_lea()` | `data16 lea rdi, [rip + disp32]`: `66 48 8D 3D cd`, field at offset 4 |
| `tls_gd_call()` | `data16 data16 rex64 call rel32`: `66 66 48 E8 cd`, field at offset 4 |

The `66` and `48` prefixes change nothing for these instructions; they
pad the general-dynamic pair to the 16 bytes of the local-exec sequence
the linker writes over it (`mov rax, fs:0; lea rax, [rax + x@tpoff]`).
The local-dynamic pair is unpadded: the linker writes the 12-byte
`data16 data16 data16 mov rax, fs:0`, or an equivalent, over it. Both
pairs need `x@tlsgd`/`x@tlsld` in `rdi` and the result in `rax` exactly as
given, and nothing between the two instructions. The initial-exec load
may use any register (the linkers relax `mov r64, [rip + x@gottpoff]` to
`mov r64, imm32` and the `add` to `lea` or `add` with an immediate).

## x87

The x87 unit, for `x86_fp80` (`long double`), which the code generator
keeps in memory and loads, operates on and stores within one lowering
(`docs/design/0006-x86-64.md` 2.6). Module `src/x87.rs`, re-exported
from the root like the rest.

**Operands.** `St` is a stack register, `St0`..`St7` (`St::new(n)` is
`None` for `n > 7`; `.code()` the number), counted from the top at the
moment the instruction executes. `X87Size` is the format of a real memory
operand: `S` (32 bits, `dword ptr`), `D` (64, `qword ptr`), `T` (80,
`tbyte ptr`, 10 bytes; `.bits()` 80). An integer memory operand's width is
a `Size` (`W`, `D`, `Q`); the real operand of an arithmetic instruction,
which has no 80-bit form, an `FpSize`. Every x87 instruction is an escape
opcode `D8`..`DF` with either a `ModRM` memory operand (`/digit`) or a
second opcode byte naming `st(i)`. There is no operand-size prefix and
never REX.W; a memory operand takes every form of "Memory operands"
(REX.B and REX.X for `r8`..`r15`, segments, RIP-relative with its site,
absolute fields), and nothing else gets a REX.

**Operand order and names.** Intel's: destination first, as the SDM and
LLVM's Intel syntax write them. A suffix names the register form: `_0i`
is `op st(0), st(i)` (the result in `st(0)`), `_i0` is `op st(i), st(0)`
(the result in `st(i)`), and the `p` forms are `op st(i), st(0)` followed
by a pop, so the result ends up in `st(i-1)`. `_m` is a memory operand,
`_st` a stack-register operand where the mnemonic has both. **The table
states what each form computes; that, not the mnemonic, is the contract.**
AT&T syntax (what `llc` writes, and GNU `as`) swaps `fsub`/`fsubr` and
`fdiv`/`fdivr` in the forms whose destination is `st(i)`, `i != 0` (the
`_i0` and `p` forms): AT&T `fsubrp %st, %st(1)` is `fsubp(St1)`,
`st(1) = st(1) - st(0)`. LLVM's Intel-syntax assembler and its
disassembler use the SDM's meaning, as hod does.

| constructor | encodes | computes | precondition |
|---|---|---|---|
| `fld_m(w: X87Size, src: Mem)` | FLD `D9 /0`, `DD /0`, `DB /5` | push `[src]`, exactly extended | |
| `fld_st(src: St)` | FLD `D9 C0+i` | push a copy of `st(i)` (`St0` duplicates the top) | |
| `fst_m(w, dst: Mem)` | FST `D9 /2`, `DD /2` | `[dst] = st(0)` rounded by the control word | `w` is `S` or `D` |
| `fstp_m(w, dst: Mem)` | FSTP `D9 /3`, `DD /3`, `DB /7` | the same (exact for `T`), then pop | |
| `fst_st(dst: St)` | FST `DD D0+i` | `st(i) = st(0)` | |
| `fstp_st(dst: St)` | FSTP `DD D8+i` | `st(i) = st(0)`, then pop (`St0`: discard the top) | |
| `fild_m(size, src: Mem)` | FILD `DF /0`, `DB /0`, `DF /5` | push the signed 16-, 32- or 64-bit integer, exactly | `size` is not `B` |
| `fist_m(size, dst: Mem)` | FIST `DF /2`, `DB /2` | `[dst] = st(0)` as a signed integer rounded by the control word; out of range or NaN: the "integer indefinite", `1 << (bits - 1)` | `size` is `W` or `D` |
| `fistp_m(size, dst: Mem)` | FISTP `DF /3`, `DB /3`, `DF /7` | the same, then pop | `size` is not `B` |
| `fldz()`, `fld1()` | FLDZ `D9 EE`, FLD1 `D9 E8` | push `+0.0`, `+1.0` | |
| `fadd_m`, `fmul_m`, `fsub_m`, `fsubr_m`, `fdiv_m`, `fdivr_m` `(fp: FpSize, src: Mem)` | `D8` (m32) / `DC` (m64) `/0`, `/1`, `/4`, `/5`, `/6`, `/7` | `st(0) = st(0) + m`, `st(0) * m`, `st(0) - m`, `m - st(0)`, `st(0) / m`, `m / st(0)` | |
| `fadd_0i`, `fmul_0i`, `fsub_0i`, `fsubr_0i`, `fdiv_0i`, `fdivr_0i` `(src: St)` | `D8 C0+i`, `C8+i`, `E0+i`, `E8+i`, `F0+i`, `F8+i` | `st(0) = st(0) + st(i)`, `st(0) * st(i)`, `st(0) - st(i)`, `st(i) - st(0)`, `st(0) / st(i)`, `st(i) / st(0)` | |
| `fadd_i0`, `fmul_i0`, `fsub_i0`, `fsubr_i0`, `fdiv_i0`, `fdivr_i0` `(dst: St)` | `DC C0+i`, `C8+i`, `E8+i`, `E0+i`, `F8+i`, `F0+i` | `st(i) = st(i) + st(0)`, `st(i) * st(0)`, `st(i) - st(0)`, `st(0) - st(i)`, `st(i) / st(0)`, `st(0) / st(i)` | |
| `faddp`, `fmulp`, `fsubp`, `fsubrp`, `fdivp`, `fdivrp` `(dst: St)` | `DE C0+i`, `C8+i`, `E8+i`, `E0+i`, `F8+i`, `F0+i` | as the `_i0` form of the same name, then pop | |
| `fchs()`, `fabs()` | `D9 E0`, `D9 E1` | `st(0) = -st(0)`, `abs(st(0))` (the sign bit) | |
| `fsqrt()`, `frndint()` | `D9 FA`, `D9 FC` | `sqrt(st(0))`; `st(0)` rounded to an integer by the control word | |
| `fprem()`, `fprem1()` | `D9 F8`, `D9 F5` | `st(0) = st(0) - q * st(1)`, `q` truncated (`fmodl`) or rounded to nearest even (`remainderl`); partial: repeat while the status word's C2 is set | |
| `fxch(other: St)` | FXCH `D9 C8+i` | exchange `st(0)` and `st(i)` | |
| `fucomi(other: St)`, `fucomip(other)` | FUCOMI `DB E8+i`, FUCOMIP `DF E8+i` | compare `st(0)` with `st(i)` into ZF, PF, CF as `ucomisd` (unordered: all three; `st(0) < st(i)`: CF), OF, SF, AF cleared; an exception only for a signaling NaN; `p`: then pop | |
| `fcomi(other: St)`, `fcomip(other)` | FCOMI `DB F0+i`, FCOMIP `DF F0+i` | the same, signaling on any NaN | |
| `fucom(other: St)`, `fucomp(other)`, `fucompp()` | `DD E0+i`, `DD E8+i`, `DA E9` | compare `st(0)` with `st(i)` (`fucompp`: `st(1)`) into the status word's C0, C2, C3; pop none, one, two | |
| `fnstsw_ax()` | FNSTSW AX `DF E0` | `ax` = the status word (C0 bit 8, C2 bit 10, C3 bit 14) | |
| `fcmov(cond: Cond, src: St)` | FCMOVcc `DA C0+i` (`B`), `DA C8+i` (`E`), `DA D0+i` (`Be`), `DA D8+i` (`P`, `fcmovu`), `DB C0+i` (`Ae`, `fcmovnb`), `DB C8+i` (`Ne`), `DB D0+i` (`A`, `fcmovnbe`), `DB D8+i` (`Np`, `fcmovnu`) | `st(0) = st(i)` if `cond` holds in EFLAGS | `cond` is one of those eight |
| `fnstcw_m(dst: Mem)`, `fldcw_m(src: Mem)` | FNSTCW `D9 /7`, FLDCW `D9 /5` | store, load the 16-bit control word | |
| `fnclex()`, `fninit()` | `DB E2`, `DB E3` | clear the exception flags; reset the unit (control word `037F`, stack empty) | |

The `_i0` forms with `St0` compute `st(0) = st(0) op st(0)`, which the
`D8` form of the same mnemonic computes too; hod writes that form
(`fsub_i0(St0)` is `D8 E0`), because it is what LLVM's assembler writes
for the text `fsub st(0), st`. Nothing else in this section has two
encodings.

**How `llc` uses them** (`docs/design/0006-data/abi/03-x87.llc*.s`, the
same at LLVM 19 and 22; and `llc -O0` on the other `x86_fp80`
operations): arithmetic is `fldt` of each operand, then `fsub %st(1),
%st` (`fsub_0i`), `faddp %st, %st(1)` (`faddp(St1)`) and the like;
`fptosi` stores the control word (`fnstcw_m`), ORs `0C00` (round toward
zero) into a copy, loads it (`fldcw_m`), converts with `fistp_m` (`fist_m`
for 16 and 32 bits), and reloads the saved word; `fptoui` to 64 bits
compares with `2^63` (`flds` from the constant pool, `fucomi`), selects
`0` or `2^63` with `fldz` and `fcmov(Be, St1)`, `fstp_st(St1)`,
subtracts (`fsubp(St1)`, AT&T `fsubrp`), converts as `fptosi`, and XORs
the sign bit back; `fcmp` is `fucomip(St1)` then `fstp_st(St0)` and a
`setcc`; `select` is `fcmov` (`B`, `Ne`, `P` among those seen); `fneg`,
`fabs`, `sqrt` are `fchs`, `fabs`, `fsqrt`; constants are `fldz`,
`fld1`, `fchs` after them, or `flds` of a narrower constant; `frem` is a
call to `fmodl`; `sitofp` is `fild_m`; `uitofp` from 64 bits adds `2^64`
through a two-entry table indexed by the sign (`flds 0(,%rax,4)`, an
absolute field). A `long double` result is returned in `st(0)` (the second
of a pair in `st(1)`). `FISTTP` (SSE3) is not in the baseline and `llc`
does not use it.

**Not provided** here: the integer-operand arithmetic (`fiadd` and the
others), `fcom`/`fcomp` with a memory operand, `fist`/`fistp` to 8 bits
(they do not exist), `fisttp` (SSE3), `fnstsw m16`, `fnstenv`/`fldenv`/
`fnsave`/`frstor`, `fxsave`, the transcendental instructions (`fsin`,
`fptan`, `fyl2x` and the others), `fscale`, `fxtract`, the waiting forms
(`fstcw`, `fstsw`, `fclex`, `finit`, `fwait`) and `ffree`.

**Tests.** Three groups of `tests/l1`, by the method of "Test method"
below: `x87` (every stack register in every register form, every
`fcmov` condition and each of the eight `Cond`s it lacks, which LLVM
refuses as a mnemonic, the fixed forms, the sampled memory forms and the
rejected widths); `x87-memory-operands` (every form of "Memory operands"
through each of the 30 memory constructor-width pairs, and every form but
RIP-relative marked `with_abs32` through `fld`, `fstp`, `fild` and `fadd`
against `{disp32}` text, each `fld`'s site checked); and `x87-llc-text`
(each x87 instruction `llc` writes for `x86_fp80`, as above, in `llc`'s
own AT&T text, which pins the AT&T swap). `byte_regs` checks the rule of
"Registers and operand kinds" for every byte-register form of the crate
(the eight ALU instructions in four forms, `test`, the `F6`/`FE` group,
`mov` in four forms, `movzx`/`movsx` from a byte, the shifts and rotates
by an immediate and by `cl`, every `setcc`, `xchg`, `xadd`, `cmpxchg`):
an 8-bit operand numbered 4 to 7, in each position, gets the REX prefix
`40` (and an operand-size prefix, if any, before it), operands numbered 0
to 3 get none, and LLVM's disassembler, at every major, reads `spl`,
`bpl`, `sil`, `dil` where intended and never `ah`, `ch`, `dh`, `bh`.

## Processor state and extensions

The instructions of the x86 system and state intrinsics (WP-190,
`src/state.rs`; `spec/codegen.md` §5.6.23 says which function may use
which). Each is checked against `llvm-mc` 19 and 22 like the rest
(`tests/l1/state.rs`).

| constructor | encodes | precondition |
|---|---|---|
| `xstate_m(op: XState, m)` | `FXSAVE`/`FXRSTOR` `0F AE /0`, `/1`; `XSAVE`/`XRSTOR`/`XSAVEOPT` `0F AE /4`, `/5`, `/6`; `XSAVEC`/`XSAVES`/`XRSTORS` `0F C7 /4`, `/5`, `/3`; each `..64` form with `REX.W` | |
| `clflushopt_m(m)`, `clwb_m(m)` | `66 0F AE /7`, `66 0F AE /6` | |
| `rdrand_r(size, r)`, `rdseed_r(size, r)` | `[66] [REX.W] 0F C7 /6`, `/7`, register operand | `size` not `B` |
| `segbase_r(op: SegBase, size, r)` | `RDFSBASE`/`RDGSBASE`/`WRFSBASE`/`WRGSBASE` `F3 [REX.W] 0F AE /0` to `/3`, register operand | `size` `D` or `Q` |
| `rdssp_r(size, r)` | `RDSSPD`/`RDSSPQ` `F3 [REX.W] 0F 1E /1`, register operand (a `nop` without shadow stacks) | `size` `D` or `Q` |
| `incssp_r(size, r)` | `INCSSPD`/`INCSSPQ` `F3 [REX.W] 0F AE /5`, register operand | `size` `D` or `Q` |
| `rdpid_r(r)` | `RDPID r64` `F3 0F C7 /7` | |
| `rdpmc()`, `xgetbv()`, `xsetbv()` | `0F 33`, `0F 01 D0`, `0F 01 D1` (`ecx` selects; `edx:eax`) | |
| `xend()`, `xtest()`, `rdpkru()`, `wrpkru()`, `serialize()` | `0F 01 D5`, `D6`, `EE`, `EF`, `E8` | |
| `xabort(imm: u8)` | `C6 F8 ib` | |
| `xbegin(disp: i32)` | `C7 F8 cd`: the abort target `disp` bytes past the instruction, written as plain bytes (no site: hod writes 0, the next instruction) | |
| `crc32_rr(size, dst, src)` | `F2 [66] [REX] 0F 38 F0 /r` (`src` a byte register; `REX` for `spl`..`dil`), `F1 /r` otherwise; `dst` 32 bits, 64 with `REX.W` for `Q` | |
| `bmi_rrr(op: Bmi, size, dst, x, y)` | three-byte VEX (`C4`, map `0F38`, `L` 0, `W` for `Q`): `BEXTR` `F7`, `BZHI` `F5` (`x` in `ModRM.rm`, `y` in `vvvv`); `PDEP` `F2.F5`, `PEXT` `F3.F5` (`x` in `vvvv`, `y` in `ModRM.rm`); operands in Intel's order | `size` `D` or `Q` |

## Not provided

By decision of the scope so far: the high-byte registers `ah`..`bh`; the
string instructions other than `rep_movs`/`rep_stos` (`cmps`, `scas`,
`lods`, `repne`, the forms without `rep`, `std`); `xadd` and `cmpxchg`
with a register destination; `movabs` to and from the accumulator with a
64-bit address; `enter`; 16-bit `push`/`pop`; far branches; `jrcxz` and
`loop`; `hlt` and the system instructions other than `rdtsc`, `rdtscp`,
`cpuid` and those of "Processor state and extensions"; a relocatable immediate other than those of "Relocatable
immediates" (an `imm32` of an ALU instruction, for one); the `-fno-plt` general-dynamic call through
the GOT (`data16 rex64 call [rip + __tls_get_addr@GOTPCREL]`) and the
descriptor call `call [rax]`, whose `R_X86_64_TLSDESC_CALL` marks an
instruction rather than a field; the SSE instructions not listed in
"Scalar floating point" or "Packed SSE2" (SSE3 and later: `rounds`,
`pshufb`, ...); MMX; and everything VEX- or EVEX-encoded (AVX, APX) but the four BMI
operations. Adding an instruction is a constructor, a
row here, a row in `tests/l1/table.rs` and a sweep.

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

The `xmm-memory-operands` sweep takes every form of "Memory operands"
through a scalar load and store with every `xmm` in the reg field and
through a 128-bit load; `absolute-fields` takes every form but
RIP-relative, marked `with_abs32`, through a load with every register, a
store with an immediate and `lea`, against LLVM's `{disp32}` text, and
checks each load's site (`Abs32`, the last four bytes, addend 0).
`packed-memory-operands` takes every form through `paddd` with every
`xmm` in the reg field, and through `pshufd` and `cmpltpd` (an `imm8`
after the displacement). `locked-memory-destinations` takes every form
at every size through `lock` with `add`, `sub`, `and`, `or`, `xor` and
`xadd` (the register cycling through all sixteen), the same five with an
immediate, and `inc`, `dec`, `not`, `neg`. `relocatable-immediates`
checks each form's site (kind, the last bytes, addend 0) on every
register and the field's boundary values; `push_i32` is swept with values
that are not a sign-extended byte, where LLVM's text has the same form
(its other values are the zero form patched, in `fixups`).

**Coverage.** `coverage::every_constructor_is_swept` reads the crate's
source for its public constructors (as `hod_mc_a64`'s does) and fails if
one has no row in the table, no case in any sweep, or (for a checked form)
no rejected operands, or if the sweeps together fall below 250,000
encodings.

**The harness itself.** `selftest` gives the harness batches that disagree
with LLVM (wrong bytes, a wrong length, a wrong instruction, bytes that
decode as two instructions, a wrong family member, text LLVM refuses, a
rejection LLVM accepts) and requires each to be reported, by every major.
The disassembler's `lock` and `rep` prefixes are part of the mnemonic
(`lock add`, `rep movsb`).

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
two GOTPCRELX kinds (104 instructions now, with the RIP-relative SSE,
packed and bit-test operands, every TLS relocation of the table,
`R_X86_64_32S` on a displacement and on the immediates, `R_X86_64_32` and
`R_X86_64_64`);
and `a_laid_out_function_matches_llvm`: a function
with forward and backward short and near branches, a call, RIP-relative
operands with and without an immediate after the field, under `lock` and
a segment override, and padding, encoded with zero fields and patched
through its sites, equals LLVM's assembly of the same source.

**TLS through the linkers**, `link`: for each model of "Thread-local
storage" (general dynamic; local dynamic with two `DTPOFF32` accesses;
initial exec with `mov` and `add` into six registers, `r12` and `r13`
among them; local exec in all three forms) an object is assembled from
hod's bytes with a `.reloc` at each of hod's sites and addends, and one
from the compiler's text for the same sequence; their code and
relocations must be equal, at every major. Both objects are linked by GNU
ld (`x86_64-linux-gnu-ld.bfd` on a non-x86-64 host), lld and mold, each
with `-m elf_x86_64 --fatal-warnings`, into a static executable, a PIE and
(except local exec) a shared object, silently, into identical files. In
an executable the dynamic models' first instruction has been rewritten;
in a shared object it has not, and the dynamic relocations
`R_X86_64_DTPMOD64` (general and local dynamic) and `R_X86_64_TPOFF64`
(initial exec) are present. The programs are not run. The control,
`tls_padding_is_what_the_linkers_see`: the general-dynamic pair without
its prefixes is refused (GNU ld 2.44: "TLS transition ... failed") or
rewritten into other bytes (lld 22, mold 2.37: into a load without the
`fs` override, that is, wrong code), so a missing prefix cannot pass.

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
- LLVM accepts a RIP-relative or symbolic displacement together with a
  symbolic immediate (two relocations); `mov_mi32s` refuses a memory
  operand that is a field; tested by hod alone.

Every encoding the crate produces is the one LLVM's assembler produces for
the same text, at both majors; there is no choice where they differ.
