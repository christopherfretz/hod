//! The front half of [`crate::compile`]: read the module, check it, and
//! decline it with the first reason (`spec/cli.md` "Reasons", "The front
//! half"). The judging is the [`Session`]'s, the same for every producer;
//! code generation is behind one call, [`generate`].
//!
//! The order is the cheap one: what needs no reading, then the module
//! level (read, then `classify`'s module pass), then the bodies, streamed
//! one at a time (read, the reader's own refusals, `classify`'s body pass,
//! `Body::verify`). A declined module is handed to `llc` next, so the
//! first refusal ends the work unless [`Settings::census`] asks for every
//! reason. The back half is `back.rs`.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hod_bc::{BcError, BcErrorKind};
use hod_ir::{Body, IrErrorKind, Module, Support, SymId, TargetSpec};
use hod_ll::{LlError, LlErrorKind};

use crate::back::{ThreadTokens, Threads, generate_with};
use crate::session::Session;
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
    /// The threads code generation may use (`spec/cli.md` "Threads");
    /// the default is the calling thread alone.
    pub threads: Threads,
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
    /// The `bridge` phase (`spec/cli.md` "Session"): the time the
    /// producer spent between the [`Session`]'s calls, from the session's
    /// start to [`Session::finish`] (in process, the bridge's copy of its
    /// module; on the command line, reading the bodies).
    pub bridge: Duration,
    /// The time spent inside the [`Session`]'s calls: judging and code
    /// generation. `bridge / (bridge + session)` is the producer's share.
    pub session: Duration,
}

impl Report {
    pub(crate) fn phase(&mut self, name: &'static str) {
        self.phases.push((name, Instant::now()));
    }

    pub(crate) fn reason(&mut self, name: &str) {
        if !self.reasons.iter().any(|known| known == name) {
            self.reasons.push(name.to_owned());
        }
    }
}

/// The reason name of a decline, as the coverage log writes it.
pub(crate) fn reason_name(error: &CompileError) -> &str {
    match error {
        CompileError::Unsupported(unsupported) => unsupported.construct.name(),
        CompileError::MalformedInput(_) => "malformed-input",
        CompileError::InvalidOptions(_) => "invalid-options",
        CompileError::Internal(_) => INTERNAL_ERROR,
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
pub(crate) fn ir_reason(kind: IrErrorKind) -> Option<Construct> {
    match kind {
        IrErrorKind::UnsupportedTarget | IrErrorKind::DataLayoutMismatch => {
            Some(Construct::UnsupportedTarget)
        }
        IrErrorKind::TableFull | IrErrorKind::TypeTooLarge => Some(named(LIMIT_READER)),
        _ => None,
    }
}

/// The reason name of [`CompileError::Internal`] (`spec/cli.md`
/// "Reasons").
pub const INTERNAL_ERROR: &str = "internal-error";

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

pub(crate) fn symbol_name(module: &Module, sym: SymId) -> Option<String> {
    (sym.index() < module.symbol_count())
        .then(|| String::from_utf8_lossy(module.symbol(sym).name()).into_owned())
}

/// The back half: the object for `module`, which `classify` accepted
/// whole, and its function bodies, every one classified and verified, in
/// the order the module defines them.
///
/// On the calling thread alone; [`generate_with`] says how many threads
/// and records the phases (`spec/cli.md` "The back half").
///
/// # Errors
/// Code generation's refusal ([`CompileError::Unsupported`]), or an
/// internal error ([`CompileError::Internal`]). Nothing is written.
pub fn generate(
    module: &Module,
    bodies: &[Body],
    options: &Options,
) -> Result<Vec<u8>, CompileError> {
    generate_with(module, bodies, options, &Threads::default(), &mut Report::default())
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
    // The reader makes the module of `-mtriple`'s target when the module
    // names none; one that names another target is declined by the
    // session.
    let forced = match options.triple.as_deref().map(TargetSpec::from_triple) {
        None => None,
        Some(Ok(target)) => Some(target),
        Some(Err(_)) => return Err(decline(Construct::UnsupportedTarget)),
    };

    // The module level is read, then the session judges it and each body
    // as the reader delivers it, by value (`session.rs`). Where tokens
    // come from a pool, the session takes them through `held`, so that
    // reading and code generation share one request (`HeldTokens`).
    let held = settings.threads.tokens.clone().map(|pool| Arc::new(HeldTokens::new(pool)));
    let mut session_settings = settings.clone();
    if let Some(held) = &held {
        session_settings.threads.tokens = Some(held.clone());
    }
    let start = |module: Module, report: &mut Report| {
        let started = std::mem::take(report);
        Session::open(module, options, session_settings.clone(), started).map_err(
            |(error, started)| {
                *report = started;
                error
            },
        )
    };
    let session = match kind {
        InputKind::Text => {
            let parse = hod_ll::ParseOptions { default_target: forced };
            let hod_ll::ParsedModule { module, text } =
                hod_ll::parse_module(input, &parse).map_err(|error| from_ll(&error))?;
            report.phase("parse");
            let mut session = start(module, report)?;
            if session.end_module().is_ok() {
                let functions = text.functions();
                let bytes = functions.iter().map(|f| (f.body.end - f.body.start) as usize).sum();
                let threads = reading_threads(&settings.threads, held.as_deref(), bytes, functions);
                let read = hod_ll::parse_bodies_on(
                    &mut session,
                    Session::module,
                    &text,
                    threads,
                    |session, function, read| feed(session, function.sym, read),
                );
                if let Err(error) = read {
                    return answer(Err(session.abort(from_ll(&error))), report);
                }
            }
            session
        }
        InputKind::Bitcode => {
            let read = hod_bc::ReadOptions { default_target: forced };
            let hod_bc::BcModule { module, bits } =
                hod_bc::read_module(input, &read).map_err(|error| from_bc(&error))?;
            report.phase("parse");
            if options.triple.is_none() {
                report.target = Some(module.target().triple().to_owned());
            }
            let mut session = start(module, report)?;
            if session.end_module().is_ok() {
                let functions = bits.functions();
                // Bitcode takes about a quarter of the bytes of the same
                // body as text.
                let bytes =
                    functions.iter().map(|f| ((f.span.end - f.span.start) / 2) as usize).sum();
                let threads = reading_threads(&settings.threads, held.as_deref(), bytes, functions);
                let read = hod_bc::read_bodies_on(
                    &mut session,
                    Session::module,
                    &bits,
                    threads,
                    |session, function, read| feed(session, function.sym, read),
                );
                if let Err(error) = read {
                    return answer(Err(session.abort(from_bc(&error))), report);
                }
            }
            session
        }
        InputKind::Module => unreachable!("InputKind::detect never says module"),
    };
    let mut object = Vec::new();
    answer(session.finish(&mut object).map(|done| (object, done)), report)
}

/// The most threads that read bodies: past four, the calling thread,
/// which takes in and delivers every body in order, is the bottleneck
/// (measured: `spec/landed/ll/WP-063.md`).
pub const READ_MAX_THREADS: usize = 4;

/// The body text, in bytes, each thread reading bodies needs (bitcode
/// counts its bytes four times): below twice this, a module is read on
/// the calling thread. Every thread then has at least eight runs of
/// bodies, each of the full `hod_ir::CHUNK_WEIGHT` (16 KiB).
pub const READ_BYTES_PER_THREAD: usize = 128 << 10;

/// The body text, in bytes, a module's bodies must have on average to be
/// read in parallel: smaller bodies cost the calling thread about as much
/// to take in as they cost a thread to read.
pub const READ_MIN_BODY_BYTES: usize = 512;

/// How many threads read the bodies of a module whose bodies are
/// `body_bytes` of text in `bodies` functions, when `granted` threads may
/// run (`spec/cli.md` "Threads"). A pure function of the three.
#[must_use]
pub fn read_threads(body_bytes: usize, bodies: usize, granted: usize) -> usize {
    if bodies < 2 || body_bytes / bodies < READ_MIN_BODY_BYTES {
        return 1;
    }
    granted.min(READ_MAX_THREADS).min(bodies).min(body_bytes / READ_BYTES_PER_THREAD).max(1)
}

/// The threads that read the bodies: [`read_threads`] of what `threads`
/// grants; where threads come from tokens, of what one request to the
/// pool gives, held for code generation after.
fn reading_threads<T>(
    threads: &Threads,
    held: Option<&HeldTokens>,
    body_bytes: usize,
    functions: &[T],
) -> usize {
    let granted = threads.max.max(1);
    let wanted = read_threads(body_bytes, functions.len(), granted);
    match held {
        Some(held) if wanted > 1 => 1 + held.fill(granted - 1).min(wanted - 1),
        Some(_) => 1,
        None => wanted,
    }
}

/// A compile's tokens: at most one request to the pool per compile.
/// Reading in parallel asks for every token the compile may use and holds
/// them ([`HeldTokens::fill`]); code generation then takes from them, and
/// what it does not take goes back to the pool at once. Without that
/// request, code generation's goes to the pool. What is held when the
/// compile ends goes back.
#[derive(Debug)]
struct HeldTokens {
    pool: Arc<dyn ThreadTokens>,
    /// Whether the pool was asked; the tokens held.
    state: Mutex<(bool, usize)>,
}

impl HeldTokens {
    fn new(pool: Arc<dyn ThreadTokens>) -> Self {
        HeldTokens { pool, state: Mutex::new((false, 0)) }
    }

    fn state(&self) -> MutexGuard<'_, (bool, usize)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks the pool for up to `wanted` tokens and holds them; the
    /// number held.
    fn fill(&self, wanted: usize) -> usize {
        let mut state = self.state();
        if !state.0 {
            state.0 = true;
            state.1 = self.pool.take(wanted);
        }
        state.1
    }
}

impl ThreadTokens for HeldTokens {
    fn take(&self, wanted: usize) -> usize {
        let mut state = self.state();
        if !state.0 {
            state.0 = true;
            return self.pool.take(wanted);
        }
        let got = state.1.min(wanted);
        let surplus = state.1 - got;
        state.1 = 0;
        drop(state);
        if surplus > 0 {
            self.pool.give_back(surplus);
        }
        got
    }

    fn give_back(&self, count: usize) {
        self.pool.give_back(count);
    }
}

impl Drop for HeldTokens {
    fn drop(&mut self) {
        let held = std::mem::take(&mut self.state().1);
        if held > 0 {
            self.pool.give_back(held);
        }
    }
}

/// Hands a body the reader delivered to the session; whether to go on.
fn feed(session: &mut Session, sym: SymId, read: Result<Body, &[Box<str>]>) -> bool {
    let fed = match read {
        Ok(body) => session.body(sym, body),
        Err(constructs) => session.unsupported_body(sym, constructs.iter().map(|c| &**c)),
    };
    fed.is_ok()
}

/// A session's answer, its report into the caller's.
fn answer(
    result: Result<(Vec<u8>, Report), (CompileError, Report)>,
    report: &mut Report,
) -> Result<Vec<u8>, CompileError> {
    match result {
        Ok((object, done)) => {
            *report = done;
            Ok(object)
        }
        Err((error, done)) => {
            *report = done;
            Err(error)
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
}
