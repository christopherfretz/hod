//! Stack probing (`spec/codegen.md` §5.6.27, WP-186): a function with
//! `"probe-stack"="inline-asm"` touches the stack it allocates page by
//! page, from high to low addresses, so that no guard page below the
//! stack is jumped over. The shapes are `llc -O0`'s (19 and 22):
//!
//! ```text
//! FRAME <= P            sub rsp, FRAME
//! P < FRAME <= 8P       (sub rsp, P · mov qword [rsp], 0) while P more remain · sub rsp, rest
//! FRAME > 8P            mov r11, rsp · sub r11, FRAME & -P
//!                       1: sub rsp, P · mov qword [rsp], 0 · cmp rsp, r11 · jne 1b
//!                       [sub rsp, FRAME % P]
//! ```
//!
//! The rest below the lowest touch (at most `P`) is left as `llc` leaves
//! it: the next push or call touches it. The CFA is `rbp + 16` from the
//! second instruction of the prologue on, so the loop needs no CFI row.
//!
//! A dynamic `alloca` or over-aligned slot ([`set_sp`]) lowers the stack
//! pointer to `r11` through `llc`'s loop: `1: cmp rsp, r11 · jbe 2f · xor
//! qword [rsp], 0 · sub rsp, P · jmp 1b · 2: mov rsp, r11`. An outgoing
//! argument area ([`adjust_sp`]) is touched below the stack pointer first
//! and then at the bottom of every page.

use hod_cg::FnCx;
use hod_ir::SymbolKind;
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Size};

use crate::common::{R11, RSP, put};
use crate::target::X64;

/// `llc`'s unroll limit, in probe sizes: a larger frame is a loop.
pub const UNROLL: u32 = 8;

/// The bytes the prologue of a probed function reserves beyond an
/// unprobed one's `sub rsp, imm32`: the longest [`write_allocation`]
/// (seven unrolled probes and the rest, 7 × 15 + 7 bytes) less those 7.
pub const RESERVE: u32 = (UNROLL - 1) * 15;

/// The probe size of the function being compiled; 0 when it does not
/// probe (`hod_ir::StackProbe`; `classify` refused the other values).
pub fn size_of(cx: &FnCx<'_, X64>) -> u32 {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return 0;
    };
    cx.module.fn_attrs(attrs).stack_probe().size().unwrap_or(0)
}

/// Instructions written one after the other into a byte buffer.
struct Writer<'b> {
    out: &'b mut [u8],
    len: usize,
}

impl Writer<'_> {
    fn put(&mut self, inst: mc::Inst) {
        let bytes = inst.bytes();
        self.out[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
}

/// `mov qword [rsp + disp], 0`.
fn touch(disp: i32) -> mc::Inst {
    mc::mov_mi(Size::Q, Mem::base(RSP, disp), 0)
}

/// Writes the allocation of a `frame`-byte local area at the start of
/// `out` (nothing for 0); `probe` is the probe size, 0 for none. Its
/// length, at most 7 + [`RESERVE`] bytes.
pub fn write_allocation(out: &mut [u8], frame: u32, probe: u32) -> usize {
    let mut w = Writer { out, len: 0 };
    let sub = |n: u32| mc::sub_ri(Size::Q, RSP, i64::from(n));
    if frame == 0 {
    } else if probe == 0 || frame <= probe {
        w.put(sub(frame));
    } else if frame <= UNROLL * probe {
        let mut done = 0;
        while done + probe < frame {
            w.put(sub(probe));
            w.put(touch(0));
            done += probe;
        }
        w.put(sub(frame - done));
    } else {
        w.put(mc::mov_rr(Size::Q, R11, RSP));
        w.put(mc::sub_ri(Size::Q, R11, i64::from(frame - frame % probe)));
        let body = [sub(probe), touch(0), mc::cmp_rr(Size::Q, RSP, R11)];
        let back = body.iter().map(|i| i.bytes().len()).sum::<usize>() + 2;
        for inst in body {
            w.put(inst);
        }
        w.put(mc::jcc_short(Cond::Ne, -(back as i8)));
        if !frame.is_multiple_of(probe) {
            w.put(sub(frame % probe));
        }
    }
    w.len
}

/// The allocation of [`write_allocation`] emitted at the current offset
/// (the coroutine stubs' copy of the prologue); `sub rsp, frame` without
/// probes, as the stubs always had.
pub fn put_allocation(cx: &mut FnCx<'_, X64>, frame: u32) {
    let probe = cx.state.frame.probe;
    if probe == 0 {
        put(cx, mc::sub_ri(Size::Q, RSP, i64::from(frame)));
        return;
    }
    let mut buf = [0u8; 7 + RESERVE as usize];
    let len = write_allocation(&mut buf, frame, probe);
    cx.emit(&buf[..len]);
}

/// `mov rsp, r11`, the end of a dynamic allocation; in a probed function
/// `llc`'s loop first, which touches every page from the stack pointer
/// down to `r11` (flags are not kept).
pub fn set_sp(cx: &mut FnCx<'_, X64>) {
    let probe = cx.state.frame.probe;
    if probe != 0 {
        let body =
            [mc::xor_mi(Size::Q, Mem::base(RSP, 0), 0), mc::sub_ri(Size::Q, RSP, i64::from(probe))];
        let cmp = mc::cmp_rr(Size::Q, RSP, R11);
        let skip = body.iter().map(|i| i.bytes().len()).sum::<usize>() + 2;
        let back = cmp.bytes().len() + 2 + skip;
        put(cx, cmp);
        put(cx, mc::jcc_short(Cond::Be, skip as i8));
        for inst in body {
            put(cx, inst);
        }
        put(cx, mc::jmp_short(-(back as i8)));
    }
    put(cx, mc::mov_rr(Size::Q, RSP, R11));
}

/// The stack pointer lowered by `size` for an outgoing argument area in a
/// probed function, flags and registers kept: the word below the stack
/// pointer touched first (the rest the prologue or an `alloca` left is
/// at most `P`), then the bottom of every page of at most `P` bytes.
/// `false` when the function does not probe (the caller lowers it).
pub fn adjust_sp(cx: &mut FnCx<'_, X64>, size: u64) -> bool {
    let probe = u64::from(cx.state.frame.probe);
    if probe == 0 || size == 0 {
        return false;
    }
    put(cx, touch(-8));
    let mut left = size;
    while left != 0 {
        let step = left.min(probe);
        put(cx, mc::lea(Size::Q, RSP, Mem::base(RSP, -(step as i32))));
        put(cx, touch(0));
        left -= step;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(frame: u32, probe: u32) -> Vec<u8> {
        let mut buf = [0u8; 7 + RESERVE as usize];
        let len = write_allocation(&mut buf, frame, probe);
        buf[..len].to_vec()
    }

    /// §5.6.27: the longest allocation fits what the prologue reserves,
    /// and an unprobed one is the plain `sub`.
    #[test]
    fn allocations_fit_the_reserve() {
        for probe in [4096, 8192, 1 << 20] {
            for frame in (0..20 * probe).step_by(16).chain([MAX, MAX - 8]) {
                assert!(shape(frame, probe).len() <= 7 + RESERVE as usize, "{frame} {probe}");
            }
        }
        assert_eq!(shape(5008, 0), mc::sub_ri(Size::Q, RSP, 5008).bytes());
        assert!(shape(0, 4096).is_empty());
    }

    const MAX: u32 = crate::frame::MAX_X64_FRAME;

    /// The loop's branch goes back to its `sub`.
    #[test]
    fn the_loop_branches_to_its_start() {
        let code = shape(40000, 4096);
        // mov r11, rsp (3) · sub r11, imm32 (7) · then the loop.
        let start = 10;
        let jne = code.iter().rposition(|&b| b == 0x75).unwrap();
        let disp = code[jne + 1] as i8;
        assert_eq!(jne as i64 + 2 + i64::from(disp), start);
    }
}
