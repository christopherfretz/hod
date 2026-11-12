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
pub fn compile_with(input: &[u8], options: &Options, settings: &Settings, report: &mut Report)
    -> Result<Vec<u8>, CompileError>;
pub fn generate(module: &hod_ir::Module, bodies: &[hod_ir::Body], options: &Options)
    -> Result<Vec<u8>, CompileError>;
pub fn generate_with(module: &hod_ir::Module, bodies: &[hod_ir::Body], options: &Options,
    threads: &Threads, report: &mut Report) -> Result<Vec<u8>, CompileError>;

pub enum CompileError {
    Unsupported(Unsupported),   // a named construct hod does not handle
    MalformedInput(String),     // the module is not valid; the message says why
    InvalidOptions(String),     // the options contradict each other or the module
    Internal(String),           // a bug of hod's: an invariant of code generation broke
}

pub struct Unsupported {
    pub construct: Construct,
    pub function: Option<String>,
    pub detail: Option<String>, // what exactly, when the name is a family (a target feature)
}

pub struct Threads {
    pub max: usize,                               // threads in all, the caller's included
    pub tokens: Option<Arc<dyn ThreadTokens>>,    // where every thread but the first comes from
    pub per_thread: usize,                        // instructions each thread needs; 0: PARALLEL_MIN_INSTS
}
pub trait ThreadTokens: Send + Sync + Debug {
    fn take(&self, wanted: usize) -> usize;       // at most `wanted`, never waiting
    fn give_back(&self, count: usize);
}
```

- `input` is one whole module, LLVM text or bitcode (see "Input"), read
  by `hod_ll` or `hod_bc` ("The front half"). `compile` is `compile_with`
  with `Settings::default()`: bitcode read natively, the first reason.
- `Settings` is how the front half works, not an `llc` option:
  `bitcode` (`BitcodeReading::Native`, or `Decline` for a caller that
  converts bitcode itself, which then gets `bitcode-input`), `census`
  (every reason, "Reasons"), `support` (the `hod_ir::Support` to classify
  with; `None` is `Support::shipped` of the module's target; for tests and
  measurements), `threads` (how many code generation may use, "Threads";
  the default is the calling thread alone).
- `Report` is what `compile_with` tells besides its answer: the stated
  target as it came to know it (`target`), the phases that ended with the
  moment each ended (`phases`, "Phase report"), and with `census` every
  reason found, each once, in the order met, the answer's first
  (`reasons`).
- `generate` is **the seam**: the back half, given the module `classify`
  accepted whole and its bodies, every one classified and verified, in the
  module's order ("The back half"). `generate` is `generate_with` on one
  thread with a report nobody reads.
- `Options` holds the `llc` options of the table in "Options", one field
  each, with `llc`'s defaults (`Options::default()` is `llc` with no
  options: assembly output, `-O2`, nothing else set).
- `Unsupported` is an answer, not a failure: the caller may give the module
  to LLVM instead. `function` is the function the construct was found in,
  `None` for a property of the module or the command line. It displays as
  `unsupported construct '<name>'` or
  `unsupported construct '<name>' in function '<f>'`, followed by
  `: <detail>` when there is one.
- `Internal` is a failure, not an answer: hod broke one of its own
  invariants (a code generator's `CgError::Internal`, an object that
  cannot be written for a reason other than a limit). It displays as
  `internal error: <message>`, the message naming the function
  (`function '<f>': ...`) or `module emission: ...`. It is never a
  decline: a harness counts it as a bug of hod's, not a gap.
- `Construct` is the vocabulary of reasons. Each has a stable name
  (`Construct::name`), which is what the coverage log records; the names
  are listed in "Reasons". The enum is `#[non_exhaustive]`: code generation
  work adds constructs. `Construct::Named` carries the names that come
  from `hod_ir::classify` and the readers, which are one namespace with
  the facade's own (`hod_ir`'s `CONSTRUCT_NAMES`).
- `compile` never panics on any `input` or `options`, and writes nothing
  but its return value.

Helpers the tool uses, also part of the facade: `InputKind::detect`,
`module_triple`, `effective_triple`, `target_supported` (see "Input" and
"Reasons").

## The back half

`generate_with`, in order; the first refusal is the answer and no object
is produced, partial or otherwise:

1. An x86-64 module declines with `codegen-not-implemented` (its code
   generator is planned, `spec/codegen.md` §5.6).
2. The options code generation cannot honour: `-relocation-model=ropi`,
   `rwpi` or `ropi-rwpi` is `option:relocation-model`, `-split-dwarf-output`
   is `option:split-dwarf-output`. A relocation model left unset is
   `static`, `llc`'s default for these targets. The optimization level is
   accepted and ignored: hod generates the same code at every `-O`.
3. Each `-mattr` item, then each function's `target-features` attribute,
   is judged by `hod_a64::features::check` (`spec/codegen.md` §5.5.2):
   a refusal is `attr:target-feature-<class>`, with the item as the
   detail, in the function for an attribute and of the module for
   `-mattr`.
4. Every body is compiled (`hod_cg::compile_function` with `hod_a64`),
   on the threads of "Threads", each with its own scratch state, into
   chunks of whole functions in module order. A `CgError::Unsupported`
   is that name, in the function (`a64:...`, `abi:...`, `limit:...`,
   `call:...`, ...); a `CgError::Internal` is `CompileError::Internal`.
   When several functions fail, the answer is the one first in module
   order, whatever thread met it first.
5. `hod_cg::emit_module` lays out the module from the chunks in module
   order, and `hod_obj::elf::to_vec` writes the object in memory: their
   `Unsupported` is a module refusal; `hod_obj`'s `NulInName` is
   `object:nul-in-name`, `TooLarge` is `limit:object`, anything else
   `Internal`.

The object is the same bytes for every thread count and every run: the
chunks are merged in module order, and `emit_module`'s output does not
depend on how the functions were grouped into chunks.

## Threads

A build system already runs one compiler per core, so hod takes no thread
it is not given (`docs/ARCHITECTURE.md`, "Parallelism"):

- With `HOD_THREADS=<n>` (a positive integer; anything else is the error
  `invalid value '<v>' for HOD_THREADS: expected a positive integer`,
  `usage`), hod may use `n` threads.
- Otherwise, under a GNU make jobserver (`MAKEFLAGS` holding
  `--jobserver-auth=fifo:<path>`, make 4.4's form; the last such word
  wins), up to 16 threads: hod runs on its implicit token and, for a
  module large enough, takes as many more tokens as are idle at that
  moment, never waiting (the fifo is opened without blocking, and only
  then), and gives each back when code generation ends, also when it
  refuses. The older descriptor forms (`--jobserver-fds=R,W`,
  `--jobserver-auth=R,W`) are not used.
- Otherwise one thread.

Threads are worth it only above a break-even: a module gets one thread per
`PARALLEL_MIN_INSTS` (4096) instructions of its bodies, at most one per
function and at most what it may use; below 2 it asks for no token.
Measured in `spec/landed/cli/WP-043.md`: a thread costs 20-60 µs to start
and join, and code generation about 90 ns per instruction, so two threads
beat one from about 2-4k instructions and eight beat four from about 30k.

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
| `--bitcode-via=<llvm-dis>` | `HOD_BITCODE_VIA` | the `llvm-dis` that turns a bitcode input into text, or `native`, the default: hod's own reader ("Bitcode conversion") |

They follow the grammar above (`--fallback <llc>`, `-strict=false`, ...).
They are never passed to the fallback. A mistake in one of them is an error
(reason `usage`, exit status 1) whatever else is configured, never a
fallback: `--strict=maybe`, `--fallback=` (empty), `--bitcode-via=`
(empty), `--coverage-log` as the last argument.

The four variables of this table are hod's **settings**.

## Environment

- The command line wins over the environment, option by option.
- A variable that is set but empty counts as unset.
- `HOD_STRICT` is `1` (on) or `0` (off). Anything else is an error:
  `invalid value '<v>' for HOD_STRICT: expected 1 or 0`.
- `HOD_IN_FALLBACK` is set by hod for the processes it starts: the
  fallback ("Fallback protocol") and the bitcode converter ("Bitcode
  conversion"). It is not a setting.
- A setting cannot be switched off from the command line once the
  environment has it, except strict mode (`--strict=false`): an empty
  `--fallback=` or `--bitcode-via=` is a mistake, not "none".
- `HOD_CENSUS` is `1` or `0`, read like `HOD_STRICT`: with `1` hod finds
  every reason a module is declined for, not only the first ("Reasons"),
  and the coverage log lists them (field 11). The answer is the same; it
  costs reading every body. It has no command-line form: it is for
  `cargo xtask corpus reasons` and for whoever asks what to implement
  next.
- `HOD_THREADS` and `MAKEFLAGS` set the threads of code generation
  ("Threads"). They have no command-line form.
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
header). This scan decides the target checks that come before reading
(`-march`, an unsupported triple); the reader then reads the module's
target itself. A bitcode module's triple is known once `hod_bc` has read
its module level: the triple of the target it made of it (normalized:
`aarch64-unknown-linux-gnu` for any spelling LLVM accepts), and once a
module has been converted ("Bitcode conversion"), the text's.

The **stated target** of an invocation is `-mtriple` if given, else the
module's own (`effective_triple`); it may be absent.

## Bitcode conversion

hod reads bitcode itself, with `hod_bc` (`docs/design/0001-input.md`):
that is the default, and `--bitcode-via=native` (or `HOD_BITCODE_VIA=native`)
says so explicitly. A **converter** can stand in instead: LLVM's
`llvm-dis`, named with `--bitcode-via=<llvm-dis>` or `HOD_BITCODE_VIA`. The
text it writes then takes the place of the input for `hod::compile`, and
for nothing else. (A converter whose path is literally `native` is named
`./native`.)

Native is the default because over the whole captured corpus every
module gets the same outcome, the same first reason and the same census
(as a set: its order follows each reader's numbering of symbols) through
`hod_bc` as through `llvm-dis` and text (`docs/status/reasons.md`,
"The routes agree"); a difference is a bug in a reader, not a choice.
Debug information is refused by name (`debug:*`) on both routes alike.

- **When.** Only when a converter is configured: hod then asks
  `hod::compile` with `BitcodeReading::Decline`, and converts when it
  declined the input with `bitcode-input`. Whatever is checked before
  the input's kind keeps its reason without a conversion (today
  `assembly-output`), and so does a rejected command line. Strict mode does
  not turn the conversion off: the converter is not the fallback.
- **The converter** must be the `llvm-dis` of the LLVM that wrote the
  bitcode, which is the LLVM the frontend was built for. An older
  `llvm-dis` does not read newer bitcode in general (it fails on the first
  attribute or record it does not know, sometimes on none); hod does not
  check versions, it reports what the converter does. The shim passes its
  installation's own (`spec/harness.md` "shim").
- **How it is run.** `<llvm-dis> -o - -`: bitcode on its standard input,
  text on its standard output, both pipes, served at the same time, so a
  module of any size passes and nothing touches the file system. It makes
  no difference whether hod read the module from a file or from its own
  standard input. A converter that exits or closes its input before
  reading everything is not an error in itself (and does not kill hod with
  `SIGPIPE`): its exit status and output decide.
- **Its environment** is hod's without hod's settings ("hod's own
  options"), plus `HOD_IN_FALLBACK=1`. A hod standing where the converter
  should be (a link to hod, or `llvm-dis` taken from a shim directory that
  replaces it) therefore fails at once with `fallback-loop` and cannot
  start a fallback or a converter of its own; the hod that started it
  reports `bitcode-converter-failed`.
- **Its standard error** is read to the end and never shown as is. The
  first 4096 bytes are kept; a diagnostic quotes the first line among them
  that is not blank: trimmed, control characters replaced by spaces, at
  most 200 characters, `...` appended when cut.
- **The result** is judged in this order; the first that applies is the
  reason ("Reasons") and `<p>` is the converter as named:

  | condition | reason | diagnostic |
  |---|---|---|
  | it could not be started, or its pipes failed | `bitcode-converter-unavailable` | `cannot run bitcode converter '<p>': <cause>` |
  | a signal ended it | `bitcode-converter-killed` | `bitcode converter '<p>' was terminated by signal <N>` (`... ended without an exit status` if the host reports neither a status nor a signal) |
  | it exited with a status other than 0 | `bitcode-converter-failed` | `bitcode converter '<p>' failed (exit status <N>)`, then `: <line>` if it wrote to standard error |
  | it exited 0 and wrote to standard error | `bitcode-converter-stderr` | `bitcode converter '<p>' wrote to standard error: <line>` |
  | it exited 0 and its standard output is empty | `bitcode-converter-output` | `bitcode converter '<p>' wrote nothing` |
  | it exited 0 and its standard output does not start with `; ModuleID = ` or does not end with a newline | `bitcode-converter-output` | `bitcode converter '<p>' wrote something that is not one whole module of LLVM text` |

  A warning on standard error with status 0 is treated as a failure
  because hod cannot tell what the converter left out or changed (LLVM
  drops debug info it finds invalid with exactly such a warning); `llc`
  given the original bitcode is the safe answer. The output rule is
  `llvm-dis`'s own shape for a module read from standard input
  (`; ModuleID = '<stdin>'` first, a newline last, LLVM 19 and 22); it
  catches a truncated or foreign output, not an invalid module, which is
  the text reader's to find.
- **Otherwise the conversion succeeded.** hod calls `compile` again with
  the text, and everything after is as for a text input: the reasons, the
  fallback, strict mode, the exit status. The stated target is now known.
- **When it fails,** hod has declined the module: with a fallback and
  strict mode off, the fallback runs; otherwise the invocation fails with
  the reason and diagnostic above, the note of "Strict mode" appended,
  exit status 1.
- **The fallback never sees the conversion.** After a conversion,
  successful or failed, it gets what it would have got without one: the
  original arguments, and the original bitcode ("Fallback protocol").
- **Bounds.** The converter is always waited for; hod leaves no process
  behind. hod sets no time limit: like the fallback, the converter is
  LLVM's own tool working on the build's own module, and a build that
  hangs in it is interrupted by whoever runs the build. hod holds the
  bitcode and the text at once, each once (plus at most 4096 bytes of the
  converter's standard error).

Converting costs a process and a print-and-parse round trip: see
`spec/landed/cli/WP-020.md` for the measurements.

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
7. Call `hod::compile`. If it declines with `bitcode-input` and a
   converter is configured, convert ("Bitcode conversion"): on success call
   `hod::compile` again with the text and go on with its answer; on failure
   run the fallback if one is configured and strict mode is off, otherwise
   fail with the conversion's reason, 1.
8. On success, write the object: to the `-o` file; to standard output for
   `-o -`; without `-o`, to standard output when the input is standard
   input, else to the input's name with a final `.ll` or `.bc` removed and
   `.o` added (`llc`'s rule). Failure: `output-unwritable`, 1. Outcome
   `hod`, exit status 0.
9. On `Unsupported`, `MalformedInput`, `InvalidOptions` or `Internal`: run the fallback
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
  fallback gets the same bytes on its standard input: the bytes hod read,
  bitcode included, never the text of a conversion. If the input is a
  file, the fallback reads the file itself and inherits hod's standard
  input. For a rejected command line hod has read nothing, and the fallback
  inherits standard input.
- **Standard output and error** are hod's own: what the fallback writes
  goes where hod's output would have gone, unbuffered and unchanged.
- **Environment:** hod's, plus `HOD_IN_FALLBACK=1`. A hod that starts with
  that variable set was started by another hod, as its fallback or as its
  bitcode converter (a misconfiguration: those must be the real `llc` and
  the real `llvm-dis`); it fails at once instead of starting another, so
  the mistake cannot recurse:
  `hod: error: this hod was started by another hod, as its fallback or its
  bitcode converter (HOD_IN_FALLBACK is set); those must be the real llc
  and the real llvm-dis` (one line).
- **Exit status:** the fallback's exit code is hod's. A fallback ended by
  signal N gives `128 + N` and
  `hod: error: fallback '<path>' was terminated by signal <N>`. The
  coverage log records which it was (field 10).
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
| `bitcode-input` | the input is bitcode and the caller asked for `BitcodeReading::Decline` (the tool does only when it then converts: see "Bitcode conversion") |
| `unsupported-target` | `-march` is given (the override is not implemented), or the stated target known before reading is not supported (`target_supported`), or `-mtriple` is not one `hod_ir::TargetSpec` describes |
| a reader's reason | reading the module level failed: the table below |
| `unsupported-target` | `-mtriple` names another target than the module's (retargeting is not implemented) |
| a `classify` name | `hod_ir::classify`'s module pass refused the module (`module:pic`, `global:function-definition`, `debug:module-flags`, ...; `docs/status/support.md`) |
| a body's reason | for each body in the module's order: the reader's error (the table below), the body reader's own refusal (`call:forwarded-varargs`, ...), `classify`'s body pass, then `Body::verify` (`malformed-input`) |
| `codegen-not-implemented` | the module reached `generate` and its target is x86-64 |
| `option:relocation-model`, `option:split-dwarf-output` | an option code generation cannot honour ("The back half") |
| `attr:target-feature-<class>` | an `-mattr` item or a function's `target-features` item hod's code is not correct under (`spec/codegen.md` §5.5.2); the item is the detail |
| a code generator's name | `hod_cg`/`hod_a64` refused a function (`a64:`, `abi:`, `limit:`, `call:`, ...), in the function; or `emit_module` refused the module |
| `object:nul-in-name`, `limit:object` | the object cannot be written: a symbol or section name with a NUL, or more than ELF can hold |

A supported target (`target_supported`) is a triple whose first component
is `aarch64` or `x86_64`, with a later component `linux`, followed by
nothing or by exactly one environment component that starts with `gnu` or
`musl`. With no stated target the check is skipped and the reader decides:
a module with neither a triple nor `-mtriple` is `unsupported-target`, and
so is one whose triple `hod_ir::TargetSpec` does not describe (today
`*-linux-musl`) or whose data layout is not its target's.

**The readers' errors** (`hod_ll::LlError`, `hod_bc::BcError`), one
namespace with the rest:

| error | reason |
|---|---|
| `LlErrorKind::Target`, `BcErrorKind::Target`, `Ir(UnsupportedTarget)`, `Ir(DataLayoutMismatch)` | `unsupported-target` |
| `LlErrorKind::Limit`, `BcErrorKind::Limit`, `Ir(TableFull)`, `Ir(TypeTooLarge)` | `limit:reader` (a limit of hod's; LLVM may take the module) |
| `BcErrorKind::Unsupported(<n>)` | `bitcode:<n>` (a record or block `hod_bc` does not read) |
| a body the reader delivers without reading, with names | the first name (`ParsedBody::unsupported`, `ReadBody::unsupported`), in the function |
| any other kind | `malformed-input`; the message is `line <l>:<c>: <text>` for text, `bit <n> (byte <m>): <text>` for bitcode |

`classify`'s `RejectionKind::Unsupported` is `Unsupported` with the
construct's name; `RejectionKind::Invalid` is `malformed-input` with the
name as its message. A refusal of the body pass, of the body reader or of
`verify` names its function; the module pass's does not.

**The first refusal, or every reason.** By default hod stops at the first
refusal: what needs no reading first, then the module level (read, then
the module pass) before any body, then the bodies streamed one at a time,
read and judged and dropped. A declined module goes to `llc` next, so a
decline must be cheap: its cost is measured in `docs/status/perf.md`.
Where the cheap reason and the most useful one differ (a module refused
at `module:pic` also has bodies `cc:ccc` refuses), **the cheap one is the
answer**; `HOD_CENSUS=1` (`Settings::census`) finds every reason — every
module-pass and body-pass construct the census of `hod_ir::classify`
names, the readers' refusals, `verify`'s — and the answer stays the first
one. A body is classified and verified only while no reason has been found:
the census of the rest is `classify`'s census, not a verdict.

**Verify.** Every body that reaches `generate` has passed `Body::verify`
(`spec/ir.md` §5.8): code generation may assume it. It costs about 27 ns
per instruction (`spec/landed/ir/WP-009.md`), against 220 for reading
bitcode and 450 for text, and nothing for a module refused before its
bodies, which is every module with a definition until code generation
lands. A body `verify` rejects is `malformed-input` (`function '<f>':
<error>`).

From the tool:

| reason | condition | may fall back |
|---|---|---|
| `malformed-input` | `CompileError::MalformedInput` | yes |
| `invalid-options` | `CompileError::InvalidOptions` | yes |
| `internal-error` | `CompileError::Internal`: a bug of hod's, not a decline | yes |
| `unknown-option` | an unknown option or option value | yes |
| `usage` | a missing value or a second input; a mistake in hod's own options or environment | the first two; never the last |
| `input-unreadable` | the input could not be read | no |
| `output-unwritable` | the object could not be written | no |
| `fallback-unavailable` | the fallback could not be started | — |
| `fallback-loop` | `HOD_IN_FALLBACK` was set | no |
| `bitcode-converter-unavailable` | the converter could not be started | yes |
| `bitcode-converter-killed` | a signal ended the converter | yes |
| `bitcode-converter-failed` | the converter exited with a status other than 0 | yes |
| `bitcode-converter-stderr` | the converter exited 0 and wrote to standard error | yes |
| `bitcode-converter-output` | the converter exited 0, silently, and wrote nothing or not one whole module of text | yes |

The `bitcode-converter-*` conditions are defined exactly in "Bitcode
conversion". The tool asks for `bitcode-input` only to convert, so it no
longer reaches a log from this hod; a converted module carries the reason
the text got.

## Exit status

| status | meaning |
|---|---|
| 0 | hod wrote the output (or printed the version or the help) |
| 1 | an error in hod: usage, I/O, `malformed-input`, `invalid-options`, `internal-error`, the coverage log, the fallback could not be started, the bitcode conversion failed (`bitcode-converter-*`) and no fallback ran |
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

A line is eleven fields separated by single tabs and ended by one newline:

| # | field | values |
|---|---|---|
| 1 | outcome | `hod`: hod wrote the output. `fallback`: the fallback ran (whatever its exit status). `error`: the invocation ended in hod without output. |
| 2 | reason | a name from "Reasons"; `-` when the outcome is `hod` |
| 3 | input kind | `text`, `bitcode`: what hod was given (a converted module stays `bitcode`); `-` if hod did not read the input |
| 4 | input bytes | decimal, of what hod was given (the bitcode's, not the converted text's); `-` if hod did not read the input |
| 5 | output path | the output as named: the `-o` value, or the derived name; `-` is standard output (and "not known" when hod did not get that far and no `-o` was given) |
| 6 | target | the stated target; before the input is read, `-mtriple` only; `-` if none. For bitcode the module's own triple is known once its module level was read ("Input") or it was converted |
| 7 | hod microseconds | decimal: wall time from process start to the point hod finished its own work (its output written, its error reported, or the fallback about to start). It includes the conversion (field 9) |
| 8 | fallback microseconds | decimal: wall time of the fallback process; `-` if none ran |
| 9 | conversion microseconds | decimal: wall time of the bitcode conversion, from starting the converter to having judged its result, whether it succeeded or failed; `-` if none was attempted |
| 10 | fallback status | how the fallback ended: its exit code in decimal (`0` to `255`); `signal:<N>` if signal N ended it (hod then exits `128 + N`); `unknown` if the host reported neither (hod then exits 1); `-` if none ran, including when it could not be started (`fallback-unavailable`) |
| 11 | reasons | with `HOD_CENSUS=1`, every reason of `hod::compile`'s last answer, each once, separated by `,`, the first being field 2's when that came from `compile`; `-` without a census or when there is none |

Fields 2, 5, 6 and 11 are free-form. In them a backslash, tab, newline and
carriage return are written `\\`, `\t`, `\n`, `\r`, and an empty value is
written `-`; bytes of an output path that are not UTF-8 are replaced with
U+FFFD (a module triple that is not UTF-8 is not a stated target); in field
11 a `,` inside a name is written `\,`. No field is ever empty or contains
a tab or a newline, so `split('\t')` on a line is always eleven fields.

**The format grows only at the end.** Fields are never reordered, removed
or given another meaning; a later version of hod may append fields. A
reader takes the fields it knows by position, accepts a line with more,
and rejects a line with fewer than it knows. Fields 9 and 10 were added
after the first eight, field 11 after ten: a reader written for eight
(`cargo xtask irdiff` and `cargo xtask bench` are such readers, and
accept eight or more) reads today's lines unchanged, and the stand-ins for
hod in the harness that still write eight fields remain readable by it.

Examples (`⇥` is a tab):

```
fallback⇥codegen-not-implemented⇥text⇥14437⇥/tmp/kiln-1/0-hello.o⇥aarch64-unknown-linux-gnu⇥225⇥10949⇥-⇥0⇥-
fallback⇥codegen-not-implemented⇥bitcode⇥15664⇥/tmp/kiln-2/0-coro.o⇥aarch64-unknown-linux-gnu⇥10411⇥13898⇥10170⇥0⇥-
fallback⇥bitcode-converter-failed⇥bitcode⇥15664⇥/tmp/kiln-2/0-coro.o⇥-⇥9120⇥13898⇥8894⇥0⇥-
fallback⇥bitcode-input⇥bitcode⇥15664⇥/tmp/kiln-2/0-coro.o⇥-⇥226⇥13898⇥-⇥0⇥-
error⇥codegen-not-implemented⇥text⇥14437⇥hello.o⇥aarch64-unknown-linux-gnu⇥100⇥-⇥-⇥-⇥-
fallback⇥unknown-option⇥-⇥-⇥a.o⇥-⇥40⇥9120⇥-⇥1⇥-
fallback⇥codegen-not-implemented⇥text⇥14437⇥a.o⇥aarch64-unknown-linux-gnu⇥231⇥2520⇥-⇥signal:9⇥-
```

(Lines from hods before the native reader.) The second line is a bitcode
module that was converted, declined as text and compiled by the fallback;
the third, one whose conversion failed; the fourth, one for which no
converter was configured. Today the reason of a kiln module is the module
pass's, and a census adds the rest:

```
error⇥module:pic⇥text⇥14437⇥hello.o⇥aarch64-unknown-linux-gnu⇥612⇥-⇥-⇥-⇥module:pic,module:pie,global:function-definition,cc:ccc,...
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
| `parse` | the module level has been read (`hod_ll::parse_module`, `hod_bc::read_module`) |
| `classify` | `classify`'s module pass has judged the module |
| `bodies` | every body has been read and judged (only when the walk did not stop at a reason) |
| `emission` | every body has been compiled to machine code ("The back half", step 4) |
| `object` | the module has been laid out and its object written in memory |
| `convert` | the bitcode converter ended ("Bitcode conversion"), with text or a failure; the attempt that declined the bitcode is in it. Only an invocation that started a conversion has it |
| `compile` | `hod::compile` returned, with an object or an error: what follows the last of `parse`, `classify`, `bodies` (code generation, today the seam's decline), or the whole call when it declined before reading |
| `write` | the object has been written |
| `fallback` | the fallback process ended |

A phase is in the report only if the invocation reached its end: a module
hod builds reports `read`, `parse`, `classify`, `bodies`, `emission`, `object`, `compile`,
`write`; one it declines in strict mode at the module pass `read`,
`parse`, `classify`, `compile`, and one it declines before reading `read`,
`compile`; one it hands to the fallback `read`, `compile`,
`fallback`; a command line it hands on unread `fallback` alone. A
conversion adds `convert` after `read`: a converted module hod then
declines reports `read`, `convert`, `compile`, and one whose conversion
failed `read`, `convert`, then `fallback` if there is one. Example of
a decline:

```
read⇥212
parse⇥380
classify⇥4
compile⇥2
total⇥640
```

`parse`, `classify` and `bodies` are the front half's (they were `ingest`
in the names reserved before); `emission` and `object` the back half's,
and `compile` is then what is left of the call. `analysis` and
`debug-info` stay reserved. A reader of the report takes whatever names it
finds.

If the report cannot be written: `hod: error: cannot write phase report
'<path>': <cause>`, and an exit status of 0 becomes 1. The fallback
inherits the variable with the rest of the environment; `llc` ignores it.

## Tests

| section | tests |
|---|---|
| The facade, Input, Reasons | `crates/hod/src/lib.rs`: `input_kind_detection`, `reasons_in_order`, `construct_names_are_stable_and_distinct`, `module_triple_scan`, `supported_targets`, `error_text`; `crates/hod/src/front.rs`: `reader_errors_and_their_reasons`, `rejections_and_their_reasons`; `crates/hod/tests/front.rs` (text, the checked-in `data/front.bc`, and bitcode from each installed `llvm-as`): `fixture_first_refusal_and_census`, `body_reasons_name_the_function`, `verify_is_on_the_path_to_code_generation`, `target_reasons`, `reader_refusals`, `installed_llvm_bitcode_agrees_with_text`; `driver.rs`: `census_of_every_reason`; the whole corpus: `cargo xtask corpus reasons` (`docs/status/reasons.md`) |
| Bitcode conversion | `crates/hod_cli/src/convert.rs`: `usable_output`, `quoted_line`, `standard_error_is_bounded`, `reasons_and_messages`; `driver.rs`: `converted_bitcode_is_compiled`, `conversion_only_when_bitcode_is_the_reason`; `cli.rs`: `bitcode_is_converted_and_goes_on_as_text`, `conversion_failures` (every row of the result table, with a fallback, in strict mode and with neither; a megabyte of input the converter may not read; a megabyte on its standard error), `converter_that_cannot_be_started`, `bitcode_file_input`, `converter_settings`, `hod_is_not_its_own_converter`, `real_llvm_dis_converts_bitcode` (each installed LLVM; plain and wrapped bitcode; the native census equals the converted one), `real_llvm_dis_of_another_major`; `kiln_e2e.rs` |
| Options | `crates/hod_cli/src/args.rs`: `defaults`, `everything_kiln_passes`, `spellings`, `spellings_of_the_rest`, `rejected_command_lines`, `rejection_reasons_and_messages`, `non_utf8_arguments`, `version_and_help`; `crates/hod_cli/src/driver.rs`: `unknown_option_without_fallback`, `version_and_help`; `crates/hod_cli/tests/cli.rs`: `unknown_options`, `version_and_help` |
| hod's own options, Environment | `args.rs`: `hod_options_are_parsed_and_not_forwarded`, `mistakes_in_hod_options_are_fatal`; `driver.rs`: `environment`, `mistakes_in_hod_settings`; `cli.rs`: `settings_from_the_environment`, `converter_settings` |
| Phase report | `driver.rs`: `phase_report`, `phase_report_of_a_conversion`; `cli.rs`: `phase_report_with_a_fallback` |
| Behaviour | `driver.rs`: `success_writes_the_object`, `options_reach_the_code_generator`, `output_names_follow_llc`, `io_errors` |
| Fallback protocol | `cli.rs`: `fallback_receives_original_arguments_and_identical_input`, `file_input`, `hod_is_not_its_own_fallback`, `real_llc_fallback_text`, `real_llc_fallback_bitcode`, and the "Bitcode conversion" tests for the original bytes after a conversion; `driver.rs`: `fallback_loop_is_refused` |
| Strict mode | `cli.rs`: `strict_mode_fails_with_the_construct_named`; `crates/hod_cli/tests/kiln_e2e.rs` |
| Reasons | `cli.rs`: `reasons_reach_the_log`; `driver.rs`: `malformed_input_and_invalid_options`, `internal_errors_are_not_declines` |
| The back half, Threads | `crates/hod/tests/back.rs`: `object_is_the_same_for_every_thread_count` (1, 2, 3, 8 threads), `tokens_are_asked_for_large_modules_and_given_back`, `the_first_refusal_in_module_order_wins`, `target_features`, `x86_64_is_a_stable_refusal`, `break_even` (ignored: the measurement); `crates/hod/src/back.rs`: `threads_wanted`, `options_for_code_generation`, `errors_of_the_back_half`; `crates/hod_cli/src/jobs.rs`: `makeflags_forms`, `where_threads_come_from`, `tokens_are_taken_without_waiting_and_given_back`; `crates/hod_a64/src/features.rs` (§5.5.2); `kiln_e2e.rs`: kiln's integer code in strict mode at -O0 and -O1, linked with a C program under each linker and checked against LLVM's; `tests/ir/exit_*.ll` |
| Exit status | `cli.rs`: `fallback_exit_status_propagates`; `driver.rs`: `unsupported_without_fallback`, `malformed_input_and_invalid_options`, `io_errors` |
| Coverage log | `crates/hod_cli/src/coverage.rs`: `line_format`, `free_form_fields_are_escaped`, `appends_to_an_existing_file`; `cli.rs`: `coverage_log_survives_concurrent_writers` (16 concurrent writers), `fallback_exit_status_propagates` (field 10, signal included), every test's `coverage` helper (eleven fields, their vocabulary); `xtask/src/irdiff/engine.rs`: `coverage_log_reading` (a reader under the append-only rule) |

The tests that need LLVM tools (`real_llc_*`, `real_llvm_dis_*`) and kiln
(`kiln_e2e.rs`) skip with a note when the tool is missing and fail instead
under `HOD_REQUIRE_TOOLS=1`. The paths a real module rarely takes
(`MalformedInput`, `InvalidOptions`, `Internal`) are tested in
`driver.rs` with a stand-in code generator.
