# Validation

hod has one job that can go wrong in two ways: wrong code, or slow code
generation. Both are tested. This document says how, which checks run when,
and what a release candidate must pass.

## Oracles

hod never has to decide by itself what the right answer is.

- **`llc -O0`** (LLVM 22, and 19) is the oracle for behavior: the same IR
  through `llc` and through hod, linked the same way, must produce programs
  with identical stdout and exit status. An oracle can be wrong: on
  AArch64 the two selectors plain `-O0` uses (GlobalISel, FastISel) each
  miscompile a small correct function, while SelectionDAG at `-O0` and
  `-O2` agree with the reference interpreter (`spec/interp.md` §10.1). The
  reference is therefore SelectionDAG at `-O0`
  (`-global-isel=0 -fast-isel=0`), and where two LLVM configurations
  disagree the test is an oracle error, never a hod failure: `xtask
  irdiff` builds with that reference, its `--witness` (on in the full gate
  and in corpus admission) checks it against `-O2`, and a disagreement is
  an ERROR (`spec/harness.md` "irdiff", "The oracle", "The witness").
- **`hod_interp`** is the oracle for the code generator's own tests: a
  generated program has one result, which every correct compilation gives.
- **LLVM's assembler and disassembler** (`llvm-mc`, `llvm-objdump`) are the
  oracle for instruction encodings.
- **`llvm-readelf`, `llvm-dwarfdump --verify`, and the linkers themselves**
  are the oracle for object-file structure.
- **gdb and lldb** are the oracle for debug information: a scripted session
  must print the same answers on hod's object as on LLVM's.
- **clang** is the oracle for the calling convention: hod-compiled and
  clang-compiled functions must be able to call each other.

LLVM is a test dependency only. Tests that need a tool skip with a note when
it is missing, and fail when `HOD_REQUIRE_TOOLS=1` (CI and release runs).

## Layers

Each layer has a directory under `tests/` and an `xtask` subcommand. Lower
layers are fast and run on every change; higher layers run before merges or
on a schedule.

| | layer | what it checks | oracle | when |
|---|---|---|---|---|
| L0 | unit tests | each crate's own invariants | - | every change |
| L1 | **encoding** | every instruction form `hod_mc_*` can produce, across operand sweeps: hod's bytes disassemble to the intended instruction, and equal `llvm-mc`'s bytes for the same assembly | `llvm-mc`, `llvm-objdump` | every change |
| L2 | **IR ingestion** | `hod_ll` parses everything in the IR corpus; what `hod_ir` prints is accepted by `llvm-as` and the verifier, and compiled by `llc` it behaves as the original does (`hod_ir` is a normal form, not a copy of the text: `spec/ir.md` §9) | `llvm-as`, `llc` | every change |
| L3 | **object structure** | hod's objects pass `llvm-readelf`/`llvm-dwarfdump --verify`, and link with every linker in the matrix | binutils, LLVM tools | every change |
| L4 | **IR differential** (`xtask irdiff`) | each program in the IR corpus built by `llc` and by hod behaves identically | `llc` | every change (small set), merge gate (all) |
| L5 | **ABI cross-linking** | generated caller/callee pairs over a grid of argument types, one side hod and one side clang, both directions | clang | merge gate |
| L6 | **kiln's corpus through hod** | kiln's executing tests (about 1500 `run` tests plus the GCC torture suite) with hod as the code generator | clang (kiln's harness) | merge gate |
| L7 | **debug info** (`xtask debug`) | scripted gdb and lldb sessions on hod's and `llc`'s programs under every linker; kiln's debugger tests through hod; `--verify` of the corpus's `-g` modules; `(function, line)` comparison with `llc` | gdb, lldb, `llc`, `llvm-dwarfdump` | every change (quick tier, in `cargo test`), merge gate (all) |
| L8 | **libc++ test suite** with kiln + hod | the long-term acceptance gate | the suite | nightly / release |
| L9 | **open-source projects** built with kiln + hod, running their own tests | - | release |
| L10 | **fuzzing** | random C programs (csmith, YARPGen) through clang `-O0 -emit-llvm`, then L4; random IR functions against an interpreter | `llc`, an IR interpreter | continuous, budgeted |

### The IR corpus

`tests/ir/` holds `.ll` files with `;@` directives (expected stdout, exit
status, required target, required LLVM version). Sources:

1. hand-written files, one per IR construct, named after the construct and
   carrying a `covers:` directive that names the `hod_ir` spec section they
   exercise (`spec/harness.md` "Test directives");
2. IR that kiln emits for its own test corpus and that clang emits for C
   test suites, captured by `xtask corpus` (not committed in bulk; generated
   from pinned inputs into `.build/`);
3. reduced reproducers for every miscompile found, committed with the fix.

Because the corpus is IR, it does not depend on kiln's state, and tests the
backend from the first milestone, before any C++ feature is complete.

### Linker matrix

Every linking test runs with each of: GNU ld, lld, mold, and wild once it
is provisioned (until then it is optional and its absence a note). The same
object must link with all of them and the programs must behave identically.
Linker versions are pinned in the provisioning script and recorded in
status reports.

### Debug information (L7)

`cargo xtask debug` (`spec/harness.md` "debug", `spec/dwarf.md` §8.4)
builds each test of `tests/debug/` from clang 19, clang 22 and kiln++
IR by hod and by `llc -O0` (SelectionDAG), links both programs with every
linker, and runs the same gdb and lldb script on each: the transcripts,
masked of addresses, offsets, ids and the program's directory only, must
be equal. Two-object tests also link each module from hod with the
others from `llc`, in both orders (COMDAT groups with `-g`). A difference
hod is not to fix now is a seed with an `xfail` and its reason; a stale
`xfail` fails. `debug kiln` runs kiln's own debugger tests with hod as
the backend; `debug verify` runs `llvm-dwarfdump --verify` on every `-g`
corpus module hod builds; the `(function, line)` sets of the two
programs must be equal up to `spec/dwarf.md` §5.7's classes. The quick
tier (about 20 s) runs in `cargo test`; the full gate runs everything.

### Hybrid, strict, bisect

`docs/ARCHITECTURE.md` "Fallback" describes the mechanism. Its use here:

- Suites L6, L8 and L9 are run in **hybrid** mode from the start. Two numbers
  are recorded for every run: the pass rate, and **coverage**: the fraction
  of translation units (and of functions) hod compiled itself. Pass rate
  below the LLVM baseline is a bug; coverage below 100% is the remaining
  work, listed by construct from the coverage log.
- Gates are **strict**: no fallback.
- A failing test in hybrid or strict mode is handed to `xtask bisect`, which
  finds the smallest set of functions that must go to `llc` for the test to
  pass, and writes the reduced IR into `tests/ir/` as a reproducer.

## Performance

Speed and memory are properties with tests, not goals with hopes.

- `bench/` holds IR inputs in size classes (small, typical, large, with and
  without debug info), generated from pinned sources.
- `xtask bench` measures, per input: wall time (fastest of N serial runs),
  peak resident memory, and output size, for hod and for `llc -O0` on the
  same input, on the same machine, in the same run.
- Results are recorded as **ratios to `llc`** as well as absolute numbers,
  because ratios survive a change of machine and absolute numbers do not.
- Baselines live in `bench/baselines/`. A change that makes hod more than 5%
  slower or 2% larger in memory on any input fails unless the baseline is
  updated with a written reason. On a machine other than the baseline's the
  check is advisory.
- The harness also reports time per phase (ingest, analysis, emission,
  debug info, object writing) and bytes of IR per second, so regressions
  are attributable.
- Cold timings (a new process per compile, as a build system runs hod) are
  taken with the binaries and inputs on a local filesystem, each binary
  copied afresh and a byte-identical copy of the base run as a control:
  on the workspace's bind mount, file-backed page faults of the binary's
  first touches dominate, and the same bytes read up to 86% apart
  (`spec/landed/ll/WP-063.md`).
- End-to-end numbers (`kiln++ -c` with hod against `kiln++ -c` with `llc`)
  are reported in `docs/status/perf.md` for the benchmark corpus.

## Gates

**Every change (local, minutes):** `cargo xtask ci --quick`: format, clippy
with warnings denied, L0 to L3, and the small L4 set. L2 is the round trip's
quick tier: one corpus entry in 32, a fixed selection by path (about 200
entries, some 20 s; `spec/harness.md` "roundtrip").

**Merge to main (CI):** `cargo xtask ci`: everything above plus the whole
corpus for L2, full L4, L5,
and, from the milestones that enable them, L6 and L7 in strict mode for the
constructs claimed as supported.

Steps that need tools or build output (the oracle, the linkers, the IR
corpus) pass with a notice without them, so a fresh checkout passes the
quick gate; `HOD_REQUIRE_TOOLS=1` makes each missing one a failure
(`spec/harness.md` "ci").

**Release candidate:** all of the above with `HOD_REQUIRE_TOOLS=1`, on both
targets, with every linker; L8 strict at the pass rate kiln achieves with
`llc`; L9; the benchmark gate; a fuzzing budget with no open findings; and
the support matrix in `docs/status/` regenerated.

## Hosted CI

GitHub Actions runs the gate on real hardware of both architectures
(`spec/harness.md` "CI" has the rules; `.github/workflows/`):

| | `ubuntu-24.04-arm` (AArch64) | `ubuntu-24.04` (x86-64) |
|---|---|---|
| container | Debian 13 (`buildpack-deps:trixie`) | the same |
| tools | LLVM 19 and 22, binutils, mold, gdb, lldb-19, the x86-64 cross toolchain, `qemu-user` | the same, with the AArch64 cross toolchain |
| AArch64 programs under test | native | `qemu-aarch64 -cpu cortex-a53` |
| x86-64 programs under test | `qemu-x86_64 -cpu qemu64,-sse3,-cx16,-lahf-lm` | native (the runner's CPU, more than the baseline) |
| jobs | `kiln`, `lints`, `test`, `irdiff`, fan-in `check` | the same, fan-in `check-x86_64` |
| not run | `roundtrip` (no corpus) | the same |

Every job sets `HOD_REQUIRE_TOOLS=1`. On the x86-64 runner an x86-64
program is not held to the baseline CPU: the emulated AArch64 lane is
where a use of an instruction beyond the baseline faults.

The libc++ suite with kiln and hod runs natively on x86-64 in the `sweep`
workflow (a `sweep/**` branch or by hand; never a required check).

**Tests that need a particular host.** A test of AArch64 programs runs
on both hosts when it takes its toolchain from `hod_testutil::target`
(natively, or under `qemu-aarch64`); so do `hod_obj`'s `link`,
`link_tls` and `link_conventions`, `hod_cg`'s `emit_link` and `emit_eh`,
and `hod_ir`'s `print_run`. These still run on an AArch64 host only, and
why:

- the debugger tests of AArch64 programs (`hod_dwarf`'s `debugger`,
  `die_debugger`, `type_debugger`; `hod_a64`'s `*_debugger`, `dies_e2e`,
  `dies_label`, `dies_decline`, `compressed`, `coro_debug`, and
  `unwind`'s gdb backtrace): gdb and lldb trace a native process; under
  `qemu-aarch64` they would need its gdb stub and a remote session, which
  the debugger scripts do not have;
- the tests that compile C++ or C with the host's clang or g++ and link
  it with hod's AArch64 objects (`hod_a64`'s `coro`, `coro_body`,
  `coro_transfer`, `dies_scope`, `unwind`; `hod_eh`'s `unwind` and
  `lsda_unwind`): they need the cross target and system root passed to
  each compiler, which they do not do yet;
- `hod_interp`'s `x87_oracle` and `x64_varargs_oracle` run x86-64
  programs, natively on x86-64 and under `qemu-x86_64` elsewhere, so they
  already run on both.

## Rules for tests

- Every behavior ships with a test, and every miscompile gets a reduced
  reproducer in `tests/ir/` before the fix is merged.
- A test names the spec section it covers.
- A known gap is marked expected-fail with a reason and a backlog row. It is
  never left failing and never deleted.
- A test whose oracle crashes or times out is an ERROR, not a pass and not a
  hod failure.
- Every child process runs with a time limit and a memory limit. The host is
  shared.
