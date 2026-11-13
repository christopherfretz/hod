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
use std::time::{Duration, Instant};

use hod_ir::{Body, IrErrorKind, Module, Support, SymId};

use crate::back::{Threads, generate_with};
use crate::{CompileError, Construct, Options};

#[cfg(feature = "readers")]
mod read;
#[cfg(feature = "readers")]
pub use read::{compile_releasing, compile_with};

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
        CompileError::Panic(_) => INTERNAL_PANIC,
    }
}

fn named(name: impl Into<Cow<'static, str>>) -> Construct {
    Construct::Named(name.into())
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

/// The reason name of [`CompileError::Panic`] (`spec/cli.md` "Reasons",
/// "Embedding"): a panic of hod's, contained.
pub const INTERNAL_PANIC: &str = "internal-panic";

/// The reason for a limit of a reader (`hod_ll`'s or `hod_bc`'s, or a
/// table of `hod_ir` they fill): LLVM may take what hod does not.
pub const LIMIT_READER: &str = "limit:reader";

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
