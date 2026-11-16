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
| `x86:i64` | an integer wider than 32 bits anywhere (value, argument, result, index, `switch`) | I7 (`int.rs`) |
| `x86:float` | a floating-point argument or result of a call or a function (§8) | I6 (`abi.rs`) |
| `x86:float:half`, `x86:float:bfloat`, `x86:float:fp128` | a value of those types | — |
| `x86:float:sse` | a `float` operation with SSE, a `double` one with SSE2 (`llc` uses `xmm`) | I12 |
| `x86:float:i64-cast` | a conversion between a floating type and an integer wider than 32 bits | I7 with I9 |
| `x86:float:intrinsic` | a floating-point intrinsic of §8 not lowered (`double` `minnum`/`maxnum`, any other) | — |
| `x86:float:cast` | a `bitcast` of a floating value, another cast not in §8 | — |
| `x86:vector` | a vector value, a vector `ptradd`, the element operations | I12 |
| `x86:aggregate` | an aggregate value, `extractvalue`, `insertvalue`, a result in memory | I11 |
| `x86:pic` | a relocation model other than static | I8 (`pic.rs`) |
| `x86:tls` | a thread-local access, `llvm.threadlocal.address`, `llvm.thread.pointer` | I8 (`tls.rs`) |
| `x86:eh` | `invoke`, `landingpad`, `resume`; a call that may unwind in a function that may unwind (no unwind tables) | I10 |
| `x86:debug-info` | a module with debug information (`hod_cg`'s emitter) | I10 |
| `x86:cf-protection` | `cf-protection-*` module flags (`hod_cg`'s `.note.gnu.property`) | — |
| `x86:atomic` | an ordered load or store, `fence`, `cmpxchg`, `atomicrmw` | I11 |
| `x86:inline-asm` | a call of inline assembly | I13 |
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

## 8. Floating point on the x87 (`x87`, WP-300, design 0008 I9)

**Types and homes.** `float`, `double` and `x86_fp80` are class `Mem`:
a home in the frame of 4, 8 or 12 bytes (`x86_fp80`'s image is the low
10). A function whose CPU has SSE (`float`) or SSE2 (`double`) is
refused `x86:float:sse` (`llc` computes in `xmm`); the CPU is the
function's `target-cpu`/`target-features` (`generic` without them).
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
constants (`llc` branches: the block splits). `llc` branches without
`cmov` also for integer `smin`/`smax`/`umin`/`umax`, `ctlz`/`cttz` and
the saturating additions and subtractions (measured); `hod_x86` refuses
those intrinsics today, and the package that lowers them adds them to
the barriers (`x87::barrier`). `freeze`, `fpext`,
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
`x86_fp80` has no copy (its home is 80 bits).

**The divergence.** With six or more products live at once in one
block without a barrier, `llc` rounds some uses (which ones follows its
fast allocator: the first product, and the first and last ones in
FIFO order or the middle ones in LIFO order), so its result is not a
function of the IR; hod keeps all of them at 80 bits. The shapes
`many6_lifo`, `many7_*`, `many8_*`, `many9_*`, `many12_*` compare with a
model of hod's rule (the same arithmetic at `x86_fp80`, rounded at the
store) instead of `llc`; `many6_fifo` equals `llc`. A folded result is
computed by hod at run time and rounded from 80 bits to its type, where
`llc`'s DAG rounds once: equal for `float` (64 bits of significand are
at least 2 x 24 + 2), possibly one unit in the last place apart for a
folded `double` `fadd`, `fsub`, `fmul`, `fdiv`, `fma` (not measured
apart; no shape has such operands).

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
bits (64 for an unsigned `i32`), `fldcw` of the saved word. An integer
wider than 32 bits is `x86:float:i64-cast`.

**libm.** `frem` is `fmodf`/`fmod`/`fmodl`; `sin`, `cos`, `tan`, `asin`,
`acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `exp`, `exp2`, `exp10`,
`log`, `log2`, `log10`, `pow`, `floor`, `ceil`, `trunc`, `round`,
`roundeven`, `nearbyint`, `rint`, `fma` call `llc`'s symbols (`sinf`,
`sin`, `sinl`, ...); `minnum`/`maxnum` call `fminf`/`fmaxf` and
`fminl`/`fmaxl` (`double` is refused `x86:float:intrinsic`: `hod_cg` has
no `fmin`/`fmax` routine yet). Arguments are copied to the outgoing area
at their width (12 bytes for `x86_fp80`), the result is `st0`.
