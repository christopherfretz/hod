# `hod_xform`: lowerings and analyses required before code generation

This file is normative for `crates/hod_xform`. Its first content is the
coroutine plan of design 0005 (`docs/design/0005-coroutines.md` §3.4),
which approach B consumes in the code generator (package C3). The design
says why; this file says what.

## 1. Scope

- `hod_xform` reads `hod_ir` and writes nothing back into a body: the plan
  is pure analysis, and its result is data for the code generator.
- `#![forbid(unsafe_code)]` (workspace lint); no dependency but `hod_ir`.
- Every unsupported shape is refused by a stable name (§3.8), never by a
  panic. A panic is a bug.

## 2. `BitSet`

`hod_xform::BitSet<T: Idx>` is a fixed-size set of dense ids (`BlockId`,
`InstId`, `SlotId`), one bit each, created with its bound (`new(len)`).

- `contains` of an id past the bound is false; `insert` of one panics.
- `iter` yields members in increasing order; `count`, `is_empty`,
  `remove`, `domain` (the bound).
- `Debug` prints the member indices as a set.

## 3. The coroutine plan

### 3.1 Interface

```rust
pub fn plan(module: &Module, body: &Body) -> Result<Option<CoroPlan>, Refusal>;
pub fn analyze(module: &Module, body: &Body) -> Result<Option<Analysis>, Refusal>;

pub struct CoroPlan {
    pub begin: InstId,                     // the coro.begin
    pub promise: Option<SlotId>,           // a header field at promise_offset
    pub promise_offset: u32,               // alignTo(16, align(promise)); 16 without one
    pub frame_align: u32,                  // ≥ 16, a power of two
    pub heap_slots: BitSet<SlotId>,        // §3.3
    pub heap_values: BitSet<InstId>,       // §3.4
    pub begin_copies: Vec<SlotId>,         // §3.5, increasing
    pub begin_value_copies: Vec<InstId>,   // §3.5, increasing
    pub suspends: Vec<Suspend>,            // §3.7, in index order
    pub suspended_region: BitSet<BlockId>, // §3.6
    pub has_unwind_end: bool,              // some coro.end unwinds
    pub final_index: Option<u32>,          // the final suspend's index
}
pub struct Suspend {
    pub save: InstId,            // its coro.save
    pub wrapper: Option<InstId>, // its coro.await.suspend.{void,bool,handle}
    pub suspend: InstId,         // the coro.suspend
    pub index: u32,
    pub is_final: bool,          // coro.suspend(…, i1 true)
}
pub struct Refusal { pub construct: &'static str, pub inst: Option<InstId>, pub slot: Option<SlotId> }
pub const REFUSALS: [&str; 11];  // §3.8
```

- `plan` gives `Ok(None)` for a body whose function lacks the
  `presplitcoroutine` attribute, whatever it contains.
- `analyze` is `plan` without its last two checks: it returns the plan
  together with `classes` (the `SlotClass` of every slot: `Promise`,
  `OutsideFrame`, `Unescaped`, `EscapedWithMarkers`,
  `EscapedWithoutMarkers`, for rules 1 to 5), `violations` (§3.6) and
  `calls_align` (the body calls `llvm.coro.align`). `plan` refuses the
  first violation as `coro:frame-access-after-suspend`, then refuses
  `coro:overaligned-frame` when `frame_align > 16` and the body does not
  call `coro.align`.
- Differences from design 0005 §3.4 (the design was updated with them):
  `heap_values` is a set of instructions, since an argument used after a
  resumption edge is refused (§3.8) and no other value can have a home;
  `begin_value_copies` was added (§3.5); `analyze`, `Analysis`,
  `SlotClass`, `Violation`, `Region` and `REFUSALS` were added.

What `hod_ir` keeps for the plan (`spec/ir.md` §5.2): each slot's
`outside_frame()` flag (the `alloca` carried `!coro.outside.frame`), and
the body's lifetime markers (`Body::lifetime_markers()`: for each
`llvm.lifetime.start`/`end` call, the instruction it stood before, its
pointer operand, and which of the two it was). Neither is code; neither
is a use. `hod_ll` and `hod_bc` record both.

### 3.2 Definitions

- **Resumption edge**: the `0` or `1` case of the `switch` on a
  `coro.suspend` result. The default case (`-1`) is the suspend's
  **suspended edge**.
- **Ramp-only edge**: the true side of a conditional branch on
  `llvm.coro.is_in_ramp()` (LLVM 22), or the false side of a conditional
  branch on the `i1` result of an unwinding `llvm.coro.end(…, i1 true, …)`
  (LLVM 19). A resumed activation never takes one.
- **Cut**: a `coro.end(…, i1 false, …)`. A resumed activation returns
  there; the instructions after it run only in the ramp.
- **After resumption**: an instruction is after resumption iff its block
  is reachable from a resumption edge's target without taking a
  ramp-only edge and without passing a block containing a cut, and, in
  a block containing a cut, it stands before the first cut. A `phi`
  incoming value is used after resumption iff its edge is a resumption
  edge, or is not ramp-only and leaves a block that is after resumption
  and contains no cut.
- **Before `coro.begin`** (pre-begin): an instruction in a block
  reachable from the entry without entering `coro.begin`'s block, or
  before `coro.begin` in that block.
- **Aliases** of a slot: results of `getelementptr` (any indices),
  `bitcast` and `addrspacecast` whose pointer operand is the slot or one
  of its aliases, transitively. An alias has a known byte offset iff
  every step has constant indices.
- **Uses** of a slot: each operand that is the slot or one of its
  aliases, except an alias's own derivation. Debug records are not uses.
  A use is one of:
  - a **read**: a `load` from it; the source of `memcpy`/`memmove`
    (also `.inline`) whose source and destination differ;
  - a **whole write**: a `store` to it, or `memcpy`/`memmove`/`memset`
    (also `.inline`) with it as destination, at offset 0 and of at least
    the slot's size (a store's size is its value type's size; a
    call's is its constant length);
  - a **partial write**: such a store or call that is not whole (a
    nonzero or unknown offset, a smaller or non-constant size);
  - an **escape**: anything else: a store *of* the address, any other
    call or `invoke` argument, `phi`, `select`, `icmp`, `ptrtoint`,
    `ret`, a `memcpy` whose source and destination are the same value.
    The promise operand of `coro.id` is not an escape.
- A slot **escapes** iff it has an escape, or one of its aliases is a
  heap value (§3.4).
- A slot **has lifetime markers** iff some `lifetime.start` names it or
  an alias at known offset 0.

### 3.3 Slots: rules 1 to 5

The first rule that applies classes the slot:

1. **Promise**: the slot that `coro.id`'s second operand names (after
   aliases at offset 0). A header field, never in `heap_slots`.
2. **Outside frame**: `outside_frame()`. Stack. A use after resumption
   (lifetime markers excepted) is refused:
   `coro:outside-frame-across-suspend`.
3. **Unescaped**: in `heap_slots` iff some read can see a whole or
   partial write made before a resumption edge (§3.4).
4. **Escaped, with lifetime markers**: in `heap_slots` iff (a) some use
   or lifetime marker is reachable from a `lifetime.start` through a
   resumption edge, or (b) a `lifetime.start` is (a start counts as a
   use, so a loop that restarts the lifetime across an edge is (b)).
5. **Escaped, without lifetime markers**: in `heap_slots`, always.

### 3.4 The dataflow

One forward dataflow decides rule 3, rule 4 and the heap values. It runs
over the blocks reachable from the entry, in reverse post-order until no
block's input changes, with two bits per tracked object joined by union:
**A**, "set on some path", and **C**, "a resumption edge was taken since
A was last reset".

| track | sets A and clears C | sets A | checks C (a hit) |
|---|---|---|---|
| rule 3 (per slot) | a whole write | a partial write | a read |
| rule 4 (per slot) | — | a `lifetime.start` at offset 0 | every use and every marker |
| value (per instruction result used outside its block) | its definition | — | every use |

- On a resumption edge, C |= A. On a ramp-only edge, C := 0. At a cut,
  C := 0 for every track. Other edges, the suspended edge included,
  carry both bits unchanged.
- A `phi` reads its incoming value at the end of the predecessor, on
  that edge.
- A track with a hit is in the heap: the slot (rules 3 and 4) or the
  value. A value is also in `heap_values` when it is a `phi` incoming
  value on a resumption edge.
- Not tracked as values (the code generator recomputes or never keeps
  them): the results of `coro.id`, `coro.save`, `coro.begin`,
  `coro.frame`, `coro.size`, `coro.align`, `coro.alloc`, `coro.suspend`,
  `coro.is_in_ramp` and `coro.end`.
- Arguments are not values here: an argument used after resumption is
  refused (`coro:argument-across-suspend`). Uses in the ramp after a cut
  (the `sret` pointer, a callee-destroyed parameter's destructor) and on
  the ramp-only side of an unwinding `coro.end` are not after
  resumption and are not refused.

### 3.5 Before `coro.begin`

- A heap slot with a use (not a marker) before `coro.begin` is listed in
  `begin_copies`: those uses address a machine-stack shadow, copied into
  the heap slot just after `coro.begin`. (The parameters a promise's
  `operator new` takes by reference.)
- A heap value defined before `coro.begin` is listed in
  `begin_value_copies`: it is stored to a machine-stack home where it is
  defined and copied to its heap home just after `coro.begin`. (`this`,
  a lambda's closure pointer, a VLA bound, each loaded before
  `coro.id` and used after a suspend.)
- A use, after `coro.begin`, of a heap slot through an alias defined
  before it is refused: `coro:frame-before-begin` (the alias would still
  point at the shadow).
- Every parameter is copied to its own slot right after `coro.begin`. A
  parameter alloca read only by the allocation's arguments and by that
  copy stays on the stack (rule 3) or is a `begin_copies` slot (rule 5,
  when the allocation takes its address). A callee-destroyed parameter
  whose destructor runs in the ramp after `coro.end` is rule 5 unless
  marked `!coro.outside.frame`, and is then refused by the safety
  conditions (§3.6).

### 3.6 The suspended region and the safety conditions

- `suspended_region` holds every block reachable from a suspended edge,
  and the blocks after a wrapper's own block on its path to its suspend
  (an `invoke`'s normal edge, an unconditional branch, the true side of
  a `.bool` wrapper's result). The instructions after the wrapper in its
  own block belong to the region too. A suspend without a wrapper (no
  such body comes from kiln, which always calls `await_suspend` through
  a wrapper) starts the region at its `coro.save`.
- **After free**: the instructions after a call (not an intrinsic) that
  takes a `coro.free` result as an argument, in its block, and every
  block reachable from that block's successors.
- **Safety conditions**: no use or definition of a heap value, no use of
  a heap slot (markers excepted), and no `phi` incoming heap value on an
  edge out of such a place or on a suspended edge, is in the suspended
  region or after free. Each one is a `Violation { slot, value, at,
  region }` (`at` is the predecessor's terminator for an edge), and
  `plan` refuses the first with `coro:frame-access-after-suspend`.
- Invariants of every plan, asserted when it is built: no promise or
  outside-frame slot is in `heap_slots`; `begin_copies` ⊆ `heap_slots`;
  `begin_value_copies` ⊆ `heap_values`; `frame_align` ≥ 16 and a power
  of two; `promise_offset` ≥ 16; suspend indices are their positions and
  the final suspend is last.

### 3.7 The header and the suspends

- `promise_offset` = alignTo(16, align(promise)); 16 without a promise.
- `frame_align` = the largest of 16, the promise's alignment, every heap
  slot's alignment and every heap value's type alignment.
- Suspends are indexed in instruction order, the final suspend last.
  `final_index` is its index; `has_unwind_end` is true iff some
  `coro.end` has `i1 true`.

### 3.8 Refusals

| name | when |
|---|---|
| `coro:variadic` | the function is variadic |
| `coro:begin-shape` | not exactly one `coro.id` and one `coro.begin`; a promise operand that is neither null nor a slot at offset 0; `coro.begin` reachable from itself |
| `coro:frame-intrinsic` | `llvm.coro.frame` |
| `coro:dynamic-alloca` | an `alloca` that is not a static slot, `llvm.stacksave`, `llvm.stackrestore` |
| `coro:suspend-shape` | a `coro.suspend` whose token is not a `coro.save`, whose result has another use than one `switch` right after it with exactly the cases 0 and 1; a second final suspend; a wrapper (or wrapper-less save) from which no single path leads to a suspend, or two leading to the same one |
| `coro:unwind-end-without-final` | an unwinding `coro.end` without a final suspend |
| `coro:outside-frame-across-suspend` | §3.3 rule 2 |
| `coro:argument-across-suspend` | §3.4 |
| `coro:frame-before-begin` | §3.5 |
| `coro:frame-access-after-suspend` | §3.6 |
| `coro:overaligned-frame` | `frame_align > 16` in a body that does not call `coro.align` |

A body with several refused constructs gets one of their names; which is
unspecified, except that `coro:frame-access-after-suspend` and then
`coro:overaligned-frame` are given only to a body with no other.

## 4. Validation

- `tests/coro_plan.rs`: each rule (both clauses of rule 4, rule 5 with
  and without markers, aliases, an alias across a suspend, values, `phi`
  on a resumption edge, both kinds of `coro.begin` copies, a member
  function using `this` after a `co_await`, parameter copies, a
  callee-destroyed parameter) and each refusal by name, on minimal
  bodies in both dialects.
- `tests/coro_seeds.rs`: design 0005's seeds `escaped-local`,
  `params-before-begin`, `gro-throws-uaf`, `post-sync`,
  `resume-inside-await-suspend`, compiled by kiln without LLVM's passes
  and cut to their coroutines (`tests/data/seeds/`), with and without
  their lifetime markers; and the same plan from `hod_bc` as from
  `hod_ll`.
- `tests/coro_census.rs`, with `HOD_XFORM_PREOPT` naming a directory of
  pre-optimisation modules: for every coroutine, `opt
  -passes=coro-early,cgscc(coro-split),coro-cleanup` (of the module's
  LLVM major) builds `%F.Frame`; each alloca it keeps there must be in
  `heap_slots` ∪ `begin_copies` ∪ {promise} unless hod classes it by
  rule 3, and each spilled value must be in `heap_values` (matched by
  the call's callee, else the opcode). The only refused bodies may be
  those of the contract-postcondition modules and `post-sync`, as
  `coro:frame-access-after-suspend`. Numbered values are renamed first
  so that `opt`'s field names (`%vN.reload.addr`, `%vN.spill.addr`)
  identify the slot (the `N`-th entry `alloca`) or the value.

## 5. Cost

Linear in the instructions plus the blocks times the tracked objects in
64-bit words, times the passes of the dataflow (the loop depth plus
two). No allocation per instruction: the tables are per body (events,
uses, aliases, the bit sets). Measured in `spec/landed/xform/WP-104.md`.
