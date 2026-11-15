# Passing arguments and returning values on i386 Linux

Status: **skeleton** (WP-240, design 0008). No code implements it yet.
Every rule below was measured from LLVM; the probe that shows it is
named after the rule, as a file of `docs/design/0008-data/abi/` (`.ll`
probes through `run.sh`; `.c`/`.cpp` probes through clang 22
`--target=i686-linux-gnu -O0 -S -emit-llvm`). The package that
implements `hod_x86::abi::assign` (design 0008 §6, I6) completes each
section, adds the grid evidence of §11, and removes this paragraph.

`hod_x86::abi::assign` will answer the question `hod_x64::abi::assign`
answers (`spec/abi-x64.md`): given an IR-level signature, where does each
byte of each argument and of the return value travel? The interface is
`hod_cg::abi` (`spec/codegen.md` §7.2), unchanged. The layout of this
document follows `spec/abi-x64.md`, section for section.

## 1. Scope

### 1.1 Target and oracle

- The target is `i686-unknown-linux-gnu` (also spelled `i386`, `i486`,
  `i586`): little-endian, ILP32, the System V i386 psABI as LLVM
  implements it. The data layout is clang's:
  `e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128`
  (clang 19 and 22 agree; `a.c`).
- The **oracle** is `llc` 22, and 19 as a second witness, at `-O0`
  (FastISel) and `-O0 -fast-isel=0`, static and `-relocation-model=pic`.
  The four agree on every rule here except §4.2's alignment, where 22 is
  the rule. `cmp.sh` counts the differences.

### 1.2 Calling conventions

| convention | status | rule (probe `05-cc`) |
|---|---|---|
| C (`ccc`) | in scope | §3 |
| `fastcc` | in scope | first two 32-bit integer arguments in `ecx`, `edx`; the caller pops |
| `tailcc` | refused | `ecx`, `edx`; the callee pops a 16-aligned area (`retl $12` for one stack word) |
| `x86_stdcallcc` | refused | stack; the callee pops (`retl $8`) |
| `x86_fastcallcc` | refused | `inreg` arguments in `ecx`, `edx`; the callee pops |
| `x86_thiscallcc` | refused | first argument in `ecx`; the callee pops the rest |
| `x86_regcallcc`, `x86_vectorcallcc` | refused | `inreg` registers from `eax`, `ecx`, `edx`, `edi`, `esi` |
| `preserve_mostcc`, `preserve_allcc`, `coldcc` | refused | argument passing as C; save sets not measured |

### 1.3 Target features

The function's `target-cpu` decides the features (design 0008 §1.9):
`i686` (clang's default) has `cmov`, `cx8`, x87 and no SSE; `pentium4`
and later CPUs have SSE2 although `target-features` lists only `+sse`.
§4.3 and §5.1 depend on SSE.

## 2. Terms

As `spec/abi-x64.md` §2. A **slot** is a run of the outgoing argument
area starting at a multiple of 4.

## 3. Assigning arguments

### 3.1 State and order

Arguments are placed in order; the first stack argument is at `0(%esp)`
at the call, `4(%esp)` on entry. An `sret` pointer that is not `inreg` is
the first stack argument (`04-aggr` `crs`). `inreg` arguments take
`eax`, `edx`, `ecx` in order, an `i64 inreg` two of them (`05-cc` `inr`,
`inr64`); when they run out the argument goes to the stack. `nest` is in
`ecx` (`08-tail` `cn`).

### 3.2 Slots

| type | slot | probe |
|---|---|---|
| `i1`-`i32`, `ptr` | 4 bytes, value in the low bytes, no extension unless §7 says | `01-int` `call64` |
| `i64` | 8 bytes, alignment 4 | `01-int` `call64` |
| `float` / `double` / `x86_fp80` | 4 / 8 / 12 bytes, alignment 4 | `02-fp` `calld` |
| `half` | 4 bytes | `02-fp` `rh` |
| `i128`, `fp128` | 16 bytes, **alignment 16 (LLVM 22), 4 (LLVM 19)** | `03-i128` `c128`, `02-fp` `rq` |
| `byval(T)` | `T`'s bytes copied into the area; the offset is the next multiple of 4 (an `align 16` does not move it) | `04-aggr` `cbv`, `cbv16` |
| vector, with SSE | the first three 128-bit vectors in `xmm0`-`xmm2`, then 16-byte slots at alignment 16; `<2 x i32>` in `xmm0`; `<8 x float>` in `xmm0`, `xmm1` | `06-vec-sse2` |
| vector, without SSE | its elements in consecutive slots | `06-vec-nosse` |

### 3.3 The outgoing area

16-byte aligned at the call (`S128`); the caller reserves it with `subl`
in its frame and does not push (`01-int` `call64`, `13-frame`).

### 3.4 Examples

To be written with the grid (I6).

## 4. From a type to pieces

### 4.1 Scalars

Each scalar is one piece of its slot (§3.2).

### 4.2 Integers wider than 32 bits

`i64` is two 4-byte words, low first. `i128` is four, as one 16-byte
slot (§3.2). `iN` for other N > 32 rounds up to a multiple of 32 bits
(to be measured by the grid).

### 4.3 Vectors

§3.2; with SSE they are `Fpr` values; without SSE they are passed as
their elements and returned through a hidden pointer (§5.2).

### 4.4 Aggregates

At the IR level an aggregate argument is passed by `byval` or expanded
by the front end: clang passes `struct { int, int }` as `i32, i32`,
`struct { double }` as `double`, `struct { float }` as `float`, others
`byval` (`b.c`). A first-class aggregate argument in IR is passed as its
leaves (to be measured by the grid).

## 5. Return values

### 5.1 In registers

| type | register | probe |
|---|---|---|
| `i1`-`i32`, `ptr` | `al`/`ax`/`eax` | `01-int` |
| `i64` | `edx:eax` | `01-int` `r64` |
| `float`, `double`, `x86_fp80` | `st0`; a caller that drops it pops it (`fstp %st(0)`) | `02-fp` `rd`, `calldrop` |
| `half` | `ax` without SSE2, `xmm0` with it | `02-fp` `rh`, `06-vec-sse2` `fh` |
| vector, with SSE | `xmm0` (`xmm0`, `xmm1` for 256 bits) | `06-vec-sse2` |
| IR aggregate of up to three 32-bit integer parts | `eax`, `edx`, `ecx` | `04-aggr` `rpair`, `rtriple`, `[2 x i32]` `rarr` |
| IR aggregate of floating parts | `st0`, `st1` (`{float, i32}`: `eax` and `st0`) | `04-aggr` `rdd`, `rfi` |
| `_Complex float` (clang) | as `i64`, `edx:eax` | `b.c` `cf` |

### 5.2 In memory

`sret`, `fp128`, `i128`, an IR aggregate larger than §5.1 allows
(`{i32 x 5}`), and a vector without SSE are returned through a hidden
pointer passed as the first stack argument; **the callee pops it**
(`retl $4`) and returns the pointer in `eax`; the caller adjusts `esp`
back by 4 after the call (`04-aggr` `rs`, `crs`, `rquint`; `02-fp` `rq`;
`03-i128` `r128`; `06-vec-nosse` `v4f`). clang returns every C struct,
however small, through `sret` (`b.c`).

## 6. Parameter attributes

### 6.1 `sret`

§5.2. With `inreg`, the pointer is in `eax` and the callee does not pop
it (`05-cc` `inrp`).

### 6.2 `byval(T)`

§3.2.

### 6.3 `nest`

`ecx` (`08-tail`).

### 6.4 `byref`, `align`, `alignstack`

To be measured (I6).

### 6.5 `inreg`

§3.1.

### 6.6 Unsupported attributes

To be listed with the refusals (I6).

## 7. Bits above an integer

Measured so far: as `spec/abi-x64.md` §7 for a narrow value (`01-int`
`r8`, `r16s`: no extension of a `signext` `i16` return). The grid of I6
pins the table for i386.

## 8. What the code generator does with an `Assignment`

### 8.1 A call

The outgoing area is part of the caller's frame; arguments are stored to
it; an `sret` call is followed by `subl $4, %esp` (or the frame accounts
for the callee's pop); a dropped x87 result is popped.

### 8.2 A function's entry

Stack arguments are read at positive offsets from `ebp` (`8(%ebp)` for
the first with the `ebp` frame).

### 8.3 A variadic function's entry

`va_start` stores the address of the first variadic slot (§9).

### 8.4 Registers

Callee-saved: `ebx`, `esi`, `edi`, `ebp` (`05-cc` `clobber`).
Caller-saved: `eax`, `ecx`, `edx`, all x87 and `xmm` registers. The x87
stack is empty at a call and at a return except for `st0` holding a
return value.

### 8.5 `musttail`

A `jmp` with the arguments rewritten in the caller's incoming area
(`08-tail` `must`, `mustfwd`).

### 8.6 DWARF register numbers

`eax` 0, `ecx` 1, `edx` 2, `ebx` 3, `esp` 4, `ebp` 5, `esi` 6, `edi` 7,
`eip` 8, `st0`-`st7` 11-18, `xmm0`-`xmm7` 21-28 (`14-eh`, `cfi.s`).

## 9. Variadic functions

### 9.1 Assignment

Variadic arguments are placed as fixed ones (§3); no register count.

### 9.2 `va_list`

One pointer (`07-varargs` `va`).

### 9.3 `va_arg`

The pointer rounds up to 4 and steps by the slot size; a `double` is at
alignment 4 (`07-varargs`).

## 10. Unsupported constructs and limits

The conventions refused in §1.2; `i128`/`fp128` arguments until the grid
covers §3.2's 19/22 difference; vectors of an SSE function until the SSE2
path (design 0008 I12).

## 11. Validation

### 11.1 The grid

To be built by I6 on the model of `crates/hod_x64/tests/abi`: generated
caller/callee pairs, one side compiled by hod, the other by llc 19 and
22, linked and run under `qemu-i386`, both directions.

### 11.2 Cost

To be measured.

## 12. Where LLVM and the psABI part, and where LLVM differs from itself

- `i128` and `fp128` stack arguments: 16-byte alignment in LLVM 22, 4 in
  LLVM 19 (`03-i128.dag19.s` against `.dag22.s`).
