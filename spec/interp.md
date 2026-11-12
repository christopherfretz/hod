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

It is single-threaded, little-endian, and has 64-bit pointers. It does not
model exceptions, variadic calls, vectors, thread-local storage or inline
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
    Agg(Agg),                         // an array or struct: its type and memory image
}
pub const MAX_INT_BITS: u32 = 128;
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
arrays and structs of these. Every other type is unsupported (§8).

Tests: `tests/memory.rs` (`aggregates`), unit tests of `value.rs`.

## 4. Memory

Memory is a set of **allocations**. An allocation has an address, a size, an
alignment, a kind and, for every byte, a **written** flag.

| kind | made | ends |
|---|---|---|
| variable | at the start of a run, for every module variable with an initializer | never |
| constant variable | the same, for a variable that is `constant` | never |
| stack | when a frame is pushed, for every stack slot of the body; by `alloca` | when the frame that made it returns |
| function | at the start of a run, for every function except an undefined `extern_weak` one | never |

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
- `extractelement`, `insertelement`, `shufflevector`: vectors are
  unsupported.

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
- `indirectbr`, `invoke`, `callbr`, `resume`, `landingpad` and the funclet
  instructions are unsupported.

Tests: `tests/instructions.rs` (`phis_are_a_parallel_copy`,
`switch_and_repeated_successors`, `unreachable_and_undefined_values`).

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
Unwritten memory is never read (§4).

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
| `type:half`, `type:bfloat`, `type:fp128`, `type:x86_fp80` | a value of that type |
| `type:vector`, `type:scalable-vector` | a vector value |
| `type:int-over-128` | an integer wider than 128 bits |
| `type:target-ext`, `type:token` | a value of that type; the `none` token constant |
| `type:opaque`, `type:function`, `type:label`, `type:metadata` | a value of that type. The builder does not make such values: defensive |
| `inst:va_arg`, `inst:invoke`, `inst:landingpad`, `inst:resume`, `inst:indirectbr`, `inst:callbr`, `inst:addrspacecast` | the instruction |
| `inst:cleanuppad`, `inst:catchpad`, `inst:catchswitch`, `inst:catchret`, `inst:cleanupret` | the funclet instructions |
| `inst:atomicrmw-fmax`, `-fmin`, `-fmaximum`, `-fminimum` | that `atomicrmw` operation |
| `intrinsic:<name>` | any intrinsic not in §5.6: the base name for one `hod_ir` knows (`intrinsic:llvm.stacksave`, `intrinsic:llvm.fmuladd`, `intrinsic:llvm.va_start`), the full name for one it does not |
| `const:blockaddress`, `const:symdiff` | that constant |
| `const-expr:<op>` | a constant expression that `hod_ir` could not fold (`const-expr:add`) |
| `const:aggregate` | an aggregate constant used as a value that cannot be flattened |
| `global:initializer` | a variable's initializer cannot be flattened, or is not as large as its type |
| `global:thread-local`, `global:alias`, `global:ifunc` | the address of such a symbol |
| `global:extern-weak` | the address of an undefined `extern_weak` function (it would be null or not, by the link) |
| `extern:<name>` | a call of a function with neither body nor host function; the address of a variable without an initializer |
| `call:inline-asm`, `call:operand-bundle`, `call:variadic` | such a call |
| `attr:byval`, `attr:inalloca`, `attr:preallocated` | a call with such an argument |

Tests: `tests/unsupported.rs` checks every reason that can be provoked
through `hod_ir`'s builder. The two `target:` reasons and the four defensive
`type:` reasons cannot (both of `hod_ir`'s targets are 64-bit and
little-endian).

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
calls), with `options.globals` the variables of §9.3, and with
`options.hosts` the declarations of `sink` and `source` (§6.1). Function
`fN` calls only functions of a higher number: there is no recursion.

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

Constructs nest up to three deep. A switch that is off removes its
constructs entirely; a switch whose needs are not met is off.

Tests: `tests/generate.rs` (`minimal_options_leave_only_integer_arithmetic`,
`each_option_switches_its_constructs`, `size_knobs`).

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

Six more are counted from the finished bodies with `Body::uses`: `use:dead`,
`use:next`, `use:once-later`, `use:many` (each instruction with a result is
one of the four), `use:other-block`, and `use:across-call` (an instruction
that uses a result with a call of a function, not of an intrinsic, between
the definition and the use in instruction order).

With the default options every counter is positive over a few hundred
seeds; with a switch off its counters are zero and all others stay
positive.

Test: `tests/generate.rs` (`default_options_cover_every_construct`,
`each_option_switches_its_constructs`).

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

## 11. Tests

| file | checks |
|---|---|
| `src/*.rs` unit tests | §3 values, §4 memory, §5.1/§5.6 arithmetic kernels, §6.1 `source_value`, trap names |
| `tests/instructions.rs` | §5.1, §5.2, §5.3, §5.5, §5.7: every instruction with hand-computed results, every trap; the second function of §10.1 |
| `tests/memory.rs` | §3, §4, §5.4, the memory intrinsics of §5.6, `limit-memory`, the first function of §10.1 |
| `tests/calls.rs` | §5.8, §5.6, §6, §7, §2 |
| `tests/unsupported.rs` | §8: every reason that can be provoked |
| `tests/robustness.rs` | §1, §2: a thousand random unguarded programs (zero divisors, wild pointers, uses before definitions, endless recursion, calls through any pointer); every body verifies, no run panics or is `Invalid`, and at least twelve kinds of end occur |
| `tests/generate.rs` | §9 |
| `tests/llvm_oracle.rs` | §10 |

Each test names its section. `tests/common` builds one-function modules;
`tests/llvm_text` is the printer of §10.

## 12. Open questions and follow-ups

Open, with the conservative choice taken:

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

Follow-ups, none of which the code generator's first milestones need:

- exceptions: `invoke`, `landingpad`, `resume`, with a model of the
  personality's decision;
- `indirectbr` and `blockaddress`; `callbr`;
- variadic functions: `va_arg`, `llvm.va_start`, `llvm.va_end`,
  `llvm.va_copy`;
- `byval` arguments (a copy in the callee's frame);
- thread-local variables (one thread: a plain variable), aliases;
- vectors;
- `llvm.fmuladd` (unspecified whether fused: needs a "both agree" rule),
  the `min`/`max` float intrinsics, `llvm.stacksave`/`stackrestore`;
- in the generator: dynamic `alloca`, atomics, aggregates as values
  (`insertvalue`, struct returns), `byval`, x86-64 as the oracle's host;
- replace `tests/llvm_text` with `hod_ir`'s printer when WP-006 lands.
