//! Vectors across calls (`spec/codegen.md` §5.6.13 "Vectors (WP-132)",
//! `spec/abi-x64.md` §4.3): the moves of the parts of kind
//! `PartKind::Lanes` (`Target::lanes_out`, `Target::lanes_in`), which
//! `abi::assign` makes for the masks LLVM passes promoted to the lanes of
//! one `xmm`.
//!
//! **Owner: WP-132.** Tests: `tests/vcall.rs`.
//!
//! # The parts
//!
//! A mask `<N x i1>` of 2, 4, 8 or 16 elements travels as the lanes of
//! one `xmm` (`v2i64`, `v4i32`, `v8i16`, `v16i8`: lanes of `16 / N`
//! bytes, element `i` in lane `i`), or that register's 16-byte image in
//! a stack slot. Its value is a general register whose bit `i` is
//! element `i` (`mask.rs`), or the image of an aggregate's member in
//! memory (element 0 in bit 0 of byte 0, the bits after the last element
//! zero). Each lane hod produces is 0 or 1; hod reads only bit 0 of a
//! lane it consumes (`llc`'s producers write all ones, its consumers
//! shift bit 0 up or mask it).
//!
//! The lanes are built and read in a 16-byte frame slot: in the stack
//! slot of a part on the stack, else in a slot of the function's own,
//! loaded into or stored from the register (`movups`).
//!
//! # Registers
//!
//! Every move writes `r10` or `r11` (the one that is not the general
//! register holding the mask's bits, which is left unchanged as a
//! source), the flags and its destination only.

use hod_cg::abi::{LaneLoc, Part, PartKind};
use hod_cg::error::CALL_PART_LANES;
use hod_cg::{CgError, CgResult, FnCx, FrameRef};
use hod_mc_x64 as mc;
use hod_mc_x64::{Reg, Size};

use crate::common::{
    R10, gp, load_fpr, load_gpr, mov_imm, other_scratch, put, store_fpr, store_gpr, xm,
};
use crate::reg;
use crate::target::X64;

/// A part of kind `Lanes`, decoded.
#[derive(Clone, Copy, Debug)]
struct Lanes {
    /// Bytes of a lane: 1, 2, 4 or 8.
    lane: u32,
    /// The element of the first lane.
    first: u32,
    /// Elements of the vector.
    len: u32,
    /// Lanes the part carries.
    count: u32,
}

impl Lanes {
    fn of(cx: &FnCx<'_, X64>, part: &Part) -> CgResult<Lanes> {
        let PartKind::Lanes { bits, lane, first, len } = part.kind else {
            return cx.bug("a lane move of a part that is no Lanes part");
        };
        let (bits, lane, first, len) =
            (u32::from(bits), u32::from(lane), u32::from(first), u32::from(len));
        let count = part.size / lane.max(1);
        let ok = bits == 1
            && matches!(lane, 1 | 2 | 4 | 8)
            && part.size == 16
            && count * lane == part.size
            && first + count <= len
            && len <= 64;
        if !ok {
            return cx.bug("a Lanes part whose shape abi::assign does not make");
        }
        Ok(Lanes { lane, first, len, count })
    }

    /// The bytes of the mask's image: `⌈len/8⌉`.
    fn image_bytes(self) -> u32 {
        self.len.div_ceil(8)
    }

    /// Whether the part carries every element (the only shape §4.3 has).
    fn whole(self) -> bool {
        self.first == 0 && self.count == self.len
    }
}

/// A 16-byte slot: `to`'s own when it is a frame place, else a new one.
fn lanes_slot(cx: &mut FnCx<'_, X64>, at: LaneLoc) -> CgResult<FrameRef> {
    match at {
        LaneLoc::Frame(slot) => Ok(slot),
        LaneLoc::Reg(_) => cx.alloc_slot(16, 16),
    }
}

/// WP-167: whether the two sides of a lane move are registers of the
/// classes this module moves: the value (a mask's bits) a general
/// register, the place an `xmm`; a frame place on either side. `hod_cg`
/// hands the value in a scratch register of the value's class, and only
/// masks (class `Gpr`, `abi.rs` `mask_leaf`) have `Lanes` parts here, so
/// anything else (an `xmm` or x87 value) is refused, never treated as a
/// general register.
fn classes_ok(value: LaneLoc, place: LaneLoc) -> bool {
    let value_ok = match value {
        LaneLoc::Reg(r) => reg::is_gpr(r),
        LaneLoc::Frame(_) => true,
    };
    let place_ok = match place {
        LaneLoc::Reg(r) => reg::is_fpr(r),
        LaneLoc::Frame(_) => true,
    };
    value_ok && place_ok
}

/// `Target::lanes_out`: the producer's side of a `Lanes` part.
///
/// # Errors
/// `call:part-lanes` when a side is a register of a class this module
/// does not move ([`classes_ok`]); `Internal` for a part `abi::assign`
/// does not make; `limit:frame-size`.
pub fn lanes_out(cx: &mut FnCx<'_, X64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    if !classes_ok(from, to) {
        return Err(CgError::unsupported(CALL_PART_LANES));
    }
    let l = Lanes::of(cx, part)?;
    if matches!(to, LaneLoc::Reg(r) if !reg::is_fpr(r)) {
        return cx.bug("a mask's lanes bound for a general register");
    }
    // The mask's bits in a general register.
    let g = match from {
        LaneLoc::Reg(g) if !reg::is_fpr(g) => gp(g),
        LaneLoc::Reg(_) => return cx.bug("a mask's bits in a vector register"),
        LaneLoc::Frame(image) => {
            load_gpr(cx, R10, image, l.image_bytes());
            R10
        }
    };
    let t = other_scratch(g);
    let slot = lanes_slot(cx, to)?;
    for i in 0..l.count {
        put(cx, mc::mov_rr(Size::Q, t, g));
        let bit = l.first + i;
        if bit != 0 {
            put(cx, mc::shr_ri(Size::Q, t, bit as u8));
        }
        put(cx, mc::and_ri(Size::D, t, 1));
        store_gpr(cx, t, slot.offset_by(i * l.lane), l.lane);
    }
    if let LaneLoc::Reg(x) = to {
        load_fpr(cx, xm(x), slot, 16);
    }
    Ok(())
}

/// `Target::lanes_in`: the consumer's side of a `Lanes` part.
///
/// # Errors
/// As [`lanes_out`].
pub fn lanes_in(cx: &mut FnCx<'_, X64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    if !classes_ok(to, from) {
        return Err(CgError::unsupported(CALL_PART_LANES));
    }
    let l = Lanes::of(cx, part)?;
    let src = match from {
        LaneLoc::Frame(slot) => slot,
        LaneLoc::Reg(x) if reg::is_fpr(x) => {
            let slot = cx.alloc_slot(16, 16)?;
            store_fpr(cx, xm(x), slot, 16);
            slot
        }
        LaneLoc::Reg(_) => return cx.bug("a mask's lanes in a general register"),
    };
    match to {
        LaneLoc::Reg(g) if !reg::is_fpr(g) => {
            gather(cx, l, src, gp(g));
            Ok(())
        }
        LaneLoc::Reg(_) => cx.bug("a mask's bits bound for a vector register"),
        LaneLoc::Frame(image) => {
            if !l.whole() {
                return cx.bug("a mask's image from a part of its elements");
            }
            gather(cx, l, src, R10);
            store_gpr(cx, R10, image, l.image_bytes());
            Ok(())
        }
    }
}

/// Bit `first + i` of `g` set to bit 0 of lane `i` at `src`, through the
/// other scratch register; the part of element 0 clears `g` first.
fn gather(cx: &mut FnCx<'_, X64>, l: Lanes, src: FrameRef, g: Reg) {
    let t = other_scratch(g);
    if l.first == 0 {
        mov_imm(cx, g, 0, 64);
    }
    for i in 0..l.count {
        load_gpr(cx, t, src.offset_by(i * l.lane), 1);
        put(cx, mc::and_ri(Size::D, t, 1));
        let bit = l.first + i;
        if bit != 0 {
            put(cx, mc::shl_ri(Size::Q, t, bit as u8));
        }
        if l.first != 0 {
            put(cx, mc::btr_ri(Size::Q, g, bit as u8));
        }
        put(cx, mc::or_rr(Size::Q, g, t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WP-167: a lane move refuses a value side that is not a general
    /// register and a place side that is not an `xmm`.
    #[test]
    fn lane_moves_accept_only_their_register_classes() {
        let (g, x, st) = (reg::gpr(Reg::Rax), reg::xmm(mc::Xmm::Xmm0), reg::st(0));
        let frame = LaneLoc::Frame(FrameRef::Local(0));
        let r = LaneLoc::Reg;
        assert!(classes_ok(r(g), r(x)));
        assert!(classes_ok(frame, r(x)));
        assert!(classes_ok(r(g), frame));
        assert!(classes_ok(frame, frame));
        for (value, place) in [(x, x), (st, x), (g, g), (g, st), (x, g)] {
            assert!(!classes_ok(r(value), r(place)), "{value:?} {place:?}");
        }
        assert!(!classes_ok(r(x), frame));
        assert!(!classes_ok(r(st), frame));
        assert!(!classes_ok(frame, r(g)));
    }
}
