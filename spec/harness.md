# The harness

`xtask` is the project's harness: `cargo xtask <command>`. This file is the
contract of its commands: `shim`, `irdiff` (with the directive format of
its tests, "Test directives") and the gate, `ci`. Commands still to be
specified here: corpus capture (`corpus`), benchmarks (`bench`), `bisect`.

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
| `-j <N>`, `-j<N>`, `--jobs <N>` | tests run at once, at least 1; under a jobserver an upper bound ("Jobserver") | the smaller of 4 and the machine's cores; more than 16 is 16 |
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

Each process also holds a jobserver token for its lifetime when the
harness inherited a pool, and its wall-clock limit starts once the token
is held ("Jobserver").

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
linkers, mode, jobs, and the jobserver pool if there is one); a line `irdiff: note: ...` per note and `irdiff:
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
sleep, `stop`, on a pool of its own).

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
