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
`gpr_bytes` 4. `hod_cg`'s pool minimum is 2 general registers for a
4-byte target (`MIN_POOL_GPR_32`, `RegInfo::min_pool_gpr`, WP-331), and
the `min_pool` stress mode uses it (the two lowest: `ebx`, `esi`).
`esi`, `edi`, `ebp`, `esp` have no low-byte register in 32-bit mode: a
byte store or a `setcc` of a value in `esi`/`edi` goes through
`ecx`/`edx`.

**The two-register rule (WP-331).** No lowering pins more than two
general pool registers at once (operands, temporaries and the result
together), on every path: a coroutine body reserves `esi` as its heap
frame's base (§18), which leaves `ebx`, `edi`. One code path serves the
three-register and the two-register function. What a lowering uses
instead of a third pool register:

- the scratch registers `eax`, `ecx`, `edx`: an operand that is only
  read into a fixed register goes there with `FnCx::put` (nothing is
  pinned), and a result may be produced in one and moved or stored;
- the operand's register: a register operand dies at its use, so the
  result takes it (`FnCx::use_reg`, `FnCx::finish`);
- memory: every value has a frame slot (`cmp r, [slot]` needs one
  register).

Placing an operand may call (a TLS address) and write `eax`, `ecx`,
`edx`: an operand that goes into a scratch register is put there after
every operand that is placed in a pool register. The sites WP-331
changed: `alu::icmp` (the result takes the left operand's register),
`atomic::rmw` (the operand into `edx`, the result in the pointer's
register), `atomic::cmpxchg` (the replacement into `ecx`), `atomic::store`
(the value into `ecx`), `bytes::copy` / `bytes::set` (the length into
`ecx`), `bits::funnel` (a variable amount into `ecx` / `edx`).

**The borrowing rule.** A lowering whose instruction names a pool
register (`rep movs`: `esi`, `edi`; `rep stos`: `edi`; `cmpxchg8b` and a
PIC call: `ebx`) borrows it one of two ways, and no other: `ebx` is
taken from the pool for the instruction (`FnCx::take_reg`, which
displaces its value; `ebx` is never the coroutine base), and `esi` /
`edi` are pushed before the sequence and popped after it, with no frame
access between the first pop and the last (`bytes.rs`: the only frame
operand of the sequence, a wide length's slot, is read before the
pops). A
coroutine's `esi` is therefore intact at every frame access.

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
-20); `hod_cg` writes them to `.eh_frame` (§14, WP-314).

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
operand shares its register; a funnel shift's variable amount goes
straight into `ecx` / `edx`), so at most two pool registers are pinned
(§2); the work registers are `eax`, `ecx`, `edx`. Every `cmov` of
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
`llvm.memcpy`/`memmove`/`memset` (and `.inline`) are `rep movsb`/`rep
stosb`, saving `esi`/`edi` around them; `memmove` copies backward (`std`
... `cld`) when the destination is above the source. A length of 33 to 64
bits is its low word (WP-315, §13.1).

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
| `x86:i64` | not reachable since WP-315 (§13.4): every integer of 33 to 64 bits is taken by §8, `abi.rs` or §13 | I6, WP-306, WP-315 |
| `x86:i64:wide` | not reachable since WP-346: an integer of 65 to 128 bits is §22's, each operation it leaves out has a name of its own | WP-346 |
| `x86:i64:wide:div`, `x86:i64:wide:intrinsic`, `x86:i64:wide:abi`, `x86:i64:wide:width` | an integer of 65 to 128 bits as a division, an integer intrinsic §22 leaves out, an argument or result; an integer wider than 128 bits (§22) | — |
| `x86:i64:wide:shift`, `x86:i64:wide:switch` | not reachable since WP-355: shifts and `switch` at 65 to 128 bits are §22's | WP-355 |
| `x86:float` | a floating-point value in a register where §11 takes none (arguments and results are I6's since the join) | — |
| `x86:float:half` | a `half` value in a function without SSE2; a `half` parameter or result of `fastcc` or `inreg` (§16) | — |
| `x86:float:half:intrinsic` | an intrinsic on `half` (§16) | — |
| `x86:float:bfloat`, `x86:float:fp128` | a value of those types | — |
| `x86:float:i64-cast` | a conversion between a floating type and an integer of 33 to 63 or more than 64 bits (§11; `fptoui` to `i64` is lowered since the i386 join C) | — |
| `x86:float:intrinsic` | a floating-point intrinsic §11 does not lower | — |
| `x86:float:cast` | a cast of a floating value not in §11 (no verified IR reaches it since the i386 join E lowers `bitcast`) | — |
| `x86:float:fp80-cast` | no verified IR reaches it: a `bitcast` between `x86_fp80` and `i80` is lowered since WP-337 (§21) | WP-337 |
| `x86:vector` | a vector `ptradd` (§17) | I12 |
| `x86:vector:no-sse2` | a vector in a function with SSE but without SSE2 (§17; without any SSE: §20) | WP-338 (no SSE) |
| `x86:vector:no-sse2:excess-precision`, `:fold` | a floating lane `llc` holds at 80 bits or folds (§20) | — |
| `x86:vector:no-sse2:cast`, `:select`, `:reduce`, `:abi` | without SSE: casts of floating lanes; `select` and `min`/`max` reductions without `cmov`; `i1`/`inreg` vectors and `fastcc` memory results at the boundary (§20) | — |
| `x86:vector:wide` | a vector of more than 16 bytes passed or returned | — |
| `x86:vector:abi` | a vector of `i1`, of one lane or of odd-width integers passed or returned | — |
| `x86:vector:element` | lanes of `half`, `bfloat`, `fp128`, `x86_fp80`, odd-width integers | — |
| `x86:vector:scalable` / `:length` | a scalable vector; more than 256 lanes | — |
| `x86:vector:i64-op` | `i64` lanes: an operation other than `add`, `sub`, bitwise, compares, moves | — |
| `x86:vector:frem` / `:cast` | `frem`; casts with unsigned 32-bit or 64-bit lanes and floating point | — |
| `x86:vector:intrinsic` | an intrinsic on vectors other than the integer and `i1` reductions | — |
| `x86:aggregate` | since WP-315 (§13.2) only an aggregate argument of more than 16 leaves (`abi.rs`'s `MAX_LEAVES`) and an aggregate without a layout | — |
| `x86:frame-depth` | `llvm.frameaddress`/`returnaddress` deeper than 256 frames (§13.5) | — |
| `x86:eh:landingpad`, `x86:eh:landingpad-clause`, `x86:eh:resume`, `x86:eh:typeid` | a `landingpad` or `resume` of a type other than `{ ptr, i32 }`; a clause that is not a symbol, `null` or an array of them; `llvm.eh.typeid.for` of a non-symbol (§14; `x86:eh` itself removed by WP-314) | — |
| `debug:dwarf-4-i386` | a DWARF 4 unit (`Dwarf Version` 4 or less) on i386 (`hod_dwarf` `split.rs`, §15; `x86:debug-info` removed by WP-314) | — (needs `dwarf4.rs` at address size 4: the unit header's address size, `.debug_ranges` and `.debug_loc` with 4-byte address pairs, `DW_OP_addr` with a 4-byte operand and `R_386_32`) |
| `debug:split-dwarf-i386` | split DWARF (`-gsplit-dwarf`) on i386 (`hod_dwarf` `split.rs`, §15) | — (needs `split.rs` at address size 4: the skeleton's and the `.dwo` unit's headers, the object's `.debug_addr` of 4-byte entries, and the `.dwo` output path through the driver) |
| `x86:nocf-check-indirect-call` | an indirect `nocf_check` call in a module with `cf-protection-branch` (needs `notrack`; §19.3; `x86:cf-protection` removed by WP-324) | — |
| `x86:atomic:no-cx8` | an 8-byte atomic operation on a CPU without `cmpxchg8b` (§12.2) | — |
| `x86:atomic:libatomic` | an atomic access of another size than 1, 2, 4, 8 bytes or below its natural alignment (§12.2) | — |
| `x86:atomic:float-op` | `atomicrmw` `fmax`, `fmin` and the other floating operations; `fadd` and `fsub` are lowered since join D (§12.2) | — |
| `x86:atomic:op` | `atomicrmw` `uinc_wrap`, `udec_wrap`, `usub_cond`, `usub_sat` | — |
| `x86:atomic:type` | an atomic value of another type (`x86_fp80`, vectors) | — |
| `x86:inline-asm:<detail>` | what inline assembly does not take (§10, `spec/asm.md` §22.5) | I13 (`asm.rs`) |
| `x86:abi:cc` | a calling convention other than C, `coldcc` and `fastcc`; `fastcc` with a floating-point argument or result, an `sret` or a result in memory (`spec/abi-x86.md` §1.2) | — (`fastcc` floating point: I12) |
| `x86:abi:attr` | the Swift attributes, `inalloca`, `preallocated`, `alignstack`; `sret` other than on the first parameter or on a non-pointer; `inreg` on a floating-point or aggregate argument or in a variadic signature; `byval` above alignment 16 or with `inreg`; `nest` twice or on a taken register (`spec/abi-x86.md` §6.6) | — |
| `x86:abi:wide-arg` | an `i128` or `fp128` argument (`llc` 19 and 22 place it differently, `spec/abi-x86.md` §12) | — |
| `x86:abi:x87-pair` | a result with two floating-point leaves (`st0`, `st1`) | I9 |
| `x86:abi:indirect-edx` | an indirect call with an argument in `edx`, which holds the callee's address (`spec/abi-x86.md` §8.1) | — |
| `x86:abi:musttail` | a `musttail` call with stack arguments and an argument in `ecx`; under PIC, one to another module's function with `eax`, `ecx` and `edx` all holding arguments (`spec/abi-x86.md` §8.5) | — |
| `x86:varargs` | `va_start` outside a variadic function (§12.1) | — |
| `x86:varargs:va-arg-type` | `va_arg` of an aggregate, a vector, an integer wider than 64 bits (§12.1) | — |
| `x86:probe-stack` | a function with `"probe-stack"="inline-asm"` (§12.3) | — |
| `x86:intrinsic` | any other intrinsic (the integer intrinsics of §4.1 and §8 are lowered; the overflow and small intrinsics of §13, `eh.typeid.for` (§14), the coroutine handle and body intrinsics of §18 and `eh.unwind.init` (§19.4) are lowered; still refused: every intrinsic none of them names) | per family |
| `x86:coro-promise` | `llvm.coro.promise` whose alignment is not a constant power of two of at most 2^30, or whose direction is not a constant (§18) | — |
| `x86:coro:asm-base` | inline assembly in a coroutine body that writes `esi`, its frame base (an `{esi}` operand, an `esi` clobber; §18) | — |
| `x86:call` | a call with operand bundles, `callbr` | — |
| `x86:blockaddress` | a `blockaddress` of another function's block in code (§19.1; `x86:indirect-branch` removed by WP-324) | — |
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
through `ecx`). Wider than 64 bits: §22.

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
  - Wider than 64 bits: §22 (WP-355), else `x86:i64:wide:intrinsic`; a width other than 128 is refused
    before, by the shared classifier, as `intrinsic:<name>/int-wide`).
- `scmp`, `ucmp` of operands of 33 to 64 bits (i386 join C;
  `int::three_way`), a result of 2 to 64 bits: `a > b` by the order
  compare above and `setcc`, kept on the stack (`push`), then `a < b`
  likewise; the result is their difference (1, 0, -1), its high word
  sign-extended for a result wider than 32 bits. No branch. Operands or
  a result wider than 64 bits: `x86:i64:wide:intrinsic` (§22).

Since WP-315 an integer of 33 to 63 bits passed or returned by value and
the `*.with.overflow` intrinsics of every width are lowered (§13).

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

**Operands wider than a register (WP-347).** clang writes C's `"r,m"`
as `r|m` and an in-out `"+r,m"`/`"+m,r"` as an indirect output
(`=*r|m`, `=*m|r`) with a tied input. The alternative is `llc`'s by
weight (`code_weight`): an integer takes the first alternative, any
other type (a pointer, `float`, `double`, `x86_fp80`) the memory one.
Measured with `llc` 19 and 22 (`-O0 -global-isel=0 -fast-isel=0`) on
clang 19 and 22's `--target=i686-linux-gnu -O0` modules:

| operand | `r\|m` input | `+r,m` / `+m,r` |
|---|---|---|
| `int`, `i32` | register | memory (the variable) |
| `long long`, an 8-byte struct (`i64`) | two registers | memory |
| pointer, `float`, `double`, `long double` | memory: a copy | memory |

- A direct `i64` input in a register (`r`, `q`, the `r` of `r|m`, a
  constant too) is two registers, the next two free of the order, low
  half in the first; the operand prints as the low register (`%eax`,
  `${0:k}`, `${0:w}` of it): `Place::Halves`. Both are written.
- A direct memory input of class `Mem` (an integer of 33 to 64 bits,
  `float`, `double`, `x86_fp80`) is a copy of its home in a slot of its
  own (its alloc size, at least 8 bytes) whose address is in a register
  (§22.4 of `spec/asm.md`: hod's memory operand prints as `(%reg)`,
  `llc`'s as the frame object, `-8(%ebp)`).
- In-outs in memory already worked (the variable's address; the value is
  read and written by the template in place).

Still refused, `x86:inline-asm:operand-type`: a 64-bit or floating-point
register OUTPUT, direct or tied (`=r`, `=r,0`; `llc` takes two
registers); a `float`/`double` with `r` only; an integer of 33 to 63
bits in a register; an indirect 64-bit register input (`*r`, which `llc`
rejects too); a memory input of another type of class `Mem` (an
integer of 65 to 128 bits, §22, refused here; `half`, `fp128` and
vectors, refused earlier today under their own names). More registers than six (four `i64`, three with `ebx`
clobbered) is `x86:inline-asm:registers` (`llc` takes `ebp`). A
template's x87 mnemonics (`fldl %0`) are `asm:mnemonic/<name>`. Tests:
`crates/hod/tests/i386_asm_wide.rs`. Record:
`spec/landed/codegen/WP-347.md`.

## 11. Floating point on the x87 (`x87`, WP-300, design 0008 I9)

**Types and homes.** `float`, `double` and `x86_fp80` are class `Mem`:
a home in the frame of 4, 8 or 12 bytes (`x86_fp80`'s image is the low
10). In a function whose CPU has SSE (`float`) or SSE2 (`double`)
those types are §16's (`llc` computes in `xmm`); the CPU is the
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
`llvm.memcpy`/`memmove`/`memset` that `llc` makes a libc call (§13.1;
the others are written inline); an `atomicrmw` that `llc` writes as a
loop (§12.4); an `invoke` (a terminator, §14);
`frem` and the libm intrinsics (calls); an integer division wider than
32 bits (`__divdi3` and kin); without `cmov`, a `select` on a variable
condition unless its type is `i1` or it chooses between two integer
constants (`llc` branches: the block splits; for an aggregate, §13.2);
without `cmov`, an
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
63 or more than 64 bits is `x86:float:i64-cast`. `uitofp` of an `i64`
is rounded to its type at once (`llc`: `fstps`/`flds`, measured 19 and
22): no 80-bit copy; `sitofp` of an `i64` keeps one.

**`bitcast`** (i386 join E) between `float` and `i32`, `double` and
`i64` (§8's slot): the same bytes through the homes. The floating home
holds the value rounded to its type, so a cast takes the bits of the
rounded value, never the 80-bit copy (`llc` at `generic`: `fstps
(%esp); movl (%esp), %eax` of a product); a cast to a floating type has
no 80-bit copy, and no bits pass through the x87 stack (a signalling
NaN pattern is kept). Not a barrier. `x86_fp80` from or to `i80`: the
10 bytes (WP-337, §21); a vector on either side: the vector's
refusal.

**libm.** `frem` is `fmodf`/`fmod`/`fmodl`; `sin`, `cos`, `tan`, `asin`,
`acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `exp`, `exp2`, `exp10`,
`log`, `log2`, `log10`, `pow`, `floor`, `ceil`, `trunc`, `round`,
`roundeven`, `nearbyint`, `rint`, `fma` call `llc`'s symbols (`sinf`,
`sin`, `sinl`, ...); `minnum`/`maxnum` call `fminf`/`fmin`/`fminl` and
`fmaxf`/`fmax`/`fmaxl`. Arguments are copied to the outgoing area at
their width (12 bytes for `x86_fp80`), the result is `st0`. Under PIC
the call goes through `pic::emit_call` (`ebx` holds the GOT pointer at
the PLT call, §9.2), as every call does.

## 12. Varargs, atomics, dynamic stack (`va`, `atomic`, `dynstack`; WP-316, design 0008 I11b)

The oracle is `llc` 19 and 22 at `-O0 -global-isel=0 -fast-isel=0`;
behaviour is compared by running (`crates/hod/tests/i386_va.rs`,
`i386_atomic.rs`, `i386_dynstack.rs`).

### 12.1 Variadic function bodies (`va`)

A `va_list` is one pointer into the caller's stack arguments
(`spec/abi-x86.md` §9.2). `llvm.va_start` stores the address of the
first variable argument (`Incoming(VaInfo::stack_offset)`);
`llvm.va_copy` copies the pointer; `llvm.va_end` emits nothing. The IR
`va_arg` instruction is lowered as `llc` expands it
(`TargetLowering::expandVAArg`): the pointer rounded up to the type's
i386 ABI alignment, the value read there, the pointer advanced by the
allocation size (`i8` 1 at 1, `i16` 2 at 2, `i32`/`ptr`/`float` 4 at
4, `i33` to `i64` and `double` 8 at 4, `x86_fp80` 12 at 4). The IR
`va_arg` instruction stays refused on every target by the shared
classifier (`inst:va_arg`: the front ends expand `va_arg` themselves),
so the facade never reaches this lowering; it is tested through
`hod_x86` directly. Refused: `x86:varargs` (`va_start` outside a
variadic function), `x86:varargs:va-arg-type` (an aggregate, a vector,
an integer wider than 64 bits).

### 12.2 Atomics (`atomic`)

The orderings are TSO's, as `hod_x64`.

- 1, 2, 4 bytes (`i8`, `i16`, `i32`, `ptr`, `float`): a load is `mov`
  (`movzx` below 4 bytes) at every ordering; a store is `mov`, `xchg`
  for `seq_cst`. `atomicrmw`: `xchg`; `lock xadd` for `add` and `sub`
  (the operand negated); every other operation a `lock cmpxchg` loop
  (old value in `eax`, new one in `ecx`). `cmpxchg`: `lock cmpxchg`, the
  flag by `sete`; `weak` is the same.
- 8 bytes (`i64`, `double`, values in two-word slots, §8): a load is
  `fild qword [p]` · `fistp qword [slot]`, a store `fild qword [slot]` ·
  `fistp qword [p]` and the fence of `seq_cst` (an aligned 8-byte x87
  access is single-copy atomic and an `i64` survives the round trip
  exactly). **Deviation:** with SSE2 `llc` uses `movq`; hod keeps the
  x87 pair on every CPU (same behaviour). `atomicrmw` and `cmpxchg`: a
  `lock cmpxchg8b` loop (old in `edx:eax`, new in `ecx:ebx`; `ebx` is
  taken for the sequence, under PIC `pic::emit_call` computes the GOT
  pointer again before the next PLT call, §9.2).
- `fence seq_cst`: `mfence` with SSE2, `lock or dword [esp], 0`
  without (as `llc`); other orderings and `syncscope("singlethread")`:
  nothing.
- The result of `cmpxchg` (`{ <ty>, i1 }`) lives in a slot: the value
  at offset 0 and the flag at `field_offset(ty, 1)`; `extractvalue` of
  it reads that slot (`atomic::extract`, its own arm in `target.rs`).
  The general aggregate `extractvalue` must use the same layout.
- `atomicrmw fadd`/`fsub` of `float` or `double` (join D): `llc`'s loop
  (measured, 19 and 22, `-O0`). The old bits (`eax`, `edx:eax` at 8
  bytes) are pushed and `fld` at the type, `fadd`/`fsub` reads the
  operand's slot, `fstp` at the type stores the new value rounded to it,
  and the bits are popped into `ecx` (`ecx:ebx`), then `lock cmpxchg`
  (`lock cmpxchg8b`), `jne` back. The x87 stack is empty before and
  after. The result is the old bits, stored to the result's slot.

Refused: `x86:atomic:no-cx8` (an 8-byte operation on `i386`/`i486`,
which have no `cmpxchg8b`: `llc` calls libatomic's `__atomic_*_8`);
`x86:atomic:libatomic` (a size other than 1, 2, 4, 8 bytes, or an access
below its natural alignment: `llc` calls libatomic's generic routines,
which `hod_cg`'s runtime table does not have with 32-bit `size_t`);
`x86:atomic:float-op` (`atomicrmw` `fmax`, `fmin` and the other floating
operations except `fadd` and `fsub`: `llc` calls
`fmaxf`/`fmax`/`fminf`/`fmin` inside its `cmpxchg` loop); `x86:atomic:op` (`uinc_wrap`, `udec_wrap`,
`usub_cond`, `usub_sat`); `x86:atomic:type` (another type: `x86_fp80`,
vectors).

### 12.3 Dynamic stack (`dynstack`)

A dynamic `alloca` (a variable count, or outside the entry block) moves
`esp` down by its size rounded up to 16, then, for an alignment above
16, rounds `esp` down with `and`; the new `esp` is the address. The
count is zero-extended or truncated to 32 bits, as `llc` does; a count
of 33 to 64 bits (§13.4) gives its low word (join D). The
frame is `ebp`-based (§3), so nothing reads `esp` but the call sequence,
which reserves its outgoing area below every dynamic area.
`llvm.stacksave` reads `esp`, `llvm.stackrestore` writes it. **Deviation:**
an over-aligned static slot (`hod_cg`'s `Target::dynamic_slot`) gets its
own aligned dynamic area in the entry code where `llc` realigns the
whole frame (`and esp, -align` in the prologue); it runs the same. No
call is made (`llc` calls nothing either). Refused: `x86:probe-stack` (a
function with `"probe-stack"="inline-asm"`: `llc` probes the frame page
by page; other values are the classifier's refusal).

### 12.4 Barriers (§11)

Measured in the `-O0` assembly of `llc` 19 and 22 at CPU `generic` and
`i686` (a label inside the function of one construct): an `atomicrmw`
that `llc` writes as a loop is a barrier (`atomic::barrier`, from
`x87::barrier`): every 8-byte one; at 1, 2, 4 bytes every operation but
`xchg`, `add`, `sub`, and but `and`, `or`, `xor` whose result is unused
(`llc` writes `lock and`/`or`/`xor`, no loop; hod's own loop for them
does not end the run); the floating operations. No barrier, no branch
and no call: `cmpxchg` (strong and weak, 8 bytes included), fences,
ordered loads and stores (8 bytes included), `va_start`, `va_copy`,
`va_end`, `va_arg`, a dynamic `alloca` (any alignment),
`llvm.stacksave`, `llvm.stackrestore`. The shapes (one construct between
an x87 result and its use, `float` and `double`, `generic` and
`pentiumpro`, plain and under `Stress::ALL`) are in
`crates/hod/tests/i386_atomic.rs`.

## 13. Memory intrinsics, aggregates, overflow and small intrinsics (`bytes`, `agg`, `bits`; WP-315, design 0008 I11)

### 13.1 Memory intrinsics

`llvm.memcpy`/`memmove`/`memset` and their `.inline` forms take a length
of `i32` or of 33 to 64 bits; a wider length is its low 32 bits (`llc`'s
`size_t` is 32 bits), so `2^32 + k` copies `k` bytes. The volatile flag
and the alignments do not change hod's code (`rep movsb`/`stosb`, §5);
they choose `llc`'s, which decides where the x87 registers' life ends
(§11). `bytes::calls_libc` is `llc`'s rule, measured with `llc` 19 and 22
at CPU `generic` and `i686` (identical; a CPU with SSE is not measured)
over lengths 0 to 140, 200, 255 to 257, 1000 and `2^32 + 8`, alignments
1 to 16, volatile or not, and checked by `the_libc_call_rule_is_llc_s`:

- the length is a constant `n` of at most 64 bits judged on its full
  value, else (variable) a call;
- inline as stores of 4, then 2, then 1 bytes: with overlap allowed
  (`memcpy`/`memset`, not volatile) `ceil(n / 4)` stores, else
  `n / 4 + [0, 1, 1, 2][n % 4]`; at most 8 stores for `memcpy`/`memmove`,
  16 for `memset`;
- past that, `memcpy`/`memset` with an alignment of at least 4 (the
  smaller of the pointers' `align` attributes; nothing else is consulted)
  and `n <= 128`: `rep movs`/`rep stos`, no call;
- otherwise a call (every `memmove` past its stores). The `.inline`
  forms never call.

### 13.2 Aggregates

A struct or array value is a `Mem` value: a frame slot of its store size
(at most `frame::MAX_FRAME`, else `hod_cg::LIMIT_FRAME_SIZE`). `load`
and `store` copy through `eax` (the address) and `ecx` in pieces of 4,
2 and 1 bytes; an ordered (atomic) access reaches `atomic` first
(§12.2: an aggregate is `x86:atomic:type`). The result of `cmpxchg` is
such a value (§12.2: the value at 0, the flag at the second field's
offset), so every lowering here reads it.
`extractvalue` of a `Gpr` member loads it, of any other member copies it;
`insertvalue` copies the aggregate, then writes the member (a `Gpr` value
zero-extended to its store size, an integer of 33 to 64 bits by halves,
other members copied; an `undef` member is not written). `phi` is
`hod_cg`'s frame copy. `select` takes `lea` of each arm, `test`/`jne`,
and copies from `[eax]`. A constant aggregate is written from its
flattened bytes (a 4-byte symbol address through `common::sym_address`;
another relocated constant is `x86:constant`); a `zeroinitializer` above
64 bytes is `rep stosb`. Arguments and results follow `abi.rs`'s
classification unchanged: a result in memory is written through the
hidden pointer `Incoming(0)`, which `eax` returns. Zero-sized members and
types are no bytes. Members: `i1` to `i64` of any width, `ptr`, `float`,
`double`, `x86_fp80`, nested structs and arrays. Refused: vectors,
`half`, `bfloat`, `fp128` and integers above 64 bits as members (by
their family), an aggregate argument of more than 16 leaves
(`x86:aggregate`).

x87 barrier (measured with `llc` 19 and 22, `generic` and `i686`):
`select` of an aggregate branches without `cmov` like a scalar one,
`{i1}` included, except between two constants or of a zero-sized type;
with `cmov` none branches. An aggregate `load`/`store` never calls (up
to 1600 bytes measured).

### 13.3 Overflow intrinsics

`llvm.{s,u}{add,sub,mul}.with.overflow` at every width 1 to 64 return
`{iN, i1}` in a slot (the integer zero-extended to its store size, the
flag a byte at the second member's offset). At most 32 bits: the
operands sign- or zero-extended to 32 bits in `eax`/`ecx`, the
operation, and the flag from the processor's flags at `N = 32`, else
(the exact result in `eax`) unsigned: a bit at `N` or above set, signed:
`eax` is not its own extension from `N` bits. 33 to 64 bits, addition
and subtraction: the high words extended, `add`/`adc` (`sub`/`sbb`), the
flags of the high word at 64 bits, else the same test on the high word;
multiplication forms the 128-bit product of
the two 64-bit values extended from `N` bits (four `mul`s, a 32-byte
slot, the signed correction by masked subtraction) and overflows when it
differs from its extension from `N` bits. No call (no `__mulodi4`) and
no branch, as `llc` 19 and 22 at every width, `generic` and `i686`: not
an x87 barrier.

### 13.4 Integers of 33 to 63 bits by value

`abi.rs` gives an integer of 33 to 64 bits two leaves (4 bytes and the
rest), so it is passed, returned (`edx:eax`) and passed to a variadic
callee as `llc` does. With this, `x86:i64` is not reachable: every other
use of such an integer is §8's.

### 13.5 Small intrinsics (`bits::small`)

None has a call or branch in `llc` 19 or 22 (`generic`, `i686`), so none
is an x87 barrier.

| intrinsic | code |
|---|---|
| `ubsantrap(k)` | `ud1 eax, [eax + k]` |
| `debugtrap` | `int3` |
| `x86.sse2.pause` | `pause` |
| `frameaddress(d)` | `ebp`, then `d` loads of the saved `ebp` (`d` > 256: `x86:frame-depth`) |
| `returnaddress(d)` | the same walk, then the word at `+4` |
| `addressofreturnaddress` | `lea [ebp + 4]` |
| `objectsize` | `hod_cg::const_fold::object_size`: -1, or 0 with `min` (`llc` evaluates known objects; the gap is the shared fold's) |
| `is.constant` | 1 for a manifest constant (`hod_cg::const_fold::is_manifest`), else 0 |
| `ptrmask` | `and` |
| `readcyclecounter` | `rdtsc` into the `i64`'s slot |
| `prefetch`, `clear_cache` | nothing |

`expect*`, `assume`, `lifetime.*`, `invariant.*` and the other hints are
removed in front of the back end. `eh.typeid.for` is §14's.

## 14. Exceptions (`eh`, WP-314, design 0008 I10)

- **Tables.** `hod_cg`'s unwind tables (`spec/eh.md`) are written for
  `EM_386` with `hod_eh::I386` (`spec/eh.md` §2.1): CIE `esp+4`, `eip`
  at `CFA-4`, the PIC encodings (FDE `0x1b`, personality `0x9b` through
  `DW.ref.<personality>`, a 4-byte hidden weak COMDAT slot, LSDA `0x1b`,
  type table `0x9b` through 4-byte stubs in `.data`), all `R_386_PC32`,
  in every relocation model. The FDE rows are §3's: `def_cfa_offset 8`,
  `ebp` at -8, `def_cfa_register ebp`, `ebx`/`esi`/`edi` at -12/-16/-20
  after the pushes, `def_cfa esp+4` after the one epilogue. Which
  functions get an FDE (`uwtable`, may unwind) is `hod_cg`'s rule, as on
  the other targets.
- **`invoke`** is the call sequence (`FnCx::call`, `pic::emit_call`
  under it): `hod_cg` records the call site and its pad. A callee-pop
  callee, an `sret` result, an `i64` or `x86_fp80`/`double` result (§8,
  §11) are those of a call; the result is stored on the normal path. The
  constant fold's rewrites (`spec/codegen.md` §5.6.8 step 4: an
  `invoke` of a `nounwind` or `noreturn` callee, ...) are honoured as on
  the 64-bit targets (the call sequence reads `FoldAction`). `musttail`
  is never an `invoke` (the IR verifier rejects it).
- **`landingpad`** (`{ ptr, i32 }`, class `Mem`, 8 bytes): the clauses go
  to the pad's row; the exception pointer (`eax`) and the selector
  (`edx`) are stored to the value's home at the pad's first instruction,
  as `llc` does. `eax` holds no value across instructions and `edx` is
  scratch (§2), so both are still the unwinder's there. `extractvalue`
  of field 0 or 1 is a load of the home at 0 or 4; `insertvalue` into
  such a value copies it (`poison`/`undef` copy nothing,
  `zeroinitializer` writes zeros) and stores the field. `target.rs`
  sends these two to `eh` before the general aggregate arms (§13.2),
  whose layout of `{ ptr, i32 }` is the same (the pointer at 0, the
  `i32` at 4), so a pad's value stored, loaded, selected or passed as an
  aggregate reads the same; any other constant aggregate `insertvalue`
  operand is `x86:aggregate`.
- **`resume`**: the exception pointer to `_Unwind_Resume` through the
  call sequence (`@PLT` with `ebx` under PIC), then `ud2`.
- **`llvm.eh.typeid.for(@ti)`**: the type id of `@ti` in the function's
  table, an `i32` constant.
- **PIC.** No GOT pointer survives into a pad: it is computed where it is
  needed (§9.2), so a pad's first PLT call or `@GOTOFF` access computes
  it again.
- **x87 (§11).** An `invoke` is a terminator, so a barrier: no 80-bit
  copy lives across it; its `double` result is stored at 64 bits on the
  normal path. A landing pad starts a block (a new run). `resume` ends
  one. Measured in `llc -O0` assembly (19 and 22, CPU `generic` and
  `pentiumpro`): a `double`/`float` product computed before an `invoke`
  is stored rounded (`fstpl`/`fstps`) before the call and reloaded in
  both successors; one computed and used inside the pad stays in an x87
  register. `eh/callconv.ll` runs both shapes (§14 tests).
- **Static model.** hod writes `llc`'s PIC encodings in the tables in
  every relocation model (as on x86-64); `llc`'s static tables use
  absolute encodings. A documented deviation: the static objects' tables
  differ from `llc`'s by design and are tested by linking and running
  (`-no-pie`, three linkers) and by decoding the LSDAs.
- **Tests** (`crates/hod/tests/i386_eh.rs`, programs in
  `tests/data/i386/eh/`): C++ programs by clang 19 and 22 (throw and
  catch by value, reference and base class, rethrow, nested `try`,
  catch-all, destructors in cleanups, dynamic exception specifications,
  `noexcept` and `std::terminate`), a hand-written module with values
  live in `ebx`/`esi`/`edi` across throwing calls, an `i64` and a
  `double` `invoke` result, `llvm.eh.typeid.for`, a filter and a
  `resume`; static, PIE and `-march=i586` (no `cmov`), each by `llc`,
  hod and hod under `Stress::ALL`, linked by GNU ld, lld and mold, run
  under `qemu-i386`: every run equals `llc`'s. A throw across a shared
  library boundary both ways with a general-dynamic thread-local
  variable in the catching frame. The tables pass `llvm-dwarfdump
  --verify` (19, 22) and decode as `llc`'s: the same CIEs, the same FDE
  count and LSDA count, the same prologue rules per FDE (without
  `ebx`/`esi`/`edi`, which hod always saves), the same relocation kinds
  of `.eh_frame`, `.gcc_except_table` and the `DW.ref` slot.
  `eh/callconv.ll`: `invoke` of an `sret` callee (it pops the pointer),
  of an `i64` result and of a `fastcc` callee, each throwing and
  returning, at CPU `generic` and `pentiumpro`, and the x87 shapes above.
  An `invoke` of `x86_stdcallcc`/`x86_fastcallcc` is refused under
  `cc:x86_stdcallcc`/`cc:x86_fastcallcc`, as a call is. The LSDAs of
  `basic.cpp`, `live.ll` and `callconv.ll` (static and PIC) decode as
  `llc`'s: the encodings (PIC), per function the call sites (with or
  without a pad, merged where consecutive ones are equal) and their
  action chains with the catch types, cleanups and filters resolved
  through the relocations. Which pad is not compared: where several
  `invoke`s unwind to one IR pad, `llc` has one landing address and hod
  one per `invoke`.

## 15. Debug information (WP-314, design 0008 I10)

hod writes `spec/dwarf.md`'s DWARF 5 sections on i386 as on the 64-bit
targets, at address size 4: the unit headers, `.debug_addr`, the
`.debug_rnglists` and `.debug_loclists` headers and every address are 4
bytes, each relocated by `R_386_32` (`hod_dwarf::target::I386`, chosen
by the object's `Machine`). The frame base is `DW_OP_reg5 EBP`; a
variable or a parameter is described in its home (`DW_OP_fbreg`, the
parameters in hod's slots, `spec/dwarf.md` §6.17), a `byval` argument
through the pointer hod saved. A thread-local variable is
`DW_OP_const4u` with `R_386_TLS_LDO_32`, then
`DW_OP_GNU_push_tls_address`, as `llc` writes it.

- **Lines.** `spec/dwarf.md` §5 unchanged. The one epilogue is at the
  function's end (`spec/codegen.md` §4), so after a landing pad laid out
  last its row is a statement (`spec/dwarf.md` §5.2, §5.4) where `llc`'s epilogue row,
  in the `ret`'s block, is not.
- **Location lists** (IR at `-O1`, `#dbg_value` records). The DWARF
  stack is 32 bits wide: a range whose value expression pushes a value
  wider than 4 bytes (an `i64`, a `double`), an address read from a
  home, or a `DIArgList` gets no entry (`<optimized out>`); a home with
  the empty expression is a memory location of any size; a 4-byte home
  with a value expression is read with `DW_OP_deref_size 4`
  (`loclists.rs` `fits_i386`). `llc` writes 64-bit stack values there,
  which gdb prints truncated (WP-314's record).
- **Call sites.** `emit_call` reports the return address right after
  the `call`, before `esp` is moved back over a popped `sret` pointer
  (`spec/dwarf.md` §6.19).
- **Unwind tables.** `.debug_frame` and `.eh_frame` as in §14.
- **Refused** (§7): `debug:dwarf-4-i386`, `debug:split-dwarf-i386`.
- **Tests** (`crates/hod/tests/i386_debug.rs`, programs in
  `tests/data/i386/debug/`): `vals.c`, `tlsg.c`, `eh.cpp` by clang 19 and
  22 at `-O0 -g`, static and PIC: hod's objects verify, their unwind
  tables decode, the DIE trees equal `llc`'s under §6.4 and §6.17, the
  epilogue rows follow the rule above and the other statement rows
  equal `llc`'s. `opt.c` at `-O1`: verifies, has location lists, call
  sites as many as `llc`'s, each after a `call`. The seed
  `tests/ir/debug_info_locals.ll` runs in irdiff's i686 column. The
  debugger sessions (gdb-multiarch under `qemu-i386 -g`) are in WP-314's
  record; the debug lane has no i686 column yet (`xtask/src/debug.rs`
  `NO_I686_LANE`).

## 16. Scalar SSE (`sse`, WP-317, design 0008 I12)

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

## 17. Vectors (`vector`, WP-317, design 0008 I12 step C)

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
`icmp` (`i64` lanes by `cmp`/`sbb` of the halves), `fcmp` (§16's
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

## 18. Coroutines (`coro`, `coro_body`; WP-327, WP-335)

The handle operations are lowered in any function, as LLVM's
`coro-early` and `coro-cleanup` leave them on i386 (measured with `opt
-passes='default<O0>'` + `llc`, 19 and 22). A coroutine body
(`presplitcoroutine`, the module a front end writes before LLVM's
passes) is compiled once, as on the 64-bit targets (`spec/codegen.md`
§7.6); §18.1 is what is particular to i386.

- **The frame header.** The resume function's address is at offset 0
  and the destroy function's at 4. Each is a `fastcc void(ptr frame)`
  function: the frame in `ecx`, and the caller pops nothing.
- **`coro.resume(p)` / `coro.destroy(p)`:** `mov r, [p + 0 | 4]`, then
  the call sequence through `r` with `p` as the `fastcc` argument
  (`FnCx::call_through`), as an `invoke` too.
- **`coro.done(p)`:** `cmp dword [p], 0`, `sete` (into `cl` when the
  result's register has no low byte), `movzx`.
- **`coro.promise(p, align, from)`:** `lea r, [p ± alignTo(8, align)]`
  (LLVM's `{ptr, ptr, i8}` field 2). The alignment is a constant power
  of two of at most 2^30, and the direction is a constant; otherwise
  `x86:coro-promise`.
- **`coro.noop()`:** the address of `.LNoopCoro.Frame.Const`, 8 bytes
  aligned to 8, two `R_386_32` words naming `__NoopCoro_ResumeDestroy`,
  whose code is `ret`. The address is `lea r, [frame]` with `R_386_32`,
  or under PIC it is formed from the GOT pointer with `R_386_GOTOFF`, as
  `llc` writes it.
- **The barrier rule (§11).** `coro.resume` and `coro.destroy` are
  barriers: a `float` or `double` live across either is stored rounded
  to its own width and loaded again after it. This is measured: `llc` 19
  and 22 spill such a value as 8 bytes across both. Without the barrier,
  `handles.ll` prints `0x0p+0` where `llc` prints `-0x1p-54`.

Tests: `crates/hod/tests/i386_coro.rs` (`coro/handles.ll`, static and
PIE, default CPU and `-mcpu=i686`, three linkers, plain and under
`Stress::ALL`, against the `opt` + `llc` oracle).

### 18.1 Bodies (`coro_body`, WP-335)

- **The frame base is `esi`**, reserved for the whole function: it is
  out of the pool and the holdable set (`REG_INFO_CORO`), so a body's
  general pool is `ebx` and `edi`, and no lowering pins more than two
  (`MIN_POOL_GPR_32`, WP-331). Under PIC `ebx` is also the GOT pointer
  of every PLT call (`pic::emit_call` displaces the value it holds), so
  across such a call `edi` is the one pool register that keeps a value.
  `esi` is callee-saved in every convention hod writes; the prologue's
  save, CFI row (CFA − 16) and every exit's restore are the frame's.
  `FrameRef::Coro(o)` is `[esi + o]`, formed in `common::frame_mem` only.
- **The header** is two pointers (8 bytes): resume at 0, destroy at 4,
  the promise at `alignTo(8, align)` (the plan's header follows the
  data layout's pointer size).
- **Entry.** The prologue stores 0 to the mode word and the frame word.
  `coro.begin(id, mem)`: `mov esi, mem`; the stubs' addresses without
  a relocation in every model (`call .+0` · `pop ecx` · `lea edx, [ecx
  + stub − end]` · `lea edx, [edx + end − pop]`), stored at 0 and 4;
  the frame word := `esi`. `F.resume` and `F.destroy` are `fastcc
  void(ptr)` stubs after the function: the prologue with its rows, `mov
  esi, ecx`, mode := 1, frame word := `ecx`, `eax` := 0 (resume) or 1
  (destroy), then `cmp dword [esi + index], k` · `je` per re-entry,
  `ud2` after the last.
- **`coro.suspend`:** `mov eax, -1`, the re-entry label, the result
  from `eax`. `coro.save` stores the index (and 0 to the resume word at
  the final suspend). The dispatch compares the `i8` low byte (through
  `cl` when the register has none: `edi`).
- **Exits.** A ramp that pops (`sret`: `ret $4`) tests the mode word
  before the teardown: a resumed activation returns with `ret`.
  `coro.end` not unwinding: a resumed activation leaves by the
  epilogue; unwinding: it stores 0 to the resume word and the final
  index. `coro.free` and `coro.frame` are `esi`; `coro.alloc` 1;
  `coro.size` and `coro.align` the layout's (an `i64` result in its two
  halves).
- **Symmetric transfer** (after the handle wrapper returned `h` in
  `eax`): `mov edx, [eax]` · `mov ecx, eax` · `cmp dword [ebp + mode],
  0` · `jne exit` · `call edx` (through `pic::emit_call`); `exit` tears
  the frame down and `jmp edx`.
- **Barriers (§11).** `coro.suspend` and the three `await.suspend`
  wrappers are x87 barriers (with `coro.resume` and `coro.destroy`): a
  resumed activation's machine stack does not hold an 80-bit copy's
  slot, and `llc` lowers each with a call or a function boundary. A
  `float` or `double` live across a suspend is a heap value of its own
  size, so it is rounded there. `llc` can differ: `CoroSplit`
  rematerialises an arithmetic instruction after the suspend from its
  spilled operands and computes it again in 80 bits (measured, 19 and
  22). The IR's rounding is hod's; the excess precision is the oracle's
  (`spec/harness.md` `x87-precision`).
- **Exceptions and debug information** are the 64-bit targets': a
  landing pad runs in the activation that made the call, `esi` restored
  by the unwinder from the prologue's or the stub's rows; variables'
  locations go through the frame word (`spec/dwarf.md` §6.16).
- **Refused:** inline assembly in a body that writes `esi` is
  `x86:coro:asm-base` (an operand is never given `esi`).
- **With §19 (the join of WP-324 and WP-335).** A protected body's ramp
  copies its stack arguments below the guard with `esi` as scratch
  (`ssp::copy_arguments`) before `esi` is the frame base (`coro.begin`;
  under `Stress::coro_frame` the `mov esi, ebp` follows the copy). A
  resumed activation has a machine frame of its own: each stub writes,
  after the frame word, the guard word (and the low guard), and the
  guarded block's pointers again (`ssp::stub_enter`, as `llc` guards
  `F.resume` and `F.destroy`), so the epilogue's check holds; without it
  every resumed activation of a protected body called
  `__stack_chk_fail`. Under `cf-protection-branch` each stub begins with
  `endbr32` (`cet::landing`), as `llc`'s `F.resume` and `F.destroy`
  do. `indirectbr` in a body is the function's own code in every
  activation (one function, one frame layout); clang refuses the
  address of a label in a coroutine, so no C++ program reaches it.
  Test: `i386_coro.rs` `protected_bodies_run_as_llc_s`
  (`coro/protected.cpp`, `-fstack-protector-strong`, `-all`, `-all`
  with `-fcf-protection=branch`).

Tests: `crates/hod/tests/i386_coro.rs` (`bodies_run_as_llc_s`: the
programs `coro/gen.cpp`, `transfer.cpp`, `throws.cpp` by clang 19 and
22, CPU `generic` and `i686`, static and PIE, plain and stressed;
`bodies_with_debug_information_verify_and_run`; `what_is_refused`) and
the `tests/ir/coro` seeds through `irdiff --target i686`.

## 19. Indirect branches, stack protectors, control-flow protection, `llvm.eh.unwind.init` (WP-324)

### 19.1 Indirect branches (`ibr`, WP-324)

`indirectbr ptr %a, [...]` is `jmp r` on the address's register; on a
`blockaddress` constant of the function itself it is a direct branch to
that block (as on x86-64). A `blockaddress` of the function's own block
in a register is position independent in every relocation model and
needs no relocation:

```text
call 0f                ; e8 00000000: pushes the address of 0
0: pop rd
lea rd, [rd + disp32]  ; REL32 fixup to the block's label: label - (0 + 7)
lea rd, [rd + 7]
```

It writes `rd` only and keeps the flags. `llc` writes `movl $.Ltmp, %r`
(`R_386_32`) in the static model and `leal .Ltmp@GOTOFF(%ebx), %r` under
PIC; the value is the same. Neither is an x87 barrier beyond what `llc`
has: `indirectbr` ends its block, a `blockaddress` is one `mov`/`lea` in
`llc`. A `blockaddress` in a global's initializer is the module
emitter's (`R_386_32` against the function's section, plus the block's
offset). The address of another function's block in code stays refused,
`x86:blockaddress`. Evidence: `tests/i386_ibr.rs` (a computed-goto
interpreter with a static table of label addresses, one built at run
time, a label address selected and compared; `-march=i586` and the
default CPU, static and PIE, GNU ld, lld and mold, `llc` 19 and 22, also
under `Stress::ALL`).

### 19.2 Stack protectors (`ssp`, WP-324)

`ssp`, `sspstrong` and `sspreq` follow `spec/codegen.md` §5.6.24 rules 1
to 4 and 7 unchanged (which functions, which slots move into the guarded
block and in which order, the block at the top of the local area, the low
guard at `Local(0)` with a dynamic area, the check at every exit
sequence: the epilogue and every `musttail` exit; none at `unreachable`,
a `noreturn` call or `resume`). The i386 parts:

- **The guard value** is `gs:[0x14]` (glibc's TCB `stack_guard`), what
  `llc` 19 and 22 read for `i686-*-linux-gnu` in the static, PIC and PIE
  models alike. The guard word is the block's 8 bytes
  (`hod_cg::GUARD_BYTES`); the value is in its first 4, right after the
  top slot. The module flags `stack-protector-guard*` are refused by the
  front half (`module-flag:<name>`, as on the other targets); `safestack`
  stays refused (`attr:safestack`).
- **Entry code** (scratch `ecx`, `edx`): `mov ecx, gs:[0x14]` · `lea edx,
  [ebp + block]` (a local access whose displacement `end_function`
  writes before the bias) · `mov [edx + guard], ecx` · (`mov [low],
  ecx`) · per slot of the block `lea ecx, [edx + k]` · `mov [pointer],
  ecx`.
- **The check**: `mov ecx, [guard]` · `cmp ecx, gs:[0x14]` · `jne fail`
  (and the same for the low guard). At a `musttail` exit `ecx` may hold
  an argument (`fastcc`, `inreg`): there the load and compare are
  bracketed by `push ecx` · `pop ecx` (`pop` keeps the flags). The result
  registers (`eax`, `edx`, `st0`) are not touched.
- **The failure call** follows the last exit, under the body's CFA rule:
  `call __stack_chk_fail` through `pic::emit_call` (`R_386_PLT32`; under
  PIC with the GOT pointer in `ebx` first). `llc` 19 and 22 call
  `__stack_chk_fail@PLT` in the static, PIC and PIE models; they never
  call `__stack_chk_fail_local`.
- **x87**: `llc` checks the guard in a block of its own (`SP_return`)
  before the `ret`, so a `float`/`double` result live into the return
  is stored at its type before the check and reloaded: a function `llc`
  protects returns its result rounded to its type, never the 80-bit copy
  of §11 (`control::ret` passes `!ssp::checks_at_return` to
  `x87::ret_copy`). Measured with `llc` 19 and 22 at `-O0`, CPU
  `generic` and `i686`, on a `float` product computed after a call and
  one computed before it (both are `fstps` · check · `flds` · `ret`).
  hod protects a superset of `llc`'s functions and its own check does not
  touch the x87 stack, so the rounding follows `llc`'s choice as far as
  hod sees it without slot types: `sspreq`; a dynamic area; under
  `sspstrong` an escaping slot; under `ssp` an escaping slot of at least
  8 bytes (clang's default `"stack-protector-buffer-size"`). Where that
  guess differs from `llc` (a struct local reached only through constant
  field offsets under `sspstrong`, an escaping non-character array of 8
  bytes or more under `ssp`) a `float`/`double` result live into the
  return is rounded by hod and not by `llc`. The check is no barrier
  elsewhere: nothing else is live across it.
- **The incoming stack arguments are copied below the guard.** They lie
  above the return address, so an overflow that gets past the guard
  reaches them before the check runs. `llc` 19 and 22 (`-O0`, CPU
  `generic` and `i686`, static and PIC) load every stack argument the
  body uses after the entry block in the entry block, and keep it in a
  spill slot below the protected arrays: the struct result's hidden
  pointer (`spec/abi-x86.md` §5.2) for each store of the result and for
  the `eax` result, a scalar or pointer argument for each later use. It
  reads `byval` copies in place. In a protected function hod copies, in
  `begin_function` right after the low guard's slot and before any
  argument register is read, every word of the incoming area that holds
  the hidden pointer or a part that is not `byval` (scratch `esi`, which
  the prologue saved and which carries no argument). The copy goes into
  one local slot, 16-aligned, that keeps each byte's offset modulo 16.
  Like every slot outside the guarded block, it lies below the block
  (`spec/codegen.md` §5.6.24 rule 3). From then on every access to those
  bytes (`ssp::incoming`, applied by `common::frame_inst`) goes to the
  copy: argument homes, the hidden pointer for the stores of the result
  and for `eax`. The exception is a `musttail` call's copy of its stack
  arguments into the real incoming area. An overflow therefore never
  makes the body write through an overwritten pointer before the check
  aborts. `byval` copies stay in place, as under `llc`. Debug locations
  of stack arguments still name the incoming slot, which holds the same
  value until it is overwritten.

Evidence: `tests/i386_ssp.rs`: `ssp/smash.c` with `-fstack-protector`,
`-strong` and `-all` and `ssp/tail.ll` run as `llc`'s (output and status;
`-march=i586` and the default CPU, static and PIE, GNU ld, lld and mold,
`llc` 19 and 22, also under `Stress::ALL`); with `-DOVERFLOW=1..6` (one
buffer, several slots, the low guard, an `i64` result, a `float` result,
a struct result through its hidden pointer) every build aborts with
`*** stack smashing detected ***` and SIGABRT, as `llc`'s. `ssp/args.ll`
(the hidden pointer, a pointer argument and a scalar argument on the
stack, used after the buffer is written) runs as `llc`'s in bounds. With
its overflow, every build aborts the same way (CPU `generic` and
`i686`, static and PIE, `llc` 19 and 22, every linker, also under
`Stress::ALL`). Without the copy, hod's build faulted on the store
through the overwritten result pointer.

### 19.3 Control-flow protection (`cet`, WP-324)

The module flags `cf-protection-branch` and `cf-protection-return`
(clang's `-fcf-protection=`) compile; `x86:cf-protection` is gone.

- **The property note.** `hod_cg`'s emitter writes `.note.gnu.property`
  for i386 as for x86-64: one `NT_GNU_PROPERTY_TYPE_0` note owned by
  `GNU` with `GNU_PROPERTY_X86_FEATURE_1_AND` (`0xc0000002`), bit IBT (1)
  for `cf-protection-branch`, bit SHSTK (2) for `cf-protection-return`.
  In ELF32 the property array is padded to 4 bytes and the section is
  aligned to 4 (`hod_obj`'s `add_gnu_properties`, the padding the
  machine's address size): 28 bytes, `llc`'s byte for byte.
- **`endbr32`** (`F3 0F 1E FB`) when the module has
  `cf-protection-branch`, where `llc` 19 and 22 put it
  (`X86IndirectBranchTracking`): first in every function but a
  `nocf_check` one, before `push ebp`; at every block whose address is
  taken and every landing pad (`Target::branch_target`, which `hod_cg`
  also calls at an `invoke`'s pad stub); right after a direct call of a
  function declared `returns_twice`. hod's places are a superset of
  `llc`'s: `llc` leaves the entry of a local function whose address is
  not taken unmarked, and puts one marker per landing pad where hod has
  one at the stub and one at the pad. `endbr32` is a no-op outside a CET
  process.
- **`cf-protection-return`** changes no code: hod's returns go to the
  address its calls pushed.
- **Refused**: an indirect call with `nocf_check` in a module with
  `cf-protection-branch` needs the `notrack` prefix, which the encoder
  lacks (`x86:nocf-check-indirect-call`, as on x86-64). Without the flag
  `nocf_check` changes nothing. hod makes no jump table, so it has no
  `notrack` jump.

Evidence: `tests/i386_cet.rs`: `cet/cet.c` and `cet/throw.cpp` built
with `-fcf-protection=full` run as `llc`'s (static and PIE, GNU ld, lld
and mold, `llc` 19 and 22, also under `Stress::ALL`); the note's bytes,
type, flags and alignment are `llc`'s and `llvm-readelf -n` reads `x86
feature: IBT, SHSTK` in both; per function hod has at least `llc`'s
`endbr32` count, one at every entry where `llc` has one, one after each
`setjmp` call and none at the `nocf_check` function's entry; a
freestanding program linked `-nostdlib -static` keeps the property
through GNU ld, lld and mold when every object has it and loses it when
one does not, as with `llc`'s objects.

### 19.4 `llvm.eh.unwind.init` (WP-324)

`llc` 19 and 22 make a function that calls `llvm.eh.unwind.init` save
every callee-saved register: `ebp` (the frame pointer), `ebx`, `edi`,
`esi`, with their CFI rows. Every hod frame already saves that set
(`frame::begin`, §3), so the intrinsic emits nothing (`call.rs`). The
other frame intrinsics are §13.5's; `llvm.eh.dwarf.cfa` stays
`x86:intrinsic`.

Evidence: `tests/ir/frame_intrinsics.ll` (newly built on i686) passes
`cargo xtask irdiff --target i686` with LLVM 19 and 22, GNU ld, lld and
mold.

## 20. Vectors without SSE (`vecscalar`, WP-338)

(Section number given at the merge.) A function whose CPU has no SSE at
all (`generic`, `i586`, clang's default `i686`: `+cmov,+cx8,+x87`).
With SSE but not SSE2 (`pentium3`) `llc` keeps `<4 x float>` in `xmm`:
still `x86:vector:no-sse2`.

**Measured** (`llc` 19 and 22 alike, `-O0 -global-isel=0
-fast-isel=0`, CPU `i686` and `generic`): `llc` scalarizes every vector
into its elements.

- *Memory.* The same image as §17: lanes packed from lane 0, `<N x i1>`
  bit by bit (lane 0 in bit 0 of byte 0); a vector `alloca`, `load`,
  `store` keeps its IR alignment.
- *Arguments.* Each lane is an argument of its own, in order, as the
  scalar of its type is (§3 of `spec/abi-x86.md`): a lane of 1 to 4
  bytes in a 4-byte slot, `i64` and `double` lanes in 8 bytes, at
  alignment 4 (`<4 x i32>` after an `i32` at offsets 4..16, `<32 x
  double>` in 256 bytes, no 16-byte slot). `fastcc` gives integer lanes
  `ecx`, `edx` then the stack; `inreg` gives `eax`, `edx`, `ecx`.
- *Results.* The lanes as the scalars of an IR aggregate's leaves
  (`spec/abi-x86.md` §5): up to three integer words in `eax`, `edx`,
  `ecx` (`<3 x i32>`; `<2 x i32>`, `<1 x i64>` in `edx:eax`; `<3 x i8>`
  in `al`, `dl`, `cl`; `<2 x i16>` in `ax`, `dx`), one floating lane in
  `st0` (`<1 x double>`), two in `st0`, `st1` (`<2 x float>`); anything
  else through the hidden result pointer, popped by the callee (`<4 x
  i8>`, `<2 x i64>`, `<4 x float>`, `<32 x i8>`). Under `fastcc` the
  hidden pointer is in `ecx` and the callee pops nothing.
- *Floating lanes* are x87 values (§11): `fld`, operate, `fstp` per
  lane, and `llc` keeps a computed lane at 80 bits for its uses in the
  block exactly as a scalar (measured: `(a + b) - a` of `<2 x double>`
  has no rounding between the two operations). A float lane passed as an
  argument goes through `flds`/`fstps` (a signalling NaN is quieted);
  `fneg` is `fchs` or an integer sign flip, as the DAG happens to choose.
- *Barriers.* Nothing in a vector operation branches or calls with
  `cmov`; without `cmov` a lane `select` and the `min`/`max` reductions
  branch.

**The scheme.** A vector is of class `Mem` with its memory image in its
home, as in §17, and every operation is a loop-free sequence over the
lanes through `eax`, `ecx`, `edx` (integers, `i64` lanes in halves as
§8) or the x87 stack (floating lanes, pushed from and popped to the
frame), so no vector holds a pool register. The integer forms of §17
that use no `xmm` register are §17's code unchanged (`vector.rs`);
`vecscalar.rs` lowers the others: `fadd`, `fsub`, `fmul`, `fdiv` and
`fcmp` of `float`/`double` lanes on the x87 (`fucomip` with `cmov`,
else `fucompp`, `fnstsw`, `sahf`), `insertelement` of an `i64`,
`float` or `double` lane and of an `i1` lane at a variable index
through `eax`/`ecx`. Each floating lane is rounded to its type when it
is popped to the result.

**Excess precision.** hod keeps no 80-bit copy of a lane. A floating
operand of an x87 lane operation (`fadd`..`fdiv`, `fcmp`,
`extractelement` of a floating lane) that `llc` would still hold at 80
bits is refused (`x86:vector:no-sse2:excess-precision`): one defined
earlier in the same block by a computing instruction (a floating
operation, a call) or by a lane move (`insertelement`,
`shufflevector`, `select`, `freeze`, `fneg`) of such a value. A
result of one or two floating lanes returned by `ret` from such a
value is refused the same way. Stores, call arguments and `bitcast`
round in `llc` too and are lowered. A floating operation whose
operands are constants (`llc` folds it: one rounding, `+qNaN`) is
refused (`x86:vector:no-sse2:fold`). hod counts a call between the
definition and the use as no end of the 80 bits (it refuses where
`llc` would read the value rounded).

**Not lowered** (`x86:vector:no-sse2:<what>`): `:cast` (a cast from or
to floating lanes), `:select` (without `cmov`), `:reduce` (`min`/`max`
reductions without `cmov`), `:abi` (an `<N x i1>` or `inreg` vector
argument or result). As for an aggregate, a `fastcc` vector of floating
lanes or returned in memory is `x86:abi:cc` and a result of two
floating lanes (`st0`, `st1`) is `x86:abi:x87-pair`. The other
refusals of §17 (`:element`, `:i64-op`, `:frem`, `:intrinsic`,
`:length`, ...) hold. `vecscalar::ret_memory` writes a vector result
in memory (`control::ret` sends it there, not to `agg`).

**No barrier.** No construct of this section is added to
`x87::barrier`: the select and reductions that branch are refused.

## 21. Floating-point classes and `modf` (`fpclass`, WP-337)

**`llvm.is.fpclass`** of a scalar `float`, `double` or `x86_fp80`, in a
function with or without SSE / SSE2: integer tests of the operand's
home. A `float` / `double` home holds the value rounded to its type, so
the class is the rounded value's, never the 80-bit copy's (§11): this
is a rounding point, as in `llc`, which stores the value at its type and
tests the bits (for the masks "NaN" and "not NaN" it compares the value
with itself, which rounding does not change). The class's index in the
mask (`LangRef`'s bits: 0 signalling NaN, 1 quiet NaN, 2 negative
infinity, ..., 9 positive infinity) is computed in `ecx` (a `double`'s
low word folded into bit 0 of the high word's magnitude, which no bound
uses), then `bt` of the mask in `edx`, `setb`; the result is the only
pool register. Measured with `llc` 19 and 22 over all 1024 masks at CPU
`generic` and `i686`, with an operand that is a product of the block:
`llc` never branches, so `is.fpclass` is no barrier (§11), and the run
continues across it.

*`x86_fp80` encodings only the x87 has* (measured, all 1024 masks):
- an unnormal (a non-zero exponent without the integer bit, a pseudo-zero
  included), a pseudo-infinity, a pseudo-NaN (the maximum exponent
  without the integer bit): true exactly when the mask has both NaN
  classes or is everything but one NaN class (`llc` 19 and 22 alike);
- a pseudo-denormal (exponent 0, integer bit set): `llc` 22 compares
  the value for the masks 3, 7, 99, 243, 508, 515, 780, 924, 1016, 1020
  (a pseudo-denormal is then a normal number of its sign); for the
  other masks it tests bits: true when the mask has both NaN classes or
  all four zero and subnormal classes, with the test inverted first
  when its complement is one class, one class of one sign, the finite
  classes (of one sign), zero or NaN, subnormal or zero, subnormal or
  zero or NaN (`invertFPClassTestIfSimpler`). hod does what `llc` 22
  does. `llc` 19 compares only 3, 99, 924, 1020 and differs from 22 at
  the masks 7, 243, 515 and 780 for a pseudo-denormal. (x86-64 decided
  otherwise: `spec/codegen.md` §5.6.12 counts a pseudo-denormal as a
  subnormal, as `fxam` does, for every mask; on i386 the two differ at
  459 of the 1024 masks, measured.)

A vector operand keeps the classifier's `intrinsic:llvm.is.fpclass/vector`;
`half` keeps its refusal. The mask is an `immarg`; a non-constant one
would be `x86:intrinsic`.

**`llvm.modf`** of `float`, `double`, `x86_fp80` (LLVM 20 and later;
clang 22 writes it for `modf`, `modff`, `modfl` on every target, kiln
calls the routines): `modff` / `modf` / `modfl` (`llc` 22, measured)
with the operand at its width in the outgoing area and the address of
the result's second field (the integral part) after it, through
`pic::emit_call`; the fractional part is popped from `st0` to the first
field at its type's width (exact). The call is a barrier (`x87::barrier`;
`llc` 22 does not fold it, and spills a product of the block across it,
measured). In an SSE function the same sequence (`llc` stores `st0`).
On x86-64 and AArch64 `llc -O0` makes the same plain call (the operand
in `xmm0` / `d0` / on the stack for `x86_fp80`, the pointer in `rdi` /
`x0`), and hod makes it there too (`spec/codegen.md`, WP-340; what
stays refused on those targets is `intrinsic:llvm.modf`).

**`llvm.frexp`** of `float`, `double`, `x86_fp80` (WP-341; clang 19
and 22 write it for `__builtin_frexp*`, libc++'s `frexp`): the same
sequence as `modf` with `frexpf` / `frexp` / `frexpl` (`llc` 19 and 22,
measured at CPU `generic`, `i686`, with SSE2, static and PIC): the
operand at its width in the outgoing area (4, 8, or 10 of 12 bytes),
the address of the result's second field (the `i32` exponent, written
by the routine) after it, the fraction popped from `st0` to the first
field at its type's width; `llc`'s exponent temporary is a stack slot
it copies to the result, hod's is the result's field itself. The call
is a barrier (`x87::barrier`) unless `llc` folds it: an operand that is
a constant or a folded result (`llc` 19 and 22 fold `frexp` of a
constant, with exponent 0 for an infinity or a NaN as libm's); hod
still calls the routine there (the same values) but keeps the block's
80-bit copies, as `llc`. A `half`, `fp128` or vector operand is the
classifier's `intrinsic:llvm.frexp/<class>` on i386.

**`bitcast` between `x86_fp80` and `i80`**: the 10 bytes copied
between the homes (the `i80`'s is a slot of its size; its bytes above
80 bits are unspecified). `icmp` of an `i80` with the constant 0, every
predicate, reads two facts of the 10 bytes: negative (bit 79) and zero
(clang's `signbit` of a `long double` is `icmp slt i80 %b, 0`). Every
other operation on an `i80` is §22's since WP-346 (an integer of three
limbs; a shift is §22's since WP-355).

**Not lowered:** `x86:atomic:libatomic` stays: `llc` 19 and 22 call
`__atomic_compare_exchange` (size 8) for a `cmpxchg i64` at `align 4`
on every CPU (`generic`, `pentium`, `i686`), never an inline `lock
cmpxchg8b`; hod's tests link no libatomic.

Tests: `crates/hod/tests/i386_fpclass.rs`, `i386_frexp.rs` (static and PIE, plain and
under `Stress::ALL`, CPU `generic`, `i686` and `pentium4` through
`target-cpu`; `llc` 19 and 22 but where named).

## 22. Integers of 65 to 128 bits (`wideint`, WP-346)

**Where they come from.** i386 has no `__int128`; clang 19 and 22
(`--target=i686-linux-gnu -O0`) write wide integers for the overflow
builtins with mixed signedness or widths. Over every triple of `int`,
`unsigned`, `long long`, `unsigned long long` (two operands, result)
of `__builtin_{add,sub,mul}_overflow` they write `i33` (already §13)
and `i65`, and on them only: `zext` / `sext` from `i32` / `i64` to
`i65`, `llvm.{sadd,ssub,smul}.with.overflow.i65`, `extractvalue`,
`trunc i65` to `i64` / `i32`, `icmp ne i65`. `_BitInt(N)` and bit-field
or aggregate coercions reach the rest of this section.

**Representation.** An integer of `N` bits, `65 <= N <= 128`, is of
class `Mem`; its home is a frame slot of its alloc size holding `L =
ceil(N / 32)` 32-bit limbs, little-endian. The bits above `N` in the
top limb are **unspecified** (as §8): `add`, `sub`, `mul`, the bitwise
operations and `trunc` leave garbage there; a consumer that depends on
them (a compare, an extension, `store`, the overflow test) extends the
top limb by zero or sign first. In memory `llc` writes exactly the
store size (`ceil(N / 8)` bytes) with the bits above `N` zero, and its
`load` relies on that (measured: `icmp slt i65` of a loaded value is
`movzbl 8(%eax)`); hod's `store` writes the same image, its `load`
reads the store size. The bits above `N` in the last byte of an image `llc` did not write
are, for `llc`, undefined (LangRef: a load of a type that is not a whole
number of bytes of memory not written by a store of that type); `llc`'s
load keeps them in the register under an assumed-zero extension, so
what a consumer sees depends on the consumer. hod's load copies them
into the slot, where every consumer ignores them (the high bits rule
above): the result is the one of the value with those bits zero.

**Registers.** Every lowering works limb by limb in `eax`, `ecx`,
`edx` (never a value's) and pins no pool register but a `Gpr` operand's
or result's, or a pointer's. A carry travels in `CF` between limbs:
between two `adc` / `sbb` only `mov` loads and stores run, which keep
the flags; a top limb's extension (which writes them) runs before the
chain.

**Operations.**
- `add` / `sub`: `add` / `sub` of limb 0, `adc` / `sbb` of the others,
  each through `eax`. `and`, `or`, `xor`: limb by limb.
- `mul`: the low `L` limbs of the product: the operands copied into a
  frame area of their own, one `mul` per pair of limbs `(i, j)` with
  `i + j < L`, its `edx:eax` added into the product limbs `i + j` and
  `i + j + 1` with the carry rippled (`adc [p + k], 0`) to the top.
- `icmp`: equality `xor`s each limb pair, `or`s them into `ecx`; an
  order compare extends both top limbs into `ecx` / `edx`, then `cmp`
  of limb 0 and `sbb` of the middle limbs through `eax`, `sbb ecx,
  edx` (operands swapped for `>` and `<=`, so only `CF` / `SF` / `OF`
  are read).
- `select`: the condition tested, a copy of the chosen arm on each side
  of a branch (a constant condition copies one). `freeze`, `phi`: a
  copy.
- `zext` / `sext` / `trunc` / `bitcast` / `ptrtoint` / `inttoptr`
  between 1..32, 33..64 and wide: a narrower result takes the low
  limbs; a wider one the source's limbs, its top limb extended from its
  width, then limbs of its sign (`sar 31`) or zero.
- A constant: one immediate store per limb (`ptrtoint` of a symbol
  through `ecx`, the other limbs zero).
- `ptradd` / `getelementptr` index: the low limb (the index truncated
  to 32 bits, as `llc`).
- `insertvalue` of such a field writes its store size, the bits above
  `N` zero; `extractvalue` copies the store size.
- `llvm.{s,u}{add,sub}.with.overflow.iN`: both top limbs extended (sign
  for `s`, zero for `u`) into `ecx` / `edx`, the `add`/`adc`
  (`sub`/`sbb`) chain, `adc ecx, edx` (`sbb`) last. At `N = 32 L` (96,
  128) the flag is `OF` (signed) or `CF` (unsigned) of the last limb.
  Below, the exact result fits `32 L` bits (`N + 1 <= 32 L`): the
  `N`-bit operation overflowed when the top limb is not its own sign
  extension from `N - 32 (L - 1)` bits (signed) or has a bit set at or
  above them (unsigned; a borrow sets them all). This is `llc`'s rule:
  it promotes `i65` to three limbs, adds, and compares the result with
  its own `sext_inreg` from 65 bits.
- `llvm.{s,u}mul.with.overflow.iN`: the exact product over `2 L` limbs
  (the `mul` above with the operands extended by sign for `s`, by zero
  for `u`; `|a b| <= 2^(2N - 2)` fits); overflow when it is not its own
  sign extension from `N` bits (`s`) or has a bit at `N` or above
  (`u`). No call: `llc` 19 and 22 call nothing either (measured on
  `smul.with.overflow.i65`).
- The overflow result `{iN, i1}`: the integer zero-extended with its
  store size, the flag as a byte at its field offset.

**Shifts, intrinsics, `switch` (WP-355).** Same slot convention: an
operand's bits above `N` are ignored (its top limb is extended where they
could reach the result), a result's are unspecified.
- `shl` / `lshr` / `ashr` (`llc` 19 and 22 do the same, in line, with no
  call and no branch at every width and CPU): a frame buffer of `L + 4`
  limbs holds the value (top limb extended by zero, by sign for `ashr`)
  and 4 limbs of fill (0, or the sign) below it for `shl`, above it for
  the right shifts. With `q = amount / 32`, `s = amount % 32`, result
  limb `i` is `shld` of buffer limbs `i - q` and `i - q - 1` by `s`
  (`shl`) or `shrd` of `i + q` and `i + q + 1` (right). A constant amount
  chooses the limbs at compile time; one of `N` or more is poison and
  nothing is computed. A variable amount's low limb goes to `ecx`;
  `edx = &buffer -/+ 4 ((ecx >> 5) & 3)`; per limb `mov eax, [edx + d]`,
  stored, `mov eax, [edx + d']`, `shld` / `shrd [r + 4 i], eax, cl`. The
  mask keeps every access inside the buffer for any amount (an
  out-of-range amount is poison and computes garbage without a fault);
  the amount's other limbs are not read.
- `switch`: the top limb zero-extended into `edx`; per case `cmp dword
  [cond + 4 k], imm` and `jne next` for each lower limb, `cmp edx, imm`
  and `je case`; then the default. A constant condition is a branch.
- Intrinsics at 128 bits (the shared classifier refuses the integer
  intrinsics at the other widths above 64, `intrinsic:<name>/int-wide`;
  `hod_x86` lowers them at every width and the run tests reach the odd
  ones through its own entry): `ctpop` (each limb's count, the top one
  zero-extended, summed), `ctlz` / `cttz` (the first limb that is not
  zero from the top / bottom, `bsr`·`xor 31` / `bsf` plus 32 per limb
  passed; `cttz` sets bit `N` of the top limb below 32 bits; `N` for 0
  whatever the flag), `bswap` / `bitreverse` (each limb reversed into the
  opposite limb of a frame area, then shifted right by `32 L - N`),
  `abs` (`x ^ s - s` with `s` the sign, the `sub` / `sbb` chain run on
  the result's limbs after the `xor`s, which write the flags), `smax` /
  `smin` / `umax` / `umin` (the order compare above, a copy of the chosen
  operand on each side of a branch). Clang 19 and 22 write all of these
  from `_BitInt(N)` with `__builtin_{popcountg,clzg,ctzg}` and
  `__builtin_elementwise_{abs,max,min,bitreverse}`; they also write
  `{s,u}{add,sub}.sat` and (clang 22) `fshl` / `fshr`, which stay
  refused (`llc` calls compiler-rt's `__umodti3` for a funnel shift at an
  odd width; libgcc for i386 has none).
- x87 barriers (`x87::int_branch`, measured at `generic` and `i686` with
  `llc` 19 and 22): without `cmov` `llc` branches for `ctlz`, `cttz` and
  min / max as at 33 to 64 bits, and not for `ctpop`, `bswap`,
  `bitreverse`, `abs` or a shift; with `cmov` for none. The constants it
  treats apart are compared as 128-bit values: `umax` / `umin` with 0 or
  all ones and `smax` / `smin` with the width's own extremes do not
  branch; `smax` with `2^63` and `smin` with `2^63 - 1` at `i128` do.

None of these is an x87 barrier: `llc` lowers none of them with a
branch, a call or a loop but `select`, which §8 already lowers the
same way.

**Refused**, each under its own name (plain `x86:i64:wide` is no
longer reached):

| name | what |
|---|---|
| `x86:i64:wide:shift` | no longer reached (WP-355: every shift compiles) |
| `x86:i64:wide:div` | `udiv`, `sdiv`, `urem`, `srem` (libgcc has no `__divti3` on i386; `llc` calls compiler-rt's) |
| `x86:i64:wide:intrinsic` | an integer intrinsic but the overflow ones and those of "Shifts, intrinsics, `switch`" below: the saturating `add` / `sub` and shifts, the funnel shifts, `scmp` / `ucmp`; a width other than 128 is the classifier's `intrinsic:<name>/int-wide` first |
| `x86:i64:wide:switch` | no longer reached (WP-355) |
| `x86:i64:wide:abi` | a result by value of 65 to 127 bits, a call's (`abi::type_refusal`); an `i128` result is returned in memory as `llc` does (hidden pointer, `eax`, `ret $4`), and a wide argument is the convention's `x86:abi:wide-arg` first |
| `x86:i64:wide:width` | an integer wider than 128 bits |

Tests: `crates/hod/tests/i386_wide_int.rs`: hand-written `.ll` at 65,
96 and 128 bits over every pair of boundary values (carries across each
limb, the sign bit of the width, the extremes), clang's three overflow
builtins over every triple of the four C types (clang 19 and 22; CPU
`i686`, `i586`, `pentium4`), static and PIE, plain and under
`Stress::ALL`, `llc` 19 and 22, run under `qemu-i386`. The edges of the
limb-wise lowering are tests of their own there (`limb_edges_as_llc`):
a stored value between guard bytes and before a `PROT_NONE` page at
65, 72, 96, 100, 120 and 128 bits; values with garbage above the width
(a `trunc` from `i128`) through every compare, the overflow
intrinsics, the extensions, `mul`, `select`, a `phi`, a `trunc` to
`i65`; an `add` of a value to itself and swapping `phi`s; a wide
`getelementptr` index with high limbs set or negative. And
`multiply_random_as_llc`: `mul`, `smul` / `umul.with.overflow` over
every pair of the boundary values and 2,048 pairs of a fixed-seed
generator per width (65, 96, 128), each checked in the program against
the exact result.
