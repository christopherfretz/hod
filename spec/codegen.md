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
| `dwarf` | each register's DWARF number, for CFI |

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
value's use has been emitted). A spill slot that is occupied at a call to
a `returns_twice` function is never reused: the code after such a call
can run a second time and reads the slot again. The choice of register
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
lowering, never an obligation, and is the only peephole in v1.

### 4.7 Dead results and undefined operands

- An instruction with no use is skipped when it has no effect: arithmetic,
  compares, casts, `PtrAdd`, `select`, `freeze`, the aggregate and vector
  element instructions, and a load that is neither volatile nor atomic.
  Everything else is emitted and its result dropped.
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
  stack has its incoming location as its home (`FrameRef::Incoming`); a
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
2. the body's stack slots (`Body::slots`), laid out before the first
   instruction, so `Value::slot(n)` is a constant from the start;
3. home slots, incoming slots and spill slots, in order of first need.

`FrameRef::Incoming(offset)` names the caller's stack-argument area.

**Over-aligned slots.** A stack slot whose alignment exceeds the stack
alignment is not in the local area. The entry code allocates it
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
arguments and raises it after the call returns. Calls without stack
arguments, the usual case, emit neither. The canonical frame address is
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
at the end of the function.

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
the function's compile fails internally with "branch out of range", the
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

How code reaches a symbol depends only on the module (PIC and PIE level,
code model) and the symbol (linkage, visibility, `dso_local`, thread-local
model, whether it is defined here):

`sym_access(module, sym) -> Direct | Got | Tls(model)`, and for calls
`Direct | Plt`. It is one target-independent function in `hod_cg`,
following the rule LLVM's `TargetMachine::shouldAssumeDSOLocal` applies,
and tested against the relocations `llc` emits for the same module and
symbol over a grid of linkages, visibilities and PIC levels. Each target
has one access sequence per answer.

### 7.5 Constants in code

An integer constant is materialized by the encoder's shortest sequence
(`mov_imm`), a floating-point constant by its bit pattern through the
encoder's FP materialization (`spec/mc-a64.md`). A constant too wide for
that (a vector, `fp128`) is placed in a **data island** after the
function's code, inside the function's own section, and loaded
PC-relative through an internal fixup: no relocation, no separate section,
nothing shared between functions. Islands are aligned to their contents
and counted in the function's size.

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
| records | `reloc`, `island`, `cfi`, `set_frame_desc`, `type_id`, `pad_clause`, `pad_filter` | a relocation (§9.2), a data island (§7.5), CFI rows and the frame description (§9.3), type ids and clauses (§9.4) |

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
| `call_sites` | `(start, length, pad, landing)` | `hod_eh` (§9.4) |
| `pads`, `clauses`, `type_ids` | §9.4 | `hod_eh` |
| `lines` | `(offset, LocId)` | `hod_dwarf` (§9.5) |
| `vars` | `(variable, expression, declared-at, location)` | `hod_dwarf` (§9.5) |
| `labels` | `(label, offset)` | `hod_dwarf` (§9.5) |
| `islands` | `(offset, length)` | mapping symbols (§5.5) |
| `block_offsets` | `(BlockId, offset)` | `blockaddress` in data (§9.6) |

`FuncRecord`: the function's `SymId`; its ranges in each table; its code
alignment; its frame description (§9.3); whether it has landing pads.

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
than relying on remembered state. The rows are
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
  order: the instruction's code range, its pad (none for a plain `call`
  or a runtime call) and its **landing offset**: the pad's block, or the
  pad stub of an `invoke` with stack arguments (§5.3). The pad gives the
  row's action, the landing offset its landing address. `hod_eh` merges
  adjacent rows that agree in both and writes the table; a call that can
  throw and has no pad must still be covered by a row, or the personality
  routine terminates the program instead of unwinding through.

### 9.5 Debug information

Recorded only when the module has debug information.

- `lines`: a row when the `LocId` of the instruction being emitted differs
  from the previous instruction's, `LocId::NONE` included. The code
  generator records transitions only; which rows become line-table
  entries, where `prologue_end` goes and what a location-less instruction
  inherits is `dwarf.md`'s policy. The entry code of §4.8 and the prologue
  have no row of their own, so they precede the first row. `end_function`
  adds a row at the epilogue: the location of the function's `ret` if
  exactly one was emitted, `LocId::NONE` otherwise, so that the shared
  epilogue does not inherit the line of whichever block was emitted last.
- `vars`: one row per `#dbg_declare`: the variable, the record's
  expression, its declaration location, and where the declared address
  is: a frame-base offset (a stack slot), a frame-base offset holding the
  address (an over-aligned slot, §5.1), or the home of a value that is
  not a slot (an `sret` argument, a dynamic `alloca`; §4.1 gives it a
  home). `hod_dwarf` composes the location with the expression. Several
  rows may name one slot (structured bindings do), each with its own
  expression. At `-O0` a row's location holds for the whole function. The
  body is dropped after its function is compiled, so everything
  `hod_dwarf` needs from it is copied here.
- `labels`: one row per `#dbg_label` in a reachable block: the label and
  the code offset of the instruction that follows it.
- `#dbg_value` records describe values that policy v1 keeps in registers
  for a few instructions; unoptimized input does not contain them. v1
  records the ones whose value has a home (as a location valid from the
  definition to the end of the function) and drops the rest: the variable
  then shows as unavailable, which is not wrong. The count dropped is
  reported in the coverage log.

### 9.6 Block addresses

For a function that contains an `IndirectBr` or has blocks the producer
marked address-taken, `block_offsets` gives the code offset of each
reachable block (and of the trap that stands for an unreachable one,
§6.1). A `blockaddress` in a global initializer (`Fixup::Block`,
`spec/ir.md` §4.3) becomes a relocation against the **section symbol** of
the function's section, with the function's offset in the section plus
the block's offset as addend, as LLVM emits it. It must not be against the
function's own symbol: that symbol may be preemptible, and the table
would then point into whatever definition the dynamic linker chose. A
`blockaddress` in code is formed PC-relative by the function itself
(§6.3).

### 9.7 Merge and module emission

`hod_cg::emit_module` (its own work package; summarized here because the
record must serve it):

- Before any function is compiled, every module symbol is added to the
  object as undefined, in `SymId` order, so that a `SymId` maps to its
  `hod_obj::SymbolId` by arithmetic and relocations can be written without
  a lookup (`spec/object.md` "Symbols").
- Chunks are merged in module order. A function goes to `.text`, or to its
  own section when it has an explicit section, is in a COMDAT, or
  `function_sections` is set (named as `llc` names it: `.text.<symbol>`);
  COMDAT members join their group. Its symbol is defined with the code's
  size; relocations and island mapping symbols are rebased to the section
  offset.
- Variables are flattened (`ConstPool::flatten`) into the section their
  linkage, constness, thread-local model and relocations call for. A
  `Fixup` becomes a relocation as `resolve` in
  `crates/hod_ir/tests/flatten_diff.rs` does it; a `Diff { a, b }` is
  encodable only when both symbols are in one section or `b` is in the
  field's section, and the variable is `Unsupported` otherwise.
- Aliases, IFUNCs, constructors and destructors, retained symbols and
  linker options follow `spec/object.md`.
- The result depends only on the module and the options: not on the
  number of workers or on how functions were divided into chunks.

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
  `limit:function-size`, and limits of the same kind added with a test.
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
  the same object.
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

- Stack protector attributes (`ssp`, `sspstrong`, `sspreq`): accept and
  ignore (hod is a debug backend; the IR's meaning is unchanged) or
  implement the canary. Until decided `classify` rejects them.
- A register cache for home values within a block (keep the register
  after the store, forget it at the next call or label). Cuts reloads;
  adopt only on a `xtask bench` measurement.
- Callee-saved registers for values live across calls (a second prologue
  shape). Same condition.
