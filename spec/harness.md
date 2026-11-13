# The harness

`xtask` is the project's harness: `cargo xtask <command>`. This file is the
contract of its commands: `shim`, `irdiff` (with the directive format of
its tests, "Test directives"), `corpus`, `roundtrip`, `bench`, `debug` and the gate,
`ci`. Still to be specified here: `bisect`.

## ci

`cargo xtask ci [--quick]` is the gate (`docs/VALIDATION.md` "Gates"): in
order `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --
-D warnings`, `cargo test --workspace --quiet`, then the `irdiff` runs of
"irdiff", "In the gate" (the quick gate's include a few seeds for the
other architecture under its emulator, the full gate's every seed with
both oracles and hybrid mode), then the `roundtrip` run of "roundtrip", "In the
gate", then (full gate only) the `debug` run of "debug", "In the gate". It
stops at the first step that fails, printing
`ci: <step> FAILED`, and ends with `ci: ok` and exit status 0 only if every
step passed.

**A step that needs build output or tools** follows one rule. What it
needs and does not find is a note and the step passes with what there is
(for `roundtrip`, a missing corpus skips the step with the one line
`roundtrip: skipped: no corpus at '<dir>' (cargo xtask corpus capture);
under HOD_REQUIRE_TOOLS=1 this fails`); under `HOD_REQUIRE_TOOLS=1` it
fails the step. So a fresh checkout passes `cargo xtask ci --quick`, and a
release candidate, which sets the variable, cannot pass without the
oracle, the linkers and the corpus. The corpus is the default one
("corpus", "Where things are"): the gate takes no corpus option.

A cargo step whose standard error has, as a whole line (leading and
trailing blanks aside), one of these two rustc errors is run **once**
more, after the note `ci: [retry] cargo <step> failed with <which>: ...`:

- a line that starts with `error[E0463]` (can't find crate);
- exactly ``error: crate `<name>` required to be available in rlib format,
  but was not found in this form`` (`<name>` one word).

The build directory is on a workspace file system (a bind mount of the
host) whose directory listings, under concurrent builds, now and then lag
behind the files cargo has just written. rustc finds a dependency that is
not on its command line (a crate of the workspace used through another)
by listing `-L dependency=<deps>`: when neither the dependency's `.rlib`
nor its `.rmeta` is listed it says E0463; when the `.rmeta` (written at
the end of analysis, seconds earlier) is listed and the `.rlib` (renamed
into place at the end of code generation) is not yet, and a binary is
being linked, it says the second. One cause; nothing is wrong with the
code, and the rerun finds the artifacts or rebuilds them. The gate's steps
run one after another, each a single cargo, and `clippy` and `test`
artifacts have different names, so nothing of the gate's own provokes it.
Any other failure (the `dylib` form of the second message among them: hod
links no dylib), and a second failure of the step, fails the gate.
(Ported from kiln's `xtask ci`, which met the same file system.) Tests:
`xtask::ci::tests::retries_only_missing_crates_once`,
`retries_a_crate_seen_only_as_rmeta_once`.

## shim

```
cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]
```

Builds `<out>`: a directory that kiln accepts as an LLVM installation and in
which `llc` is hod, with the installation's `llc` as its fallback and
bitcode read by hod's own reader (`spec/cli.md` "Fallback protocol",
"Bitcode conversion"). This is how hod is plugged into kiln, and
through kiln into every C++ test suite, without changing kiln. Unix hosts
only. Arguments are `--name <value>` or `--name=<value>`. Exit status: 0; 1
when the directory cannot be built; 2 for a usage error.

### What kiln requires of an LLVM directory

Facts about kiln's driver that the shim depends on. (Paths are in the kiln
repository; kiln's own contract is its `spec/driver.md`, "LLVM
discovery".)

- kiln takes the directory from `--llvm-dir=<dir>` (joined form only; the
  last one wins), else `$KILN_LLVM_DIR`, else a search
  (`crates/kiln_driver/src/llvm.rs`, `Search::run`). The flag beats the
  variable, which matters because kiln's own test harness sets the
  variable.
- The directory must contain `bin/opt`, `bin/llc` and `bin/clang++`, each a
  file after following links (`llvm.rs`, `REQUIRED_TOOLS` and `check`).
- kiln must be able to tell the directory's LLVM major, and it must be one
  kiln supports (22 and 19 today). It tries, in order: the single all-digits entry
  of `<dir>/lib/clang/`; the leading integer of `<dir>/bin/llvm-config
  --version`; the directory's own name, `llvm-N`, `llvmN` or `llvm@N`
  (`llvm.rs`, `dir_major`). kiln never runs `llc --version`.
- Every tool is run as `<dir>/bin/<tool>` (`crates/kiln_driver/src/
  toolchain.rs`, `find_tool`), and libc++'s headers, when wanted, are
  looked for at `<dir>/include/c++/v1`.
- kiln runs `llc` with the module on standard input and `-o <object>`, and
  for some translation units (coroutines, `always_inline`, sanitizers,
  coverage, `-O1` and above) runs `opt` first and gives `llc` bitcode
  (`crates/kiln_driver/src/codegen_cmd.rs`, `ir_passes` and `llc`). A
  failing `llc` is reported as `backend command failed with exit code N`
  (`crates/kiln_diag/src/ids/drv.rs`, `drv_command_failed`), after its
  standard error.

### Layout

```
<out>/
  .hod-shim            marker: this directory was made by `cargo xtask shim`
  bin/                 a real directory
    llc                script: hod, with the real llc as its fallback and
                       hod's own bitcode reader
    <every other entry of <llvm>/bin>     symbolic links to the real ones
  <every other entry of <llvm>>           symbolic links to the real ones
  kiln++-hod           script, with --kiln only: kiln++ using this directory
```

- `<llvm>` is resolved (`canonicalize`) first; every link is absolute and
  points into the resolved installation. `lib`, `include`, `share` and the
  rest are single links, so kiln finds `lib/clang/<N>`, the runtime
  libraries and the headers exactly as in the real installation.
- `bin/llc` is a `/bin/sh` script, mode 755:

  ```sh
  HOD_FALLBACK_LLC='<llvm>/bin/llc' HOD_BITCODE_VIA=native exec '<hod>' "$@"
  ```

  It sets the fallback and the bitcode reading and nothing else:
  `HOD_STRICT` and `HOD_COVERAGE_LOG` reach hod from the caller's
  environment, so the same shim serves hybrid and strict runs
  (`spec/cli.md`). Both are fixed whatever the caller's environment says:
  the fallback is the installation's own `llc`, and bitcode (which kiln
  hands `llc` after `opt`) is read by `hod_bc`, never by a converter a
  caller names, which could be another LLVM's. Until WP-068 the script
  named the installation's `llvm-dis` as converter; hod's reader gives the
  same object (the evidence: `spec/landed/cli/WP-068.md`) without a
  process and a print-and-parse round trip per module (about 100 ms on a
  libc++ test module). A bitcode module `hod_bc` cannot read is declined
  with the reader's reason and goes to the fallback, which reads bitcode
  itself, so no text is ever needed. The command's summary says
  `bitcode:  read by hod (HOD_BITCODE_VIA=native)`.
- `bin/llvm-dis` is a link to the real one, like every other tool.
- The script costs one `sh` start per translation unit before hod runs
  (measured: `spec/landed/cli/WP-020.md`). It stays a script because the
  corpus capture puts its own program where hod stands and relies on the
  variables the script exports (`spec/harness.md` "corpus").
- `kiln++-hod` is a `/bin/sh` script, mode 755:

  ```sh
  exec '<kiln++>' --llvm-dir='<out>' "$@"
  ```

  The flag comes first, so the wrapper wins over `$KILN_LLVM_DIR`, and a
  caller's own later `--llvm-dir=` wins over the wrapper.
- Paths in the scripts are absolute and quoted for the shell (single
  quotes; a single quote in a path is written `'\''`). A path that is not
  UTF-8 is refused.
- `.hod-shim` is a regular file that records the request:
  `hod-shim 1`, `llvm <path>`, `hod <path>`, `kiln <path or ->`, one per
  line.

The shim must show kiln the same major as `<llvm>`, by a means that
survives mirroring. `lib/clang/<N>` and `bin/llvm-config` do (they are
links). An installation whose major is told only by its name is accepted
only if `<out>` is itself named `llvm-<N>` with the same N; an installation
with no major kiln could tell is refused.

### Updating and refusing

- The command is idempotent: run again with the same request, it changes
  nothing (no file or link is rewritten).
- Run again with a different request (another hod, another LLVM, with or
  without `--kiln`), it makes `<out>` match: stale links and the generated
  scripts are replaced or removed. Scripts are written to a temporary file
  beside the destination and renamed over it, so a compile running at the
  same time never sees half a script.
- It writes only inside `<out>`, and never through a symbolic link: an
  entry of `<out>` that should be a script and is a link is removed and
  replaced, never opened for writing. The real installation is never
  modified.
- It refuses, with a message and without changing anything:
  - an `<out>` that exists and is not a directory with a regular-file
    `.hod-shim` (it was not made by this command);
  - an `<out>` that contains, or is inside, the LLVM installation;
  - an `--llvm` that is itself a shim, has no `bin/opt`, `bin/llc` or
    `bin/clang++`, or has an entry named `.hod-shim` or `kiln++-hod`;
  - a `--hod` or `--kiln` that is not a file.
- It stops, leaving what it has done, if `<out>` (already a shim) holds
  something this command did not make: a regular file or a directory that
  is neither a link nor one of the generated files, or a `bin` that is not
  a real directory.

### Library

`xtask::shim::build(&Request) -> Result<Shim, String>` is the command as a
function; tests that need a shim call it (`crates/hod_cli/tests/
kiln_e2e.rs`).

### Tests

`xtask/src/shim.rs`: `layout`, `llc_runs_hod_with_the_real_llc_as_fallback`
(and hod's own bitcode reader), `no_converter_without_llvm_dis`,
`wrapper_runs_kiln_with_the_shim`, `paths_with_spaces_and_quotes`,
`idempotent`, `rebuild_follows_the_request`,
`refuses_a_directory_it_did_not_make`, `never_writes_through_links`,
`rejects_unusable_requests`, `major_detection`, `argument_parsing`. These
use a fake installation and need no tools.

`crates/hod_cli/tests/kiln_e2e.rs`,
`kiln_builds_and_runs_programs_through_the_shim`: with a real kiln and a
shim of the host's LLVM, and then of every other supported LLVM installed
as `/usr/lib/llvm-<N>`, a program with iostream, a virtual call and an
exception is compiled through `kiln++-hod`, linked, run and its output
checked, and the coverage log has one line per translation unit, which
says the module was text and was not converted. Three programs reach hod
as bitcode (a coroutine, an `always_inline` function, and the first
program at `-O1`): each is linked, run and checked, and its coverage line
says that hod was given bitcode and converted nothing (no conversion
time), and either built it or declined it with the fallback exiting 0. A
caller's `HOD_BITCODE_VIA` naming a file that does not exist is set
throughout and must not matter. With `HOD_STRICT=1` the compile fails
with hod's diagnostic for text and for bitcode alike. The test
needs `kiln++`: `$HOD_KILN`, else
`<build root>/target-kiln/release/kiln++` beside this checkout's target
directory. Without one it skips with a note; under `HOD_REQUIRE_TOOLS=1`
it fails.

## irdiff

```
cargo xtask irdiff [options] [<file.ll or directory>...]
```

The IR differential harness, layer L4 of `docs/VALIDATION.md`. A test is
one LLVM IR module with a `main` and `;@` directives ("Test directives"),
or several modules that make one program ("Several modules"). The harness
compiles it with the oracle (`llc -O0` with the SelectionDAG selector,
"The oracle") and with hod, links the objects with every linker of the
matrix, runs the programs, and requires the oracle's program to behave as
the directives say and hod's program to behave as the oracle's. Unix hosts only; programs are linked
and run on Linux only, those of the architecture that is not the host's
under its emulator ("Cross runs").

A path argument is a test file (any name) or a directory, searched
recursively for `*.ll`. With no path the tests are the workspace's
`tests/ir/`. Tests run in the order of their names, without duplicates. A
test's name is its path relative to the workspace when it is inside it,
else the path as given.

### Options

Value options are `--name <value>` or `--name=<value>`; `--` ends the
options.

| option | meaning | default |
|---|---|---|
| `-j <N>`, `-j<N>`, `--jobs <N>` | tests run at once, at least 1; under a jobserver an upper bound ("Jobserver") | the smaller of 4 and the machine's cores; more than 16 is 16 |
| `--llvm <major or dir>` | the oracle: an LLVM major, or an installation directory (its `bin/llc`) | the first of 22 and 19 that is installed |
| `--target <arch>` | `aarch64` or `x86_64` | this host's |
| `--linker <name>` | `bfd`, `lld`, `mold` or `wild`; repeatable; the order given does not matter | every one that is found |
| `--tier <quick or all>` | `quick`: only tests with `tier: quick` | `all` |
| `--hod <binary>` | the hod to test | build the workspace's (`cargo build --package hod_cli --bin hod`, development profile, into the build directory) |
| `--hybrid` | let hod fall back to the oracle ("Hybrid mode") | strict |
| `--witness` | also build each test with `llc -O2` and require its program to behave as the oracle's ("The witness") | no witness |
| `--keep` | keep the work directories of failing tests ("Work directory") | remove them |
| `--results <file>` | where the results file goes | under the build directory ("Results file") |
| `-v`, `--verbose` | list every result in the summary | only the notable ones |
| `-h`, `--help` | print the usage and exit 0 | |

The build directory is `$CARGO_TARGET_DIR` when it is set, else
`<workspace>/target`. Everything the command writes is under
`<build directory>/irdiff/`, except a `--results` file.

### Tools

- **Oracle.** For a major `N`: `llc-N` on `PATH`, else
  `/usr/lib/llvm-N/bin/llc` (either must report that major in `llc
  --version`: "LLVM version N."), else a plain `llc` on `PATH` that
  reports it. With no `--llvm`, 22 is tried, then 19, then a plain `llc`
  that is one of the two. `--llvm <dir>` is `<dir>/bin/llc`, of whatever
  major it reports.
- **Linker driver.** `gcc` on `PATH` for the host's architecture, the GNU
  cross driver `<prefix>-gcc` for the other (`x86_64-linux-gnu-gcc`,
  `aarch64-linux-gnu-gcc`). Programs are linked through it so that the C
  runtime and libraries are found the way a C++ build finds them.
- **Linkers.** `bfd`: `ld.bfd`, or `<prefix>-ld.bfd` for the other
  architecture (lld, mold and wild link every target as they are). `lld`: `ld.lld-N` on `PATH` or
  `/usr/lib/llvm-N/bin/ld.lld` for 22 then 19, then `ld.lld`. `mold`:
  `mold`, then `ld.mold`. `wild`: `wild`, then `ld.wild`. The matrix is
  every one of these that is found, in this order.
- **Emulator**, for the architecture that is not the host's ("Cross
  runs"): `qemu-<arch>` on `PATH`, then `qemu-<arch>-static`; and the
  cross system root `/usr/<prefix>` (the target's C library and dynamic
  loader).

The GNU prefix is `aarch64-linux-gnu` or `x86_64-linux-gnu`. A note for a
missing tool names its Debian package: `gcc-<arch>-linux-gnu` (with `_`
as `-`) for the driver, `binutils-<arch>-linux-gnu` for `bfd`,
`qemu-user` for the emulator, `libc6-dev-<debian arch>-cross`
(`amd64`, `arm64`) for the system root.

What is missing:

| missing | effect | under `HOD_REQUIRE_TOOLS=1` |
|---|---|---|
| the oracle | every test is SKIP, with a note | failure (exit status 3) |
| the linker driver | objects are built, nothing is linked or run ("Cross targets"), with a note | failure |
| the emulator or the system root, for the other architecture | objects are built, nothing is linked or run ("Cross targets"), with a note | failure |
| `bfd`, `lld` or `mold` | that linker is not in the matrix, with a note | failure |
| `wild` | not in the matrix, with a note | still a note: it is optional until it is provisioned |
| a linker named with `--linker` | usage error (exit status 2) | the same |

A failure for a missing tool does not stop the run: the tests still run
with what there is.

### What a test does

For a test whose `target`, `llvm` and `dialect` directives admit this
run (else it is SKIP), in its own directory under the work directory:

0. **opt**, for a test with `input: pre-opt` ("Pre-opt input, dialects
   and oracle skips"): `opt -passes=default<O0> -mtriple=<triple> -S -o
   pre-opt.ll <test>`, whose output is what the oracle (and the witness)
   build in step 1. hod builds the test itself.

1. **Oracle.** `llc -O0 -global-isel=0 -fast-isel=0 -filetype=obj
   -mtriple=<triple> -relocation-model=<model> <llc-args> -o oracle.o
   <test>`. The triple is `aarch64-unknown-linux-gnu` or
   `x86_64-unknown-linux-gnu`; a test file has no `target triple` of its
   own, which is what lets one file serve both targets. The model is
   `pic`, or `static` for a test with `link: no-pie`. `<llc-args>` are the
   test's `llc-args` directive, if any.
2. **hod.** `hod --coverage-log=<dir>/coverage.tsv --strict -O0
   -filetype=obj -mtriple=<triple> -relocation-model=<model> <llc-args>
   -o hod.o <test>`: what a compiler driver gives `llc` at `-O0`, without
   the oracle's selector, with `HOD_FALLBACK_LLC`, `HOD_STRICT`,
   `HOD_COVERAGE_LOG` and `HOD_IN_FALLBACK` removed from its environment:
   hod is configured by these options alone. What hod did is read from its
   exit status and the coverage log's one line (`spec/cli.md`): status 0
   and outcome `hod` with an object written is "built"; status 2 and
   outcome `error` is "declined", with the line's reason. Anything else is
   a failure of hod: another status, a signal, the time limit, no object,
   no log, or a log that is not one well-formed line.
3. **Link and run**, once per linker, each side in the same way: `gcc
   -B<dir with that linker as ld> -pie -Wl,--fatal-warnings -o <program>
   <objects>` and then `-lstdc++` and `-lm` as the test's `link` directive
   asks; `-no-pie` instead of `-pie` for `link: no-pie`, and no
   `-Wl,--fatal-warnings` for `link: allow-warnings`. lld and mold are
   told to use one thread (`-Wl,--threads=1`, `-Wl,--thread-count=1`): the
   harness is what runs things in parallel. The harness then checks which
   linker made the program, by the signature in its `.comment` section
   (`LLD <digit>`, `mold <digit>`, neither for GNU ld; wild's is not
   specified yet and is not checked); a program made by another linker is
   an ERROR. The program runs in its directory with the test's `args` and
   `stdin`.
4. **Compare.** The oracle's program against the directives: how it ended
   (`exit`) and its standard output, byte for byte. hod's program against
   the oracle's: the same two things. Standard error is not compared.

The oracle's side runs first and in full, whatever hod did: a test whose
oracle side is wrong is a broken test, and that is reported before
anything is said about hod.

### The oracle

The reference compiler is `llc -O0` with the SelectionDAG instruction
selector (`-global-isel=0 -fast-isel=0`), on every target. Plain `llc
-O0` is not a reference: on AArch64 it selects with GlobalISel, and with
`-global-isel=0` alone with FastISel, and each of those miscompiles a
correct program (`spec/interp.md` §10.1, the seed tests
`selector_narrow_load.ll` and `selector_i1_compare.ll`, with LLVM 22 and
19). SelectionDAG is the selector every optimized build uses, so it is the
best-tested one.

The arguments are chosen per target in one place,
`xtask::irdiff::tools::reference_selector`, which the hybrid fallback and
every harness command that builds a reference use. x86-64 gets the same
arguments. Plain `llc -O0` there selects with FastISel (with SelectionDAG
behind it for what FastISel does not handle). With LLVM 22 and with
LLVM 19, FastISel and `-fast-isel=0` built different code for every seed
(43 of 43 differ in assembly) and the same behaviour for every seed with
every linker under the emulator (WP-025: a stand-in hod of each kind in
hybrid mode, 0 differences; `coro/` built after `opt` by hand), so no x86-64
miscompile is known. The reference keeps SelectionDAG alone all the same:
it is the selector every optimized build uses. hod is not given the
selector: it gets what a compiler driver gives `llc`.

### The witness

With `--witness`, each module is also built by the oracle's `llc` at
`-O2` (`-O2 -filetype=obj -mtriple=... -relocation-model=... <llc-args>
-o witness.o`), and its program is linked and run once per test, with the
first linker whose oracle program is what the directives say. Its exit
and standard output must equal the oracle's program's. A disagreement, a
witness build that fails or a witness link that fails is an ERROR of that
linker and of every later one, whose detail starts with `witness: `
("the program of llc -O2's object wrote stdout that differs from llc
-O0's at line 1: ..."): two configurations of LLVM disagree, so the
reference cannot be trusted for this test, which says nothing about hod.
The directives are checked first, so a wrong test is reported as a wrong
test. A test with undefined behaviour can trip the witness; that is a
broken test.

### Outcomes

One result per test and linker; a test that is decided before any linker
runs has one result with no linker.

| outcome | meaning |
|---|---|
| PASS | hod built the object and its program behaved as the oracle's (as the directives say, where an `oracle-skip` applies) |
| FAIL | hod is wrong: it failed (step 2), its object did not link, its program ended or printed differently or ran into a limit; or the test's `unsupported` directives do not describe what hod did |
| UNSUPPORTED | hod declined, with the reason an `unsupported` directive of the test names |
| XFAIL | a FAIL where an `xfail` directive applies; the detail is the directive's reason, then the failure |
| XPASS | a PASS where an `xfail` directive applies: the directive is stale |
| ERROR | the test or a tool is wrong, which says nothing about hod: the oracle crashed, failed, timed out or wrote no object; the oracle's object did not link; the witness did not confirm the oracle ("The witness"); the oracle's program did not behave as the directives say or ran into a limit; the program was linked by the wrong linker; the directives do not parse; the harness could not do its work |
| SKIP | the test is not valid for this target or this LLVM, or is in another LLVM's dialect; or no oracle was found; or programs cannot be run here and hod built the object ("Cross targets") |

The `unsupported` directives are checked exactly, and `xfail` does not
excuse them. Each of these is a FAIL that stays a FAIL:

- hod declines and no `unsupported` directive applies;
- hod declines with a reason other than the directive's;
- hod builds a test that an applying `unsupported` directive says it
  cannot.

So a test stops being UNSUPPORTED in the commit that makes hod compile it:
that commit removes the directive, and from then on the test is compared.

An oracle that crashes or times out is an ERROR, never a pass and never a
hod failure (`docs/VALIDATION.md` "Rules for tests").

### Hybrid mode

With `--hybrid`, hod is run with `--fallback=<work>/reference-llc` instead
of `--strict`: a script the run writes that executes the oracle's `llc`
with hod's arguments and then `-global-isel=0 -fast-isel=0`, so that what
the fallback builds is what the oracle builds. Status 0 with the log outcome `fallback` and an object written
is also "built": the object is linked, run and compared like any other,
and the result records that the fallback built it and why. An
`unsupported` directive that applies is satisfied by a fallback with its
reason; a fallback with another reason, or hod building the test itself,
is the FAIL above. The summary's coverage line counts both.

### Pre-opt input, dialects and oracle skips

Three directives (WP-105, design 0005 §5.3) serve tests of IR *before*
optimisation, as a front end writes it for its driver's pipeline:
coroutines first, whose intrinsics only `opt`'s coroutine passes lower.

- **`input: pre-opt`.** The oracle is `opt -passes=default<O0>` (the
  passes a compiler driver runs at `-O0`, the coroutine passes among
  them; WP-021's method) and then `llc` as above; so is the witness.
  `opt` is the one beside the oracle's `llc`: `opt-<N>` beside
  `llc-<N>`, else `opt` beside `llc`; an oracle without it is an ERROR
  of the harness. A failing `opt` is an ERROR (`oracle: opt failed
  ...`). In hybrid mode such a test's fallback is
  `<work>/reference-opt-llc`, which runs that `opt` on hod's input
  (into `<output>.pre-opt.ll`, removed afterwards) and then the
  oracle's `llc` on the result, with hod's other arguments and the
  selector.
- **`dialect: N`.** The test is written in LLVM `N`'s dialect of the IR
  (LLVM 22's coroutine intrinsics are not LLVM 19's), so it runs only
  with the oracle of that major, and is SKIP with another ("in the LLVM
  19 dialect (this run: 22)"). It excludes `llvm`. In the seed corpus a
  test in one dialect has its twin in the other, `<stem>-19.ll` and
  `<stem>-22.ll`: the "both LLVMs" rule holds per dialect.
- **`oracle-skip(llvm=N[,M...]): <reason>`.** With an oracle of a major
  it names, the oracle is not trusted for this test: neither it nor the
  witness is built or run, and the program of hod's object is compared
  with the directives alone, which the test must therefore spell out
  completely. Never "either output". The record's detail says
  `oracle skipped: <reason>`. A test may have several, with different
  majors; the first that names the run's major applies.

### Oracle skips

Every `oracle-skip` of `tests/ir/`, one row each, by path, majors and
reason exactly as the directive spells them (`irdiff_real`'s
`dialects_come_in_pairs_and_oracle_skips_are_listed` checks this):

| test | LLVM | reason |
|---|---|---|

None yet: the first is expected with WP-106's `gro-throws-uaf` seed,
whose oracle crashes under poisoning (design 0005 §5.3).

### Cross runs

`--target` names the architecture the objects are for. When it is not the
host's (x86-64 on an AArch64 machine, the case this is built for), the
programs are linked with that architecture's cross driver and run under
its emulator, the user-mode `qemu-<arch>`, as

```
qemu-<arch> -cpu <cpu> -L /usr/<prefix> <program> <args>
```

with everything else as for the host ("What a test does"). `-L` points the
emulator at the cross system root, where the program's dynamic loader and
C library are; without it a dynamic program does not start. `-cpu` is
chosen, not the emulator's default (`max`, every extension it knows):
programs run on the target's baseline, so an object that uses an
instruction the baseline does not have dies with SIGILL instead of
passing. The CPUs are `hod_testutil::target`'s:

| target | `-cpu` | what it is |
|---|---|---|
| x86-64 | `qemu64,-sse3,-cx16,-lahf-lm` | x86-64-v1: SSE2, no SSE3, SSSE3, SSE4, POPCNT, CMPXCHG16B or LAHF in 64-bit mode |
| AArch64 | `cortex-a53` | Armv8.0-A with the FP and SIMD unit, no LSE atomics |

`cx16` is off although LLVM's default x86-64 CPU does not use it either
(an `i128` `cmpxchg` is a call to `__atomic_compare_exchange_16` with
LLVM 19 and 22): if hod ever uses it, the choice is a decision of its own
in `spec/abi-x64.md`, not something the emulator lets through.

The summary's first line says `(emulated: <emulator> -cpu <cpu> -L
<root>)` after the target. A program under the emulator has six times a
native program's wall-clock limit ("Child processes"); its address-space
limit is the same (qemu runs a program in 1 GiB). A run in which hod
declined every test it was asked to build (more than one), with the same
reason or not, says so in one note,
`hod declined all <N> tests it was asked to build for <arch>; only the
reference side (llc, the linkers, the programs) was checked`, instead of
leaving it to be read from the counts: the reference side is still run in
full, so a broken test or a broken cross toolchain is an ERROR. (WP-025:
for x86-64 hod declines every seed with `global:variable-definition` or
an earlier construct, and every seed's reference side passes with every
linker, with LLVM 22 and 19.)

A debugger can attach to an emulated program through qemu's gdb stub
(`qemu-<arch> -g <port>`): LLDB (`lldb-19`, `gdb-remote <port>`) debugs
an x86-64 program from this AArch64 host with breakpoints, backtraces,
variables and registers, but sees no shared libraries; the host's `gdb`
is AArch64-only and needs `gdb-multiarch` (`spec/landed/harness/WP-025.md`).
`debug` does not use the emulator.

### Cross targets

Programs are linked and run only when the target is this host's
architecture or its emulator, cross driver and system root were found
("Cross runs"), the host is Linux, and at least one linker was found.
Otherwise the run says why in a note, both objects are still
built, and each test has one result with no linker:

- ERROR if the oracle's side failed;
- FAIL, XFAIL or UNSUPPORTED if hod's side is decided without a program
  (it failed, or declined), with "the oracle's program was not run" in the
  detail; an `xfail` condition that names a linker does not apply;
- else SKIP: hod built an object and nothing here can run it.

Running programs on a second machine is not specified.

### Child processes

Every process the harness starts (the oracle, hod, the driver and through
it the linker, the programs) has:

- a wall-clock limit: when it passes, the process and everything it
  started (its process group) are killed, and the step "timed out";
- an address-space limit and a CPU-time limit set in the child before it
  starts (`ulimit -v`, `ulimit -t`), which hold even if the harness dies.
  The CPU limit is the wall-clock limit rounded up to a second, plus one;
- core files switched off (`ulimit -c 0`);
- at most 16 MiB of each output stream kept; a program that writes more
  does not match;
- its own process group; its standard input is what the harness gives it
  (nothing, or a test's `stdin`), its standard output and error are
  captured.

| | wall clock | address space |
|---|---|---|
| `llc`, `hod`, the driver and linker | 60 s | 4 GiB |
| a test's program | 10 s | 1 GiB |
| a test's program under the emulator ("Cross runs") | 60 s | 1 GiB |

A harness run therefore ends whatever its children do, and `-j` bounds
the processes alive at once to one per job (plus a linker under its
driver).

Each process also holds a jobserver token for its lifetime when the
harness inherited a pool, and its wall-clock limit starts once the token
is held ("Jobserver").

### Work directory

Each run works in `<build directory>/irdiff/run-<pid>-<time>/`, a new
directory, so that runs at the same time do not meet. It holds the
hybrid fallback `reference-llc`, one directory per linker
(`linkers/<linker>/`) with that linker under the name `ld`, and one
directory per test (`NNNN-<name>`) with `oracle.o`, `witness.o` (with
`--witness`), `hod.o`, `coverage.tsv` and, per linker, the programs
`<linker>/oracle`, `<linker>/witness` and `<linker>/hod`. A test of
several modules has, instead of the objects and the log, `modules/<name>.ll`,
`.oracle.o`, `.witness.o`, `.hod.o` and `.coverage.tsv` per module. A test's directory is removed when the test is done
and the run's directory when the run ends, also when it fails. With
`--keep`, the directories of tests with a FAIL, XPASS or ERROR are kept,
and the summary says where.

### Results file

`<build directory>/irdiff/results-<arch>-llvm<major>.tsv`
(`results-<arch>-llvm<major>-hybrid.tsv` in hybrid mode,
`results-<arch>-nollvm.tsv` when no oracle was found), or the `--results`
file. Each run replaces it. One line per result, in order of test name,
then linker in matrix order; nine fields separated by tabs:

| # | field | value |
|---|---|---|
| 1 | outcome | `PASS`, `FAIL`, `UNSUPPORTED`, `XFAIL`, `XPASS`, `ERROR`, `SKIP` |
| 2 | test | the test's name |
| 3 | target | the triple |
| 4 | LLVM | the oracle's major, or `-` |
| 5 | linker | `bfd`, `lld`, `mold`, `wild`, or `-` |
| 6 | built by | `hod`; `fallback`; `declined`; `-` when hod was not run or failed |
| 7 | reason | the coverage-log reason of a decline or a fallback, or `-` |
| 8 | detail | what differed, what failed, why it was skipped, or `-` |
| 9 | covers | the test's `covers` directive, or `-` |

Fields 2 and 7 to 9 are free text, escaped as in hod's coverage log
(`spec/cli.md`): `\\`, `\t`, `\n`, `\r`; an empty value is `-`. The file
does not depend on `-j`.

### Summary

On standard output: one line that says what ran (oracle, hod, target,
linkers, mode, `; witness` with the witness, jobs, and the jobserver pool
if there is one); a line `irdiff: note: ...` per note and `irdiff:
error: ... (HOD_REQUIRE_TOOLS=1)` per missing required tool; one line per
FAIL, XPASS, ERROR, XFAIL and SKIP (per PASS and UNSUPPORTED too with
`-v`), where the linkers of a test with the same result share a line, and
the text is the reason and the detail:

```
FAIL        tests/ir/switch.ll [lld mold]: the program of hod's object ended with exit status 1, llc's with exit status 0
```

then the counts, the coverage, and where the results are:

```
irdiff: 29 tests, 87 results: 0 PASS, 0 FAIL, 87 UNSUPPORTED, 0 XFAIL, 0 XPASS, 0 ERROR, 0 SKIP in 4.4s
irdiff: coverage: hod built 0 of 29 objects; declined codegen-not-implemented: 29
irdiff: results: <build directory>/irdiff/results-aarch64-llvm22.tsv
```

Coverage is per test: how many of the objects hod was asked for it built
itself, and the others by `declined <reason>` or `fallback <reason>`. A
test of several modules counts once: built by hod when hod built every
module, else by the first module hod declined or fell back on.

### Exit status

| status | when |
|---|---|
| 0 | no FAIL, XPASS or ERROR, and no required tool missing |
| 1 | at least one FAIL or XPASS |
| 3 | no FAIL or XPASS, but an ERROR, or a required tool is missing |
| 2 | the request cannot be carried out: a usage error, a path that names no test, no test in the selected tier, an unusable `--llvm`, `--hod` or `--linker`; nothing was run |

1 is "hod, or a test's claim about hod, is wrong"; 3 is "the run cannot be
trusted". UNSUPPORTED, XFAIL and SKIP do not fail a run.

### In the gate

The gate's runs are planned by `xtask::ci::irdiff_runs` and each is
logged as the command that makes it (`ci: cargo xtask irdiff ...`). On a
Linux host of a known architecture, "the other architecture" is the one
that is not the host's (x86-64 on AArch64), run under its emulator ("Cross
runs").

| tier | runs, in order |
|---|---|
| `ci --quick` | `irdiff --tier quick` with the default oracle; `irdiff --target <other>` on the fixed `xtask::ci::QUICK_CROSS_SEEDS` (`main_args.ll`, `exit_float.ll`, `two_modules.ll`, `eh_invoke.ll`) |
| `ci` | `irdiff --witness` over every test with LLVM 22, then 19; `irdiff --hybrid` over every test with the default oracle; `irdiff --target <other>` over every test with LLVM 22, then 19 |

The quick cross seeds cross the process interface (arguments, input, exit
status), call into the C library, use floating point, link two modules
and throw a C++ exception, in both dialects; they are valid for both
targets. The full gate runs the other architecture once per oracle, so a
program LLVM 19 builds differently from LLVM 22 for that target is a
failure of one of the two runs. The cross runs have no witness: the
witness checks the reference, and the reference of the other target is
the same `llc` with the same selector (WP-025 ran it: no difference).

What they cost, measured at load average 20 to 22 on the 16-core AArch64
host with `-j 4` (WP-025, `spec/landed/harness/WP-025.md`): every seed
for x86-64 under the emulator, with hod declining, takes 4 to 18 s, and
natively, with hod building them, 6 to 13 s. The spread is the load, not
the emulator. The quick cross run takes about a second. An emulated
program costs about 27 ms of wall time, a native one 3 ms; a link costs
25 to 60 ms either way. Nothing in a test depends on wall time: the
limits are bounds, not measurements.

The gate fails if any of these runs would exit non-zero. Without the tools
the runs skip with their notes and the gate passes; under
`HOD_REQUIRE_TOOLS=1` a missing oracle (for the full gate: either one),
driver, `bfd`, `lld` or `mold`, or for the cross runs the other
architecture's driver, emulator or system root fails it.

### Library

`xtask::irdiff`, for the harness commands that produce IR themselves
(`corpus`, `bisect`, fuzzing):

- `run(&Config) -> Result<Report, String>` is the command as a function:
  `Config` has one field per option, plus the build directory, the
  limits, whether tools are required, and where tools are searched. `Err`
  is exit status 2.
- `Session { toolchain, arch, host, mode, tool_limits, run_limits, work,
  keep, witness }` is the core, and knows nothing of `tests/ir/` or of directives
  in files: `Session::run_tests(&[Test], jobs) -> Result<Vec<Vec<Record>>,
  String>` takes tests that are a name, an IR file and an `Expectations`,
  and returns each test's records in the order given.
- `Expectations` is what directives say; `directives::parse` reads it
  from a file's text, `Expectations::to_directives` writes it back, and a
  caller may equally fill it in itself.
- `Report` has the records, the notes, the missing tools,
  `exit_status()`, `summary(verbose)`, `results_text()` and
  `write_results(path)`.
- `Record::failure` is which step went wrong, for a caller to decide on
  (the detail says the same for a reader, and its words may change). It
  is set on every ERROR, FAIL and XFAIL, and is `None` on the others; it
  is not in the results file. An ERROR's kind is about the oracle's side,
  the witness or the harness; a FAIL's about hod's side.

  | `Failure` | the step |
  |---|---|
  | `ReferenceBuild` | the oracle's `llc` failed or wrote no object |
  | `Witness` | the witness did not confirm the oracle (every witness ERROR) |
  | `HodBuild` | hod failed on a module (step 2), whatever the way |
  | `Declined` | hod declined, or did what the `unsupported` directives do not describe |
  | `Link` | a link failed or wrote no program |
  | `Run` | a program ran into its time limit or wrote more than is kept |
  | `Comparison` | a program ended or printed otherwise than the directives or the oracle's program |
  | `Timeout` | `llc`, the driver or the linker ran into its time limit |
  | `Crash` | `llc`, the driver or the linker was killed by a signal |
  | `Harness` | the harness could not do its work, the directives do not parse, or the program was linked by another linker |

- `process::run` is the bounded child process of "Child processes".
- `engine::REFERENCE_SELECTOR` is the oracle's selector arguments, and
  `engine::WITNESS` the start of a witness ERROR's detail.

### Seed corpus

`tests/ir/` starts with one hand-written program per construct family of
`spec/ir.md` §5.4 and the module features around it. Each prints through
the C library; its expected output was derived independently of any
compiler and then confirmed with the oracle, on LLVM 22 and 19. Each is
valid for both targets and both LLVM majors, names what it covers in a
`covers` directive, and carries `unsupported: *` until hod compiles it,
then `unsupported(x86_64): *` until the x86-64 code generator does (any reason: which construct hod names first is the support matrix's
business, not the program's; "Test directives").

| file | covers | tier |
|---|---|---|
| `int_arith.ll` | add, sub, mul, and, or, xor at i8 to i64 | quick |
| `int_div.ll` | udiv, sdiv, urem, srem at several widths | |
| `int_shift.ll` | shl, lshr, ashr at several widths | |
| `icmp_br.ll` | the ten icmp predicates; br | quick |
| `loop_phi.ll` | loops, phi | quick |
| `select.ll` | select on integers, pointers, floating point | |
| `switch.ll` | switch, dense and sparse | quick |
| `memory_gep.ll` | load, store, getelementptr | quick |
| `global_reloc.ll` | initialized globals with addresses in them | quick |
| `indirect_call.ll` | calls through pointers and tables | |
| `recursion.ll` | direct and mutual recursion | quick |
| `varargs_caller.ll` | calls to variadic functions, past the argument registers | |
| `varargs_callee.ll` | va_start, va_copy, va_end in a variadic definition | quick |
| `float_arith.ll` | fadd, fsub, fmul, fdiv, frem, fneg | quick |
| `fcmp.ll` | the sixteen fcmp predicates, unordered operands | |
| `casts.ll` | the twelve casts | |
| `aggregate.ll` | insertvalue, extractvalue, aggregates by value | |
| `i128.ll` | 128-bit arithmetic, shifts, compares, division | |
| `alloca_dynamic.ll` | dynamic alloca, over-aligned, stacksave/stackrestore | |
| `mem_intrinsics.ll` | memcpy, memmove, memset | |
| `overflow_intrinsics.ll` | the six `.with.overflow` intrinsics | |
| `atomics.ll` | atomic load/store, atomicrmw, cmpxchg, fence, with threads | |
| `linkonce_comdat.ll` | linkonce_odr, COMDAT groups, weak, extern_weak, aliases | |
| `tls.ll` | thread-local variables in each TLS model, with threads | |
| `global_ctors.ll` | constructors and destructors with priorities | |
| `eh_invoke.ll` | C++ exceptions: invoke, landingpad, resume, rethrow | quick |
| `main_args.ll` | argc and argv, stdin, a non-zero exit status | quick |
| `no_pie.ll` | the static relocation model, a non-PIE link | |
| `noreturn_abort.ll` | a noreturn call, unreachable, death by SIGABRT | |
| `selector_narrow_load.ll` | `spec/interp.md` §10.1: a narrow load GlobalISel miscompiles on AArch64 | |
| `selector_i1_compare.ll` | `spec/interp.md` §10.1: an `i1` compare FastISel miscompiles on AArch64 | |
| `two_modules.ll` | two modules: definitions and declarations across them, internal linkage, linkonce_odr | quick |
| `exit_int.ll` | call-free: integer operations, shifts, compares, casts, select at i8 to i64 | quick |
| `exit_float.ll` | call-free: floating-point arithmetic, conversions, the fcmp predicates with NaN | quick |
| `exit_control.ll` | call-free: loops, phi, switch, indirectbr, local arrays, narrow loads and stores, a pointer table | quick |
| `coro/handles.ll` | `input: pre-opt`: `coro.resume`, `destroy` (call and invoke, one unwinding), `done`, `promise` both ways at alignments 8 and 32, on frames built by hand outside any coroutine | |
| `coro/noop.ll` | `input: pre-opt`: `coro.noop` from two functions, resumed and destroyed (also through invokes), never done | |
| `bit_intrinsics.ll` | ctpop, ctlz, cttz, bswap, bitreverse, fshl, fshr, abs, smin, umax at i8 to i64 | |
| `fp_intrinsics.ll` | sqrt, fabs, floor, ceil, trunc, rint, nearbyint, round, roundeven, minimum, maximum, minnum, maxnum, copysign, fma on double and float | |
| `byval_sret.ll` | `byval` and `sret`: a callee writing its copy, past the argument registers at alignment 16, through a pointer, both in one call, a 3-byte struct | |
| `half_fp128.ll` | scalar `half` (arithmetic, rounding, conversions) and `fp128` (libgcc's soft float, a subnormal, from `i128`) | |
| `vector_arith.ll` | whole-vector integer and floating-point arithmetic at 128 bits, the min/max and saturating intrinsics, `icmp` of vectors | |
| `x86_fp80.ll` | `target: x86_64`: `x86_fp80` arithmetic, conversions, `%Lg` through varargs | |

The three `exit_*.ll` programs make no call, so hod compiled them before
it had calls (WP-043): each checks its results against values derived
independently of any compiler and reports through its exit status alone
(7 when every check passed, `100 + n` for check `n`). They carry
`unsupported(x86_64): *`, since x86-64 has no code generator yet.

The seeds from `bit_intrinsics.ll` to `x86_fp80.ll` (WP-025) were written
for the gaps the cross runs exposed. Their expected output comes from
scripts with exact arithmetic (Python's integers and fractions; NaNs print
as `nan` whatever their sign, since the targets' default NaNs differ),
and each was confirmed with `llc` 19 and 22 for AArch64 natively and for
x86-64 under the emulator, linked by the GNU drivers with libgcc, not
compiler-rt. `x86_fp80.ll` is the one seed for a single target: LLVM has
no `x86_fp80` for AArch64. `indirectbr` is in `exit_control.ll`.

The `va_arg` instruction is not in the corpus, because it has no oracle:
for AArch64 Linux, `llc` (19 and 22) accepts it and reads the `va_list` as
if it were a plain pointer, so the program gets garbage. C and C++
frontends lower `va_arg` themselves for that reason. Variadic callees are
covered through `va_start`, `va_copy`, `va_end` and the C library's
`v*printf`.

### Tests

`xtask/src/irdiff/directives.rs` (the format): `defaults`,
`stdout_and_stdin_lines`, `exit_status`, `list_and_word_directives`,
`conditional_directives`, `errors`, `only_lines_that_start_with_the_marker`,
`documented_examples_round_trip`, `canonical_form`.

`xtask/src/irdiff/process.rs` ("Child processes"):
`captures_output_and_status`, `reports_a_signal`,
`time_limit_kills_the_group`, `never_hangs_on_pipes`,
`limits_are_set_in_the_child`, `memory_limit_fires`, `output_is_capped`,
`harness_errors`.

`xtask/src/irdiff/tools.rs` ("Tools", "What a test does" step 3):
`llvm_default_search`, `plain_llc_must_be_a_wanted_major`,
`llvm_directory_and_errors`, `linkers_and_driver`, `linker_signatures`,
`single_threaded_linkers`, `opt_beside_the_oracle`, `cross_tools` (the
cross driver and `bfd`, the emulator, one reference per target).
`xtask/src/ci.rs` ("In the gate"): `the_gates_irdiff_runs`.
`xtask/src/irdiff/report.rs`:
`exit_status_mapping`, `results_lines`, `results_file_is_replaced`,
`summary_text`. `xtask/src/irdiff/engine.rs`: `differences_are_located`,
`coverage_log_reading`. `xtask/src/irdiff.rs`: `defaults`, `options`,
`usage_errors`, `collecting_tests`.

`xtask/src/irdiff/directives.rs` also: `output_without_a_final_newline`,
`llc_arguments`, `several_modules`, `pre_opt_dialects_and_oracle_skips`.

`xtask/tests/irdiff_fakes.rs` runs the whole command against fake tools
(shell scripts standing in for `llc`, `hod`, `gcc` and the linkers), so
every outcome is provoked on any Linux machine: `the_witness`,
`directives_reach_the_tools`, `several_modules`, `pass_with_every_linker`,
`fail_on_a_difference`, `fail_with_one_linker_only`, `fail_when_hod_fails`,
`unsupported_expected_and_unexpected`, `xfail_and_xpass`,
`oracle_trouble_is_an_error`, `limits_are_in_force_for_programs`, `skips`,
`the_oracle_is_selected`, `cross_target_builds_and_does_not_run`,
`cross_target_runs_under_the_emulator` (the fake emulator requires
`-cpu <cpu> -L <root>`; a hang under it is cut at six times the program
limit), `cross_runs_need_the_emulator_and_the_system_root` (each missing
is a note naming its Debian package, and under `HOD_REQUIRE_TOOLS=1` a
missing required tool), `a_target_hod_declines_wholesale_is_one_note`,
`the_linker_matrix`, `the_wrong_linker_is_detected`, `hybrid_mode`,
`parallel_runs_are_deterministic`, `tiers_and_paths`,
`work_directory_and_keep`, `results_file`, `command_exit_status`,
`command_environment`, `the_fakes_work`, `a_pre_opt_test_goes_through_opt`
(the oracle's and the witness's program lines reach them only through a
fake `opt`, which must get the passes; a failing `opt` and a missing one
are ERRORs; hybrid mode's fallback goes through `opt` too),
`a_dialect_test_runs_only_with_its_llvm`,
`an_oracle_skip_compares_with_the_directives_only` (an oracle and a
witness that would fail are not run; hod's program is compared with the
directives, `xfail` still applies, and another major's mark leaves the
oracle in use).

`xtask/tests/irdiff_real.rs` uses the real tools and skips without them
(fails under `HOD_REQUIRE_TOOLS=1`, which also requires both LLVM 22 and
19): `the_seed_corpus_is_green` (each oracle, every linker),
`the_seed_corpus_passes_in_hybrid_mode`,
`the_seed_corpus_runs_for_the_other_target` (with LLVM 22 and 19, every
admitted test with every linker under the emulator, no ERROR or FAIL; or,
without the emulator, objects only),
`a_real_miscompile_is_detected` (a stand-in hod that runs the real `llc`
on a rewritten module: a wrong result, wrong output, a hang and a missing
`main` are each a FAIL with every linker, and the unmodified module is a
PASS), `limits_hold_for_real_programs`, `the_reference_is_not_plain_o0`
(a hod that is the reference passes both selector reproducers; one that
is plain `llc -O0` fails the narrow load and one that is `llc -O0
-global-isel=0` fails the i1 compare, on AArch64; when an LLVM no longer
miscompiles, the test passes with a note on stderr),
`the_witness_confirms_the_reference_and_catches_plain_o0` (the seed corpus
with the witness, and an oracle without the selector arguments on a test
written from what it prints: an ERROR `witness: ...`),
`llc_args_reach_every_compiler` (the oracle's, the witness's and hod's
objects have a section per function and variable), 
`a_link_that_warns_and_an_output_without_a_newline` (glibc's `tmpnam`
link warning fails `ld.bfd` only under `--fatal-warnings`),
`a_real_program_of_two_modules`, `a_pre_opt_seed_goes_through_opt`
(`coro/noop.ll` passes, and without `input: pre-opt` the real `llc`
cannot build it), `dialects_come_in_pairs_and_oracle_skips_are_listed`.

## Toolchain for tests

The tests that link and run programs (`crates/hod_obj/tests/`, and the
cross runs of "irdiff") find their tools through one description of a
target, `hod_testutil::Target` (`crates/hod_testutil/src/target.rs`), so
that a test for AArch64 and one for x86-64 differ in one value:

| | the host's architecture | the other one |
|---|---|---|
| C and C++ drivers | `gcc`, `g++` | `<prefix>-gcc`, `<prefix>-g++` |
| GNU ld | `ld.bfd` | `<prefix>-ld.bfd` |
| system root | - | `/usr/<prefix>` |
| runner | the program itself | `qemu-<arch> -cpu <cpu> -L /usr/<prefix>` ("Cross runs") |

`Target::tool(what)` finds one of them, or prints a note naming its
Debian package (`gcc-x86-64-linux-gnu`, `g++-x86-64-linux-gnu`,
`binutils-x86-64-linux-gnu`, `libc6-dev-amd64-cross`,
`libstdc++-14-dev-amd64-cross`, `qemu-user`, and their AArch64
counterparts) and the test skips; under `HOD_REQUIRE_TOOLS=1` it panics.
Nothing is ever installed by a test.

`crates/hod_obj/tests/common/linking.rs`'s `Toolchain` holds a target, its
drivers, its linkers and its runner: `Toolchain::host` is the host's
(unchanged from before targets: `gcc`, the linkers as they are, the
program run directly), `Toolchain::for_target` any target's. A linker is
selected with `-B<dir>` holding it as `ld.<name>` and `-fuse-ld=<name>`
(`spec/object.md` "Toolchains"); lld is selected per major
(`Toolchain::lld(scratch, 19 or 22)`, since only `ld.lld-19` and
`ld.lld-22` are installed). An emulated program has a five-minute limit.

What this machine does (WP-025): with the cross gcc 14, bfd, lld 19, lld
22 and mold each link C and C++ programs for x86-64 as PIE, non-PIE,
static and static PIE, with C++ exceptions, `thread_local` and
`std::thread`, and the programs run under the emulator; so do objects
from `llc` 19 and 22.

Tests: `crates/hod_testutil/src/target.rs` `names_of_a_targets_tools`;
`crates/hod_obj/tests/link_cross.rs` (the other architecture, objects of
the cross gcc and of `llc` only, no hod code generation):
`a_c_program_runs_with_every_linker_and_mode`,
`cxx_exceptions_and_thread_locals_run`, `llc_objects_run`,
`the_emulated_cpu_is_the_baseline` (the emulated x86-64 CPU reports SSE2
and not SSE3, CMPXCHG16B or POPCNT, and `popcnt` dies with SIGILL).

## Jobserver

Every agent on a machine shares one CPU and memory budget, so the harness
is a GNU make **jobserver client**, like the cargo and rustc it runs
alongside. The client is ported from kiln's (`xtask/src/jobs.rs` there);
"Differences from kiln" lists what hod changed.

- **The pool.** `scripts/jobserver.sh` owns it: `start` makes a fifo
  (`$HOD_JOBSERVER_FIFO`, else `/run/hod/jobserver.fifo`) holding one byte
  per token (`$HOD_JOBS`, else 12), a holder process that keeps it open,
  and a loop that runs `refill` every 60 s. `scripts/env.sh` exports
  `MAKEFLAGS=--jobserver-auth=fifo:<path>` while that fifo exists (and
  `HOD_NO_JOBSERVER` is unset); otherwise it caps `CARGO_BUILD_JOBS`
  and there is no pool.
- **What is honoured.** `MAKEFLAGS` containing `--jobserver-auth=fifo:<path>`
  (make 4.4's fifo style; the last `--jobserver-auth=` word wins), read
  once per process. The fifo is opened read+write and stays open until the
  process ends; a token is one byte read from it, and the same byte is
  written back on release.
- **The clients.** One implementation, `hod_testutil::jobs` (`Client`,
  `Token`, `global()`), for the two kinds of process that start children:
  - a **test binary**: every child started through `hod_testutil::run_with`
    (and so `run` and `write_script`) holds one token for its whole
    lifetime;
  - **xtask**: every child started through `irdiff::process::run` holds one
    token for its whole lifetime: each oracle, hod, linker-driver and
    program process of `cargo xtask irdiff` (and of the `irdiff` step of
    `cargo xtask ci`), the `llc --version` probes, and the `llvm-config
    --version` of `cargo xtask shim`.

  A time limit starts once the token is held: waiting for the pool is never
  a timeout. The `cargo` steps (`fmt`, `clippy`, `test` of `cargo xtask
  ci`; the `cargo build` of the workspace's hod for `irdiff`) hold no token
  of xtask's: cargo is a client itself, and so are the test binaries it
  runs. The `/bin/sh` that delivers a signal to a process group is not
  counted. What a child starts itself (hod's fallback `llc`, a linker
  under its driver) runs on the child's token.
- **The implicit token.** As with make, each client process owns one token
  that is not in the pool: one child may run without reading the fifo;
  each further concurrent child needs a pool byte. So N concurrent clients
  on a pool of P tokens run at most P + N children. (A client that is
  itself the child of a client runs on the token its parent holds for it,
  so the bound holds for a tree of them.)
- **`-j`** and the test threads stay what they are: `irdiff -j N` has N
  worker threads and a test binary has `RUST_TEST_THREADS` test threads, so
  they are an upper bound on in-flight children; the pool can only lower
  the effective parallelism. A worker waits for a token in `acquire`, and
  takes the implicit token if that becomes free first.
- **Release.** Tokens are released by `Drop`: on a normal exit, an error
  return, a panic of the thread, and a child killed by a limit.
- **What the pool's refill sees.** Tokens get lost (a client killed with
  SIGKILL takes its tokens with it), so the pool script's `refill` tops the
  pool up to its size less the tokens in use, which it must never
  under-count. It counts as in use, besides the compilers it knows by
  name, **one token per child process of every process that has the fifo
  open**. The client fits that accounting, and must keep doing so:
  - it keeps the fifo open from its first use to the end of the process;
  - it holds a pool token only for a child process of its own, from just
    before the child is started until just after it is reaped and its
    output is read, and one token per child at most;
  - a byte read for a waiting thread is handed to that thread or written
    straight back; nothing is kept in reserve.

  So the tokens a client holds never exceed its live children for longer
  than a spawn or a reap takes, and `refill` adds a token only for a
  deficit that two passes a minute apart both saw. (A test's child that
  leaves a descendant holding its output keeps `run_with`, and its token,
  waiting without a child; that is a defect of the test, not of the
  accounting.) `scripts/jobserver.sh refill` run twice while a client
  holds tokens for sleeping children adds nothing.

  A pass looks at the pool in one process, in milliseconds: it counts the
  idle tokens, finds the clients and the tokens they hold, and counts the
  idle tokens again, keeping the larger count. A client whose tokens go
  back while the pass looks is then counted idle, not missed by both
  counts. (The pass was a shell scan of `/proc`, one `readlink` per
  descriptor of the machine, which under load took seconds; a client that
  ended in that time had its tokens seen neither idle nor held, and the
  pass, finding no client, refilled them: minted. That is how the shared
  pool could go above its size.) A process whose descriptors cannot be
  read counts as a client. A deficit is refilled only when two
  consecutive passes saw it, by the smaller of the two, with or without
  clients: a pool that lost tokens while nothing ran is whole again two
  passes later.
- **The shared pool's guard.** `scripts/jobserver.sh refill`, `resize`
  and `stop` refuse the shared pool, the default fifo
  (`/run/hod/jobserver.fifo`), with exit status 3 and a line on standard
  error that says why and how to name a private pool, unless the caller
  confirms with `--shared` (any position after the verb) or
  `HOD_JOBSERVER_SHARED=1`. The refill loop the script starts (the
  process whose pid is in `<pool>.refill.pid`) needs no confirmation, so
  a loop that is running when the script changes keeps working. `start`
  and `status` are not guarded. (A check meant for a private pool, whose
  `HOD_JOBSERVER_FIFO` was lost on the way, once stopped the shared pool
  under every build of the machine.) A private pool is named on the
  command itself: `HOD_JOBSERVER_FIFO=<path> scripts/jobserver.sh <verb>`.
  `HOD_JOBSERVER_SHARED_FIFO` moves what the script takes for the shared
  pool; only its tests set it, to a path of their own, so that no command
  of theirs can reach the machine's pool even without
  `HOD_JOBSERVER_FIFO`.
- **No pool.** `scripts/env.sh` without a fifo exports
  `CARGO_BUILD_JOBS=8` (unless set) and says so once on standard error
  (`env.sh: no jobserver pool at <fifo>: ...`; once per environment: a
  second `source` in it is quiet), unless `HOD_NO_JOBSERVER` is set, which
  asks for no pool. Nothing parses the standard error of `env.sh`.
- **Signals.** xtask catches **SIGINT and SIGTERM** on a dedicated thread
  (the `signal-hook` crate, a dependency of `xtask` alone), with or without
  a pool. The thread starts with the first child or cargo step. The
  handler's `unsafe` stays inside that dependency, so xtask remains
  `forbid(unsafe_code)`. On either signal:
  1. **The children die first.** Every child started through
     `irdiff::process::run` is in a table from spawn to reap
     (`xtask::jobs::children`), and each is the leader of its own process
     group.
     - Each live group is killed with `SIGKILL`: the limit wrapper, the
       tool, and what the tool started all go together.
     - The `cargo` steps go through `jobs::children::run_step`, which puts
       them in the same table when the signal thread is running. Their
       group gets `SIGTERM` first, so cargo can stop its rustc and test
       children, and `SIGKILL` 3 s later if it is still alive. Cargo is a
       jobserver client of its own and holds no xtask token, so tokens
       that cargo or a test binary still held when it died are lost until
       the pool is refilled.
     - The thread then waits until each child is reaped and no spawn is in
       flight. It waits at most 10 s, for a child in uninterruptible sleep.
     - A child whose spawn was in flight is killed as it registers.
     - From then on nothing is started: `process::run` and `run_step` do
       not return to their callers any more (their threads park), so a
       child the interrupt killed is nobody's result, no results file is
       written, and nothing races the signal thread to the exit.
     - stderr says `xtask: signal N: killed K child process(es)`.
  2. Every pool byte the process still holds (by live tokens, or already
     read for waiting workers) is written back to the fifo. This happens
     only after step 1, so an orphan never runs on a returned token. Tokens
     whose children were reaped in step 1 went back through their normal
     `Drop`. The helper thread may still be in a read that nobody waits
     for any more; it takes the next byte that reaches the fifo, as likely
     as not one of those just written, and writes it straight back. The
     signal thread waits until that read is done (at most 200 ms: with an
     empty pool the read gets nothing), so that the process does not end
     with a byte in the helper's hands; no other read is started.
  3. Later token drops return nothing, and new acquisitions are free. Each
     token is therefore returned exactly once.
  4. The signal's default action ends the process (falling back to exit
     status 128 + signal). stderr says `xtask: signal N: returned K
     jobserver token(s)` when K is not 0.

  The memory and time limits are unchanged: they kill one child's group
  and leave the table to its normal reap.
- **A test binary handles no signal.** `hod_testutil` has no dependency to
  do it with, and libtest owns the process. A test binary that is killed
  loses the pool tokens it held, and its children live on until they end
  or their limits stop them. (The test binaries that call `xtask` as a
  library, its own and `hod_cli`'s, get xtask's signal thread with their
  first `process::run` and behave as xtask does.)
- **What a normal exit can lose.** A helper thread left in a read that
  nobody waits for takes a byte that reaches the fifo and writes it back
  at once. A process that ends between those two steps takes the byte with
  it. Nothing in a process can close that window for an exit it does not
  control; the pool script's refill covers it.
- **SIGKILL cannot be handled.** No process can catch it, so `kill -9`
  (including the kernel's OOM killer), or any other fatal signal a client
  does not catch, loses the tokens the client held, as it would for make
  and cargo. The pool script's refill loop restores them within two
  minutes; `cargo xtask jobserver refill` does it at once.
- **`cargo xtask jobserver status|refill [--fifo <path>] [--jobs <n>]`.**
  - The fifo is `MAKEFLAGS`' `fifo:` path, else `$HOD_JOBSERVER_FIFO`,
    else `/run/hod/jobserver.fifo`.
  - `<n>` is `$HOD_JOBS`, else the number `scripts/jobserver.sh` recorded
    beside the fifo (`jobserver.jobs` for `jobserver.fifo`: the fifo's
    path with `.fifo` replaced by `.jobs`), else 12.
  - Both open the fifo read+write with `O_NONBLOCK`. They count the idle
    tokens by reading until `EWOULDBLOCK` and write the same bytes back.
    `status` prints `jobserver <path>: I idle token(s) of <n>`.
  - `refill` also writes `+` bytes up to `<n>` idle tokens. It is only
    correct while no client holds tokens (nothing building or testing):
    tokens in use come back on top of the refill. It leaves a pool above
    `<n>` alone and reports it. (`scripts/jobserver.sh refill` is the one
    that is safe while clients run.)
  - Neither ever blocks, whatever the state of the pool: a fifo nobody
    holds open has no idle token. The non-blocking open uses a hard-coded
    `O_NONBLOCK` per OS (Linux, Android, macOS and the BSDs); on other
    platforms both fail with an error.
  - A usage error, or a path that is not a fifo, is exit status 2.
- **Fallback.** With no `MAKEFLAGS`, the descriptor forms
  (`--jobserver-auth=R,W`, `--jobserver-fds=R,W`, which need inherited
  descriptors), a path that is not a fifo or cannot be opened (one warning
  per process, `jobserver: warning: cannot use jobserver fifo <path>
  (<why>); running without it`), or a later fifo I/O error (a warning;
  acquisition becomes free), `-j` and the test threads alone bound the
  parallelism, exactly as without a jobserver. A fifo that has no token
  and never gets one (its holder is gone) is still a pool: a client runs
  its children one at a time, on its implicit token, and does not hang.
- **The run header** of `irdiff` names the pool after the jobs: `-j N
  (jobserver fifo:<path>)`; without a pool it is `-j N` as before.

### Differences from kiln

- The client is in `hod_testutil` (a crate of test support that is never
  shipped, and has no dependency), not in `xtask`, because hod's tests
  start most of their oracle processes from the test binaries, which kiln's
  do not. `xtask` depends on `hod_testutil` for it and adds the signal
  thread, the table of children and the command (`xtask/src/jobs.rs`,
  `xtask/src/jobs/children.rs`).
- Test binaries are clients, without signal handling.
- `process::run` does not return once xtask is interrupted (kiln's returns
  `Interrupted` to a runner that is about to be ended anyway); see
  "Signals".
- `release_all` waits for the helper's read in flight ("Signals", step 2);
  kiln's returns at once, and an interrupted kiln harness can end with one
  byte in its helper's hands.
- Signals are delivered to a process group by `/bin/sh -c 'kill ...'`,
  whatever `PATH` is, as "irdiff", "Child processes" already did.
- The pool size defaults to 12 and is read from the file the pool script
  writes; the fifo defaults to `/run/hod/jobserver.fifo`.
- hod has no measured runs yet (`bench`): when it does, their clock too
  starts once the token is held.

### Tests

`crates/hod_testutil/src/jobs.rs` (the client, each over a pool of the
test's own): `parses_makeflags`, `no_jobserver_is_free`,
`rejects_a_non_fifo`, `bounds_concurrency_by_pool_plus_implicit`,
`implicit_token_frees_a_waiter`,
`release_all_returns_held_and_spare_bytes`,
`release_all_waits_for_the_helpers_read`,
`refill_tops_up_without_blocking`,
`a_stale_fifo_is_counted_without_blocking`, `tokens_return_on_panic`,
`a_broken_pool_is_free`. `crates/hod_testutil/src/lib.rs`:
`run_holds_a_token_for_the_childs_lifetime` (and returns it when the
child is killed at its time limit),
`the_time_limit_starts_once_the_token_is_held`.

`crates/hod_testutil/tests/jobserver.rs` runs the test binary itself as a
client of a private pool: `a_test_binary_is_bounded_by_the_pool` (8
threads on 2 tokens: never more than 3 children alive, measured by the
children), `without_a_usable_pool_nothing_waits` (each fallback, and the
one warning), `a_pool_without_tokens_serializes_and_does_not_hang`,
`the_pool_script_does_not_mint_tokens_for_this_client`
(`scripts/jobserver.sh start`, `refill` twice while the client's children
sleep, `stop`, on a pool of its own; each pass is judged only while the
client lives, and the held children's limit is ten minutes, a hang guard),
`the_pool_script_refuses_the_shared_pool_unless_told` (the guard, with
the test's pool made the "shared" one, with and without
`HOD_JOBSERVER_FIFO`). `xtask/tests/jobserver.rs`:
`env_sh_names_the_pool_while_its_fifo_exists` (and the one line without
it).

**Time in these tests.** No verdict depends on how busy the machine is.
`a_stale_fifo_is_counted_without_blocking` judges the count once no
process has the abandoned fifo open (a child forked by another test
thread keeps its bytes alive until it calls `exec`, which under load has
outlasted any fixed wait); `release_all_waits_for_the_helpers_read`
checks that the wait ended with the helper's read, under a grace that is
a hang guard (`release_all` itself keeps its 200 ms);
`the_time_limit_starts_once_the_token_is_held` gives `true` 3 s.
`xtask/tests/bench_fakes.rs` counts a time only when it differs by 1 s
and makes a slow run sleep 1.5 s (a run of fakes is assumed under 0.5 s
at load 32); `xtask/tests/irdiff_fakes.rs` makes a hang sleep 600 s and
bounds a run with hangs at 4 minutes (the details prove each hang was
killed at its 3 s limit; the bound says nothing waited for one, and
waits for the shared pool are in it).

`xtask/src/jobs.rs`: `jobserver_cli`, `jobserver_cli_defaults`.
`xtask/src/jobs/children.rs` ("Signals", step 1):
`shutdown_kills_and_reaps_live_children`,
`run_step_is_registered_and_terminated`,
`run_step_ignoring_term_is_killed`, `run_step_without_table`,
`run_step_runs_a_command`.

`xtask/tests/jobserver.rs` runs the command: `irdiff_is_bounded_by_the_pool`
(`-j 8` over fake tools on 2 tokens: at most 3 children, the header, the
pool left full), `irdiff_without_a_usable_pool`,
`an_interrupted_irdiff_kills_its_children_and_returns_its_tokens` (SIGTERM
and SIGINT: the children are gone, the pool has all its tokens, xtask ended
by the signal, no results file), `the_jobserver_command` (the defaults from
the environment; a fifo nobody holds does not block it),
`env_sh_names_the_pool_while_its_fifo_exists`.

Every one of these makes its fifo in the system's temporary directory
(`hod_testutil::jobs::PrivatePool`, `LocalDir`), not in the build
directory: a fifo is unreliable on a bind mount. None reads or writes the
machine's pool, and all pass with and without one.

## Test directives

A test of `irdiff` is an LLVM IR file. Its directives are comments, so the
file is still plain IR for every LLVM tool.

**A line that starts with `;@` is a directive, and must be one.** Nothing
else is: not a `;@` after white space or after code. A directive is

```
;@ <key>: <value>
;@ <key>(<condition>,<condition>...): <value>
```

exactly: `;@`, one space, the key (lower-case letters and `-`), for
`xfail` and `unsupported` optionally a parenthesised condition list
without spaces (for `oracle-skip` a required `(llvm=<majors>)`), a colon, and the value. One space after the colon, if
there is one, is not part of the value. A file with no directive is a test
of a program that prints nothing and exits with status 0, valid
everywhere.

| key | value | times | default |
|---|---|---|---|
| `stdout` | one line of the expected standard output | any | no output |
| `stdout-no-newline` | the end of the expected standard output after its last newline, verbatim; no `stdout` after it | once | the output ends with a newline |
| `stdin` | one line of the program's standard input | any | no input |
| `exit` | `N` (0 to 255): the exit status. `signal N` (1 to 64): terminated by that signal | once | `0` |
| `args` | the program's arguments, separated by spaces or tabs | once | none |
| `target` | the architectures the test is valid for: `aarch64`, `x86_64` | once | both |
| `llvm` | the LLVM majors the test is valid for: `N` (exactly) or `N+` (that or later); any one must hold | once | all |
| `dialect` | `N`: the test is in LLVM `N`'s dialect and runs only with that oracle; excludes `llvm` ("Pre-opt input, dialects and oracle skips") | once | every LLVM |
| `input` | `pre-opt`: IR before optimisation; the oracle and the witness run `opt -passes=default<O0>` first | once | IR `llc` builds as it is |
| `oracle-skip` | `oracle-skip(llvm=N[,M...]): <reason>`: with those oracles, hod's program is compared with the directives only; the reason is free text, never empty, and the test is listed in "Oracle skips" | any | the oracle is used |
| `link` | what the link needs: `m` (the math library), `c++` (the C++ runtime), `no-pie` (a position-dependent program, compiled with the static relocation model), `pie` (the default, spelled out), `allow-warnings` (link without `-Wl,--fatal-warnings`) | once | the C library, PIE, warnings fatal |
| `llc-args` | code-generation arguments the oracle, the witness and hod all get, from the allow-list below | once | none |
| `module` | starts the next module of a test of several, and names it ("Several modules") | any | one module |
| `tier` | `quick` or `full` | once | `full` |
| `covers` | the spec section the test exercises, free text | once | none |
| `unsupported` | the coverage-log reason hod declines this test with (lower-case letters, digits, `-`, `:`, `.`, `_`: the names of `spec/cli.md` "Reasons", `global:function-definition`, `intrinsic:llvm.memcpy`), or `*`: hod declines, with any reason | any | hod must compile it |
| `xfail` | why hod is known to fail this test, free text; never empty | any | it must pass |

- `stdout` and `stdin` lines are taken verbatim, in order, each followed
  by a newline. The whole expected output is their concatenation, then
  the `stdout-no-newline` value if there is one (taken verbatim, white
  space too); the program's output must equal it byte for byte. Bytes that
  are not valid UTF-8 cannot be written.
- `llc-args` is a list of words, each one of: `-code-model=<v>`,
  `-data-sections`, `-debugger-tune=<v>`, `-fp-contract=<v>`,
  `-function-sections`, `-mcpu=<v>`, `-x86-branches-within-32B-boundaries`,
  where `<v>` is letters, digits, `.`, `_`, `+` and `-`. These are what a
  compiler driver gives `llc` that change the code and name no file. The
  optimization level, `-filetype`, `-mtriple`, `-relocation-model`, `-o`
  and the selector are the harness's to give.
- List values (`args`, `target`, `llvm`, `link`) are words separated by
  spaces or tabs. An argument cannot contain white space.
- For the other keys, white space around the value is dropped.

**`unsupported: *` or a named reason.** `*` says "hod declines this
test; which construct it names first is not this test's business": the
test is UNSUPPORTED whatever reason hod declines or falls back with, and
still FAILs with "remove its 'unsupported' directive" once hod compiles
it. Behavioral tests — every program under `tests/ir`, the seed corpus —
use `*`: the first refusal follows the support matrix
(`crates/hod_ir/src/classify/matrix.rs`), which every code-generation
package moves, and a named reason would have to be edited at every such
merge. A named reason is for a test *about* the reason (that hod refuses
this construct with this name), and should name a construct no package
is about to turn on (refused by design, unplanned, or a property of the
command line).

**Conditions** narrow an `unsupported` or `xfail` directive to part of the
matrix. A condition is a target (`aarch64`, `x86_64`), a linker (`bfd`,
`lld`, `mold`, `wild`) or an oracle major (`llvm19`, `llvm22`, ...). All
conditions of a directive must hold, and there is at most one of each
kind. Several directives of the same key are alternatives; the first one
in the file whose conditions hold is the one that applies. `unsupported`
cannot depend on a linker, because hod declines before anything is linked.

**Several modules.** A `module` directive's value is a name of letters,
digits, `.`, `_` and `-` that starts with a letter or a digit. A file with
`module` directives is a test of several modules: each module is the
text from its `module` line to the next one or the end of the file, and
before the first one there are only directives, comments and empty lines.
Directives may stand anywhere, and are the whole test's. Each module is
compiled on its own, by each compiler, as if it were a file of one module
(its lines before the module are empty, so that diagnostics give the
file's line numbers); the objects are linked in the order of the file into
one program, which is run once per linker. The first module hod does not
build (declines, falls back on, or fails on) decides the test, with the
module's name in the detail (`module 'lib': ...`).

A test hod builds on AArch64 and declines on x86-64, with input,
arguments and a status (the directives of `tests/ir/main_args.ll`):

```llvm
;@ covers: spec/ir.md §5.4 call, memory (the process interface every other test relies on)
;@ tier: quick
;@ args: alpha beta
;@ stdin: one line of input
;@ exit: 3
;@ unsupported(x86_64): *
;@ stdout: argc=3
;@ stdout: alpha beta
;@ stdout:
;@ stdout: one line of input
```

A test for one target and newer oracles, that needs the C++ runtime and
the math library in a position-dependent program, that hod compiles on
AArch64 with LLVM 22 but not with LLVM 21, that is known to fail with one
linker, and whose program ends by `abort`:

```llvm
;@ target: aarch64
;@ llvm: 20+
;@ link: c++ m no-pie
;@ unsupported(llvm21): codegen-not-implemented
;@ xfail(mold): the reason says what fails and where it is tracked
;@ exit: signal 6
;@ stdout: about to abort
```

### Errors

A test with a directive error is an ERROR and is not run. Every error of
a file is reported, with its line:

- a `;@` line that is not `;@ <key>...` (no space, an upper-case or empty
  key), has no `:` directly after the key or the condition list, or has an
  unclosed condition list;
- an unknown key;
- a condition list on a key other than `unsupported`, `xfail` and
  `oracle-skip`;
- a second directive of a key that may appear once;
- an empty value, for every key but `stdout` and `stdin`;
- `exit` that is not `N` in 0 to 255 or `signal N` in 1 to 64;
- an unknown word in `target`, `link` or `tier`, or the same word twice;
  `link` with both `pie` and `no-pie`;
- a `stdout` after `stdout-no-newline`;
- an `llc-args` word not on the allow-list (one the harness gives itself
  is reported as the harness's), or one option twice;
- a `module` name that is not valid or is given twice, or code before the
  first `module` directive;
- an `llvm` item that is not `N` or `N+` with `N` a number without a
  leading zero;
- an `unsupported` reason with other characters than lower-case letters,
  digits, `-`, `:`, `.` and `_` (other than `*` alone);
- an unknown condition, an empty one, a second condition of the same
  kind, or a linker condition on `unsupported`;
- `input` other than `pre-opt`; `dialect` that is not a major; both
  `dialect` and `llvm`;
- `oracle-skip` without `(llvm=N[,M...])`, with an item that is not a
  major or a major twice, or without a reason.

### Canonical form

`Expectations::to_directives` writes directives in this order, leaving
out everything that is the default: `covers`, `tier`, `target`, `llvm`,
`dialect`, `input`, `link` (libraries, then `no-pie`, then
`allow-warnings`), `llc-args`, each `oracle-skip`, each `unsupported`, each `xfail`, `args`, each `stdin`, `exit`, each `stdout`,
`stdout-no-newline`. `module` directives belong to the modules and are
not written. Reading what it writes gives
the same `Expectations` back. Files need not be in this order.

## corpus

```
cargo xtask corpus capture [--source kiln|gcc-torture|all] [--llvm <major>]
                           [--out <dir>] [--kiln <kiln++>] [--kiln-repo <dir>]
                           [--torture <dir>] [--limit <N>] [-j <N>]
                           [--force] [--retry-skipped]
cargo xtask corpus verify  [-j <N>] [--hod <binary>] [--strict]
                           [--manifest-only] [--keep] [-v] [<corpus dir>]
cargo xtask corpus census  [--report <file>] [<corpus dir>]
cargo xtask corpus fetch   [--torture <dir>]
```

The corpus is the IR hod really has to compile: the modules its front
ends give their backend, each kept as an `irdiff` test that says what the
program built from it does. `capture` makes it, `verify` proves it,
`census` counts what is in it, `fetch` gets the one suite that is not on
the machine already. The corpus is build output: it is not in the
repository, and nothing in the gate needs it, kiln, or the suite.

A **source** is a producer with its suite: `kiln` (kiln++ over kiln's own
executing tests) and `gcc-torture` (clang over GCC's `gcc.c-torture/execute`).
A **group** is a source with an LLVM major (22, 19): the major whose `llc`,
`clang` and installation produced and built the group's modules. Groups
share nothing.

### Where things are

| what | default | set by |
|---|---|---|
| the corpus | `<build root>/corpus` | `--out`, or the directory argument |
| kiln++ | `<build root>/target-kiln/release/kiln++` | `--kiln`, `$HOD_KILN` |
| kiln's repository | `<build root>/../kiln` | `--kiln-repo`, `$HOD_KILN_REPO` |
| the GCC checkout | `<build root>/../.toolchain/gcc-tests` | `--torture`, `$HOD_GCC_TESTS` |
| the work directory | `<corpus>.work`, beside the corpus | |

`<build root>` is the build directory (`CARGO_TARGET_DIR`, else `target/`
of the workspace), or its parent when the build directory is named
`target-<something>` (one of several per-checkout directories,
`scripts/env.sh`). LLVM, the linker driver and the linkers are found as
for `irdiff` ("Tools"); `clang` for a major is `clang-<N>` on `PATH`, else
`/usr/lib/llvm-<N>/bin/clang`.

A source that is not there (no kiln++, no kiln repository, no suite) is a
note and is skipped, also under `HOD_REQUIRE_TOOLS=1`: they are not tools
of the gate. A missing oracle, `clang`, linker driver or required linker
is a note, and under `HOD_REQUIRE_TOOLS=1` a failure (exit status 3) after
everything that can be captured has been; `wild` is optional, as for
`irdiff`. Nothing in kiln's repository or the GCC checkout is written.

### capture

| option | meaning |
|---|---|
| `--source <name>` | `kiln`, `gcc-torture` or `all`; repeatable. Default: all. |
| `--llvm <major>` | 22 or 19; repeatable. Default: each one installed, newest first. |
| `--out <dir>` | The corpus directory. |
| `--kiln`, `--kiln-repo`, `--torture` | "Where things are". |
| `--limit <N>` | Only the first N tests, in name order, of each group. |
| `-j <N>`, `--jobs <N>` | Tests captured at once, and `irdiff`'s jobs in admission. Default 3, at most 16. |
| `--force` | Capture again what is already in the corpus. |
| `--retry-skipped` | Capture again what the last capture skipped. |

The default is every source with every installed LLVM major and no
limit: on the reference machine that is 6250 entries in 191 MiB, made in
about 37 minutes at `-j 3` (admission runs the witness) (`docs/status/corpus.md` has the numbers
of the last capture). `--limit` is the bounded, deterministic selection
for a quick look.

For each group, in the order kiln before gcc-torture and newer LLVM
first, a capture:

1. lists the suite's tests and decides, from each test alone, whether it
   can be an entry ("Skip reasons");
2. leaves alone the tests it has already dealt with ("Incremental");
3. builds and runs the others, at most `-j` at once, each in a directory
   of its own under the work directory and every child process within
   `irdiff`'s kind of limits ("Child processes"); what passes is a
   **candidate**, staged outside the corpus;
4. runs the candidates through admission ("Admission");
5. moves the admitted entries into the corpus and writes the three
   tables ("Manifest"), each by writing a new file and renaming it.

A capture interrupted before step 5 leaves the group as it was. Step 5
only writes, entries before tables; interrupted there, it can leave
entry files the manifest does not name, which `verify` reports and the
next capture makes again.

A progress line is printed every 200 tests; at the end, per group, one
line `corpus: <group>: N tests: N captured, N reused, N skipped, N
removed; <seconds>` and one line per skip reason with its count, then the
corpus's entries and size.

Exit status: 0 also when tests were skipped (a skip is a result, not an
error); 3 when a required tool is missing under `HOD_REQUIRE_TOOLS=1`,
or the corpus cannot be read or written; 2 for a usage error.

### The kiln source

The tests are the files with `//@ expect: run` in their header under
`tests/c`, `tests/conformance` and `tests/stress` of kiln's repository
(`.c`, `.cpp`, and headers that carry a header), in name order. A test's
name is its path from the repository's root.

Each is built the way kiln's own harness builds it, by one command, run in
a directory that mirrors the repository's root (links to `tests` and
`resource`), so that the paths in the module are relative and the same on
every machine:

```
kiln++ --llvm-dir=<capture directory> [-x c | -x c++] -std=<std> \
       -fno-crash-diagnostics -fclang-compat=<major> <the header's args> \
       -fdebug-prefix-map=<cwd>=. <test> -o <program>
```

- kiln++ is run **under the name it is given**. `kiln++` is a link to
  `kiln`, and the name is what selects the C++ driver; resolved to `kiln`
  it links C++ programs without the C++ library.
- `<std>` is the header's `std:`, else `c17` for a `.c` test and `c++26`.
  In the arguments `%{test_dir}` is the test's directory relative to the
  root, `%{repo_root}` is `.`, `%{resource_dir}` is `resource`.
- The **capture directory** is a shim ("shim") of the group's LLVM
  installation whose `bin/llc` is a script instead of hod: it saves its
  arguments and its standard input, then runs the real `llc` on them with
  the oracle's selector (`-global-isel=0 -fast-isel=0`, "irdiff", "The
  oracle") after kiln's arguments. So the module kept is byte for byte
  what kiln gave its backend, and the program whose behaviour becomes the
  entry's expectations is the one the reference compiler makes.
  `TMPDIR` and `XDG_CACHE_HOME` point into the work directory; `LC_ALL=C`.
- The program is then run in an empty directory with the header's
  `run-args:` and `run-stdin:` (or `run-stdin-file:`), within the run
  limits (10 s, 1 GiB; the compile has 120 s and 8 GiB, as in kiln's
  harness). It must exit with the header's `exit:` (default 0) and, if the
  header has `stdout:` lines or a `stdout-file:`, write exactly that.

The entry's expectations are what the program **did**: its exit status and
its standard output. Its link is `link: c++ m`, with `no-pie` when llc was
given `-relocation-model=static`.

**Bitcode.** When kiln ran `opt` first (sanitizers, coverage, `-O1` and
up, coroutines, `always_inline`), `llc` is given bitcode. The capture
writes it as text with the group's `llvm-dis` (`llvm-dis-<N>`, found as
`clang` is) and the entry holds that text, with `; delivered: bitcode`. A
failing `llvm-dis` is `llvm-dis-fails`; without `llvm-dis` the test is
skipped with its `opt-first-*` reason. `-flto` is still `opt-first-lto`:
kiln gives `llc` nothing.

**Several translation units.** A test with `module:` or `unit:` lines in
its header, or `two-step`, is built in separate steps as kiln's own
harness builds it (kiln `xtask/src/test/runner.rs`, `build_two_step`):
each `module:` unit, then the test, then each `unit:` file compiled with
its own command (a unit in the other language at its default standard),
then one link of the objects, the modules first, then the units, the test
last. Each `llc` run is a module of the entry, named after its unit (`m.part`
for `m:part`, `unit0`, `unit1`, ..., `test`). A one-command build of
several inputs is captured when kiln's object names (`<n>-<stem>.o`,
kiln `crates/kiln_driver/src/codegen_cmd.rs`) give the link order; each
module is named after its object's stem.
Still `multi-file`: `pch:` tests, modules that were given different `llc`
arguments, two modules that would have one name, and a link order that is
not known.

### The gcc-torture source

The tests are the `.c` files of `gcc.c-torture/execute`, of its `ieee/`
and of its `builtins/`, in name order. A `builtins/` test `x.c` is built
with `builtins/x-lib.c` and `builtins/lib/main.c` (`builtins/builtins.exp`),
three modules named `test`, `lib` and `main`, linked in that order; one
without those files is `multi-file`. Each file is compiled in the suite's
directory:

```
clang-<N> -O0 -S -emit-llvm -w <flags> -o <module> <file>
llc-<N> -O0 -relocation-model=pic -filetype=obj -global-isel=0 -fast-isel=0 -o <object> <module>
gcc -pie -o <program> <objects> -lm
```

`<flags>` are the test's `dg-options` and `dg-additional-options` (those
without a target selector, or whose selector does not name the host's
architecture; one that does is `special-flags`) and the options of its
`.x` file, translated: language, warning, `-f[no-]` code-generation and
`-D`/`-U`/`-I` options clang knows are kept; GCC-only tuning options
(`-ftree-*`, `-fno-ipa-*`, `--param`, `-fdump-*`, ...) are dropped;
`-fpermissive` becomes `-Wno-error=` for implicit function declarations,
implicit `int`, and `int` and pointer conversions; `-Wl,`, `-Wa,`, `-Wp,`
and anything unknown make the test `special-flags`. Tests of `ieee/` get
`-fno-inline`, as in the suite's own driver. The program is run without
arguments and must exit with status 0: the suite's tests report failure
by `abort` or a non-zero status. The entry's link is `link: m`.

### Entries

An entry is one file:

```
;@ covers: <source> <test>
;@ target: <host architecture>
;@ llvm: <major>
;@ link: [c++] m [no-pie] [allow-warnings]
;@ llc-args: ...        (if the producer gave llc arguments on the allow-list)
;@ args: ...            (if any)
;@ stdin: ...           (one per line of input)
;@ exit: <status>       (if not 0)
;@ stdout: ...          (one per line of output)
;@ stdout-no-newline: . (the output after its last newline, if any)
; corpus entry written by `cargo xtask corpus capture`; do not edit
; source: <source> <test>
; producer-args: <the producer's arguments, the machine's directory as <cwd>>
; delivered: <text or bitcode, one word per module>
; llc-args: <what the producer's driver gave llc, without -o <object> and the input>
<the module, unchanged>
```

An entry of several modules has, in place of the one module, each module
after its `;@ module: <name>` line, in the order of the link; a module
that does not end with a newline gets one.

The directives are in canonical form ("Test directives"). `covers` names
the test the entry was captured from, not a part of a specification. An
entry is for the architecture it was captured on and for one LLVM major:
the module carries its own triple and data layout.

Only what the directive format can say is an entry. Of `llc`'s arguments
those on the `llc-args` allow-list ("Test directives") are repeated in an
`llc-args` directive (kiln passes `-debugger-tune=gdb` for `-g` tests);
the optimization level, the relocation model and the file type are the
harness's own; anything else is recorded and not repeated, and the entry
is still an entry if it passes admission. The manifest and the census
report say which entries have such arguments.

### Skip reasons

A test without an entry has one row in `SKIPPED.tsv` with one of these
reasons and a detail: the failing tool's first line of output, with the
machine's paths replaced, and for a failed admission without the
program's output and cut before the first path of the run, so that the
table is the same after every capture. Counted per group by `capture` and
in the census report.

Decided from the test alone, without running anything:

| reason | the test |
|---|---|
| `header-error` | has a header that cannot be read, or names a file that is not there |
| `marked-skip`, `marked-xfail` | is marked `skip:` or `xfail:` for kiln |
| `multi-file` | is several translation units the capture does not replay: `pch:` in the header; a torture test of `builtins/` without its `-lib.c` or `lib/main.c` |
| `needs-env` | needs an environment (`env:`) |
| `other-target` | names a target other than the host (`--target`) |
| `requires` | has a `requires:` or `min-oracle:` the host or the group's LLVM does not meet |
| `needs-toolchain` | uses a `%{...}` placeholder that needs a toolchain kiln's suite locates (`%{gcc_toolchain}`, ...) |
| `libc++` | is built with `-stdlib=libc++`; an entry's link is against libstdc++ |
| `link-flags` | has link options the entry's link cannot repeat (`-l`, `-L`, `-Wl,`, `-fuse-ld=`, `-static`, ...) |
| `special-flags` | (torture) asks for options that are not translated |

Found by running:

| reason | what happened |
|---|---|
| `compile-fails`, `clang-rejects` | the producer failed before a module reached llc |
| `llc-fails` | (torture) llc failed on the module |
| `link-fails` | the module was compiled and the link failed |
| `no-module` | kiln++ succeeded without running llc |
| `multi-file` | kiln++ ran llc more than once, and the modules were given different arguments, would have one name, or have no known link order |
| `opt-first-lto`, `-sanitizer`, `-coverage`, `-optimized`, `-coroutine`, `-always-inline` | llc was given nothing (LTO), or bitcode and no `llvm-dis` is installed: kiln ran `opt` first. Which one is told from the arguments, else from the source text (a coroutine keyword or header, searched through the test's own `#include "..."` files), else it is `always-inline`, the remaining cause. A guess, good enough to count. |
| `llvm-dis-fails` | llc was given bitcode and `llvm-dis` did not write it as text |
| `llc-args-unexpected` | llc's command line was not `... -o <object> -` |
| `module-not-text` | the module is not UTF-8 text, has a NUL, or has a line that starts with `;@` |
| `program-fails` | the program was killed, timed out, or (torture) exited with a non-zero status |
| `expectation-mismatch` | (kiln) the program did not do what the header says |
| `stdout-not-expressible`, `stdin-not-expressible`, `args-not-expressible` | the directive format cannot hold the output (not lines of UTF-8, more than 256 KiB), the input, or an argument |
| `not-reproducible` | the entry did not pass admission |
| `oracle-unconfirmed` | the entry's oracle side passed admission and the witness did not confirm it ("irdiff", "The witness") |
| `capture-broken` | the capture itself failed (a tool could not be run) |

### Admission

Before anything enters the corpus, the group's candidates are run by
`irdiff`'s own engine ("What a test does") in strict mode with the
witness, with a stand-in for hod that declines every module: the real
`llc` builds the entry file as `irdiff` will build it, the program is
linked by every linker found and must do what the directives say, and the
`llc -O2` build must behave the same. A candidate is admitted when that
holds for every linker. A candidate whose only trouble is the witness is
`oracle-unconfirmed`. A candidate refused because the oracle's objects did
not link is run once more with `link: allow-warnings`; if that admits it,
it is an entry with that directive (glibc's link-time warnings, which
`ld.bfd` reports and `--fatal-warnings` makes errors), else the first
verdict stands. Anything else is `not-reproducible` with the first failure
as its detail. So every entry is, by construction, a test whose oracle
side passes with every linker and is confirmed by the witness, built from
the entry file alone. `verify` does not use the witness again.

### Layout

```
<corpus>/MANIFEST.tsv
<corpus>/SKIPPED.tsv
<corpus>/CAPTURE.tsv
<corpus>/<source>/llvm<N>/<test>.ll
<corpus>.work/                       shims, kiln's caches, per-run directories
```

`<test>` is the test's name, for kiln without the leading `tests/`
(`kiln/llvm22/conformance/expr.add/x.cpp.ll`, `gcc-torture/llvm19/ieee/inf-1.c.ll`).
Each `<corpus>/<source>/llvm<N>` is a directory of `irdiff` tests: `cargo
xtask irdiff --llvm <N> --hybrid <that directory>` runs it. Given the
whole corpus, `irdiff` runs the entries of its oracle's major and reports
the others as SKIP. A capture
removes its per-run directory when it ends; the rest of the work directory
is reusable and can be deleted at any time.

### Manifest

Three tab-separated tables with a header line, sorted, in which a
backslash, tab, newline and carriage return are written `\\`, `\t`, `\n`,
`\r` and an empty value is `-`.

- `MANIFEST.tsv`, one row per entry: `entry` (the path in the corpus),
  `source`, `llvm`, `test`, `revision` (the commit of the suite's
  checkout, with `-dirty` if tracked files differ, `unknown` if it is not
  the top of a checkout), `producer` (the first line of its `--version`),
  `producer_args`, `llc_args`, `delivered` (the entry's `; delivered:`
  words), `sha256` and `bytes` of the entry file, `key`. A manifest of
  the format before `delivered` (11 columns) is still read.
- `SKIPPED.tsv`, one row per skipped test: `source`, `llvm`, `test`,
  `reason`, `detail`, `key`.
- `CAPTURE.tsv`, one row per group: what the last capture that made the
  whole group anew (nothing reused, no `--limit`) did, how long it took
  and with how many jobs. It is the one file of the corpus that is not a
  function of the inputs.

### Incremental

The `key` of a test is the SHA-256 of: a format tag (`hod-corpus-2`,
changed when entries are made differently), the source, the LLVM major,
`llc`'s `LLVM version` line, the producer's version line, the suite's
revision, the test's name and the SHA-256 of the test's file. A capture
leaves a test alone when its manifest row has the same key and the entry
file still has the row's hash, or when its skip row has the same key;
everything else is captured again. `--force` captures everything again;
`--retry-skipped` captures the skipped again (a skip can be the machine's
fault). Entries and rows of tests that are no longer in the suite are
removed, except by a capture with `--limit`, which looks at the first N
tests of each group and removes nothing. A capture of one source or one
major leaves the other groups as they are.

The key does not see what a test includes: a changed header under a
test's directory, a changed system header, or a changed `xtask` that
captures differently without a new format tag need `--force`.

**What is reproducible.** With the same machine and inputs, a repeated
capture, forced or not, writes `MANIFEST.tsv`, `SKIPPED.tsv` and every
entry byte for byte: the tables hold no times or process ids, kiln runs
in a mirror of its repository so that modules name relative paths, and
details have the machine's directories replaced. The corpus directory's
own location does not matter. Across machines the corpus is stable only
as far as its inputs are: an entry depends on the architecture (it is
captured for the host), on kiln's revision and build, on the exact
versions of clang and LLVM, and on the system's C and C++ library headers
(their inline functions and macros are in the modules). The manifest
records the first three; the last is why a corpus is captured where it is
used and not committed.

### verify

1. **Manifest.** Every row's file is there, at the path its test gives,
   with the row's size and hash, and reads as directives that accept the
   row's LLVM and carry `covers`; no other `.ll` file is under the corpus.
   Each difference is one `MANIFEST <entry>: <what>` line.
2. **irdiff.** For each LLVM major of the corpus, newest first, one
   `irdiff` run over that major's group directories, in hybrid mode
   (`--strict` for strict), with the workspace's hod (or `--hod`); results
   in `<build>/corpus/verify-llvm<N>-<mode>.tsv`. `--manifest-only` stops
   after step 1.

Exit status: 1 when an `irdiff` run has a FAIL or XPASS; else 3 when the
corpus differs from its manifest, or a run has an ERROR or a missing
required tool; else 0. A directory without a corpus is an error (status
3), not a pass.

### census

Reads the tables and every entry's module and writes one Markdown report,
`docs/status/corpus.md` (`--report`); the same corpus gives the same
report. Its parts:

- **Capture**: per group, tests seen, captured and skipped, bytes,
  producer and revision; the last capture's times; skip reasons by group;
  `llc` argument sets and how many entries have arguments `irdiff` does
  not repeat; "Modules of an entry": per group, the entries of several
  modules and those with a module delivered as bitcode.
- **Census**: per group, each table below as `occurrences (modules with at
  least one)`, each module of an entry of several counted on its own, rows sorted by total count, long tables cut with a line
  that says how much was cut; totals; and median, 90th percentile and
  maximum of module size (bytes, lines), definitions, declarations, global
  variables, basic blocks, instructions and metadata nodes per module, and
  blocks and instructions per function.
- **Tests that are not in the corpus**: the kiln tests that fail with
  plain kiln++ and the real `llc` (the reasons `compile-fails`,
  `llc-fails`, `link-fails`, `program-fails`, `expectation-mismatch`,
  `no-module`, `not-reproducible`, `oracle-unconfirmed`, `llvm-dis-fails`),
  then every skipped test by source and reason.

The tables: top-level statements; triples, data layouts and producers;
instructions by opcode, and by opcode and operand type; casts by source
and destination; comparison predicates; instruction flags and atomic
orderings; `getelementptr` shapes; alignments; `switch` sizes; calls by
kind (direct, indirect, intrinsic, inline assembly, through a constant
expression, `tail`/`musttail`/`notail`, explicit function type);
intrinsics called and declared, by name with the overload suffix folded
(`llvm.memcpy.p0.p0.i64` is `llvm.memcpy`); inline assembly; module
assembly; calling conventions; linkage; visibility, preemption, DLL
storage and `unnamed_addr`; thread-local models; COMDATs; global variable
kinds; sections; function signatures; function attributes on definitions
and on declarations (through attribute groups too); parameter, return,
call-site and call-argument attributes; operand bundles; integer widths;
other type mentions (floating-point kinds, pointers with address spaces,
arrays by element); vector types; aggregates passed and returned by value,
by shape with named types expanded; constant expressions and constant
kinds, in initializers and as operands; exception handling (`invoke`,
`landingpad` clauses, `resume`, personalities, funclet instructions);
module flags; named metadata; metadata attachments on instructions and on
globals; `#dbg_*` records against `llvm.dbg.*` calls; metadata node
kinds; DWARF constants; and what the scanner did not recognise.

The census is **lexical**, not a parse. The scanner splits a module into
tokens, in which a comment is nothing and a string, a `c"..."` constant
and a quoted name are one token each, so text inside them is never
counted as code; it joins lines into statements (a line continues the one
before while a bracket is open, and inside a function body when it starts
with `to`, `unwind`, `catch`, `cleanup` or `filter`); and it reads of each
statement only what a table needs. What that approximates is printed in
the report: type mentions are counted per token, so a constant's type is
a mention; constant expressions are recognised by keyword and `(`;
instruction counts leave out `#dbg_*` records; a basic block is a label
line or a function's unlabelled entry block. Anything the scanner cannot
place is counted under "Not recognised", so a gap is visible.

### fetch

Clones `https://github.com/gcc-mirror/gcc.git` at the tag
`releases/gcc-16.1.0` into the `--torture` directory: shallow, without
blobs, with a sparse checkout of `gcc/testsuite/gcc.c-torture/execute/`
only (about 32 MB). The tag is the pin; the checkout's commit is what the
manifest records as the suite's revision
(`6afcc4f6da931eb93f3ab001a0dd9650ea71d1ea` for this tag). A directory
that already holds the suite is left alone. This is the command's only
use of the network.

### Library

`xtask::corpus`: `capture::{Options, capture, CaptureReport}`,
`verify::{Options, verify, VerifyReport, check_manifest}`,
`census::{scan_module, count_corpus, render, write_report, GroupCensus, T,
D}`, `manifest::{Manifest, Skips}`, `entry::{compose, module_of}`, and the
two sources' rules, `kiln::{discover, parse_header, plan}` and
`torture::{discover, flags, clang_args}`. `irdiff`'s `Search::on_path` and
`Search::versioned` are public for it.

### Tests

- Unit tests beside the code: the tables' escaping and round trip, the
  key, entry composition and what cannot be an entry, kiln's header rules
  and every static skip, the capture script, the torture suite's flag
  translation, admission verdicts, the pinned fetch, the command line; for
  the census, the tokens, a module that has a row in every table checked
  row by row, code inside comments, strings and quoted names, intrinsic
  folding, rarer constructs, and module counts.
- `xtask/tests/corpus_fakes.rs`: capture, admission, both tables, entries,
  every run-time skip reason, the witness's and the warning retry's
  verdicts, bitcode with and without `llvm-dis`, several modules,
  incremental and forced captures, `--limit`,
  `--retry-skipped`, two LLVM majors, missing sources and tools, `verify`
  on a sound and on a damaged corpus, and the report, with a kiln++, a
  clang, an `llc`, a driver and linkers that are shell scripts.
- `xtask/tests/corpus_real.rs`: a few C programs laid out as the torture
  suite (among them a `builtins/` test of three files, an output without
  a final newline, and a `tmpnam` program only `ld.bfd` refuses under
  `--fatal-warnings`), captured with the real clang and `llc` for each
  installed major,
  verified through `irdiff --hybrid` with the workspace's hod, and
  counted. Skips without the tools; fails under `HOD_REQUIRE_TOOLS=1`. It
  needs neither kiln nor the GCC suite.

## roundtrip

```
cargo xtask roundtrip [--corpus <dir>] [--llvm <N>]... [--filter <text>]...
                      [--limit <N>] [--tier <quick or all>] [--linker <name>]... [-j <N>]
                      [--expected <file>] [--update-expected]
                      [--results <file>] [--keep] [-v]
```

Layer L2 of `docs/VALIDATION.md`, IR ingestion. Every entry of the IR
corpus ("corpus") is read by `hod_ll` (`spec/ll.md`) into `hod_ir`,
printed back by `hod_ir`'s printer (`spec/ir.md` §9) in the dialect of the
entry's LLVM major, and the printed text must be accepted by that major's
`llvm-as` and verifier and, built by its `llc` in the reference
configuration, linked and run as `irdiff` runs the original, do what the
entry's directives say. `hod_ir` is a normal form, not a copy of the
text, so the comparison is of behaviour, never of text. Each entry ends
in exactly one outcome, and a checked-in expectation file lists the
entries that do not pass, so that the command is a gate. Unix hosts only;
programs are run on Linux, on the corpus's own architecture.

### Options

| option | meaning | default |
|---|---|---|
| `--corpus <dir>` | the corpus ("corpus", "Layout") | `<build root>/corpus` ("corpus", "Where things are") |
| `--llvm <N>` | only the entries of LLVM major `N` (22 or 19); repeatable | every major of the corpus |
| `--filter <text>` | only the entries whose path in the corpus contains `<text>`; repeatable, any one must match | all |
| `--limit <N>` | only the first `N` selected entries of each major, in path order | all |
| `--tier <quick or all>` | `quick`: only the entries of the quick tier | `all` |
| `--linker <name>` | `bfd`, `lld`, `mold` or `wild`; repeatable | every one found, as for `irdiff` |
| `-j <N>`, `--jobs <N>` | entries at once, at most 16; under a jobserver an upper bound | as `irdiff` |
| `--expected <file>` | the expectation file | `tests/roundtrip/expected-<arch>.tsv` of the workspace |
| `--update-expected` | write what this run found into the expectation file | |
| `--results <file>` | the results file | `<build directory>/roundtrip/results-<arch>.tsv` |
| `--keep` | keep the work directories of entries whose outcome is of rank 2 ("Verdicts") | remove them |
| `-v`, `--verbose` | list every entry that does not pass | only those not as expected |

Entries are taken from the corpus's `MANIFEST.tsv`, newest major first,
then in path order. The selection is deterministic, so a filtered,
limited run is the way to iterate; the whole corpus is thousands of
compile-link-run cycles and runs in the background. The tier is applied
before the limit.

**The quick tier** is the entries whose path in the corpus has a SHA-256
whose first byte is below 8: one in 32, by the path alone, so it is
spread over every source, major and directory in proportion, and an
entry's membership does not change when others are added or removed.
On the 6,250-entry corpus it is 206 entries (188 of the 5,760 before it)
(`spec/landed/harness/WP-045.md`).

### Tools

For a major `N`: `llc` as `irdiff` finds the oracle for `--llvm N`
("irdiff", "Tools"); `llvm-as` and `opt` as `<tool>-N` on `PATH`, else
`/usr/lib/llvm-N/bin/<tool>`. The driver and the linkers as for `irdiff`.
A missing `llc`, `llvm-as` or `opt` of a selected major, the driver, or
`bfd`, `lld` or `mold` is a note, and under `HOD_REQUIRE_TOOLS=1` a
failure (exit status 3); `wild` is optional. Without them the entries are
still parsed and printed, and those that get that far are `skipped`.

The **reference `llc` configuration** is SelectionDAG at `-O0`
(`docs/VALIDATION.md` "Oracles"), which is what `irdiff`'s engine gives
its oracle ("irdiff", "What a test does", step 1): the round trip passes
`llc` itself, and the witness is not asked for (the corpus was admitted
with it; a round trip compares with the entry's recorded behaviour).

### What an entry goes through

An entry is one module, or several (`;@ module:` lines, "Test
directives", "Several modules"); its directives (`directives::parse`)
are the whole entry's. Before step 1 the entry's module count must equal
the number of words of its manifest row's `delivered` column ("corpus",
"Manifest"), else the outcome is `harness-error`. How a module was
delivered changes nothing here: a bitcode module is stored as text.

1. **Parse.** Each module's text (the whole file for one module, with
   its directives, which are comments; for several, the module's text
   as `directives::modules` cuts it), by `hod_ll::parse_module` with
   default options (the module has its own triple), then every body by
   a `BodyParser` in file order. Each body must pass `Body::verify`.
   Then, if `hod_ir` recorded any unsupported construct
   (`Module::unsupported`, module level and bodies), the module's
   outcome is `parser-unsupported` with the distinct construct names;
   else the symbols must pass `Module::verify_symbols`.
2. **Print.** `Module::print` with every body, in `Dialect::new(N)`.
   Steps 1 and 2 run in the harness's own process, under
   `catch_unwind`: a panic is an outcome of its own. Every module goes
   through steps 1 and 2; if any fails, the entry's outcome is that of
   the first module whose outcome has rank 2, else of the first of rank 1
   (a bug is not hidden behind an earlier module's honest
   "unsupported").
3. **Assemble.** Each module in turn: `llvm-as -disable-verify -o
   <m>.bc <m>.ll`.
4. **Verify.** And then `opt -passes=verify -disable-output <m>.bc`.
   Steps 3 and 4 stop at the first module that fails.
5. **Build, link, run.** `irdiff`'s engine (`Session::run_test`), in
   strict mode, with the entry's directives as the test's `Expectations`,
   the printed text as the test file (for several modules, each printed
   module after its `;@ module: <name>` line, in the entry's order), the
   oracle's `llc`, and `corpus capture`'s stand-in for hod that declines
   every module ("corpus", "Admission"): the oracle's side is the whole
   check. The engine applies every directive (`llc-args`, `link`,
   `args`, `stdin`, `exit`, `stdout`, `stdout-no-newline`), builds each
   module, links the objects into one program with every linker of the
   run and runs it; its exit status and standard output must be what
   the directives say. An entry passes when every linker's record is the
   stand-in's decline after the oracle's side passed.

For an entry of several modules, the detail of a step-1-to-4 outcome,
and of an `llc-failed`, starts with `module '<name>': `.

Every child of steps 3 to 5 is started by `irdiff::process::run`: it
holds a jobserver token and has `irdiff`'s limits ("Child processes").

### Outcomes

| outcome | rank | meaning |
|---|---|---|
| `pass` | 0 | the printed module did what the entry says, with every linker |
| `parser-unsupported` | 1 | `hod_ll` named constructs it does not support; the detail lists them |
| `printer-not-expressible` | 1 | the printer has no text for a construct in this dialect (`spec/ir.md` §9.5, §9.6); the detail is the construct and the symbol |
| `parser-error` | 2 | `hod_ll` refused the module or a body, or a body or the symbols do not verify |
| `parser-crashed` | 2 | `hod_ll` panicked |
| `printer-error` | 2 | the printer found the IR malformed |
| `printer-crashed` | 2 | the printer panicked |
| `llvm-as-refused` | 2 | `llvm-as` failed on the printed text |
| `verifier-refused` | 2 | `opt -passes=verify` failed on it |
| `llc-failed` | 2 | `llc` failed or wrote no object |
| `link-failed` | 2 | the object did not link (the first linker that failed is named) |
| `behaviour-differs` | 2 | the program ended or printed otherwise than the directives say, or ran into its limit |
| `tool-crashed` | 2 | `llvm-as`, `opt`, `llc`, the driver or a linker was killed by a signal |
| `tool-timed-out` | 2 | one of them ran into its time limit |
| `harness-error` | 2 | the harness could not do its work: an unreadable entry, directives that do not parse, a tool that cannot be started, an engine record it does not know |
| `skipped` | - | not decided: a tool for the remaining steps is missing, or the entry is for another architecture or LLVM; not compared |

The outcome of rank 2 are bugs in hod's parser or printer unless shown
otherwise: the corpus was admitted with these very tools, so the original
of every entry passes step 5. The first failing step decides; the detail
says what failed, with the linker when it is step 5.

The engine's outcome is read from its records' kind of failure
("irdiff", "Library", `Record::failure`), never from their words: an
ERROR of kind `ReferenceBuild` is `llc-failed`, `Link` is `link-failed`,
`Run` or `Comparison` is `behaviour-differs` (the program's own crash,
hang or flood is behaviour), `Crash` is `tool-crashed`, `Timeout` is
`tool-timed-out`. A passing record is a FAIL of kind `Declined` built by
`declined` with the stand-in's reason; any other record is a
`harness-error`, so nothing the engine says can turn a failure into a
pass. The detail is the record's, without the engine's leading
`oracle: `.

### Verdicts

Each entry's outcome is compared with what the expectation file says of
it (an entry not listed must pass):

- **expected**: the same outcome;
- **better**: an outcome of lower rank (a listed failure that now passes,
  or a bug that is now an honest "unsupported"): the file is stale;
- **worse**: any other difference, including another outcome of the same
  rank, which is a different failure to look at;
- **unchecked**: the outcome is `skipped`.

A complete run (no filter, no limit, no `--llvm`, tier `all`) against the
corpus the file is for also reports the entries the file lists that are
not in the corpus (**stale**).

### Expectation file

`tests/roundtrip/expected-<arch>.tsv`, checked in: `#` comment lines,
then one line per entry that does not pass, in path order, three fields
separated by tabs: the entry's path in the corpus, its outcome, and the
detail (for the reader; not compared), escaped as the corpus's tables
are. `--update-expected` writes it, by a new file and a rename: the
selected entries' lines are replaced by what the run found (a pass
removes the line, `skipped` leaves it), the others are kept, and a
complete run drops the lines of entries the corpus does not have. Since
the corpus is captured on the machine where it is used ("corpus",
"Incremental"), the file is that of the reference machine's corpus.

**The file names its corpus**, in one comment line after the header:

```
# corpus: 6250 entries, MANIFEST.tsv sha256 <64 hex digits>
```

the corpus's entry count and the SHA-256 of its `MANIFEST.tsv`, which is
a function of the capture's inputs alone (the suites' revisions, the
producers, the entries' bytes) and not of where the corpus is: moving or
renaming the directory keeps it, a capture that changes any entry does
not. `--update-expected` writes the line for the run's corpus. A run
against another corpus than the file names (or a file without the line)
says so in a note and in the totals line (`against <file>, which is for
another corpus`), compares entries by name as usual, and does not report
stale lines: the entries the other corpus had are not news. Updating the
file for another corpus needs a complete run, else it is a usage error
(exit status 2), so that one file never mixes two corpora.

### Results file

One line per entry, in the order of the run, six tab-separated fields:
outcome, entry, LLVM major, verdict (`expected`, `WORSE`, `BETTER`,
`unchecked`), the expected outcome (`pass` for an entry not listed),
and the detail, escaped as above. Each run replaces it.

### Summary

On standard output: one line that says what ran (the corpus, the
majors, the `llc` configuration, the linkers, the jobs and the jobserver
pool), the notes and missing required tools, one line per entry whose
verdict is worse or better (with `-v`, per entry that does not pass):

```
WORSE     kiln/llvm22/conformance/x.cpp.ll: llc-failed (expected pass): with bfd: llc failed (exit status 1): ...
```

then per major the count of each outcome that occurred, and the totals
by verdict:

```
roundtrip: LLVM 22: 2896 entries: 2890 pass, 4 parser-unsupported, 2 printer-not-expressible
roundtrip: 5760 entries, 5760 as expected, 0 worse, 0 better, 0 unchecked, 0 stale (against tests/roundtrip/expected-aarch64.tsv) in 600.0s
```

The first line says `(quick tier)` after the corpus for `--tier quick`.

### Exit status

| status | when |
|---|---|
| 0 | every entry as expected (or `--update-expected`), and no required tool missing |
| 1 | an entry worse or better than expected, or a stale line, without `--update-expected` |
| 3 | no such entry, but a required tool is missing |
| 2 | a usage error, no corpus, no entry selected, an unreadable expectation file, a linker asked for and not found, `--update-expected` of a partial run for another corpus; nothing was run |

### Work directory

`<build directory>/roundtrip/run-<pid>-<time>/`: the stand-in for hod,
and per major `llvm<N>/` with the engine's directory and one directory
per entry (`NNNNN/printed.ll`, `printed.bc`; for several modules
`NNNNN/printed.ll`, the engine's test, and `NNNNN/modules/<name>.ll`,
`<name>.bc`), removed when the entry is done. With `--keep` the directories of entries of
rank 2 are kept, and the run's directory with them.

### Library

`xtask::roundtrip`: `Options`, `parse_args`, `run(&Options) ->
Result<Report, String>` (`Err` is exit status 2), `Report` (`results`,
`exit_status`, `summary`, `totals_line`, `results_text`), `Outcome`,
`in_quick_tier`, `corpus_identity`, `gate` (the step of "ci");
`ingest::ingest` (steps 1 and 2 for one module's text);
`expected::{Expectations, Verdict, verdict}`; `stages` (steps 3 to 5).

### Tests

Unit: `roundtrip::tests::{outcome_names, options, quick_tier}`,
`roundtrip::ingest::tests::ingest_outcomes`,
`roundtrip::stages::tests::records_map_to_outcomes`,
`roundtrip::expected::tests::{verdicts, the_file}`.
`xtask/tests/roundtrip_fakes.rs` runs the command over small
corpora of real LLVM text with fake `llvm-as`, `opt`, `llc`, driver and
linker that misbehave on the names of the module's globals:
`every_outcome_is_named`, `the_expectation_file_is_a_gate`, `selection`,
`missing_tools`, `work_directory_and_keep`, `several_modules`,
`directives_are_the_engines`, `the_file_names_its_corpus`,
`the_quick_tier`, `the_gate_step`.
`xtask/tests/roundtrip_real.rs` (`real_tools_round_trip`) builds four
hand-made entries per installed major with the real tools: one passes,
one differs from its directives, one of two modules linked together
passes, and one with `llc-args`, `link: allow-warnings` and
`stdout-no-newline` passes. It skips without the tools and fails under
`HOD_REQUIRE_TOOLS=1`.

### In the gate

`cargo xtask ci --quick` runs `roundtrip --tier quick`, and `cargo xtask
ci` the whole corpus, after the `irdiff` step, with the default corpus,
expectation file, linkers and jobs ("ci" for a missing corpus). The step
fails if the run would exit non-zero; it prints the totals line when it
passes and the summary when it fails. The quick tier costs about 20 s at
`-j 4` on the reference machine, the whole corpus 10 to 20 minutes
depending on the shared host's load (`spec/landed/harness/WP-045.md`).

## bench

```
cargo xtask bench [--generate | --check | --update-baseline --reason <text> | --report] [options]
```

The measurement harness: how long `llc -O0` and hod take to turn a set of
pinned inputs into object files, how much memory they need, and how large
the objects are. Speed and memory are tested properties of hod
(`docs/VALIDATION.md`), and this command is where they are measured,
compared with `llc`, and gated against a recorded baseline. Unix hosts
only; the memory probe needs GNU `time`.

Without an action the command produces the inputs that are not up to
date, measures every input with every tool configuration, writes a
results file and prints a summary. The default run (20 inputs, 3 runs)
takes well under a minute on the development host, and everything the
command starts runs serially, one process at a time.

### Options

One action at most; value options are `--name <value>` or
`--name=<value>`. The command takes no other arguments.

| option | meaning | default |
|---|---|---|
| `--generate` | action: produce every selected input's IR again, list the inputs, measure nothing ("Inputs") | |
| `--check` | action: measure, then judge hod against a baseline ("The gate") | |
| `--update-baseline` | action: measure, then record the run as this machine's baseline ("Baselines"); needs `--reason`; takes neither `--filter` nor `--kiln`, and only the preferred LLVM (22) | |
| `--report` | action: write the report from a results file, measure nothing ("The report") | |
| `--reason <text>` | why the baseline changes; only with `--update-baseline`, and not blank | |
| `--runs <N>` | runs per input and tool configuration, 1 to 100 | 3 |
| `--filter <text>` | only the inputs whose name contains `<text>`; repeatable (any of them); matching no input is a usage error | every input |
| `--llvm <major or dir>` | the LLVM whose `llc` is the reference, as for `irdiff` | the first of 22 and 19 that is installed |
| `--hod <binary>` | the hod to measure | build the workspace's with the release profile (`cargo build --release --package hod_cli --bin hod`, into the build directory) |
| `--kiln <kiln++>` | produce the inputs with kiln instead of clang ("Inputs") | clang |
| `--judge <tool>` | the configuration `--check` judges: `hod` or `hod+fallback` | `hod` |
| `--baseline <file>` | the baseline to check against or to write | `bench/baselines/<machine id>.tsv` |
| `--results <file>` | the results file to write; for `--report`, to read | under the build directory ("Results file") |
| `--out <file>` | what `--report` writes | `docs/status/perf.md` |
| `-v`, `--verbose` | also print hod's phase times | |
| `-h`, `--help` | print the usage and exit 0 | |

The build directory is that of `irdiff`. Everything the command writes is
under `<build directory>/bench/`, except a `--results` file, the baseline
and its log (`--update-baseline`), and the report (`--report`).

### Tools

- **Reference.** `llc`, found as `irdiff` finds its oracle ("irdiff",
  "Tools").
- **Producer.** The C and C++ compilers of the same LLVM: for `clang` and
  `clang++`, the file beside `llc` under `llc`'s own naming (`llc-22` has
  `clang-22`, a plain `llc` has `clang`), else `<name>-<major>` on
  `PATH`, else `/usr/lib/llvm-<major>/bin/<name>`. With `--kiln <kiln++>`:
  that file for C++ and the file beside it named without the `++` for C.
  The producer's version is the first non-empty line of its C++ compiler's
  `--version` (the C compiler's when there is no C++ compiler).
- **hod.** `--hod`, or the workspace's built with the release profile:
  what is measured is what a user would run.
- **Memory probe.** GNU `time` at `/usr/bin/time`. It is used if a trial
  run of it (`-f '%M %U %S' -o <file> -- /bin/sh -c :`) writes its three
  numbers. Without it only wall time is measured, and the summary says so.

Without `llc` or without a producer nothing is measured: the summary says
what was not found and the exit status is 0, or 3 under
`HOD_REQUIRE_TOOLS=1`, which also makes a missing memory probe a failure
(the run is still made).

### Manifest

`bench/manifest` lists the inputs. It is read line by line; surrounding
blanks are ignored, as are empty lines and lines that start with `#`.

```
common = -O0 -S -emit-llvm -fdebug-compilation-dir=.

[stl-heavy]
class = large
source = stl_heavy.cpp
flags = -std=c++23
```

- `[name]` starts a section. A name is lower-case letters, digits and `-`,
  starts with a letter or a digit, does not end in `-` or `-g`, and is
  used once.
- Any other line is `key = value`, split at the first `=`.
- Before the first section the only key is `common`, required, once: the
  flags of every compile, separated by blanks (there is no quoting).
- In a section: `class` (required): `small`, `typical`, `large` or
  `pathological`. `source` (required): a file under `bench/src/`, as a
  relative path without `..`, ending in `.c` (C) or `.cpp`, `.cc`, `.cxx`
  (C++). `flags` (optional, may be empty): more flags for this source.
  Each key once.
- `-o` and `-g` may not appear in `common` or `flags`: the harness adds
  them.

Every error of the file is reported with its line (`line 7: unknown class
'huge': ...`), and a manifest with an error, or without a section, is
exit status 2.

A section gives two inputs, in this order: `<name>`, compiled with
`common` then `flags`, and `<name>-g`, compiled with `-g` after those.

### Inputs

The sources in `bench/src/` are written for this repository, each with a
comment that says what it stands for, each a complete program whose `main`
checks its own results and returns 0. The classes:

| class | inputs | what it is |
|---|---|---|
| `small` | `small-c`, `small-cxx` | a few functions; process start-up is most of the time |
| `typical` | `interp` (C: an interpreter with one large `switch`), `tables` (C: large initialised data with relocations, a large zero-initialised array), `exceptions` (C++: nested handlers, cleanups, rethrow), `containers` (C++: standard containers and strings) | an ordinary translation unit, hundreds of KiB to a few MiB of IR |
| `large` | `templates` (thousands of small `linkonce_odr` functions from our own templates), `stl-heavy` (streams, regular expressions, variants, hash maps) | thousands of functions, several MiB of IR |
| `pathological` | `huge-function` (one function of tens of thousands of instructions), `tiny-functions` (five thousand functions of a few instructions) | one shape taken to an extreme |

An input's IR is `<build directory>/bench/inputs/<producer>/<input>.ll`,
where `<producer>` is `clang<major>` or `kiln`. It is produced by running,
in `bench/src/`,

```
<compiler> <common> <flags> [-g] -o <temporary file> <source>
```

under the limits of "Child processes" below with 3 minutes and 8 GiB, and
renaming the result. The compile runs in the source's directory with the
source named relatively, and `common` carries
`-fdebug-compilation-dir=.`, so that the IR, and with it its hash, does
not depend on where the checkout is. Beside the IR, `<input>.stamp`
records, as `key<TAB>value` lines, `key` (the hash of the producer's
version, the compile's arguments and the source's bytes, each ended by a
NUL), `ir_bytes`, `functions` and `ir_hash`.

An input is up to date when its stamp has the key the manifest, source
and producer give now and the IR file has the recorded size. A run
produces the inputs that are not up to date; `--generate` produces every
selected input again and prints, per input, its class, size, function
count, hash and whether it was produced. (The stamp does not see the
headers a source includes. The sources include standard-library headers
only, which change with the toolchain; `--generate` is the remedy.)

An input is identified by:

- **`ir_bytes`**, the size of the IR file;
- **`functions`**, the number of lines of it that start with `define `;
- **`ir_hash`**, `fnv1a64:` and the 16 lower-case hexadecimal digits of
  the 64-bit FNV-1a hash of its bytes (offset `cbf29ce484222325`, prime
  `100000001b3`).

An input that cannot be produced (the compiler fails, times out, writes
nothing, or there is no compiler for its language) is one `error` record
with the tool `generate` and is not measured.

### Measurement

Each input is measured with three tool configurations:

| tool | command | what it is |
|---|---|---|
| `llc` | `<llc> -O0 -filetype=obj -relocation-model=pic -o <object> <input>.ll` | the reference |
| `hod` | `<hod> --strict --coverage-log=<file>` and the same arguments | what hod does by itself |
| `hod+fallback` | `<hod> --fallback=<llc> --coverage-log=<file>` and the same arguments | what a hybrid build runs: hod, and `llc` for what hod declines |

A **round** runs each configuration once, in that order. An input is
measured in `--runs` rounds, then the next input: so a burst of load on
the machine falls on all three configurations and not on every run of
one. Nothing else is started by the harness while a child is measured.

A measured child:

- runs in the run's work directory, with the harness's environment
  without `MAKEFLAGS` and `CARGO_MAKEFLAGS` (a measured child is one
  process and is not to find a jobserver), and, for hod, without
  `HOD_FALLBACK_LLC`, `HOD_STRICT`, `HOD_COVERAGE_LOG` and
  `HOD_IN_FALLBACK` and with `HOD_TIME_PHASES=<file>` ("Phases");
- has no standard input or output; the first 64 KiB of its standard error
  are kept for diagnostics;
- is started as `/bin/sh -c '<set the limits>; exec "$@"' sh <KiB>
  <seconds> [/usr/bin/time -f '%M %U %S' -o <file> --] <program>
  <arguments>`, in its own process group, with the limits of "irdiff",
  "Child processes" (2 minutes, 8 GiB), set outside GNU `time` so that its
  report is the program's alone;
- has its object, coverage log, phase report and usage report removed
  before it starts.

What is measured:

- **Wall time**: from just before the child is started to the return of
  `wait`, taken by a thread that is blocked in `wait` (not by polling, which
  would add its interval). It includes starting the shell and GNU `time`
  ("The method's error").
- **CPU time**: user plus system seconds of the program, from GNU `time`
  (`%U`, `%S`), in microseconds.
- **Peak memory**: the peak resident set size of the program in KiB, from
  GNU `time` (`%M`). GNU `time` is used because the kernel's resource
  record of a child (`wait4`) cannot be read from safe Rust without a
  dependency.
- **Object size**: the size of the object file after the run.

Over the runs of one configuration on one input:

| figure | value |
|---|---|
| wall time | the fastest run (`wall_us_min`), the gate's figure and the one in every ratio; also the middle (`wall_us_median`) and the slowest (`wall_us_max`) |
| CPU time | the smallest |
| peak memory | the middle run (`rss_kib`); also the smallest and the largest |
| object size | the last run's |
| phases | those of the fastest run |

The middle of an even number of values is the lower of the two middle
values plus half their difference, rounded down. The fastest run is the
estimate because noise on a shared machine only ever adds time; the
spread, `(wall_us_median - wall_us_min) / wall_us_min`, says how much it
added to a typical run.

### Outcomes

Every record has one of four statuses.

| status | when |
|---|---|
| `ok` | the tool exited with status 0 and wrote a non-empty object itself: `llc` always; hod when its coverage line says `hod` |
| `fallback` | `hod+fallback` exited with status 0, its coverage line says `fallback`, and the object is there. The detail is the coverage reason |
| `declined` | `hod` (strict) exited with status 2 and its coverage line says `error`. The detail is the coverage reason. No object |
| `error` | anything else: another exit status, a signal, the time limit, no object, no coverage line, a malformed phase report, runs of the same tool that did not end the same way, a harness failure; or the input could not be produced |

A **decline is not a time for building an object**. Its record keeps what
declining cost (hod reads the whole module before it declines), but has
no object size, no ratio to `llc` and no throughput, and the summary shows
it as `declined: <reason>`, never in the column of build times.

An **error is not a zero**. An `error` record has no numbers at all, 0
runs, and the detail says what happened (`llc exited with status 1: <first
line of its standard error>`, `hod was terminated by signal 11`, `llc timed
out after 120 s`, `hod+fallback exited with status 0 and wrote no object`).
The tool is not run again on that input; the other tools and inputs are
measured, and the run's exit status is 3.

### The method's error

Each run measures its own method first and records the result in the
header of the results file:

- **`floor_us`**: the fastest of 5 runs of `/bin/true` as a measured
  child. Every wall time includes about this much (the shell, GNU `time`,
  a process start). It is recorded, not subtracted: the same amount is in
  `llc`'s and hod's times, which makes ratios of small inputs closer to 1
  than the tools are, and the reader can see by how much.
- **`rss_error_kib`**: the peak memory measured for `dd if=/dev/zero
  of=/dev/null count=1` with a block of 64 MiB plus 4 KiB, minus that for
  a block of 4 KiB, minus 65536. `dd` allocates its block and fills it, so
  this is the error of the probe on a known allocation, signed.

The stated error of the peak-memory figure is 1% of a 64 MiB allocation
(655 KiB) and a test holds the probe to it; on the development host the
measured error is under 100 KiB. CPU time has the resolution of GNU
`time`'s report, 10 ms, and is therefore reported but neither gated nor
meaningful for the small inputs.

### Ratios and throughput

Every record of hod that produced an object (`ok` or `fallback`) carries,
besides its absolute figures, the ratio of each to `llc`'s for the same
input in the same run: `wall_ratio` (fastest over fastest), `cpu_ratio`,
`rss_ratio` (middle over middle), `size_ratio`. A ratio is a decimal with
four digits after the point, rounded to nearest (`0.1250`); it is absent
when either side lacks the figure, when `llc`'s record is not `ok`, and
for `llc` itself.

Every record that produced an object carries its throughput on the
fastest run: `ir_bytes_per_s` = `ir_bytes` × 1000000 / `wall_us_min` and
`functions_per_s` likewise, rounded down.

### Phases

hod reports where its time went through the phase report of `spec/cli.md`
("Phase report"): the harness sets `HOD_TIME_PHASES` to a file in the work
directory for every run of hod and reads it afterwards. A record carries
the phases of its fastest run, in hod's order, `total` included. A hod
that writes no report (one older than the mechanism) has no phases; a
report with a line that is not `<phase><TAB><digits>` is an error.

The harness takes whatever phase names it finds. Today they are `read`,
`compile`, `write` or `fallback`, and `total`; the names reserved for the
code generator's own phases are `ingest`, `analysis`, `emission`,
`debug-info` and `object`.

### Results file

`<build directory>/bench/results-llvm<major>.tsv`
(`results-kiln-llvm<major>.tsv` with `--kiln`), or the `--results` file.
Each run replaces it (written to a temporary name beside it and renamed).
A baseline is the same format. The file is text:

```
# hod-bench 1
# <key>⇥<value>            (the header, one line per key)
# fields⇥status⇥input⇥...  (the names of the 25 fields)
<record>                   (one line per input and tool)
```

The header keys, in order; a key whose value is not known is left out:

| key | value |
|---|---|
| `date` | when the measurement started, UTC, `YYYY-MM-DDThh:mm:ssZ` |
| `machine` | the machine id ("Machine") |
| `cpu`, `cores`, `arch` | what the id is made of |
| `llc_version` | `LLVM version <x.y.z>`, from `llc --version` |
| `hod_version` | the first line of `hod --version` |
| `hod_build` | `workspace, release profile` or `given with --hod` |
| `producer` | the producer's version |
| `runs` | `--runs` |
| `usage` | `gnu-time (<path> -f '%M %U %S')` or `none` |
| `floor_us`, `rss_error_kib` | "The method's error"; the latter with its sign |
| `load` | the three load averages of `/proc/loadavg` when the measurement started |

Records are in manifest order, and for each input `llc`, `hod`,
`hod+fallback` (or the one `generate` record). The fields, separated by
tabs; a figure that does not exist is `-`:

| # | field | value |
|---|---|---|
| 1 | `status` | `ok`, `fallback`, `declined`, `error` |
| 2 | `input` | the input's name |
| 3 | `class` | its class |
| 4 | `debug` | `g` for a `-g` input, else `-` |
| 5 | `tool` | `llc`, `hod`, `hod+fallback`, `generate` |
| 6 | `ir_bytes` | |
| 7 | `functions` | |
| 8 | `ir_hash` | |
| 9 | `runs` | the runs the figures are from; 0 for an error |
| 10–12 | `wall_us_min`, `wall_us_median`, `wall_us_max` | microseconds |
| 13 | `cpu_us` | microseconds |
| 14–16 | `rss_kib`, `rss_kib_min`, `rss_kib_max` | KiB |
| 17 | `object_bytes` | |
| 18–21 | `wall_ratio`, `cpu_ratio`, `rss_ratio`, `size_ratio` | to `llc` |
| 22–23 | `ir_bytes_per_s`, `functions_per_s` | |
| 24 | `phases` | `<phase>=<microseconds>` joined by `,` |
| 25 | `detail` | the reason of a decline or a fallback; what failed |

Every number is a decimal integer except the ratios. Header values and
fields 2, 8, 24 and 25 are free text, escaped as in hod's coverage log
(`\\`, `\t`, `\n`, `\r`; an empty value is `-`, and a value that is
exactly `-` is written `\-`). Fields 22 and 23 are derived from 6, 7 and
10 and are ignored when a file is read. A file read and written again is
the same bytes. A reader refuses a first line that is not `# hod-bench
1`, a `fields` line that differs from its own, a header key that appears
twice or after a record, and a record that is not 25 well-formed fields,
with the line number.

### Machine

The machine id says which runs' absolute numbers may be compared. It is
`<arch>-<cpu>-<cores>c`, where `<arch>` is the architecture the harness
was built for, `<cpu>` is the first `model name` of `/proc/cpuinfo`, else
`implementer <CPU implementer> part <CPU part>` from its first such lines
(Arm), else `unknown`, and `<cores>` is the parallelism available to the
process; then every run of characters other than ASCII letters and digits
becomes one `-`, letters become lower-case, and no `-` is left at the
ends. Example: `aarch64-implementer-0x61-part-0x000-24c`. It names no
host and no user. Two machines of the same model share an id, and their
numbers are comparable to the extent that they are the same machine.

### Summary

On standard output: a line per missing tool and per note; one line that
says how many inputs, on which machine, produced by what; one that states
the method (runs, floor, memory error, load); a table of `llc`'s figures
per input (class, IR size, functions, fastest time, spread, peak memory,
object size, IR MB/s, functions/s); a table of hod's (time and memory of
what hod built itself with their ratios to `llc`, the same for
`hod+fallback`, and in words what strict hod did: `built`, or `declined:
<reason> (in <time>, <memory>)`); with `-v` a line of phases per record
that has them; a line `bench: ERROR <input> <tool>: <detail>` per error;
the sums of the fastest runs of hod and of `llc` over the inputs both
produced, with their ratio; the middle and the largest spread of `llc`'s
times, with the input of the largest; the counts; and where the results
are. Times are in milliseconds, memory in MiB, sizes in KiB.

### Baselines

A baseline is the results file of a run on one machine, committed as
`bench/baselines/<machine id>.tsv`, with every tool's records: hod's are
what the gate compares, `llc`'s are its control.

`--update-baseline --reason "<text>"` measures every input and then

- refuses, with exit status 3 and nothing written, if the run has an
  `error` record;
- writes the run as the baseline (`--baseline <file>` or this machine's);
- appends one line to the log beside it (the baseline's name with `.log`
  for `.tsv`): `<date><TAB><reason><TAB><N> inputs, hod built <M>;
  <hod_version>; <runs> runs`, with tabs and line ends in the reason
  replaced by blanks;
- says how many inputs have a time, of any tool that built them, whose
  spread is above 5%: the gate will not fail on those times ("The gate",
  "Spread"). A baseline that is to bind times is recorded on a quiet
  machine, or with more `--runs`.

The baseline is replaced deliberately, by a person or agent who can say
why hod's numbers changed; the log is the history of those reasons, next
to the numbers in the repository's own history. A baseline holds every
input of the manifest, produced by clang, measured against the preferred
LLVM: `--update-baseline` with `--filter`, with `--kiln`, without a
reason, or with another LLVM is a usage error.

### The gate

`--check` measures, then judges every record of the judged configuration
(`hod`; `hod+fallback` with `--judge`) against the baseline: the one
named with `--baseline`, else this machine's, else the first by name of
the baselines recorded for other machines; with none at all there is
nothing to judge, the summary says so, and the exit status is 0.

The baseline **binds** the run when its `machine` and its `llc_version`
are the run's. Per input, with `base` the baseline's record and `now` the
run's, the verdict is the first of these that applies:

| verdict | when | effect |
|---|---|---|
| not judged: the run has an error for this input | `now` is an `error` | the run's exit status is 3 for the error |
| new: the baseline does not have this input | no `base`, or `base` is an `error` | |
| advisory: the input changed since the baseline | the `ir_hash` differs | |
| not judged: declined, as in the baseline | both are `declined` | |
| advisory: another machine (`<id>`), another llc (`<version>`) | the baseline does not bind | |
| LOST | `base` is `ok` and `now` is not: hod no longer builds what it built | fails |
| new: hod builds this input now | `now` is `ok` and `base` is not | |
| REGRESSED (time), (memory), (time, memory) | below | fails |
| NOISY | below | exit status 3 |
| improved | not regressed, and `base` is above `now` by more than the same margins (without drift) in time or in memory | a line that reminds of `--update-baseline` |
| ok | otherwise | |

A decline is not judged: it is not a time for building an object, and a
3 ms process is below what the method resolves. The table shows what it
cost then and now. What hod hands to its
fallback (`--judge hod+fallback`, status `fallback` on both sides) is a
build and is judged like one.

**Memory** is regressed when `now.rss_kib` > `base.rss_kib` × 1.02 and
by more than 512 KiB. It is judged whatever the machine is doing.

**Time** is slower when `now.wall_us_min` > `base.wall_us_min` × drift ×
1.05 and by more than 1 ms. A slower time is a regression, unless the
measurement cannot carry it (drift, spread), in which case the input is
NOISY if its memory did not regress.

The **5% and 2%** are hod's performance contract: a change that makes
hod more than 5% slower or 2% larger on any input does not pass without
a baseline update that says why. The **absolute margins** keep the gate
from judging differences the method cannot resolve: a process start
varies by more than 1 ms on a busy machine, and the resident set of one
program on one input by a few hundred KiB with the state of the page
cache (up to 316 KiB between runs was measured for hod reading a module,
and 436 KiB for `llc`).

**Drift.** `llc` is the control: it is the same program as when the
baseline was recorded, so what it lost since, the machine lost. The drift
of an input is `now_llc.wall_us_min / base_llc.wall_us_min`, at least 1
(a faster machine does not tighten the gate), and 1 when either record is
missing or not `ok`. It scales the time threshold only. A drift above
1.25 means the machine is too busy for a time measured now to say
anything about hod, slower or faster: the time is not judged.

**Spread.** "Slower" rests on four fastest runs: hod's and `llc`'s, in
the baseline and now. If the spread of any of the four
(`(wall_us_median - wall_us_min) / wall_us_min`) is above 5%, the
threshold itself, its runs disagree with each other by more than the
difference being claimed, and the time is not a regression. (A time that
is not slower needs no such care: noise only adds time, so a fastest run
under the threshold was really that fast.)

**NOISY** is the verdict of an input whose time could not be judged for
either reason and whose memory did not regress. It is not a failure of
hod and not a pass: the exit status is 3.

**Measuring again.** An input whose verdict is REGRESSED or NOISY is
measured again, `--runs` more rounds, up to twice. The new runs join the
old ones (so the fastest of all of them is judged, the spread is that of
all of them, and `llc`'s drift is taken from all of them) and the input
is judged again; only a verdict that stands after that counts. The
summary says how often each input was measured again, and the results
file holds what was judged.

**On another machine.** When the baseline does not bind, the check is
advisory: the table compares the ratios to `llc` (`wall_ratio`,
`rss_ratio`) of baseline and run instead of absolute figures, nothing is
measured again, and the exit status is 0.

The summary of a check is a table (input, baseline and current time and
their difference, `llc`'s drift, baseline and current memory and their
difference, verdict, with `; declined` and `; measured again <n>x` where
they apply) and a line of counts with the thresholds.

**Where the gate means something.** The time half of the gate needs a
machine that repeats itself to well under 5%. The shared development
host does not. With eight agents building on it (load average 10 to 18 on
24 cores), over five runs of the command within five minutes the fastest
of 3 runs of `llc` on one input ranged over 27% for the middle input and
62% for the worst; the fastest of 15, over four runs, still over 23%.
The ratio of two tools measured in the same rounds, which is what the
drift correction relies on, was steadier and still moved by 5% to 20%
from one set of 9 rounds to the next. Against a baseline recorded there,
a time verdict that is not `ok` is usually NOISY. The memory half holds
everywhere: `llc`'s peak memory on one input stayed within 0.75% across
the same runs, hod's within 316 KiB.

So `--check` is run where a baseline exists for the machine and the
machine is quiet, and it is not part of `cargo xtask ci`, whose runs
share the machine with other work by design. On the shared host it is a
memory gate and a detector of large time regressions, and says NOISY for
the rest.

### The report

`--report` reads a results file (`--results`, else the default file of
the preferred LLVM, or of `--llvm <major>`) and writes
`docs/status/perf.md` (`--out`): where and how the numbers were measured
(date, machine, reference, hod, producer, method, spread), a table of
`llc`'s figures per input, a table of hod's with what it declined, hod's
phases, and the errors. The report is a function of the results file; it
says that it is generated and that its numbers are those of one machine.
It is regenerated when the baseline is.

### Work directory

Each measuring run works in `<build directory>/bench/run-<pid>-<time>/`,
which holds the objects, coverage logs, phase and usage reports of the
child being measured, and is removed when the run ends.

### Exit status

| status | when |
|---|---|
| 0 | measured (or listed, or reported) without an error; a check passed, was advisory, or had no baseline |
| 1 | `--check`: an input is REGRESSED or LOST against a binding baseline |
| 3 | not 1, and: a record is an `error`, an input could not be produced, a required tool is missing, an input is NOISY, or the baseline update was refused |
| 2 | the request cannot be carried out: a usage error, a manifest with errors, a filter that matches nothing, an unusable `--llvm`, `--hod` or `--kiln`, a file that cannot be read or written |

### Library

`xtask::bench`: `run(&Config) -> Result<Run, String>` is the command as a
function (`Err` is exit status 2); `Config` has a field per option, plus
the checkout, the build directory, the tool search, the probe's path, the
child limits, the machine, the gate's `Thresholds` and the number of
retries, which is how the tests run it against fake tools. `Run` has the
results, the check, `summary(verbose)` and `exit_status()`.
`measure::run_measured` is the measured child; `record::Results` reads
and writes the file; `check::judge` is the gate's rule for one input;
`report::perf_md` is the report.

`measure::run_measured` duplicates the limit wrapper of
`irdiff::process::run` because that one polls for its child's end. A
measured child holds one jobserver token ("Jobserver") from before the
clock starts until it is reaped, so waiting for the pool is never part of
a measurement; the build of the workspace's hod is a cargo step
(`jobs::children::run_step`).

### Tests

`xtask/src/bench/manifest.rs` ("Manifest", "Inputs", "Tools"):
`manifest_is_parsed`, `manifest_errors`, `the_repository_manifest`,
`inputs_are_generated_and_cached`, `generation_failures`,
`producers_are_found`.

`xtask/src/bench/measure.rs` ("Measurement", "The method's error"):
`usage_report_parsing`, `fastest_of_n_selection`, `a_measured_run`,
`signals_and_the_time_limit`, `harness_errors_and_probe_detection`,
`real_probe_measures_a_known_allocation` (the real GNU `time`: within 1%
of a 64 MiB allocation; skipped without it, required under
`HOD_REQUIRE_TOOLS=1`).

`xtask/src/bench/record.rs` ("Ratios and throughput", "Results file",
"Machine"): `ratio_computation`, `throughput`, `record_lines_round_trip`,
`malformed_lines`, `results_file_round_trip`,
`results_are_written_and_read`, `input_hash`, `machine_id`.

`xtask/src/bench/check.rs` ("The gate", "Baselines"):
`check_pass_and_fail`, `small_differences_are_not_judged`,
`the_reference_tool_is_the_control`,
`an_unsteady_measurement_is_not_a_regression`,
`advisory_on_another_machine`,
`inputs_without_a_comparison`, `judging_a_run_and_its_text`,
`number_formats`, `baseline_update_with_reason`.

`xtask/src/bench/report.rs` ("Summary", "The report"): `summary_text`,
`perf_report`, `units`. `xtask/src/bench.rs` ("Options", "Phases"):
`options`, `utc_dates`, `phase_reports`.

`xtask/tests/bench_fakes.rs` runs the whole command against fake tools
(shell scripts standing in for the compiler, `llc`, hod and GNU `time`;
files beside them make a tool slow, large or unwilling), so every outcome
and verdict is provoked on any Linux machine:
`a_run_measures_every_tool` (every input and tool, fastest of N, ratios,
a decline reported as a decline, phases, the order of a round, the file
round trip), `failures_are_error_lines_not_zeros`,
`a_tool_that_hangs_is_an_error`, `check_pass_and_fail` (pass; a decline
is not judged; slower; larger; LOST; `--judge`), `the_remeasure_rule`,
`advisory_on_another_machine`, `a_busy_machine_is_not_judged`,
`baseline_update_with_reason`, `missing_tools`, `generate_and_report`.
Their gate thresholds are the contract's except the absolute time margin
(100 ms) and the drift and spread limits (off, but for the test of a
busy machine): the fakes run for milliseconds, and the tests that judge
time make a tool sleep for far longer than that.

`xtask/tests/bench_real.rs` uses the real tools and skips without them
(fails under `HOD_REQUIRE_TOOLS=1`): `one_input_end_to_end` (clang, `llc`,
GNU `time`, the workspace's hod: `llc`'s record is a time, a peak memory
and an object; hod's is a decline with its reason or an object; hod with
its fallback costs `llc`'s memory and reports its phases; the memory
probe is within its stated error).

## debug

```
cargo xtask debug [run] [options] [<test or directory>...]
cargo xtask debug kiln [--kiln <kiln++>] [--kiln-repo <dir>] [--hod <binary>]
cargo xtask debug verify [-j <N>] [--hod <binary>] [-v] [<corpus dir or .ll>...]
```

Layer L7 of `docs/VALIDATION.md`: hod's debug information under the
debuggers, against `llc`'s (`spec/dwarf.md` §8.4). Unix hosts only.
Code: `xtask/src/debug.rs` and `xtask/src/debug/`.

### Options

`run` (the default):

| option | meaning |
|---|---|
| `-j <N>`, `--jobs <N>`, `-j<N>` | units (a test and a front end) at once; default `min(4, cores)`, at most 16 |
| `--hod <binary>` | the hod tested; default: this workspace's, built (`irdiff::build_workspace_hod`) |
| `--linker <name>` | `bfd`, `lld`, `mold`, `wild`; repeatable; default every one found. One asked for and not found is an error |
| `--debugger <name>` | `gdb`, `lldb` (every lldb), `lldb-19`, `lldb-22`; repeatable; default all |
| `--frontend <name>` | `clang-19`, `clang-22`, `kiln`, `ir-19`, `ir-22`; repeatable; default every one the test allows |
| `--tier <quick\|all>` | `quick`: only the tests with `tier: quick`; default `all` |
| `--no-lines` | no `(function, line)` comparison |
| `--kiln <kiln++>` | the kiln front end; default the build root's (`corpus::capture_defaults`) |
| `--keep` | keep the work directories of units with a result other than PASS or SKIP |
| `--results <file>` | default `<build>/l7/results.tsv` |
| `-v`, `--verbose` | every result and every lines difference, not only the notable ones |
| `--` | the rest are paths |

### Tools

Found as `irdiff`'s are (`irdiff::tools::Search`): `clang-19`,
`clang++-19`, `clang-22`, `clang++-22` (front ends); `llc-19` and `llc-22`
(each the reference of IR from its own major; kiln writes LLVM 22); the
driver `gcc` and the linkers of `irdiff`'s matrix; `gdb` on `PATH`;
`lldb-22` and `lldb-19` (on `PATH` or in `/usr/lib/llvm-<N>/bin`; a plain
`lldb` only when neither is found, under the name its `--version` gives);
`llvm-dwarfdump`, `llvm-nm` and `llvm-objdump` of LLVM 22, else 19, for
the lines comparison. kiln++ is a front end of the harness, not a tool of
the gate: without it its units are SKIP. Every other missing tool is a
note, and under `HOD_REQUIRE_TOOLS=1` exit status 3 (`wild` excepted, as
in `irdiff`).

### Tests

A debugger test is a `.c`, `.cpp` or `.ll` file under `tests/debug/` (the
directory is walked; a file without a directive is not a test) whose
directives are comment lines, `//@ key: value` (`;@` in IR), **at the end
of the file**, so that editing them moves no source line:

| directive | meaning |
|---|---|
| `gdb: <command>` | the gdb script, in order |
| `lldb: <command>` | the lldb script, in order |
| `gdb-check: <text>`, `lldb-check: <text>` | lines `llc`'s masked transcript must have, in order, each on a later line than the previous one's |
| `module: <file>` | another source of the program, relative to the test; the test's own file is module 0 |
| `objects: whole\|mixed` | `mixed`: also link each module from hod with the others from `llc` (below) |
| `frontend: <name>...` | the front ends; default `clang-19 clang-22 kiln` for C++, `clang-19 clang-22` for C, `ir-19 ir-22` for IR |
| `cflags: <flags>` | after `-g -O0 -S -emit-llvm` |
| `link: <flags>` | after the objects |
| `tier: quick\|all` | default `all` |
| `xfail(<scope>): <reason>`, `xfail: <reason>` | a known difference (below) |

A scope is words naming front ends, linkers or debuggers (`lldb` is any
lldb), comma- or space-separated; it covers a result when, for each kind
it names, one of its words of that kind is the result's. An unknown key, word or value
is an ERROR of the test. A test's checks must hold on `llc`'s program: the
checks say what the test means, the comparison with `llc` is what judges
hod. Tests print no uninitialized value (stack contents differ between the
programs), and a program with threads prints nothing (its output races
the debugger's).

### What a test does

Per unit (a test and a front end), in its own directory under
`<build>/l7/run-<pid>-<seconds>-<n>/`:

1. each module through the front end into IR (`-g -O0 -S -emit-llvm` and
   the `cflags`, run in the module's directory with its file name, so the
   unit's name is the file's), then into an object by
   `llc -O0 -global-isel=0 -fast-isel=0 -filetype=obj
   -relocation-model=pic` and by `hod --strict` with the same options and
   a coverage log. A front end or `llc` failure is an ERROR; hod
   declining or failing is a FAIL;
2. the variants: `hod` (every module from hod) and, with `objects:
   mixed`, `hod:<file>` for each module (that one from hod, the others
   from `llc`), so both orders of a two-object program are linked;
3. per linker: `llc`'s program and each variant's, by `gcc -B<dir> -pie`
   with the linker as `ld`, the `link` flags, `-lstdc++` for C++, `-lm`;
   the program's signature checked as `irdiff` does;
   A module whose IR records a `.dwo` name (`splitDebugFilename`, a test
   with `cflags: -gsplit-dwarf`; WP-062) gets `-split-dwarf-file=<that
   name> -split-dwarf-output=<its object>.dwo` on both sides, and each
   program gets each of its objects' `.dwo` beside it under the name's
   last component, where gdb and lldb look after the unit's directory
   (the test's own directory, which has none). Such a test names only
   clang front ends: kiln++ records the absolute path of its output,
   which gdb does not look for beside the program;
4. on the first linker, the lines comparison of `hod` against `llc`;
5. per debugger with a script: the session on `llc`'s program, then on
   each variant's. Each masked transcript is written beside its program
   as `<debugger>.txt`.

### Sessions

gdb: `gdb -nx -batch`, then `-ex` for each of `set pagination off`, `set
confirm off`, `set width 0`, `set debuginfod enabled off`, `set style
enabled off`, `set print thread-events off`, then the script.
lldb: `lldb --no-lldbinit --batch`, `-o` for each of `settings set
use-color false`, `settings set target.disable-aslr false` (a container
cannot turn randomization off), `settings set auto-confirm true`, then
the script. The working directory is the program's; `LC_ALL=C`;
`DEBUGINFOD_URLS` and `TERM` removed. The transcript is standard output
then standard error. Every session goes through `irdiff::process::run`
(limits, jobserver): 5 min and 8 GiB.

A session ends **done**, **crashed** (killed by a signal, or its output
has `PLEASE submit a bug report`, `Stack dump:`, `internal-error:` or `A
fatal error internal to`) or **timed out**. A crash is its own outcome,
never a difference.

### Masking

Exactly this is masked, and nothing else (`transcript::mask`):

- the program's directory becomes `DIR`;
- `0x` and the hexadecimal digits after it become `0x?` (addresses,
  pointer values);
- the digits after `process `, `Process `, `LWP ` and `tid = ` become `?`;
- the digits of a code offset become `?`: ` + N` after a backtick with no
  ` at ` between (lldb's `` prog`main + 20 ``, `` prog`geo::Box::area()
  const + 12 ``), and `+N` closed by `>` in a word that starts with `<`
  (gdb's `<main+20>`, `<+20>`).

Values, line numbers, columns, names, frames, the number of stops and
the order of everything must be equal. hod's prologues are not `llc`'s,
so breakpoint addresses and offsets differ legitimately; columns do not.

### Outcomes

One record per session of a hod variant (and per failure before one):

| outcome | when | run |
|---|---|---|
| PASS | the masked transcripts are equal | |
| FAIL | they differ (the first differing line is the detail), hod declined or failed, hod's program did not link, hod's session timed out | fails |
| XFAIL | a FAIL, or a crash on hod's program, that an `xfail` covers | |
| XPASS | a PASS that an `xfail` covers: the directive is stale | fails |
| CRASH | the debugger crashed: on hod's program it fails the run, on `llc`'s it is an error of the tools | fails / error |
| ERROR | the front end or `llc` failed, `llc`'s program did not link, `llc`'s session timed out or does not meet the checks (hod's sessions still run; the detail says what hod's would have been), the directives do not parse | error |
| SKIP | a front end or `llc` this unit needs is missing | |

### Lines

The `(function, line)` comparison (`spec/dwarf.md` §8.4): for each
program, the pairs `(function, file, line)` of every `is_stmt` row of
`.debug_line` (`llvm-dwarfdump --debug-line`, file indices resolved per
table) whose line is not 0 and whose address range (to the next row) is
not empty, the function being the symbol (`llvm-nm -S --defined-only`,
`T`/`t`/`W`/`w` with a size) that contains the address. The pairs only
one program has are classified: **P3** (`spec/dwarf.md` §5.7) a pair
only hod's has whose line is that of its function's first row (the scope
line of a prologue `llc` does not have); **P1** a pair only `llc`'s has
whose every range holds only unconditional branches (`b`, by
`llvm-objdump -d`); else, by the IR of the unit's modules
(`permitted::IrFacts`: per defined function, the opcode, line and column
of each instruction with a `!dbg`, scopes not followed),
**load-placement** when the function's instructions at that line are
loads and `getelementptr`s with at least one load (either side), and
**is-constant** a pair only hod's has in a function that calls
`llvm.is.constant` (`spec/dwarf.md` §5.7); else **unclassified**. An
unclassified pair fails the run.

### Columns

The same rows with their columns, per function both programs have
(`lines::function_rows`, `compare_function_rows`): the *positions*
`(file, line, column)` of every row with a line other than 0 whose range
is not empty, and the *sequence* of `(file, line, column, is_stmt)` of
every such row, line 0 included, a row equal to the one before in all
four not repeated (hod's `epilogue_begin` row). Counted: functions, with
the same set of positions, with the same set but for P1 and P3
positions, with the same sequence; and the positions one side lacks (P1,
P3 or unclassified, as for pairs). Columns are reported (`debug:
columns:`), never a failure: `llc` folds instructions into others within
a line (`spec/dwarf.md` §5.7).

`debug columns [-j N] [--hod <binary>] [--dump <dir>] [-v] [<dir or
.ll>...]` runs both comparisons over every `-g` module of the paths
(default: the corpus): hod `--strict -O0 -filetype=obj
-relocation-model=pic` and `llc` of the module's LLVM major (a path
component `llvm19` or `llvm22`, else 22) with the reference options,
both with the module's `;@ llc-args:`; each object linked by `ld.lld`
into a position-independent executable with undefined symbols allowed
and no entry (`-pie --unresolved-symbols=ignore-all -z notext -e 0`),
so that every function has an address and a symbol. `--dump` writes
`llvm-objdump -d -r` of hod's object (its path replaced by `hod.o`) to
`<dir>/<index>.dis`: two dumps compared with `diff -r` show that a
change of rows left the code and relocations alone. `-j` at most 4.
Exit status 1 when a module fails, else 0.

### Permitted differences of sessions

A hod session whose masked transcript differs from `llc`'s PASSes with
the detail `permitted: return-column` when the session is lldb's and
`permitted::only_return_columns` holds: the transcripts have the same
number of lines, and every differing pair of lines is in a stop whose
`stop reason` is `step out` (after `finish`), and is either the
`frame #0:` line, equal but for the column of its ` at file:line:col`,
`llc`'s column being that of a `call` or `invoke` at that line in the
IR, or the source caret line (`^`) after such a line
(`spec/dwarf.md` §5.7 `return-column`). Nothing else is permitted.

### Results file

One line per record, tab-separated: test, front end, variant, linker
(`-`), debugger (`-`), outcome, detail.

### Summary and exit status

The tools, the notes, every result other than PASS (`-v`: all), the lines
of units with unclassified pairs (`-v`: all), the lines total, the counts
and the session count. Exit status: 1 if a record fails the run or a pair
is unclassified; else 3 for an ERROR, a crash on `llc`'s program or a
missing required tool; else 0. 2 is a usage error.

### kiln

`debug kiln` runs kiln's own debugger tests (`<kiln repo>/tests/debuginfo/
*.cpp` with `//@ expect: debugger`; kiln's `spec/harness.md`) with kiln's
directives `std:` (default `c++26`), `args:`, `gdb:`, `lldb:`,
`gdb-check:`, `lldb-check:`. Each is compiled and linked by `kiln++
--llvm-dir=<dir> -std=<std> -fno-crash-diagnostics <args> -O0 -g`, from
the kiln repository's root with the test's relative path (the checks name
`tests/debuginfo/...`), twice: with LLVM 22's installation (`llc`), and
with hod's shim of it (`spec/harness.md` "shim") under `HOD_STRICT=1` and
a coverage log. Every debugger runs the test's script on both programs
with the sessions above; a program passes when its raw output has the
checks in order, as kiln's runner requires. The masked transcripts are
compared and their first difference is reported, but only the checks
judge. Exit status 1 when a test passes with `llc` and not with hod.
kiln is read, never written.

### verify

`debug verify` builds every `-g` module (a `.ll` with `!llvm.dbg.cu`) of
the corpus (default `corpus::default_dir`) or of the paths with `hod
--strict -O0 -filetype=obj` and the module's `;@ llc-args:`, then runs
`llvm-dwarfdump --verify` of LLVM 22 and of 19 on the object. A module
whose `;@ target:` is not the host's is skipped; a decline is counted
with its reason; a module that does not verify fails (exit 1), as does
hod failing. `-j` at most 16, default 4.

### In the gate

The quick gate has no step of its own: `cargo test` runs
`xtask/tests/debug_real.rs`, whose `quick_tier_matches_llc` is the quick
tier (about 20 s on the development host at load, 120 sessions; the
result does not depend on time or load). The full gate runs every
debugger test, then `debug verify` (`debug::gate`), and fails on a
non-zero exit status of either.

### Library

`xtask::debug::{sessions, directives, transcript, build, lines,
permitted, columns, kiln, verify}`; `sessions::run(&Options) -> Report`, `verify::run`,
`kiln::run`, `debug::gate`.

### Tests

Unit tests: `debug::tests::run_options`; `directives::tests::
{reads_every_directive, refuses_broken_tests, scopes, checks_in_order,
discovery}`; `transcript::tests::{masks_what_may_differ,
first_differing_line, crash_reports}`; `lines::tests::{reads_line_tables,
reads_instructions, summary_counts_classes, compares_columns}`;
`permitted::tests::{reads_ir_and_classifies_pairs, return_columns}`;
`columns::tests::major_of_a_module`; `verify::tests::
{reads_module_directives, summary_and_status}`;
`kiln::tests::reads_kiln_directives`.

`xtask/tests/debug_real.rs` (real tools; skips without them, fails under
`HOD_REQUIRE_TOOLS=1`): `quick_tier_matches_llc` (the quick tier: no
FAIL, ERROR, XPASS or crash; no unclassified pair, at least 95% of pairs
the same) and `catches_differences_and_broken_tests` (a hod whose line 2
is line 3 FAILs; the same test PASSes with hod; a check `llc`'s session
does not meet is an ERROR; an `xfail` on a passing test is an XPASS).
