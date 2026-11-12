# Passing arguments and returning values on AArch64 Linux

`hod_a64::abi::assign` answers one question: given an IR-level signature,
where does each byte of each argument and of the return value travel?
The answer is the one LLVM gives for the same signature, so that code
compiled by hod and code compiled by LLVM can call each other in both
directions. This document states the rules completely enough to write
`assign` again from it, and states what a code generator still has to do
with the answer.

The interface (`Sig`, `Part`, `Place`, `Assignment`, `VaInfo`,
`AbiUnsupported`) is `hod_cg::abi`, described in `spec/codegen.md` §7.2.
The types and attributes are `hod_ir`'s (`spec/ir.md`).

## 1. Scope

### 1.1 Target and oracle

- The target is `aarch64-unknown-linux-gnu`: little-endian, LP64, with
  FP and Advanced SIMD registers, without SVE or SME. Sizes, offsets and
  alignments of types come from the module's data layout through
  `hod_ir::TypeTable` (`alloc_size_of`, `align_of`, `pref_align_of`,
  `field_offsets`); nothing here assumes a particular layout string.
- The **oracle** is `llc`, version 22, and version 19 as a second
  witness. Each is run at `-O0`, where it selects instructions with
  GlobalISel, and at `-O2`, where it uses SelectionDAG. These four
  configurations are what "LLVM" means below.
- A rule is stated only where all four configurations agree, with two
  exceptions that follow llc 22 and are marked (§6.3, §3.3's
  `abi:bfloat-array`). Where the two instruction selectors of one LLVM
  version disagree with each other, there is no single LLVM behavior to
  match, and the construct is **unsupported** under a stable name (§10).
- AAPCS64 is the *Procedure Call Standard for the Arm 64-bit
  Architecture*; rules are cited as "AAPCS64 C.9" by their labels in its
  section "Parameter passing rules" (the numbering with rules C.1 to
  C.17, release 2023Q3), and other parts by their section titles.
  AAPCS64 speaks about C types; this document speaks about IR types. A
  frontend has already turned the first into the second (`sret`, coerced
  aggregates, pointers to copies), so some of what AAPCS64 says is the
  frontend's business and is not repeated here. §12 lists where LLVM's
  treatment of IR types differs from what AAPCS64 says of the C types of
  the same name.
- Every rule names its evidence: the AAPCS64 rule where there is one,
  and a test. The tests are in `crates/hod_a64/tests/abi/` and are named
  `module::test`; "the grid" is `grid::systematic_grid` and
  `grid::random_grid` together (§11). `table::*` are assignments written
  out by hand, which double as this document's examples.
- Not covered: the registers a call preserves and the frame
  (`spec/codegen.md` §5.5), unwinding, and the entry sequences of branch
  protection (`spec/codegen.md` §5.5 refers here for them; they are not
  specified yet, and `classify` rejects the module property).

### 1.2 Calling conventions

| convention | `assign` |
|---|---|
| `ccc` (0) | specified by this document |
| `fastcc` (8), `swiftcc` (16), `cxx_fast_tlscc` (17) | exactly as `ccc` |
| every other number | `abi:cc` |

`hod_a64::abi::cc_is_c` is the predicate for the first two rows. "Exactly
as `ccc`" covers the places, the extension duties and the registers a
call preserves: LLVM compiles a module that defines and calls functions
of each of the three conventions (arguments of every register class and
on the stack, values live across calls, returns in registers and in
memory, `sret`, `byval`) to assembly text identical to the one it emits
with `ccc`, in all four configurations.

A variadic `fastcc` signature is `abi:invalid-signature`: LLVM's verifier
refuses it. `coldcc` is `abi:cc`: LLVM itself reports "unsupported
calling convention" for it on this target.

*Evidence:* `cc::conventions_equal_to_c` (which also shows the comparison
can fail: `preserve_mostcc` differs); `table::conventions` (every number
0..=1023); the grid's `conventions()` cases, which include variadic
`swiftcc` and `cxx_fast_tlscc` calls.

## 2. Terms

**Registers.** A `PReg` index names a register as `hod_a64::reg` does:
`x0`..`x30` are 0..=30, `sp` is 31, `v0`..`v31` are 32..=63. A value in a
vector register occupies its low bytes (`h`, `s`, `d`, `q` are views of
the same `v`). *Test:* `reg::tests::numbering`.

**Part.** `Part { arg, kind, offset, size, place }` says: bytes
`offset..offset + size` of argument number `arg` (or of the return value,
for which `arg` is 0) travel at `place`.

- `Place::Reg(r)`: byte `offset + k` of the value is bits `8k..8k + 8` of
  `r`. The bits of the register above the part are unspecified, except
  as §7 says.
- `Place::Stack(o)`: byte `offset + k` of the value is at address
  `S + o + k`, where `S` is the value of `sp` at the call instruction
  (so, for the callee, at entry). The **stack-argument area** is
  `S..S + stack_size`.
- `PartKind::Value`: the bytes are those of the argument's value, laid
  out as the value's in-memory image (`spec/ir.md` §4.3).
  `PartKind::Pointee`: the bytes are those of the memory a `byval`
  argument points to (§6.2).

`Assignment.args` lists parts in argument order and, within an argument,
in `offset` order. `Assignment.ret` does the same for the return value.

**Output.** `assign(types, sig, out)` clears `out` first. On success it
has filled it; on an error `out` is left equal to
`Assignment::default()`. The vectors of `out` are reused, so a call with
a warm `out` allocates nothing. `assign` never panics on any signature
built from the `types` it is given. *Tests:* `table::output_is_reused`,
`errors::any_signature_is_answered`.

On this target `Assignment::sret_returned_in` is always `None`,
`RetMemory::returned_in` is always `None` and `vector_regs_used` is
always 0.

## 3. Assigning arguments

### 3.1 State and order

The state is AAPCS64's (stage A): **NGRN**, the next general register
number, 0..=8; **NSRN**, the next vector register number, 0..=8;
**NSAA**, the next stacked argument address, as a byte offset from `S`.
All start at 0. Two more registers are each used at most once: `x8`
(§5.2, §6.1) and `x15` (§6.3).

1. The return value is assigned first (§5). If it travels in memory, the
   hidden address takes `x8`.
2. The arguments are assigned in order, each completely before the
   next. An argument's attributes select a rule of §6; without one, the
   argument is passed **by value**: its type is broken into leaves and
   the leaves into pieces (§4), and each piece is placed by §3.2 or, for
   a block, §3.3.
3. `stack_size` is NSAA after the last argument, not rounded.

Nothing is ever back-filled: a register skipped by any rule stays
unused, and once a piece of a register class has gone to the stack no
later piece gets a register of that class (AAPCS64 C.13 for the general
registers; for the vector registers it follows from NSRN only growing).

*Evidence:* AAPCS64 stages A and C; `table::scalars`; the grid.

### 3.2 A piece

A piece is up to 16 bytes of a value that travel in one register or one
stack slot. It has a **class**: general (`W` for a value of at most 32
bits, `X` otherwise) or vector (`H`, `Bf`, `S`, `D`, `Q` for the
register view; `Bf` is the `H` view holding a `bfloat`). A piece outside
a block is placed as follows.

- **General.** If NGRN < 8, the piece goes to `x[NGRN]` and NGRN
  increases by 1 (AAPCS64 C.9). Otherwise it goes to the stack: NSAA is
  rounded up to 8, the piece is at NSAA, and NSAA increases by 8
  (AAPCS64 C.14, C.16, C.17).
- **General, the first piece of an integer wider than 64 bits.** NGRN is
  first rounded up to an even number (AAPCS64 C.10). If it is then below
  8, the piece goes to `x[NGRN]` and NGRN increases by 1. Otherwise NGRN
  becomes 8 (so `x7`, if it was free, stays unused), NSAA is rounded up
  to 16, the piece is at NSAA, and NSAA increases by 8. The following
  pieces of the integer are ordinary general pieces, so an integer wider
  than 128 bits can lie partly in registers and partly on the stack.
- **Vector.** If NSRN < 8, the piece goes to `v[NSRN]` and NSRN
  increases by 1 (AAPCS64 C.1). Otherwise it goes to the stack: for
  class `Q`, NSAA is rounded up to 16 and increases by 16; for the other
  classes it is rounded up to 8 and increases by 8 (AAPCS64 C.4 to C.6).

A stack slot is thus 8 bytes, 16 for a `Q` piece. The piece's bytes are
at the slot's lowest addresses; the rest of the slot is unspecified,
except as §7 says.

*Evidence:* `table::scalars`, `table::wide_integers`; the grid's
`pressures()` put every type behind 0, 1, 6, 7 and 8 used registers of
each class and at odd stack offsets.

### 3.3 A block

An argument is a **block** when all of the following hold:

- its type is an array type (at the top level: a structure that contains
  an array is not a block);
- it has at least one leaf, and all its leaves, however nested, have the
  same value type, where a `ptr` counts as an `i64` (two vector types
  are the same when element type and length are);
- its leaf's class is not `W`.

A block's pieces are placed together (AAPCS64 C.2, C.3 and C.12, C.13
generalized). Let `n` be the number of leaves and `N` the register
number of the leaf's class (NGRN or NSRN):

- If `N + n <= 8`, the leaves go to registers `N..N + n` in order, and
  `N` increases by `n`.
- Otherwise `N` becomes 8 (the remaining registers of that class stay
  unused) and all leaves go to the stack, **packed**: NSAA is rounded up
  to `A` for the first leaf only, and each leaf takes exactly its packed
  size (2 bytes for `H`, 4 for `S`, 8 for `X` and `D`, 16 for `Q`).
  `A` is the argument's `alignstack` if it has one and the ABI alignment
  of the array type otherwise, raised to 8 if smaller and lowered to 16
  if larger. NSAA is not rounded afterwards: a block of three `float`s
  leaves it 12 bytes on.

The even-register rule does not apply inside a block, and `alignstack`
has no other effect on a block (in particular none when it fits the
registers).

An array whose leaves are of class `W` (`[3 x i8]`, `[2 x i32]`) is not
a block: each leaf is an ordinary piece (§3.2), with a register or an
8-byte slot of its own. So is every structure.

Four kinds of array are **unsupported**, because LLVM does not have one
answer for them:

- `abi:wide-int-array`: the leaf is an integer wider than 64 bits.
  (After one `i64`, GlobalISel passes `[1 x i128]` in `x2`, `x3` and
  SelectionDAG in `x1`, `x2`.)
- `abi:bfloat-array`: the leaf's class is `Bf`. (llc 22 treats it as a
  block, llc 19 does not.)
- `abi:vector-array`: the leaf is a vector that takes more than one
  piece (§4.3). (The selectors disagree on how many registers the block
  needs.)
- `abi:one-leaf-array`: `n` is 1 and `A` differs from the slot size the
  leaf has as an ordinary piece (8, or 16 for `Q`). (One selector passes
  a block, the other an ordinary value; they differ in the stack
  alignment. `[1 x double]`, `[1 x fp128]` and `[1 x float]
  alignstack(8)` are supported; `[1 x float] alignstack(16)` is not.)

These names concern arguments only. As a return value (§5) and as a
`byval` pointee (§6.2) every such array is supported.

*Evidence:* `table::aggregates`, `table::vectors`; the grid's
`aggregates()` under `pressures()`, and `attributes()` for `alignstack`
1, 4, 8, 16 and 32 on arrays, scalars, vectors and structures;
`errors::every_name`.

### 3.4 From pieces to parts

Each piece gives one `Value` part: `offset` is the offset in the
argument's value of the piece's first byte, `size` the number of value
bytes it carries (§4), `place` where §3.2 or §3.3 put it. A stack part
that starts exactly where the previous part of the same argument ends,
both in the value and on the stack, is merged into it. So `i128` on the
stack is one part of 16 bytes, and a stack block of `[4 x i64]` is one
part of 32 bytes, while `{ i8, i128 }` on the stack is two parts (the
padding between them is not passed).

Padding inside an aggregate belongs to no part and is not passed.

### 3.5 Examples

All from `table::*`; `s+N` is `Place::Stack(N)`, and `a[o..e]` are bytes
of argument `a`.

| signature | parts | `stack_size` |
|---|---|---|
| `(i32, ptr, double, float)` | `x0`(4 bytes), `x1`, `v0`, `v1`(4) | 0 |
| `(i64 × 9)` | `x0`..`x7`; 8[0..8] at s+0 | 8 |
| `(double × 9, float, half)` | `v0`..`v7`; s+0 (8 bytes); s+8 (4); s+16 (2) | 24 |
| `(double × 8, float, fp128)` | `v0`..`v7`; s+0 (4); s+16 (16) | 32 |
| `(i8, i128, i64, i128, i32, i128, i128)` | `x0`; `x2`,`x3`; `x4`; `x6`,`x7`; s+0 (4); s+16 (16); s+32 (16) | 48 |
| `(i64 × 7, i128, i64)` | `x0`..`x6`; s+0 (16); s+16 (8) — `x7` unused | 24 |
| `(i64 × 5, i256)` | `x0`..`x4`; `x6`,`x7`; 5[16..32] at s+0 | 16 |
| `(i64 × 7, { i64, i64 })` | `x0`..`x6`; 7[0..8] in `x7`; 7[8..16] at s+0 | 8 |
| `(i64 × 7, [2 x i64], i64)` | `x0`..`x6`; s+0 (16); s+16 (8) — `x7` unused | 24 |
| `({ i8, i32 }, { float, i32 })` | 0[0..1] `x0`; 0[4..8] `x1`; 1[0..4] `v0`; 1[4..8] `x2` | 0 |
| `(double × 6, [3 x float], float)` | `v0`..`v5`; s+0 (12); s+16 (4) — `v6`, `v7` unused | 24 |
| `(i64 × 8, i8, [2 x i64])` | `x0`..`x7`; s+0 (1); s+8 (16) | 24 |
| `(i64 × 8, i8, [2 x i64] alignstack(16))` | `x0`..`x7`; s+0 (1); s+16 (16) | 32 |
| `([3 x i8])` | `x0`, `x1`, `x2`, one byte each | 0 |

## 4. From a type to pieces

A **leaf** is a value that is not an array or a structure. §4.1 to §4.3
give the pieces of each leaf type as "class, bytes"; a piece's part has
that many bytes.

### 4.1 Scalars

| type | pieces |
|---|---|
| `iN`, N ≤ 32 | one `W`, ⌈N/8⌉ bytes |
| `iN`, 33 ≤ N ≤ 64 | one `X`, ⌈N/8⌉ bytes |
| `ptr` (address space 0) | one `X`, 8 bytes |
| `half` | one `H`, 2 bytes |
| `bfloat` | one `Bf`, 2 bytes |
| `float` | one `S`, 4 bytes |
| `double` | one `D`, 8 bytes |
| `fp128` | one `Q`, 16 bytes |

`x86_fp80` is `abi:type-x86_fp80`, a target extension type
`abi:target-ext-type`, a pointer in another address space
`abi:addrspace`. A type that no parameter can have (`void`, `label`,
`metadata`, `token`, an opaque structure, a function type) is
`abi:invalid-signature`.

*Evidence:* AAPCS64 C.1, C.9; `table::scalars`; the grid's `scalars()`
(20 integer widths from 1 to 256).

### 4.2 Integers wider than 64 bits

`iN` with N > 64 is ⌈N/64⌉ pieces of class `X`, lowest bits first. Each
has 8 bytes, except the last, which has ⌈N/8⌉ − 8·(pieces − 1). The first
is "the first piece of an integer wider than 64 bits" of §3.2 when the
integer is an argument or a leaf of one (outside a block).

*Evidence:* AAPCS64 C.10, C.11 for `i128`; LLVM extends the same
treatment to every width. `table::wide_integers`; the grid (65, 96, 127,
128, 129, 192, 256 bits).

### 4.3 Fixed vectors

For `<n x T>`, let `b` be the element's size in bytes and `t = n·b`.
Supported elements are `i8`, `i16`, `i32`, `i64`, `ptr`, `half`,
`bfloat`, `float`, `double` and `fp128`; any other element (`i1`, `i7`,
`i128`, …) is `abi:vector-element`. A scalable vector is
`abi:scalable-vector`. The first row that applies gives the pieces:

| condition | pieces | example |
|---|---|---|
| `T` is `fp128` | `n` of class `Q`, 16 bytes each | `<2 x fp128>` |
| `t` = 8 | one `D`, 8 bytes | `<2 x float>`, `<1 x i64>` |
| `t` = 16 | one `Q`, 16 bytes | `<4 x i32>` |
| `n` = 1, `T` is `half` or `bfloat` | one `H` or `Bf`, 2 bytes | `<1 x half>` |
| `n` = 1 | one `D`, `b` bytes | `<1 x float>`, `<1 x i8>` |
| `n` a power of two, `t` > 16 | `t`/16 of class `Q`, 16 bytes each | `<8 x i32>` |
| `n` a power of two, `t` < 8, integer or pointer elements | `abi:vector-promoted` | `<2 x i8>`, `<2 x i16>` |
| `n` a power of two, `t` < 8, floating-point elements | one `D`, `t` bytes | `<2 x half>` |
| `n` not a power of two; `m·b` is 8 or 16 for `m` the next power of two | one `D` or `Q`, `t` bytes | `<3 x float>`, `<7 x i8>` |
| `n` not a power of two, otherwise | `n` pieces, `b` bytes each: class `W` for `i8`, `i16`, `i32`; `D` for `i64`, `ptr`, `double`; `H`, `Bf`, `S` for `half`, `bfloat`, `float` | `<3 x i64>`, `<5 x i32>`, `<3 x i8>` |

Piece `i` of a multi-piece vector carries the bytes from `i` times the
piece size on. Note that `<1 x i64>` and the elements of `<3 x i64>`
travel in vector registers, while the elements of `<3 x i8>` travel in
general registers.

`abi:vector-promoted`: LLVM passes such a vector with each element
widened to a larger lane, which is not the value's bytes at some place
and so cannot be said with a `Part`.

*Evidence:* AAPCS64 C.1 for the 8- and 16-byte ("short") vectors; the
rest is LLVM's type legalization. `table::vectors`; the grid's
`vectors()` (58 shapes over all ten element types).

### 4.4 Aggregates

The leaves of a structure are those of its fields, each at the field's
offset (`TypeTable::field_offsets`, so a packed structure needs no
special rule); the leaves of an array are those of its elements, element
`i` at `i` times the element's alloc size. Leaves are taken in offset
order, to any depth, and each is broken into pieces by §4.1 to §4.3. A
piece's `offset` is the leaf's offset plus the piece's offset in the
leaf.

Passed by value and not a block, an aggregate is simply the sequence of
its pieces, each placed by §3.2 independently: it may lie partly in
registers and partly on the stack, in both register classes, and there
is no limit on its size.

*Evidence:* `table::aggregates`; the grid's `aggregates()` and the
random grid (nesting to depth 2, packed structures, empty types).

### 4.5 Types without a leaf

`{}`, `[0 x T]` and aggregates made only of them have no leaf. An
argument of such a type has no part and changes no state; see §6.5 for
what may follow it. A return value of such a type has no part and is not
returned in memory.

*Evidence:* `table::aggregates` (`({}, i64)`), `table::returns`; the
grid.

## 5. Return values

### 5.1 In registers

The return value's type is broken into leaves and pieces as in §4.
Piece by piece, a general piece takes the next of `x0`..`x7` and a
vector piece the next of `v0`..`v7`. There is no even-register rule and
no block rule, and the unsupported arrays of §3.3 are supported here.
`void` and a type without a leaf return nothing.

If the general pieces number more than 8, or the vector pieces more than
8, the whole value is returned in memory (§5.2) and `ret` is empty.

`ret_attrs` may hold only `zeroext` or `signext` (§7), and only on an
integer; `nest` or a memory attribute there is `abi:invalid-signature`,
and the flags of §6.6 report their names.

| return type | `ret` |
|---|---|
| `i64` | `x0` |
| `{ i64, double, i8, float, i128 }` | [0..8] `x0`; [8..16] `v0`; [16..17] `x1`; [20..24] `v1`; [32..40] `x2`; [40..48] `x3` |
| `i512`, `[8 x i64]` | `x0`..`x7` |
| `[8 x double]` | `v0`..`v7` |
| `[2 x i128]` | `x0`..`x3` |
| `[9 x i64]`, `[9 x float]`, `i513` | memory |

*Evidence:* AAPCS64 "Result return" for the types it knows; `table::returns`;
the grid, in which every `pressures()` type is also a return type, plus
`returns_only()`.

### 5.2 In memory

`ret_memory` is `Some(RetMemory { address: Place::Reg(x8), returned_in:
None })`. The caller passes in `x8` the address of a buffer of
`alloc_size_of(ret)` bytes aligned to `pref_align_of(ret)`; the callee
leaves the value there, as a `store` of it to that address would (each
leaf's in-memory image; padding bytes are unspecified), and does not
hand the address back in any register. `x8` is not preserved by the
call. The buffer must not overlap anything the callee can otherwise
reach.

The hidden address is assigned before the arguments and uses no argument
register, so it changes no argument's place. `x8` is
`hod_a64::abi::INDIRECT_RESULT`.

This is what LLVM does with an IR return type that does not fit its
return registers ("`sret` demotion"). It is the same convention as a
`void` function with an `sret` argument (§6.1).

*Evidence:* AAPCS64 "Result return" (the address in `x8`);
`table::returns`; the grid in both directions (hod's callee leaves junk
in the padding bytes of the buffer; LLVM's caller reads only the
leaves). The alignment is what LLVM's caller provides and LLVM's callee
assumes; it cannot be observed on this target, where LLVM's stores
tolerate any address, and the grid's buffers are 16-byte aligned.

## 6. Parameter attributes

An argument with none of these attributes is passed by value (§3).

### 6.1 `sret`

The argument, which must be a `ptr` in address space 0, is one `Value`
part of 8 bytes in `x8`. It uses no argument register and changes no
other argument's place. For the callee it is an ordinary pointer
argument that happens to arrive in `x8`; nothing is returned for it.

`abi:invalid-signature` if the return type is not `void`, if the
signature has a second `sret`, or if the argument also has `nest`
(LLVM's verifier refuses each).

*Evidence:* AAPCS64 "Result return"; `table::attributes`; the grid's
`attributes()` (`sret` first, in the middle, with nine other arguments).

### 6.2 `byval(T)`

The caller passes a copy of the pointee in the stack-argument area; the
pointer itself is not passed. With `size = alloc_size_of(T)`:

- the alignment `a` is the argument's `alignstack` if present, else its
  `align` if present, else the ABI alignment of `T`; `a` above 16 is
  `abi:byval-align` (the area's base is aligned to 16 only, so no offset
  in it can promise more);
- NSAA is rounded up to `max(a, 8)`; the copy is at NSAA; NSAA increases
  by `max(size, 8)` rounded up to a multiple of 8;
- the argument has one `Pointee` part: offset 0, `size` bytes, at that
  stack place. A pointee of size 0 still gets its 8 bytes and a part of
  size 0.

No register is used and the copy never goes to a register. The callee's
parameter value is the address of the copy, `S` plus the part's stack
offset; the callee owns the copy and may write to it.

The argument must be a `ptr` in address space 0 and `T` must be sized,
else `abi:invalid-signature`; `nest` with it likewise. A scalable `T` is
`abi:scalable-vector`.

*Evidence:* LLVM's rule (AAPCS64 has no such construct; clang does not
emit `byval` for this target). `table::attributes`; the grid's
`attributes()` (14 pointee types × 9 alignment combinations × 3
positions).

### 6.3 `nest`

The argument, which must be a `ptr` in address space 0, is one `Value`
part of 8 bytes in `x15` (`hod_a64::abi::NEST`). It uses no argument
register. A second `nest` is `abi:invalid-signature`.

This follows llc 22. llc 19 passes `nest` in `x18`; the grid does not
run `nest` signatures against llc 19.

*Evidence:* `table::attributes`; the grid's `attributes()` against llc
22 at `-O0` and `-O2`.

### 6.4 `byref`, `align`, `alignstack`

- `byref(T)`: the argument is an ordinary pointer, passed by value. On
  anything but a pointer it is `abi:invalid-signature`.
- `align` matters only together with `byval` (§6.2).
- `alignstack` matters only for a block that goes to the stack (§3.3)
  and for `byval` (§6.2). On a scalar, a vector or a structure it
  changes nothing.

*Evidence:* `table::attributes`, `table::aggregates`; the grid's
`attributes()`.

### 6.5 After an argument without a part

Once an argument has had no part (§4.5), every later argument that has
`zeroext`, `signext`, `nest`, `alignstack`, `sret` or `byval` is
`abi:attribute-after-empty-argument`. (GlobalISel skips a zero-sized
argument without advancing its position in the attribute list, and so
reads the attributes of every later parameter from the wrong parameter;
SelectionDAG does not.) Later arguments without those attributes are
passed as usual.

*Evidence:* `errors::every_name`; the random grid, which draws empty
types and then only passes what `assign` accepts.

### 6.6 Unsupported attributes

| attribute | name |
|---|---|
| `inreg` | `abi:inreg` |
| `swiftself` | `abi:swiftself` |
| `swifterror` | `abi:swifterror` |
| `swiftasync` | `abi:swiftasync` |
| `inalloca` | `abi:inalloca` |
| `preallocated` | `abi:preallocated` |
| `elementtype` | `abi:elementtype` |

`zeroext` or `signext` on a type that is not an integer is
`abi:invalid-signature`. Attributes that say nothing about passing
(`noundef`, `nonnull`, `noalias`, …) are not part of `ParamAttrs`'s
passing information and have no effect.

*Evidence:* `errors::every_name`.

## 7. Bits above an integer

"Extended to `k` bits" means: bits `N..k` of the register, or of the
stack slot read as a little-endian number, equal bit `N − 1` (by sign) or
are zero (by zero). The **producer** of a value is the caller for an
argument and the callee for a return value; the **consumer** is the
other side.

These rules apply to an argument, or a return value, whose own type is
an integer `iN` and that is passed by value. Leaves of aggregates and
elements of vectors carry no promise: their producer owes nothing above
the leaf, and their consumer must assume nothing. For an integer wider
than 64 bits the rules speak of its last piece.

**What the producer must do** (the same in a register and on the
stack; `produced_extension(bits, attrs)`):

| type and attribute | extend |
|---|---|
| `i1`, no attribute | by zero to 8 bits |
| any other `iN`, no attribute | nothing |
| `iN zeroext` / `signext`, N < 32 | by zero / sign to 32 bits |
| `i32 zeroext` / `signext` | nothing |
| `iN zeroext` / `signext`, N > 32 | by zero / sign to the next multiple of 64 bits (nothing if N is one) |

**What the consumer may assume** (`assumed_extension(bits, attrs,
place)`, with `place` that of the value's last part) is exactly the same
table, with one restriction: on the stack, an `i1` or `i8` with an extension attribute
may be assumed extended to 8 bits only, and an `i16` to 16 bits only.
(LLVM's callers store just that many bytes of a stack slot for those
three types; hod's producer rule above does more than LLVM needs there,
for the sake of one rule.) Everything a table does not promise is
unspecified and must not be relied on: in particular, without an
attribute the upper bits of a `w` register holding an `i8` are
arbitrary.

The first row is AAPCS64's `_Bool`: LLVM's callee assumes that an `i1`
argument without attributes is a byte holding 0 or 1, and clang passes
`bool` as a plain `i1`. LLVM's caller does not rely on it for a returned
`i1`, but LLVM's callee provides it, and so must hod's.

*Evidence:* AAPCS64 "Fundamental data types" (`_Bool`) and C.9, C.16
(unused bits unspecified); the attribute rows are LLVM's. The grid's
`extensions()` (20 widths × both attributes × registers, stack, return,
variadic): hod's producer does exactly the minimum of the first table and
puts junk in every other bit, and LLVM's consumer, made to extend what
it received, must then show the right bits; hod's consumer records
whole 8-byte units and must find what the second table promises. The
tables are `hod_a64::abi::produced_extension` and `assumed_extension`,
which the tests' emitter and comparison call; `table::extension` pins
them row by row. For aggregates, LLVM's consumer stores each member and the stored bytes must
be right although hod passed junk above every leaf.

## 8. What the code generator does with an `Assignment`

`assign` only says where; these are the duties that go with it. In the
tests they are implemented by `emit.rs`, an emitter driven by nothing
but an `Assignment`, and checked by the grid in both directions.

### 8.1 A call

1. Reserve the stack-argument area: `stack_size` rounded up to 16 bytes
   at the lowest addresses of the frame, so that `sp` is its base and is
   16-byte aligned at the call instruction (AAPCS64 "The stack").
2. For each `Value` part, copy `size` bytes of the argument's in-memory
   image from `offset` to the part's place; for an integer argument
   apply §7.
3. For each `Pointee` part, copy `size` bytes from the address the
   argument's value holds to the part's stack place. The pointer is not
   passed anywhere.
4. If `ret_memory` is set, make the buffer of §5.2 and put its address
   at `ret_memory.address`.
5. After the call, assemble the return value from the `ret` parts, or
   find it in the buffer. Assume of the bits outside the parts only what
   §7 allows.

Bytes of the area that no part covers, and register bits above a part,
may hold anything. The area still belongs to the caller after the call,
but the callee may have overwritten all of it (AAPCS64 "Parameter
passing": the callee may modify its stack arguments).

### 8.2 A function's entry

1. Each `Value` part of a parameter is found at its place, with `S` the
   value of `sp` at entry. A parameter may be assembled from several
   parts; the bytes between them are padding.
2. A parameter with a `Pointee` part has as its value the address
   `S + offset` of that part (§6.2).
3. If `ret_memory` is set, the address at `ret_memory.address` must be
   kept until the return and the value stored through it (§5.2).
4. On return, each `ret` part is put at its place and §7 applied.

### 8.3 A variadic function's entry

Besides §8.2, a variadic function that uses `va_start` keeps the
argument registers that hold no named argument (§9.3).

### 8.4 Registers

Argument and return registers that an assignment does not name hold
nothing meaningful at a call and nothing meaningful on return. Which
registers a call preserves is AAPCS64's "General-purpose registers" and
"SIMD and floating-point registers" (`x19`..`x28`, `x29`, `sp`, and the
low 64 bits of `v8`..`v15`), for every convention of §1.2;
`spec/codegen.md` §5.5 builds on that.

### 8.5 `musttail`

LLVM's verifier requires of a `musttail` call that caller and callee
have the same calling convention, are both variadic or both not, have
the same number of parameters with the same types and the same return
type, and agree on every parameter in `sret`, `byval`, `inalloca`,
`inreg`, `alignstack`, `swiftself`, `swiftasync`, `swifterror`,
`preallocated`, `byref`, and in `align` where `byval` or `byref` is
present. They may differ in `zeroext`, `signext` and `nest`.

- `zeroext` and `signext` change no place (§7 is about bits, not
  places). So if the two signatures do not differ in `nest`, their
  assignments are equal, part for part, and so are their stack-argument
  areas: the callee's arguments fit exactly where the caller's own
  arrived.
- If they differ in `nest`, one of them has an argument in `x15` that
  the other has in an argument register, and every place after it may
  differ, including `stack_size`. `spec/codegen.md` §1 already rejects a
  `musttail` call whose stack-argument area is larger than the caller's.
- The duties of §7 hold for a `musttail` call as for any call: a value
  received without an extension attribute and passed on with one has to
  be extended.
- With `ret_memory`, the callee's buffer is the caller's own: the
  address that arrived in `x8` is passed on in `x8`.
- In a variadic function, a `musttail` call passes on, beyond the named
  arguments, everything the caller received: `x0`..`x7` and all 128
  bits of `v0`..`v7` as far as they hold no named argument, `x8`, and
  the stack-argument area beyond the named arguments, all exactly as
  they were at the caller's entry. (This is what makes a variadic
  forwarding thunk possible; the thunk cannot know which of them carry
  arguments.)

*Evidence:* `table::musttail_pairs`; `cc::musttail_in_llvm` (LLVM accepts
a pair differing in `nest`, refuses one differing in `alignstack`, and
its variadic thunk restores `x1`..`x7`, `x8` and `q0`..`q7` before the
branch, in all four configurations).

## 9. Variadic functions

### 9.1 Assignment

On Linux a variadic call is assigned exactly as a non-variadic one with
the same argument types: named and variable arguments alike go through
§3, in order, with the same state. `Sig::params` lists the types of all
arguments of the call, `Sig::fixed` is the number of named ones; for a
function's own entry `params` holds just the named ones and `fixed`
equals their number. `fixed` larger than the number of `params` is
`abi:invalid-signature`.

(The frontend has already applied C's default argument promotions and
AAPCS64's coercions to the variable arguments. clang passes them as, for
example, `i32`, `i64`, `double`, `ptr`, `fp128`, `i128`, `[2 x i64]`,
`[3 x float] alignstack(8)`, `[2 x double] alignstack(8)`, `[2 x fp128]
alignstack(16)`, `<4 x i32>`.)

*Evidence:* AAPCS64 C.1 to C.17 make no distinction; `table::variadic`;
the grid's three variadic `pressures()`; `variadic::variadic_against_c`.

### 9.2 `va_list`

`va_list` is AAPCS64's ("The `va_list` type"): 32 bytes, 8-byte
aligned.

| offset | field | type |
|---|---|---|
| 0 | `__stack` | pointer: the next variable argument on the stack |
| 8 | `__gr_top` | pointer: the end of the general register save area |
| 16 | `__vr_top` | pointer: the end of the vector register save area |
| 24 | `__gr_offs` | `i32`: offset from `__gr_top` of the next general register argument; ≤ 0 |
| 28 | `__vr_offs` | `i32`: offset from `__vr_top` of the next vector register argument; ≤ 0 |

### 9.3 The save areas and `va_start`

`Assignment::va` of the function's own signature is `VaInfo { gpr_used:
g, fpr_used: f, stack_offset: s }` (§9.4). A variadic function that uses
`va_start` must, at entry:

- keep `x[g]`..`x7` in the **general register save area**: 8 bytes each,
  in ascending register order at ascending addresses, so that `x[i]` is
  at `__gr_top − 8·(8 − i)`;
- keep `q[f]`..`q7` (all 128 bits) in the **vector register save
  area**: 16 bytes each, likewise, so that `q[i]` is at
  `__vr_top − 16·(8 − i)`.

The areas may be anywhere in the frame and may hold all eight registers
of each class. `llvm.va_start(ap)` stores into `ap`:

| field | value |
|---|---|
| `__stack` | `S + s`, with `S` the value of `sp` at the function's entry |
| `__gr_top` | the address just past `x7` in the general save area |
| `__vr_top` | the address just past `q7` in the vector save area |
| `__gr_offs` | `−8·(8 − g)` |
| `__vr_offs` | `−16·(8 − f)` |

`llvm.va_copy(dst, src)` copies the 32 bytes; `llvm.va_end` does
nothing. Reading an argument out of a `va_list` is AAPCS64's "The
`va_arg()` macro", which a frontend expands into IR; the IR `va_arg`
instruction is not specified here (LLVM's own lowering of it for this
target reads `__stack` only).

*Evidence:* AAPCS64 "Variable argument lists" ("Register save areas",
"The `va_start()` macro"). `variadic::variadic_against_c`: a hod variadic
callee built from `VaInfo` alone as above hands its `va_list` to a C
function that reads it with `va_arg` (clang 22 and 19, `-O0` and `-O2`),
after a C caller called it; and hod's caller calls C variadic functions.
The C types cover every kind clang passes: integers, `double`, pointers,
`long double`, `__int128`, small structures, homogeneous float, double
and vector aggregates, vectors.

### 9.4 `VaInfo`

For a variadic signature, `va` is the state just after the last named
argument:

- `gpr_used`: NGRN. It counts registers skipped by the even-register
  rule and is 8 once a block or a wide integer has exhausted the general
  registers.
- `fpr_used`: NSRN, likewise.
- `stack_offset`: NSAA rounded up to 8. (A packed stack block can leave
  NSAA at 12; the first variable argument on the stack is at 16.)

For a signature that is not variadic, `va` is all zero.

*Evidence:* `table::variadic`; `variadic::variadic_against_c`, whose hod
callee builds its `va_list` from these three numbers.

## 10. Unsupported constructs and limits

`assign` fails with `AbiUnsupported(name)`; `name` is one of
`hod_a64::abi::UNSUPPORTED`, in this order. The names are stable: the
coverage log (`spec/cli.md`) counts by them.

| name | what |
|---|---|
| `abi:cc` | a calling convention other than those of §1.2 |
| `abi:invalid-signature` | a signature LLVM's verifier would refuse or that cannot occur: `fixed` beyond the parameters; variadic `fastcc`; an extension attribute on a non-integer; a memory attribute or `nest` on the return value; `sret`, `byval`, `nest` or `byref` on something that is not a pointer (in address space 0 for the first three); `sret` with a non-`void` return, twice, or with `nest`; `byval` of an unsized type or with `nest`; `nest` twice; a parameter or leaf type that is not first class |
| `abi:type-x86_fp80` | `x86_fp80` anywhere in a parameter or return type |
| `abi:scalable-vector` | a scalable vector, also as a `byval` pointee |
| `abi:target-ext-type` | a target extension type |
| `abi:addrspace` | a pointer in an address space other than 0 |
| `abi:vector-element` | a vector whose element is not one of §4.3's |
| `abi:vector-promoted` | a vector LLVM passes with widened lanes (§4.3) |
| `abi:wide-int-array` | an array argument of integers wider than 64 bits (§3.3) |
| `abi:bfloat-array` | an array argument of `bfloat` (§3.3) |
| `abi:vector-array` | an array argument of vectors of more than one piece (§3.3) |
| `abi:one-leaf-array` | an array argument with one leaf and a different stack alignment than the leaf's (§3.3) |
| `abi:attribute-after-empty-argument` | §6.5 |
| `abi:inreg`, `abi:swiftself`, `abi:swifterror`, `abi:swiftasync`, `abi:inalloca`, `abi:preallocated`, `abi:elementtype` | the attribute (§6.6) |
| `abi:byval-align` | `byval` aligned above 16 (§6.2) |
| `abi:too-many-pieces` | the arguments of one signature need more than `MAX_PIECES` (65,536) pieces; a stack block that §3.4 merges into one part counts as one piece, and `sret`, `byval` and `nest` arguments as one each |
| `abi:size-limit` | an offset in a value, a part's size, a stack offset or `stack_size` does not fit 32 bits |

When a signature is unsupported for several reasons, which name is
reported is not specified.

The limits bound the work: `assign` runs in time proportional to the
description of the types plus the number of parts it produces, never to
an array's length as such (a `[1000000 x i64]` argument is one stack
part; as a return value it is "memory" after nine leaves), and recurses
no deeper than the types nest.

*Evidence:* `errors::every_name` (each name is produced by a signature,
and the set of names produced is exactly `UNSUPPORTED`; the limits);
`errors::any_signature_is_answered` (20,000 random signatures over odd
types and attribute combinations: an answer or a name, never a panic).

## 11. Validation

### 11.1 The grid

The gate for this document is an executable cross-check against the
oracle, in both directions, for every supported signature of a grid:

- **hod calls LLVM**: a caller written by the test's emitter from the
  `Assignment` (§8.1) passes known bytes to a callee compiled by `llc`
  from IR, which stores every parameter; and takes its return value.
- **LLVM calls hod**: a caller compiled by `llc` passes known values to a
  callee written by the emitter (§8.2), which records what it finds at
  each place and returns a known value.

Bits that carry nothing are filled with junk on hod's side (registers
above a part, unused argument registers, stack bytes outside the parts,
padding in a memory return), so that anything LLVM reads beyond what
this document says shows up. Every case runs under all four
configurations of §1.1, except that `nest` runs under llc 22 only
(§6.3) and that in the IR grid a variadic case runs with LLVM calling
only (the other direction is `variadic::variadic_against_c`, §9.3).
The cases of a batch share a handful of compiler and linker runs.

- `grid::systematic_grid`: every scalar (§4.1), 58 vectors (§4.3) and
  some 70 aggregates (§4.4), each under 13 kinds of register and stack
  pressure and as a return type; the extension attributes at 20 widths
  (§7); `sret`, `byval`, `nest`, `byref` and `alignstack` (§6); return
  types beyond the registers (§5); the conventions (§1.2). About 2,970
  signatures.
- `grid::random_grid`: seeded random signatures of up to 19 parameters
  over nested random types and attributes; whatever `assign` accepts is
  run. 400 signatures in the quick gate; 8,000 with `HOD_ABI_GRID=full`.
- `variadic::variadic_against_c`: §9, against C compiled by clang then
  `llc`. About 760 signatures in the quick gate, about 2,090 with
  `HOD_ABI_GRID=full`.

`llc` itself crashes on a few odd signatures (GlobalISel's legalizer);
the harness leaves the function out, retries, and reports it. With
`HOD_REQUIRE_TOOLS=1` a missing tool fails the tests instead of skipping
them; `HOD_ABI_FAILURES=<file>` writes every failure to a file;
`HOD_ABI_KEEP=1` keeps the generated sources.

The tool-free tests (`table::*`, `errors::*`, `reg::tests`) pin the
assignments of this document's examples and the error names.

### 11.2 Cost

`perf::perf_of_assign` prints nanoseconds per call for a warm `out`
(`cargo test --release -p hod_a64 --test abi -- --nocapture perf`).
`assign` allocates nothing once `out` has grown. It costs about 5 ns
for an empty signature and 12 to 18 ns per scalar parameter; a signature
of eight aggregate and attributed parameters (15 parts) costs about
340 ns.

## 12. Where LLVM and AAPCS64 part, and where LLVM differs from itself

**LLVM's treatment of IR types against AAPCS64's of C types.** None of
these is a bug in either; they are why a frontend coerces C types, and
why this document cannot simply cite AAPCS64.

- A structure is passed leaf by leaf (§4.4): it can be split between
  registers and the stack and between register classes, and is never
  replaced by a pointer to a copy, whatever its size (AAPCS64 B.4, C.12
  to C.15 say otherwise of C structures; clang therefore passes
  `[2 x i64]`, `i128` or a `ptr`).
- An array of integers of at most 32 bits is not a block: `[4 x i32]`
  takes four registers or four 8-byte slots (§3.3).
- A block of floating-point or vector leaves may have up to eight
  members (AAPCS64's homogeneous aggregates stop at four), and a block
  of `i64` may fill all eight general registers.
- Integers wider than 128 bits exist and may straddle registers and
  stack (§3.2).
- Vectors other than 8 and 16 bytes exist (§4.3).
- A return value may use eight registers of each class and several
  classes at once (§5.1; AAPCS64 returns at most 16 bytes or four
  floating-point members in registers).
- `zeroext` and `signext` create duties AAPCS64 does not have (§7);
  without them LLVM follows AAPCS64 in leaving upper bits unspecified,
  and in treating a plain `i1` as `_Bool`.
- `byval` (§6.2) and `nest` (§6.3) are LLVM's own.
- A value returned in memory because the IR type does not fit eight
  registers is LLVM's own decision; that the address travels in `x8` is
  AAPCS64's.

**llc 19 against llc 22.**

- `nest` is passed in `x18` by llc 19 and in `x15` by llc 22 (§6.3).
- `[N x bfloat]` is a block for llc 22 and not for llc 19 (§3.3).
- Nothing else in the grid distinguishes them.

**GlobalISel (`-O0`) against SelectionDAG (`-O2`)**, the same in both
versions; each is an unsupported name:

- arrays of integers wider than 64 bits: whether the even-register rule
  applies to the members (`abi:wide-int-array`);
- arrays of vectors that take more than one register
  (`abi:vector-array`);
- one-leaf arrays whose stack alignment differs from the leaf's
  (`abi:one-leaf-array`);
- parameter attributes after a zero-sized argument
  (`abi:attribute-after-empty-argument`).

Also observed: LLVM's verifier refuses `sret` on a function that returns
a value and variadic `fastcc`; LLVM has no AArch64 lowering for
`coldcc`; and on the stack LLVM's callers write one byte for an `i1` or
`i8` argument and two for an `i16`, whatever the extension attribute
(§7).
