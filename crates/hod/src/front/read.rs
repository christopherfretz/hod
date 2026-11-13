//! The front half's reading (feature `readers`): [`compile_with`] reads
//! the module with `hod_ll` (text) or `hod_bc` (bitcode) and feeds a
//! [`Session`] with it (`spec/cli.md` "The facade", "Reasons").

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use hod_bc::{BcError, BcErrorKind};
use hod_ir::{Body, Module, SymId, TargetSpec};
use hod_ll::{LlError, LlErrorKind};

use super::{
    BitcodeReading, LIMIT_READER, Report, Settings, ir_reason, named, read_threads, reason_name,
};
use crate::back::{ThreadTokens, Threads};
use crate::contain::contained;
use crate::session::Session;
use crate::{
    CompileError, Construct, InputKind, Options, OutputKind, Unsupported, module_triple,
    target_supported,
};

fn decline(construct: Construct) -> CompileError {
    Unsupported::module(construct).into()
}

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
    // A panic of hod's is contained (`spec/cli.md` "Embedding"). Unwind
    // safe: `input`, `options` and `settings` are only read (the tokens
    // behind `settings.threads` are given back by guards as the panic
    // unwinds, and their locks ignore poisoning); the reader, the session
    // and their threads are dropped and joined inside; `report` is left as
    // the front half last wrote it, which the caller reads as such.
    let result = contained(|| front(input, options, settings, report)).and_then(|result| result);
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
    #[cfg(test)]
    crate::contain::fault::hit(crate::contain::fault::Site::Reader);
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
    use hod_ir::IrErrorKind;

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
