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
6. **IR that today needs an LLVM pass at `-O0`** is hod's to handle, in
   this order: `alwaysinline`, coroutines (a splitting pass of hod's own,
   `hod_xform`), coverage. Sanitizers that LLVM instruments stay on the
   LLVM path. Until each lands, such translation units take the fallback.

   `alwaysinline` needs no inliner, with one exception. A call to an
   `alwaysinline` function whose definition has ordinary linkage is
   correct out of line. But kiln, like clang, emits some `alwaysinline`
   bodies with `available_externally` linkage (extern-template and
   `gnu_inline` cases), for which no out-of-line definition may exist
   anywhere: such programs link only because every call is inlined. For
   those, hod emits the body as a local copy in this object and directs
   the module's calls to it. An `available_externally` body that is not
   `alwaysinline` is discarded and referenced as an external symbol, as
   LLVM does at `-O0`.

   This matters for the text-first integration: nearly every translation
   unit that includes the standard library contains an `alwaysinline`
   function, so kiln runs `opt` and hands `llc` bitcode in the common
   case. Until `hod_bc` exists, the shim converts bitcode to text with
   `llvm-dis`, which also means hod sees those modules already inlined.
   The in-process path receives the module before any LLVM pass.

   This interim step is done (WP-020): hod runs the installation's
   `llvm-dis` itself, over pipes, when the shim names one (`spec/cli.md`
   "Bitcode conversion", `spec/harness.md` "shim"). Its cost, which is
   what `hod_bc` has to remove, is measured in
   `spec/landed/cli/WP-020.md`.

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

## Review by kiln's maintainers (2026-11-11)

Facts established for the stages above, at kiln `bd60e182`:

- A backend seam in kiln's driver is acceptable. Today the built module
  never reaches the driver (only its text does), so the seam starts with
  returning the module. kiln will depend on hod through an optional cargo
  feature on its driver (off by default), with the bridge crate in kiln's
  workspace and hod pinned by revision (decided 2026-11-11).
- **Frame up front:** available. Entry-block allocas go through one helper
  and are recorded in a list; debug declarations are recorded separately
  and can be joined. Variable-length arrays and `__builtin_alloca` are
  dynamic allocations.
- **Resolved addresses:** available cheaply. Field offsets are already
  computed where field accesses are emitted.
- **Use facts:** not available without a counting pass on kiln's side. hod's
  builder maintains them as operands are appended (`spec/ir.md` §5.6), so
  no producer needs to supply them.
- **Call shapes:** the ABI classification is computed per signature and per
  call but not stored in kiln's IR model. Only a producer that lowers
  directly into `hod_ir` can supply it.
- **Function at a time, from kiln's lowering:** a function is final when
  lowered except when (a) a finished body is later discarded and redefined,
  (b) a declaration is later replaced by an alias, IFUNC or another
  function, changing what earlier references mean, (c) a body refers to a
  constructor or destructor variant not yet resolved and is parked until
  the end, (d) a variadic virtual body is cloned for thunks. A streaming
  producer must either hold back the functions these touch or hod must
  accept late symbol resolution. `hod_ir` symbols are referred to by id,
  never by name, so (b) can be a change to the symbol's definition rather
  than a rewrite of its users; the rule will be fixed with the stage-3
  design.
- `musttail` is required for correctness in variadic thunks. `naked`
  functions have no prologue.

## Measurement that can overturn point 2

At M5, on the benchmark corpus: time spent copying `kiln_llvm` → `hod_ir`
as a fraction of hod's total. If it exceeds 5%, evaluate having
`kiln_codegen` build `hod_ir` directly, or a zero-copy view.

## Prior art

- TPDE (Schwarz et al., CGO 2026, arXiv 2505.22610): an adapter over
  an existing SSA IR, one analysis walk and one walk that selects, allocates
  and encodes together. Reports 8x to 26x lower back-end compile time than
  LLVM `-O0` on SPECint 2017 at similar run time, with 10% to 30% larger
  code. hod's design follows it, with the analysis walk replaced by facts
  the IR builder maintains; hod differs in owning its IR and in
  treating debug info, unwind tables and full C++ coverage as requirements.
- Cranelift, LLVM FastISel + fast register allocator, QBE, tcc, and
  copy-and-patch were considered. All but tcc build a machine IR that hod
  avoids; tcc has no SSA input.
