# The harness

`xtask` is the project's harness: `cargo xtask <command>`. This file is the
contract of its commands: `shim`, `irdiff` (with the directive format of
its tests, "Test directives"), `bench` and the gate, `ci`. Commands still
to be specified here: corpus capture (`corpus`), `bisect`.

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
  replaced by blanks.
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
`irdiff::process::run` because that one polls for its child's end. When
the harness's jobserver client lands (WP-024), a measured child takes one
token before the clock starts, at the place the function marks.

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
