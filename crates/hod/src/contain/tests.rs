//! `spec/cli.md` "Embedding", "Panics": a panic forced at each boundary
//! is contained, answers `internal-panic`, gives back every jobserver
//! token, leaves no thread running, and changes no byte of the next
//! compile in the process.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod_ir::{Body, SymId};
use hod_testutil::jobs::PrivatePool;

use super::fault::{self, Plan, Site};
use crate::front::reason_name;
use crate::jobserver::Jobserver;
use crate::{
    CompileError, OptLevel, Options, OutputKind, RelocModel, Session, Settings, ThreadTokens,
    Threads,
};

const AARCH64: &str = "aarch64-unknown-linux-gnu";
const X86_64: &str = "x86_64-unknown-linux-gnu";

fn options() -> Options {
    Options {
        output_kind: OutputKind::Object,
        opt_level: OptLevel::O0,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    }
}

/// Eight small functions: enough bodies for four threads.
fn eight(triple: &str) -> String {
    let mut text = format!("target triple = \"{triple}\"\n");
    for i in 0..8 {
        let _ = writeln!(
            text,
            "define i32 @f{i}(i32 %x) nounwind {{\n  %y = add i32 %x, {i}\n  ret i32 %y\n}}"
        );
    }
    text
}

/// `text` read by `hod_ll` and fed to a public [`Session`], as an
/// embedding producer does.
fn through_session(text: &str, settings: Settings) -> Result<Vec<u8>, CompileError> {
    let parsed =
        hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).expect("readable");
    let mut module = parsed.module;
    let mut parser = hod_ll::BodyParser::new(&parsed.text);
    let mut bodies: Vec<(SymId, Result<Body, Vec<String>>)> = Vec::new();
    for function in parsed.text.functions() {
        let body = parser.parse(&mut module, function).expect("a readable body");
        let body = body.ok_or_else(|| parser.unsupported().map(str::to_owned).collect());
        bodies.push((function.sym, body));
    }
    let mut session = Session::with_module(module, &options(), settings)?;
    session.end_module()?;
    for (sym, body) in bodies {
        match body {
            Ok(body) => session.body(sym, body)?,
            Err(names) => session.unsupported_body(sym, names.iter().map(String::as_str))?,
        }
    }
    let mut out = Vec::new();
    session.finish(&mut out).map_err(|(error, _)| error)?;
    Ok(out)
}

/// Runs `call` with `site` armed on this thread; the plan, for its
/// record of the workers.
fn armed<T>(site: Site, call: impl FnOnce() -> T) -> (T, Plan) {
    let plan = Plan { site: Some(site), ..Plan::default() };
    fault::arm(plan.clone());
    let result = call();
    fault::arm(Plan::default());
    (result, plan)
}

/// Threads granted outright, one per function: every module with two
/// bodies or more starts workers.
fn many_threads() -> Settings {
    Settings { threads: Threads { max: 4, tokens: None, per_thread: 1 }, ..Settings::default() }
}

fn assert_panic(result: &Result<Vec<u8>, CompileError>, site: Site) {
    match result {
        Err(error @ CompileError::Panic(message)) => {
            assert_eq!(message, &format!("forced panic at {site:?}"));
            assert_eq!(reason_name(error), "internal-panic");
        }
        other => panic!("{site:?}: expected a contained panic, got {other:?}"),
    }
}

// "Embedding", "Panics": each call of a session contains a panic in it,
// and a poisoned session answers the panic from every later call, census
// or not; the module it hands out then is an empty one.
#[test]
fn a_panic_in_a_session_call_is_contained_and_poisons_the_session() {
    let text = eight(AARCH64);
    for site in [Site::EndModule, Site::Feed, Site::Finish, Site::Codegen] {
        let (result, _) = armed(site, || through_session(&text, Settings::default()));
        assert_panic(&result, site);
    }
    let (result, plan) = armed(Site::Worker, || through_session(&text, many_threads()));
    assert_panic(&result, Site::Worker);
    assert!(plan.workers.started.load(Ordering::SeqCst) >= 1, "workers ran");
    assert_eq!(plan.workers.live.load(Ordering::SeqCst), 0, "no worker is left running");

    // Poisoned: every later call answers the panic, nothing is judged.
    for census in [false, true] {
        let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
            .expect("readable");
        let mut module = parsed.module;
        let mut parser = hod_ll::BodyParser::new(&parsed.text);
        let bodies: Vec<_> = (parsed.text.functions().iter())
            .map(|f| (f.sym, parser.parse(&mut module, f).unwrap().expect("a body")))
            .collect();
        let mut bodies = bodies.into_iter();
        let settings = Settings { census, ..Settings::default() };
        let mut session = Session::with_module(module, &options(), settings).unwrap();
        session.end_module().unwrap();
        let (sym, body) = bodies.next().unwrap();
        let (fed, _) = armed(Site::Feed, || session.body(sym, body));
        let panic = CompileError::Panic("forced panic at Feed".into());
        assert_eq!(fed, Err(panic.clone()));
        assert_eq!(session.module().symbol_count(), 0, "the state is gone");
        let (sym, body) = bodies.next().unwrap();
        assert_eq!(session.body(sym, body), Err(panic.clone()));
        assert_eq!(session.unsupported_body(sym, ["cc:ghccc"]), Err(panic.clone()));
        assert_eq!(session.end_module(), Err(panic.clone()));
        let mut out = b"kept".to_vec();
        let (error, report) = session.finish(&mut out).expect_err("the panic");
        assert_eq!((error, &out[..]), (panic, &b"kept"[..]));
        let reasons: &[&str] = if census { &["internal-panic"] } else { &[] };
        assert_eq!(report.reasons, reasons);
        assert!(report.phases.iter().all(|&(name, _)| name != "bodies"));
    }
}

// "Embedding", "Panics": `compile_with` contains a panic of the reader's
// side (outside the session's calls) and of each call inside it.
#[cfg(feature = "readers")]
#[test]
fn a_panic_under_compile_with_is_contained() {
    let text = eight(X86_64);
    let compile = |settings: &Settings| {
        crate::compile_with(text.as_bytes(), &options(), settings, &mut crate::Report::default())
    };
    for site in [Site::Reader, Site::EndModule, Site::Feed, Site::Codegen, Site::Finish] {
        let (result, _) = armed(site, || compile(&Settings::default()));
        assert_panic(&result, site);
    }
    let census = Settings { census: true, ..many_threads() };
    let mut report = crate::Report::default();
    let (result, plan) = armed(Site::Worker, || {
        crate::compile_with(text.as_bytes(), &options(), &census, &mut report)
    });
    assert_panic(&result, Site::Worker);
    assert_eq!(report.reasons, ["internal-panic"]);
    assert_eq!(plan.workers.live.load(Ordering::SeqCst), 0, "no worker is left running");
}

/// A way to compile a module: through the session, or `compile_with`.
type Route = fn(&str, Settings) -> Result<Vec<u8>, CompileError>;

/// A jobserver client that counts what it holds.
#[derive(Debug)]
struct Counting {
    inner: Jobserver,
    held: AtomicUsize,
    peak: AtomicUsize,
}

impl ThreadTokens for Counting {
    fn take(&self, wanted: usize) -> usize {
        let got = self.inner.take(wanted);
        let held = self.held.fetch_add(got, Ordering::SeqCst) + got;
        self.peak.fetch_max(held, Ordering::SeqCst);
        got
    }

    fn give_back(&self, count: usize) {
        self.held.fetch_sub(count, Ordering::SeqCst);
        self.inner.give_back(count);
    }
}

// "Embedding", "Panics": the tokens a compile took from a real jobserver
// fifo are all back in it after a panic, on both routes, at every site
// that runs while they are held.
#[test]
fn a_contained_panic_gives_back_every_token() {
    const TOKENS: usize = 3;
    let pool = PrivatePool::new("wp093-contain", TOKENS);
    let text = eight(AARCH64);
    #[cfg_attr(not(feature = "readers"), allow(unused_mut))]
    let mut routes: Vec<(&str, Route)> = vec![("session", through_session)];
    #[cfg(feature = "readers")]
    routes.push(("compile_with", |text, settings| {
        crate::compile_with(text.as_bytes(), &options(), &settings, &mut crate::Report::default())
    }));
    for (route, run) in routes {
        for site in [Site::Worker, Site::Codegen] {
            assert_eq!(pool.idle(), TOKENS, "{route}, {site:?}: before");
            let counting = Arc::new(Counting {
                inner: Jobserver::open(pool.path()).expect("the pool's fifo"),
                held: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            });
            let tokens: Arc<dyn ThreadTokens> = counting.clone();
            let threads = Threads { max: 4, tokens: Some(tokens), per_thread: 1 };
            let settings = Settings { threads, ..Settings::default() };
            let (result, plan) = armed(site, || run(&text, settings));
            assert_panic(&result, site);
            assert_eq!(counting.peak.load(Ordering::SeqCst), TOKENS, "{route}: all were taken");
            assert_eq!(counting.held.load(Ordering::SeqCst), 0, "{route}: none is held");
            assert_eq!(plan.workers.started.load(Ordering::SeqCst), TOKENS, "{route}");
            assert_eq!(plan.workers.live.load(Ordering::SeqCst), 0, "{route}: none is running");
            drop(counting);
            assert_eq!(pool.idle(), TOKENS, "{route}, {site:?}: every token is back");
        }
    }
}

/// The seed corpus (`tests/ir`), each module for `triple`.
fn seed_modules(triple: &str) -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ir");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/ir")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "ll"))
        .collect();
    files.sort();
    let mut modules = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("a seed test");
        // `;@ module:` splits an entry of several modules.
        let mut parts: Vec<(String, String)> = Vec::new();
        for line in text.split_inclusive('\n') {
            if let Some(module) = line.strip_prefix(";@ module:") {
                parts.push((format!("{name}#{}", module.trim()), String::new()));
            }
            if let Some((_, part)) = parts.last_mut() {
                part.push_str(line);
            }
        }
        if parts.is_empty() {
            parts.push((name, text));
        }
        let header = format!("target triple = \"{triple}\"\n");
        modules.extend(parts.into_iter().map(|(name, text)| (name, format!("{header}{text}"))));
    }
    modules
}

// "Embedding", "Panics": after a contained panic at any site, the next
// compile in the process, on the thread that panicked, writes the bytes a
// thread that never saw a panic writes, for every seed module and both
// targets (the per-thread buffers of code generation are the state a
// panic could leave behind on that thread).
#[test]
fn the_next_compile_after_a_panic_writes_the_same_bytes() {
    let sites = [Site::EndModule, Site::Feed, Site::Worker, Site::Codegen, Site::Finish];
    let mut objects = 0;
    let mut panics = [0; 5];
    for triple in [AARCH64, X86_64] {
        let modules = seed_modules(triple);
        // The reference: each module on a thread of its own, no panic.
        let fresh: Vec<_> = std::thread::scope(|scope| {
            let made: Vec<_> = modules
                .iter()
                .map(|(_, text)| {
                    scope.spawn(move || through_session(text, Settings::default()).ok())
                })
                .collect();
            made.into_iter().map(|made| made.join().expect("no panic")).collect()
        });
        for ((name, text), fresh) in modules.iter().zip(fresh) {
            let Some(fresh) = fresh else { continue };
            objects += 1;
            for (at, site) in sites.into_iter().enumerate() {
                let (result, _) = armed(site, || through_session(text, many_threads()));
                // A site the module does not reach (no body, one body
                // and no worker) leaves the compile as it is.
                let panicked = matches!(&result, Err(CompileError::Panic(_)));
                panics[at] += usize::from(panicked);
                assert!(
                    panicked || result.as_ref() == Ok(&fresh),
                    "{name} ({triple}), {site:?}: {result:?}"
                );
                let next = through_session(text, Settings::default());
                assert_eq!(next.as_ref(), Ok(&fresh), "{name} ({triple}), after {site:?}");
            }
        }
    }
    assert!(objects >= 60, "{objects} objects");
    assert_eq!(panics[0], objects, "end_module is always reached");
    assert!(panics.iter().all(|&n| n >= objects / 2), "{panics:?} of {objects}");
}
