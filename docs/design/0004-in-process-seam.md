# Design 0004: the in-process seam (stage 2)

Status: accepted in shape by kiln's maintainers (2026-11-12, section 10);
kiln's K1 measures the print first and K2 onward wait for that number
against the stop rule of 7.6. Proposed 2026-11-12 (WP-089). Design 0001 fixed the stages: 1, hod as
`llc`'s stand-in (done); **2, kiln hands its built module to hod in the
same process (this note)**; 3, kiln lowers into `hod_ir` directly.

Paths are hod's unless marked `kiln:` (relative to kiln's repository at
`bd60e182`). Times come from a shared host (load 17-27): proportions, not
baselines. Every estimate says which measurement replaces it.

## Question

What crosses from kiln's driver to hod in one process, what API hod owes
that caller, what each side builds, and how we know it is correct and
worth it.

## 1. Facts

### 1.1 kiln's driver

- One source input is one module. `CodegenJob::run` returns its **text**
  (`kiln:crates/kiln_driver/src/codegen_cmd.rs:240`): `lower` calls
  `kiln_codegen::emit_with_debug` (`:466`), drops the AST and the
  evaluator, then prints (`:494-498`, so that the peak holds two of AST,
  module and text, not three). The `kiln_llvm::Module` never reaches the
  driver.
- The plan (`:742-811`): for `-c` and for each source of a link, a
  frontend step piped into `llc` (`:776-781`, `:793-803`; a link's objects
  go to a temporary directory, `:703`), then `clang++` links (`:1118`).
  `-S -emit-llvm` writes the text, `-c -emit-llvm` pipes it to `llvm-as`,
  `-flto` goes to `opt` and never to `llc` (`:762-766`).
- `ir_passes` (`:860-882`) runs `opt` before `llc`, which then reads
  bitcode: at `-O1` and above always; at `-O0` for sanitizers, coverage,
  `-finstrument-functions`, an `alwaysinline` definition (`:839`) or a
  coroutine (`:831`).
- `llc`'s command line (`:1032-1091`, `-O` level from `:135`) is a
  subset hod's shim already parses; target and features are in the
  module. `-gz` is an `llvm-objcopy` step after `llc` (`:784-791`).
- Steps run in sequence on one thread (`execute`, `:1252-1392`); children
  are spawned with pipes (`kiln:crates/kiln_driver/src/link.rs:94`). A
  build system runs one kiln per source: one module per process is the
  common case. kiln reads no `MAKEFLAGS`. The compile thread has a 1 GiB
  stack (`kiln:crates/kiln_driver/src/main.rs:29-54`).
- Panics: every kiln profile keeps `panic = "unwind"` (`kiln:Cargo.toml:58`).
  The hook records the **first panic of the process** and prints nothing
  (`kiln:crates/kiln_driver/src/crash.rs:118-140`); `main` catches the
  unwind and writes the report and reproducer (`main.rs:10-19`). A
  thread-local `QUIET` silences the hook on the current thread only
  (`crash.rs:87`, used at `:207-211`).
- kiln is MIT OR Apache-2.0 (`kiln:Cargo.toml:9`), as hod; both pin Rust
  1.95.0, edition 2024, `unsafe_code = "forbid"`.

### 1.2 `kiln_llvm`'s model

Index arenas (`kiln:crates/kiln_llvm/src/lib.rs:1-14`). A `Module`
(`module.rs:326`) holds interned types, interned constants (`ConstData`,
`constant.rs:27`: integers, floats, aggregates, splats, bytes,
GEP/cast/binary expressions, `blockaddress`, `dso_local_equivalent`,
`no_cfi`), functions, globals, aliases and IFUNCs, COMDATs, inline asm,
interned attribute sets (`attr.rs:200`), metadata (`MdNode`,
`metadata.rs:37`: strings, values, tuples, and `Specialized { name,
fields }`, one carrier for every `DI*` node), named metadata, module asm.
A `Value` is a 4-byte tag and index (`value.rs:90`). A `Function`
(`function.rs:51`) holds 24-byte `Inst`s (`inst.rs:769`) in creation
order, blocks listing their ids, a separate layout, operand pools, call
data (`inst.rs:831`), instruction metadata and `DbgRecord`s
(`#dbg_declare`, `#dbg_value`, `function.rs:23`). Erased declarations are
redirected by `Module::resolve` (`module.rs:728`); the print order is
`ordered_funcs`/`ordered_globals` (`module.rs:744`, `:855`).

**From outside the crate, headers are readable and bodies are not.** The
instruction payload (`InstKind`, `inst.rs:587`), call and landing-pad data
(`:831`, `:867`), operand pools, instruction and function metadata, the
debug-record list (`function.rs:83-98`), the constant and metadata tables,
named metadata (`module.rs:339-367`) and the print order are `pub(crate)`.
Public reading stops at `Inst::opcode`, `Function::layout`,
`block_insts`, `debug_records`, `Module::const_data`, `attrs_of`,
`md_node`. Only the in-crate printer (`print.rs`, 2,384 lines) walks
everything. A bridge outside `kiln_llvm` needs a read API (K2).

### 1.3 hod's facade and builder

- `hod::compile` (`crates/hod/src/lib.rs:371`) reads text or bitcode; the
  front half (`crates/hod/src/front.rs:396`) streams bodies through a
  `Judge` (`:197`): the module pass, then per body `classify`, `verify`
  and a **clone** of each accepted body (`:325`). `generate_with`
  (`back.rs:313`) is the documented seam (`spec/cli.md` "The facade"): it
  compiles the accepted bodies on scoped threads (`back.rs:210`) into a
  `Vec<u8>` (`back.rs:360`). `Options` is `llc`'s command line; `Threads`
  takes tokens from a `ThreadTokens` (`back.rs:39`). The jobserver client
  (`crates/hod_cli/src/jobs.rs`), the coverage line and the `llc`
  argument parser are CLI code.
- `hod_ir`'s builder (`spec/ir.md` §5-§6, `crates/hod_ir/src/builder.rs`):
  symbols first, in LLVM's module order (§2.2); one `FunctionBuilder` per
  body; placeholders for forward uses; ids are layout positions; static
  allocas are slots (§5.2); GEPs are `PtrAdd` with byte offsets; debug
  nodes are typed, filled through the field table, and the producer
  records what it cannot map (§6.9).
- GEP lowering exists twice (`crates/hod_ll/src/inst.rs:759`,
  `crates/hod_bc/src/inst.rs:687`), as do attributes, special globals,
  module flags and debug fields.
- The release profile is fat LTO, one unit, `panic = "abort"`
  (`Cargo.toml:63`, WP-068). Cargo applies the **root** workspace's
  profile only: inside kiln, hod gets kiln's profile and unwinds.
- hod's process-wide state: two thread-local scratch buffers, taken and
  cleared on each use (`crates/hod_a64/src/control.rs:136`,
  `common.rs:676`). Jobserver tokens are returned by a `Drop` guard,
  panic included (`back.rs:77-86`).

### 1.4 Measurements

| | value | source |
|---|---|---|
| hod's own work to decline a 4-line module | 57 µs | `spec/landed/cli/WP-068.md` |
| a hod process (exec, page faults) | 1-5 ms | same |
| `hod_bc`, module and bodies / `hod_ll`, bodies | 219 / 263-450 ns per instruction | `spec/landed/bc/WP-046.md`, `ll/WP-071.md` |
| builder, fresh / recycled storage | 15.1 / 10.5 ns per instruction | `spec/landed/ir/WP-004.md` |
| `classify` / `verify` / code generation | 9 / 26 / ~90 ns per instruction | WP-009, WP-004, `cli/WP-043.md` |
| libc++ modules built by hod / declined | 5438 / 1788 (75.3% / 24.7%) | `docs/status/libcxx-hod.md` |
| hod's time on libc++ declines | 62 s of a 588 s back end, half `llvm-dis` | same |

New for this note (release builds of `main` and of kiln, `HOD_THREADS=1`,
3-5 runs, ranges). kiln's `regex-match-libstdcxx.cpp` (3.2 MB text, ~40k
instructions): `-fsyntax-only` 0.91-1.27 s, `-S -emit-llvm -o /dev/null`
1.19-1.60 s, with `-g` 1.44-1.93 s: **IR build and print ~0.28 s (`-O0`)
and ~0.53 s (`-g`) on a 0.9 s front end**; the split between building and
printing is not measured (K1 adds a trace scope). Its back end: `llc -O0`
0.36-0.39 s and 83 MB, hod 0.04-0.07 s and 14 MB, of which reading the
text is about half. `cleanup-scopes-30k` (330k instructions), hod on
text: `bodies` 203-214 ms, `emission` 51-59, `object` 3-9, total
316-340 ms; on its bitcode, `bodies` 121-148 and total 193-221 ms. The
`-g` regex module is declined up front (`debug:module-flags`; WP-087).

## 2. Decision

1. **The bridge copies `kiln_llvm::Module` into `hod_ir` through the
   builder, after lowering and before any `opt`.** It is the readers'
   mapping a third time, *specified by equivalence*: `bridge(M)` is the
   `hod_ir` that `hod_ll` reads from `M.to_text()`, equal under `hod_ir`'s
   printer (`spec/ir.md` §9.7) with the same recorded refusals, up to the
   exceptions of section 3. One test then covers the whole mapping.
2. **The bridge is `kiln_hod`, in kiln's workspace**, behind the optional
   feature `hod` of `kiln_driver` (off by default), with hod pinned by
   revision: the review's terms. It reads `kiln_llvm` through a **public
   read-only view** kiln adds (K2). hod never depends on kiln.
3. **hod's in-process API is a session that judges as it is fed**
   (`hod::Session`, section 4). The producer builds the module level into
   the session's `hod_ir::Module`, ends it (module pass), then hands over
   bodies one at a time **by value**; each is classified and verified on
   arrival, and the first refusal stops the producer. `finish` appends the
   object to the caller's buffer. The command-line front half is rebuilt
   on the same session: one judge for both routes.
4. **Fallback: print only on decline.** kiln keeps its module until hod
   answers; on any decline it prints the text and takes today's route
   unchanged (`opt` if `ir_passes` says so, then `llc`). Nothing is
   classified on `kiln_llvm`; the only pre-check is the command line.
5. **Options come from the `llc` command line kiln already plans**, read
   by hod's own `llc` parser, moved into the facade; threads, strict mode,
   census and the coverage log come from the shim's environment through
   facade functions. Both routes agree on settings by construction.
6. **Panics are contained**: `catch_unwind` around the session, kiln's
   crash hook silenced for every thread the session starts, fallback with
   the new reason `internal-panic`. kiln's profiles stay `unwind`.
7. **The route is chosen at run time** (Q3), and only for an object (`-c`,
   or before a link) at `-O0` without `-flto`, sanitizer, coverage or
   instrumentation passes. Everything else takes today's route.
8. **Validation is byte equality** with `hod` run on kiln's pre-`opt` text
   and the same `llc` arguments, over kiln's tests and libc++; the copy's
   share of 0001 is measured by the bridge's own timer (section 7).

Rejected:

- *Text in memory* (`hod::compile(&text)`): saves the exec only; parsing
  is ~10x the copy. Kept as the route's first step (K4), not the end.
- *A bitcode writer in kiln*: none exists; `hod_bc` is ~7x the copy.
- *The bridge in hod's workspace, or a trait view*: hod must not depend
  on kiln; 0001 point 2 rejected a generic codegen.
- *The bridge inside `kiln_llvm`*: ties kiln's IR crate to a hod
  revision; a read view is smaller (Q1 lets kiln decide otherwise).
- *Classify on `kiln_llvm` first*: a second vocabulary that would drift
  from the census; streaming already bounds the waste (section 5).
- *Speculative printing on another thread*: CPU spent on every module.
- *Straight to stage 3*: kiln still needs its LLVM module for every
  decline, so two lowerings; stage 2 is testable now.

## 3. What crosses the seam

As `hod_ll` does it unless said otherwise. The bridge memoizes each kiln
id it maps (types, constants, attribute sets, symbols, metadata) in a
`Vec` indexed by that id: module-level work is linear, per-instruction
work is array lookups.

**1:1**: types; constants (`hod_ir`'s folding for GEP, cast and binary
expressions); variables, functions, aliases and IFUNCs with linkage,
visibility, `dso_local`, `unnamed_addr`, TLS, section, alignment, address
space, COMDAT; module asm; inline asm; every instruction kind kiln emits
(arithmetic, compares, casts, memory with orderings and scopes,
`cmpxchg`, `atomicrmw`, `fence`, `select`, `phi`, `freeze`, `va_arg`,
vector element and shuffle operations, `call`/`invoke`/`callbr` with
bundles, `landingpad`, `resume`, the terminators); `!dbg` attachments;
module flags, `llvm.dbg.cu`, linker options, dependent libraries;
`llvm.global_ctors`, `dtors`, `used`, `compiler.used`.

**Decided here:**

- *Symbol order*: the printer's (variables in `ordered_globals`,
  functions in `ordered_funcs`, then aliases, IFUNCs), because object
  layout follows it (§2.2); operands through `Module::resolve`.
- *Layout*: blocks in `Function::layout` order; a use before its
  definition in that order gets a placeholder, resolved at the definition.
- *Static allocas* (entry block, constant count) become slots sized by
  `hod_ir`'s layout; a `#dbg_declare` of one becomes a slot variable.
- *GEP* becomes `PtrAdd` (constant offsets folded, variable indices
  scaled); aggregate indices become byte offsets.
- *Attributes*: kiln's `AttrKind` is mapped to `hod_ir`'s table once per
  process, each interned set once per module.
- *Dropped* (§7): names, instruction metadata other than `!dbg`,
  optimization-only flags, `!llvm.ident`, global sanitizer flags other
  than `memtag`.
- *Exceptions to equivalence*: names and `source_filename` are not
  transmitted (no object byte depends on them); a `blockaddress` of an
  unnamed block is representable in process (text: refused as
  `blockaddress:unlabelled-block`).

**What `hod_ir` cannot represent** (the bridge records the reader's name
with `Module::record_unsupported`, and classify declines by it):
`alloca:inalloca`; `alloca:scalable` and `getelementptr:scalable` (SVE);
`getelementptr:vector-element-not-bytes`; `aggregate:no-fixed-offset`;
the funclet instructions (`inst:catchswitch`, `catchpad`, `cleanuppad`,
`catchret`, `cleanupret`; kiln emits Itanium only); `gc`; `dll-storage`;
`global-sanitize-memtag`; `personality-not-a-symbol`;
`metadata-arg:indirect`; `bundle:metadata-input`;
`symbol:anonymous-external`; `debug:DIEnumerator.wide`; each
`debug:<kind>[.<field>]` the field table marks unsupported. Target
extension types are represented and refused by `type:target-ext`; kiln
has no `x86_mmx`, `x86_amx` or `ppc_fp128`.

**Debug information** (§6.9, "the producer's duty"). A `Specialized`
node maps by `di_kind(name)` and `di_field(kind, field)`, cached per
`&'static str` key; `MdField::Raw` values (`DW_TAG_*`, `DIFlag... | ...`,
`DW_ATE_*`, `DW_LANG_*`) through `hod_ir`'s name tables; `MdField::Node`
through the memo, forward references by reserved nodes (§6.3);
`DILocation` to a module `LocId`; `DIExpression` to an `ExprId`; debug
records to `dbg_declare`/`dbg_value`; a function's `!dbg` to its
subprogram, a global's to `attach_global_expr`; then `finish_debug`. An
unsupported field with a non-default value is recorded and an unknown
kind becomes `DiOther`: never silently degraded. The pre-`opt` module
has no always-inlined code, so these modules do not need WP-085's
inlined scopes.

**Size and cost (estimates).** Each reader is 9-10k lines, mostly lexing
or bitstream decoding; kiln's whole-model printer is 2.4k. The bridge:
**~2,000-2,500 lines** plus tests, ~1,500 if the shared mapping first
moves into `hod_ir` (H3). Per instruction: kiln's record and pools ~5 ns,
id lookups ~5, the builder 10.5-15, GEP, slot and attribute work ~5-10:
**25-40 ns**, against 219 (bitcode) or 263-450 (text) for the reader it
replaces, before counting the print and the exec. hod's work after
reading is ~150-250 ns per instruction, so **the copy will be ~10-20% of
hod's in-process total, above 0001's 5%**: the builder alone is 5-7%.
Measured by the `bridge` phase (H1): the bridge times its copy, the
session times the rest, and the share is Σcopy / Σ(copy + session) over
kiln's tests and a libc++ sample, on one thread, with the per-module
distribution. Above 5%, 0001 says to evaluate stage 3 (section 6
prepares it); stage 2 still removes the print, the exec and the parse.

## 4. The API hod owes the bridge

```rust
pub struct Session { /* the hod_ir::Module, the judge, options, report */ }
impl Session {
    pub fn new(target: TargetSpec, options: &Options, settings: Settings)
        -> Result<Session, CompileError>;              // assembly-output, target, options
    pub fn module(&mut self) -> &mut hod_ir::Module;   // also between bodies
    pub fn end_module(&mut self) -> Result<(), CompileError>; // finish_debug, module pass
    pub fn body(&mut self, sym: SymId, body: Body) -> Result<(), CompileError>;
    pub fn finish(self, out: &mut Vec<u8>) -> Result<Report, (CompileError, Report)>;
}
impl Options { pub fn from_llc_args(args: &[impl AsRef<str>]) -> Result<Options, ArgError>; }
impl Settings { pub fn from_env() -> Result<Settings, EnvError>; } // threads, census, strict
pub mod coverage { pub fn line(/* the eleven fields */) -> String; }
```

- **Input** is a module built into the session and bodies fed one by
  one, not a finished `&Module`: a decline stops the copy, and stage 3
  feeds the same API. `body` takes ownership, so the clone of
  `front.rs:325` goes away for every producer.
- **Output** is appended to the caller's `Vec<u8>` (the object is laid
  out in memory anyway); kiln writes it where `llc -o` would have.
- **Decline** is `CompileError::Unsupported` with a stable `Construct`
  name, cheap (a `Cow` and the function's name). `Internal`,
  `MalformedInput` and `InvalidOptions` fall back as in hybrid mode
  (`spec/cli.md` "Fallback protocol").
- **Threads**: kiln compiles on one thread and holds make's implicit
  token. `Settings::from_env` is the shim's rule (`HOD_THREADS`, else the
  jobserver fifo, else one), moved from `hod_cli` with its tests: extra
  tokens only for large modules, never waiting, returned on every path.
- **Memory** (estimate): module plus `hod_ir` (~37 bytes per
  instruction, WP-004) in place of module, text (~80 bytes per
  instruction for regex) and a child (14 MB hod, 83 MB `llc`); measured
  by `/usr/bin/time %M` (section 7).
- **Determinism**: the command-line route's bytes, at any thread count.
- **Versioning**: the *producer surface* (the items above and `hod_ir`'s
  module-building and builder API, `spec/ir.md` §2-§6) changes only with
  a bump of `hod::EMBED_API` and a line in `spec/cli.md` "Embedding". The
  bridge asserts the value at compile time, so a break shows at the pin
  bump, never silently. Revisions that pass the bridge's tests are tagged
  `seam-N` (H6). A builder break costs kiln a bridge edit; additions cost
  nothing until kiln emits them.
- **Build**: a default feature `readers` (`hod_ll`, `hod_bc`) on the
  `hod` crate; kiln takes `default-features = false` and compiles ~77k
  lines of hod, not ~97k. kiln may give hod's crates `codegen-units = 1`
  overrides if measured faster (WP-068: 4-6% on big modules).

**Panics and safety.** Inside kiln, hod unwinds (1.3). `Session` promises
unwind safety: a panic leaves no state behind (the thread-local buffers
are taken and cleared per use, tokens are returned by `Drop`), so the
next session in the process gives the same bytes; H4 tests exactly that
with a forced panic. The bridge wraps `end_module`, the body loop and
`finish` in one `catch_unwind` with `AssertUnwindSafe`, drops the
session, and falls back with reason `internal-panic` (a harness counts
it as a hod bug, as `internal-error`). kiln must keep its hook from
recording a panic on hod's **worker threads**, where its thread-local
`QUIET` is not set, or a later real crash would be reported as hod's: a
"contained" scope that also covers threads started inside it (Q5). A
second panic while unwinding still aborts, as anywhere in kiln. No
`unsafe` on either side.

## 5. Fallback in process

On decline kiln calls `to_text()` and continues exactly as today, so a
fallback object is byte-identical to today's.

| a decline | today (shim) | in process |
|---|---|---|
| print | always | only now |
| process | 1-5 ms exec | none |
| reading up to the refusal | 263-450 ns per instruction (text; bitcode 219, plus `llvm-dis` after `opt`) | 25-40 ns per instruction |
| module-level refusal | after reading the whole module level | before any body |

A body-level refusal costs the copy of the bodies before it: at most
330k × 40 ns ≈ 13 ms on the largest corpus module, whose text takes ~200
ms to read today. The command-line pre-check (point 7) uses what kiln knows already:
phase, `-O` level, LTO, sanitizers, coverage, `-finstrument-functions`;
and optionally `-g` while WP-087 is open (refused at the module pass
today, at negligible cost).

`alwaysinline` and coroutines: the in-process route sees the module
**before** `opt`. A call to an `alwaysinline` function with an ordinary
definition is compiled out of line, which is correct (0001 point 6); an
`available_externally` body is refused (`linkage:available_externally`
until WP-021) and goes the `opt` way. Coroutines are refused
(`attr:presplitcoroutine`, the `llvm.coro.*` rows) and go the `opt` way.
Q4 asks kiln to confirm no other case is wrong out of line.

## 6. What stage 2 must not preclude

- **Function at a time.** `Session::body` takes one body and promises
  nothing about when it is compiled: stage 3 may compile on arrival, on
  workers, while kiln lowers (with the growing tables of `spec/ir.md`
  §10). No API hands out "all bodies".
- **Late resolution, the review's (a)-(d).** Symbols are ids, so a body
  may name a symbol defined later (c), and a declaration may become an
  alias or IFUNC (b) through a `Module` method stage 3 adds. A redefined
  body (a) needs a `Session::replace_body` before `finish`; nothing in
  stage 2 treats a delivered body as final before `finish`. Thunk clones
  (d) are kiln's. Once bodies compile early, code generation must not
  read a symbol's final properties (`dso_local`, linkage) before
  `finish`: a stage-3 rule, recorded so that stage 2 does not bake in the
  opposite.
- **Producer-side facts.** Slots and folded addresses go through the
  builder; stage 3 states them with the same calls. Call shapes need a
  builder extension (§10), not a session change.
- **Order.** Byte equality needs LLVM's symbol order; stage 3 needs
  another oracle (sorted symbol tables, or the execution check).

## 7. Validation and measurement

1. **IR equality** (the bridge's tests, in kiln's CI with the feature):
   for every kiln test that lowers (`tests/conformance`, `tests/stress`;
   the corpus's 1,540), `print(bridge(M)) == print(hod_ll(M.to_text()))`
   and the same refusals. Fast, and it names the field that differs.
2. **Object equality**: the in-process object against `hod` with the same
   `llc` arguments on `kiln -S -emit-llvm -Xclang -disable-llvm-passes`,
   byte for byte, over the same tests and both text dialects (19, 22).
   With `KILN_HOD_DUAL=1` the bridge also runs `hod::compile` on
   `M.to_text()` in process and fails on any difference: this covers
   libc++ without a second harness.
3. **libc++ in process**, hybrid with `KILN_HOD_DUAL=1`, the run of
   `docs/status/libcxx-hod.md` against its baseline: 0 test differences,
   no `internal-*` reason, no dual mismatch. Then strict: every test whose
   objects hod built passes.
4. **Copy share**: section 3's metric over kiln's tests and every 10th
   libc++ module, `HOD_THREADS=1`.
5. **End to end**: kiln `-c` wall time and peak RSS (kiln, process tree)
   for `llc`, shim and in process, kiln's `cargo xtask bench --runs 7`
   interleaved over `bench/frontend` objects and `bench/libcxx-slice`
   (K5), at load below 2 or marked advisory.
6. **When to stop.** An object inequality is a bug that blocks enabling
   the route, not a reason to stop. **Stop stage 2** (keep the shim, design
   stage 3) if, with equality holding, the in-process route is neither
   ≥3% faster than the shim on the median of the object entries nor ≥10%
   lower in process-tree peak memory, or if kiln's own peak RSS grows by
   more than 15%. If the copy share exceeds 25%, do not tune the bridge:
   start stage 3. Expected from 1.4: the regex unit saves its print, ~20-35
   ms of parsing and the exec on a ~1.2-1.5 s compile, a few percent; more
   with `-g` and on IR-heavy units.

## 8. Packages

hod's side (WP numbers when filed):

| id | scope | depends | size |
|---|---|---|---|
| H1 | `hod::Session` (`new`, `module`, `end_module`, `body` by value, `finish`), the front half rebuilt on it, a `bridge` phase in `Report`. Test: every corpus module fed from `hod_ll` through `Session` gives `compile`'s bytes. | — | S |
| H2 | Into the facade: `Options::from_llc_args`, `Settings::from_env` with the jobserver client, the coverage line (field 3 gains `module`); the CLI uses them, its tests move along. | — | S |
| H3 | `hod_ir` producer kit: GEP to `PtrAdd`, static alloca to slot, attributes, special globals, module flags, debug fields; both readers use it. Corpus equality before and after. | — | M |
| H4 | Unwind safety: a forced panic through `Session` returns its tokens and the next session's bytes are equal; reason `internal-panic`; `spec/cli.md` "Embedding". | H1 | S |
| H5 | Feature `readers`, `EMBED_API`, the promise of section 4 in the spec. | H1, H2 | S |
| H6 | `cargo xtask kiln-bridge`: kiln's bridge built against a hod checkout (`[patch]`) and tested; tag `seam-N` when green. | K3a | S |
| H7 | The measurements of section 7 (3-5), reported in `docs/status/`. | K4, K5 | M |

kiln's side (by kiln's maintainers, or hod's agents through pull
requests; Q2):

| id | scope | depends | size |
|---|---|---|---|
| K1 | `CodegenJob::run` returns the `kiln_llvm::Module`, the caller prints; a trace scope around the print. Test: `-S -emit-llvm` output unchanged over the suite. | — | S |
| K2 | `kiln_llvm` read view: instruction payloads, call and landing-pad data, pools, instruction and function metadata, print order, constant and metadata tables, named metadata. Test: a walker over the view reaches what `print.rs` prints. | — | M |
| K4 | The route behind feature `hod`: selection (Q3), options from the planned `llc` command (H2), contained `catch_unwind`, print-on-decline fallback, coverage line, `HOD_STRICT`, `-v`/`-###`; first over `hod::compile(&text)`, without the bridge. | K1, H1, H2, H4 | M |
| K3a | `kiln_hod` without debug information; IR equality on tests without `-g`; the route switched from text to the bridge. | K2, K4, H1, (H3) | M |
| K3b | `kiln_hod` debug information; IR equality on `-g` tests. | K3a, WP-087 | M |
| K5 | Bench: object entries and the libc++ slice at `-c` for three back ends; `KILN_HOD_DUAL`. | K4 | S |
| K6 | One CI job with the feature on (fetching the pinned hod) running the bridge's tests. | K3a | S |

Order: H1 and K1 (a day each) in parallel; H2, H3, K2 in parallel; K4,
the whole route testable before the bridge exists; K3a; H4, H5, H6, K5,
K6; K3b once WP-087 has landed; H7.

## 9. Open questions for kiln's maintainers

1. May `kiln_llvm` gain a public read-only view of its model (K2), or do
   you want the bridge as a feature-gated module inside `kiln_llvm`?
2. Who writes `kiln_hod`: kiln's agents, or hod's agents through pull
   requests you review?
3. How is the in-process route chosen at run time: a kiln flag (such as
   `-fkiln-backend=hod|llvm`), an environment variable, or both?
4. Is any `alwaysinline` function kiln emits wrong when called out of
   line, other than `available_externally` bodies?
5. Will you add a "contained" scope to the crash hook that records no
   panic of a thread started inside it?
6. May the module live until hod answers, its text printed only on
   decline?
7. Will every kiln profile stay `panic = "unwind"` with the feature on?
8. Do you accept pinning hod to `seam-N` tags, with a compile-time
   `hod::EMBED_API` check in the bridge?
9. Can the module change after `emit_with_debug` returns, before it is
   printed?
10. Does every module handed over pass `kiln_llvm::Module::verify`, or
    must the bridge not rely on it?
11. For `-gz`, keep `llvm-objcopy` after hod, or let hod compress
    (`Options::debug_compression`, different bytes)?
12. May one kiln CI job build with the feature, fetching hod by git
    revision?

## 10. Review by kiln's maintainers (2026-11-12)

Checked against kiln main `6c46e943e` (the logic at `bd60e182` is the
same; line numbers of `codegen_cmd.rs` moved). Where this section and
the text above differ, this section holds.

**One structural change (decision 2).** kiln's root `Cargo.lock` has only
crates.io sources and packagers build kiln offline from vendored crates;
an optional git dependency would enter that lock, feature off or not.
So `kiln_hod` is **its own small workspace inside kiln's repository**
(excluded from the root workspace, its own `Cargo.lock` carrying hod's
revision) holding the bridge and a thin `kiln++`-with-hod binary;
`kiln_driver` gains a dependency-free backend hook that the wrapper
registers. The stock build, its lock and its packaging are untouched.
H6's `[patch]` flow is unchanged. If hod is published to crates.io with
an exact pin, folding it back into a feature becomes possible.

**One missing dependency (decision 4).** `ir_passes` decides by scanning
the printed text. Handing the module over before printing needs the same
predicate computed from the `Module` (kiln's K4: tested equal to the text
scan over the suite). First version, conservative: a module for which
the predicate says `opt` is not handed over unless an explicit opt-in is
set; the dual-mode libc++ run decides when the default flips. What runs
`opt` at `-O0`: any `-flto`; the sanitizers fuzzer-no-link, memory,
thread, address, hwaddress (not UBSan alone); gcov or
`-fprofile-instr-generate`; any `alwaysinline` attribute group, on a
definition **or a call site** (`[[clang::always_inline]]` statements,
calls inside `flatten` functions); the `instrument-function-entry`
family; a declared `llvm.coro.*`. `-disable-llvm-passes` suppresses all
but LTO.

**Answers.**

1. A read-only view in `kiln_llvm` (K2), a kiln-internal surface with no
   stability promise (the bridge is built in kiln's CI). Smaller than
   1.2 says: print order, `Inst::ty`, names, successors, attribute
   lists, `gc`, `personality`, initializers, alias targets, constants
   and inline asm are public already. Missing beyond 1.2's list:
   metadata attachments on globals, a metadata-kind id-to-name lookup,
   sync-scope names, alias/IFUNC value types, COMDAT kind by id, counts
   of metadata and attribute sets.
2. kiln's agents write kiln's side (K1-K6); hod reviews each pull
   request and owns H1-H7.
3. Both: `-fkiln-backend=llvm|hod` (default `llvm`; `hod` on a kiln
   without the bridge is an error) and `KILN_BACKEND`; the flag wins.
4. Wrong out of line: only `available_externally` bodies (inline +
   `always_inline` and defined elsewhere: `extern template`,
   `gnu_inline`, a C inline definition), which rely on `opt`. Not
   determined: an unused discardable `always_inline` definition that
   names an undefined symbol; `alloca` or return-address builtins in
   such a body; target-feature mismatch between caller and callee. The
   conservative default above covers them.
5. Yes, in `kiln_driver`: it owns the `catch_unwind` and a process-wide
   contained scope the hook consults (the first-panic record is
   process-wide and is filled even by a caught panic); the bridge
   returns a `Result`.
6. Yes; the hand-over sits after the error gate (`lower` prints even
   when errors were emitted and `run` discards the text).
7. Yes: no profile sets `panic`, unwinding is kiln's policy.
8. Exact revision plus the compile-time `hod::EMBED_API` assert; `seam-N`
   tags are names, the lock records the commit.
9. No: nothing changes the module after `emit_with_debug`; the text has
   two more readers, `ir_passes` and the PCH image check.
10. Do not rely on `Module::verify` (structural only, tests only): hod
    verifies every bridged body.
11. Keep `llvm-objcopy` first (byte equality is the validation); hod's
    own compression is a later, measured step.
12. Yes, one CI job building the `kiln_hod` workspace.

**Corrections to section 1.** `llc` also gets `-mcpu`,
`-relocation-model`, function and data sections, `-fp-contract`,
`-code-model`, x86 branch alignment, `-debugger-tune` and the
split-DWARF flags: `Options::from_llc_args` (H2) must accept or decline
each. `-S -emit-llvm` at `-O0` writes `opt`'s output when `ir_passes`
fires. "One thread" is the dedicated compile thread; each child process
has two helper threads. `kiln:crates/kiln_llvm/src/attr.rs:200` is
`enum Attr`; the interned table is `AttrTable` (`:362`).

**Schedule.** K1 first, with the split of IR build against print for the
regex unit and two larger ones at `-O0` and `-g`; K2 and K4 only if that
number passes 7.6, behind kiln's release work.
