//! Stack probing (`spec/codegen.md` §5.6.27, WP-186): a function with
//! `"probe-stack"="inline-asm"` touches the stack it allocates page by
//! page, from high to low addresses, so that no guard page below the
//! stack is jumped over.
//!
//! The protocol is LLVM's: a caller leaves at most 1024 bytes below its
//! lowest touch unprobed when it calls (`StackProbeMaxUnprobedStack`), and
//! a callee touches no further than the probe size `P` below its last
//! touch. hod's frame record is at the bottom of the frame (§5.5), so its
//! `stp` is the touch of the bottom page, and the top of the frame is
//! touched first:
//!
//! ```text
//! FRAME + 1024 <= P     sub sp, sp, #lo12 · sub sp, sp, #hi12        (as unprobed)
//! otherwise             str xzr, [sp, #-16]!                          cfa sp + 16
//!   k <= 4              (sub sp, sp, #P · str xzr, [sp]) k times      cfa sp + 16 + iP
//!   [sub sp, sp, #r lo12 · #r hi12]                                   cfa sp + FRAME
//! k > 4                 stp x29, x30, [sp, #-16]!                     cfa sp + 16, x29/x30 at -16/-8
//!                       mov x29, sp                                   cfa x29 + 16
//!                       sub x17, sp, #kP
//!                       1: sub sp, sp, #P · str xzr, [sp] · cmp sp, x17 · b.ne 1b
//!                       [sub sp, sp, #r lo12 · #r hi12]
//!                       ldr x29, [x29]                                cfa sp + FRAME
//! then                  stp x29, x30, [sp] · mov x29, sp
//! ```
//!
//! with `k = (FRAME - 16) / P`, `r = (FRAME - 16) % P`, 4 being `llc`'s
//! unroll limit (`StackProbeMaxLoopUnroll`). The loop's bound is in `x17`
//! (`x16` holds the incoming base; neither is preserved by any
//! convention). The loop runs under a temporary frame record at the top of
//! the frame, its `stp` being the top touch, so the CFA is `x29 + 16`
//! while the stack pointer moves: an asynchronous signal unwinds, and so
//! does a debugger that emulates frame 0 rather than reading its rows
//! (lldb). `ldr x29, [x29]` gives the caller's `x29` back (`x30` was never
//! written) before the usual record is saved; the temporary record's
//! rules hold until then, and nothing reads it after, so the local area's
//! top 16 bytes, which it overlaps, need not be reserved.
//!
//! A dynamic `alloca` or over-aligned slot ([`set_sp`]) lowers the stack
//! pointer to `x16` through `llc`'s loop: `1: sub sp, sp, #P · cmp sp, x16
//! · b.ls 2f · str xzr, [sp] · b 1b · 2: mov sp, x16 · ldr xzr, [sp]`.
//! An outgoing argument area of more than 1024 bytes ([`adjust_sp`]) is
//! touched at the bottom of every page.

use hod_cg::{CfiOp, FnCx};
use hod_ir::SymbolKind;
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Load, MemSize, Reg, RegZr, Size};

use crate::common::{FP, SP, X16, X17, put};
use crate::frame::{DWARF_FP, DWARF_LR, DWARF_SP};
use crate::target::A64;

/// LLVM's bound on the stack a caller leaves unprobed at a call.
pub const UNPROBED: u32 = 1024;

/// `llc`'s unroll limit, in probe sizes: more is a loop.
pub const UNROLL: u32 = 4;

/// The bytes the prologue of a probed function reserves beyond an
/// unprobed one's two `sub`s: the longest [`allocation`] (`str`, four
/// unrolled probes, two `sub`s: 44 bytes; the loop is 40) less those 8.
pub const RESERVE: u32 = 4 * (1 + 2 * UNROLL + 2) - 8;

/// The probe size of the function being compiled; 0 when it does not
/// probe (`hod_ir::StackProbe`; `classify` refused the other values).
pub fn size_of(cx: &FnCx<'_, A64>) -> u32 {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return 0;
    };
    cx.module.fn_attrs(attrs).stack_probe().size().unwrap_or(0)
}

/// Whether a `frame`-byte frame is allocated with probes.
pub const fn probes(frame: u32, probe: u32) -> bool {
    probe != 0 && frame + UNPROBED > probe
}

/// The words of a probed allocation of `frame` bytes (when [`probes`]),
/// and its CFI rows, each with the number of words it follows.
pub fn allocation(frame: u32, probe: u32) -> (Vec<u32>, Vec<(usize, CfiOp)>) {
    let mut words = Vec::with_capacity(11);
    let mut rows = Vec::with_capacity(8);
    let (k, r) = ((frame - 16) / probe, (frame - 16) % probe);
    let page = u64::from(probe);
    let (lo, hi) = (r & 0xfff, r & 0xff_f000);
    if k > UNROLL {
        // The temporary frame record: the CFA is `x29 + 16` while the
        // stack pointer moves, a rule an unwinder that emulates frame 0
        // keeps through the loop.
        words.push(mc::stp_pre(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, -16));
        rows.push((1, CfiOp::DefCfaOffset(16)));
        rows.push((1, CfiOp::Offset { reg: DWARF_FP, cfa_offset: -16 }));
        rows.push((1, CfiOp::Offset { reg: DWARF_LR, cfa_offset: -8 }));
        words.push(mc::mov_sp(Size::X, FP, SP));
        rows.push((2, CfiOp::DefCfaRegister(DWARF_FP)));
        words.push(mc::sub_imm(Size::X, X17.sp(), SP, u64::from(k * probe)));
        words.push(mc::sub_imm(Size::X, SP, SP, page));
        words.push(mc::str_off(MemSize::X, RegZr::ZR, SP, 0));
        words.push(mc::cmp_ext(Size::X, SP, X17.zr(), mc::Extend::Uxtx, 0));
        words.push(mc::b_cond(Cond::Ne, -12));
        for part in [lo, hi] {
            if part != 0 {
                words.push(mc::sub_imm(Size::X, SP, SP, u64::from(part)));
            }
        }
        // The caller's `x29` back; `x30` was not written. The record's
        // rules hold until the prologue's `stp` saves the pair again.
        words.push(mc::ldr_off(Load::X, Reg::X29.zr(), FP, 0));
        rows.push((words.len(), CfiOp::DefCfa { reg: DWARF_SP, offset: frame as i32 }));
        return (words, rows);
    }
    words.push(mc::str_pre(MemSize::X, RegZr::ZR, SP, -16));
    rows.push((1, CfiOp::DefCfaOffset(16)));
    for i in 1..=k {
        words.push(mc::sub_imm(Size::X, SP, SP, page));
        rows.push((words.len(), CfiOp::DefCfaOffset((16 + i * probe) as i32)));
        words.push(mc::str_off(MemSize::X, RegZr::ZR, SP, 0));
    }
    if lo != 0 {
        words.push(mc::sub_imm(Size::X, SP, SP, u64::from(lo)));
        rows.push((words.len(), CfiOp::DefCfaOffset((frame - r + lo) as i32)));
    }
    if hi != 0 {
        words.push(mc::sub_imm(Size::X, SP, SP, u64::from(hi)));
        rows.push((words.len(), CfiOp::DefCfaOffset(frame as i32)));
    }
    (words, rows)
}

/// The allocation of `frame` bytes emitted at the current offset with its
/// rows (the coroutine stubs' copy of the prologue): the two `sub`s
/// without probes, as the stubs always had.
pub fn put_allocation(cx: &mut FnCx<'_, A64>, frame: u32) {
    let probe = cx.state.frame.probe;
    if !probes(frame, probe) {
        let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
        put(cx, mc::sub_imm(Size::X, SP, SP, u64::from(lo)));
        if lo != 0 {
            cx.cfi(CfiOp::DefCfaOffset(lo as i32));
        }
        put(cx, mc::sub_imm(Size::X, SP, SP, u64::from(hi)));
        if hi != 0 {
            cx.cfi(CfiOp::DefCfaOffset(frame as i32));
        }
        return;
    }
    let (words, rows) = allocation(frame, probe);
    let mut rows = rows.into_iter().peekable();
    for (index, word) in words.into_iter().enumerate() {
        put(cx, word);
        while let Some((_, op)) = rows.next_if(|&(after, _)| after == index + 1) {
            cx.cfi(op);
        }
    }
}

/// `mov sp, x16`, the end of a dynamic allocation; in a probed function
/// `llc`'s loop first, which touches every page from the stack pointer
/// down to `x16`, and a touch of the new bottom after (flags are not
/// kept).
pub fn set_sp(cx: &mut FnCx<'_, A64>) {
    let probe = cx.state.frame.probe;
    if probe != 0 {
        put(cx, mc::sub_imm(Size::X, SP, SP, u64::from(probe)));
        put(cx, mc::cmp_ext(Size::X, SP, X16.zr(), mc::Extend::Uxtx, 0));
        put(cx, mc::b_cond(Cond::Ls, 12));
        put(cx, mc::str_off(MemSize::X, RegZr::ZR, SP, 0));
        put(cx, mc::b(-16));
    }
    put(cx, mc::mov_sp(Size::X, SP, X16.sp()));
    if probe != 0 {
        put(cx, mc::ldr_off(Load::X, RegZr::ZR, SP, 0));
    }
}

/// The stack pointer lowered by `size` for an outgoing argument area in a
/// probed function: past 1024 bytes, the bottom of every page of at most
/// `P` bytes touched (no other register written). `false` when nothing is
/// probed (the caller lowers it).
pub fn adjust_sp(cx: &mut FnCx<'_, A64>, size: u64) -> bool {
    let probe = u64::from(cx.state.frame.probe);
    if probe == 0 || size <= u64::from(UNPROBED) {
        return false;
    }
    let mut left = size;
    while left != 0 {
        let step = left.min(probe);
        let (lo, hi) = (step & 0xfff, step & 0xff_f000);
        for part in [hi, lo] {
            if part != 0 {
                put(cx, mc::sub_imm(Size::X, SP, SP, part));
            }
        }
        put(cx, mc::str_off(MemSize::X, RegZr::ZR, SP, 0));
        left -= step;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6.27: the longest allocation fits what the prologue reserves;
    /// its rows end with the CFA at `sp + FRAME`.
    #[test]
    fn allocations_fit_the_reserve() {
        for probe in [4096, 8192, 65536] {
            for frame in (16..30 * probe).step_by(16).filter(|&f| probes(f, probe)) {
                let (words, rows) = allocation(frame, probe);
                assert!(4 * words.len() as u32 <= 8 + RESERVE, "{frame} {probe}");
                assert!(rows.windows(2).all(|w| w[0].0 <= w[1].0));
                let mut cfa = (DWARF_SP, 0);
                for &(_, op) in &rows {
                    match op {
                        CfiOp::DefCfaOffset(o) => cfa.1 = o,
                        CfiOp::DefCfa { reg, offset } => cfa = (reg, offset),
                        CfiOp::DefCfaRegister(reg) => cfa.0 = reg,
                        // The looped shape's temporary frame record.
                        CfiOp::Offset { reg: DWARF_FP, cfa_offset: -16 }
                        | CfiOp::Offset { reg: DWARF_LR, cfa_offset: -8 } => {}
                        _ => unreachable!("a probe row that is not the CFA's"),
                    }
                }
                assert_eq!(cfa, (DWARF_SP, frame as i32), "{frame} {probe}");
            }
        }
        assert!(!probes(3072, 4096) && probes(3088, 4096) && !probes(1 << 20, 0));
    }
}
