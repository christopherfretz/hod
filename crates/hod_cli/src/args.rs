//! Command-line parsing. Contract: `spec/cli.md` "Options".
//!
//! The grammar is `llc`'s (LLVM's `cl` library), for the options hod knows:
//! one or two leading dashes mean the same; a value is joined with `=` or,
//! for options that require one, is the next argument; `-O` prefixes its
//! value; a later occurrence of an option replaces an earlier one; `--` ends
//! the options; at most one positional argument names the input.
//!
//! Parsing never fails outright. What hod cannot accept is recorded
//! ([`CommandLine::rejected`], [`CommandLine::fatal`]) and the caller decides
//! between an error and the fallback, which needs the rest of the command
//! line either way.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

use hod::{
    CodeModel, Compression, DebuggerTuning, FpContract, OptLevel, Options, OutputKind, RelocModel,
};

/// What the invocation asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Compile the input (the default).
    Compile,
    /// `--version`.
    Version,
    /// `--help`.
    Help,
}

/// Where the module comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Standard input: no positional argument, or `-`.
    Stdin,
    /// A file.
    Path(PathBuf),
}

/// Why hod cannot honor a command line that `llc` might accept. With a
/// fallback configured the whole invocation goes to it; without one this is
/// the error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// An option hod does not know, as written.
    UnknownOption(String),
    /// A known option with a value hod does not know.
    UnknownValue { option: String, value: String },
    /// A known option that requires a value, at the end of the arguments.
    MissingValue(String),
    /// A second positional argument.
    ExtraInput(String),
}

impl Rejected {
    /// The reason in the coverage log (`spec/cli.md` "Reasons").
    pub fn reason(&self) -> &'static str {
        match self {
            Rejected::UnknownOption(_) | Rejected::UnknownValue { .. } => "unknown-option",
            Rejected::MissingValue(_) | Rejected::ExtraInput(_) => "usage",
        }
    }
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::UnknownOption(option) => write!(f, "unknown option '{option}'"),
            Rejected::UnknownValue { option, value } => {
                write!(f, "unknown value '{value}' for option '{option}'")
            }
            Rejected::MissingValue(option) => write!(f, "option '{option}' requires a value"),
            Rejected::ExtraInput(input) => {
                write!(f, "more than one input file ('{input}'); at most one is accepted")
            }
        }
    }
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLine {
    /// Compile, or print the version or the help.
    pub action: Action,
    /// The `llc` options hod understands.
    pub options: Options,
    /// The input.
    pub input: Input,
    /// `-o`, as given (`-` is standard output).
    pub output: Option<OsString>,
    /// `--fallback=`.
    pub fallback: Option<PathBuf>,
    /// `--strict` (or `--strict=<bool>`), if given.
    pub strict: Option<bool>,
    /// `--coverage-log=`.
    pub coverage_log: Option<PathBuf>,
    /// `--bitcode-via=`: the `llvm-dis` that turns bitcode into text.
    pub bitcode_via: Option<PathBuf>,
    /// `--compress-debug-sections=`, if given (WP-061).
    pub compress_debug_sections: Option<Compression>,
    /// The arguments for the fallback: the original ones, in order and as
    /// spelled, without hod's own options.
    pub forwarded: Vec<OsString>,
    /// The first thing on the command line hod cannot honor, if any.
    pub rejected: Option<Rejected>,
    /// A mistake in one of hod's own options: always an error, never handed
    /// to the fallback.
    pub fatal: Option<String>,
}

/// How an option takes its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opt {
    // Boolean flags: `-name` or `-name=<bool>`.
    FunctionSections,
    DataSections,
    X86BranchesWithin32B,
    TimeTrace,
    Strict,
    Version,
    Help,
    // Options with a required value: `-name=<v>` or `-name <v>`.
    RelocationModel,
    FileType,
    Cpu,
    Attr,
    Arch,
    Triple,
    FpContract,
    CodeModel,
    DebuggerTune,
    SplitDwarfFile,
    SplitDwarfOutput,
    TimeTraceGranularity,
    TimeTraceFile,
    Output,
    Fallback,
    CoverageLog,
    BitcodeVia,
    CompressDebugSections,
}

impl Opt {
    fn lookup(name: &str) -> Option<Opt> {
        Some(match name {
            "function-sections" => Opt::FunctionSections,
            "data-sections" => Opt::DataSections,
            "x86-branches-within-32B-boundaries" => Opt::X86BranchesWithin32B,
            "time-trace" => Opt::TimeTrace,
            "strict" => Opt::Strict,
            "version" => Opt::Version,
            "help" | "h" => Opt::Help,
            "relocation-model" => Opt::RelocationModel,
            "filetype" => Opt::FileType,
            "mcpu" => Opt::Cpu,
            "mattr" => Opt::Attr,
            "march" => Opt::Arch,
            "mtriple" => Opt::Triple,
            "fp-contract" => Opt::FpContract,
            "code-model" => Opt::CodeModel,
            "debugger-tune" => Opt::DebuggerTune,
            "split-dwarf-file" => Opt::SplitDwarfFile,
            "split-dwarf-output" => Opt::SplitDwarfOutput,
            "time-trace-granularity" => Opt::TimeTraceGranularity,
            "time-trace-file" => Opt::TimeTraceFile,
            "o" => Opt::Output,
            "fallback" => Opt::Fallback,
            "coverage-log" => Opt::CoverageLog,
            "bitcode-via" => Opt::BitcodeVia,
            "compress-debug-sections" => Opt::CompressDebugSections,
            _ => return None,
        })
    }

    fn is_flag(self) -> bool {
        matches!(
            self,
            Opt::FunctionSections
                | Opt::DataSections
                | Opt::X86BranchesWithin32B
                | Opt::TimeTrace
                | Opt::Strict
                | Opt::Version
                | Opt::Help
        )
    }

    /// hod's own options: not `llc`'s, never forwarded to the fallback.
    fn is_hod_specific(self) -> bool {
        matches!(
            self,
            Opt::Fallback
                | Opt::Strict
                | Opt::CoverageLog
                | Opt::BitcodeVia
                | Opt::CompressDebugSections
        )
    }
}

/// `cl::parser<bool>`'s spellings.
fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "" | "true" | "TRUE" | "True" | "1" => Some(true),
        "false" | "FALSE" | "False" | "0" => Some(false),
        _ => None,
    }
}

fn opt_level(value: &str) -> Option<OptLevel> {
    Some(match value {
        "0" => OptLevel::O0,
        "1" => OptLevel::O1,
        "2" => OptLevel::O2,
        "3" => OptLevel::O3,
        _ => return None,
    })
}

fn relocation_model(value: &str) -> Option<RelocModel> {
    Some(match value {
        "static" => RelocModel::Static,
        "pic" => RelocModel::Pic,
        "dynamic-no-pic" => RelocModel::DynamicNoPic,
        "ropi" => RelocModel::Ropi,
        "rwpi" => RelocModel::Rwpi,
        "ropi-rwpi" => RelocModel::RopiRwpi,
        _ => return None,
    })
}

fn code_model(value: &str) -> Option<CodeModel> {
    Some(match value {
        "tiny" => CodeModel::Tiny,
        "small" => CodeModel::Small,
        "kernel" => CodeModel::Kernel,
        "medium" => CodeModel::Medium,
        "large" => CodeModel::Large,
        _ => return None,
    })
}

fn debugger_tuning(value: &str) -> Option<DebuggerTuning> {
    Some(match value {
        "gdb" => DebuggerTuning::Gdb,
        "lldb" => DebuggerTuning::Lldb,
        "dbx" => DebuggerTuning::Dbx,
        "sce" => DebuggerTuning::Sce,
        _ => return None,
    })
}

fn fp_contract(value: &str) -> Option<FpContract> {
    Some(match value {
        "fast" => FpContract::Fast,
        "on" => FpContract::On,
        "off" => FpContract::Off,
        _ => return None,
    })
}

fn output_kind(value: &str) -> Option<OutputKind> {
    Some(match value {
        "asm" => OutputKind::Assembly,
        "obj" => OutputKind::Object,
        // `null` (no output) is `llc`'s; hod does not offer it.
        _ => return None,
    })
}

/// A value of `--compress-debug-sections=` or
/// `HOD_COMPRESS_DEBUG_SECTIONS` (`spec/cli.md` "hod's own options"), or
/// the message of its mistake.
pub fn compression(value: &OsStr) -> Result<Compression, String> {
    match value.to_str() {
        Some("none") => Ok(Compression::None),
        Some("zlib") => Ok(Compression::Zlib),
        Some("zstd") => Err("compression 'zstd' is not supported: hod writes zlib only".into()),
        _ => {
            Err(format!("invalid compression '{}': expected none or zlib", value.to_string_lossy()))
        }
    }
}

/// Stores a recognized value; says whether there was one.
fn set<T>(slot: &mut Option<T>, value: Option<T>) -> bool {
    let known = value.is_some();
    if known {
        *slot = value;
    }
    known
}

/// An argument that `cl` reads as an option: it starts with a dash and is
/// not the lone `-` that names standard input.
fn is_option(arg: &OsStr) -> bool {
    let bytes = arg.as_encoded_bytes();
    bytes.len() > 1 && bytes[0] == b'-'
}

struct Parser {
    line: CommandLine,
    inputs: usize,
}

impl Parser {
    fn reject(&mut self, rejected: Rejected) {
        self.line.rejected.get_or_insert(rejected);
    }

    fn fatal(&mut self, message: String) {
        self.line.fatal.get_or_insert(message);
    }

    fn positional(&mut self, arg: &OsStr) {
        self.inputs += 1;
        if self.inputs > 1 {
            self.reject(Rejected::ExtraInput(arg.to_string_lossy().into_owned()));
        } else if arg == "-" {
            self.line.input = Input::Stdin;
        } else {
            self.line.input = Input::Path(PathBuf::from(arg));
        }
    }

    fn unknown_value(&mut self, written: &str, value: &OsStr) {
        self.reject(Rejected::UnknownValue {
            option: written.to_owned(),
            value: value.to_string_lossy().into_owned(),
        });
    }

    fn flag(&mut self, opt: Opt, written: &str, value: &str) {
        let Some(on) = parse_bool(value) else {
            if opt.is_hod_specific() {
                self.fatal(format!("invalid value '{value}' for '{written}': expected a boolean"));
            } else {
                self.unknown_value(written, OsStr::new(value));
            }
            return;
        };
        let options = &mut self.line.options;
        match opt {
            Opt::FunctionSections => options.function_sections = on,
            Opt::DataSections => options.data_sections = on,
            Opt::X86BranchesWithin32B => options.x86_branches_within_32b = on,
            Opt::Strict => self.line.strict = Some(on),
            Opt::Version if on && self.line.action == Action::Compile => {
                self.line.action = Action::Version;
            }
            Opt::Help if on && self.line.action == Action::Compile => {
                self.line.action = Action::Help;
            }
            // `--time-trace` asks `llc` for its own trace file; a caller
            // must not depend on it (spec/cli.md "Options").
            _ => {}
        }
    }

    fn value(&mut self, opt: Opt, written: &str, value: &OsStr) {
        // Options whose value is a path take any bytes.
        match opt {
            Opt::Output => {
                self.line.output = Some(value.to_owned());
                return;
            }
            Opt::SplitDwarfFile => {
                self.line.options.split_dwarf_file = Some(PathBuf::from(value));
                return;
            }
            Opt::SplitDwarfOutput => {
                self.line.options.split_dwarf_output = Some(PathBuf::from(value));
                return;
            }
            Opt::Fallback | Opt::CoverageLog | Opt::BitcodeVia => {
                if value.is_empty() {
                    self.fatal(format!("option '{written}' requires a non-empty value"));
                    return;
                }
                let slot = match opt {
                    Opt::Fallback => &mut self.line.fallback,
                    Opt::CoverageLog => &mut self.line.coverage_log,
                    _ => &mut self.line.bitcode_via,
                };
                *slot = Some(PathBuf::from(value));
                return;
            }
            Opt::TimeTraceFile => return,
            Opt::CompressDebugSections => {
                match compression(value) {
                    Ok(c) => self.line.compress_debug_sections = Some(c),
                    Err(message) => self.fatal(format!("'{written}': {message}")),
                }
                return;
            }
            _ => {}
        }
        let Some(text) = value.to_str() else {
            self.unknown_value(written, value);
            return;
        };
        let options = &mut self.line.options;
        let known = match opt {
            Opt::RelocationModel => set(&mut options.relocation_model, relocation_model(text)),
            Opt::FpContract => set(&mut options.fp_contract, fp_contract(text)),
            Opt::CodeModel => set(&mut options.code_model, code_model(text)),
            Opt::DebuggerTune => set(&mut options.debugger_tuning, debugger_tuning(text)),
            Opt::FileType => match output_kind(text) {
                Some(kind) => {
                    options.output_kind = kind;
                    true
                }
                None => false,
            },
            Opt::TimeTraceGranularity => text.parse::<u32>().is_ok(),
            // `help` makes `llc` print its list of CPUs or features.
            Opt::Cpu => set(&mut options.cpu, (text != "help").then(|| text.to_owned())),
            Opt::Arch => set(&mut options.arch, (text != "help").then(|| text.to_owned())),
            Opt::Attr if text.split(',').any(|feature| feature == "help") => false,
            Opt::Attr => {
                let features = text.split(',').filter(|feature| !feature.is_empty());
                options.features.extend(features.map(str::to_owned));
                true
            }
            Opt::Triple => {
                options.triple = Some(text.to_owned());
                true
            }
            _ => unreachable!("option {opt:?} is a flag or was handled above"),
        };
        if !known {
            self.unknown_value(written, value);
        }
    }
}

/// Parses the arguments after the program name.
pub fn parse(args: &[OsString]) -> CommandLine {
    let mut parser = Parser {
        line: CommandLine {
            action: Action::Compile,
            options: Options::default(),
            input: Input::Stdin,
            output: None,
            fallback: None,
            strict: None,
            coverage_log: None,
            bitcode_via: None,
            compress_debug_sections: None,
            forwarded: Vec::with_capacity(args.len()),
            rejected: None,
            fatal: None,
        },
        inputs: 0,
    };
    let mut options_ended = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if options_ended || !is_option(arg) {
            parser.positional(arg);
            parser.line.forwarded.push(arg.clone());
            continue;
        }
        if arg == "--" {
            options_ended = true;
            parser.line.forwarded.push(arg.clone());
            continue;
        }
        let Some(text) = arg.to_str() else {
            parser.reject(Rejected::UnknownOption(arg.to_string_lossy().into_owned()));
            parser.line.forwarded.push(arg.clone());
            continue;
        };
        let body = text.strip_prefix("--").or_else(|| text.strip_prefix('-')).unwrap_or(text);
        let (name, joined) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        let written = &text[..text.len() - joined.map_or(0, |v| v.len() + 1)];
        let Some(opt) = Opt::lookup(name) else {
            // `-O<level>`: the one prefix option. `-O2`, `-O=2` and `-O 2`.
            parser.line.forwarded.push(arg.clone());
            if let Some(level) = body.strip_prefix('O') {
                let level = level.strip_prefix('=').unwrap_or(level);
                let level = if level.is_empty() {
                    let Some(next) = rest.next() else {
                        parser.reject(Rejected::MissingValue(text.to_owned()));
                        continue;
                    };
                    parser.line.forwarded.push(next.clone());
                    next.to_string_lossy().into_owned()
                } else {
                    level.to_owned()
                };
                match opt_level(&level) {
                    Some(level) => parser.line.options.opt_level = level,
                    None => parser.unknown_value("-O", OsStr::new(&level)),
                }
            } else {
                parser.reject(Rejected::UnknownOption(text.to_owned()));
            }
            continue;
        };
        if !opt.is_hod_specific() {
            parser.line.forwarded.push(arg.clone());
        }
        if opt.is_flag() {
            parser.flag(opt, written, joined.unwrap_or(""));
        } else if let Some(value) = joined {
            parser.value(opt, written, OsStr::new(value));
        } else if let Some(next) = rest.next() {
            if !opt.is_hod_specific() {
                parser.line.forwarded.push(next.clone());
            }
            parser.value(opt, written, next);
        } else if opt.is_hod_specific() {
            parser.fatal(format!("option '{written}' requires a value"));
        } else {
            parser.reject(Rejected::MissingValue(written.to_owned()));
        }
    }
    parser.line
}

/// `--help`.
pub const HELP: &str = "\
OVERVIEW: hod, a debug-build code generator with llc's command line

USAGE: hod [options] <input LLVM IR file, or - for standard input (the default)>

OPTIONS (one or two dashes; a value is joined with '=' or is the next argument):
  -o <file>                       Output file ('-' is standard output)
  -O<0|1|2|3>                     Optimization level (accepted; hod does not optimize)
  --filetype=<asm|obj>            Output kind (hod writes objects only)
  --mtriple=<triple>              Override the module's target triple
  --march=<arch>                  Architecture override
  --mcpu=<cpu>                    Target CPU
  --mattr=<+a,-b,...>             Target features
  --relocation-model=<static|pic|dynamic-no-pic|ropi|rwpi|ropi-rwpi>
  --code-model=<tiny|small|kernel|medium|large>
  --function-sections             One section per function
  --data-sections                 One section per global
  --fp-contract=<fast|on|off>     Floating-point contraction
  --debugger-tune=<gdb|lldb|dbx|sce>
  --split-dwarf-file=<name>       The .dwo name recorded in the object
  --split-dwarf-output=<file>     Where the .dwo file is written
  --x86-branches-within-32B-boundaries
  --time-trace, --time-trace-granularity=<n>, --time-trace-file=<file>
                                  Accepted; hod itself writes no trace
  --version                       Print the version
  --help                          Print this help

hod's own options (each also settable in the environment):
  --fallback=<llc>                HOD_FALLBACK_LLC   The real llc, run on the same input with the
                                                     same arguments when hod does not support
                                                     something in the module or on the command line
  --strict                        HOD_STRICT=1       No fallback: unsupported is an error
  --coverage-log=<file>           HOD_COVERAGE_LOG   Append one line per invocation to <file>
  --bitcode-via=<llvm-dis>        HOD_BITCODE_VIA    Convert a bitcode input to text with this
                                                     llvm-dis (of the LLVM that wrote the bitcode)
                                                     and compile the text
  --compress-debug-sections=<none|zlib>
                                  HOD_COMPRESS_DEBUG_SECTIONS
                                                     Write the .debug_* sections compressed
                                                     (SHF_COMPRESSED, zlib); zstd is not supported

EXIT STATUS: 0 on success; 1 on an error; 2 when the module is unsupported and no fallback
ran; otherwise the fallback's own status. The contract is spec/cli.md.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn parsed(args: &[&str]) -> CommandLine {
        parse(&os(args))
    }

    /// Parses and asserts that hod accepts the whole command line.
    fn accepted(args: &[&str]) -> CommandLine {
        let line = parsed(args);
        assert_eq!(line.rejected, None, "{args:?}");
        assert_eq!(line.fatal, None, "{args:?}");
        assert_eq!(line.forwarded, os(args), "{args:?}: nothing hod-specific to drop");
        line
    }

    // spec/cli.md "Options": with no arguments hod is `llc` with no
    // arguments: standard input, assembly, -O2.
    #[test]
    fn defaults() {
        let line = accepted(&[]);
        assert_eq!(line.action, Action::Compile);
        assert_eq!(line.options, Options::default());
        assert_eq!(line.input, Input::Stdin);
        assert_eq!(line.output, None);
        assert_eq!((line.fallback, line.strict, line.coverage_log), (None, None, None));
        assert_eq!(line.bitcode_via, None);
    }

    // spec/cli.md "Options": the command line kiln builds
    // (kiln_driver/src/codegen_cmd.rs `llc`), with everything on.
    #[test]
    fn everything_kiln_passes() {
        let line = accepted(&[
            "-O0",
            "-relocation-model=pic",
            "-filetype=obj",
            "-function-sections",
            "-data-sections",
            "-mcpu=neoverse-n1",
            "-fp-contract=fast",
            "-code-model=large",
            "-x86-branches-within-32B-boundaries",
            "-debugger-tune=gdb",
            "-split-dwarf-file=a.dwo",
            "-split-dwarf-output=a.dwo",
            "-o",
            "/tmp/x/a.o",
            "-",
            "--time-trace",
            "--time-trace-granularity=500",
            "--time-trace-file=/tmp/x/time-trace-1.json",
        ]);
        let expected = Options {
            cpu: Some("neoverse-n1".into()),
            opt_level: OptLevel::O0,
            relocation_model: Some(RelocModel::Pic),
            code_model: Some(CodeModel::Large),
            function_sections: true,
            data_sections: true,
            fp_contract: Some(FpContract::Fast),
            x86_branches_within_32b: true,
            debugger_tuning: Some(DebuggerTuning::Gdb),
            split_dwarf_file: Some("a.dwo".into()),
            split_dwarf_output: Some("a.dwo".into()),
            output_kind: OutputKind::Object,
            ..Options::default()
        };
        assert_eq!(line.options, expected);
        assert_eq!(line.input, Input::Stdin);
        assert_eq!(line.output, Some("/tmp/x/a.o".into()));
    }

    type Check = fn(&CommandLine) -> bool;

    /// Each command line is accepted whole and parses as its check says.
    fn check_all(table: &[(&[&str], Check)]) {
        for (args, check) in table {
            let line = accepted(args);
            assert_eq!(line.action, Action::Compile, "{args:?}");
            assert!(check(&line), "{args:?}: {line:#?}");
        }
    }

    // spec/cli.md "Options": every option, in every spelling `llc` accepts.
    #[test]
    fn spellings() {
        check_all(&[
            (&["-O0"], |l| l.options.opt_level == OptLevel::O0),
            (&["--O1"], |l| l.options.opt_level == OptLevel::O1),
            (&["-O=2"], |l| l.options.opt_level == OptLevel::O2),
            (&["-O", "3"], |l| l.options.opt_level == OptLevel::O3),
            (&["-O3", "-O0"], |l| l.options.opt_level == OptLevel::O0),
            (&["-relocation-model=pic"], |l| l.options.relocation_model == Some(RelocModel::Pic)),
            (&["--relocation-model=static"], |l| {
                l.options.relocation_model == Some(RelocModel::Static)
            }),
            (&["-relocation-model", "dynamic-no-pic"], |l| {
                l.options.relocation_model == Some(RelocModel::DynamicNoPic)
            }),
            (&["--relocation-model", "ropi"], |l| {
                l.options.relocation_model == Some(RelocModel::Ropi)
            }),
            (&["-relocation-model=rwpi"], |l| l.options.relocation_model == Some(RelocModel::Rwpi)),
            (&["-relocation-model=ropi-rwpi"], |l| {
                l.options.relocation_model == Some(RelocModel::RopiRwpi)
            }),
            (&["-relocation-model=static", "-relocation-model=pic"], |l| {
                l.options.relocation_model == Some(RelocModel::Pic)
            }),
            (&["-filetype=obj"], |l| l.options.output_kind == OutputKind::Object),
            (&["--filetype=asm"], |l| l.options.output_kind == OutputKind::Assembly),
            (&["-filetype", "obj"], |l| l.options.output_kind == OutputKind::Object),
            (&["-function-sections"], |l| l.options.function_sections),
            (&["--function-sections"], |l| l.options.function_sections),
            (&["-function-sections=true"], |l| l.options.function_sections),
            (&["-function-sections=1"], |l| l.options.function_sections),
            (&["-function-sections", "-function-sections=false"], |l| !l.options.function_sections),
            (&["-function-sections", "--function-sections=0"], |l| !l.options.function_sections),
            (&["-function-sections", "-function-sections=FALSE"], |l| !l.options.function_sections),
            (&["-data-sections"], |l| l.options.data_sections && !l.options.function_sections),
            (&["--data-sections=True"], |l| l.options.data_sections),
            (&["-mcpu=generic"], |l| l.options.cpu.as_deref() == Some("generic")),
            (&["--mcpu", "x86-64-v3"], |l| l.options.cpu.as_deref() == Some("x86-64-v3")),
            (&["-mattr=+neon,-sve"], |l| l.options.features == ["+neon", "-sve"]),
            (&["-mattr", "+a", "--mattr=+b,,+c"], |l| l.options.features == ["+a", "+b", "+c"]),
            (&["-march=aarch64"], |l| l.options.arch.as_deref() == Some("aarch64")),
            (&["--march", "x86-64"], |l| l.options.arch.as_deref() == Some("x86-64")),
            (&["-mtriple=x86_64-unknown-linux-gnu"], |l| {
                l.options.triple.as_deref() == Some("x86_64-unknown-linux-gnu")
            }),
            (&["--mtriple", "aarch64-unknown-linux-gnu"], |l| {
                l.options.triple.as_deref() == Some("aarch64-unknown-linux-gnu")
            }),
            (&["-fp-contract=fast"], |l| l.options.fp_contract == Some(FpContract::Fast)),
            (&["--fp-contract", "on"], |l| l.options.fp_contract == Some(FpContract::On)),
            (&["-fp-contract=off"], |l| l.options.fp_contract == Some(FpContract::Off)),
            (&["-code-model=tiny"], |l| l.options.code_model == Some(CodeModel::Tiny)),
            (&["-code-model=small"], |l| l.options.code_model == Some(CodeModel::Small)),
            (&["-code-model=kernel"], |l| l.options.code_model == Some(CodeModel::Kernel)),
            (&["--code-model=medium"], |l| l.options.code_model == Some(CodeModel::Medium)),
            (&["-code-model", "large"], |l| l.options.code_model == Some(CodeModel::Large)),
            (&["-debugger-tune=gdb"], |l| l.options.debugger_tuning == Some(DebuggerTuning::Gdb)),
            (&["--debugger-tune=lldb"], |l| {
                l.options.debugger_tuning == Some(DebuggerTuning::Lldb)
            }),
            (&["-debugger-tune", "sce"], |l| {
                l.options.debugger_tuning == Some(DebuggerTuning::Sce)
            }),
            (&["-debugger-tune=dbx"], |l| l.options.debugger_tuning == Some(DebuggerTuning::Dbx)),
            (&["-split-dwarf-file=x.dwo"], |l| {
                l.options.split_dwarf_file == Some("x.dwo".into())
                    && l.options.split_dwarf_output.is_none()
            }),
            (&["--split-dwarf-file", "x.dwo"], |l| {
                l.options.split_dwarf_file == Some("x.dwo".into())
            }),
            (&["-split-dwarf-output=out/x.dwo"], |l| {
                l.options.split_dwarf_output == Some("out/x.dwo".into())
            }),
            (&["--split-dwarf-output", "x.dwo"], |l| {
                l.options.split_dwarf_output == Some("x.dwo".into())
            }),
        ]);
    }

    // spec/cli.md "Options": the options hod accepts and ignores, the
    // output and the input.
    #[test]
    fn spellings_of_the_rest() {
        check_all(&[
            (&["-x86-branches-within-32B-boundaries"], |l| l.options.x86_branches_within_32b),
            (&["--x86-branches-within-32B-boundaries=false"], |l| {
                !l.options.x86_branches_within_32b
            }),
            (&["--time-trace"], |l| l.options == Options::default()),
            (&["-time-trace=false"], |l| l.options == Options::default()),
            (&["--time-trace-granularity=500"], |l| l.options == Options::default()),
            (&["-time-trace-granularity", "0"], |l| l.options == Options::default()),
            (&["--time-trace-file=t.json"], |l| l.options == Options::default()),
            (&["-time-trace-file", "t.json"], |l| {
                l.options == Options::default() && l.input == Input::Stdin
            }),
            (&["-o", "a.o"], |l| l.output == Some("a.o".into())),
            (&["--o", "a.o"], |l| l.output == Some("a.o".into())),
            (&["-o=a.o"], |l| l.output == Some("a.o".into())),
            (&["-o", "-"], |l| l.output == Some("-".into()) && l.input == Input::Stdin),
            (&["-o", "a.o", "-o", "b.o"], |l| l.output == Some("b.o".into())),
            (&["-o", "-o"], |l| l.output == Some("-o".into())),
            (&["-"], |l| l.input == Input::Stdin),
            (&["m.ll"], |l| l.input == Input::Path("m.ll".into())),
            (&["-o", "a.o", "dir/m.bc"], |l| l.input == Input::Path("dir/m.bc".into())),
            (&["dir/m.ll", "-filetype=obj"], |l| {
                l.input == Input::Path("dir/m.ll".into())
                    && l.options.output_kind == OutputKind::Object
            }),
            (&["--", "-O0"], |l| {
                l.input == Input::Path("-O0".into()) && l.options.opt_level == OptLevel::O2
            }),
            (&["-O0", "--", "-"], |l| {
                l.input == Input::Stdin && l.options.opt_level == OptLevel::O0
            }),
            (&[""], |l| l.input == Input::Path("".into())),
        ]);
    }

    // spec/cli.md "Options": what hod does not know is recorded, the first
    // one wins, and the fallback still gets every argument.
    #[test]
    fn rejected_command_lines() {
        let unknown = |o: &str| Rejected::UnknownOption(o.to_owned());
        let value =
            |o: &str, v: &str| Rejected::UnknownValue { option: o.to_owned(), value: v.to_owned() };
        let missing = |o: &str| Rejected::MissingValue(o.to_owned());
        let table: &[(&[&str], Rejected)] = &[
            (&["-bogus"], unknown("-bogus")),
            (&["--bogus=1", "-o", "a.o"], unknown("--bogus=1")),
            (&["-O0", "-stats", "-also-bogus"], unknown("-stats")),
            (&["---version"], unknown("---version")),
            (&["-ofile.o"], unknown("-ofile.o")),
            (&["-o-"], unknown("-o-")),
            (&["-filetype=null"], value("-filetype", "null")),
            (&["--filetype", "elf"], value("--filetype", "elf")),
            (&["-O4"], value("-O", "4")),
            (&["-Os"], value("-O", "s")),
            (&["-O", "fast"], value("-O", "fast")),
            (&["-relocation-model=pie"], value("-relocation-model", "pie")),
            (&["-code-model=huge"], value("-code-model", "huge")),
            (&["-debugger-tune=windbg"], value("-debugger-tune", "windbg")),
            (&["-fp-contract=maybe"], value("-fp-contract", "maybe")),
            (&["-function-sections=yes"], value("-function-sections", "yes")),
            (&["-time-trace-granularity=fine"], value("-time-trace-granularity", "fine")),
            (&["-mcpu=help"], value("-mcpu", "help")),
            (&["-mattr=+a,help"], value("-mattr", "+a,help")),
            (&["-march=help"], value("-march", "help")),
            (&["-o"], missing("-o")),
            (&["-filetype=obj", "--mcpu"], missing("--mcpu")),
            (&["-O"], missing("-O")),
            (&["a.ll", "b.ll"], Rejected::ExtraInput("b.ll".into())),
            (&["-", "-"], Rejected::ExtraInput("-".into())),
            // A flag takes no separate value: `false` is a second input.
            (&["a.ll", "-function-sections", "false"], Rejected::ExtraInput("false".into())),
        ];
        for (args, expected) in table {
            let line = parsed(args);
            assert_eq!(line.rejected.as_ref(), Some(expected), "{args:?}");
            assert_eq!(line.fatal, None, "{args:?}");
            assert_eq!(line.forwarded, os(args), "{args:?}");
        }
    }

    #[test]
    fn rejection_reasons_and_messages() {
        let unknown = Rejected::UnknownOption("-bogus".into());
        assert_eq!(unknown.reason(), "unknown-option");
        assert_eq!(unknown.to_string(), "unknown option '-bogus'");
        let value = Rejected::UnknownValue { option: "-filetype".into(), value: "null".into() };
        assert_eq!(value.reason(), "unknown-option");
        assert_eq!(value.to_string(), "unknown value 'null' for option '-filetype'");
        let missing = Rejected::MissingValue("-o".into());
        assert_eq!(missing.reason(), "usage");
        assert_eq!(missing.to_string(), "option '-o' requires a value");
        let extra = Rejected::ExtraInput("b.ll".into());
        assert_eq!(extra.reason(), "usage");
        assert!(extra.to_string().contains("'b.ll'"));
    }

    // spec/cli.md "hod's own options": parsed in every spelling, and never
    // forwarded to the fallback.
    #[test]
    fn hod_options_are_parsed_and_not_forwarded() {
        let line = parsed(&[
            "-O0",
            "--fallback=/usr/lib/llvm-22/bin/llc",
            "--strict",
            "-filetype=obj",
            "--coverage-log=cov.tsv",
            "--bitcode-via=/usr/lib/llvm-22/bin/llvm-dis",
            "--compress-debug-sections=zlib",
            "-o",
            "a.o",
            "-",
        ]);
        assert_eq!(line.compress_debug_sections, Some(Compression::Zlib));
        assert_eq!((&line.rejected, &line.fatal), (&None, &None));
        assert_eq!(line.fallback, Some("/usr/lib/llvm-22/bin/llc".into()));
        assert_eq!(line.strict, Some(true));
        assert_eq!(line.coverage_log, Some("cov.tsv".into()));
        assert_eq!(line.bitcode_via, Some("/usr/lib/llvm-22/bin/llvm-dis".into()));
        assert_eq!(line.forwarded, os(&["-O0", "-filetype=obj", "-o", "a.o", "-"]));

        let line = parsed(&[
            "-fallback",
            "llc",
            "-coverage-log",
            "c",
            "-strict=false",
            "-bitcode-via",
            "dis",
            "-compress-debug-sections",
            "none",
            "m.ll",
        ]);
        assert_eq!(line.compress_debug_sections, Some(Compression::None));
        assert_eq!((&line.rejected, &line.fatal), (&None, &None));
        assert_eq!(line.fallback, Some("llc".into()));
        assert_eq!(line.coverage_log, Some("c".into()));
        assert_eq!(line.strict, Some(false));
        assert_eq!(line.bitcode_via, Some("dis".into()));
        assert_eq!(line.input, Input::Path("m.ll".into()));
        assert_eq!(line.forwarded, os(&["m.ll"]));

        // The last one wins, as for llc's options.
        let line = parsed(&[
            "--fallback=a",
            "--fallback=b",
            "--strict=0",
            "--strict=1",
            "--bitcode-via=c",
            "--bitcode-via=d",
            "--compress-debug-sections=zlib",
            "--compress-debug-sections=none",
        ]);
        assert_eq!(line.compress_debug_sections, Some(Compression::None));
        assert_eq!((line.fallback, line.strict), (Some("b".into()), Some(true)));
        assert_eq!(line.bitcode_via, Some("d".into()));
        assert!(line.forwarded.is_empty());

        // An unknown option does not hide them: the fallback is still found.
        let line = parsed(&["-bogus", "--fallback=llc", "-o", "a.o"]);
        assert_eq!(line.rejected, Some(Rejected::UnknownOption("-bogus".into())));
        assert_eq!(line.fallback, Some("llc".into()));
        assert_eq!(line.forwarded, os(&["-bogus", "-o", "a.o"]));
    }

    // spec/cli.md "hod's own options": a mistake in one is an error whatever
    // else is configured.
    #[test]
    fn mistakes_in_hod_options_are_fatal() {
        for (args, needle) in [
            (&["--fallback"][..], "'--fallback' requires a value"),
            (&["-O0", "--coverage-log"], "'--coverage-log' requires a value"),
            (&["--fallback="], "'--fallback' requires a non-empty value"),
            (&["--coverage-log", ""], "'--coverage-log' requires a non-empty value"),
            (&["-filetype=obj", "--bitcode-via"], "'--bitcode-via' requires a value"),
            (&["--bitcode-via="], "'--bitcode-via' requires a non-empty value"),
            (&["--strict=maybe"], "invalid value 'maybe' for '--strict'"),
            (&["--compress-debug-sections=zstd"], "compression 'zstd' is not supported"),
            (&["--compress-debug-sections", "gzip"], "invalid compression 'gzip'"),
            (&["--compress-debug-sections"], "'--compress-debug-sections' requires a value"),
        ] {
            let line = parsed(args);
            let fatal = line.fatal.unwrap_or_else(|| panic!("{args:?}: expected a fatal error"));
            assert!(fatal.contains(needle), "{args:?}: {fatal}");
        }
    }

    #[test]
    fn version_and_help() {
        for args in [&["--version"][..], &["-version"], &["-O0", "--version", "-o", "x"]] {
            assert_eq!(accepted(args).action, Action::Version, "{args:?}");
        }
        for args in [&["--help"][..], &["-help"], &["-h"], &["--h"], &["-filetype=obj", "-h"]] {
            assert_eq!(accepted(args).action, Action::Help, "{args:?}");
        }
        assert_eq!(accepted(&["--version", "--help"]).action, Action::Version);
        assert_eq!(accepted(&["--help", "--version"]).action, Action::Help);
        assert_eq!(accepted(&["--version=false"]).action, Action::Compile);
        assert!(HELP.contains("--fallback=<llc>") && HELP.contains("HOD_COVERAGE_LOG"));
        assert!(HELP.contains("--bitcode-via=<llvm-dis>") && HELP.contains("HOD_BITCODE_VIA"));
        assert!(HELP.contains("--compress-debug-sections=<none|zlib>"));
    }

    // Arguments are bytes: a path that is not UTF-8 is a path all the same,
    // and an option that is not UTF-8 is unknown.
    #[cfg(unix)]
    #[test]
    fn non_utf8_arguments() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"caf\xE9.ll".to_vec());
        let out = OsString::from_vec(b"caf\xE9.o".to_vec());
        let args = vec!["-o".into(), out.clone(), path.clone()];
        let line = parse(&args);
        assert_eq!((&line.rejected, &line.fatal), (&None, &None));
        assert_eq!(line.output, Some(out));
        assert_eq!(line.input, Input::Path(PathBuf::from(path)));
        assert_eq!(line.forwarded, args);

        let option = OsString::from_vec(b"-mcpu=\xFF".to_vec());
        let line = parse(std::slice::from_ref(&option));
        assert!(matches!(line.rejected, Some(Rejected::UnknownOption(_))));
        assert_eq!(line.forwarded, [option]);

        let line = parse(&["-mcpu".into(), OsString::from_vec(b"\xFF".to_vec())]);
        assert!(matches!(line.rejected, Some(Rejected::UnknownValue { .. })));
    }
}
