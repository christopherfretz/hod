# hod architecture

hod turns one translation unit's IR into one relocatable object file. It is
the debug-build backend for [kiln](https://github.com/christopherfretz/kiln):
kiln lowers C++ to LLVM IR, and today LLVM's `llc` turns that IR into an
object. hod replaces `llc` on that path. It does not optimize.

This document is the map. Contracts live in `spec/`, the plan in
`docs/ROADMAP.md`, how correctness and speed are established in
`docs/VALIDATION.md`, and the reasoning behind the larger decisions in
`docs/design/`.

## Principles

1. **Correct, or loudly unsupported.** hod either emits correct code for a
   construct or reports `Unsupported` with the construct's name. It never
   guesses, and never emits code for IR it does not fully understand. The
   driver can then fall back to LLVM for that translation unit (see
   "Fallback"), so partial coverage is usable and measurable from the start.
2. **Speed and memory are tested properties.** Wall-clock time and peak
   memory of code generation are measured by the harness against recorded
   baselines and against `llc -O0`, and a regression fails like a test.
3. **Safe Rust end to end.** Every crate is `#![forbid(unsafe_code)]`.
   `unsafe` is admitted only for a measured need, inside a small separately
   reviewed component, with the decision recorded in the spec. No LLVM
   linkage of any kind: LLVM's tools are used only as test oracles.
4. **No third-party dependencies in the shipped crates** unless approved in a
   work package and recorded here. (None so far.)
5. **Deterministic.** The object file depends only on the input and the
   options: not on thread count, hash iteration order, addresses or time.
6. **Specs over folklore.** Each crate has a contract in `spec/` with
   behavioral tests tied to it. Re-implementing a crate from its spec and
   tests must be possible.
7. **Debuggable output is part of "correct".** DWARF that gdb and lldb use,
   unwind tables that the C++ runtime and profilers use, and objects that
   GNU ld, lld, mold and wild all link.

## What "does not optimize" means

hod's input is unoptimized, clang-shaped IR: locals live in `alloca`s, most
SSA values are used once, right after they are defined. hod generates code in
that shape, one IR instruction at a time. It does the local things that cost
nothing at compile time and keep the output from being needlessly large
(keeping a just-computed value in a register for its single use, folding a
compare into the branch that follows, addressing a stack slot directly). It
has no pass that exists to make the output faster.

## Pipeline

```
LLVM text IR ──── hod_ll ───┐
LLVM bitcode ──── hod_bc ───┤  (later)
kiln's IR model ─ kiln_hod ─┤  (lives in kiln; in-process, no text)
                            ▼
                         hod_ir        compact owned IR, LLVM semantics,
                            │          delivered one function at a time
                            ▼
                         hod_xform     lowerings the IR requires before
                            │          codegen (coroutine split, ...)
                            ▼
              hod_cg + hod_a64 | hod_x64     one function → machine code,
                            │                relocations, unwind and line rows
                            ▼
              hod_obj  (+ hod_eh, hod_dwarf)    ELF relocatable object
```

There is no machine IR, no assembly text, and no separate assembler step
between the IR and the bytes.

## Crates

| crate | role | depends on |
|---|---|---|
| `hod_ir` | The IR hod consumes: types, constants, globals, functions, instructions, attributes, debug metadata. Index arenas (`u32` ids), no pointer graphs. A builder API for producers, a printer (LLVM text) for debugging and for module splitting, and `classify`: the single place that decides whether a construct is supported. | - |
| `hod_ll` | LLVM textual IR parser → `hod_ir`. Accepts the LLVM 19 to 22 dialects. | `hod_ir` |
| `hod_bc` | LLVM bitcode reader → `hod_ir`. (Later.) | `hod_ir` |
| `hod_obj` | Object-file model and the ELF64 relocatable writer: sections, COMDAT groups, symbols, relocations, string tables. Knows nothing about code generation. | - |
| `hod_mc_a64` | AArch64 instruction encoder: typed constructors → 32-bit words, plus fixup kinds. No IR knowledge. | - |
| `hod_mc_x64` | x86-64 instruction encoder: typed constructors → instruction bytes with their PC-relative site, plus fixup kinds. No IR knowledge. | - |
| `hod_cg` | Target-independent code generation: the walk over a function, frame layout, value locations, the call-lowering driver, the per-function output record, and the scheduler that runs functions on worker threads and merges results in order. Defines the `Target` trait. `emit_module` writes the unwind tables through `hod_eh` and the line tables and unit DIEs through `hod_dwarf`. | `hod_ir`, `hod_obj`, `hod_eh`, `hod_dwarf`, `hod_xform` (coroutine analysis, `spec/codegen.md` §7.6) |
| `hod_a64` | AArch64 `Target`: instruction lowering, AAPCS64 for IR-level types, intrinsics, TLS and GOT access sequences. | `hod_cg`, `hod_mc_a64` |
| `hod_x64` | x86-64 SysV `Target`. | `hod_cg`, `hod_mc_x64` |
| `hod_eh` | `.eh_frame` (CFI) and `.gcc_except_table` (LSDA) from the per-function output records. | `hod_obj` |
| `hod_dwarf` | DWARF 5: `.debug_info`, `.debug_abbrev`, `.debug_line`, `.debug_str`, `.debug_line_str`, `.debug_addr`, `.debug_rnglists`, from the IR's debug metadata plus the per-function line rows and variable locations. | `hod_ir`, `hod_obj` |
| `hod_xform` | IR → IR lowerings that are required for correctness, not speed: coroutine splitting, and whatever else the IR contract requires before codegen. | `hod_ir` |
| `hod_asm` | Assembler-syntax parser for `module asm` and inline `asm` bodies. (Later.) | `hod_mc_*` |
| `hod` | The library facade: `compile(module, options) -> Result<Object, Unsupported>`. | all of the above |
| `hod_cli` | The `hod` binary: an `llc`-compatible command line, so it drops into any driver that runs `llc`. | `hod`, `hod_ll` |
| `hod_interp` | A reference interpreter for `hod_ir`, for tests: the oracle for the code generator's policy tests and for fuzzing; and a generator of random programs that it runs to a result. Not shipped. | `hod_ir` |
| `xtask` | The harness: differential runs, linker matrix, benchmarks, bisection, CI entry point. Not shipped. | - |

Dependency direction is top-down in the pipeline. The encoders and the
object writer are leaves with no knowledge of the IR, so they are specified,
implemented and tested in isolation against LLVM's assembler and `readelf`.
`hod_cg` talks to a target only through the `Target` trait, and to the
debug-info and unwind writers only through the per-function output record.
Those three interfaces (`hod_ir`'s builder, `Target`, the output record) are
what let independent work proceed in parallel; they are fixed in `spec/`
before the work that depends on them starts.

## Input

The decision and its measurements are in
[design/0001-input.md](design/0001-input.md). In short:

- hod's input is **LLVM IR semantics** held in hod's own compact model,
  `hod_ir`. LLVM IR is the right level: kiln's lowering to it is large,
  mirrors clang, and already encodes the C++ ABI; nothing below it is
  frontend-independent, and everything above it would have to be redone in
  the backend.
- There are three ways in. **Text** (`hod_ll`) makes hod an `llc` drop-in
  that works with kiln today, with clang's output, and with every test
  oracle. **In-process** (`kiln_hod`, a bridge crate in kiln) copies kiln's
  in-memory IR into `hod_ir` one function at a time, skipping the IR printer,
  the pipe, the parser and a process spawn; this is the path that matters
  for speed. **Bitcode** (`hod_bc`) comes later, for IR that passed through
  LLVM's `opt`.
- Producers deliver **module-level declarations first, then functions one
  at a time**. A function's IR can be dropped as soon as its code exists, so
  peak memory follows the largest function, not the module.

## Code generation

One function at a time, in **one walk** over its IR; the contract is
`spec/codegen.md`. What an analysis walk would compute is already in the
IR: `hod_ir`'s builder maintains each value's use count and whether it
leaves its block, and a function's fixed stack objects are a list in its
header (`spec/ir.md` §5). For each block in layout order, for each
instruction, the target selects, allocates and encodes in one step,
appending bytes to the function's code. Branches within the function are
fixed up when their target is emitted; references to other symbols become
relocations.

**Value locations.** A value used once, in the block that defines it,
lives in a register from its definition to its use. Every other value has
a home slot in the frame: it is stored when defined and loaded at each
use. No register holds a value across a block boundary, so there is no
register state to reconcile on control-flow edges; a `phi` is a slot that
predecessors store to and the block loads from. A call displaces whatever
registers still hold values to the frame. This is allocation policy v1,
chosen because it is simple to get right and matches the input (most
values at `-O0` are single-use). The policy sits behind an interface in
`hod_cg`; a policy that keeps values in registers across blocks can
replace it without touching the targets.

**Frame.** Always a frame pointer. Stack objects, home slots and spill
slots are addressed from the frame pointer at constant offsets. The frame
size is known only after emission, so the prologue is emitted in a
fixed-size form and patched, and each function has one epilogue, at its
end. One prologue shape per target keeps the unwind information a
constant pattern plus the frame size.

**Calls.** The frontend has already applied the C++ ABI (`sret`, `byval`,
coerced aggregates). The target assigns IR-level argument types to registers
and stack exactly as LLVM does for the same signature, including varargs,
`[N x T]` and struct arguments passed as first-class values, and `i128`. The
rules are written down per target in `spec/` and tested by linking hod's
objects against clang's in both directions.

**Per-function output record.** Code bytes, relocations, the symbol's
properties, call-site ranges and landing pads for the LSDA, the frame size
for the CFI, line rows `(offset, file, line, column)`, and variable
locations (at `-O0`, a `#dbg_declare` of a frame slot is one frame-relative
location for the whole function).

**Parallelism.** A function's record depends only on that function and the
module-level tables, which are read-only during code generation. Worker
threads take functions; the merge concatenates records in module order, so
output is identical for any thread count. Build systems already run one
compiler per core, so the default is to take extra threads only from the
GNU make jobserver when one is present, or from an explicit option.

**Memory.** Each worker owns scratch arenas that are reset, not freed,
between functions. Section contents are written once, in final form. The
target is peak memory proportional to the output size plus the largest
function.

## Object file

ELF64 little-endian relocatable objects for `aarch64-unknown-linux-gnu`
(first; it is the development machine) and `x86_64-unknown-linux-gnu`.
The writer supports what C++ needs: `-ffunction-sections` /
`-fdata-sections`, COMDAT section groups, weak and hidden symbols, TLS
sections and all four TLS models, `.init_array` / `.fini_array` with
priorities, IFUNC symbols, aliases, mergeable string sections,
`.note.GNU-stack`, `.note.gnu.property` (BTI/PAC/CET markings),
`.eh_frame`, `.gcc_except_table`, and the `.debug_*` sections. Mach-O and
COFF are out of scope until the ELF targets are done; `hod_obj`'s model is
kept format-neutral so they can be added.

## Fallback

`hod` returns `Unsupported { construct, function }` instead of an object when
the module contains anything `hod_ir::classify` rejects. The `hod` binary,
run as `llc`, can then re-run the real `llc` on the same input
(`--fallback=<llc>`) and append one line to a coverage log. This gives three
modes with one mechanism:

- **hybrid** (development): every test suite runs end to end from the first
  milestone, and the fraction of translation units and functions hod handled
  is a number that rises over time;
- **strict** (gates): `Unsupported` is a failure;
- **bisect** (debugging): the harness splits a module into "functions for
  hod" and "functions for llc", links the two objects, and binary-searches
  for the function hod miscompiles.

## Source tree

```
Cargo.toml          workspace
crates/<crate>/     one crate per row of the table above
spec/               one contract per crate or cross-cutting topic
tests/              IR and source inputs by category (docs/VALIDATION.md)
bench/              benchmark inputs and recorded baselines
docs/               this file, ROADMAP, VALIDATION, AGENTS, BACKLOG, design/, status/
xtask/              the harness
```
