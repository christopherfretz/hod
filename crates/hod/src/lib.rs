//! A debug-build code generator: the library facade.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role. The part of the contract
//! that the command-line tool programs against is in `spec/cli.md`
//! ("The facade"): [`Options`], [`compile`], and the two ways a compile can
//! decline, [`Unsupported`] (a named construct; the caller may fall back to
//! LLVM) and the other [`CompileError`]s.
//!
//! The input is a byte slice today because no IR model exists yet. The
//! IR-level entry point (`docs/ARCHITECTURE.md` "Input") will replace it;
//! [`Options`] and the error types are shaped to stay.

use std::fmt;
use std::path::PathBuf;

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
}

/// Why hod declined a module: the name is what the coverage log and the
/// strict-mode diagnostic print (`spec/cli.md` "Reasons").
///
/// The per-construct reasons of `hod_ir::classify` join this list when the
/// IR model lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Construct {
    /// No code generator exists yet: every module that passes the checks
    /// below is declined with this.
    CodegenNotImplemented,
    /// The input is LLVM bitcode; hod has no bitcode reader yet.
    BitcodeInput,
    /// Assembly output was requested; hod writes objects only.
    AssemblyOutput,
    /// The target is not one hod generates code for.
    UnsupportedTarget,
}

impl Construct {
    /// Every construct, in the order [`compile`] checks them.
    pub const ALL: [Construct; 4] = [
        Construct::AssemblyOutput,
        Construct::BitcodeInput,
        Construct::UnsupportedTarget,
        Construct::CodegenNotImplemented,
    ];

    /// The stable name.
    pub fn name(self) -> &'static str {
        match self {
            Construct::CodegenNotImplemented => "codegen-not-implemented",
            Construct::BitcodeInput => "bitcode-input",
            Construct::AssemblyOutput => "assembly-output",
            Construct::UnsupportedTarget => "unsupported-target",
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
}

impl Unsupported {
    /// A module-level reason (no function).
    pub fn module(construct: Construct) -> Self {
        Self { construct, function: None }
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported construct '{}'", self.construct)?;
        match &self.function {
            Some(function) => write!(f, " in function '{function}'"),
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
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Unsupported(unsupported) => unsupported.fmt(f),
            CompileError::MalformedInput(message) => write!(f, "malformed input: {message}"),
            CompileError::InvalidOptions(message) => write!(f, "invalid options: {message}"),
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

    /// `text` or `bitcode`.
    pub fn name(self) -> &'static str {
        match self {
            InputKind::Text => "text",
            InputKind::Bitcode => "bitcode",
        }
    }
}

/// How far into a text module [`module_triple`] looks.
pub const TRIPLE_SCAN_LIMIT: usize = 64 * 1024;

/// The `target triple = "..."` of a text module, if a line starting with it
/// begins within the first [`TRIPLE_SCAN_LIMIT`] bytes.
///
/// LLVM's printer, clang and kiln all write it in the module header. This is
/// a header scan, not a parse; it stands in for the IR reader until that
/// exists, and returns `None` for bitcode.
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
/// stated: `-mtriple=`, else the module's own (text modules only, see
/// [`module_triple`]).
pub fn effective_triple<'a>(input: &'a [u8], options: &'a Options) -> Option<&'a str> {
    options.triple.as_deref().or_else(|| module_triple(input))
}

/// Compiles one module to one object file.
///
/// `input` is a whole module, LLVM text or bitcode ([`InputKind::detect`]).
/// Today every call ends in [`CompileError::Unsupported`]; the checks run in
/// the order of [`Construct::ALL`], so the reason is deterministic:
///
/// 1. `assembly-output`: `options.output_kind` is not an object;
/// 2. `bitcode-input`: the input is bitcode;
/// 3. `unsupported-target`: `-march=` is given (the override is not
///    implemented), or the stated triple ([`effective_triple`]) is not one
///    [`target_supported`] accepts;
/// 4. `codegen-not-implemented`: everything else.
pub fn compile(input: &[u8], options: &Options) -> Result<Vec<u8>, CompileError> {
    if options.output_kind != OutputKind::Object {
        return Err(Unsupported::module(Construct::AssemblyOutput).into());
    }
    if InputKind::detect(input) == InputKind::Bitcode {
        return Err(Unsupported::module(Construct::BitcodeInput).into());
    }
    let target_ok =
        options.arch.is_none() && effective_triple(input, options).is_none_or(target_supported);
    if !target_ok {
        return Err(Unsupported::module(Construct::UnsupportedTarget).into());
    }
    Err(Unsupported::module(Construct::CodegenNotImplemented).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &[u8] = b"; ModuleID = 'm'\nsource_filename = \"m.cpp\"\n\
        target datalayout = \"e-m:e\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n\n\
        define i32 @f() {\n  ret i32 7\n}\n";

    fn object_options() -> Options {
        Options { output_kind: OutputKind::Object, ..Options::default() }
    }

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
    }

    // spec/cli.md "Reasons": the order of the checks.
    #[test]
    fn reasons_in_order() {
        let obj = object_options();
        assert_eq!(reason(MODULE, &obj), Construct::CodegenNotImplemented);
        assert_eq!(reason(b"", &obj), Construct::CodegenNotImplemented);
        assert_eq!(reason(b"BC\xC0\xDE....", &obj), Construct::BitcodeInput);
        assert_eq!(reason(MODULE, &Options::default()), Construct::AssemblyOutput);
        assert_eq!(reason(b"BC\xC0\xDE", &Options::default()), Construct::AssemblyOutput);
        let riscv = Options { triple: Some("riscv64-unknown-linux-gnu".into()), ..obj.clone() };
        assert_eq!(reason(MODULE, &riscv), Construct::UnsupportedTarget);
        assert_eq!(reason(b"BC\xC0\xDE", &riscv), Construct::BitcodeInput);
        let march = Options { arch: Some("aarch64".into()), ..obj.clone() };
        assert_eq!(reason(MODULE, &march), Construct::UnsupportedTarget);
        let x64 = Options { triple: Some("x86_64-pc-linux-gnu".into()), ..obj.clone() };
        assert_eq!(reason(MODULE, &x64), Construct::CodegenNotImplemented);
        let wasm = b"target triple = \"wasm32-unknown-unknown\"\n";
        assert_eq!(reason(wasm, &obj), Construct::UnsupportedTarget);
        // -mtriple overrides the module's triple.
        let linux = Options { triple: Some("aarch64-unknown-linux-gnu".into()), ..obj };
        assert_eq!(reason(wasm, &linux), Construct::CodegenNotImplemented);
    }

    #[test]
    fn construct_names_are_stable_and_distinct() {
        let names: Vec<&str> = Construct::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            ["assembly-output", "bitcode-input", "unsupported-target", "codegen-not-implemented"]
        );
        assert_eq!(Construct::BitcodeInput.to_string(), "bitcode-input");
    }

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

    #[test]
    fn error_text() {
        let module = Unsupported::module(Construct::BitcodeInput);
        assert_eq!(module.to_string(), "unsupported construct 'bitcode-input'");
        let function = Unsupported {
            construct: Construct::CodegenNotImplemented,
            function: Some("_Z1fv".into()),
        };
        assert_eq!(
            CompileError::from(function).to_string(),
            "unsupported construct 'codegen-not-implemented' in function '_Z1fv'"
        );
        assert_eq!(
            CompileError::MalformedInput("line 3: expected type".into()).to_string(),
            "malformed input: line 3: expected type"
        );
        assert_eq!(CompileError::InvalidOptions("x".into()).to_string(), "invalid options: x");
    }
}
