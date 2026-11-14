//! Target-independent code generation: the walk, frame layout, value
//! locations, calls, and the per-function output record.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/codegen.md`
//! for its contract. A target implements [`Target`]; [`compile_function`]
//! walks one function once and appends its code and records to a
//! [`Chunk`]. Everything generic is monomorphized over the target: there
//! is no trait object on the per-instruction path, and no machine IR.
//!
//! | module | spec | contents |
//! |---|---|---|
//! | `reg` | §2.1 | `PReg`, `RegSet`, `RegInfo` |
//! | `loc` | §2.2, §3 | `Class`, `Loc`, the location table's entry |
//! | `policy` | §4.1 to §4.4, §4.6 | the value-location policy v1: operands, results, registers, deferral |
//! | `phi` | §4.5 | the `phi` protocol: incoming slots |
//! | `frame` | §5.1 to §5.3 | `Frame`, `FrameRef`, `SlotPlace` |
//! | `label` | §6.2, §6.3 | `Label`, `Labels`, fixups |
//! | `shrink` | §6.4 | cuts and branch relaxation once the code is complete |
//! | `call` | §4.4, §7.1 to §7.3 | the call sequence, `musttail`, `invoke`, runtime calls |
//! | `abi` | §7.2 | `Sig`, `Part`, `Assignment`: the argument-assignment interface; the leaf walk (`Classify`, `place_leaves`) |
//! | `features` | §7.3 | `FeatureTable`: the reading of a `target-features` string |
//! | `memplan` | §7.3 | `mem_plan`: what a `memcpy`, `memmove` or `memset` becomes |
//! | `switch` | §7.3 | `canonical_cases`, `search_step`: the `switch` plan |
//! | `sym` | §7.3, §9.2 | `SymRef`, `RuntimeFn`; the routines by type and target (`fp_routine`, ...) |
//! | `lanes` | §7.3 | the lane loop: `LaneKernels`, `setup`, `each_lane`, `finish`; `Elem`, `lane_log2` |
//! | `asm_constraints` | §7.3 | `parse`: an inline-assembly constraint string, `ConstraintSyntax` the target's part |
//! | `target` | §8.2 | the `Target` trait |
//! | `cx` | §8.1, §11 | `FnCx`: what a lowering is given; `Scratch` |
//! | `record` | §9.1 to §9.6 | `Chunk`, `FuncRecord` and the rows |
//! | `error` | §10 | `CgError`, the named limits |
//! | `options` | §7.4, §9.7, §12 | `Options`: relocation and code model, sections, the stress options |
//! | `access` | §7.4 | `sym_access`, `call_access` |
//! | `emit` | §9.7 | `ModuleEmitter`, `emit_module`: the module's object |
//! | `driver` | §1, §4.7, §4.8, §6.1, §6.3 | `compile_function`: the walk, entry code, reachability, the far-mode retry |
//! | `coro` | "Coroutines" | the noop coroutine frame a function refers to (WP-105) |
//! | `coro_body` | §7.6 | coroutine bodies: the heap frame, the `coro.begin` copies, the suspend switch, the suspended-region check (WP-106) |
//! | `coro_debug` | §7.6 | the variable locations of a coroutine body (WP-108) |
//!
//! Not here: any real target. The test target that exercises this crate
//! is in `tests/common/`.

pub mod abi;
mod access;
pub mod asm_constraints;
mod call;
mod coro;
mod coro_body;
mod coro_debug;
mod cx;
mod dbg_value;
mod debug_calls;
mod driver;
pub mod emit;
pub mod error;
pub mod features;
mod frame;
mod guard;
mod idhash;
mod key_rows;
mod label;
pub mod lanes;
mod line_rows;
mod loc;
pub mod memplan;
mod options;
mod phi;
mod policy;
mod record;
mod reg;
mod shrink;
pub mod switch;
pub mod sym;
mod target;

pub use access::{Access, CallAccess, TlsAccess, call_access, prefers_local_alias, sym_access};
pub use call::CallArg;
pub use coro::NOOP_CORO_TARGET;
pub use coro_body::{
    BEGIN_SHARED_BLOCK, CORO_TARGET, CoroLayout, CoroSlots, OUTSIDE_COROUTINE, OVERALIGNED_STACK,
};
pub use cx::{FnCx, Scratch, TailExit};
pub use dbg_value::{DEBUG_DBG_VALUE_ARGLIST, DEBUG_DBG_VALUE_COROUTINE, ValueAt, ValueRange};
pub use debug_calls::{DebugCallRow, DebugCallee};
pub use driver::{MAX_FUNCTION_SIZE, compile_function};
pub use emit::{COPY_SUFFIX, Emitted, ModuleEmitter, PlacedFunc, emit_module};
pub use error::{CgError, CgResult, LIMIT_FRAME_SIZE, LIMIT_FUNCTION_SIZE};
pub use frame::{Frame, FrameRef, MAX_FRAME, SlotPlace};
pub use guard::{GUARD_BYTES, GuardPlan, SSP_COROUTINE, SSP_TARGET};
pub use key_rows::{DEBUG_KEY_INSTRUCTIONS_COROUTINE, DEBUG_KEY_INSTRUCTIONS_MIXED};
pub use label::{FixupKind, Label, Labels, OutOfRange};
pub use loc::{Class, Loc};
pub use options::{LIMIT_COMPRESSED_SECTION, Options, RelocModel, Stress};
pub use policy::MAX_DEFERRED_RUN;
pub use record::{
    BlockOffset, CallSiteRow, CfiOp, CfiRow, Chunk, ClauseRow, FrameDesc, FuncRecord, IslandRow,
    LineRow, NO_PAD, PadRow, Reloc, Stmt, VarPlace, VarRow,
};
pub use reg::{MIN_POOL_FPR, MIN_POOL_GPR, PReg, RegInfo, RegSet};
pub use sym::{RuntimeFn, SymRef};
pub use target::{BranchTarget, CallEmit, CallTarget, Target};
