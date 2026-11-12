# 0003: fixed-width vectors as values on AArch64 (WP-076)

This is the design note of WP-076. It counts where vectors appear in the
corpus, sets out what LLVM and AAPCS64 require, and proposes the work
packages that make `type:vector` *supported* for a64. It changes no code,
specs or matrix rows. Section 8 lists the spec sections that later
packages must amend.

State on `main` when this was written: the matrix row `type:vector` is
`todo` for a64 (`crates/hod_ir/src/classify/matrix.rs:674`), so no vector
ever reaches `hod_a64` in the pipeline. Behind that row a good deal is
already lowered and tested directly:

- **WP-039** (`float.rs`): NEON arithmetic on Fpr vectors.
- **WP-067**: vector constants and Fpr loads and stores.
- **WP-040** (`aggr.rs`): Mem vectors as memory images.

Anything else is refused by name: `a64:vector-element`, `a64:vector-op`,
`a64:vector-icmp`, `a64:vector-fcmp`, `a64:vector-select`,
`a64:mem-vector`, `a64:intrinsic-vector`, and the ABI names
`abi:vector-element` and `abi:vector-promoted`.

## 1. Census

Tool: `scripts/vector-census.py`, committed. It is small and
dependency-free and is what later packages use to re-measure. Commands:

```
scripts/vector-census.py --modules .build/vec/modules.tsv .build/corpus   # feature table per group
scripts/vector-census.py --needs .build/corpus < .build/vec/modules.tsv  # V1..V5 needs per module
```

Groups come from MANIFEST.tsv: source, language, `-O` level and LLVM
major. The counts are lexical (lines of `.ll`). The packages in §6 must
re-measure their unlocks with `HOD_CENSUS` on a hod that includes their
change. The hod reasons used below come from a WP-043 build, because
before WP-043 every module stops at `attr:target-features`.

### 1.1 Where vectors appear

| group | modules | LLVM 22 with vectors | LLVM 19 with vectors |
|---|---:|---:|---:|
| gcc-torture | 1709 / 1711 | 36 | 36 |
| kiln C and C++, -O0 | 1392 / 1354 | 31 | 27 |
| kiln C and C++, -O1/-O2 | 42 / 42 | 10 | 10 |
| **total** | | **77** | **73** |

- The corpus has no scalable vectors, no vectors of pointers, and no
  non-byte elements other than `i1`.
- On LLVM 22, 67 of the 77 vector modules have no reason besides vectors
  (64 on LLVM 19). This counts vector forms of scalar intrinsics as
  vector reasons. Without them the count is 63 (60).
- The other ten are blocked by reasons that are not about vectors:
  - 20060420-1: const-expr sub, int-of-addr.
  - pr53645 and pr53645-2: inline asm.
  - pr71626-1 and pr71626-2: an address inside an aggregate constant.
  - f128-wrapper-args: alignstack, extractvalue of fp128.
  - lifetime-markers-O2-g-fpic: debug.
  - run-wide-function-unwind-O2: `eh.typeid.for`.
  - convertvector-const-fold: const-expr bitcast (LLVM 22).
  - image-deptypes: malformed input.
- **kiln C++ needs vectors only where the source asks for them.** At -O0
  every vector module comes from vector extensions or builtins in the
  test: `ext.vector`, `ext.clang.vector_type`, `ext.gnu.*`, the 14
  `target.neon` modules, and C compound literals of vectors. At -O1/-O2
  the SLP vectorizer adds `<2 x i64>`, splat stores of `<16 x i8>`,
  `vector.reduce.add` and one vector phi.
- gcc-torture's `vector_size` tests are about element-wise arithmetic,
  including division and remainder, and about passing vectors as
  arguments. kiln's tests are about element access, shuffles, bitcasts
  and target intrinsics.

### 1.2 STL-heavy C++ at -O0

- libc++ 22's `__algorithm/simd_utils.h` uses `ext_vector_type` at -O0
  under clang-family compilers. kiln defines `__ARM_NEON`, so it takes
  that path too. The header is used by `find`, `mismatch`,
  `lexicographical_compare` and `__locale_dir/num.h`.
- `std::find` and `std::mismatch` over `int`, `char` and `short`, built
  with kiln++ or clang++ 22 against libc++ 22, produce:
  - `<4 x i32>`, `<8 x i16>`, `<16 x i8>` and `<N x i1>`;
  - `icmp`, insertelement chains, `vector.reduce.and` and `.or`, and
    bitcasts from a mask to `iN`;
  - vector arguments and returns of **defined** functions. kiln does not
    inline the `always_inline` `__load_vector`.
- Translation units with only containers or iostreams produce no
  vectors. Neither does anything built against libstdc++ (clang 19
  or 22).
- A sample of the libc++ test suite: 591 tests compiled (36 more failed
  to compile in kiln). Two contain vectors, both in `locale.num.get`
  (`get_long`, `get_long_double`). They use `<32 x i8>` (class Mem),
  `<32 x i1>` and `vector.reduce.or.v32i1`. That is 0.34%, about 30
  tests across the suite. **All of them need only V1+V2 (§6).**

### 1.3 Features, LLVM 22: uses (modules)

| feature | gcc-torture | kiln -O0 | kiln -O1+ |
|---|---|---|---|
| store / load | 486 (36) / 1101 (36) | 2065 (28) / 1686 (30) | 1366 (9) / 352 (7) |
| of which under-aligned | 4 (1) / 0 | 63 (4) / 35 (6) | 1362 (8) / 329 (6) |
| alloca | 288 (34) | 1173 (27) | |
| vector literal / splat constant | 190 (29) | 253 (23) | 1367 (9) |
| zeroinitializer / poison / undef | 45 (15) | 891 (27) | 22 (7) |
| extractelement, constant / variable index | 604 (15) / 1 (1) | 332 (27) / 24 (7) | 1 (1) / 2 (1) |
| insertelement (all with a constant index but one) | 99 (8) | 392 (16) | 10 (6) |
| shufflevector | 12 (3) | 117 (14) | 14 (7) |
| bitcast vec↔vec / vec↔scalar | 5 (3) / 13 (6) | 314 (11) / 68 (9) | 1 (1) |
| define: vector return / param | 19 (12) / 9 (7) | 12 (8) / 7 (5) | |
| call: vector return / arg | 26 (12) / 11 (7) | 16 (8) / 19 (5) | |
| global (with a vector initializer) | 34 (10), 14 (6) | 11 (7), 10 (7) | |
| struct with a vector field | 8 (6) | 22 (8) | |
| add / sub / mul | 16 (7) / 8 (6) / 21 (7) | 9 (3) / 2 (2) / 1 (1) | 345 (3) / 1 (1) / |
| and / or / xor / shifts | 10 / 10 / 17 / 2 | 9 / 4 / 3 / 11 | 0 / 1 / 1 / 2 |
| sdiv / udiv / srem / urem | 24 (6) / 17 (4) / 16 (5) / 12 (2) | | |
| fadd / fsub / fmul / fdiv | 21 / 16 / 16 / 16 (2–4) | 5 / 0 / 1 / 0 | 2 (1) |
| icmp / fcmp (result `<N x i1>`) | 9 (5) / 2 (1) | 19 (4) | 1 (1) |
| sext / zext / trunc / fp casts | 11 (6) / 0 / 0 / 0 | 9 (6) / 5 (3) / 3 (3) / 7 (3) | 1 / 21 / 0 / 0 |
| select with a vector condition | | 1 (1) | |
| phi | | | 3 (1) |
| `vector.reduce.add` / `.or` | | | 6 (4) / 1 (1) |
| vector forms of fma, fabs, sqrt, ceil, floor, trunc, round, roundeven, ctpop, ctlz | | 1–4 each (1–2) | |
| `llvm.aarch64.neon.*` / `.crypto.*` (distinct names) | | 96 (14) | |

Element types and shapes:

- **Most frequent:** `<4 x i32>`, `<2 x i32>`, `<16 x i8>`,
  `<4 x float>`, `<8 x i8>`, `<8 x i16>`, `<8 x i32>`, `<2 x i64>`,
  `<16 x i32>`.
- **Masks:** `<1|2|3|4|8|16 x i1>`. The most common is `<8 x i1>` (97
  uses in 5 modules). Mask idioms:
  - bitcast `<8 x i1>` ↔ `i8` (27 and 24 uses);
  - shufflevector `<4 x i1>` → `<8 x i1>` (21) and back (16);
  - sext of masks, bitcast `i16` → `<16 x i1>`, extractelement of mask
    elements.
- **Shapes outside the D/Q classes:**
  - one element: `<1 x i32>`, `<1 x float>`, `<1 x i128>` (pr105613);
  - three elements: `<3 x float>`, `<3 x i32>`, `<3 x i64>`;
  - wide: `<32 x i8>`, `<64 x i8>`;
  - promoted: `<2 x i8>`, `<4 x i8>`, `<2 x i16>`;
  - `<4|8 x half>` and `<4|8 x bfloat>`.
- **Vector types in signatures:**

  | type | uses |
  |---|---:|
  | `<4 x i32>` | 75 |
  | `<4 x float>` | 22 |
  | `<2 x i32>` | 21 |
  | `<16 x i8>` | 21 |
  | `<2 x i64>` | 10 |
  | `<2 x float>` | 8 |
  | `<4 x i1>` | 7 |
  | `<8 x i8>` | 6 |
  | `<4 x i8>` | 6 |
  | `<1 x i32>` | 5 |
  | `<1 x i64>` | 4 |
  | `[1 x <4 x float>]` | 2 |
  | `<8 x i32>` | 2 |

- **Shufflevector masks:**

  | kind | count |
  |---|---:|
  | identity or resize | 84 |
  | splat | 27 |
  | one-source permute with a resize | 16 |
  | one-source permute, same length | 5 |
  | two-source | 8 |

## 2. Semantics

LangRef rules a lowering must keep, and how hod's IR describes them
(`spec/ir.md` §4):

- **Layout.**
  - An element with a whole number of bytes sits at `i·b` in memory.
  - A vector of sub-byte elements is stored as one little-endian integer
    of `N·bits` bits, with element 0 in the lowest bits. Its store size
    is rounded up to bytes, and the extra bits are zero when stored.
    llc writes `and #0x7` before `strb` for `<3 x i1>`.
  - A bitcast between a vector and an integer, or between two vectors,
    is this in-memory image, so it is a copy for byte elements.
- **Element-wise operations** act per lane with the scalar rules:
  - poison per lane;
  - division and remainder are UB if any lane divides by zero or
    overflows;
  - shift amounts are per lane, and a lane whose amount is at least the
    element width is poison;
  - nsw/nuw/exact flags are per lane.
- **`icmp`/`fcmp`** produce `<N x i1>`. **`select`** with a vector
  condition chooses per lane.
- **`extractelement`/`insertelement`** with an index ≥ N give poison.
  The lowering must never read or write out of bounds, so the index is
  masked or clamped.
- **`shufflevector`** takes a constant mask whose entries are an index
  into the concatenation of both sources or `poison`. Results take the
  mask's length.
- **`vector.reduce.fadd`/`fmul`** are ordered (sequential, starting from
  the start value) unless `reassoc` is set. hod always reduces in order,
  which is correct for both. The integer reductions, including
  `and/or/xor/smax/smin/umax/umin` over `i1`, are ordinary folds.
- **`undef`/`poison` lanes** may be any value. hod makes them 0, as it
  does for scalars (codegen.md §5.1).

`hod_interp` (`spec/interp.md` §3, §8, §11) has no vectors, `half` or
`fp128`. It refuses with `type:vector` (`interp.rs:694`, `:1305`). What
it lacks:

- a vector value, represented as its byte image (bit image for `i1`);
- element-wise evaluation through the existing scalar operations;
- compares, select, extract/insert/shuffle;
- reductions and the vector forms of scalar intrinsics.

Package V0 adds these (§6). Because there is no `half`, half vectors keep
llc as their only oracle.

## 3. ABI

### 3.1 What AAPCS64 and LLVM do

These were observed with llc 19 and 22 at `-O0`, using both
`-global-isel=0 -fast-isel=0` and the default GlobalISel. The two
selectors agree on every observable layout. Scratch files are under
`.build/target-wp-076/vec/abi/`.

| type | argument | return |
|---|---|---|
| 64- or 128-bit vectors (byte elements) | one D or Q register (an HFVA member) | v0 |
| `<3 x i32>`, `<3 x float>` | one Q register (widened) | v0 |
| `<8 x i64>`, `<16 x i32>`, `<32 x i32>` | several Q registers in a row, up to v7 | v0.. |
| `<2 x i8>`, `<2 x i16>` | D register as `2s` (promoted) | same |
| `<4 x i8>` | D register as `4h`. The caller writes `ldr s` + `ushll .8h`, and the callee answers with `uzp1` | same |
| `<3 x i8>` | one W register per element (w0..w2) | same |
| `<1 x i1>` | `w0` bit 0 (DAG sbfx, GISel and; callee `and #1`) | same |
| `<2|4|8|16 x i1>` | v0 as `2s`/`4h`/`8b`/`16b`, one lane per element, only bit 0 meaningful | same. Producer: `cmgt` + `xtn`. Consumer: `shl #(w-1)` + `cmlt` |
| `<3 x i1>` | w0..w2, one element per W register | same |
| `<32 x i1>` | one element per W register. Elements past w7 go to 8-byte stack slots (element 17 at `sp+72`) | bit-packed into the x8 buffer |
| `<64 x i1>` | as `<32 x i1>` | bit-packed into the x8 buffer |
| promoted vector on the stack | an 8-byte slot holding the D image (`<4 x i1>` at `sp+0` as `4h`; `<4 x i8>` at `sp+8` as `4h`) | — |

### 3.2 What hod has today

`hod_a64::abi::assign` follows `spec/abi-a64.md` §4.3 through
`vector_leaf` (`crates/hod_a64/src/abi.rs:402`). That covers:

- D, Q and multi-Q vectors;
- one-element vectors;
- widening of shapes whose length is not a power of two;
- one piece per element otherwise.

It refuses:

- integer vectors under 8 bytes with `abi:vector-promoted`;
- `i1` vectors with `abi:vector-element`.

The reason is hod_cg's `Part` (`spec/codegen.md` §7.2). A part is a byte
range of the value placed in a register or slot. It cannot say "lane i
of the register holds byte i of the value, widened". The promoted and
mask rows need that.

## 4. Design

### 4.1 Value class (codegen.md §2–§4)

- **Fpr (unchanged):** a power-of-two length, a size of 64 or 128 bits,
  and an element that is not `i1`. This is `vector_class`,
  `target.rs:107`.
- **Mask:** `<N x i1>` with N ≤ 64 becomes class **Gpr** and is
  bit-packed like `iN`:
  - bit i is element i, and the bits above N are unspecified, as for
    `iN`;
  - a bitcast to or from `iN` is a copy;
  - load and store are those of `iN`, with the extra bits zeroed on
    store as for `iN`;
  - compares produce it with a NEON compare followed by a pack, or one
    `cset`/`bfi` per lane; selects consume it with `tst` per lane, or an
    unpack to lanes followed by `bsl`;
  - this is the representation LLVM uses in memory, and it is what
    libc++'s `simd_utils` does with masks (bitcast to `iN`, then
    `ctz`). A copy is one move.
- **Masks with N > 64** stay **Mem**. A bit-packed image in a slot keeps
  element operations open for later; for now they are refused as
  `a64:mask-wide`.
- **Mem (unchanged):** every other vector, as an image in a slot.

The mask class needs one hook at the top of `target.rs::lower`. Every
instruction with a mask operand or result goes to `mask.rs` *before* the
generic scalar paths. Without the hook, `int.rs` would lower `add` or
`icmp` of a Gpr mask as an integer operation, which is wrong.

Lane semantics in `mask.rs` (one bit per lane):

| operation | bit result |
|---|---|
| add, sub | eor |
| mul | and |
| shifts | the lhs (a shift by ≥ 1 is poison) |
| smax | and |
| smin | or |
| `reduce.add` | parity (xor) |
| `reduce.and` | `cmp` against the all-ones mask of N bits |
| `reduce.or` | `tst` |

### 4.2 Lowering strategy

The order of priority is correctness, then emitter speed, then code
size.

1. **Keep the WP-039 NEON paths** for Fpr vectors and the operations
   that map to one instruction. Emitting one instruction is the fastest
   path, both to emit and at run time.
2. **Add one universal lane loop** (`lanes.rs`). It covers every Mem
   vector operation and the Fpr leftovers:
   - division and remainder;
   - `i64` mul;
   - half arithmetic, by promotion per lane;
   - casts with no single instruction;
   - every vector intrinsic with a scalar form.

   How it works:
   - The loop is unrolled at emit time.
   - For each lane it loads the operand element from the image, either
     a frame slot or the Fpr value spilled to its home, using
     `x16`/`x17` and pinned temporaries.
   - It reuses the *scalar* lowering of the element operation (int.rs,
     float.rs, fpintr.rs entry points that take registers) and stores
     the result element.
   - This gives one implementation per operation for every shape, and
     it is correct by construction from the scalar code, which is
     already tested against llc.
   - Its cost is about 3 instructions plus the scalar operation per
     lane. The cap is `MAX_LANES = 256`; longer vectors are refused as
     `a64:vector-length`. No corpus vector has more than 64 lanes.
3. **Element access** (V1):
   - `extractelement` and `insertelement` with a constant index:
     `umov`/`smov`/`ins` or `dup_elem` for Fpr, and a load or store at
     `i·b` for Mem.
   - With a variable index, the element is addressed in the image after
     `and` (power-of-two N) or `csel`-clamping the index. Out-of-range
     lanes are poison, so any lane is a correct result.
   - `shufflevector`: `dup_elem` for a splat. Otherwise one `ins_elem`
     per lane for Fpr, or one element copy per lane for Mem. Masks go
     through `mask.rs` with `ubfx`/`bfi`. `tbl` is not used: building
     the index vector costs a constant load and is slower to emit.
4. **Fusions** are allowed only where they are local. For example,
   `icmp` whose only use is `sext` or a `select` on Fpr operands becomes
   `cm*` (+ `bsl`), with no mask in between. They are optional and come
   after V2 is correct.

`hod_mc_a64` (`spec/mc-a64.md`) already has every encoding that V1–V4
need:

- `dup`, `ins`, `umov`/`smov`, `movi`;
- `cm*`, `bsl`, `xtn`, `shl`/`sshr`, `uzp1`, `ushll`/`sshll`;
- `addv`/`umaxv`/`uminv`.

Missing, and needed only by V5 or by optional fusions:

- crypto and dot product;
- LD1–4/ST1–4;
- TBX, tbl3/tbl4;
- saturating, halving and pairwise operations;
- SADDLV and the FP reductions;
- rounding and narrowing shifts;
- widening multiplies;
- by-element forms;
- FP16 arithmetic.

### 4.3 Legalization at calls

The ABI legalizes at the call boundary only. Inside a function, a
vector keeps its class from §4.1. At a call or a `ret`, the target hooks
of V4 convert:

- mask (Gpr bits) ↔ N lanes of a D/Q register (`2s`/`4h`/`8b`/`16b`),
  using `ubfx` + `ins` per lane, or `dup`+`cmtst` with a constant of the
  lane bits;
- mask ↔ one W per element (`<1|3 x i1>`, `<32|64 x i1>` arguments, with
  stack slots past w7);
- mask ↔ a packed x8 buffer (`<32|64 x i1>` returns);
- promoted Mem or Fpr image ↔ widened D lanes (`ushll`/`uzp1` or a lane
  loop);
- `<3 x i8>` and similar ↔ one W per element.

hod_cg needs a new part kind, `PartKind::Lanes { elem, lane }` or an
equivalent: a part that the target fills from the value's image through
a hook, not by a byte copy. This changes the interface in
`spec/codegen.md` §7.2, so V4 owns `hod_cg::abi` as well as
`hod_a64::abi`.

### 4.4 Stays refused

| form | name | why |
|---|---|---|
| scalable vectors | `type:scalable-vector` (matrix), `abi:scalable-vector` | no SVE; no corpus use |
| vectors of pointers, `getelementptr` over them | `a64:ptradd-vector` | no corpus use |
| non-byte elements other than `i1` (`<N x i4>`) | `a64:vector-odd-element` | no corpus use |
| `i128` and `fp128` elements | `a64:vector-wide-element` | one module (pr105613); add to V3 when there is a second |
| `bfloat` arithmetic | `a64:vector-bfloat` | loads, stores and bitcasts work; arithmetic is not in the corpus |
| vector `frem` | `a64:vector-op` | no corpus use; it would be a lane loop calling `fmod` |
| masks with more than 64 lanes, element operations | `a64:mask-wide` | only as libc++ `<32 x i1>`; 64 suffices |
| vectors over 256 lanes | `a64:vector-length` | emit-time cap |
| `llvm.aarch64.neon.*`, `.crypto.*` | `a64:intrinsic-target` | until V5 |

## 5. Oracle plan

- **Differential against llc.** This is the main oracle.
  - llc's DAG at `-O0 -global-isel=0 -fast-isel=0` runs on the same `.ll`.
  - Test functions take their vectors through pointers (`load`, op,
    `store`), so V1–V3 can be tested before V4.
  - The harnesses are those of `crates/hod_a64/tests/float.rs` and the
    ABI grid.
  - The checks are run-and-compare of lane images, plus the poison
    rule: any value is accepted where the result is poison.
- **gcc as a second oracle.** C with `vector_size` typedefs, built with
  gcc, gives an independent check of the ABI rows of §3.1 through
  mixed-compiler calls.
- **hod_interp** after V0. Mutation comparison runs every new lowering
  against the interpreter on random lane values, including the boundary
  values of division and shifts.
- **Corpus.** Every package re-runs `scripts/vector-census.py --needs`
  and `HOD_CENSUS`. The module counts in §6 are its acceptance figures,
  as numbers to re-measure and not promises. Corpus verify must stay
  green.
- **Mutations.** Each package names three mutations its tests must
  catch. Examples:
  - lane order reversed;
  - `sshr` swapped with `ushr`;
  - the bits above N not zeroed on a mask store.

## 6. Work packages

Rows are in BACKLOG format, ordered by modules unlocked. Counts are for
LLVM 22, out of the 67 vector-only modules (LLVM 19 in brackets).

| id | track | scope (files owned) | deps |
|---|---|---|---|
| WP-V1 (base) | T-A64 | Flip `type:vector` and the element rows in `matrix.rs` for a64. Add a new `crates/hod_a64/src/vector.rs` with the shared helpers (vector shape, lane address in an image, operand image of Fpr or Mem) and extract/insert/shuffle for byte elements. Amend codegen.md §5.5.1. **33 (32)**; 15 need only the flip. | — |
| WP-V3 (lane loop) | T-A64 | Add `crates/hod_a64/src/lanes.rs` with element-wise arithmetic and casts on Mem vectors and the Fpr leftovers, and the vector forms of scalar intrinsics through `intrinsic.rs`/`fpintr.rs` dispatch. V1+V3: **46 (45)**. Without V3, all five reach only 48. | V1's helpers |
| WP-V2 (masks) | T-A64 | Add `crates/hod_a64/src/mask.rs` and the dispatch hook in `target.rs::lower`: the Gpr class for `<N x i1>`, compares (Fpr operands through NEON then a pack; Mem through lanes), vector-condition select, mask casts, shuffles and reductions. Change `vector_class`. This is the libc++ `simd_utils` path. V1+V2: **37 (36)**; V1+V2+V3: **52 (50)**. | V1's helpers; V3 for Mem compares |
| WP-V5 (NEON intrinsics) | T-A64 + T-IR + mc | Add the `llvm.aarch64.neon.*`/`.crypto.*` table to the intrinsic list (ir.md) and the missing encodings (mc-a64.md). 11 modules, all kiln `target.neon`. V1+V2+V3+V5: **63 (62)**. | V1–V3 |
| WP-V4 (vector ABI) | T-A64 + hod_cg | `PartKind::Lanes` (or equivalent) in `hod_cg::abi` with target hooks to widen and narrow, and the mask and promoted rows of §3.1 in `hod_a64::abi`. Amend abi-a64.md §4.3 and codegen.md §7.2. 4 modules; all five: **67 (64)**. | V1, V2 |
| WP-V0 (interp vectors) | T-H | Vectors in `hod_interp` (a value is its image), checked against llc. Amend interp.md §3, §4, §8, §11. | none; parallel with everything |

**Parallel work.** V0 runs in parallel with all of them. V1 is small and
goes first; it should land its helpers early, as its own first commit.
V2, V3 and V4 then own disjoint files (`mask.rs`, `lanes.rs`, `abi.rs`
and `hod_cg::abi`). The only shared point is the dispatch in
`target.rs`, which each package touches in one line.

How many modules need each package (LLVM 22, out of 67):

| package | modules |
|---|---:|
| V2 | 11 |
| V3 | 19 |
| V4 | 4 |
| V5 | 11 |

**What V1 must settle first:**

- the helper API that V2–V4 build on: shape, lane address and operand
  image;
- the home of an Fpr vector when a lane operation needs its image (spill
  to the value's slot, or a scratch slot);
- the poison-index rule (mask with `and`, or clamp);
- the list of refusal names in §4.4, so that later packages add rows
  without renaming;
- the re-measurement with `HOD_CENSUS` of the 15 modules that need only
  the flip.

## 7. Open questions

1. **Mask representation.** The choices are Gpr bit-packed, Mem image,
   or LLVM-style lanes in a D/Q register.
   *Recommendation: Gpr bit-packed for N ≤ 64.* It matches the memory
   image and the `iN` bitcasts libc++ uses, and copies are one move.
   Lanes would make every bitcast and store a pack anyway.
2. **Lane loop versus NEON per operation.** *Recommendation: one
   unrolled lane loop with a cap of 256 lanes, reusing the scalar
   lowering.* It is correct by construction. NEON stays only where
   WP-039 already has it.
3. **The V4 interface change to hod_cg `Part`.** *Recommendation: a
   `Lanes` part kind filled through target hooks, decided in V4 together
   with the hod_cg owner.* Encoding promotion as byte parts is not
   possible.
4. **NEON target intrinsics (V5).** *Recommendation: defer.* They appear
   only in kiln's own `target.neon` tests (96 names, 11 modules) and in
   neither libc++ nor the torture suite.
5. **kiln issues found.** *Recommendation: report them to the kiln
   agents; hod does not depend on either.*
   - A dependent `ext_vector_type` in an alias template is mangled as
     `u7unknown`. For example, kiln gives
     `_ZNSt3__18__all_ofB9nqe220108IiLm4EEEbu7unknown` where clang gives
     `...DvT0__T_`. It is seen in `conformance/temp.dep/splice-type.cpp.ll`
     and `temp.names/splice-type.cpp.ll`.
   - kiln does not inline `always_inline` at -O0, so libc++'s
     `__load_vector` has vector arguments and returns. That makes V4
     part of the libc++ path only if a mask crosses a call; in the
     sample, none did.
6. **Wide masks (`<32 x i1>`, `<64 x i1>`).** *Recommendation: they are
   inside the Gpr class (N ≤ 64). Their ABI rows go in V4. Anything
   wider stays `a64:mask-wide`.*
7. **Half vectors.** The interpreter has no `half`.
   *Recommendation: lane loop with promotion to `float`, checked against
   llc only, until hod_interp gains `half`.*

## 8. Spec sections to amend (by package)

- `spec/codegen.md`:
  - §2.2, the classes bullet (V2);
  - §5.5.1, new headings for vector element access, masks and lanes
    (V1–V3);
  - §7.2, part kinds (V4).
- `spec/abi-a64.md` §4.3 and §9: mask and promoted rows, and the
  removal of `abi:vector-element` and `abi:vector-promoted` (V4).
- `spec/interp.md` §3, §4, §8, §11 (V0).
- `spec/ir.md`, the intrinsic list: `aarch64.neon` and the masked family
  if they are adopted (V5).
- `spec/mc-a64.md` "Not provided" (V5).
- `crates/hod_ir/src/classify/matrix.rs` rows and
  `docs/status/support.md` (each package, for its own rows).
