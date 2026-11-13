//! Reading a module's bodies on several threads (`spec/ir.md` §5.1,
//! "Reading bodies in parallel").
//!
//! The module stays as it is. A seed made of it once holds a copy of its
//! tables, layered ([`Layered`]); each thread reads bodies against a fork
//! of the seed, which shares those tables and adds to tables of its own. A body read in
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
use crate::intern::{Layered, Layering};
use crate::module::{Delta, Module, Remap};

/// A reader of the bodies of one module, one per thread.
pub trait BodySource: Send {
    /// The reader's error.
    type Error: Send;

    /// Reads body `index` (in file order) against `module`: the module
    /// the text was read into ([`crate::NoLayer`]), or a fork of its seed
    /// ([`Layered`]). The two are separate instantiations: the one-thread
    /// reading runs only the first. `Ok(None)` for a body with
    /// constructs the reader cannot represent, which it records in
    /// `module` and names in [`BodySource::unsupported`].
    ///
    /// # Errors
    /// The reader's.
    fn read_body<L: Layering>(
        &mut self,
        module: &mut Module<L>,
        index: usize,
    ) -> Result<Option<Body>, Self::Error>;

    /// The constructs of the last body read that was `Ok(None)`.
    fn unsupported(&self) -> Vec<Box<str>>;

    /// About what reading body `index` costs, in bytes of LLVM IR text
    /// or their equivalent: bodies are handed to threads in runs of
    /// consecutive bodies of about [`CHUNK_WEIGHT`].
    fn weight(&self, index: usize) -> usize {
        let _ = index;
        CHUNK_WEIGHT
    }
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

/// The weight ([`BodySource::weight`]) of the run of consecutive bodies a
/// thread takes at once, about: a lock and a wake-up per run, not per
/// body. Smaller when the bodies would not give each thread four runs.
pub const CHUNK_WEIGHT: usize = 16 << 10;

/// How many runs past the last one delivered a thread may read ahead,
/// per thread: bounds the bodies held at once.
const AHEAD_PER_THREAD: usize = 4;

/// What a fork made of one body.
struct Done {
    /// The fork's work, or `None`: read the body again on the module.
    work: Option<(Delta, Option<Body>, Vec<Box<str>>)>,
    worker: usize,
    generation: u32,
}

/// The runs read and not yet delivered, from the first not delivered.
struct Queue {
    runs: VecDeque<Option<Vec<Done>>>,
    /// The runs delivered.
    delivered: usize,
    /// Whether the calling thread waits for the first run.
    merger_waits: bool,
    /// How many threads wait for room ahead.
    readers_wait: usize,
}

struct Shared<'s> {
    /// The module's seed, of which every fork is made.
    seed: &'s Module<Layered>,
    /// Run `r` is the bodies `starts[r]..starts[r + 1]`.
    starts: Vec<usize>,
    /// The next run to read.
    next: AtomicUsize,
    stop: AtomicBool,
    state: Mutex<Queue>,
    /// The first run is ready.
    filled: Condvar,
    /// A run was delivered: room ahead.
    room: Condvar,
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
    fork: Module<Layered>,
    reader: R,
    generation: u32,
}

impl<R: BodySource> Worker<R> {
    /// Reads body `index` in the fork; on an error or a change the fork
    /// cannot hand over, starts a new fork and reader.
    fn read(
        &mut self,
        index: usize,
        seed: &Module<Layered>,
        new_reader: &(dyn Fn() -> R + Sync),
    ) -> Done {
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

    /// Reads run `run`, body by body.
    fn read_run(
        &mut self,
        shared: &Shared<'_>,
        run: usize,
        new_reader: &(dyn Fn() -> R + Sync),
    ) -> Vec<Done> {
        let bodies = shared.starts[run]..shared.starts[run + 1];
        bodies.map(|index| self.read(index, shared.seed, new_reader)).collect()
    }
}

impl Shared<'_> {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn runs(&self) -> usize {
        self.starts.len() - 1
    }

    /// Stores what run `run` made; wakes the calling thread if it waits
    /// for that run.
    fn put(&self, run: usize, done: Vec<Done>) {
        let mut state = self.lock();
        let at = run - state.delivered;
        if state.runs.len() <= at {
            state.runs.resize_with(at + 1, || None);
        }
        state.runs[at] = Some(done);
        let wake = at == 0 && state.merger_waits;
        drop(state);
        if wake {
            self.filled.notify_one();
        }
    }

    /// The next run for a thread to read, waiting while it is too far
    /// ahead of the delivered ones; `None` when there is none or the
    /// reading stopped.
    fn take(&self) -> Option<usize> {
        if self.stop.load(Ordering::Acquire) {
            return None;
        }
        let run = self.next.fetch_add(1, Ordering::AcqRel);
        if run >= self.runs() {
            return None;
        }
        let mut state = self.lock();
        while run >= state.delivered + self.ahead && !self.stop.load(Ordering::Acquire) {
            state.readers_wait += 1;
            state = self.room.wait(state).unwrap_or_else(PoisonError::into_inner);
            state.readers_wait -= 1;
        }
        (!self.stop.load(Ordering::Acquire)).then_some(run)
    }

    fn halt(&self) {
        self.stop.store(true, Ordering::Release);
        let _guard = self.lock();
        self.filled.notify_all();
        self.room.notify_all();
    }
}

/// Run boundaries: consecutive bodies of about `CHUNK_WEIGHT` together,
/// at least four runs per thread where there are bodies enough.
fn runs<R: BodySource>(reader: &R, count: usize, threads: usize) -> Vec<usize> {
    let weights: Vec<usize> = (0..count).map(|index| reader.weight(index).max(1)).collect();
    let total: usize = weights.iter().sum();
    let target = CHUNK_WEIGHT.min(total / (threads * 4)).max(1);
    let mut starts = vec![0];
    let mut weight = 0;
    for (index, body) in weights.into_iter().enumerate() {
        weight += body;
        if weight >= target {
            starts.push(index + 1);
            weight = 0;
        }
    }
    if *starts.last().expect("a start") != count {
        starts.push(count);
    }
    starts
}

/// Reads the bodies `0..count` with `source`s on up to `threads` threads,
/// the calling one included, and delivers each to `each` in order, with
/// the module the host holds (`module(host)`) as it would be after
/// reading the bodies before it and that one sequentially
/// (`spec/ir.md` §5.1). `each` returns whether to go on.
///
/// With one thread (or one body) it is the sequential reading: one
/// reader, against the module, in order, and nothing else. With more, a
/// seed is made of the module (a copy of its tables) and forks of it read
/// the bodies; the bodies delivered, the module's tables and every id in
/// them are the sequential reading's, at any number of threads.
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
    let starts = runs(&main, count, threads);
    {
        let seed = module(host).seed();
        let queue =
            Queue { runs: VecDeque::new(), delivered: 0, merger_waits: false, readers_wait: 0 };
        let shared = Shared {
            seed: &seed,
            starts,
            next: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            state: Mutex::new(queue),
            filled: Condvar::new(),
            room: Condvar::new(),
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
                    while let Some(run) = shared.take() {
                        let done = worker.read_run(shared, run, new_reader);
                        shared.put(run, done);
                    }
                });
            }
            let _halt = HaltOnPanic(shared);
            let result = merge(host, module, shared, &mut main, new_reader, threads, each);
            shared.halt();
            result
        })
    }
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

/// The next run in order, from the queue; the calling thread reads runs
/// itself while it is not ready. `None` if a thread panicked.
fn next_run<R: BodySource>(
    shared: &Shared<'_>,
    run: usize,
    own: &mut Worker<R>,
    new_reader: &(dyn Fn() -> R + Sync),
) -> Option<Vec<Done>> {
    loop {
        let mut queue = shared.lock();
        if let Some(done) = queue.runs.front_mut().and_then(Option::take) {
            queue.runs.pop_front();
            queue.delivered = run + 1;
            let wake = queue.readers_wait > 0;
            drop(queue);
            if wake {
                shared.room.notify_all();
            }
            return Some(done);
        }
        drop(queue);
        // Not ready: read a run meanwhile, or wait for one.
        let next = shared.next.fetch_add(1, Ordering::AcqRel);
        if next < shared.runs() {
            let done = own.read_run(shared, next, new_reader);
            shared.put(next, done);
            continue;
        }
        let mut waiting = shared.lock();
        while waiting.runs.front().is_none_or(Option::is_none) {
            if shared.stop.load(Ordering::Acquire) {
                // A thread panicked: the scope passes its panic on.
                return None;
            }
            waiting.merger_waits = true;
            waiting = shared.filled.wait(waiting).unwrap_or_else(PoisonError::into_inner);
            waiting.merger_waits = false;
        }
    }
}

/// The calling thread: adopts and delivers the bodies in order, reading
/// runs itself while the next one is not ready.
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
    for run in 0..shared.runs() {
        let Some(dones) = next_run(shared, run, &mut own, new_reader) else { return Ok(stats) };
        for (index, done) in (shared.starts[run]..).zip(dones) {
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
                    // Read on the module itself, in its turn. If that
                    // changed what the forks share, they read a module
                    // that is no longer this one: the rest is read here,
                    // in order.
                    stats.read_again += 1;
                    let before = module(host).stamp();
                    let go_on = match deliver_read(host, module, index, main, each) {
                        Ok(go_on) => go_on,
                        Err(error) => {
                            shared.halt();
                            return Err(error);
                        }
                    };
                    let count = *shared.starts.last().expect("a start");
                    if go_on && module(host).stamp() != before {
                        shared.halt();
                        stats.sequential_from = Some(index + 1);
                        sequential(host, module, index + 1, count, main, each)?;
                        return Ok(stats);
                    }
                    go_on
                }
            };
            if !go_on {
                return Ok(stats);
            }
        }
    }
    Ok(stats)
}
