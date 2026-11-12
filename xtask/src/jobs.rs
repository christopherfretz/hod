//! xtask as a GNU make jobserver client (`spec/harness.md` "Jobserver").
//! Ported from kiln's `xtask/src/jobs.rs`.
//!
//! The client itself is `hod_testutil::jobs` (the test binaries are clients
//! too, and share it): the fifo named by `MAKEFLAGS`, the implicit token,
//! the helper thread, [`Token`] with release on `Drop`. This module adds
//! what only a long-running command needs:
//!
//! - [`global`] also starts a thread that catches SIGINT and SIGTERM (a
//!   `signal-hook` thread, so the `unsafe` of signal handling stays in that
//!   dependency): every live child of [`children`] is killed and reaped,
//!   then the held and spare pool bytes are written back, then the signal's
//!   default action ends the process. Only SIGKILL (which no process can
//!   catch) or another uncaught fatal signal loses the tokens xtask held,
//!   as it would for make or cargo; `cargo xtask jobserver refill` restores
//!   the pool afterwards.
//! - [`children`]: the table of live children, and the cargo steps.
//! - [`main`]: `cargo xtask jobserver status|refill`.

pub mod children;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

pub use hod_testutil::jobs::{Client, Refill, Token, fifo_from_makeflags, refill};

/// The jobserver this process inherited, if any (read from `MAKEFLAGS` once).
/// The first call also starts the SIGINT/SIGTERM thread (with or without a
/// pool: it kills the live children either way).
pub fn global() -> &'static Client {
    static SIGNALS: Once = Once::new();
    let client = hod_testutil::jobs::global();
    SIGNALS.call_once(|| release_on_signal(client));
    client
}

/// Whether the SIGINT/SIGTERM thread runs (set once it is started).
static SIGNALS_CAUGHT: AtomicBool = AtomicBool::new(false);

/// Starts the SIGINT/SIGTERM thread if it is not running yet ([`global`])
/// and says whether it runs: when it does, a child in a process group of
/// its own is still stopped by an interrupt of xtask
/// ([`children::run_step`]).
pub fn signals_caught() -> bool {
    global();
    SIGNALS_CAUGHT.load(Ordering::SeqCst)
}

/// How long an interrupted xtask waits for its killed children to be reaped
/// before returning the tokens anyway (a child in uninterruptible sleep).
#[cfg(unix)]
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Starts a thread that, on SIGINT or SIGTERM, kills and reaps every live
/// child ([`children::kill_live_children`]), then returns every pool byte
/// the process holds, then ends it with the signal's default action (so the
/// parent still sees "killed by SIGINT"; exit status 128 + signal if that
/// cannot be emulated). The tokens go back after the children are dead, so
/// the pool is never oversubscribed by an orphan, and exactly once.
#[cfg(unix)]
fn release_on_signal(client: &'static Client) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    let mut signals = match signal_hook::iterator::Signals::new([SIGINT, SIGTERM]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "xtask: warning: cannot catch SIGINT/SIGTERM ({e}); an interrupted run \
                 will neither kill its children nor return its jobserver tokens"
            );
            return;
        }
    };
    let spawned = std::thread::Builder::new().name("jobserver-signals".into()).spawn(move || {
        if let Some(sig) = signals.forever().next() {
            let killed = children::kill_live_children(SHUTDOWN_GRACE);
            if killed > 0 {
                eprintln!("xtask: signal {sig}: killed {killed} child process(es)");
            }
            let n = client.release_all();
            if n > 0 {
                eprintln!("xtask: signal {sig}: returned {n} jobserver token(s)");
            }
            let _ = signal_hook::low_level::emulate_default_handler(sig);
            std::process::exit(128 + sig);
        }
    });
    match spawned {
        Ok(_) => SIGNALS_CAUGHT.store(true, Ordering::SeqCst),
        Err(e) => eprintln!("xtask: warning: cannot start the jobserver signal thread ({e})"),
    }
}

#[cfg(not(unix))]
fn release_on_signal(_: &'static Client) {}

// ----- `cargo xtask jobserver` ---------------------------------------------

const JOBSERVER_USAGE: &str = "\
usage: cargo xtask jobserver <status|refill> [--fifo <path>] [--jobs <n>]

  status   print the idle tokens in the pool (read without blocking and put
           straight back)
  refill   top the pool up to <n> idle tokens: count them with non-blocking
           reads, write them back plus the missing ones with non-blocking
           writes. Only meaningful while no client holds tokens (no build or
           test running): tokens in use come back on top. A pool above <n>
           is reported and left alone.

  --fifo <path>  default: MAKEFLAGS' --jobserver-auth=fifo:, else
                 $HOD_JOBSERVER_FIFO, else /run/hod/jobserver.fifo
  --jobs <n>     the pool size; default $HOD_JOBS, else the number in the
                 file beside the fifo (jobserver.jobs), else 12
";

/// `cargo xtask jobserver status|refill` (`spec/harness.md` "Jobserver").
pub fn main(args: impl IntoIterator<Item = String>) -> ExitCode {
    let env = Env {
        makeflags: std::env::var("MAKEFLAGS").ok(),
        fifo: std::env::var_os("HOD_JOBSERVER_FIFO").map(PathBuf::from),
        jobs: std::env::var("HOD_JOBS").ok(),
    };
    match jobserver_cmd(args, &env) {
        Ok(Some(line)) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Ok(None) => {
            print!("{JOBSERVER_USAGE}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xtask jobserver: {e}\n\n{JOBSERVER_USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The variables the command's defaults come from.
#[derive(Clone, Debug, Default)]
struct Env {
    /// `MAKEFLAGS`.
    makeflags: Option<String>,
    /// `HOD_JOBSERVER_FIFO`.
    fifo: Option<PathBuf>,
    /// `HOD_JOBS`.
    jobs: Option<String>,
}

fn jobserver_cmd(
    args: impl IntoIterator<Item = String>,
    env: &Env,
) -> Result<Option<String>, String> {
    let mut cmd = None;
    let mut fifo = None;
    let mut jobs = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline.clone().or_else(|| it.next()).ok_or_else(|| format!("`{name}` requires a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--fifo" => fifo = Some(PathBuf::from(value("--fifo")?)),
            "--jobs" => {
                let v = value("--jobs")?;
                jobs = Some(v.parse::<usize>().map_err(|_| format!("bad --jobs `{v}`"))?);
            }
            "status" | "refill" if cmd.is_none() => cmd = Some(flag),
            f => return Err(format!("unknown argument `{f}`")),
        }
    }
    let Some(cmd) = cmd else { return Err("missing command (status or refill)".into()) };
    let fifo = fifo.unwrap_or_else(|| default_fifo(env));
    let jobs = match jobs {
        Some(n) => n,
        None => match &env.jobs {
            Some(v) => v.parse().map_err(|_| format!("bad HOD_JOBS `{v}`"))?,
            None => recorded_pool(&fifo).unwrap_or(DEFAULT_POOL),
        },
    };
    let target = if cmd == "refill" { Some(jobs) } else { None };
    let r = refill(&fifo, target).map_err(|e| format!("{}: {e}", fifo.display()))?;
    let at = fifo.display();
    Ok(Some(match target {
        None => format!("jobserver {at}: {} idle token(s) of {jobs}", r.idle),
        Some(_) if r.added > 0 => {
            format!("jobserver {at}: {} idle, added {}: now {jobs}", r.idle, r.added)
        }
        Some(_) if r.idle > jobs => {
            format!("jobserver {at}: {} idle, above {jobs}; left alone", r.idle)
        }
        Some(_) => format!("jobserver {at}: {} idle; full", r.idle),
    }))
}

/// The pool size of `scripts/jobserver.sh` when `HOD_JOBS` is unset and the
/// pool recorded none.
const DEFAULT_POOL: usize = 12;

/// The fifo of `scripts/jobserver.sh` when `HOD_JOBSERVER_FIFO` is unset.
const DEFAULT_FIFO: &str = "/run/hod/jobserver.fifo";

fn default_fifo(env: &Env) -> PathBuf {
    fifo_from_makeflags(env.makeflags.as_deref().unwrap_or_default())
        .or_else(|| env.fifo.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FIFO))
}

/// The size `scripts/jobserver.sh` recorded for the pool at `fifo`: the
/// number in `<fifo without ".fifo">.jobs`.
fn recorded_pool(fifo: &Path) -> Option<usize> {
    let text = fifo.to_str()?;
    let file = format!("{}.jobs", text.strip_suffix(".fifo").unwrap_or(text));
    std::fs::read_to_string(file).ok()?.trim().parse().ok().filter(|&n| n > 0)
}

// spec/harness.md "Jobserver", "`cargo xtask jobserver`". The client's own
// tests are in `hod_testutil::jobs`.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use hod_testutil::jobs::PrivatePool;

    fn cmd(args: &[&str], env: &Env) -> Result<Option<String>, String> {
        jobserver_cmd(args.iter().map(|s| (*s).to_owned()), env)
    }

    #[test]
    fn jobserver_cli() {
        let none = Env::default();
        let a = |v: &[&str]| cmd(v, &none);
        assert_eq!(a(&["--help"]), Ok(None));
        assert!(a(&[]).is_err());
        assert!(a(&["drain"]).is_err());
        assert!(a(&["status", "--jobs", "x"]).is_err());
        let pool = PrivatePool::new("cli", 1);
        let p = pool.path().display().to_string();
        let line = a(&["refill", "--fifo", &p, "--jobs=3"]).unwrap().unwrap();
        assert!(line.ends_with("1 idle, added 2: now 3"), "{line}");
        let line = a(&["status", "--fifo", &p, "--jobs", "3"]).unwrap().unwrap();
        assert!(line.ends_with("3 idle token(s) of 3"), "{line}");
        let line = a(&["refill", "--fifo", &p, "--jobs", "3"]).unwrap().unwrap();
        assert!(line.ends_with("3 idle; full"), "{line}");
        let line = a(&["refill", "--fifo", &p, "--jobs", "2"]).unwrap().unwrap();
        assert!(line.ends_with("3 idle, above 2; left alone"), "{line}");
        assert_eq!(pool.idle(), 3);
        // Not a fifo: an error, not a wait.
        let plain = pool.path().with_file_name("plain");
        std::fs::write(&plain, "").unwrap();
        let refused = a(&["status", "--fifo", &plain.display().to_string()]).unwrap_err();
        assert!(refused.ends_with("not a fifo"), "{refused}");
    }

    // The fifo: MAKEFLAGS' fifo, else $HOD_JOBSERVER_FIFO, else the pool
    // script's default. The size: $HOD_JOBS, else the number the pool
    // script recorded beside the fifo, else 12.
    #[test]
    fn jobserver_cli_defaults() {
        let pool = PrivatePool::new("cli-defaults", 2);
        let other = PrivatePool::new("cli-defaults-other", 5);
        let from_makeflags = Env {
            makeflags: Some(format!("-j4 {}", pool.makeflags())),
            fifo: Some(other.path().to_owned()),
            jobs: None,
        };
        let line = cmd(&["status"], &from_makeflags).unwrap().unwrap();
        assert_eq!(line, format!("jobserver {}: 2 idle token(s) of 12", pool.path().display()));
        let from_variable = Env { makeflags: Some("-j4".into()), ..from_makeflags.clone() };
        let line = cmd(&["status"], &from_variable).unwrap().unwrap();
        assert_eq!(line, format!("jobserver {}: 5 idle token(s) of 12", other.path().display()));
        assert_eq!(default_fifo(&Env::default()), Path::new("/run/hod/jobserver.fifo"));

        // `jobserver.jobs` beside `jobserver.fifo`, as the script writes it.
        let recorded = pool.path().with_file_name("jobserver.jobs");
        std::fs::write(&recorded, "7\n").unwrap();
        assert_eq!(recorded_pool(pool.path()), Some(7));
        let line = cmd(&["status"], &from_makeflags).unwrap().unwrap();
        assert!(line.ends_with("2 idle token(s) of 7"), "{line}");
        let with_jobs = Env { jobs: Some("9".into()), ..from_makeflags.clone() };
        let line = cmd(&["status"], &with_jobs).unwrap().unwrap();
        assert!(line.ends_with("2 idle token(s) of 9"), "{line}");
        let line = cmd(&["status", "--jobs", "4"], &with_jobs).unwrap().unwrap();
        assert!(line.ends_with("2 idle token(s) of 4"), "{line}");
        let bad = Env { jobs: Some("many".into()), ..with_jobs.clone() };
        assert_eq!(cmd(&["status"], &bad).unwrap_err(), "bad HOD_JOBS `many`");
        std::fs::write(&recorded, "nonsense").unwrap();
        assert_eq!(recorded_pool(pool.path()), None);
        // A fifo of another name: `<path>.jobs`.
        assert_eq!(recorded_pool(Path::new("/nonexistent/pool")), None);
    }
}
