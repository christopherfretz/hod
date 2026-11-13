//! A debug-build code generator: the library facade.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role. The part of the contract
//! that the command-line tool programs against is in `spec/cli.md`
//! ("The facade"): [`Options`], [`compile`], and the two ways a compile can
//! decline, [`Unsupported`] (a named construct; the caller may fall back to
//! LLVM) and the other [`CompileError`]s.
//!
//! [`compile`] reads its input with `hod_ll` (text) or `hod_bc` (bitcode)
//! into a [`Session`], which checks it with `hod_ir::classify` as it is
//! fed and declines it with the first reason (the front half, `front.rs`
//! and `session.rs`); a module accepted whole goes to [`generate`], the
//! back half (`back.rs`): code generation with `hod_a64`, on worker
//! threads for a large module, and the object. A program that builds its
//! own `hod_ir` module (kiln's bridge) feeds a [`Session`] directly.
//!
//! What the `hod` binary and such a program share is here too
//! (`spec/cli.md` "The command line in the facade"): `llc`'s command line
//! ([`args`], [`Options::from_llc_args`]), the environment ([`env`],
//! [`Settings::from_env`]) with the jobserver client ([`jobserver`]), and
//! the coverage line ([`coverage`]).
//!
//! Feature `readers` (default) brings the readers and [`compile`]; a
//! program that builds `hod_ir` itself turns it off. What such a program
//! may rely on, versioned by [`EMBED_API`], and how a panic of hod's is
//! contained ([`CompileError::Panic`]) is `spec/cli.md` "Embedding".

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

pub mod args;
mod back;
mod contain;
pub mod coverage;
pub mod env;
mod front;
pub mod jobserver;
mod session;

pub use hod_obj::Compression;

pub use args::ArgError;
pub use env::{EnvError, Environment};

pub use back::{
    LIMIT_OBJECT, OBJECT_NUL_IN_NAME, OPTION_RELOCATION_MODEL, OPTION_SPLIT_DWARF,
    PARALLEL_MIN_INSTS, ThreadTokens, Threads, generate_with,
};
#[cfg(feature = "readers")]
pub use front::compile_with;
pub use front::{
    BitcodeReading, INTERNAL_ERROR, INTERNAL_PANIC, LIMIT_READER, READ_BYTES_PER_THREAD,
    READ_MAX_THREADS, READ_MIN_BODY_BYTES, Report, Settings, generate, read_threads,
};
pub use session::Session;
/// The names of the bodies declined because their meaning depends on being
/// inlined (`spec/cli.md` "Always-inline").
pub use session::inlining::reason as inline_reason;

/// The version of the producer surface (`spec/cli.md` "Embedding"): the
/// items a program that compiles in process uses (the [`Session`] and its
/// calls, [`Options`], [`Settings`], [`Environment`], [`Report`],
/// [`CompileError`] and the reason names, [`Threads`], [`coverage`],
/// [`jobserver`]), `hod_ir`'s module-building and builder API with its
/// producer kit, and what "Embedding" promises of them. It goes up by one
/// with every change that can break such a program, recorded in the
/// spec's table; additions that cannot leave it as it is. kiln's bridge
/// asserts it at compile time.
pub const EMBED_API: u32 = 1;

/// What [`compile`] writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputKind {
    /// Assembly text (`llc`'s default, `-filetype=asm`). hod has no
    /// assembly printer: always [`Construct::AssemblyOutput`].
    #[default]
    Assembly,
    /// A relocatable object file (`-filetype=obj`).
    Object,
}

/// `llc`'s `-O<n>`. hod generates the same code at every level; the level is
/// recorded because the caller passed it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptLevel {
    O0,
    O1,
    /// `llc`'s default.
    #[default]
    O2,
    O3,
}

/// `llc`'s `-relocation-model=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocModel {
    Static,
    Pic,
    DynamicNoPic,
    Ropi,
    Rwpi,
    RopiRwpi,
}

/// `llc`'s `-code-model=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeModel {
    Tiny,
    Small,
    Kernel,
    Medium,
    Large,
}

/// `llc`'s `-debugger-tune=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebuggerTuning {
    Gdb,
    Lldb,
    Dbx,
    Sce,
}

/// `llc`'s `-fp-contract=`: whether the backend may fuse a multiply and an
/// add across statements. Fusing is a permission, never an obligation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpContract {
    Fast,
    On,
    Off,
}

/// Everything a caller can ask of one compile, in `llc`'s terms.
///
/// [`Options::default`] is `llc` run with no options: assembly output at
/// `-O2`, everything else left to the module and the target.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `-mtriple=`: overrides the module's `target triple`.
    pub triple: Option<String>,
    /// `-march=`: `llc`'s architecture override.
    pub arch: Option<String>,
    /// `-mcpu=`.
    pub cpu: Option<String>,
    /// `-mattr=`: target features, each `+name` or `-name`, in order.
    pub features: Vec<String>,
    /// `-O<n>`.
    pub opt_level: OptLevel,
    /// `-relocation-model=`; `None` is the target's default.
    pub relocation_model: Option<RelocModel>,
    /// `-code-model=`; `None` is the module's or the target's default.
    pub code_model: Option<CodeModel>,
    /// `-function-sections`.
    pub function_sections: bool,
    /// `-data-sections`.
    pub data_sections: bool,
    /// `-fp-contract=`.
    pub fp_contract: Option<FpContract>,
    /// `-x86-branches-within-32B-boundaries`.
    pub x86_branches_within_32b: bool,
    /// `-debugger-tune=`.
    pub debugger_tuning: Option<DebuggerTuning>,
    /// `-split-dwarf-file=`: the `.dwo` name recorded in the skeleton unit.
    pub split_dwarf_file: Option<PathBuf>,
    /// `-split-dwarf-output=`: where the `.dwo` file is written.
    pub split_dwarf_output: Option<PathBuf>,
    /// `-filetype=`.
    pub output_kind: OutputKind,
    /// hod's `--compress-debug-sections=` (not `llc`'s; `clang -gz`'s):
    /// the `.debug_*` sections as `SHF_COMPRESSED` zlib streams
    /// (`spec/object.md` "Compressed sections"). A module handed to the
    /// fallback is written as the fallback writes it, uncompressed.
    pub debug_compression: Compression,
}

/// Why hod declined a module: the name is what the coverage log and the
/// strict-mode diagnostic print (`spec/cli.md` "Reasons").
///
/// The three fixed reasons are the facade's own; every other reason is
/// [`Construct::Named`]: a name of `hod_ir::classify`'s vocabulary
/// (`spec/ir.md` §8.5) or of a reader (`spec/cli.md` "Reasons").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Construct {
    /// The input is LLVM bitcode and the caller asked for it not to be
    /// read ([`BitcodeReading::Decline`]).
    BitcodeInput,
    /// Assembly output was requested; hod writes objects only.
    AssemblyOutput,
    /// The target is not one hod generates code for.
    UnsupportedTarget,
    /// A construct named by `classify` (`group:name`) or by a reader
    /// (`bitcode:<name>`, [`LIMIT_READER`]).
    Named(Cow<'static, str>),
}

impl Construct {
    /// The fixed constructs, in the order [`compile`] checks them.
    pub const ALL: [Construct; 3] =
        [Construct::AssemblyOutput, Construct::BitcodeInput, Construct::UnsupportedTarget];

    /// The stable name.
    pub fn name(&self) -> &str {
        match self {
            Construct::BitcodeInput => "bitcode-input",
            Construct::AssemblyOutput => "assembly-output",
            Construct::UnsupportedTarget => "unsupported-target",
            Construct::Named(name) => name,
        }
    }
}

impl fmt::Display for Construct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// hod declined the module: it contains something hod does not generate code
/// for. Nothing was written. The caller may hand the module to LLVM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// What was not supported.
    pub construct: Construct,
    /// The function it was found in, when it is inside one.
    pub function: Option<String>,
    /// What the name does not say, for the diagnostic: the feature an
    /// `attr:target-feature-*` reason refused. Not in the coverage log.
    pub detail: Option<String>,
}

impl Unsupported {
    /// A module-level reason (no function).
    pub fn module(construct: Construct) -> Self {
        Self { construct, function: None, detail: None }
    }

    /// A reason found in `function`.
    pub fn in_function(construct: Construct, function: Option<String>) -> Self {
        Self { construct, function, detail: None }
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported construct '{}'", self.construct)?;
        if let Some(function) = &self.function {
            write!(f, " in function '{function}'")?;
        }
        match &self.detail {
            Some(detail) => write!(f, ": {detail}"),
            None => Ok(()),
        }
    }
}

/// Why [`compile`] produced no object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    /// The input is fine as far as hod can tell, but hod does not support
    /// something in it.
    Unsupported(Unsupported),
    /// The input is not a valid module; the message says why.
    MalformedInput(String),
    /// The options contradict each other or the input; the message says how.
    InvalidOptions(String),
    /// Code generation failed in a way that is a bug in hod (or input
    /// `classify` and `verify` should have refused), not an unsupported
    /// construct: `CgError::Internal` (`spec/codegen.md` §10). Nothing was
    /// written; the caller may still hand the module to LLVM.
    Internal(String),
    /// hod panicked, and the [`Session`] (or [`compile_with`]) contained
    /// the panic (`spec/cli.md` "Embedding"): a bug of hod's, as
    /// [`CompileError::Internal`], reason [`INTERNAL_PANIC`]. The message
    /// is the panic's. Nothing was written, nothing is left running, and
    /// the session that met it answers this from every later call; the
    /// caller may hand the module to LLVM.
    Panic(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Unsupported(unsupported) => unsupported.fmt(f),
            CompileError::MalformedInput(message) => write!(f, "malformed input: {message}"),
            CompileError::InvalidOptions(message) => write!(f, "invalid options: {message}"),
            CompileError::Internal(message) => write!(f, "internal error: {message}"),
            CompileError::Panic(message) => write!(f, "internal error: hod panicked: {message}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<Unsupported> for CompileError {
    fn from(unsupported: Unsupported) -> Self {
        CompileError::Unsupported(unsupported)
    }
}

/// The encoding of a module handed to [`compile`] (`spec/cli.md` "Input").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// LLVM textual IR.
    Text,
    /// LLVM bitcode, bare or in the bitcode wrapper.
    Bitcode,
    /// A module built in process and handed to a [`Session`] (coverage
    /// field 3, `spec/cli.md` "Coverage log"). [`InputKind::detect`]
    /// never says it: bytes are text or bitcode.
    Module,
}

impl InputKind {
    /// Tells the encoding from the first four bytes: the bitcode magic
    /// `'B' 'C' 0xC0 0xDE`, or the bitcode wrapper magic `0x0B17C0DE`
    /// (little-endian, so `DE C0 17 0B`), is bitcode; anything else is text.
    pub fn detect(input: &[u8]) -> Self {
        const RAW: [u8; 4] = [b'B', b'C', 0xC0, 0xDE];
        const WRAPPER: [u8; 4] = 0x0B17_C0DE_u32.to_le_bytes();
        if input.starts_with(&RAW) || input.starts_with(&WRAPPER) {
            InputKind::Bitcode
        } else {
            InputKind::Text
        }
    }

    /// `text`, `bitcode` or `module`.
    pub fn name(self) -> &'static str {
        match self {
            InputKind::Text => "text",
            InputKind::Bitcode => "bitcode",
            InputKind::Module => "module",
        }
    }
}

/// How far into a text module [`module_triple`] looks.
pub const TRIPLE_SCAN_LIMIT: usize = 64 * 1024;

/// The `target triple = "..."` of a text module, if a line starting with it
/// begins within the first [`TRIPLE_SCAN_LIMIT`] bytes.
///
/// LLVM's printer, clang and kiln all write it in the module header. This is
/// a header scan, not a parse: the stated target is checked with it before
/// the module is read. `None` for bitcode.
pub fn module_triple(input: &[u8]) -> Option<&str> {
    const PREFIX: &[u8] = b"target triple";
    if InputKind::detect(input) == InputKind::Bitcode {
        return None;
    }
    let mut start = 0;
    while start < input.len().min(TRIPLE_SCAN_LIMIT) {
        let rest = &input[start..];
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = &rest[..end];
        if line.starts_with(PREFIX) {
            let open = line.iter().position(|&b| b == b'"')?;
            let value = &line[open + 1..];
            let close = value.iter().position(|&b| b == b'"')?;
            return std::str::from_utf8(&value[..close]).ok();
        }
        start += end + 1;
    }
    None
}

/// Whether hod generates code for `triple`: AArch64 or x86-64, Linux, ELF
/// (`docs/ARCHITECTURE.md` "Object file"). The architecture is the first
/// component (`aarch64` or `x86_64`), a later component must be `linux`, and
/// what follows it, if anything, must be a GNU or musl environment
/// (`gnu`, `gnueabi...`, `musl`): Android and the other Linux environments
/// have their own ABI details and are not claimed.
pub fn target_supported(triple: &str) -> bool {
    let mut parts = triple.split('-');
    let arch = parts.next().unwrap_or_default();
    if !matches!(arch, "aarch64" | "x86_64") || !parts.any(|part| part == "linux") {
        return false;
    }
    match parts.next() {
        None => true,
        Some(env) => (env.starts_with("gnu") || env.starts_with("musl")) && parts.next().is_none(),
    }
}

/// The triple a compile of `input` with `options` targets, when one is
/// stated before reading it: `-mtriple=`, else the module's own (text
/// modules only, see [`module_triple`]; [`Report::target`] has a bitcode
/// module's).
pub fn effective_triple<'a>(input: &'a [u8], options: &'a Options) -> Option<&'a str> {
    options.triple.as_deref().or_else(|| module_triple(input))
}

/// Compiles one module to one object file.
///
/// `input` is a whole module, LLVM text or bitcode ([`InputKind::detect`]),
/// both read natively. The checks run in this order, and the first that
/// applies is the answer (`spec/cli.md` "Reasons"):
///
/// 1. `assembly-output`: `options.output_kind` is not an object;
/// 2. `unsupported-target`: `-march=` is given, or the stated triple
///    ([`effective_triple`]) is not one [`target_supported`] accepts;
/// 3. the module level is read: a reader's refusal by name, or malformed
///    input;
/// 4. `classify`'s module pass;
/// 5. each body, as it is read: the reader's refusal, `classify`'s body
///    pass, `Body::verify`;
/// 6. [`generate`] for a module accepted whole: the object, or code
///    generation's refusal (`spec/cli.md` "The back half").
///
/// [`compile_with`] is the same with [`Settings`] and a [`Report`].
///
/// # Errors
/// Why no object was produced.
#[cfg(feature = "readers")]
pub fn compile(input: &[u8], options: &Options) -> Result<Vec<u8>, CompileError> {
    compile_with(input, options, &Settings::default(), &mut Report::default())
}

#[cfg(all(test, feature = "readers"))]
mod tests {
    use super::*;

    const MODULE: &[u8] = b"; ModuleID = 'm'\nsource_filename = \"m.cpp\"\n\
        target datalayout = \"e-m:e\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n\n\
        define i32 @f() {\n  ret i32 7\n}\n";

    fn object_options() -> Options {
        Options { output_kind: OutputKind::Object, ..Options::default() }
    }

    /// Declarations only: `classify` accepts the module whole.
    const DECLARATIONS: &[u8] =
        b"target triple = \"aarch64-unknown-linux-gnu\"\ndeclare i32 @g()\n";

    fn reason(input: &[u8], options: &Options) -> Construct {
        match compile(input, options) {
            Err(CompileError::Unsupported(u)) => {
                assert_eq!(u.function, None);
                u.construct
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    // spec/cli.md "Input": the two bitcode magics; everything else is text.
    #[test]
    fn input_kind_detection() {
        assert_eq!(InputKind::detect(b"BC\xC0\xDE\x35\x14"), InputKind::Bitcode);
        assert_eq!(InputKind::detect(b"\xDE\xC0\x17\x0B\0\0\0\0"), InputKind::Bitcode);
        assert_eq!(InputKind::detect(MODULE), InputKind::Text);
        assert_eq!(InputKind::detect(b""), InputKind::Text);
        assert_eq!(InputKind::detect(b"BC\xC0"), InputKind::Text);
        assert_eq!(InputKind::detect(b"\x0B\x17\xC0\xDE"), InputKind::Text);
        assert_eq!(InputKind::Text.name(), "text");
        assert_eq!(InputKind::Bitcode.name(), "bitcode");
        assert_eq!(InputKind::Module.name(), "module");
    }

    // spec/cli.md "Reasons": the order of the checks before the module is
    // read, and the last line.
    #[test]
    fn reasons_in_order() {
        let obj = object_options();
        let named = |name: &'static str| Construct::Named(name.into());
        // A module `classify` and code generation accept whole is an
        // object (`DECLARATIONS` has nothing to compile).
        assert!(compile(DECLARATIONS, &obj).is_ok_and(|o| o.starts_with(b"\x7FELF")));
        // The stages, with constructs no package is about to support:
        // the tiny code model (`module:code-model-tiny`, the module pass) and `ghccc`
        // (`cc:ghccc`, unplanned, the body pass).
        let triple = "target triple = \"aarch64-unknown-linux-gnu\"\n";
        let module_asm = format!(
            "{triple}!llvm.module.flags = !{{!900}}\n!900 = !{{i32 1, !\"Code Model\", i32 0}}\ndeclare i32 @g()\n"
        );
        assert_eq!(reason(module_asm.as_bytes(), &obj), named("module:code-model-tiny"));
        let ghc = "define ghccc void @h() nounwind {\n  ret void\n}\n";
        let body = format!("{triple}{ghc}");
        let refused = Unsupported::in_function(named("cc:ghccc"), Some("h".into()));
        assert_eq!(compile(body.as_bytes(), &obj), Err(refused.into()));
        // The module pass comes before any body.
        let both = format!("{module_asm}{ghc}");
        assert_eq!(reason(both.as_bytes(), &obj), named("module:code-model-tiny"));
        // A definition `classify` accepts whole reaches `generate`, its
        // body read, classified and verified, and becomes an object.
        let accepted = format!("{triple}define i32 @f() nounwind {{\n  ret i32 7\n}}\n");
        let mut report = Report::default();
        let generated = compile_with(accepted.as_bytes(), &obj, &Settings::default(), &mut report);
        assert!(generated.is_ok_and(|o| o.starts_with(b"\x7FELF")));
        let phases: Vec<&str> = report.phases.iter().map(|&(name, _)| name).collect();
        assert_eq!(phases, ["parse", "classify", "bodies", "emission", "object"]);
        // MODULE's data layout is not the target's.
        assert_eq!(reason(MODULE, &obj), Construct::UnsupportedTarget);
        // No stated target: hod does not guess the host's.
        assert_eq!(reason(b"", &obj), Construct::UnsupportedTarget);
        assert_eq!(reason(MODULE, &Options::default()), Construct::AssemblyOutput);
        assert_eq!(reason(b"BC\xC0\xDE", &Options::default()), Construct::AssemblyOutput);
        let riscv = Options { triple: Some("riscv64-unknown-linux-gnu".into()), ..obj.clone() };
        assert_eq!(reason(MODULE, &riscv), Construct::UnsupportedTarget);
        assert_eq!(reason(b"BC\xC0\xDE", &riscv), Construct::UnsupportedTarget);
        let march = Options { arch: Some("aarch64".into()), ..obj.clone() };
        assert_eq!(reason(MODULE, &march), Construct::UnsupportedTarget);
        let x64 = Options { triple: Some("x86_64-pc-linux-gnu".into()), ..obj.clone() };
        let x64_decls = b"target triple = \"x86_64-pc-linux-gnu\"\ndeclare i32 @g()\n";
        assert!(compile(x64_decls, &x64).is_ok_and(|o| o[18..20] == [0x3e, 0]));
        // -mtriple names another target than the module's: not retargeted.
        assert_eq!(reason(DECLARATIONS, &x64), Construct::UnsupportedTarget);
        let wasm = b"target triple = \"wasm32-unknown-unknown\"\n";
        assert_eq!(reason(wasm, &obj), Construct::UnsupportedTarget);
        // -mtriple overrides the module's triple in the check, and the
        // module is then read for its own.
        let linux = Options { triple: Some("aarch64-unknown-linux-gnu".into()), ..obj.clone() };
        assert_eq!(reason(wasm, &linux), Construct::UnsupportedTarget);
        // -mtriple is the target of a module that names none.
        assert!(compile(b"declare i32 @g()\n", &linux).is_ok());
        // Bitcode the caller does not want read.
        let decline = Settings { bitcode: BitcodeReading::Decline, ..Settings::default() };
        let declined = compile_with(b"BC\xC0\xDE....", &obj, &decline, &mut Report::default());
        assert_eq!(declined, Err(Unsupported::module(Construct::BitcodeInput).into()));
        let asm =
            compile_with(b"BC\xC0\xDE", &Options::default(), &decline, &mut Report::default());
        assert_eq!(asm, Err(Unsupported::module(Construct::AssemblyOutput).into()));
    }

    // spec/cli.md "Reasons": the names are the coverage log's vocabulary.
    #[test]
    fn construct_names_are_stable_and_distinct() {
        let names: Vec<&str> = Construct::ALL.iter().map(Construct::name).collect();
        assert_eq!(names, ["assembly-output", "bitcode-input", "unsupported-target"]);
        assert_eq!(Construct::BitcodeInput.to_string(), "bitcode-input");
        assert_eq!(Construct::Named("cc:ccc".into()).name(), "cc:ccc");
        assert_eq!(Construct::Named(LIMIT_READER.into()).to_string(), "limit:reader");
    }

    // spec/cli.md "Input": the module's own target, from its header.
    #[test]
    fn module_triple_scan() {
        assert_eq!(module_triple(MODULE), Some("aarch64-unknown-linux-gnu"));
        assert_eq!(
            module_triple(b"target triple = \"x86_64-pc-linux-gnu\""),
            Some("x86_64-pc-linux-gnu")
        );
        assert_eq!(module_triple(b"define void @f() {\n  ret void\n}\n"), None);
        assert_eq!(module_triple(b""), None);
        assert_eq!(module_triple(b"target triple = \n"), None);
        assert_eq!(module_triple(b"target triple = \"unterminated\n"), None);
        assert_eq!(module_triple(b"BC\xC0\xDEtarget triple = \"a-b-c\"\n"), None);
        // Only lines that begin within the limit are looked at.
        let mut late = vec![b';'; TRIPLE_SCAN_LIMIT - 1];
        late.push(b'\n');
        late.extend_from_slice(b"target triple = \"aarch64-unknown-linux-gnu\"\n");
        assert_eq!(module_triple(&late), None);
        late.remove(0);
        assert_eq!(module_triple(&late), Some("aarch64-unknown-linux-gnu"));
    }

    // spec/cli.md "Reasons": what a supported target is.
    #[test]
    fn supported_targets() {
        for triple in [
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-linux-gnu",
            "aarch64-linux-gnu",
            "x86_64-unknown-linux-musl",
        ] {
            assert!(target_supported(triple), "{triple}");
        }
        for triple in [
            "",
            "aarch64",
            "arm64-apple-macosx14.0.0",
            "x86_64-pc-windows-msvc",
            "riscv64-unknown-linux-gnu",
            "aarch64_be-unknown-linux-gnu",
            "i386-unknown-linux-gnu",
            "aarch64-unknown-linux-android",
            "x86_64-unknown-linux-gnu-extra",
            "x86_64-linux-unknown-gnu",
        ] {
            assert!(!target_supported(triple), "{triple}");
        }
    }

    // spec/cli.md "The facade": how the errors display.
    #[test]
    fn error_text() {
        let module = Unsupported::module(Construct::BitcodeInput);
        assert_eq!(module.to_string(), "unsupported construct 'bitcode-input'");
        let function =
            Unsupported::in_function(Construct::Named("x64:todo:abi".into()), Some("_Z1fv".into()));
        assert_eq!(
            CompileError::from(function).to_string(),
            "unsupported construct 'x64:todo:abi' in function '_Z1fv'"
        );
        let feature = Unsupported {
            construct: Construct::Named("attr:target-feature-unknown".into()),
            function: Some("f".into()),
            detail: Some("+bogus".into()),
        };
        assert_eq!(
            feature.to_string(),
            "unsupported construct 'attr:target-feature-unknown' in function 'f': +bogus"
        );
        assert_eq!(
            CompileError::Internal("function 'f': a lost value".into()).to_string(),
            "internal error: function 'f': a lost value"
        );
        assert_eq!(
            CompileError::MalformedInput("line 3: expected type".into()).to_string(),
            "malformed input: line 3: expected type"
        );
        assert_eq!(CompileError::InvalidOptions("x".into()).to_string(), "invalid options: x");
        assert_eq!(
            CompileError::Panic("index out of bounds".into()).to_string(),
            "internal error: hod panicked: index out of bounds"
        );
    }
}
