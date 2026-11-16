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
| L4 | **IR differential** (`xtask irdiff`) | each program in the IR corpus built by `llc` and by hod behaves identically | `llc` | every change (small set), full gate (all) |
| L5 | **ABI cross-linking** | generated caller/callee pairs over a grid of argument types, one side hod and one side clang, both directions | clang | every change (the ABI grids, in `cargo test`) |
| L6 | **kiln's corpus through hod** | kiln's executing tests (about 1500 `run` tests plus the GCC torture suite) with hod as the code generator | clang (kiln's harness) | full gate (`corpus-<arch>`: strict, both targets, with `docs/status/corpus-expected.tsv`) |
| L7 | **debug info** (`xtask debug`) | scripted gdb and lldb sessions on hod's and `llc`'s programs under every linker; kiln's debugger tests through hod; `--verify` of the corpus's `-g` modules; `(function, line)` comparison with `llc` | gdb, lldb, `llc`, `llvm-dwarfdump` | every change (quick tier, in `cargo test`), full gate (all: `debug-<arch>`, both targets) |
| L8 | **libc++ test suite** with kiln + hod | the long-term acceptance gate | the suite | release; run by hand (`cargo xtask libcxx`, `docs/status/libcxx-hod.md`) or by the hosted sweep (`.github/workflows/sweep.yml`, started by a `sweep/<name>` branch or by hand); no nightly lane exists |
| L9 | **open-source projects** built with kiln + hod, running their own tests ("Real projects (L9)") | kiln with `llc` | release |
| L10 | **fuzzing** (`xtask fuzz`) | random C programs (csmith in option sets, and hod's own ABI generator; YARPGen once installed) through clang `-O0 -emit-llvm` for each target, built by hod `--strict` and by the reference `llc`, linked and run, exit status and output compared; the module split so each compiler builds half (`--mix`); with `-g`, DWARF verified and lines compared (`--debug`); random IR functions against an interpreter (not yet) | `llc` (`-O2` as the witness of a finding) | the reduced seeds of findings in every change (as L4 seeds); long runs in the sweep, budgeted |

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

### Moving kiln's pin

kiln is pinned in three places that move together: the `kiln` submodule
of the development workspace (its `target-kiln` build is what tests and
the harness run as kiln++), the kiln groups of both corpora (each
entry's key holds kiln's revision), and `.github/kiln-revision` (the
hosted lane's kiln++). The GCC groups do not depend on kiln and are
kept. A move is prepared on a branch beside the pinned state, then
switched by the supervisor when no gate is in flight
(`spec/landed/harness/WP-165.md` rehearsed it). `<B>` is the build
root (`.build`), `<P>` the branch's build directory (`<B>/target-wp-NNN`)
and `<R>` kiln's new commit.

Preparing (on the branch; nothing shared is written):

1. Build hod (`cargo build --release -p hod_cli`), then clone the
   submodule into `<P>/kiln-src` (`git clone -q <B>/../kiln
   <P>/kiln-src`, `git -C <P>/kiln-src checkout -q <R>`) and build it
   there: `CARGO_TARGET_DIR=<P>/kiln-target cargo build --release -j 6`
   in `<P>/kiln-src`.
2. Copy both corpora (`cp -a <B>/corpus <P>/new/corpus`, the same for
   `corpus-x64`) and capture kiln's groups again into the copies:
   `cargo xtask corpus capture [--target x86_64] --source kiln -j 4
   --kiln <P>/kiln-target/release/kiln++ --kiln-repo <P>/kiln-src
   --out <P>/new/corpus[-x64]` (about 25 minutes each at `-j 4`). Keep
   the copies out of `<P>/corpus`: `corpus verify` writes its results
   to `<build dir>/corpus`.
3. Regenerate, against the new corpora, what the gate or the status
   pages read from them, and commit it on the branch:
   - `cargo xtask roundtrip --corpus <P>/new/corpus --update-expected`
     (about 55 minutes at `-j 3`) writes `tests/roundtrip/expected-aarch64.tsv` (its `# corpus:`
     line names the new manifest; the gate's `roundtrip` step compares
     with it);
   - `cargo xtask corpus census <P>/new/corpus` writes
     `docs/status/corpus.md`; with `--target x86_64` and
     `<P>/new/corpus-x64`, `docs/status/corpus-x64-census.md`;
   - `cargo xtask corpus reasons [--target x86_64] --llc -j 4 --report
     docs/status/reasons[-x64].md <P>/new/corpus[-x64]` (about 45
     minutes each at `-j 3`);
   - `cargo xtask corpus compare --target x86_64 --against
     <P>/new/corpus <P>/new/corpus-x64`: a difference it reports is
     classified in `docs/status/corpus-x64-differences.tsv`;
   - `.github/kiln-revision` holds `<R>` in full.
4. Rehearse what changes with kiln++ itself, with
   `HOD_KILN=<P>/kiln-target/release/kiln++`: `cargo test` of the crates
   whose tests run kiln++ (`hod`, `hod_cli`, `hod_a64`, `hod_dwarf`,
   `hod_ir`, `xtask`), `cargo xtask debug --frontend kiln --kiln
   <P>/kiln-target/release/kiln++`, and `cargo xtask debug verify
   <P>/new/corpus` and `cargo xtask debug verify --target x86_64
   <P>/new/corpus-x64` (without `--target x86_64` every module of the
   x86-64 corpus is skipped).

Switching (the supervisor, no gate in flight, in this order):

1. `git -C kiln checkout <R>` in the workspace; commit the submodule's
   new revision in the workspace repository.
2. Rebuild kiln++ where tests find it: in `kiln/`,
   `CARGO_TARGET_DIR=<B>/target-kiln cargo build --release -j 6`.
3. Swap the corpora: `mv <B>/corpus <B>/corpus.old`, `mv
   <P>/new/corpus <B>/corpus`, the same for `corpus-x64`; remove the
   old `<B>/corpus.work` and `<B>/corpus-x64.work` (`<corpus>.work` is
   only a capture's scratch). `cargo xtask corpus verify --manifest-only`
   and `cargo xtask corpus verify --target x86_64 --manifest-only` must
   pass.
4. Merge the branch (its expectation file, status pages and
   `.github/kiln-revision` are only valid after steps 1 to 3); the
   first hosted lane builds kiln at the new revision once per
   architecture and caches it (`.github/actions/kiln`).
5. Run the gate; when it is green, delete `<B>/corpus.old`,
   `<B>/corpus-x64.old` and `<P>`.

Status pages that record a measurement (`docs/status/debug.md`,
`libcxx-*.md`, `corpus-x64.md`, `perf.md`) name the kiln they were
measured with and stay as they are until they are measured again. The
benchmark inputs (`bench/manifest`) are produced by clang, not kiln, and
do not move with the pin.

### Random programs (L10)

`cargo xtask fuzz --seed N --count M --target T [--llvm 19] [-j J]
[--profile P] [--mix | --debug | --census]` (contract: `spec/harness.md`
"Random programs"; WP-154, WP-160) makes seed `S` a random C program,
compiles it with clang at `-O0` to LLVM text for the target
(position-independent, as a driver builds by default), builds that
module with the reference `llc` and with `hod --strict`, links both with
the target's driver and runs them (x86-64 under `qemu-x86_64` with the
baseline CPU, as the L4 cross runs do), with the same time limit on both
targets (5 s; a program of hod's that times out gets four times as long
before it is called a hang). Every child goes through the harness's
limits and jobserver.

The generator is a profile: csmith with an option set (`default`,
`aggregates`, `float`, `wide`, `volatile`, `narrow`), or `abi`, hod's
own generator of calls across the calling conventions (aggregates of
every shape the conventions classify, by value and returned, `long
double`, `__int128`, many arguments, variadic calls read with
`va_arg`), which csmith does not reach. `--census` counts what a
profile's modules contain (opcodes, types, `byval`/`sret`, variadic
definitions and calls) without building them.

The modes:
- **whole** (the default): one program of hod's object, one of
  `llc`'s;
- **mix** (`--mix`): `llvm-extract` splits the module in two by a
  seeded choice per function and global (internal symbols become hidden
  externals, so the halves link); hod builds one half and `llc` the
  other, both ways round, against the all-`llc` program. This is what
  sees a calling convention that agrees with itself but not with
  another compiler's;
- **debug** (`--debug`): with `-g`; `llvm-dwarfdump --verify` on hod's
  object, then the `(function, line)` comparison of `cargo xtask debug`
  against `llc`'s; an unclassified pair is a finding.

Per seed the verdict is:

- **agree**: same exit status, same output (the programs print a
  checksum or every global);
- **refused**: hod declined, counted by its coverage-log reason (that is
  coverage, not a bug);
- **skipped**: the generator or clang failed, a program timed out (both,
  or the oracle's), there was nothing to split, or a `--float` program
  converts a float to an integer type that cannot hold it (`ub-float-cast`:
  gcc's UBSan pre-run fired; undefined behaviour, on which `llc`'s own
  instruction selectors differ; `spec/harness.md` "Float casts");
- **error**: the oracle's side failed (`llc`, its link, `llc`'s two
  halves disagreeing with its whole module);
- **finding**: the programs disagree, hod failed, hod's object does not
  link, hod's program hangs, hod's DWARF does not verify, or its lines
  differ without a permitted class;
- **set apart**: a finding a class sets apart on evidence gathered for
  that finding in that run, counted and listed with its seed, never by a
  label (`spec/harness.md` "Set apart (seeds)"). One class:
  `llc19-int128-last-register`, a `disagree` of the `abi` profile at
  x86-64 LLVM 19 whose module passes an `__int128` with only r9 left
  (`llc` 19 leaves r9 unused where the psABI gives it to the next
  integer argument), set apart only when gcc on `prog.c` and `llc` 22 on
  the module both print exactly hod's output. In `--debug`,
  `selector-split` sets apart a line pair where LLVM's other two `-O0`
  selectors keep hod's row.

A finding is saved under `<build>/fuzz/findings/<run>-seed<S>/` with the
source, the module, every command as a shell line, both outputs and the
verdict of the witness (`llc -O2` of the same module, and in `--mix` hod
on the whole module), then reduced by cutting the module's text (in
`--mix` re-splitting each candidate, so the split is kept) and written
as a seed test. The run prints a table and the rate and exits 1 on a
finding. The gate does not run csmith: the checked-in reduced seeds are
its share of L10, and its tests of the command (`xtask/tests/fuzz_real.rs`)
skip without the tools; a planted calling-convention bug that only
`--mix` can see shows the mode works. The numbers of the long runs are
in `docs/status/fuzz.md`.

**The budget** (WP-241) is what a release candidate runs: `cargo xtask
fuzz --budget --seed <first> --hod <frozen hod> -j 4` (contract:
`spec/harness.md` "Random programs", "Budget"). Per target (AArch64
natively, x86-64 under `qemu-x86_64`), with clang and `llc` 22:

| profile | mode | seeds |
|---|---|---:|
| `default` | whole | 2000 |
| `aggregates`, `float`, `wide`, `volatile`, `narrow` | whole | 1000 each |
| `abi` | whole | 2000 |
| `default` | `--mix` | 500 |
| `abi` | `--mix` | 500 |
| `default` | `--debug` | 500 |

and with LLVM 19 a quarter of each line (500, 250 each, 500, 125, 125,
125): 10,500 seeds per target with LLVM 22, 2625 with 19, 26,250 in
all, in 62 slices of at most 500 seeds. Why these numbers: the budget
has to run on the shared reference machine (load about 20 to 30 on 24
cores, the shared jobserver's tokens mostly taken) in one working day at
`-j 4`, one slice at a time. The cost is set by csmith's seeds whose
programs run past the 5 s limit on both sides (about one in eight; each
costs 10 s); the first slice of WP-241's run (500 `default` seeds on
AArch64) ran at 114 programs per minute, the `abi` profile is several
times faster, `--mix` and `--debug` slower, which puts the whole budget
at about 5 to 6 hours. A smaller budget (half the csmith lines) was
planned from a 20-seed smoke that measured 10 to 200 programs per
minute, and given up when the first slice showed the rate. The time
limit stays 5 s (a shorter one would turn more seeds into skips). If a
later machine is slower, cut seeds evenly, never a profile or a mode,
and say so here.

**Fresh seeds.** Every slice has its own range, disjoint from every
other slice's (targets and LLVM majors included); the plan lays them out
one after another from `--seed`. A candidate's first seed is the next
multiple of 100,000, plus 1, above every seed an earlier budget or
recorded run used; `docs/status/fuzz.md` names the ranges each measured
candidate used (the runs of WP-154 and WP-160 used 1 to 2500; WP-241's
budget 100001 to 126250, so the next candidate starts at 200001).

**Met** when the run on the candidate's frozen hod has no finding left
open: each is fixed (with a regression test in the crate that owns the
defect), or shown not to be hod's (`llc`, clang, csmith's undefined
behaviour, the harness, a timeout of the loaded machine that does not
happen when the seed runs alone) with its evidence on the page. A
refusal is coverage, not a finding; errors (the oracle's side) are
listed and explained. Seeds set apart (`set-apart`, with their
evidence) are not findings; skips such as `ub-float-cast` are not
either. The run is made with a harness that has every class known at
the candidate (since WP-249: `ub-float-cast`, `selector-split`,
`llc19-int128-last-register`).

### Real projects (L9)

Open-source projects built through their own build systems with kiln as
the compiler and hod as its back end, running their own tests
(`spec/harness.md` "Real projects", `spec/landed/harness/WP-177.md`). The
rows are kiln's (`scripts/oss/projects.toml`, kiln's format); each is
built at `-O0` and at `-O0 -g`, three ways: kiln with `llc` (the
control), kiln with hod falling back to `llc` where hod declines (the
**census**: every object kiln made, whether hod built it, and each
refusal by name and function), and kiln with hod **strict** when the
census is empty (the build must complete with every offered module from
hod). A test that passes under `llc` and not under hod is the lane's
most important finding; a refusal is coverage work for another package.

```
source scripts/env.sh
cargo build --release -p hod_cli
scripts/oss/oss-run.sh fmt googletest abseil      # the whole procedure, one build at a time
scripts/oss/oss-build.sh re2 --mode strict --flavour O0g   # one build
python3 scripts/oss/oss-report.py status --out docs/status/oss.md
```

Results: `docs/status/oss.md` and, for x86-64, `docs/status/oss-x64.md`
(generated; `status --root` reads several lanes' roots, `--provenance
scripts/oss/provenance-<target>.tsv` adds the hod and kiln commits a
report does not carry and the lanes' notes, `--candidate` marks the rows
measured before the candidate and `--carried-from` the rows carried
over from the previous candidate under `docs/RELEASE.md` section 5's
rule 5: `docs/RELEASE.md` section 7 has each
page's one command); LLVM and Clang, built by their
own scripts (`scripts/oss/llvm/`), are in `docs/status/oss-llvm.md`. The
refused modules are kept
under `findings/oss/<project>/<reason>/` in the build root, with their
command lines. The hosted form is `.github/workflows/oss.yml` (x86-64 and
AArch64 runners; by hand or on an `oss/**` branch).

With `HOD_OSS_KEEP_MODULES=<absolute dir>` (off by default; WP-272) the
lane also keeps every module the hod and strict builds hand hod, with
hod's arguments, under `<dir>/<target>/<project>/<flavour>/`
(`<hash>.ll` and `MODULES.tsv`; an identical module is kept once), and a
module that cannot be kept fails its compile. Two hod binaries are then
compared on the projects' own translation units, as on the corpus
(`spec/harness.md` "compare-binaries"; `docs/RELEASE.md` section 5,
rule 5):

```
HOD_OSS_KEEP_MODULES=$PWD/../.build/kept scripts/oss/oss-run.sh fmt --modes "hod strict"
cargo xtask compare-binaries --base <old hod> --head <new hod> --modules ../.build/kept --out <abs dir>
```

The same lane runs for x86-64 on the AArch64 host (`--target x86_64`,
`spec/landed/harness/WP-191.md`): kiln is given
`--target=x86_64-unknown-linux-gnu` and finds the cross GCC and its
system root itself, CMake is told it cross-compiles and runs every test
program under `qemu-x86_64 -L /usr/x86_64-linux-gnu`
(`CMAKE_CROSSCOMPILING_EMULATOR`, which ctest honours; `projects.py
emulate-tests` also puts the emulator before the test commands CMake
leaves alone: a program named by path, a `cmake -P` test's
`-D<var>=<program>`, and CTestCustom.cmake's pre- and post-test
commands). A program that a build step or a test starts by itself
(protoc and its plugins, a death test re-running `/proc/self/exe`, a
python harness) gets no emulator. The host kernel's x86-64 translation
runs it instead. For that, every x86-64 link of the lane, in the
control and in hod's builds alike, names the system root's loader as
interpreter and puts its libraries on the run path
(`-Wl,--dynamic-linker=/usr/x86_64-linux-gnu/lib/ld-linux-x86-64.so.2
-Wl,-rpath,/usr/x86_64-linux-gnu/lib`, through
`CMAKE_EXE_LINKER_FLAGS`/`CMAKE_SHARED_LINKER_FLAGS`). These are link
flags only: no compile sees them, and the objects are the same with and
without them. `HOD_OSS_X64_INTERP=0` turns this off. So there are two
emulators: ctest's own test programs run under qemu, and
self-started programs run under the host's translation. The two differ
(AVX-512, some system calls), so when a self-started program fails in
the strict build but not in the control, it is rerun under qemu before
the difference is called hod's. The trees, findings and census are the
cross lane's own:

```
scripts/oss/oss-run.sh fmt googletest abseil re2 --target x86_64
scripts/oss/oss-run.sh abseil --target x86_64 --modes strict --strict   # a new hod: strict only, the control kept
python3 scripts/oss/oss-report.py --target x86_64 census --out <findings>/X64-CENSUS.md
```

Trees under `oss/x64-build/` and `oss/x64-install/`, findings under
`findings/oss/x64/<project>/<reason>/` (each with `min.ll`, the module
reduced by `scripts/oss/oss-reduce.py` while hod still refuses it with
that reason). A strict build is judged per object by the `backend` field
of kiln's statistics log (the census page's "Strict builds"), not by the
build's exit status.

### Linker matrix

Every linking test runs with each of: GNU ld, lld and mold, which are
required, and wild where it is installed (`hod_testutil::linkers`,
`spec/harness.md` "Linkers"). The same object must link with all of them
and the programs must behave identically. wild is optional and not a
condition of a release: hod's code generators and object writer take no
workaround for it, and what it cannot pass is excepted on the harness
side as below.
Linker versions are pinned in the provisioning script and recorded in
status reports; what links and runs per linker and target, and every
exception with its reason, is `docs/status/linkers.md`.

wild is not a Debian package: a pinned release (0.11.0) is installed by
the workspace's provisioning script under `.toolchain/wild/<version>/`
and by the hosted set-up (`.github/actions/hod-ci`, checksum verified,
`HOD_WILD`). It is selected as the driver's `ld` with `-B` (GCC 14 has no
`-fuse-ld=wild`) and given `--no-gc-sections`, since it discards unused
sections by default where the other three do not. A missing wild is a
note under `HOD_REQUIRE_TOOLS=1`, never a failure
(`linkers::WILD_REQUIRED` is false and stays so).

mortar is the second optional linker (WP-281), built from one pinned
commit (`linkers::MORTAR_COMMIT`) into `.toolchain/mortar/<commit>/`
or named by `HOD_MORTAR`, selected as `ld` like wild, and optional in
the same way (`linkers::MORTAR_REQUIRED`). The output kinds it refuses by
name (`-shared`, `-r`) are cells recorded as REFUSED after the refusal's
text is checked (`linkers::REFUSALS`), not failures; a refused kind that
starts to link fails the test so that the cell is enabled.

A case one linker cannot pass is excepted in ONE table,
`hod_testutil::linkers::EXCEPTIONS` (linker, version, case, class,
reason), never by skipping a test: the test changes that one assertion
and still fails when the linker regresses on anything else. A failure
with one linker is triaged as (a) hod writing something the others
accept by leniency and the ABI forbids: fixed in hod; (b) the linker's
bug or an allowed difference, kept with a reproduction from objects hod
did not write; (c) a feature the linker says it does not support. In
the debugger layer, a listed case that breaks the ORACLE's session (the
`llc` program linked by that linker misses a check it meets with the
other three) is reported as XLINK, counted and listed, without failing
the run; only for the (linker, case, target, test, debugger) measured, and only
when the linked programs show the case (`spec/harness.md` "debug",
"Linker exceptions"). The exceptions are listed in
`docs/status/linkers.md` "Exceptions".

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

### Real projects for x86-64 on the AArch64 host (LLVM, cross-built)

`scripts/oss/llvm-x64/` (WP-180) builds LLVM 19.1.7 for
`x86_64-unknown-linux-gnu` on the AArch64 development host twice from
one source tree: by kiln with hod (fallback allowed, `HOD_CENSUS=1`;
`.build/oss/llvm-x64-hod`) and by kiln with its LLVM back end, the
control (`.build/oss/llvm-x64-llc`). It is WP-178's native recipe
(`scripts/oss/llvm/`: the wrapper over kiln's logging wrapper,
`KILN_STATS_LOG`, hod's coverage log, the census and the saved modules)
with the cross settings:

- the compiler wrappers add `--target=x86_64-unknown-linux-gnu`; kiln
  then finds the distribution's cross GCC (`/usr/lib/gcc-cross`) and its
  system root by itself, and mold links x86-64;
- CMake: `CMAKE_SYSTEM_NAME=Linux`, `CMAKE_SYSTEM_PROCESSOR=x86_64`,
  `CMAKE_CROSSCOMPILING_EMULATOR="qemu-x86_64;-L;/usr/x86_64-linux-gnu"`,
  `LLVM_HOST_TRIPLE` and `LLVM_DEFAULT_TARGET_TRIPLE` the target,
  `-O0`, assertions, no `-g`, static libraries, `X86;AArch64`;
- host tools: `llvm-tblgen` and `llvm-min-tblgen` copied once from
  WP-178's native tree (`LLVM_NATIVE_TOOL_DIR`); `llvm-config`,
  `llvm-nm` and `llvm-readobj` from the distribution's LLVM 19
  (`LLVM_CONFIG_PATH`, `LLVM_NM`, `LLVM_READOBJ`), so no `NATIVE`
  sub-build is configured;
- links get 16 GB of address space (mold maps -O0 archives whole);
- job slots (WP-222, both `scripts/oss/llvm/` and `llvm-x64/`): ninja is
  not a jobserver client, so the compiler wrapper takes one token of the
  real-project pool (`MAKEFLAGS`, set by the scripts' `env.sh` to
  `/run/hod/lanes.fifo`) per compile or link, and `lit.sh` (4) and
  `unittests.sh` (1) take theirs, through `scripts/oss/llvm/lane-token.py`:
  a blocking read of one byte per token, the same bytes written back once
  on every exit (also SIGINT, SIGTERM, SIGHUP); hod inside counts the
  first as its implicit slot. ninja's `-j` stays the upper bound;
- tests: the container has no `binfmt_misc` entry, so unit-test binaries
  run as `qemu-x86_64 -L /usr/x86_64-linux-gnu <binary>`
  (`unittests.sh`), and `qemu-tools.sh` replaces each x86-64 tool of
  `build/bin` by a launcher before `llvm-lit` runs (`lit.sh`, which also
  takes a list of tests: `lit-codegen-x86.txt` and `lit-instcombine.txt`
  are every 15th test of `test/CodeGen/X86` and every 10th of
  `test/Transforms/InstCombine`, sorted by name).

The census, the saved refused modules with their command lines and
smallest reproducers, and the test comparisons are in
`.build/findings/oss/LLVM-X64-CENSUS.md` (`census-file.sh`); the numbers
are in `spec/landed/harness/WP-180.md`. Under the emulator, tests that
spawn x86-64 child processes (`ProgramEnvTest.*`) and read the host's
`/proc/cpuinfo` (`Threading.NumPhysicalCoresSupported`) fail in both
trees.

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
- On a loaded machine wall time cannot be judged (`--check` says NOISY),
  so what binds there is the **counts gate** (WP-292, `xtask bench
  --check --counts`): the instructions strict hod executes under
  callgrind, which repeat to 0.02% at any load, and peak memory, against
  a counts baseline of their own (`bench/baselines/<machine
  id>[-<arch>]-counts.tsv`); wall time is shown and not judged. More
  than 1% more instructions on an input, more than 0.5% on the sum over
  the inputs, or more than 2% and 3 MiB more peak memory (hod on one
  thread; the peak resident set moves by up to 2.2 MiB under load) fails;
  fewer is "improved: re-record". It is a step of the full gate per
  target (`bench-aarch64`, `bench-x86_64`, about 90 s each with the
  release build current) and not of the hosted lanes,
  which have no baseline of their own (`spec/harness.md` "bench", "The
  counts gate").
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

**Merge to main (CI):** the quick gate above, `cargo xtask ci --quick`
with `HOD_REQUIRE_TOOLS=1`, run by the supervisor on the merged branch
and by the hosted lane ("Hosted CI", without `roundtrip`). L5 is part of
it, in `cargo test` (the ABI grids of `hod_a64` and `hod_x64`).

**Full gate:** `cargo xtask ci` without `--quick`: the quick gate's
steps with the whole corpus for L2 and every L4 seed with both oracles
and in hybrid mode, then, cheapest first, L7 and L6 for each target
architecture: `debug-x86_64`, `debug-aarch64` (every debugger test,
then `debug verify` of the target's corpus), `corpus-aarch64`,
`corpus-x86_64` (`corpus verify --strict` of the target's corpus with
the list of expected results, `docs/status/corpus-expected.tsv`: a
refusal or an undefined entry not on the list fails, and so does a row
the corpus no longer needs; `spec/harness.md` "corpus", "Expected
results"). It runs on the development machine, where the corpora are;
each step prints its wall time and the gate ends with a table of the
steps. "Both targets" means both target architectures, from one host
(the other's programs under its emulator), unless a step says
otherwise.

Steps that need tools or build output (the oracle, the linkers, the IR
corpus) pass with a notice without them, so a fresh checkout passes the
quick gate; `HOD_REQUIRE_TOOLS=1` makes each missing one a failure
(`spec/harness.md` "ci").

**Release candidate:** the full gate with `HOD_REQUIRE_TOOLS=1` (both
targets, every linker, the corpora present); L8 strict at the pass rate kiln achieves with
`llc`; L9 on the 1.0 set (`docs/RELEASE.md` section 5: the projects the
lanes have measured up to the candidate; each page row names the hod and
kiln commits it was measured at, a row measured at an earlier hod than
the candidate is marked so and never presented as measured at it; the
candidate re-runs the strict build and tests of as many rows as fit, in
the order LLVM and Clang, DuckDB, gRPC, Boost, abseil, then the rest; a
control is reused when kiln's pin and the project's revision are
unchanged, since it does not depend on hod); the linkers GNU ld, lld and
mold (wild is optional and not a condition); the fuzzing budget (L10, "The budget": `cargo xtask fuzz
--budget` on the candidate's frozen hod, fresh seeds) with no open
findings; and the support matrix
in `docs/status/` regenerated. The benchmark gate is not a condition of
the first release (decided 2026-11-14): `docs/status/perf.md` stays as
last measured, and the gate returns as a release condition with the
performance milestone that follows 1.0 (`docs/ROADMAP.md` M5).

## Hosted CI

GitHub Actions runs the gate on real hardware of both architectures
(`spec/harness.md` "CI" has the rules; `.github/workflows/`):

| | `ubuntu-24.04-arm` (AArch64) | `ubuntu-24.04` (x86-64) |
|---|---|---|
| container | Debian 13 (`buildpack-deps:trixie`) | the same |
| tools | LLVM 19 and 22, binutils, mold, gdb, lldb-19, the x86-64 cross toolchain, `qemu-user` | the same, with the AArch64 cross toolchain |
| AArch64 programs under test | native | `qemu-aarch64 -cpu cortex-a53` |
| x86-64 programs under test | `qemu-x86_64 -cpu qemu64,-sse3,-cx16,-lahf-lm` | native (the runner's CPU, more than the baseline) |
| jobs | `kiln`, `lints`, `test <part>` (five parts, `xtask::ci::TEST_PARTS`), `irdiff`, fan-in `check` | the same, fan-in `check-x86_64` |
| not run | `roundtrip` (no corpus) | the same |

Every job sets `HOD_REQUIRE_TOOLS=1`. On the x86-64 runner an x86-64
program is not held to the baseline CPU: the emulated AArch64 lane is
where a use of an instruction beyond the baseline faults.

**AVX-512 execution** (WP-229, WP-238; `spec/harness.md` "CI" item 8).
No `qemu` executes AVX-512, so its execution tests (`hod_x64`'s
`x86_avx512::*_on_hardware` and `abi::zmm_grid`) run on the x86-64 lane
only: natively when the runner's processor has a program's features
(the fleet is mixed: some runners have AVX-512, some none), else under
Intel SDE emulating a Sapphire Rapids (`.github/actions/sde`); only the
grid's Knights Landing machine (ER, PF) stays skipped. `avx512.yml` (an
`avx512/**` branch) runs them alone under SDE and fails when anything
else did not execute. The `test x64` job's summary says what ran where;
it uses SDE, and then fails the same way, with the repository variable
`HOD_CI_SDE=1`, which is set (since 2026-11-15), and skips otherwise.
Measured with it: the `test x64` job took 20 min 40 s on an AMD EPYC
9V74, 17 AVX-512 programs natively, the 2 AVX512-FP16 programs under SDE,
and Knights Landing's machine built and compared but not executed; on an
Intel Xeon 6975P-C (no SDE needed) 19 programs ran natively in a 17-min
job. A runner without AVX-512 (all 19 under SDE) is not measured yet.

The libc++ suite with kiln runs natively on both runners in the `sweep`
workflow (a `sweep/**` branch or by hand; never a required check), once
with hod and once with `llc`; a test that passes with `llc` and not with
hod fails it (`scripts/libcxx-sweep.py`; numbers in
`docs/status/libcxx-hod-x64.md` and `docs/status/libcxx-hod.md`, "Native
... runner").

**Tests that need a particular host.** A test of AArch64 programs runs
on both hosts when it takes its toolchain from `hod_testutil::target`
(natively, or under `qemu-aarch64`, its C and C++ drivers built by the
AArch64 gcc, g++ or `clang --target=aarch64-linux-gnu`); so do
`hod_obj`'s `link`, `link_tls` and `link_conventions`, `hod_cg`'s
`emit_link` and `emit_eh`, `hod_ir`'s `print_run`, `hod`'s `back` and
`musttail`, and `hod_a64`'s program tests (`Toolchain::aarch64`). A test
of an extension's instructions asks for QEMU's `max` model
(`Toolchain::with_emulated_cpu`: `neon`, `tail3`'s bf16 intrinsics).
`hod_interp`'s NaNs under `NanPolicy::Host` are the host's, so off an
AArch64 host `hod_a64`'s float tests take any NaN for the interpreter's.
These still run on an AArch64 host only, and why:

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
