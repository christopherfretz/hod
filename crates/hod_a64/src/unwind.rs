//! The frame as an unwinder sees it (`spec/codegen.md` §5.6.38,
//! WP-215): `llvm.sponentry`, `llvm.addressofreturnaddress` and
//! `llvm.eh.unwind.init`.
//!
//! - `llvm.sponentry` is the stack pointer at entry, which is the CFA
//!   (§5.5): the function keeps an incoming base (`frame::begin`), and the
//!   result is that base, as `llc`'s `x29 + 16` is in its frame.
//! - `llvm.addressofreturnaddress` is `x29 + 8`, the saved `lr` of the
//!   frame record, as `llc` 19 and 22 compute it.
//! - `llvm.eh.unwind.init` makes the function save `x19`..`x28` and
//!   `d8`..`d15` (every callee-saved register: hod's pool has none) right
//!   after `mov x29, sp`, with their rows, and reload them in every exit
//!   (`[x29 + 16]`, the bytes after the frame record):
//!
//! ```text
//! stp x19, x20, [x29, #16] .. stp x27, x28, [x29, #80]
//! stp d8, d9, [x29, #96]   .. stp d14, d15, [x29, #144]
//! ```
//!
//! `llvm.eh.dwarf.cfa` and `llvm.eh.return.i64` are refused by
//! `classify` on this target (`llc` miscompiles both).

use hod_cg::{CfiOp, CgResult, FnCx, FrameRef};
use hod_ir::{Inst, Intrinsic, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Reg, Size, VPairSize, VReg};

use crate::common::{FP, deferred_first, frame_address, put, unsupported, xr};
use crate::target::A64;

/// `llvm.eh.unwind.init` in a `preserve_mostcc` or `preserve_allcc`
/// function or a coroutine body: their own save areas take the bytes
/// after the frame record, or `x19`.
pub const UNWIND_INIT_CONVENTION: &str = "a64:unwind-init-convention";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[UNWIND_INIT_CONVENTION];

/// The save area of `llvm.eh.unwind.init`: right after the frame record.
const AREA: u32 = 16;
/// Its size: ten general registers and eight `d` registers.
const AREA_SIZE: u32 = 8 * (10 + 8);
/// The bytes of its stores: nine `stp`.
const STORE_BYTES: u32 = 4 * 9;
/// The first general register saved and the first `d` register.
const FIRST_X: u8 = 19;
const FIRST_D: u8 = 8;
/// The DWARF number of `v0` (`d0`).
const DWARF_V0: u16 = 64;

/// What the frame intrinsics of the body ask of the prologue.
#[derive(Clone, Copy, Default, Debug)]
pub struct UnwindState {
    /// The body calls `llvm.eh.unwind.init`: the callee-saved registers
    /// are in the area at [`AREA`].
    pub save: bool,
    /// The code offset right after their stores.
    rows_at: u32,
}

/// Whether the body calls `llvm.sponentry` (it then keeps an incoming
/// base, §5.5).
pub fn needs_entry_sp(cx: &FnCx<'_, A64>) -> bool {
    cx.body.calls().any(|site| site.intrinsic() == Some(Intrinsic::SpOnEntry))
}

/// The pairs of the area: each register pair's offset from `x29`, the
/// general ones first.
fn pairs() -> impl Iterator<Item = (bool, u8, i64)> {
    let general = (0..5u8).map(|k| (true, FIRST_X + 2 * k, i64::from(AREA) + 16 * i64::from(k)));
    let float =
        (0..4u8).map(|k| (false, FIRST_D + 2 * k, i64::from(AREA) + 80 + 16 * i64::from(k)));
    general.chain(float)
}

fn x(n: u8) -> Reg {
    Reg::new(n).expect("x19..x28")
}

fn d(n: u8) -> VReg {
    VReg::new(n).expect("d8..d15")
}

/// From `frame::begin`, right after `mov x29, sp`: a body that calls
/// `llvm.eh.unwind.init` stores the callee-saved registers in the area.
///
/// # Errors
/// [`UNWIND_INIT_CONVENTION`]; what `alloc_slot` reports.
pub fn save(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    cx.state.frame.unwind = UnwindState::default();
    if !cx.body.calls().any(|site| site.intrinsic() == Some(Intrinsic::EhUnwindInit)) {
        return Ok(());
    }
    let preserving = crate::tail2::is_preserve_most(cx) || crate::cconv::kept(cx).is_some();
    if preserving || cx.has_coro_frame() {
        return unsupported(UNWIND_INIT_CONVENTION);
    }
    if cx.alloc_slot(u64::from(AREA_SIZE), 16)? != FrameRef::Local(AREA) {
        return cx.bug("the unwind.init save area is not after the frame record");
    }
    for (general, n, at) in pairs() {
        if general {
            put(cx, mc::stp_off(Size::X, x(n).zr(), x(n + 1).zr(), FP, at));
        } else {
            put(cx, mc::stp_fp_off(VPairSize::D, d(n), d(n + 1), FP, at));
        }
    }
    cx.state.frame.unwind = UnwindState { save: true, rows_at: cx.offset() };
    Ok(())
}

/// The rules saying where [`save`] put each register, given the CFA is
/// `x29 + frame`: from after the stores (`Some`, from `end`), or from the
/// current offset (`None`, the body rows a later exit restates).
///
/// # Errors
/// What `cfi_at` reports.
pub fn rows(cx: &mut FnCx<'_, A64>, from_prologue: bool, frame: u32) -> CgResult<()> {
    let state = cx.state.frame.unwind;
    if !state.save {
        return Ok(());
    }
    debug_assert_eq!(STORE_BYTES, 4 * pairs().count() as u32, "one stp per pair");
    for (general, n, at) in pairs() {
        for (k, saved) in [(0u8, at), (1, at + 8)] {
            let reg = if general { u16::from(n + k) } else { DWARF_V0 + u16::from(n + k) };
            let op = CfiOp::Offset { reg, cfa_offset: saved as i32 - frame as i32 };
            if from_prologue {
                cx.cfi_at(state.rows_at, op)?;
            } else {
                cx.cfi(op);
            }
        }
    }
    Ok(())
}

/// From `frame::teardown`, before the frame record is reloaded: the
/// registers [`save`] stored, loaded back, each rule back to "same
/// value".
pub fn restore(cx: &mut FnCx<'_, A64>) {
    if !cx.state.frame.unwind.save {
        return;
    }
    for (general, n, at) in pairs() {
        if general {
            put(cx, mc::ldp_off(Size::X, x(n).zr(), x(n + 1).zr(), FP, at));
        } else {
            put(cx, mc::ldp_fp_off(VPairSize::D, d(n), d(n + 1), FP, at));
        }
    }
    for (general, n, _) in pairs() {
        for k in 0..2 {
            let reg = if general { u16::from(n + k) } else { DWARF_V0 + u16::from(n + k) };
            cx.cfi(CfiOp::Restore { reg });
        }
    }
}

/// A call of `which`, one of this module's intrinsics, with `args`.
///
/// # Errors
/// What the register policy reports.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &[Value],
) -> CgResult<()> {
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (I::SpOnEntry, []) => {
            let rd = cx.def_reg(inst)?;
            frame_address(cx, xr(rd), FrameRef::Incoming(0));
            cx.finish(inst, rd)
        }
        (I::AddressOfReturnAddress, []) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::add_imm(Size::X, xr(rd).sp(), FP, 8));
            cx.finish(inst, rd)
        }
        // The registers are saved by the prologue (`save`).
        (I::EhUnwindInit, []) => Ok(()),
        _ => cx.bug("a frame intrinsic this target does not lower"),
    }
}
