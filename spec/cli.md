# The `hod` command line

`hod` is a drop-in for LLVM's `llc` in a debug build: the same command line,
a relocatable object as the result. What hod does not support it hands to a
real `llc` (the **fallback**) and records in a **coverage log**, so a build
that uses hod always completes and the share hod handled is a number
(`docs/ARCHITECTURE.md` "Fallback", `docs/VALIDATION.md` "Hybrid, strict,
bisect").

Crates: `crates/hod` (the facade) and `crates/hod_cli` (the `hod` binary).
"`llc`" below is LLVM 22's; its command-line grammar is LLVM's `cl` library.

## The facade

`crates/hod` is what a program links to compile a module. Its contract with
the command-line tool:

```rust
pub fn compile(input: &[u8], options: &Options) -> Result<Vec<u8>, CompileError>;

pub enum CompileError {
    Unsupported(Unsupported),   // a named construct hod does not handle
    MalformedInput(String),     // the module is not valid; the message says why
    InvalidOptions(String),     // the options contradict each other or the module
}

pub struct Unsupported { pub construct: Construct, pub function: Option<String> }
```

- `input` is one whole module, LLVM text or bitcode (see "Input"). It is a
  byte slice until the IR model exists; the IR-level entry point will
  replace it, `Options` and the error types stay.
- `Options` holds the `llc` options of the table in "Options", one field
  each, with `llc`'s defaults (`Options::default()` is `llc` with no
  options: assembly output, `-O2`, nothing else set).
- `Unsupported` is an answer, not a failure: the caller may give the module
  to LLVM instead. `function` is the function the construct was found in,
  `None` for a property of the module or the command line. It displays as
  `unsupported construct '<name>'` or
  `unsupported construct '<name>' in function '<f>'`.
- `Construct` is the vocabulary of reasons. Each has a stable name
  (`Construct::name`), which is what the coverage log records; the names
  are listed in "Reasons". The enum is `#[non_exhaustive]`: code generation
  work adds constructs.
- `compile` never panics on any `input` or `options`, and writes nothing
  but its return value.

Helpers the tool uses, also part of the facade: `InputKind::detect`,
`module_triple`, `effective_triple`, `target_supported` (see "Input" and
"Reasons").

## Options

The grammar is `llc`'s:

- An argument that starts with `-` and is longer than `-` is an option. One
  leading dash and two mean the same (`-filetype=obj`, `--filetype=obj`).
- An option that takes a value takes it joined with `=` or as the next
  argument, whatever that argument is (`-o a.o`, `-o=a.o`, `-o -o`).
- A boolean flag is on when given bare, and takes a value only joined:
  `=true`, `=TRUE`, `=True`, `=1`, `=false`, `=FALSE`, `=False`, `=0`.
- `-O` prefixes its value: `-O0`, `-O=0`, `-O 0`.
- An option given more than once: the last one wins. `-mattr` accumulates.
- `--` ends the options; every later argument is positional.
- At most one positional argument: the input file, or `-` for standard
  input. With none, the input is standard input.

| option | value | `Options` field |
|---|---|---|
| `-o` | file, or `-` for standard output | (the output; see "Behaviour") |
| `-O` | `0`, `1`, `2`, `3` | `opt_level` (default `O2`; hod generates the same code at every level) |
| `-filetype` | `asm`, `obj` | `output_kind` (default assembly, as in `llc`) |
| `-mtriple` | triple | `triple` (overrides the module's) |
| `-march` | architecture name | `arch` |
| `-mcpu` | CPU name | `cpu` |
| `-mattr` | `+a,-b,...` | `features` (empty items dropped) |
| `-relocation-model` | `static`, `pic`, `dynamic-no-pic`, `ropi`, `rwpi`, `ropi-rwpi` | `relocation_model` |
| `-code-model` | `tiny`, `small`, `kernel`, `medium`, `large` | `code_model` |
| `-function-sections` | flag | `function_sections` |
| `-data-sections` | flag | `data_sections` |
| `-fp-contract` | `fast`, `on`, `off` | `fp_contract` |
| `-debugger-tune` | `gdb`, `lldb`, `dbx`, `sce` | `debugger_tuning` |
| `-split-dwarf-file` | name | `split_dwarf_file` |
| `-split-dwarf-output` | file | `split_dwarf_output` |
| `-x86-branches-within-32B-boundaries` | flag | `x86_branches_within_32b` |
| `-time-trace` | flag | none: accepted and ignored |
| `-time-trace-granularity` | unsigned integer | none: accepted and ignored |
| `-time-trace-file` | file | none: accepted and ignored |
| `-version` | flag | prints the version (below) |
| `-help`, `-h` | flag | prints the help |

This is every option kiln's driver passes to `llc`, plus the ones the
harness needs. `-fp-contract` and `-x86-branches-within-32B-boundaries`
are permissions and tuning, not obligations: code that ignores them is
correct. The `-time-trace` options ask `llc` for a trace file of its own
phases; hod writes none, and a caller must not depend on the file existing
(kiln does not: a child that wrote no trace loses its events, nothing
else).

**What hod does not know.** The first of these on a command line is the
invocation's *rejection*:

| condition | diagnostic text | reason |
|---|---|---|
| an option not in the table (or not valid UTF-8) | `unknown option '<as written>'` | `unknown-option` |
| a value not in the table for a known option, including `-filetype=null`, `-O4`, and `help` for `-mcpu`/`-mattr`/`-march` | `unknown value '<v>' for option '<opt>'` | `unknown-option` |
| a value option as the last argument | `option '<opt>' requires a value` | `usage` |
| a second positional argument | `more than one input file ('<arg>'); at most one is accepted` | `usage` |

A rejected command line is handed to the fallback whole when one is
configured and strict mode is off ("Fallback protocol"): hod cannot know
what the rest of such a command line means, so it does not read the input
either. Otherwise it is an error: `hod: error: <text><note>`, exit status
1, where `<note>` is as in "Strict mode".

**`--version`** prints `hod version <version>` as its first line and exits
0. With a fallback configured, two more things follow: a line
`fallback: <path>` and the fallback's own `--version` output, so that a
script that looks for an LLVM version in `llc --version` finds it. (kiln's
driver never runs `llc --version`; it reads the LLVM major from the
installation directory, see `spec/harness.md` "shim".)

**`--help`** prints the options and exits 0. Neither `--version` nor
`--help` reads the input, runs the fallback or writes a coverage line.

## hod's own options

| option | environment | meaning |
|---|---|---|
| `--fallback=<llc>` | `HOD_FALLBACK_LLC` | the real `llc` ("Fallback protocol") |
| `--strict` | `HOD_STRICT=1` | no fallback ("Strict mode") |
| `--coverage-log=<file>` | `HOD_COVERAGE_LOG` | append one line per invocation ("Coverage log") |

They follow the grammar above (`--fallback <llc>`, `-strict=false`, ...).
They are never passed to the fallback. A mistake in one of them is an error
(reason `usage`, exit status 1) whatever else is configured, never a
fallback: `--strict=maybe`, `--fallback=` (empty), `--coverage-log` as the
last argument.

## Environment

- The command line wins over the environment, option by option.
- A variable that is set but empty counts as unset.
- `HOD_STRICT` is `1` (on) or `0` (off). Anything else is an error:
  `invalid value '<v>' for HOD_STRICT: expected 1 or 0`.
- `HOD_IN_FALLBACK` is set by hod for the fallback process ("Fallback
  protocol"). It is not a setting.
- `HOD_TIME_PHASES=<file>` asks for a phase report ("Phase report"). It
  has no command-line form: it is a measurement hook for the benchmark
  harness, not a setting of a build.

## Input

One module, read whole from the input file or from standard input.

- **Bitcode** if it starts with the bitcode magic `42 43 C0 DE` (`BC`
  `0xC0DE`) or the bitcode wrapper magic `DE C0 17 0B` (`0x0B17C0DE`,
  little-endian). `InputKind::detect`.
- **Text** otherwise.

For a text module, the module's own target is the value of the first line
that starts with `target triple` among the lines beginning in the first 64
KiB (`module_triple`; LLVM's printer, clang and kiln write it in the module
header). This is a header scan, in place until the IR reader exists. A
bitcode module's triple is not read.

The **stated target** of an invocation is `-mtriple` if given, else the
module's own (`effective_triple`); it may be absent.

## Behaviour

A compile invocation does this, in order. "Fails" means: one line
`hod: error: <message>` on standard error, the exit status given, and a
coverage line with outcome `error` and the reason given.

1. Parse the command line. `--version` and `--help` end here.
2. Open the coverage log, if one is configured. If it cannot be opened:
   `hod: error: cannot open coverage log '<path>': <cause>`, exit status 1,
   and nothing else happens. (A log that silently drops lines would make
   coverage numbers wrong; a build that asks for a log gets one or fails.)
3. A mistake in hod's own options or environment fails: `usage`, 1.
4. If `HOD_IN_FALLBACK` is set, fail: `fallback-loop`, 1.
5. A rejected command line ("Options") goes to the fallback, or fails with
   its reason, 1.
6. Read the input. Failure: `input-unreadable`, 1
   (`cannot read '<path>': <cause>`).
7. Call `hod::compile`.
8. On success, write the object: to the `-o` file; to standard output for
   `-o -`; without `-o`, to standard output when the input is standard
   input, else to the input's name with a final `.ll` or `.bc` removed and
   `.o` added (`llc`'s rule). Failure: `output-unwritable`, 1. Outcome
   `hod`, exit status 0.
9. On `Unsupported`, `MalformedInput` or `InvalidOptions`: run the fallback
   if one is configured and strict mode is off; otherwise fail with the
   reason of "Reasons" and the exit status of "Exit status". The diagnostic
   is the error's text followed by the note of "Strict mode".
10. Append the coverage line. If that fails:
    `hod: error: cannot write coverage log '<path>': <cause>`, and an exit
    status of 0 becomes 1.

hod writes nothing to standard output except an object requested there and
the `--version`/`--help` text, and nothing to standard error except the
diagnostics above.

## Fallback protocol

The fallback is a program with `llc`'s command line: the `llc` of the LLVM
installation the frontend was built for.

- **Arguments:** the original arguments, in order and exactly as spelled,
  minus hod's own options (and their values, when given as separate
  arguments).
- **Standard input:** if hod read the module from standard input, the
  fallback gets the same bytes on its standard input. If the input is a
  file, the fallback reads the file itself and inherits hod's standard
  input. For a rejected command line hod has read nothing, and the fallback
  inherits standard input.
- **Standard output and error** are hod's own: what the fallback writes
  goes where hod's output would have gone, unbuffered and unchanged.
- **Environment:** hod's, plus `HOD_IN_FALLBACK=1`. A hod that starts with
  that variable set is some hod's fallback (a misconfiguration: the
  fallback must be the real `llc`); it fails at once instead of starting
  another, so the mistake cannot recurse.
- **Exit status:** the fallback's exit code is hod's. A fallback ended by
  signal N gives `128 + N` and
  `hod: error: fallback '<path>' was terminated by signal <N>`.
- If the fallback cannot be started:
  `hod: error: cannot run fallback '<path>': <cause>`, exit status 1,
  outcome `error`, reason `fallback-unavailable`.
- hod writes no output file itself when it falls back.

Hybrid mode falls back for every way `compile` can decline, including
`MalformedInput` and `InvalidOptions`: the build's result is then exactly
`llc`'s (its diagnostics, its exit status), and the log line's reason says
that hod disagreed with the input. A module hod calls malformed and `llc`
accepts is a hod bug the log makes visible.

## Strict mode

With `--strict` (or `HOD_STRICT=1`) the fallback never runs, configured or
not. What would have fallen back fails, and the diagnostic ends with
` (strict mode: no fallback)`:

```
hod: error: unsupported construct 'codegen-not-implemented' (strict mode: no fallback)
```

Without strict mode and without a fallback the same failures end with
`; no fallback is configured (--fallback=<llc> or HOD_FALLBACK_LLC)`.

## Reasons

The reason is why hod did not produce the output itself. `-` when it did.

From `hod::compile` (`Construct::name`), checked in this order; the first
that applies is the reason:

| reason | condition |
|---|---|
| `assembly-output` | the output kind is not `obj` (hod has no assembly printer) |
| `bitcode-input` | the input is bitcode (hod has no bitcode reader) |
| `unsupported-target` | `-march` is given (the override is not implemented), or the stated target is not supported |
| `codegen-not-implemented` | everything else, until code generation exists |

A supported target (`target_supported`) is a triple whose first component
is `aarch64` or `x86_64`, with a later component `linux`, followed by
nothing or by exactly one environment component that starts with `gnu` or
`musl`. With no stated target, the target check is skipped.

`codegen-not-implemented` is where the per-construct reasons of
`hod_ir::classify` will appear as code generation lands; every new
`Construct` is added to this table.

From the tool:

| reason | condition | may fall back |
|---|---|---|
| `malformed-input` | `CompileError::MalformedInput` | yes |
| `invalid-options` | `CompileError::InvalidOptions` | yes |
| `unknown-option` | an unknown option or option value | yes |
| `usage` | a missing value or a second input; a mistake in hod's own options or environment | the first two; never the last |
| `input-unreadable` | the input could not be read | no |
| `output-unwritable` | the object could not be written | no |
| `fallback-unavailable` | the fallback could not be started | — |
| `fallback-loop` | `HOD_IN_FALLBACK` was set | no |

## Exit status

| status | meaning |
|---|---|
| 0 | hod wrote the output (or printed the version or the help) |
| 1 | an error in hod: usage, I/O, `malformed-input`, `invalid-options`, the coverage log, the fallback could not be started |
| 2 | the module is unsupported (`CompileError::Unsupported`) and no fallback ran |
| other | when the fallback ran: its exit status, whatever it is (including 0, 1 and 2) |

`llc` itself exits 1 for its errors. Status 2 lets a harness in strict
mode tell "hod does not support this yet" from "hod failed".

## Coverage log

When a log is configured, every compile invocation that got past opening
it appends exactly one line. The file is opened in append mode (created if
missing) and the line is written with a single `write` call, so lines from
any number of concurrent hod processes never interleave. A short write is
reported as a failure (step 10 of "Behaviour"), not completed with a second
call.

A line is eight fields separated by single tabs and ended by one newline:

| # | field | values |
|---|---|---|
| 1 | outcome | `hod`: hod wrote the output. `fallback`: the fallback ran (whatever its exit status). `error`: the invocation ended in hod without output. |
| 2 | reason | a name from "Reasons"; `-` when the outcome is `hod` |
| 3 | input kind | `text`, `bitcode`; `-` if hod did not read the input |
| 4 | input bytes | decimal; `-` if hod did not read the input |
| 5 | output path | the output as named: the `-o` value, or the derived name; `-` is standard output (and "not known" when hod did not get that far and no `-o` was given) |
| 6 | target | the stated target; before the input is read, `-mtriple` only; `-` if none |
| 7 | hod microseconds | decimal: wall time from process start to the point hod finished its own work (its output written, its error reported, or the fallback about to start) |
| 8 | fallback microseconds | decimal: wall time of the fallback process; `-` if none ran |

Fields 5 and 6 are free-form. In them a backslash, tab, newline and
carriage return are written `\\`, `\t`, `\n`, `\r`, and an empty value is
written `-`; bytes of an output path that are not UTF-8 are replaced with
U+FFFD (a module triple that is not UTF-8 is not a stated target). No field
is ever empty or contains a tab or a newline, so `split('\t')` on a line is
always eight fields.

Examples (`⇥` is a tab):

```
fallback⇥codegen-not-implemented⇥text⇥14437⇥/tmp/kiln-1/0-hello.o⇥aarch64-unknown-linux-gnu⇥225⇥10949
fallback⇥bitcode-input⇥bitcode⇥15664⇥/tmp/kiln-2/0-coro.o⇥-⇥226⇥13898
error⇥codegen-not-implemented⇥text⇥14437⇥hello.o⇥aarch64-unknown-linux-gnu⇥100⇥-
fallback⇥unknown-option⇥-⇥-⇥a.o⇥-⇥40⇥9120
```

Coverage of a build is the share of lines with outcome `hod`. The log has
no header and no per-function records yet; both arrive with code
generation (`docs/VALIDATION.md` counts functions as well as translation
units).

## Phase report

With `HOD_TIME_PHASES=<file>` (set and not empty), every compile
invocation appends a report of where its time went to `<file>`, after its
coverage line: opened in append mode, created if missing, written with a
single `write` call (a short write is a failure), like a coverage line.
`--version` and `--help` write none, and neither does an invocation that
ended because its coverage log could not be opened or written.

The report is one line per phase, in the order the phases ended, then a
line for `total`:

```
<phase>⇥<microseconds>
```

A phase's name is made of lower-case letters, digits and `-`. Its time is
decimal wall-clock microseconds from the end of the phase before it (the
first one: from process start, as in the coverage log) to its own end, so
the phases do not overlap. `total` is from process start to the moment the
report is written; it is at least the sum of the phases, and more by what
no phase covers (the coverage line).

| phase | ends when |
|---|---|
| `read` | the input has been read (command-line parsing and opening the coverage log are in it) |
| `compile` | `hod::compile` returned, with an object or an error |
| `write` | the object has been written |
| `fallback` | the fallback process ended |

A phase is in the report only if the invocation reached its end: a module
hod builds reports `read`, `compile`, `write`; one it declines in strict
mode `read`, `compile`; one it hands to the fallback `read`, `compile`,
`fallback`; a command line it hands on unread `fallback` alone. Example of
a decline:

```
read⇥212
compile⇥3
total⇥301
```

`compile` is one phase until the code generator reports its own. The names
reserved for that, which will then replace `compile` in the report, are
`ingest` (the module parsed and checked), `analysis`, `emission` (machine
code), `debug-info` and `object` (the object file built in memory). A
reader of the report takes whatever names it finds.

If the report cannot be written: `hod: error: cannot write phase report
'<path>': <cause>`, and an exit status of 0 becomes 1. The fallback
inherits the variable with the rest of the environment; `llc` ignores it.

## Tests

| section | tests |
|---|---|
| The facade, Input, Reasons | `crates/hod/src/lib.rs`: `input_kind_detection`, `reasons_in_order`, `construct_names_are_stable_and_distinct`, `module_triple_scan`, `supported_targets`, `error_text` |
| Options | `crates/hod_cli/src/args.rs`: `defaults`, `everything_kiln_passes`, `spellings`, `spellings_of_the_rest`, `rejected_command_lines`, `rejection_reasons_and_messages`, `non_utf8_arguments`, `version_and_help`; `crates/hod_cli/src/driver.rs`: `unknown_option_without_fallback`, `version_and_help`; `crates/hod_cli/tests/cli.rs`: `unknown_options`, `version_and_help` |
| hod's own options, Environment | `args.rs`: `hod_options_are_parsed_and_not_forwarded`, `mistakes_in_hod_options_are_fatal`; `driver.rs`: `environment`, `mistakes_in_hod_settings`; `cli.rs`: `settings_from_the_environment` |
| Phase report | `driver.rs`: `phase_report`; `cli.rs`: `phase_report_with_a_fallback` |
| Behaviour | `driver.rs`: `success_writes_the_object`, `options_reach_the_code_generator`, `output_names_follow_llc`, `io_errors` |
| Fallback protocol | `cli.rs`: `fallback_receives_original_arguments_and_identical_input`, `file_input`, `hod_is_not_its_own_fallback`, `real_llc_fallback_text`, `real_llc_fallback_bitcode`; `driver.rs`: `fallback_loop_is_refused` |
| Strict mode | `cli.rs`: `strict_mode_fails_with_the_construct_named`; `crates/hod_cli/tests/kiln_e2e.rs` |
| Reasons | `cli.rs`: `reasons_reach_the_log`; `driver.rs`: `malformed_input_and_invalid_options` |
| Exit status | `cli.rs`: `fallback_exit_status_propagates`; `driver.rs`: `unsupported_without_fallback`, `malformed_input_and_invalid_options`, `io_errors` |
| Coverage log | `crates/hod_cli/src/coverage.rs`: `line_format`, `free_form_fields_are_escaped`, `appends_to_an_existing_file`; `cli.rs`: `coverage_log_survives_concurrent_writers` (16 concurrent writers) |

The tests that need LLVM tools (`real_llc_*`) and kiln (`kiln_e2e.rs`) skip
with a note when the tool is missing and fail instead under
`HOD_REQUIRE_TOOLS=1`. The paths `hod::compile` cannot take yet (success,
`MalformedInput`, `InvalidOptions`) are tested in `driver.rs` with a
stand-in code generator.
