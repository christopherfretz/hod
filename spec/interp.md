# `hod_interp`: the reference interpreter and the program generator

Crate: `crates/hod_interp`. Depends on `hod_ir` only. Test infrastructure:
it is never part of a shipped binary.

"LangRef" below is the LLVM Language Reference Manual of LLVM 22; sections
are cited by their titles. "`ir.md`" is `spec/ir.md`.

## 1. Purpose and scope

The crate has two parts.

- An **interpreter** for function bodies of `hod_ir` (§2..§8). It is the
  oracle for the code generator: compiled code is correct when it does what
  the interpreter does.
- A **generator** of random programs that the interpreter runs to a result
  (§9), so that a test of the code generator needs no hand-written input
  and no LLVM.

The interpreter obeys one rule, the **determinism rule**:

> A run has a result only if every correct compilation of the program has
> that same result. Otherwise the run stops with a named **trap**.

So wherever LangRef says "undefined behavior", "poison", "undef" or
"unspecified" and a compiler could turn that into observably different
programs, the interpreter traps instead of choosing. The one exception is
stated in §5.7: `undef` and `poison` *constants* are zero, as `ir.md` §4.1
defines for hod.

The interpreter is itself checked against LLVM (§10).

It is single-threaded, little-endian, and has 64-bit pointers. It models
C++ exceptions of the Itanium ABI (§5.9), computed gotos (§5.5) and
fixed-width vectors (§3.1, §5.10). It does
not model variadic calls, thread-local storage, `callbr` or inline
assembly: those are **unsupported** (§8), which is a third kind of answer,
neither a result nor a trap.

The interpreter is a library with no global state. Nothing derivable from
its input makes it panic: a body `Body::verify` accepts leads to a result,
a trap, or unsupported; anything else leads to `Stop::Invalid`.

Test: `tests/robustness.rs` (`any_verified_body_gets_an_answer`).

## 2. Programs and runs

```rust
pub struct Program<'m>;            // a module and the bodies of its functions
impl<'m> Program<'m> {
    pub fn new(module: &'m Module) -> Program<'m>;
    pub fn with_bodies(module: &'m Module, bodies: impl IntoIterator<Item = &'m Body>)
        -> Result<Program<'m>, Stop>;
    pub fn add_body(&mut self, body: &'m Body) -> Result<(), Stop>;
    pub fn module(&self) -> &'m Module;
    pub fn body(&self, sym: SymId) -> Option<&'m Body>;
    pub fn bodies(&self) -> impl Iterator<Item = &'m Body>;
    pub fn verify(&self) -> IrResult<()>;      // Body::verify of every body
}

pub struct Interp<'a> { pub config: Config, /* hosts */ }
impl<'a> Interp<'a> {
    pub fn new(program: &'a Program<'a>) -> Interp<'a>;
    pub fn with_config(self, config: Config) -> Interp<'a>;
    pub fn host(&mut self, name: &str,
        f: impl FnMut(&mut HostCx<'_>, &[Val]) -> Result<Option<Val>, Trap> + 'a) -> &mut Interp<'a>;
    pub fn run(&mut self, entry: SymId, args: &[Val]) -> Outcome;
}

pub struct Outcome {
    pub result: Result<Option<Val>, Stop>,   // Ok(None): a `void` function returned
    pub trace: Vec<Event>,                   // host calls, in order (§6)
    pub steps: u64,                          // §7
    pub max_depth: u32,                      // deepest nesting of frames; the entry is 1
    pub peak_memory: u64,                    // most live bytes (§4)
    pub stopped_at: Option<(SymId, InstId)>, // where a trap or unsupported stop happened
}
impl Outcome { pub fn trace_text(&self) -> String; }   // one `Event` per line

pub enum Stop { Trap(Trap), Unsupported(String), Invalid(String) }
```

- `add_body` refuses (`Stop::Invalid`) a body whose symbol is not a function
  of the module with the body's function type, and a second body for one
  function.
- A function is defined for a run if the program has a body for it, or else
  if a host function of its name is registered (§6). A body wins over a host
  function of the same name.
- **A run is independent of every other run.** Each `run` builds memory
  afresh (§4): module variables have their initial values again. Only the
  state inside host closures survives.
- `run` is `Stop::Invalid` when `entry` has no body in the program, when the
  number of `args` is not the function's, or when an argument is not of its
  parameter's type (§3).
- `Stop::Invalid` is also the answer to a body that breaks an invariant
  `Body::verify` checks (operands of two widths, a `phi` without an entry
  for the edge taken, ...). The text is for people and is not stable.
- The interpreter never recurses on the host stack: calls push a frame onto
  an explicit stack, so 100,000 nested calls need only the configured depth.

Tests: `tests/calls.rs` (`invalid_runs`, `call_depth`,
`standard_hosts_and_the_trace`), `tests/memory.rs` (`module_variables`).

## 3. Values

```rust
pub enum Val {
    Int { bits: u32, value: u128 },   // 1..=128 bits; `value` has no bit above `bits`
    Ptr(u64),
    Float(u32),                       // IEEE-754 binary32 bit pattern
    Double(u64),                      // IEEE-754 binary64 bit pattern
    Agg(Agg),                         // an array, a struct or a vector (§3.1): its type and memory image
}
pub const MAX_INT_BITS: u32 = 128;
pub type Vector = Agg;
pub enum Lane { Int(u32), Ptr, Float, Double }
```

`Val::int(bits, value)` masks `value` to `bits`; `i1` .. `i128`, `f32`,
`f64` construct, `as_int`, `as_signed`, `as_u64`, `as_ptr`, `as_f32`,
`as_f64` read. Floats are compared and carried as bit patterns, so `-0.0`
and `0.0` are different values.

An aggregate value is the byte image of its type as `ir.md` §3 lays it out
(fields at `TypeTable::field_offsets`, array elements at multiples of the
element's allocation size), with padding bytes zero. `Agg::ty()`,
`Agg::bytes()`.

The types a value may have: `iN` for N ≤ 128, `ptr` (any address space is
accepted as a type; `addrspacecast` is unsupported), `float`, `double`,
arrays and structs of these, and the vectors of §3.1. Every other type is
unsupported (§8).

Tests: `tests/memory.rs` (`aggregates`), unit tests of `value.rs`.

### 3.1 Vectors

A fixed-width vector `<N x T>` (WP-078) is a `Val::Agg` whose type is the
vector type, and like every aggregate it is **its memory image** (design
0003 §1): the image LLVM stores for it on a little-endian target.

- **Lanes** (`Lane`): `T` is `iN` with N a multiple of 8 up to 128, `i1`,
  `ptr`, `float` or `double`. `N` is 1 or more; the image must fit
  `Config::max_memory` (else `limit-memory`).
- **Layout.** Lane `i` of a byte-sized lane is at byte `i × size(T)`, low
  byte first (`<4 x i32> <1, 2, 3, 4>` is `01 00 00 00 02 00 ...`). An
  `<N x i1>` mask is **bit-packed**: lane `i` is bit `i % 8` of byte
  `i / 8` (bit 0 the least significant), the image is ⌈N/8⌉ bytes, and the
  bits above lane N-1 are zero. So `<3 x i1> <1, 0, 1>` is `05`, `<13 x
  i1>` alternating from 1 is `55 15`, and `bitcast <8 x i1> to i8` is the
  byte. Evidence: compiled by `llc` 19 and 22 (`-O0 -global-isel=0
  -fast-isel=0`) and run, a store of `<3 x i1> <1, 0, 1>` writes `05` and
  of the alternating `<13 x i1>` writes `55 15`, a load of `01 12` as
  `<13 x i1>` has lanes 0, 9 and 12 set, `bitcast <8 x i1> <1, 0, 0, 1, 0,
  0, 0, 1>` to `i8` is `0x89`, and a global `<3 x i1> <1, 0, 1>` is the
  byte `05`. `tests/vectors.rs` (`layout_in_memory`) has the 13-lane store,
  the bitcasts and the padding rule; the oracle (§10.3) checks thousands of
  mask stores, loads and bitcasts more.
- **Poison per lane.** A vector carries a poison flag per lane
  (`is_poison(i)`, `has_poison()`; `lane(i)` is `None` for one). The bytes
  of a poison lane are zero in the image. A poison lane is created only by
  the rules of §5.10 and is **observed**, trapping `poison-lane`, only where
  §5.10 says.
- `Agg::is_vector()`, `lane_kind()`, `lane_count()`, `lane(i)`,
  `from_lanes(types, ty, &[Option<Val>])` (`None` a poison lane) and
  `from_image(types, ty, bytes)` read and build them; `bytes()` is the
  image. A vector prints as `<i32 1, poison,
  i32 3>`.
- **Odd lanes are refused**, `type:vector-odd-element`: an `iN` lane with N
  neither 1 nor a multiple of 8 (`<4 x i3>`, `<2 x i33>`). LangRef packs
  them like masks, but `llc` 19 and 22 store `trunc <4 x i8> <1, 2, 7, 253>
  to <4 x i3>` at SelectionDAG `-O0` as bytes that are not that packing
  (while `<2 x i33>` comes out packed), so no image could be proven; no
  front end emits them.

Tests: `tests/vectors.rs` (`layout_in_memory`, `vectors_as_values`), unit
tests of `vector.rs`.

## 4. Memory

Memory is a set of **allocations**. An allocation has an address, a size, an
alignment, a kind and, for every byte, a **written** flag.

| kind | made | ends |
|---|---|---|
| variable | at the start of a run, for every module variable with an initializer | never |
| constant variable | the same, for a variable that is `constant` | never |
| stack | when a frame is pushed, for every stack slot of the body; by `alloca` | when the frame that made it returns |
| function | at the start of a run, for every function except an undefined `extern_weak` one | never |
| declared variable | at the start of a run, after all the others, for every variable without an initializer (a type-information object, `errno`): one address, no bytes; an access traps as `extern:<name>` (§8) | never |
| (reserved address) | for every `blockaddress` constant at the start of a run, and for every exception thrown: an identity in no allocation; an access is `out-of-bounds` | never |

- **Addresses.** `Config::addr_base` is below the first allocation;
  consecutive allocations are at least `Config::addr_gap` bytes apart, each
  aligned as asked and occupying at least one address. Addresses only grow
  and are **never reused**: the addresses of a returned frame stay dead for
  the rest of the run. Null is address 0 and is in no allocation. A function
  has one address and no bytes; distinct functions have distinct addresses.
- **An access** (a load, a store, each end of a `memcpy`, a host function's
  read or write) of `len` bytes at `addr` with alignment `align` is checked
  in this order:
  1. The whole range must be inside one live data allocation: else
     `out-of-bounds`, or `use-after-free` when `addr` is in a dead stack
     range. Function addresses are not data. An access of zero bytes is
     not checked at all (LangRef "'llvm.memcpy' Intrinsic": with a length
     of 0 the pointers may be anything).
  2. `align` must be at most the allocation's alignment and divide `addr`:
     else `misaligned`. The alignment of an access is a promise the
     compiler may use (LangRef "'load' Instruction": "the behavior is
     undefined if the address is not aligned"), and whether an address that
     the allocation does not guarantee happens to be aligned depends on
     layout: so it is the *allocation's* alignment that counts.
  3. A write to a constant variable is `write-to-constant` (LangRef "Global
     Variables": writing to a constant is undefined behavior).
  4. A read of a byte whose written flag is clear is `uninitialized-load`
     (LangRef "Undefined Values": such a load is `undef`).
- **Written flags.** Stack memory starts unwritten. A variable's bytes are
  written where its initializer has a scalar and unwritten in the padding
  between fields. A store of a scalar writes its bytes. A store of an
  aggregate writes the bytes of its scalars and **clears** the flags of the
  padding between them (LangRef "'store' Instruction": padding of a stored
  aggregate is undefined). `memcpy` and `memmove` copy the flags with the
  bytes; `memset` sets them.
- **Scalars in memory** (LangRef "Data Layout", little-endian):
  an `iN` occupies ⌈N/8⌉ bytes, low byte first, the bits above N zero;
  `ptr` 8 bytes; `float` 4; `double` 8. A load of an `iN` whose N is not a
  multiple of 8 from bytes whose high bits are not zero is
  `non-zero-padding-bits` (LangRef "'load' Instruction": the result is
  undefined when the memory was not written by a store of the same type).
- **Vectors in memory** (§3.1): a load or store is one access of the
  image's bytes. A load is lenient per lane, as LangRef's `load` makes an
  unwritten byte `undef` and not undefined behavior: a lane with a byte
  whose written flag is clear is **poison** instead of `uninitialized-load`;
  a mask whose last byte was written with bits above the lanes set is
  `non-zero-padding-bits`. A store writes the image and then **clears** the
  written flags of every byte that holds a poison lane (for a mask, the
  whole byte of a poison bit), so a scalar load of them traps
  `uninitialized-load` and a vector load gives the poison back.
- **Limit.** The live bytes of variables and stack never exceed
  `Config::max_memory`: an allocation that would is `limit-memory` (§7).
- **Module variables.** The initializer is flattened by
  `ConstPool::flatten` (`ir.md` §4.3); address fix-ups take the addresses of
  this run. A variable without an initializer (a declaration) has no
  allocation: taking its address is unsupported `extern:<name>`. The
  module's constructor list is not run.

`Memory` is public so that host functions can use it: `read(addr, len)`,
`write(addr, data)` (alignment 1, the rules above), `live_bytes()`,
`peak_bytes()`.

A program can observe addresses (`ptrtoint`, a comparison of pointers to
different allocations, a pointer passed to a host function). Such a result
depends on `addr_base` and `addr_gap`, and the interpreter does **not**
detect it (§12). Callers that need address-independence run twice with two
layouts and compare, as `tests/generate.rs` does.

Tests: `tests/memory.rs` (`scalars_are_little_endian_bytes`,
`odd_width_integers_in_memory`, `out_of_bounds`, `uninitialized_loads`,
`alignment`, `use_after_return`, `module_variables`), unit tests of
`memory.rs`.

## 5. Semantics

Every instruction of `ir.md` §5.4 is listed here or in §8. "Traps `x`"
names a `Trap` by its stable name (`Trap::name`).

```rust
pub enum Trap { /* one variant per name below, and the three limits of §7 */ }
impl Trap { pub const ALL: &[Trap]; pub const fn name(self) -> &'static str;
            pub const fn is_limit(self) -> bool; }
```

| name | when |
|---|---|
| `division-by-zero` | §5.1 |
| `division-overflow` | §5.1 |
| `shift-too-large` | §5.1 |
| `float-to-int-overflow` | §5.3 |
| `nan-result` | §5.1 |
| `intrinsic-poison` | §5.6 |
| `poison-lane` | §5.10 |
| `out-of-bounds`, `use-after-free`, `misaligned`, `uninitialized-load`, `non-zero-padding-bits`, `write-to-constant` | §4 |
| `overlapping-copy` | §5.6 |
| `undefined-value` | §5.5 |
| `unreachable` | §5.5 |
| `llvm.trap` | §5.6 |
| `not-a-function`, `function-type-mismatch`, `calling-convention-mismatch`, `noreturn-returned` | §5.8 |
| `host` | §6 |
| `limit-fuel`, `limit-call-depth`, `limit-memory` | §7 |

### 5.1 Arithmetic

Integers (LangRef "Binary Operations", "Bitwise Binary Operations"), at any
width 1..=128, results modulo 2^N:

- `add`, `sub`, `mul`, `and`, `or`, `xor`: as named. `hod_ir` carries no
  `nuw`/`nsw`/`disjoint` flags, so there is no poison to model.
- `udiv`, `urem`: unsigned; traps `division-by-zero` when the divisor is 0.
- `sdiv`, `srem`: signed, quotient rounded toward zero, remainder with the
  sign of the dividend; traps `division-by-zero`, and `division-overflow`
  for the smallest value divided by -1 (LangRef: undefined behavior, for
  `srem` too).
- `shl`, `lshr`, `ashr`: traps `shift-too-large` when the amount (unsigned)
  is the width or more (LangRef: poison).

Floats (LangRef "Binary Operations", "Floating-Point Environment"): `fadd`,
`fsub`, `fmul`, `fdiv` are the IEEE-754 operations in the default
environment (round to nearest even, no traps); `frem` is C's `fmod`; `fneg`
flips the sign bit and nothing else. There are no fast-math flags in
`hod_ir`.

**NaN.** LangRef "Behavior of Floating-Point NaN values" leaves the sign and
payload of a NaN that an operation produces to the implementation. So, with
`Config::nan == NanPolicy::Trap` (the default), an operation that *computes*
a float (`fadd` .. `frem`, `fptrunc`, `fpext`, `sqrt`,
`floor`, `ceil`, `trunc`, `round`, `roundeven`, `rint`, `nearbyint`, `fma`,
the float `atomicrmw` operations) traps `nan-result` when its result is a
NaN, whatever its operands. Operations that only move bits (`fneg`, `fabs`,
`copysign`, `select`, `phi`, `load`, `store`, `bitcast`) and compares never
trap for a NaN. `NanPolicy::Host` returns what the host's arithmetic
produces instead; such a result is not covered by the determinism rule.

Tests: `tests/instructions.rs` (`integer_arithmetic_wraps_at_the_width`,
`integer_traps`, `float_arithmetic_is_ieee`,
`computed_nan_traps_by_default`), unit tests of `ops.rs`.

**`hod_interp::ops` is public** (WP-044): `int_binary`, `icmp`, `fcmp`,
`float_to_int`, `int_intrinsic` with `IntOp`, `checked` with `Checked`,
`three_way`. They are the arithmetic of this section and of §5.6 on
`u128` bit patterns of a given width, as the interpreter itself uses them;
a test of another crate may state its expectations with them. They are an
interface: a change of behaviour is a change of this section.

### 5.2 Compares and `select`

- `icmp` (LangRef "'icmp' Instruction"): the ten predicates on two integers
  of one width, or on two pointers, which compare as their addresses.
  Result `i1`.
- `fcmp` (LangRef "'fcmp' Instruction"): the sixteen predicates; "ordered"
  is "neither operand is a NaN"; `-0.0` equals `0.0`.
- `select` (LangRef "'select' Instruction"): the condition is an `i1`; the
  result is the chosen operand of any supported type.

Test: `tests/instructions.rs` (`compares_and_select`).

### 5.3 Casts

LangRef "Conversion Operations".

| | |
|---|---|
| `trunc`, `zext`, `sext` | as named |
| `fptrunc`, `fpext` | IEEE conversion, round to nearest even; NaN policy of §5.1 |
| `uitofp`, `sitofp` | IEEE conversion from the unsigned or signed value, round to nearest even |
| `fptoui`, `fptosi` | toward zero; traps `float-to-int-overflow` when the truncated value does not fit the result type, or the operand is a NaN or an infinity (LangRef: poison) |
| `ptrtoint`, `ptrtoaddr` | the address, truncated or zero-extended to the result width |
| `inttoptr` | the integer, truncated or zero-extended to 64 bits. There is no provenance: the pointer may be used wherever its address may (§12) |
| `bitcast` | between `float` and `i32`, `double` and `i64`, a type and itself: the same bits |
| `addrspacecast` | unsupported |

Tests: `tests/instructions.rs` (`integer_casts`, `float_casts`,
`pointer_casts`, `narrow_values_stay_narrow`).

### 5.4 Memory instructions

LangRef "Memory Access and Addressing Operations", "Aggregate Operations".

- `load`, `store`: an access (§4) of the type's bytes with the
  instruction's alignment. An aggregate is loaded and stored scalar by
  scalar at its field offsets; the padding of a loaded aggregate is zero in
  the value. `volatile` and the atomic orderings change nothing.
- A stack slot's address is the frame's allocation for it. Each activation
  of a function has its own.
- `alloca` (dynamic): a stack allocation of `count * elem_size` bytes, live
  until the function returns; `limit-memory` if the product overflows.
- `ptradd`: `base + offset + Σ sext64(index_i) * scale_i`, modulo 2^64; an
  index wider than 64 bits is truncated
  (LangRef "'getelementptr' Instruction", with the offsets already computed
  as `ir.md` §5.4 says). No bounds are checked here: `hod_ir` carries no
  `inbounds`, so only an access through the result is checked.
- `fence`: nothing.
- `cmpxchg`: loads; if the loaded value equals `expected`, stores
  `replacement`; the result is the pair of the loaded value and the `i1`
  "was equal". A `weak` one behaves as a strong one: it never fails
  spuriously (allowed by LangRef "'cmpxchg' Instruction", and the only
  deterministic choice).
- `atomicrmw`: loads `old`, stores `op(old, value)`, results in `old`. The
  operations: `xchg`, `add`, `sub`, `and`, `nand`, `or`, `xor`, `max`,
  `min`, `umax`, `umin`, `fadd`, `fsub` (NaN policy of §5.1), `uinc_wrap`,
  `udec_wrap`, `usub_cond`, `usub_sat` as LangRef "'atomicrmw' Instruction"
  defines. `fmax`, `fmin`, `fmaximum`, `fminimum` are unsupported.
- `extractvalue`, `insertvalue`: the scalar or aggregate at the byte offset
  of the image (§3).
- `extractelement`, `insertelement`, `shufflevector`, and every other
  instruction on vectors: §5.10.

Tests: `tests/memory.rs` (`ptr_add_indices`, `dynamic_allocation`,
`aggregates`, `atomics_are_sequential`).

### 5.5 Control flow and `phi`

LangRef "Terminator Instructions", "'phi' Instruction".

- Execution starts at the first instruction of the entry block after its
  `phi`s and goes instruction by instruction.
- `br`, `condbr` (on an `i1`), `switch` (the first case equal to the
  operand, else the default) **take an edge** from the current block to the
  target.
- Taking an edge into a block evaluates all of the block's `phi`s **in
  parallel**: every `phi` reads the operand it lists for the source block
  with the values as they were before the edge, then all are assigned. A
  `phi` that reads another `phi` of the same block therefore sees the old
  value (two `phi`s that read each other swap), and a block that branches
  to itself re-reads its own `phi`s. When the source block is listed more
  than once (both edges of a `condbr`, several `switch` cases), the entries
  agree by `Body::verify`'s rule and the first is used.
- `ret` ends the frame: its stack allocations die, the caller's call
  instruction gets the value. A `ret` from the entry function ends the run
  with that value.
- `unreachable` traps `unreachable`.
- **A use of an instruction result that has not been computed in this
  activation traps `undefined-value`.** `Body::verify` does not check that
  definitions dominate uses; the interpreter does not either, it traps when
  an execution actually reads such a value. Values do not survive from one
  activation to the next.
- **`blockaddress(@f, %b)`** is a pointer: at the start of a run each
  `blockaddress` constant of the module gets a reserved address of its own
  (§4), so two of one block are equal and two of different blocks differ.
  It may be stored, loaded, put in a variable's initializer (a table) and
  passed around. LangRef ("Addresses of Basic Blocks") defines it only as
  an operand of `indirectbr` and in a comparison with `null`: **an `icmp`
  of one with any other non-null pointer, and a `ptrtoint` of one, trap
  `blockaddress-inspected`** (`llc -O2` folds an equality of two block
  addresses as it likes; the address itself is the layout's).
- `indirectbr addr, [targets]` takes the edge to the block `addr` is the
  address of, if that block is of the current function and one of
  `targets`; any other address (of another function's block, of a slot, a
  block not listed) traps `indirectbr-target`.
- `invoke`, `landingpad` and `resume`: §5.9. `callbr` and the funclet
  instructions are unsupported.

Tests: `tests/instructions.rs` (`phis_are_a_parallel_copy`,
`switch_and_repeated_successors`, `unreachable_and_undefined_values`),
`tests/block_addr.rs` (`computed_goto_through_a_table`,
`inspecting_a_block_address`, `indirectbr_off_its_list`).

### 5.6 Intrinsics

LangRef "Intrinsic Functions". Each is defined at every integer width
1..=128 unless noted.

| intrinsic | meaning |
|---|---|
| `llvm.memcpy`, `llvm.memcpy.inline` | copies `len` bytes and their written flags; traps `overlapping-copy` when the ranges overlap without being the same range (LangRef "'llvm.memcpy' Intrinsic"). Each pointer is an access of `len` bytes with the alignment of its `align` attribute, 1 without |
| `llvm.memmove` | the same, any overlap allowed |
| `llvm.memset`, `llvm.memset.inline` | writes `len` copies of the byte |
| `llvm.abs` | traps `intrinsic-poison` for the smallest value when the flag is true; otherwise the smallest value is its own `abs` |
| `llvm.smax`, `smin`, `umax`, `umin` | as named |
| `llvm.ctpop`, `llvm.bitreverse` | as named |
| `llvm.bswap` | widths that are multiples of 16 |
| `llvm.ctlz`, `llvm.cttz` | of 0 the width, or `intrinsic-poison` when the flag is true |
| `llvm.fshl`, `llvm.fshr` | the funnel shifts; the amount is taken modulo the width |
| `llvm.sadd.sat`, `uadd.sat`, `ssub.sat`, `usub.sat` | saturating |
| `llvm.sadd.with.overflow`, `uadd`, `ssub`, `usub`, `smul`, `umul` | the pair of the wrapped result and the `i1` "it overflowed" |
| `llvm.scmp`, `llvm.ucmp` | -1, 0 or 1 in the result type |
| `llvm.fabs`, `llvm.copysign` | bit operations |
| `llvm.sqrt`, `floor`, `ceil`, `trunc`, `round`, `roundeven`, `rint`, `nearbyint` | the correctly rounded IEEE operations; `round` rounds halves away from zero, the last three to even (the default rounding mode is the only one). NaN policy of §5.1, so `sqrt` of a negative number traps |
| `llvm.fma` | fused: one rounding |
| `llvm.trap` | traps `llvm.trap` |
| the no-code intrinsics of `ir.md` §5.5 | those that are dropped (`llvm.lifetime.*`, `llvm.assume`, `llvm.donothing`, `llvm.dbg.*`, ...) do nothing, and a result is zero; those that pass their first argument through (`llvm.expect`, `llvm.launder.invariant.group`, ...) return it |

Every other intrinsic is unsupported, `intrinsic:<name>` (§8).

Tests: `tests/calls.rs` (`integer_intrinsics`, `checked_arithmetic`,
`float_and_other_intrinsics`), `tests/memory.rs` (`memory_intrinsics`).

### 5.7 `undef`, `poison`, `freeze`

`ir.md` §4.1 defines `undef` and `poison` constants as zero for hod: a
compiler from `hod_ir` must materialize them so. The interpreter does the
same, so this is not a place where compilations differ. Consequently
`freeze` copies its operand (LangRef "'freeze' Instruction").

Poison that an operation would *produce* is a trap (§5.1, §5.3, §5.6).
Unwritten memory is never read (§4). Vectors are the exception: there it
is a poison lane, observed only later, and `freeze` keeps it (§5.10).

Test: `tests/instructions.rs` (`undef_is_zero_and_freeze_copies`).

### 5.8 Calls

LangRef "'call' Instruction".

- The callee is a function symbol (a direct call) or a pointer. A pointer
  must be exactly the address of a function: else `not-a-function`.
- The call's function type must be the callee's type: else
  `function-type-mismatch`. Its calling convention must be the callee's:
  else `calling-convention-mismatch` (LangRef: both are undefined
  behavior).
- A callee with a body gets a new frame: its stack slots are allocated, its
  arguments are the evaluated operands. More frames than
  `Config::max_call_depth` is `limit-call-depth`.
- A callee without a body is a host function (§6), or unsupported
  `extern:<name>`.
- A call marked `noreturn` that returns traps `noreturn-returned`.
- Tail-call markers change nothing. Parameter attributes change nothing,
  except `byval`, `inalloca` and `preallocated`, which are unsupported.
  Variadic callees, inline assembly callees and operand bundles are
  unsupported.

Tests: `tests/calls.rs` (`direct_calls_and_recursion`, `indirect_calls`,
`void_calls_and_unused_results`, `call_depth`).

### 5.9 Exceptions

LangRef "Exception Handling in LLVM", "'invoke'", "'landingpad'",
"'resume'"; the Itanium C++ ABI's "Exception Handling" (the two-phase
unwind of its level I, the personality of its level II). LLVM leaves what
a landing pad receives to the personality routine; the model is that of
**`__gxx_personality_v0`**, the one personality it knows, as libstdc++
and libc++abi implement it. A function whose frame an exception reaches
and whose personality is another is unsupported `eh:personality`.

**Throwing and catching.** `hod_interp::eh` names two functions that the
interpreter provides itself when the program has no body for them (they
are not host functions and leave no event in the trace):

- `void hod_throw(ptr type, i64 payload)` throws an exception. `type` is
  the address of a **type-information object**, which is the exception's
  type identity; `payload` is its value. A null `type` traps `host`.
- `i64 hod_catch(ptr exception)` catches the exception a handler's
  landing pad received and returns its payload: `__cxa_begin_catch`
  followed by `__cxa_end_catch`. Anything that is not an exception waiting
  in its handler traps `not-an-exception`.

`eh::declare(&mut Module)` declares both, the personality, and four
type-information objects of the C++ runtime (`_ZTIi`, `_ZTIl`, `_ZTIj`,
`_ZTIs`: `int`, `long`, `unsigned`, `short`) as external constants. For
these the personality's type match is pointer identity, which is the
model's. (Class types with bases, pointer adjustments and `catch` by
reference are a C++ runtime's matter, outside what LLVM IR decides.)

**Clauses.** A function's type ids are 1, 2, ... in the order of first
mention over its landing pads' clauses (pads in block order), then over
its `llvm.eh.typeid.for` operands. A `catch @ti` clause matches an
exception of type `@ti`, `catch null` every exception; its selector is the
type id of `@ti` (of `null`). A `filter [n x ptr] [...]` clause matches an
exception whose type is *not* listed; its selector is negative, -1 for the
function's first filter, -2 for the second, .... A filter's elements must
be type-information addresses (`eh:filter-catch-all` for a null one,
`eh:filter-form` for a form other than an array). `llvm.eh.typeid.for(@ti)`
is `@ti`'s type id. **Only two facts about a selector are the program's to
rely on**: whether it equals `llvm.eh.typeid.for` of a type, and its sign
(LLVM numbers ids per function differently, and its inliner merges
clauses: §10.2).

**The pad's decision.** For an exception of type `t`, a landing pad's
clauses are tried in order and the first that matches is a **handler**
with its selector; if none matches, the pad is a **cleanup** (selector 0)
if it has the `cleanup` flag, and otherwise does **nothing**.

**Unwinding.** A throw is two phases, as in the Itanium ABI:

1. **Search.** From the frame that called `hod_throw` outwards, each frame
   is at a call; if that call is an `invoke`, its landing pad's decision is
   taken. The first frame whose pad is a handler is the **handler frame**.
   If there is none, the run traps `uncaught-exception`: `__cxa_throw`
   calls `std::terminate`, and **no cleanup runs** (nothing printed by a
   cleanup is in the trace).
2. **Cleanup.** From the same frame outwards again: a frame at a `call`, or
   at an `invoke` whose pad does nothing, is popped (its slots die, §4); a
   frame whose pad is a cleanup is entered there with selector 0; the
   handler frame is entered at its pad with the handler's selector.

Entering a pad takes the `invoke`'s unwind edge (§5.5: the pad's `phi`s
read their entries for the `invoke`'s block); its `landingpad` yields the
pair `{ ptr exception, i32 selector }`, `exception` being a reserved
address (§4) that names this exception. A `landingpad` reached otherwise
than by unwinding is `Stop::Invalid` (the verifier admits no such edge).

`resume { ptr, i32 }` continues the cleanup phase of the exception the
pair names, from the frame of the function that resumes: it is popped and
the next frame outwards decides. A pair that names no exception waiting in
a cleanup (one already caught, one made up) traps `not-an-exception`.

The **outcome of an exception leaving the outermost frame** is always
defined: the search phase finds no handler and the run traps
`uncaught-exception` at the throw, before any cleanup.

**When the phases disagree.** Phase 2 normally meets exactly what phase 1
saw. It may not when a cleanup changes the frames in between, for
instance by calling, from the cleanup, a function that resumes. (A
cleanup that returns or branches away instead of resuming abandons the
exception; that is no disagreement and the run goes on.) If phase 2 meets a handler where phase 1 found none,
libstdc++ installs it and libc++abi calls `std::terminate`; if it does not
meet the handler frame at the call phase 1 saw, or a `resume` has no
handler frame left outside it, the result depends on the runtime too. All
of these trap `eh-phase-mismatch`.

**`nounwind`.** An exception that would leave a `call` or `invoke` marked
`nounwind`, or a function marked `nounwind`, traps `unwind-from-nounwind`
(LangRef: undefined behavior; C++ compilers make it `std::terminate`
through a pad, hod_cg's code would unwind on).

**Cost.** Each frame the search looks at and each frame the cleanup
phase pops is one step (§7).

Tests: `tests/unwind.rs` (`catch_clauses_in_order`, `filters`,
`typeid_for`, `cleanup_then_resume`, `uncaught_runs_no_cleanup`,
`nounwind_violations`, `shared_pad_and_nested`, `invoke_returns`,
`misuse`, `null_type`); the cases of `tests/eh_cases` against LLVM
(§10.2).

### 5.10 Vectors

LangRef "Vector Type", "Vector Operations", "'llvm.vector.reduce.*'
Intrinsics" (WP-078; the census of design 0003 §1). A vector operation
applies the scalar rule of §5.1..§5.6 to each lane, lane 0 first, through
the same kernels, with one difference: **what the scalar rules call poison
makes the lane poison instead of trapping**, and a poison lane is
**observed** (traps `poison-lane`) only where a scalar would be made of it.
Undefined behavior in any lane traps as for a scalar, and the first lane
that traps is the trap.

| construct | meaning |
|---|---|
| `add` .. `xor`, `shl`, `lshr`, `ashr` | per lane; a poison operand lane gives a poison lane; a shift amount of the width or more gives a poison lane (scalar: `shift-too-large`) |
| `udiv`, `sdiv`, `urem`, `srem` | per lane; a divisor lane that is 0 **or poison** traps `division-by-zero` (LangRef: undefined behavior either way); a signed division by -1 traps `division-overflow` when the dividend lane is the smallest value or poison |
| `fadd` .. `frem`, `fneg` | per lane, NaN policy of §5.1 (a computed NaN in any lane traps `nan-result`) |
| `icmp`, `fcmp` | `<N x i1>`; on pointer lanes as for scalars (§5.2, §5.5's `blockaddress-inspected`) |
| `select` | with an `i1` condition, the chosen vector whole; with an `<N x i1>` condition, lane by lane, and a poison condition lane gives a poison lane |
| casts | `trunc`, `zext`, `sext`, `fptrunc`, `fpext`, `uitofp`, `sitofp`, `fptoui`, `fptosi`, `ptrtoint`, `inttoptr` per lane (an `fptoui`/`fptosi` that does not fit gives a poison lane); `bitcast` between a vector and a scalar or vector of the same number of bits is the image (§3.1): a poison lane makes every result lane that holds one of its bits poison, and a bitcast to a scalar of a vector with a poison lane is observed |
| `extractelement` | the lane; an index (unsigned, any width) out of range, or a poison lane, is observed |
| `insertelement` | the vector with the lane replaced; an index out of range makes every lane poison (LangRef: poison) |
| `shufflevector` | lane `i` of the result is lane `mask[i]` of the two operands concatenated; a `poison` mask entry (`SHUFFLE_UNDEF`) gives a poison lane |
| `freeze` | **keeps** poison lanes: each compilation may fix one to a different value, so there is still no one result (§12) |
| `load`, `store`, `phi`, call arguments and returns, `extractvalue` of a vector field | as any value (§4 for memory). `insertvalue` of a vector with a poison lane into an aggregate is observed |
| `llvm.vector.reduce.add`, `mul`, `and`, `or`, `xor`, `smax`, `smin`, `umax`, `umin` | the fold of the integer lanes in order, result the lane type; any poison lane is observed |
| `llvm.vector.reduce.fadd`, `fmul` | ordered: `((start op v0) op v1) ...`, NaN policy per step (`hod_ir` has no `reassoc`); any poison lane is observed |
| `llvm.vector.extract`, `llvm.vector.insert` | the subvector at a lane index that is a multiple of its length; past the end the result is all poison |
| the integer intrinsics of §5.6 on vectors | `abs`, `smax` .. `umin`, `ctpop`, `ctlz`, `cttz`, `bitreverse`, `bswap`, `fshl`, `fshr`, the saturating four, `scmp`, `ucmp`, per lane; the `i1` flag of `abs`/`ctlz`/`cttz` is one scalar for all lanes; what §5.6 calls `intrinsic-poison` gives a poison lane |
| the float intrinsics of §5.6 on vectors | `fabs`, `copysign`, `sqrt`, `floor`, `ceil`, `trunc`, `round`, `roundeven`, `rint`, `nearbyint`, `fma`, per lane, NaN policy of §5.1 |

`hod_ir` carries no `nsw`/`nuw`/`exact` flags, so no lane is poison by a
flag. Refused (§8): vector `getelementptr` (`inst:ptradd-vector`), vector
`atomicrmw` (`inst:atomicrmw-vector`), vector forms of the `with.overflow`
intrinsics (`intrinsic-vector:<name>`), and every vector intrinsic not
listed, by its scalar rule (`intrinsic:<name>`: `llvm.vector.reduce.fmax`,
the masked loads and stores, `llvm.experimental.*`, ...).

Tests: `tests/vectors.rs` (`integer_lanes_and_poison`,
`float_lanes_and_casts`, `compares_and_selects`,
`element_access_and_shuffles`, `reductions`, `vector_intrinsics`,
`vectors_as_values`, `truncation_of_wide_lanes`, `refused_vector_forms`),
`tests/vector_oracle.rs` (§10.3).

## 6. Host functions and the trace

A host function stands for a function the module declares and does not
define.

```rust
interp.host("name", |cx: &mut HostCx<'_>, args: &[Val]| -> Result<Option<Val>, Trap> { ... });

pub struct HostCx<'r> {
    pub memory: &'r mut Memory,   // to follow pointer arguments (§4)
    pub index: u64,               // completed calls of this host function in this run
}
pub struct Event { pub name: Arc<str>, pub args: Vec<Val>, pub result: Option<Val> }
```

- Registration is by symbol name. Registering a name again replaces the
  earlier function. A name the module does not declare is ignored.
- Every completed call appends an `Event` to `Outcome::trace`. The trace is
  the program's observable behavior besides its result: two runs agree when
  result and trace are equal. `Event` displays as `name(i64 1, ptr 0x10040)`
  or `name() = i64 7`.
- A host function returns `Err(trap)` to stop the run with that trap
  (`Trap::Host` is for "I refuse these arguments"). Returning a value of the
  wrong type for the declaration is `Stop::Invalid`.
- A host function must be deterministic in its arguments, the memory it
  reads and `cx.index`, or the determinism rule is void.

### 6.1 The standard host functions

```rust
pub mod host {
    pub const SINK: &str = "sink";       // void sink(i64)
    pub const SOURCE: &str = "source";   // i64 source()
    pub struct Standard { pub sink: SymId, pub sink_ty: TypeId, pub source: SymId, pub source_ty: TypeId }
    pub fn declare(module: &mut Module) -> IrResult<Standard>;
    pub fn register(interp: &mut Interp<'_>);
    pub const fn source_value(index: u64) -> u64;
}
```

- `sink(x)` does nothing; the call is in the trace.
- `source()` returns `source_value(cx.index)`: the outputs of SplitMix64
  seeded with zero. With `z = (index + 1) * 0x9e3779b97f4a7c15`,
  `z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9`,
  `z = (z ^ (z >> 27)) * 0x94d049bb133111eb`, the value is `z ^ (z >> 31)`,
  all modulo 2^64. The first three are `0xe220a8397b1dcdaf`,
  `0x6e789e6aa1b965f4`, `0x06c45d188009454f`.

Tests: `tests/calls.rs` (`standard_hosts_and_the_trace`, `host_functions`),
unit test of `host.rs`.

## 7. Limits

```rust
pub struct Config {
    pub fuel: u64,             // default 10_000_000
    pub max_call_depth: u32,   // default 1000
    pub max_memory: u64,       // default 64 MiB
    pub addr_base: u64,        // default 0x1_0000 (§4)
    pub addr_gap: u64,         // default 64 (§4)
    pub nan: NanPolicy,        // default Trap (§5.1)
}
```

- **Steps.** Every executed instruction is one step, and every `phi`
  evaluated on an edge taken is one step. A call is one step (the callee's
  steps are its own); a host call is one step. A run that would take step
  `fuel + 1` traps `limit-fuel` with `Outcome::steps == fuel`. A run of
  exactly `fuel` steps completes.
- **Depth.** The entry function is depth 1. A call that would make the
  depth exceed `max_call_depth` traps `limit-call-depth`.
- **Memory.** §4.

The three limit traps (`Trap::is_limit`) say nothing about the program:
a test that meets one must raise the limit or call the run inconclusive.

Tests: `tests/calls.rs` (`fuel`, `call_depth`), `tests/memory.rs`
(`memory_limit`).

## 8. Unsupported

`Stop::Unsupported(reason)`: the program uses something this interpreter
does not implement. The reason is a stable string `group:name`. It is
reported **when execution reaches** the construct (or needs the type, the
constant or the symbol), not for its mere presence, with two exceptions:
the two `target:` reasons and `global:initializer` are found when a run
starts.

| reason | when |
|---|---|
| `target:pointer-width` | pointers are not 64 bits |
| `target:big-endian` | the target is big-endian |
| `type:half`, `type:bfloat`, `type:fp128`, `type:x86_fp80` | a value of that type, or a vector of lanes of it |
| `type:vector-odd-element` | a vector whose lanes are `iN` with N neither 1 nor a multiple of 8 (§3.1) |
| `type:scalable-vector` | a `<vscale x N x T>` value |
| `type:vector` | a vector of lanes that are not of §3.1 and have no reason of their own. Defensive: `hod_ir` makes no such type |
| `inst:ptradd-vector`, `inst:atomicrmw-vector` | a vector `getelementptr`; a vector `atomicrmw` (§5.10) |
| `intrinsic-vector:<name>` | a vector form of a `with.overflow` intrinsic (§5.10) |
| `type:int-over-128` | an integer wider than 128 bits |
| `type:target-ext`, `type:token` | a value of that type; the `none` token constant |
| `type:opaque`, `type:function`, `type:label`, `type:metadata` | a value of that type. The builder does not make such values: defensive |
| `inst:va_arg`, `inst:callbr`, `inst:addrspacecast` | the instruction |
| `eh:personality` | an exception reaches a frame whose personality is not `__gxx_personality_v0` (§5.9) |
| `eh:filter-form`, `eh:filter-catch-all` | a `filter` clause that is not an array of type-information addresses |
| `inst:cleanuppad`, `inst:catchpad`, `inst:catchswitch`, `inst:catchret`, `inst:cleanupret` | the funclet instructions |
| `inst:atomicrmw-fmax`, `-fmin`, `-fmaximum`, `-fminimum` | that `atomicrmw` operation |
| `intrinsic:<name>` | any intrinsic not in §5.6: the base name for one `hod_ir` knows (`intrinsic:llvm.stacksave`, `intrinsic:llvm.fmuladd`, `intrinsic:llvm.va_start`), the full name for one it does not |
| `const:symdiff` | that constant |
| `const-expr:<op>` | a constant expression that `hod_ir` could not fold (`const-expr:add`) |
| `const:aggregate` | an aggregate constant used as a value that cannot be flattened |
| `global:initializer` | a variable's initializer cannot be flattened, or is not as large as its type |
| `global:thread-local`, `global:alias`, `global:ifunc` | the address of such a symbol |
| `global:extern-weak` | the address of an undefined `extern_weak` function (it would be null or not, by the link) |
| `extern:<name>` | a call of a function with neither body nor host function; an access of a variable without an initializer |
| `call:inline-asm`, `call:operand-bundle`, `call:variadic` | such a call |
| `attr:byval`, `attr:inalloca`, `attr:preallocated` | a call with such an argument |

Tests: `tests/unsupported.rs` checks every reason that can be provoked
through `hod_ir`'s builder, and `tests/vectors.rs`
(`refused_vector_forms`, `reductions`, `vector_intrinsics`) the vector
ones. The two `target:` reasons and the five defensive `type:` reasons
cannot be (both of `hod_ir`'s targets are 64-bit and little-endian);
`inst:atomicrmw-vector` is not yet provoked by a test.

## 9. The program generator

`hod_interp::generate`: a deterministic map from a seed and options to a
program the interpreter runs to a result.

### 9.1 What is generated

```rust
pub fn generate(seed: u64, options: &Options) -> Generated;

pub struct Generated {
    pub module: Module,
    pub bodies: Vec<Body>,       // one per generated function
    pub entry: SymId,            // `i64 entry(i64, i64)`
    pub args: Vec<Val>,          // two `i64`
    pub fuel_bound: u64,
    pub hosts: Option<Standard>, // when `options.hosts`
    pub coverage: Coverage,
}
impl Generated {
    pub fn program(&self) -> Program<'_>;
    pub fn run(&self) -> Outcome;                  // standard hosts, fuel = fuel_bound,
                                                   // call depth >= the number of functions
    pub fn run_with(&self, config: Config) -> Outcome;
}
```

Every generated program satisfies, for every seed and all options:

1. It is built only through `hod_ir`'s public builder (`FunctionBuilder`,
   `Module`), and every body passes `Body::verify`.
2. `generated.run()` returns `Ok(Some(i64))`: no trap, nothing unsupported.
3. The run takes at most `fuel_bound` steps (§7).
4. Result and trace do not depend on `Config::addr_base` and `addr_gap`.
5. Every definition dominates its uses (which `Body::verify` does not ask),
   so the program is also valid LLVM IR.
6. The same seed and options give the same program.

`generate` panics only if the generator itself is wrong.

The module holds the function `entry`, functions `f1`, `f2`, ... with
signatures drawn from a small pool (so that several share one, for indirect
calls), with `options.globals` the variables of §9.3, with
`options.hosts` the declarations of `sink` and `source` (§6.1), and with
`options.exceptions` those of `eh::declare` (§5.9). Function `fN` calls
only functions of a higher number: there is no recursion.

Tests: `tests/generate.rs` (`programs_verify_and_run_within_their_bound`,
`generation_is_deterministic`, `results_do_not_depend_on_addresses`).

### 9.2 Why a generated program cannot trap

The generator tracks what it must know about each value and gives every
operation that could trap operands that cannot make it:

| trap | how it is excluded |
|---|---|
| `division-by-zero`, `division-overflow` | an unsigned divisor is `x \| 1` or a non-zero constant; a signed divisor is `(x & m) \| 1` with `m` at most the largest positive value, or a positive constant, so it is neither 0 nor -1; there is no signed division at `i1` |
| `shift-too-large` | the amount is a constant below the width, or `x & m` with `m` the largest `2^k - 1` below the width |
| `nan-result`, `float-to-int-overflow` | every float has a tracked bound `2^mag` on its magnitude, `mag` ≤ 90, so nothing overflows to infinity (`float` holds `2^127`) and no NaN arises: `fadd`/`fsub` give `max(mag) + 1`, `fmul` the sum, operands chosen so the result stays within 90; `fdiv` and `frem` only divide by a value known to be at least 1 in magnitude (made as `uitofp((x & 0xfff) \| 1)`, or a product of such); floats come from integers below 2^65, from constants in [-8, 8], and from memory, parameters, call results and loop-carried `phi`s, which are all held to `mag` ≤ 30; `fptosi` to `i64` needs `mag` ≤ 62, `fptoui` moreover a value known not to be negative; no integer is `bitcast` to a float |
| `intrinsic-poison` | `ctlz`, `cttz` and `abs` are called with the flag false |
| `out-of-bounds`, `misaligned`, `uninitialized-load`, `non-zero-padding-bits`, `write-to-constant` | all memory is in *regions* the generator knows: byte regions (16 bytes, aligned to 8, fully written: any of `i8`, `i16`, `i32`, `i64` at a multiple of its size) and arrays of cells of one type (only that type, only at cell boundaries). Every stack slot is written in full in the entry block. Offsets are constants or `x & (count - 1)` scaled. Constant variables are only read. A pointer parameter is the start of a writable byte region: callers pass nothing else |
| `use-after-free` | no function returns or stores a pointer |
| `overlapping-copy` | `memcpy` copies between the same offset of two byte regions, which are the same region or disjoint; other copies are `memmove` |
| `undefined-value` | the generator keeps a scoped pool of the values whose definitions dominate the current point (5 of §9.1) |
| `unreachable`, `llvm.trap` | an `unreachable` is only in a block without predecessors or behind a branch whose condition is false by construction (`(x & 3) < 4`) |
| the call traps | calls use the callee's type; indirect calls choose among functions of one signature |
| `limit-fuel` | `fuel_bound` is computed (§9.5) |
| `limit-call-depth` | the call graph is acyclic and at most `functions` deep |
| `poison-lane` (with `vectors`) | no vector has a poison lane: vectors are built from pool integers by `insertelement` at constant in-range indices and a `shufflevector` of in-range mask entries; shift amounts are `b & splat(width - 1)`, unsigned divisors `b \| splat(1)`; there is no signed division of vectors, no float vector, and `ctlz`/`cttz`/`abs` have the flag false |

**Addresses.** `ptrtoint` results depend on the layout, so they never join
the pool of values. They occur only in three forms whose results do not:
`inttoptr(ptrtoint(p) + k)` used as a pointer into `p`'s region;
`ptrtoint(p + k) - ptrtoint(p)`, which is `k`; and an `icmp` of two pointers
into one region.

### 9.3 Options

```rust
pub struct Options { pub target: TargetSpec, pub functions: u32, pub insts: u32,
                     pub max_fuel: u64, /* and the switches below */ }
impl Default for Options;                 // everything on, 4 functions of about 60 instructions
impl Options {
    pub fn minimal() -> Options;          // everything off: one function, `i64` arithmetic
    pub fn sized(self, functions: u32, insts: u32) -> Options;
    pub fn effective(&self) -> Options;   // with the "needs" below applied
}
```

- `functions`: how many functions (at least 1). `insts`: about how many
  instructions each function has; the function has at least that many and,
  for the default options, fewer than four times as many. From 1 to
  thousands.
- `max_fuel`: loops and calls are left out where they would take the bound
  past it. `fuel_bound` itself may exceed `max_fuel` by the straight-line
  code that follows.

| switch | constructs | needs |
|---|---|---|
| (always) | `add` .. `xor`, shifts, the four divisions at `i64`; `freeze`; results that are never used, used once by the next instruction, used once later, used many times | |
| `narrow_ints` | the same at `i1`, `i8`, `i16`, `i32` | `casts` |
| `odd_ints` | at `i7` and `i33`, in memory too | `casts` |
| `i128` | at `i128` | `casts` |
| `floats` | `fadd` .. `frem`, `fneg`, `fcmp`, `fptrunc`, `fpext`, the four int/float conversions, `bitcast` to an integer, at `float` and `double` | |
| `compares` | `icmp` as a value and as a branch condition | |
| `selects` | `select` of integers, floats and pointers | `compares` |
| `casts` | `trunc`, `zext`, `sext` | |
| `memory` | stack slots; `load` and `store` of every type in use at natural and at byte alignment; `ptradd` with a constant offset, with a scaled index of any width, with both; pointer parameters | |
| `globals` | variables with initializers: writable and constant byte arrays, `[1 x i64]`, a constant `[4 x i64]`, `[4 x i32]`, a struct with padding `{ i8, i32, i16, i64 }`, `[2 x i33]`, `[2 x double]` (those whose element types are in use); a constant array of function addresses per signature | `memory` |
| `ptr_int` | `ptrtoint`, `inttoptr` (§9.2) | `memory` |
| `intrinsics` | `ctpop`, `bitreverse`, `bswap`, `ctlz`, `cttz`, `abs`, `smax` .. `umin`, the saturating four, `fshl`, `fshr`, the six `with.overflow` with both fields extracted, at `i8` .. `i64`; with `memory`, `memset`, `memcpy`, `memmove` | |
| `branches` | a diamond with `phi`s at the join; a triangle (one edge of the `condbr` is critical); a `condbr` whose two edges reach the same block, with `phi`s there; a branch to an `unreachable` that is never taken | `compares` |
| `loops` | a counted loop of 1 to 4 iterations around nested code, and one that is a single block branching to itself; each with an induction `phi`, accumulators, and two `phi`s that swap on the back edge | `compares` |
| `switches` | `switch` with dense cases (0..n) and with sparse ones; two cases share a destination that has `phi`s (one entry per edge); cases that go straight to the join | `compares` |
| `unreachable_blocks` | a block without predecessors that branches into the code, with `phi` entries | |
| `calls` | direct calls of generated functions, 0 to 4 parameters of mixed types, results used and unused, `void` functions; values live across calls | |
| `indirect_calls` | calls through a `select` of two function addresses, or through an address loaded from a constant table | `calls`, and `selects` or `globals` |
| `many_args` | functions of 9 to 12 parameters of mixed types | `calls` |
| `hosts` | calls of `sink` and `source` | |
| `exceptions` | §5.9: a `hod_throw` under a condition that leaves the function; one or two `invoke`s of `hod_throw` under conditions to one shared pad, with `phi`s in the pad; `invoke`s of generated functions that may throw, the pad rejoining the normal destination with a `phi` of the result; calls marked `nounwind` of functions that cannot throw. A pad has 1 to 3 clauses (`catch` of one of the four types, catch-all, `filter` of 1 to 4 types or empty) and maybe `cleanup`; it dispatches on `selector == llvm.eh.typeid.for(@ti)`, then on the selector's sign; the cleanup runs code and `resume`s; the handler `hod_catch`es and runs code. With `hosts` each pad sinks a number of its own and its integer `phi`s, and the handler sinks which test of the selector matched. The entry function catches everything; pads nest two deep | `compares` |
| `block_addresses` | §5.5: a constant table of 2 or 4 `blockaddress`es of the function's own blocks, an `indirectbr` to one loaded by a masked index, the arms joining with `phi`s; the loaded address compared with `null` | `globals`, `compares` |
| `vectors` | §5.10 on `<N x iW>`, N in {1, 2, 3, 4, 8, 16}, W the widths of 8..64 in use: built by `insertelement` into `zeroinitializer` and a broadcasting `shufflevector`, then up to three more `insertelement`s (`i32` or `i64` index); integer binary operations; with `compares` an `icmp` to `<N x i1>`, then a lane-wise `select` (with `selects`), a `bitcast` of the mask to `iN`, or a `sext`; `shufflevector` to 1, 2, 4, N or 2N lanes; with `casts` `trunc`/`zext`/`sext` between widths; with `intrinsics` `ctpop`, `bitreverse`, `ctlz`, `cttz`, `abs`, `smax` .. `umin` and the saturating four; back to a scalar by a `bitcast` to `i(N·W)` (when that width is in use), a `llvm.vector.reduce.*` of the nine integer kinds, or an `extractelement` at a constant index | |

Constructs nest up to three deep. A switch that is off removes its
constructs entirely; a switch whose needs are not met is off.

`exceptions` and `block_addresses` are **off in `Options::default()`**,
and when off they draw nothing from the random stream: every seed gives
the program it gave before they existed. `Options::everything()` is the
default with both on. A switch added later must keep this property.

`vectors` (WP-078) keeps it too: it is off in `Options::default()` **and
in `Options::everything()`** (which other crates' tests use, for code
generators that may not yet lower vectors), and when off the generator's
choice of construct skips it without drawing, so every existing seed gives
its old program (`vector_switch_keeps_existing_programs` fixes hashes of
three option sets taken before the switch existed). Turn it on with
`Options { vectors: true, ..Options::default() }`.

Tests: `tests/generate.rs` (`minimal_options_leave_only_integer_arithmetic`,
`each_option_switches_its_constructs`, `size_knobs`, `unwinding_switches`,
`existing_seeds_keep_their_programs`: a hash of the printed programs of
seeds of three option sets, fixed before WP-044. It depends on
`hod_ir`'s printer: a change of the printer changes the constants, and
the new ones are taken only after checking that the generator did not
change).

### 9.4 Coverage

`Generated::coverage` counts constructs as they are emitted:
`Coverage::NAMES` lists the counters, `get(name)`, `iter()`, `merge(&other)`.

`int:i1`, `int:i8`, `int:i16`, `int:i32`, `int:i64`, `int:i7`, `int:i33`,
`int:i128` (integer binary operations by width), `float:op`, `float:fcmp`,
`float:convert`, `icmp:value`, `icmp:branch`, `select`, `cast:trunc`,
`cast:zext`, `cast:sext`, `cast:ptrtoint`, `cast:inttoptr`, `freeze`,
`intrinsic:int`, `intrinsic:checked`, `intrinsic:mem`, `mem:slot`,
`mem:load`, `mem:store`, `mem:odd-width`, `ptradd:const`, `ptradd:scaled`,
`global:load`, `global:store`, `cf:diamond`, `cf:critical-edge`,
`cf:condbr-twice`, `cf:dead-trap`, `cf:loop`, `cf:self-loop`, `cf:swap`,
`cf:switch-dense`, `cf:switch-sparse`, `cf:switch-shared`,
`cf:unreachable-block`, `phi`, `call:direct`, `call:indirect`,
`call:many-args`, `call:unused-result`, `call:sink`, `call:source`.

`Coverage::UNWIND_NAMES` are the counters of `exceptions` and
`block_addresses`: `eh:throw`, `eh:invoke-throw`, `eh:invoke-call`,
`eh:shared-pad`, `eh:pad-phi`, `eh:catch`, `eh:catch-all`, `eh:filter`,
`eh:filter-empty`, `eh:cleanup`, `eh:typeid-for`, `eh:nounwind-call`,
`eh:normal-phi`, `cf:indirectbr`, `blockaddress:table`,
`blockaddress:compare`. They are zero for the default options and
positive over a few hundred seeds of `Options::everything()`.

`Coverage::VECTOR_NAMES` are the counters of `vectors`: `vector:build`,
`vector:int`, `vector:compare`, `vector:select`, `vector:shuffle`,
`vector:cast`, `vector:intrinsic`, `vector:extract`, `vector:reduce`,
`vector:bitcast`, `vector:mask-bitcast`. They are zero when `vectors` is
off and positive over a few hundred seeds with it on
(`vector_programs_cover_every_construct`).

Six more are counted from the finished bodies with `Body::uses`: `use:dead`,
`use:next`, `use:once-later`, `use:many` (each instruction with a result is
one of the four), `use:other-block`, and `use:across-call` (an instruction
that uses a result with a call of a function, not of an intrinsic, between
the definition and the use in instruction order).

With the default options every counter is positive over a few hundred
seeds; with a switch off its counters are zero and all others stay
positive.

Test: `tests/generate.rs` (`default_options_cover_every_construct`,
`each_option_switches_its_constructs`,
`unwinding_programs_cover_every_construct`).

### 9.5 The fuel bound and throughput

The generator counts, for every instruction it emits, how often it can run
in one call: 1, times 4 for each enclosing loop. A function's cost is the
sum over its instructions plus, for each call, that multiplicity times the
callee's cost (callees are generated first; an indirect call counts the
dearer of its two targets). `fuel_bound` is the entry function's cost. For
straight-line code (`Options::minimal()`) it is exact.

Measured on the development host (AArch64, 24 cores, one thread), release
build, by `tests/generate.rs` (`throughput`): see
`spec/landed/interp/WP-031.md`.

## 10. The LLVM oracle

`tests/llvm_oracle.rs` checks the interpreter and the generator against
LLVM. For a batch of seeds it

1. generates each program for the host's target and runs it in the
   interpreter;
2. prints all of them into one file of LLVM assembly (`tests/llvm_text`),
   each program's symbols under its own prefix, with a `main` that, per
   program, resets `source`, prints `p <seed>`, calls the entry function on
   its arguments and prints `r <result>`; `sink` is defined to print
   `s <argument>` and `source` to compute `source_value` (§6.1);
3. compiles the file with `llc -O0 -global-isel=0 -fast-isel=0` (§10.1),
   and again through `opt -O2` and `llc -O2`; links each with `gcc -lm`
   (`frem` is `fmod`); runs both;
4. requires the output of each to be, program by program, the interpreter's
   `sink` events and result.

The entry functions are external and `noinline`, so at `-O2` they are
compiled for unknown arguments; the other functions are `internal`, so the
optimizer may inline, specialize and fold them.

A disagreement fails the test and keeps the one program as
`<build>/tmp/llvm-oracle-seed-<seed>.ll`. It is a bug in the interpreter,
the printer or the generator until LLVM is proven wrong: the program
reduced (`llvm-reduce`, with "this way of compiling differs from the others"
as the test) to a function whose answer can be worked out by hand and whose
wrong answer can be read off the assembly. §10.1 has the two times it was.

The printer covers exactly what the generator emits. It is to be replaced
by `hod_ir`'s printer (WP-006).

Tools are found with `hod_testutil::tool` (`llc`, `opt`, `gcc`; LLVM 22
preferred). Without them, or on a host that is not Linux on AArch64 or
x86-64, the tests skip with a note; with `HOD_REQUIRE_TOOLS=1` they fail.

- Quick (in `cargo test`): 48 programs of the default options and 6 of
  8 functions × 300 instructions, each at both levels.
- Slow: `cargo test -p hod_interp --test llvm_oracle -- --ignored`
  (`many_programs_agree_with_llvm`): `HOD_ORACLE_SEEDS` (default 2000)
  seeds of the default options and as many of tiny programs, a tenth as
  many of 6 × 400 and a fiftieth of 3 × 3000.
- `HOD_ORACLE_KEEP=<dir>` keeps every file compiled.

### 10.2 Exceptions and block addresses

The programs of §5.9 and §5.5 are checked against the real Itanium ABI:

- `hod_throw` and `hod_catch` are a C++ shim (in `tests/llvm_oracle.rs`)
  compiled with `g++`: `__cxa_allocate_exception` of an `int64_t`,
  `__cxa_throw` with the given `std::type_info`; `__cxa_begin_catch`,
  read the payload, `__cxa_end_catch`. The type-information objects are
  the C++ runtime's own; the link is with `g++` (libstdc++, libgcc's
  unwinder).
- The printer gives every function `personality ptr
  @__gxx_personality_v0`, and marks the non-entry functions that have
  landing pads `noinline`: LLVM's inliner appends the caller's clauses to
  an inlined landing pad, so a selector that the pad's own clauses would
  not give reaches its dispatch. That is correct LLVM (C++ dispatches on
  `typeid.for` equality only), but the generator's sign tests would see
  it; the model has no inlining, so the oracle does not inline them.
- An uncaught exception is checked by its end: the binary dies by
  `SIGABRT` from `std::terminate`, having printed what the program
  printed before the throw and nothing of any cleanup
  (`uncaught_is_terminate`).
- Each test runs with every LLVM major installed of 22 and 19 (`llc-N`,
  `opt-N`), and needs one at least.

Tests:

- `unwinding_cases_agree_with_llvm`: the hand-written cases of
  `tests/eh_cases` (each clause kind, nesting, `resume` through frames
  without pads, a computed goto through a table), each called on several
  arguments: 12 per major.
- `unwinding_programs_agree_with_llvm` (quick): 48 seeds of
  `Options::everything()`, 48 of `everything().sized(2, 12)` and 6 of
  `sized(8, 300)`: 102 programs per major.
- `many_unwinding_programs_agree_with_llvm` (slow, `--ignored`):
  the shapes of `many_programs_agree_with_llvm` (§10) with
  `Options::everything()`: `HOD_ORACLE_SEEDS` of the default size and as
  many tiny ones, a tenth as many of 6 × 400, a fiftieth of 3 × 3000.

Results at WP-044 (LLVM 22.1 and 19.1, AArch64): 12 cases, 102 quick and
2,120 slow programs (`HOD_ORACLE_SEEDS=1000`) agree at `-O0` and `-O2`
with each major. Two disagreements were found while it was built, both
in the oracle's own assumptions and both at `-O2` only: `llc -O2` folded
an `icmp eq` of two `blockaddress`es that the interpreter called unequal
(LangRef defines no such comparison: it became the trap
`blockaddress-inspected`, and the generator compares with `null`), and
the inliner's merged clauses above (`noinline`).

### 10.1 The unoptimized leg is SelectionDAG

`llc -O0` has three instruction selectors. For AArch64 the default is
GlobalISel; `-global-isel=0` gives FastISel; `-global-isel=0 -fast-isel=0`
gives SelectionDAG, which is also what `-O2` uses. The first two each
miscompile a correct program (LLVM 19.1 and 22.1), found by the slow batch
and reduced to these. In both, every other way of compiling the function
(the other two selectors at `-O0`, `llc -O2`, `opt -O2` with `llc -O2`)
agrees with the interpreter.

GlobalISel (seed 30149 of the 6 × 400 batch):

```llvm
@g = global i16 -5408                ; 0xEAE0
define i64 @f() {
  %a = load i16, ptr @g
  %b = zext i16 %a to i32
  %c = zext i32 %b to i33
  %d = sext i32 %b to i64
  %e = zext i33 %c to i64
  %r = or i64 %d, %e
  ret i64 %r
}
```

`%b` is 60128, so `%d`, `%e` and `%r` are 60128. `llc -O0` returns -5408:
its combiner turns the load into a sign-extending one (`ldrsh x8, [x8]`)
and uses that for `%d`. The interpreter's test of it is
`zero_extended_load_then_sign_extended` in `tests/memory.rs`.

FastISel (seed 10507 of the default batch):

```llvm
define i32 @f(i7 %a) {               ; called with 26
entry:
  %t = trunc i7 %a to i1
  %s = shl i1 %t, 0
  %c = icmp ult i1 0, %s
  br i1 %c, label %yes, label %no
yes:
  ret i32 1
no:
  ret i32 0
}
```

`%t` is the low bit of 26, which is 0; so is `%s`; 0 is not below 0 and the
function returns 0. `llc -O0 -global-isel=0` returns 1: it compares 0 with
the whole register that holds `%a` (`cmp w8, w0`), the `trunc` and the
shift having left the bits above the first in place. The interpreter's
test of it is `shifted_bit_compares_as_one_bit` in `tests/instructions.rs`.

So the unoptimized leg of the oracle is SelectionDAG at `-O0`: the
instructions as written, with no optimization of the IR, through the
selector that got both right. It shares its legalizer with the `-O2` leg;
what it adds is that nothing was folded, inlined or simplified first.

Anything that takes plain `llc -O0` for AArch64 as the truth about what a
program does meets these miscompilations when the program computes in
widths that are not a power of two or in `i1`.

### 10.3 Vectors

`tests/vector_oracle.rs` (WP-078) checks §3.1, §4 and §5.10 against
`llc -O0 -global-isel=0 -fast-isel=0` (§10.1) of each LLVM major installed
of 22 and 19, natively on AArch64. It writes LLVM text itself (the
generator's vectors, §9.3, are narrower): `HOD_VECTOR_ORACLE_COUNT`
functions (default 3,000), 300 to a file, so a run is ten files, each
compiled once per major, linked with a C driver by `gcc` and run once.

- Each function draws a shape: lanes `i1`, `i8`, `i16`, `i32`, `i64`,
  `i128`, `ptr`, `float` or `double`; 1, 2, 3, 4, 5, 8, 13 or 16 lanes, or
  a large count (64 for lanes up to 32 bits, else 32; `i128` at most 4).
- Inputs are loaded from variables with random and edge values (0, 1,
  all ones, the smallest and largest signed, values near them; `±0.0`,
  infinity, the largest finite and the smallest denormal, 2^24 + 1), so
  nothing is folded; pointer lanes are addresses into one array, reported
  as offsets.
- 1 to 4 operations: every row of the table of §5.10 at the shape, plus
  type puns through memory at alignment 1, `phi` of vectors through a
  diamond, and vectors passed to and returned from an `internal` helper
  function. Each result is stored to a global and printed by
  `emit(ptr, len)`; the interpreter's `emit` prints an unwritten byte (a
  poison lane, §4) as `??`, which matches any byte.
- A function the interpreter traps on is compiled but not run, and
  counted; an unsupported answer fails the test.

Result at WP-078 (LLVM 22.1 and 19.1): 2,917 of 3,000 functions agree with
both majors, byte for byte, with 3,940 poison bytes; 83 are not run (68
`nan-result`, 13 `poison-lane`, 2 `division-by-zero`). About 20 s for both
majors. `functions_are_deterministic` checks the text is a function of the
seed. `tests/llvm_oracle.rs` (`vector_programs_agree_with_llvm`) also runs
66 generated programs with `vectors` at `-O0` and `-O2` per major.

Where LLVM is wrong, the oracle steers around it, each with a comment:

- **`trunc <3 x i64>` and `<3 x i128>` to `<3 x i8>` (llc 19 and 22,
  SelectionDAG at every level).** `<3 x i64> <-1, 11, -1>` truncates to
  `ff ff 00`, `<3 x i128> <-1, 1, …d7>` to `ff 00 00`; the low bytes are
  `ff 0b ff` and `ff 01 d7`, which GlobalISel (`llc -O0`) gives. A probe
  of lengths {2, 3, 4, 5, 6, 7, 8, 13, 16}, sources {16, 32, 64, 128} and
  results {1, 8, 16, 32} found only these two wrong. The oracle truncates
  such lanes to `i16` instead; `truncation_of_wide_lanes` in
  `tests/vectors.rs` has both.
- **`llvm.vector.insert` (llc 19)** crashes ("Don't know how to widen the
  operands for INSERT_SUBVECTOR") for a length that is not a power of two,
  a one-lane subvector or masks; the oracle uses it on power-of-two
  vectors of byte lanes with subvectors of two or more lanes.
- **Odd lanes**: §3.1.

## 11. Tests

| file | checks |
|---|---|
| `src/*.rs` unit tests | §3 values, §4 memory, §5.1/§5.6 arithmetic kernels, §6.1 `source_value`, trap names |
| `tests/instructions.rs` | §5.1, §5.2, §5.3, §5.5, §5.7: every instruction with hand-computed results, every trap; the second function of §10.1 |
| `tests/memory.rs` | §3, §4, §5.4, the memory intrinsics of §5.6, `limit-memory`, the first function of §10.1 |
| `tests/calls.rs` | §5.8, §5.6, §6, §7, §2 |
| `tests/unsupported.rs` | §8: every reason that can be provoked |
| `tests/robustness.rs` | §1, §2: a thousand random unguarded programs (zero divisors, wild pointers, uses before definitions, endless recursion, calls through any pointer); every body verifies, no run panics or is `Invalid`, and at least twelve kinds of end occur |
| `tests/unwind.rs` | §5.9 |
| `tests/block_addr.rs` | §5.5: `blockaddress`, `indirectbr` |
| `tests/generate.rs` | §9 |
| `tests/llvm_oracle.rs` | §10, §10.2 (with the cases of `tests/eh_cases`), the generated vector programs of §10.3 |
| `tests/vectors.rs` | §3.1, §4 (vectors in memory), §5.10, the vector reasons of §8: hand-computed lanes, poison and traps |
| `tests/vector_oracle.rs` | §10.3 |

Each test names its section. `tests/common` builds one-function modules;
`tests/llvm_text` is the printer of §10; `tests/vector_text` reads LLVM
text through `hod_ll` (a dev-dependency) for the vector tests.

## 12. Open questions and follow-ups

Open, with the conservative choice taken:

- **Comparing block addresses.** The brief of WP-044 asked for a
  `blockaddress` "compared for equality"; LangRef defines only the
  comparison with `null`, and `llc -O2` does fold an equality of two
  differently from the addresses it emits. The model traps
  (`blockaddress-inspected`); if C++ front ends rely on an equality (a
  GNU computed-goto idiom), what LLVM guarantees must be settled first.
- **`unwind-from-nounwind` is stricter than compiled code**: a C++
  compiler puts a terminate pad where it needs one, and hod_cg's code
  unwinds on through a `nounwind` function that has no pad. No generated
  program reaches it.

- **Address-dependent results are not detected.** A program that returns
  `ptrtoint` of a stack slot has a result here, and a different one in
  compiled code. Detecting it needs provenance on integers. Until then:
  generated programs avoid it by construction (§9.2), and a test of other
  programs should compare two layouts.
- **No pointer provenance.** `inttoptr` of any integer that happens to be a
  live address is usable; an access that leaves one allocation and lands in
  another is caught only because allocations are `addr_gap` apart. LLVM's
  rules (LangRef "Pointer Aliasing Rules") are stricter, so the interpreter
  may give a result where LLVM sees undefined behavior. No generated program
  relies on it.
- **Padding of a variable's initializer is unwritten.** LLVM emits zeros
  there; a program that reads the padding of a global gets
  `uninitialized-load` here although every compilation agrees on zero. The
  same holds for an `undef` initializer element. Conservative: a trap, not
  a wrong result.
- **`undef`/`poison` constants are zero** because `ir.md` §4.1 says so. A
  comparison of hod against LLVM on a program that uses them may differ;
  the generator emits none.
- **NaN.** Trapping on every computed NaN is stricter than needed (a NaN
  that only reaches an ordered compare is harmless). `NanPolicy::Host` is
  the escape hatch.
- **Module constructors** (`llvm.global_ctors`) are not run.
- **`freeze` of a vector keeps its poison lanes** (§5.10). LangRef fixes
  each to an arbitrary value, which compilations may choose differently,
  so a later observation still has no one result. Conservative: a trap
  where a program that never looks at the frozen lane's value would have
  one.
- **A vector division by a poison lane traps** although LangRef makes only
  that lane's result undefined behavior; the same is true of a scalar.

Follow-ups, none of which the code generator's first milestones need:

- `callbr` (an `asm goto`: needs a model of inline assembly);
- exceptions of class types (base-class matching, `catch` by pointer with
  qualification conversions): the C++ runtime's `__do_catch`, beyond the
  four fundamental types; foreign exceptions; the funclet instructions;
- variadic functions: `va_arg`, `llvm.va_start`, `llvm.va_end`,
  `llvm.va_copy`;
- `byval` arguments (a copy in the callee's frame);
- thread-local variables (one thread: a plain variable), aliases;
- vectors beyond §5.10: `half`/`bfloat` lanes; scalable vectors; vector
  `getelementptr`; vector `with.overflow`; the masked, gather and scatter
  intrinsics; `llvm.vector.reduce.fmax`/`fmin`/`fmaximum`/`fminimum`;
  lanes of odd widths (once `llc` stores them as LangRef packs them); in
  the generator, float and pointer lanes and signed division;
- `llvm.fmuladd` (unspecified whether fused: needs a "both agree" rule),
  the `min`/`max` float intrinsics, `llvm.stacksave`/`stackrestore`;
- in the generator: dynamic `alloca`, atomics, aggregates as values
  (`insertvalue`, struct returns), `byval`, x86-64 as the oracle's host;
- replace `tests/llvm_text` with `hod_ir`'s printer when WP-006 lands.
