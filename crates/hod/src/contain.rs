//! Panic containment (`spec/cli.md` "Embedding", design 0004 §4, H4).
//!
//! A panic of hod's under a [`crate::Session`] call or [`crate::compile_with`]
//! is caught at that call's boundary and becomes
//! [`CompileError::Panic`] (reason `internal-panic`): it never unwinds into
//! the caller. What a caught panic can leave behind, and why nothing of
//! it reaches the next compile:
//!
//! - the session's own state (module, accepted bodies, judge): replaced
//!   whole by a poisoned session (`Session::poison`), which answers the
//!   panic from every later call;
//! - worker threads: every thread hod starts is a scoped thread
//!   (`std::thread::scope`, code generation and the readers), joined
//!   before the call returns, panicking or not;
//! - jobserver tokens: held by guards that give them back when dropped,
//!   which unwinding does (`back.rs` `Lease`, `front/read.rs`
//!   `HeldTokens`, `jobserver.rs` `Jobserver`); their locks ignore
//!   poisoning;
//! - per-thread scratch buffers of the code generators (`thread_local!`
//!   in `hod_a64` and `hod_x64`): each is taken out or cleared at the
//!   start of every use, so what a panic leaves in one is never read;
//! - process-wide state: `hod_asm`'s set of interned reason names, a
//!   mutex whose poisoning is ignored and whose content does not depend
//!   on any one compile.
//!
//! hod installs no panic hook and does not silence one: the process's
//! hook runs for a contained panic as for any other (the caller's
//! business, design 0004 §10 answer 5). A second panic while unwinding
//! aborts the process, as Rust does everywhere. With `panic = "abort"`
//! (hod's own release profile) nothing is caught: the process ends.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::CompileError;

/// Runs `call`; a panic in it is [`CompileError::Panic`].
///
/// The caller states in a comment why `call`'s captures are unwind safe
/// (the closure is wrapped in `AssertUnwindSafe` here, once).
pub(crate) fn contained<T>(call: impl FnOnce() -> T) -> Result<T, CompileError> {
    catch_unwind(AssertUnwindSafe(call)).map_err(panic_error)
}

/// The error of a caught panic, its message taken from the payload.
pub(crate) fn panic_error(payload: Box<dyn Any + Send>) -> CompileError {
    let message = if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic without a message".to_owned()
    };
    // Dropping a payload runs its destructor, which may panic in turn;
    // that one is contained too, and its own payload is leaked rather
    // than dropped.
    if let Err(again) = catch_unwind(AssertUnwindSafe(move || drop(payload))) {
        std::mem::forget(again);
    }
    CompileError::Panic(message)
}

/// Forced panics for the containment tests: compiled into the test build
/// only, armed per calling thread (and passed to the workers it starts),
/// so that no other test running in the same process meets them.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Where a forced panic happens.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Site {
        /// `Session::end_module`, before the module pass.
        EndModule,
        /// `Session::body`, while a body is judged.
        Feed,
        /// The command line's front half, as the reader hands a body
        /// over (outside the session's calls).
        #[cfg_attr(not(feature = "readers"), allow(dead_code))]
        Reader,
        /// Code generation on a worker thread, as it starts.
        Worker,
        /// Code generation on the calling thread, as it starts (its
        /// workers started already).
        Codegen,
        /// `Session::finish`, after code generation (emission).
        Finish,
    }

    /// What the worker threads did, for the tests to check.
    #[derive(Debug, Default)]
    pub(crate) struct Workers {
        /// Worker threads started.
        pub(crate) started: AtomicUsize,
        /// Worker threads whose work has not ended.
        pub(crate) live: AtomicUsize,
    }

    /// One test's plan: the site, and the record of its workers.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct Plan {
        pub(crate) site: Option<Site>,
        pub(crate) workers: Arc<Workers>,
    }

    thread_local! {
        static PLAN: RefCell<Plan> = RefCell::new(Plan::default());
        static WORKER: Cell<bool> = const { Cell::new(false) };
    }

    /// Arms `plan` on this thread.
    pub(crate) fn arm(plan: Plan) {
        PLAN.with(|cell| *cell.borrow_mut() = plan);
    }

    /// This thread's plan.
    pub(crate) fn current() -> Plan {
        PLAN.with(|cell| cell.borrow().clone())
    }

    /// Panics if this thread's plan names `site`.
    pub(crate) fn hit(site: Site) {
        let armed = PLAN.with(|cell| cell.borrow().site) == Some(site);
        let worker = WORKER.with(Cell::get);
        let here = match site {
            Site::Worker => worker,
            Site::Codegen => !worker,
            _ => true,
        };
        assert!(!(armed && here), "forced panic at {site:?}");
    }

    /// Counts a worker thread while it lives.
    pub(crate) struct WorkerGuard(Arc<Workers>);

    /// Enters a worker thread started by a thread whose plan is `plan`.
    pub(crate) fn enter_worker(plan: Plan) -> WorkerGuard {
        let workers = plan.workers.clone();
        workers.started.fetch_add(1, Ordering::SeqCst);
        workers.live.fetch_add(1, Ordering::SeqCst);
        WORKER.with(|cell| cell.set(true));
        arm(plan);
        WorkerGuard(workers)
    }

    impl Drop for WorkerGuard {
        fn drop(&mut self) {
            self.0.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests;
