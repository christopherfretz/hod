//! [`Session`]: a module judged as it is fed (`spec/cli.md` "Session",
//! `docs/design/0004-in-process-seam.md` §4).
//!
//! A producer builds the module level into the session's
//! `hod_ir::Module`, ends it ([`Session::end_module`]: the module pass),
//! then hands the bodies over one at a time, by value; each is classified
//! and verified on arrival, and the first refusal is the answer.
//! [`Session::finish`] generates the object from the accepted bodies in
//! module order and appends it to the caller's buffer.
//!
//! This is the one judge of both routes: the command line's front half
//! (`front.rs`) feeds it from `hod_ll` or `hod_bc`, and an embedding
//! producer (kiln's bridge) from its own module.

use std::time::Instant;

use hod_ir::{
    Arch, Body, Census, Classifier, IrError, Module, Rejection, RejectionKind, Support, SymId,
    SymbolKind, TargetSpec,
};

use crate::back::generate_with;
use crate::contain::contained;
use crate::front::{INTERNAL_PANIC, Report, Settings, reason_name, symbol_name};
use crate::{CompileError, Construct, Options, OutputKind, Unsupported};

mod coro;
pub(crate) mod inlining;

/// What the producer may do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Build the module level; [`Session::end_module`] ends it.
    Module,
    /// Hand over bodies; [`Session::finish`] ends them.
    Bodies,
}

/// One module compiled in process: built into the session, judged as it
/// is fed, generated at [`Session::finish`] (`spec/cli.md` "Session").
///
/// The calls, in order: [`Session::new`] (or [`Session::with_module`]),
/// the module level through [`Session::module`], [`Session::end_module`],
/// then for each function with a body either [`Session::body`] or
/// [`Session::unsupported_body`], in any order, then [`Session::finish`].
/// A call out of this order, a body for a symbol that is not a function
/// with a body, a second body for one symbol, or `finish` with a body
/// missing is [`CompileError::Internal`] (the producer's bug), and the
/// session answers it from then on. Nothing panics on the producer's
/// mistakes.
///
/// After a refusal, every later call returns it again (unless
/// [`Settings::census`] asks for every reason, when the calls go on and
/// `finish` answers the first): the producer stops at the first `Err`.
///
/// A panic in a call is contained (`spec/cli.md` "Embedding"): the call
/// returns [`CompileError::Panic`], the session's state is dropped and
/// replaced by an empty one that answers the panic from every later call
/// (with [`Settings::census`] too), and the next session in the process
/// is not affected.
#[derive(Debug)]
pub struct Session {
    module: Module,
    options: Options,
    settings: Settings,
    report: Report,
    stage: Stage,
    census: Option<Census>,
    /// The first reason found, while the walk goes on (census).
    first: Option<CompileError>,
    /// The answer once the walk has stopped: every later call returns it.
    stopped: Option<CompileError>,
    /// The accepted bodies, in arrival order.
    bodies: Vec<Body>,
    /// Whether `bodies` is in module order so far.
    in_order: bool,
    /// One bit per symbol: a body or a refusal was delivered for it.
    delivered: Vec<u64>,
    /// When the session last returned to the producer.
    idle_since: Instant,
    /// What `-O0`'s always-inliner would have done (WP-021).
    inlining: inlining::Inlining,
    /// What the module level says about coroutines (WP-109).
    coro: coro::CoroModule,
}

/// A producer's mistake (`spec/cli.md` "Session").
fn misuse(what: &str) -> CompileError {
    CompileError::Internal(format!("session: {what}"))
}

/// A `hod_ir` error the producer passed on: a limit or an unsupported
/// target by name, anything else malformed input.
fn from_ir(error: &IrError) -> CompileError {
    match crate::front::ir_reason(error.kind) {
        Some(construct) => Unsupported::module(construct).into(),
        None => CompileError::MalformedInput(error.to_string()),
    }
}

/// AArch64's stack-argument rule, the hook `classify` decides `musttail`
/// calls with (WP-069, WP-066).
static A64_STACK_ARGS: hod_cg::abi::TargetStackArgs<'static, hod_a64::A64> =
    hod_cg::abi::TargetStackArgs::new(&hod_a64::A64);

/// x86-64's stack-argument rule (WP-148, `spec/abi-x64.md` §8.5): each
/// side assigned with the target of the function making the call, whose
/// features decide its convention (§1.3), as the call sequence assigns it.
static X64_STACK_ARGS: hod_cg::abi::TargetStackArgs<'static, hod_x64::X64> =
    hod_cg::abi::TargetStackArgs::per_function(&hod_x64::X64, hod_x64::call::target_of);

/// The classifier of `arch`, with its stack-argument rule
/// (`spec/codegen.md` §7.1, "Tail kinds"; `spec/ir.md` §8.6): without it
/// every `musttail` call is declined as `musttail:stack-args-unknown`.
fn classifier_for(arch: Arch, support: &Support) -> Classifier<'_> {
    let classifier = Classifier::new(support);
    match arch {
        Arch::Aarch64 => classifier.with_stack_args(&A64_STACK_ARGS),
        Arch::X86_64 => classifier.with_stack_args(&X64_STACK_ARGS),
    }
}

/// The matrix `settings` classifies `arch` with.
fn support_of(settings: &Settings, arch: Arch) -> &Support {
    settings.support.as_ref().unwrap_or_else(|| Support::shipped(arch))
}

/// A `classify` refusal as a decline (`spec/cli.md` "Reasons"): the
/// function is named for the body pass.
pub(crate) fn from_rejection(module: &Module, rejection: Rejection, body: bool) -> CompileError {
    let function =
        if body { rejection.symbol.and_then(|sym| symbol_name(module, sym)) } else { None };
    match rejection.kind {
        RejectionKind::Unsupported => {
            Unsupported::in_function(Construct::Named(rejection.construct), function).into()
        }
        RejectionKind::Invalid => CompileError::MalformedInput(match function {
            Some(function) => format!("{} in function '{function}'", rejection.construct),
            None => rejection.construct.into_owned(),
        }),
    }
}

impl Session {
    /// A session for an empty module of `target`, compiled with `options`.
    ///
    /// # Errors
    /// What is judged before anything is read (`spec/cli.md` "Reasons"):
    /// `assembly-output` when `options` does not ask for an object, then
    /// `unsupported-target` when `-march` is given or `-mtriple` names
    /// another target than `target`.
    pub fn new(
        target: TargetSpec,
        options: &Options,
        settings: Settings,
    ) -> Result<Session, CompileError> {
        Session::with_module(Module::new(target), options, settings)
    }

    /// [`Session::new`] for a module a producer has already built: its
    /// module level, with no body delivered yet. The command line's
    /// readers, which make their own module, start so.
    ///
    /// # Errors
    /// As [`Session::new`], for `module`'s target.
    pub fn with_module(
        module: Module,
        options: &Options,
        settings: Settings,
    ) -> Result<Session, CompileError> {
        let target = options.triple.clone().or_else(|| Some(module.target().triple().into()));
        let report = Report { target, ..Report::default() };
        // Unwind safe: `module`, `settings` and `report` move in and are
        // dropped with a panic; `options` is only read.
        contained(|| Session::open(module, options, settings, report).map_err(|(error, _)| error))?
    }

    /// [`Session::with_module`] that starts from `report` and gives it
    /// back with the error.
    #[allow(clippy::result_large_err, reason = "the report goes back with the error")]
    pub(crate) fn open(
        module: Module,
        options: &Options,
        settings: Settings,
        report: Report,
    ) -> Result<Session, (CompileError, Report)> {
        let decline = |construct| Err((Unsupported::module(construct).into(), report.clone()));
        if options.output_kind != OutputKind::Object {
            return decline(Construct::AssemblyOutput);
        }
        // `-mtriple` is what the module is compiled for; a module that
        // names another target is not retargeted (not implemented).
        let forced = options.triple.as_deref().map(TargetSpec::from_triple);
        let retargeted = forced.is_some_and(|forced| forced.as_ref() != Ok(module.target()));
        if options.arch.is_some() || retargeted {
            return decline(Construct::UnsupportedTarget);
        }
        Ok(Session::fresh(module, options.clone(), settings, report))
    }

    /// A session at its start: nothing judged, nothing delivered.
    fn fresh(module: Module, options: Options, settings: Settings, report: Report) -> Session {
        Session {
            module,
            options,
            census: settings.census.then(Census::new),
            settings,
            report,
            stage: Stage::Module,
            first: None,
            stopped: None,
            bodies: Vec::new(),
            in_order: true,
            delivered: Vec::new(),
            idle_since: Instant::now(),
            inlining: inlining::Inlining::default(),
            coro: coro::CoroModule::default(),
        }
    }

    /// The module the producer builds: its module level before
    /// [`Session::end_module`], and what a body adds to it (types,
    /// constants, inline assembly) between bodies after.
    pub fn module(&mut self) -> &mut Module {
        &mut self.module
    }

    /// Ends the module level: closes its debug tables
    /// (`Module::finish_debug`) and runs `classify`'s module pass.
    ///
    /// # Errors
    /// The module pass's refusal, or the debug tables' error; a call
    /// other than the first after [`Session::new`] is a misuse.
    pub fn end_module(&mut self) -> Result<(), CompileError> {
        self.guarded(Session::judge_module)
    }

    /// Hands over the body of `sym`, a function the module defines,
    /// built against [`Session::module`]. Classified and verified now,
    /// compiled no earlier than [`Session::finish`]; bodies may come in
    /// any order, the object lays them out in module order.
    ///
    /// # Errors
    /// The body pass's refusal (named in its function), `verify`'s
    /// (`malformed-input`), a refusal met earlier, or a misuse: before
    /// [`Session::end_module`], `sym` not a function with a body, `body`
    /// another symbol's, `sym` delivered before.
    pub fn body(&mut self, sym: SymId, body: Body) -> Result<(), CompileError> {
        // WP-166: the body is moved once, to where an accepted body stays
        // (the end of `bodies`), and judged there; the guarded call takes
        // it out again unless it is accepted. Moving it into the guarded
        // closure instead would copy it once more. The push is outside the
        // guard only where it cannot panic: with room reserved.
        if self.bodies.try_reserve(1).is_err() {
            return self.guarded(|session| {
                session.bodies.push(body);
                session.judge_body(sym, Ok(()))
            });
        }
        self.bodies.push(body);
        self.guarded(|session| session.judge_body(sym, Ok(())))
    }

    /// Says that the producer could not build the body of `sym`, a
    /// function the module defines, because of `constructs` (each a
    /// stable name, the first the reason; the readers'
    /// `unsupported()`): a decline in that function. The producer records
    /// them in the module as well (`Module::record_unsupported`), as the
    /// readers do.
    ///
    /// # Errors
    /// The decline (`malformed-input` if `constructs` is empty), a
    /// refusal met earlier, or a misuse as for [`Session::body`]. With
    /// [`Settings::census`], `Ok`: the walk goes on.
    pub fn unsupported_body<'c>(
        &mut self,
        sym: SymId,
        constructs: impl IntoIterator<Item = &'c str>,
    ) -> Result<(), CompileError> {
        self.guarded(|session| session.judge_body(sym, Err(&mut constructs.into_iter())))
    }

    /// Generates the object from the accepted bodies, in module order,
    /// and appends it to `out`: the bytes `hod::compile` writes for the
    /// same module and options, at any thread count.
    ///
    /// # Errors
    /// The answer (the first refusal, or code generation's: "The back
    /// half"), with the report; `out` is then as it was. Also a misuse:
    /// before [`Session::end_module`], or a function with a body that
    /// was never delivered.
    ///
    /// `finish` takes the session, so it cannot be called twice:
    ///
    /// ```compile_fail
    /// fn twice(session: hod::Session) {
    ///     let mut out = Vec::new();
    ///     let _ = session.finish(&mut out);
    ///     let _ = session.finish(&mut out);
    /// }
    /// ```
    #[allow(clippy::result_large_err, reason = "design 0004 §4: the report comes with the answer")]
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<Report, (CompileError, Report)> {
        match self.guarded(Session::conclude) {
            Ok(object) if out.is_empty() => *out = object,
            Ok(object) => out.extend_from_slice(&object),
            Err(error) => return Err((error, self.report)),
        }
        Ok(self.report)
    }

    /// Stops the walk with a producer's error (a reader's), and answers:
    /// the first reason found, else `error`.
    #[cfg(feature = "readers")]
    pub(crate) fn abort(mut self, error: CompileError) -> (CompileError, Report) {
        self.stop(error);
        match self.guarded(Session::conclude) {
            Err(error) => (error, self.report),
            Ok(_) => unreachable!("a stopped session concludes with its error"),
        }
    }

    // ---- containment (`spec/cli.md` "Embedding") ----

    /// One call of the producer's: timed (the clock below), and a panic
    /// in it contained, the session then poisoned.
    fn guarded<T>(
        &mut self,
        call: impl FnOnce(&mut Session) -> Result<T, CompileError>,
    ) -> Result<T, CompileError> {
        let entered = self.enter();
        // Unwind safe: after a panic nothing of the state `call` may have
        // left half changed is used again; `poison` replaces the whole
        // session before it returns to the producer.
        let result = contained(|| call(self)).unwrap_or_else(|panic| Err(self.poison(panic)));
        self.leave(entered);
        result
    }

    /// Replaces the session by an empty one of the same target that
    /// answers `error` from every call: what a panic left in the module,
    /// the bodies, the judge or the settings (their tokens) is dropped
    /// here, by construction rather than by checks. The report is kept
    /// (plain data), with the reason under [`Settings::census`].
    fn poison(&mut self, error: CompileError) -> CompileError {
        let target = self.module.target().clone();
        let mut report = std::mem::take(&mut self.report);
        if self.census.is_some() {
            report.reason(INTERNAL_PANIC);
        }
        *self =
            Session::fresh(Module::new(target), Options::default(), Settings::default(), report);
        self.stopped = Some(error.clone());
        error
    }

    // ---- the clock (`Report::bridge`, `Report::session`) ----

    fn enter(&mut self) -> Instant {
        let now = Instant::now();
        self.report.bridge += now.saturating_duration_since(self.idle_since);
        now
    }

    fn leave(&mut self, entered: Instant) {
        let now = Instant::now();
        self.report.session += now.saturating_duration_since(entered);
        self.idle_since = now;
    }

    // ---- the judge ----

    /// Stops the walk at `error` unless it has stopped already.
    fn stop(&mut self, error: CompileError) {
        self.stopped.get_or_insert(error);
    }

    /// A producer's mistake: it stops the walk, and is the answer unless
    /// a reason was found before it.
    fn misuse(&mut self, what: &str) -> CompileError {
        let error = misuse(what);
        self.stop(error.clone());
        self.stopped.clone().unwrap_or(error)
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

    /// Ends a step: stop at the first reason unless every reason is asked
    /// for.
    fn verdict(&mut self) -> Result<(), CompileError> {
        if self.census.is_none()
            && let Some(error) = self.first.take()
        {
            self.stop(error);
        }
        match &self.stopped {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn judge_module(&mut self) -> Result<(), CompileError> {
        if let Some(error) = &self.stopped {
            return Err(error.clone());
        }
        #[cfg(test)]
        crate::contain::fault::hit(crate::contain::fault::Site::EndModule);
        if self.stage != Stage::Module {
            return Err(self.misuse("end_module after end_module"));
        }
        self.stage = Stage::Bodies;
        if let Err(error) = self.module.finish_debug() {
            self.stop(from_ir(&error));
            return self.verdict();
        }
        let arch = self.module.target().arch();
        let classifier = classifier_for(arch, support_of(&self.settings, arch));
        if let Some(census) = &mut self.census {
            classifier.census_module(&self.module, census);
            self.drain_census();
        }
        let classifier = classifier_for(arch, support_of(&self.settings, arch));
        if let Err(rejection) = classifier.module(&self.module) {
            // A function attribute the readers refused is judged here, in
            // the module pass, and names its function as a body's
            // refusal does (WP-183); any other module refusal names none.
            let attribute = rejection.construct.starts_with("attr:");
            let error = from_rejection(&self.module, rejection, attribute);
            self.note(error);
        }
        // WP-109: a coroutine module the target cannot build is declined
        // before any body is read.
        let (coro, refusal) = coro::module_refusal(&self.module, support_of(&self.settings, arch));
        self.coro = coro;
        if let Some(name) = refusal {
            self.note(Unsupported::module(Construct::Named(name.into())).into());
        }
        self.report.phase("classify");
        self.verdict()
    }

    /// Marks `sym` delivered; `false` if it was already.
    fn deliver(&mut self, sym: SymId) -> bool {
        let (word, bit) = (sym.index() / 64, 1u64 << (sym.index() % 64));
        if self.delivered.len() <= word {
            self.delivered.resize(word + 1, 0);
        }
        let fresh = self.delivered[word] & bit == 0;
        self.delivered[word] |= bit;
        fresh
    }

    fn is_delivered(&self, sym: SymId) -> bool {
        let word = sym.index() / 64;
        self.delivered.get(word).is_some_and(|w| w & (1u64 << (sym.index() % 64)) != 0)
    }

    /// The misuse a delivery for `sym` would be, if any.
    fn check_delivery(&mut self, sym: SymId, body: Option<SymId>) -> Result<(), CompileError> {
        if self.stage != Stage::Bodies {
            return Err(self.misuse("a body before end_module"));
        }
        if sym.index() >= self.module.symbol_count() {
            return Err(self.misuse(&format!("a body for {sym:?}, not a symbol of the module")));
        }
        let name = |module: &Module| symbol_name(module, sym).unwrap_or_default();
        if !matches!(self.module.symbol(sym).kind(), SymbolKind::Function { has_body: true, .. }) {
            let what = format!("a body for '{}', not a function with a body", name(&self.module));
            return Err(self.misuse(&what));
        }
        if let Some(body) = body
            && body != sym
        {
            let what = format!("the body of {body:?} handed over for '{}'", name(&self.module));
            return Err(self.misuse(&what));
        }
        if !self.deliver(sym) {
            let what = format!("a second body for '{}'", name(&self.module));
            return Err(self.misuse(&what));
        }
        Ok(())
    }

    /// One body as the producer delivered it: the body (`Ok`: the last of
    /// `bodies`, taken out again unless it is accepted), or the names of
    /// what kept the producer from building it.
    fn judge_body(
        &mut self,
        sym: SymId,
        body: Result<(), &mut dyn Iterator<Item = &str>>,
    ) -> Result<(), CompileError> {
        let delivered = match body {
            Ok(()) => Some(self.bodies.last().map(Body::symbol).expect("the body was pushed")),
            Err(_) => None,
        };
        if let Some(error) = &self.stopped {
            let error = error.clone();
            if delivered.is_some() {
                self.bodies.pop();
            }
            return Err(error);
        }
        if let Err(error) = self.check_delivery(sym, delivered) {
            if delivered.is_some() {
                self.bodies.pop();
            }
            return Err(error);
        }
        match body {
            Err(refused) => {
                let mut first = None;
                for name in refused {
                    if self.census.is_some() {
                        self.report.reason(name);
                    }
                    if first.is_none() {
                        first = Some(name.to_owned());
                    }
                    if self.census.is_none() {
                        break;
                    }
                }
                let function = symbol_name(&self.module, sym);
                let error = match first {
                    Some(first) => {
                        Unsupported::in_function(Construct::Named(first.into()), function).into()
                    }
                    // Defensive: a producer names what it refused.
                    None => CompileError::MalformedInput(format!(
                        "no body was read for function '{}'",
                        function.unwrap_or_default()
                    )),
                };
                self.note(error);
            }
            // WP-021: an `available_externally` body waits for `finish`.
            Ok(()) if inlining::is_available_externally(&self.module, sym) => {
                let body = self.bodies.pop().expect("the body was pushed");
                match self.inlining.divert(&self.module, body) {
                    Ok(Some(body)) => self.judge_built(body),
                    Ok(None) => {}
                    Err(error) => self.note(error),
                }
            }
            Ok(()) => self.judge_last(),
        }
        self.verdict()
    }

    /// A body built at `finish` (an `available_externally` copy): judged
    /// as the producer's are.
    fn judge_built(&mut self, body: Body) {
        self.bodies.push(body);
        self.judge_last();
    }

    /// The last of `bodies`, just delivered: kept if [`Session::judge`]
    /// accepts it, else taken out.
    fn judge_last(&mut self) {
        // Out of `self` while it is judged by reference (a panic drops
        // it with the rest of the session, which `poison` replaces).
        let bodies = std::mem::take(&mut self.bodies);
        let body = bodies.last().expect("the body was pushed");
        let accepted = self.judge(body);
        self.bodies = bodies;
        if accepted {
            self.accept_last();
        } else {
            self.bodies.pop();
        }
    }

    /// A body the producer built: census, `classify`, `verify`; whether
    /// it is accepted.
    fn judge(&mut self, body: &Body) -> bool {
        #[cfg(test)]
        crate::contain::fault::hit(crate::contain::fault::Site::Feed);
        let arch = self.module.target().arch();
        let sym = body.symbol();
        if let Some(census) = &mut self.census {
            let classifier = classifier_for(arch, support_of(&self.settings, arch));
            classifier.census_body(&self.module, body, census);
            self.drain_census();
        }
        if self.first.is_none() {
            let classifier = classifier_for(arch, support_of(&self.settings, arch));
            match classifier.body(&self.module, body) {
                Err(rejection) => {
                    let error = from_rejection(&self.module, rejection, true);
                    self.note(error);
                }
                // Code generation may assume what `verify` checks
                // (`spec/ir.md` §5.8): a body that reaches it is
                // verified, always.
                Ok(()) => match body.verify(&self.module) {
                    Err(error) => {
                        let function = symbol_name(&self.module, sym).unwrap_or_default();
                        self.note(CompileError::MalformedInput(format!(
                            "function '{function}': {error}"
                        )));
                    }
                    // WP-106, WP-109: a coroutine the plan or code
                    // generation would refuse, by its name, now.
                    Ok(()) => match coro::refusal(&self.module, body, self.coro) {
                        Some(name) => {
                            let function = symbol_name(&self.module, sym);
                            let construct = Construct::Named(name.into());
                            self.note(Unsupported::in_function(construct, function).into());
                        }
                        None => return true,
                    },
                },
            }
        }
        false
    }

    /// The last of `bodies` is accepted.
    fn accept_last(&mut self) {
        if let [.., before, last] = self.bodies.as_slice()
            && before.symbol().index() > last.symbol().index()
        {
            self.in_order = false;
        }
        if let Some(last) = self.bodies.last() {
            self.inlining.note(&self.module, last);
        }
    }

    /// The end of the walk: the answer, or the object.
    fn conclude(&mut self) -> Result<Vec<u8>, CompileError> {
        if self.stopped.is_none() && self.stage == Stage::Module {
            let _ = self.misuse("finish before end_module");
        }
        if let Some(error) = self.stopped.take() {
            if self.census.is_some() {
                self.report.reason(reason_name(&error));
            }
            return Err(self.first.take().unwrap_or(error));
        }
        self.report.phase("bodies");
        if let Some(error) = self.first.take() {
            return Err(error);
        }
        let module = &self.module;
        let missing = module.symbols().enumerate().find(|&(index, symbol)| {
            matches!(symbol.kind(), SymbolKind::Function { has_body: true, .. })
                && !self.is_delivered(SymId::new(index as u32))
        });
        if let Some((index, _)) = missing {
            let name = symbol_name(module, SymId::new(index as u32)).unwrap_or_default();
            return Err(misuse(&format!("no body was delivered for '{name}'")));
        }
        // WP-021: the `available_externally` copies, judged now.
        match self.inlining.conclude(&mut self.module, &mut self.bodies) {
            Ok(copies) => {
                for copy in copies {
                    self.judge_built(copy);
                }
            }
            Err(error) => self.note(error),
        }
        if let Some(error) = self.first.take() {
            return Err(error);
        }
        if !self.in_order {
            self.bodies.sort_by_key(|body| body.symbol().index());
        }
        let module = &self.module;
        generate_with(module, &self.bodies, &self.options, &self.settings.threads, &mut self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LIMIT_READER;

    // spec/cli.md "Reasons": a refusal of the body pass names its function;
    // malformed input from `classify` is malformed input.
    #[test]
    fn rejections_and_their_reasons() {
        let named = |name: &'static str| Construct::Named(name.into());
        let source = b"target triple = \"aarch64-unknown-linux-gnu\"\ndeclare void @f()\n";
        let module = hod_ll::parse_module(source, &hod_ll::ParseOptions::default()).unwrap().module;
        let sym = module.find_symbol(b"f");
        assert!(sym.is_some());
        let rejection =
            |kind| Rejection { kind, construct: "cc:ccc".into(), symbol: sym, inst: None };
        let body = from_rejection(&module, rejection(RejectionKind::Unsupported), true);
        assert_eq!(body, Unsupported::in_function(named("cc:ccc"), Some("f".into())).into());
        let pass = from_rejection(&module, rejection(RejectionKind::Unsupported), false);
        assert_eq!(pass, Unsupported::module(named("cc:ccc")).into());
        let invalid = from_rejection(&module, rejection(RejectionKind::Invalid), true);
        assert_eq!(invalid, CompileError::MalformedInput("cc:ccc in function 'f'".into()));
        // A symbol id the module does not have is not named.
        let stray =
            Rejection { symbol: Some(SymId::new(99)), ..rejection(RejectionKind::Unsupported) };
        assert_eq!(
            from_rejection(&module, stray, true),
            Unsupported::module(named("cc:ccc")).into()
        );
    }

    // spec/cli.md "Session": the producer's own errors.
    #[test]
    fn errors_from_hod_ir() {
        use hod_ir::IrErrorKind;
        let limit = IrError::new(IrErrorKind::TableFull, "full");
        assert_eq!(
            from_ir(&limit),
            Unsupported::module(Construct::Named(LIMIT_READER.into())).into()
        );
        let bad = IrError::new(IrErrorKind::UndefinedDebugNode, "di3");
        assert_eq!(from_ir(&bad), CompileError::MalformedInput("UndefinedDebugNode: di3".into()));
    }
}
