//! AArch64 code generation target.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/` for its contract.
//!
//! | module | spec | contents |
//! |---|---|---|
//! | `reg` | `abi-a64.md` §2 | the registers as `hod_cg::PReg` indexes |
//! | `abi` | `abi-a64.md` | `assign`: where arguments and return values travel |

pub mod abi;
pub mod reg;
