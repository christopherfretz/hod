# spec: hod_cg — code generation

Status: **design** (supervisor-owned, WP-014). The T-CG work packages
implement it and replace each "Signatures" note with the real API, the way
`spec/ir.md` was filled in. Normative for `crates/hod_cg` and for what a
target crate (`hod_a64`, `hod_x64`) must provide. Background:
`docs/ARCHITECTURE.md` "Code generation".

How IR-level types are passed and returned is not here: one file per
target (`abi-a64.md`, `abi-x64.md`). What `hod_eh` and `hod_dwarf` write
from the output record is in `eh.md` and `dwarf.md`. This file fixes the
three interfaces those documents and the instruction-group work packages
meet at: **the value-location policy** (§3, §4), **the `Target` trait**
(§8) and **the per-function output record** (§9).

## 1. Shape

`hod_cg` compiles one `hod_ir::Body` at a time into machine code, in **one
walk over the instructions**. For each instruction, in layout order, the
target selects, allocates and encodes in one step and appends bytes to a
code buffer. There is no machine IR and no second pass over emitted code;
what cannot be known when an instruction is emitted (a forward branch's
distance, the frame size) is a fixup patched when it becomes known.

What code generation needs to know about a function before it starts is
either in the body already or cheap:

- use facts for every instruction result and argument (`spec/ir.md` §5.6),
  maintained by the IR builder;
- the fixed stack objects (`Body::slots`, `spec/ir.md` §5.2);
- which blocks can be reached (§6.1): one walk over the terminators.

Everything is generic over the target (`compile_function::<T>`), so the
target's lowering is inlined into the walk; there are no trait objects on
the per-instruction path.

**A function compile is restartable.** It appends to a worker-owned
`Chunk` (§9) and to scratch state that is reset per function. On any
error the chunk is truncated to where the function began and nothing of
the attempt remains. §6.3 uses this to retry a function in far-branch
mode.

**Preconditions.** `hod_ir::classify` has accepted the module and the
body (`spec/ir.md` §8), so every construct met is one a work package has
implemented and tested. Meeting anything else is `CgError::Internal`,
never a guess (§10). The IR is valid in LLVM's sense (definitions dominate
uses in reachable code); code generation does not verify it. Besides whole
instructions and types, this design relies on `classify` rejecting:

- a thread-local symbol as the callee of a call, and the address of a
  thread-local symbol inside a constant other than a plain address
  operand (an initializer, an aggregate or nested constant, a symbol
  difference, a landing-pad clause) (§3);
- a `blockaddress` in a body that names a block of another function
  (§9.2);
- `naked` functions (§5.4);
- a `musttail` call whose stack-argument area is larger than the
  caller's own, or whose calling convention has the callee pop its
  arguments (§7.1).

## 2. Registers and value classes

### 2.1 Physical registers

`PReg` is a `u8` index into the target's register file; `RegSet` is a
`u64` bit set over it (AArch64: 31 general and 32 vector registers; x86-64:
16 and 16). The target describes its registers once, in a `RegInfo`:

| field | meaning |
|---|---|
| `gpr`, `fpr` | the registers of each class |
| `pool` | registers a fresh value may be allocated to. None is reserved (below), and none is written by argument placement or by a return under any supported calling convention (argument and return registers, the indirect-result register, the `nest` register). Each is caller-saved or in the target's save set (below) |
| `holdable` | registers a value may rest in: `pool` plus the registers argument placement and returns write. An incoming argument rests in its argument register and a call's result in the return register until used or displaced; one that arrives in a register that is not holdable (x86-64's `rcx`, `rdx`, `rax`) is stored to its home, or moved to a temporary, at once |
| `scratch` | at least two general and one vector register that never hold a value across instructions. They belong to whatever is being emitted right now: glue (§8.2), large offsets, phi copies. On AArch64 these include `x16`/`x17`, which a linker veneer may clobber at any call |
| `eh_data` | the two general registers a landing pad receives (exception pointer, selector); not necessarily holdable (x86-64: `rax`, `rdx`) |
| `dwarf` | each register's DWARF number, for CFI (a function from `PReg`) |
| `callee` | the scratch register an indirect callee's address is brought into (§7.1) |
| `gpr_bytes`, `fpr_bytes` | the size of a register of each class: the size of its home and spill slots |

Never touched: the platform register (`x18`), the thread pointer. `pool`
and `holdable` are disjoint from `scratch`.

**Save set.** A target may name a set of callee-saved registers that its
prologue may save and its epilogue restore (§5); x86-64 saves only those
of them the function used (§5.6.15). They are then ordinary pool registers;
§4.4 displaces them at calls like any other. No other callee-saved
register (besides the frame pointer and the link register) is used in v1.
AArch64 has enough caller-saved registers and an empty save set (§5.5).
x86-64 has only `r10` and `r11` that are caller-saved and written by no
argument placement or return, so its pool is a save set (§5.6).

**Minimum.** Every target's pool has at least **four** general and **two**
vector registers. A lowering pins at most that many of a class at once
(§4.2), counting operands, results and temporaries; what it needs beyond
that it takes from `scratch` or does through memory.

### 2.2 Value classes

The target maps every first-class IR type to a class
(`Target::class_of`):

| class | holds | examples |
|---|---|---|
| `Gpr` | a value in one general register | integers up to the register width, pointers |
| `Fpr` | a value in one floating-point/vector register | `float`, `double`, `half`, vectors that fit the register, `fp128` where the ABI passes it in a vector register |
| `Mem` | a value that lives only in memory, in a frame slot of its alloc size | integers wider than a register (`i128` included), structs and arrays as first-class values, wider vectors, `x86_fp80` |

`Mem` is deliberate: a first-class aggregate or `i128` is rare in `-O0`
C++ IR (a landing pad's `{ptr, i32}`, an overflow intrinsic's result, a
coerced small struct), and operating on it slot-to-slot needs no
multi-register bookkeeping. Calls marshal `Mem` values to and from the
registers the ABI names.

**Width invariant.** In a general register the bits above an integer
type's width are **unspecified**. A consumer that depends on them
(`zext`, `sext`, compares, division, right shifts, `switch`, a conversion
to floating point, `inttoptr` from a narrower integer, an index or an
`alloca` count narrower than a pointer) extends first. An argument or a
return value is extended as the target's ABI document requires of its
producer (`abi-a64.md` §7: with `zeroext`/`signext`, and a plain `i1` to
8 bits), and what arrives may be assumed extended only as far as that
document lets its consumer assume. A conditional branch or a
`select` on `i1` tests bit 0. In memory an integer occupies
its store size with the bits above its width zero (`spec/ir.md` §4.3);
`store` writes that and `load` may rely on it. A spill slot is a full
register wide, written and read whole, so the invariant carries through a
spill. A home or incoming slot of an integer is its store size rounded up
to a power of two (§5.1 "Value slots"), written and read with exactly
those bytes: the bits above the width are unspecified in the register
after the load, as the invariant allows.

## 3. Where a value lives

Each value that code generation tracks (an instruction result, an
argument) has a location, kept in a per-function table indexed by the
value's id and reset between functions:

| location | meaning |
|---|---|
| `None` | not yet defined, or dead |
| `Reg(r)` | only in register `r` |
| `Frame(ref)` | in a frame slot (§5.1): its **home** for a value that has one, a **spill** slot for a register value that was displaced |
| `Deferred` | not computed: the next instruction will fuse it or ask for it (§4.6) |

A `Mem`-class value is always `Frame`. The other operand kinds have no
table entry and are produced where they are used: a constant is
materialized into a register (or folded into the instruction when the
target's encoding allows), a symbol address is formed by the target's
addressing sequence (§7.4), a stack-slot address is the frame pointer plus
a constant, and a load or store *through* a stack-slot address addresses
the frame directly.

Producing an operand never calls anything and writes no register but its
destination and `scratch`, so it is safe at any point of an instruction's
lowering, argument placement included. A thread-local address is an
operand too (WP-073): its access sequence may call the TLS descriptor
resolver, but it saves and restores what that call clobbers, so it
writes its destination and nothing else either (§5.5.1, "Thread-local
storage and symbol offsets (WP-073)"). `llvm.threadlocal.address` is
the operand's value.

## 4. Value-location policy v1

The policy is chosen for being simple to get right on unoptimized IR, where
most values are used once, soon after they are defined. It sits behind the
`FnCx` services of §8.1; a later policy can replace it without touching the
targets' lowering code.

### 4.1 At a definition

From the use facts of the result (and its class):

| facts | the value is | it lives |
|---|---|---|
| no use | **dead** | nowhere. The instruction is still emitted if it has an effect (§4.7) |
| one use, in the defining block | a **register value** | in a register from its definition to its use (class `Gpr` or `Fpr`) |
| more than one use, or a use in another block, or named by a `#dbg_declare` | a **home value** | in a home slot. The defining instruction computes into a register and stores it at once; the register is then free. Every use loads it |
| any, class `Mem` | a home value | in its home slot; the defining instruction writes the slot |

The facts are `spec/ir.md` §5.6's, for instruction results and arguments
alike, read as follows; the policy is only sound with these readings:

- Each operand slot counts once: a value an instruction uses twice has
  two uses.
- A use by a `phi` is a use **at the terminator of the incoming block**,
  not in the `phi`'s block, because that is where §4.5 reads the value.
  Each entry of the `phi` counts.
- "The next instruction" (§4.6) is the next instruction of the same block
  and is not a `phi`; a use by a `phi` never qualifies.
- A `#dbg_declare` that names a value which is not a stack slot (an `sret`
  argument, a dynamic `alloca`) is not a use, but makes the value a home
  value, so that the variable has one location for the whole function
  (§9.5). This is the only way debug information changes the code.

A home slot is assigned on the first mention of the value, definition or
use, so blocks may be laid out in any order. A home slot is never reused
while its value can still be read: only the home of a value whose
definition and every use are in one block (a **block slot**) is handed
out again, in another block (§5.1 "Value slots").

### 4.2 At a use

- A register value is in its register, or in the spill slot it was
  displaced to; in the second case it is loaded into a free register.
  Either way this was its only use: the register (and the spill slot) are
  free once the using instruction is emitted. The result of that
  instruction may take over the register.
- A home value is loaded from its home into a free register for the
  duration of the instruction, or addressed in place when the instruction
  can take a memory operand.
- Registers holding the current instruction's operands and results are
  **pinned** until it is emitted: nothing below displaces them. A lowering
  never pins more registers than the pool's minimum (§2.1); operand lists
  of unbounded length (call arguments, `PtrAdd` indices, `switch` cases)
  are consumed one at a time.
- When an instruction has been emitted, or skipped (§4.7), **the driver**
  releases every register value among its operands that is still held. A
  lowering need not read every operand (`llvm.va_end` reads none), and a
  skipped instruction's lowering never runs. Releasing a `Deferred`
  operand (§4.6) releases the deferred instruction's operands in turn.

### 4.3 Running out of registers

When a register is needed and no pool register is free, an unpinned
register value is **displaced**: stored to a spill slot (allocated from
the frame, 8 or 16 bytes by class; spill slots are reused once their
value's use has been emitted). After a call to a `returns_twice`
function no spill slot is reused for the rest of the function: the code
after such a call can run a second time and reads its slots again (v1
does not track which ones were occupied at the call). A target lowering
whose code can run a second time without a call instruction sets the
same state with `FnCx::never_reuse_spill_slots` (x86-64's
`llvm.eh.sjlj.setjmp`, §5.6.18 (4)). The choice of register
must not depend on anything but the function's IR: v1 takes the lowest-numbered unpinned occupied pool
register. A lowering that needs one particular register (a return
register, a shift count register on x86-64) displaces its occupant the
same way, or moves it to a free pool register.

### 4.4 Calls

A call clobbers every `holdable` register. Before the arguments are
placed:

1. every register value that is not an operand of this call is displaced
   to a spill slot;
2. every operand of this call that rests in a register outside `pool` (an
   argument or return register) is displaced too.

What remains in registers are operands of this call in `pool` registers.
Argument placement then reads from pool registers, frame slots and
constants and writes to argument registers and the outgoing stack, and the
two sets are disjoint: there is no parallel-move problem to solve. The
call's result is left in the return register as an ordinary register value
(`holdable`), or stored to its home. (A `musttail` call writes the
caller's own incoming arguments, which are homes; §7.1 has its rule.)

### 4.5 Block boundaries and `phi`

**At the end of every block no register holds a value.** This follows
from §4.1 for valid IR (a register value's one use is in its own block);
code generation checks it and reports `CgError::Internal` otherwise. So
there is no register state to reconcile on any control-flow edge.

A `phi` has, besides its location as a value, an **incoming slot**, laid
out like a home slot of its class (§2.2). The protocol:

- A terminator, before it transfers control, stores to the incoming slot
  of every `phi` of every successor the value that `phi` lists for this
  block. It does so for **all** successors, whichever is taken. (A value
  listed for this block dominates the end of this block, so it is
  available on every path out of it.) Sources are read from value
  locations; only incoming slots are written. A successor that the
  terminator lists more than once is stored once: a `phi`'s entries for
  one predecessor are equal (and, being several uses, make their value a
  home value).
- At the head of its block a `phi` is lowered like a load from its
  incoming slot, with the result placed by §4.1.

The driver does these stores before it lowers the terminator (for an
`invoke`, the call sequence does them at their two places), so a
terminator's lowering does not. The exception is a `callbr` whose result
a successor's `phi` takes: the driver skips that successor and the
`callbr`'s lowering stores its `phi`s on each edge to it, after the
result has its location (WP-181). A successor so skipped that the
`callbr` lists more than once is refused as
`call:callbr-result-repeated-edge` (each listing is its own code path on
AArch64, and a successor's stores are written once per terminator).

No edge is ever split and no parallel copy is ever sequenced: the values
read (locations) and the slots written (incoming slots) are disjoint, so
the "swap" and "lost copy" problems cannot occur, a conditional branch,
`switch` or `indirectbr` needs no per-edge code, and a write for an edge
that is not taken is dead (every edge into a block rewrites the slots
before the block reads them).

`invoke` and `callbr` are the supported terminators whose result can be
a successor's incoming value. Its unwind destination's incoming slots are
written **before** the call (the result is not available on that edge); its normal destination's
incoming slots are written **after** the call, once the result has its
location.

The cost is one extra slot and one extra load per `phi`, and stores that
are executed for edges not taken. `phi`s are infrequent in the input
(short-circuit operators, conditional expressions), and none of it costs
compile time.

### 4.6 Fusing with the next instruction

When a result's only use is by the instruction immediately after it
(`spec/ir.md` §5.6), the target may leave the instruction unemitted and
mark the result `Deferred`. Its operands stay where they are: no code is
emitted in between. The next instruction's lowering either fuses the pair
(a compare into the branch or `select` that consumes it, a `PtrAdd` into
the addressing mode of a load or store) or asks for the value, which
emits the deferred instruction then. Deferral is an option of each
lowering, never an obligation, and is the only peephole in v1. A run of
deferred instructions (one deferred instruction consuming another) is at
most `MAX_DEFERRED_RUN` (8) long. A lowering asks for an operand that may
be deferred before it pins any other operand (§8.1), and the call
sequence emits deferred arguments before it displaces anything.

### 4.7 Dead results and undefined operands

- An instruction with no use is skipped when it has no effect: arithmetic,
  compares, casts, `PtrAdd`, `select`, `freeze`, the aggregate and vector
  element instructions, and a load that is neither volatile nor atomic.
  Everything else is emitted and its result dropped. A `Mem`-class result
  with no use gets a slot that lives for the instruction only
  (`FnCx::def_mem`).
- `undef` and `poison` operands are zero (`spec/ir.md` §4.1). `freeze` is
  a copy.
- Behavior LLVM leaves undefined (a shift by the width or more, a
  division by zero, signed overflow under a flag hod does not keep) needs
  no handling: whatever the instruction does is correct.
- `unreachable` emits the target's trap instruction, so control never
  falls into whatever follows.

### 4.8 Arguments and landing pads

- On entry an argument passed in a register rests there, as a register
  value, if its use facts say "one use, in the entry block"; otherwise the
  function's entry code stores it to its home. An argument passed on the
  stack has its incoming location as its home (`FrameRef::Incoming`, when
the argument is wholly in one stack part: of that part's width); a
  `Mem`-class argument passed in registers is stored to a home slot in the
  entry code. An argument whose value is the address of memory the caller
  passed on the stack (`byval`) is computed by the entry code from the
  incoming area's address and then treated as a definition (§4.1).
- On entry to a landing-pad block the two `eh_data` registers are reserved
  until the `LandingPad` instruction (the first instruction after the
  `phi`s) has stored them into its result.

## 5. The frame

### 5.1 Slots

`Frame` is a per-function bump allocator over one **local area**. It hands
out `FrameRef::Local(offset)` for a size and an alignment up to the
target's stack alignment; offsets only grow, and the area's final size is
known when the function has been emitted. The target turns a `FrameRef`
into an addressing mode (§5.4). The local area holds, in this order:

1. what the target reserves in `begin_function` (the frame record, a
   register save area for a variadic function, the incoming-argument base
   when the target needs one);
2. the body's stack slots (`Body::slots`), laid out by the driver after
   `begin_function` and before the first instruction, so `Value::slot(n)`
   is a constant from the start;
3. home slots, incoming slots and spill slots, in order of first need,
   some of them in the alignment holes of 2. and 3. ("Value slots"
   below).

`FrameRef::Incoming(offset)` names the caller's stack-argument area.

**Value slots** (WP-198). Home, incoming and spill slots are *value
slots*; the body's stack slots and the slots a lowering takes with
`FnCx::alloc_slot` are not, and are laid out as above, never moved,
never shared (the stack protector's guarded block, §5.6.24, is one of
them and keeps its place). Three rules make value slots small, none of
them an analysis:

1. **Size.** A home or incoming slot of class `Gpr` holding an integer
   is the integer's store size rounded up to a power of two (1, 2, 4 or
   8 bytes); of class `Fpr`, 4 bytes for a `float` and 8 for a `double`;
   anything else of those classes (a pointer, a vector) a register's
   bytes; of class `Mem`, the type's allocation size at its alignment
   (capped at the stack alignment). Each is aligned to its size, written
   and read with exactly that many bytes, phi copies included (§4.5). A
   spill slot stays a register wide (§4.3). In a coroutine body (§7.6)
   every `Gpr` and `Fpr` home is a register wide: its shadow homes are.
2. **Holes.** The bytes a slot's alignment skips are remembered (at most
   16 holes per function); a new value slot of at most 16 bytes goes in
   the lowest hole that holds it at its alignment before the area grows.
   Only value slots fill holes, so the body's slots keep the offsets the
   bump gives them.
3. **Block slots.** The home of a value that is an instruction result
   (not an argument, not a `phi`), defined in the block being emitted,
   with no use outside that block (a use by a `phi` is a use in the
   incoming block, §4.1) and not named by a `#dbg_declare`, is a block
   slot: it holds the value from its definition to the end of the block
   and is free again when the next block begins. Every use of the value
   is emitted inside the block, so no read of it follows the block's
   end in any execution that did not pass the definition again. Block
   slots of up to 16 bytes come in size classes of 1, 2, 4, 8 and 16
   bytes, aligned to the class (or the stack alignment if smaller); a
   block takes a free slot of the request's class, else of the next
   larger class that has one, else makes a slot of the request's class.
   Each class's slots are taken in the order they were made, so a
   block's state is a count per class. A larger `Mem` block slot is a
   plain value slot, never reused. An `insertvalue` of class `Mem` whose
   home is a block slot takes over the block slot of its aggregate
   operand when that operand is a `Mem` home in a block slot, defined in
   the same block, with this as its only use: the lowering copies the
   aggregate into the result (nothing when the two are one slot) and
   then writes the field.

   There are no block slots in a function where code can run a second
   time or where the debugger reads a value's location over a range:
   - a coroutine body (§7.6);
   - a function with a `#dbg_value` record (`spec/dwarf.md` §6.23: a
     record's range may outlast the block);
   - a function that calls a `returns_twice` function or sets
     `FnCx::never_reuse_spill_slots` (§4.3): the code after the call can
     run again after other blocks have run. The driver learns this
     during emission; it then throws the function's code away and
     compiles it again without block slots, as it retries a function in
     far-branch form (§6.3).

These rules change where values live, never the code's semantics: a
named variable's storage is its body slot or a `#dbg_declare` home, and
neither is ever a block slot.

**Over-aligned slots.** A stack slot whose alignment exceeds the stack
alignment is not in the local area. The entry code
(`Target::dynamic_slot`) allocates it
dynamically (as §5.2) and keeps its address in an ordinary pointer slot;
`Value::slot(n)` for it is a load of that pointer, and its debug location
is the pointer slot, dereferenced. The frame itself is never realigned,
so frame-pointer-relative addresses and the CFI stay constant.

### 5.2 Dynamic allocation

`Alloca` (and the entry code for over-aligned slots) lowers the stack
pointer by the rounded size and yields the new stack pointer, aligned as
asked. Nothing in the frame is addressed from the stack pointer, so no
other code changes. `llvm.stacksave` and `llvm.stackrestore` read and
write the stack pointer. The epilogue restores the stack pointer from the
frame pointer.

### 5.3 Outgoing arguments

There is no reserved outgoing-argument area. A call that passes anything
on the stack lowers the stack pointer by the (aligned) size before placing
arguments and raises it after the call returns; stack arguments are
written through `FrameRef::Outgoing(offset)`, an offset from the lowered
stack pointer. Calls without stack arguments, the usual case, emit
neither. The canonical frame address is
defined from the frame pointer (§5.5), so these adjustments need no CFI.

An `invoke` that lowered the stack pointer must have it raised on the
unwind edge as well: the unwinder enters a landing pad with the stack
pointer as it was at the call, and the frame may hold dynamic allocations,
so the pad cannot recompute it. Such an `invoke` gets a **pad stub**,
emitted right after its branch to the normal destination: raise the stack
pointer by the same amount, then branch to the landing-pad block. The
stub, not the block, is the landing address in the call-site row (§9.4).
It leaves the `eh_data` registers alone.

### 5.4 Prologue, epilogue and limits

- **Always a frame pointer**, with the standard frame record, so stack
  walkers that follow the frame chain work without unwind tables.
- `Target::begin_function` emits the prologue in a **fixed-size form**
  whose immediates are not yet known, allocates what it needs from the
  frame, and emits the entry code of §4.8. `Target::end_function`, called
  when the local area's size is final, patches the prologue, emits the
  function's **exit sequences** at its end, and describes the frame for
  unwinding (§9.3). The exit sequences are the **one epilogue** that
  returns and, after it, one **tail exit** per `musttail` call (§7.1):
  the same teardown ending in a branch to the callee.
- `Ret` places the return value and branches to the epilogue; a `Ret` that
  ends the last emitted block falls through. Because every exit sequence
  is emitted when the frame size is known, nothing but the prologue is
  patched for it.
- A frame the fixed-size prologue cannot express is
  `Unsupported("limit:frame-size")` for that function (§10).
- A `naked` function has no frame and so cannot be compiled this way;
  `classify` rejects it (§1).

### 5.5 AArch64 (normative for `hod_a64`)

```
            | caller's frame                       |
entry sp -> +--------------------------------------+  = fp + FRAME   (CFA)
            | local area, upward from fp + 16:     |
            |   target reservations, body slots,   |
            |   home / incoming / spill slots      |
            | saved lr                 [fp + 8]    |
  fp, sp -> | saved fp                 [fp + 0]    |  frame record
            +--------------------------------------+
            | dynamic allocations, outgoing args   |  (sp moves below fp)
```

- `FRAME` is the local area's size rounded up to 16. The frame record is at
  the **bottom** (AAPCS64 does not fix its position), so every local is at
  a small **positive** offset from `fp`, which the scaled unsigned-offset
  load/store form reaches without a scratch register for frames up to
  32 KiB of 8-byte slots. `FrameRef::Local(o)` is `[fp, #o]`; the first 16
  bytes of the local area are the frame record.
- Prologue (16 bytes, fixed): `sub sp, sp, #lo12` · `sub sp, sp, #hi12,
  lsl #12` · `stp x29, x30, [sp]` · `mov x29, sp`. `FRAME` up to 2^24 - 16.
- Incoming stack arguments are at `fp + FRAME + n`, which is not known
  while the body is emitted. A function that needs the address of the
  incoming area (it has stack-passed or `byval` arguments, or is variadic:
  known from its signature before anything is emitted) keeps it in a
  pointer slot, the **incoming base**: its prologue is preceded by
  `mov x16, sp` and followed by a store of `x16` to that slot.
  `FrameRef::Incoming(n)` is addressed through the slot.
- Epilogue: `mov sp, x29` (only if the function has dynamic allocation,
  §5.2; adjustments for outgoing arguments are undone on every path,
  §5.3) · `ldp x29, x30, [sp]` · `add sp, sp, #FRAME` (one or two
  instructions) · `ret`. A tail exit is the same with `b callee`, or
  `br x16` for an indirect callee, in place of `ret`.
- CFI: after the prologue the CFA is `x29 + FRAME`, with `x29` saved at
  `CFA - FRAME` and `x30` at `CFA - FRAME + 8`, for the whole body. The
  rows for the prologue instructions and for each exit sequence are a
  constant pattern in `FRAME` (§9.3).
- Pool: `x9`..`x14`, `v16`..`v29`. Holdable: the pool, `x0`..`x8`, `x15`
  (LLVM passes a `nest` argument there), `v0`..`v7`. Scratch: `x16`,
  `x17`, `v30`, `v31`. Landing-pad data: `x0`, `x1`. The save set is
  empty.
- Mapping symbols: a function's code starts in A64 state (`$x`); a data
  island inside a function (§9.2) is bracketed `$d` … `$x`.
- With branch protection on (module property), the function starts with
  the matching `bti`/`paciasp` forms. They are not specified yet
  (`abi-a64.md` §1.1); until they are, `classify` rejects the property.

#### 5.5.1 Settled by the skeleton (WP-033)

These are decisions the group packages (WP-035..041) build on.

- **Crate layout.** One module per instruction group; the ownership table
  is the crate documentation of `hod_a64` (`src/lib.rs`). The dispatch in
  `target.rs` is final: it routes each instruction by opcode and by the
  class of the value it operates on (the result's; for `icmp` and a store
  the operand's; for a cast either side, `Mem` before `Fpr`): `Gpr` to
  `int`, `Fpr` to `float` (with `fcmp`), `Mem` to `aggr` (with
  `extractvalue`, `insertvalue`); loads and stores of a `Gpr` or `Fpr`
  value to `mem`, of a `Mem` value to `aggr`; control to `control`, calls and unwinding to
  `call`, intrinsics other than the stack, `va_*` and overflow ones to
  `intrinsic`. A stub refuses with its group's stable name
  (`a64:int-binary`, `a64:load`, `a64:condbr`, `a64:call`,
  `a64:fp-binary`, `a64:mem-binary`, `a64:intrinsic`, …; each a `pub
  const` of its module), so `cargo test` of a package's file is its whole
  contract. Shared helpers live in `common.rs`, one function each.
- **Classes.** Integers up to 64 bits and pointers are `Gpr`; `half`,
  `bfloat`, `float`, `double`, `fp128` are `Fpr`; a vector is `Fpr` when
  its length is a power of two, its size 64 or 128 bits and its element
  not `i1`; everything else (wider integers, aggregates, other vectors)
  is `Mem`.
- **Prologue rows.** The prologue is emitted with placeholder immediates
  by `begin_function` and patched in `end_function`, when `FRAME` is
  known. Its CFI rows are added then too, with `FnCx::cfi_at` (§8.1),
  which inserts rows at an offset that is already emitted as long as the
  rows stay in offset order. For the prologue at `p`: `def_cfa_offset lo`
  at `p+4` (if `lo != 0`), `def_cfa_offset FRAME` at `p+8` (if `hi !=
  0`), `offset x29, -FRAME` and `offset x30, -FRAME+8` at `p+12`,
  `def_cfa_register x29` at `p+16`. An exit sequence at `e` (after
  `mov sp, x29`, if any): `def_cfa sp, FRAME` at `e`, `restore x29, x30`
  after the `ldp`, `def_cfa_offset lo` between the two `add`s when there
  are two, `def_cfa_offset 0` after the last; a tail exit then restates
  the body's rows for the code that follows it.
- **Incoming base slot** is `Local(16)`, directly above the frame record,
  when the function needs one; the saved `x8` of a function that returns
  in memory follows it. A function without the slot that addresses
  `Incoming` is a bug and traps (`brk #0xf000`).
- **Far mode.** `b`, `bl` reach ±128 MiB in far mode as well; a
  conditional branch, `cbz`/`cbnz` and `tbz`/`tbnz` become their inverse
  over a `b`. A function that a `b` cannot span is
  `limit:function-size`.
- **Fixup kinds** (`FixupKind(n)`, patched by `hod_mc_a64::Fixup`): 0
  `b`/`bl` imm26, 1 `b.cond`/`cbz` imm19, 2 `tbz` imm14, 3 `ldr`
  literal imm19, 4 `adr` imm21. `at` is the instruction's own offset.
- **Constants.** `fp128` is built in the register from its two halves
  (`mov_imm` into `x16`, `fmov d`, `fmov v.d[1]`), not loaded from a data
  island (§7.5): no island, no fixup, and the same in far mode. A vector
  constant other than zero is `a64:constant` until WP-039. A `Mem`-class
  constant is stored into its home: zero-like ones with `xzr`, wide
  integers word by word, other aggregates from their flattened image,
  whose only accepted relocations are aligned 8-byte symbol addresses
  (else `a64:constant-relocation`; WP-072 writes every other relocation
  of a non-vector image, "Constant operands with addresses" below). Symbol addresses follow §7.4:
  `adrp`+`add` with the addend folded as `llc` folds it (WP-073), or
  `adrp`+`ldr` from the GOT then the addend added; a code model other
  than small is `a64:code-model`.
- **Other reason names of the target**: `a64:atomic`,
  `a64:vector-element` (no longer emitted: WP-079 lowers the element
  operations), `a64:thread-local` (no longer returned: WP-073 lowers TLS),
  `a64:funclet` (never on ELF), `a64:blockaddress` (WP-037),
  `a64:variadic-definition` (WP-038).
- **The differential harness** is `crates/hod_a64/tests/diff.rs`: what it
  generates is the table `tests/common/groups.rs`, one line per landed
  group. `hod_interp`'s generator always emits integer arithmetic, so the
  harness runs the skeleton's own programs (`tests/common/skeleton.rs`)
  until WP-035 switches the generator on; it prints the missing mask (the
  generator switches still off).

#### Integers (WP-035)

`src/int.rs`: binary operations, `icmp`, casts, `select`, `freeze` on
`Gpr` values of every width (`i1` to `i64`, pointers).

- **Narrow values** follow §2.2: the bits above the width are
  unspecified, so an operation extends an operand only where its result
  depends on them: the left operand of `lshr`, `udiv`, `urem` (`ubfx`)
  and `ashr`, `sdiv`, `srem` (`sbfx`), and the divisor; a register shift
  amount of a width below 5 (`lslv` reads 5 or 6 bits). A constant is
  materialized already extended. A constant `lshr`/`ashr` of an odd
  width is one `ubfx`/`sbfx`. A remainder is `udiv`/`sdiv` into `x17`,
  then `msub`.
- **Compares of an odd width** shift instead of extending: with `s` =
  register width − width, `lsl a, a, #s` then `cmp a, b, lsl #s`, which
  orders both signed and unsigned. With a constant, the operand is
  extended and compared with `cmp #k`, `cmn #-k`, or `x16`. A constant
  left operand swaps the predicate.
- **Every `icmp` defers itself** when `may_defer` allows (§4.6), and is
  otherwise `cmp` + `cset`. `int::fusable(cx, value)` is the deferred
  compare of an integer or pointer a value is, if any; `int::flags(cx,
  compare)` emits it and returns the condition true when the predicate
  holds. The consumer calls it after `deferred_first(cx, inst,
  Some(value))`, once per compare, and emits nothing that writes NZCV
  between it and its use; it pins at most two registers and writes them,
  NZCV and `x16`. `select` fuses into `csel`, `zext`/`sext` of an `i1`
  into `cset`/`csetm`; a non-fused `i1` condition is `tst #1`.
- **Undefined results** (§4.7): a constant shift amount of the width or
  more emits nothing; a `select` arm that is the constant 0 is `xzr`.
- **`addrspacecast`** is refused as `a64:addrspacecast` (classify
  rejects it first). `i128` is `Mem`: `aggr` (WP-040).
- **The differential harness** runs the generator from this package on
  (`narrow_ints`, `odd_ints`, `casts`, `compares`, `selects`); a program
  refused by another group's name is counted as skipped, by name, never
  as agreeing, and the proportion that ran end to end is printed.

#### Mem-class values (WP-040)

`src/aggr.rs`: integers wider than 64 bits, structs, arrays and `Mem`
vectors (§2.2), and the overflow intrinsics. Every entry point of
`target.rs` that meets a `Mem` value routes to `aggr`; `phi` of a `Mem`
value is the glue's copy (§4.5), so no other group's file has a hook.

- **The slot image.** A `Mem` value is the first store-size bytes of its
  slot; the bytes after are unspecified. A wide `iN` is `n = ⌈N/64⌉`
  little-endian limbs, and **the bits above `N` in the top limb are
  unspecified**, like a narrow value's (§2.2): an argument or a call
  result stores only the bytes of its last piece, and `add`, `sub`,
  `mul`, `shl` and the bitwise operations leave whatever the top limb
  computes. A consumer that depends on those bits extends the top limb
  first (`ubfx`/`sbfx` in a scratch register): compares, `lshr`, `ashr`,
  division, `zext`, `sext`, `store` and the integer fields an
  `insertvalue` writes, the wide overflow checks. A constant is
  materialized canonical. An aggregate or vector holds its exact memory
  image: its integer fields canonical, its padding unspecified.
- **A `store` writes exactly the store size**, the bits above an
  integer's width zero (a `load` then reads a canonical value); a `load`
  copies the store size. Both go through `x16`/`x17` and the pointer's
  register; an atomic one is `a64:atomic`.
- **Arithmetic** is limb by limb through `x16`/`x17`: `adds`/`adcs`/`adc`
  and `subs`/`sbcs`/`sbc` chains; `mul` of two limbs is `mul` + `umulh`
  + two `madd`s, wider a schoolbook product into the zeroed result. A
  constant shift is `extr` per limb (an amount of the width or more
  emits nothing, §4.7). A variable shift copies the source into a frame
  area of `2n` limbs whose other half is the fill (0, or the sign), picks
  the limb offset `min(s >> 6, n - 1)` by `csel` and combines two limbs
  per result limb with `lslv`/`lsrv` and a `lsr #1` · shift by `~s`
  (so a bit offset of 0 needs no branch); `ashr` also uses `lsrv`, the
  sign coming from the fill. `mul` and variable shifts are quadratic or
  need twice the frame, so beyond `MAX_QUADRATIC_BITS` (4096) they are
  refused as `a64:wide-mul` and `a64:wide-shift`; the linear operations
  have no limit.
- **Division up to 128 bits** calls `__udivti3`, `__divti3`,
  `__umodti3`, `__modti3` (`RuntimeFn`) with the operands in 16-byte
  slots, extended there first when narrower than 128 bits. Wider is
  `a64:wide-division`.
- **Compares**: `eq`/`ne` accumulate `eor`/`orr` over the limbs (the top
  limb shifted left by `64 - N mod 64`); the orderings are a `cmp`/`sbcs`
  chain over the limbs with both top limbs shifted the same way, which
  orders signed and unsigned alike; the operands are swapped for
  `ugt`/`ule`/`sgt`/`sle`. The result is a `cset` (no deferral).
- **Casts**: `zext`/`sext` fill with `xzr` or `asr #63` of the extended
  top limb; `trunc` to a `Gpr` width is the low limb, wider copies limbs;
  `ptrtoint`/`ptrtoaddr`/`inttoptr` go through the low limb; `bitcast`
  between `Mem` types copies, from a `Gpr` stores the masked register,
  from or to an `Fpr` value goes through `v31`. Conversions between wide
  integers and floating point are `a64:wide-fp-cast`.
- **Aggregates**: `extractvalue` reads the field at its offset (a `Gpr`
  or `Fpr` field into its register, a `Mem` field copied);
  `insertvalue` copies the aggregate, then writes the field (an integer
  masked to its width). `select` with a non-constant condition forms both
  addresses, `tst #1`, `csel` and copies; `freeze` copies (`freeze` of
  `poison`/`undef` is 0, as everywhere in hod). Layouts are `hod_ir`'s
  (`field_offsets`, `element_stride`); `{}` and `[0 x T]` have no bytes.
- **`Mem` vectors** (`<3 x i32>`, `<N x i1>`, 256-bit vectors, …) are
  loaded, stored, selected (scalar condition), frozen, merged by `phi`
  and bitcast; any element-wise operation, a vector `select` condition
  and a vector `icmp` are `a64:mem-vector`.
- **Overflow intrinsics** up to 64 bits extend the operands in their
  registers, then: unsigned add `adds` + `cs` at 64 bits, else `add` and
  a test of the bits above the width; signed add/sub `adds`/`subs` + `vs`
  at 64 bits, else the result compared with its own `sbfx`; unsigned sub
  `subs` + `cc`; multiply up to 32 bits is one `mul` with the same
  checks, wider `umulh`/`smulh` with `mul` (the signed form checks both
  that the high half is the sign of the low and the bits between the
  width and 64). The `{ iN, i1 }` result is written to its slot (the
  value masked to its width, the flag a byte at the field offset). Add
  and sub wider than 64 bits are a limb chain on extended operands;
  `mul` wider than 64 bits is `a64:wide-mul-overflow`.

#### Floating point (WP-039)

`crates/hod_a64/src/float.rs`; the evidence is `spec/landed/codegen/WP-039.md`.

- **Register contents.** A `float` lives in `Sn`, a `double` in `Dn`, a
  `half` in `Hn`, an `fp128` in `Qn`, a 64-bit vector in `Dn`, a 128-bit
  vector in `Qn`. The bits of the register above the value's width are
  unspecified, the same rule as for integers in a `Gpr` (§2). Every
  lowering reads only the value's own width.
- **`half`** is computed in single precision: `fcvt` to `S`, then the
  operation, then `fcvt` back to `H`. This is correctly rounded for
  `fadd`, `fsub`, `fmul` and `fdiv`. The reason is that single precision
  has more than `2·11+2` significand bits, so rounding twice gives the
  same result as rounding once. An integer converted to `half` goes
  through `S` the same way. That is exact as well: an integer of at least
  2^24 overflows `half` to infinity either way, and a smaller one is
  exact in `S`. `frem` of `half` is refused as `a64:half-frem`. `bfloat`
  has no arithmetic without FEAT_BF16, which the baseline lacks, so it is
  refused as `a64:bfloat` (but `fneg`, `bitcast`, `freeze` and `select`
  of it only move bits and are lowered).
- **`frem`** calls `fmodf` or `fmod` (LangRef: `frem` is `fmod`).
  `fadd`, `fsub`, `fmul` and `fdiv` of `fp128` call `__addtf3`,
  `__subtf3`, `__multf3` and `__divtf3`. `fneg` of `fp128` flips bit 127
  through `x16`. The other `fp128` operations are refused as
  `a64:fp128-libcall`, because their runtime functions are not in
  `hod_cg::RuntimeFn`: the compares (`__eqtf2` and others), the
  conversions (`__extend*tf2`, `__trunctf*`, `__fix*tf*`, `__float*tf`)
  and `frem` (`fmodl`).
- **NaNs.** hod emits the hardware instruction for each operation and
  keeps whatever NaN the hardware produces: the default NaN rules of
  FPCR with DN=0, with NaN operands propagated and made quiet. LangRef
  "Behavior of Floating-Point NaN values" allows any NaN, and
  `hod_interp` with `NanPolicy::Host` (`spec/interp.md` §5.1) gives the
  same bits. The tests compare hod with the interpreter bit for bit,
  NaNs included. `llc` is allowed to differ, and does: its SelectionDAG
  folds `x + -0.0` to `x` and `-0.0 - x` to `fneg x`, which changes the
  sign or the quietness of a NaN. So the `llc` comparison treats two
  NaNs as equal and counts how often it had to. `fneg` flips only the
  sign bit, a NaN's too.
- **`fptoui`/`fptosi` out of range** (a NaN, an infinity, or a value
  whose truncation does not fit) is poison (LangRef). hod's result is
  whatever `fcvtzs`/`fcvtzu` give: a saturated value at the W or X width,
  then truncated. Results narrower than 32 bits are converted at W width.
- **Fast-math flags** do not exist in `hod_ir`, which drops them. A
  function with flags compiles to the same words as one without.
- **`fcmp`** sets the flags with `fcmp`. Each predicate then needs one or
  two conditions (`float::FpCond { first, second }`; the result is
  `first || second`), the same table as LLVM's `changeFPCCToAArch64CC`:
  `oeq` EQ, `ogt` GT, `oge` GE, `olt` MI, `ole` LS, `one` MI|GT, `ord`
  VC, `uno` VS, `ueq` EQ|VS, `ugt` HI, `uge` PL, `ult` LT, `ule` LE,
  `une` NE. `false` and `true` emit `cmp xzr, xzr`, then use NE and EQ.
  An unfused result is `cset`, plus `csinc r, r, wzr, inv(second)` when
  there is a second condition.
- **Fusing (§4.6).** `fcmp` defers when `may_defer` allows it. `select`
  on a deferred `fcmp` sets the flags itself. `float::flags(cx, compare)`
  is the contract for `condbr` (WP-037): it emits the compare of a
  deferred `fcmp` and returns its `FpCond`. A branch then takes
  `b.first` and, if there is a `second`, `b.second` to the same target.
  The operands' registers are given back before it returns, so the
  caller may allocate freely.
- **`select`** of a value of at most 64 bits uses `fcsel` on the `D`
  view. With two conditions it takes two steps: first `b' = second ? a :
  b`, then `r = first ? a : b'`. A 128-bit value (`fp128` or a `Q`
  vector) uses `csetm x16` (with `csinv` for the second condition), then
  `dup v31.2d, x16` and `bsl`. A vector condition is `a64:vector-select`.
- **Vectors.** The FP vector arithmetic of `2S`, `4S` and `2D`; the
  integer vector `add`, `sub`, `mul` (not of `i64` elements), `and`,
  `or`, `xor`, and shifts through `ushl`/`sshl` (with the amount negated
  for right shifts); `fneg`; the casts with an instruction for them
  (`fcvtl`, `fcvtn`, `fcvtz*`, `*cvtf`, `xtn`, `uxtl`/`sxtl` by a factor
  of two). Everything else is `a64:vector-op` (integer division and
  remainder, `i64`-element `mul`, vector `frem`, arithmetic on `half`
  vectors) or `a64:fp-cast`. Vector `fcmp` and `icmp` are
  `a64:vector-fcmp` and `a64:vector-icmp`, because their `<N x i1>`
  result is `Mem` class (WP-040).
- **Reason names**: `a64:fp-binary`, `a64:fneg`, `a64:fp-freeze`,
  `a64:fp-cast`, `a64:fcmp`, `a64:vector-icmp`, `a64:fp-select` (the
  skeleton's), and `a64:bfloat`, `a64:fp128-libcall`, `a64:half-frem`,
  `a64:vector-fcmp`, `a64:vector-select`, `a64:vector-op`.

#### Floating point, the leftovers (WP-067)

`crates/hod_a64/src/float.rs`; the evidence is `spec/landed/codegen/WP-067.md`.
This supersedes the `a64:half-frem` and `a64:fp128-libcall` sentences of
"Floating point (WP-039)" above and the `a64:load-fpr` / `a64:store-fpr`
refusal of "Memory (WP-036)" below: those names are no longer produced.

- **Loads and stores** of an `Fpr`-class value are one `ldr`/`str`
  (`ldur`/`stur` when the offset is not scaled) of the value's store
  size: 2 bytes for `half` and `bfloat`, 4 for `float`, 8 for `double`
  and 64-bit vectors, 16 for `fp128` and 128-bit vectors. A vector of
  elements narrower than a byte is bit-packed in the register as in
  memory, so its store size is also its register view. `mem.rs` refuses
  an atomic access, then hands the access to `float::load` /
  `float::store` with its own address computation (a deferred `ptradd`
  fused, an index added into its register when there is one): every
  addressing form of the integer accesses applies. Unaligned accesses
  are plain accesses (normal memory on Linux allows them).
- **`fp128` compares** call one runtime function and compare its `int`
  result with 0, as LLVM's `softenSetCCOperands`, relying only on the
  signs that libgcc and compiler-rt share: `oeq`/`une` `__eqtf2` EQ/NE;
  `olt`/`uge` `__lttf2(a, b)` LT/GE; `ogt`/`ule` `__lttf2(b, a)` LT/GE;
  `ole`/`ugt` `__letf2(a, b)` LE/GT; `oge`/`ult` `__letf2(b, a)` LE/GT;
  `ord`/`uno` `__unordtf2` EQ/NE. `ueq` and `one` keep copies of the
  operands in a 48-byte frame slot, call `__eqtf2` (result kept in the
  slot) and `__unordtf2`, then `cmp unord, #0` · `ccmp eq, #0, #Z, eq`:
  `ueq` is EQ, `one` is NE. `float::flags` does the calls, so a caller
  (`select`, `condbr`) asks for its other operands after it.
- **`fp128` conversions** are one call each: `fpext` from `float` and
  `double` (`half` widened to `float` first, exactly) by
  `__extendsftf2`/`__extenddftf2`; `fptrunc` to `half`, `float` and
  `double` by `__trunctfhf2`/`__trunctfsf2`/`__trunctfdf2` (one rounding:
  never through a wider intermediate); `fptosi`/`fptoui` to at most 32
  bits by `__fixtfsi`/`__fixunstfsi`, to 33..64 bits by the `di`
  versions; `sitofp`/`uitofp` likewise by `__float[un]sitf` and
  `__float[un]ditf`, a width other than 32 and 64 extended first.
  `frem` of `fp128` calls `fmodl`.
- **`frem` of `half`** widens both operands to `float` into
  temporaries (one pinned at a time, through `v31`), calls `fmodf`, and
  narrows the result: `fmod` is exact, so the narrowing is the only
  rounding.
- **Vector constants** (and any other `Fpr`-class constant that is not a
  scalar float or zero) are materialized from the constant's in-memory
  image, 8 bytes at a time through `x16` and `fmov dN, x16` /
  `fmov vN.d[1], x16` (`xzr` for a zero word, `common::sym_address` for
  an aligned 8-byte symbol address). No literal pool: no new relocation
  kind, nothing out of reach in far mode. Another relocation in the
  image is `a64:constant-relocation`.
- **`phi`s** of `Fpr`-class values need nothing of this group: WP-037's
  copies move them (tested with loops carrying 19 values of `double`,
  `float`, `half`, `fp128` and vectors, a swap cycle among them).
- **Still refused**: vector `icmp`, `fcmp` and `select` with a vector
  condition (their `<N x i1>` is WP-040's), `bfloat` arithmetic, and the
  vector operations of WP-039's list.

#### Memory (WP-036)

- **Addresses.** An access is `[base, #off]` (scaled unsigned, else
  unscaled; else the offset is added into `x17`), `[base, xI, lsl #s]`
  or `[base, wI, sxtw #s]`. The base is `x29` for a directly placed
  slot, the pointer's register otherwise; a symbol, a constant address
  or an indirectly placed (over-aligned) slot is computed into `x17` for
  a load or store and into the result's register for a `ptradd`.
- **Fusion (§4.6).** A `ptradd` is deferred whenever `may_defer` allows;
  the access through it folds constant offsets and constant indices
  (taken as signed, truncated to 64 bits) into `off`, and its variable
  indices into the index register: the first one stays a `sxtw` index
  when it is an `i32` and its scale is a power of two up to 16; other
  widths are sign-extended with `sbfx`, other scales multiply (`lsl`,
  or `mov x16, #scale` and `mul`); further variable indices are added
  into the first. Unfused, the same sum is computed into the result.
- **Symbol offsets** are passed to `common::sym_address` when the
  total is below 2^20 in magnitude; it folds them into the relocation's
  addend only as `llc` does (WP-073, §7.4); beyond 2^20 the offset is
  added after the address.
- **Widths.** An access reads or writes exactly the store size. Sizes 1,
  2, 4, 8 are one access; 3 is `h` + `b`, 5 `w` + `b`, 6 `w` + `h`, 7
  `w` + `h` + `b`, joined on a load by `orr … lsl` through `x16`, split
  on a store by `lsr` into `x16`. A store of a width that is not a
  multiple of 8 masks the value in its register first (`and`; the
  register is the store's own operand, §4.2), so padding bits are zero.
  A zero constant is stored from `xzr`/`wzr`.
- **Volatile** accesses are lowered as plain ones; `hod_cg` never drops
  or repeats them. Atomic loads and stores are WP-074's ("Atomics").
- **`alloca`** with a constant count rounds the size up to 16 and
  subtracts it from `sp` (`sub` immediates, or `mov x17` and `sub`
  extended); a variable count is zero-extended from its width, scaled,
  rounded up to 16 and subtracted. An alignment above 16 then rounds
  `x16` down (`and`); an alignment that is not a power of two or above
  2^32 is `limit:frame-size`. The result is the new `sp`; the frame is
  marked dynamic. `llvm.stacksave` is `mov xD, sp`;
  `llvm.stackrestore` is `mov sp, xS` and marks the frame dynamic.
- **Refused:** a `load`/`store` of an `Fpr` value, `a64:load-fpr` and
  `a64:store-fpr` (WP-039); a `ptradd` producing a vector of pointers,
  `a64:ptradd-vector`. `Mem`-class values are `aggr`'s (WP-040).

##### Control flow (WP-037)

- **`condbr`** tests bit 0 (§2.2): `tbz` to the false block when the
  true block is emitted next, else `tbnz` to the true block and `b` to
  the false one unless it comes next. A constant condition, and two
  equal targets, are a `br`. A deferred condition is asked for (emitted)
  first; fusing a deferred compare into the branch is left to the
  integration package after WP-035 and WP-039, at the place marked in
  `control.rs`.
- **`switch`** follows the plan of §7.3 "`switch`" (`hod_cg::switch`).
  The condition is zero-extended in its register (`ubfx`, below 64
  bits); a node is `cmp` · `b.eq` to its block · `b.hi` to the upper
  half; a chain is `cmp` · `b.eq` per case ending in `b default` (or
  `b.ne default` when the last case's block comes next). A case value is
  compared by `cmp #imm`, `cmn #-imm`, or through `x16`. No data
  island. A `switch` wider than 64 bits is a
  compare chain (WP-084 below); `a64:switch-wide` is no longer produced.
- **`indirectbr`** is `br` of the address in its register; a
  `blockaddress` constant operand is formed in `x16` first.
- **`blockaddress` in code** (`common::block_address`): `adr rd, label`
  (fixup kind 4, ±1 MiB). In far mode: `adr rd, 1f` · `ldr wT, [rd]` ·
  `sbfx xT, xT, #0, #26` · `add rd, rd, xT, lsl #2` · `b 2f` ·
  `1: b label` · `2:`: the distance is read back from the `imm26` of a
  `b` that is never executed, so the only fixup is a `b`'s, which reaches
  the whole function. This reads `.text` as data: an execute-only text
  mapping would break it (none of the supported linkers makes one by
  default). The glue's constant path (`materialize_gpr_const`) still
  refuses `blockaddress` (`a64:blockaddress`) until it calls this helper,
  so a `blockaddress` reaches code only as `indirectbr`'s own operand;
  `operand:blockaddress` stays not-implemented until then.
- **`phi`** needs nothing from the target beyond the glue (§4.5); the
  protocol is tested here on the real target for `int` and `ptr`; its
  matrix row is supported for every class, the `type:*` rows gating.

#### WP-038: calls

- **Division of labour.** `hod_cg`'s call sequence (§7.1) does the
  placement, the stack area, `byval` copies, results in memory, the
  extensions, `musttail` staging, pad stubs and call-site rows; `frame`
  emits `bl`/`blr` and the tail exits. `crate::call` adds what the
  sequence does not: the refusals below, the clearing of the bits above
  a narrow integer leaf of a `Mem`-class result in its home after a call
  (a producer owes nothing above a leaf, memory holds them zero), and
  the variadic parts.
- **Variadic definitions** always save `x[g]..x7` and `q[f]..q7` (named
  registers excluded) in two frame slots at entry; `va_start` writes the
  32-byte AAPCS64 `va_list` from them and the incoming base; `va_copy`
  copies 32 bytes; `va_end` is nothing; `va_arg` stays `a64:va_arg`
  (clang expands it inline). A `musttail` call in a variadic function
  reloads the unnamed argument registers from the save areas first, so
  the callee receives them as this function did; the stack area beyond
  the named arguments is never written. Variable arguments of the call
  before the `...`, which `hod_ir`'s verifier accepts since WP-064, are
  placed as any argument, over the forwarded registers they take, as
  `llc` places them (WP-115): `classify` learns their area from
  `TargetStackArgs::of_call`, which types them from the body; in
  registers they compile, on the stack they are
  `musttail:stack-args-grow` (the caller's area holds only its named
  arguments).
- **Landing pads** are `{ ptr, i32 }` only: `x0`/`x1` stored to the
  pad's home; clauses are `catch` of a symbol or `null` and `filter` of
  such arrays. `resume` calls `_Unwind_Resume` with the exception
  pointer, then traps.
- **Refused by name**: `a64:call-bundle`, `a64:va-intrinsic`, `a64:va_arg`, `a64:landingpad`,
  `a64:landingpad-clause`, `a64:resume`. `a64:variadic-definition` is
  no longer produced, nor, since WP-084, `a64:invoke-odd-int-result`
  (an `invoke`'s odd leaves are cleared by `FnCx::call`); WP-069 removed `a64:call-wide-int-extension` and
  `a64:call-narrow-ext-stack-arg` (`abi-a64.md` §7.1) and
  `a64:call-mismatched-declaration` (§7.1 here). Since WP-103 every
  call of inline assembly, `callbr` included, goes to §5.5.3, so
  `a64:call` (`callbr`) and `a64:call-inline-asm` are no longer produced.
- **Tests** (`tests/call.rs`): random signatures (`tests/common/
  callgen.rs`) linking gcc-compiled C with modules compiled by hod and
  `llc` in every pairing, stressed, PIC with a shared object; the call
  and GOT relocations equal `llc`'s; `hod_interp` as the oracle of IR
  call chains; handwritten forms (`tests/data/call/hand.ll`); real C++
  exceptions (`eh.ll`, `eh_comdat.ll`, `eh.cpp`) through `emit_module`'s
  own tables (WP-049), static, stressed and PIC, against `llc`'s
  objects of the same modules. Nested try in one frame dispatches on
  the exception's type, not with `llvm.eh.typeid.for` (the intrinsic
  group's): `llc` numbers type ids in reverse clause order, hod in
  order (`FnCx::type_id`), so a selector constant is not portable.

#### Intrinsics (WP-041)

- **Integer intrinsics** take `i1` to `i64` (`Gpr`); the bits above the
  width are unspecified in operands and results (§2.2). An `i128`
  operand is `a64:intrinsic-wide`, a vector one `a64:intrinsic-vector`.
  For a width `N` in a register of `R` = 32 or 64 bits:
  - `ctlz`: when `N < R`, `lsl #(R-N)` and `orr #(1 << (R-N-1))` first,
    so a zero counts `N`; then `clz`. `cttz`: `orr #(1 << N)` when
    `N < R`, then `rbit` and `clz`. The poison flag is ignored.
  - `ctpop`: `ubfx` to the width, `fmov` into `v31`, `cnt v31.8b`,
    `uaddlv h31`, `fmov w`.
  - `bswap`: `rev16 w` at 16 bits, else `rev` and `lsr #(R-N)`; a width
    that is not a multiple of 16 is `a64:intrinsic`. `bitreverse`:
    `rbit` and `lsr #(R-N)`.
  - `abs`: `sbfx`, `cmp #0`, `cneg mi` (`INT_MIN` maps to itself).
    `smax`/`smin`/`umax`/`umin`: both operands extended, `cmp`, `csel`.
  - `fshl`/`fshr`: the amount is taken modulo `N`. A constant amount
    `k = c % N` of 0 is the first (`fshl`) or second (`fshr`) operand;
    otherwise `extr` at `N = R`, `ubfx` + `orr ..., lsl` below. A
    variable amount at `N = R` uses the register shifts' own modulo
    (`lslv`, `mvn`, `lsr #1`, `lsrv`, `orr`); below it the amount is
    reduced by `and #(N-1)` for a power of two, else by `udiv` + `msub`
    against `N` in `x16`, and both shifts are by `k` and `N - k`.
- **Memory intrinsics** follow the plan of §7.3 "Memory intrinsics"
  (`hod_cg::memplan`). Inline, the accesses are the largest
  power-of-two ones, the tail overlapping the previous access for
  copies, exact-size stores for sets. An inline `memmove` reads every
  byte (into `x16`, `x17`, `v30`, `v31`) before it writes one; 32 bytes
  is what those four registers hold. A length narrower than 64 bits is
  zero-extended for a call. The loop of an `.inline` form is 16-byte
  post-indexed `q31` accesses counted in `x16` with an overlapping last
  access, the fill byte replicated by `dup v31.16b`; a variable
  `.inline` length is `a64:mem-inline-length`. Volatile is lowered as
  the plain form (each byte is accessed once).
- **Traps:** `trap` is `brk #1`, `debugtrap` `brk #0xf000`,
  `ubsantrap(k)` `brk #(0x5500 | k)`, as `llc`.
- **Frame:** `frameaddress(0)` is `x29`; `frameaddress(n)` loads the
  saved `x29` `n` times; `returnaddress(n)` loads `[record, #8]` of the
  `n`-th record. Every hod function saves `lr` in its frame record, so
  `returnaddress(0)` is a load too. A depth above 256 is
  `a64:frame-depth`.
- **Folded:** `objectsize` is `-1` (`min` false) or `0`; `is.constant`
  is true for a literal constant operand (an integer, a float, `null`,
  `zeroinitializer`, `undef`, `poison`) and false otherwise, as `llc`'s
  `LowerConstantIntrinsics`; `prefetch` emits nothing; `ptrmask` is an
  `and` of the 64-bit registers.
- **Refused:** every other intrinsic (floating-point math, saturating
  arithmetic, `scmp`/`ucmp`, `clmul`, reductions, ...) is
  `a64:intrinsic`.

#### Intrinsics, the second set (WP-070)

`fpintr.rs` lowers the floating-point intrinsics; `intrinsic.rs` routes
them there, and has the saturating arithmetic, `scmp`/`ucmp`, the
`i128` forms, `eh.typeid.for` and `clear_cache`.

- **`float`, `double`**: the instruction where ARMv8.0 has one: `fsqrt`;
  `frintm`/`frintp`/`frintz`/`frinta` (round)/`frintn` (roundeven)/
  `frintx` (rint)/`frinti` (nearbyint); `fmadd` for `fma` and for
  `fmuladd` (fused, as `llc`); `fminnm`/`fmaxnm` for `minnum`/`maxnum`;
  `fmin`/`fmax` for `minimum`/`maximum`; `minimumnum`/`maximumnum` quiet
  each operand with `fminnm x, x` first, then `fminnm`/`fmaxnm`;
  `canonicalize` is `fminnm x, x`; `lround` is `fcvtas`, `lrint`
  `frintx` + `fcvtzs`; `fptosi.sat`/`fptoui.sat` are `fcvtzs`/`fcvtzu`
  at 32 or 64 bits (which saturate) clamped with `cmp` + `csel` below
  32 bits and between 33 and 63.
- **Bit operations**: `fabs`, `copysign` and `is.fpclass` act on the
  bits in `x16`/`x17`, NaNs untouched. `is.fpclass` needs a constant
  mask (0 and all ten bits fold); it counts the class of the magnitude
  (zero, subnormal, normal, infinity, signalling, quiet NaN) with
  `cset` and four `cinc`s and indexes a 6-bit table chosen by the sign.
- **`half`**: computed as `llc` does: widened to `float` (to `double` for
  `fma`, where one rounding of the exact product sum to `half` is
  correct), the operation, narrowed. `fmuladd` of `half` is a rounded
  multiply then a rounded add. Widening quiets a signalling NaN, so the
  `num` forms treat it as missing, as `llc`.
- **`fp128`**: a call of the `long double` function: `sqrtl`, `floorl`,
  ..., `fmal`, `fminl`/`fmaxl`, the C23 `fminimuml`, `fmaximuml`,
  `fminimum_numl`, `fmaximum_numl` (glibc 2.35+; `llc` 22 cannot select
  `minimum`/`maximum` of `fp128` at all), `lroundl`/`llroundl`/`lrintl`/
  `llrintl`; `fmuladd` is `__multf3` + `__addtf3`; `canonicalize`
  `__multf3(x, 1.0)`. `fabs`, `copysign`, `is.fpclass` are inline.
  `fptosi.sat`/`fptoui.sat` of `fp128` are refused
  (`a64:intrinsic-fp128`).
- **Calls**: `sin` .. `log10`, `pow`, `atan2`, `powi`, `ldexp` and
  `frexp` call the routines of §7.3 "Runtime routines by type and
  target" (`sinf`/`sin`/`sinl`, `__powisf2`, ...); `frexp` with the
  address of a 4-byte slot, its `{T, i32}` result a `Mem`-class value.
  `half` goes through the `float` function. An exponent narrower than 32
  bits is sign-extended; a wider `ldexp` exponent is clamped to the
  `int` range (`llc` truncates it); a wider `powi` exponent is refused
  (`a64:intrinsic`).
- **Saturating arithmetic** at 32 and 64 bits uses the flags of
  `adds`/`subs` (`csel vs` with `(r asr N-1) eor 1<<(N-1)`, `csinv lo`,
  `csel hs`); narrower, the operands are extended to 64 bits, where the
  exact result fits, and clamped. `scmp`/`ucmp`: extend, `cmp`, `cset
  gt|hi`, `csinv ge|hs`, any result width from 2 to 128 bits.
- **`i128`** (class `Mem`, through the slots, limbs in `x16`/`x17` and
  at most four temporaries): `ctlz`/`cttz` select between the limbs'
  counts; `ctpop` is `cnt` + `uaddlv` over both limbs in `v31`;
  `bswap`/`bitreverse` reverse each limb and swap them; `abs` is
  `(x ^ s) - s`; min/max and `scmp`/`ucmp` order by `cmp` + `sbcs`;
  the saturating forms use `adds`/`adcs`, `subs`/`sbcs`; `fshl`/`fshr`
  pick three consecutive limbs by `tst k, #64` + `csel` and funnel each
  result limb by `k mod 64` (the other limb moved by one more, so no
  branch for `k mod 64 = 0`).
- **`eh.typeid.for`** of a symbol is `mov w, #FnCx::type_id(sym)`, the
  index `emit_module` gives the type in the LSDA's type table; of
  anything else it is refused (`a64:eh-typeid`).
- **`clear_cache`** calls `__clear_cache`. **`readcyclecounter`** is
  `mrs rd, CNTVCT_EL0` since WP-084 (`a64:readcyclecounter` is no longer
  produced).
- **Named registers** (`read_register`, `read_volatile_register`,
  `write_register`) are `hod_a64::named_reg`'s since WP-064 ("Named
  registers" below).
- `sshl.sat`/`ushl.sat` are lowered by WP-084 (below). Vector forms are `a64:intrinsic-vector`,
  `bfloat` ones `a64:intrinsic-bfloat`.

#### Named registers (WP-064)

`hod_a64::named_reg` lowers `llvm.read_register`,
`llvm.read_volatile_register` and `llvm.write_register` of an `i64`
whose metadata operand (`spec/ir.md` §5.3) is a node of one string, as
`llc -O0 -global-isel=0 -fast-isel=0` of LLVM 19 and 22 does
(`tests/named_reg.rs` compares 39 accesses with both, and runs 16):

- `sp`, `x29`, `x30` (exactly so spelled; `llc` refuses `SP`, `fp`,
  `lr`) read as `mov xd, sp` (`add xd, sp, #0`) or `mov xd, x29`/`x30`;
  written, they are `a64:write-register-gpr` (the frame `llc` and hod
  address slots through would break). The facade declines a body that
  reads one of them where it may still be always-inlined
  (`alwaysinline:frame-address`): inlined, it reads the caller's frame.
- A system register named in `SYSREGS` (any case, as `llc`; the timer,
  `CTR_EL0`, `DCZID_EL0`, `DAIF`, `NZCV`, `FPCR`, `FPSR`, `TPIDR_EL0`,
  `TPIDRRO_EL0`, `MIDR_EL1`, `MPIDR_EL1`, `REVIDR_EL1`), or as
  `op0:op1:CRn:CRm:op2` in decimal with `op0` 2 or 3 and every field in
  range, reads with `mrs` and writes with `msr`. A write of a register
  the table marks read-only is `a64:named-register`, as `llc` 22 refuses
  it (`llc` 19 aborts).
- Anything else is `a64:named-register`: the general registers `llc`
  takes only when reserved (`-ffixed-xN`, which hod does not model),
  `x0` and `xzr` (`llc` reads nothing), the `w` names (`llc` misreads
  them as `NZCV`), a system register outside the table, a numeric form
  out of range (`llc` encodes garbage). Another type than `i64` is
  `a64:named-register-width` (`llc` emits an unencodable `mrs w0`).

The interpreter refuses the three by name (`intrinsic:<name>`): what a
register holds is the machine's.

#### Assembly that needs no assembler (WP-075)

`hod_a64::barrier` lowers a call (or `invoke`) of inline assembly whose
template is empty (`spec/ir.md` §2.3) as `llc -O0` does; `call.rs` routes
every inline-assembly callee to it. Such a call emits no instruction; it
orders as a call does (deferred operands are emitted first). The flags
`sideeffect`, `alignstack`, `inteldialect` and `unwind` change nothing an
empty template does, and an `invoke` continues at its normal destination.
Its constraints decide its one possible effect:

- **Clobbers** of `memory`, `cc`, `fp`, `lr`, `x0..x30`, `w0..w30` and
  `v`/`q`/`d`/`s`/`h`/`b` `0..31` need nothing: an empty template writes
  no register and no memory, and the ordering a `memory` clobber asks for
  is the call's own (hod moves no memory access across an instruction).
  Any other (`sp`, `x31`, `dirflag`, ...) is `a64:inline-asm-clobber`.
- **Alternatives** (`|`) are chosen by LLVM's weights
  (`getSingleConstraintMatchWeight`): invalid, default 0, register 1
  (`r`/`g` for an integer operand, `w` for a floating-point or vector
  one), memory 2, constant 3 (`i`/`n` for a constant integer); a direct
  output weighs 0. The first alternative of the greatest sum wins, the
  first if all are invalid.
- **An input** has no effect. An input whose chosen codes are only
  `i`/`n` and whose value is not a constant integer is
  `a64:inline-asm-operand`.
- **An indirect output** (`=*`) of memory codes only has no effect, tied
  or not (`llc` passes the address; the template writes nothing). Of a
  register code and tied to a direct input of its `elementtype`, it
  stores the input to the address (`=*r,0`: `llc` loads the register from
  the input and stores it back); tied to an indirect input of the same
  address, nothing. Otherwise (untied, or of another type) it is
  `a64:inline-asm-output`, or `a64:inline-asm-operand` for a type
  mismatch.
- **A direct output** (`=r`) must be tied to a register-class input of
  its type, which becomes the result; an untied direct output is
  undefined after the template, `a64:inline-asm-output`.
- More than one effect (two direct outputs, a struct result, two stores)
  is `a64:inline-asm-outputs`; an operand bundle on the call is
  `a64:inline-asm-bundle`; constraint text that is not read (`!i`, `Q`,
  a tied output, an input before an output, a digit naming no output, a
  count of operands unequal to the call's) is `a64:inline-asm-constraint`.

A template that is not empty stays `call:inline-asm`. The tests are
`crates/hod_a64/tests/barrier.rs`: one module of every form, built by hod
and by `llc` 19 and 22, linked with a C driver that checks each effect
under every linker, dynamic and static.

#### Vectors, the base (WP-079)

Design note `docs/design/0003-vectors.md` (V1); the code is
`crates/hod_a64/src/vector.rs`, whose module docs are part of this
section.

- **Shapes.** A fixed vector `<N x E>` has `hod_ir`'s layout: lanes
  packed, lane `i` at byte `i × size(E)` of its image, the store size
  `N × size(E)` for byte-sized `E`, the alloc size and alignment of the
  layout. Its class is §2's: `Fpr` for a power-of-two `N` of 64 or 128
  bits (element not `i1`), else `Mem`, whose value is its image in a
  frame slot (the bytes after the store size, up to the alloc size, are
  unspecified and never read).
- **The helper API** (used by every later vector package, WP-080..083,
  instead of their own): `Shape::of` / `lanes` (the shape, refused by
  name unless its lanes are 1, 2, 4 or 8 bytes and at most 256);
  `Lanes::offset` (where lane `i` is); `image` / `spill` / `reload` (a
  frame place holding an operand's image: a `Mem` vector's own slot, or
  one of three 48-byte scratch images per function for an `Fpr` one,
  allocated on first use); `index` / `lane_cursor` (a lane index
  operand and a cursor at that lane, never outside the image);
  `load_lane` / `store_lane` (one lane to or from a general or a vector
  register).
- **Lane indices.** A constant index `< N` names its lane. A constant
  `≥ N`, `undef` or `poison` is poison: `extractelement` reads lane 0,
  `insertelement` writes nothing (either result is poison). A variable
  index is reduced to its width (the bits above are unspecified), then
  masked with `N − 1` for a power-of-two `N`, else clamped to lane 0
  when `≥ N` (`cmp` + `csel` with `xzr`): the access stays inside the
  image whatever the value. An index wider than 64 bits is
  `a64:vector-index`.
- **`extractelement`.** `Fpr` vector, constant index: `umov` into a
  general register, `dup` (scalar) into a vector one. Otherwise one load
  of the lane from the image (an `Fpr` vector spilled to scratch image
  0).
- **`insertelement`.** `Fpr` vector, constant index: `ins` (from a
  general or a vector register) into the vector's register, which is the
  result. Variable index: spilled, the lane stored, reloaded. A `Mem`
  vector: copied (or its constant written) into the result's slot, the
  lane stored there.
- **`shufflevector`.** Unrolled lane by lane (at most 256 lanes); an
  undefined mask entry gives zero; a source no entry names is not asked
  for; `%a, %a` asks once. `Fpr` result: a splat of a register source is
  one `dup`; otherwise the lanes are inserted into `v31` (`ins` element,
  or a load into `x16` and `ins` for a `Mem` source), then moved into an
  operand's register, which dies here, so two sources and the result fit
  the smallest pool (stress `min_pool`, two vector registers). `Mem`
  result: each lane through `x16` (`umov` or a load), stored into the
  slot. Widening, narrowing and changes of class all take these paths.
- **Whole values.** `load`, `store`, `select` on a scalar condition,
  `phi`, `freeze`, constants (poison lanes included), `bitcast` and
  vectors as fields of aggregates take the paths of WP-039 (`Fpr`) and
  WP-040 (`Mem`), masks and odd elements included (their images are
  `hod_ir`'s); only lane operations refuse those shapes. The
  element-wise operations WP-039 lowers on `Fpr` vectors are on in the
  matrix for `Vector`; on a `Mem` vector they are `a64:mem-vector`
  (WP-080).
- **Calls.** `Fpr` vectors are passed and returned as `abi::assign`
  places them (`v0`..`v7`, then the stack; the AAPCS64 short-vector
  rules, checked against `llc` by the ABI grid and here against gcc's
  `vector_size`). A `Mem` vector in a signature (parameter, result, or
  a leaf of an aggregate one) is WP-082's (below).
- **Refused, by name** (`hod_a64::vector`): `a64:vector-mask` (a lane
  operation on `<N x i1>`, `N ≤ 64`: WP-081), `a64:mask-wide` (more
  than 64 `i1`), `a64:vector-odd-element` (integer lanes of a width other
  than 8, 16, 32, 64 and at most 64), `a64:vector-wide-element`
  (`i128` or `fp128` lanes), `a64:vector-length` (more than 256 lanes),
  `a64:vector-index`; `getelementptr` over a vector
  of pointers stays `a64:ptradd-vector`, element-wise arithmetic
  `a64:vector-op` / `a64:mem-vector` (WP-080). `a64:vector-bfloat` and
  `a64:intrinsic-target` are defined for WP-080 and WP-083.
- **Tests**: `crates/hod_a64/tests/vector.rs` (the generator over shapes
  × operations × indices against a model, `llc` and clang
  `ext_vector_type`, under bfd, lld and mold; calls both ways with gcc;
  the refusals), `tests/ir/vectors.ll`.

#### Vectors, the lane loop (WP-080)

Design note `docs/design/0003-vectors.md` (V3); the code is
`crates/hod_a64/src/lanes.rs`, whose module docs are part of this
section.

- **One loop, the scalar kernels.** Every element-wise operation on a
  vector that has no one-instruction form (all of them on a `Mem`
  vector; division, remainder, `mul` of 64-bit lanes, `frem`, `half`
  arithmetic and the casts without a NEON instruction on an `Fpr` one)
  is one loop unrolled at emit time over at most 256 lanes. Per lane:
  pool temporaries of the element's class, each operand's lane loaded
  (zero-extended), **the scalar lowering's own register kernel**, the
  result lane stored. The kernels are factored out of the scalar
  lowerings (`int::lane_binary`, `int::lane_cast`, `float::lane_binary`,
  `float::lane_frem`, `float::lane_fneg`, `float::lane_convert`,
  `intrinsic::lane_unary`/`lane_min_max`/`lane_saturating`/
  `lane_three_way`/`lane_funnel`, `fpintr::lane_inline`/`lane_call`),
  which call them too: a lane computes exactly what the scalar
  instruction computes, poison and undefined behaviour included.
- **Images.** Operands: a `Mem` vector's slot; an `Fpr` vector stored
  into scratch image `k` (WP-079) and its register given back at once,
  so the loop has the whole pool (stress `min_pool`: four general and
  two vector registers; the third floating-point operand of `fma` and
  `fmuladd` is loaded into `v31`). An operand named twice has one image;
  a scalar operand (`powi`'s exponent) is put into a register in every
  lane. Result: a `Mem` vector's slot (`def_mem`); an `Fpr` vector's
  lanes go to the next free scratch image (the first operand's when
  three are taken: three operands of the result's type, each lane read
  before it is written) and the image is loaded into the result's
  register at the end.
- **Calls per lane.** `frem`, the libm intrinsics, `pow`, `atan2`,
  `powi` and `ldexp` call per lane as the scalar does (`fmodf`/`fmod`,
  `sinf`/`sin`, `__powisf2`, `ldexpf`, ...; `half` widened, the result
  narrowed). A call ends every temporary, so each lane takes its own;
  the images are frame memory and survive.
- **Covered.** `add` .. `srem` and the shifts on `i8`..`i64` lanes;
  `fadd` .. `frem` and `fneg` on `half`, `float`, `double`; `trunc`,
  `zext`, `sext`, `ptrtoint`, `ptrtoaddr`, `inttoptr`, `fpext`,
  `fptrunc`, `fptoui`, `fptosi`, `uitofp`, `sitofp` between any two
  vectors of one length (the classes may differ); the vector forms of
  `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs`, `smax`, `smin`,
  `umax`, `umin`, the four `*.sat`, `scmp`, `ucmp` (any result width),
  `fshl`, `fshr`, `ptrmask`, and of every floating-point intrinsic of
  WP-070 but `frexp` and `is.fpclass`. A `select` on a scalar `i1` was
  already lowered (WP-039/040).
- **Refused, by name.** The shapes `vector::lanes` refuses keep its
  names (`a64:vector-length`, `a64:vector-wide-element` for `i128` and
  `fp128` lanes, `a64:vector-odd-element`; the arithmetic, `fcmp` and
  casts of `i128`, `fp128` and `bfloat` lanes are WP-101's, §5.5.1); any
  other lane operation on `bfloat` is `a64:vector-bfloat`. Lanes of `i1` are WP-081's (below), which lowers
  their binary operations and casts; an intrinsic of one stays
  `a64:intrinsic-vector`. `frexp` and `is.fpclass` of a vector
  stay `a64:intrinsic-vector`.
- **Cost.** The code is a constant number of bytes per lane (no lane
  shares work with another), emitted without allocating; see the landed
  record for the numbers.
- **Hooks.** `aggr::binary` and `aggr::cast` (a vector), `float::binary`
  (the three `a64:vector-op` places) and `float::vector_cast` (its
  fallback), the `fneg` arm of `A64::lower` for class `Mem`, and the
  first line of `intrinsic::intrinsic`.
- **Tests**: `crates/hod_a64/tests/lanes.rs` (every operation × element
  × lane count 1, 2, 3, 4, 8, 16, 256 against a Rust model, `llc` 22 and
  19, clang `ext_vector_type` and gcc `vector_size` C, under bfd, lld
  and mold, plain and stressed; refusals; 256-lane compiles; bytes per
  lane), `tests/ir/vector_lanes.ll`.

#### Vectors, masks (WP-081)

Design note `docs/design/0003-vectors.md` (V2, §4.1); the code is
`crates/hod_a64/src/mask.rs`, whose module docs (the bit rules) are
part of this section.

- **Representation.** `<N x i1>` with `1 ≤ N ≤ 64` is of class `Gpr`:
  lane `i` is bit `i` of the register, the bits from `N` up are
  unspecified (as for `iN`, §2.2). That is LLVM's in-memory image
  (lane 0 in bit 0 of byte 0, `⌈N/8⌉` bytes), so a `bitcast` between a
  mask and `iN`, a byte vector or a float of `N` bits is the copy the
  other groups make, and `phi`, `freeze`, a scalar `select`,
  `extractvalue`/`insertvalue` and loads are unchanged.
- **Memory layout, settled by experiment** (`llc -O0 -global-isel=0
  -fast-isel=0`, LLVM 22 and 19, and `hod_interp`): a stored mask has
  the bits from `N` to the end of its last byte **zero** (`llc`: `and
  w8, w0, #0x7` then `strb` for `<3 x i1>`, `#0xfff` then `strh` for
  `<12 x i1>`; `<40 x i1>` five bytes); hod clears them before every
  store of a mask whose `N` is not a multiple of 8. A load assumes
  nothing about them (`llc` assumes they are zero: a `<3 x i1>` loaded,
  bitcast to `i3` and zero-extended is one `ldrb`): every reader of a
  mask's lanes reads only its `N` bits. `N = 1` is one bit in one byte
  (`cset`, `strb`); `N` not a multiple of 8 differs only in the padding.
  A byte that holds a poison lane is poison as a whole (LLVM's memory is
  poison byte by byte; `llc` lets the bits of an out-of-range `fptoui`
  to `i1` spill into the neighbouring lanes), so tests do not compare it.
- **Lowered.** Vector `icmp` (integer and pointer lanes, every
  predicate; lanes extended by the predicate's signedness) and `fcmp`
  (`half`, `float`, `double`, every predicate) as one `cmp`/`fcmp` +
  `cset` per lane, the bit inserted; the logical and arithmetic
  operations and `icmp` on masks as one bit instruction each; `select`
  of masks (`(c & t) | (f & ~c)`) and of lanes on a mask (`tst` +
  `csel` per lane); `zext`/`sext` of masks to integer lanes,
  `uitofp`/`sitofp` to float lanes; `trunc` to a mask, `fptoui`/`fptosi`
  to a mask (each lane through `float::lane_convert`);
  `extractelement`, `insertelement` (constant or variable index),
  `shufflevector` of masks (runs of lanes by `ubfx`/`bfi`); and
  `llvm.vector.reduce.{add,mul,and,or,xor,smax,smin,umax,umin}` of masks
  (one flag: any, all, parity) and of integer lanes,
  `reduce.{fadd,fmul}` (in lane order from the start value, which is
  right with or without `reassoc`) and `reduce.{fmax,fmin}` (as
  `maxnum`/`minnum`) of float lanes, up to 256 lanes.
- **Poison.** An undefined or poison lane of a constant or of a
  shuffle mask is 0; an out-of-range constant index reads lane 0; a lane
  that LLVM leaves poison takes what the bit rule gives.
- **Refused, by name.** A lane operation on a mask of more than 64 lanes
  (it stays of class `Mem`): `a64:mask-wide`. A mask crossing a call or a
  return is WP-082's (below). An intrinsic of
  `i1` lanes (`llvm.umax.v4i1`): the intrinsic group's
  `a64:intrinsic-vector`. (`fcmp` of `bfloat` lanes was
  `a64:vector-bfloat` until WP-101.) A reduction whose operand list is not the
  intrinsic's: `a64:vector-reduce`. `reduce.fmaximum`/`fminimum` stay
  the intrinsic group's refusal.
- **Known `llc` defects** (not hod's, not chased; the interpreter and
  gcc/clang vector extensions decide): SelectionDAG stores a `trunc`
  of `<3 x i64>` to `<3 x i8>` into the wrong bytes (WP-080); `llc` 19
  crashes on `llvm.vector.insert` of non-power-of-two lengths; stores of
  `<4 x i3>` are wrong. None of them is on a mask path tested here.
- **Hooks.** `target::vector_class` (a vector of `i1` of at most 64
  lanes is `Gpr`), the first line of `A64::lower` (`mask::lower`) and
  the constant arm of `A64::materialize` (`mask::materialize`).
- **Tests**: `crates/hod_a64/tests/mask.rs` (every construct × lane type
  × lane count against a Rust model, `hod_interp` on value-form twins,
  `llc` 22 and 19, under bfd, lld and mold, plain and stressed; masks
  loaded with their padding bits set; refusals; the widest masks;
  bytes per lane), `tests/ir/vector_masks.ll`.

#### Vectors across calls (WP-082)

Every vector `abi::assign` places crosses a call and a return, in both
directions, exactly as `llc -O0` passes it (`spec/abi-a64.md` §4.3);
what `llc` passes differently from any `Part` hod can say is refused by
name, never approximated. (WP-079 and WP-081 refused these as
`a64:vector-call`; the name, and the check that reported it, are gone,
WP-101.)

- **Bytes.** A `Mem` vector whose pieces are bytes of its image
  (`<3 x float>`, `<8 x i32>`, `<3 x i64>`, `<5 x i32>`, `<3 x half>`,
  `<16 x float>`, results of more than eight pieces in memory) takes the
  generic `Mem` path of §7.1 as any aggregate does.
- **Lanes.** A widened vector (`<2 x i8>`, `<4 x i8>`, `<2 x i16>`) and
  every mask of up to 64 elements has `PartKind::Lanes` parts (§7.2),
  which the call sequence hands to `Target::lanes_out` (to a place) and
  `Target::lanes_in` (from one). `hod_a64::vcall` implements them:
  - a widened vector's image is loaded as `h`/`s` and widened with
    `uxtl` (B to H, H to S), and narrowed back with `xtn` and stored as
    its image; the consumer reads only the low bits of each lane;
  - a mask (a `Gpr` value: bit `i` is element `i`, §5.5.1 "Vectors,
    masks") goes to a `W` piece as `ubfx` of its bit, to a `D`/`Q`
    piece as `movi #0` plus `ubfx`/`ins` per lane; it comes back as `bfi`
    of bit 0 of each `W` piece or `umov`+`bfi` per lane. A `W` piece on
    the stack is 4 bytes in an 8-byte slot; a `D`/`Q` piece is built in
    `v31` and stored. A mask inside a `Mem` aggregate moves between its
    bit image (padding bits zero) and the place the same way.
  - A mask result with more than eight `W` pieces is returned in memory:
    the callee (`vcall::ret_image`) stores its 8-byte image with the
    bits from `N` up cleared; the caller allocates a temporary buffer,
    passes it in `x8` and loads the mask from it.
  - Registers: only the scratch registers (`x16`, `x17`, `v30`, `v31`)
    and the destination are written.
- **Refused, by name.** A mask of more than 64 elements:
  `abi:mask-wide` (`abi:vector-promoted` is no longer reported). An
  array of vectors of more than one piece stays `abi:vector-array`, a
  `musttail` of a result in memory without `sret`
  `call:musttail-ret-memory`, and `va_arg` `a64:va_arg`. x86-64 passes
  no `Lanes` part: its `Target` keeps the default hooks
  (`call:part-lanes`), and its `assign` is unchanged.
- **Hooks.** `A64::assign` (`abi::assign` directly), `A64::lanes_out` and `A64::lanes_in` (to `vcall`), and
  the memory-return arm of `term::ret` (`vcall::ret_image`).
- **Tests**: `crates/hod_a64/tests/vcall.rs`: hod (plain and stressed)
  callers against `llc` 22 and 19 callees and the reverse, under bfd,
  lld and mold; every mask length to 64 and every widened and byte shape
  alone, after full argument registers and as results; structures and
  arrays of them; calls through a pointer, `invoke`, `musttail`
  forwarders; variadic callees with named vectors of every shape and
  variable vectors and vector aggregates read by gcc's `va_arg`; gcc
  callers and callees of every `vector_size` vector and vector
  aggregate; refusals.

#### Atomics (WP-074)

`crates/hod_a64/src/atomic.rs` lowers atomic `load` and `store`,
`atomicrmw`, `cmpxchg` and `fence`; `target.rs` dispatches them before
the class-based arms.

- **Which sequence.** The function's `target-features` decide (the last
  `±outline-atomics` and the last `±lse` item; `-mattr` is not
  consulted): `+outline-atomics` without `+lse` (what clang and kiln
  write) is the **outline** strategy; anything else (no item,
  `-outline-atomics`, or `+lse`) is **LL/SC**. hod never emits LSE
  instructions inline: an exclusive loop is correct on every ARMv8.0
  core, and the helpers already choose LSE at run time.
- **Outline.** The `__aarch64_*` helpers of libgcc/compiler-rt (in
  `hod_cg::RuntimeFn`, arguments in `x0`/`w0`, then `x1`, ..., result in
  `x0`; `cas16` takes and returns pairs in `x0:x1`, `x2:x3`, pointer
  `x4`) are called exactly where LLVM 22 at `-O0` calls them:
  `cmpxchg` of 1..16 bytes (`casN`), an `i128` atomic load (`cas16` of
  zero by zero), and `atomicrmw xchg`, `add`, `sub` (negated into
  `ldadd`), `and` (inverted into `ldclr`), `or` (`ldset`), `xor`
  (`ldeor`) of 1..8 bytes. Everything else (`nand`, the minimums and
  maximums, `uinc_wrap`, `udec_wrap`, `usub_cond`, `usub_sat`, the
  floating-point operations, an `i128` store or read-modify-write) is
  an LL/SC loop under both strategies, as atomic as LLVM's
  `casN`-in-a-loop.
- **Orderings.** An operation acquires when its ordering is `acquire`,
  `acq_rel` or `seq_cst`, releases when `release`, `acq_rel` or
  `seq_cst`; a `cmpxchg` takes the union of its success ordering and its
  failure ordering (a `seq_cst` failure also releases). The helper
  suffix is `relax`, `acq`, `rel`, `acq_rel` by those two bits;
  `seq_cst` uses `acq_rel`, as LLVM does. LL/SC uses `ldaxr`/`ldxr` and
  `stlxr`/`stxr` (pairs: `ldaxp`/`ldxp`, `stlxp`/`stxp`). `unordered` and
  `monotonic` are relaxed. Weak `cmpxchg` is strong (it never fails
  spuriously, which LangRef allows).
- **Loads and stores** of 1..8 bytes: `ldar`/`stlr` when they acquire or
  release, else plain `ldr`/`str` (single-copy atomic when aligned).
  `half`, `bfloat`, `float`, `double` go through `x16` and `fmov`.
  `i128` (LL/SC): a load is a `ldxp`/`stxp` loop that stores back what it
  read (release on the store only for `seq_cst`); a store is a
  `ldxp`/`st(l)xp` loop.
- **The exclusive window.** Every operand, address and temporary is in a
  register, and the 128-bit operands are in `v30`/`v31`, before the loop
  label is bound; the loop body is only instruction words and branches
  (`x16`, `x17`, `w17` for the status of narrower loops). So no glue (a
  spill, a reload, a rematerialised address) is placed between the
  exclusive load and the exclusive store, under any `Stress` setting.
  A `cmpxchg` whose comparison fails leaves through `clrex` (an `i128`
  one first stores the old value back with `st(l)xp`, so the failure
  is also an atomic read).
- **Operations.** Sub-word signed minimum and maximum sign-extend the
  old value into `x16` and compare with the operand sign-extended;
  `uinc_wrap` is `old >= v ? 0 : old + 1`, `udec_wrap`
  `old == 0 || old > v ? v : old - 1`, `usub_cond` `old >= v ? old - v
  : old`, `usub_sat` `old >= v ? old - v : 0`. `fmax`/`fmin` are
  `fmaxnm`/`fminnm`, `fmaximum`/`fminimum` `fmax`/`fmin`; `half`
  arithmetic converts to single and back (exact: one rounding). `i128`
  operations are pairwise (`adds`/`adc`, `subs`/`sbc`, `cmp` with `sbcs`
  for the comparisons).
- **`cmpxchg` result.** `{ T, i1 }` is a `Mem` value: the old value at
  offset 0 and the flag byte at the second field's offset. A sub-word
  outline compare zero-extends the expected value (`cmp ... uxtb/uxth`).
- **`fence`**: `dmb ishld` for `acquire`, `dmb ish` otherwise;
  `syncscope("singlethread")` emits nothing (a compiler barrier: hod
  does not reorder).
- **Refused, by name:** `a64:atomic-unaligned`, an access whose
  alignment is below its size (LLVM calls `__atomic_*` there; hod has no
  such call yet); `a64:atomic-type`, an atomic of `fp128`, a vector, an
  integer that is not 8, 16, 32, 64 or 128 bits, or an operation whose
  type does not fit it; `a64:atomic-bfloat-op`, `bfloat` arithmetic
  (`xchg`, loads and stores of `bfloat` are lowered).

Tests: `crates/hod_a64/tests/atomic.rs` (the table, the window scan,
the helpers, the refusals, the threaded programs of
`tests/common/atomic_threads.rs`), `crates/hod/tests/atomic_cxx.rs`.

#### Constant operands with addresses (WP-072)

`src/constaddr.rs`, hooked from `common::materialize_gpr_const` and
`common::materialize_mem_const`. Rows `operand:int-of-addr`,
`operand:sym-diff` and `operand:address-in-aggregate` are on.

- **Everything is computed in code.** An address in an operand constant
  is formed by §7.4's access (`common::sym_address`: `adrp`+`add`, or the
  GOT) or by the function's own block address (`common::block_address`),
  then stored or kept in the register. No constant pool and no data
  relocation: the code is the same shape for PIC and non-PIC, a
  preemptible or `extern_weak` symbol comes from its GOT entry (0 for an
  undefined weak one), and a difference of symbols in different sections
  or a truncated address needs no relocation that could not express it.
- **`IntOfAddr`** of class `Gpr` is its address in the register (the bits
  above a narrower type's width are unspecified, §2.2); in an image it is
  a field of 1, 2, 4 or 8 bytes.
- **`SymDiff`** of class `Gpr` (`operand:sym-diff`) is `a + addend` in
  the destination, `b` in the other scratch register
  (`common::other_scratch`), `sub`. Writes the destination and that
  scratch register only, as a block address does.
- **`Mem`-class images.** An image whose relocations are all aligned
  8-byte symbol addresses is written by `common` as before. Any other
  (a field of 1, 2 or 4 bytes, an unaligned field of a packed struct, a
  block address, `dso_local_equivalent`, a difference) is written by
  `constaddr::write_image`: the bytes between fields as 8/4/2/1-byte
  stores (`xzr` for zero), each field computed in `x17` (`x16` the second
  register of a difference) and stored with its width, a fresh frame
  cursor (scratch `x16`) per store. Writes `x16` and `x17` only.
- **`dso_local_equivalent @f`** in an image, or in a difference, is `f`'s
  own address: what a call through it needs. As a scalar operand it is
  still `operand:dso-local-equivalent` (and `no_cfi` `operand:no-cfi`).
- **Refused**: an address in an integer whose width is not 8, 16, 32 or
  64 bits inside an image, and an `i128` (or wider) `ptrtoint` of an
  address, have no image: `a64:constant`. A vector constant with an
  address that is not an aligned 8-byte symbol address stays
  `a64:constant-relocation` (vectors are WP-078 and after). A
  thread-local symbol in a constant is `classify`'s `tls:address-in-constant` rule.
  Every other expression `hod_ir` keeps (`Expr`) is `const-expr:<op>`.

#### The groups together (WP-066)

- **Fused `condbr`** (`control.rs`): a `br i1 %c, ...` with two
  different targets whose `%c` is deferred (§4.6: its only use is the
  branch, the next instruction) does not materialize `%c`. If
  `int::fusable` accepts the compare, its other deferred operands are
  emitted (`deferred_first`), `int::flags` sets the flags and yields one
  condition; an `fcmp` goes through `float::flags`, which yields one or
  two conditions (`one`/`ueq` need two; `fp128` calls `__lttf2` and
  friends first). Then: with one condition and the true block next,
  `b.<inverse>` to the false block and fall through; otherwise
  `b.<first>` to the true block, `b.<second>` to it when there is a
  second, and `term::br` to the false block (omitted when it is next).
  `branch_cond` keeps its far form. A compare with another use, or one
  not right before the branch, is materialized and tested with `tbnz`,
  as before. Phi stores precede the compare's flag setting, so they
  cannot clobber the flags.
- **The facade's classifier** (`crates/hod/src/front.rs`): for
  `Arch::Aarch64` the `Judge` and the census use
  `Classifier::with_stack_args(&TargetStackArgs::new(&A64))`, so a
  `musttail` whose stack argument area fits the caller's is judged
  (`musttail:stack-args-grow` when it does not); x86-64 keeps no rule
  (`musttail:stack-args-unknown`).
- **`invoke` with an odd-leaf `Mem` result**: lowered since WP-084
  (below).
- Tests: `tests/fused.rs` (every predicate × width or float type ×
  operand form × block layout, the two-use and separated shapes, and an
  object check that a fused branch has no `cset`), `tests/phis.rs`
  (diamonds with critical edges, loops with permuted phis, selects, of
  every class), `tests/mem_calls.rs` (`Mem`-class arguments and results
  in registers, on the stack, `byval`, `sret`, between hod, `llc` and
  `gcc` both ways), `crates/hod/tests/musttail.rs`; `tests/diff.rs`
  with every `groups.rs` line on.

#### Thread-local storage and symbol offsets (WP-073)

`src/tls.rs` writes the address of a thread-local symbol by the model
`hod_cg::sym_access` names (§7.4), the sequences AArch64 Linux `llc`
writes with the same relocations, so every linker recognizes and relaxes
them; `common::sym_address` calls it for `Access::Tls`.

- **General and local dynamic** are TLS descriptors (LLVM lowers local
  dynamic so, on the symbol itself; in a shared library a `dso_local`
  symbol goes through `.L<name>$local`, §7.4): `adrp x0, :tlsdesc:s`;
  `ldr x1, [x0, :tlsdesc_lo12:s]`; `add x0, x0, :tlsdesc_lo12:s`;
  `blr x1` (`R_AARCH64_TLSDESC_ADR_PAGE21`, `_LD64_LO12`, `_ADD_LO12`,
  `_CALL`); then `mrs x1, tpidr_el0`; `add rd, x1, x0`. The call
  clobbers `x0`, `x1`, `NZCV` and `x30` (`llc`'s `TLSDESC_CALLSEQ`; the
  resolver keeps the rest, `CSR_AArch64_TLS_ELF`), so the sequence is
  bracketed by `stp x0, x1, [sp, #-32]!`; `mrs x1, nzcv`;
  `str x1, [sp, #16]` and, after it, `ldr t, [sp, #16]`; `msr nzcv, t`
  (`t` is `x1`, `x0` when `rd` is `x1`), the reload of `x0`/`x1` but
  `rd`, `add sp, sp, #32`. `x30` holds no value in a body (the prologue
  saved it); the CFA is `x29`-based, so the stack-pointer moves need no
  CFI row. The `blr` is a call site (§7.1, `record_call_site`).
- **Initial exec**: `adrp rd, :gottprel:s`; `ldr rd, [rd,
  :gottprel_lo12:s]` (`R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21`,
  `_LD64_GOTTPREL_LO12_NC`); `t` (`x0`, `x1` when `rd` is `x0`) saved
  with `str t, [sp, #-16]!`, `mrs t, tpidr_el0`, `add rd, t, rd`,
  `ldr t, [sp], #16`.
- **Local exec** (`llc`'s default TLS size, 24 bits): `mrs rd,
  tpidr_el0`; `add rd, rd, #:tprel_hi12:s, lsl #12`; `add rd, rd,
  #:tprel_lo12_nc:s` (`R_AARCH64_TLSLE_ADD_TPREL_HI12`, `_LO12_NC`).
- **Offsets.** TLS relocations carry addend 0, as `llc`'s; an offset is
  added after the sequence. `common::add_addend` adds up to 2^24 - 1 in
  magnitude with `add`/`sub` immediates; beyond, it borrows `x0` (`x1`
  when `rd` is `x0`) for a `mov`-built constant and restores it. The same
  rule serves `Direct` offsets that do not fold and every `Got` offset
  (§7.4, "Offsets from a symbol").
- **Operands.** A thread-local symbol, alone or with a constant offset
  (`getelementptr` of it), is an ordinary address operand (§3): it may be
  a call argument, a phi or `select` arm, a stored value.
  `llvm.threadlocal.address(p)` is the operand `p`; `llvm.thread.pointer`
  is `mrs rd, tpidr_el0`. `classify` still refuses a thread-local callee
  (`tls:direct-operand`) and a thread-local address in data or inside
  another constant (`tls:address-in-constant`).
- **Linker relaxations** seen (`tests/tls.rs`): bfd and lld relax
  descriptors to local or initial exec and initial exec to local exec in
  an executable; mold relaxes descriptors of preemptible symbols to
  initial exec (its form puts the two `nop`s first), even in a static
  link, and keeps initial exec as it is.

#### The short tail (WP-084)

`src/tail.rs`; each construct is reached by one call from the module
that refused it before.

- **`fp128` and `half` fields** of aggregates (`extractvalue`,
  `insertvalue`): `aggr` already moved an `Fpr` field through
  `fpr_view`; the matrix rows now include `half` and `fp128`.
- **Conversions between floating point and `i65`..`i128`**
  (`aggr::cast` → `tail::fp_int_cast`) are the calls `llc` makes:
  `fptosi`/`fptoui` call `__fix{,uns}{hf,sf,df,tf}ti` (a `half` operand
  directly), the 128-bit result is the value's home (the bits above a
  narrower width are unspecified, §2.2); `sitofp`/`uitofp` call
  `__float{,un}ti{sf,df,tf}` with the operand extended (by its
  signedness) into a 16-byte slot when it is narrower than 128 bits; to
  `half` they call the `sf` function and `fcvt h, s`, two roundings, as
  `llc`. Wider integers stay `a64:wide-fp-cast` (`llc` expands them
  inline), and so does `bfloat` (`llc` cannot select it either).
- **`{s,u}mul.with.overflow` wider than 64 bits** (`aggr` →
  `tail::mul_overflow_wide`): both operands extended (by signedness) to
  `2N` bits in slots of their own, `aggr::multiply` forms the exact
  product, the unsigned flag is any product bit at or above `N`, the
  signed one any such bit that differs from bit `N - 1`; the low `N`
  bits (top limb masked) and the flag byte are written as for the other
  overflow intrinsics. Beyond `N = MAX_QUADRATIC_BITS / 2` (2048) it is
  `a64:wide-mul-overflow`. `llc` lowers these inline too; no libcall.
- **`switch` on an integer of class `Mem`** (`control::switch` →
  `tail::switch_wide`): for each case not to the default, in the IR's
  order, each limb of the condition (the top one zero-extended) is
  loaded into `x16`, the case's limb built in `x17`, compared by `cmp`
  then `ccmp …, #0, eq`, and `b.eq` to the case; then the default. A
  constant condition is a `br`.
- **`readcyclecounter`** is `mrs rd, CNTVCT_EL0` (`SysReg::CntvctEl0`,
  read only), as `llc`.
- **`aarch64.crc32{,c}{b,h,w,x}`** are the `crc32*` instructions on the
  operands' registers (`b` and `h` read only the data register's low
  bits). `llc` cannot select them without `+crc` in the function's
  features; hod chooses no instruction by a feature (§5.5.2) and lowers
  them either way (ARMv8.1-A makes CRC32 mandatory).
- **`sshl.sat`, `ushl.sat`** up to 64 bits: the operand shifted to the
  top of the register, the amount zero-extended from the width, `lslv`,
  shifted back by `asrv`/`lsrv` and compared; on a difference the
  result is all ones (unsigned) or the extreme of the operand's sign
  (`asr #63` · `eor #0x7fff…`), then shifted down. `i128` and wider are
  `a64:intrinsic-wide`, vectors `a64:intrinsic-vector`.
- **`is.constant`** is true for what `llc`'s `isManifestConstant` calls
  manifest: data (a number, `null`, a zero, `undef`, `poison`, `none`, a
  byte string) and literal aggregates, splats and constant expressions
  all of whose operands are (`inttoptr` of an integer included). Anything
  holding a symbol's or a block's address is false.
- **The operands of folded `is.constant` and `objectsize`.** `llc`'s IR
  pass (`LowerConstantIntrinsics`; in LLVM 22 inside
  `pre-isel-intrinsic-lowering`) replaces those calls by constants and
  folds branches on them; it deletes nothing else. SelectionDAG then
  builds each block's nodes and deletes every node nothing uses that has
  no effect, recursively (a non-volatile load included); a value used in
  another block, or by a `phi`, is exported and stays. So in a function
  that calls either intrinsic (`Target::begin_function` looks at the
  call sites, nothing more for a function without them), hod does not
  emit an instruction that has no effect (the driver's `has_no_effect`
  set), is not used outside its block or by a `phi` by any instruction
  but a folded call, and whose users in its block are all not emitted
  (`tail::begin`, one backward pass per block; `target::lower` returns at
  once for them). GCC's `bcp-1.c` (`__builtin_constant_p(p[123456])`
  with `p` null) is the case: the load is not run. The same holds for
  `objectsize`'s pointer operand. Not modelled: the blocks `llc` deletes
  after folding a branch on the result (hod emits them; they never run).
  In functions without these calls hod still skips only an instruction
  nothing uses (§4.7), not its operands; `llc` deletes such dead chains
  everywhere, a known difference for loads through invalid pointers in
  dead code.
- **`invoke` with an odd-leaf `Mem` result**: `FnCx::call`
  (`hod_cg/src/call.rs`) clears the bits above the odd leaves of an
  `invoke`'s result in its home after the call and before the normal
  edge's phi stores and branch (`clear_odd_leaves`, as for incoming
  arguments); `call::call` clears a plain call's itself as before.
- **Stays refused**: `const-expr:trunc` (a truncated difference of two
  `blockaddress`es in a global's initializer; needs a constant form for
  label differences in `hod_ir`, the readers and module emission),
  `special-global:llvm.global.annotations` (`llc` drops the array, a
  global in section `llvm.metadata`; the readers would have to treat it
  as one of their special arrays, `hod_ll/src/globals.rs` and
  `hod_bc/src/module.rs`), `type:bfloat` (a type class of its own: ABI,
  conversions, arithmetic through `float`).

#### NEON intrinsics (WP-083)

`src/neon.rs`, reached by one call: the last arm of
`intrinsic::intrinsic` hands it every intrinsic nothing before it took.
It lowers the AArch64 target intrinsics on `hod_ir`'s list (§5.5,
`spec/ir.md`): `llvm.aarch64.neon.*`, `crypto.*`, `sisd.*`, `cls`,
`cls64`, `dmb`, `dsb`, `isb`, `hint`. The bfloat ones (`bfcvt`,
`bfcvtn`, `bfcvtn2`, `bfdot`, `bfmlalb`, `bfmlalt`, `bfmmla`) are on the
list with a matrix row that is not supported, so they stay
`intrinsic:<name>` (vectors of `bfloat` are not lowered; the scalar is,
WP-095 below).

- **Target features.** Like `crc32` (WP-084), an intrinsic is lowered
  whatever the function's `target-features` say: a feature is
  availability (§5.5.2), and hod never chooses or refuses an instruction
  by one. `llc` refuses some of them without the feature (`+aes`,
  `+sha3`, `+dotprod`, ...); hod emits them, and the program is as
  correct as the CPU it runs on supports the instruction.
- **Table-driven.** Each intrinsic maps to a lowering family; the family
  reads the call's types (vector width and element, or a scalar) and
  takes the encoder family of `hod_mc_a64::neon` that has that shape: the
  vector form, the scalar (`<1 x i64>`, `i32`, `i64`, `float`, `double`,
  `<1 x double>`) form, or the general-register form (`fcvt*` to `i32`/
  `i64`, `cls`). The checked encoder refusing the shape is the refusal.
- **Operands.** The first two vector-register operands are `use_reg`'d
  (an accumulating instruction writes operand 0's register); the others
  are put into `v30`, `v31`, then temporaries; an `i32`/`i64` operand is
  moved in with `fmov`. A general result is moved out with `fmov`,
  zero-extending: also the signed reductions of bytes and halfwords
  (`saddv`, `smaxv`, `sminv`), as `llc` does. `saddlv` of bytes is the
  exception (WP-101, `tail3::saddlv`): its 16-bit signed sum is
  sign-extended to `i32` (`smov w, v.h[0]`), as `llc` 19 does; `llc` 22
  zero-extends it, which is a different `i32` for a negative sum
  (`<arm_neon.h>` truncates either to 16 bits).
- **Consecutive registers.** `tbl`/`tbx` tables and the structure loads
  and stores use `v24`..`v27`, taken (`take_reg`) before the deferred
  operands. A structure result (`ld2`.. `ld4`, `ld2lane`.., `ld2r`..) is
  a `Mem` value: the registers are stored to its slot. `ld<n>`/`st<n>`
  of `<1 x i64>`/`<1 x double>` are `ld1`/`st1` of `n` registers, as
  `llc`.
- **Composite forms.** `sq{,r}dmulh.lane{,q}`: `dup` of the lane into
  the second operand's register, then the instruction. Two-element
  reductions are the scalar pairwise forms (`addp d`, `faddp`/`fmaxp`..
  `s|d`); `faddv` of four floats is `faddp` then `faddp s`; `saddlv`/
  `uaddlv` of `<2 x i32>` is `saddlp`/`uaddlp` to `1D`. `fmax`/`fmin`/
  `fmaxnm`/`fminnm` of `<1 x double>` are the scalar instructions,
  `frint{32,64}{x,z}` of `<1 x double>` the scalar `frint`.
  `vcvtfp2hf`/`vcvthf2fp` are `fcvtn`/`fcvtl` of `4S`/`4H`. `sqshlu`'s
  shift is a constant splat (or constant). `pmull64`'s operands are
  moved from general registers.
- **Barriers and hints.** `dmb`/`dsb` take a constant option that is a
  barrier (`Barrier::ALL`), `isb` only 15 (`SY`), `hint` any constant
  below 128.
- **Refused by name.** `a64:intrinsic-target`: a known intrinsic with a
  signature no instruction takes (`smax` of `<2 x i64>`, `frint32x` of
  half, a type that is not a 64- or 128-bit vector). `a64:intrinsic-
  immediate`: a shift amount, lane or barrier option that is not a
  constant in range.

Tests: `crates/hod_a64/tests/neon.rs` (`cases_match_llc` over
`tests/data/neon/cases.txt`, `every_case_compiles`, the refusals); the
encodings `crates/hod_mc_a64/tests/l1/neon.rs`.

#### The short tail, second part (WP-095)

`src/tail2.rs`; each construct is reached by one call from the module
that refused it before (WP-084's "stays refused" list is settled here).

- **Scalar `bfloat`** (`float::{cast,binary,flags}` → `tail2`), as `llc`
  lowers it without `FEAT_BF16` (hod chooses no instruction by a
  feature): a `bfloat` lives in the low 16 bits of an `H` view like
  `half`. Widening to `float` is exact: `fmov w16, s` · `lsl #16` ·
  `fmov s, w16` (LLVM 22 writes `shll`, the same bits); to `double` it
  is that and `fcvt d, s`. Narrowing from `float` is `llc`'s integer
  rounding to nearest, ties to even, on the single-precision bits:
  `x + 0x7fff + bit16(x)`, upper half; for `fptrunc float` only, a NaN
  keeps its upper bits with the quiet bit set (`orr #0x400000`, `fcmp s,
  s`, `csel vs`). From `double`, `FCVTXN` (round to odd, encoded in
  `tail2`, not in `hod_mc_a64`) first and no NaN arm. `fadd`..`fdiv`
  widen both operands, compute in single precision and narrow without
  the NaN arm; `frem` calls `fmodf`; `fcmp` widens both and compares in
  single precision; `fptosi`/`fptoui` widen then `fcvtz*`; `sitofp`/
  `uitofp` from up to 16 bits are `scvtf`/`ucvtf s` (exact), from 32
  bits `?cvtf d` then `FCVTXN`, from 64 bits a round-to-odd by hand (the
  bits that do not fit 53 folded into a sticky bit, then `?cvtf d`, then
  the sticky bit ORed into the double's last bit) then `FCVTXN`; every
  path ends with the narrowing above. A conversion with an `fp128` side
  is `a64:bfloat-fp128` (`llc` 19 cannot select it); with an integer
  wider than 64 bits, `a64:wide-fp-cast`. `bfloat` arguments in arrays
  stay `abi:bfloat-array` (`hod_a64::abi`).
- **`extractelement` from `<N x i128>`** (`target.rs` → `tail2`): a
  `<1 x i128>` is class `Fpr` (stored as a `Q` register into the result's
  slot); a wider one is `Mem`, its lane's 16 bytes copied by the lane
  cursor into the result's slot. A variable index wraps as
  `vector::index` does.
- **Label differences** in initializers: an integer constant built only
  of `ptrtoint blockaddress`, numbers, `add`, `sub` and `trunc`, adding
  one block and subtracting another of the same function
  (`ConstPool::block_diff`, `spec/ir.md` §4.3), flattens to a
  `FixupTarget::BlockDiff` of 1, 2, 4 or 8 bytes. Module emission writes
  `offset(a) - offset(b) + addend` into the data, the number the
  assembler folds `llc`'s `.word .Ltmp1-.Ltmp0` to; no relocation. The
  function needs `block_offsets`, so an `indirectbr`; a block without
  one is `init:blockaddress-unplaced`. As an operand, in an aggregate or
  not, it is `operand:block-diff` (classify). Two blocks of different
  functions, or any other shape, keep the expression's name
  (`const-expr:sub`, `const-expr:trunc`).
- **Block addresses of an unreachable `indirectbr`**: `mark_reachable`
  (`hod_cg/src/driver.rs`) also gives a function `block_offsets` when
  its only `indirectbr` is in an unreachable block, so a `blockaddress`
  in data of it is placed (`llc` keeps those labels).
- **Address-taken blocks without an `indirectbr`** (WP-115):
  `mark_reachable` also gives `block_offsets` to a function with a
  block whose address is taken (`ConstPool::has_address_taken_block`)
  and no `indirectbr`. Of its unreachable blocks only the address-taken
  ones (`ConstPool::is_address_taken`) are bound to the trap and get a
  row, so a function without one emits no trap for them. `llc` deletes
  such an unreachable block and writes `1` for its address in data;
  hod writes the trap's address: neither can be branched to (no
  `indirectbr`), both are non-null and differ from every reachable
  block's.
- **`bitcast` of a constant vector to a scalar** is folded by
  `ConstPool::fold_cast` when every element is a number and the vector
  is at most 128 bits: its image, as `llc` folds it (`<8 x i1>` packs
  element 0 in bit 0).
- **`preserve_mostcc`**, as caller and callee: `Target::assign` assigns
  it as `ccc` (`tail2::assign_preserve_most`); a callee also saves
  `x9`..`x15` (LLVM's `CSR_AArch64_RT_MostRegs`: AAPCS64's set plus
  those; `x8`, `x16`..`x18` and the vector registers are not preserved)
  in 64 bytes after the frame record, right after `mov x29, sp`, with
  one `DW_CFA_offset` per register after the stores, and reloads them
  with `DW_CFA_restore` in every exit sequence before the frame record.
  All seven are saved (`llc` saves the ones its allocator touched; hod's
  lowerings may write any of them and every call clobbers them). A
  caller needs nothing: hod keeps no value in a register across a call.
- **`llvm.global.annotations`** in section `llvm.metadata` is dropped by
  both readers (`spec/ll.md`, `spec/bc.md`), and `"fmv-features"` is a
  dropped attribute (`spec/ir.md`): `llc` 22's object is the same
  without it.
- **Stays refused**:
  - `const-expr:sub` of a number minus an address (`0 - ptrtoint @g`, as
    an operand): `spec/ir.md` §4.2 keeps it an `Expr`; no ELF data
    relocation is `-S`, and as an operand it needs a normal form of its
    own and a lowering (`neg` of the address). One corpus module.
    (WP-101 gives it both, below.)
  - `metadata-arg:llvm.read_volatile_register.i64`: the register name is
    a metadata operand, which the IR does not carry for intrinsics
    (BACKLOG WP-064) and `hod_ll`'s body reader would have to read.
  - `debug:dbg-value`: was refused here, since hod wrote one location
    per variable for the whole function (§9.5). WP-162 writes value
    records as location lists (`spec/dwarf.md` §6.23); the one corpus
    module (`-O2`) now stops at `debug:dbg-value-arglist`.
  - `cc:preserve_allcc`, `cc:win64cc` (`cc:79` before WP-141) and `cc:aarch64_vector_pcs`
    (WP-082), which the one `preserve_mostcc` corpus module also uses.
    (`preserve_allcc` since WP-101, below.)

#### The short tail, third part (WP-101)

`src/tail3.rs`, reached by one call at the top of `A64Target::lower`
(`target.rs`) that answers only for the vector forms below, and by one
call each from the places named with the other items.

- **Vectors of `i128` and `fp128` lanes** ("wide lanes"). `<1 x i128>`
  and `<1 x fp128>` are class `Fpr` (one `Q` register); every longer one
  is `Mem`, lane `i` at byte `16·i`. A lowering spills an `Fpr` operand
  to a scratch image (`vector::scratch`) and works on memory images;
  an `Fpr` result is built in the last scratch image and loaded. Per
  lane, as `llc` scalarizes them:
  - `add`, `sub` (`adds`/`adc`, `subs`/`sbc`), `and`, `or`, `xor` limb
    by limb; `mul` as the scalar `i128` multiply (`umulh`, `madd`);
  - `icmp`: the low limbs compared, then `ccmp` of the high limbs for
    `eq`/`ne` or `sbcs xzr` for an ordering; the result is a mask
    (`<N x i1>`, at most 64 lanes, bit-packed in a `Gpr`; more is
    `a64:mask-wide`);
  - `fadd`, `fsub`, `fmul`, `fdiv`, `frem`, `fcmp` of `fp128` lanes are
    the runtime calls of the scalar (`__addtf3` .. `__divtf3`, `fmodl`,
    `__eqtf2`, `__lttf2`, `__letf2`, `__unordtf2`, with
    `float::quad_flags`' table); the mask of `fcmp` is collected in a
    frame slot across the calls; `fneg` flips bit 127;
  - `select` with a mask condition, `tst` and `csel` per limb;
  - `sext`/`zext` of a mask, of `i8`..`i64` lanes to `i128` lanes,
    `trunc` of `i128` lanes to a mask or to `i8`..`i64` lanes;
  - `insertelement`, `extractelement` (constant or variable index,
    wrapped as `vector::index` does) and `shufflevector` (an undefined
    lane is zero).
  Refused, by name: shifts, `udiv`, `sdiv`, `urem`, `srem` of `i128`
  lanes (`a64:vector-i128-op`: `llc` calls `__ashlti3`.. per lane; no
  corpus module); any other cast with a wide side
  (`a64:vector-wide-cast`: `sitofp`, `fpext`, `fptrunc`, bitcasts
  between floating-point and integer lanes); intrinsics on wide lanes
  stay `a64:vector-wide-element`.
- **Wide lanes across calls** (`abi.rs`, `spec/abi-a64.md` §3.3, §4.3):
  each `i128` element travels as an `i128` (two general registers under
  the even-register rule, or a 16-aligned stack slot), as both LLVM
  selectors in 19 and 22 pass it; an `fp128` element in a `Q` register.
  A lone `<1 x i128>` argument or result (WP-167; libc++'s
  `__load_vector` returns one for `std::find` over `__int128`) travels
  as an `i128` too: it is a `Q`-register value, so its two pieces are
  `Lanes` parts of one 64-bit limb each (§7.2), which `vcall.rs` moves
  with `umov xN, vM.d[i]` and `ins vM.d[i], xN` (through `x16` for a
  stack slot) between the scratch `v30` and the place. In a structure or
  an array it is memory and its pieces are plain bytes
  (`std::experimental::simd<__int128>` passes `[1 x <1 x i128>]
  alignstack(16)` and returns `{<1 x i128>}`).
- **Arrays of `bfloat`** (HFAs and `[n x bfloat]`, `spec/abi-a64.md`
  §3.3): passed as a block of `H` registers when `NSRN + n <= 8`, where
  `llc` 22 (a block) and `llc` 19 (piece by piece) place them alike;
  otherwise `abi:bfloat-array`, since the two versions put the stack
  pieces in different places.
- **Vectors of `bfloat` without `+bf16`.** Load, store, passing, lane
  insertion, extraction and shuffles are the 16-bit-lane forms of
  `vector.rs` (a 64- or 128-bit power-of-two count is `Fpr`, any other
  `Mem`). Per lane, as `llc` scalarizes them: `fadd`, `fsub`, `fmul`,
  `fdiv` widen both lanes to `float` (a 16-bit shift), operate in `S`,
  and narrow with round-to-nearest-even as the scalar does
  (`tail2::narrow`); `frem` calls `fmodf`; `fneg` flips bit 15; `fcmp`
  compares in `S` and collects the mask in a `Gpr`; `fpext` to `float` or
  `double`, `fptrunc` from `float`, or from `double` through `fcvtxn`
  (round to odd) then the narrowing. Any other cast is
  `a64:vector-bfloat-cast`.
  `llc -O0` of LLVM 19 and 22 miscompiles the vector form: its rounding
  takes the rounding bit from the operand's image rather than the
  result's (`ushr v1.4s, v0.4s, #16` of the loaded operand), so its
  vector results differ from its own scalar ones. hod follows the
  scalar, and its tests compare with `llc`'s build of the scalarized
  program.
- **The `bfloat` NEON intrinsics** `bfcvt`, `bfcvtn`, `bfcvtn2`, `bfdot`,
  `bfmlalb`, `bfmlalt` and `bfmmla` (`intrinsic.rs`, one arm): the
  instruction itself (FEAT_BF16), as `llc` emits for a function with
  `+bf16`; the accumulator operand is the result register. Another
  signature is `a64:intrinsic-target`.
- **A number minus an address** as an operand (`operand:neg-addr`,
  `spec/ir.md` §4.2; `common::materialize_gpr_const` hands an `Expr` to
  `tail3::neg_addr`): the symbol's address with `offset - addend` added,
  by the symbol access of §7.4, then `neg`. Data and aggregate operands
  are `init:neg-addr`.
- **`preserve_allcc`**, as caller and callee, done as `preserve_mostcc`
  above (`tail3::assign_preserve_all`, `save_all`, `all_rows`,
  `restore_all`, one hook each in `target.rs` and `frame.rs`): assigned
  as `ccc`; a callee also saves `x9`..`x15` and the whole of `q8`..`q31`
  (LLVM's `CSR_AArch64_RT_AllRegs`, the same in 19 and 22; `x8`,
  `x16`..`x18` and `q0`..`q7` are not preserved) in 448 bytes after the
  frame record (`x9` lowest, then `q8` lowest, `stp` pairs), with one
  `DW_CFA_offset` per register after the stores (a vector register's
  DWARF number is 64 plus its index, as `llc`'s `.cfi_offset b8`), and
  reloads them with `DW_CFA_restore` in every exit sequence. All are
  saved, as `llc -O0` saves them in a function with a call.
- **`saddlv` of bytes** (`llvm.aarch64.neon.saddlv.i32.v8i8`/`.v16i8`,
  `tail3::saddlv` from `intrinsic.rs`): `saddlv h30`, then `smov w,
  v30.h[0]`, the sum sign-extended, as `llc` 19 (see "Operands" in the
  NEON section; `llc` 22 zero-extends). Every other `saddlv` signature
  is `neon`'s.

#### 5.5.2 Target features (WP-043)

`hod_a64::features::check` judges a `target-features` string (a
function's attribute) and each `-mattr` item, read as §7.3 "Target
features" says (`hod_cg::features`) against AArch64's table. hod emits
ARMv8.0 code with FP and Advanced SIMD and never chooses an instruction
by a feature, so:

- **Accepted, either sign**: a feature that only makes instructions or
  system registers available. The architecture versions `v8a`..`v8.9a`,
  `v9a`..`v9.6a`; `fp-armv8` and `neon` enabled; the extensions of LLVM
  19's and 22's `AArch64Features.td` (`aes`, `sha2`, `sha3`, `sm4`,
  `crypto`, `crc`, `lse`, `lse2`, `rdm`, `ras`, `rcpc`, `fullfp16`,
  `dotprod`, `i8mm`, `bf16`, `pauth`, `bti`, `mte`, `sve*`, `sme*`, `gcs`,
  ...; the full list is `AVAILABILITY` in `crates/hod_a64/src/features.rs`);
  `fmv`, which changes no code hod emits (no function multi-versioning);
  and `outline-atomics` with `lse`, which pick an atomic sequence
  ("Atomics (WP-074)"), each one correct on every ARMv8.0 core. `+bti` and
  `+pauth` are availability: whether a function must begin with `bti` or
  sign its return address is the branch-protection attributes and module
  flags, which `classify` refuses (§5.5).
- **Refused by name**: a feature that changes the contract the code
  keeps. Disabling one of the last eight is accepted: it is the default,
  which hod's code keeps.

  | item | reason |
  |---|---|
  | `-fp-armv8` | `attr:target-feature-soft-float` |
  | `-neon` | `attr:target-feature-no-neon` |
  | `+strict-align` | `attr:target-feature-strict-align` |
  | `+reserve-<reg>` | `attr:target-feature-reserve` |
  | `+call-saved-<reg>` | `attr:target-feature-call-saved` |
  | `+tagged-globals` | `attr:target-feature-tagged-globals` |
  | `+execute-only` | `attr:target-feature-execute-only` (hod writes data islands) |
  | `+harden-sls-*` | `attr:target-feature-harden-sls` |
  | `+fix-cortex-*` | `attr:target-feature-erratum` |
  | `+v8r` | `attr:target-feature-profile` |

- **Unknown, refused**: any other name, either sign, and an item without a
  sign: `attr:target-feature-unknown`. A feature hod has not judged is not
  assumed harmless; adding one to a list is a decision with a test.

The first refused item of the string is the answer, and the facade
reports it as the refusal's detail (`spec/cli.md` "The back half").
Tests: `features.rs` (`what_compilers_write_is_accepted`,
`contract_features_are_refused_by_name`, `unknown_features_are_refused`),
`crates/hod/tests/back.rs` `target_features`.

#### 5.5.3 Inline assembly with instructions (WP-103)

`hod_a64::inline_asm::call` lowers every `call`, `invoke` and `callbr`
of inline assembly (`call.rs` sends each one there). A template that is
empty goes to "Assembly that needs no assembler (WP-075)" first; what
that module refuses by its own model (an output it does not take, `Q`,
two outputs, a struct result) falls through to the sequence below, which
handles any template. The template is substituted and assembled by
`hod_asm` (`spec/asm.md` §4, §5) **before** anything is emitted, so a
template that refuses leaves no trace; the words written are exactly
the words `hod_asm` encoded.

**Operands.** Constraints are read as `spec/asm.md` §6 lists. Of several
alternatives (`r|m`), the one of LLVM's weights, as in WP-075. Within
the chosen alternative an operand is, in order: its explicit register
(`{x0}`, `{w3}`, `{d8}`, `{v1}`); its tied output's place (a digit, or
`+`); an immediate (`i`, `n`, `I`..`N`) for a constant integer, a
symbol (`i`) for a symbol's address; memory (`m`, `Q`, `o`) when the
alternative has it and the operand is no tied output; else a register,
general for `r` and integer or pointer `X`, vector for `w`, `x`
(`v0`..`v15`), `y` (`v0`..`v7`) and floating-point `X`. A flag output
`={@cc<cond>}` is no register: it is read after the template. A label
operand (`!i`, `callbr` only) prints as a local name the sequence
binds.

**Registers.** Every operand gets a register of its own, distinct from
every other operand's and every clobber, except a tied input, which
shares its output's (giving an output its own register is always a
valid choice of what `llc` might share). General registers are chosen
from `x9`..`x15`, then `x0`..`x8`; vector registers from `v16`..`v29`,
then `v0`..`v7`. Refused: `sp`, `x29`/`fp` as an operand or clobber
(`a64:inline-asm-frame-register`: the frame is addressed from them), and
`x16`, `x17`, `v30`, `v31` as an operand (`a64:inline-asm-scratch-register`:
the sequence uses them). The clobbers `~{x16}`, `~{x17}`, `~{x18}`,
`~{v30}`, `~{v31}` are accepted: no value of hod's lives in them across
the template (hod never allocates `x18`).

**The sequence.**

1. Deferred operands are emitted (as for any call). Every input, and the
   pointer of every indirect or memory operand, is evaluated and stored
   to a 16-byte frame slot (one per distinct value); every direct
   output gets a slot.
2. Every register value is displaced (§4.4: the template is a call for
   the location policy); every register the template may write
   (operands, clobbers) is taken. Those among `x19`..`x28` and
   `v8`..`v15` (their low 64 bits, AAPCS64) are saved to slots: a
   template may clobber them and the function's own saves do not cover
   them. `x30` is always free across the template (the frame record
   holds the return address), so `~{lr}` and `~{x30}` need nothing;
   `llc` ignores `~{x30}`, hod honours both.
3. Inputs are loaded into their registers, vector ones first; an
   integer narrower than 64 bits is zero-extended (`i1` included);
   a memory operand's address is loaded into its register.
4. The template's words; each symbol reference becomes a relocation of
   its kind (`CALL26`, `JUMP26`, `CONDBR19`, `TSTBR14`, `LD_PREL_LO19`,
   `ADR_PREL_LO21`) against the module's symbol.
5. Register outputs are stored to their slots, or for an indirect
   output (`=*r`) through its pointer, its `elementtype`'s size; each
   flag output is `cset` of its condition and stored; the saved
   callee-saved registers are reloaded.
6. The result is read from the slots: one scalar, or each field of the
   struct of several outputs at its offset.

**Flags.** `sideeffect` and `alignstack` change nothing (hod moves no
instruction across the template, and `sp` is always 16-byte aligned at
it). `inteldialect` with a non-empty template is `asm:dialect`.
`unwind` on an `invoke` of a non-empty template is
`a64:inline-asm-unwind` (no call site is recorded for the template's
words); an `invoke` of assembly that does not unwind continues at its
normal destination.

**`callbr`** (asm goto): the normal path stores its phis and branches to
the normal destination; then, for each indirect destination, a
trampoline the template's branches to `${N:l}` are fixed up to: it
reloads the callee-saved registers, stores that edge's phis and branches
to the destination. Outputs (WP-112) are written on every path: the
normal path writes them as a `call` does, and each trampoline writes
them again, after reloading the callee-saved registers and before that
edge's phis, from the registers the template left them in (the result
is a home or dead, never a register value, so writing it twice is one
store each time). `llc` 22 stores the outputs in the indirect
destination's block; `llc` 19 does too, but there it can reload a
spilled output before storing it (a two-output `callbr`, seed
`goto_out2`, returns a stale value on the indirect edge). hod follows
22; the seed test expects that one failure of `llc` 19's object. An operand
bundle on any inline-assembly call is `a64:inline-asm-form`.

**Refused by name**, besides `spec/asm.md` §8:
`a64:inline-asm-frame-register`, `a64:inline-asm-scratch-register`,
`a64:inline-asm-operand-type` (an aggregate, an integer wider than 64
bits, a vector wider than 16 bytes, more than 8 bytes in a general
register), `a64:inline-asm-operands` (more than 16 operands, 4
alternatives, or the registers of a class), `a64:inline-asm-unwind`,
`a64:inline-asm-form` (an operand bundle); `asm:constraint/count` when the constraints do
not match the call's arguments, result or labels; `asm:symbol` for a
template reference to a name the module does not have.

**Tests** (`tests/inline_asm.rs`, seeds in `tests/data/inline_asm/`):
32 functions (syscalls through `svc`, `mrs`/`msr` of `tpidr_el0`,
`cntvct_el0` and `fpcr`, an exclusive-monitor loop, explicit registers,
tied, memory and `rm` operands, clobbers of registers hod uses and of
callee-saved ones, flag outputs, immediates with modifiers, two
outputs, an indirect output, a `bl` from the template, variants and
`${:uid}`, local labels, `asm goto` with phis, `invoke`) built by hod
(plain, `Stress::ALL`, far) and by `llc` 22 and 19, linked with a C
driver under bfd, lld and mold, dynamic and static; the template's
words compared with `llvm-mc`'s; the refusals; features from
`target-features`.

### 5.6 x86-64 (normative for `hod_x64`)

```
            | caller's frame                       |
            | incoming stack arguments [rbp+16+n]  |
            | return address           [rbp + 8]   |  CFA = rbp + 16
      rbp ->| saved rbp                [rbp + 0]   |
            | the save set, n of rbx, r12..r15     |  [rbp - 8] .. [rbp - 8n]
            | local area: FRAME bytes              |  Local(o) = [rbp + o - 8n - FRAME]
      sp -> +--------------------------------------+
            | dynamic allocations, outgoing args   |  (rsp moves below)
```

- **Prologue**: `push rbp` · `mov rbp, rsp` · the pushes of the save
  set, the `n` of `rbx`, `r12`..`r15` the function used, in that order ·
  `sub rsp, FRAME` (omitted when `FRAME` is 0, `imm8` when it fits).
  It is emitted at its largest and cut to this in `end_function`
  (§5.6.15). With the return address and `rbp` the pushes are `16 + 8n`
  bytes, so `FRAME` is the local area rounded up to `≡ 8n (mod 16)`, and
  `rsp` is 16-aligned after the prologue and at every `call`. `FRAME`
  up to `2^31 - 2^16`; above it, `limit:frame-size`.
- **The red zone is not used**; `noredzone` is accepted and changes
  nothing.
- **Frame places.** `FrameRef::Local(o)` is `[rbp + o - 8n - FRAME]`:
  `FRAME` is not known while the body is emitted, so the access is
  emitted as `[rbp + o]` with a 32-bit displacement and every such
  displacement is recorded and biased by `-8n - FRAME` in
  `end_function` (`hod_x64::common::frame_inst`); one that then fits a
  signed byte is rewritten as `disp8` and the three bytes it no longer
  needs are cut (§5.6.17). `Incoming(n)` is
  `[rbp + 16 + n]`: no incoming base slot. `Outgoing(n)` is `[rsp + n]`.
  The frame base for debug information is `rbp` (DWARF 6).
- **Exit sequences.** The epilogue is `lea rsp, [rbp - 8n]` · the pops
  of the save set in reverse · `pop rbp` · `ret` (`leave` · `ret` when
  `n` is 0); nothing in it depends on `FRAME` or on dynamic allocation. A tail exit is the same teardown ending in `jmp
  callee` (`R_X86_64_PLT32`) or `jmp r11`.
- **CFI** (design 0006 §4), written as the prologue is emitted, since the
  CFA is `rbp + 16` from the second instruction on: after `push rbp`
  `def_cfa_offset 16` and `offset rbp, -16`; after `mov rbp, rsp`
  `def_cfa_register rbp`; after the last save, one `offset` row per
  saved register, the `i`-th pushed at `-24 - 8i` (§5.6.15).
  After the `pop rbp` of an exit: `def_cfa rsp, 8`. A tail exit, which
  follows another exit sequence, first restates `def_cfa rbp, 16` (§9.3);
  the save rows never change. DWARF numbers (SysV psABI figure 3.36):
  `rax rdx rcx rbx rsi rdi rbp rsp` 0..7, `r8`..`r15` 8..15, `rip` 16,
  `xmm0`..`xmm15` 17..32.
- **Registers** (design 0006 §2.1). Pool: the save set `rbx`, `r12`..`r15`,
  and `xmm8`..`xmm13` (SysV has no callee-saved vector register: a value
  in one is displaced at a call, §4.4). Holdable: the pool, `rsi`, `rdi`,
  `r8`, `r9`, `xmm0`..`xmm7`. Scratch: `r11`, `r10` (in that order of
  use), `xmm14`, `xmm15`. `rax`, `rcx`, `rdx` are general registers that
  **never hold a value**: a lowering may clobber them without asking the
  driver (division, wide multiply, shifts by `cl`, `cmpxchg`, `al` of a
  variadic call), and a call result returned in `rax` is moved to a
  temporary at once (`hod_cg` `run_call`). Landing-pad data: `rax`,
  `rdx` (not holdable, so the landing pad moves them out). The indirect
  callee register is `r11`. `rsp` is in no set. A lowering pins at most
  four general pool registers (§4.2; the pool has five).
- **`nest`** is refused as `x64:nest`: LLVM passes it in `r10`, which is
  scratch here; the corpus has no use of it.
- **No deferral into an argument window across a fixed register.** An
  operand evaluated straight into an argument register must not clobber
  one already placed, and `rcx`/`rdx` are argument registers. The driver
  already guarantees it: `hod_cg::call` emits every deferred argument,
  and the deferred callee, before it places any (§7.1), so no lowering
  runs inside the window. A lowering that uses a fixed register needs no
  query of its own.
- **Two-operand instructions** (design 0006 §2.2). `dst = a op b` is
  `mov t, a` · `op t, b` · (`mov dst, t`); when `dst` is `b` and `op`
  does not commute the result is computed in a temporary. `lea` is the
  three-operand, flag-free `add`.
- **Flags** (design 0006 §2.3). Between the instruction that sets the
  flags and a fused reader (`jcc`, `setcc`, `cmovcc`) only flag-keeping
  glue is emitted: `mov` (an immediate included: zero is `mov r32, 0`,
  never `xor`), `lea`, `movzx`/`movsx`, `push`/`pop`, SSE moves. Every
  glue sequence of `hod_x64` keeps the flags: odd-size stores of a
  register bounce through the stack below `rsp` with `lea`, stack
  adjustments are `lea rsp, [rsp + d]`.
- **Classes.** Integers up to 64 bits and pointers are `Gpr`; `half`,
  `bfloat`, `float`, `double`, `fp128` are `Fpr`; `x86_fp80` is `Mem` (a
  16-byte slot of which 10 bytes are used; its arithmetic is X-x87's, on
  the x87 stack inside one lowering); a vector is `Fpr` when its length is
  a power of two, its size 64 or 128 bits and its element not `i1`; a
  vector of at most 64 `i1` is a mask of class `Gpr`; everything else is
  `Mem`.
- **Branches** (design 0006 §2.10). `jmp rel32` and `jcc rel32` reach ±2
  GiB, so there is no far mode and the §6.3 retry never fires. A branch
  to a label already bound uses the short `rel8` form when it reaches;
  any other is `rel32`, made `rel8` once the function is complete when
  it then reaches (§5.6.15). Fixup kinds: 0 `rel32`, 1 `rel8`; `at` is
  the field's offset, and the displacement written is `target - (at +
  field size)` (`hod_x64::common::patch_branch`). `unreachable` and
  `llvm.trap` are `ud2`. Functions are 16-byte aligned (the emitter
  pads between them with `llc`'s `nop`s, §5.6.6); the padding before a
  data island is `int3` (`0xcc`, `Target::PAD_BYTE`).
- **Integer constants**: `mov r32, imm32` (zero-extends) for a value of
  at most 32 bits; `mov r64, simm32` for one that sign-extends;
  `movabs` otherwise.
- **PIC, symbols and code model.** Calls are `call rel32` with
  `R_X86_64_PLT32`, local callee or not, as `llc` writes them. The
  address of a dso-local symbol is `lea r, [rip + x]` (`R_X86_64_PC32`,
  an addend below 16 MiB folded); of a preemptible one `mov r, [rip +
  x@GOTPCREL]` (`R_X86_64_REX_GOTPCRELX`, relaxable) then the addend
  added with `lea`. A `Code Model` other than small is `x64:code-model`.
  A thread-local symbol is §5.6.11's. A variadic call
  sets `al` to the number of vector registers used.

#### 5.6.1 Settled by the skeleton (WP-116)

- **Crate layout.** As `hod_a64` (§5.5.1): one module per group, the
  ownership table in `hod_x64/src/lib.rs`, the dispatch in `target.rs`
  final. It routes as on AArch64, with three additions: an instruction
  whose result or an operand is a mask goes to `mask` first; a `Mem`
  binary, `fneg`, cast or `fcmp` of `x86_fp80` goes to `x87`, of a
  vector to `lanes`; a call of inline assembly goes to `barrier`, the
  floating-point intrinsics to `fpintr`, the coroutine handle ones to
  `coro`, the thread-local ones to `tls`. Each stub refuses with
  `x64:todo:<group>` (`int`, `mem`, `control`, `call`, `vcall`, `aggr`,
  `float`, `fpintr`, `intrinsic`, `barrier`, `atomic`, `tls`, `vector`,
  `lanes`, `mask`, `x87`, `constaddr`, `coro`), a `pub const` of its
  module; the stub `abi` (WP-118 replaces it) assigns `ccc`, non-variadic
  signatures of up to six integer or pointer arguments and such a result,
  and refuses the rest as `x64:todo:abi`. Other names: `x64:funclet`,
  `x64:nest`, `x64:code-model`, `x64:todo:constant` (a constant `common`
  cannot materialize yet).
- **What the skeleton lowers**: `br`, `ret` (of a value in a register),
  `unreachable`, `phi` (the driver's copies over the glue), direct calls
  through the driver, constants, symbol and slot addresses.

#### Integers (WP-122)

`hod_x64/src/int.rs`: binary operations, `icmp`, casts, `select`,
`freeze` on `Gpr` values of every width (`i1` to `i64`, pointers). The
decisions that are not instruction selection are §5.5.1 "Integers
(WP-035)"'s.

- **Two operands.** `FnCx::use_reg` gives every operand in a register
  the lowering may overwrite, never the same register for two operands,
  so `dst = a op b` is `op a, b` in `a`'s register and the result stays
  there: the destination is never the right operand. A constant left
  operand of a commutative operation is swapped to the right. No `lea`
  is used for `add` (no flags are live at an `add`).
- **Widths.** A value of at most 32 bits is computed with 32-bit
  instructions, above with 64-bit ones; 8- and 16-bit instructions are
  never used (no `0x66`, no partial-register writes) except `setcc` and
  `test r8, 1`. Extensions as WP-035 decides which (left operand of
  `lshr`, `udiv`, `urem` and unsigned compares by zero; of `ashr`,
  `sdiv`, `srem` and signed compares by sign; the divisor too):
  `movzx`/`movsx` from 8 and 16 bits, `mov r32, r32` / `movsxd` from 32
  to 64, `and r32, 2^N-1` by zero below 32 bits, a `shl`/`shr` or
  `shl`/`sar` pair otherwise. A constant `lshr`/`ashr` of an odd width
  is `shl s` then `shr`/`sar s+k` (`s` the unused bits); of 8 or 16
  bits `movzx`/`movsx` then the shift.
- **Immediates.** A constant right operand of `add`, `sub`, `and`,
  `or`, `xor`, `imul` (three-operand), `cmp` is the instruction's
  immediate when the value, sign-extended from its width to the
  register, is one (`imm8` or `imm32`; any 32-bit value at 32 bits);
  else it goes to `r11`. `xor` with all ones is `not`; `add`/`sub`/
  `or`/`xor` of 0, `and` of all ones and `mul` by 1 emit nothing.
- **Fixed registers.** A register shift amount goes to `rcx` (`and
  ecx, 2^N-1` first for a width below 5, since `shl r32, cl` reads five
  bits), then `shl`/`shr`/`sar r, cl`. A division puts the extended
  dividend in `rax`, zeroes `edx` with `mov edx, 0` or sign-fills it
  with `cdq`/`cqo`, divides by the extended divisor in its own register
  (`r11` for a constant) with `div`/`idiv` at the register width, and
  moves `eax`/`rax` (quotient) or `edx`/`rdx` (remainder) to the
  divisor's register, or to a temporary. Undefined divisions (by zero,
  the smallest value by -1 at 32 or 64 bits) trap (`#DE`), which §4.7
  allows.
- **Compares.** `cmp a, imm32` (or `cmp a, r11`) with a constant, after
  extending `a` by sign or zero as the predicate orders; a constant
  left operand swaps the predicate. Two registers of 8 or 16 bits are
  both extended; of another width below their register both are shifted
  left by the unused bits (`shl a, s` · `shl b, s` · `cmp a, b`), which
  orders signed and unsigned alike. The conditions are `e ne a ae b be
  g ge l le`.
- **Fusion.** Every `icmp` defers itself when `may_defer` allows (§4.6);
  otherwise it is `cmp` + `setcc r8` + `movzx r32, r8`. `int::fusable(cx,
  value)` and `int::flags(cx, compare)` are WP-035's contract with an
  x86 condition (`hod_mc_x64::Cond`): the consumer calls `flags` after
  `deferred_first(cx, inst, Some(value))`, once, and emits nothing that
  writes the flags until its reader; `flags` pins at most two registers
  and writes them, the flags and `r11` (never `rax`, `rcx`, `rdx`).
  `select` fuses into `cmovcc`, `zext` of an `i1` into `setcc` +
  `movzx`, `sext` into `setcc` + `movzx` + `neg`. A non-fused `i1`
  condition is `test r8, 1` (bit 0: a plain `i1` may carry anything
  above it, `abi-x64.md` §7).
- **`select`** is `cmovcc` from the true arm's register into the false
  arm's, never a branch: there is no 8-bit `cmov`, and the 32-bit one
  gives the same low bits to any value of at most 32 bits (it zeroes
  bits 32..64 even when it does not move, which the width invariant
  allows). A constant arm is materialized with `mov` (flags kept).
- **Undefined results** (§4.7): a constant shift amount of the width or
  more, or 0, emits nothing.
- **`addrspacecast`** is refused as `x64:addrspacecast` (classify
  rejects it first); `x64:int-binary` and `x64:int-cast` name forms the
  dispatch never sends. `i128` is `Mem`: `aggr` (WP-126).

##### X-call: calls (WP-125)

- **Division of labour** as §5.5.1's WP-038: `hod_cg`'s call sequence
  places arguments, copies `byval`, receives results in memory, stages
  `musttail` and records call sites; `frame::emit_call` emits `mov al,
  n` (a variadic callee: the vector registers used), then `call rel32`
  (`R_X86_64_PLT32`) or `call r11`; tail exits are `jmp rel32` or `jmp
  r11`. `crate::call` adds the rest below.
- **The address handed back in `rax`** (`abi-x64.md` §5.2, §6.1): a
  function with an `sret` parameter, or whose result travels in memory,
  saves the address in a frame slot in the prologue (`call::enter`,
  before any argument register is read) and reloads it into `rax` at the
  start of the shared epilogue (`call::before_return`), which every
  `ret` reaches. A `musttail` exit does not reload it: the callee
  returns it.
- **Extension** (`abi-x64.md` §7): `Target::extend_arg` brings a
  `zeroext`/`signext` integer below 32 bits to 32 (`movzx`/`movsx` for 8
  and 16, else `and` or `shl`·`sar` on 32 bits), one of 33..63 bits to 64
  (`shl`·`shr`/`sar`), and the last piece of a wider one to 64. 32 and 64
  bits need nothing. Results take the same rule (the core calls it at
  `ret`), which over-delivers §7's 8 and 16 for narrow results.
- **Odd leaves.** After a call whose `Mem`-class result has at most 16
  leaves (it may come back in registers) and an integer leaf whose width
  is not a multiple of 8, the byte holding that leaf's top bit is masked
  in the result's home (through `r11`): a producer owes nothing above a
  leaf, memory holds zeros there (§2.2). An `invoke`'s are cleared by
  the call sequence.
- **Variadic definitions** (`abi-x64.md` §9.3) always save `rdi`..`r9`
  and `xmm0`..`xmm7` (`movups`) into a 176-byte, 16-aligned register save
  area in the prologue, and `al` into a slot of its own. `va_start`
  writes the 24-byte `va_list`: `gp_offset` = 8 x the named general
  registers, `fp_offset` = 48 + 16 x the named vector registers,
  `overflow_arg_area` = `rbp + 16 +` the named stack area,
  `reg_save_area`. `va_copy` copies 24 bytes (`xmm15`, `r11`), `va_end`
  is nothing, `va_arg` is refused (`x64:va_arg`; front ends expand it).
  `va_start` outside a variadic function is `x64:va-intrinsic`.
- **`musttail`** in a variadic function first reloads the argument
  registers no named argument uses from the save area (every live value
  displaced before), and its tail exit reloads `al` (`movzx eax, byte`):
  the callee receives the variable arguments as this function did. `r10`
  and `r11` are not forwarded (a `nest` is refused anyway); the stack
  area beyond the named arguments is never written. `tail` and `notail`
  are hints: no tail call is made for them. Indirect calls and tail
  exits go through `r11`.
- **Landing pads** are `{ ptr, i32 }`: `rax` and `edx` (`RegInfo::
  eh_data`) are stored to the pad's home; clauses are `catch` of a symbol
  or `null` and `filter` of arrays of them. `resume` calls
  `_Unwind_Resume` with the exception pointer, then `ud2`. The tables are
  `hod_eh`'s for x86-64: the personality and LSDA pointers are always
  `pcrel | sdata4` (indirect for the personality), also in a static
  object, where `llc` uses absolute `udata4` pointers.
- **Refused by name**: `x64:call` (a `callbr` that is not inline
  assembly), `x64:call-bundle`, `x64:va-intrinsic`, `x64:va_arg`,
  `x64:landingpad`, `x64:landingpad-clause`, `x64:resume`,
  `x64:musttail-variable-arguments` (a `musttail` call with variable
  arguments before a forwarded `...`, `abi-x64.md` §8.5); an
  `x86_fp80` part in a signature stays `x64:todo:abi` (WP-131), a
  `float`/`double` return part in `st0`/`st1` is `x64:ret-x87-part`
  (§5.6.9), a value returned in memory at `ret` is WP-126's.

#### 5.6.2 Target features (WP-116, WP-201)

As §5.5.2, with `hod_x64::features`'s table (design 0006 §10.1). hod
emits the base x86-64 instruction set (SSE2) and never chooses an
instruction by a feature, so a feature that only makes instructions
available is **accepted and ignored**, `+` or `-`. WP-201 judged every
feature of LLVM 19's and 22's x86 lists (`llc -mattr=help`) and every
feature clang 19, clang 22 and kiln write for the `-march=` CPUs they
accept (`x86-64-v2` to `-v4`, `nehalem` to `diamondrapids`, `znver1` to
`znver5`, ...): `crates/hod_x64/tests/features/x64_features.tsv` is the
table, `x64_census.tsv` the census, `x64_cpus.tsv` the CPUs against
outcomes.

- **Available** (`AVAILABLE`): the baseline `mmx`, `fxsr`, `cmov`, `cx8`,
  `nopl` and every instruction feature above it: SSE3 to SSE4.2, `sse4a`,
  `popcnt`, `lzcnt`, `bmi*`, `movbe`, `crc32`, `sahf`, `avx`, `avx2`,
  `fma`, `fma4`, `f16c`, `xop`, `aes`, `pclmul`, `vaes`, `vpclmulqdq`,
  `gfni`, `sha`, `sha512`, `sm3`, `sm4`, the `xsave*` and the system and
  state features (`fsgsbase`, `invpcid`, `rdrnd`, `rdseed`, ...), `cx16`
  (X-atomic uses `cmpxchg16b` inline with it, libatomic without; on a CPU
  with the instruction libatomic uses it too, so both halves of a program
  agree on lock-freedom), the AVX-512 and AVX10 families and `evex512`,
  the AVX-VNNI/IFMA/NE-convert ones, AMX (`amx-*`), and APX (`egpr`,
  `ndd`, `ppx`, `push2pop2`, `ccmp`, `cf`, `nf`, `zu`: `r16`..`r31` are
  caller-saved and carry no argument).
- **Tuning** (`tuning`): accepted either way: `fast-*`, `slow-*`,
  `prefer-*`, `false-deps-*`, `tuning-*`, `no-bypass-delay*` and the named
  ones (`macrofusion`, `ermsb`, `idivq-to-divl`, `vzeroupper`,
  `inline-asm-use-gpr32`, ...). `target-cpu` and `tune-cpu` are accepted
  with any value.
- **Refused by name**:

  | item | reason |
  |---|---|
  | `-sse`, `-sse2` | `attr:target-feature-no-sse` |
  | `-x87` | `attr:target-feature-no-x87`, only for a function that has an `x86_fp80` value (§5.6.9); in `-mattr`, always |
  | `+soft-float` | `attr:target-feature-soft-float` |
  | `+x32` | `attr:target-feature-x32` (32-bit pointers) |
  | `+16bit-mode`, `+32bit-mode`, `-64bit`, `-64bit-mode` | `attr:target-feature-mode` |
  | `+tagged-globals` | `attr:target-feature-tagged-globals` |
  | `+retpoline*` | `attr:target-feature-retpoline` |
  | `+harden-sls-*` | `attr:target-feature-harden-sls` |
  | `+lvi-*` | `attr:target-feature-lvi` |
  | `+seses` | `attr:target-feature-seses` |
  | anything else, or an item that is not `+name`/`-name` | `attr:target-feature-unknown` |

  The opposite sign of each is accepted (it is what hod's code keeps).

**The evidence (WP-201).** "Available" means no call boundary can tell:
for every census string (and its CPU alone), every feature enabled alone
on `x86-64` and every feature disabled on `x86-64-v4`,
`sapphirerapids`, `diamondrapids`, `znver5`, `haswell` and `x86-64`,
`llc -O0 -stop-after=finalize-isel` 19 and 22 give 26 probe signatures
(integers, `i128`, `float`, `double`, `x86_fp80`, `half`, `bfloat`,
128-, 256- and 512-bit vectors, vectors of `i1`, structures of them,
stack arguments, `byval`, `fastcc`, `preserve_mostcc`, `preserve_allcc`
and a variadic call) the same argument registers and stack slots, return
registers and call register masks as a function without features, except
(a) a vector wider than 128 bits under anything that implies AVX, and a
vector of `i1` under AVX-512, which `x64:wide-vector-abi` refuses with the
function's features (§5.6.9; `sha512`, `sm3` and `sm4` imply AVX), and
(b) `preserve_allcc`, whose call's register mask keeps all of
`ymm0`..`ymm15` under AVX while §5.6.23's callee keeps their low halves:
`llc`'s own callee returns through `vzeroupper` whenever it called
anything (and saves nothing it does not touch when it is a leaf), so the
upper halves are lost under `llc` as under hod and no program tells the
two apart (`features_census::preserve_all_under_avx_is_llc_s`). AVX-512 is therefore judged per signature,
not per function: a function carrying `+avx512f` compiles unless a
signature it touches has a wide vector or a vector of `i1`; nothing else
differs (`csr_64` is the register mask of every `ccc` call under every
feature; hod never writes a `ymm`/`zmm` upper half, so no `vzeroupper`
is owed; a 64-byte-aligned value's alignment is the IR's, not the
feature's). x86-64 `-march=native` on an AArch64 host makes clang 19 and
22 write AArch64 names (`+neon`, `+lse`, ...) into the x86-64 string;
they stay unknown (kiln writes the baseline).

`-mattr` items are checked the same way, by the module's target.

#### 5.6.3 Control flow (WP-124)

`hod_x64::control`. Observable behaviour is `hod_a64`'s (§6, §7.3); what
x86-64 settles:

- **`condbr` on an `i1`**: `test r8, 1` (only bit 0 is defined), then
  `jne` to the true block, or `je` to the false one when the true block
  is next; the branch to the block emitted next is omitted. A constant
  condition (`undef`, `poison` false) or two equal targets is a `br`.
- **`condbr` on a deferred integer `icmp`** (§4.6, operands of class
  `Gpr`, pointers included): the compare's operands are placed first
  (a constant on the left is swapped to the right with the predicate),
  then `cmp`/`test`, then the `jcc` at once: nothing at all runs between
  the flags' writer and their reader. The successors' `phi` stores are
  written by the driver before the terminator (§4.5), so before the
  compare. Widths 8, 16, 32, 64 compare at their own operand size; an
  odd width is shifted left in place to the top of 32 bits (below 32) or
  64 bits (above), both operands and a constant alike, which keeps
  equality and both orders whatever the bits above the width. A constant
  0 is `test r, r`; another constant is `cmp r, imm` when the immediate
  form takes it (sign-extended 32 bits at 64), else it goes through
  `r11`. Unsigned predicates are `a`/`ae`/`b`/`be`, signed `g`/`ge`/
  `l`/`le`. A deferred `fcmp` is refused as `x64:todo:condbr-fcmp`
  until X-float (WP-127). The fused compare is `control`'s own
  (`fused_icmp`), a candidate to share with `int` (WP-130).
- **`switch`** of class `Gpr`: `hod_cg::switch`'s plan; the condition is
  zero-extended in its register (`movzx` for 8 and 16 bits, `and r32,
  mask` for an odd width below 32, `shl`/`shr` above 32); up to 32 bits
  it is compared with 32-bit `cmp r32, imm32`, above with 64-bit `cmp`,
  the value through `r11` when no sign-extended 32-bit immediate takes
  it; a node is `cmp` · `je` · `ja`. **Class `Mem`** (`i65` and wider):
  for each case not to the default, in the IR's order, each 64-bit limb
  loaded into `r10` (the top limb shifted left past the bits above the
  width, the case's limb with it), compared, `jne` to the next case,
  `je` to the block after the top limb; then the default (as
  `hod_a64`'s `tail::switch_wide`).
- **`indirectbr`**: `jmp r`; on a `blockaddress` constant of the same
  function, a direct `jmp` to the block (omitted when it is next).
- **`blockaddress` in code**: `lea r, [rip + label]`, the `disp32` a
  `rel32` fixup to the block's label (the field ends the instruction).
  `common::materialize_gpr_const` routes every `blockaddress` constant
  there (glue: `phi` stores, returns, arguments). A block of another
  function is `x64:blockaddress`. An unreachable block's label is the
  shared `ud2` trap at the end of the function (§6.1), in code and in
  data.

#### 5.6.4 Memory (WP-123)

`hod_x64::mem`: non-atomic `load`/`store` of `Gpr` and `Fpr` values,
`alloca`, `ptradd`, `llvm.stacksave`/`stackrestore`. `Mem`-class values
are X-aggr's, atomic accesses X-atomic's (`x64:todo:atomic`).

- **Addresses** (`mem::compute`, `mem::At`). A `ptradd` whose only use
  is the address of a load or store is deferred when the driver allows it
  (§4.6) and fused into that access; otherwise it is computed into its
  result with `lea` (or `mov`, a plain base; the frame address, a slot
  plus a constant). Order inside a lowering: every operand is asked for
  first (`use_reg`, `def_reg`); the first variable index is put in `rcx`,
  sign-extended from its width (`movsxd`/`movsx`, or `shl`/`sar` for an
  odd width), scaled by the addressing mode when the scale is 1, 2, 4 or
  8, else by `shl` or `imul`; a second goes through `rdx` and is added
  into `rcx` with `lea`. Constant indices fold into the offset. The base
  is formed last, in `r11`: a symbol's address with the whole constant
  offset as its addend (§7.4); a slot's address; a constant pointer. A
  slot with no variable index and an offset below `2^24` is a frame
  access, no address formed. An offset outside the 32-bit displacement
  (minus 16, the room of a 16-byte access in pieces) is moved into
  `rcx` before `r11` is written. `rax` is left to the access.
- **Flags.** `shl`, `imul` and `and` of a `ptradd`, an odd-width store or
  an `alloca` write the flags; none of them is glue and none runs
  between a compare and its fused reader (a deferred compare is emitted
  by its reader).
- **Integer loads** read exactly the store size: 1, 2, 4 bytes with
  `movzx`/`mov r32` (zero above), 8 with `mov`; 3 is a word, then the
  byte at 2 shifted by 16 and `or`ed; 5, 6, 7 are a dword, then the rest
  (byte, word, or word and byte) shifted by 32 and `or`ed, through `rax`
  and `rdx`. The bits loaded above an odd width are those memory holds,
  zero for a defined program.
- **Integer stores** write exactly the store size and zero the bits above
  the width (§2.2, as `llc`): a width that is a multiple of 8 and a store
  size of 1, 2, 4 or 8 is one `mov`; otherwise the value is copied to
  `rax`, masked (`and`, `mov r32` for 32, `shl`/`shr` for widths above
  32) when the width is odd, and stored in pieces: 3 is a word, `shr eax,
  16`, a byte; 5..7 a dword, `shr rax, 32`, then a byte, a word, or a word
  and a byte. A constant is stored as an immediate (`mov m, imm32`, or
  through `rax` when it does not fit a sign-extended 32 bits).
- **`Fpr` values**: by store size, `half`/`bfloat` with `pinsrw x, m, 0`
  (load) and `pextrw eax, x, 0` + `mov m16` (store), 4 bytes `movss`, 8
  `movsd`, 16 (`fp128`, 128-bit vectors) `movups`; alignment is never
  assumed. A constant `Fpr` value is stored as integer immediates (two
  8-byte stores for 16 bytes), not loaded into a register.
- **`volatile`** is an ordinary access; a volatile load whose result is
  unused is still emitted (once).
- **`alloca`** (dynamic; fixed ones are slots, §5.6): the size is
  `count × element size`, the count zero-extended from its width
  (`count` is unsigned, as `llc`), the product wrapping at 64 bits,
  rounded up to 16; `rsp` moves down by it through `r11` (`lea r11, [rsp
  - size]` for a constant that fits, else `mov`/`sub`), then, for an
  alignment above 16, `and r11, -align`; `mov rsp, r11`; the result is
  the new `rsp`. **An alignment above 16 is supported** (design 0006 §2.9
  planned `x64:stackrealign`; the frame is never realigned, the
  allocation is aligned by itself, as on AArch64). Outgoing arguments
  are allocated per call below it (§5.6), and the epilogue restores `rsp`
  from `rbp`. An alignment that is not a power of two or is above `2^32`,
  or a constant size at or above `2^63`, is `limit:frame-size`.
- **`stacksave`** is `mov r, rsp`; **`stackrestore`** `mov rsp, r`.
- **Refused**: `x64:ptradd-vector`, a `ptradd` whose result is a vector
  of pointers (a `<2 x ptr>` is `Fpr`; X-lanes may take it).

#### 5.6.5 Mem-class values (WP-126)

`src/aggr.rs` and `src/constaddr.rs`: integers wider than 64 bits,
structs and arrays, copies of `Mem` vectors and `x86_fp80` images, the
overflow intrinsics, `Mem` constants and constants with addresses. The
AArch64 rules of "Mem-class values (WP-040)" (§5.5.1) hold as written,
including the slot image (the bits above `N` in a wide integer's top
limb unspecified, an aggregate's image exact), what extends first, a
`store` of exactly the store size and the layouts; what differs is
below.

- **Registers.** A lowering computes in `rax`, `rcx`, `rdx`, `r10`,
  `r11` and `xmm15` and asks the pool for nothing; limbs are loaded,
  operated on and stored back (no load folding). A carry chain is
  `add`/`adc` (`sub`/`sbb`) with only `mov` between its links; a top
  limb that must be extended is extended into `rcx`/`rdx` before the
  chain (the shift pair that extends writes the flags). Extension in a
  register is `movzx`/`movsx`/`movsxd`/`mov r32` for 8, 16 and 32 bits,
  `shl` + `shr`/`sar` otherwise. Copies go 16 bytes at a time through
  `xmm15`, then 8, 4, 2, 1 through `r10`.
- **Arithmetic.** `mul` of two limbs is `mul` and two `imul`s; wider,
  a schoolbook product into the zeroed result through `rdx:rax` with
  the carry in `rcx`. A constant shift is `shld`/`shrd` per limb (an
  amount of the width or more emits nothing). A variable shift copies the
  source into a frame area of `2n` limbs next to the fill (0, or the
  sign), moves the area's address by `min(s >> 6, n - 1)` limbs and
  combines two limbs per result limb with `shld`/`shrd` by `cl` (which
  takes the amount mod 64 and leaves the destination alone for 0). Beyond
  `MAX_QUADRATIC_BITS` (4096) they are `x64:wide-mul` and
  `x64:wide-shift`. Division up to 128 bits calls `__udivti3`,
  `__divti3`, `__umodti3`, `__modti3` (`int_divide_routine`), the
  operands extended into 16-byte slots when narrower; wider is
  `x64:wide-division`.
- **Compares**: `eq`/`ne` or the limbs' differences; the orderings are a
  `cmp`/`sbb` chain over the limbs, both top limbs shifted left by
  `64 - N mod 64` first. The result is a `setcc` (no deferral).
- **Casts.** As AArch64, `xmm15` in place of `v31`. `fptosi`, `fptoui`,
  `sitofp`, `uitofp` between `half`, `float`, `double` or `fp128` and an
  integer of 65 to 128 bits call `__fix[uns]{hf,sf,df,tf}ti` and
  `__float[un]ti{hf,sf,df,tf}` (`fp_to_int_routine`,
  `int_to_fp_routine`), the integer extended to 128 bits in a slot
  first, for every width from 65 to 128 bits, not only `i128` (WP-144).
  Wider integers, and `bfloat` with an integer wider than 64 bits, are
  `x64:wide-fp-cast`; `addrspacecast` is `x64:mem-cast`.
- **Aggregates**: as AArch64. `select` with a non-constant condition
  forms both addresses, `test` and `cmovz`, then copies once. A member
  of `half`, `bfloat`, `float`, `double` or `fp128` moves through
  `xmm15` or its slot as AArch64's does (WP-144); `x86_fp80` members
  wait for WP-131. An atomic `load` or `store` of a `Mem` value is
  §5.6.10's.
- **Overflow intrinsics**, every width up to `MAX_QUADRATIC_BITS / 2`.
  At 64 bits and below they are computed in `r10`/`r11` with the
  operands extended to 64 bits, and the flag comes from the carry, `OF`,
  the bits above the width, or the result not being its own sign
  extension. Wider, `add`/`sub` is a carry chain over the extended top
  limbs, and `mul` an exact product at twice the width. The result
  `{iN, i1}` is written as its image. A vector is
  `x64:overflow-intrinsic`, and `mul` wider than 2048 bits is
  `x64:wide-mul-overflow`.
- **Returned in memory** (`spec/abi-x64.md` §5.2, §8.2.3). The
  prologue saves the buffer's address (`call::enter`,
  `CallState::ret_addr`, WP-125). `ret` (`term::ret` →
  `aggr::ret_memory`) writes the value through it as a `store` would;
  the epilogue (`call::before_return`) reloads it into `rax`.
- **Constants** (`materialize_mem`, glue: `rax`, `r11`, `xmm15` only,
  never `r10`, flags kept). A zero-like constant is immediate stores
  of zero. A wide integer is written word by word, with an 8-byte word
  that is not a sign-extended 32-bit value going through `r11`.
  Anything else is written from its flattened image
  (`ConstPool::flatten`).
- **Addresses in constants** (`constaddr`). Every relocated field of an
  image is computed in code with the symbol access of §7.4, in `rax`,
  and stored with its width (1, 2, 4 or 8 bytes, at any offset). This
  covers `Sym`, `Plt`, `Diff` and `PltDiff`. There is no constant pool
  and no data relocation, so the code is right for preemptible, local
  and weak undefined symbols alike. As a `Gpr` operand, a `SymDiff` is
  `a + addend` plus the complement of `b` plus 1 (`not`, `lea`), and a
  number minus an address (`ConstPool::neg_addr`) is `not` and `lea +1`
  of the address. Both keep the flags. A block address is
  `control::block_address` (§5.6.3). A block difference is
  `x64:constant-relocation`, and so is a vector image with any
  relocation other than an aligned 8-byte symbol address.

#### 5.6.6 Data and module-level constructs (WP-143)

Module emission (§9.7) is shared by both targets; on x86-64 it writes
what `llc -O0 -fast-isel=0 -relocation-model=pic` writes (LLVM 19 and
22), section by section, with these target rules:

- **`.text`** is created 4-aligned, as on AArch64; a function raises it
  to its own alignment (16).
- **Padding between functions** is `X86AsmBackend::writeNopData`'s for
  the default CPU: `nop`s of at most 10 bytes, the longest first (`66 2e
  0f 1f 84 00 00 00 00 00`, then the 1- to 9-byte forms of the SDM's
  table).
- **An address in data** (an initializer, a constructor or destructor
  entry) names the symbol itself, `dso_local` or not: x86-64 uses
  `AsmPrinter::lowerConstant`, which calls `getSymbol`, where AArch64's
  override reaches a `dso_local` definition of a shared object through
  its `.L<name>$local` alias (§9.7, "local aliases"). A local symbol is
  reached through its section symbol on both targets, unless its section
  is mergeable with a non-zero addend or thread-local.
- **Relocations of data.** An address field is `R_X86_64_64`, `_32`,
  `_16` or `_8` by its width; a difference against the field's section
  is `R_X86_64_PC64`, `_PC32`, `_PC16` or `_PC8`. `llc` writes the 32-bit and
  narrower absolute kinds in PIC objects too; a linker refuses them in a
  position-independent output unless the value fits, exactly as for
  `llc`'s object. A number minus an address (`init:neg-addr`) has no
  relocation on x86-64 either: `llc` fails ("Cannot represent a
  difference across sections"), and the row is rejected on both
  targets.
- **Sections, flags and entry sizes** are the target-neutral rules of
  §9.7 unchanged: the mergeable names (`.rodata.str<k>.<n>`,
  `.rodata.cst<n>` for 4, 8, 16 and 32 bytes; a 2- or 64-byte constant
  is plain `.rodata`), `SHF_GNU_RETAIN`, `.tdata`/`.tbss`, groups,
  `.init_array.<p>`/`.fini_array.<p>`, `.linker-options` and `.deplibs`,
  common symbols, `STT_GNU_IFUNC`, private `.L` names, aliases with
  offsets. `x86_fp80` data is 10 bytes in a 16-byte, 16-aligned object
  (an `unnamed_addr` constant of it is `.rodata.cst16`).
- **Thread-local definitions** (`.tdata`, `.tbss`, `STT_TLS`) are
  written as on AArch64; `tls:*` rows describe the symbol and are on.
  An access is §5.6.11's (WP-134).
- **`cf-protection-return`** writes `.note.gnu.property` (`SHT_NOTE`,
  `SHF_ALLOC`, 8-aligned, 32 bytes): `GNU_PROPERTY_X86_FEATURE_1_AND`
  (`0xc0000002`) with the shadow-stack bit (2). The code does not
  change, which requires every x86-64 lowering to return only to the
  address its call pushed (no `push`/`ret` or `call`-to-get-`rip`
  idioms). **`cf-protection-branch`** (WP-184, §5.6.25) writes
  `endbr64` at function entries, address-taken blocks, landing pads and
  after direct calls of `returns_twice` functions, and sets the IBT bit
  (1) in the note. `llc` 19 and 22 read both CET flags by their presence, not their
  value: `i32 0` turns them on as well (`endbr64` and the IBT bit; the
  shadow-stack bit), and an absent flag is off. `hod_ir` reads them the
  same way since WP-144 (before, 0 was off).
- **Module assembly of symbol directives** (§9.7.1) is read with
  x86-64's comment character, `#`.

Evidence: `tests/emit_llc.rs` (every snippet on both targets, and the
x86-64 snippets `dso-local-data`, `pie`, `narrow-fields`,
`cf-protection-return`, `x86-types`), `tests/emit_asm.rs`, and
`tests/emit_link.rs` (the data module with `llc`'s `main`, a data
module whose code is a C driver, and an `available_externally` copy,
linked by bfd, lld and mold and run under `qemu-x86_64`).

#### 5.6.7 Scalar floating point (WP-127)

`hod_x64::float` lowers the instructions of class `Fpr` and
`hod_x64::fpintr` the floating-point intrinsics, in SSE2 and nothing
above it (no `roundss`, no `cvttps2udq`, no AVX, no x87: `x86_fp80` is
`x64:x87`'s, vectors `x64:todo:vector`'s). The decisions:

- **`float` and `double`** are the scalar SSE2 instructions, two-operand,
  the result in the left operand's register; no load is folded. `frem`
  calls `fmodf`/`fmod`. No instruction reads or writes `MXCSR`: a NaN an
  operation computes is the hardware's (the first NaN operand quieted, or
  the negative default NaN), which LangRef allows.
- **`half`** is computed in `float` through the shared runtime routines,
  as `llc` without F16C: `__extendhfsf2` in, the operation,
  `__truncsfhf2` out (`__truncdfhf2` from `double`). It travels in the
  low 16 bits of an `xmm` register; the bits above are unspecified.
- **`bfloat`** has no routine in the table: it is widened by a 16-bit
  shift (exact) and narrowed inline to nearest even, a NaN kept a NaN
  with its quiet bit set (what `__truncsfbf2` returns). A `double` or an
  integer of more than 24 bits is first rounded to odd in `float`, so
  that every conversion rounds once (`llc` rounds an integer twice:
  `uitofp i33 0x1feffffff` is `0x5000` there, `0x4fff` here). Conversions
  between `bfloat` and `fp128` are `x64:bfloat-fp128`; every intrinsic of
  `bfloat` is `x64:intrinsic-bfloat`.
- **`fp128`** is calls throughout: `__addtf3` .. `__divtf3`,
  `fmodf128`, the `__*tf2` compares, the `__*tf*` conversions and the
  `...f128` functions of the intrinsics; its constants come from a data
  island (`movups`).
- **`fcmp`** is `ucomiss`/`ucomisd`, the operands swapped where that
  makes the condition one of `A`/`AE`/`B`/`BE` (unordered sets `ZF`,
  `PF` and `CF`): `oeq` is `E` and `NP`, `une` is `NE` or `P`, `ueq`/`one`
  are `E`/`NE`, `ord`/`uno` `NP`/`P`, `true`/`false` `E`/`NE` after `cmp
  eax, eax`. A compare whose only use is the next `select` or `br` is
  deferred and fused (§4.6): the branches read the flags directly (two
  `jcc` for `oeq`/`une`); otherwise `setcc` (and `and`/`or` of two) and
  `movzx`. `fp128` compares call `__eqtf2` .. `__unordtf2` and test
  `eax`; `ueq`/`one` call `__eqtf2` and `__unordtf2`.
- **Conversions.** `fptosi` is `cvttss2si`/`cvttsd2si`, the 32-bit form
  up to 32 bits (31 unsigned), the 64-bit form above; `fptoui` to 64
  bits is `llc`'s sequence (the value less `2^63` converted too, or-ed in
  where the first conversion gave the integer indefinite). Out-of-range
  results are poison: the indefinite is as good as any value.
  `sitofp`/`uitofp` extend the source to 32 or 64 bits; `uitofp` of 64
  bits halves the value with its low bit sticky when the sign bit is
  set, converts and doubles. `bitcast` with a `Gpr` side is
  `movd`/`movq` of the 32- or 64-bit view.
- **`select`, `phi`, `freeze`** of class `Fpr` are moves (`movaps`):
  bits are kept, signalling NaNs included. `freeze` of `poison` or
  `undef` is zero.
- **Intrinsics.** `sqrt` is `sqrtss`/`sqrtsd`; the rounding family,
  `fma`, `lround`/`llround` call libm (`floorf`, `fmaf`, `lroundf`, ...),
  `lrint`/`llrint` are `cvtss2si`; `fmuladd` is `mulss` + `addss`
  (unfused, as `llc -O0`); `canonicalize` is `x * 1.0`; `fabs` and
  `copysign` are bit operations in `rax`/`rcx`. `minnum`/`maxnum` and
  `minimum`/`maximum`/`minimumnum`/`maximumnum` are `llc`'s sequences
  (`minss` with a `cmpunordss` mask; the operands ordered by the first
  one's sign so that `-0 < +0`, then a NaN test). `fptosi.sat`/
  `fptoui.sat` truncate at 64 bits and `cmov` the bounds after `ucomis`
  against them, and 0 for a NaN. `is.fpclass` (a constant mask) counts
  the magnitude against the class bounds and tests a bit of a table
  chosen by the sign. `half` is computed in `float` (`fma` in `double`:
  `mulsd`, exact for two `half`s, then `addsd`, rounding as `llc`'s call
  of `fma` does), `fp128` calls the `...f128` functions; `powi`,
  `ldexp` (an `i64` exponent clamped to `int`), `frexp` and the libm
  intrinsics are calls. Refused: `x64:fp-intrinsic` (a non-constant
  `is.fpclass` mask, a `powi` exponent wider than 32 bits),
  `x64:intrinsic-fp128` (`fptosi.sat`/`fptoui.sat` of `fp128`),
  `x64:intrinsic-wide` (an integer result wider than 64 bits).

A lowering here writes, besides its operands' registers, `rax`, `rcx`,
`rdx`, `r11`, `xmm14` and `xmm15`; the glue that materializes an `Fpr`
constant writes `r11` only.

Evidence: `crates/hod_x64/tests/float.rs` and `tests/fpintr.rs`
(`common/float_gen.rs`): every operation, predicate, conversion and
intrinsic on edge values (signed zeros, subnormals, infinities, NaNs with
payloads, the conversion boundaries `±2^31`, `2^32`, `±2^63`, `2^64`
and their neighbours) and random ones, compiled plainly and with every
stress option, run under `qemu-x86_64` and compared with `hod_interp`
(`float`, `double`; NaN for NaN, the host being AArch64) and bit for bit
with `llc` 22 and 19. Where `llc` is not right or not able, a model in
the test is the oracle: an integer of more than 24 bits to `bfloat`
(`llc` rounds twice), `ldexp` with an `i64` exponent outside `int`
(`llc` truncates it), `minimum`/`maximum` of `fp128` (`llc` cannot
select them). Where LangRef leaves the NaN open and `llc` picks another,
`llc` is not compared: two `fp128` NaNs into `minimumnum`/`maximumnum`,
a signalling `bfloat` NaN through a `phi` (`llc` 22 quiets it).

#### 5.6.8 Intrinsics (WP-128)

`hod_x64::intrinsic` lowers the integer, memory, trap and frame
intrinsics; `hod_x64::barrier` lowers inline assembly whose template is
empty. The scratch registers are `rax`, `rcx`, `rdx`, `r10`, `r11`; no
sequence puts an instruction other than `mov`, `lea`, `movzx`/`movsx`,
`push` or `pop` between a flag setter and its reader.

- **Bit counts.** `ctlz`/`cttz` use `bsr`/`bsf`, with a `cmove` for a
  zero operand; `ctpop` is the SWAR sum; `bitreverse` is `bswap`, then
  nibbles, pairs and bits swapped by masks. i128 forms work on the
  limbs.
- **`abs`** is `neg` + `cmovs`; **`smin`/`smax`/`umin`/`umax`** are
  `cmp` + `cmov`; **`scmp`/`ucmp`** are `setg`/`seta` and
  `setl`/`setb` minus each other.
- **Funnel shifts** use `shld`/`shrd` (by `cl`) at 32 and 64 bits.
  Below that, `k = amount mod width` (`and`, or `div` for a width that
  is not a power of two) and the two operands are shifted by `k` and
  `width - k` and `or`ed. i128 picks limbs by `cmov` on bit 6 of `k`.
- **Saturating `add`/`sub`** at the register width use `cmovo`
  (signed) or `cmovb`/`cmovae` (unsigned) against the bound; below the
  register width the operation is done at 64 bits and clamped.
  **`sshl.sat`/`ushl.sat`** move the operand to the top of the register,
  shift it, shift it back and compare; on a difference the result is the
  saturated value.
- **Memory.** `memcpy` and `memset` of a constant length up to
  `INLINE_MAX`, and `memmove` up to `INLINE_MOVE_MAX`, are inline: 8-, 4-, 2- and 1-byte moves, or 16-byte
  `movups` with an overlapping last move; a `memmove` loads every piece
  before storing any. `memcpy.inline`/`memset.inline` longer than that
  are a loop of 16-byte steps and a tail. Every other length calls
  `memcpy`, `memmove` or `memset`. A variable length of `i32` is
  zero-extended first.
- **Traps.** `trap` is `ud2`; `ubsantrap(kind)` is `ud1 eax, [eax +
  kind]` with 32-bit addressing (`67 0F B9`, then `00`, `40 kk` or `80
  kk 00 00 00`), byte for byte `llc`'s (WP-144; `ud2` before, the same
  `SIGILL`). `debugtrap` is `int3`.
- **Frames.** `frameaddress(n)` follows `rbp` n times; `returnaddress(n)`
  reads `[frame + 8]`. Depths above 256 are refused
  (`x64:frame-depth`).
- **The rest.** `readcyclecounter` is `rdtsc`, then `shl rdx, 32; or
  rax, rdx`. `prefetch` and `clear_cache` emit nothing (x86 caches are
  coherent). `ptrmask` is `and`. `eh.typeid.for` is the type's index
  (`FnCx::type_id`; `x64:eh-typeid` without one). `is.constant` and
  `objectsize` fold to their constants as `llc -O0` folds them, and the
  operands of a folded call are not computed (`intrinsic::begin` and
  `intrinsic::dropped`, called by the target's `begin_function` and
  `lower`).
- **Inline assembly.** An empty template with `r`, `m`, `i`, `n`, `x`,
  `g`, `X`, a `{reg}` code, a tied output, and the classes `q`, `Q`,
  `R` is a barrier. The alternative chosen is `llc`'s, by LLVM's x86
  constraint weights. `llc` does not read clang's `a`, `b`, `c`, `d`,
  `S` or `D` (a front end writes `{ax}`), so they are refused. `A`,
  which front ends leave untranslated (`rdtsc` with `"=A"`), `llc` 19
  and 22 do read on x86-64: `rax` for a value of up to 64 bits (an
  `i64` `=A` after `rdtsc` is `rax` alone, without `edx`), `rdx:rax`
  for `i128`, as an input, an output or a tied pair; hod keeps refusing
  `A`, as `x64:inline-asm-constraint` in an empty template and as
  `asm:constraint/a` (names are lower-cased) in one of instructions (WP-144, WP-187,
  `tests/leftovers.rs`).

Refused names: `x64:intrinsic` (an intrinsic not listed), and also
`x64:intrinsic-wide` (a width above 128, or i128 where only `Int` is
on), `x64:intrinsic-vector`, `x64:mem-inline-length` (an `.inline` form
with a variable length), `x64:frame-depth`, `x64:eh-typeid`,
`x64:inline-asm` (a `callbr` whose template is not empty; other
templates are §5.6.28's since WP-187), and
`x64:inline-asm-{output,outputs,constraint,clobber,operand,bundle}`.

Evidence: `crates/hod_x64/tests/intrinsic.rs` and `tests/barrier.rs`,
summarized in `spec/landed/codegen/WP-128.md`.

#### 5.6.9 ABI follow-ups (WP-140)

- **The function's features.** `X64` carries `FnFeatures { abi:
  Features { avx, avx512 }, no_x87 }`. The constant `hod_x64::X64` has
  none of them (the module-level value); `X64::for_function` (§8.2)
  returns one with the features `features::of_function` reads from the
  function's `"target-cpu"` and `"target-features"`, and the driver
  compiles the whole function with it. Every signature the function
  touches (its own, each call's, each `invoke`'s) is assigned with the
  compiled function's features, never the callee's: `llc` does the same
  (`spec/abi-x64.md` §1.3, which has the CPU tables and the rule). So
  without AVX a `<8 x float>` argument is two `xmm` registers and a
  `<4 x i1>` one `xmm`, as `llc` places them; with AVX (or a CPU that
  may have it) a wider-than-128-bit vector is `x64:wide-vector-abi`, and
  with AVX-512 a vector of `i1`.
- **`-x87`** is refused (`attr:target-feature-no-x87`) only for a
  function that has a value of a type containing `x86_fp80` (its
  signature, an instruction's result or an operand): `llc` cannot
  select x87 arithmetic without the x87 and stops with an error, so hod
  refuses where `llc` does not compile. A function without one compiles
  to the same code with and without `-x87`. In `-mattr` (which reaches
  no function), `-x87` stays refused.
- **x87 places.** `abi::assign` places a `float`/`double` return piece
  that finds no free `xmm` register in `st0`/`st1` (`spec/abi-x64.md`
  §5.1); the size of the part (4 or 8) says how it is loaded.
  `X64::assign` refuses a signature with such a part as
  `x64:ret-x87-part`; `call::guard` keeps refusing an `x86_fp80` part as
  `x64:todo:abi` until WP-131.
- **One walk, one register file.** `hod_x64::abi` classifies through
  `hod_cg::abi`'s `Classify` and places with `place_leaves`/`place_leaf`
  (as `hod_a64::abi` does); its registers are `hod_x64::reg`'s (`st0`,
  `st1` at 32 and 33, DWARF 33 and 34).
- **`fastcc`** is `ccc` (`spec/abi-x64.md` §1.2): its `Assignment`
  differs only by the third `xmm` return register, and the code
  generator compiles it as any other `Assignment`.
- **`sret`, `byval`, `uwtable`, `insertvalue`/`extractvalue`** of
  integer and pointer members (floating-point members with WP-144,
  §5.6.14) needed no new lowering; they run against
  `hod_interp` and `llc` in both mixings (`tests/abi_followups.rs`), and
  the support matrix's x86-64 column says so. The seeds `byval_sret.ll`
  and `aggregate.ll` now stop at `type:double` (WP-127).

Evidence: `spec/landed/codegen/WP-140.md`.

#### 5.6.10 Atomics (WP-133)

`crates/hod_x64/src/atomic.rs` lowers atomic `load` and `store`,
`atomicrmw`, `cmpxchg` and `fence`, dispatched from `target.rs` (and
from `mem.rs` for an atomic access) before the class-based arms. The
sequences are `llc -O0`'s where that is correct by itself; the
differences are listed below. x86-64 is TSO, so no ordering but
`seq_cst` needs an instruction of its own.

- **Orderings.** Every aligned load of 1..8 bytes is an acquire load and
  every aligned store a release store; every `lock`ed instruction and
  `xchg` with memory is a full barrier. So `unordered`, `monotonic`,
  `acquire`, `release`, `acq_rel` and `seq_cst` select the same
  instruction for every operation except a store: a `seq_cst` store is
  `xchg` with memory (the old value discarded), what `llc` writes, which
  keeps it ordered before a later load; the others are `mov`. A
  `cmpxchg`'s orderings change nothing. Weak `cmpxchg` is strong.
- **Loads and stores** of 1..8 bytes are `mov` (`movzx` into a 32-bit
  register for 1 and 2 bytes); `half`, `bfloat`, `float`, `double` go
  through `rax` (`movd`/`movq`, `pextrw`/`pinsrw` for 2 bytes).
- **`atomicrmw`** of 1..8 bytes (integers and pointers): `xchg` is
  `xchg` (implicitly locked); `add` is `lock xadd`, `sub` `neg` then
  `lock xadd`; when the result is unused (`FnCx::is_dead`), `add`, `sub`,
  `and`, `or`, `xor` are `lock add`/`sub`/`and`/`or`/`xor` with memory
  from a register (a constant operand is materialised first). Every
  other operation is a `lock cmpxchg` loop: the old value in `rax`
  (loaded once before the loop, then refreshed by the failing
  `cmpxchg`), the new one computed into `r10` with `rcx` as a second
  temporary: `nand` is `and` then `not`; `max`, `min`, `umax`, `umin`
  `cmp` at the operation's width then `cmovl`/`cmovg`/`cmovb`/`cmova`
  (32-bit `cmov` for 1 and 2 bytes: the compared bits are the width's);
  `uinc_wrap` is `old >= v ? 0 : old + 1`, `udec_wrap` `old == 0 ||
  old > v ? v : old - 1`, `usub_cond` `old >= v ? old - v : old`,
  `usub_sat` `old >= v ? old - v : 0`, each by `cmov` after the
  comparison (§5.6.1's flags rule: only `mov`, `lea`, `movzx` between
  a flag setter and its reader, `mov r32, 0` for a zero). Every operand
  is in a register or is an immediate before the loop's label; the loop
  body touches memory only through its `lock cmpxchg`, under any
  `Stress`.
- **Floating `atomicrmw`.** `xchg` of `half`, `bfloat`, `float`,
  `double` exchanges the bits through `rax`. `fadd` and `fsub` of
  `float` and `double` are a `lock cmpxchg` loop computing in `xmm15`
  (`addss`/`addsd`, `subss`/`subsd`) from the old bits in `rax`.
- **16 bytes** (`i128`, 16-aligned) need `cmpxchg16b`: with `+cx16` in
  the function's `target-features` (the last `±cx16` item decides, as
  `llc` reads it), every operation is `lock cmpxchg16b`: a load is a
  compare of `rdx:rax` = `rcx:rbx` = 0 (which writes back what it found
  if it was zero, and fails otherwise; the old value is `rdx:rax` either
  way), a store and every `atomicrmw` a loop on the new value in
  `rcx:rbx`, computed pairwise (`add`/`adc`, `sub`/`sbb`, `cmp` and `sbb`
  for the comparisons, then `cmov`s). `rbx` is a callee-saved register
  taken for the operation (`FnCx::take_reg`). Without `+cx16`, hod
  calls libatomic's `__atomic_load_16` and its siblings as `llc` does
  (WP-144, §5.6.14).
  A load through `cmpxchg16b` writes: it faults on read-only memory
  exactly as `llc`'s does.
- **`cmpxchg` result.** `{ T, i1 }` is a `Mem` value: the old value
  (`rax`, or `rdx:rax`) at offset 0 and `sete`'s byte at the flag
  field's offset; `extractvalue` reads either field (`aggr.rs`). The
  expected value goes in `rax` (`rdx:rax`), the replacement in a
  register (`rcx:rbx`).
- **`fence`**: `mfence` for `seq_cst` with the system scope; nothing for
  `acquire`, `release`, `acq_rel` (TSO already orders them; hod does not
  reorder memory accesses) and nothing for `syncscope("singlethread")`,
  as on AArch64.
- **Volatile** changes nothing: an atomic volatile access is the same
  instruction, and a plain volatile access is not atomic (`mem.rs`).
- **Refused, by name:** `x64:atomic-unaligned`, an access whose
  alignment is below its size (`llc` calls `__atomic_*`);
  `x64:atomic-type`, an atomic of `fp128`, `x86_fp80`, a vector, an
  integer that is not 8, 16, 32, 64 or 128 bits, or an operation whose
  type does not fit it; `x64:atomic-bfloat-op`, `bfloat` arithmetic;
  `x64:atomic-float-op` and `x64:atomic-libatomic` are no longer
  returned (WP-144: `fmax`, `fmin`, `fmaximum`, `fminimum`, `half`
  `fadd`/`fsub`, and 16 bytes without `+cx16`).
- **Differences from `llc -O0`.** hod never uses an immediate form
  (`lock add [m], imm`): the operand is in a register first. `llc`
  keeps the values of a `lock cmpxchg` loop in stack slots between
  iterations; hod keeps them in `rax` and `r10`. None changes what
  another thread can observe.

**What the emulator proves.** The threaded programs run under
`qemu-x86_64` user mode, which runs each guest thread on a host thread:
on a multi-core host the counters, spinlocks and the stack are really
contended, and a read-modify-write without `lock` loses updates (WP-133's
mutation check: the threaded counters catch it, the single-threaded
table does not). qemu translates a `lock`ed instruction to a host atomic
operation and, for an x86 guest on a weakly ordered host, adds host
barriers for the orderings TSO guarantees (all but store-then-load), so
the message-passing programs pass whatever ordering the guest code asks
for: they show that the sequences are atomic, not that hod's choice of
instructions gives TSO's guarantees. A `seq_cst` store written as a
plain `mov` is not caught by any emulated run here (only a
store-buffering litmus could see it, and only on a host that reorders);
the instruction forms test pins `xchg` for that store and `mfence` for
the `seq_cst` fence.

Tests: `crates/hod_x64/tests/atomic.rs` (the table, against the
interpreter and `llc` 22 and 19; 16 bytes with `+cx16` in
`tests/common/atomic_wide.rs`; the threaded programs of
`tests/common/atomic_threads.rs`; the instruction forms; the
refusals), and `tests/ir/atomics.ll` under `cargo xtask irdiff --target
x86_64`.

#### 5.6.11 Thread-local storage (WP-134)

The model is `hod_cg::sym_access`'s (`TargetMachine::getTLSModel`, as
on AArch64): a shared object (PIC without `PIE Level`) gets general
dynamic for a preemptible variable and local dynamic for a `dso_local`
one; an executable, PIE or not, initial exec and local exec; a model
the variable asks for wins when it is more specific. x86-64 lowers
local dynamic as itself (`llc` uses no TLS descriptors here). Every
sequence is `llc -relocation-model=pic`'s, with its relocations and
without an addend in them; an offset from the variable is added in code
afterwards (`common::add_addend`), as on AArch64:

| model | sequence | relocations |
|---|---|---|
| general dynamic | `data16 lea rdi, [rip + x@tlsgd]` · `data16 data16 rex64 call __tls_get_addr@PLT`: exactly `66 48 8D 3D` · `66 66 48 E8` and their fields, 16 bytes; `rax` = `&x` | `TLSGD`, `PLT32` |
| local dynamic | `lea rdi, [rip + x@tlsld]` · `call __tls_get_addr@PLT` (12 bytes) · `lea rd, [rax + x@dtpoff]` (`disp32`) | `TLSLD`, `PLT32`, `DTPOFF32` |
| initial exec | `mov rd, [rip + x@gottpoff]` · `push t` · `mov t, fs:[0]` · `lea rd, [rd + t]` · `pop t` (`t` is `r11`, or `r10` when `rd` is `r11`) | `GOTTPOFF` |
| local exec | `mov rd, fs:[0]` · `lea rd, [rd + x@tpoff]` (`disp32`) | `TPOFF32` |

`llvm.thread.pointer` is `mov rd, fs:[0]`. A dso-local variable with
external linkage in a shared object is reached through its local alias
(§9.7), as `llc` does.

- **Where `llc` differs.** Initial exec puts `GOTTPOFF` on the `mov`
  and adds the thread pointer with `lea` under a saved `t`, where `llc`
  writes `mov rd, fs:[0]` · `add rd, [rip + x@gottpoff]` or folds
  `fs:` into the access: `add` writes the flags, which an operand may
  not (below). Both forms are the ones the linkers relax. `llc` folds
  `fs:[x@tpoff]` and `[rax + x@dtpoff]` into loads and stores; hod
  forms the address, then accesses it (no load folding, §5.6).
- **`llvm.threadlocal.address` of a dynamic model is a call** (§4.4):
  every register value is displaced, then the bare pair is emitted and
  `rax` (or the `DTPOFF32` `lea` from it) is moved to the result. The
  call is recorded as a call site (§7.1). The static models are
  lowered as an operand.
- **A thread-local address is an operand** like any other symbol
  address (§3; the AArch64 decision of WP-073): `common::sym_address`
  hands it to `tls::address`, which may run at any point of a lowering
  (argument placement, between a fused compare and its `cmovcc`) and
  writes `rd` and nothing else, the flags kept. For the dynamic models
  that means saving what `__tls_get_addr` clobbers below `rsp`, in a
  336-byte area (so `rsp` stays 16-aligned at the call; the CFA is
  `rbp`-based, so no CFI row changes): the flags first by `pushfq`
  (`rflags` in the area's last 8 bytes, then `lea rsp, [rsp - 328]`),
  the nine caller-saved general registers but `rd`, and `xmm0`..`xmm15`
  (`movdqu`). After the call the registers are reloaded, `lea rsp,
  [rsp + 328]`, and `popfq` restores every flag (WP-144, §5.6.14; the
  five `setcc` bytes and their rebuild by `test` and `rol` before it).
  The x87 stack must be empty there, as at any call (WP-131).
- **Refused**: as on AArch64, `tls:direct-operand` (a thread-local
  callee) and `tls:address-in-constant` by `hod_ir`'s classification;
  a code model other than small is `x64:code-model`. `x64:todo:tls` is
  no longer returned. The DWARF location of a thread-local variable
  (`DW_OP_const8u x@dtpoff; DW_OP_GNU_push_tls_address`,
  `R_X86_64_DTPOFF64`) stays refused by `hod_dwarf`
  (`debug:tls-location` on x86-64) for WP-136.

Evidence: `hod_x64/tests/tls.rs` (WP-134 record).

#### 5.6.12 x87 (WP-131)

`hod_x64::x87` lowers `x86_fp80` (`long double`). A value is class
`Mem`: a 16-byte slot of which the low 10 bytes are the value (the
significand with its explicit integer bit, then the sign and the 15-bit
exponent); the bytes above are never read. The x87 register stack is
the computing device only: **it is empty between IR instructions**, and
so at every call, branch and return glue. An instruction pushes its
operands from their slots (`fld tbyte`), computes, and pops its result
to its slot (`fstp tbyte`). The x87 control word is the platform's
(round to nearest, 64-bit precision, exceptions masked) and is changed
only around a truncating store (below).

- **Arithmetic.** `fadd` .. `fdiv` push both operands and use the
  popping form (`faddp`, `fsubp`, `fmulp`, `fdivp` with `st(1)`); `frem`
  calls `fmodl`; `fneg` copies the 10 bytes and flips bit 7 of byte 9.
  The NaN an operation returns is the x87's: of two NaNs the one of the
  larger significand, quieted; an invalid operation (an unnormal, a
  pseudo-infinity or a pseudo-NaN as an operand, `inf - inf`, ...) the
  negative default NaN (`0xffff_c000_0000_0000_0000`).
- **`fcmp`** pushes `y` then `x`, `fucomip st(1)` (`ZF`, `PF`, `CF` as
  `ucomisd` sets them), then `fstp st(0)`, which keeps `EFLAGS`; the
  predicates map to conditions as in §5.6.7. Between the flag setter and
  its readers the flags rule (§5.6 "Flags") is kept: only `fstp st(0)`,
  `fld`, `fxch` and the moves of §5.6 run. A compare whose only use is
  the next `select` or `br` is fused: the branches read the flags (two
  `jcc` for `oeq`/`une`); `select` of `x86_fp80` picks the address of
  one arm with `cmov` (two for `oeq`/`une`) and copies 10 bytes.
  Otherwise `setcc` (and `and`/`or` of two) and `movzx`.
- **Conversions.** `fpext` from `float`/`double` is `fld dword`/`qword`
  of the operand stored to the result's slot; from `bfloat` the bits
  shifted into a `float` first; `fptrunc` to `float`/`double` is `fstp`
  to the 16 bytes below `rsp` (no red zone is assumed: `lea rsp, [rsp -
  16]` first and `lea rsp, [rsp + 16]` after) and a load into the `xmm`
  register. `half` and `fp128` call `__extendhfxf2`, `__truncxfhf2`,
  `__extendxftf2`, `__trunctfxf2`. `fptrunc` to `bfloat` calls
  `__truncxfbf2` (libgcc 13 and later, compiler-rt; as `llc` 22 does,
  `llc` 19 cannot select it; WP-147). `fptosi`/`fptoui` to
  at most 64 bits are `fistp qword` under a control word with the
  rounding set to truncation (`fnstcw`, `or` of `0x0c00`, `fldcw`, the
  store, `fldcw` of the saved word), all in the 16 bytes below `rsp`;
  `fptoui` to 64 bits compares with `2^63` (`fucomi`), subtracts it
  where the value is not below, and flips bit 63 of the result there.
  Wider results call `__fixxfti`/`__fixunsxfti`. `sitofp`/`uitofp`
  extend the source to 64 bits in the result's slot and `fild qword`
  it; `uitofp` of 64 bits adds `2^64` (a `float` constant) where the
  sign bit is set (exact: the significand has 64 bits). Wider sources
  call `__floattixf`/`__floatuntixf`; integers wider than 128 bits are
  `x64:wide-fp-cast`. `bitcast` to and from `i80` is a copy of 10
  bytes. An opcode that is no conversion of `x86_fp80` (invalid IR) is
  `x64:x87-cast`.
- **`load`, `store`, `select`, `phi`, `freeze`, constants** move the 10
  bytes, as any `Mem` value (§5.6.5): bits are kept, signalling NaNs and
  invalid encodings included. A constant is materialized from its 10
  bytes.
- **Calls.** An `x86_fp80` argument is passed in memory (16 bytes,
  16-aligned, in order with the other memory arguments); a result
  comes back in `st(0)`, and the second `x86_fp80` of a `{x86_fp80,
  x86_fp80}` in `st(1)`; in `{x86_fp80, double}` the `double` is in
  `xmm0`. The glue that places such a result pushes the parts
  (`fld`, `fxch` for `st(1)`) and the glue that takes one pops them in
  order. `call::guard` no longer refuses the x87.
- **Intrinsics.** Inline, as `llc`: `fabs`, `copysign` (byte 9),
  `arithmetic.fence` (a copy), `sqrt` (`fsqrt`), `canonicalize` (`fld1`,
  `fmulp`), `fmuladd` (`fmulp`, `faddp`: unfused, as `llc -O0`),
  `lrint`/`llrint` (`fistp` of the result's width under the current
  rounding), `fptosi.sat`/`fptoui.sat` (0 for a NaN, the bounds by
  `fucomip` against `float` constants, else the truncating store), and
  `is.fpclass` of a constant mask (the class's index from the image,
  then `bt` of the mask). The rest call the C library's `l` functions
  through `x87::libcall`, which displaces every register value, copies
  each operand to a 16-byte slot of an outgoing area made by `lea rsp`
  and pops the result from `st(0)`: the rounding family (`floorl`, ...,
  `rintl`), `fmal`, `fminl`/`fmaxl`, `fminimuml`/`fmaximuml` (`llc`
  cannot select `minimum`/`maximum` of `x86_fp80`),
  `fminimum_numl`/`fmaximum_numl`, `powl`, the transcendental
  functions, `lroundl`/`llroundl` (the result in `rax`), `ldexpl` (the
  `i32` in `edi`), `frexpl` (the exponent's address, in the area, in
  `rdi`); `powi` calls `__powixf2`. Refused: `x64:x87-intrinsic` (a
  non-constant `is.fpclass` mask, a `powi`/`ldexp` exponent other than
  `i32`, an integer result wider than 64 bits, an intrinsic with no
  routine).
- **`is.fpclass` of the encodings only the x87 has.** A pseudo-denormal
  (exponent 0, integer bit set) is a subnormal of its sign for every
  mask, as `fxam` classifies it (denormal: C3 C2 C0 = 1 1 0) and as the
  x87 loads it. `llc` is not followed: 19 and 22 put it in no class
  (false for each single class) yet answer true for some unions by
  their expansion, differing on four (19: `0x7`, `0xf3`, `0x203`; 22:
  `0x30c`), which no class gives (WP-147). An
  unnormal, a pseudo-zero, a pseudo-infinity or a pseudo-NaN is in the
  test when the test has both NaN classes, or is all classes but one NaN
  class (`x87::invalid_in`): `llc` 19 and 22 agree on all 1024 masks.

A lowering here writes, besides its operands' slots, `rax`, `rcx`,
`rdx`, `r10`, `r11` and the flags (the libcalls: every caller-saved
register), and the 16 bytes below `rsp` only between its own `lea`s.

Evidence: `hod_x64/tests/x87.rs` (WP-131 record).

#### 5.6.13 Vectors (WP-132)

Every fixed vector the shapes of §5.5.1 allow is lowered on x86-64 with
SSE2 only (no `pshufb`, `pmulld`, blends, `round*`, and of `pextr`/`pinsr`
only `pextrw`/`pinsrw`). The model is AArch64's: a vector is a value of
class `Fpr` (64 or 128 bits, a power of two of lanes, in an `xmm`), a
mask (`<N x i1>`, `N ≤ 64`, class `Gpr`, bit `i` = element `i`), or
`Mem` (everything else, its bytes in a frame slot, §5.6.5).

- **Shapes and refusals** (`vector.rs`). A lane is 1, 2, 4 or 8
  addressable bytes, at most 256 lanes. Every other shape is refused by
  the generic name AArch64 uses: `x64:mask-wide` (an operation on the
  lanes of more than 64 `i1`), `x64:vector-odd-element` (`i4`, `i24`),
  `x64:vector-wide-element` (`i128`, `fp128`, `x86_fp80`; lane
  moves of `i128` and `x86_fp80` since §5.6.18),
  `x64:vector-length` (more than 256 lanes), `x64:vector-index` (an
  index wider than 64 bits), `x64:ptradd-vector` (a `ptradd` of a
  vector of pointers, `mem.rs`'s); a conversion to
  `bfloat` lanes from 64-bit integers is `x64:vector-bfloat`.
- **Images.** A lane is reached in the vector's image: a `Mem` vector's
  own slot, or a 16-byte frame slot an `Fpr` operand is stored into
  (`movups`). Each image is a fresh slot (the target keeps no
  per-function vector state); after the store the operand's register
  goes back to the pool, so a three-operand lowering fits the minimal
  pool (4 GPRs, 2 vector registers).
- **Element access** (`vector.rs`). `extractelement`, `insertelement`,
  `shufflevector` through the images, index rules of §5.5.1 (a constant
  index `≥ N` or undefined is poison; a variable one is masked with
  `N - 1` for a power-of-two `N`, else clamped to lane 0). A constant
  extract from an `Fpr` vector needs no memory: `pshufd` and
  `movd`/`movq` for 4- and 8-byte lanes, `pextrw` for 1- and 2-byte
  ones.
- **Element-wise operations** (`lanes.rs`): one packed instruction on an
  `Fpr` vector where SSE2 has it — `padd*`/`psub*`, `pand`/`por`/`pxor`,
  `pmullw`, `addps`..`divpd`, `fneg` as `xorps` with the lane sign
  bits, and a shift by a constant splat below the lane width as
  `psll*`/`psrl*`/`psra*` (16-, 32-, 64-bit lanes; no `psraq`) — else
  the lane loop of §7.3 (`hod_cg::lanes`) over the scalar kernels, so a
  lane is exactly the scalar instruction's (a lane of `udiv` by zero
  traps). The integer and floating-point intrinsics AArch64 lowers on
  vectors are lowered the same way; the rounding functions and `fma`
  call the library per lane (`roundf`, `fmaf`, ...), `half` lanes
  through `__extendhfsf2` and back. A floating-point reduction of `half`
  lanes rounds to `half` after each step, as `llc` promotes it.
- **Masks** (`mask.rs`). The bit rules of `hod_a64::mask` (the module
  doc of `mask.rs` has the table). `icmp` of lanes compares each pair
  sign- or zero-extended to 64 bits; `fcmp` finds each lane's class
  (unordered, less, equal, greater, one-hot 8/4/1/2) and tests it
  against the predicate's code: `ucomiss`/`ucomisd` for `float` and
  `double`, integer keys for `half` and `bfloat` (NaN when the magnitude
  is above the infinity's, the key the magnitude negated for a set
  sign, so `-0 == +0`). `select` on a mask, the casts between masks and
  lanes, element access and shuffles of masks, stores (the bits above
  `N` cleared up to the byte), and the reductions of masks
  (`or`/`umax`/`smin` = any, `and`/`umin`/`smax`/`mul` = all,
  `xor`/`add` = parity). An intrinsic of `i1` lanes other than a
  reduction is `x64:vector-mask`.
- **Calls** (`vcall.rs`, `spec/abi-x64.md` §4.3). A vector of class
  `Fpr` or `Mem` travels as `abi::assign` places it (one or two `xmm`,
  or memory). A mask of 2, 4, 8 or 16 elements travels as one `Lanes`
  part: the lanes of one `xmm` (`16/N` bytes each, element `i` in lane
  `i`). The producer writes each lane as 0 or 1 into a 16-byte slot
  (the part's stack slot, else one of the function's own) and loads it;
  the consumer stores the register to a slot and gathers bit 0 of each
  lane (an `llc` producer writes all ones). Other mask lengths are
  `x64:mask-vector`; with AVX-512 a mask is `x64:wide-vector-abi`
  (§1.3 of `spec/abi-x64.md`, WP-140). Complex `float` is
  `<2 x float>`, one `xmm`.
- **Where `llc` differs.** `llc` computes most operations with packed
  instructions after legalizing (widening odd lengths, promoting masks
  to lanes); hod uses one instruction where SSE2 has it and a lane loop
  otherwise, about 2.6× `llc -O0`'s text on generated vector code.
  `llc` 19 aborts on some odd-length inserts (`INSERT_SUBVECTOR`
  widening); hod does not.
- **Not yet.** A constant mask other than zero as a `phi` incoming, a
  call argument or a returned value is `x64:todo:constant`
  (`common::materialize_gpr_const` has no mask form); `fp128` lanes
  are `x64:vector-wide-element` (AArch64 lowers them).

Evidence: `hod_x64/tests/{vector,lanes,mask,vcall}.rs` (WP-132 record).

#### 5.6.14 Leftovers (WP-144)

The small forms the x86-64 groups left, each against `hod_interp`
(where it has the type) and `llc -O0 -fast-isel=0` 22 and 19 under the
emulator (`crates/hod_x64/tests/leftovers.rs`):

1. **Floating-point members** of structs, arrays and packed structs
   (`half`, `bfloat`, `float`, `double`, `fp128`) in `extractvalue`
   and `insertvalue`: the lowering was there (`aggr.rs` moves an `Fpr`
   field as AArch64 does); the matrix now says so. `x86_fp80` members
   stay refused (WP-131).
2. **Conversions with integers of 65 to 127 bits**: `aggr::fp_int_cast`
   extends to 128 bits and calls the `ti` routines (§5.6.5), as
   AArch64; wider, or `bfloat` with more than 64 bits, is
   `x64:wide-fp-cast` (AArch64's `a64:wide-fp-cast`).
3. **The x86-64 irdiff corpus**: the seeds `aggregate.ll` and
   `tail_wide.ll` run; `varargs_callee.ll` spelled `llvm.va_copy.p0.p0`,
   which LLVM 19 and 22 themselves remangle to `llvm.va_copy.p0`, the
   name `hod_ir` reads. What the corpus still declines is vectors,
   `x86_fp80`, coroutines and one `unsupported-target` seed.
4. **`atomicrmw` `fmax`, `fmin`, `fmaximum`, `fminimum`** of `half`,
   `float`, `double`, and `half` `fadd`/`fsub`: a `lock cmpxchg` loop
   (§5.6.10) whose step is `maxnum`, `minnum`, `maximum`, `minimum`
   (`fpintr::min_max_reg`) or `addss`/`subss`. `half` computes in
   `float`, converted inline by `atomic::half_to_float` (a signaling
   NaN quieted, as libgcc's `__extendhfsf2`) and `atomic::float_to_half`
   (round to nearest even). Where `maxnum`/`minnum` of two NaNs or two
   zeros of different sign may return either operand, `llc`'s `half`
   form takes the other one; both are allowed. `bfloat` arithmetic
   stays `x64:atomic-bfloat-op`.
5. **16-byte atomics without `+cx16`** call libatomic as `llc` does,
   with the C `memory_order` numbers (`unordered`/`monotonic` 0,
   `acquire` 2, `release` 3, `acq_rel` 4, `seq_cst` 5):
   `__atomic_load_16(p, o)`, `__atomic_store_16(p, v, o)`,
   `__atomic_exchange_16`, `__atomic_fetch_{add,sub,and,nand,or,xor}_16`
   and `__atomic_compare_exchange_16(p, &expected, desired, success,
   failure)`, the expected value in the result's slot (the routine
   writes the cell's value there on failure) and the flag from `al`.
   `max`, `min`, `umax`, `umin`, `uinc_wrap`, `udec_wrap`, `usub_cond`,
   `usub_sat` are `llc`'s expansion: a plain read of the cell, then a
   loop that computes the new value in `rax`, `rcx`, `rdx`, `r10`,
   `r11` into a slot and calls `__atomic_compare_exchange_16` with the
   strongest failure ordering the success ordering allows. Every value
   is displaced before the loop, so the call's glue reloads only what
   every pass shares; the pointer stays in its callee-saved pool
   register. The routines are `hod_cg::RuntimeFn` members; a program
   links `-latomic`.
6. **`pushfq`/`popfq`** (`9C`/`9D`): the operand form of a dynamic
   thread-local access (§5.6.11) saves `rflags` with `pushfq` and
   restores all of them with `popfq`.
7. **`ud1`**: `llvm.ubsantrap(kind)` is `llc`'s `ud1 eax, [eax + kind]`
   (§5.6.8).
8. **`hod_ir`**: the CET module flags are on by their presence (§5.6.6),
   and `//` is a line comment in x86-64 assembly text as in AArch64's
   (`llvm-mc` 19 and 22 read `nop // x` as `nop`), so module assembly
   with it is read and an inline template of only such comments is
   empty.
9. **The constraint letter `A`**: read by `llc`, refused by hod
   (§5.6.8).

Evidence: `spec/landed/codegen/WP-144.md`.

#### 5.6.15 Text size, the first cuts (WP-146)

hod's x86-64 text was about twice `llc -O0`'s (WP-130). Three cuts,
each independent of the others:

**Shrinking a function (`hod_cg::shrink`).** Once a function's code is
complete (after its exits, its coroutine stubs and the trap, before
its data islands, so that an island is aligned in the final layout),
`FnCx::shrink` removes byte ranges the target asked for and relaxes
branches the target marked:

- `FnCx::cut(at, len)`: `len` bytes at `at` go; nothing refers into
  them and no row or label is inside them. x86-64 cuts the prologue's
  unused bytes (cut 1 below) and the three high bytes of each local
  displacement that fits a byte (§5.6.17).
- `FnCx::relaxable(start, short, kind)`, right after the `fixup` of a
  branch that starts at `start` and ends with its field: if the label
  ends up in reach of a signed byte, the branch becomes `[short, rel8]`
  with a field of `kind`.

The relaxed set is a fixpoint of at most 8 passes, each only adding:
a forward branch is taken when `map(target) - map(start) - long` fits
(the target moves by what the branch itself loses, so the distance is
the same from either form's end), a backward one when `map(target) -
map(start) - 2` does; any set so found stays valid, since removing
bytes only shortens distances. The bytes are compacted, and every
offset the function holds moves by what is removed before it (an offset
inside a removed range goes to its start): labels, pending fixups,
relocations, CFI rows, call-site rows (start, end, landing), pad rows,
line rows, `VarPlace::Label`s, island rows, block offsets, the
coroutine offsets and the described calls' rows (§9.5; a row a target
has not completed stays incomplete). A table of code offsets a later
package adds to the function's record is moved here too: a test must
compare such offsets with the final bytes of a function that shrank.
Every reference to a bound label is then patched again; one out of
reach is an internal error. A function that asked for
neither costs nothing.

**1. The save set.** The prologue pushes only the members of `rbx`,
`r12`..`r15` the body used (`hod_cg` records every register `alloc`,
`take_reg` and `own` hand out: `FnCx::regs_used`), plus `rbx` in a
function with the coroutine frame (`coro_body` writes it directly),
and all five in a function with a `llvm.eh.sjlj.setjmp` (its resume
point is entered with the registers of the code that jumped, §5.6.16).
Nothing else writes a callee-saved register: inline assembly templates
assemble to nothing, the ABI refuses `swift*` (no argument arrives in
one), and `cmpxchg16b` takes `rbx` with `take_reg`. The body is
emitted before the set is known, so `begin` writes the largest
prologue (`push rbp` · `mov rbp, rsp` · five pushes · `sub rsp,
imm32`, 16 bytes after `mov`) and `end` rewrites those bytes as the
set's pushes and `sub rsp, FRAME` (`imm8` when it fits, nothing when
`FRAME` is 0), then cuts the rest. `FRAME ≡ 8n (mod 16)`, a local is
at `[rbp + o - 8n - FRAME]`, and `FrameDesc::frame_size` is `8n +
FRAME`, the bytes below `rbp`. The save rows are written once, at the
offset after the last push: x86-64 has no CFI row inside the body
before the epilogue, and the CFA is `rbp + 16` from the second
instruction on. Until then each register still holds the caller's value
(a push does not change it), which is what a register without a row
means, so the rows are exact at every instruction. Each exit is `lea rsp, [rbp - 8n]` · the pops in
reverse · `pop rbp` (`leave` when `n` is 0), and the coroutine stubs
push the same set. `invoke`, landing pads, `musttail`, variadic
functions and stack probes are unchanged: none depends on the set.

**2. Branches.** A branch to the block emitted next was already
omitted (§6.2). Every other `jmp rel32` and `jcc rel32` (`common::branch`,
`branch_cond`: block edges, the lowerings' local loops and skips, the
coroutine stubs' dispatch) is marked relaxable and becomes `EB cb` or
`70+cc cb` when its label is then in reach; a backward branch that
reaches when it is emitted is short at once and recorded as a reference
(`REL8`) so that it is patched again. Nothing else in `hod_x64` encodes
a displacement between two places of a function except through a
label: `blockaddress` (a `lea` with a `REL32` field), the data islands
and the call-site, line, CFI and variable rows all move with the code.
Relaxation keeps every branch's target, so line rows, CFI advances and
DWARF ranges, computed from the moved offsets, stay exact.

**3. Copies.** `common::copy_frame` (frame place to frame place) and
`common::copy_from_ptr` (memory to a frame place) of more than 64 bytes
(`UNROLLED_MAX`) are `push rcx` · `push rsi` · `push rdi` · `lea rsi,
src` · `lea rdi, dst` · `mov ecx, n` · `rep movsb` · `pop rdi` · `pop
rsi` · `pop rcx`; 64 bytes and fewer stay moves through `xmm15` and
`r11`. Every instruction keeps the flags (the flags rule, §5.6), so a
copy may sit between a compare and its reader. `rcx` never holds a value
but may hold an argument already placed at a call site, as `rsi` and
`rdi` may, so all three are kept. An `Outgoing` place is addressed 24
bytes further from `rsp` while they are pushed (`frame_inst_shifted`).
In `copy_from_ptr`, `rsi` is computed first when the pointer is in
`rdi`, and a displacement beyond 1 GiB is added to `rsi` in steps.
`rep movsb` copies forward, byte by byte, with the direction flag clear
(psABI): the source and destination of these copies never overlap, and
no volatile access goes through them (`memcpy` and `memmove`, volatile
or not, are `intrinsic.rs`'s).

#### 5.6.16 Leftovers, second set (WP-147)

Each against `llc -O0 -fast-isel=0` 22 and 19 under the emulator, and
`hod_interp` where it has the type (`crates/hod_x64/tests/leftovers2.rs`,
`crates/hod_x64/tests/coro_wide_atomic.rs`):

1. **`ptrtoint` of a constant address as an operand**
   (`operand:int-of-addr`): `common::materialize_gpr_const` already
   formed it (`lea` of the symbol, plus the offset); the matrix now says
   so. A vector constant whose lanes are addresses (`<1 x i64>`,
   `<2 x i64>`, `<2 x ptr>`) is written to the 16 bytes below `rsp` by
   `constaddr::write_image` and loaded into the `xmm` register
   (`vector::materialize`). An address in a lane narrower than 8 bytes
   stays `x64:constant-relocation`.
2. **`llvm.eh.sjlj.setjmp` / `llvm.eh.sjlj.longjmp`** with `llc`'s
   x86-64 buffer: word 0 the frame pointer and word 2 the stack pointer
   (both stored by the front end), word 1 the resume address. `setjmp`
   displaces every value, stores `lea r11, [rip + resume]` to word 1 and
   is 0; at the resume label it is 1. A function with a `setjmp` saves
   all of `rbx`, `r12`–`r15` whatever its body used (§5.6.15): they
   arrive at the resume label as the jumping code left them, and the
   epilogue restores the caller's from `rbp`.
   `longjmp` reads all three words before it writes `rbp`, then `rsp`,
   then `jmp`s. hod's and `llc`'s objects jump into each other's
   buffers. A value live across the `setjmp` is read from memory after
   it (WP-152, §5.6.18 (4)). Refused: `x64:sjlj-coroutine` (`rbx`, the frame base, is not restored
   at the resume address), `x64:sjlj-shadow-stack` (the
   `cf-protection-return` module flag: the jump would unbalance the
   shadow stack). On AArch64, `llc` compiles `setjmp` to 0 and `longjmp`
   to nothing, and clang refuses the builtins; hod refuses both there.
3. **A constant mask other than zero** as a `phi` incoming, a call
   argument or a return value: `vector::materialize`, from the target's
   materialize hook, puts its bits in a general register (`mask::
   const_bits`, `mov` of the 64-bit immediate), for every width of
   §5.6.13's masks.
4. **`fp128` lanes** stay `x64:vector-wide-element`: no corpus module
   has a vector of `fp128`; each lane operation would be a libcall.
5. **`fptrunc x86_fp80` to `bfloat`** calls `__truncxfbf2` (libgcc 13
   and later, compiler-rt), as `llc` 22 does; `llc` 19 cannot select it
   (§5.6.12).
6. **16-byte atomics in a coroutine** (§7.6 "x86-64"): with `+cx16`,
   in a function with the coroutine frame, `push rbx` before the
   sequence writes `rbx` and `pop rbx` after the last `cmpxchg16b`; the
   new value is loaded high half (`rcx`) first, so its low half's load
   is the last use of `rbx` as the frame base; the results are stored
   after the `pop`. The libatomic calls without `+cx16` need nothing.
7. **`hod_ir`** reads `llvm.va_start` and `llvm.va_end` without a
   suffix and `llvm.va_copy.p0.p0` as the `.p0` intrinsics (LLVM 19 and
   22 rename them on reading).
8. **`tests/ir/const_operands.ll`** names no target and runs on both.
9. **`is.fpclass` of a pseudo-denormal**: §5.6.12.

Evidence: `spec/landed/codegen/WP-147.md`.

#### 5.6.17 Short frame displacements (WP-151)

**A local's displacement is `disp8` when it fits.** Every access to a
local is emitted as `[rbp + o]` with a 32-bit displacement and its field
recorded (`common::frame_inst`, §5.6), because `8n + FRAME` is known
only in `end_function`. There, `common::bias_local_sites` computes each
field's final displacement `o - 8n - FRAME`; one in `-128..=127` is
rewritten in place as `[rbp + disp8]`: the `ModRM` byte before the field
goes from `mod` 10 to `mod` 01 (`rm` 101, `reg` unchanged), the low byte
of the field becomes the displacement, and the three bytes after it are
cut (`FnCx::cut`, §5.6.15). Whatever follows the field in the same
instruction (an immediate of 1, 2 or 4 bytes, or nothing) moves with the
code, so the instruction stays whole; prefixes (`66`, `F2`, `F3`, REX,
`lock`) and the opcode come before the `ModRM` and are untouched. A
recorded field whose preceding byte is not the `[rbp + disp32]` `ModRM`
(`mod` 10, `rm` 101) keeps its `disp32`; no access emitted today has
another shape. `Incoming(n)` and `Outgoing(n)` places, RIP-relative
operands, `SIB` forms and the coroutine stubs' own frame words are
emitted at their final displacement and were already the shortest form
(`hod_mc_x64` picks it); `FrameRef::Coro` is `[rbx + o]`, not a local.

The cuts are shrink-pass ranges like the prologue's (§5.6.15): they
are disjoint (a field is inside one instruction, after its `ModRM`),
recorded in offset order, and nothing refers into them — a relocation,
a label, a fixup or a row is never inside an instruction's
displacement. Each table of offsets moves by the bytes cut before it,
so relocations of a later field of the same instruction (none today),
call-site rows (whose ends are mapped, not their lengths), line rows,
CFI rows and labels stay exact; the relaxation of §5.6.15 sees the cut
bytes, so a branch over frame accesses that reaches only once they are
`disp8` is relaxed too. Nothing in `hod_x64` records an instruction's
length or a distance between two offsets except through those tables
(§5.6.15, cut 2): relaxable branches have no frame operand; `rep movsb`
copies have no loop; inline assembly is a barrier with no frame
operand. Variable locations are `DW_OP_fbreg` from `rbp`: they name
the displacement, not its encoding, and do not change.

The frame layout (`hod_cg`'s, §5.1) is unchanged: ordering the slots by
use, so that the most used are nearest `rbp`, would make about 0.7% of
the corpus's text smaller at best (`spec/landed/codegen/WP-151.md`)
and needs a layout pass in `hod_cg`; it is not done. A store to a slot
followed at once by a load of it is not rewritten either: about 0.5%
at best, and the load is the target of a branch in 43% of those pairs,
which an x86-64 peephole at emission cannot see without a query on
`hod_cg`'s labels.

#### 5.6.18 The first libc++ run's leftovers (WP-152)

Each against `llc -O0 -fast-isel=0` 22 and 19 under the emulator, and
`hod_interp` where it has the type (`crates/hod_x64/tests/leftovers3.rs`):

1. **Lanes of `i128` and `x86_fp80`** (`vector.rs`, "wide lanes"). Such
   a vector is a `Mem` value (`<1 x i128>` too: `target::vector_class`
   keeps it out of `Fpr`, as it travels in two general registers like an
   `i128`, §4.3 of `spec/abi-x64.md`). Its image is packed as LLVM lays
   it out: lane `i` at `16·i` (`i128`) or `10·i` (`x86_fp80`, 10 bytes
   each, so a lane may start at any even offset). `extractelement`,
   `insertelement` and `shufflevector` copy lanes between images:
   a constant index is a frame place; a variable index (`lane_number`,
   §5.6.13: an index out of range is poison, any lane) is scaled into
   `rcx` (`shl rax, 4`, or `lea rax, [rax + rax·4]` · `lea rcx, [rcx +
   rax·2]`) and the lane copied through `rcx`. A `poison` index of
   `insertelement` copies nothing; an undefined `shufflevector` lane is
   zeroed. More than `MAX_LANES` lanes is `x64:vector-length`; `fp128`
   lanes stay `x64:vector-wide-element` (§5.6.16). Arithmetic on such
   lanes, and casts other than item 5's, stay `x64:vector-wide-element`.
2. **Vectors of `i128` and `x86_fp80` as arguments and results**
   (`abi.rs`, `spec/abi-x64.md` §4.3): element by element, `x86_fp80`
   as a scalar `x86_fp80` (16-byte stack slots; `st0`/`st1` results up
   to two elements), `i128` as LLVM 22's 8-byte pieces where LLVM 19
   agrees, else `x64:vector-i128-split`.
3. **`byval` aligned above 16** (`call::align_stack_area`,
   `spec/abi-x64.md` §6.2): before a call sequence with such an
   argument, `sub rsp, c` · `and rsp, -a` · `add rsp, c` (`c` the
   rounded stack size mod `a`) makes the stack-argument area
   `a`-aligned, as `llc`'s realigned frame does. The callee needs
   nothing: its copy is at the offset §6.2 gives. Not on a `musttail`
   call (it reuses the caller's incoming area, which the caller's caller
   aligned: `spec/abi-x64.md` §8.5 (5)). Above 2^30 (the `and`
   immediate) or not a power of two stays `abi:byval-align`.
4. **Values live across `llvm.eh.sjlj.setjmp`** (was
   `x64:sjlj-live-value`). `intrinsic::sjlj_setjmp` calls
   `FnCx::never_reuse_spill_slots` (§4.3), then `displace_all`: every
   register value, an argument of the entry block still in its
   register included, is stored to a spill slot, and its reload after
   either return reads that slot, which nothing reuses. Home values
   (§4.1) are in their slots already; a deferred value is computed
   again from operands in memory; the buffer operand is pinned, and its
   only register use is the `setjmp`. `rbx`, `r12`–`r15` are saved
   whatever the body uses (§5.6.15), so no callee-saved register holds
   a value of the caller across the jump.

5. **Masks of `i128` lanes** (`mask.rs`, "`i128` lanes"): `icmp` of
   `<N x i128>` and `zext`/`sext` of a mask to `<N x i128>`, `trunc`
   back, lane by lane on the vector's slot as `llc` does. Equality:
   `xor` of each half, `or`, `sete`/`setne`. An order: `cmp` of the low
   halves, `sbb` of the high halves, `setb`/`setae`/`setl`/`setge`, the
   operands swapped for `>` and `<=`. `zext`/`sext`: bit `i` of the mask
   (negated for `sext`) as the low half, 0 or the same as the high half.
   `trunc`: bit 0 of each low half. A cast of a mask to `x86_fp80` lanes
   stays `x64:vector-wide-element`.

Evidence: `spec/landed/codegen/WP-152.md`.

#### 5.6.19 Arithmetic on `i128` and `x86_fp80` lanes; `pause` (WP-161)

Each against `llc -O0 -fast-isel=0` 22 and 19 under the emulator (hod
plain and stressed), and `hod_interp` for `i128` lanes
(`crates/hod_x64/tests/wide_arith.rs`). All work lane by lane on the
images of §5.6.18 (1): the operands are `Mem` places, the result a slot
written lane by lane at `16·i` or `10·i`.

1. **Integer binary operators on `i128` lanes** (`vector::wide_binary`):
   each lane is the scalar `i128` sequence of §5.6.5 on 16-byte places
   (`aggr::limbwise` for `add`/`sub`/`and`/`or`/`xor`, `aggr::multiply`,
   and `__udivti3`/`__divti3`/`__umodti3`/`__modti3` per lane through
   `call_runtime`, as `llc`'s scalarized division calls them). Shifts: a
   lane's constant amount uses `aggr::shift_const`; 128 or more is
   poison and that lane is not written; a variable amount uses
   `aggr::shift_var` (an amount of 128 or more gives some value, never a
   fault, and other lanes are unaffected).
2. **Casts with `i128` lanes** (`vector::wide_cast`): `zext`/`sext`/
   `ptrtoint`/`ptrtoaddr` from integer or pointer lanes of at most 64
   bits (low half the lane extended, high half 0 or its sign), `trunc`/
   `inttoptr` to them (the low half's low bits), `sitofp`/`uitofp` to
   and `fptosi`/`fptoui` from `float` and `double` lanes through the
   runtime routines of the scalar conversion (§5.6.7: `__floattisf`,
   `__floatuntidf`, `__fixsfti`, `__fixunsdfti`, ...) per lane. The
   casts of masks stay §5.6.18 (5)'s; casts with odd-width lanes keep
   their names.
3. **`x86_fp80` lanes** (`vector::wide_binary`, `wide_fneg`,
   `mask::wide_fcmp`): `fadd`/`fsub`/`fmul`/`fdiv` per lane as the scalar
   x87 sequence of §5.6.12 (`fld tbyte` both, `faddp`/..., `fstp tbyte`); `fneg`
   copies the image and flips bit 7 of each lane's byte 9; `fcmp` per
   lane `fld` right, `fld` left, `fucomip st1`, `fstp st0`, then the
   predicate's code from the flags as the scalar `fcmp` (§5.6.12), its
   bit set in the mask. `false`/`true` are constant masks.
4. **`select` of `i128` and `x86_fp80` lanes on a mask**
   (`mask::wide_select`): per lane, the addresses of both lanes,
   `bt` of the mask bit, `cmovae`, and a copy of the lane through the
   chosen address.
5. **`llvm.x86.sse2.pause`** (`intrinsic.rs`) is `pause` (`F3 90`), as
   `llc`'s. AArch64 refuses it by its name (`never(X64_ONLY)`).

Still `x64:vector-wide-element`: `frem` of `x86_fp80` lanes; any cast
to, from or between `x86_fp80` lanes and conversions between `i128`
lanes and `half`, `bfloat` or `fp128` lanes; intrinsics and reductions
on `i128` or `x86_fp80` lanes; `fp128` lanes.

Evidence: `spec/landed/codegen/WP-161.md`.

#### 5.6.20 Vectors of `i128` and `<1 x i64>` across calls (WP-171)

Against `llc -O0 -fast-isel=0` 22 and 19 under the emulator, both
directions (`crates/hod_x64/tests/vcall_i128.rs`):

1. **`<N x i128>` arguments** (`abi::check_i128_vector`,
   `spec/abi-x64.md` §4.3): accepted when the first element is in two
   registers or on the stack from an offset that is a multiple of 16,
   whatever the later elements straddle; LLVM 19 pairs only the first
   element. `x64:vector-i128-split` stays for a first element whose low
   half would be in `r9`, and for a vector on the stack from an offset of
   8 mod 16: there LLVM 19 and 22 place it differently (each such case
   of the test loses a leaf between an `llc` 19 caller and an `llc` 22
   callee).
2. **`musttail` with such a signature** stays
   `musttail:stack-args-unknown` (`spec/abi-x64.md` §8.5, item 6): the
   rule has no stack-argument area for a signature `assign` refuses, and
   `llc` 19 and 22 disagree on where its arguments are.
3. **`<1 x i64>` and `<1 x ptr>`** were `call:part-register-class`
   here; WP-174 (§5.6.21) passes them.

Evidence: `spec/landed/codegen/WP-171.md`.

#### 5.6.21 `<1 x i64>` across calls, label differences (WP-174)

1. **A lone `<1 x i64>` or `<1 x ptr>`** argument or result (the value
   an `xmm`, `target::vector_class`; LLVM 19 and 22 scalarize it to an
   `i64`: a general register, an 8-byte stack slot, `rax`) has one part
   of kind `Lanes { bits: 64, lane: 8, first: 0, len: 1 }`
   (`abi::LONE_LANE`, `spec/abi-x64.md` §4.3). `hod_cg` hands the value
   in the `Fpr` scratch register (WP-167's paths: argument, result,
   `place_return`, entry); `vcall::lone_out`/`lone_in` move it with
   `movq r64, xmm` / `movq xmm, r64`, or an 8-byte store or load for a
   stack slot. A side of another class is `call:part-lanes`. As a member
   of an aggregate the value is memory and its bytes travel as `Value`
   parts, unchanged. No `hod_cg` change was needed.
2. **`init:block-diff`** on x86-64: the shared rule (§5.5.1 "WP-095",
   module emission writes `offset(a) - offset(b) + addend`, no
   relocation) holds unchanged; the block rows it reads are moved by
   `shrink.rs` (§5.6.15), so the number is the one of the final code.
   Only the classification row was `x64:todo`.
3. **Kept refused** then: `inst:callbr`, `cc:preserve_mostcc` /
   `cc:preserve_allcc`; built since WP-181 (§5.6.23). Sizes and reasons:
   `spec/landed/codegen/WP-174.md`.

#### 5.6.22 x86 target intrinsics, SSE to SSSE3 (WP-173)

The `llvm.x86.*` calls clang 19 and 22 write at `-O0` for the functions
of `<xmmintrin.h>`, `<emmintrin.h>`, `<pmmintrin.h>` and
`<tmmintrin.h>` (`crates/hod_x64/src/x86.rs`, `hod_ir`'s `Intrinsic`
members `X86Sse*`, `X86Sse2*`, `X86Sse3*`, `X86Ssse3*`, matrix rows
`never(X64_ONLY)` / `on(X86_SSE)`). Each against `llc -O0
-fast-isel=0` 22 and 19 under the emulator, hod plain and stressed
(`crates/hod_x64/tests/x86_intrinsics.rs`). A call whose operand or
result types are not LLVM's declaration is `x64:x86-intrinsic-signature`.

1. **SSE and SSE2** (the baseline; no feature check), one instruction as
   `llc` writes it, the result in the first operand's register:
   `max.ps`/`min.ps`/`max.ss`/`min.ss` and the `pd`/`sd` forms are
   `maxps`/`minps`/`maxss`/... (the second operand when either is NaN or
   both are zeros); `cmp.ss`/`cmp.sd` are `cmpss`/`cmpsd` with the
   `immarg` predicate's low three bits (the legacy encoding's, as `llc`
   encodes 8 to 31; with AVX, §5.6.33 (4)); `comi{eq,ge,gt,le,lt,neq}.ss` and `.sd` are
   `comiss`/`comisd`, the `ucomi` ones `ucomiss`/`ucomisd`, then the
   predicate from the flags: `gt` `seta`, `ge` `setae`, `lt` and `le` the
   same with the operands swapped, `eq` `sete` and `setnp`, `neq`
   `setne` or `setp`, zero-extended to `i32`; `cvtss2si`, `cvttss2si`,
   `cvtsd2si`, `cvttsd2si` (and the `64` forms) are those instructions
   to a general register (`0x80000000...` for NaN and out of range);
   `cvtpd2dq`, `cvttpd2dq`, `cvtps2dq`, `cvttps2dq`, `cvtpd2ps`,
   `cvtsd2ss`; `movmsk.ps`/`movmsk.pd`/`pmovmskb.128` to an `i32`;
   `packssdw`, `packsswb`, `packuswb`, `pavg.b`/`.w`, `pmadd.wd`,
   `pmulh.w`, `pmulhu.w`, `psad.bw`; `psll`/`psrl`/`psra` `.w`/`.d`/`.q`
   by a vector (its low 64 bits are the count) and the `i` forms by an
   `i32`: a constant count is the immediate, every count of 255 or more
   written as 255 (each count at or above the lane width shifts out
   every bit, or fills with the sign for `psra`), a variable count is
   moved to `xmm15` by `movd` and the vector form used; `sfence`,
   `lfence`, `mfence`. `pause` stays §5.6.19 (5). WP-181:

   | intrinsic | instruction |
   |---|---|
   | `sse.ldmxcsr(ptr)`, `sse.stmxcsr(ptr)` | `ldmxcsr [p]`, `stmxcsr [p]` |
   | `sse2.clflush(ptr)` | `clflush [p]` |
   | `sse.rcp.ps`, `sse.rsqrt.ps` | `rcpps`, `rsqrtps` in place (the hardware's approximation, the same instruction as `llc`'s) |
   | `sse.rcp.ss`, `sse.rsqrt.ss` | `rcpss`, `rsqrtss` in place: lanes 1 to 3 of the operand kept |
   | `sse2.maskmov.dqu(data, mask, ptr)` | `mov rax, rdi` · `mov rdi, p` · `maskmovdqu data, mask` · `mov rdi, rax` (the address is `rdi`; `rax` never holds a value) |
2. **SSE3**, only in a function whose `target-features` enable `sse3`
   (the last item that implies it, `+sse3`, `+ssse3`, `+sse4.1`,
   `+sse4.2`, `+avx` or an AVX feature, or disables it, `-sse`, `-sse2`,
   `-sse3`, decides; none is off). hod writes SSE2 instructions only:
   `hadd.ps`/`hsub.ps` gather the even lanes of `a` then `b` by `shufps
   0b10001000` and the odd ones by `shufps 0b11011101` into `xmm14`, and
   `addps`/`subps` them (even minus odd); `hadd.pd`/`hsub.pd` the same
   with `unpcklpd`/`unpckhpd`; `addsub.ps`/`.pd` compute `a - b` and
   `a + b` and merge the even lanes of the difference with the odd ones
   of the sum through a mask in `xmm15` (`pcmpeqd`, then `psrlq 32` or
   `psrldq 8`); `ldu.dq` is `movdqu`.
3. **SSSE3** `.128` forms, only where the features enable `ssse3` (as
   (2), `-ssse3` too): lane by lane in general registers on 16-byte
   frame images of the operands, the result's image loaded back:
   `phadd`/`phsub` `.w`/`.d` (wrapping) and `.sw` (saturated to `i16`
   by `cmovg`/`cmovl`), the pairs of `a` then of `b`; `pmadd.ub.sw`
   (unsigned bytes of `a` times signed bytes of `b`, pairs summed and
   saturated); `pmul.hr.sw` (`(a·b + 0x4000) >> 15` per `i16`); `pshuf.b`
   (byte `b[i] & 15` of `a`, 0 where bit 7 of `b[i]` is set); `psign.b`/
   `.w`/`.d` (`a`, `-a` or 0 as `b` is positive, negative or 0).
4. **Refused by name.** An SSE3 or SSSE3 intrinsic in a function without
   its feature is `intrinsic:` and its LLVM name, as `llc` cannot select
   it. Every `llvm.x86.*` name hod does not list is `intrinsic:` and its
   name from `classify` (`spec/ir.md`): `sse3.monitor`/`mwait` (no
   encoding in `hod_mc_x64`), and the `x86_mmx` forms clang 19
   writes (`mmx.*`, `sse.cvtpi2ps`, ..., the 64-bit `ssse3.*`). SSE4.1,
   SSE4.2 and later are §5.6.29 (`sse42.crc32.*`) and §5.6.33 (WP-202).
   AArch64 refuses every `llvm.x86.*` name.

Evidence: `spec/landed/codegen/WP-173.md`.

#### 5.6.23 `callbr` of an empty template, `preserve_mostcc` and `preserve_allcc` (WP-181)

1. **`callbr` of an empty template** (`barrier.rs`, `inst:callbr` `ON`
   on x86-64). The constraint string has one `!i` per indirect
   destination, after the operands; a label takes no call argument and
   any other count of labels (or a label on `call` or `invoke`) is
   `x64:inline-asm-constraint`. The template never jumps, so the call is
   §5.6.8's barrier (inputs evaluated, a tied output is its input)
   followed by the fallthrough edge: its `phi`s and a branch unless the
   fallthrough is the next block. Nothing is written for an indirect
   edge: the indirect destinations are blocks like any other, emitted
   because they are successors, entered only from elsewhere (a branch,
   an `indirectbr` through their `blockaddress`), as `llc` builds them.
   A template that assembles to something stays `x64:inline-asm` (no
   assembler). A destination's `phi` that takes the result is written
   on the fallthrough edge after the result (§4.5; the refusal
   `x64:callbr-result-phi` of the first landing is retired: WP-181).
   AArch64 builds `callbr` through its assembler (§5.5.3) and is not
   changed.
2. **`preserve_mostcc` and `preserve_allcc`** (`frame.rs`, `abi.rs`;
   `cc:preserve_mostcc` and `cc:preserve_allcc` `ON` on x86-64).
   `assign` gives both `ccc`'s assignment (`spec/abi-x64.md` §1.2).
   *Callee:* after `mov rbp, rsp` the prologue pushes `rax`, `rcx`,
   `rdx`, `rsi`, `rdi`, `r8`, `r9`, `r10` (`frame::PRESERVED`) but the
   registers a part of the result is returned in, with their rows at
   `-24`, `-32`, ...; the save set (§5.6.15) follows them, its rows
   below; a `preserve_allcc` function then stores `xmm0`..`xmm15` but
   the result's (`movups`) in a 16-aligned local area after `sub rsp`,
   their rows (`DW_CFA_offset` of DWARF 17..32) after the last store.
   Every exit loads the `xmm` registers back, then `lea rsp, [rbp -
   8n]` (`n` counting every push), pops the save set, the extra
   registers and `rbp`. This is LLVM 19's and 22's set, measured on
   their `.cfi_offset` rows: `r11` is not kept; `rax` is kept in a
   `void` or `sret` function (the caller's `rax` comes back over the
   `sret` address), not for `i64`/`i32`/`i1`, `rax` and `rdx` not for
   `i128`, `rax`, `rdx`, `rcx` not for three `i64`s, `xmm0` (`xmm1`)
   not for a `float`, `double`, `<4 x float>` (two `double`s); an
   `x86_fp80` result keeps them all. hod pushes every register of the
   set, `llc` those its allocator touched: the caller cannot tell.
   *Caller:* a call is a `ccc` call. The callee keeps a superset of
   `ccc`'s callee-saved registers, so the code around it, which assumes
   only `ccc`'s, is correct; it does not use what the callee keeps
   beyond them.
   *Refused:* a result returned in memory is
   `x64:preserve-cc-return-memory` (LLVM 19 and 22 keep sets off the
   rule there: `xmm0`/`xmm1` unsaved for four `double`s, the address
   in `rax` popped over); a `musttail` exit from either is
   `x64:preserve-cc-tail-call` (the teardown restores the registers the
   tail call passes arguments in); a coroutine body is
   `x64:coro-preserve-cc`. `swifttailcc`, `preserve_nonecc` and the
   other conventions stay `abi:cc`.

#### 5.6.24 Stack protectors (WP-183, both targets)

`ssp`, `sspstrong` and `sspreq` (clang's `-fstack-protector`, `-strong`,
`-all`; kiln passes them through) are built on both targets. The rules
are target-neutral (`hod_cg::guard`); each target emits the code
(`hod_x64::guard`, `hod_a64::guard`).

1. **Which functions.** A function with one of the three attributes is
   protected when it has `sspreq`, at least one body slot, or an
   `alloca` instruction. LLVM (`StackProtector::requiresStackProtector`)
   protects under `ssp` a function with a char array of at least
   `"stack-protector-buffer-size"` bytes (in a struct too) or an
   `alloca` of a non-constant or large count, and under `sspstrong` also
   any array, any aggregate holding one and any local whose address is
   taken; under `sspreq` every function. hod keeps no slot types
   (`spec/ir.md` §5.2), so it cannot tell an array from a scalar; the
   rule above is a superset of LLVM's for each attribute, which is what
   matters (never weaker than `llc`), and costs one guard more in
   functions `llc` leaves alone. `"stack-protector-buffer-size"` is read
   with the attribute and changes nothing. A protected presplit
   coroutine is built (rule 6: its resume and destroy stubs repeat the
   entry code); one that would need the low guard (rule 4) is refused,
   `ssp:coroutine`, a shape the coroutine plan already refuses (dynamic
   `alloca`s and over-aligned machine-stack slots, §7.6). A target
   without the hook, `ssp:target`.
2. **Which slots move.** A slot **escapes** when its address is used
   other than as the pointer operand of a `load` or `store` whose access
   lies inside the slot (a `getelementptr`, a call argument, a stored
   value, a `phi`, a cast, ...). Only an escaping slot can be written out
   of bounds. The escaping slots of a protected function whose alignment
   is at most the stack alignment, and not in a coroutine's heap frame
   (§7.6), form the **guarded block**, from its top down by decreasing
   size, ties in body order, each at its alignment; the **guard word**
   (8 bytes) follows the top slot at once, unaligned if need be, so that
   a one-byte overflow of it reaches the guard (`llc` may leave padding
   there). `llc` puts arrays nearest its guard, then the other
   address-taken locals, the first in body order nearest; hod keeps no
   slot types, and size is what tells a buffer from the small objects
   whose address C++ takes (a `this`, an awaiter). With several slots,
   an overflow of a lower one reaches the guard after the slots above
   it, in both builds. The block is
   rounded up to 16. Each slot of the block is addressed through a
   pointer slot (`SlotPlace::Indirect`, as an over-aligned slot, §5.1);
   the pointer slots are the first slots after `begin_function`'s. Its
   variable's location is the pointer slot, dereferenced (§9.5). Slots
   that do not escape stay where §5.1 puts them, below the block.
3. **Where the block is.** The block is the local area's last
   allocation, made in `end_function` before the frame size is read, so
   it is at the top of the local area: on x86-64 right below the save
   set (at most 8 bytes of padding between them, §5.6), on AArch64
   right below the CFA (§5.5). A linear overflow of any slot of the block
   reaches the guard word before the saved registers and the return
   address (x86-64) or the caller's frame and frame record (AArch64).
   Everything that is not in the block is below it.
4. **The low guard.** A protected function with a dynamic area (an
   `alloca` instruction or an over-aligned slot) has a second guard word
   between that area and the rest of the frame: on x86-64 at `Local(0)`,
   the first 16 bytes `begin_function` reserves; on AArch64 at
   `[x29 - 16]`, 16 bytes the entry code takes below the frame record
   (`sub sp, sp, #16`; the epilogue then restores `sp` from `x29`).
   AArch64 needs it because hod's frame record is at the bottom of the
   frame: without it, the overflow of a dynamic allocation would reach the
   saved `lr` first. On x86-64 `llc` has no such word (a dynamic
   allocation's overflow runs through the fixed objects before it reaches
   the guard); hod's makes it stronger there.
5. **The guard value.** x86-64 Linux: `fs:[0x28]` (the TCB's
   `stack_guard`), read with `mov r10, fs:[0x28]`. AArch64 Linux: the
   variable `__stack_chk_guard` through the GOT (`adrp`
   `R_AARCH64_ADR_GOT_PAGE`, `ldr` `R_AARCH64_LD64_GOT_LO12_NC`, `ldr`),
   under every relocation model and whatever the module's `PIC Level`,
   as `llc` reads it in a PIC module. (`llc` reads it directly, `adrp`
   `R_AARCH64_ADR_PREL_PG_HI21`, in a module without `PIC Level` even
   under `-relocation-model=pic`, an object no PIE link accepts; the GOT
   form links everywhere, and the linker relaxes it.) Both symbols are
   `RuntimeFn` members (`StackChkGuard` is a variable, only addressed).
   The module flags `stack-protector-guard`, `-guard-reg`,
   `-guard-offset`, `-guard-symbol` (`-mstack-protector-guard*`) are
   refused by their names, `module-flag:<name>` (§2.1 of `spec/ir.md`).
6. **Entry code** (`Target::enter_guard`, after the arguments' entry
   code and before the over-aligned slots', scratch registers only): the
   value stored in the guard word and in the low guard, then each slot's
   address in its pointer slot. The block's offset is not known yet: on
   x86-64 the `lea r11, [rbp + disp32]` of its base is a local access
   (§5.6) whose displacement `end_function` writes before biasing; on
   AArch64 the guard word's address is made by `add x16, x29, #lo12` ·
   `add x16, x16, #hi12, lsl #12`, patched in `end_function`, and each
   slot's address is `x16` less its distance from the guard. A
   coroutine's resume and destroy stubs (§7.6) rebuild a machine frame
   and jump into the body past this code, so each stub repeats it
   (`guard::stub_enter` of each target, after the frame word's store), with the
   final offsets, which are known by then: x86-64 `mov r10, fs:[0x28]` ·
   `mov [rbp + guard], r10` · per slot `lea r10, [rbp + slot]` · `mov
   [rbp + pointer], r10`; AArch64 the same through `x16`/`x17`. Every
   activation of the coroutine body then has a fresh guard and checks it
   at its exit (rule 7), as `llc` does in each function CoroSplit makes.
7. **The check** starts every exit sequence (the epilogue and every tail
   exit, so a `musttail` call is checked before its jump; a `noreturn`
   call and `unreachable` have none; unwinding through a protected frame
   checks nothing, as with `llc`). x86-64: `mov r10, [guard]` · `cmp r10,
   fs:[0x28]` · `jne fail`, the same for the low guard; `r10` is free at
   every exit (`r11` may hold an indirect tail callee), the flags are
   dead. AArch64: `str x16, [sp, #-16]!` (an indirect tail callee is in
   `x16`) · the value into `x16` · `ldr x17, [guard]` · `cmp x16, x17` ·
   `b.ne fail` (and the low guard) · `ldr x16, [sp], #16`. The check
   runs before the result is moved or anything is restored, so it
   writes no register that holds a result or a tail call's argument.
8. **The failure call** follows the last exit sequence: the body's CFA
   rules restated (x86-64 `def_cfa rbp, 16`; AArch64 the body rows, as a
   tail exit), then `call __stack_chk_fail` (`R_X86_64_PLT32`) or `bl
   __stack_chk_fail` (`R_AARCH64_CALL26`), which does not return. No row
   changes for it otherwise, and no call-site row covers it.
9. **Intrinsics.** `llvm.stackprotector` and `llvm.stackguard` are not
   written by clang or kiln; `classify` refuses them by name
   (`intrinsic:llvm.stackprotector`, `intrinsic:llvm.stackguard`).

Evidence: `spec/landed/codegen/WP-183.md`.

#### 5.6.25 Indirect-branch tracking: `cf-protection-branch` (WP-184)

A module with the flag `cf-protection-branch` (any value, §5.6 "Module
flags"; `BranchProtection::CET_IBT`) gets an `endbr64` (`f3 0f 1e fa`)
at each place an indirect branch, a `longjmp` or the unwinder may
reach (`crates/hod_x64/src/cet.rs`). The rules are those of `llc` 19
and 22 (`X86IndirectBranchTracking`, identical in both), with one
deliberate superset:

1. **Function entry**, the prologue's first instruction (`cet::entry`,
   one call from `frame::begin`), unless the function has `nocf_check`.
   `llc` also leaves out a function with local linkage whose address is
   never taken (no data reference, no `llvm.used`, no cast or indirect
   use); hod marks those too. That is harmless: `endbr64` is in the
   reserved-`nop` space and executes as a `nop` on every x86-64
   processor, with or without CET, and an extra mark only permits a
   branch the program never makes. It saves a module-wide scan.
2. **Address-taken blocks** (`blockaddress`, the targets of
   `indirectbr`) and **landing pads**: right after the block's label and
   the offset recorded for it (the `BlockOffset` and the call-site
   table's landing both point at the `endbr64`), before the block's
   `phi` loads (`hod_cg`'s `Target::branch_target` hook, called from
   `emit_block`). An `invoke` whose stack arguments must be released
   lands in a stub that restores the stack pointer first; the stub is the
   landing address, so it is marked as well (one more `endbr64` than
   `llc`, which has no stub).
3. **The resume point of `llvm.eh.sjlj.setjmp`** (`llc` takes its
   address as a block's; `longjmp` reaches it with `jmp *`), and each
   coroutine entry stub `F.resume`/`F.destroy` (functions entered
   through the frame's pointers).
4. **After a direct call of a function declared `returns_twice`**: the
   callee's declaration decides, as in `llc`; `returns_twice` on the
   call site only, or on an indirect call, marks nothing. The return
   address the call-site table records is the call's, before the mark.
5. **Not** at `callbr` indirect targets (`llc` marks none) or switch
   cases (hod makes no jump table, so it has no `notrack jmp`).

All marks are emitted in the instruction stream, so every offset table
(labels, line rows, CFI, call sites, block offsets, the shrink tables)
sees them as ordinary instructions.

**`nocf_check`** (`FnAttrs::NO_CF_CHECK`, a call-site flag too): on a
function it drops the entry mark; on a direct call it changes nothing.
An indirect call with `nocf_check` under `cf-protection-branch` needs the
`notrack` prefix (`3e`), which `hod_mc_x64` does not encode: it is
refused, `x64:nocf-check-indirect-call`, `musttail` or not (`llc` 19 and
22 abort on the `musttail` form). Without the flag it is an ordinary
call, as in `llc`.

**The note** carries IBT (1) with this flag and SHSTK (2) with
`cf-protection-return`, written once by the module emitter through
`hod_obj::elf::add_gnu_properties` (`spec/object.md`), byte for byte
`llc`'s. AArch64 refuses both flags (`never(X64_ONLY)`); `llc` ignores
them there.

Evidence: `spec/landed/codegen/WP-184.md`.

#### 5.6.26 AArch64 branch protection (WP-185)

`-mbranch-protection` as clang 19 and 22 and kiln write it. The module
flags `branch-target-enforcement`, `sign-return-address`,
`sign-return-address-all`, `sign-return-address-with-bkey` and
`guarded-control-stack` (value 1, clang 19; value 2, clang 22: the
functions carry the policy) become per-function attributes in `hod_ll`
(`spec/ll.md` §7.3); from there the function attributes
`"branch-target-enforcement"`, `"sign-return-address"` (`none`,
`non-leaf`, `all`), `"sign-return-address-key"` (`a_key`, `b_key`) and
`"guarded-control-stack"` decide (`hod_a64/src/protect.rs`). Any other
value of the two strings is `a64:branch-protection-value`.
`branch-protection-pauth-lr` (FEAT_PAuth_LR) stays refused on AArch64.
x86-64 ignores every one of these names, as `llc` does.

1. **Entry.** A signed function begins with `paciasp` (`hint #25`) or,
   with the B key, `pacibsp` (`hint #27`); both are BTI landing pads for
   calls, so no `bti c` follows. An unsigned function under BTI begins
   with `bti c` (`hint #34`). Every function gets it, address taken or
   not, as `llc` 19 does (`llc` 22 omits it for a local function whose
   address is not taken): a linker veneer reaches a far callee by `br
   x16`. hod's frame always saves `x30`, so `non-leaf` signs every
   function, as `all` does (`llc` signs only functions that spill `x30`).
2. **Exit.** Every return and every `musttail` exit authenticates before
   leaving: `ldp x29, x30`, the stack restored, then `autiasp`
   (`hint #29`) or `autibsp` (`hint #31`), then `ret`, `b` or `br x16`.
   A coroutine's resume and destroy stubs (`coro_body.rs`) sign at
   their own entry.
3. **Landing pads in a function.** Under BTI, `bti j` (`hint #36`) begins
   every block whose address is taken (`blockaddress`), every landing pad
   (the unwinder branches to it; `llc` 22 does this, `llc` 19 does not),
   and follows every call that returns twice (the call site's or the
   callee's `returns_twice`: `longjmp` returns there by `br`). hod makes
   no jump tables, so a `switch` needs none.
4. **CFI.** A signed function has `DW_CFA_AARCH64_negate_ra_state` after
   the sign instruction (offset 4) and after each `auti*sp`; a body
   restated after an exit (§9.3) restores the signed state with another,
   so the parity at every offset is the signing state. A B-key function's
   FDE begins with `BKeyFrame`, which selects a CIE with augmentation `B`
   (`spec/eh.md` §2.2, §4.5).
5. **`llvm.returnaddress`.** The loaded address is stripped with
   `xpaclri` (`hint #7`) through `x30`, always, as `llc` does.
6. **The note.** Each object carries `.note.gnu.property` with
   `GNU_PROPERTY_AARCH64_FEATURE_1_AND` (BTI 1, PAC 2, GCS 4, each from a
   non-zero module flag), byte-equal to `llc` 19 and 22's
   (`spec/object.md` "GNU property note"); no flag, no note. GCS changes
   no code.

Evidence: `spec/landed/codegen/WP-185.md`.

#### 5.6.27 Stack probing (WP-186)

A function with `"probe-stack"="inline-asm"` (`hod_ir::StackProbe::
Inline(P)`, `spec/ir.md` §5.7; `P` is 4096 unless `"stack-probe-size"`
says otherwise) touches the stack it allocates so that a guard region of
`P` bytes below the stack cannot be jumped over: after each allocation
step the new lowest page is touched before the stack pointer goes lower
again, from high to low addresses, and the CFI is valid at every
instruction, the probe loops included. Both targets keep the probe code
in one module (`hod_x64::probe`, `hod_a64::probe`) called where the
frame is allocated (the prologue, and the coroutine stubs' copy of it),
where a dynamic allocation sets the stack pointer (`alloca`, an
over-aligned slot: `dynamic_slot`) and where an outgoing argument area
is allocated (`adjust_sp`). A function without the attribute is
byte-identical to what it was.

1. **x86-64**, `llc -O0`'s shapes (19 and 22). `FRAME` (the `sub` after
   the pushes, §5.6.15) of at most `P` is one `sub`; up to `8P`, `sub rsp,
   P` · `mov qword [rsp], 0` while more than `P` remain, then `sub rsp,
   rest`; above, `mov r11, rsp` · `sub r11, FRAME & -P` · `1: sub rsp, P`
   · `mov qword [rsp], 0` · `cmp rsp, r11` · `jne 1b` · `[sub rsp, FRAME %
   P]`. The rest below the last touch (at most `P`) is left as `llc`
   leaves it: the next push or call touches it. The CFA is `rbp + 16`
   from the second instruction on, so no row changes in the loop. The
   prologue of a probed function reserves 105 bytes more, cut with the
   rest (§5.6.15). A dynamic allocation ends in `llc`'s loop before `mov
   rsp, r11`: `1: cmp rsp, r11` · `jbe 2f` · `xor qword [rsp], 0` · `sub
   rsp, P` · `jmp 1b` (`r11` already aligned: hod aligns before the loop,
   `llc` after). An outgoing argument area is `mov qword [rsp - 8], 0`
   (a dead word below the stack pointer: the touch of what the frame
   left) then `lea rsp, [rsp - c]` · `mov qword [rsp], 0` per page of at
   most `P`, flags kept.
2. **AArch64.** LLVM's protocol: a caller leaves at most 1024 bytes
   below its lowest touch unprobed at a call. hod's frame record is at
   the bottom of the frame (§5.5), so its `stp` is the touch of the
   bottom page and the top is touched first. A `FRAME` with `FRAME + 1024
   <= P` is the unprobed prologue. Otherwise, with `k = (FRAME - 16) / P`
   and `r = (FRAME - 16) % P`: for `k <= 4` (`llc`'s unroll limit) `str
   xzr, [sp, #-16]!` (CFA `sp + 16`), `k` times `sub sp, sp, #P` · `str
   xzr, [sp]` (CFA `sp + 16 + iP` after each `sub`), then `r` in one or
   two `sub`s (CFA `sp + FRAME`). For `k > 4` the loop runs under a
   temporary frame record (WP-194): `stp x29, x30, [sp, #-16]!` (the top
   touch; CFA `sp + 16`, `x29` and `x30` at CFA `- 16` and `- 8`) · `mov
   x29, sp` (CFA `x29 + 16`) · `sub x17, sp, #kP` · `1: sub sp, sp, #P` ·
   `str xzr, [sp]` · `cmp sp, x17` · `b.ne 1b` · `r` in one or two `sub`s
   (no row: the CFA does not follow `sp`) · `ldr x29, [x29]` (the
   caller's `x29` back; CFA `sp + FRAME`). Either way `stp x29, x30,
   [sp]` follows as always, and from it the usual rows. With the CFA on
   `x29` an unwinder that emulates frame 0 instead of reading its rows
   (lldb) follows the loop; with it on `sp` or `x17` it lost one page per
   pass. Nothing reads the temporary record after the usual `stp`, so the
   local area's top 16 bytes, which it overlaps, are not reserved;
   nothing is written below `sp`. `x17` because `x16` may hold the
   incoming base and `x9`..`x15` are `preserve_most`'s. The prologue
   reserves 36 bytes more. A dynamic
   allocation: `1: sub sp, sp, #P` · `cmp sp, x16` · `b.ls 2f` · `str
   xzr, [sp]` · `b 1b` · `2: mov sp, x16` · `ldr xzr, [sp]`, `llc`'s loop.
   An outgoing argument area of more than 1024 bytes: `sub sp, sp, #c` ·
   `str xzr, [sp]` per page of at most `P`. `llc` lays the frame out the
   other way round (frame record on top, locals below, the rest probed
   above 1024 bytes); the loop decision is the same.
3. **Refused by name:** `attr:probe-stack-function`,
   `attr:stack-probe-size-value` (`spec/ir.md` §5.7).

Evidence: `spec/landed/codegen/WP-186.md`.

#### 5.6.28 Inline assembly with a template (WP-187)

A call or `invoke` of inline assembly whose template is not empty is
`crates/hod_x64/src/barrier/template.rs` (hook: `barrier::asm`); the
template is substituted (`hod_asm::template`, `hod_asm::x64::print_operand`)
and assembled (`hod_asm::x64::assemble`, `spec/asm.md` §10) before
anything is emitted, so a template that refuses leaves no code. The
sequence:

1. Every input placed in a register or memory, and every pointer of an
   indirect operand, is evaluated and stored to a 16-byte slot of its
   own (one per distinct value).
2. Every register value is displaced; every holdable register the
   template may write (operand registers, clobbers) is taken. A taken
   register is a used one, so the prologue saves `rbx` and `r12`..`r15`
   among them (§5.6.15); `rax`, `rcx`, `rdx`, `r10`, `r11` and the SSE
   registers hold no value across an instruction.
3. Inputs are loaded into their registers (`movzx` for 1 and 2 bytes,
   the 32-bit `mov` for 4, so an integer is zero-extended to 32 bits at
   least; `movss`/`movsd`/`movups` for SSE); a memory operand's address
   is put in its register (`lea` of the slot, or the pointer loaded).
4. The template's bytes, with a relocation at each symbol field
   (`spec/asm.md` §10.2 "Symbols") against the module's symbol of that
   name (another name is `asm:symbol`).
5. Register outputs are stored to their slots (`mov`, flags kept); flag
   outputs are `setcc r11b`, `movzx`, stored; indirect register outputs
   are copied from their slots through their pointers (`r11`, the value
   in `r10` or `xmm15`).
6. The result is loaded from its slot, or the struct of several results
   copied field by field; an `invoke` continues at its normal
   destination.

Only frame accesses (`rbp`-relative, `rbx`-relative in a coroutine body)
and the operand registers are touched between 3 and 5 besides the
template, so `push`/`pop` inside a template keep the frame valid (the
CFA is `rbp`-based throughout, §5.6.15).

**Registers.** Every operand gets a register of its own, distinct from
every other operand's and every clobber, except a tied input, which is
its output's register; this satisfies `&` always. General registers in
the order `rax`, `rcx`, `rdx`, `rsi`, `rdi`, `r8`..`r11`, `rbx`,
`r12`..`r15` within the code's class (`Q`: `rax`..`rbx`; `R`: the
legacy eight); SSE registers from `xmm0`. `{reg}` is that register;
`rsp`, `rbp` operands or clobbers are `x64:inline-asm-frame-register`;
a template that may write `rbx` in a coroutine body (its frame base,
WP-137) is `x64:inline-asm-frame-base`.

**Choice.** Of several alternatives, `llc`'s by weights (as §5.6.8, with
the ranged immediates weighing 3 when the constant fits). Within one, as
`llc`'s `ComputeConstraintToUse`: a flag output; an explicit register; a
tied input's output register; an immediate code (or `X`) for a constant
integer in range (a constant out of every range of an immediate-only
alternative is `asm:constraint/range`); memory (`m`, `o`) unless the
operand is a direct output or an output an input is tied to (`rm` on an
input is memory, as `llc` takes it); otherwise a register (`X` of a
floating-point or vector value: SSE).

**Types.** A general register holds `i8`, `i16`, `i32`, `i64`, `ptr`,
`float`, `double`; an SSE register `float`, `double`, `i32`, `i64`,
`i128`, `fp128` and vectors of 4, 8 or 16 bytes; a tied pair must have
the same size; a flag output is an integer. Otherwise
`x64:inline-asm-operand-type`. More than 16 operands, 4 alternatives,
or the registers of a class: `x64:inline-asm-operands`.

**Refused.** `inteldialect` (`x64:inline-asm-intel`), `unwind` on an
`invoke` (`x64:inline-asm-unwind`), a bundle (`x64:inline-asm-bundle`),
a `callbr` with a template (`x64:inline-asm`), `asm goto` labels in a
call, a symbol outside a memory operand (`asm:operand/symbol`), and
`hod_asm`'s `asm:*` names.

Evidence: `crates/hod_asm/tests/llvm_mc_x64.rs`,
`crates/hod_x64/tests/inline_asm.rs` (execution against `llc` 19 and
22 under the emulator; the `forced_*` templates' bytes, and the
templates' relocations, equal to `llc`'s),
summarized in `spec/landed/codegen/WP-187.md`.

#### 5.6.29 x86 system and state intrinsics (WP-190)

The `llvm.x86.*` calls clang 19 and 22 write at `-O0` for the functions
of the non-vector headers: `<ia32intrin.h>`, `<fxsrintrin.h>`,
`<xsave*intrin.h>`, `<immintrin.h>`'s `_rdrand*`/`_rdseed*`/`_readfsbase*`
/..., `<adcintrin.h>`/`<adxintrin.h>`, `<bmiintrin.h>`, `<bmi2intrin.h>`,
`<crc32intrin.h>`, `<clflushoptintrin.h>`, `<clwbintrin.h>`,
`<rtmintrin.h>`, `<xtestintrin.h>`, `<pkuintrin.h>`,
`<serializeintrin.h>`, `<cetintrin.h>`'s shadow-stack reads
(`crates/hod_x64/src/x86/sys.rs`; 67 `hod_ir` `Intrinsic` members, matrix
rows `never(X64_ONLY)` / `on(X86_SYS)`). A new section after §5.6.22,
not a part of it: the lowerings share §5.6.22's signature rule
(`x64:x86-intrinsic-signature`) and its feature rule, nothing else. A
`{ iA, iB }` result is a `Mem`-class value (§4.1): its fields are stored
to the result's slot.

1. **Baseline** (no feature check, as `llc` builds them on any x86-64):
   `flags.read.u64` is `pushfq; pop r`, `flags.write.u64` `push r;
   popfq` (no red zone, and the CFA is `rbp + 16` throughout, so the
   eight bytes below `rsp` are free and the unwind rows do not change);
   `rdtsc` and `rdpmc` (`ecx` = the operand) leave `edx:eax`, combined by
   `shl rdx, 32; or rax, rdx`; `rdtscp` stores that and `ecx` (the
   `TSC_AUX`) as the pair's fields; `addcarry.32`/`.64` and
   `subborrow.32`/`.64` make CF from the carry byte (`add cl, -1`: set
   when it is not 0), then `adc`/`sbb`, the carry out by `setb`.
2. **Feature-gated**, each only where the function's `target-features`
   enable the feature clang requires for its header function (§5.6.22's
   rule: the last item that turns it on or off decides, none is off, a
   `target-cpu` alone does not count). Otherwise refused by `intrinsic:`
   and its LLVM name, also where `llc` 22 would select the instruction
   anyway (`crc32`, `xgetbv`, `xsetbv`, the BMI ones, `rdssp*`,
   `incssp*`, `rdpkru`): hod does not write an instruction the function
   does not declare.

   | intrinsics | feature | instruction |
   |---|---|---|
   | `fxsave`, `fxsave64`, `fxrstor`, `fxrstor64` | `fxsr` | at the pointer |
   | `xsave`, `xsave64`, `xrstor`, `xrstor64`, `xgetbv`, `xsetbv` | `xsave` (or `xsaveopt`/`xsavec`/`xsaves`, which imply it) | mask high in `edx`, low in `eax`; `xgetbv`/`xsetbv` index in `ecx` |
   | `xsaveopt`, `xsaveopt64` | `xsaveopt` (off with `-xsave` too) | as `xsave` |
   | `xsavec`, `xsavec64` | `xsavec` (as above) | as `xsave` |
   | `xsaves`, `xsaves64`, `xrstors`, `xrstors64` | `xsaves` (as above) | as `xsave` |
   | `rdrand.16`/`.32`/`.64` | `rdrnd` | into `rax`, then `setb`: the flag field 1 when a number was delivered (the hardware zeroes the value otherwise, which `llc`'s `cmovb` relies on) |
   | `rdseed.16`/`.32`/`.64` | `rdseed` | as `rdrand` |
   | `clflushopt`, `clwb` | `clflushopt`, `clwb` | at the pointer |
   | `bmi.bextr.32`/`.64` | `bmi` | VEX, the control in `vvvv` |
   | `bmi.bzhi.*`, `bmi.pdep.*`, `bmi.pext.*` | `bmi2` | VEX; `pdep`/`pext` the mask in `r/m` |
   | `sse42.crc32.32.8`/`.32.16`/`.32.32`/`.64.64` | `crc32` (or `sse4.2` and every later SSE or AVX level) | `crc32` into the first operand's register |
   | `rdfsbase.*`, `rdgsbase.*`, `wrfsbase.*`, `wrgsbase.*` | `fsgsbase` | the 32- or 64-bit form |
   | `xbegin`, `xend`, `xabort`, `xtest` | `rtm` | `xbegin` is `mov eax, -1; xbegin` to the next instruction (an abort resumes there with the status in `eax`); `xabort` of its `immarg`; `xtest` then `setne`, zero-extended |
   | `rdpid` | `rdpid` | the 64-bit register form, its low half |
   | `rdpkru`, `wrpkru` | `pku` | `ecx` (and `edx`) zeroed |
   | `serialize` | `serialize` | |
   | `incsspd`/`q`, `rdsspd`/`q` | `shstk` | `rdssp` on the operand's register: without shadow stacks a `nop`, the result is the operand |
3. **Refused by name** (`classify`, `spec/ir.md`): every other name the
   headers write, among them `cmpccxadd*`, `enqcmd*`, `invpcid`,
   `directstore*` (`_directstoreu_*`)/`movdir64b`, `monitorx`/`mwaitx`, `ptwrite*`, `aadd*`/
   `aand*`/`aor*`/`axor*`, `rdpru`, the `uintr` ones (`senduipi`,
   `testui`, `clui`, `stui`), `urdmsr`/`uwrmsr`, `umonitor`/`umwait`/
   `tpause`, `wbinvd`, `wbnoinvd`, `xresldtrk`/`xsusldtrk`,
   `clrssbsy`, `rstorssp`, `saveprevssp`, `setssbsy`, `wrss*`/`wruss*`,
   `cldemote`, `clzero`, the `lwp` ones (`llwpcb`, `slwpcb`, `lwpins*`,
   `lwpval*`) and `tbm.bextri.*` (clang 19 only). AArch64
   refuses every `llvm.x86.*` name.

Evidence: `spec/landed/codegen/WP-173.md` for the rules this reuses,
`spec/landed/codegen/WP-190.md` for these.

#### 5.6.30 The hardening pieces together (WP-194)

Stack protectors (§5.6.24), branch tracking or protection (§5.6.25,
§5.6.26) and stack probing (§5.6.27) compose with each other, with a
dynamic area, `byval` and stack arguments, `nest`, `musttail`,
`returns_twice`, `invoke` and varargs, and on x86-64 with
`preserve_most`/`preserve_all`. Each piece keeps its own place and its
own registers, so no combination is refused for being one.

1. **x86-64 prologue:** `endbr64` (`cet::entry`, first) · `push rbp` ·
   `mov rbp, rsp` · the pushes of the preserved set (§5.6.23) · the save
   area and `sub rsp, FRAME`, written in `end` as the probed allocation
   (its 105 reserved bytes cut with the rest) · the guard's entry store
   and the low guard (`guard::reserve`, `Local(0)`) · the `xmm` saves of
   `preserve_all`. Epilogue: the guard check · the `xmm` reloads · `lea
   rsp, [rbp - 8n]` · the pops · `ret`. Registers: the guard's entry
   uses `r10` and `r11`, its check `r10`; the probe loop and the dynamic
   loop use `r11`. None of them is preserved by any convention, and the
   preserved set is saved before any of them is written.
2. **AArch64 prologue:** `bti c` or `paciasp`/`pacibsp` at offset 0
   (`protect::entry`; its rows, `B key` at 0 and the negated RA state
   after the signing instruction, are written first in `end`) · `[mov
   x16, sp]` (the incoming base) · the allocation (§5.6.27 item 2: two
   `sub`s, or the probes, or the looped shape under its temporary
   record) · `stp x29, x30, [sp]` · `mov x29, sp` · the saves of
   `preserve_most` and the coroutine's `x19` · the incoming base and
   result slots · the guard's entry store. `guard::place` makes the
   guard block last in the local area, at its top; the low guard of a
   dynamic area is at `x29 - 16`. Epilogue: the guard check · `[mov sp,
   x29]` · `ldp x29, x30, [sp]` · `add sp` · `autiasp`/`autibsp` (the
   stack pointer is the entry's again) · `ret`, or the tail branch.
   Registers: `x16` (the incoming base, the dynamic loop's bound), `x17`
   (the probe loop's bound, the guard's scratch); the guard check saves
   `x16` with `str x16, [sp, #-16]!` (a tail branch through `x16`
   needs it), compares through both and reloads it, after the body and
   before anything is restored. `x30` is signed
   before the temporary record stores it, so the CFA the unwinder
   authenticates it against is the entry stack pointer in every row.
3. **Property notes:** the GNU property note is written from the module
   flags as alone (BTI, PAC, GCS on AArch64; IBT, SHSTK on x86-64), and
   is the same as `llc`'s for every flag set measured.

Evidence: `spec/landed/codegen/WP-194.md`.

#### 5.6.33 x86 vector intrinsics past SSSE3 (WP-202)

The `llvm.x86.*` calls clang 19 and 22 write at `-O0` for the functions
of `<smmintrin.h>`/`<nmmintrin.h>` (SSE4.1, SSE4.2), `<wmmintrin.h>`
(AES, PCLMUL), `<shaintrin.h>`, `<avxintrin.h>`, `<avx2intrin.h>`,
`<fmaintrin.h>`, `<f16cintrin.h>` and the 128- and 256-bit forms of
`<gfniintrin.h>` (`crates/hod_x64/src/x86/ext.rs`; 173 `hod_ir`
`Intrinsic` members, matrix rows `never(X64_ONLY)` / `on(X86_SSE)`). A
section of its own: it shares §5.6.22's signature rule
(`x64:x86-intrinsic-signature`) and feature rule, and §5.6.29's
"refused by name" for a function without the feature. With the feature
hod writes the instruction `llc` selects or instructions that compute
the same bits, so it writes SSE4, AES, SHA, GFNI and `VEX` instructions
there (§5.6.2's "SSE2 only" holds for every function and construct
without them: the code of the x86-64 baseline is unchanged).

1. **Features.** Each row needs the feature of clang's header function;
   the last item of the function's `target-features` that turns it on or
   off decides, none is off, a `target-cpu` alone does not count:

   | need | on by | off by |
   |---|---|---|
   | `sse4.1`, `sse4.2` | that level or a later one of §5.6.22's chain (`avx`, every `avx*`, `fma`, `f16c`, ...) | it, an earlier level, `sse`, `sse2` |
   | `avx` | `avx` or any `avx*`, `fma`, `f16c`, `xop`, `fma4` | as above, and `avx` |
   | `avx2` | `avx2` or any `avx*` but `avx` (AVX-512, AVX10, `avxvnni`, ...) | `avx2`, or what turns `avx` off |
   | `fma`, `f16c` | it, or an `avx512*`/`avx10*` feature | it, or what turns `avx` off |
   | `aes`, `pclmul` | it, or `vaes`, `vpclmulqdq` | it, `sse`, `sse2` |
   | `sha`, `gfni` | it | it, `sse`, `sse2` |

   The 256-bit GFNI forms need `gfni` and `avx`. (`aes`, `pclmul`,
   `sha` and `gfni` are not judged by §5.6.2's feature check yet: a
   module that names them is refused there before code generation.)
2. **128-bit vectors** (class `Fpr`): the legacy encoding in place on
   the first operand's register, also where AVX is on and `llc` writes
   the `VEX` form (the same lanes; the upper half of a `ymm` holds none
   of hod's values): `sse41.packusdw`, `phminposuw`, `ptestz`/`c`/`nzc`
   (`ptest`, then `sete`/`setb`/`seta` zero-extended), `round.ps`/`pd`/
   `ss`/`sd`, `dpps`, `dppd`, `insertps`, `mpsadbw`; `pclmulqdq`;
   `aesni.aesenc`/`aesenclast`/`aesdec`/`aesdeclast`/`aesimc`/
   `aeskeygenassist`; `sha1rnds4`, `sha1nexte`, `sha1msg1`/`2`,
   `sha256msg1`/`2`; `vgf2p8affineqb.128`, `vgf2p8affineinvqb.128`,
   `vgf2p8mulb.128`. An `immarg` is the immediate (its low byte). Built
   otherwise:
   - `sse41.blendvps`/`blendvpd`/`pblendvb` (the instruction reads its
     mask from `xmm0`): the sign of each mask element spread to all its
     bits in `xmm14` (`psrad 31`, and `pshufd 0xf5` for `pd`; `pcmpgtb`
     against zero for bytes), then `b & m | a & ~m` by `pand`/`pandn`/
     `por`.
   - `sse42.pcmpistr*128` and `pcmpestr*128`: the lengths moved to `eax`
     and `edx`; `pcmpistri`/`pcmpestri` leave the index in `ecx`, the
     `a`/`c`/`o`/`s`/`z` forms the flag (`seta`, `setb`, `seto`, `sets`,
     `sete`); `pcmpistrm`/`pcmpestrm` write `xmm0`, which is kept in
     `xmm15` around them. `eax`, `ecx`, `edx` never hold values.
   - `sha256rnds2`: `xmm0` saved to a slot, the third operand moved
     into it, the instruction on `xmm14`, `xmm0` reloaded.
   - The AVX-only 128-bit forms are their `VEX.128` instruction:
     `avx.vpermilvar.ps`/`pd`, `avx.vtestz`/`c`/`nzc.ps`/`pd`,
     `avx2.psllv`/`psrlv` `.d`/`.q`, `psrav.d`, `avx.maskload`/
     `maskstore.ps`/`pd`, `avx2.maskload`/`maskstore.d`/`q`,
     `fma.vfmaddsub.ps`/`pd` (`vfmaddsub213`), `vcvtps2ph.128`.
   A third vector operand is copied to `xmm15` and its register given
   back before the other two are asked for: no lowering pins three
   registers of the class.
3. **256-bit vectors** (class `Mem`, the bytes in a frame slot, §4.1,
   `spec/abi-x64.md` §1.3; no value of hod's is ever kept in a `ymm`):
   the first operand loaded into `ymm14` by `vmovdqu`, the `VEX.256`
   instruction with the second operand from its slot (a 128-bit shift
   count from its register, a third operand loaded into `ymm15`), and
   `ymm14` stored to the result's slot by `vmovdqu`; an `i32` or 128-bit
   result is moved from `eax`/`xmm14` to its register. The instructions
   are `llc`'s, one per intrinsic: `avx.addsub`/`max`/`min`/`hadd`/
   `hsub` `.pd.256`/`.ps.256`, `blendv.*.256`, `dp.ps.256`,
   `cmp.ps.256`/`.pd.256` (the five-bit predicate), the conversions
   `cvt.pd2.ps.256`, `cvt.ps2dq.256`, `cvt(t).pd2dq.256`,
   `cvtt.ps2dq.256`, `rcp.ps.256`, `rsqrt.ps.256`, `round.*.256`,
   `vpermilvar.*.256`, `vtest*.256`, `ptest*.256`, `movmsk.*.256`,
   `ldu.dq.256` (`vmovdqu`), `maskload`/`maskstore` `.256`; `avx2.pack*`,
   `pavg.b`/`.w`, `pblendvb`, `phadd`/`phsub` `.d`/`.w`/`.sw`,
   `pmadd.ub.sw`, `pmadd.wd`, `pmovmskb`, `pmul.hr.sw`, `pmulh.w`,
   `pmulhu.w`, `psad.bw`, `pshuf.b`, `psign.*`, `psll`/`psrl`/`psra` by
   a vector and the `i` forms by an `i32` (a constant the immediate,
   saturated at 255, a variable one through `movd xmm15`, as §5.6.22),
   `psllv`/`psrlv`/`psrav` `.256`, `permd`/`permps` (the index operand
   in `ymm14`: `vpermd` takes it in `vvvv`), `mpsadbw`, `maskload`/
   `maskstore` `.256`; `fma.vfmaddsub.*.256`; `vcvtps2ph.256`;
   `vgf2p8affineqb.256`, `vgf2p8affineinvqb.256`, `vgf2p8mulb.256`.
   `avx.vzeroupper` and `avx.vzeroall` are `vzeroupper`: hod keeps no
   value in an upper half, and `vzeroall` would clear the `xmm`s that
   hold values (`llc` keeps its live ones in memory around it); what
   the registers hold is not observable by the program.
4. **§5.6.22's compares with AVX.** `sse.cmp.ss` and `sse2.cmp.sd` with
   a predicate of 8 to 31 in a function with AVX are `vcmpss`/`vcmpsd`
   with the whole predicate, as `llc`'s (the legacy encoding reads
   three bits, which is right only without AVX).
5. **Refused by name** (`classify`): the gathers (`avx2.gather.*`, 16
   names), every AVX-512, AVX10, VAES, VPCLMULQDQ and 512-bit GFNI name
   (counted in `spec/landed/codegen/WP-202.md`), and every other
   `llvm.x86.*` name not listed. AArch64 refuses every `llvm.x86.*` name.

Evidence: `spec/landed/codegen/WP-202.md`.

#### 5.6.35 AVX-512 and the masked memory intrinsics (WP-208)

Design: `docs/design/0007-avx512.md`. Evidence:
`spec/landed/codegen/WP-208.md`; tests `crates/hod_x64/tests/x86_avx512.rs`.

1. **The masked memory intrinsics** (generic; clang 19 and 22 write
   them for the masked loads and stores, expand loads and compress
   stores of the AVX-512 headers): `llvm.masked.load`,
   `llvm.masked.store`, `llvm.masked.expandload`,
   `llvm.masked.compressstore` (`crates/hod_x64/src/vector.rs`,
   `masked`; matrix rows `todo(LATER)` / `on(MASKED)`). Built on
   x86-64 whatever the function's features, lane by lane as `llc`
   scalarizes them without AVX-512: for each lane whose mask bit is
   set, the lane's bytes copied through `r11` between memory and the
   vector's image (the result's slot, or a scratch image of the
   pass-through reloaded into the result's register for a vector of
   class `Fpr`; the stored value's slot or image). `load`/`store` reach
   lane `i` at `ptr + i * size`; `expandload`/`compressstore` the `k`-th
   set lane at `ptr + k * size`. A lane whose bit is clear touches no
   memory. A constant mask copies its set lanes (an undefined lane is
   clear); a variable mask (class `Gpr`, §5.6.13) is tested lane by lane
   by `bt` and `jae`, the packed forms' cursor in `rax`. Both
   signatures are read: LLVM 19's (`i32` alignment operand of `load`
   and `store`) and LLVM 22's (the pointer's `align` attribute); the
   alignment is not needed, every access being one lane. Refused: a
   mask of more than 64 lanes (`x64:mask-wide`), what §5.6.13 refuses
   for the vector's lanes, and a call whose operands are not the
   intrinsic's (`x64:vector-op`). AArch64 does not build them yet.
   Execution evidence: on the emulator against `llc` 22 and 19.

## 6. Control flow

### 6.1 Reachability

Before emission, one walk over terminators from the entry block marks the
reachable blocks (successors include an `invoke`'s unwind destination and
every destination an `indirectbr` lists). **Unreachable blocks are not
emitted.** LLVM exempts them from the dominance rule, so they may contain
uses with no definition on any path; skipping them removes that case
instead of handling it. Having its address taken does not make a block
reachable: an `indirectbr` lists every block it may jump to, so a block
that no reachable terminator names cannot be entered. A label that must
exist for an unreachable block (its address is taken) is bound to a trap
at the end of the function; all unreachable blocks share one trap.

### 6.2 Labels and fixups

Each block has a label: bound to a code offset when the block is emitted,
and until then the head of a list of fixups `(offset, kind)` for branches
that refer to it. Binding a label patches its list
(`Target::patch_branch`); a branch to a bound label is encoded directly.
The same mechanism serves the epilogue label and labels local to a
lowering. Block layout order is emission order; a branch to the block
emitted next is omitted.

`switch` is lowered to compares and branches (a linear chain for few
cases, a binary search otherwise; the threshold is the target's). There
are no jump tables in v1, so a function's code needs no relocation against
itself.

### 6.3 Branch range

A target's short branch forms may not reach across a very large function
(AArch64 conditional branches reach ±1 MiB). If a fixup cannot be patched,
the function's compile fails with an internal "branch out of range" flag
(not a `CgError` variant: the public error keeps two, §10), the
driver truncates the chunk (§1) and compiles the function again with
`FnCx::far` set; a target must then emit only branch forms that reach the
whole function (AArch64: an inverted short branch over an unconditional
`b`). The same holds for every other reference a function makes to its
own code without a relocation: a load from a data island (§7.5) and the
address of a block (§9.6) are fixups too, fail the same way, and in far
mode are formed from the address of the referring instruction plus a
materialized distance. A function that fails in far mode is
`Unsupported("limit:function-size")`.

## 7. Calls, symbols and constants

### 7.1 The call sequence

For `Call` and `Invoke`, in this order:

1. (`Invoke`) write the unwind destination's incoming slots (§4.5).
2. Displace registers (§4.4).
3. Ask the target's argument assigner (§7.2) where each argument goes.
   Lower the stack pointer if anything goes on the stack. Place stack
   arguments, then register arguments. An indirect callee's address is
   brought into a scratch register last.
4. Emit the call instruction, with its relocation for a direct callee, and
   record the call site (§9.4): the code range of the instruction and,
   for an `invoke`, the landing pad.
5. Raise the stack pointer again if step 3 lowered it. Take the result
   from the return registers (§4.4), or copy it into the result's home for
   a `Mem` class.

6. (`Invoke`) write the normal destination's incoming slots and branch to
   it; then, if step 3 lowered the stack pointer, emit the pad stub
   (§5.3).

A call marked `noreturn` is followed by the trap instruction.

**Effective attributes.** A call passes its arguments and takes its
result with its *effective* attributes, which `classify` reads too, both
through `hod_ir::call_arg_attrs` and `call_ret_attrs` (one rule in one
place; `hod_ir/src/classify/attrs.rs`). They are LLVM's as `llc` lowers a
call (`CallBase::paramHasAttr` and `hasRetAttr`, which consult the callee
through `getCalledFunction`, and that is null when the call's function
type is not the callee's), measured with `llc` 19 and 22
(`-O0 -global-isel=0 -fast-isel=0`) on AArch64:

| call | effective attributes of an argument or the result |
|---|---|
| indirect, of an alias, or direct through a function type other than the callee's (another parameter list, variadic against fixed either way) | the call site's alone |
| direct through the callee's own function type | per attribute: a flag (`zeroext`, `signext`, `inreg`, `nest`, `swiftself`, `swifterror`, `swiftasync`) if either side has it, `signext` winning over `zeroext`; the typed attribute (`sret`, `byval`, …) of the call site if it has one, else the declaration's, with its type; `align` and `alignstack` of the call site alone |

So `declare void @g(ptr byval([5 x i64]) align 8)` called as `call void
(ptr, i64) @g(ptr %p, i64 1)` passes a pointer and an `i64` in `x0` and
`x1`; called as `call void @g(ptr align 8 %p)` it passes a copy (the
declaration's `byval`, although the site has an attribute of its own),
placed by the call site's alignment, not the declaration's `align 16`.
A typed attribute that changes passing (`sret`, `byval`, `inalloca`,
`preallocated`) on one side and a different one on the other is refused
as `call:attribute-conflict` (`llc` passes `sret` on AArch64, whichever
side has it; hod does not encode that precedence); `byref` and
`elementtype` change nothing about passing and yield to the other side.
Function attributes are not part of this rule (`returns_twice` and
`nounwind` are read from a direct callee whatever its type, as LLVM does).

`FnCx::place_return` places a `ret`'s value in the return registers; a
value returned in memory (`ret_memory`) is the target's to copy.

**Extended integers.** An integer argument or result with `zeroext` or
`signext` is extended as `abi-<target>.md` asks; for AArch64, §7 there.
The sequence calls `Target::extend_arg` for the part that holds the
integer's last piece, in its register, on both sides of a call and in
`place_return`: for an integer no wider than a general register the
register holds the whole value; for a wider one whose width is no
multiple of the register's, it holds the last piece, and `extend_arg`
extends that piece's `N mod 64` bits to the register's width. A last
piece on the stack is extended through a scratch register and stored as
a whole register, and so is any extended integer that a general
register would carry, so that its slot holds the extended value
whatever its width. A result returned in memory (`ret_memory`) is its
image alone. The incoming side extends nothing: it may assume what the
target's table promises.

**Odd leaves of incoming `Mem`-class arguments.** The ABI promises
nothing above an integer leaf of an aggregate or above an integer wider
than a register without an extension attribute (`abi-a64.md` §7), but a
`Mem`-class value lives in memory, where §2.2 has the bits above every
width zero. So once an incoming `Mem`-class argument's parts are in its
home (gathered, or its incoming stack location), every integer leaf
whose width is not a multiple of 8 is cleared there: its bytes, or for a
leaf wider than a register the bytes of its last piece, are loaded into
a scratch register, extended by zero through `Target::extend_arg`, and
stored back with their own size. Elements of vectors are not leaves.
The callee may write its incoming area (`abi-a64.md` §8.1). A call's
`Mem`-class result is the target's to clear (AArch64: `call.rs`).

**Every call instruction in a function's code is emitted by this
sequence** and so has a call-site row, including the calls of runtime
functions (§7.3) that lowerings of other instructions make (`Resume`'s
`_Unwind_Resume`, a `memcpy`): a call that throws from a function with
landing pads and is covered by no row terminates the program (§9.4).
`FnCx` offers the sequence as a service (§8.1). The one call instruction
a lowering emits itself is the one inside a TLS access sequence that the
psABI fixes byte for byte (the linker has to recognize it to relax it):
the lowering displaces what the sequence clobbers and records the row
through the same service.

**Tail kinds.** `tail` and `notail` are permissions and are ignored:
hod makes no tail call but a `musttail` one, in any function (where `llc
-O0` turns a `tail` call into a jump, hod calls). A function's
`"disable-tail-calls"="true"` (WP-179) therefore changes nothing: LLVM's
rule for it, no tail call except a `musttail` one, is hod's rule
everywhere; `"false"` is the default.
`musttail` is an obligation (variadic thunks depend on it,
`docs/design/0001-input.md`). Control transfers by a branch, with the
frame gone and the arguments where the *caller's own* incoming arguments
were:

1. Displace registers (§4.4).
2. Stack arguments overwrite the caller's incoming area, which may hold
   the homes of the very values being passed (§4.8), so §4.4's "disjoint"
   does not hold. All stack arguments are first placed in a **staging**
   slot in the local area, laid out as the callee's stack-argument area;
   then the staging slot is copied over the incoming area. (`classify`
   guarantees it fits, §1.)
3. Register arguments are placed; an indirect callee's address goes into
   a scratch register that the exit sequence does not use.
4. Branch to a **tail exit** for this call, which `end_function` emits
   after the epilogue (§5.4): the teardown, then a branch to the callee.
   No call-site row is recorded: nothing returns here.

The rest of the block (an optional cast of the result and the `ret`) is
not emitted. In a variadic function the argument registers that carry no
named argument are passed through unchanged; the target's ABI file states
how.

**The `musttail` hook.** `classify` refuses a `musttail` call whose
stack-argument area is larger than its caller's (`musttail:stack-args-
grow`), or whose areas it cannot learn (`musttail:stack-args-unknown`,
`spec/ir.md` §8.6); only `assign` knows them. `hod_cg::abi::
TargetStackArgs::new(&target)` is that `hod_ir::StackArgs` hook for any
`Target`, handed to `Classifier::with_stack_args`:

- `of_function`: `assign` of the function's own signature (its symbol's
  convention, return and parameter attributes, its type; the named
  parameters only, also for a variadic function), as the entry assigns
  it (§4.8); its `stack_size`.
- `of_call`: `assign` of the call with its effective attributes (above),
  its convention and its function type; its `stack_size`. The variable
  arguments a variadic call lists (before a forwarded `...`, which the
  call site does not list) are typed by their values in the call's body
  (`CallSite::body`, `Body::value_type`), as the call sequence types
  them (WP-115). A call listing more arguments than a non-variadic type
  names, or fewer than it names, has no answer.
- No answer when `assign` refuses the signature.

The sizes are compared as `assign` gives them; the call sequence's own
check (step 2) allows the caller's area rounded up to `STACK_ALIGN`, so
`classify` is the stricter. A `musttail` call of a function returned in
memory without `sret` stays `call:musttail-ret-memory`, which only the
call sequence sees. The driver side (staging, the copy over the incoming
area, the branch to a tail exit) is the target-independent sequence
above; a target owes the tail exits after its epilogue
(`FnCx::tail_exit`; AArch64 `frame.rs`) and, in a variadic function, the
pass-through of the variable-argument registers (AArch64 `call.rs`,
`abi-a64.md` §8.5). AArch64 has both (WP-038).
The facade hands the hook to its classifier for both targets
(`crates/hod/src/session.rs` `classifier_for`): AArch64 with
`TargetStackArgs::new(&target)`, x86-64 with
`TargetStackArgs::per_function(&target, hod_x64::call::target_of)`,
because `X64::assign` depends on the target features of the function
making the call (`spec/abi-x64.md` §8.5, WP-148). A classifier without
the hook refuses every `musttail` call it must size as
`musttail:stack-args-unknown`.

### 7.2 Argument assignment

`Target::assign` maps a signature (function type, calling convention,
return and parameter `ParamAttrs`, variadic-call flag and the number of
fixed parameters) to a list of **parts**: `(argument index, byte offset in
the value, size, where)` with `where` a register or a stack offset, plus
the same for the return value, the total stack size, and, for a variadic
callee, what `va_start` needs. The same function serves both sides: a
call's outgoing placement and a function's incoming homes (§4.8).

Its rules are the target's ABI as LLVM applies it to IR-level types, so
that hod-compiled and LLVM-compiled code interoperate in both directions.
They are specified in `abi-<target>.md` and tested by layer L5
(`docs/VALIDATION.md`). The frontend has already applied the C++ ABI
(`sret`, `byval`, coerced aggregates); the assigner never looks at a
source-level type.

Signatures (`hod_cg::abi`, fixed ahead of the code generator so that the
ABI work packages can proceed independently): `Sig { cc, ret, ret_attrs,
params, param_attrs, fixed, variadic }`; `Part { arg, kind, offset, size,
place }` with `place` a `Place::Reg(PReg)` or a `Place::Stack(offset)`
measured from the stack pointer at the call, and `kind` either `Value`
(bytes of the argument's value) or `Pointee` (bytes of the memory a
`byval` argument points to, copied into the stack-argument area) or
`Lanes { bits, lane, first, len }` (elements of a vector of `len`
elements of `bits` bits each, element `first + i` zero-extended in lane
`i` of the place, `lane` bytes each; the consumer reads only the
element's bits; WP-082). The call sequence never copies a `Lanes` part
as bytes: it hands it to the target's `lanes_out(part, from, to)` and
`lanes_in(part, from, to)`, each side a `LaneLoc` (a register, or a
frame place: the stack slot, or the vector's image in a `Mem` value);
a register value is gathered in (or produced from) the scratch register
of its own class: a `Gpr` mask in the general one, an `Fpr` value in
the vector one (WP-167: AArch64's lone `<1 x i128>`, a `Q` register
whose two 64-bit limbs travel as `Lanes { bits: 64, lane: 8, len: 2 }`
parts, `first` the limb). A target's hook refuses with
`call:part-lanes` a register side of a class it does not move (x86-64:
a value side that is not a general register, a place that is not an
`xmm`; AArch64: a mask's or promoted vector's value side in a vector
register), so no register is read as another class. The
default hooks refuse with `call:part-lanes`;
`Assignment { args, ret, ret_memory, sret_returned_in, stack_size, va,
vector_regs_used }`. `ret_memory` is set when a return value does not fit
the return registers and the signature has no `sret` parameter: the
caller then passes the address of a buffer, as LLVM does. A target's
`assign(types, sig, out) -> Result<(), AbiUnsupported>` names what it
cannot pass (`abi:<what>`); the facade runs it over a module's signatures
as part of deciding support, so code generation never meets a failure.

#### The leaf walk (WP-117)

The part of assignment every target shares is in `hod_cg::abi`; the
rules that place each piece stay the target's.

- A **leaf** is a value that is not an array or a structure. `walk`
  visits the leaves of a type with their byte offsets in offset order,
  to any depth: a structure's fields at `TypeTable::field_offsets`, an
  array's element `i` at `i` times the element's alloc size. An array
  whose element has no leaf (`shape` is `Empty`) is skipped without
  iterating it. `shape` tells whether all leaves of an aggregate have one
  LLVM value type (`leaf_key`: a pointer is an `i64`), in time
  proportional to the type's description.
- The **classification hook** is the trait `Classify`: `leaf(types, ty)`
  returns a `Leaf<Class>` (the target's class, `count` pieces of `size`
  bytes `stride` apart, `last_size` for the last, `split` marking the
  pieces that start an integer wider than a register, `lanes` for a
  vector widened to lanes) or the target's `abi:<what>` name.
  `check_type` refuses an aggregate with a refused leaf.
- `place_leaf(leaf, arg, offset, parts, place)` asks the target's
  `place(leaf, piece)` for each piece's `PieceLoc` (a register or a
  stack offset) and appends the part with `push`; `place` sees the whole
  leaf, so a pair kept whole (x86-64's `i128`) is decided at its first
  piece, and the target keeps its own state (next registers, stack
  offset, the vector-register count for `al`). `place_leaves` is
  `walk` + the hook + `charge` (the piece budget,
  `abi:too-many-pieces`) + `place_leaf` for one argument of any type.
- `push` merges a stack part into the previous one when both are of one
  argument and kind (never `Lanes`) and continue each other in the value
  and on the stack; an offset or end beyond `u32` is `abi:size-limit`.
- Where a return value goes, and whether it is demoted to memory, is the
  target's: its return `place` fails, and it clears the parts.

### 7.3 Runtime functions

Some instructions lower to calls of functions that are not in the module:
`memcpy`/`memmove`/`memset`, 128-bit division, `fp128` arithmetic, atomic
fallbacks, `fmod`, `_Unwind_Resume`, the TLS helpers. `RuntimeFn` is a
closed enum of them with each one's symbol name and signature. A reference
to one is `SymRef::Runtime` (§9.2); module emission maps it to the module's
own symbol of that name if the module declares one, and to a new undefined
global symbol otherwise, so that the object never has two symbols with one
name. A lowering calls one through `FnCx` (§8.1) with operands and a
result like an IR call's; the service runs §7.1, so registers are
displaced and the call site is recorded.

#### Runtime routines by type and target (WP-117)

Which routine an operation calls is a table in `hod_cg::sym`, keyed by
the type (`FpType`: `half`, `float`, `double`, `fp128`, `x86_fp80`) and
the target (`hod_ir::Arch`), so that every target asks one place. A table
answers `None` where the target computes the operation inline, or
refuses it by its own name. The x86-64 column is `llc` 22's
(`-O0 -fast-isel=0`, SSE2, no SSE4.1, no FMA); the AArch64 column is
`llc` 22's with `-global-isel=0`. `crates/hod_cg/tests/runtime_names.rs`
compares every row with `llc` 22 and 19 on both targets, from the IR in
`crates/hod_cg/tests/data/runtime/`.

```rust
pub fn fp_routine(op: FpOp, ty: FpType, arch: Arch) -> Option<RuntimeFn>;
pub fn fp_ext_routine(from: FpType, to: FpType, arch: Arch) -> Option<RuntimeFn>;
pub fn fp_trunc_routine(from: FpType, to: FpType, arch: Arch) -> Option<RuntimeFn>;
pub fn fp_to_int_routine(from: FpType, signed: bool, bits: u32, arch: Arch) -> Option<RuntimeFn>;
pub fn int_to_fp_routine(signed: bool, bits: u32, to: FpType, arch: Arch) -> Option<RuntimeFn>;
pub fn fp128_compare(pred: FloatPredicate) -> Option<Fp128Compare>;   // { func, swap, result: ZeroCmp }
pub fn int_divide_routine(op: Opcode) -> Option<RuntimeFn>;
```

- `fp128` is AArch64's `long double`: `sinl`, `fmodl`, `fmal`, ... On
  x86-64 it is `__float128`: `sinf128`, `fmodf128`, `fmaf128`. Its
  arithmetic is `__addtf3`, `__subtf3`, `__multf3`, `__divtf3` on both.
  `x86_fp80` is x86-64's `long double`: `sinl`, `fmodl`, `__powixf2`;
  its arithmetic, `sqrt`, `lrint` and `llrint` are x87 instructions. A
  `long double` routine (`...l`) is one `RuntimeFn` whose signature is
  written with `fp128`; on x86-64 the same symbol takes `x86_fp80`.
- `half` calls the `float` routine, widened first and the result
  narrowed, except on x86-64: `fma` calls `fma`, `minnum`/`maxnum` call
  `fminf`/`fmaxf` (which `float` does not), and `frexp`, `lround` and
  `llround` are no single call.
- On AArch64 `float` and `double` call only the libm functions (`sin`
  .. `log10`, `pow`, `atan2`), `frem` (`fmodf`/`fmod`), `powi`
  (`__powisf2`/`__powidf2`), `ldexp` and `frexp`; everything else is an
  instruction. On x86-64 the rounding functions (`floor` ..
  `nearbyint`), `fma`, `lround` and `llround` are calls too.
- Conversions: `fpext`/`fptrunc` with `fp128` are `__extend{sf,df}tf2`
  and `__trunctf{hf,sf,df}2`; on x86-64 `half` is `__extendhfsf2` /
  `__truncsfhf2` / `__truncdfhf2` (`__extendhftf2`, `__extendhfxf2`,
  `__truncxfhf2` with the long types), `x86_fp80` and `fp128` are
  `__extendxftf2` / `__trunctfxf2`. AArch64 widens `half` to `float`
  before `__extendsftf2` (exact; `llc` calls `__extendhftf2`). Integer
  conversions of `fp128` are `__fix{,uns}tf{si,di,ti}` and
  `__float{,un}{si,di,ti}tf` by the integer's width class (1 to 32, 33
  to 64, 65 to 128 bits); `i128` with the others is `__fix{,uns}{sf,df,
  xf}ti` and `__float{,un}ti{hf,sf,df,xf}`.
- `fp128` compares are `__eqtf2`, `__lttf2`, `__letf2`, `__unordtf2`,
  operands swapped for the reversed orderings, the result compared with
  0 (`ZeroCmp`); `ueq` and `one` are two calls (`None`, the target's).
- `udiv`, `sdiv`, `urem`, `srem` of 65 to 128 bits are `__udivti3`,
  `__divti3`, `__umodti3`, `__modti3` on both targets.
- hod's choice where `llc` has none: `minimum`, `maximum`, `minimumnum`
  and `maximumnum` of `fp128` and `x86_fp80` call the C23 functions
  (`fminimuml`, `fminimum_numl`, `fminimumf128`, ...; glibc 2.35+).

#### Memory intrinsics (WP-117)

`hod_cg::memplan::mem_plan(op, len) -> MemPlan` says what a `memcpy`,
`memcpy.inline`, `memmove`, `memset` or `memset.inline` of a constant
(`Some`) or variable (`None`) length becomes; the target emits it.

- A length of 0 is `Nothing` (the pointers may be null).
- A constant length of at most `INLINE_MAX` = 64 bytes (`memcpy`,
  `memset`) or `INLINE_MOVE_MAX` = 32 bytes (`memmove`) is `Inline`:
  straight-line accesses touching exactly the bytes of the range. An
  inline `memmove` reads every byte before it writes one, so overlap in
  either direction is correct; 32 is what two 16-byte registers and two
  scratch general registers hold. 64 keeps the inline code to a few
  loads and stores while covering the common small struct copies and
  clears; beyond it the library routine is as fast and the code stays
  small.
- Anything longer, and every variable length, is `Call` of `memcpy`,
  `memmove` or `memset` through `hod_cg`'s runtime-call sequence, so live
  values are saved as for any call; a variable length narrower than 64
  bits is zero-extended first.
- The `.inline` forms never call: a constant length above 64 is a
  `Loop`; a variable length is `VariableInline`, which the target refuses
  by its own name.

#### `switch` (WP-117)

`hod_cg::switch` plans a `switch` on a condition of at most 64 bits; the
target emits the compares and branches.

- Case values and the condition are compared zero-extended
  (`case_mask(bits)`).
- A constant condition goes to the first case of its value, else the
  default (`constant_target`): one `br`.
- `canonical_cases` drops the cases that go to the default, sorts the
  others by unsigned value (stable; skipped when they come sorted, so
  compile time is linear for a frontend's usual order and `n log n`
  otherwise) and keeps one case per value. Of two cases of one value
  (invalid IR), the first of those not to the default wins; a first one
  to the default is dropped before that, so the second is kept, where
  the constant fold above takes the default.
- No cases left is a `br` to the default. Otherwise the search
  (`search_step`): a run of at most `LINEAR_CASES` = 4 cases is a chain
  (compare with each in order, equal goes to its block, then the
  default); a longer one is a node at its middle case `len / 2` (equal
  goes to its block, above to the search of the upper half, else the
  lower half). Code is linear in the cases; a run does about
  `log2(n) + 4` compares; no jump table, so a function's code has no
  relocation against itself (§6.2).

#### Target features (WP-117)

`hod_cg::features::FeatureTable { required, available, contract }` reads
a `target-features` string or an `-mattr` item; the table is the
target's (AArch64: §5.5.2). Items are separated by `,`, empty items
skipped; each is `+name` or `-name`. In order:

1. `-name` of a `required` feature is refused with its reason.
2. A name in `available` is accepted with either sign.
3. A name `contract` gives a reason for is refused with it when `+`,
   and accepted when `-` (the default, which hod's code keeps).
4. Anything else, either sign, and an item without a sign, is
   `attr:target-feature-unknown` (`UNKNOWN`).

The first refused item of a string is the answer (`Refused { reason,
feature }`).

#### The lane loop (WP-129)

`hod_cg::lanes` is the loop of an element-wise operation on a vector, one
lane at a time, unrolled at emit time (AArch64's use: §5.5.1 "Vectors,
the lane loop"). A target implements `LaneKernels` and passes the scalar
operation of one lane as a closure; every decision below is the driver's.

**Shapes.** `lane_log2(elem, count)` decides which vectors have
addressable lanes, checking in this order: `i1` elements are a mask
(`Mask`, or `MaskWide` beyond 64 of them); more than `MAX_LANES` = 256
lanes is `Length`; an element wider than 64 bits is `WideElement`; an
integer of a width other than 8, 16, 32, 64 is `OddElement`; otherwise
the lane is 1, 2, 4 or 8 bytes (`Elem`: `i8`..`i64`, `ptr`, `half`,
`bfloat`, `float`, `double`). Lane `i` is at byte `i << log2` of the
vector's in-memory image (elements are packed). A target names each
`ShapeRefusal` itself and may refuse more (AArch64: `bfloat`
arithmetic, masks under the groups' names).

**The target's part (`LaneKernels`).**

- `type Lanes`: the target's view of a vector with lanes (what its
  moves need); `geometry(&Lanes) -> Geometry { count, log2, class,
  elem_class }` is all the driver reads of it. `class` is the vector's
  (`Fpr`: a register vector; `Mem`: a slot); `elem_class` a lane's as a
  scalar (`Gpr` for integers and pointers, `Fpr` for floating point).
- `SCRATCH_IMAGES`, `scratch(k)`: `k` frame areas of the function, each
  as large and aligned as the widest register vector, valid until the
  lowering ends (plain frame memory: they survive calls).
- `spill(reg, lanes, k)` / `reload(reg, lanes, image)`: a register
  vector to and from an image.
- `load_lane(lanes, at, rt)`: the lane at frame place `at` into `rt`,
  zero-extended into a general register, or the scalar view of a
  floating-point one. `store_lane(lanes, at, rt)`: the reverse, the low
  bytes only.
- `deferred_first(inst)`: the operands deferred into `inst` (§4.6)
  emitted before anything else.

None of these may leave a value in a pool register or expect anything of
the flags.

**`setup(inst, operands, lanes_of)`** (at most three operands): the
result's lanes, then each vector operand's (a scalar operand is not
asked: it is the same in every lane), every length equal to the
result's; nothing is emitted before these checks. Then
`deferred_first`, then the images (`images`, also used alone by a
lowering with its own per-lane code, as AArch64's masks): an operand
repeated shares its first image; a `Mem` vector's image is its slot; a
register vector is asked for, spilled to the next scratch image and
given back at once, so that the loop has the whole pool. The result's
image is its slot (`Mem`), else the next free scratch image, else the
first operand's image when its lanes are as wide as the result's (each
lane of it is read before the same lane is written).

**`each_lane(loop, takes, kernel)`**: for lane 0, 1, ... in order: for
each `(operand, class)` of `takes` in order, a new pool temporary of
`class` with that operand's lane loaded into it (a scalar operand put
whole); `kernel(lane, temporaries)` emits the operation and returns the
register that holds the result lane (one of the temporaries, or a
temporary of its own: a conversion to another class, a runtime call's
result); that register is stored as the result's lane; the result's
register and every taken temporary are given back. A kernel that makes
a call loses its temporaries there (§7.1) and returns the call's
result. The value of a lane is the scalar instruction's for it, poison
and undefined behaviour included: the driver adds no rule of its own.

**`finish(inst, lanes, dst)`**: a register-vector result is loaded from
its image into the result's register; a `Mem` result is already in its
slot.

**x86-64 (WP-132), the reasoning the trait is designed for.**

- *Two-operand instructions* (design 0006 §2.2): the kernel returns the
  register that holds the result, so `add t0, t1` returns `t0` and a
  non-commuting form that ends in the second operand returns `t1`;
  nothing in the driver assumes a three-operand `dst = a op b`.
- *Fixed registers*: `rax`, `rcx`, `rdx` are not in the pool and are
  never held (§5.6), so a kernel uses them freely (a variable shift's
  `cl`, a division's `rdx:rax`) without asking; the driver's
  temporaries are pool registers and never one of them.
- *Flags* (design §2.3): the driver emits nothing between kernel calls
  but lane moves, and keeps no flags live from one call to the next; a
  kernel sets and reads its flags within itself. `load_lane` and
  `store_lane` on x86-64 are `movzx`/`mov`/`movss`/`movsd`/`movd` with a
  frame operand, which leave the flags alone, but the driver does not
  rely on that.
- *Scalar floating point*: a lane of class `Fpr` is the low lane of an
  `xmm` temporary (`movss`/`movsd`; a `half` lane is 16 bits moved
  through a general register, `movzx` then `movd`), so the kernels are the SSE2 scalar
  lowerings of X-float; there is no third floating-point scratch like
  AArch64's `v31`, so a three-operand floating-point kernel (`fma`) takes
  three operands through `takes`.
- *Images*: a frame operand on x86-64 needs no scratch register for a
  far offset (a 32-bit displacement), so `load_lane`/`store_lane` write
  only `rt`; packed SSE2 operations are used instead of the loop where
  one instruction exists (design §2.6), and the loop for the rest.

#### Inline-assembly constraints (WP-129)

`hod_cg::asm_constraints::parse(syntax, constraints)` reads the
constraint string of an inline-assembly call into `Constraints`, with
the target's part behind `ConstraintSyntax`. AArch64's use is §5.5.3
"WP-103". The flags of the call (`sideeffect`, `alignstack`,
`inteldialect`, `unwind`) are `AsmFlags::of`.

**The string.** An empty string has no operands. Otherwise it is split
at `,`; each part, in order:

1. `~{name}` is a clobber: `memory` sets `Constraints::memory`;
   anything else goes to `syntax.clobber(name)`: `Ignored` (the flags
   register: AArch64 `cc`, `nzcv`; x86-64 `flags`, `dirflag`, `fpsr`),
   `Reg(r)` (into the `clobbers` bit set by `PReg::index`), `Unknown`
   (refused `Clobber(name)`), or the target's refusal. A clobber without
   braces is `Clobber(text)`.
2. Past `MAX_OPS` = 16 operands: `TooMany`.
3. `=` makes an output; `!` a label, which must be `!i` (else
   `Constraint(part)`) and has one alternative, the target's `i` code.
4. Then any of `*` (indirect: the operand is a pointer to the memory),
   `&` (early clobber), `%` (commutative), in any order.
5. Then the alternatives, separated by `|`; past `MAX_ALTS` = 4:
   `TooMany`. Each alternative is read code by code:
   - `{@cc<cond>}` in an output is a flag output: `FLAG`, with
     `syntax.flag(cond)`; an empty or unknown condition is
     `Constraint(name)`. In an input it is a register name (and so
     refused as one);
   - `{name}`: `syntax.operand_reg(name)`: a register (`SPEC`), no
     register (`Constraint(name)`), or the target's refusal (AArch64:
     a frame register, a scratch register); no `}` is
     `Constraint(rest)`;
   - digits in an input: the output it is tied to (`TIED`); a second
     tie in one alternative, or a number of `MAX_OPS` or more, is
     `TooMany`;
   - anything else: `syntax.letters(code, output)` gives the code bits
     (below bit 29: `SPEC`, `TIED`, `FLAG` are the parser's) and the
     length (x86-64 has two-letter codes such as `Yz`); none is
     `Letter(byte)`;
   - an alternative with no code is `Constraint("empty")`.
6. A direct output (`=` without `*`) is a field of the call's result,
   numbered in order (`field`); every other operand but a label is a
   call argument, numbered in order (`arg`).

Finally the order: the outputs, then the inputs, then the labels, and
every tie names an output; otherwise `Constraint("order")`. The first
refusal of the string, in reading order, is the answer.

**Choosing an alternative.** `Constraints::choose(args, weight)` is
`llc`'s choice: with fewer than two alternatives anywhere, 0; otherwise,
for each index below the greatest count, the sum over the operands of
`weight(operand, codes)` of the operand's alternative at that index (the
first when it has fewer), where an operand weighing `None` excludes the
index; the first index of the greatest sum wins, 0 if none is possible.
The weights are the target's (AArch64: immediate 3, memory 2, a
register class 1 for a value it holds, the best code of an alternative).

**What stays the target's.** The register a place gets, the template's
substitution and assembly, the sequence around it, and the meaning of
each letter. On x86-64 the letters `a b c d S D` name one register each
(through `letters` they are codes the target places as it places
`{name}`), `A` is `rdx:rax`, `q`, `Q`, `R`, `l` restrict the general
registers, `x`/`v` are `xmm`, `f`/`t`/`u` the x87 stack (refused while
x87 is memory-only, design §2.6); the clobbers `~{dirflag}`, `~{fpsr}`,
`~{flags}` that clang adds to every statement are `Ignored`.

### 7.4 Symbol addresses

How code reaches a symbol depends only on the module (PIE level, code
model), the relocation model (`Options::reloc_model`: `llc`'s
`-relocation-model=`, `Static` by default as `llc`'s is on ELF) and the
symbol (linkage, visibility, `dso_local`, thread-local model). Three pure,
target-independent functions in `hod_cg` (`src/access.rs`) answer it:

```rust
pub fn sym_access(module: &Module, options: &Options, sym: SymId) -> Access; // Direct | Got | Tls(TlsAccess)
pub fn call_access(module: &Module, sym: SymId) -> CallAccess;               // Direct | Plt
pub fn prefers_local_alias(module: &Module, options: Options, sym: SymId) -> bool;
```

- `sym_access` follows `TargetMachine::shouldAssumeDSOLocal` (on ELF
  exactly the symbol's `dso_local`, which `hod_ir` stores as LLVM implies
  it): not `dso_local` is `Got`. On AArch64 under the small or tiny code
  model an `extern_weak` symbol is `Got` as well (`adrp` cannot produce 0).
  A thread-local symbol is `Tls(m)`, `m` being `TargetMachine::getTLSModel`:
  the model the output implies (a shared library is PIC with no `PIE
  Level`; `dso_local` gives the local variant), or the symbol's own if that
  is more specific. AArch64 lowers `LocalDynamic` as `GeneralDynamic`.
- `call_access` is `Direct` for a `dso_local` callee, else `Plt`.
- `prefers_local_alias` is `AsmPrinter::getSymbolPreferLocal`: in a shared
  library, a `dso_local` external definition of default visibility, not an
  IFUNC and not in a deduplicating COMDAT, is reached through a local
  alias (`.L<name>$local`, or its section symbol, as below), because the
  linker considers its global symbol preemptible. This holds for code and
  for data relocations alike, but not for the subtracted side of a
  PC-relative difference (`lowerRelativeReference` uses the symbol).

A reference to a local symbol, or to one reached through its local alias,
is written against the **section symbol** with the symbol's offset in the
addend, as the ELF assembler does, when the symbol is placed, is not an
IFUNC, and its section is neither thread-local nor, unless the addend is
0, mergeable. Otherwise it names the symbol (for a local alias, the
`.L<name>$local` symbol module emission defines next to it).

`tests/sym_access.rs` checks all three against the relocations `llc`
(LLVM 19 and 22) emits for one getter and one caller per symbol over a
grid of declared and defined variables and functions × every linkage ×
visibility × `dso_local` × thread-local model, on AArch64 and x86-64,
static, PIC and PIC with `PIE Level` (2880 answers; the section-symbol
rule is checked on the same relocations). Each target has one access
sequence per answer.

**Offsets from a symbol** (WP-073) fold into the relocation's addend as
`llc`'s `performGlobalAddressCombine` folds them: for a `Direct` access,
only when `0 <= offset < 2^20` and the offset is at most the allocation
size of the symbol's value type (one past the end folds; a negative
offset never does). Otherwise, and always for `Got` and `Tls`, the
relocation carries the rest of the offset (0 for `Got` and `Tls`) and
the offset is added in code after the address (`common::add_addend`).
An addend so folded stays within the object the symbol names, which
the linker can place anywhere: an offset outside it in the relocation
can overflow `R_AARCH64_ADR_PREL_PG_HI21` (gcc-torture `pr79286.c`).

### 7.5 Constants in code

An integer constant is materialized by the encoder's shortest sequence
(`mov_imm`), a floating-point constant by its bit pattern through the
encoder's FP materialization (`spec/mc-a64.md`). A constant too wide for
that (a vector, `fp128`) is placed in a **data island** after the
function's code, inside the function's own section, and loaded
PC-relative through an internal fixup: no relocation, no separate section,
nothing shared between functions. Islands are aligned to their contents
and counted in the function's size. (AArch64 builds `fp128` in the register instead, §5.5.1.)

### 7.6 Coroutines

Design 0005. A coroutine's frame starts with two code addresses: the
resume function at offset 0 and the destroy function at offset 8, each a
`void(ptr frame)` function called `fastcc`. That header, the promise's
place and the noop frame are an ABI between objects: every object that
holds a `std::coroutine_handle` relies on them, whoever compiled the
coroutine. hod lowers what LLVM's `coro-early` and `coro-cleanup` passes
make of the handle intrinsics (`opt -passes='default<O0>'`, LLVM 19 and
22 agree), so that a hod object and an `opt` + `llc` object share
handles.

**Handle operations in any function (WP-105).** `hod_a64/src/coro.rs`,
one arm of `intrinsic::intrinsic`:

| intrinsic | lowering (AArch64) |
|---|---|
| `coro.resume(p)` / `coro.destroy(p)` | `ldr x, [p, #0]` / `[p, #8]` into a temporary, then the core's call sequence through it with `p` as the one argument, `fastcc` (the C convention for one pointer on AArch64): `FnCx::call_through`. As an `invoke` too: the normal edge's `phi` stores, the jump and the landing-pad stub are the ones of `FnCx::call` |
| `coro.done(p)` | the resume pointer is null: `ldr x, [p]`, `cmp x, #0`, `cset w, eq` |
| `coro.promise(p, align, from)` | `p + alignTo(16, align)`, or `p - alignTo(16, align)` when `from` is true: `add`/`sub` of a constant |
| `coro.noop()` | the address of the object's noop frame: `adrp` + `add :lo12:` (small code model; `a64:code-model` otherwise) |

`align` must be a constant power of two of at most 2^30 and `from` a
constant (`a64:coro-promise` otherwise): LLVM computes the offset in
`i32` from the floor of the alignment's logarithm (an alignment of 0, 24
or 48 gives 16, 16 and 32), which no front end relies on; front ends
pass `alignof(promise)`.

`FnCx::call_through(inst, callee, args, params, cc)` is the generic call
through a computed address: `callee` is a temporary the lowering loaded,
moved to the sequence's callee register after the arguments are placed;
the call has no result. It is §7.1's sequence with `Callee::Temp`.

**The noop frame (WP-105).** Generic, in `hod_cg` (`coro.rs`,
`emit/noop.rs`). `FnCx::noop_coro_frame()` returns
`SymRef::NoopCoroFrame` and records, in the chunk, the target's
`Target::NOOP_CORO_FN` (the code of a function that only returns; empty
means the target does not lower `coro.noop`: `coro:noop-target`). That is
the one target hook. Module emission places, at the first relocation to
the frame (after every function is placed, as `llc` orders them):

- `__NoopCoro_ResumeDestroy`: the function's code, in `.text` after the
  other functions (`.text.__NoopCoro_ResumeDestroy` with function
  sections), aligned to 4, with a local `STT_FUNC` symbol of that name and
  size: LLVM 22 makes the function `internal` (LLVM 19 `private`, so its
  object has no symbol). No unwind entry: it calls nothing and changes
  no register.
- the frame, LLVM's `NoopCoro.Frame.Const`: 16 bytes, aligned to 8, both
  words `R_AARCH64_ABS64` to the function (section symbol plus offset),
  in `.data.rel.ro` under PIC and `.rodata` otherwise
  (`.data.rel.ro..LNoopCoro.Frame.Const` / `.rodata..LNoopCoro.Frame.Const` with data
  sections), with no symbol: it is private, and references name its
  section.

Each object has its own frame, as each `llc` object has: two
`coro.noop()` of one object are equal, of two objects are not
(`noop_coroutine()` handles are compared only within one translation
unit's expectations; LLVM guarantees nothing across objects).

**Coroutine bodies (WP-106).** Design 0005's approach B: a
`presplitcoroutine` body is compiled once; what crosses a suspend lives
in a heap frame addressed through a reserved, saved register; `F.resume`
and `F.destroy` are entry stubs that rebuild the same machine frame and
jump to the code after the suspend. Residency is `hod_xform::coro::plan`
(`spec/xform.md`); what the plan refuses is refused by its `coro:*` name.
Generic parts in `hod_cg/src/coro_body.rs`, AArch64 in
`hod_a64/src/coro_body.rs`.

*The heap frame* (`FrameRef::Coro(offset)`, an offset from the frame
base). In order: the header (resume at 0, destroy at 8); the promise at
the plan's `promise_offset`; the 4-byte suspend index at
`alignTo(promise end, 4)`; the heap slots in slot order; the heap values
(instruction results live across a suspend) in instruction order, each
at its own alignment. `coro.size` is the end, `coro.align` is
`max(frame_align, 16)`; both are constants.

*Homes.* A heap slot's place is its heap offset; a heap value's home is
its heap offset; token results live nowhere; `coro.begin`'s result is the
**frame word**, a machine-stack slot. A slot or value the plan lists in
`begin_copies` / `begin_value_copies`, and the promise (whose address
`coro.id` names before `coro.begin`), has a machine-stack **shadow**: the
blocks before `coro.begin` (reachable from the entry without leaving
`coro.begin`'s block) address the shadow, `coro.begin` copies every
shadow to its heap place, and every later block addresses the heap. A
block reached both before and after `coro.begin` (the ramp's return after
a failed allocation) is refused, `coro:begin-shared-block`, when it names
a shadowed slot or value; otherwise any heap access in it is an internal
error. A machine-stack slot aligned above the stack alignment is
`coro:overaligned-stack-slot`.

*The machine frame* (AArch64). x19 is the frame base, saved in the
prologue after the other saves (with its CFI row), restored before every
exit and tail exit. Two more words: **mode** (0 in the ramp, 1 in an
activation a stub entered) and the frame word. `preserve_most` and
`preserve_all` coroutines are `a64:coro-preserve-cc`.

*Intrinsics* (AArch64):

| intrinsic | lowering |
|---|---|
| `coro.id` | nothing (a token) |
| `coro.alloc` | `1` (no elision) |
| `coro.size`, `coro.align` | the layout's constants |
| `coro.begin(id, mem)` | x19 := mem; the frame word := mem; `adr` of each stub stored to `[x19, #0]` and `[x19, #8]`; the shadow copies; the result is the frame word |
| `coro.save` | `[x19, #index]` := the suspend's index; at the final suspend also `[x19, #0]` := 0 (`done()`) |
| `await.suspend.void` / `.bool` | a call of the wrapper with (awaiter, frame), through a temporary, C convention; `.bool`'s result is the call's |
| `coro.suspend` | `w0` := −1, the suspend's **re-entry label**, the result is `w0`: a call that stores nothing |
| `coro.end` (LLVM 19 `i1`, 22 `void`) | not unwinding: when mode ≠ 0, branch to the epilogue; unwinding: when mode ≠ 0, `[x19, #0]` := 0 and the index := the final suspend's (the coroutine is done). LLVM 19's result is mode |
| `coro.is_in_ramp` | mode xor 1 |
| `coro.free`, `coro.frame` | x19 (`coro.frame` itself is the plan's `coro:frame-intrinsic`) |
| `await.suspend.handle` | the call of the wrapper as for `.void`, then the transfer below (WP-107) |

A body intrinsic in a function without a plan is
`coro:outside-coroutine`; a target with `Target::CORO_BODIES` false
refuses coroutines as `coro:target`.

*The suspend switch.* The `switch` on a `coro.suspend` result is lowered
as a dispatch first (`cmp w, #0` / `b.eq`, `cmp w, #1` / `b.eq`), then
each edge's `phi` stores on that edge: the −1 edge (the activation that
suspended) jumps to its block, the 0 edge (resume) and 1 edge (destroy)
to theirs.

*The stubs*, emitted after the function's code, before its literal
islands; local `STT_FUNC` symbols `F.resume` and `F.destroy` (F's own
symbol's size ends at the first stub). Each builds the same machine frame
as F's prologue (same CFI rows: first the entry state, then the
prologue's, then x19's), sets x19 := x0, mode := 1, the frame word := x0,
in a protected function the guard word and the pointer slots (§5.6.24
rule 6), `w0` := 0 (resume) or 1 (destroy), loads the index and branches to that
suspend's re-entry label (`cmp` / `b.eq` per suspend), and ends in
`brk #1`. Entering after the suspend with `w0` as its result, the
switch takes the 0 or 1 edge.

*The suspended region* (design 0005 §3.3). From the return of an
`await_suspend` wrapper (or, without wrappers, from `coro.save`) to the
suspend, and on the −1 edge, the frame may already be freed: no
instruction may address it. Blocks of the plan's `suspended_region`,
the code after a wrapper's call and the −1 edge's `phi` stores are in the
region; every heap address the target forms passes `FnCx::coro_access`,
and one formed in the region (or before `coro.begin`) makes the function
an internal error.

*Symmetric transfer (WP-107).* `coro.await.suspend.handle(awaiter,
frame, wrapper)` (a `call` or an `invoke`) calls the wrapper, which
returns the handle `h` to resume; the coroutine must then resume `h`
without growing the stack, however long a chain of transfers runs
(LLVM: the clones end in `musttail call fastcc %resume(h)`, the ramp
makes a plain call and goes on to its suspend). Right after the
wrapper's call returns, before the `invoke`'s normal edge:

```text
    ldr x16, [x0]               h's resume function (offset 0 of its frame)
    ldr w17, [x29, #mode]
    cbnz w17, exit              a resumed activation: a tail exit
    blr x16                     the ramp: a call with x0 = h, its own
                                call-site row without a landing pad
    ...                         the invoke's normal edge, the suspend (−1)
exit:                           after the epilogue, with the other tail exits:
    the epilogue's teardown (x19, x29, x30 restored, the frame released)
    br x16                      x0 = h
```

- The resumed activation leaves its machine frame entirely before it
  jumps: the resume function runs at the stack depth at which the stub
  that entered the activation was called, so an unbounded chain of
  transfers runs in constant stack, with a coroutine compiled by hod or
  by `llc` on either side (`fastcc` is the C convention for one pointer
  argument).
- The ramp calls: its caller is still waiting for the return object.
  The call is outside the `invoke`'s range (an exception from the
  resumed coroutine leaves the ramp, as from LLVM's plain call); the
  ramp then continues to the suspend's −1 edge, which returns.
- Both paths are emitted; the mode decides. The suspended region starts
  at the wrapper's return (design 0005 §3.3): neither path addresses the
  frame. The memory read are `h`'s first word (the other coroutine's
  frame) and the mode word; the tail exit's restore of x19 reads the
  machine stack.
- The suspend index is stored by `coro.save`, before the wrapper runs:
  the wrapper may resume or destroy the coroutine (on this thread,
  inside the wrapper, or on another), and the stub that enters it
  dispatches on that index.
- `FnCx::coro_tail_exit()` makes the tail exit (a `TailExit` whose
  target is the callee register); `Target::coro_transfer(cx, exit)` is
  the code above, called by `coro_called` from `call_through`.

*Exceptions in the ramp and in an await* (LLVM 19 and 22 agree; WP-107):

- **A throwing `await_suspend`** ([expr.await]/5.1: the coroutine is
  resumed and the exception rethrown in it). The wrapper is an `invoke`
  whose landing pad lies in the body: it runs in the activation that
  called the wrapper, with the frame alive and the activation's mode
  unchanged, so heap slots are addressed as anywhere outside the
  suspended region (the pad is not in it). The transfer never runs: it
  follows only the wrapper's normal return. The index stored by
  `coro.save` stays until the next `coro.save`; nothing reads it unless
  the coroutine suspends again.
- **A throwing `get_return_object`** (or any exception in the ramp
  before the initial suspend). The ramp's unwind path runs the
  cleanups, frees the frame (`coro.free`, the front end's `operator
  delete`), and resumes the exception. The frame is dead from the free
  on: nothing after it may address the frame. The exception pointer and
  selector slots are unescaped, so the plan keeps them on the machine
  stack (design 0005 §3.4 rule 3); a heap slot or value used after the
  free is refused (`coro:frame-access-after-suspend`). LLVM's split
  pairs every use with every use, so in the seed `gro-throws-uaf`
  (whose final `await_suspend` returns a handle) it puts those slots in
  the frame and reads them after the free (design 0005 §1.4): its
  program crashes once the freed frame is poisoned (the seed's promise
  does that itself; an `oracle-skip` of `spec/harness.md`). hod's
  program prints the expected output.

*Hooks.* `Target::CORO_BODIES`, `Target::coro_stubs`,
`Target::coro_dispatch`, `Target::coro_transfer`; `FnCx::coro_*` for the
target; one call each in the driver (`coro_plan`, `coro_setup`,
`coro_slot_place`, `coro_block_start`, `coro_switch`, `coro_finish`,
`coro_entries`), in `call_through` (`coro_called`, which also starts the
transfer) and in module emission (the stub symbols).
`Stress::coro_frame` (§12, not in `Stress::ALL`) gives every function
the coroutine machine frame and clobbers x19 after saving it.

*The facade.* `classify` accepts `presplitcoroutine`, `type:token` and
the body intrinsics on AArch64; the session refuses a body by the plan's
`coro:*` name before code generation. (`coro:debug-info`, WP-106's
refusal of a coroutine in a module with debug information, is lifted by
WP-108.)

*Debug information* (WP-108, `spec/dwarf.md` §6.16). The target's
obligations, so that one location per variable holds at every pc:
the prologue of a coroutine body stores 0 to the frame word before any
other code (it stays 0 until `coro.begin` writes the frame's address);
each stub calls `FnCx::coro_stub_ready` once the mode and frame words are
written, which places the stub's `prologue_end` row; the stubs stay inside
the body's subprogram range. `hod_cg` (`FnCx::coro_vars`, from
`finish_vars`) rewrites the body's variable rows onto the frame word.

#### x86-64 (WP-137)

`hod_x64/src/coro.rs` (handle operations) and `hod_x64/src/coro_body.rs`
(bodies); the generic parts above are unchanged. `Target::CORO_BODIES`
is true; the obligations are AArch64's, with these registers and shapes.

*Handle operations.* `coro.resume`/`coro.destroy`: `mov r, [p + 0 | 8]`,
then `call_through` with `p`, `fastcc` (the C convention for one
pointer). `coro.done`: `cmp qword [p], 0` + `sete`. `coro.promise`:
`lea r, [p ± max(16, align)]`; an alignment that is not a constant power
of two of at most 2^30, or a direction that is not a constant, is
`x64:coro-promise`. `coro.noop`: `lea r, [rip + frame]`
(`R_X86_64_PC32`), small code model only (`x64:code-model` otherwise);
the noop frame's function is one `ret` (`NOOP_CORO_FN`).

*The machine frame.* `rbx` is the frame base. A function with the
coroutine frame always has `rbx` in its save set (§5.6.15), so a
coroutine only takes `rbx` out of the allocation pool
and the holdable set: `X64::for_function` sets `X64::coro` from
`presplitcoroutine` and `reg_info` is then `REG_INFO_CORO`. The mode
and frame words are `[rbp − d]` slots; the prologue stores 0 to both
right after `sub rsp, FRAME`. `Stress::coro_frame` gives every function
those words and clobbers `rbx` (`mov rbx, rbp`) after the prologue.
`cmpxchg16b` needs `rbx`: in a function with the coroutine frame a
16-byte atomic pushes `rbx` before the sequence writes it and pops it
after the last `cmpxchg16b`; between the two nothing addresses the heap
frame, and the new value's high half is loaded before its low half
(into `rbx`). Without `+cx16` the libatomic calls need nothing (WP-147,
§5.6.16).

*Intrinsics.* As on AArch64, with `rbx` for x19: `coro.begin` is
`mov rbx, mem`, `lea r11, [rip + F.resume]` / `[rip + F.destroy]`
stored to `[rbx]` / `[rbx + 8]`, the frame word := `rbx`; `coro.save`
stores the index (`mov dword [rbx + index], k`, and at the final
suspend `mov qword [rbx], 0`); `coro.suspend` is `mov eax, −1`, the
re-entry label, the result from `eax`; `coro.end` compares the mode word
with 0. The `await_suspend` wrappers are called through a temporary
with (awaiter, frame), C convention.

*The stubs.* Local `STT_FUNC` symbols `F.resume` and `F.destroy`
after F's code. Each states the entry CFI (CFA = `rsp + 8`, `rbp` and
the save set restored), then builds F's machine frame with F's rows
(`push rbp`, `mov rbp, rsp`, the save set's pushes, `sub rsp, FRAME`),
then `mov rbx, rdi`, mode := 1, the frame word := `rdi`, in a protected
function the guard word and the pointer slots (§5.6.24 rule 6)
(`coro_stub_ready` here), `mov eax, 0 | 1`, one `cmp dword [rbx +
index], k` / `je` per suspend, and `ud2`. Their displacements are the
final ones (`frame::base_offset`): they are emitted after the frame's
locals are biased.

*The suspend switch* dispatches on `eax`'s low byte: `cmp r8b, 0` /
`je`, `cmp r8b, 1` / `je`.

*Symmetric transfer.* After the wrapper's call (`rax` = h):

```text
    mov r11, [rax]              h's resume function
    mov rdi, rax
    cmp qword [rbp - mode], 0
    jne exit                    a resumed activation: a tail exit
    call r11                    the ramp: its own call-site row
    ...
exit:                           lea rsp, [rbp - 8n]; the save set's pops; pop rbp
    jmp r11
```

## 8. The `Target` trait

A target is a zero-sized or small `Sync` value. The driver owns the walk;
the target owns everything that depends on the instruction set. The
signatures below are `crates/hod_cg/src/target.rs` and `cx.rs` (WP-030);
the rustdoc there is the detailed contract.

### 8.1 What `hod_cg` provides to a lowering (`FnCx<T>`)

`FnCx<'a, T: Target>` is one function being compiled. Its public fields
are read access to the inputs (`module`, `body`, `options`, `target`,
and `state: T::FnState`, the target's own per-function state); everything
else is a method:

| group | methods | responsibility |
|---|---|---|
| operands | `loc`, `use_reg`, `put`, `use_mem`, `deferred`, `class_of` | the location of a value; "this value in a register" (pinned; loaded or materialized as needed); "this value in that register" (arguments, return values); the address of a `Mem` value or a home |
| results | `def_reg`, `finish`, `def_mem`, `may_defer`, `defer` | a register for the result (optionally an operand's); `finish` applies §4.1 (store to the home, or keep); the home of a `Mem` result; deferral (§4.6) |
| registers | `temp`, `take_reg`, `free_temp`, `release`, `displace`, `displace_all`, `scratch_reg` | a temporary for the instruction; displacing one register or all (§4.3, §4.4) |
| calls | `call`, `call_runtime`, `record_call_site`, `place_return`, `fn_assignment`, `tail_exit_count`, `tail_exit`, `phi_stores` | the sequence of §7.1 for the instruction being lowered, and for a `RuntimeFn` with `CallArg` operands (§7.3); the function's own assignment and its tail exits (§7.1) |
| frame | `alloc_slot`, `slot_place`, `frame_size` | a slot; the `FrameRef` or `SlotPlace` of a body slot |
| code | `offset`, `emit` and the `emit_*` family, `code_mut`, `block_label`, `epilogue`, `new_label`, `bind`, `fixup`, `falls_through`, `far`, `trap` | bytes; labels, fixups and binding (§6.2); patching emitted bytes |
| records | `reloc`, `island`, `cfi`, `cfi_at`, `set_frame_desc`, `type_id`, `pad_clause`, `pad_filter` | a relocation (§9.2), a data island (§7.5), CFI rows and the frame description (§9.3), type ids and clauses (§9.4) |

Call sites, line rows and block offsets are recorded by the driver, not by
lowerings. `FrameRef` is `Local(u32)`, `Incoming(u32)` or `Outgoing(u32)`:
the last addresses a stack argument from the lowered stack pointer between
`adjust_sp` and the call (§5.3).

**A lowering's obligations.** It does not keep a register across a call it
makes; it pins at most the pool's minimum of a class (§2.1); it leaves no
result in a scratch register; and it asks for an operand that may be
`Deferred` (`FnCx::deferred`) **before** it pins any other operand, since
emitting the deferred instruction needs registers of its own. A run of
deferred instructions is at most `MAX_DEFERRED_RUN` (8) long.

### 8.2 What a target provides

```rust
pub trait Target: Sized + Sync {
    type FnState: Default;            // per-function state, in FnCx::state
    const TRAP: &'static [u8];        // the trap instruction (lengths differ)
    const FN_ALIGN: u32;
    const PAD_BYTE: u8;
    const STACK_ALIGN: u32;

    fn reg_info(&self) -> &RegInfo;                                    // §2.1
    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class;        // §2.2
    fn assign(&self, types: &TypeTable, sig: &Sig<'_>, out: &mut Assignment)
        -> Result<(), AbiUnsupported>;                                 // §7.2
    fn for_function(&self, module: &Module, body: &Body) -> Option<Self> { None }
    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()>;
    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()>;
    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()>;

    // glue
    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32);
    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32);
    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32);
    fn copy_from_ptr(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, ptr: PReg,
        offset: u32, bytes: u32);
    fn mov(&self, cx: &mut FnCx<'_, Self>, dst: PReg, src: PReg);
    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()>;
    fn materialize_mem(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, value: Value)
        -> CgResult<()>;
    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef);
    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64);
    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs);
    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()>;
    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()>;
    fn dynamic_slot(&self, cx: &mut FnCx<'_, Self>, ptr: FrameRef, size: u64, align: u64)
        -> CgResult<()>;

    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool;
}
```

| member | responsibility |
|---|---|
| `reg_info()` | §2.1 |
| `class_of(types, ty)` | §2.2; total over the types `classify` accepts |
| `begin_function(cx)` | the fixed-size prologue and target reservations in the frame. The driver then emits the entry code for arguments (§4.8) and lays out the body's slots, calling `dynamic_slot` for an over-aligned one (§5.1) |
| `end_function(cx)` | patch the prologue; emit the epilogue and the tail exits, with their CFI rows; fill the frame description (§9.3). The driver has added the epilogue's line row (§9.5) |
| glue: `load`, `store`, `copy`, `copy_from_ptr`, `mov`, `materialize`, `materialize_mem`, `frame_addr`, `adjust_sp`, `extend_arg`, `jump` | the primitive moves the generic policy, the `phi` protocol and the call sequence are made of. They use only `scratch` registers besides their operands, and never fail for an offset or size: a far offset costs a scratch register, not an error. `extend_arg` widens a narrow integer argument or return value as the convention asks |
| `emit_call(cx, call)` | the call instruction itself, to a `CallTarget` (a symbol, or a register the sequence loaded); the sequence records the row around what it emits (§9.4) |
| `dynamic_slot(cx, ptr, size, align)` | the entry code for one over-aligned slot: allocate it (§5.2) and store its address to `ptr` |
| `lower(cx, inst)` | every instruction that is not a `phi`, including terminators and calls. One `match` on the opcode that dispatches to the target's modules, one per instruction group, so that groups are separate work packages |
| `assign(types, sig, out)` | §7.2 |
| `for_function(module, body)` | the target to compile `body` with, when it is not `self`: `compile_function` calls it once, before anything else, and every other member is called on the value it returns for the whole function. x86-64 returns `self` with the function's `target-cpu`/`target-features` (§5.6.9), and `None` for a target that is already a function's own (`X64::of_attrs`, field `own`: the driver makes one per attribute set, WP-166); the default, `None`, keeps `self` (AArch64: its convention does not depend on a function's features) |
| `patch_branch(code, at, kind, displacement) -> bool` | §6.2; `false` when the field cannot hold the displacement |
| constants | `TRAP` (a byte slice), the function alignment, the padding byte between functions, the stack alignment |

The call sequence of §7.1 and the `phi` protocol of §4.5 are implemented
once, in `hod_cg`, on top of glue and `assign`; a target's `lower` for
`Call` calls `FnCx::call` and supplies, through `emit_call`, only the call
instruction itself and what is special to the target (variadic
conventions, TLS call sequences).

A data-dependent operand of an encoder call uses the encoder's checked
(`try_`) form, with the scratch-register fallback when it does not fit
(`spec/mc-a64.md` "Checked and unchecked forms"); the unchecked forms are
for operands the lowering itself fixed.

## 9. Output

### 9.1 `Chunk`

A worker compiles a run of consecutive functions into a `Chunk`: flat,
append-only tables that one function after another adds to, plus one
`FuncRecord` per function holding ranges into them. Nothing in a chunk is
allocated per function. All offsets in the tables are **relative to the
start of their function's code**, so a function's rows do not depend on
which chunk it is in, nor on what else is in it.

| table | row | for |
|---|---|---|
| `code` | bytes | the function's instructions and data islands |
| `relocs` | `(offset, RelocKind, SymRef, addend)` | `hod_obj` relocations (§9.2) |
| `cfi` | `(offset, CfiOp)` | `hod_eh` (§9.3) |
| `call_sites` | `(start, length, pad, landing, nounwind)` | `hod_eh` (§9.4) |
| `pads`, `clauses`, `type_ids`, `filter_ids` | §9.4 | `hod_eh` |
| `lines` | `(offset, LocId)` | `hod_dwarf` (§9.5) |
| `vars` | `(variable, expression, declared-at, location)` | `hod_dwarf` (§9.5) |
| `labels` | `(label, offset)` | `hod_dwarf` (§9.5); not yet: `hod_ir` has no `#dbg_label` (WP-005) |
| `islands` | `(offset, length)` | mapping symbols (§5.5) |
| `block_offsets` | `(BlockId, offset)` | `blockaddress` in data (§9.6) |

`FuncRecord`: the function's `SymId`; its ranges in each table; its code
alignment; its frame description (§9.3); whether it has landing pads; and
what its unwind-table entry needs (`spec/eh.md` §1.1), read from the
module when the function ends: its personality, whether it is `nounwind`
and whether it has `uwtable`. `FuncRecord::needs_unwind_entry` is the FDE
rule.

### 9.2 Relocations

`RelocKind` is `hod_obj`'s (`spec/object.md` "Relocations"): the target
picks the psABI relocation. `SymRef` is `Module(SymId)` or
`Runtime(RuntimeFn)` (§7.3). Code never refers to another function's
blocks (§1) or islands, to its own section, or to anything `hod_eh` and
`hod_dwarf` generate; those references are made by their writers.

### 9.3 Frame description and CFI

`FrameDesc`: the final frame size and the DWARF register of the frame
pointer, which is also the function's DWARF frame base (variable
locations are offsets from it). `cfi` rows are DWARF call-frame
instructions as data: `DefCfa { reg, offset }`, `DefCfaOffset`,
`DefCfaRegister`, `Offset { reg, cfa_offset }`, `Restore { reg }`, each
with the code offset from which it holds. The target emits the rows for
its prologue and its exit sequences. The exit sequences are the last code
of the function, so nothing after them needs the body's state back; each
tail exit, coming after another exit sequence, starts with rows that
state the body's rules again (the CFA and the saved registers) rather
than relying on remembered state. So does the shared trap of §6.1, which
the core places after the exit sequences and the body branches to: before
it, the core restates the net effect of the rows emitted before the
epilogue was bound (one CFA rule, then the last rule of each saved
register). `CfiOp` and `CfiRow` are `hod_eh`'s `Op` and `Row`,
re-exported, so the rows reach `.eh_frame` as they are. The rows are
exact at every instruction boundary (asynchronous unwind tables), which
is what debuggers and profilers need; `hod_eh` writes them into
`.eh_frame` and may share a CIE between all functions of an object that
agree on personality and encodings.

### 9.4 Exception tables

Recorded only for a function that has landing pads; a function without
them has no LSDA, and exceptions pass through it by its CFI alone.

- `type_ids`: the function's type-info table: `Option<SymId>` (none is
  catch-all) in order of first mention. `llvm.eh.typeid.for(sym)` lowers
  to the constant index + 1 of `sym` in it. `hod_eh` must emit the LSDA
  type table in exactly this order, because those constants are already in
  the code.
- `pads`: per landing pad, the code offset of its block, the cleanup flag
  and its range of `clauses`; a clause is `Catch(type id)` or
  `Filter(range of type ids)`.
- `call_sites`: one row per call instruction emitted (§7.1), in code
  order: the code range of everything `Target::emit_call` emitted for it,
  its pad (none for a plain `call`
  or a runtime call) and its **landing offset**: the pad's block, or the
  pad stub of an `invoke` with stack arguments (§5.3). The pad gives the
  row's action, the landing offset its landing address. `hod_eh` merges
  adjacent rows that agree in both and writes the table; a call that can
  throw and has no pad must still be covered by a row, or the personality
  routine terminates the program instead of unwinding through. The pad
  and landing offset are resolved when the function ends; a landing
  offset of 0 in a row with a pad means the pad block's own offset, and
  it is 0 in a row without one. `nounwind` is set for a direct call of a
  function declared `nounwind` (`llc`'s `callToNoUnwindFunction`); such
  a row without a pad gets no LSDA entry (`spec/eh.md` §9.3). Calls a
  target records itself (`FnCx::record_call_site`) are not `nounwind`.
- `filter_ids`: the type ids of the `filter` clauses, which a clause
  names by range.

### 9.5 Debug information

Recorded only when the module has debug information.

- `lines`: a row when the `LocId` of the instruction being emitted differs
  from the previous instruction's, `LocId::NONE` included. The code
  generator records transitions only; which rows become line-table
  entries, where `prologue_end` goes and what a location-less instruction
  inherits is `dwarf.md`'s policy. The entry code of §4.8 and the prologue
  have no row of their own, so they precede the first row. Only an
  instruction that emitted code gets a row; the loads of a block's `phi`s
  count as its first emitting instruction's. The driver, before
  `end_function`, adds a row at the epilogue: the location of the function's `ret` if
  exactly one was emitted, `LocId::NONE` otherwise, so that the shared
  epilogue does not inherit the line of whichever block was emitted last.
- `vars` (`VarRow`, implemented in WP-057): one row per `#dbg_declare`:
  the variable, the record's expression (`ExprId`), its location, and
  where the declared address is (`VarPlace`): `Frame(off)`, the variable
  is `off` bytes from the DWARF frame base (a stack slot), or
  `FrameIndirect(off)`, the address is stored there (an over-aligned
  slot, §5.1, or the home of a value that is not a slot, such as an
  `sret` or by-reference argument; §4.1 gives it a home). The driver
  records `Slot`/`Indirect` places while the frame grows and rewrites them
  through `Target::frame_base_offset` once the frame is final; a place
  the target cannot reach by a fixed offset is `Refused("debug:frame-place")`.
  `hod_dwarf` composes the location with the expression. Several rows
  may name one slot (structured bindings do), each with its own
  expression. A `#dbg_declare` of a `phi` has no row (WP-111): `llc
  -O0`'s `SelectionDAGBuilder` describes a declared address that is not
  a static `alloca` or an argument in memory only through the node the
  address has in the declaration's block, and a `phi` has none until an
  instruction of the block uses it, so `llc` drops the declaration (a
  `CoroSplit` ramp declares the frame's variables right after the frame
  pointer's `phi`). `hod_ir`'s declarations have no position, so the two
  other cases `llc` tells apart by position stay as they are: a value
  from another block (`llc` drops it) and a value declared after its
  definition in its own block (`llc` writes a location list from there);
  hod writes the value's home for the whole function. No front end emits
  either at `-O0` (`crates/hod_a64/tests/dies_followups.rs`, two
  ignored tests). At `-O0` a row's location holds for the whole function. The
  body is dropped after its function is compiled, so everything
  `hod_dwarf` needs from it is copied here.
- `#dbg_value` records are rows of `vars` too (WP-162, `spec/dwarf.md`
  §6.23): one `VarPlace::Value(ValueRange)` row per record in a
  reachable block, the code range it holds over and what holds the value
  there (a frame home, a constant, or nothing); `move_rows` moves the
  ranges.
- labels are rows of `vars` too: one `VarPlace::Label(offset)` row per
  `#dbg_label` in a reachable block, the label and the code offset of
  the instruction that follows it.
- a record hod cannot describe exactly is a `VarPlace::Refused(name)`
  row; a `FullDebug` unit then refuses the function under that name:
  `debug:dbg-value-arglist` (a `#dbg_value` of a `DIArgList`) and
  `debug:dbg-value-coroutine` (one in a coroutine body), `debug:dbg-assign`, `debug:declare-address` (a `#dbg_declare` of a
  constant, a symbol, or a value that never got a home; a declaration of
  `undef` or `poison` gets no row at all, as `llc` drops it, WP-156,
  `spec/dwarf.md` §6.22).
- `debug_calls` (`DebugCallRow`, WP-113, `spec/dwarf.md` §6.19): in a
  function whose subprogram has `DIFlagAllCallsDescribed`, one row per
  call instruction an IR `call` or `invoke` became, in lowering order:
  the callee (a module function, an indirect call's DWARF register, or
  `Refused(name)`), whether it is a `musttail` call, and the offset after
  the call instruction (for a `musttail` call, of the tail exit's
  branch). The engine arms the row before `Target::emit_call`; the
  target completes it with `FnCx::call_returns_here` right after the
  call instruction and `FnCx::tail_branch_here` right before a tail
  exit's branch. A row left incomplete is an internal error. Calls the
  code generator makes on its own have none.

### 9.6 Block addresses

For a function that contains an `IndirectBr`, `block_offsets` gives the code offset of each
reachable block (and of the trap that stands for an unreachable one,
§6.1). A `blockaddress` in a global initializer (`Fixup::Block`,
`spec/ir.md` §4.3) becomes a relocation against the **section symbol** of
the function's section, with the function's offset in the section plus
the block's offset as addend, as LLVM emits it. It must not be against the
function's own symbol: that symbol may be preemptible, and the table
would then point into whatever definition the dynamic linker chose. A
`blockaddress` in code is formed PC-relative by the function itself
(§6.3). (`hod_ir` marks address-taken blocks since WP-064,
`ConstPool::is_address_taken`; since WP-115 a function with an
address-taken block has `block_offsets` with or without an
`IndirectBr`, and an unreachable address-taken block has the trap's
offset.)

### 9.7 Merge and module emission

`hod_cg::emit_module(module, &Options, chunks) -> CgResult<Emitted>`
(`src/emit/`), or step by step `ModuleEmitter::new`, `add_chunk` per
chunk in module order, `finish`. `add_chunk` takes the chunk by `&mut`
and empties it (`Chunk::clear`, allocations kept), so that a caller can
add each chunk as soon as it is made and compile the next into the same
buffers (WP-166); the object does not depend on how the functions were
divided into chunks. The object is laid out as `llc -O0`
lays it out for the same options (`reloc_model`, `code_model`,
`function_sections`, `data_sections`); the oracle is `llc` of LLVM 19 and
22, and every difference that remains is listed below.

**Symbols.** `new` adds every module symbol to the object as undefined,
in `SymId` order, so a `SymId` is its `hod_obj::SymbolId` by arithmetic
(`spec/object.md` "Symbols"); an anonymous symbol is named
`__unnamed_<n>`, `n` counting anonymous symbols in `SymId` order. `finish`
defines them: binding from linkage (internal and private local;
`linkonce`, `linkonce_odr`, `weak`, `weak_odr` and `extern_weak` weak; the
rest global), visibility, type (`OBJECT`, `FUNC`, `TLS`, `GNU_IFUNC`) and
size (the initializer's alloc size, the code's length). A private symbol
is named `.L<name>` and is written only when a relocation or a group names
it; an undefined symbol nothing references is left out by the writer
(`spec/object.md`), as `llc` leaves it out, except a declaration `llc`
has a directive for (non-default visibility, `extern_weak`), which is
kept, untyped. Only the names of symbols the object keeps reach the
string table. Runtime functions (§7.3) map to the
module's symbol of that name if there is one, else to one new undefined
global symbol each.

**Sections** (`section.rs`) are chosen by `TargetLoweringObjectFileELF`'s
rules: `getKindForGlobal` (TLS zero → `.tbss`, else `.tdata`; `common`;
all-zero or all-`undef` → `.bss`; constant without relocations →
`.rodata`, or for an `unnamed_addr` one a C string → `.rodata.str<w>.<a>`
and 4, 8, 16 or 32 bytes → `.rodata.cst<n>`; constant with relocations →
`.rodata` when static or when every relocation is a difference of two
`dso_local` symbols, else `.data.rel.ro`; the rest `.data`), unique
sections `<prefix>.<symbol>` for `-data-sections`, `-function-sections`
and COMDAT members, explicit sections with `getELFKindForNamedSection`
and `getELFSectionType`, `SHF_GNU_RETAIN` for `llvm.used` members, and
`llc`'s unique-ID rule for an explicit section met with conflicting flags,
entry size or group (`calcUniqueIDUpdateFlagsAndSize`). Sections are
created in the order the symbols are met; `.text` first. A variable in
`llvm.metadata` is not emitted.

**Initializers** are flattened (`ConstPool::flatten`); each `Fixup`
becomes a relocation: `Sym` an absolute one of the field's width (§7.4
decides the target symbol); `Diff { a, b }` folds to a number when both
are in one section and placed, else is PC-relative to `a` when `b` is in
the field's section (`PLT32` for `PltDiff`), else `init:sym-diff-unencodable`;
`Block` resolves through the function's `block_offsets` (§9.6).

**Functions.** A function goes to `.text`, or to its own section when it
has an explicit section, is in a COMDAT, or `function_sections` is set;
its symbol is defined with the code's size, its relocations and islands
are rebased to the section offset (AArch64 adds `$x`/`$d` mapping symbols
at each change). A code relocation to a local function, or one reached
through its local alias, that is not placed yet waits for `finish`, which
writes it against the section symbol. Padding between functions is the
target's (AArch64: `nop`s; x86-64: `llc`'s long `nop`s, §5.6.6).

**COMDATs.** One group per COMDAT (`any` and `nodeduplicate`; another
selection kind is `comdat:selection-kind`), its signature the module
symbol of that name or a new local symbol. A constructor whose associated
symbol is a COMDAT member joins that COMDAT's group.

**Aliases and IFUNCs** are defined in `finish` at their target's place
plus offset (an alias of a declaration or of an expression that is not a
symbol plus an offset is `global:alias-target`; an IFUNC whose resolver is
not defined here is `global:ifunc-resolver`).

**Constructors and destructors** go to `.init_array[.<priority>]` and
`.fini_array[.<priority>]` (65535 unsuffixed), one 8-byte entry per
structor in the module's order, in the group of its associated symbol's
COMDAT if it has one. `!llvm.linker.options` become `.linker-options`
(`SHT_LLVM_LINKER_OPTIONS`, `SHF_EXCLUDE`), `!llvm.dependent-libraries`
`.deplibs` (`SHT_LLVM_DEPENDENT_LIBRARIES`, `SHF_MERGE|SHF_STRINGS`).

**Unwind tables** (`src/emit/unwind.rs`, `spec/eh.md` §1.1, §9) are
written by `finish`, after the linker options and before the symbols are
defined: the LSDAs of the functions with landing pads in one
`hod_eh::write_lsdas` call (`.gcc_except_table`, or
`.gcc_except_table.<name>` in the function's group or with
`function_sections`; type-information stubs in `.data`), then one FDE per
function that needs one (`FuncRecord::needs_unwind_entry`) in one
`write_eh_frame` call, each starting at the function's section symbol
plus its offset after the merge, so a COMDAT member's FDE refers into its
group as `llc`'s does. The personality slot `DW.ref.<personality>` comes
from `hod_eh::personality_ref`, one per personality. The rows are read
from `Emitted::rows` and the placed records only, so the tables do not
depend on the chunking; nothing is allocated per function (the mapped
LSDA rows are in three buffers sized from the row counts). The tables
are byte-identical to `llc`'s (`tests/emit_eh.rs`), except that `llc`
orders the CIEs of two personalities by a pointer (`spec/eh.md` §5) and
defines a `GCC_except_table<n>` label at each LSDA that nothing refers
to. `.debug_frame`, for a `nounwind` function without `uwtable` that has
debug information, is not written (`spec/dwarf.md`'s, a follow-up).

**Debug information** (`src/emit/lines.rs`, `spec/dwarf.md` §5.7, §9):
for each `FullDebug` compile unit, the line table (one sequence per
function with a subprogram, encoded as the function is placed, against
the section symbol of its code plus its offset) and the unit's DIEs
(`DW_AT_stmt_list`, the ranges of its functions' code, its globals),
written after the unwind tables. Nothing is done for a module without
`!llvm.dbg.cu` or whose units are all `NoDebug`: the bytes are those of
the module without debug information.

**`Emitted`**: the object; `funcs: Vec<PlacedFunc>` in module order, each
the function's `FuncRecord` with `code` its byte range in `section`, the
row ranges ranges of `Emitted::rows`, and `relocs` empty; `rows`, every
function's rows merged (`code`, `relocs` and `funcs` empty), for `hod_eh`
and `hod_dwarf`.

**Determinism and cost.** The result depends only on the module, the
options and the functions' records, not on how functions were divided
into chunks or on which thread compiled them (`tests/emit_merge.rs`).
Emission is linear: symbols are found by index, sections through one hash
table keyed by name, group and unique ID, and nothing is allocated per
symbol beyond the object's tables (local aliases and COMDAT signatures are
in maps sized by their count).

**Refused by name** (`hod_cg::emit::unsupported`, §10): `module:asm`
(module-level inline assembly), `comdat:selection-kind`,
`linkage:appending` (outside the special arrays),
`global:initializer-in-bss`, `global:alias-target`,
`global:ifunc-resolver`, `init:sym-diff-unencodable`,
`init:address-width` (an address in a 1-byte field on AArch64),
`init:dso-local-equivalent` (outside a difference),
`init:blockaddress-unplaced`, `module:linker-option-not-a-pair` (an
`!llvm.linker.options` entry of other than two strings: `llc` writes
`.linker-options` as pairs), `eh:unknown-personality-without-pads`
(`spec/eh.md` §1.1), `debug:die-unsupported` (a unit with a node
the DIE writer does not write yet, `spec/dwarf.md` §6.5).

**Known differences from `llc`**, none of which changes what a linker
makes of the object (`tests/emit_link.rs` links both with GNU ld, lld and
mold and runs them):

- Order: `hod` creates the variables' sections and defines their symbols
  before the functions'; `llc` interleaves them in module order. The
  symbol table's order differs accordingly (locals first in both).
- Variables are laid out in `SymId` order; `llc` lays them out in the
  order of their definitions in the text. `hod_ll` numbers a named
  symbol at its first mention, which can come before its definition, so
  the two orders can differ (1 corpus module per LLVM major: RTTI of a
  local class). A follow-up for `hod_ll` (number the definitions in file
  order first) would close it.
- `hod` writes no `STT_FILE` symbol, no `$d` mapping symbols in data
  sections, and sets `SHF_INFO_LINK` on `.rela` sections.
- A COMDAT signature symbol `hod` makes is defined in the group's first
  member section; `llc`'s is in `.group`.
- `hod` keeps relocations that `llc`'s assembler resolves inside one
  section (a `bl` to a local function in the same section).
- A `.L<name>$local` symbol appears in the symbol table when a reference
  through a local alias cannot use the section symbol (a mergeable
  section with a non-zero addend); `llc` writes the same symbol.
- `llc` replaces a load of a GOT-equivalent private constant
  (`@x = private unnamed_addr constant ptr @y`) by a `GOTPCREL` of `@y`
  in a difference; `hod` keeps the constant (both are correct).
- x86-64 (WP-143): none beyond the above; §5.6.6 gives the target's
  rules.

#### 9.7.1 Module assembly of symbol directives (WP-075)

On x86-64, `emit_module` refuses module assembly that
`module_asm_effects` (`spec/ir.md` §2.3) refuses, with `module:asm`,
before it emits anything; on AArch64 such text is §9.7.3's.
Once every module symbol is defined, it writes each effect into the
object: an undefined symbol for a name the module does not have, the
binding, visibility and type a directive gives, and `retain` for a
declaration the text names, so that it is in the symbol table even if no
relocation refers to it. A directive makes no relocation and changes no
section. The oracle is `crates/hod_cg/tests/emit_asm.rs`: the sections,
symbols and relocations of hod's object equal `llc`'s (19 and 22), which
must compile each case without a diagnostic; what `llc` warns about or
refuses is refused.

#### 9.7.2 Copies of `available_externally` functions (WP-021)

`emit_module` writes nothing for an `available_externally` function or
alias: each stays an undefined symbol, as in `llc`'s object. When it is
given a chunk for an `available_externally` function whose attributes
are `alwaysinline` (the session gives one only for a function a body of
the object calls directly, `spec/cli.md` "Always-inline"), it places the
code as a copy:

- a new symbol `<name>.alwaysinline` (`hod_cg::COPY_SUFFIX`; GNU
  demanglers print it as `[clone .alwaysinline]`), `STB_LOCAL`,
  `STT_FUNC`, sized to the code, in `.symtab` like any local function;
- in the text section the function's own name selects (`.text` or
  `.text.<name>` under `function_sections`), in no group: a copy is
  private to the object, so nothing may discard or merge it;
- its unwind entry and its DWARF subprogram are written as any
  function's (both refer to section symbols and offsets, not to the
  function's symbol);
- `R_AARCH64_CALL26`/`JUMP26` (x86-64: `R_X86_64_PLT32`) relocations of
  the object's code to the function go to the copy, resolved when every
  chunk is placed; every other relocation (an address, data, an alias)
  stays on the external symbol, which stays undefined.

A function whose chunk is not given keeps `llc`'s layout exactly.

#### 9.7.3 Module assembly of code and data (WP-112, `emit/module_asm.rs`)

On AArch64, module text that `module_asm_effects` refuses is assembled
by `hod_asm::module::assemble_module` (`spec/asm.md` §9) with the base
features, in `ModuleEmitter::new` before any function, variable or
section of the module is placed, so its bytes come first in each
section as in `llc`'s object. A refusal is `emit_module`'s error, by
the `asm:*` name. Then:

- **Sections.** A standard name (`.text`, `.rodata`, `.data`, `.bss`)
  is the section module emission would choose for that kind; another
  name is found or created as an explicit section of that kind, no
  group. The section is empty at that point (else an internal error);
  its alignment is raised to the text's, its bytes (or `.bss` size)
  appended, and a code section's mapping symbols written; the
  emitter's mapping state is the text's last, so the next function in
  `.text` writes `$x` only if the text ended in data.
- **Symbols.** A label or `.set` of a name the module does not have is
  a new object symbol (local unless `.globl`/`.weak`; visibility and
  type from the directives; `.set` absolute; `.size` as written; kept in
  the symbol table). If its name is a runtime function's
  (`RuntimeFn`), the code's references to that function use it. A label
  named as a module symbol must be an external declaration (a function
  or a non-thread-local variable) and becomes its definition: placed at
  the label, binding global or weak as the text gives it (a label
  of local binding for a module name is `asm:symbol`: `llc` would
  reach it through its section), visibility the IR's if not default (it is emitted after the
  text), else the text's; type and size the text's. Any other meeting
  (a definition, a local, a thread-local, a `.set` of a module name, a
  name prefixed `__unnamed_`) is `asm:symbol`. Directives on module
  names the text does not define follow §9.7.1's rules. Definitions
  are applied after `define` (`finish_module_asm`).
- **Relocations** of the text are the object's relocations, against
  the module symbol, the new symbol, or the section with an addend
  (`spec/asm.md` §9).

The oracle is `crates/hod_cg/tests/emit_module_asm.rs` (sections with
bytes, symbols, relocations, code-section mapping symbols equal to
`llc` 19's and 22's, LLVM 19's alignment of a created code section
normalised from 1 to 4); the three corpus modules with code and data in
their text link (bfd, lld, mold) and behave as `llc`'s.

## 10. Errors and limits

```rust
pub enum CgError {
    Unsupported { construct: &'static str },   // a named limit; a fallback reason
    Internal(String),                          // a bug in hod or invalid IR
}
```

- `classify` decides what is supported, before code generation
  (`spec/ir.md` §8). Code generation reports `Unsupported` only for what
  cannot be known until code exists: `limit:frame-size`,
  `limit:function-size`, and limits of the same kind added with a test;
  also the `call:` reasons for a call the assignment cannot place
  (`call:musttail-ret-memory`, `call:part-register-class`,
  `call:byval-not-on-stack`). A function has at most 1 GiB of code
  (`MAX_FUNCTION_SIZE`), a chunk less than 2 GiB.
  The facade turns it into the coverage-log reason (`spec/cli.md`
  "Reasons").
- `Internal` means a lowering met a case `classify` should have rejected,
  or an invariant of §4 failed. It is reported as an internal error, with
  the function's name; in hybrid mode the module still falls back. It is
  never a panic and never silently wrong code.
- A panic in code generation is a bug. `debug_assert!` guards invariants;
  conditions that depend on the input are checked in release builds too.

## 11. Parallelism, determinism, memory

- `compile_function` reads `&Module` and `&Body` and writes only to state
  its worker owns (`FnCx` scratch, a `Chunk`). Module tables are read-only
  while any function is being compiled.
- A function's rows depend only on the module, the body, the options and
  the target: not on the worker, the chunk, addresses, hashing order or
  time. Together with §9.7 this makes the object identical for any thread
  count.
- Scratch state (location table, register state, labels, frame) is reset
  between functions, not freed; a `Chunk`'s tables grow and are handed to
  the merge. Steady state allocates nothing per function and nothing per
  instruction. Peak memory is the chunks not yet merged plus one body per
  worker.
- The scheduler (worker threads, jobserver, chunk size, back-pressure so
  that unmerged chunks stay bounded) is a later work package; nothing
  above depends on it.

## 12. Tests

- **Policy, without a real machine.** `hod_cg` has a test target: a small
  register machine (a handful of registers per class, so displacement
  happens constantly) whose code a test interpreter executes. Bodies from
  the IR generator (`spec/ir.md` §11) are compiled for it and run, and
  the results compared with a direct interpretation of the same IR. This
  checks §4 and §6 (every location state, spills at calls, the `phi`
  protocol including swaps and self-loops, far-branch retry, unreachable
  blocks) independently of any instruction encoding.
- **Unit:** the frame allocator; labels and fixups; `sym_access` against
  `llc` (§7.4); record contents for hand-built bodies; restart leaves no
  trace in the chunk; the same functions in one chunk and in many give
  the same object (`tests/emit_merge.rs`).
- **Module emission** (§9.7): snippets against `llc` 19 and 22, with
  `llc`'s own code for the functions (`tests/emit_llc.rs`); a data module
  linked by GNU ld, lld and mold and run against `llc`'s
  (`tests/emit_link.rs`); every corpus module with its bodies stripped
  against `llc` (`tests/emit_corpus.rs`, `HOD_CG_CORPUS`); each refusal by
  name; time and peak memory against `llc` (`tests/emit_bench.rs`).
- **L4** (`xtask irdiff`): each instruction-group work package adds
  `tests/ir/` programs for its group and flips their expected-unsupported
  directives; behavior must equal `llc`'s with every linker.
- **L5**: `assign` against clang, both directions (`abi-<target>.md`).
- **Stress options**, for tests only: the pool restricted to its minimum
  (§2.1); `far` forced on; every register value displaced after each
  instruction. A program must behave the same under all of them.
- **Cases the design was reviewed against**, each a named test when its
  work package lands: a dead instruction whose operands are register
  values, and one whose operand is `Deferred` (§4.2); a value held across
  a `returns_twice` call with a later call that `longjmp`s (§4.3); an
  `invoke` with stack arguments caught in a loop a million times (§5.3);
  `musttail` that swaps two stack arguments (§7.1); `resume` in a
  function with landing pads (§7.1); an interposed function with a
  `blockaddress` table (§9.6); an address-taken block no `indirectbr`
  lists (§6.1).
- **Performance** (`xtask bench`): time and peak memory per phase against
  `llc -O0`; bytes of code against `llc -O0` as a tracked ratio, not a
  gate.

## 13. Open questions

- AArch64: the shared trap that `hod_cg` emits for unreachable
  address-taken blocks comes after the exit sequences, so it is covered
  by the CFI state at the end of the last epilogue (CFA `sp + 0`), not
  the body's. Harmless for a trap; revisit with unwind tables (WP-038).
- AArch64: `attr:target-features` stays refused: the target must read it
  (refuse a feature set without FP/SIMD, accept the default ones). Until
  then everything kiln emits with features is refused there.

- (Closed by WP-069.) A direct call's attributes: §7.1, "Effective
  attributes", measured against `llc` 19 and 22.
- (Closed by WP-115.) Address-taken blocks without an `indirectbr` in
  their function (§9.6).

- (Closed by WP-183.) Stack protector attributes (`ssp`, `sspstrong`,
  `sspreq`): implemented, §5.6.24.
- A register cache for home values within a block (keep the register
  after the store, forget it at the next call or label). Cuts reloads;
  adopt only on a `xtask bench` measurement.
- Callee-saved registers for values live across calls (a second prologue
  shape). Same condition.
