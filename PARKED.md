# WP-006 (the `hod_ir` printer) — PARKED

Parked on the supervisor's instruction (token budget). Branch `wp/006-printer`,
worktree `/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-006`. Nothing pushed.
This file is a work note; delete it when the package is finished.

Read this top to bottom, then the WP-006 brief. You need no other memory.

## State in one paragraph

The printer itself is written and works: every test that exists passes on
LLVM 19 and LLVM 22. What is missing is (a) the module-level test file, which
is half written, (b) a speed test, (c) the specification text `spec/ir.md` §9
and the landing record `spec/landed/ir/WP-006.md`, (d) `cargo fmt` / clippy
clean-up of the new test files, (e) the gate run. The gate has NEVER been run
on this branch.

## FLAG: touched outside my files

- `crates/hod_ir/src/random.rs`: `fn generate` became
  `pub(crate) fn generate(seed, storage, complete) -> (Module, IrResult<Body>)`.
  Visibility only, no behavior change. It is needed by
  `crates/hod_ir/src/print/random_tests.rs`. Must be reported at the top of
  the handback.
- `crates/hod_ir/src/lib.rs`: only the doc-table row, `mod print;` and
  `pub use print::{Dialect, IoSink, Part, PrintError};` (allowed by the brief).

## Done (do not redo)

- `crates/hod_ir/src/print/mod.rs` — public API:
  `Dialect` (`LLVM19..LLVM22`, `ALL`, `new(major)`, `major()`),
  `Part { Whole, Main, Rest }`,
  `PrintError { Unsupported{construct, symbol}, Malformed{what, symbol}, Write }`,
  `IoSink<W: io::Write>` (`new`, `finish`),
  `Module::print(out, dialect, bodies)`,
  `Module::print_part(out, dialect, part, bodies)`,
  `Module::print_body(out, dialect, body)`.
- `crates/hod_ir/src/print/module.rs` — module-level text (symbols, attribute
  groups, metadata, `param_attrs(attrs, after_type)`).
- `crates/hod_ir/src/print/body.rs` — instructions. Debug-information hooks
  `dbg_function` / `dbg_slot_vars` / `dbg_inst` are there and empty on purpose
  (debug info is not in this package).
- `crates/hod_ir/src/print/random_tests.rs` — unit test
  `random_bodies_are_accepted_by_llvm`. Last result: LLVM 22: 345 bodies
  verified, 802 parsed only, 353 not expressible; LLVM 19: 324 / 686 / 490;
  1500 seeds, 4.75 s.
- `crates/hod_ir/tests/print_support/mod.rs` — shared test helpers (`printed`,
  `define`, `declare`, `int`, `Fx`, `Llvm`, `parse_symbols`, `link_and_run`).
- `crates/hod_ir/tests/print_data.rs` — 4 tests, pass. Initializers byte for
  byte against `flatten` (652 variables, 68160 bytes of `.data`, 189
  relocations per config; aarch64/LLVM 19: 578 / 55478 / 149).
- `crates/hod_ir/tests/print_run.rs` — 3 tests, pass. A 29-function program
  compiled by `llc` 19 and 22 at `-O0`/`-O2`, linked by `gcc`, run; the split
  module; landing pads catching a C++ exception.
- `crates/hod_ir/tests/print_bodies.rs` — 3 tests, pass. Every opcode,
  predicate, `RmwOp`, `TailKind` printed and accepted (LLVM 19: 17 bodies, 289
  instructions; LLVM 22: 25 bodies, 305 instructions); funclet opcodes are
  errors; dialect differences.

## Half done

`crates/hod_ir/tests/print_module.rs`

- `fn everything(major, for_object) -> (Module, Vec<Body>)` is written and
  compiles, but has NEVER been run. Expect some `unwrap` to panic on the first
  run (a builder call rejecting an input); fix the test input, not the library.
- `fn explore` is a temporary test: with the environment variable
  `HOD_WP006_DUMP=<dir>` set it writes `module22.ll`, `module19.ll`,
  `module22-text.ll`, `module19-text.ll` to that directory. Without the
  variable it does nothing. It also holds dummy uses of imports so the file
  compiles without warnings. Replace it with the real tests below.
- `fn defined_no_body` is a pointless wrapper around `FunctionDesc::new`;
  inline it.
- One local is named `_gap` and then used; rename it to `gap` (clippy).

## Remaining, in order

1. Run the dump:
   `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-006 && source scripts/env.sh && HOD_WP006_DUMP=/Volumes/AgentWork/hod-helper/.build/wp006-scratch cargo test -p hod_ir --test print_module`
   Read the four `.ll` files. Check each by hand with
   `llvm-as-22 -o /dev/null f.ll`, `opt-22 -passes=verify -S f.ll`, and the
   same with `-19`. Fix whatever the printer gets wrong in
   `crates/hod_ir/src/print/module.rs`.
2. Replace `explore` with real tests, each with a doc comment naming
   `spec/ir.md` §9.x:
   - for majors 22 and 19: `Llvm::new(major)`, `llvm.accept(text, ..)`;
   - `llvm.reprint(text, ..)` (this is `opt -S`) and assert it contains the
     expected line for each symbol, attribute group and metadata node;
   - `llvm.object(..)` on the `for_object = true` module, then
     `llvm.readelf(object, &["-S", "-s", "--wide"])` + `parse_symbols` and
     assert kind / binding / visibility / section / size per symbol; check the
     256-byte string `v_bytes` in `.rodata` with `llvm.section`;
   - module-level error tests, one assertion per construct name: `Dialect::new(18)`
     (`dialect:llvm-18`), `FlagValue::Other` (`module-flag:<name>`),
     `frame_pointer = FramePointer::Reserved` before 22
     (`module-flag:frame-pointer=4`), a `sanitize_alloc_token` function on
     LLVM 19 (`attr:sanitize_alloc_token`), `record_unsupported`, a symbol
     never defined or declared (`unresolved-symbol`), `ptr addrspace(270)` on
     LLVM 19 (`type:ptr addrspace(270)`), `split:comdat`,
     `blockaddress-of-declaration`, `blockaddress-of-entry-block`, a
     `Malformed` body (a body passed twice, or for a symbol that is not a
     function);
   - determinism: print twice, equal; print through `IoSink` into a `Vec<u8>`,
     equal to the `String`.
3. Remove the two temporary lines that mention `HOD_WP006_DUMP` in
   `tests/print_run.rs` and `tests/print_bodies.rs` (grep for the name).
4. Add `crates/hod_ir/tests/print_perf.rs` after the pattern of
   `crates/hod_ir/tests/symbol_perf.rs`: build a large module (thousands of
   small functions), print it, report MB/s with `eprintln!`. No hard
   threshold that could flake on a shared machine.
5. Write `spec/ir.md` §9. The code and tests already cite these numbers, so
   keep them: §9.1 API; §9.2 the dialect table; §9.3 module-level text,
   constants and initializers; §9.4 bodies and lowering rules; §9.5 the
   instructions that are errors (funclets); §9.6 parts / the split and the
   error constructs; §9.7 determinism, "not a round trip", the debug-info
   hook; §9.8 the LLVM rules `Body::verify` lacks; §9.9 the tests. Read the
   printer source for the exact rules; do not write the spec from memory.
6. Write `spec/landed/ir/WP-006.md` (look at a sibling file in
   `spec/landed/ir/` for the shape).
7. `cargo fmt`, then `cargo clippy -p hod_ir --all-targets` and fix every
   warning. Clippy has not been run since the test files were written; the
   older test files in the same directory show how casts are allowed
   (`#[expect(clippy::..., reason = "...")]`).
8. `cargo test -p hod_ir`.
9. The gate, in the background, output to a log:
   `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick > /Volumes/AgentWork/hod-helper/.build/wp006-gate.log 2>&1`
10. Commit (trailer `Co-Authored-By: ...` as the brief says), delete this
    file, hand back.

## Decisions made, and why

Rule used throughout: where LLVM cannot say what hod means, the printer
returns `PrintError::Unsupported { construct, .. }` naming the construct. It
never prints text that means something else.

- Names: values `%v<id>`, arguments `%a<n>`, blocks `b<id>`, stack slots
  `%s<N> = alloca [size x i8], align A` in the entry block. `PtrAdd` prints as
  `getelementptr i8` with helper values `%v<id>.g` / `.c` / `.m`.
- Struct types print as literal types, not named ones (no names are kept).
- `Part::Main` prints the given bodies as definitions and everything else as
  declarations; `Part::Rest` is the complement. A function whose blocks are
  addressed by a variable must be in `Part::Main`
  (`blockaddress-of-declaration`). COMDAT members cannot be split
  (`split:comdat`).
- Errors by construct: `unresolved-symbol`, `type:target("name")`,
  `type:ptr addrspace(N)`, `module-flag:<name>`, `module-flag:frame-pointer=4`,
  `address-of-symbol-in-address-space`, `split:comdat`,
  `shufflevector:scalable-mask`, `blockaddress-of-declaration`,
  `blockaddress-of-entry-block`, `invoke:<tail>` / `callbr:<tail>`,
  `const-expr:<op>`, `attr:<name>`, `intrinsic:<name>`,
  `intrinsic-overload:<name>`, `intrinsic-signature:<name>`,
  `atomicrmw:<op>`, `inst:<opcode>` (the five funclet opcodes),
  `dialect:llvm-<major>`.

## What each dialect changes (for §9.2; all checked against 19.1.7 and 22.1.8)

- `ptrtoaddr`: LLVM 22 only; printed as `ptrtoint` before.
- `llvm.thread.pointer`: no suffix before 22, `.p0` on 22.
- Function attributes: `sanitize_alloc_token` 22; `sanitize_realtime`,
  `sanitize_realtime_blocking`, `sanitize_type` from 20.
- `atomicrmw usub_cond` / `usub_sat` from 20; `fmaximum` / `fminimum` from 21.
- Intrinsics: `clmul`, `coro.is_in_ramp` 22; `minimumnum`, `maximumnum`,
  `atan2` from 20.
- Constant expressions: `mul` only up to 20.
- `frame-pointer` module flag value 4 (reserved): 22 only.
- An `i1` global: default alignment 1 on LLVM 19, 4 on LLVM 22 (aarch64), so
  the alignment is printed explicitly.
- LLVM 19's aarch64 data layout has no address spaces 270 to 272.
- Majors 20 and 21 are specified by these differences and not tested (the
  tools are not installed).

## Disagreements between `hod_ir` and LLVM (the handback must list these)

`Body::verify` accepts, LLVM rejects (counts are of 1500 generator seeds from
`FIRST_SEED = 100` in `random_tests.rs`, LLVM 22):

- a use not dominated by its definition (263), a self-use in a reachable
  block (73), uses of an `invoke`/`callbr` result off the normal edge;
- `callbr` of a callee that is not inline asm (114). LLVM 22's verifier
  segfaults on `callbr void @llvm.hod.first() to label %a [label %b]`;
  reproducer `/Volumes/AgentWork/hod-helper/.build/wp006-scratch/r/one.ll`;
- `invoke` of an intrinsic (47);
- tail markers on `invoke` / `callbr`; `musttail` rules (92): matching
  signature and calling convention, `ret` directly after;
- an `immarg` parameter given a non-constant (79), e.g. the last argument of
  `llvm.memcpy`; LLVM 22 also rejects `undef` there;
- a `"kcfi"` bundle without exactly one operand (58);
- `signext` on a value that is not an integer (76); parameter attributes are
  not checked against types or positions (`sret` must be on the first or
  second parameter of a function returning void).

Model gaps: the address of a symbol is typed `ptr` whatever the symbol's
address space; branch-protection flag value 1 versus 2 is not kept; module
flag behaviors and integer widths of known flags are not kept; `TargetExt`
type parameters are not kept; hod's layout rules give an `i1` global LLVM 22's
alignment, not LLVM 19's. `flatten` agreed with LLVM everywhere it was tested.

## Ruled out

- Depending on `crates/hod_interp` from the tests (the brief forbids it).
- Editing `verify.rs`, `builder.rs`, `body.rs`, `module.rs`, `symbol.rs`,
  `inst.rs`, `constant.rs` — not allowed; none was needed.
- COMDAT kinds other than `any` and `nodeduplicate` in the object test: ELF
  `llc` refuses them. They are text-only (`for_object = false`).
- `fptosi` of an out-of-range value in a run test: poison, `-O0` and `-O2`
  differ. That was a test bug, not a printer bug.

## Last known test results

- `cargo check -p hod_ir --tests`: clean, no warnings (run at park time).
- Passing when last run: `print::random_tests` (unit), `print_data` (4),
  `print_run` (3), `print_bodies` (3).
- Never run: `print_module`, `cargo fmt --check`, clippy on the new test
  files, the full `cargo test -p hod_ir`, the gate.

## Follow-ups to list in the handback (find exact lines with grep)

- debug information: `dbg_function`, `dbg_slot_vars`, `dbg_inst` in
  `crates/hod_ir/src/print/body.rs`;
- intrinsic signatures that differ between majors (for example `coro.end`);
- dialects 20 and 21 untested;
- literal struct types make large text; named types would need names kept;
- the random generator numbers the function symbol by seed, so each module
  declares `@g0..@g<seed-1>` (quadratic over a run of seeds).
