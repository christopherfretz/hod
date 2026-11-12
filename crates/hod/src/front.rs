//! The front half of [`crate::compile`]: read the module, check it, and
//! decline it with the first reason (`spec/cli.md` "Reasons", "The front
//! half"). Code generation is behind one call, [`generate`].
//!
//! The order is the cheap one: what needs no reading, then the module
//! level (read, then `classify`'s module pass), then the bodies, streamed
//! one at a time (read, the reader's own refusals, `classify`'s body pass,
//! `Body::verify`). A declined module is handed to `llc` next, so the
//! first refusal ends the work unless [`Settings::census`] asks for every
//! reason.

use std::borrow::Cow;
use std::time::Instant;

use hod_bc::{BcError, BcErrorKind};
use hod_ir::{Body, Census, Classifier, IrErrorKind, Module, Rejection, RejectionKind, SymId};
use hod_ir::{Support, TargetSpec};
use hod_ll::{LlError, LlErrorKind};

use crate::{
    CompileError, Construct, InputKind, Options, OutputKind, Unsupported, module_triple,
    target_supported,
};

/// How a bitcode input is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BitcodeReading {
    /// With `hod_bc`.
    #[default]
    Native,
    /// Not at all: declined as `bitcode-input`, for a caller that converts
    /// bitcode to text itself (`spec/cli.md` "Bitcode conversion").
    Decline,
}

/// How [`compile_with`] works; not an `llc` option. The default reads
/// bitcode natively and stops at the first reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// How a bitcode input is read.
    pub bitcode: BitcodeReading,
    /// Find every reason the module is declined, not only the first
    /// ([`Report::reasons`]). The answer is the same; it costs reading
    /// every body.
    pub census: bool,
    /// The matrix `classify` decides with; `None` is
    /// `Support::shipped` for the module's target. For tests and
    /// measurements of what is not supported yet.
    pub support: Option<Support>,
}

/// What [`compile_with`] tells besides its answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The stated target: `-mtriple`, else the module's own triple (for
    /// text, as its header writes it; for bitcode, the target the reader
    /// made of it, once the module level was read).
    pub target: Option<String>,
    /// The phases that ended, in order, with the moment each ended
    /// (`spec/cli.md` "Phase report").
    pub phases: Vec<(&'static str, Instant)>,
    /// With [`Settings::census`]: every reason found, each once, in the
    /// order first met. The first is the answer's. Empty otherwise.
    pub reasons: Vec<String>,
}

impl Report {
    fn phase(&mut self, name: &'static str) {
        self.phases.push((name, Instant::now()));
    }

    fn reason(&mut self, name: &str) {
        if !self.reasons.iter().any(|known| known == name) {
            self.reasons.push(name.to_owned());
        }
    }
}

/// The reason name of a decline, as the coverage log writes it.
fn reason_name(error: &CompileError) -> &str {
    match error {
        CompileError::Unsupported(unsupported) => unsupported.construct.name(),
        CompileError::MalformedInput(_) => "malformed-input",
        CompileError::InvalidOptions(_) => "invalid-options",
    }
}

fn named(name: impl Into<Cow<'static, str>>) -> Construct {
    Construct::Named(name.into())
}

fn decline(construct: Construct) -> CompileError {
    Unsupported::module(construct).into()
}

/// The reason of a reader failure that is not about the input's
/// validity: a target hod has no description for, a limit of hod's.
fn ir_reason(kind: IrErrorKind) -> Option<Construct> {
    match kind {
        IrErrorKind::UnsupportedTarget | IrErrorKind::DataLayoutMismatch => {
            Some(Construct::UnsupportedTarget)
        }
        IrErrorKind::TableFull | IrErrorKind::TypeTooLarge => Some(named(LIMIT_READER)),
        _ => None,
    }
}

/// The reason for a limit of a reader (`hod_ll`'s or `hod_bc`'s, or a
/// table of `hod_ir` they fill): LLVM may take what hod does not.
pub const LIMIT_READER: &str = "limit:reader";

/// `spec/cli.md` "Reasons": what a text reader's error is.
fn from_ll(error: &LlError) -> CompileError {
    let construct = match error.kind {
        LlErrorKind::Target => Some(Construct::UnsupportedTarget),
        LlErrorKind::Limit => Some(named(LIMIT_READER)),
        LlErrorKind::Ir(kind) => ir_reason(kind),
        _ => None,
    };
    construct.map_or_else(|| CompileError::MalformedInput(format!("line {error}")), decline)
}

/// `spec/cli.md` "Reasons": what a bitcode reader's error is.
fn from_bc(error: &BcError) -> CompileError {
    let construct = match error.kind {
        BcErrorKind::Unsupported(name) => Some(named(format!("bitcode:{name}"))),
        BcErrorKind::Target => Some(Construct::UnsupportedTarget),
        BcErrorKind::Limit => Some(named(LIMIT_READER)),
        BcErrorKind::Ir(kind) => ir_reason(kind),
        _ => None,
    };
    construct.map_or_else(|| CompileError::MalformedInput(error.to_string()), decline)
}

/// Why the walk over the bodies stopped early.
struct Stop(CompileError);

impl From<LlError> for Stop {
    fn from(error: LlError) -> Self {
        Stop(from_ll(&error))
    }
}

impl From<BcError> for Stop {
    fn from(error: BcError) -> Self {
        Stop(from_bc(&error))
    }
}

fn symbol_name(module: &Module, sym: SymId) -> Option<String> {
    (sym.index() < module.symbol_count())
        .then(|| String::from_utf8_lossy(module.symbol(sym).name()).into_owned())
}

/// A `classify` refusal as a decline (`spec/cli.md` "Reasons"): the
/// function is named for the body pass.
fn from_rejection(module: &Module, rejection: Rejection, body: bool) -> CompileError {
    let function =
        if body { rejection.symbol.and_then(|sym| symbol_name(module, sym)) } else { None };
    match rejection.kind {
        RejectionKind::Unsupported => {
            Unsupported { construct: Construct::Named(rejection.construct), function }.into()
        }
        RejectionKind::Invalid => CompileError::MalformedInput(match function {
            Some(function) => format!("{} in function '{function}'", rejection.construct),
            None => rejection.construct.into_owned(),
        }),
    }
}

/// The checks and the bookkeeping shared by the text and the bitcode
/// walks over a module.
struct Judge<'s> {
    census: Option<Census>,
    support: &'s Support,
    first: Option<CompileError>,
    report: &'s mut Report,
    accepted: Vec<Body>,
}

impl<'s> Judge<'s> {
    /// Judges `module`'s module level: `classify`'s module pass.
    fn new(module: &Module, settings: &'s Settings, report: &'s mut Report) -> Judge<'s> {
        let mut judge = Judge {
            census: settings.census.then(Census::new),
            support: settings
                .support
                .as_ref()
                .unwrap_or_else(|| Support::shipped(module.target().arch())),
            first: None,
            report,
            accepted: Vec::new(),
        };
        judge.module(module);
        judge
    }

    fn classifier(&self) -> Classifier<'_> {
        Classifier::new(self.support)
    }

    fn note(&mut self, error: CompileError) {
        if self.census.is_some() {
            self.report.reason(reason_name(&error));
        }
        self.first.get_or_insert(error);
    }

    /// The census findings so far, into the report's reasons.
    fn drain_census(&mut self) {
        if let Some(census) = &mut self.census {
            for entry in census.entries() {
                let name = &entry.construct;
                if !self.report.reasons.iter().any(|known| known == name) {
                    self.report.reasons.push(name.clone());
                }
            }
            *census = Census::new();
        }
    }

    fn module(&mut self, module: &Module) {
        if self.census.is_some() {
            let mut census = Census::new();
            self.classifier().census_module(module, &mut census);
            self.census = Some(census);
            self.drain_census();
        }
        if let Err(rejection) = self.classifier().module(module) {
            let error = from_rejection(module, rejection, false);
            self.note(error);
        }
        self.report.phase("classify");
    }

    /// Ends a step of the walk: stop at the first reason unless every
    /// reason is asked for.
    fn verdict(&mut self) -> Result<(), Stop> {
        if self.census.is_some() {
            return Ok(());
        }
        self.first.take().map_or(Ok(()), |error| Err(Stop(error)))
    }

    /// One body as a reader delivered it: `None` with the reader's own
    /// refusals, or the body.
    fn body(
        &mut self,
        module: &Module,
        sym: SymId,
        body: Option<&Body>,
        refused: &[Cow<'static, str>],
    ) -> Result<(), Stop> {
        match body {
            None => {
                if self.census.is_some() {
                    for name in refused {
                        self.report.reason(name);
                    }
                }
                let function = symbol_name(module, sym);
                let error = match refused.first() {
                    Some(first) => {
                        Unsupported { construct: Construct::Named(first.clone()), function }.into()
                    }
                    // Defensive: a reader names what it refused.
                    None => CompileError::MalformedInput(format!(
                        "no body was read for function '{}'",
                        function.unwrap_or_default()
                    )),
                };
                self.note(error);
            }
            Some(body) => {
                if let Some(census) = &mut self.census {
                    Classifier::new(self.support).census_body(module, body, census);
                    self.drain_census();
                }
                if self.first.is_none() {
                    match self.classifier().body(module, body) {
                        Err(rejection) => self.note(from_rejection(module, rejection, true)),
                        // Code generation may assume what `verify` checks
                        // (`spec/ir.md` §5.8): a body that reaches it is
                        // verified, always.
                        Ok(()) => match body.verify(module) {
                            Err(error) => {
                                let function = symbol_name(module, sym).unwrap_or_default();
                                self.note(CompileError::MalformedInput(format!(
                                    "function '{function}': {error}"
                                )));
                            }
                            Ok(()) => self.accepted.push(body.clone()),
                        },
                    }
                }
            }
        }
        self.verdict()
    }

    /// The end of the walk: the first reason, or the module and its bodies
    /// for code generation.
    fn finish(
        self,
        module: &Module,
        walked: Result<(), Stop>,
        options: &Options,
    ) -> Result<Vec<u8>, CompileError> {
        let Judge { first, report, accepted, census, .. } = self;
        if let Err(Stop(error)) = walked {
            if census.is_some() {
                report.reason(reason_name(&error));
            }
            return Err(first.unwrap_or(error));
        }
        report.phase("bodies");
        if let Some(error) = first {
            return Err(error);
        }
        generate(module, &accepted, options)
    }
}

/// The back half: the object for `module`, which `classify` accepted
/// whole, and its function bodies, every one classified and verified, in
/// the order the module defines them.
///
/// There is no code generator yet: every module that gets here is
/// declined with `codegen-not-implemented`. WP-043 replaces this body.
pub fn generate(
    module: &Module,
    bodies: &[Body],
    options: &Options,
) -> Result<Vec<u8>, CompileError> {
    let _ = (module, bodies, options);
    Err(decline(Construct::CodegenNotImplemented))
}

/// [`crate::compile`] with its settings and report (`spec/cli.md` "The
/// facade", "Reasons").
///
/// # Errors
/// As [`crate::compile`].
pub fn compile_with(
    input: &[u8],
    options: &Options,
    settings: &Settings,
    report: &mut Report,
) -> Result<Vec<u8>, CompileError> {
    let result = front(input, options, settings, report);
    if settings.census
        && report.reasons.is_empty()
        && let Err(error) = &result
    {
        report.reason(reason_name(error));
    }
    result
}

fn front(
    input: &[u8],
    options: &Options,
    settings: &Settings,
    report: &mut Report,
) -> Result<Vec<u8>, CompileError> {
    if options.output_kind != OutputKind::Object {
        return Err(decline(Construct::AssemblyOutput));
    }
    let kind = InputKind::detect(input);
    if kind == InputKind::Bitcode && settings.bitcode == BitcodeReading::Decline {
        return Err(decline(Construct::BitcodeInput));
    }
    report.target.clone_from(&options.triple);
    if kind == InputKind::Text && report.target.is_none() {
        report.target = module_triple(input).map(str::to_owned);
    }
    if options.arch.is_some() || !report.target.as_deref().is_none_or(target_supported) {
        return Err(decline(Construct::UnsupportedTarget));
    }
    // `-mtriple` is what the module is compiled for; a module that names
    // another target is not retargeted (not implemented).
    let forced = match options.triple.as_deref().map(TargetSpec::from_triple) {
        None => None,
        Some(Ok(target)) => Some(target),
        Some(Err(_)) => return Err(decline(Construct::UnsupportedTarget)),
    };
    let retargeted = |module: &Module| forced.as_ref().is_some_and(|t| t != module.target());

    match kind {
        InputKind::Text => {
            let parse = hod_ll::ParseOptions { default_target: forced.clone() };
            let hod_ll::ParsedModule { mut module, text } =
                hod_ll::parse_module(input, &parse).map_err(|error| from_ll(&error))?;
            report.phase("parse");
            if retargeted(&module) {
                return Err(decline(Construct::UnsupportedTarget));
            }
            let mut judge = Judge::new(&module, settings, report);
            let walked = judge.verdict().and_then(|()| {
                hod_ll::parse_bodies(&mut module, &text, |module, parsed| {
                    judge.body(module, parsed.function.sym, parsed.body, parsed.unsupported)
                })
            });
            judge.finish(&module, walked, options)
        }
        InputKind::Bitcode => {
            let read = hod_bc::ReadOptions { default_target: forced.clone() };
            let hod_bc::BcModule { mut module, bits } =
                hod_bc::read_module(input, &read).map_err(|error| from_bc(&error))?;
            report.phase("parse");
            if options.triple.is_none() {
                report.target = Some(module.target().triple().to_owned());
            }
            if retargeted(&module) {
                return Err(decline(Construct::UnsupportedTarget));
            }
            let mut judge = Judge::new(&module, settings, report);
            let walked = judge.verdict().and_then(|()| {
                hod_bc::read_bodies(&mut module, &bits, |module, read| {
                    judge.body(module, read.function.sym, read.body, read.unsupported)
                })
            });
            judge.finish(&module, walked, options)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ll(kind: LlErrorKind) -> CompileError {
        from_ll(&LlError { kind, offset: 5, line: 2, column: 3, message: "what".into() })
    }

    fn bc(kind: BcErrorKind) -> CompileError {
        from_bc(&BcError { kind, bit: 64, message: "what".into() })
    }

    // spec/cli.md "Reasons", the readers' table: every error kind of
    // `hod_ll` and `hod_bc` and the reason it is.
    #[test]
    fn reader_errors_and_their_reasons() {
        let target = decline(Construct::UnsupportedTarget);
        let limit = decline(named(LIMIT_READER));
        for kind in [LlErrorKind::Target, LlErrorKind::Ir(IrErrorKind::UnsupportedTarget)] {
            assert_eq!(ll(kind), target);
        }
        assert_eq!(ll(LlErrorKind::Ir(IrErrorKind::DataLayoutMismatch)), target);
        for kind in [
            LlErrorKind::Limit,
            LlErrorKind::Ir(IrErrorKind::TableFull),
            LlErrorKind::Ir(IrErrorKind::TypeTooLarge),
        ] {
            assert_eq!(ll(kind), limit);
        }
        for kind in [
            LlErrorKind::Lex,
            LlErrorKind::Syntax,
            LlErrorKind::Undefined,
            LlErrorKind::Redefined,
            LlErrorKind::Type,
            LlErrorKind::Ir(IrErrorKind::InvalidDataLayout),
            LlErrorKind::Ir(IrErrorKind::TypeMismatch),
        ] {
            assert_eq!(ll(kind), CompileError::MalformedInput("line 2:3: what".into()));
        }

        assert_eq!(bc(BcErrorKind::Unsupported("record")), decline(named("bitcode:record")));
        assert_eq!(bc(BcErrorKind::Target), target);
        assert_eq!(bc(BcErrorKind::Ir(IrErrorKind::DataLayoutMismatch)), target);
        assert_eq!(bc(BcErrorKind::Limit), limit);
        assert_eq!(bc(BcErrorKind::Ir(IrErrorKind::TableFull)), limit);
        for kind in [
            BcErrorKind::Malformed,
            BcErrorKind::Invalid,
            BcErrorKind::Type,
            BcErrorKind::Redefined,
            BcErrorKind::Ir(IrErrorKind::BadBlock),
        ] {
            assert_eq!(bc(kind), CompileError::MalformedInput("bit 64 (byte 8): what".into()));
        }
    }

    // spec/cli.md "Reasons": a refusal of the body pass names its function;
    // malformed input from `classify` is malformed input.
    #[test]
    fn rejections_and_their_reasons() {
        let source = b"target triple = \"aarch64-unknown-linux-gnu\"\ndeclare void @f()\n";
        let module = hod_ll::parse_module(source, &hod_ll::ParseOptions::default()).unwrap().module;
        let sym = module.find_symbol(b"f");
        assert!(sym.is_some());
        let rejection =
            |kind| Rejection { kind, construct: "cc:ccc".into(), symbol: sym, inst: None };
        let body = from_rejection(&module, rejection(RejectionKind::Unsupported), true);
        assert_eq!(
            body,
            Unsupported { construct: named("cc:ccc"), function: Some("f".into()) }.into()
        );
        let pass = from_rejection(&module, rejection(RejectionKind::Unsupported), false);
        assert_eq!(pass, decline(named("cc:ccc")));
        let invalid = from_rejection(&module, rejection(RejectionKind::Invalid), true);
        assert_eq!(invalid, CompileError::MalformedInput("cc:ccc in function 'f'".into()));
        // A symbol id the module does not have is not named.
        let stray =
            Rejection { symbol: Some(SymId::new(99)), ..rejection(RejectionKind::Unsupported) };
        assert_eq!(from_rejection(&module, stray, true), decline(named("cc:ccc")));
    }
}
