//! The coroutine plan (`spec/xform.md`; design 0005 §3.4): which stack
//! slots and SSA values of an unsplit switch-resumed coroutine live in its
//! heap frame, how its suspend points are numbered, which code runs while
//! the coroutine may already run elsewhere, and what is refused by name.
//!
//! Pure analysis: the body is not rewritten and no code is generated.
//! The cost is linear in the instructions plus blocks × tracked objects
//! (one bit each), times the loop depth of the dataflow.

use std::fmt;

use hod_ir::{Body, InstId, Module, SlotId};

use crate::bitset::BitSet;

mod analysis;

/// `coro:begin-shape`: not exactly one `coro.id` and one `coro.begin`, a
/// promise operand that is not a stack slot, or a `coro.begin` that can
/// run twice.
pub const BEGIN_SHAPE: &str = "coro:begin-shape";
/// `coro:suspend-shape`: a `coro.suspend` whose token is not a
/// `coro.save`, whose result is not used by exactly one `switch` with the
/// cases 0 and 1 right after it, a second final suspend, or an
/// `await_suspend` wrapper (or, in the older dialect, a `coro.save`) from
/// which no single path leads to its suspend.
pub const SUSPEND_SHAPE: &str = "coro:suspend-shape";
/// `coro:frame-intrinsic`: `llvm.coro.frame` (none in the corpus).
pub const FRAME_INTRINSIC: &str = "coro:frame-intrinsic";
/// `coro:dynamic-alloca`: a dynamic `alloca`, `stacksave` or
/// `stackrestore` in a coroutine body.
pub const DYNAMIC_ALLOCA: &str = "coro:dynamic-alloca";
/// `coro:variadic`: a variadic coroutine.
pub const VARIADIC: &str = "coro:variadic";
/// `coro:unwind-end-without-final`: an unwinding `coro.end` in a
/// coroutine without a final suspend.
pub const UNWIND_END_WITHOUT_FINAL: &str = "coro:unwind-end-without-final";
/// `coro:outside-frame-across-suspend`: a slot marked
/// `!coro.outside.frame` used after a resumption edge.
pub const OUTSIDE_FRAME_ACROSS_SUSPEND: &str = "coro:outside-frame-across-suspend";
/// `coro:argument-across-suspend`: an argument used after a resumption
/// edge.
pub const ARGUMENT_ACROSS_SUSPEND: &str = "coro:argument-across-suspend";
/// `coro:frame-before-begin`: an address derived from a heap slot before
/// `coro.begin` and used after it.
pub const FRAME_BEFORE_BEGIN: &str = "coro:frame-before-begin";
/// `coro:frame-access-after-suspend`: a heap slot or heap value used in
/// the suspended region or after the frame is freed (the safety
/// conditions).
pub const FRAME_ACCESS_AFTER_SUSPEND: &str = "coro:frame-access-after-suspend";
/// `coro:overaligned-frame`: a frame resident aligned above 16 in a
/// coroutine that does not call `llvm.coro.align`.
pub const OVERALIGNED_FRAME: &str = "coro:overaligned-frame";

/// Every refusal name. A body with several refused constructs gets one of
/// them: which one is not specified, except that
/// [`FRAME_ACCESS_AFTER_SUSPEND`] and then [`OVERALIGNED_FRAME`] are given
/// only to a body with no other (`spec/xform.md` §3.8).
pub const REFUSALS: [&str; 11] = [
    VARIADIC,
    BEGIN_SHAPE,
    FRAME_INTRINSIC,
    DYNAMIC_ALLOCA,
    SUSPEND_SHAPE,
    UNWIND_END_WITHOUT_FINAL,
    OUTSIDE_FRAME_ACROSS_SUSPEND,
    ARGUMENT_ACROSS_SUSPEND,
    FRAME_BEFORE_BEGIN,
    FRAME_ACCESS_AFTER_SUSPEND,
    OVERALIGNED_FRAME,
];

/// Why a coroutine body has no plan: one of the `coro:*` names above,
/// and where it was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// The stable name (one of [`REFUSALS`]).
    pub construct: &'static str,
    /// The instruction it was found at, if one.
    pub inst: Option<InstId>,
    /// The slot concerned, if one.
    pub slot: Option<SlotId>,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported construct {}", self.construct)?;
        if let Some(inst) = self.inst {
            write!(f, " at instruction {}", inst.index())?;
        }
        if let Some(slot) = self.slot {
            write!(f, " (slot {})", slot.index())?;
        }
        Ok(())
    }
}

/// One suspend point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Suspend {
    /// Its `coro.save`.
    pub save: InstId,
    /// The `coro.await.suspend.{void,bool,handle}` call that leads to it;
    /// `None` in the older dialect, where the suspended region starts at
    /// `save`.
    pub wrapper: Option<InstId>,
    /// The `coro.suspend`.
    pub suspend: InstId,
    /// Its index: instruction order, the final suspend last.
    pub index: u32,
    /// `coro.suspend(…, i1 true)`.
    pub is_final: bool,
}

/// What the code generator needs to compile one coroutine body
/// (`spec/xform.md` §3).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoroPlan {
    /// The `coro.begin`.
    pub begin: InstId,
    /// The promise slot (`coro.id`'s second operand), a header field at
    /// `promise_offset`; never in `heap_slots`.
    pub promise: Option<SlotId>,
    /// `alignTo(16, align(promise))`; 16 without a promise.
    pub promise_offset: u32,
    /// The frame's alignment: 16, or more for a frame resident aligned
    /// above it (then the body calls `coro.align`).
    pub frame_align: u32,
    /// The slots that live in the frame.
    pub heap_slots: BitSet<SlotId>,
    /// The instruction results whose home is in the frame: live-in on a
    /// resumption edge. (No argument: one used after a resumption edge is
    /// refused.)
    pub heap_values: BitSet<InstId>,
    /// Heap slots with a use before `coro.begin`: those uses address a
    /// machine-stack shadow, copied into the heap slot just after
    /// `coro.begin`. Increasing order.
    pub begin_copies: Vec<SlotId>,
    /// Heap values defined before `coro.begin` (`this`, a closure
    /// pointer, a VLA bound): stored to a machine-stack home where
    /// defined, and copied to their heap home just after `coro.begin`.
    /// Increasing order.
    pub begin_value_copies: Vec<InstId>,
    /// The suspend points, in index order.
    pub suspends: Vec<Suspend>,
    /// The blocks of the suspended region (design 0005 §3.3): every block
    /// reachable from a suspend's −1 edge, and the blocks after a
    /// wrapper's (or older-dialect save's) block on its path to its
    /// suspend. The instructions after `Suspend::wrapper` in the
    /// wrapper's own block belong to it too.
    pub suspended_region: BitSet<hod_ir::BlockId>,
    /// Whether some `coro.end` unwinds.
    pub has_unwind_end: bool,
    /// The final suspend's index, if there is one.
    pub final_index: Option<u32>,
}

/// How rules 1 to 5 class a slot (`spec/xform.md` §3.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SlotClass {
    /// Rule 1: the promise.
    Promise,
    /// Rule 2: `!coro.outside.frame`.
    OutsideFrame,
    /// Rule 3: unescaped.
    Unescaped,
    /// Rule 4: escaped, with lifetime markers.
    EscapedWithMarkers,
    /// Rule 5: escaped, without lifetime markers.
    EscapedWithoutMarkers,
}

/// Where a frame access may not be (the safety conditions).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Region {
    /// After an `await_suspend` wrapper returned, or on a −1 path.
    Suspended,
    /// After the frame was freed.
    AfterFree,
}

/// A heap slot or heap value used where the safety conditions forbid it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Violation {
    /// The slot, for a heap slot.
    pub slot: Option<SlotId>,
    /// The value, for a heap value.
    pub value: Option<InstId>,
    /// The instruction that uses it (for a use on a `phi` edge, the
    /// predecessor's terminator).
    pub at: InstId,
    /// Which region.
    pub region: Region,
}

/// The plan with what it was derived from, for tests and censuses: the
/// class of every slot and every violation of the safety conditions
/// (which [`plan`] refuses as [`FRAME_ACCESS_AFTER_SUSPEND`]).
#[derive(Clone, Debug)]
pub struct Analysis {
    /// The plan, as [`plan`] would return it if there were no violation
    /// and no over-aligned frame.
    pub plan: CoroPlan,
    /// The class of each slot, indexed by [`SlotId`].
    pub classes: Vec<SlotClass>,
    /// The violations, in instruction order per kind.
    pub violations: Vec<Violation>,
    /// Whether the body calls `llvm.coro.align`.
    pub calls_align: bool,
}

/// The plan of `body` (`spec/xform.md` §3): `None` for a function without
/// `presplitcoroutine`.
///
/// # Errors
/// A [`Refusal`] named by one of [`REFUSALS`].
pub fn plan(module: &Module, body: &Body) -> Result<Option<CoroPlan>, Refusal> {
    let Some(analysis) = analyze(module, body)? else { return Ok(None) };
    if let Some(violation) = analysis.violations.first() {
        return Err(Refusal {
            construct: FRAME_ACCESS_AFTER_SUSPEND,
            inst: Some(violation.at),
            slot: violation.slot,
        });
    }
    if analysis.plan.frame_align > 16 && !analysis.calls_align {
        return Err(Refusal { construct: OVERALIGNED_FRAME, inst: None, slot: None });
    }
    Ok(Some(analysis.plan))
}

/// [`plan`] without its last two refusals: the safety conditions are
/// reported as [`Violation`]s and the frame alignment is not checked.
///
/// # Errors
/// A [`Refusal`] named by one of [`REFUSALS`] before
/// [`FRAME_ACCESS_AFTER_SUSPEND`].
pub fn analyze(module: &Module, body: &Body) -> Result<Option<Analysis>, Refusal> {
    analysis::analyze(module, body)
}
