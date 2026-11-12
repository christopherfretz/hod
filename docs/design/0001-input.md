# Design 0001: what hod consumes

Status: accepted (2026-11-11). Revisit at the M5 measurement named below.

## Question

kiln lowers C++ to LLVM IR text and pipes it to `llc`. Should hod consume
LLVM IR, or something closer to kiln's internals that never passes through
IR lowering or serialization?

## What kiln has

Read from kiln at `bd60e182` (v1.0.1 + 326 commits):

- `kiln_codegen` lowers the typed AST to `kiln_llvm::Module`, an in-memory
  model of LLVM IR built on `u32` index arenas (24-byte instructions, 4-byte
  values, interned types and constants). Each function is built in a mutable
  per-function buffer and lowered into the module when final.
- The whole module is built, then printed to one text string and piped to
  `llc` on stdin. There is no other serialization.
- The lowering mirrors clang's `-O0` output: locals in allocas, `phi` only in
  a handful of places, Itanium exception handling only, debug info as
  `#dbg_declare` records and `DI*` metadata. The C++ ABI (argument coercion,
  `sret`, `byval`, vtables, mangling) is already applied.
- Some IR needs an LLVM pass before `llc` even at `-O0`: coroutines
  (`llvm.coro.*`, split by LLVM), `alwaysinline`, sanitizers other than
  UBSan, and coverage. In those cases `llc` receives bitcode from `opt`.

## Measurements

One standard-library-heavy translation unit (2085 function definitions),
aarch64, kiln release build, LLVM 22.1.8, single run each, 2026-11-11:

| | `-O0` | `-O0 -g` |
|---|---|---|
| IR text size | 3.9 MB | 10.1 MB |
| `kiln++ -fsyntax-only` | 0.74 s | 0.81 s |
| `kiln++ -S -emit-llvm` (adds IR build and print) | 1.10 s | 1.41 s |
| `kiln++ -c`, front-end span / back-end span | 0.99 s / 0.39 s | 1.15 s / 0.82 s |
| `llc -O0` alone on the text, of which parsing | 0.26 s, 0.05 s | 0.53 s, 0.17 s |

So on this input the LLVM back end is 28% of the compile without debug info
and 42% with it, and with `-g` a third of `llc`'s own time is re-parsing
text that kiln spent time printing. These are single runs on a shared
machine: good for proportions, not for baselines. The benchmark harness
(docs/VALIDATION.md) replaces them.

## Decision

1. **The level is LLVM IR.** Going below it (typed AST straight to machine
   code) would move kiln's whole lowering, tens of thousands of lines that
   mirror clang and encode the ABI, into the backend, and tie hod to one
   frontend. Nothing kiln has between the AST and the IR model is a better
   seam: the per-function buffer is the same IR in a mutable form.
2. **hod owns its IR model, `hod_ir`,** with LLVM semantics and a shape close
   to `kiln_llvm`'s (index arenas, compact instructions), rather than
   reading `kiln_llvm` through a trait or depending on kiln's crate.
   - A trait wide enough to expose all of LLVM IR is a large generic
     surface that every codegen function would be parameterized over. A
     concrete model keeps the codegen simple, quick to build, and easy to
     split among implementers.
   - hod must not depend on kiln (kiln depends on hod), and must accept IR
     from text and bitcode too.
   - The cost is one linear copy per function. Expected to be a few percent
     of code generation; measured at M5 (below).
3. **Three producers, one builder API.** `hod_ll` (text), `hod_bc` (bitcode,
   later) and `kiln_hod` (a bridge crate inside kiln that walks
   `kiln_llvm::Module`) all call the same `hod_ir` builder.
4. **Functions are delivered one at a time**, after the module-level
   declarations. The consumer may compile and drop each function before the
   next arrives. This is what later allows kiln to hand over each function
   as soon as it is final, on a worker thread, while the front end keeps
   going, without ever holding the whole module's IR.
5. **Text first.** The first integration is the `hod` binary accepting
   `llc`'s command line, selected with kiln's existing `--llvm-dir`. It
   needs no change to kiln and makes every `.ll` file from any source a
   test input. The in-process bridge follows once the IR contract is stable
   (M5); it needs a small hook in kiln's driver, to be agreed with kiln's
   maintainers.
6. **IR that today needs an LLVM pass at `-O0`** is hod's to lower
   (`hod_xform`), in this order: `alwaysinline` (no pass needed when the
   definition is in the module: calling it out of line is correct),
   coroutines (a splitting pass of hod's own), coverage. Sanitizers that
   LLVM instruments stay on the LLVM path. Until each lands, such
   translation units take the fallback.

## Beyond LLVM IR: facts the producer already has

"LLVM semantics" is the floor, not the ceiling. `hod_ir` is hod's own format
and is free to carry things LLVM IR cannot say, when a producer knows them
for free and they remove work from hod. The text and bitcode producers
compute them; a producer like kiln, which knows them while lowering, just
states them. Candidates, each to be adopted only on a measurement:

- **The frame up front.** A function header listing its fixed stack objects
  (size, alignment, debug variable) instead of `alloca` instructions to
  find and classify.
- **Resolved addresses.** Field and element addresses as a base plus a
  constant byte offset (plus scaled indices), instead of GEPs whose offsets
  hod recomputes from type layouts.
- **Use facts.** Per value: "used once, by the next instruction" and "does
  not leave its block". With these, the analysis walk has nothing left to
  compute and code generation is a single walk.
- **Call shapes.** Argument and return locations pre-assigned by the
  frontend's ABI layer, which already classified every argument.
- **No names, no text.** Symbols by id; value names not transmitted at all.

The first producer to supply them is the in-process bridge; the step after
that is kiln's lowering writing `hod_ir` directly, function by function,
with no intermediate LLVM-shaped module. That removes the copy in point 2
and one whole IR build from the compile. The `hod_ir` spec (WP-002 to
WP-005) is written with this end state in mind, and its design is reviewed
with kiln's maintainers before it is fixed.

## Measurement that can overturn point 2

At M5, on the benchmark corpus: time spent copying `kiln_llvm` → `hod_ir`
as a fraction of hod's total. If it exceeds 5%, evaluate having
`kiln_codegen` build `hod_ir` directly, or a zero-copy view.

## Prior art

- TPDE (Schwarz et al., CGO 2026, arXiv 2505.22610): an adapter over
  an existing SSA IR, one analysis walk and one walk that selects, allocates
  and encodes together. Reports 8x to 26x lower back-end compile time than
  LLVM `-O0` on SPECint 2017 at similar run time, with 10% to 30% larger
  code. hod's two-walk design follows it; hod differs in owning its IR and in
  treating debug info, unwind tables and full C++ coverage as requirements.
- Cranelift, LLVM FastISel + fast register allocator, QBE, tcc, and
  copy-and-patch were considered. All but tcc build a machine IR that hod
  avoids; tcc has no SSA input.
