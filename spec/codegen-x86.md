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
| `x86:i64` | an integer of 33 to 64 bits where §8 does not take it: an argument or result passed by value, a `switch` condition (also wider), a `ptradd` index | I6 (`abi.rs`: arguments, results); — |
| `x86:i64:wide` | an integer wider than 64 bits (§8) | — |
| `x86:float` | a floating-point value, `fcmp`, `fneg` | I9 (`x87.rs`) |
| `x86:vector` | a vector value, a vector `ptradd`, the element operations | I12 |
| `x86:aggregate` | an aggregate value, `extractvalue`, `insertvalue`, a result in memory | I11 |
| `x86:pic` | a relocation model other than static | I8 (`pic.rs`) |
| `x86:tls` | a thread-local access, `llvm.threadlocal.address`, `llvm.thread.pointer` | I8 (`tls.rs`) |
| `x86:eh` | `invoke`, `landingpad`, `resume`; a call that may unwind in a function that may unwind (no unwind tables) | I10 |
| `x86:debug-info` | a module with debug information (`hod_cg`'s emitter) | I10 |
| `x86:cf-protection` | `cf-protection-*` module flags (`hod_cg`'s `.note.gnu.property`) | — |
| `x86:atomic` | an ordered load or store, `fence`, `cmpxchg`, `atomicrmw` | I11 |
| `x86:inline-asm:<detail>` | what inline assembly does not take (§10, `spec/asm.md` §22.5) | I13 (`asm.rs`) |
| `x86:abi:cc` | a calling convention other than C | I6 (`abi.rs`) |
| `x86:abi:attr` | `byval`, `sret`, `inreg`, `nest`, the Swift attributes, `inalloca`, `preallocated`, `alignstack` | I6 (`abi.rs`) |
| `x86:abi:musttail` | a `musttail` call | I6 |
| `x86:varargs` | `va_start`, `va_copy`, `va_end`, `va_arg` | I11 |
| `x86:dynamic-alloca` | a dynamic `alloca`, an over-aligned slot, `llvm.stacksave`/`stackrestore` | I11 |
| `x86:intrinsic` | any other intrinsic | per family |
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
  in an object that divides 64-bit integers.
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

Still refused as `x86:i64`: such an integer passed or returned by value
(I6, `abi.rs`), a `switch` on one (or wider), a `ptradd` index of one.
Intrinsics of these widths stay `x86:intrinsic`, as for every width: the
`*.with.overflow` ones return the aggregate `{iN, i1}` (I11's
`extractvalue`), and the bit counts and min/max are not lowered yet.

## 10. Inline assembly (`asm`, WP-301)

A call of inline assembly is `crate::asm::call` (`call::lower`'s asm
arm): the operands placed in the six general registers, memory or
immediates as the constraints ask, the template substituted and
assembled in 32-bit mode by `hod_asm::x86`, the outputs stored back.
The rules, the constraint letters, the register order and every
refusal (`x86:inline-asm:<detail>`, `asm:*`, `x86-mode:*`) are
`spec/asm.md` §22. Record: `spec/landed/asm/WP-301.md`.
