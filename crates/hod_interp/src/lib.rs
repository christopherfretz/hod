//! A reference interpreter for `hod_ir` function bodies, and a generator
//! of executable random programs (`spec/interp.md`).
//!
//! Test infrastructure: the oracle the code generator's tests and the
//! fuzzers compare compiled code against. It is never shipped.
//!
//! The interpreter gives a program a result only when every correct
//! compilation of it gives that same result; anything else is a [`Trap`]
//! with a name, and anything not implemented is [`Stop::Unsupported`]
//! with a stable reason.
//!
//! ```
//! use hod_interp::{Interp, Program, Val, generate};
//!
//! let generated = generate::generate(7, &generate::Options::default());
//! let program = generated.program();
//! let mut interp = Interp::new(&program);
//! hod_interp::host::register(&mut interp);
//! let outcome = interp.run(generated.entry, &generated.args);
//! assert!(matches!(outcome.result, Ok(Some(Val::Int { bits: 64, .. }))));
//! ```

pub mod eh;
mod error;
pub mod generate;
pub mod host;
mod interp;
mod memory;
pub mod ops;
mod value;

pub use error::{Stop, Trap};
pub use interp::{Config, Event, HostCx, Interp, NanPolicy, Outcome, Program};
pub use memory::Memory;
pub use value::{Agg, MAX_INT_BITS, Val};
