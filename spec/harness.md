# The harness

`xtask` is the project's harness: `cargo xtask <command>`. This file is the
contract of its commands: `shim`, `irdiff` (with the directive format of
its tests, "Test directives"), `corpus` and the gate, `ci`. Commands still
to be specified here: benchmarks (`bench`), `bisect`.

## shim

```
cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]
```

Builds `<out>`: a directory that kiln accepts as an LLVM installation and
in which `llc` is hod. This is how hod is plugged into kiln, and through
kiln into every C++ test suite, without changing kiln. Unix hosts only.
Arguments are `--name <value>` or `--name=<value>`. Exit status: 0; 1 when
the directory cannot be built; 2 for a usage error.

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
    llc                script: hod, with the real llc as its fallback
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
  HOD_FALLBACK_LLC='<llvm>/bin/llc' exec '<hod>' "$@"
  ```

  It sets the fallback and nothing else: `HOD_STRICT` and
  `HOD_COVERAGE_LOG` reach hod from the caller's environment, so the same
  shim serves hybrid and strict runs (`spec/cli.md`).
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

`xtask/src/shim.rs`: `layout`, `llc_runs_hod_with_the_real_llc_as_fallback`,
`wrapper_runs_kiln_with_the_shim`, `paths_with_spaces_and_quotes`,
`idempotent`, `rebuild_follows_the_request`,
`refuses_a_directory_it_did_not_make`, `never_writes_through_links`,
`rejects_unusable_requests`, `major_detection`, `argument_parsing`. These
use a fake installation and need no tools.

`crates/hod_cli/tests/kiln_e2e.rs`,
`kiln_builds_and_runs_programs_through_the_shim`: with a real kiln and the
host's LLVM, a program with iostream, a virtual call and an exception is
compiled through `kiln++-hod`, linked, run and its output checked, and the
coverage log has one line per translation unit; a coroutine program
reaches hod as bitcode; with `HOD_STRICT=1` the compile fails with hod's
diagnostic. The test needs `kiln++`: `$HOD_KILN`, else
`<build root>/target-kiln/release/kiln++` beside this checkout's target
directory. Without one it skips with a note; under `HOD_REQUIRE_TOOLS=1`
it fails.

## irdiff

```
cargo xtask irdiff [options] [<file.ll or directory>...]
```

The IR differential harness, layer L4 of `docs/VALIDATION.md`. A test is
one LLVM IR module with a `main` and `;@` directives ("Test directives").
The harness compiles it with the oracle (`llc -O0`) and with hod, links
each object with every linker of the matrix, runs the programs, and
requires the oracle's program to behave as the directives say and hod's
program to behave as the oracle's. Unix hosts only; programs are linked
and run on Linux only.

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
| `-j <N>`, `-j<N>`, `--jobs <N>` | tests run at once, at least 1 | the smaller of 4 and the machine's cores; more than 16 is 16 |
| `--llvm <major or dir>` | the oracle: an LLVM major, or an installation directory (its `bin/llc`) | the first of 22 and 19 that is installed |
| `--target <arch>` | `aarch64` or `x86_64` | this host's |
| `--linker <name>` | `bfd`, `lld`, `mold` or `wild`; repeatable; the order given does not matter | every one that is found |
| `--tier <quick or all>` | `quick`: only tests with `tier: quick` | `all` |
| `--hod <binary>` | the hod to test | build the workspace's (`cargo build --package hod_cli --bin hod`, development profile, into the build directory) |
| `--hybrid` | let hod fall back to the oracle ("Hybrid mode") | strict |
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
- **Linker driver.** `gcc` on `PATH`. Programs are linked through it so
  that the C runtime and libraries are found the way a C++ build finds
  them.
- **Linkers.** `bfd`: `ld.bfd`. `lld`: `ld.lld-N` on `PATH` or
  `/usr/lib/llvm-N/bin/ld.lld` for 22 then 19, then `ld.lld`. `mold`:
  `mold`, then `ld.mold`. `wild`: `wild`, then `ld.wild`. The matrix is
  every one of these that is found, in this order.

What is missing:

| missing | effect | under `HOD_REQUIRE_TOOLS=1` |
|---|---|---|
| the oracle | every test is SKIP, with a note | failure (exit status 3) |
| the linker driver | objects are built, nothing is linked or run ("Cross targets"), with a note | failure |
| `bfd`, `lld` or `mold` | that linker is not in the matrix, with a note | failure |
| `wild` | not in the matrix, with a note | still a note: it is optional until it is provisioned |
| a linker named with `--linker` | usage error (exit status 2) | the same |

A failure for a missing tool does not stop the run: the tests still run
with what there is.

### What a test does

For a test whose `target` and `llvm` directives admit this run (else it is
SKIP), in its own directory under the work directory:

1. **Oracle.** `llc -O0 -filetype=obj -mtriple=<triple>
   -relocation-model=<model> -o oracle.o <test>`. The triple is
   `aarch64-unknown-linux-gnu` or `x86_64-unknown-linux-gnu`; a test file
   has no `target triple` of its own, which is what lets one file serve
   both targets. The model is `pic`, or `static` for a test with
   `link: no-pie`.
2. **hod.** `hod --coverage-log=<dir>/coverage.tsv --strict <the same
   arguments> -o hod.o <test>`, with `HOD_FALLBACK_LLC`, `HOD_STRICT`,
   `HOD_COVERAGE_LOG` and `HOD_IN_FALLBACK` removed from its environment:
   hod is configured by these options alone. What hod did is read from its
   exit status and the coverage log's one line (`spec/cli.md`): status 0
   and outcome `hod` with an object written is "built"; status 2 and
   outcome `error` is "declined", with the line's reason. Anything else is
   a failure of hod: another status, a signal, the time limit, no object,
   no log, or a log that is not one well-formed line.
3. **Link and run**, once per linker, each side in the same way: `gcc
   -B<dir with that linker as ld> -pie -Wl,--fatal-warnings -o <program>
   <object>` and then `-lstdc++` and `-lm` as the test's `link` directive
   asks; `-no-pie` instead of `-pie` for `link: no-pie`. lld and mold are
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

### Outcomes

One result per test and linker; a test that is decided before any linker
runs has one result with no linker.

| outcome | meaning |
|---|---|
| PASS | hod built the object and its program behaved as the oracle's |
| FAIL | hod is wrong: it failed (step 2), its object did not link, its program ended or printed differently or ran into a limit; or the test's `unsupported` directives do not describe what hod did |
| UNSUPPORTED | hod declined, with the reason an `unsupported` directive of the test names |
| XFAIL | a FAIL where an `xfail` directive applies; the detail is the directive's reason, then the failure |
| XPASS | a PASS where an `xfail` directive applies: the directive is stale |
| ERROR | the test or a tool is wrong, which says nothing about hod: the oracle crashed, failed, timed out or wrote no object; the oracle's object did not link; the oracle's program did not behave as the directives say or ran into a limit; the program was linked by the wrong linker; the directives do not parse; the harness could not do its work |
| SKIP | the test is not valid for this target or this LLVM; or no oracle was found; or programs cannot be run here and hod built the object ("Cross targets") |

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

With `--hybrid`, hod is run with `--fallback=<the oracle's llc>` instead of
`--strict`. Status 0 with the log outcome `fallback` and an object written
is also "built": the object is linked, run and compared like any other,
and the result records that the fallback built it and why. An
`unsupported` directive that applies is satisfied by a fallback with its
reason; a fallback with another reason, or hod building the test itself,
is the FAIL above. The summary's coverage line counts both.

### Cross targets

Programs are linked and run only when the target is this host's
architecture, the host is Linux, and the driver and at least one linker
were found. Otherwise the run says why in a note, both objects are still
built, and each test has one result with no linker:

- ERROR if the oracle's side failed;
- FAIL, XFAIL or UNSUPPORTED if hod's side is decided without a program
  (it failed, or declined), with "the oracle's program was not run" in the
  detail; an `xfail` condition that names a linker does not apply;
- else SKIP: hod built an object and nothing here can run it.

Running such programs under an emulator, or on a second machine, is not
specified yet.

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

A harness run therefore ends whatever its children do, and `-j` bounds
the processes alive at once to one per job (plus a linker under its
driver).

### Work directory

Each run works in `<build directory>/irdiff/run-<pid>-<time>/`, a new
directory, so that runs at the same time do not meet. It holds one
directory per linker (`linkers/<linker>/`) with that linker under the
name `ld`, and one directory per test (`NNNN-<name>`) with `oracle.o`,
`hod.o`, `coverage.tsv` and, per linker, the programs `<linker>/oracle`
and `<linker>/hod`. A test's directory is removed when the test is done
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
linkers, mode, jobs); a line `irdiff: note: ...` per note and `irdiff:
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
itself, and the others by `declined <reason>` or `fallback <reason>`.

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

`cargo xtask ci --quick` runs, after the format check, clippy and the
tests, `irdiff --tier quick` with the default oracle. `cargo xtask ci`
runs `irdiff` over every test once per oracle, LLVM 22 and LLVM 19. The
gate fails if any of these runs would exit non-zero. Without the tools the
runs skip with their notes and the gate passes; under
`HOD_REQUIRE_TOOLS=1` a missing oracle (for the full gate: either one),
driver, or `bfd`, `lld` or `mold` fails it.

### Library

`xtask::irdiff`, for the harness commands that produce IR themselves
(`corpus`, `bisect`, fuzzing):

- `run(&Config) -> Result<Report, String>` is the command as a function:
  `Config` has one field per option, plus the build directory, the
  limits, whether tools are required, and where tools are searched. `Err`
  is exit status 2.
- `Session { toolchain, arch, host, mode, tool_limits, run_limits, work,
  keep }` is the core, and knows nothing of `tests/ir/` or of directives
  in files: `Session::run_tests(&[Test], jobs) -> Result<Vec<Vec<Record>>,
  String>` takes tests that are a name, an IR file and an `Expectations`,
  and returns each test's records in the order given.
- `Expectations` is what directives say; `directives::parse` reads it
  from a file's text, `Expectations::to_directives` writes it back, and a
  caller may equally fill it in itself.
- `Report` has the records, the notes, the missing tools,
  `exit_status()`, `summary(verbose)`, `results_text()` and
  `write_results(path)`.
- `process::run` is the bounded child process of "Child processes".

### Seed corpus

`tests/ir/` starts with one hand-written program per construct family of
`spec/ir.md` §5.4 and the module features around it. Each prints through
the C library; its expected output was derived independently of any
compiler and then confirmed with the oracle, on LLVM 22 and 19. Each is
valid for both targets and both LLVM majors, names what it covers in a
`covers` directive, and carries `unsupported: codegen-not-implemented`
until hod compiles it.

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
`single_threaded_linkers`. `xtask/src/irdiff/report.rs`:
`exit_status_mapping`, `results_lines`, `results_file_is_replaced`,
`summary_text`. `xtask/src/irdiff/engine.rs`: `differences_are_located`,
`coverage_log_reading`. `xtask/src/irdiff.rs`: `defaults`, `options`,
`usage_errors`, `collecting_tests`.

`xtask/tests/irdiff_fakes.rs` runs the whole command against fake tools
(shell scripts standing in for `llc`, `hod`, `gcc` and the linkers), so
every outcome is provoked on any Linux machine: `pass_with_every_linker`,
`fail_on_a_difference`, `fail_with_one_linker_only`, `fail_when_hod_fails`,
`unsupported_expected_and_unexpected`, `xfail_and_xpass`,
`oracle_trouble_is_an_error`, `limits_are_in_force_for_programs`, `skips`,
`the_oracle_is_selected`, `cross_target_builds_and_does_not_run`,
`the_linker_matrix`, `the_wrong_linker_is_detected`, `hybrid_mode`,
`parallel_runs_are_deterministic`, `tiers_and_paths`,
`work_directory_and_keep`, `results_file`, `command_exit_status`,
`command_environment`, `the_fakes_work`.

`xtask/tests/irdiff_real.rs` uses the real tools and skips without them
(fails under `HOD_REQUIRE_TOOLS=1`, which also requires both LLVM 22 and
19): `the_seed_corpus_is_green` (each oracle, every linker),
`the_seed_corpus_passes_in_hybrid_mode`,
`the_seed_corpus_builds_for_the_other_target`,
`a_real_miscompile_is_detected` (a stand-in hod that runs the real `llc`
on a rewritten module: a wrong result, wrong output, a hang and a missing
`main` are each a FAIL with every linker, and the unmodified module is a
PASS), `limits_hold_for_real_programs`.

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
without spaces, a colon, and the value. One space after the colon, if
there is one, is not part of the value. A file with no directive is a test
of a program that prints nothing and exits with status 0, valid
everywhere.

| key | value | times | default |
|---|---|---|---|
| `stdout` | one line of the expected standard output | any | no output |
| `stdin` | one line of the program's standard input | any | no input |
| `exit` | `N` (0 to 255): the exit status. `signal N` (1 to 64): terminated by that signal | once | `0` |
| `args` | the program's arguments, separated by spaces or tabs | once | none |
| `target` | the architectures the test is valid for: `aarch64`, `x86_64` | once | both |
| `llvm` | the LLVM majors the test is valid for: `N` (exactly) or `N+` (that or later); any one must hold | once | all |
| `link` | what the link needs: `m` (the math library), `c++` (the C++ runtime), `no-pie` (a position-dependent program, compiled with the static relocation model), `pie` (the default, spelled out) | once | the C library, PIE |
| `tier` | `quick` or `full` | once | `full` |
| `covers` | the spec section the test exercises, free text | once | none |
| `unsupported` | the coverage-log reason hod declines this test with (lower-case letters, digits, `-`) | any | hod must compile it |
| `xfail` | why hod is known to fail this test, free text; never empty | any | it must pass |

- `stdout` and `stdin` lines are taken verbatim, in order, each followed
  by a newline. The whole expected output is their concatenation; the
  program's output must equal it byte for byte. Output that does not end
  in a newline, and bytes that are not valid UTF-8, cannot be written.
- List values (`args`, `target`, `llvm`, `link`) are words separated by
  spaces or tabs. An argument cannot contain white space.
- For the other keys, white space around the value is dropped.

**Conditions** narrow an `unsupported` or `xfail` directive to part of the
matrix. A condition is a target (`aarch64`, `x86_64`), a linker (`bfd`,
`lld`, `mold`, `wild`) or an oracle major (`llvm19`, `llvm22`, ...). All
conditions of a directive must hold, and there is at most one of each
kind. Several directives of the same key are alternatives; the first one
in the file whose conditions hold is the one that applies. `unsupported`
cannot depend on a linker, because hod declines before anything is linked.

A test that is unsupported today, with input, arguments and a status (the
directives of `tests/ir/main_args.ll`):

```llvm
;@ covers: spec/ir.md §5.4 call, memory (the process interface every other test relies on)
;@ tier: quick
;@ args: alpha beta
;@ stdin: one line of input
;@ exit: 3
;@ unsupported: codegen-not-implemented
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
- a condition list on a key other than `unsupported` and `xfail`;
- a second directive of a key that may appear once;
- an empty value, for every key but `stdout` and `stdin`;
- `exit` that is not `N` in 0 to 255 or `signal N` in 1 to 64;
- an unknown word in `target`, `link` or `tier`, or the same word twice;
  `link` with both `pie` and `no-pie`;
- an `llvm` item that is not `N` or `N+` with `N` a number without a
  leading zero;
- an `unsupported` reason with other characters than lower-case letters,
  digits and `-`;
- an unknown condition, an empty one, a second condition of the same
  kind, or a linker condition on `unsupported`.

### Canonical form

`Expectations::to_directives` writes directives in this order, leaving
out everything that is the default: `covers`, `tier`, `target`, `llvm`,
`link` (libraries, then `no-pie`), each `unsupported`, each `xfail`,
`args`, each `stdin`, `exit`, each `stdout`. Reading what it writes gives
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
limit: on the reference machine that is 5760 entries in 164 MiB, made in
about eleven minutes at `-j 3` (`docs/status/corpus.md` has the numbers
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
  arguments and its standard input, then runs the real `llc` on them. So
  the module kept is byte for byte what kiln gave its backend, and the
  program is built by the real tools exactly as without the capture.
  `TMPDIR` and `XDG_CACHE_HOME` point into the work directory; `LC_ALL=C`.
- The program is then run in an empty directory with the header's
  `run-args:` and `run-stdin:` (or `run-stdin-file:`), within the run
  limits (10 s, 1 GiB; the compile has 120 s and 8 GiB, as in kiln's
  harness). It must exit with the header's `exit:` (default 0) and, if the
  header has `stdout:` lines or a `stdout-file:`, write exactly that.

The entry's expectations are what the program **did**: its exit status and
its standard output. Its link is `link: c++ m`, with `no-pie` when llc was
given `-relocation-model=static`.

### The gcc-torture source

The tests are the `.c` files of `gcc.c-torture/execute`, of its `ieee/`
and of its `builtins/` (each of which is built with a second file and is
therefore skipped), in name order. Each is compiled in the suite's
directory:

```
clang-<N> -O0 -S -emit-llvm -w <flags> -o <module> <test>
llc-<N> -O0 -relocation-model=pic -filetype=obj -o <object> <module>
gcc -pie -o <program> <object> -lm
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
;@ link: [c++] m [no-pie]
;@ args: ...            (if any)
;@ stdin: ...           (one per line of input)
;@ exit: <status>       (if not 0)
;@ stdout: ...          (one per line of output)
; corpus entry written by `cargo xtask corpus capture`; do not edit
; source: <source> <test>
; producer-args: <the producer's arguments, the machine's directory as <cwd>>
; llc-args: <what the producer's driver gave llc, without -o <object> and the input>
<the module, unchanged>
```

The directives are in canonical form ("Test directives"). `covers` names
the test the entry was captured from, not a part of a specification. An
entry is for the architecture it was captured on and for one LLVM major:
the module carries its own triple and data layout.

Only what the directive format can say is an entry. `llc`'s arguments are
recorded but not replayed: `irdiff` builds an entry with `-O0
-filetype=obj -relocation-model=pic|static` and the module's triple. An
entry whose producer gave `llc` more than that (kiln passes
`-debugger-tune=gdb` for `-g` tests) is still an entry if it passes
admission; the manifest and the census report say which those are.

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
| `multi-file` | is more than one translation unit built by one command (`module:`, `unit:`, `header-unit:`, `pch:`, `two-step:`; a torture test of `builtins/`) |
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
| `multi-file` | kiln++ ran llc more than once |
| `opt-first-lto`, `-sanitizer`, `-coverage`, `-optimized`, `-coroutine`, `-always-inline` | llc was given bitcode (or, for LTO, nothing): kiln ran `opt` first. Which one is told from the arguments, else from the source text (a coroutine keyword or header, searched through the test's own `#include "..."` files), else it is `always-inline`, the remaining cause. A guess, good enough to count. |
| `llc-args-unexpected` | llc's command line was not `... -o <object> -` |
| `module-not-text` | the module is not UTF-8 text, has a NUL, or has a line that starts with `;@` |
| `program-fails` | the program was killed, timed out, or (torture) exited with a non-zero status |
| `expectation-mismatch` | (kiln) the program did not do what the header says |
| `stdout-not-expressible`, `stdin-not-expressible`, `args-not-expressible` | the directive format cannot hold the output (not lines of UTF-8, more than 256 KiB), the input, or an argument |
| `not-reproducible` | the entry did not pass admission |
| `capture-broken` | the capture itself failed (a tool could not be run) |

### Admission

Before anything enters the corpus, the group's candidates are run by
`irdiff`'s own engine ("What a test does") in strict mode, with a stand-in
for hod that declines every module: the real `llc` builds the entry file
as `irdiff` will build it, the program is linked by every linker found and
must do what the directives say. A candidate is admitted when that holds
for every linker; otherwise it is `not-reproducible` with the first
failure as its detail. So every entry is, by construction, a test whose
oracle side passes with every linker, built from the entry file alone.

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
  `producer_args`, `llc_args`, `sha256` and `bytes` of the entry file,
  `key`.
- `SKIPPED.tsv`, one row per skipped test: `source`, `llvm`, `test`,
  `reason`, `detail`, `key`.
- `CAPTURE.tsv`, one row per group: what the last capture that made the
  whole group anew (nothing reused, no `--limit`) did, how long it took
  and with how many jobs. It is the one file of the corpus that is not a
  function of the inputs.

### Incremental

The `key` of a test is the SHA-256 of: a format tag (`hod-corpus-1`,
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
  not repeat.
- **Census**: per group, each table below as `occurrences (modules with at
  least one)`, rows sorted by total count, long tables cut with a line
  that says how much was cut; totals; and median, 90th percentile and
  maximum of module size (bytes, lines), definitions, declarations, global
  variables, basic blocks, instructions and metadata nodes per module, and
  blocks and instructions per function.
- **Tests that are not in the corpus**: the kiln tests that fail with
  plain kiln++ and the real `llc` (the reasons `compile-fails`,
  `llc-fails`, `link-fails`, `program-fails`, `expectation-mismatch`,
  `no-module`, `not-reproducible`), then every skipped test by source and
  reason.

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
  every run-time skip reason, incremental and forced captures, `--limit`,
  `--retry-skipped`, two LLVM majors, missing sources and tools, `verify`
  on a sound and on a damaged corpus, and the report, with a kiln++, a
  clang, an `llc`, a driver and linkers that are shell scripts.
- `xtask/tests/corpus_real.rs`: a few C programs laid out as the torture
  suite, captured with the real clang and `llc` for each installed major,
  verified through `irdiff --hybrid` with the workspace's hod, and
  counted. Skips without the tools; fails under `HOD_REQUIRE_TOOLS=1`. It
  needs neither kiln nor the GCC suite.
