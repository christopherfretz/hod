//! GNU make jobserver client (fifo style), so the processes hod's tests and
//! harness spawn share one machine-wide parallelism pool with cargo and
//! rustc (`spec/harness.md` "Jobserver"). Ported from kiln's
//! `xtask/src/jobs.rs`.
//!
//! When `MAKEFLAGS` carries `--jobserver-auth=fifo:<path>` (make 4.4's fifo
//! style; `scripts/env.sh` exports it when the shared pool is live), every
//! child started through [`crate::run_with`] (a test binary) or through the
//! harness's `process::run` (`xtask`) holds a [`Token`] for its lifetime.
//! Like make, the process owns one *implicit* token: one child may run
//! without taking anything from the pool, every further concurrent child
//! reads one byte from the fifo and writes the same byte back when it exits.
//! The test threads of a test binary, and the `-j N` worker threads of the
//! harness, stay what they are, so they are an upper bound on in-flight
//! children; the pool can only lower it. Without a jobserver (no
//! `MAKEFLAGS`, the older `--jobserver-fds=R,W` form, which needs inherited
//! descriptors, or a fifo that cannot be opened) [`Client::acquire`] is free
//! and the threads alone bound the parallelism, exactly as before.
//!
//! Waiting is done by one lazily started helper thread blocked in `read` on
//! the fifo, never by the workers themselves: a worker blocked in `read`
//! could not notice that the implicit token became free in the meantime.
//! Tokens are released by `Drop`, so a panicking worker, a child killed by
//! a limit, and an early `?` return all give the token back.
//!
//! This crate handles no signals. `xtask` does (`xtask/src/jobs.rs`: a
//! `signal-hook` thread kills its children and calls
//! [`Client::release_all`] on SIGINT and SIGTERM). A test binary that is
//! killed loses the pool tokens it held, as a process killed with SIGKILL
//! does, and as make or cargo would; `cargo xtask jobserver refill` (or
//! `scripts/jobserver.sh refill`) restores the pool afterwards.

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// The jobserver this process inherited, if any (read from `MAKEFLAGS`
/// once). A fifo that cannot be used is reported once, on standard error,
/// and the process runs without a jobserver.
pub fn global() -> &'static Client {
    static GLOBAL: OnceLock<Client> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        let flags = std::env::var("MAKEFLAGS").unwrap_or_default();
        let Some(path) = fifo_from_makeflags(&flags) else { return Client::none() };
        Client::open(&path).unwrap_or_else(|e| {
            eprintln!(
                "jobserver: warning: cannot use jobserver fifo {} ({e}); running without it",
                path.display()
            );
            Client::none()
        })
    })
}

/// The fifo of the last `--jobserver-auth=` word of `MAKEFLAGS` when it is
/// the `fifo:<path>` form (make passes the last one as authoritative).
pub fn fifo_from_makeflags(flags: &str) -> Option<PathBuf> {
    flags
        .split_ascii_whitespace()
        .filter_map(|w| w.strip_prefix("--jobserver-auth="))
        .next_back()?
        .strip_prefix("fifo:")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// A jobserver connection, or the "no jobserver" stand-in.
#[derive(Debug)]
pub struct Client {
    pool: Option<Arc<Pool>>,
}

#[derive(Debug)]
struct Pool {
    path: PathBuf,
    fifo: File,
    state: Mutex<State>,
    /// Workers waiting for a token.
    workers: Condvar,
    /// The helper waiting for a read request.
    helper: Condvar,
    /// [`Client::release_all`] waiting for the helper's read in flight.
    quiet: Condvar,
}

// Four independent flags of one mutex-guarded state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct State {
    implicit_free: bool,
    /// Pool bytes read for (or handed over to) waiting workers.
    spare: Vec<u8>,
    waiters: usize,
    /// Reads the helper still has to do (including one in flight).
    requested: usize,
    helper_started: bool,
    /// The helper is in a read, or has a byte it read in its hands.
    reading: bool,
    /// The fifo failed; acquisition is free from then on.
    broken: bool,
    /// Pool bytes held by live [`Token`]s (returned by
    /// [`Client::release_all`] if the process is interrupted).
    held: Vec<u8>,
    /// [`Client::release_all`] ran: every byte went back, so tokens dropped
    /// later return nothing and new ones are free (the process is exiting).
    released: bool,
}

/// A held parallelism slot; dropping it releases it.
#[derive(Debug)]
pub struct Token {
    kind: Kind,
    pool: Option<Arc<Pool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// No jobserver (or a broken one): nothing to give back.
    Free,
    Implicit,
    Pool(u8),
}

impl Token {
    /// Whether the slot was taken from the pool (a byte read from the
    /// fifo), rather than being the implicit token or free.
    pub fn is_from_pool(&self) -> bool {
        matches!(self.kind, Kind::Pool(_))
    }
}

/// `path` is a fifo, or the error that says it is not.
fn require_fifo(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        if !std::fs::metadata(path)?.file_type().is_fifo() {
            return Err(std::io::Error::other("not a fifo"));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

impl Client {
    /// No jobserver: [`Client::acquire`] never waits.
    pub fn none() -> Self {
        Self { pool: None }
    }

    /// Connects to the fifo at `path` (opened read+write, so neither end
    /// ever sees EOF or EPIPE).
    pub fn open(path: &Path) -> std::io::Result<Self> {
        require_fifo(path)?;
        let fifo = OpenOptions::new().read(true).write(true).open(path)?;
        let state = State {
            implicit_free: true,
            spare: Vec::new(),
            waiters: 0,
            requested: 0,
            helper_started: false,
            reading: false,
            broken: false,
            held: Vec::new(),
            released: false,
        };
        Ok(Self {
            pool: Some(Arc::new(Pool {
                path: path.to_owned(),
                fifo,
                state: Mutex::new(state),
                workers: Condvar::new(),
                helper: Condvar::new(),
                quiet: Condvar::new(),
            })),
        })
    }

    /// `fifo:<path>`, or `None` without a jobserver.
    pub fn describe(&self) -> Option<String> {
        self.pool.as_ref().map(|p| format!("fifo:{}", p.path.display()))
    }

    /// Blocks until a slot is available: the implicit token if it is free,
    /// else one byte from the pool. Without a jobserver it returns at once.
    pub fn acquire(&self) -> Token {
        let Some(pool) = &self.pool else { return Token { kind: Kind::Free, pool: None } };
        let token = |kind| Token { kind, pool: Some(Arc::clone(pool)) };
        let mut st = pool.lock();
        if st.implicit_free {
            st.implicit_free = false;
            return token(Kind::Implicit);
        }
        if st.broken || st.released {
            return token(Kind::Free);
        }
        st.waiters += 1;
        st.requested += 1;
        if !st.helper_started {
            st.helper_started = true;
            let p = Arc::clone(pool);
            let spawned =
                std::thread::Builder::new().name("jobserver".into()).spawn(move || helper(&p));
            if let Err(e) = spawned {
                eprintln!(
                    "jobserver: warning: cannot start jobserver helper ({e}); running without it"
                );
                st.broken = true;
            }
        }
        pool.helper.notify_one();
        loop {
            // A spare byte was read (or handed over) for a waiter: take it
            // before the implicit token, otherwise it would be stranded in
            // `spare` with nobody waiting for it — a pool token this process
            // keeps but does not use, and leaks on a normal exit.
            if let Some(b) = st.spare.pop() {
                st.waiters -= 1;
                st.held.push(b);
                return token(Kind::Pool(b));
            }
            if st.implicit_free {
                st.implicit_free = false;
                st.waiters -= 1;
                // The read requested for us is no longer needed.
                st.requested = st.requested.saturating_sub(1);
                return token(Kind::Implicit);
            }
            if st.broken || st.released {
                st.waiters -= 1;
                return token(Kind::Free);
            }
            st = pool.workers.wait(st).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Writes every pool byte this process holds (by live tokens, or read
    /// for waiting workers) back to the fifo, and returns how many. After
    /// it, dropping a token returns nothing and acquiring is free: it is
    /// the last act of an interrupted process. Idempotent.
    ///
    /// The helper may be in a read that nobody waits for any more, and
    /// that read takes the next byte that reaches the fifo, one of those
    /// written here as likely as not, to write it straight back. A process
    /// that ended in between would take the byte with it, so this waits
    /// (at most [`HELPER_GRACE`]) until the helper has no read in flight;
    /// it starts no other.
    pub fn release_all(&self) -> usize {
        self.release_all_within(HELPER_GRACE)
    }

    /// [`Client::release_all`], waiting at most `grace` for the helper.
    fn release_all_within(&self, grace: Duration) -> usize {
        let Some(pool) = &self.pool else { return 0 };
        let bytes: Vec<u8> = {
            let mut st = pool.lock();
            st.released = true;
            st.requested = 0;
            let mut v = std::mem::take(&mut st.held);
            v.append(&mut st.spare);
            v
        };
        pool.workers.notify_all();
        for &b in &bytes {
            pool.give_back(b);
        }
        let deadline = Instant::now() + grace;
        let mut st = pool.lock();
        while st.reading {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            st = pool.quiet.wait_timeout(st, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        bytes.len()
    }
}

/// How long [`Client::release_all`] waits for the helper's read in flight
/// to get a byte and write it back. With an empty pool the read gets
/// none, and the wait is this long.
const HELPER_GRACE: Duration = Duration::from_millis(200);

impl Pool {
    /// The state is a handful of counters updated atomically under the lock,
    /// so a poisoned lock still holds a consistent value.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns `byte` to the shared pool.
    fn give_back(&self, byte: u8) {
        if let Err(e) = (&self.fifo).write_all(&[byte]) {
            eprintln!(
                "jobserver: warning: lost a jobserver token: write to {} failed: {e}",
                self.path.display()
            );
        }
    }
}

/// Reads one pool byte per request; a byte nobody waits for any more goes
/// straight back to the pool.
fn helper(pool: &Pool) {
    loop {
        {
            let mut st = pool.lock();
            while st.requested == 0 {
                st = pool.helper.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
            st.reading = true;
        }
        let mut byte = [0u8; 1];
        let r = (&pool.fifo).read_exact(&mut byte);
        let mut st = pool.lock();
        st.requested = st.requested.saturating_sub(1);
        match r {
            Ok(()) if st.waiters > st.spare.len() && !st.released => {
                st.spare.push(byte[0]);
                pool.workers.notify_all();
            }
            Ok(()) => {
                drop(st);
                pool.give_back(byte[0]);
                st = pool.lock();
            }
            Err(e) => {
                eprintln!(
                    "jobserver: warning: jobserver read from {} failed ({e}); running without it",
                    pool.path.display()
                );
                st.broken = true;
                st.reading = false;
                pool.workers.notify_all();
                pool.quiet.notify_all();
                return;
            }
        }
        // The byte is in `spare` or back in the fifo.
        st.reading = false;
        pool.quiet.notify_all();
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        let Some(pool) = &self.pool else { return };
        match self.kind {
            Kind::Free => {}
            Kind::Implicit => {
                pool.lock().implicit_free = true;
                pool.workers.notify_all();
            }
            Kind::Pool(b) => {
                let mut st = pool.lock();
                if st.released {
                    // `release_all` already wrote this byte back.
                    return;
                }
                if let Some(i) = st.held.iter().position(|&h| h == b) {
                    st.held.swap_remove(i);
                }
                if st.waiters > st.spare.len() {
                    // Hand it to a waiting worker directly; its read request
                    // is then satisfied.
                    st.spare.push(b);
                    st.requested = st.requested.saturating_sub(1);
                    pool.workers.notify_all();
                } else {
                    drop(st);
                    pool.give_back(b);
                }
            }
        }
    }
}

// ----- counting and refilling a pool ----------------------------------------

/// What [`refill`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refill {
    /// Idle tokens found.
    pub idle: usize,
    /// Tokens written on top of the ones put back.
    pub added: usize,
}

/// `O_NONBLOCK` for `OpenOptionsExt::custom_flags` (a plain constant; no
/// `libc` dependency for one number).
#[cfg(all(
    any(target_os = "linux", target_os = "android"),
    not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "sparc",
        target_arch = "sparc64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    ))
))]
const O_NONBLOCK: Option<i32> = Some(0o4000);
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
const O_NONBLOCK: Option<i32> = Some(0x0004);
#[cfg(not(any(
    all(
        any(target_os = "linux", target_os = "android"),
        not(any(
            target_arch = "mips",
            target_arch = "mips64",
            target_arch = "sparc",
            target_arch = "sparc64",
            target_arch = "powerpc",
            target_arch = "powerpc64"
        ))
    ),
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
const O_NONBLOCK: Option<i32> = None;

/// Drains the fifo at `path` without blocking, writes the bytes back, and
/// with `Some(target)` adds `+` bytes up to `target` idle tokens (a pool
/// above the target is left alone). `None` only counts. It never blocks,
/// whatever the state of the pool: an empty fifo, or one nobody else has
/// open, has no idle tokens.
pub fn refill(path: &Path, target: Option<usize>) -> std::io::Result<Refill> {
    use std::io::ErrorKind;
    require_fifo(path)?;
    let Some(flag) = O_NONBLOCK else {
        return Err(std::io::Error::other("non-blocking open is not supported on this platform"));
    };
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, flag);
    #[cfg(not(unix))]
    let _ = flag;
    let mut f = opts.open(path)?;
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        match f.read(&mut buf) {
            // Opened read+write, so there is always a writer: 0 cannot be
            // EOF, but stop rather than spin.
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => {
                // Put back what was taken before failing.
                let _ = f.write_all(&got);
                return Err(e);
            }
        }
    }
    let idle = got.len();
    let added = target.map_or(0, |t| t.saturating_sub(idle));
    got.resize(idle + added, b'+');
    f.write_all(&got)?;
    Ok(Refill { idle, added })
}

/// A directory on the machine's own file system, removed on drop: in the
/// system's temporary directory, under a name unique to this process and
/// call. For a test's fifo, and what a test's child processes share.
///
/// Not a [`crate::Scratch`]: that is in the build directory, which on a
/// shared machine is a bind mount of the host, where a fifo is unreliable
/// (`scripts/jobserver.sh`; the machine's own pool is in `/run` for that
/// reason).
#[derive(Debug)]
pub struct LocalDir {
    path: PathBuf,
}

impl LocalDir {
    /// Creates a new directory whose name starts with `hod-jobs-<label>`.
    ///
    /// # Panics
    /// Panics if the directory cannot be created.
    pub fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("hod-jobs-{label}-{}-{unique}", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
        Self { path }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside the directory.
    pub fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for LocalDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A pool of its own for a test: a fifo primed with tokens, kept open by
/// this value as the holder of `scripts/jobserver.sh` keeps the machine's
/// open (a fifo forgets its bytes when nobody has it open). Tests of the
/// jobserver never use, and never disturb, the pool of the machine.
#[derive(Debug)]
pub struct PrivatePool {
    path: PathBuf,
    _holder: File,
    dir: LocalDir,
}

impl PrivatePool {
    /// Creates a fifo with `tokens` idle tokens in a new [`LocalDir`] whose
    /// name has `label` in it.
    ///
    /// # Panics
    /// Panics if the fifo cannot be made (`mkfifo`) or written.
    pub fn new(label: &str, tokens: usize) -> Self {
        let dir = LocalDir::new(label);
        let path = dir.join("jobserver.fifo");
        let made = crate::run("mkfifo", [&path]);
        assert!(made.success(), "mkfifo {}: {}", path.display(), made.stderr_text());
        let mut holder = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));
        holder
            .write_all(&vec![b'+'; tokens])
            .unwrap_or_else(|e| panic!("cannot fill {}: {e}", path.display()));
        Self { path, _holder: holder, dir }
    }

    /// The fifo.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `MAKEFLAGS` value that makes a child process a client of this
    /// pool.
    pub fn makeflags(&self) -> String {
        format!("--jobserver-auth=fifo:{}", self.path.display())
    }

    /// The idle tokens, counted without blocking.
    ///
    /// # Panics
    /// Panics if the fifo cannot be read.
    pub fn idle(&self) -> usize {
        idle(&self.path)
    }

    /// Closes the holder's end and keeps the fifo: what is left of a pool
    /// whose holder is gone. Nobody has it open, so its tokens are gone
    /// too. The fifo is removed with the directory returned.
    pub fn abandon(self) -> (PathBuf, LocalDir) {
        (self.path, self.dir)
    }
}

/// The idle tokens of the pool at `path`, counted without blocking.
///
/// # Panics
/// Panics if `path` is not a fifo that can be read.
pub fn idle(path: &Path) -> usize {
    refill(path, None).unwrap_or_else(|e| panic!("cannot count {}: {e}", path.display())).idle
}

// spec/harness.md "Jobserver": the client. Ported from kiln's tests of the
// same names.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::Scratch;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // "Jobserver", "What is honoured" and "Fallback": only the fifo form.
    #[test]
    fn parses_makeflags() {
        let f = |s: &str| fifo_from_makeflags(s);
        assert_eq!(f(""), None);
        assert_eq!(f("-j8"), None);
        assert_eq!(
            f(" -j8 --jobserver-auth=fifo:/run/hod/jobserver.fifo"),
            Some(PathBuf::from("/run/hod/jobserver.fifo"))
        );
        assert_eq!(f("--jobserver-auth=fifo:/a --jobserver-auth=fifo:/b"), Some("/b".into()));
        // The descriptor forms need inherited fds: ignored.
        assert_eq!(f("-j --jobserver-auth=3,4"), None);
        assert_eq!(f("--jobserver-fds=3,4 -j"), None);
        assert_eq!(f("--jobserver-auth=fifo:"), None);
        // The last word is the authoritative one, whatever its form.
        assert_eq!(f("--jobserver-auth=fifo:/a --jobserver-auth=3,4"), None);
    }

    // "Jobserver", "Fallback": without a jobserver nothing waits.
    #[test]
    fn no_jobserver_is_free() {
        let c = Client::none();
        assert_eq!(c.describe(), None);
        let toks: Vec<Token> = (0..16).map(|_| c.acquire()).collect();
        assert!(toks.iter().all(|t| t.kind == Kind::Free && !t.is_from_pool()));
        assert_eq!(c.release_all(), 0);
    }

    /// A private pool of `tokens`.
    fn fifo(tag: &str, tokens: usize) -> (PathBuf, PrivatePool) {
        let pool = PrivatePool::new(tag, tokens);
        (pool.path().to_owned(), pool)
    }

    /// Idle tokens in the fifo (drained non-destructively via a probe client).
    fn idle(path: &Path, expect: usize) -> usize {
        // Read exactly `expect` bytes with a blocking reader in a thread; if
        // fewer are there it would hang, so bound it with a timeout.
        let (tx, rx) = std::sync::mpsc::channel();
        let p = path.to_owned();
        std::thread::spawn(move || {
            let mut f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
            let mut buf = vec![0u8; expect];
            f.read_exact(&mut buf).unwrap();
            f.write_all(&buf).unwrap();
            let _ = tx.send(());
        });
        usize::from(rx.recv_timeout(Duration::from_secs(5)).is_ok()) * expect
    }

    // "Jobserver", "Fallback": a path that is not a fifo is not a pool.
    #[test]
    fn rejects_a_non_fifo() {
        let scratch = Scratch::new("jobs-plain");
        let p = scratch.write("plain", "++");
        assert!(Client::open(&p).is_err());
        assert!(Client::open(&scratch.join("missing")).is_err());
    }

    // "Jobserver", "The implicit token": at most pool + 1 slots at once,
    // and every pool byte comes back.
    #[test]
    fn bounds_concurrency_by_pool_plus_implicit() {
        let (path, _holder) = fifo("bound", 2);
        let c = Client::open(&path).unwrap();
        assert!(c.describe().unwrap().starts_with("fifo:"));
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..5 {
                        let _t = c.acquire();
                        let n = live.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(n, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(5));
                        live.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        });
        let peak = peak.load(Ordering::SeqCst);
        assert!(peak <= 3, "peak {peak} exceeds 2 pool tokens + the implicit one");
        assert!(peak >= 2, "peak {peak}: the pool was never used");
        // Every pool token came back.
        assert_eq!(idle(&path, 2), 2);
    }

    // "Jobserver", "The implicit token": a waiter for the pool takes the
    // implicit token when that is released first.
    #[test]
    fn implicit_token_frees_a_waiter() {
        // Empty pool: only the implicit token exists.
        let (path, _holder) = fifo("implicit", 0);
        let c = Client::open(&path).unwrap();
        let first = c.acquire();
        assert_eq!(first.kind, Kind::Implicit);
        std::thread::scope(|s| {
            let h = s.spawn(|| c.acquire().kind);
            std::thread::sleep(Duration::from_millis(50));
            drop(first);
            assert_eq!(h.join().unwrap(), Kind::Implicit);
        });
    }

    // "Jobserver", "Signals" steps 2 and 3: everything held goes back
    // once, and exactly once.
    #[test]
    fn release_all_returns_held_and_spare_bytes() {
        let (path, _holder) = fifo("release", 2);
        let c = Client::open(&path).unwrap();
        let implicit = c.acquire();
        let a = c.acquire();
        let b = c.acquire();
        assert!(matches!((a.kind, b.kind), (Kind::Pool(_), Kind::Pool(_))));
        assert!(a.is_from_pool() && !implicit.is_from_pool());
        assert_eq!(refill(&path, None).unwrap().idle, 0);
        assert_eq!(c.release_all(), 2);
        assert_eq!(c.release_all(), 0, "idempotent");
        // Dropping the tokens afterwards does not return them twice, and a
        // new acquisition is free.
        drop((a, b));
        assert_eq!(c.acquire().kind, Kind::Free);
        assert_eq!(refill(&path, None).unwrap(), Refill { idle: 2, added: 0 });
    }

    // "Jobserver", "Signals" step 2: the helper's read in flight, which
    // nobody waits for any more, takes a byte `release_all` wrote and
    // writes it back; `release_all` returns only then, so a process that
    // ends right after it leaves the pool whole.
    #[test]
    fn release_all_waits_for_the_helpers_read() {
        for round in 0..20 {
            let (path, _holder) = fifo("release-helper", 1);
            let c = Client::open(&path).unwrap();
            let implicit = c.acquire();
            let held = c.acquire();
            assert!(held.is_from_pool() && !implicit.is_from_pool());
            std::thread::scope(|s| {
                // A waiter: the helper starts a read on the empty fifo.
                let waiter = s.spawn(|| c.acquire().kind);
                let pool = c.pool.as_ref().unwrap();
                while !pool.lock().reading {
                    std::thread::sleep(Duration::from_millis(1));
                }
                // The grace of a real exit is HELPER_GRACE, which a helper
                // thread that is not scheduled for that long (a loaded
                // machine) outlasts. What is tested is that the wait is for
                // the read in flight, so the grace here is a hang guard.
                assert_eq!(c.release_all_within(Duration::from_mins(1)), 1);
                assert!(!pool.lock().reading, "round {round}: the wait timed out");
                assert_eq!(refill(&path, None).unwrap().idle, 1, "round {round}");
                assert_eq!(waiter.join().unwrap(), Kind::Free);
            });
            drop((implicit, held));
            assert_eq!(refill(&path, None).unwrap().idle, 1, "round {round}");
        }
        // With no byte to be had, the wait is bounded.
        let (path, _holder) = fifo("release-helper-empty", 0);
        let c = Client::open(&path).unwrap();
        let _implicit = c.acquire();
        std::thread::scope(|s| {
            let waiter = s.spawn(|| c.acquire().kind);
            let pool = c.pool.as_ref().unwrap();
            while !pool.lock().reading {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(c.release_all(), 0);
            assert_eq!(waiter.join().unwrap(), Kind::Free);
        });
    }

    // "Jobserver", "`cargo xtask jobserver`": counting and topping up
    // never block.
    #[test]
    fn refill_tops_up_without_blocking() {
        let (path, pool) = fifo("refill", 3);
        assert_eq!(refill(&path, None).unwrap(), Refill { idle: 3, added: 0 });
        assert_eq!(refill(&path, Some(5)).unwrap(), Refill { idle: 3, added: 2 });
        assert_eq!(refill(&path, Some(5)).unwrap(), Refill { idle: 5, added: 0 });
        // Above the target: left alone.
        assert_eq!(refill(&path, Some(4)).unwrap(), Refill { idle: 5, added: 0 });
        assert_eq!(refill(&path, None).unwrap().idle, 5);
        assert_eq!(pool.idle(), 5);
        // An empty pool is counted, not waited on.
        let (empty, _h) = fifo("refill-empty", 0);
        assert_eq!(refill(&empty, Some(2)).unwrap(), Refill { idle: 0, added: 2 });
        let scratch = Scratch::new("jobs-refill-plain");
        let p = scratch.write("plain", "");
        assert!(refill(&p, Some(1)).is_err(), "not a fifo");
    }

    // "Jobserver", "`cargo xtask jobserver`": a fifo that outlived its
    // holder (a restart of the machine: nobody has it open, it holds no
    // token) is counted as empty at once.
    #[test]
    fn a_stale_fifo_is_counted_without_blocking() {
        // The last holder goes, and the bytes with it; the fifo stays.
        let (path, _dir) = PrivatePool::new("stale", 4).abandon();
        // A child another test of this process is starting holds a copy of
        // the pool's descriptors from its fork to its exec, and the bytes
        // live that long, which under load has been longer than any fixed
        // wait. So the count is judged once no process has the fifo open
        // (none can open it any more: it was never shared by path). The
        // wait has a bound only so that it cannot hang; every count must
        // still return without blocking.
        let hang_guard = Instant::now() + Duration::from_mins(5);
        loop {
            let (tx, rx) = std::sync::mpsc::channel();
            let p = path.clone();
            let quiet = openers(&p) == Some(0);
            std::thread::spawn(move || {
                let _ = tx.send(refill(&p, None));
            });
            // A refill that blocks blocks for good: the bound is a hang
            // guard, not a speed requirement.
            let counted = rx.recv_timeout(Duration::from_mins(1)).expect("refill blocked");
            let counted = counted.unwrap();
            if counted == (Refill { idle: 0, added: 0 }) {
                break;
            }
            assert!(!quiet, "nobody had the abandoned fifo open, yet it held {counted:?}");
            assert!(Instant::now() < hang_guard, "the abandoned fifo still holds {counted:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// How many processes have the fifo at `path` open, by `/proc`; `None`
    /// where that cannot be seen (no `/proc`).
    fn openers(path: &Path) -> Option<usize> {
        let path = std::fs::canonicalize(path).ok()?;
        let procs = std::fs::read_dir("/proc").ok()?;
        let mut n = 0;
        for proc in procs.flatten() {
            let Ok(fds) = std::fs::read_dir(proc.path().join("fd")) else { continue };
            if fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|to| to == path)) {
                n += 1;
            }
        }
        Some(n)
    }

    // "Jobserver", "Release": a panic returns the token.
    #[test]
    fn tokens_return_on_panic() {
        let (path, _holder) = fifo("panic", 1);
        let c = Client::open(&path).unwrap();
        let _implicit = c.acquire();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let t = c.acquire();
            assert!(matches!(t.kind, Kind::Pool(b'+')));
            panic!("worker died");
        }));
        assert!(r.is_err());
        assert_eq!(idle(&path, 1), 1);
    }

    // "Jobserver", "Fallback": a fifo I/O error after the start makes
    // acquisition free instead of hanging the waiters.
    #[test]
    fn a_broken_pool_is_free() {
        let (path, _holder) = fifo("broken", 0);
        let c = Client::open(&path).unwrap();
        let _implicit = c.acquire();
        c.pool.as_ref().unwrap().lock().broken = true;
        assert_eq!(c.acquire().kind, Kind::Free);
    }
}
