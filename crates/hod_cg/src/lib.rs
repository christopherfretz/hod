//! Target-independent code generation: walks, frame layout, value locations, scheduling.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/codegen.md`
//! for its contract.
//!
//! | module | spec | contents |
//! |---|---|---|
//! | `reg` | §2.1 | `PReg`, `RegSet` |
//! | `abi` | §7.2 | `Sig`, `Part`, `Assignment`: the argument-assignment interface |

pub mod abi;
mod reg;

pub use reg::{PReg, RegSet};
