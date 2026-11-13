//! Branch protection (`-mbranch-protection`, `spec/codegen.md` §5.6.26):
//! BTI landing pads and return-address signing.
//!
//! **Owner: WP-185.** Tests: `tests/protect.rs`. Hooked from `frame`
//! (the entry, the entry's CFI rows, every exit sequence, the body's
//! rules restated before a tail exit), `coro_body::stubs` (each stub's
//! entry), `intrinsic::frame_walk` (a loaded return address is
//! stripped), and `Target::branch_target` / `Target::emit_call` (`bti j`).
//!
//! A function's policy is its string attributes, as `llc` 19 and 22 read
//! them once `hod_ll` has upgraded an older module (`spec/ll.md` §7.3):
//!
//! - `"branch-target-enforcement"` (present): its entry is a landing
//!   pad, `bti c` unless it starts with `paciasp`/`pacibsp`, which are
//!   landing pads for calls too; so are the entries of its coroutine
//!   stubs; and every place an indirect branch may land gets `bti j`: a
//!   block whose address is taken, a landing pad, the return of a
//!   `returns_twice` call.
//! - `"sign-return-address"` `"non-leaf"` or `"all"`: every hod function
//!   saves `x30` in its frame record, so both sign: `paciasp` (`pacibsp`
//!   with `"sign-return-address-key"="b_key"`) first, with the CFI row
//!   `DW_CFA_AARCH64_negate_ra_state` after it, and `autiasp`
//!   (`autibsp`) at the end of each exit sequence, when the stack pointer
//!   is the entry's again, with another such row, before the `ret` or
//!   the tail branch.
//!
//! ```text
//! F:   paciasp            | bti c       (or nothing)
//!      [mov x16, sp] · sub · sub · stp x29, x30 · mov x29, sp ...
//!      ...
//!      ldp x29, x30 · add sp · autiasp · ret
//! ```

use hod_cg::{CallTarget, CfiOp, CgResult, FnCx, SymRef};
use hod_ir::{FnAttrs, FnStr, SymbolKind, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Reg, Size};

use crate::common::{put, unsupported};
use crate::target::A64;

/// A `"sign-return-address"` value other than `none`, `non-leaf` and
/// `all`, or a `"sign-return-address-key"` other than `a_key` and
/// `b_key`: `llc` asserts on both.
pub const PROTECT_VALUE: &str = "a64:branch-protection-value";

/// `xpaclri`: strips the authentication code from `x30` (a hint).
const XPACLRI: u32 = 0xd503_20ff;

/// The function's branch protection, decided at its entry.
#[derive(Clone, Copy, Default, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "independent facts of one function")]
pub struct ProtectState {
    /// `bti c` at entries and `bti j` at indirect-branch targets.
    pub bti: bool,
    /// The return address is signed.
    pub sign: bool,
    /// With the B key.
    pub b_key: bool,
    /// The return address is signed at the current offset, as the CFI
    /// rows emitted so far say: the parity of their negations.
    signed_rows: bool,
}

/// The policy of the function being compiled.
fn policy(cx: &FnCx<'_, A64>) -> CgResult<ProtectState> {
    let mut state = ProtectState::default();
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return Ok(state);
    };
    let attrs = cx.module.fn_attrs(attrs);
    state.bti = attrs.string(FnStr::BranchTargetEnforcement).is_some();
    state.sign = match attrs.string(FnStr::SignReturnAddress) {
        None | Some(b"none") => false,
        Some(b"non-leaf" | b"all") => true,
        Some(_) => return unsupported(PROTECT_VALUE),
    };
    state.b_key = match attrs.string(FnStr::SignReturnAddressKey) {
        None | Some(b"a_key") => false,
        Some(b"b_key") => true,
        Some(_) => return unsupported(PROTECT_VALUE),
    };
    Ok(state)
}

/// Adds a negation of the signing state at the current offset.
fn negate(cx: &mut FnCx<'_, A64>) {
    cx.cfi(CfiOp::NegateRaState);
    let state = &mut cx.state.frame.protect;
    state.signed_rows = !state.signed_rows;
}

/// The signing instruction, or the call landing pad.
fn entry_word(state: ProtectState) -> Option<u32> {
    match (state.sign, state.b_key, state.bti) {
        (true, false, _) => Some(mc::paciasp()),
        (true, true, _) => Some(mc::pacibsp()),
        (false, _, true) => Some(mc::bti_c()),
        (false, _, false) => None,
    }
}

/// From `frame::begin`, before anything else: decides the policy and
/// emits the entry's `paciasp`/`pacibsp` or `bti c`. Its CFI rows follow
/// in [`entry_rows`].
///
/// # Errors
/// [`PROTECT_VALUE`].
pub fn entry(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let state = policy(cx)?;
    cx.state.frame.protect = state;
    if let Some(word) = entry_word(state) {
        put(cx, word);
    }
    Ok(())
}

/// From `frame::end`, before the prologue's rows: for a signing entry
/// (at offset 0), the B-key mark at 0 and the negation after it.
///
/// # Errors
/// What [`FnCx::cfi_at`] reports.
pub fn entry_rows(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let state = cx.state.frame.protect;
    if !state.sign {
        return Ok(());
    }
    if state.b_key {
        cx.cfi_at(0, CfiOp::BKeyFrame)?;
    }
    cx.cfi_at(4, CfiOp::NegateRaState)?;
    cx.state.frame.protect.signed_rows = true;
    Ok(())
}

/// From `frame::teardown`, last: the stack pointer is the entry's again;
/// `autiasp`/`autibsp` and its row.
pub fn exit(cx: &mut FnCx<'_, A64>) {
    let state = cx.state.frame.protect;
    if state.sign {
        put(cx, if state.b_key { mc::autibsp() } else { mc::autiasp() });
        negate(cx);
    }
}

/// From `frame::body_rows`: the body's signing state stated again after
/// an exit sequence.
pub fn restate(cx: &mut FnCx<'_, A64>) {
    let state = cx.state.frame.protect;
    if state.sign && !state.signed_rows {
        negate(cx);
    }
}

/// From `coro_body::stubs`, at each stub's entry after its entry-state
/// rows: the stub is called through a pointer and builds the function's
/// frame, so it starts as the function does; the signing state is first
/// brought back to the entry's (unsigned).
pub fn stub_entry(cx: &mut FnCx<'_, A64>) {
    let state = cx.state.frame.protect;
    if state.sign && state.signed_rows {
        negate(cx);
    }
    if let Some(word) = entry_word(state) {
        put(cx, word);
    }
    if state.sign {
        negate(cx);
    }
}

/// `Target::branch_target` (`hod_cg`'s hook, WP-184): `bti j` at an
/// address-taken block, a landing pad and an `invoke`'s pad stub (where
/// the unwinder lands), under BTI.
///
/// # Errors
/// None today.
pub fn branch_target(cx: &mut FnCx<'_, A64>, at: hod_cg::BranchTarget) -> CgResult<()> {
    let _ = at;
    if cx.state.frame.protect.bti {
        put(cx, mc::bti_j());
    }
    Ok(())
}

/// `Target::emit_call`, after the call instruction and its return
/// offset are recorded: `bti j` after a call of a `returns_twice`
/// function (`longjmp` returns there with an indirect branch), under BTI.
pub fn after_call(cx: &mut FnCx<'_, A64>, target: CallTarget) {
    if !cx.state.frame.protect.bti {
        return;
    }
    let Some(site) = cx.inst().call() else { return };
    let callee = match site.callee_value().kind() {
        ValueKind::Sym(sym) => Some(sym),
        _ => None,
    };
    // Only the call of the instruction itself, not a runtime call its
    // lowering made first.
    match (target, callee) {
        (CallTarget::Sym(SymRef::Module(sym)), Some(callee)) if sym == callee => {}
        (CallTarget::Reg(_), None) => {}
        _ => return,
    }
    let declared = callee.is_some_and(|sym| match cx.module.symbol(sym).kind() {
        SymbolKind::Function { attrs, .. } => cx.module.fn_attrs(attrs).has(FnAttrs::RETURNS_TWICE),
        _ => false,
    });
    if site.returns_twice() || declared {
        put(cx, mc::bti_j());
    }
}

/// From `intrinsic::frame_walk`: a return address loaded from a frame
/// record into `r` is stripped of its authentication code, whoever saved
/// it, as `llc` strips it in every function: `mov x30, r` · `xpaclri` ·
/// `mov r, x30` (`xpaclri` is a hint, so the sequence runs everywhere).
pub fn strip_return_address(cx: &mut FnCx<'_, A64>, r: Reg) {
    put(cx, mc::mov(Size::X, Reg::LR.zr(), r.zr()));
    put(cx, XPACLRI);
    put(cx, mc::mov(Size::X, r.zr(), Reg::LR.zr()));
}
