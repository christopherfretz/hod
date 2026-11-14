# Passing arguments and returning values on x86-64 Linux

`hod_x64::abi::assign` answers one question: given an IR-level signature,
where does each byte of each argument and of the return value travel?
The answer is the one LLVM gives for the same signature, so that code
compiled by hod and code compiled by LLVM can call each other in both
directions. This document states the rules completely enough to write
`assign` again from it, and states what a code generator still has to do
with the answer.

The interface (`Sig`, `Part`, `Place`, `Assignment`, `VaInfo`,
`AbiUnsupported`) is `hod_cg::abi`, described in `spec/codegen.md` §7.2.
The types and attributes are `hod_ir`'s (`spec/ir.md`). The layout of
this document follows `spec/abi-a64.md`, section for section.

## 1. Scope

### 1.1 Target and oracle

- The target is `x86_64-unknown-linux-gnu`: little-endian, LP64, the
  System V AMD64 psABI, SSE2 and nothing above it unless §1.3 says
  otherwise. Sizes, offsets and alignments come from the module's data
  layout through `hod_ir::TypeTable`; nothing here assumes a particular
  layout string.
- The **oracle** is `llc`, version 22, and version 19 as a second
  witness, each at `-O0` with FastISel (its default), at `-O0
  -fast-isel=0` (SelectionDAG; design 0006's reference configuration)
  and at `-O2`; always `-relocation-model=pic`. These six
  configurations are what "LLVM" means below. FastISel hands every call
  it does not lower itself to SelectionDAG, and all six agree on every
  rule except the ones marked "LLVM 19" (§4.2, §9.4), where this document
  follows LLVM 22.
- The psABI is the *System V Application Binary Interface, AMD64
  Architecture Processor Supplement*; it speaks about C types, this
  document about IR types. A frontend has already turned the first into
  the second (coerced aggregates, `byval`, `sret`), so the psABI's
  classification of C aggregates (INTEGER, SSE, MEMORY over eightbytes)
  is the frontend's business and does **not** happen here: LLVM passes a
  first-class IR aggregate leaf by leaf (§4.4). §12 lists where LLVM's
  treatment of IR types differs from what the psABI says of the C types.
- Every rule names its evidence. The tests are in
  `crates/hod_x64/tests/abi/` and are named `module::test`; "the grid"
  is `grid::systematic_grid`, `grid::random_grid` and
  `data::data_signatures_against_llc` (§11). `table::*` are assignments
  written out by hand, which double as this document's examples.
  `tests/abi/data/` is a copy of design 0006 §3's experiments
  (`NN-*.ll` with `llc` 19 and 22's output beside each, and `run.sh`
  that produced it).
- Not covered: the frame and the prologue (design 0006 §2.9,
  `spec/codegen.md` §5.6), unwinding, DWARF beyond the register numbers
  of §8.6.

### 1.2 Calling conventions

| convention | `assign` |
|---|---|
| `ccc` (0) | specified by this document |
| `fastcc` (8) | as `ccc`, except one more return register for `float` and `double` (§5.1) |
| `x86_64_sysvcc` (78) | as `ccc`: it is the System V convention, which is this target's `ccc` (WP-141) |
| `preserve_mostcc` (14), `preserve_allcc` (15) | as `ccc`; the callee keeps more registers, which is the frame's (`spec/codegen.md` §5.6.23, WP-181) |
| every other number, including `swiftcc` (16) and `cxx_fast_tlscc` (17) | `abi:cc` |

`hod_x64::abi::cc_is_c` is the predicate for the first three rows. "As
`ccc`": LLVM compiles a module that defines and calls `fastcc`
functions (arguments of every class and on the stack, values live across
calls, returns in registers, in memory and on the x87 stack, `sret`,
`byval`) to the very assembly it emits for `ccc`, in all six
configurations. The one difference is in the return registers: under
`fastcc`, LLVM 19 and 22 give `float` and `double` return pieces
`xmm0`, `xmm1` *and `xmm2`* before `st0`, `st1` (so `{ double, double,
double }` returns in registers, and `<5 x float>` is refused as
`x64:ret-x87-part` rather than returned in memory). `cxx_fast_tlscc` preserves more registers than `ccc` on
x86-64 (the same module compiles differently), and `swiftcc` returns in
more registers and gives `swiftself`/`swifterror` registers of their
own; both are refused. A variadic `fastcc` signature is
`abi:invalid-signature` (LLVM's verifier refuses it).

The code generator does the same: a `fastcc` definition, call or
`invoke` is compiled exactly as a `ccc` one with the `fastcc`
`Assignment` (WP-140; the third `xmm` return register is the only place
the two conventions differ on x86-64, and it comes from `assign`). This
is LLVM's `-O0` behaviour: `fastcc` lets an optimizing compiler choose
another convention, which neither `llc -O0` nor hod does, so a `fastcc`
function compiled by one may be called from an object of the other
(`abi_followups::rows_run_against_the_interpreter_and_llc`).

*Evidence:* `cc::conventions_equal_to_c` (which also shows the
comparison can fail: `cxx_fast_tlscc` differs); `table::conventions`; the
grid's `conventions()` cases; `data::data_signatures_assign` (`swiftcc`
in `06-ptrattrs.ll`).

### 1.3 Target features

`assign` takes a `Features { avx, avx512 }`. Without either, the rules
below are complete. With `avx` (or anything that implies it), LLVM
passes and returns a vector wider than 128 bits in one `ymm` (and with
`avx512`, 512 bits in one `zmm`, and `<N x i1>` in a mask register);
those signatures are refused as `x64:wide-vector-abi`:

- with `avx`: an argument or return value with a vector leaf of more
  than 16 bytes;
- with `avx512` besides: a vector of `i1`, whatever its length.

A 128-bit or narrower vector travels as §4.3 says whatever the features.

*Evidence:* `errors::every_name` (the refusals with the features, the
same signatures accepted without them). The grid runs without AVX only.

**Whose features decide (WP-140).** The *compiled function's*: every
signature a function's code touches (its own, and that of every call and
`invoke` in it, whatever the callee's attributes) is assigned with the
features of the function being compiled. `llc` 19 and 22 do the same: a
caller without AVX passes a `<4 x double>` in two `xmm` registers to a
callee declared or defined with `"target-features"="+avx"`, which reads
it from one `ymm`. Such a mismatch is the program's (clang warns about
it); hod reproduces LLVM, it does not repair it.

`X64` (the `hod_cg::Target`) learns the features through
`Target::for_function` (`spec/codegen.md` §8.2), which
`hod_x64::features::of_function` computes from the function's
`"target-cpu"` and `"target-features"` attributes:

1. The CPU. Without `"target-cpu"`, or with `generic`, `x86-64`,
   `x86-64-v2` or another CPU LLVM knows without AVX (the table
   `CPUS_WITHOUT_AVX`), neither feature. A CPU LLVM knows with AVX and
   without AVX-512 (`CPUS_WITHOUT_AVX512`: `x86-64-v3`, `haswell`,
   `skylake`, `znver3`, ...): `avx`. Any other CPU, known or not: both
   (an unknown CPU may have AVX-512; the answer is a refusal, never a
   wrong placement).
2. Then each `+name`/`-name` of `"target-features"` in order: `+avx512*`
   or `+avx10*` sets both; `+avx`, `+avx2`, `+fma`, `+f16c`, `+fma4`,
   `+xop`, `+vaes`, `+vpclmulqdq` or any other `+avx*` sets `avx`;
   `-sse`, `-sse2`, `-sse3`, `-ssse3`, `-sse4.1`, `-sse4.2` or `-avx`
   (what AVX implies) clears both; `-avx2`, `-fma`, `-f16c` or `-avx512f`
   (what AVX-512 implies) clears `avx512`.

The command line's `-mcpu`/`-mattr` do not reach a function and do not
take part (`hod_cg::Options` has no field for them). An over-approximation
only refuses (`x64:wide-vector-abi`); an under-approximation would place a
wide vector in `xmm` registers where LLVM uses one `ymm`, so the tables
only list CPUs whose lack of AVX (or AVX-512) was checked against `llc`.
*Evidence:* `features::tests::features_that_decide_the_convention`;
`abi_followups::cpu_tables_agree_with_llc` and
`abi_followups::wide_vectors_follow_the_function` (hod's object mixed with
`llc`'s under qemu, both directions).

## 2. Terms

**Registers.** A `PReg` index names a register as `hod_x64::reg` does
(`hod_x64::abi` uses those registers since WP-140):

| index | register |
|---|---|
| 0..=15 | the general registers by hardware encoding: `rax` 0, `rcx` 1, `rdx` 2, `rbx` 3, `rsp` 4, `rbp` 5, `rsi` 6, `rdi` 7, `r8`..`r15` 8..=15 |
| 16..=31 | `xmm0`..`xmm15` |
| 32..=39 | `st(0)`..`st(7)`, the x87 stack from its top |

`reg::gpr(n)`, `reg::fpr(n)`, `reg::st(n)`, `is_gpr`, `is_fpr`, `is_x87`
and `number` (the hardware number) convert. A value in an `xmm` occupies its low bytes; an x87 register
holds an 80-bit `x86_fp80`.

**Part.** `Part { arg, kind, offset, size, place }` says: bytes
`offset..offset + size` of argument number `arg` (or of the return
value, for which `arg` is 0) travel at `place`.

- `Place::Reg(r)`: byte `offset + k` of the value is bits `8k..8k + 8`
  of `r`; the bits above are unspecified except as §7 says. For an x87
  register the part is the 10 bytes of an `x86_fp80`'s image.
- `Place::Stack(o)`: byte `offset + k` is at address `S + o + k`, where
  `S` is the value of `rsp` at the `call` instruction. At the callee's
  entry, after the return address was pushed, that is `rsp + 8 + o + k`.
  The **stack-argument area** is `S..S + stack_size`.
- `PartKind::Value`: the bytes are those of the argument's value, laid
  out as its in-memory image (`spec/ir.md` §4.3). `PartKind::Pointee`:
  the bytes of the memory a `byval` argument points to (§6.2).
  `PartKind::Lanes`: a vector of `i1` spread over the lanes of a
  register (§4.3).

`Assignment.args` lists parts in argument order and, within an argument,
in `offset` order; `Assignment.ret` does the same for the return value.

**Output.** `assign(types, sig, features, out)` clears `out` first. On
success it has filled it; on an error `out` is left equal to
`Assignment::default()`. The vectors of `out` are reused, so a call with
a warm `out` allocates nothing. `assign` never panics on any signature
built from the `types` it is given. *Tests:* `table::output_is_reused`,
`errors::any_signature_is_answered`.

On this target:

- `RetMemory` is always `{ address: rdi, returned_in: Some(rax) }` (§5.2);
- `sret_returned_in` is `Some(rax)` when the signature has an `sret`
  argument and `None` otherwise (§6.1);
- `vector_regs_used` is the number of `xmm` registers the arguments use
  (§9.1);
- `stack_size` is the end of the last stack slot, not rounded to 16
  (§3.1).

## 3. Assigning arguments

### 3.1 State and order

The state is two counters and an offset: **G**, the next general
argument register (0..=6, indexing `rdi`, `rsi`, `rdx`, `rcx`, `r8`,
`r9`); **V**, the next vector argument register (0..=8, indexing
`xmm0`..`xmm7`); **O**, the next stack offset from `S`. All start at 0.
The two register classes are independent: running out of one does not
affect the other.

1. The return value is assigned first (§5). If it travels in memory, the
   hidden address takes `rdi` and G becomes 1.
2. The arguments are assigned in order, each completely before the next.
   An argument's attributes select a rule of §6; without one, the
   argument is passed **by value**: its type is broken into leaves and
   the leaves into pieces (§4), and each piece is placed by §3.2 (an
   `i128` by §4.2).
3. `stack_size` is O after the last argument: the end of the last slot,
   not rounded to the stack alignment.

Nothing is back-filled: a register class, once it has sent a piece to
the stack, has no register left (G and V only grow, and only reach 6 and
8 by being used). The exception is `i128` (§4.2), which can go to the
stack while G < 6 and leaves the register for the next piece.

*Evidence:* psABI §3.2.3 "Parameter Passing" (the register sequences);
`table::scalars`; the grid.

### 3.2 A piece

A piece is up to 16 bytes of a value that travel in one register or one
stack slot. Its class (§4) is **general**, **vector** (`half`, `float`,
`fp128`, vector) or **x87**.

- **General.** If G < 6, the piece goes to the G-th argument register
  and G increases by 1. Otherwise it goes to the stack: O is rounded up
  to 8, the piece is at O, O increases by 8.
- **Vector.** If V < 8, the piece goes to `xmm[V]` and V increases by 1.
  Otherwise it goes to the stack: an 8-byte slot aligned to 8 for a
  `half`, `bfloat`, `float` or `double` piece; a 16-byte slot aligned to
  16 for an `fp128` or vector piece.
- **x87.** Always the stack: a 16-byte slot aligned to 16, of which the
  first 10 bytes are the value.

The piece's bytes are at the slot's lowest addresses; the rest of the
slot is unspecified, except as §7 says. (LLVM writes a narrow stack
argument with a store of its own width or of 32 bits, never the whole
slot.)

*Evidence:* `table::scalars`; the grid's `pressures()` put every type
behind 0, 1, 4, 5, 6 used general registers and 6, 7, 8 used vector
registers, and at odd stack offsets.

### 3.3 Blocks

x86-64 has none: no rule ever places the pieces of an aggregate
together, and no attribute (`alignstack` included, §6.4) changes the
place of a by-value argument. Each piece of §4 is placed on its own by
§3.2.

### 3.4 From pieces to parts

Each piece gives one `Value` part (or one `Lanes` part, §4.3): `offset`
is the offset in the argument's value of the piece's first byte, `size`
the number of value bytes it carries, `place` where §3.2 put it. A stack
`Value` part that starts exactly where the previous part of the same
argument ends, both in the value and on the stack, is merged into it.
So `i128` on the stack is one part of 16 bytes and `[3 x i64]` on the
stack one part of 24, while `{ i32, i32 }` on the stack is two parts
(at `+0` and `+8`, 4 bytes each). Padding inside an aggregate belongs to
no part and is not passed.

### 3.5 Examples

All from `table::*`; `s+N` is `Place::Stack(N)`, and `a[o..e]` are
bytes of argument `a`.

| signature | parts | `stack_size` |
|---|---|---|
| `(i32, ptr, double, float, i8, half, bfloat, fp128)` | `rdi`(4), `rsi`, `xmm0`, `xmm1`(4), `rdx`(1), `xmm2`(2), `xmm3`(2), `xmm4`(16) | 0 |
| `(i64 × 6, double × 8, i16, float, i1)` | the registers; s+0 (2), s+8 (4), s+16 (1) | 24 |
| `(double × 8, i8, fp128, x86_fp80, half)` | `xmm0`..`xmm7`, `rdi`; s+0 (16); s+16 (10); s+32 (2) | 40 |
| `(x86_fp80, double)` | s+0 (10), `xmm0` | 16 |
| `(i64 × 5, i128, i64)` | `rdi`..`r8`; s+0 (16); `r9` | 16 |
| `(i64 × 4, i192, i8)` | `rdi`..`rcx`; 4[0..8] `r8`; 4[8..16] `r9`; 4[16..24] s+0; s+8 (1) | 16 |
| `(i64 × 5, [3 x i64])` | `rdi`..`r8`; 5[0..8] `r9`; 5[8..24] s+0 | 16 |
| `({ i8, i32, double }, [3 x float])` | 0[0..1] `rdi`; 0[4..8] `rsi`; 0[8..16] `xmm0`; 1 in `xmm1`..`xmm3`, 4 bytes each | 0 |

## 4. From a type to pieces

`classify(types, ty, features)` is the per-target hook: it turns one
**leaf** type (anything but a structure or an array) into pieces. The
walk over an aggregate's leaves (§4.4) is shared with AArch64 in
spirit; it is a private copy in `hod_x64::abi` until `hod_cg`'s lands
(WP-117).

### 4.1 Scalars

| type | pieces |
|---|---|
| `i1`..`i64`, `ptr` (address space 0) | one general piece of the type's store size |
| `half`, `bfloat` | one vector piece of 2 bytes |
| `float`, `double` | one vector piece of 4, 8 bytes |
| `fp128` | one vector piece of 16 bytes |
| `x86_fp80` | one x87 piece of 10 bytes (as an argument: the stack, §3.2) |
| `ptr` in another address space | `abi:addrspace` |
| a scalable vector, a target extension type | `abi:scalable-vector`, `abi:target-ext-type` |

*Evidence:* `table::scalars`; the grid (every scalar under every
pressure); `02-fp.ll`, `03-x87.ll`.

### 4.2 Integers wider than 64 bits

`iN` with N > 64 is `ceil(N/64)` general pieces of 8 bytes, the last
one `ceil(N/8) − 8(ceil(N/64) − 1)` bytes. They are ordinary pieces:
an `i192` or an `i96` may lie partly in registers and partly on the
stack.

**`i128` alone** (an argument whose own type is `i128`) is the
exception: if two general registers are left (G ≤ 4) it takes the next
two; otherwise it goes to the stack as 16 bytes aligned to 16, and G is
unchanged, so a later general piece still gets the register that was
left. It is never split.

*LLVM 19:* LLVM 19 keeps every value that splits into several general
registers of one IR type whole: an integer wider than 64 bits (`i65`,
`i96`, `i128`, `i192`, `i256`, also as a leaf of an aggregate) and a
vector of `i64` or `ptr` passed element by element (`<3 x i64>`,
`<6 x i64>`, `<3 x ptr>`). If its pieces do not all fit in the general
registers left, all of them go to the stack (8-byte slots; 16 aligned to
16 for an `i128`), and the registers left are marked used, so a later
`i32` goes to the stack too. LLVM 22 splits such a value between `r9`
and the stack, and leaves unused registers for later arguments; for an
`i128` leaf of an aggregate it gives two ordinary pieces where LLVM 19
keeps a pair. This document follows LLVM 22. The two agree whenever
every piece gets a register; the grid runs a signature holding such a
type against LLVM 19 only when nothing of the signature is on the
stack.

*Evidence:* `table::wide_integers`; the grid (the LLVM 19 skip is
`Case::has_wide_general` in `tests/abi/sigs.rs`; run without it, the
systematic grid fails 384 LLVM 19 runs, every one a signature of this
kind with part of it on the stack); `04-i128.ll` and `10-edges.ll`
(`in_struct`), whose `.llc19.s`/`.llc22.s` show the difference.

### 4.3 Fixed vectors

Let N be the length and E the element size in bytes; the element must
be `i8`, `i16`, `i32`, `i64`, `ptr`, `half`, `bfloat`, `float`, `double`,
`fp128`, `x86_fp80`, `i128` or `i1`; any other (`i24`, `i256`, …) is
`abi:vector-element`.

| vector | pieces |
|---|---|
| N = 1, element `half` or `bfloat` | one vector piece of 2 bytes (LLVM widens it: a 16-byte stack slot, and `xmm3` among the return registers, where a `half` takes 8 and `xmm0`..`xmm2`) |
| N = 1, any other element | the element, as a scalar (`<1 x i64>` in a general register, `<1 x float>` in an `xmm`); a lone `<1 x i64>` or `<1 x ptr>` argument or result is one `Lanes { bits: 64, lane: 8, first: 0, len: 1 }` part (`abi::LONE_LANE`, WP-174): its value is an `xmm` and the part a general register or an 8-byte slot, moved with `movq`; as a member of an aggregate it is plain bytes |
| N a power of two, N·E ≤ 16 | one vector piece of N·E bytes (`<2 x float>` widened into one `xmm`) |
| N a power of two, N·E > 16 | N·E/16 vector pieces of 16 bytes (`<8 x float>`: two `xmm`) |
| N not a power of two, `next_pow2(N)`·E = 16 | one vector piece of N·E bytes (`<3 x float>`, `<6 x i16>`) |
| `<3 x half>`, `<3 x bfloat>` | one vector piece of 6 bytes |
| any other N not a power of two | N pieces, one per element, each of the element's class (`<3 x i8>`: three general pieces; `<6 x float>`: six `xmm`; `<3 x i64>`: three general pieces) |
| `<N x fp128>` | N vector pieces of 16 bytes |
| `<N x x86_fp80>` | N `x86_fp80` pieces, as N scalars: an argument a 16-byte stack slot each; a result in `st0` (N = 1), `st0` and `st1` (N = 2), memory (N ≥ 3, §5.2). The elements are 10 bytes apart in the vector's image |
| `<N x i128>` | 2N general pieces of 8 bytes, element `i`'s low half first; no pair rule (§4.2): see below. A result in `rax`:`rdx` (N = 1), memory (N ≥ 2) |

**Vectors of `i128` across the last register** (WP-152, WP-171). LLVM 22
scalarizes the argument into 2N ordinary 8-byte pieces: one may go in
`r9` and the next on the stack, and a stack piece is 8-aligned. LLVM 19
places the **first element** as a lone `i128` (§4.2: both halves in
registers if two are left, else both on the stack at an offset aligned
to 16, the register left marked used) and the pieces after it as
ordinary pieces, exactly as LLVM 22 does (element 1 of `<2 x i128>`
after three `i64`s is `r9` and `s+0` under both). So the two agree
exactly when the first element is in two registers, or is on the stack
from an offset that is a multiple of 16; hod places the vector as LLVM
22 does then. The rest is `x64:vector-i128-split`: the first element's
low half in `r9` (five general registers taken before it) or the vector
on the stack from an offset of 8 mod 16. (`assign` checks the placement
after it places the pieces; the caller and the callee see the same.) Where
kiln's IR has such a vector it comes from a vector of `__int128` (libc++
22's `std::__load_vector` returns a `<1 x i128>`, in `rax`:`rdx`, which
both versions agree on); the refused placements stay refused until one
version can be followed alone.

As a leaf of an aggregate a vector of `i128` is placed piece by piece
(§4.4), as LLVM 22 places it; LLVM 19 keeps its first element a pair
there too (`{ i64, <1 x i128> }` after four `i64`s: LLVM 22 `r9`, `s+0`;
LLVM 19 `s+0`, `s+8`), as it does an `i128` leaf (§4.2).

**Vectors of `i1`:**

| N | pieces |
|---|---|
| 1 | one general piece of 1 byte, as an `i1` |
| 2, 4, 8, 16 | one `Lanes { bits: 1, lane: 16/N, first: 0, len: N }` part in an `xmm` (or a 16-byte stack slot): element `i` is the low bit of lane `i`, the lanes being `i64`, `i32`, `i16`, `i8` |
| any other | `x64:mask-vector` (LLVM passes each element as an `i8`, which no `Part` describes as a whole) |

The other bits of a lane are unspecified: LLVM's producer writes 0 or
1, LLVM's consumer reads the low bit only.

With `avx`/`avx512`, §1.3 applies before this table.

*Evidence:* `table::vectors`; the grid's `vectors()` (every element
type at the lengths above, under every pressure, as returns); `07-vec.ll`,
`10-edges.ll`. `x86_fp80` and `i128` elements:
`hod_x64/tests/leftovers3.rs` (`wide_vectors_and_aligned_byval_interoperate`:
`<1 x i128>` in registers and on the stack, `<2 x i128>` and
`<3 x i128>` all in registers and all on the stack from offset 0,
`<1 x i128>` and `<2 x i128>` results, `<1..3 x x86_fp80>` arguments
and results, callers and callees each from hod or `llc` 22/19;
`wide_vector_and_byval_refusals_by_name`);
`hod_x64/tests/vcall_i128.rs` (WP-171: WP-167's 81 signatures and 52 of
`<1..4 x i128>` after 0 to 10 general arguments, each case compiled
alone, the cases hod builds run hod against `llc` 22 and 19 both ways,
and every case refused as `x64:vector-i128-split` shown to lose a leaf
between `llc` 19 and `llc` 22).

### 4.4 Aggregates

A structure or an array is passed leaf by leaf, in offset order: each
leaf is broken into pieces (§4.1–§4.3, with an `i128` leaf as two
ordinary pieces, §4.2) and each piece placed by §3.2 on its own, at the
leaf's offset in the aggregate. There is no "all in registers or all in
memory" rule: an aggregate may be split between registers and the stack
(`{ i64, i64 }` after five `i64`s: `r9` and `s+0`), and its general and
vector pieces interleave with nothing skipped. Packed structures use
their packed offsets.

*Evidence:* `table::aggregates`; the grid's `aggregates()` under
`pressures()`; `05-aggr.ll`, `10-edges.ll` (`split_pair`).

### 4.5 Types without a leaf

A structure or array with no leaf (`{}`, `[0 x i64]`, `[2 x {}]`) has no
part and changes no state. Later arguments are unaffected.

## 5. Return values

### 5.1 In registers

The return value's type is broken into leaves and pieces as in §4 (an
`i128` is two general pieces). Piece by piece, in offset order:

| class | takes the first free of |
|---|---|
| general | `rax`, `rdx`, `rcx` |
| `half`, `bfloat` | `xmm0`, `xmm1`, `xmm2` |
| `float`, `double` | `xmm0`, `xmm1` (under `fastcc` also `xmm2`), then `st0`, `st1` |
| `fp128` | `xmm0`, `xmm1` |
| vector | `xmm0`, `xmm1`, `xmm2`, `xmm3` |
| x87 (`x86_fp80`) | `st0`, `st1` |

"First free" is over the registers already taken by earlier pieces of
any class: the `xmm` and x87 registers are shared between the rows. If a
piece finds none free, the whole value is returned in memory (§5.2) and
`ret` is empty.

A `float` or `double` piece that lands in `st0` or `st1` is placed there
(WP-140): the part's `size` is 4 or 8 and its bytes are the `float` or
`double` image, which travels converted to the x87's 80-bit format (an
`fld` of that size by the callee, an `fstp` of that size by the caller,
which quiets a signaling NaN). `hod_x64::abi::x87_float_part` names such
a part. The code generator refuses a signature that has one as
`x64:ret-x87-part` (`X64::assign`, not `abi::assign`), and refuses an
`x86_fp80` part in an x87 register as `x64:todo:abi` (`call::guard`,
until WP-131). An `x86_fp80` in `st0`/`st1` is the 10-byte image.

| return type | `ret` |
|---|---|
| `{ i64, double, i32, float }` | [0..8] `rax`; [8..16] `xmm0`; [16..20] `rdx`; [20..24] `xmm1` |
| `{ i128, i64 }`, `i192` | `rax`, `rdx`, `rcx` |
| `<3 x i8>` | `al`, `dl`, `cl` |
| `{ half, half, half }` | `xmm0`, `xmm1`, `xmm2` |
| `<16 x float>` | `xmm0`..`xmm3` |
| `x86_fp80` | `st0` |
| `{ x86_fp80, x86_fp80 }` | `st0`, `st1` |
| `{ x86_fp80, double }` | `st0`, `xmm0` |
| `i256`, `[4 x i64]`, `{ half × 4 }`, `{ fp128 × 3 }`, `{ x86_fp80 × 3 }`, `<32 x float>`, `<6 x float>`, `[2 x i128]` | memory |
| `{ double × 3 }`, `<3 x double>` | `xmm0`, `xmm1`, `st0` |
| `{ i8, double, double, float }` | `al`; `xmm0`, `xmm1`, `st0` |
| `{ float × 4 }` | one leaf each: `xmm0`, `xmm1`, `st0`, `st1` |

`ret_attrs` may hold only `zeroext` or `signext` (§7), and only on an
integer; a memory attribute there is `abi:invalid-signature`, `nest`
`x64:nest`, and the flags of §6.6 report their names.

*Evidence:* `table::returns`; the grid, in which every `pressures()`
type is also a return type, plus `returns_only()` (the table above and
its neighbours) and `conventions()` (the `fastcc` returns);
`table::conventions`; `05-aggr.ll`, `10-edges.ll`, `03-x87.ll`
(`ld_complex`).

### 5.2 In memory

`ret_memory` is `Some(RetMemory { address: Place::Reg(rdi), returned_in:
Some(rax) })`. The caller passes in `rdi` the address of a buffer of
`alloc_size_of(ret)` bytes aligned to `pref_align_of(ret)`; the callee
leaves the value there, as a `store` of it would (padding unspecified),
and returns the same address in `rax`. The hidden address is assigned
before the arguments and takes the first argument register, so every
general argument moves one register on (§3.1).

This is LLVM's "`sret` demotion", and the same convention as a `void`
function with an `sret` first argument (§6.1).

*Evidence:* psABI §3.2.3 "Returning of Values" (class MEMORY: address in
`rdi`, returned in `rax`); `table::returns`; the grid in both directions,
which also checks that the callee hands the address back in `rax`.

## 6. Parameter attributes

An argument with none of these attributes is passed by value (§3).

### 6.1 `sret`

The argument, a `ptr` in address space 0, is an ordinary pointer
argument: one general piece (§3.2), normally `rdi`. The callee returns
its value in `rax` (`sret_returned_in = Some(rax)`); LLVM's callers do
not rely on it, but LLVM's callees provide it, and so must hod's.
`abi:invalid-signature` if the return type is not `void`, if `sret` is on
a parameter after the second, or on anything but a pointer (LLVM's
verifier refuses each).

*Evidence:* `table::attributes`; the grid's `attributes()` (which checks
`rax` after the call); `06-ptrattrs.ll` (`def_sret`).

### 6.2 `byval(T)`

The caller passes a copy of the pointee in the stack-argument area; the
pointer itself is not passed. With `size = alloc_size_of(T)`:

- the alignment `a` is the argument's `alignstack` if present, else its
  `align` if present, else the ABI alignment of `T`, and at least 8;
  `a` above 2^30 or not a power of two is `abi:byval-align`;
- O is rounded up to `a`; the copy is at O; O increases by `max(size, 8)`;
- the argument has one `Pointee` part: offset 0, `size` bytes, at that
  stack place.

No register is used. The callee's parameter value is the address of the
copy, which the callee owns.

**`a` above 16** (WP-152). O is relative to the start of the area,
which the psABI only promises to be 16-aligned; `llc` (19 and 22 agree)
realigns its frame (`and rsp, -a` in the prologue) so that the area is
`a`-aligned, and the callee takes the copy at `rsp + 8 + O` on entry,
trusting it. hod's caller aligns `rsp` just before the call sequence
(`call::align_stack_area`): with `area` the stack size rounded to 16
and `c = area mod a`, `sub rsp, c` · `and rsp, -a` · `add rsp, c`, so
the area the sequence then reserves starts `a`-aligned. `rsp` stays
16-aligned, and the sequence is idempotent: a call in a loop does not
move the stack further. The pointer argument itself must be `a`-aligned
(LLVM copies the pointee with `a`-aligned moves). The argument must be a `ptr` in address
space 0 and `T` sized, else `abi:invalid-signature`; a scalable `T` is
`abi:scalable-vector`.

*Evidence:* `table::attributes` (`byval(%S) align 8` at s+0 and
`byval([3 x double]) align 16` at s+32, as `06-ptrattrs.ll`'s
`f_byval`); the grid's `attributes()` (14 pointee types × 9 alignment
combinations × 3 positions); `variadic::variadic_against_c` (C
structures clang passes `byval`).

### 6.3 `nest`

LLVM passes a `nest` argument in `r10`. It is refused as `x64:nest`
(design 0006 §2.1: `r10` is the code generator's scratch register, and
the corpora have no use of it). A `nest` return attribute is refused
the same way.

*Evidence:* `errors::every_name`; `data::data_signatures_assign`
(`f_nest`).

### 6.4 `byref`, `align`, `alignstack`

- `byref(T)`: an ordinary pointer, passed by value; on anything but a
  pointer `abi:invalid-signature`.
- `align` matters only together with `byval` (§6.2).
- `alignstack` matters only for `byval` (§6.2); on any other argument it
  changes nothing (LLVM ignores it on x86-64).

*Evidence:* `table::attributes`; the grid's `attributes()`
(`alignstack` 1 to 32 on arrays, scalars, vectors and structures).

### 6.5 `inreg`

Ignored: an `inreg` argument or return value is passed as without it.

*Evidence:* `errors::every_name`; `06-ptrattrs.ll` (`f_inreg`) and the
data grid.

### 6.6 Unsupported attributes

| attribute | name |
|---|---|
| `nest` | `x64:nest` (§6.3) |
| `swiftself` | `abi:swiftself` |
| `swifterror` | `abi:swifterror` |
| `swiftasync` | `abi:swiftasync` |
| `inalloca` | `abi:inalloca` |
| `preallocated` | `abi:preallocated` |
| `elementtype` | `abi:elementtype` |

The `swift*` attributes are refused under every convention: LLVM gives
them `r13`, `r12` and `r14` under `swiftcc` only, and `swiftcc` itself
is refused (§1.2). `zeroext` or `signext` on a type that is not an
integer is `abi:invalid-signature`.

*Evidence:* `errors::every_name`.

## 7. Bits above an integer

"Extended to `k` bits" means: bits `N..k` of the register, or of the
stack slot read as a little-endian number, equal bit `N − 1` (by sign)
or are zero (by zero). The **producer** is the caller for an argument
and the callee for a return value; the **consumer** is the other side.
The rules apply to an argument or return value whose own type is an
integer `iN`, passed by value; leaves of aggregates and elements of
vectors carry no promise. For an integer wider than 64 bits they speak
of its last piece.

**Arguments** (`produced_extension(bits, attrs, false)`):

| type and attribute | extend |
|---|---|
| `iN`, no attribute (`i1` included) | nothing |
| `iN zeroext` / `signext`, N < 32 | by zero / sign to 32 bits |
| `i32 zeroext` / `signext` | nothing |
| `iN zeroext` / `signext`, N > 32 | to the next multiple of 64 bits (nothing if N is one) |

**Return values** (`produced_extension(bits, attrs, true)`):

| type and attribute | extend |
|---|---|
| `iN`, no attribute | nothing |
| `i1 zeroext` / `signext` | to 8 bits |
| `i8`, `i16` with either attribute | nothing |
| `iN zeroext` / `signext`, 8 < N < 32 (not 16) | to 32 bits |
| `iN zeroext` / `signext`, N > 32 | to the next multiple of 64 bits |

The consumer may assume exactly what the producer owes
(`assumed_extension`), in a register and on the stack alike, and nothing
more: in particular an `i8 signext` return value is a byte, and a plain
`i1` argument or return value may have anything in bits 1..8 (LLVM's
consumer masks it with `and $1`).

*Evidence:* the grid's `extensions()` (20 widths × both attributes ×
registers, stack, return, variadic): hod's producer does exactly the
minimum of the tables and puts junk in every other bit, and LLVM's
consumer, made to extend what it received, must then show the right
bits; hod's consumer records whole 8-byte units and must find what the
tables promise; `table::extension_tables` pins them row by row.
`01-int.ll` and `11-i1.ll` show LLVM's producers (`movl` for a narrow
stack argument, `movb` for `ret_s8`, `andb $1` for `ret_bool`).

## 8. What the code generator does with an `Assignment`

`assign` only says where; these are the duties that go with it. In the
tests they are implemented by `emit.rs`, an emitter driven by nothing
but an `Assignment`, and checked by the grid in both directions.

### 8.1 A call

1. Reserve the stack-argument area: `stack_size` rounded up to 16, at
   the lowest addresses of the frame, so that `rsp` is its base and is
   16-byte aligned at the `call` instruction (psABI §3.2.2).
2. For each `Value` part, copy `size` bytes of the argument's image from
   `offset` to the part's place, and apply §7. For a `Lanes` part, write
   element `i` to the low bit of lane `i`.
3. For each `Pointee` part, copy `size` bytes from the address the
   argument holds to the part's stack place.
4. If `ret_memory` is set, make the buffer of §5.2 and put its address
   in `rdi`.
5. If the callee is variadic, or may be (an unprototyped call), set `al`
   to `vector_regs_used` (§9.1). Doing so for every call is harmless.
6. After the call, assemble the return value from the `ret` parts or
   find it in the buffer. Pop every x87 part: `st0` first, which makes
   `st1` the new `st0` (the x87 stack must be empty again; psABI
   §3.2.1).

### 8.2 A function's entry

1. Each `Value` part of a parameter is found at its place, with stack
   places relative to `rsp + 8` at entry (`rbp + 16` after `push rbp;
   mov rsp, rbp`).
2. A parameter with a `Pointee` part has as its value the address of
   that part.
3. If `ret_memory` is set, the address in `rdi` must be kept until the
   return, the value stored through it, and the address returned in
   `rax`. If the function has an `sret` parameter, its value is returned
   in `rax`.
4. On return, each `ret` part is put at its place and §7 applied. x87
   parts are pushed `st1` first, so that the x87 stack holds exactly
   the parts.

### 8.3 A variadic function's entry

Besides §8.2, a variadic function that uses `va_start` saves the
argument registers in the register save area (§9.3).

### 8.4 Registers

Argument and return registers an assignment does not name hold nothing
meaningful at a call and on return; `al` is meaningful at a call only
to a variadic callee. A call preserves `rbx`, `rbp`, `rsp`, `r12`..`r15`,
the x87 control word and the MXCSR control bits (psABI §3.2.1, figure
3.4), for both conventions of §1.2; every `xmm` and `rax`, `rcx`,
`rdx`, `rsi`, `rdi`, `r8`..`r11` are clobbered. The x87 stack is empty
at a call and at a return, except for the x87 return parts. The
direction flag is clear at both. `rsp` is 16-byte aligned at every
`call` (so ≡ 8 mod 16 at a function's entry). A hod function saves and
restores only the callee-saved registers it writes (its save set,
`codegen.md` §5.6.15); `rbp` always.

### 8.5 `musttail`

LLVM's verifier requires a `musttail` caller and callee to agree in
convention, variadicness, parameter and return types and in the
attributes that change places (`sret`, `byval`, `inreg`, `alignstack`,
`swift*`, …); it refuses a pair differing in `alignstack`. Of the
attributes that may differ, `zeroext` and `signext` change no place, and
`nest` is refused (§6.3). So the two assignments are equal part for
part, and the callee's arguments fit exactly where the caller's own
arrived.

- The duties of §7 hold for a `musttail` call as for any call.
- With `ret_memory`, the buffer is the caller's own: the address that
  arrived in `rdi` is passed on in `rdi`.
- In a variadic function, a `musttail` call passes on everything the
  caller received: `rdi`..`r9` and all 128 bits of `xmm0`..`xmm7` as far
  as they hold no named argument, `al`, and the stack-argument area,
  exactly as they were at entry. `r10` and `r11` are not passed on.
- Variable arguments of the call before the forwarded `...` (which
  LLVM's verifier accepts) are refused, `x64:musttail-variable-arguments`
  (`hod_x64::call`). `llc` 19 and 22 give them no meaning to follow:
  they place them, then write the forwarded registers over them, so
  the callee receives the caller's entry values in those registers
  (`musttail call (ptr, ...) @printf(ptr %f, i32 42, double 1.5, ...)`
  prints the caller's own second and third arguments, not 42 and 1.5),
  and `al` is passed on as it arrived, not counted. AArch64 places them
  over the forwarded registers (`abi-a64.md` §8.5).

**The stack-argument rule (WP-148).** `classify` decides a `musttail`
call with the facade's hook (`spec/codegen.md` §7.1 "The `musttail`
hook"), which for x86-64 is
`hod_cg::abi::TargetStackArgs::per_function(&X64, hod_x64::call::target_of)`:

1. The target of a function is `hod_x64::call::target_of(module,
   function)`: the features of its `"target-cpu"` and
   `"target-features"` (§1.3) and its coroutine mark, exactly what
   `X64::for_function` gives the code generator for its body.
2. The caller's area (`of_function`) is the `stack_size` of `X64::assign`
   of the caller's own signature (named parameters only, declared
   attributes) with the caller's target.
3. The call's area (`of_call`) is the `stack_size` of `X64::assign` of
   the call (its effective attributes, the variable arguments before a
   forwarded `...` typed by their values) with the target of the
   function the call is in: the caller's, never the callee's.
4. The call is `musttail:stack-args-grow` when the call's area is larger
   than the caller's, both as `assign` gives them (not rounded to 16).
   Since the verifier makes the two signatures agree (above), this
   happens only through variable arguments before the `...` that go to
   the stack. Otherwise the rule accepts the call, and the code
   generator lowers it, or refuses one with such variable arguments as
   `x64:musttail-variable-arguments`.
5. A `byval` aligned above 16 (§6.2) needs no realignment in a
   `musttail` call: the verifier makes the callee's parameter list the
   caller's, its alignment included (`hod_ir`'s verifier rejects a
   differing one as malformed input, as LLVM's does), so each copy goes
   where the caller's arrived, in an area the caller's caller aligned;
   `call::align_stack_area` does not run for the call (WP-152). Growth
   with such a copy is possible only through variable arguments before
   the `...`, refused by the two names of item 4.
6. When `X64::assign` refuses either signature (for example a vector
   wider than 16 bytes under AVX, `x64:wide-vector-abi`, or an
   `x86_fp80` under `-x87`), or the symbol is not a function, the rule
   has no answer and the call is `musttail:stack-args-unknown`.

*Evidence:* `cc::musttail_in_llvm` (LLVM refuses a pair differing in
`alignstack`; its variadic thunk restores `rdi`..`r9`, `xmm0`..`xmm7`
and `al` before its `jmp`, in all six configurations); `09-musttail.ll`;
`crates/hod_x64/tests/musttail_varargs.rs` (the refusal, and `llc`'s
program under `qemu-x86_64` losing the variable arguments);
`crates/hod/tests/musttail_x64.rs` (generated thunks of every class of
§4 and §6 through the facade, the rule against a model of §3 on every
case, those that compile run against `llc` 19 and 22 under bfd, lld and
mold; the features of the calling function deciding);
`crates/hod/tests/musttail_forward.rs` (the three outcomes by text and
bitcode); `crates/hod/tests/musttail_byval_x64.rs` (WP-152: thunks with
`byval` aligned 32 and 64, forwarded, permuted after a clobbering call,
two copies swapped, variadic, from the facade, hod stressed and `llc`
22, with `llc` 22's and hod's callers and callees that trap on a
misaligned copy, under bfd, lld and mold; growth refused by name; a
differing alignment malformed; `llc` 19 is no leg, it stages
`musttail` `byval` copies over the return address).

### 8.6 DWARF register numbers

For `.debug_frame`/`.eh_frame` and location expressions (psABI §3.6.2,
figure 3.36): `rax` 0, `rdx` 1, `rcx` 2, `rbx` 3, `rsi` 4, `rdi` 5, `rbp`
6, `rsp` 7, `r8`..`r15` 8..=15, the return address 16, `xmm0`..`xmm15`
17..=32, `st0`..`st7` 33..=40. These differ from both the hardware
encoding and the `PReg` numbering of §2. `hod_x64::abi` provides
`dwarf_number(reg)`.

*Evidence:* psABI figure 3.36; `table::dwarf_numbers` pins every
register, and `14-frame.llc22.s` shows LLVM's `.cfi_offset` for the
callee-saved registers it pushes.

## 9. Variadic functions

### 9.1 Assignment

A variadic call is assigned exactly as a non-variadic one with the same
argument types: named and variable arguments alike go through §3, in
order, with the same state. `Sig::params` lists the types of all
arguments of the call and `Sig::fixed` the number of named ones; for a
function's own entry `params` holds just the named ones and `fixed`
equals their number.

`vector_regs_used` is V after the last argument: the number of `xmm`
registers the call uses, named and variable. The caller passes it in
`al` (the psABI allows any upper bound up to 8; LLVM writes the exact
count, and so does hod).

*Evidence:* `table::variadic`; `variadic::variadic_against_c` (C
callees whose prologue skips saving `xmm` when `al` is 0); the grid's
variadic cases.

### 9.2 `va_list`

`va_list` is an array of one `{ i32 gp_offset, i32 fp_offset, ptr
overflow_arg_area, ptr reg_save_area }` (24 bytes, aligned to 8),
passed by its address.

### 9.3 The register save area and `va_start`

The register save area is 176 bytes: `rdi`, `rsi`, `rdx`, `rcx`, `r8`,
`r9` at 0, 8, …, 40, then `xmm0`..`xmm7` at 48, 64, …, 160 (all 16
bytes of each). LLVM's prologue stores the `xmm` half only if `al` ≠ 0.
`va_start` writes:

- `gp_offset` = 8 × `VaInfo::gpr_used`;
- `fp_offset` = 48 + 16 × `VaInfo::fpr_used`;
- `overflow_arg_area` = the address of the first variable argument on
  the stack: the callee's stack-argument area plus `VaInfo::stack_offset`;
- `reg_save_area` = the address of the save area.

The `va_arg` instruction is not supported (`inst:va_arg`): clang and kiln
expand it in IR for this target.

### 9.4 `VaInfo`

`VaInfo` is the state after the named arguments: `gpr_used` = G,
`fpr_used` = V, `stack_offset` = O rounded up to 8.

*LLVM 19:* with an `i128` named argument on the stack while general
registers were left, LLVM 19's `gp_offset` is 48 (all used), LLVM 22's
counts only the registers taken (40 after five `i64`s). This follows
LLVM 22 (§4.2).

*Evidence:* `table::variadic`; `variadic::variadic_against_c` (a hod
callee builds the `va_list` from `VaInfo` alone and a C function compiled
by clang and `llc` walks it with `va_arg`; and hod calls C variadic
functions); `08-varargs.ll`.

## 10. Unsupported constructs and limits

`assign` refuses with one of these names (`hod_x64::abi::UNSUPPORTED`):

| name | what |
|---|---|
| `abi:cc` | a convention other than `ccc`, `fastcc` and `x86_64_sysvcc` (§1.2) |
| `abi:invalid-signature` | what LLVM's verifier refuses (§1.2, §5.1, §6) |
| `abi:scalable-vector` | a scalable vector anywhere in a type |
| `abi:target-ext-type` | a target extension type |
| `abi:addrspace` | a pointer outside address space 0 |
| `abi:vector-element` | a vector of an element §4.3 does not list (`i24`, `i256`, …) |
| `x64:vector-i128-split` | a vector of `i128` argument whose first element is partly in registers, or which is on the stack from an offset not a multiple of 16 (§4.3) |
| `x64:mask-vector` | `<N x i1>` with N not 1, 2, 4, 8 or 16 (§4.3) |
| `x64:wide-vector-abi` | a wide vector, or an `i1` vector, under AVX/AVX-512 (§1.3) |
| `x64:nest` | `nest` (§6.3) |
| `abi:swiftself`, `abi:swifterror`, `abi:swiftasync`, `abi:inalloca`, `abi:preallocated`, `abi:elementtype` | §6.6 |
| `abi:byval-align` | a `byval` alignment above 2^30, or not a power of two (§6.2) |
| `abi:too-many-pieces` | more than 2^16 argument pieces in one signature |
| `abi:size-limit` | an offset or size beyond `u32` |

`x64:ret-x87-part` (a `float`/`double` return piece in `st0`/`st1`,
§5.1) is no longer `assign`'s: `X64::assign` reports it.

*Evidence:* `errors::every_name` (each name, and that `UNSUPPORTED`
lists exactly these; `x64:ret-x87-part` from `X64::assign` only); `errors::any_signature_is_answered` (20 000
random signatures over odd types and attributes: an answer or one of the
names, never a panic).

## 11. Validation

### 11.1 The grid

For each signature, the test assembles (AT&T, `clang --target=x86_64-…`)
a hod caller and a hod callee driven by the `Assignment` alone
(`emit.rs`), and has `llc` compile an LLVM caller and callee for the
same IR signature (`ir.rs`). A C driver runs each direction; both sides
record what they received into output blobs, which are compared byte
for byte under the value's mask (padding excluded, §7's bits included).
The producer puts junk (`0xe7…`) in every byte and register bit it does
not owe, and poisons every argument register it does not use. The
random values are any bits except two kinds LLVM may change in transit
while doing nothing wrong: an `x86_fp80` is a normal number (the x87
does not keep an unnormal), and a `half`, `bfloat`, `float` or `double`
NaN is quiet (LLVM may pass a `half` through `float`, and a `float` or
`double` returned in `st0` goes through an x87 load, each of which quiets
a signaling NaN). The C driver, system root and emulator are WP-025's
(`hod_testutil::Target::X86_64`): on an AArch64 host the program is
linked with `x86_64-linux-gnu-gcc` and run under `qemu-x86_64 -cpu
qemu64,-sse3,-cx16,-lahf-lm -L /usr/x86_64-linux-gnu`; on an x86-64 host
natively.

| test | what |
|---|---|
| `grid::systematic_grid` | every type of §4 under every pressure, as argument and return; extensions; attributes; returns; conventions |
| `grid::random_grid` | seeded random signatures (400; 8 000 under `HOD_ABI_GRID=full`) |
| `data::data_signatures_against_llc` | every function of `tests/abi/data/*.ll` |
| `variadic::variadic_against_c` | variable arguments against C walkers (§9) |

Each runs against the six configurations of §1.1.

### 11.2 Cost

`perf::perf_of_assign` prints the time of one `assign` call (a
measurement, not a gate).

## 12. Where LLVM and the psABI part, and where LLVM differs from itself

- **Aggregates.** The psABI classifies a C aggregate by eightbytes and
  passes it wholly in registers or wholly in memory. LLVM passes an IR
  aggregate leaf by leaf and splits it freely (§4.4); clang therefore
  never passes a C aggregate as an IR aggregate, but coerces it (`{ i64,
  double }` becomes `i64, double`) or uses `byval`.
- **Return of three or four floating-point leaves.** The psABI has no
  x87 class for `float`/`double`; LLVM returns the third and fourth such
  leaf in `st0`, `st1` (§5.1). clang never produces such a return type.
- **`i128`.** The psABI's `__int128` is passed in two registers or in
  memory; LLVM does the same for a lone `i128` but not inside an
  aggregate (§4.2), and LLVM 19 and 22 differ there and in the registers
  left behind (§4.2, §9.4). A vector of `i128` is split as 8-byte
  pieces by LLVM 22; LLVM 19 keeps its first element a pair; hod takes
  the placements both agree on and refuses the rest (§4.3,
  `x64:vector-i128-split`).
- **`byval` aligned above 16.** The psABI's stack-argument area is
  16-aligned; LLVM aligns it to the copy's alignment by realigning the
  caller's frame, and the callee relies on it (§6.2).
- **Vectors.** The psABI's `__m128` and `__m256`; LLVM additionally
  splits, widens and scalarizes vector types C has no name for (§4.3).
- **Design 0006 §3's outline.** Where this document and the outline
  differ, this document follows `llc`: an `i128` on the stack leaves
  `r9` usable (LLVM 22; the outline's "r9 stays unused" is LLVM 19);
  `{ i128 }` is two ordinary pieces, not a pair (LLVM 22); the two
  versions do not agree on every rule (§4.2, §9.4); the return table
  has rows for `half`, `fp128` and vectors, and `fastcc` has a third
  `float`/`double` return register (§1.2, §5.1); `<N x i1>` with N
  not 1, 2, 4, 8 or 16 is scalarized (§4.3); `<1 x half>` is widened,
  not scalarized (§4.3); the variadic `musttail` thunk does not forward
  `r10` (§8.5).
