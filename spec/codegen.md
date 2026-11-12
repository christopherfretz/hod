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

- a thread-local symbol as an operand of anything but
  `llvm.threadlocal.address` (§3);
- a `blockaddress` in a body that names a block of another function
  (§9.2);
- `naked` functions (§5.4) and `callbr`;
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
| `holdable` | registers a value may rest in: `pool` plus the registers argument placement and returns write. An incoming argument rests in its argument register and a call's result in the return register until used or displaced |
| `scratch` | at least two general and one vector register that never hold a value across instructions. They belong to whatever is being emitted right now: glue (§8.2), large offsets, phi copies. On AArch64 these include `x16`/`x17`, which a linker veneer may clobber at any call |
| `eh_data` | the two registers a landing pad receives (exception pointer, selector) |
| `dwarf` | each register's DWARF number, for CFI (a function from `PReg`) |
| `callee` | the scratch register an indirect callee's address is brought into (§7.1) |
| `gpr_bytes`, `fpr_bytes` | the size of a register of each class: the size of its home and spill slots |

Never touched: the platform register (`x18`), the thread pointer. `pool`
and `holdable` are disjoint from `scratch`.

**Save set.** A target may name a fixed set of callee-saved registers that
its prologue saves and its epilogue restores in *every* function, so the
prologue keeps one shape (§5). They are then ordinary pool registers;
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
`store` writes that and `load` may rely on it. A frame slot hod makes for
a `Gpr` value is a full register wide, written and read whole, so the
invariant carries through a spill.

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
lowering, argument placement included. A thread-local address, whose
access sequence may call and clobber argument registers, is therefore
never an operand: it is the result of `llvm.threadlocal.address` (§1),
lowered as an instruction.

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
use, so blocks may be laid out in any order. Home slots are never reused.

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
does not track which ones were occupied at the call). The choice of register
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
terminator's lowering does not.

No edge is ever split and no parallel copy is ever sequenced: the values
read (locations) and the slots written (incoming slots) are disjoint, so
the "swap" and "lost copy" problems cannot occur, a conditional branch,
`switch` or `indirectbr` needs no per-edge code, and a write for an edge
that is not taken is dead (every edge into a block rewrites the slots
before the block reads them).

`invoke` is the one supported terminator whose result can be a
successor's incoming value. Its unwind destination's incoming slots are
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
3. home slots, incoming slots and spill slots, in order of first need.

`FrameRef::Incoming(offset)` names the caller's stack-argument area.

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
  (else `a64:constant-relocation`). Symbol addresses follow §7.4:
  `adrp`+`add` with the addend folded, or `adrp`+`ldr` from the GOT then
  the addend added; a code model other than small is `a64:code-model`.
- **Other reason names of the target**: `a64:atomic`,
  `a64:vector-element` (no package yet), `a64:thread-local` (TLS, M2),
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
- **Symbol offsets** are folded into the relocation's addend when the
  total is below 2^20 in magnitude, as `llc` does; beyond it the offset
  is added after the address (§7.4).
- **Widths.** An access reads or writes exactly the store size. Sizes 1,
  2, 4, 8 are one access; 3 is `h` + `b`, 5 `w` + `b`, 6 `w` + `h`, 7
  `w` + `h` + `b`, joined on a load by `orr … lsl` through `x16`, split
  on a store by `lsr` into `x16`. A store of a width that is not a
  multiple of 8 masks the value in its register first (`and`; the
  register is the store's own operand, §4.2), so padding bits are zero.
  A zero constant is stored from `xzr`/`wzr`.
- **Volatile** accesses are lowered as plain ones; `hod_cg` never drops
  or repeats them. Atomic loads and stores are `a64:atomic`.
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
- **`switch`** zero-extends the condition in its register (`ubfx`, below
  64 bits), drops the cases that go to the default, sorts the others by
  unsigned value (skipped when they come sorted) and emits a binary
  search: a node compares with its middle case (`b.eq` to its block,
  `b.hi` to the upper half), a run of at most four cases is a chain of
  `cmp` · `b.eq` ending in `b default` (or `b.ne default` when the last
  case's block comes next). A case value is compared by `cmp #imm`,
  `cmn #-imm`, or through `x16`. Code is linear in the cases; a run does
  about `log2(n) + 4` compares; no jump table and no data island. A
  constant condition is a `br`. A `switch` wider than 64 bits is
  `a64:switch-wide` (`classify` supports `inst:switch` for `int` only).
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
  the named arguments is never written.
- **Landing pads** are `{ ptr, i32 }` only: `x0`/`x1` stored to the
  pad's home; clauses are `catch` of a symbol or `null` and `filter` of
  such arrays. `resume` calls `_Unwind_Resume` with the exception
  pointer, then traps.
- **Refused by name**: `a64:call` (`callbr`), `a64:call-inline-asm`,
  `a64:call-bundle`, `a64:invoke-odd-int-result` (an `invoke` whose
  `Mem` result needs leaf clearing on the normal edge),
  `a64:va-intrinsic`, `a64:va_arg`, `a64:landingpad`,
  `a64:landingpad-clause`, `a64:resume`. `a64:variadic-definition` is
  no longer produced; WP-069 removed `a64:call-wide-int-extension` and
  `a64:call-narrow-ext-stack-arg` (`abi-a64.md` §7.1) and
  `a64:call-mismatched-declaration` (§7.1 here).
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
- **Memory intrinsics.** A length of 0 emits nothing (the pointers may
  be null). A constant length of at most 64 bytes (`memcpy`, `memset`)
  or 32 bytes (`memmove`) is inline and touches exactly the bytes of the
  range: the largest power-of-two accesses, the tail overlapping the
  previous access for copies, exact-size stores for sets. An inline
  `memmove` reads every byte (into `x16`, `x17`, `v30`, `v31`) before
  it writes one, so overlap in either direction is correct; 32 is what
  those four registers hold. 64 keeps the inline code to a few
  loads and stores while covering the common small
  struct copies and clears; beyond it the library routine is as fast
  and the code stays small. Anything longer, and every
  variable length, calls `memcpy`, `memmove` or `memset` through
  `hod_cg`'s runtime-call sequence, so live values are saved as for any
  call; a length narrower than 64 bits is zero-extended first. The
  `.inline` forms never call: a constant length above 64 is a loop of
  16-byte post-indexed `q31` accesses counted in `x16` with an
  overlapping last access, the fill byte replicated by `dup v31.16b`;
  a variable length is `a64:mem-inline-length`. Volatile is lowered as
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
- **Calls**: `sin` .. `log10`, `pow`, `atan2` call libm (`sinf`/`sin`/
  `sinl`); `powi` `__powisf2`/`__powidf2`/`__powitf2`; `ldexp`
  `ldexpf`/`ldexp`/`ldexpl`; `frexp` `frexpf`/`frexp`/`frexpl` with the
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
- **`clear_cache`** calls `__clear_cache`. **`readcyclecounter`** needs
  `mrs CNTVCT_EL0`, which the encoder's `SysReg` lacks: refused
  (`a64:readcyclecounter`).
- `sshl.sat`/`ushl.sat` are not in `hod_ir`'s intrinsic list: classify
  refuses them as `intrinsic:<name>`. Vector forms are `a64:intrinsic-vector`,
  `bfloat` ones `a64:intrinsic-bfloat`.

#### 5.5.2 Target features (WP-043)

`hod_a64::features::check` judges a `target-features` string (a
function's attribute) and each `-mattr` item: items separated by `,`,
each `+name` or `-name`, empty items skipped. hod emits ARMv8.0 code with
FP and Advanced SIMD and never chooses an instruction by a feature, so:

- **Accepted, either sign**: a feature that only makes instructions or
  system registers available. The architecture versions `v8a`..`v8.9a`,
  `v9a`..`v9.6a`; `fp-armv8` and `neon` enabled; the extensions of LLVM
  19's and 22's `AArch64Features.td` (`aes`, `sha2`, `sha3`, `sm4`,
  `crypto`, `crc`, `lse`, `lse2`, `rdm`, `ras`, `rcpc`, `fullfp16`,
  `dotprod`, `i8mm`, `bf16`, `pauth`, `bti`, `mte`, `sve*`, `sme*`, `gcs`,
  ...; the full list is `AVAILABILITY` in `crates/hod_a64/src/features.rs`);
  and `fmv` and `outline-atomics`, which change no code hod emits (no
  function multi-versioning; `classify` refuses atomics). `+bti` and
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

### 5.6 x86-64 (planned)

`push rbp` · `mov rbp, rsp` · pushes of the save set · `sub rsp, imm32`
(patched). Locals at negative offsets from `rbp`, incoming arguments at
`rbp + 16 + n`, so no incoming base slot is needed, and the epilogue
(`lea rsp, [rbp - saved]` · pops · `pop rbp` · `ret`) does not depend on
the frame size. The save set (`rbx`, `r12`..`r15`) is the general pool
(§2.1); `r10` and `r11` are scratch, with `r10` also the `nest` register,
which the first work package must reconcile. The red zone is not used.
Written out with `hod_x64`'s first work package.

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

**Tail kinds.** `tail` and `notail` are permissions and are ignored.
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
  its convention and its function type; its `stack_size`. A call that
  lists more arguments than its type names has no answer (the types of
  the variable arguments are the body's, which the hook is not given):
  a variadic thunk forwards `...`, which the call site does not list, so
  its call is answered.
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
The facade has to hand the hook to its classifier; until it does,
every `musttail` call is `musttail:stack-args-unknown` there.

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
`byval` argument points to, copied into the stack-argument area);
`Assignment { args, ret, ret_memory, sret_returned_in, stack_size, va,
vector_regs_used }`. `ret_memory` is set when a return value does not fit
the return registers and the signature has no `sret` parameter: the
caller then passes the address of a buffer, as LLVM does. A target's
`assign(types, sig, out) -> Result<(), AbiUnsupported>` names what it
cannot pass (`abi:<what>`); the facade runs it over a module's signatures
as part of deciding support, so code generation never meets a failure.

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

### 7.5 Constants in code

An integer constant is materialized by the encoder's shortest sequence
(`mov_imm`), a floating-point constant by its bit pattern through the
encoder's FP materialization (`spec/mc-a64.md`). A constant too wide for
that (a vector, `fp128`) is placed in a **data island** after the
function's code, inside the function's own section, and loaded
PC-relative through an internal fixup: no relocation, no separate section,
nothing shared between functions. Islands are aligned to their contents
and counted in the function's size. (AArch64 builds `fp128` in the register instead, §5.5.1.)

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
  expression. At `-O0` a row's location holds for the whole function. The
  body is dropped after its function is compiled, so everything
  `hod_dwarf` needs from it is copied here.
- labels are rows of `vars` too: one `VarPlace::Label(offset)` row per
  `#dbg_label` in a reachable block, the label and the code offset of
  the instruction that follows it.
- a record hod cannot describe exactly is a `VarPlace::Refused(name)`
  row; a `FullDebug` unit then refuses the function under that name:
  `debug:dbg-value` (any `#dbg_value`: clang and kiln do not emit them at
  `-O0`, and none is described yet, `spec/dwarf.md` open question 9),
  `debug:dbg-assign`, `debug:declare-address` (a `#dbg_declare` of a
  constant, a symbol, or a value that never got a home).

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
(§6.3). (`hod_ir` has no address-taken marker on blocks; until it does,
a function without an `IndirectBr` has no `block_offsets`; what a
`blockaddress` in data of one of its blocks refers to is an open
question, §13.)

### 9.7 Merge and module emission

`hod_cg::emit_module(module, &Options, chunks) -> CgResult<Emitted>`
(`src/emit/`), or step by step `ModuleEmitter::new`, `add_chunk` per
chunk in module order, `finish`. The object is laid out as `llc -O0`
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
target's (AArch64: `nop`s).

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
`init:address-width` (an address in a 1-byte field),
`init:dso-local-equivalent` (outside a difference),
`init:blockaddress-unplaced`, `module:linker-option-not-a-pair` (an
`!llvm.linker.options` entry of other than two strings: `llc` writes
`.linker-options` as pairs), `eh:unknown-personality-without-pads`
(`spec/eh.md` §1.1), `debug:emission-kind` (a `LineTablesOnly` or
`DebugDirectivesOnly` unit), `debug:die-unsupported` (a unit with a node
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
- x86-64: no target yet; the PC bias of a code relocation is not
  considered in the mergeable-section addend check, and padding is
  `0x90`.

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
- Address-taken blocks without an `indirectbr` in their function (§9.6):
  `hod_ir` has no marker, so `block_offsets` is not recorded for them.
  Either `hod_ir` marks address-taken blocks, or `hod_cg` scans the
  module's constants for `blockaddress`es of each function.

- Stack protector attributes (`ssp`, `sspstrong`, `sspreq`): accept and
  ignore (hod is a debug backend; the IR's meaning is unchanged) or
  implement the canary. Until decided `classify` rejects them.
- A register cache for home values within a block (keep the register
  after the store, forget it at the next call or label). Cuts reloads;
  adopt only on a `xtask bench` measurement.
- Callee-saved registers for values live across calls (a second prologue
  shape). Same condition.
