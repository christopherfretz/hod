# 32-bit x86 code generation (`hod_x86`)

Design: `docs/design/0008-i386.md`. Convention: `spec/abi-x86.md`.
Target-independent rules: `spec/codegen.md`. This file is what
`crates/hod_x86` does; each section names the package that owns it. The
module table (who extends what next) is `crates/hod_x86/README.md`.

## 1. Scope (WP-293, milestone A)

`hod_x86::X86` implements `hod_cg::Target` for `i386`/`i486`/`i586`/
`i686-*-linux-gnu` in the static relocation model. It lowers values of
class `Gpr`: `i1` to `i32` and `ptr`. Every other type is class `Mem` and
every instruction that would need one is refused (§7). A module is never
miscompiled: what is not lowered is refused under a stable `x86:` name.

Every instruction goes through `hod_mc_x64`'s encoder in its 32-bit mode
(`Mode::X86.admit`, `common::put`). An instruction the mode refuses is a
bug of the lowering: `ud2` stands for it and the function ends in an
internal error naming the mode's refusal. Padding is `nop` (`0x90`, one
byte per byte); functions are aligned to 16 and `.text` to 4, as `llc`
writes them. hod emits no `endbr32`, never `syscall`, and no `cmov`.

## 2. Registers

| index | register | role | DWARF |
|---|---|---|---|
| 0 | `eax` | result, dividend, `select` condition, `ptradd` index; never holds a value; no glue writes it | 0 |
| 1 | `ecx` | scratch (shift count, glue) | 1 |
| 2 | `edx` | scratch (glue, remainder), the callee register | 2 |
| 3 | `ebx` | pool, callee-saved | 3 |
| 4 | `esp` | stack pointer | 4 |
| 5 | `ebp` | frame pointer | 5 |
| 6 | `esi` | pool, callee-saved | 6 |
| 7 | `edi` | pool, callee-saved | 7 |
| 8 + n | `xmm<n>` | `xmm0`..`xmm5` pool, `xmm6`, `xmm7` scratch; unused until I12 | 21 + n |

`RegInfo`: pool = holdable = `ebx`, `esi`, `edi` (and `xmm0`..`xmm5`);
scratch `ecx`, `edx`, `xmm6`, `xmm7`; landing-pad data `eax`, `edx`;
`gpr_bytes` 4. `hod_cg`'s pool minimum is 3 general registers for a
4-byte target (`MIN_POOL_GPR_32`, `RegInfo::min_pool_gpr`), and the
`min_pool` stress mode uses it. `esi`, `edi`, `ebp`, `esp` have no
low-byte register in 32-bit mode: a byte store or a `setcc` of a value in
`esi`/`edi` goes through `ecx`/`edx`.

## 3. Frame

```text
push ebp; mov ebp, esp; push ebx; push esi; push edi; sub esp, FRAME
...
lea esp, [ebp - 12]; pop edi; pop esi; pop ebx; pop ebp; ret
```

Every function saves the three pool registers. `FRAME` (an `imm32`,
patched at the end of the function) is the local area rounded up to
`≡ 12 (mod 16)`, so `esp` is 16-aligned after the prologue and at every
call. A local `Local(o)` is `[ebp + o - 12 - FRAME]` (a `disp32` biased at
the end of the function); an incoming argument `Incoming(n)` is
`[ebp + 8 + n]`; an outgoing one `[esp + n]`. The DWARF frame base is
`ebp`, the frame description `12 + FRAME` bytes below it. The CFI rows are
recorded (CFA `ebp + 8`, `ebp` at -8, `ebx`, `esi`, `edi` at -12, -16,
-20), but no `.eh_frame` is written (I10): `hod_cg`'s unwind tables skip
`EM_386`.

## 4. Integers (`alu`)

A value narrower than 32 bits leaves the bits above its width unspecified;
an operation that reads them extends first: by zero for `lshr`, `udiv`,
`urem`, an unsigned compare, a `switch`, `zext`, `inttoptr`; by sign for
`ashr`, `sdiv`, `srem`, a signed compare, `sext`, a `ptradd` index.
`add`, `sub`, `mul`, `and`, `or`, `xor`, `shl` work on 32 bits and need
no extension. Division: dividend in `eax`, `cdq`/`idiv` or zeroed
`edx`/`div`; the quotient (`eax`) or the remainder (`edx`) is moved to the
divisor's register. A shift by a register goes through `cl`; a constant
shift of at least the width writes nothing (the result is poison).
`icmp`: `cmp`, `setcc` (into `cl` when the result's register has no low
byte), `movzx`. `select`: always a branch (`test al, 1`; `je`; `mov`).
`freeze` is a copy. Casts among `Gpr` types: `trunc`, `ptrtoint`,
`ptrtoaddr`, `bitcast` copy; `zext`, `inttoptr`, `sext` extend.

### 4.1 Integer intrinsics (`bits`, WP-309)

The integer intrinsics with a result of 1 to 32 bits (wider: §8), in
the first operand's register; each operand is asked for once (a repeated
operand shares its register), so at most three pool registers are
pinned; the work registers are `eax`, `ecx`, `edx`. Every `cmov` of
`hod_x64`'s shape is a branch, on every CPU. `popcnt`, `lzcnt`, `tzcnt`
are never used (hod's feature model has `popcnt` but no `lzcnt`/`bmi`,
and the back end reads no feature yet): the generic sequences below
give the same results. `is_zero_poison` and `is_int_min_poison` are
ignored (the defined result is computed). With `N` the width:

- `bswap` (`N` = 16, 32; the verifier rejects other widths): `bswap`,
  then `shr 32 - N`. `bitreverse`: `bswap` and the three mask-and-shift
  swaps, then `shr 32 - N`. Both drop the unspecified bits.
- `ctpop`: zero-extend, the SWAR sequence (`imul 0x01010101`, `shr 24`).
- `ctlz`: `N` = 32: `bsr`; `jne`; `mov 63`; `xor 31`. `N` < 32: `shl 32 - N`,
  `or` of bit `31 - N` (a zero counts `N`), `bsr`, `xor 31`.
- `cttz`: `N` = 32: `bsf`; `jne`; `mov 32`. `N` < 32: `or` of bit `N`, `bsf`.
- `abs`: sign-extend, `(x ^ s) - s` with `s = x >> 31` (the minimum maps to
  itself).
- `smax`, `smin`, `umax`, `umin`: both extended (sign or zero), `cmp`, a
  branch over the copy of the second operand.
- `sadd.sat`, `ssub.sat`, `uadd.sat`, `usub.sat`: `N` = 32: the operation's
  own `OF` (the bound chosen from the first operand's sign before it) or
  `CF`, a branch over the bound. `N` < 32: both extended, the exact 32-bit
  result clamped to the width's bounds by compares and branches (`usub`:
  negative means 0).
- `sshl.sat`, `ushl.sat`: the amount zero-extended into `cl`, the value
  moved to the top of the register (`shl 32 - N`), shifted left and back
  (`sar`/`shr cl`); if that differs, the saturated value (all ones, or
  `(x >> 31) ^ 0x7fffffff`); then `sar`/`shr 32 - N`. An amount of `N` or
  more is poison.
- `fshl`, `fshr`: a constant amount is taken modulo `N` (0: the first,
  or second, operand unchanged); `N` = 32: `shld`/`shrd` (by `cl` for a
  variable amount, which the instruction takes modulo 32). `N` < 32: the
  second operand zero-extended in `eax`, the two halves shifted by `k`
  and `N - k` and or-ed; a variable `k` is `c and (N - 1)` for a power of
  two, else the remainder of a `div` by `N` (`c` zero-extended); `N` = 1
  is the unchanged operand.
- `scmp`, `ucmp` of operands of 1 to 32 bits: both extended, `cmp`,
  `setg`/`seta` into `al`, `setl`/`setb` into `cl`, `sub al, cl`, `movsx`;
  a result of 33 to 64 bits (class `Mem`) is stored as that word and its
  sign. Operands wider than 32 bits stay `x86:intrinsic`.

Where `llc` 19 and 22 (`-O0 -global-isel=0 -fast-isel=0`) branch inside
these intrinsics (a basic-block label in the function; measured with
`tests/data/i386/bits/gen.py --probe` and `tests/data/i386/i64_rest/branches.py`
at `N` = 1, 7, 8, 16, 24, 31, 32): only at a CPU without `cmov`
(`generic`): `ctlz` with `is_zero_poison` false at every `N` but 1;
`cttz` with `is_zero_poison` false at 32 only (with it true, neither
branches); `smax`, `smin`, `umax`, `umin` at every `N`
but 1; `sadd.sat`, `uadd.sat`, `ssub.sat`, `usub.sat` at every `N` but 1
(LLVM 22) or at every `N` (LLVM 19); `sshl.sat`, `ushl.sat` at every `N`
but 1. With `cmov` (`pentium4`, `haswell`) none. `bswap`, `ctpop`,
`bitreverse`, `abs`, `fshl`/`fshr` (constant or variable amount),
`scmp`/`ucmp` never branch.

## 5. Memory (`mem`, `bytes`)

An address is a stack slot's frame place, a symbol's absolute `disp32`
(`R_386_32`), or a register; nothing is fused. Loads of 1 and 2 bytes are
`movzx`; 3 bytes (an `i24`) are two loads; 4 a `mov`. A `ptradd` puts its
base in a register, adds each index sign-extended and scaled (`lea` for
scales 1, 2, 4, 8, else `imul`) and the constant offset, modulo 2^32. A
static `alloca` is a stack slot of the generic layout.
`llvm.memcpy`/`memmove`/`memset` (and `.inline`) with a length of at most
32 bits are `rep movsb`/`rep stosb`, saving `esi`/`edi` around them;
`memmove` copies backward (`std` ... `cld`) when the destination is above
the source.

## 6. Control flow and calls (`control`, `call`, `abi`, `frame`)

`br`, `condbr` (`test r, 1`), `switch` (a `cmp`/`je` chain in case
order, then the default), `ret` (the value in `eax`), `unreachable` and
`llvm.trap` (`ud2`). Branches are `rel8` backward when they reach, else
`rel32`, relaxed by `hod_cg`. Calls go through `hod_cg`'s call sequence
with `abi::assign`: the C convention, every argument on the stack at the
next multiple of 4, its store size wide, `zeroext`/`signext` extended to
32 bits by the producer; the result in `eax`. A direct call is
`call rel32` with `R_386_PC32` for a callee in this output and
`R_386_PLT32` otherwise (as `llc`'s static model); an indirect call is
`call edx`. Variadic calls are made (the variable arguments are typed by
their values).

## 7. Refusals

| name | what | removed by |
|---|---|---|
| `x86:i64` | an integer of 33 to 64 bits where §8 does not take it: since the i386 join C only an integer of 33 to 63 bits passed or returned by value (also a variadic argument), and an `i64` length of `memcpy`/`memmove`/`memset` | I6 (`abi.rs`: `i64` arguments, results); WP-306 (`switch` condition, `ptradd` index, intrinsics); the memory intrinsics' package |
| `x86:i64:wide` | an integer wider than 64 bits (§8), also as a `switch` condition, a `ptradd` index and the result of an integer intrinsic of §8 | — |
| `x86:float` | a floating-point value in a register where §11 takes none (arguments and results are I6's since the join) | — |
| `x86:float:half` | a `half` value in a function without SSE2; a `half` parameter or result of `fastcc` or `inreg` (§12) | — |
| `x86:float:half:intrinsic` | an intrinsic on `half` (§12) | — |
| `x86:float:bfloat`, `x86:float:fp128` | a value of those types | — |
| `x86:float:i64-cast` | a conversion between a floating type and an integer of 33 to 63 or more than 64 bits (§11; `fptoui` to `i64` is lowered since the i386 join C) | — |
| `x86:float:intrinsic` | a floating-point intrinsic §11 does not lower | — |
| `x86:float:cast` | a `bitcast` of a floating value, another cast not in §11 | — |
| `x86:vector` | a vector `ptradd` | I12 |
| `x86:vector:no-sse2` | a vector in a function without SSE2 | — |
| `x86:vector:wide` | a vector of more than 16 bytes passed or returned | — |
| `x86:vector:abi` | a vector of `i1`, of one lane or of odd-width integers passed or returned | — |
| `x86:vector:element` | lanes of `half`, `bfloat`, `fp128`, `x86_fp80`, odd-width integers | — |
| `x86:vector:scalable` / `:length` | a scalable vector; more than 256 lanes | — |
| `x86:vector:i64-op` | `i64` lanes: an operation other than `add`, `sub`, bitwise, compares, moves | — |
| `x86:vector:frem` / `:cast` | `frem`; casts with unsigned 32-bit or 64-bit lanes and floating point | — |
| `x86:vector:intrinsic` | an intrinsic on vectors other than the integer and `i1` reductions | — |
| `x86:aggregate` | an aggregate value, `extractvalue`, `insertvalue`, a result in memory | I11 |
| `x86:eh` | `invoke`, `landingpad`, `resume`; a call that may unwind in a function that may unwind (no unwind tables) | I10 |
| `x86:debug-info` | a module with debug information (`hod_cg`'s emitter) | I10 |
| `x86:cf-protection` | `cf-protection-*` module flags (`hod_cg`'s `.note.gnu.property`) | — |
| `x86:atomic` | an ordered load or store, `fence`, `cmpxchg`, `atomicrmw` | I11 |
| `x86:inline-asm:<detail>` | what inline assembly does not take (§10, `spec/asm.md` §22.5) | I13 (`asm.rs`) |
| `x86:abi:cc` | a calling convention other than C, `coldcc` and `fastcc`; `fastcc` with a floating-point argument or result, an `sret` or a result in memory (`spec/abi-x86.md` §1.2) | — (`fastcc` floating point: I12) |
| `x86:abi:attr` | the Swift attributes, `inalloca`, `preallocated`, `alignstack`; `sret` other than on the first parameter or on a non-pointer; `inreg` on a floating-point or aggregate argument or in a variadic signature; `byval` above alignment 16 or with `inreg`; `nest` twice or on a taken register (`spec/abi-x86.md` §6.6) | — |
| `x86:abi:wide-arg` | an `i128` or `fp128` argument (`llc` 19 and 22 place it differently, `spec/abi-x86.md` §12) | — |
| `x86:abi:x87-pair` | a result with two floating-point leaves (`st0`, `st1`) | I9 |
| `x86:abi:indirect-edx` | an indirect call with an argument in `edx`, which holds the callee's address (`spec/abi-x86.md` §8.1) | — |
| `x86:abi:musttail` | a `musttail` call with stack arguments and an argument in `ecx`; under PIC, one to another module's function with `eax`, `ecx` and `edx` all holding arguments (`spec/abi-x86.md` §8.5) | — |
| `x86:varargs` | `va_start`, `va_copy`, `va_end`, `va_arg` | I11 |
| `x86:dynamic-alloca` | a dynamic `alloca`, an over-aligned slot, `llvm.stacksave`/`stackrestore` | I11 |
| `x86:intrinsic` | any other intrinsic (the integer intrinsics of §4.1 and §8 are lowered; still refused: the `*.with.overflow` family, `eh.typeid.for`, `frameaddress`, `returnaddress`, `ubsantrap`, `ptrmask`, `is.constant`, `objectsize`, `clear_cache`, `readcyclecounter`, ...) | per family |
| `x86:call` | a call with operand bundles, `callbr` | — |
| `x86:blockaddress`, `x86:indirect-branch` | `blockaddress`, `indirectbr` | — |
| `x86:stack-protector` | a function with a stack guard | — |
| `x86:addrspacecast`, `x86:int-cast` | those casts | — |
| `x86:constant` | a constant expression or other constant reaching a register | — |
| `x86:funclet` | a funclet pad | — |

A nonzero `NumRegisterParameters` module flag stays refused by the front
half. A test does not name the first refusal of a program with several
constructs.

## 8. Integers of 33 to 64 bits (`int`, WP-298)

An integer `iN`, `33 <= N <= 64`, is of class `Mem` (design 0008 §1.4):
its home is an 8-byte frame slot, low word at offset 0, high word at 4.
The bits above `N` in the high word are unspecified; an operation that
reads them extends the high word first from its `N - 32` bits (by zero
for `lshr`, `udiv`, `urem`, an unsigned or equality compare, `zext`, a
`store`; by sign for `ashr`, `sdiv`, `srem`, a signed compare, `sext`).
Lowerings work in `eax`, `ecx`, `edx` only and read every operand word
before writing the result's slot; the glue copies a slot through `ecx`
and writes a constant with immediate stores (a `ptrtoint` constant
through `ecx`). Wider than 64 bits is refused as `x86:i64:wide`.

- `add`/`sub`: `add`/`adc` (`sub`/`sbb`) of the halves; `and`, `or`,
  `xor` half by half; `freeze`, `bitcast` between integers, `trunc` to
  another wide width: a copy.
- `mul`: `lo:hi = a0·b0` (`mul`), `hi += a0·b1 + a1·b0` (`imul`).
- Shifts by a constant `k`: `shld`/`shrd` and a shift for `k < 32`, a
  word move and a shift (the vacated word 0, or the sign for `ashr`) for
  `k >= 32`; `k >= N` is poison and writes nothing. By a variable: the
  amount's low word in `ecx`, `shld`/`shrd` and a shift by `cl`, then
  `test cl, 32` and the word move.
- `udiv`, `sdiv`, `urem`, `srem`: calls of libgcc's `__udivdi3`,
  `__divdi3`, `__umoddi3`, `__moddi3` (`R_386_PLT32`), made by `int`
  itself: divisor then dividend pushed high word first (extended), the
  result in `edx:eax`, `add esp, 16`. These four symbols are undefined
  in an object that divides 64-bit integers. The call is `pic::emit_call`
  (§9.1): under PIC through the PLT with the GOT pointer in `ebx`, as
  `llc` does; a libgcc linked as a shared library (`-lgcc_s`) is reached
  through a real PLT entry, which reads `ebx`.
- Symbols (§9): a wide integer's memory is addressed through a register
  only: `load`/`store` of a symbol take its address into `eax`
  (`common::sym_address`, i.e. `pic::address` or `tls::address`, which
  write `eax` alone) before any word is moved through `ecx`/`edx`; a
  thread-local symbol's address is the pointer operand's register (a
  value materialized as any other). Nothing of `int` builds an
  instruction through `pic::mem_inst`, so its `eax` rule is never met;
  a `ptrtoint` constant goes through `ecx` (`common::materialize_const`).
- `icmp`: equality as `xor` of each half and `or` of the two; order as
  `cmp` of the low halves and `sbb` of the high halves, operands swapped
  for `>` and `<=` so that only carry, sign and overflow are read; then
  `setcc` and `movzx` into the `i1` result.
- `select`: `test al, 1` and a copy of the chosen arm on each side of a
  branch, on every CPU.
- Casts: `zext`, `sext`, `ptrtoint` from a `Gpr` value store its
  register (extended) and a high word of 0 or the sign; `trunc` and
  `inttoptr` to a `Gpr` value load the low word. `load`/`store` move
  exactly the store size (5 to 8 bytes).

- `switch` (WP-306): the low word in `eax`, the high word zero-extended
  in `edx`; per case (its value masked to `N` bits) `cmp eax, lo`,
  `jne` past it, `cmp edx, hi`, `je` the case's block; then the default.
  A constant condition is a branch to its block.
- `ptradd` index (WP-306): the low word of the slot, i.e. the index
  truncated to the 32 bits of a pointer (as `llc`), then scaled as a
  `Gpr` index (§5); a constant index adds its low 32 bits times the scale
  to the offset.
- Intrinsics (WP-306; `call::lower` sends those whose result is wide to
  `int`), all in `eax`, `ecx`, `edx`, with a frame slot of its own where
  a lowering needs more (`alloc_slot`, never reused):
  - `ctpop`: the sum of each word's count (shifts, masks and a multiply;
    the high word zero-extended); `ctlz`: `bsr` of the zero-extended
    high word if it is not zero, else 32 plus that of the low word, 64
    for zero, less `64 - N`; `cttz`: `bsf` of the low word if it is not
    zero, else 32 plus that of the high word with bit `N - 32` set (so
    `N` for zero; 64 for a zero `i64`). Both branch. `is_zero_poison` is
    ignored (the defined result is computed).
  - `bswap` (`N` a multiple of 16) and `bitreverse`: each word reversed
    (`bswap`; then nibbles, pairs, bits) and the words exchanged, the
    pair shifted right by `64 - N` (which drops the unspecified bits).
  - `abs`: `(x ^ s) - s` over the halves, `s` the sign of the
    sign-extended high word; the minimum maps to itself.
  - `smax`, `smin`, `umax`, `umin`: the order compare of `icmp` (`sge`,
    `sle`, `uge`, `ule`) and a copy of the chosen operand on each side
    of a branch.
  - `uadd.sat`, `usub.sat`: zero-extended `add`/`adc` (`sub`/`sbb`); the
    bound (all ones, or 0) on a carry out of 64 bits, or for an add of
    fewer bits when bit `N` of the sum is set. `sadd.sat`, `ssub.sat`: of
    64 bits, on overflow the bound of the sign opposite to the wrapped
    result's; of fewer, the operands sign-extended (no 64-bit overflow),
    the bound when the high word differs from itself sign-extended from
    `N - 32` bits. The bound of sign mask `s` is low `~s`, high
    `s ^ (2^(N-33) - 1)`.
  - `sshl.sat`, `ushl.sat` by `k < N` (more is poison): `x << (64 - N)`
    kept in a slot, shifted left by `k` (into the result's slot, every
    operand read) and back right (arithmetic or logical); if that
    differs, the bound (of the sign, or all ones), else the shifted value
    shifted right by `64 - N`.
  - `fshl`, `fshr`: `B = b << (64 - N)` in a slot, the amount `k = c mod
    N` (masked by 63 for `N = 64`, else two `div` by `N` of the
    zero-extended amount, high word then low; a constant is reduced at
    compile time), then the 64-bit funnel shift of `a:B` by `k`: three
    words of `a:B` chosen by bit 5 of `k` (`test cl, 32` and a branch, or
    at compile time) and two `shld`/`shrd` by `k mod 32`; `fshr`'s result
    shifted right by `64 - N`. No libgcc call (`llc` calls `__umoddi3`
    for a variable amount of a width that is no power of two; the result
    is the same).
  - Wider than 64 bits: `x86:i64:wide` (an odd width above 64 is refused
    before, by the shared classifier, as `intrinsic:<name>/int-wide`).
- `scmp`, `ucmp` of operands of 33 to 64 bits (i386 join C;
  `int::three_way`), a result of 2 to 64 bits: `a > b` by the order
  compare above and `setcc`, kept on the stack (`push`), then `a < b`
  likewise; the result is their difference (1, 0, -1), its high word
  sign-extended for a result wider than 32 bits. No branch. Operands or
  a result wider than 64 bits: `x86:i64:wide`.

Still refused: such an integer of 33 to 63 bits passed or returned by
value (`x86:i64`, `abi.rs`; `i64` itself is I6's). The `*.with.overflow` intrinsics of every width return
the aggregate `{iN, i1}`: their call stays `x86:intrinsic` and their
`extractvalue` `x86:aggregate` (I11). Every other intrinsic of these
widths stays `x86:intrinsic`.

Where `llc` 19 and 22 (`-O0 -global-isel=0 -fast-isel=0`) branch inside
these intrinsics (a basic block label in the function; measured with
`tests/data/i386/i64_rest/gen.py --probe` and `branches.py`): at
`N` = 33, 48, 57, 64 and a CPU without `cmov` (`generic`), `ctlz` and
`cttz` (both `is_zero_poison` values), `smax`, `smin`, `umax`, `umin`,
`sadd.sat`, `uadd.sat`, `ssub.sat`, `usub.sat`, `sshl.sat`, `ushl.sat`,
and `fshl`/`fshr` by a variable amount; with `cmov` (`i686`,
`pentium4`) none. `bswap`, `ctpop`, `bitreverse`, `abs` and funnel
shifts by a constant never branch. A `switch` is a terminator.

## 9. Position-independent code and TLS (`pic`, `tls`; WP-299, design 0008 I8)

### 9.1 The access choice

Every symbol reference goes through `pic`: `common::sym_address` (an
address in a register) is `pic::address`, `common::sym_inst` (a memory
operand) is `pic::mem_inst`, `Target::emit_call` is `pic::emit_call`.
The choice follows `llc` 19 and 22 (`X86Subtarget::classifyGlobalReference`
for 32-bit ELF) on top of `hod_cg::sym_access`; functions and data alike:

| relocation model | `dso_local` (local, hidden, protected, PIE definitions) | preemptible (declarations, default-visibility definitions in a shared object) |
|---|---|---|
| static, dynamic-no-pic: data, function address | `[sym]` / `lea rd, [sym]`, `R_386_32` | the same, `R_386_32` |
| static: call | `call f`, `R_386_PC32` | `call f@PLT`, `R_386_PLT32`, no `ebx` |
| PIC: data, function address | GOT pointer, `sym@GOTOFF(r)`, `R_386_GOTOFF` (addend folded) | GOT pointer, `mov r, sym@GOT(r)`, `R_386_GOT32X`, the addend added by `lea` |
| PIC: call | `call f`, `R_386_PC32` | GOT pointer in `ebx`, `call f@PLT`, `R_386_PLT32` |

A program (PIE) and a shared object differ only through `sym_access`
(`dso_local` as LLVM implies it; a shared object's default-visibility
definition is reached through its local alias, `hod_cg` §9.7). `nonlazybind`
does not change the 32-bit call at `-O0` (`llc` 22 measured). Runtime
functions are called as preemptible ones.

### 9.2 The GOT pointer

Computed where it is needed, never kept: `call .L` (`e8 00 00 00 00`, no
relocation), `.L: pop r`, `lea r, [r + disp32]` with `R_386_GOTPC`
against `_GLOBAL_OFFSET_TABLE_`, addend 3 (from `.L` to the field). `llc`
writes the same call/pop with `addl` (no `__x86.get_pc_thunk.*` at
`-O0`, both majors); `lea` keeps the flags. The sequence writes `r` and
the word below `esp` only.

- `pic::address` computes it in `rd` itself: it writes `rd` and nothing
  else, flags kept, at any point of a lowering.
- `pic::mem_inst` computes it in `eax` (a thread-local symbol's address
  too, in every model): `eax` holds no value across instructions, and the
  instruction built over the operand must not read `eax`, neither as its
  register operand (a store of `al`/`ax`/`eax`, an operation, `xchg`,
  `cmov`, two-operand `imul`, `xadd`, `bt*`, `shld`/`shrd` with `eax`)
  nor implicitly (`mul`, `imul`, `div`, `idiv` of memory, `cmpxchg`,
  `cmpxchg8b`). Such an instruction is an internal error
  (`pic::reads_eax`), never emitted. No valid program reaches it today:
  the only caller is `mem`'s load and store, whose register is a value's
  or a temporary's, from the pool (`ebx`, `esi`, `edi`: `eax` is in
  neither `pool` nor `holdable`, and a call's result is moved out of
  `eax` at once, `hod_cg` §4.4) or `ecx`/`edx` (`scratch`);
  `tests/data/i386/pic/eax.ll` stores call results, quotients and
  arguments of each width to preemptible and hidden globals (also under
  `Stress::ALL`). I7's `i64` halves (§8) do not use `mem_inst`: they
  address a symbol through `eax` taken by `common::sym_address` first.
  **A package that builds other instructions over a
  symbol** (I9's x87 and SSE accesses, I11's atomics)
  passes no `eax` operand and no instruction that uses `eax` implicitly:
  it takes the symbol's address into another register
  (`common::sym_address`) and addresses through it instead. The x87
  escapes (`D8`..`DF`) and SSE forms name no general register and pass
  the check.
- `pic::emit_call` displaces the value `ebx` holds (`FnCx::take_reg`)
  and computes it in `ebx` immediately before `call f@PLT` (i386 psABI).
  It is the one way to call a symbol: `hod_cg`'s call sequence
  (`Target::emit_call`) and the calls a lowering makes itself (§8's
  libgcc divisions) alike; only `tls`'s `___tls_get_addr` sequence, which
  sets `ebx` itself, calls `frame::emit_call` directly.

### 9.3 Tables and constants

hod builds no jump tables (`switch` is a compare chain). There are no
constant pools yet; I9's constants reach memory through `pic::mem_inst`
with the pool's symbol, which gives `@GOTOFF` under PIC as `llc` does.

### 9.4 Thread-local storage

The thread pointer is `gs:[0]`. The model is `hod_cg::sym_access`'s
(`TargetMachine::getTLSModel`: a program's are local or initial exec
whatever `-ftls-model=` asks for; a shared object's are general or local
dynamic unless initial exec is asked). Both ways in (a symbol operand,
`llvm.threadlocal.address`) are `tls::address`, which writes `rd` only,
the flags kept, except `llvm.threadlocal.address` of a dynamic model,
lowered as a call (below); `llvm.thread.pointer` is `mov rd, gs:[0]`.

| model | sequence | relocation |
|---|---|---|
| local exec | `mov rd, gs:[0]`; `lea rd, [rd + x@NTPOFF + addend]` | `R_386_TLS_LE` |
| initial exec, static | `mov rd, [x@INDNTPOFF]` (`A1` for `eax`, else `8B` r/m=101) | `R_386_TLS_IE` |
| initial exec, PIC | GOT pointer in `rd`; `mov rd, [rd + x@GOTNTPOFF]` (disp32) | `R_386_TLS_GOTIE` |
| initial exec, then | `push t; mov t, gs:[0]; lea rd, [rd + t]; pop t`; `lea rd, [rd + addend]` (`t` = `ecx`, `edx` if `rd` is `ecx`) | — |
| general dynamic | GOT pointer in `ebx`; `lea eax, [ebx*1 + x@TLSGD]` (`8D 04 1D`, SIB without base); `call ___tls_get_addr@PLT`; `mov rd, eax` | `R_386_TLS_GD`, `R_386_PLT32` |
| local dynamic | GOT pointer in `ebx`; `lea eax, [ebx + x@TLSLDM]` (`8D 83`); `call ___tls_get_addr@PLT`; `lea rd, [eax + x@DTPOFF]` | `R_386_TLS_LDM`, `R_386_PLT32`, `R_386_TLS_LDO_32` |
| dynamic, then | `lea rd, [rd + addend]` | — |

The initial-exec loads are the forms GNU ld, lld and mold relax to local
exec in a program; measured on linked programs (hod's objects): GNU ld
and lld rewrite `mov rd, [rd + x@GOTNTPOFF]` and `mov rd, [x@INDNTPOFF]`
into `mov rd, imm32` (`C7 C0+r`), mold keeps the GOT load (correct, not
relaxed). An initial-exec access to a variable of a shared library is
not relaxed by any linker and runs (`i386_tls.rs`).

The dynamic models call `___tls_get_addr` (three underscores: the GNU
entry with its argument in `eax`, `hod_cg::RuntimeFn::TlsGetAddrRegparm`)
in the exact byte forms of `llc` 19 = 22 at `-O0` (design 0008 §3.3): the
`lea` and the `call` adjacent, `ebx` the GOT pointer at the call, the
relocations without addend. In a PIE GNU ld, lld and mold relax both to
local or initial exec (no `___tls_get_addr` call is left in the linked
`tls.c`). Under `RtLibUseGOT` (`clang -fno-plt`) `llc` writes `call
*___tls_get_addr@GOT(%ebx)`; hod always writes the `@PLT` form, which the
linkers accept and relax alike. The model is `hod_cg::sym_access`'s
(`thread_local(<model>)`, the relocation model, `dso_local`, as `llc`).

There are two ways in, as on x86-64:

- `llvm.threadlocal.address` of a dynamic-model symbol is lowered as a
  call: every register value displaced, `ebx` taken (`FnCx::take_reg`,
  so the prologue saves it), the bare sequence, the result moved to the
  result's register, the call recorded as a call site.
- A dynamic-model symbol as a plain operand (`common::sym_address`, or
  `pic::mem_inst` into `eax`) keeps `sym_address`'s contract (writes `rd`
  only, flags kept, at any point of a lowering): `pushfd`; `lea esp,
  [esp - (size - 4)]`; `eax`, `ecx`, `edx`, `ebx` but `rd` stored at
  `[esp + gpr_at + 4i]`; `xmm0`..`xmm7` stored by `movups` at `[esp +
  16i]` when the function has used an `xmm` register so far (none holds
  a value otherwise; size 160, `gpr_at` 128; without them size 32,
  `gpr_at` 0); the sequence; the result in `rd`; the restores; `lea esp,
  [esp + size - 4]`; `popfd`. `esp` is 16-aligned at the call (it is in
  a body, outgoing areas being multiples of 16); the CFA is `ebp`-based,
  so no CFI row changes. The x87 stack must be empty there, as at any
  call: a lowering that keeps x87 values on the stack across a symbol
  materialization must not let that symbol be a dynamic-model
  thread-local.

## 10. Inline assembly (`asm`, WP-301)

A call of inline assembly is `crate::asm::call` (`call::lower`'s asm
arm): the operands placed in the six general registers, memory or
immediates as the constraints ask, the template substituted and
assembled in 32-bit mode by `hod_asm::x86`, the outputs stored back.
The rules, the constraint letters, the register order and every
refusal (`x86:inline-asm:<detail>`, `asm:*`, `x86-mode:*`) are
`spec/asm.md` §22. Record: `spec/landed/asm/WP-301.md`.

With §8 and §9: an `A` operand of 33 to 64 bits is a §8 value (its
frame slot, low word in `eax`, high word in `edx`), as input, output
(`=A`, `=&A`) and tied (`+A`). A memory operand or a pointer that is a
symbol (a global or a thread-local of any model) is evaluated before
the operands are placed (step 1), as any value: its address comes from
`common::sym_address` under the function's relocation model, so the
template sees a register, never a relocation of PIC or TLS. A template
that writes `ebx` (a clobber or an operand) is followed by PLT calls
that compute the GOT pointer again (§9.2: it is never kept).
Module-level assembly with code or data is `hod_asm`'s 32-bit module
assembler (`spec/asm.md` §23, WP-307); text of symbol directives only is
written as on the other targets.

## 11. Floating point on the x87 (`x87`, WP-300, design 0008 I9)

**Types and homes.** `float`, `double` and `x86_fp80` are class `Mem`:
a home in the frame of 4, 8 or 12 bytes (`x86_fp80`'s image is the low
10). In a function whose CPU has SSE (`float`) or SSE2 (`double`)
those types are §12's (`llc` computes in `xmm`); the CPU is the
function's `target-cpu`/`target-features`, else the tool's `-mcpu`/
`-mattr` (`generic` without them; `X86::with_cpu`, as `llc` takes its
options for a function without the attributes).
Loads, stores, `phi`s and a `select` without `fcmov` copy the encoding
with integer moves (bit exact). A constant is written by `mov`
immediates of its encoding (no constant pool, no relocation). An
operation pushes its operands (`fld` at their width), operates
(`faddp`/`fsubp`/`fmulp`/`fdivp st1`, `fchs`, `fabs`, `fsqrt`) and pops
the result to its home at its type's width (`fstp`), where it rounds.
The x87 stack is empty between instructions. `fneg`/`fabs` of a value
without an 80-bit copy flip/clear the sign byte in memory; `copysign`
copies the sign byte; `fmuladd` is `fmul` then `fadd` without rounding
between (no FMA).

**Excess precision: `llc`'s rule.** Measured with `llc` 19 and 22 at
`-O0 -global-isel=0 -fast-isel=0`, `float` and `double`, `generic` and
`pentiumpro` (the shapes of `crates/hod/tests/i386_x87.rs`): a computed
result stays in an x87 register at 80 bits for every use in its block
before the next instruction that ends the registers' life, in any
number, order and distance; a use after one, or in another block, reads
it rounded to its type (`llc`'s fast allocator spills it).
The instructions that end it (*barriers*): the terminator; a call; an
`llvm.memcpy`/`memmove` of more than 32 or a variable number of bytes
and an `llvm.memset` of more than 64 (shorter ones are written inline);
`frem` and the libm intrinsics (calls); an integer division wider than
32 bits (`__divdi3` and kin); without `cmov`, a `select` on a variable
condition unless its type is `i1` or it chooses between two integer
constants (`llc` branches: the block splits); without `cmov`, an
`fptoui` to `i64` (see "Conversions") and the integer intrinsics below. *Integer intrinsics* (measured with `llc` 19
and 22 at widths 1 to 64, CPU `generic`; with `cmov`, `i686`,
`pentiumpro`, `pentium4`, none branches), at a result of `N` bits:
`ctlz` with zero defined at `N` >= 2 and with zero poison at `N` > 32;
`cttz` with zero defined at `N` >= 32 and with zero poison at `N` > 32;
`smax`, `smin`, `umax`, `umin`, `sadd.sat`, `uadd.sat`, `ssub.sat`,
`usub.sat`, `sshl.sat`, `ushl.sat` at `N` >= 2; `fshl`/`fshr` by a
variable amount at `N` > 32. Never: `bswap`, `ctpop`, `bitreverse`,
`abs`, `scmp`, `ucmp`, a funnel shift by a constant amount. Not where
`llc`'s DAG folds the call: all operands constants or folded results; a
min/max of one value twice or with the type's least or greatest value;
a saturating addition of 0; a saturating subtraction of a value from
itself or of 0, `usub.sat` from all ones; a saturating shift of 0 or by
0; a funnel shift of one constant twice (an operand that is a folded
result equal to such a constant is not recognized: hod counts a
barrier there, `llc` none). LLVM 19 also branches for `sadd.sat`,
`uadd.sat`, `ssub.sat`, `usub.sat` at `N` = 1, LLVM 22 does not; hod
follows 22 (the shape `saddsat1_between` of
`crates/hod/tests/data/i386/x87/int-shapes-*.ll` differs from `llc` 19's
program at CPU `generic`). With the other packages (measured, the
shapes of `crates/hod/tests/data/i386/join_b/join_b.ll`): a
thread-local of a dynamic model under PIC is a barrier where `llc` calls
`___tls_get_addr` (the first use of its symbol in the block, of any
local-dynamic symbol for the module's block); a GOT access (`call; pop`)
is none. Inline assembly is none unless it clobbers `st(0)` (`~{st}`)
or has an x87 operand (`{st}`, `{st(1)}`, ...); a clobber of a deeper
register alone spills nothing. A `musttail` call is a call. `freeze`, `fpext`,
`fptrunc (fpext x)` back to `x`'s type in one block, a `select` of a
constant condition and, with `cmov`, a `select` (`fcmov`) pass the 80
bits on; `fptrunc` otherwise rounds. A result that `llc`'s DAG folds to
a constant (an operation of `fadd`..`frem`, `fneg`, casts, integer
arithmetic, `select`, `fabs`, `copysign`, `floor`, `ceil`, `trunc`,
`minnum`, `maxnum`, `fma`, `fmuladd` whose operands are constants or
such results of its block, eight levels deep) has its type's precision,
and a folded `frem` or intrinsic is no barrier. `sqrt`, `rint`,
`round`, `nearbyint`, `roundeven` and the other libm routines are not
folded.

**hod's rule.** A *run* is the instructions after a computed `float` or
`double` result up to the next barrier. A computed result (arithmetic,
casts to a floating type, `sqrt`, `fmuladd`, a libm call's `st0`, an
`fcmov`, and `freeze`/`fneg`/`fabs`/`select` of a constant condition
of an operand read through its 80-bit copy) that is not folded and has
an instruction of its run after it is stored twice: `fld st0`, `fstp
tbyte` into a 12-byte frame slot (its *80-bit copy*), then `fstp` to its
home. Every use in the run loads the copy (`fld tbyte`), every other
use the home. Copy slots are reused from one run to the next.
`x86_fp80` has no copy (its home is 80 bits). Across calls (with I6's
`abi`): a `float`/`double` call result arrives in `st0` as the callee
left it, at 80 bits, and `llc` keeps it for its run, so it is a
computed result (its copy stored by the glue store of `st0`,
`x87::store_call_result`); a `ret` belongs to the run it ends, and a
result of the run returns its 80-bit copy in `st0` (`fdivrp; retl` in
`llc`), every other value its home.

**The divergence.** With six or more products live at once in one
block without a barrier, `llc` rounds some uses (which ones follows its
fast allocator: the first product, and the first and last ones in
FIFO order or the middle ones in LIFO order), so its result is not a
function of the IR; hod keeps all of them at 80 bits. The shapes
`many6_lifo`, `many7_*`, `many8_*`, `many9_*`, `many12_*` compare with a
model of hod's rule (the same arithmetic at `x86_fp80`, rounded at the
store) instead of `llc`; `many6_fifo` equals `llc`.

**Folding.** `fadd`, `fsub`, `fmul`, `fdiv` of a `float` or `double`
whose operands are constants or such results of its block (through
`fneg` and `fpext`/`fptrunc` between `float` and `double`, eight levels)
are computed by hod with Rust's `f32`/`f64` arithmetic (one rounding, as
`llc`'s `APFloat`) and written as a constant: at run time the x87 would
round twice (to 64 bits, then to the type), one unit in the last place
apart for some `double` operands (measured: `1.0 + 0x3CA0000008000000`
is `0x3FF0000000000001` in `llc`, `1.0` at run time; the four
operations' shapes in `join_b.ll`). A NaN result stays on the run-time
path: `llc` folds an invalid operation to `+qNaN` (`0x7FF8000000000000`,
`float` `0x7FC00000`), quiets an sNaN operand (`0x7FFC...`) and passes a
NaN operand's payload, where the x87 gives `-qNaN` (`0xFFF8...`) for an
invalid operation: no shape compares a NaN. `frem`, the intrinsics and
the other folded operations are computed at run time (equal for
`float`; for `double` measured in the i386 join C: `frem`, `fma`,
`minnum`, `maxnum`, `sqrt` and the libm routines over signed zeros,
infinities, NaNs, subnormals and halfway cases run as `llc`'s,
`crates/hod/tests/data/i386/join_c/libm.c`).

**Compares.** `fcmp` puts the operands as the predicate needs (`olt`,
`ole`, `ugt`, `uge` swapped) and compares `st0` with `st1`: with `cmov`
`fucomip st1` and `fstp st0`; without, `fucompp`, `fnstsw ax`, `sahf`.
Unordered sets `ZF`, `PF`, `CF`: `oeq` is `sete` and `setnp`, `une`
`setne` or `setp`, `one` `setne`, `ord`/`uno` `setnp`/`setp`, the others
one `setcc` of the unsigned family. `true`/`false` are `cmp eax, eax`
and `sete`/`setne`. A compare of a value with itself is `fld st0`.

**Select.** A constant condition picks its operand. With `cmov` (a
scalar `i1` condition): false value in `st1`, true in `st0`, `test al,
1`, `fcmove st1`, `fstp st1`. Without: a branch and a copy of the
encoding.

**Conversions.** `sitofp`/`uitofp` from at most 32 bits: the integer
extended in `ecx`, stored below `esp`, `fild` of 32 bits (of 64 with a
zero high half for an unsigned `i32`). `fptosi`/`fptoui` to at most 32
bits: `fnstcw`, the word `or 0xC00` (truncate), `fldcw`, `fistp` of 32
bits (64 for an unsigned `i32`), `fldcw` of the saved word. With §8's
64-bit integers: `sitofp` from `i64` is `fild qword` of its home;
`uitofp` adds `2^64` as a `float` (`0x5F800000`, chosen by the sign of
the high word: `sar 31`, `and`) with `faddp` (`llc`'s sequence);
`fptosi` to `i64` is `fistp qword` under the truncating control word.
`fptoui` to `i64` (i386 join C) is `llc`'s sequence (measured, 19 and
22 alike): `x` compared with `2^63` (a `float` `0x5F000000`; `fucomip`
with `cmov`, else `fucomp`, `fnstsw ax`, `sahf`), the flag "`2^63 <=
x` or unordered" (`setbe`), `x - 2^63` under the flag and `x - 0`
otherwise (`llc` chooses with `fcmov`, or without `cmov` with a branch,
so it is a barrier there: its operand is read rounded; hod chooses with
a mask), `fistp qword` under the truncating control word, the flag
`xor`ed into bit 31 of the high word. So `2^63` and above convert
exactly below `2^64`; a NaN, `+inf`, values from `2^64` give 0; negative
values below -1 give what `fistp` gives (`0x8000000000000000` when out
of range). A conversion between a floating type and an integer of 33 to
63 or more than 64 bits is `x86:float:i64-cast`.

**libm.** `frem` is `fmodf`/`fmod`/`fmodl`; `sin`, `cos`, `tan`, `asin`,
`acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `exp`, `exp2`, `exp10`,
`log`, `log2`, `log10`, `pow`, `floor`, `ceil`, `trunc`, `round`,
`roundeven`, `nearbyint`, `rint`, `fma` call `llc`'s symbols (`sinf`,
`sin`, `sinl`, ...); `minnum`/`maxnum` call `fminf`/`fmin`/`fminl` and
`fmaxf`/`fmax`/`fmaxl`. Arguments are copied to the outgoing area at
their width (12 bytes for `x86_fp80`), the result is `st0`. Under PIC
the call goes through `pic::emit_call` (`ebx` holds the GOT pointer at
the PLT call, §9.2), as every call does.

## 12. Scalar SSE (`sse`, WP-317, design 0008 I12)

**Which values.** In a function whose CPU has SSE, `float` is an SSE
type; with SSE2, `double` too (the CPU as §11 decides it: the
function's `target-cpu`/`target-features`, else `-mcpu`).
`sse::owns` is the one decision point: an instruction with an SSE value
as its result or an operand (`fcmp`, a store, a cast, an intrinsic) is
lowered by `sse`; every other floating instruction by `x87` as §11
says, unchanged. `x86_fp80` is always §11's; at SSE without SSE2
(`pentium3`) so is `double`.

**Homes, ABI, glue.** SSE values are class `Mem` as on the x87 (4 or 8
bytes): arguments on the stack, `phi`s, and `float`/`double` returns
and call results through `st0` are §11's glue. This departs from
design 0008's `Fpr` row on purpose: hod does not aim at fast code, a
value in memory needs no `xmm` glue for arguments, returns, `phi`s or
spills (the x87 path already has all of it), and `class_of` has only
the types, not the function's features. A call result of an SSE type
is stored by `fstp` at its type's width, which every SSE use reads;
an `fpext`, `fptrunc` or `ret` may read its 80-bit value instead
("The 80-bit value of a call result" below). A return loads the home
(`fld`), as `llc` writes `movsd` to a stack slot and `fldl`.

**Operations.** The first operand is loaded into `xmm6`
(`movss`/`movsd`), the operation takes the second from its home, the
result is stored to its home: every result is rounded to its type at
once, as `llc`'s, so no SSE value has an 80-bit copy. `fadd`, `fsub`,
`fmul`, `fdiv` (`adds`...), `sqrt` (`sqrts`), `fmuladd` (`muls` then
`adds`: two roundings, as `llc` at these CPUs), `minnum`/`maxnum` as
`llc`: `b` where `a` is a NaN, else `mins b, a` (`b < a ? b : a`;
`maxs` `b > a ? b : a`). `fneg`, `fabs`, `copysign` change the sign bit
of the copy in memory (bit-exact as `llc`'s masks). Loads and stores
through `xmm6`; `freeze` and a `select` of a constant condition copy;
`select` takes the address of the chosen home by `cmov` (every SSE CPU
has it) and copies through `xmm6`. `fcmp`: `ucomis` with §11's operand
order and flag reading. Constants are written into the home by `mov`
immediates (§11; no constant pool, no relocation under PIC).

**Conversions.** `fpext`/`fptrunc` (between `float`, `double`,
`x86_fp80`, at least one an SSE type): `fld` of the operand's 80-bit
value for this use (below), or its §11 copy (at SSE without SSE2 a
computed `double`, an x87 value that `llc` rounds to `float`
directly), else its home, and `fstp` to the result's home (one
rounding). `sitofp` of at most 32 bits: `cvtsi2s` of the
extended value; `uitofp` of an `i32`: `fild` of 64 bits with a zero
high word and `fstp` (one rounding, as `llc`'s `2^52` sequence).
`fptosi` to at most 32 bits: `cvtts2si` (a narrower result is its low
bits); `fptoui` to `i32` as `llc`: `x - 2^31` and `x` truncated, the
first masked by the sign of the second, or-ed. 64-bit integers, as
`llc` on the x87 from the home: `sitofp` = `fild`, `fstp`; `uitofp` to
`float` = `fild`, `fadd` of 0 or `2^64` at 80 bits, `fstp`; `uitofp`
to `double` = high word times `2^32` plus the low word, both exact,
`addsd` (one rounding, as `llc`'s `2^52`/`2^84` sequence); `fptosi`/
`fptoui` to `i64` = §11's control-word sequence on the home's value.
`bitcast` between an SSE type and the integer of its width is a copy.
Integers of 33 to 63 or more than 64 bits: `x86:float:i64-cast`.

**Calls.** `frem` and the libm intrinsics are calls of `fmodf`/`fmod`,
`floorf`/`floor`, ... (`fma` too) through `pic::emit_call`, arguments
copied to the outgoing area at their width, the result stored from
`st0` at its type's width. Barriers (§11) are unchanged: no new
construct of this section is lowered by `llc` with a branch or a call
that §11 does not already list (`frem` and the libm intrinsics are
calls there for every type).

**The 80-bit value of a call result** (measured, `llc` 19 and 22
identical, `-O0`, `pentium4` and `pentium3`). `llc` takes a call result
of an SSE type out of `st0` as an exact `fp_round` of the 80-bit
register `x`, and the DAG of the call's block folds that node into
some uses. A *view* is a value `llc` still has as `x`: the call result;
an `fpext` of a view; an `fptrunc` of an `fpext` of a view `w` back to
`w`'s type. A view reaches the uses in the call's block up to an
`atomicrmw` that `AtomicExpand` turns into a loop (an `and`/`or`/`xor`
whose result is used, any operation wider than 32 bits, the floating
operations, `nand`, `min`/`max` and the rest; `xchg`, `add`, `sub` of
at most 32 bits, an unused `and`/`or`/`xor` and `cmpxchg` stay one
instruction), and across everything else: calls, libm calls, `memcpy`,
a `select` that `llc` lowers with a branch (`llc` spills `x` at 80
bits, `fstpt`/`fldt`). Within the reach:
- `fpext` of a view to `U`: `x` rounded to `U` (`x` for `x86_fp80`);
- `fptrunc` of a view: `x` rounded to the result type once;
  `fptrunc (fpext w)` of a view `w` to `w`'s type is `w`; to another
  type it is `x` rounded to `w`'s type and converted, unless every use
  of the result is a store (then `x` rounded once: `llc` folds the
  store);
- `ret` of a view returns `x` itself.
Every other use (arithmetic, `fcmp`, `store`, an argument, `freeze`,
`fneg`, a use in another block or past the reach) reads the home, the
value rounded to its type. hod (`sse::Views`): at the call
(`x87::call_result`) a result of an SSE type that an `fpext`, `fptrunc`
or `ret` in its reach reads gets a 12-byte slot, stored at 80 bits
before the glue store (`x87::store_call_result`); `fp_cast` and
`x87::ret_copy` read the slot for a view; `x87` keeps no §11 copy of
such a result. Not modelled: `llc`'s bit-test form of an `atomicrmw
or`/`and` (one instruction) counts as a loop. At SSE without SSE2
(`pentium3`) an `fpext` of a `float` call result to `double` is an x87
value that `llc` keeps as `x` for every x87 use in the reach (`faddp`
of `x`, measured 19 and 22): `x87::push` reads the view's slot for
every x87 operand, and `x87::cast` (`fpext`/`fptrunc` between
`double` and `x86_fp80`) pushes what `fp_cast` would
(`sse::x87_cast_source`: `fptrunc x86_fp80 (fpext w)` to `double`
rounds `x` through `float`). Without SSE no view exists and neither
function changes its code.

**`half` with SSE2** (WP-317 third session; measured `llc` 19 and 22,
`-O0`, `pentium4`, identical but for `fneg`). The function's feature
set reaches the target through `Target::for_function` (`X86::features`).
A `half` argument takes a 4-byte stack slot (its two bytes at the
start); a result is returned in `xmm0` (`abi::assign_for` asks
`sse::half_place`; glue `load` is `movzx eax` + `movd xmm0`, glue
`store` `movd eax` + a 16-bit store). A `half` value lives in a 2-byte
home (class `Mem`). Every operation is `llc`'s libgcc sequence, all
calls through `pic::emit_call`: each operand widened by `__extendhfsf2`
(exact, result in `st0`), the `float` operation (`adds`..`divs`,
`ucomiss` for `fcmp`, `fmodf` for `frem`), and `__truncsfhf2`, whose
result is read from `xmm0`. `fpext` stores the widened value at the
result's width (exact for `float`, `double`, `x86_fp80`); `fptrunc`
passes its operand at the operand's width to `__truncsfhf2` /
`__truncdfhf2` / `__truncxfhf2` (one rounding; a view of an SSE call
result is `x` rounded to the operand's type, as `llc` folds no rounding
into a conversion to `half`). `sitofp`/`uitofp` produce the `float`
that the `float` conversion would (`cvtsi2ss`; an unsigned `i32` and
an `i64` by `fild`, the unsigned `i64` with `llc`'s `2^64` addition)
and narrow it; `fptosi`/`fptoui` widen and truncate with `cvttss2si`
(every `half` in the result's range converts exactly; an `i64` result
is sign-extended). `load`, `store`, `freeze`, `select` (the address
`cmov`), `bitcast` to and from `i16` are 16-bit copies. `fneg` flips bit
15 (LLVM 22; LLVM 19 widens, negates and narrows, which differs only
for a signalling NaN: tests use no signalling NaN). The libgcc calls
are calls, but with SSE2 §11 keeps no 80-bit copy (`float` and
`double` are SSE values), so they need no `x87::barrier` entry; a
view of an SSE call result reaches across them as across any call.
Refused: `half` in a function without SSE2 (`x86:float:half`: `llc`
returns it in `ax` there and reads `ax` after `__truncsfhf2`, but
libgcc's routine writes only `xmm0`, an SSE2 `movd`), a `half`
parameter or result of `fastcc` or `inreg` (`x86:float:half`), and
the `half` intrinsics (`x86:float:half:intrinsic`).

**Remains refused.** `bfloat` (`x86:float:bfloat`: `llc` 19 aborts on
part of its operations, "Unsupported library call operation" and
"soft promote", and errors on its return without SSE: no oracle),
`fp128` (`x86:float:fp128`: libgcc calls with a
hidden result pointer, and LLVM 19 and 22 align its arguments
differently), other
intrinsics (`x86:float:intrinsic`).

## 13. Vectors (`vector`, WP-317, design 0008 I12 step C)

**Measured** (`llc` 19 and 22, `-O0 -global-isel=0 -fast-isel=0`, at
`pentium4`): a vector of at most 16 bytes, at least two lanes and lanes
of 8 to 64 bits is one `xmm`, its bytes packed from lane 0 (`<2 x
i32>`, `<4 x i8>`, `<3 x i32>` widened, not promoted). The result is in
`xmm0`; the first three arguments in `xmm0`-`xmm2` (`fastcc` and
`inreg` the same); every other one, and every vector of a variadic
call, in a 16-byte stack slot aligned 16. `<1 x T>` is scalarized, `<N
x i1>` promoted, a wider vector split over several `xmm`: refused at
the boundary (`x86:vector:abi`, `x86:vector:wide`). Without SSE2
(`pentium3`, `generic`) `llc` scalarizes every vector: refused
(`x86:vector:no-sse2`). `llc` does not realign the stack for a 16-byte
slot: it relies on `esp` being 16-aligned after its prologue, which
hod's frame keeps (§ "Frame"), so `frame.rs` is unchanged.

**Values.** Every vector is of class `Mem`; its home holds its memory
image (lanes packed; `i1` lanes bit by bit, lane 0 in bit 0), so
`load`, `store`, `bitcast`, `freeze` and `phi` are copies and a
constant is its flattened image. The boundary is `sse::vector_place`
(the part's size is the allocation size: 2, 4, 8 or 16) with the glue
moves `sse::load_xmm` / `store_xmm` (`movzx`+`movd`, `movd`, `movsd`,
`movdqu`).

**Operations**, lane by lane through `eax`, `ecx`, `edx`, `xmm6`:
integer `add` .. `ashr`, `udiv` .. `srem` (lanes of 1 to 4 bytes,
extended to 32 bits; a lane divided by zero traps, as `llc`'s);
`i64` lanes `add`/`adc`, `sub`/`sbb`, bitwise; `i1` lanes bitwise on
bytes; `fadd` .. `fdiv` (`adds`.. on `xmm6`), `fneg` (the sign bit);
`icmp` (`i64` lanes by `cmp`/`sbb` of the halves), `fcmp` (§12's
conditions) into an `i1` vector; `select` by a scalar or an `i1`
vector without a branch (`f ^ ((t ^ f) & m)`); `extractelement` /
`insertelement` with a constant or variable index (masked with `N-1`
for a power-of-two `N`, else 0 when `≥ N`); `shufflevector`; `trunc`,
`zext`, `sext` (with `i1` lanes), `fpext`, `fptrunc`, `sitofp` and
`fptosi` (lanes up to 32 bits, `uitofp` / `fptoui` up to 16);
`llvm.vector.reduce.{add,mul,and,or,xor,smax,smin,umax,umin}` of lanes
up to 32 bits (`cmov` for the extremes) and of at most 32 `i1`. A
result whose lanes move (shuffle, cast, compare, `i1`) is built in a
slot of its own and copied to its home. Nothing branches or calls: no
vector construct is an x87 barrier.
