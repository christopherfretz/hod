//! Extra code-generation threads (`spec/cli.md` "Threads"): from an
//! explicit `HOD_THREADS`, or else from a GNU make jobserver. The client
//! of the shim and of an embedding caller alike ([`crate::Environment`]).
//!
//! A build system already runs one compiler per core, so hod takes no
//! thread it is not given (`docs/ARCHITECTURE.md` "Parallelism"). Under a
//! jobserver (`MAKEFLAGS` with `--jobserver-auth=fifo:<path>`, make 4.4's
//! form) hod owns its implicit token and takes more only for a module
//! large enough to use them, only as many as are idle at that moment
//! (never waiting: a read of the fifo opened without blocking), and gives
//! each back when code generation ends. A hod killed in between loses
//! them, as a killed make job does. The older `--jobserver-fds=R,W` and
//! `--jobserver-auth=R,W` forms name inherited descriptors and are not
//! used: hod runs on its implicit token alone.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::{ThreadTokens, Threads};

/// The most threads hod uses under a jobserver.
pub const JOBSERVER_MAX_THREADS: usize = 16;

/// The fifo of the last `--jobserver-auth=` word of `MAKEFLAGS` when it is
/// the `fifo:<path>` form (make passes the last one as authoritative).
pub fn fifo_from_makeflags(flags: &str) -> Option<PathBuf> {
    flags
        .split_ascii_whitespace()
        .filter_map(|word| word.strip_prefix("--jobserver-auth="))
        .next_back()?
        .strip_prefix("fifo:")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// A jobserver fifo opened without blocking, and the tokens taken from it
/// and not yet given back.
#[derive(Debug)]
pub struct Jobserver {
    fifo: File,
    held: Mutex<Vec<u8>>,
}

/// `O_NONBLOCK` on Linux for the architectures hod runs on.
#[cfg(target_os = "linux")]
const O_NONBLOCK: Option<i32> = Some(0o4000);
#[cfg(not(target_os = "linux"))]
const O_NONBLOCK: Option<i32> = None;

impl Jobserver {
    /// Opens the fifo at `path`, read and write, without blocking; `None`
    /// if it is not a fifo or cannot be opened (hod then runs alone).
    pub fn open(path: &Path) -> Option<Jobserver> {
        use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
        let flag = O_NONBLOCK?;
        if !std::fs::metadata(path).ok()?.file_type().is_fifo() {
            return None;
        }
        let fifo = OpenOptions::new().read(true).write(true).custom_flags(flag).open(path).ok()?;
        Some(Jobserver { fifo, held: Mutex::new(Vec::new()) })
    }
}

impl ThreadTokens for Jobserver {
    fn take(&self, wanted: usize) -> usize {
        let mut bytes = vec![0; wanted];
        let mut got = 0;
        while got < wanted {
            match (&self.fifo).read(&mut bytes[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                // `WouldBlock`: no idle token now. Anything else: none.
                Err(_) => break,
            }
        }
        self.held.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(&bytes[..got]);
        got
    }

    fn give_back(&self, count: usize) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let from = held.len().saturating_sub(count);
        // A full fifo cannot happen (the bytes came from it); a failed
        // write loses tokens as a killed job would.
        let _ = (&self.fifo).write_all(&held[from..]);
        held.truncate(from);
    }
}

impl Drop for Jobserver {
    fn drop(&mut self) {
        let held = self.held.get_mut().unwrap_or_else(PoisonError::into_inner);
        if !held.is_empty() {
            let _ = (&self.fifo).write_all(held);
        }
    }
}

/// A jobserver opened at its first use: a module too small for threads
/// never touches the fifo.
#[derive(Debug)]
struct Lazy {
    path: PathBuf,
    opened: OnceLock<Option<Jobserver>>,
}

impl Lazy {
    fn get(&self) -> Option<&Jobserver> {
        self.opened.get_or_init(|| Jobserver::open(&self.path)).as_ref()
    }
}

impl ThreadTokens for Lazy {
    fn take(&self, wanted: usize) -> usize {
        self.get().map_or(0, |jobserver| jobserver.take(wanted))
    }

    fn give_back(&self, count: usize) {
        if let Some(jobserver) = self.get() {
            jobserver.give_back(count);
        }
    }
}

/// The threads of one invocation: `explicit` (`HOD_THREADS`) granted
/// outright; else, with a jobserver fifo in `makeflags`, up to
/// [`JOBSERVER_MAX_THREADS`], every one but the first taken from it when
/// a module is large enough (the pool's size is the bound: make sizes it
/// to the machine); else the calling thread alone. Touches no file.
pub fn threads(explicit: Option<usize>, makeflags: Option<&str>) -> Threads {
    if let Some(max) = explicit {
        return Threads { max, ..Threads::default() };
    }
    let Some(path) = makeflags.and_then(fifo_from_makeflags) else {
        return Threads::default();
    };
    let tokens: Arc<dyn ThreadTokens> = Arc::new(Lazy { path, opened: OnceLock::new() });
    Threads { max: JOBSERVER_MAX_THREADS, tokens: Some(tokens), per_thread: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::jobs::PrivatePool;

    #[test]
    fn makeflags_forms() {
        let fifo = |flags| fifo_from_makeflags(flags);
        assert_eq!(fifo("-j8 --jobserver-auth=fifo:/a/b"), Some("/a/b".into()));
        assert_eq!(fifo("--jobserver-auth=fifo:/a --jobserver-auth=fifo:/b"), Some("/b".into()));
        assert_eq!(fifo("--jobserver-auth=3,4"), None);
        assert_eq!(fifo("--jobserver-fds=3,4 -j"), None);
        assert_eq!(fifo("--jobserver-auth=fifo:"), None);
        assert_eq!(fifo(""), None);
    }

    // spec/cli.md "Threads": the explicit setting wins; without one and
    // without a jobserver, one thread.
    #[test]
    fn where_threads_come_from() {
        assert_eq!(threads(Some(3), Some("--jobserver-auth=fifo:/nonexistent")).max, 3);
        assert_eq!(threads(None, None), Threads::default());
        assert_eq!(threads(None, Some("-j4 --jobserver-fds=3,4")), Threads::default());
        // A fifo that is not there gives no token.
        let missing = threads(None, Some("--jobserver-auth=fifo:/nonexistent"));
        assert_eq!(missing.max, JOBSERVER_MAX_THREADS);
        assert_eq!(missing.tokens.as_ref().map(|t| t.take(3)), Some(0));
        let pool = PrivatePool::new("hod-cli-threads", 2);
        let found = threads(None, Some(&pool.makeflags()));
        let tokens = found.tokens.expect("a jobserver");
        assert_eq!(tokens.take(3), 2);
        tokens.give_back(2);
        assert_eq!(tokens.take(1), 1);
        tokens.give_back(1);
    }

    // spec/cli.md "Threads": only idle tokens are taken, never waited
    // for, and every one goes back.
    #[test]
    fn tokens_are_taken_without_waiting_and_given_back() {
        let pool = PrivatePool::new("hod-cli-jobserver", 3);
        let jobserver = Jobserver::open(pool.path()).expect("the pool's fifo");
        assert_eq!(jobserver.take(2), 2);
        assert_eq!(jobserver.take(5), 1, "one was left");
        assert_eq!(jobserver.take(1), 0, "an empty pool does not block");
        jobserver.give_back(3);
        let other = Jobserver::open(pool.path()).expect("the pool's fifo");
        assert_eq!(other.take(8), 3, "all three are back");
        drop(other);
        // Dropping a client gives back what it still holds.
        assert_eq!(jobserver.take(8), 3);
        drop(jobserver);
        let last = Jobserver::open(pool.path()).expect("the pool's fifo");
        assert_eq!(last.take(8), 3);
        last.give_back(3);
        assert!(Jobserver::open(Path::new("/nonexistent/fifo")).is_none());
        assert!(Jobserver::open(Path::new("Cargo.toml")).is_none(), "not a fifo");
    }
}
