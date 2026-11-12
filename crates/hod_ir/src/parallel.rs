//! Reading a module's bodies on several threads (`spec/ir.md` §5.1,
//! "Reading bodies in parallel").
//!
//! The module is frozen; each thread reads bodies against a fork of it,
//! which shares its tables and adds to tables of its own. A body read in
//! a fork is handed to the module as a [`Delta`] of what it added and the
//! body itself, in fork ids. The calling thread adopts the deltas in file
//! order and translates each body, so the module grows exactly as reading
//! the bodies one after the other would grow it, and every id is the
//! sequential reader's. It delivers each body in file order too.
//!
//! A body whose reading fails, or that changes in its fork what a fork
//! cannot hand over, is read again by the calling thread against the
//! module itself, in its turn: its error is the sequential reader's, and
//! the first in file order is the one returned. The fork that met it is
//! made anew.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use crate::body::Body;
use crate::module::{Delta, Module, Remap};

/// A reader of the bodies of one module, one per thread.
pub trait BodySource: Send {
    /// The reader's error.
    type Error: Send;

    /// Reads body `index` (in file order) against `module`: the module
    /// the text was read into, or a fork of it. `Ok(None)` for a body with
    /// constructs the reader cannot represent, which it records in
    /// `module` and names in [`BodySource::unsupported`].
    ///
    /// # Errors
    /// The reader's.
    fn read_body(&mut self, module: &mut Module, index: usize)
    -> Result<Option<Body>, Self::Error>;

    /// The constructs of the last body read that was `Ok(None)`.
    fn unsupported(&self) -> Vec<Box<str>>;
}

/// One body as [`read_bodies`] delivers it: built, or the constructs that
/// kept it from being built.
#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "a body is moved to its consumer, not boxed")]
pub enum BodyRead<'a> {
    /// The body, in the module's ids.
    Body(Body),
    /// Each construct once, the first the reason.
    Unsupported(&'a [Box<str>]),
}

/// How a [`read_bodies`] went.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReadStats {
    /// The threads that read, the calling one included; 0 for the
    /// sequential reading.
    pub threads: usize,
    /// The bodies read again on the module itself, in their turn: an
    /// error, or a change a fork cannot hand over.
    pub read_again: usize,
    /// The body from which the rest was read sequentially, if one read
    /// again changed what the forks read.
    pub sequential_from: Option<usize>,
}

/// How many bodies past the last one delivered a thread may read ahead,
/// per thread: bounds the bodies held at once.
const AHEAD_PER_THREAD: usize = 16;

/// What a fork made of one body.
struct Done {
    /// The fork's work, or `None`: read the body again on the module.
    work: Option<(Delta, Option<Body>, Vec<Box<str>>)>,
    worker: usize,
    generation: u32,
}

struct Shared<'s> {
    /// The frozen module's fork that every fork is made of.
    seed: &'s Module,
    count: usize,
    /// The next body to read.
    next: AtomicUsize,
    stop: AtomicBool,
    /// Results by body, from `delivered` on; and the number delivered.
    state: Mutex<Queue>,
    ready: Condvar,
    ahead: usize,
}

/// Stops the reading when a thread panics, so that no thread waits for
/// a body it will never read; the scope then passes the panic on.
struct HaltOnPanic<'a, 's>(&'a Shared<'s>);

impl Drop for HaltOnPanic<'_, '_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.halt();
        }
    }
}

/// A thread's fork and reader.
struct Worker<R> {
    id: usize,
    fork: Module,
    reader: R,
    generation: u32,
}

impl<R: BodySource> Worker<R> {
    /// Reads body `index` in the fork; on an error or a change the fork
    /// cannot hand over, starts a new fork and reader.
    fn read(&mut self, index: usize, seed: &Module, new_reader: &(dyn Fn() -> R + Sync)) -> Done {
        let mark = self.fork.mark();
        let work = match self.reader.read_body(&mut self.fork, index) {
            Ok(body) => match self.fork.delta(mark) {
                Ok(Some(delta)) => {
                    let unsupported =
                        if body.is_none() { self.reader.unsupported() } else { Vec::new() };
                    Some((delta, body, unsupported))
                }
                _ => None,
            },
            Err(_) => None,
        };
        let done = Done { work, worker: self.id, generation: self.generation };
        if done.work.is_none() {
            self.fork = seed.fork();
            self.reader = new_reader();
            self.generation += 1;
        }
        done
    }
}

/// The results waiting, by body from the first not delivered; and the
/// number delivered.
type Queue = (VecDeque<Option<Done>>, usize);

impl Shared<'_> {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores the result of body `index`.
    fn put(&self, index: usize, done: Done) {
        let mut state = self.lock();
        let at = index - state.1;
        if state.0.len() <= at {
            state.0.resize_with(at + 1, || None);
        }
        state.0[at] = Some(done);
        drop(state);
        self.ready.notify_all();
    }

    /// The next body for a thread to read, waiting while it is too far
    /// ahead of the delivered ones; `None` when there is none or the
    /// reading stopped.
    fn take(&self) -> Option<usize> {
        if self.stop.load(Ordering::Acquire) {
            return None;
        }
        let index = self.next.fetch_add(1, Ordering::AcqRel);
        if index >= self.count {
            return None;
        }
        let mut state = self.lock();
        while index >= state.1 + self.ahead && !self.stop.load(Ordering::Acquire) {
            state = self.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        (!self.stop.load(Ordering::Acquire)).then_some(index)
    }

    fn halt(&self) {
        self.stop.store(true, Ordering::Release);
        let _guard = self.lock();
        self.ready.notify_all();
    }
}

/// Reads the bodies `0..count` with `source`s on up to `threads` threads,
/// the calling one included, and delivers each to `each` in order, with
/// the module the host holds (`module(host)`) as it would be after
/// reading the bodies before it and that one sequentially
/// (`spec/ir.md` §5.1). `each` returns whether to go on.
///
/// With one thread (or one body) it is the sequential reading: one
/// reader, against the module, in order. With more, the module is frozen
/// for the reading and thawed after; the bodies delivered, the module's
/// tables and every id in them are the sequential reading's, at any
/// number of threads.
///
/// # Errors
/// The first error in file order: the error the sequential reading
/// returns, the module as it leaves it.
pub fn read_bodies<H, R: BodySource>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    count: usize,
    threads: usize,
    new_reader: &(dyn Fn() -> R + Sync),
    each: &mut dyn FnMut(&mut H, usize, BodyRead<'_>) -> bool,
) -> Result<ReadStats, R::Error> {
    let mut main = new_reader();
    if threads <= 1 || count <= 1 {
        sequential(host, module, 0, count, &mut main, each)?;
        return Ok(ReadStats::default());
    }
    module(host).freeze();
    // The forks, the seed among them, are gone before the thaw.
    let result = {
        let seed = module(host).fork();
        let shared = Shared {
            seed: &seed,
            count,
            next: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            state: Mutex::new((VecDeque::new(), 0)),
            ready: Condvar::new(),
            ahead: AHEAD_PER_THREAD * threads,
        };
        let shared = &shared;
        std::thread::scope(|scope| {
            for id in 1..threads {
                scope.spawn(move || {
                    let _halt = HaltOnPanic(shared);
                    let mut worker = Worker {
                        id,
                        fork: shared.seed.fork(),
                        reader: new_reader(),
                        generation: 0,
                    };
                    while let Some(index) = shared.take() {
                        let done = worker.read(index, shared.seed, new_reader);
                        shared.put(index, done);
                    }
                });
            }
            let _halt = HaltOnPanic(shared);
            let result = merge(host, module, shared, &mut main, new_reader, threads, each);
            shared.halt();
            result
        })
    };
    module(host).thaw();
    result
}

/// The bodies `from..count`, one after the other.
fn sequential<H, R: BodySource>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    from: usize,
    count: usize,
    reader: &mut R,
    each: &mut dyn FnMut(&mut H, usize, BodyRead<'_>) -> bool,
) -> Result<(), R::Error> {
    for index in from..count {
        if !deliver_read(host, module, index, reader, each)? {
            break;
        }
    }
    Ok(())
}

/// Reads body `index` against the module itself and delivers it; whether
/// to go on.
fn deliver_read<H, R: BodySource>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    index: usize,
    reader: &mut R,
    each: &mut dyn FnMut(&mut H, usize, BodyRead<'_>) -> bool,
) -> Result<bool, R::Error> {
    Ok(match reader.read_body(module(host), index)? {
        Some(body) => each(host, index, BodyRead::Body(body)),
        None => each(host, index, BodyRead::Unsupported(&reader.unsupported())),
    })
}

/// The calling thread: adopts and delivers the bodies in order, reading
/// bodies itself while the next one is not ready.
fn merge<H, R: BodySource>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    shared: &Shared<'_>,
    main: &mut R,
    new_reader: &(dyn Fn() -> R + Sync),
    threads: usize,
    each: &mut dyn FnMut(&mut H, usize, BodyRead<'_>) -> bool,
) -> Result<ReadStats, R::Error> {
    let mut own = Worker { id: 0, fork: shared.seed.fork(), reader: new_reader(), generation: 0 };
    let mut remaps: Vec<(u32, Remap)> = vec![(0, shared.seed.remap()); threads];
    let mut stats = ReadStats { threads, ..ReadStats::default() };
    for index in 0..shared.count {
        let done = loop {
            let mut queue = shared.lock();
            if let Some(done) = queue.0.front_mut().and_then(Option::take) {
                queue.0.pop_front();
                queue.1 = index + 1;
                drop(queue);
                shared.ready.notify_all();
                break done;
            }
            drop(queue);
            // Not ready: read a body meanwhile, or wait for one.
            let next = shared.next.fetch_add(1, Ordering::AcqRel);
            if next < shared.count {
                let done = own.read(next, shared.seed, new_reader);
                shared.put(next, done);
                continue;
            }
            let waiting = shared.lock();
            if waiting.0.front().is_none_or(Option::is_none) {
                if shared.stop.load(Ordering::Acquire) {
                    // A thread panicked: the scope passes its panic on.
                    return Ok(stats);
                }
                drop(shared.ready.wait(waiting).unwrap_or_else(PoisonError::into_inner));
            }
        };
        let Done { work, worker, generation } = done;
        let (remap_generation, remap) = &mut remaps[worker];
        if *remap_generation != generation {
            *remap_generation = generation;
            *remap = shared.seed.remap();
        }
        let adopted = work.and_then(|(delta, body, unsupported)| {
            module(host).adopt(&delta, remap).ok()?;
            match body {
                Some(mut body) => {
                    body.remap(remap).ok()?;
                    Some(Ok(body))
                }
                None => Some(Err(unsupported)),
            }
        });
        let go_on = match adopted {
            Some(Ok(body)) => each(host, index, BodyRead::Body(body)),
            Some(Err(unsupported)) => each(host, index, BodyRead::Unsupported(&unsupported)),
            None => {
                // Read on the module itself, in its turn. If that changed
                // what the forks share, they read a module that is no
                // longer this one: the rest is read here, in order.
                stats.read_again += 1;
                let before = module(host).stamp();
                let go_on = match deliver_read(host, module, index, main, each) {
                    Ok(go_on) => go_on,
                    Err(error) => {
                        shared.halt();
                        return Err(error);
                    }
                };
                if go_on && module(host).stamp() != before {
                    shared.halt();
                    stats.sequential_from = Some(index + 1);
                    sequential(host, module, index + 1, shared.count, main, each)?;
                    return Ok(stats);
                }
                go_on
            }
        };
        if !go_on {
            break;
        }
    }
    Ok(stats)
}
