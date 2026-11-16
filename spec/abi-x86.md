# Passing arguments and returning values on i386 Linux

Status: **normative** (WP-297, design 0008's I6). `hod_x86::abi::assign`
implements §3 to §6 and §9.1; `hod_x86::frame` implements §8. Every rule
was measured from LLVM; the probe that shows it is named after the rule,
as a file of `docs/design/0008-data/abi/` (`.ll` probes through
`run.sh`; `.c`/`.cpp` probes through clang 22 `--target=i686-linux-gnu
-O0 -S -emit-llvm`), or is a row of the grid (§11). What I7, I9, I11 and
I12 add for their types extends the tables here; a construct this
document refuses is refused under the name §10 gives it.

`hod_x86::abi::assign` answers the question `hod_x64::abi::assign`
answers (`spec/abi-x64.md`): given an IR-level signature, where does each
byte of each argument and of the return value travel? The interface is
`hod_cg::abi` (`spec/codegen.md` §7.2) with one field added for this
target, `Assignment::callee_pop` (§5.2). The layout of this document
follows `spec/abi-x64.md`, section for section.

## 1. Scope

### 1.1 Target and oracle

- The target is `i686-unknown-linux-gnu` (also spelled `i386`, `i486`,
  `i586`): little-endian, ILP32, the System V i386 psABI as LLVM
  implements it. The data layout is clang's:
  `e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128`
  (clang 19 and 22 agree; `a.c`).
- The **oracle** is `llc` 22, and 19 as a second witness, at `-O0
  -global-isel=0 -fast-isel=0` (and FastISel), static and
  `-relocation-model=pic`. The four agree on every rule here except
  §4.2's alignment, where they differ and hod refuses (§10). Behaviour is
  compared by running both sides under `qemu-i386` (§11).

### 1.2 Calling conventions

| convention | status | rule (probe `05-cc`, grid) |
|---|---|---|
| C (`ccc`) | compiled | §3 |
| `coldcc` | compiled, as C | `llc` 19 and 22 pass and return exactly as C on i386 |
| `fastcc` | compiled without floating point | each 32-bit integer word (a leaf of an aggregate included) in `ecx`, then `edx`, then the stack; an `i64` may split `edx` + stack; `nest` in `eax`; the caller pops; floating point (SSE2 `xmm0`-`xmm2`), `sret` and results in memory refused `x86:abi:cc` |
| `tailcc` | refused `x86:abi:cc` | `ecx`, `edx`; the callee pops a 16-aligned area (`retl $12` for one stack word) |
| `x86_stdcallcc` | refused (`cc:x86_stdcallcc` in the classifier's i386 column) | stack; the callee pops (`retl $8`) |
| `x86_fastcallcc` | refused (`cc:x86_fastcallcc`) | `inreg` arguments in `ecx`, `edx`; the callee pops |
| `x86_thiscallcc` | refused (`cc:x86_thiscallcc`) | first argument in `ecx`; the callee pops the rest |
| `x86_regcallcc`, `x86_vectorcallcc` | refused | `inreg` registers from `eax`, `ecx`, `edx`, `edi`, `esi` |
| `preserve_mostcc`, `preserve_allcc`, `x86_64_sysvcc`, `win64cc` | refused (classifier, i386 column) | not measured on i386 |
| any other | refused `x86:abi:cc` | — |

### 1.3 Target features

The function's `target-cpu` decides the features (design 0008 §1.9):
`generic` (no attribute) and `i686` have no SSE; `pentium4` and later
CPUs have SSE2 although `target-features` lists only `+sse`. Nothing
`assign` answers today depends on them: the rows that do (`fastcc`
floating point, vectors, §4.3) are refused until I12.

## 2. Terms

As `spec/abi-x64.md` §2. A **slot** is a run of the outgoing argument
area starting at a multiple of 4. A **leaf** is a scalar of an argument
or result after aggregates are flattened in field order (§4.4).

## 3. Assigning arguments

### 3.1 State and order

Arguments are placed in order; the first stack argument is at `0(%esp)`
at the call, `8(%ebp)` in the callee. A hidden result pointer (§5.2) or
an `sret` pointer that is not `inreg` is the first stack argument (`04-aggr`
`crs`). `inreg` integer words take `eax`, `edx`, `ecx` in order, one word
at a time: an `i64 inreg` takes two registers, or the last free one and
a stack slot for its high word; once the registers run out, every later
`inreg` argument goes to the stack (`05-cc` `inr`, `inr64`; grid). `nest`
is in `ecx` (`eax` under `fastcc`) and takes that register from the
`inreg` sequence (`08-tail` `cn`; grid). A parameter that is neither goes
to the stack.

### 3.2 Slots

| type | slot | probe |
|---|---|---|
| `i1`-`i32`, `ptr` | 4 bytes, value in the low bytes, no extension unless §7 says | `01-int` `call64` |
| `i64` | 8 bytes, alignment 4 (one part: the incoming slot is the value's home) | `01-int` `call64` |
| `float` / `double` / `x86_fp80` | 4 / 8 / 12 bytes, alignment 4 (`x86_fp80` is a 10-byte part) | `02-fp` `calld` |
| `half` | 4 bytes (refused `x86:float` until I9) | `02-fp` `rh` |
| `i128`, `fp128` | 16 bytes, alignment 16 (LLVM 22), 4 (LLVM 19): refused `x86:abi:wide-arg` | `03-i128` `c128`, `02-fp` `rq` |
| `byval(T)` | `T`'s bytes copied into the area at the parameter's `align` (at least 4, at most 16), its size rounded up to 4 | `04-aggr` `cbv`, `cbv16`; grid |
| vector | refused `x86:vector` until I12 (with SSE: `xmm0`-`xmm2`, then 16-byte slots at alignment 16; without SSE: its elements in consecutive slots) | `06-vec-sse2`, `06-vec-nosse` |

**Correction (WP-297).** The skeleton of this section said that an
`align 16` `byval` stays at the next multiple of 4. The grid shows the
opposite: `llc` 19 and 22 both move the copy to the next multiple of 16
(`i32, byval(<20 bytes>) align 16, i32`: the copy at `16(%esp)`, the
last `i32` at `36(%esp)`), and so does hod.

### 3.3 The outgoing area

16-byte aligned at the call (`S128`); the caller reserves it with `subl`
(hod: `lea esp, [esp - n]`) and does not push (`01-int` `call64`,
`13-frame`). `Assignment::stack_size` is the area's size before rounding.

### 3.4 Examples

From the grid's fixed rows (§11.1); `S` is the stack offset at the call.

| signature | arguments |
|---|---|
| `i32 (i32, byval([5 x i32]) align 16, i32)` | `S0`; copy at `S16`..`S36`; `S36` |
| `i64 (i32 inreg, i32 inreg, i64 inreg, i32 inreg)` | `eax`; `edx`; low word `ecx`, high word `S0`; `S4` |
| `fastcc i64 (i32, i64, i8)` | `ecx`; low word `edx`, high word `S0`; `S4` |
| `i32 (ptr nest, i32, i32, i64)` | `ecx`; `S0`; `S4`; `S8`..`S16` |
| `fastcc i32 (ptr nest, i32, i32, i64)` | `eax`; `ecx`; `edx`; `S0`..`S8` |
| `void (ptr sret(%S), i32 inreg, x86_fp80)` | `S0` (popped by the callee); `eax`; `S4`..`S14` |
| `void (ptr sret(%S) inreg, i32 inreg, i64)` | `eax` (not popped); `edx`; `S0`..`S8` |
| `i32 (i32, ...)` called with `i32, double, i64, i32` | `S0`; `S4`; `S8`; `S16`; `S24` |

## 4. From a type to pieces

### 4.1 Scalars

Each scalar is one leaf in its own slot (§3.2).

### 4.2 Integers wider than 32 bits

`i64` is two 4-byte words, low first: in registers (§3.1) each word is
placed on its own; on the stack the two are one 8-byte part. `i128` is
refused as an argument (`x86:abi:wide-arg`, the 19/22 difference) and
returned in memory (§5.2). Other `iN` with N > 32 are refused under the
type's own name (`x86:i64`).

### 4.3 Vectors

Refused (`x86:vector`) until I12, which writes this section from
`06-vec-sse2` and `06-vec-nosse`.

### 4.4 Aggregates

At the IR level an aggregate argument is passed by `byval` or expanded
by the front end: clang passes `struct { int, int }` as `i32, i32`,
`struct { double }` as `double`, `struct { float }` as `float`, others
`byval` (`b.c`). A first-class aggregate argument in IR is passed as its
leaves, in field order, each leaf in a slot of its own as if it were a
separate argument (a `{ i8, i32 }` takes two 4-byte slots); under
`fastcc` each integer leaf takes the next of `ecx`, `edx` (grid). At most
16 leaves; more is refused `x86:aggregate`.

## 5. Return values

### 5.1 In registers

| type | register | probe |
|---|---|---|
| `i1`-`i32`, `ptr` | `al`/`ax`/`eax` | `01-int` |
| `i64` | `edx:eax` | `01-int` `r64` |
| `float`, `double`, `x86_fp80` | `st0`; the caller stores it to its slot with `fstp`, which pops it, also when the result is unused | `02-fp` `rd`, `calldrop`; grid (ten dropped results) |
| `half` | `ax` without SSE2, `xmm0` with it (refused `x86:float` until I9) | `02-fp` `rh`, `06-vec-sse2` `fh` |
| IR aggregate of up to three 32-bit integer words and at most one floating-point leaf | the words in `eax`, `edx`, `ecx` in field order, the floating leaf in `st0` | `04-aggr` `rpair`, `rtriple`, `rarr`, `rfi` |
| IR aggregate of two floating-point leaves | `st0`, `st1`: refused `x86:abi:x87-pair` (the call sequence moves result parts in one order, the x87 stack needs the opposite one on the other side) | `04-aggr` `rdd` |
| `_Complex float` (clang) | as `i64`, `edx:eax` | `b.c` `cf` |

`st0` is `hod_x86::abi::ST0`, a register of no allocatable class: the
target's glue moves it from and to the frame slot of the result with
`fld`/`fstp` (I9 keeps floating values in slots of their size). With
I9's 80-bit rule (`spec/codegen-x86.md` §11): `st0` carries 80 bits
both ways. A `float`/`double` result the caller uses in its run is also
stored at 80 bits (`fld st0; fstp tbyte`) before the `fstp` to its slot,
and a function returns a computed result of its last run from its
80-bit copy (`fld tbyte`), as `llc` does.

### 5.2 In memory

`fp128`, `i128`, an IR aggregate with more than three integer words, more
than two floating-point leaves or more than 16 leaves (`{i32 x 4}`,
`{i64, i64}`) and a vector without SSE are returned through a hidden
pointer passed as the first stack argument; **the callee pops it**
(`retl $4`) and returns the pointer in `eax` (`04-aggr` `rs`, `crs`,
`rquint`; `02-fp` `rq`; `03-i128` `r128`; `06-vec-nosse` `v4f`). clang
returns every C struct, however small, through `sret` (`b.c`), which
pops the same way (§6.1).

`Assignment::callee_pop` is 4 for such a signature and for a stack
`sret` pointer, 0 otherwise; the call sequence passes it to the call
instruction (`CallEmit::callee_pop`) and the caller moves `esp` back by
it after the call (§8.1). A **definition** that returns in memory is
refused `x86:aggregate` until I11; `fastcc` with a result in memory is
refused `x86:abi:cc`.

## 6. Parameter attributes

### 6.1 `sret`

On the first parameter only, a pointer: the first stack slot, popped by
the callee (`retl $4`), returned in `eax` (the callee reloads it from its
incoming slot before the return). With `inreg`, the pointer is in `eax`,
the callee does not pop it and keeps it in a frame slot from its entry
to return it (`05-cc` `inrp`). Elsewhere, or under `fastcc`: refused.

### 6.2 `byval(T)`

§3.2. The callee's parameter is the address of the copy in its incoming
area.

### 6.3 `nest`

§3.1. A second `nest`, or `nest` when its register is taken, is refused
`x86:abi:attr`.

### 6.4 `byref`, `align`, `alignstack`

`byref(T)` is a plain pointer argument (grid: a hod forwarder between
two `llc` functions). `align` on a pointer argument changes nothing at
the boundary; on `byval` it is §3.2's alignment. `alignstack(n)` is
refused `x86:abi:attr`.

### 6.5 `inreg`

§3.1. On a floating-point or aggregate argument, or in a variadic
signature, refused `x86:abi:attr`; on the result, refused
`x86:abi:attr`. Ignored under `fastcc` (every integer word is a register
candidate there).

### 6.6 Unsupported attributes

Refused `x86:abi:attr`: `swiftself`, `swifterror`, `swiftasync`,
`inalloca`, `preallocated`, `alignstack`; `sret` not on the first
parameter or not on a pointer; `byval` above alignment 16, with `inreg`
or without its type; the cases of §6.3 and §6.5; a result attribute
other than the extensions, `noundef` and the like (`inreg`, `nest`, a
memory attribute).

## 7. Bits above an integer

Measured so far: as `spec/abi-x64.md` §7 for a narrow value (`01-int`
`r8`, `r16s`: no extension of a `signext` `i16` return). A `signext` or
`zeroext` `i8`/`i16` argument is extended by the caller to 32 bits (the
grid checks it from the `llc` callee, which receives it as `i32`).

## 8. What the code generator does with an `Assignment`

### 8.1 A call

The outgoing area is reserved below the caller's frame and freed after
the call; arguments are stored to it, then register arguments are
placed; after the call `esp` moves back by `callee_pop` (`lea esp, [esp -
4]`), so that the area is freed whole. A dropped x87 result is popped
(§5.1). An indirect call puts the callee's address in `edx`
(`RegInfo::callee`) after the arguments: a signature with an argument in
`edx` (two `inreg` words, `fastcc` with two words) is refused
`x86:abi:indirect-edx` by `abi::check_call` before the call sequence
runs. `llc` picks a free register instead (`eax` when it is free, else a
callee-saved one); hod's call sequence has one fixed callee register per
target, so taking another would be a `hod_cg` change for a rare shape
(`-mregparm` code).

### 8.2 A function's entry

Stack arguments are read at positive offsets from `ebp` (`8(%ebp)` for
the first with the `ebp` frame). An `inreg` `sret` pointer is stored to
a frame slot in the prologue, before any argument register is read.

### 8.3 A variadic function's entry

`va_start` stores the address of the first variadic slot (§9): I11.

### 8.4 Registers

Callee-saved: `ebx`, `esi`, `edi`, `ebp` (`05-cc` `clobber`).
Caller-saved: `eax`, `ecx`, `edx`, all x87 and `xmm` registers. The x87
stack is empty at a call and at a return except for `st0` holding a
return value.

### 8.5 `musttail`

As `llc` at `-O0` (`08-tail` `must`, `mustfwd`; WP-297 probe): the stack
arguments are written over the caller's incoming ones (the call sequence
stages them in the local area first and copies them in place), register
arguments are placed as for a call, then the exit tears the frame down
(`lea esp, [ebp - 12]`, `pop edi`, `pop esi`, `pop ebx`, `pop ebp`) and
jumps: `jmp callee` with `R_386_PLT32` (`R_386_PC32` for a callee in this
output), or `jmp edx`. Under PIC (with I8's `pic`), `ebx` is the
caller's again at the jump, so a callee reached through the PLT is not
jumped to there: as `llc` (19 and 22, measured), the GOT pointer is
computed into the first of `eax`, `ecx`, `edx` that holds no argument
(`call; pop; lea` with `R_386_GOTPC`), the callee's address loaded from
its GOT entry (`R_386_GOT32X`) and jumped to (`jmp reg`); with all three
holding arguments it is refused `x86:abi:musttail` (`llc` fails, "ran
out of registers"). A `dso_local` callee stays `jmp` with
`R_386_PC32`. The callee has the caller's signature, so it pops
what the caller would have popped (`sret` included). Refused: a result
in memory (`call:musttail-ret-memory`, the call sequence), and stack
arguments together with an argument in `ecx`, which the copy into the
incoming area uses (`x86:abi:musttail`). The classifier's
`musttail:stack-args-unknown` rule uses `assign` through
`hod_cg::abi::TargetStackArgs` (`crates/hod/src/session.rs`).

### 8.6 DWARF register numbers

`eax` 0, `ecx` 1, `edx` 2, `ebx` 3, `esp` 4, `ebp` 5, `esi` 6, `edi` 7,
`eip` 8, `st0`-`st7` 11-18, `xmm0`-`xmm7` 21-28 (`14-eh`, `cfi.s`).

## 9. Variadic functions

### 9.1 Assignment

Every argument of a variadic signature, fixed or variable, is placed on
the stack as §3 says; no register count, no `inreg` (refused
`x86:abi:attr`), `fastcc` variadic refused `x86:abi:cc`.
`VaInfo::stack_offset` is the offset of the first variable argument.

### 9.2 `va_list`

One pointer (`07-varargs` `va`): I11.

### 9.3 `va_arg`

The pointer rounds up to 4 and steps by the slot size; a `double` is at
alignment 4 (`07-varargs`): I11.

## 10. Unsupported constructs and limits

| name | what |
|---|---|
| `x86:abi:cc` | §1.2 |
| `x86:abi:attr` | §6.6 |
| `x86:abi:wide-arg` | an `i128` or `fp128` argument (§3.2, §12) |
| `x86:abi:x87-pair` | a result with two floating-point leaves (§5.1) |
| `x86:abi:indirect-edx` | §8.1 |
| `x86:abi:musttail` | §8.5 |
| `x86:aggregate` | more than 16 leaves; a definition returning in memory (I11) |
| `x86:vector`, `x86:float`, `x86:i64` | a type the back end does not pass yet (I12, I9, I7) |

## 11. Validation

### 11.1 The grid

`crates/hod/tests/i386_abi.rs`. For each signature `k` the hod module
defines `hod_k`, which forwards its arguments to an `llc` function
`chk_k` and returns its result; the `llc` module's `main` calls every
`hod_k` with known constants and checks the result, and `chk_k` checks
every argument and the stack alignment at its entry. Both directions
cross the boundary for every signature. 103 signatures: 96 drawn by a
fixed generator over the conventions (C, `fastcc`, `coldcc`), the types
(`i8`, `i16` with and without extensions, `i32`, `ptr`, `i64`, `float`,
`double`, `x86_fp80`, IR aggregates, `byval`), `inreg` and the result
kinds (void, scalars, pairs, mixed, in memory, `sret` plain and
`inreg`), and 7 fixed ones (§3.4). A second program, the fixed rows,
covers ten dropped `double` results and a dropped `x86_fp80`, a variadic
call with `i32`, `double` and `i64` arguments, two results in memory
dropped, `byref`, an indirect `inreg` call, and `musttail` a million
deep (direct with stack arguments, `sret`, indirect, `inreg`, `fastcc`):
a non-tail call there overflows `qemu-i386`'s stack. The `llc` side is
built by `llc` 19 and 22, the hod side by the facade and under
`hod_cg::Stress::ALL`; the grid links with GNU ld (4 runs), the fixed
rows with GNU ld, lld and mold (12 runs). Every run prints `ok`.

### 11.2 Cost

`assign` allocates nothing beyond the reused `Assignment`; the grid's
hod module (103 functions) compiles in a few milliseconds.

## 12. Where LLVM and the psABI part, and where LLVM differs from itself

- `i128` and `fp128` stack arguments: 16-byte alignment in LLVM 22, 4 in
  LLVM 19 (`03-i128.dag19.s` against `.dag22.s`): refused
  (`x86:abi:wide-arg`).
- An `align 16` `byval` copy: both move it to a 16-aligned offset (§3.2),
  which the psABI's 4-byte slots do not require.
