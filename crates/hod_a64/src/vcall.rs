//! Vectors across calls (`spec/codegen.md` §5.5.1 "Vectors across calls
//! (WP-082)", §7.2; `spec/abi-a64.md` §4.3; design note
//! `docs/design/0003-vectors.md` §3, §4.3): the moves of the parts of
//! kind `PartKind::Lanes`, which `abi::assign` makes for the vectors LLVM
//! passes with each element widened to a lane.
//!
//! **Owner: WP-082.** Tests: `tests/vcall.rs`. The hooks are
//! `Target::lanes_out` / `Target::lanes_in` of `A64` (`target.rs`) and
//! the return of a mask in memory (`term.rs`).
//!
//! # The two kinds of vector
//!
//! - **Masks** (`<N x i1>`, `N ≤ 64`): their value is a general register
//!   whose bit `i` is element `i` (WP-081), or bits of an aggregate's
//!   image in memory (lane 0 in bit 0 of byte 0, the bits after the last
//!   element zero). They travel as the lanes of one `D` or `Q` register
//!   (2, 4, 8, 16 elements: `2s`, `4h`, `8b`, `16b`), or one element per
//!   general register and 8-byte stack slot (any other count; 4 bytes
//!   written). Each lane is 0 or 1 when hod produces it, as `llc` does;
//!   only bit 0 of a lane is read when hod consumes it (`llc`'s
//!   consumers do `shl` + `cmlt`, or `and #1`).
//! - **Promoted vectors** (`<2 x i8>`, `<2 x i16>` as `2s`, `<4 x i8>` as
//!   `4h`): class `Mem`, an image of 2 or 4 bytes. Produced with `uxtl`
//!   (lanes extended by zero, as `llc`'s `ushll`), consumed with `xtn`
//!   (only the low bits of a lane are read: `llc` 19 leaves garbage above
//!   the element in lane 0).
//!
//! - **A lone `<1 x i128>`** (WP-167): a `Q` register value that travels
//!   as an `i128`, in two general registers or 8-byte stack slots. Each
//!   part is one 64-bit limb (`bits` 64, `lane` 8, `len` 2, `first` the
//!   limb), moved with `umov`/`ins` between the value's `Q` register (the
//!   scratch `v30` that `hod_cg` gives for the value) and the place.
//!
//! On the stack a `D`/`Q` part is the register's image in its slot.
//!
//! # Registers
//!
//! Every move writes `x16`, `x17`, `v30`, `v31` and its destination only;
//! a general register holding a mask's bits as the source is left
//! unchanged (`hod_cg` puts it in `x16`).

use hod_cg::abi::{LaneLoc, Part, PartKind};
use hod_cg::error::CALL_PART_LANES;
use hod_cg::{CgError, CgResult, FnCx, FrameRef};
use hod_ir::{TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Elem, Load, MemSize, Reg, Size, VMemSize, VReg, VecBits};

use crate::common::{
    Cursor, V30, V31, X16, X17, load_fpr_at, load_gpr_at, other_scratch, put, store_fpr_at,
    store_gpr_at, value_ty, vr, xr,
};
use crate::reg;
use crate::target::A64;

/// A part of kind `Lanes`, decoded.
#[derive(Clone, Copy, Debug)]
struct Lanes {
    /// Bits of an element: 1, 8 or 16.
    bits: u32,
    /// Bytes of a lane at the place.
    lane: u32,
    /// The element of the first lane.
    first: u32,
    /// Elements of the vector.
    len: u32,
    /// Lanes the part carries.
    count: u32,
    /// Bytes of the place: 4, 8 or 16.
    size: u32,
}

impl Lanes {
    fn of(cx: &FnCx<'_, A64>, part: &Part) -> CgResult<Lanes> {
        let PartKind::Lanes { bits, lane, first, len } = part.kind else {
            return cx.bug("a lane move of a part that is no Lanes part");
        };
        let (bits, lane, first, len) =
            (u32::from(bits), u32::from(lane), u32::from(first), u32::from(len));
        let count = part.size / lane.max(1);
        let ok = matches!(bits, 1 | 8 | 16)
            && matches!(lane, 1 | 2 | 4)
            && matches!(part.size, 4 | 8 | 16)
            && count * lane == part.size
            && 8 * lane >= bits
            && first + count <= len
            && len <= 64;
        if !ok {
            return cx.bug("a Lanes part whose shape abi::assign does not make");
        }
        Ok(Lanes { bits, lane, first, len, count, size: part.size })
    }

    /// The element arrangement of a lane.
    fn elem(self) -> Elem {
        match self.lane {
            1 => Elem::B,
            2 => Elem::H,
            _ => Elem::S,
        }
    }

    /// The bytes of the vector's image: `⌈len·bits/8⌉`.
    fn image_bytes(self) -> u32 {
        (self.len * self.bits).div_ceil(8)
    }

    fn view(self) -> VMemSize {
        match self.size {
            4 => VMemSize::S,
            8 => VMemSize::D,
            _ => VMemSize::Q,
        }
    }
}

/// The load or store size of `bytes` (1, 2, 4 or 8) bytes.
fn vmem(bytes: u32) -> VMemSize {
    match bytes {
        1 => VMemSize::B,
        2 => VMemSize::H,
        4 => VMemSize::S,
        _ => VMemSize::D,
    }
}

/// `Target::lanes_out`: the producer's side of a `Lanes` part.
///
/// # Errors
/// `call:part-lanes` for a mask or promoted vector whose value side is a
/// vector register; `Internal` for a part `abi::assign` does not make.
pub fn lanes_out(cx: &mut FnCx<'_, A64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    if let Some(limb) = limb_of(cx, part)? {
        return limb_out(cx, limb, from, to);
    }
    if !value_side_ok(from) {
        return Err(CgError::unsupported(CALL_PART_LANES));
    }
    let l = Lanes::of(cx, part)?;
    if l.bits != 1 {
        return promoted_out(cx, l, from, to);
    }
    // A mask: its bits in a general register.
    let g = match from {
        LaneLoc::Reg(g) => xr(g),
        LaneLoc::Frame(image) => {
            load_bits(cx, X16, image, l.image_bytes());
            X16
        }
    };
    let t = other_scratch(g);
    match to {
        LaneLoc::Reg(r) if reg::is_gpr(r) => {
            put(cx, mc::ubfx(Size::X, xr(r).zr(), g.zr(), l.first, 1));
        }
        LaneLoc::Reg(r) => bits_to_lanes(cx, l, g, t, vr(r)),
        LaneLoc::Frame(slot) => {
            if l.size == 4 {
                put(cx, mc::ubfx(Size::X, t.zr(), g.zr(), l.first, 1));
                put(cx, mc::fmov_gpr_to_fp(Size::W, V31, t.zr()));
            } else {
                bits_to_lanes(cx, l, g, t, V31);
            }
            let mut at = Cursor::frame(cx, slot, t);
            store_fpr_at(cx, l.view(), V31, &mut at);
        }
    }
    Ok(())
}

/// The lanes of `v`, zeroed, then lane `i` set to bit `first + i` of `g`
/// through `t`.
fn bits_to_lanes(cx: &mut FnCx<'_, A64>, l: Lanes, g: Reg, t: Reg, v: VReg) {
    put(cx, mc::movi_mask(VecBits::V128, v, 0));
    for i in 0..l.count {
        put(cx, mc::ubfx(Size::X, t.zr(), g.zr(), l.first + i, 1));
        put(cx, mc::ins_gpr(l.elem(), v, i, t.zr()));
    }
}

/// Loads at least `bytes` (1 to 8) bytes of a mask's image at `image`
/// into `rd` (the bits above are unspecified); the cursor's scratch is
/// the other one.
fn load_bits(cx: &mut FnCx<'_, A64>, rd: Reg, image: FrameRef, bytes: u32) {
    let mut at = Cursor::frame(cx, image, other_scratch(rd));
    load_gpr_at(cx, crate::common::gpr_load_op(bytes), rd, &mut at);
}

/// A promoted vector's image at `from` widened into the lanes of `to`.
fn promoted_out(cx: &mut FnCx<'_, A64>, l: Lanes, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let LaneLoc::Frame(image) = from else {
        return cx.bug("a promoted vector that is not in memory");
    };
    let d = match to {
        LaneLoc::Reg(r) => vr(r),
        LaneLoc::Frame(_) => V31,
    };
    let mut at = Cursor::frame(cx, image, X16);
    load_fpr_at(cx, vmem(l.image_bytes()), d, &mut at);
    let mut bits = l.bits;
    while bits < 8 * l.lane {
        let from = if bits == 8 { Elem::B } else { Elem::H };
        put(cx, mc::uxtl(from, d, d));
        bits *= 2;
    }
    if let LaneLoc::Frame(slot) = to {
        let mut at = Cursor::frame(cx, slot, X17);
        store_fpr_at(cx, l.view(), V31, &mut at);
    }
    Ok(())
}

/// `Target::lanes_in`: the consumer's side of a `Lanes` part.
///
/// # Errors
/// As [`lanes_out`].
pub fn lanes_in(cx: &mut FnCx<'_, A64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    if let Some(limb) = limb_of(cx, part)? {
        return limb_in(cx, limb, from, to);
    }
    if !value_side_ok(to) {
        return Err(CgError::unsupported(CALL_PART_LANES));
    }
    let l = Lanes::of(cx, part)?;
    if l.bits != 1 {
        return promoted_in(cx, l, from, to);
    }
    match to {
        LaneLoc::Reg(g) => {
            let g = xr(g);
            let t = other_scratch(g);
            match from {
                LaneLoc::Reg(r) if reg::is_gpr(r) => {
                    put(cx, mc::bfi(Size::X, g.zr(), xr(r).zr(), l.first, 1));
                }
                LaneLoc::Reg(r) => lanes_to_bits(cx, l, vr(r), g, t),
                LaneLoc::Frame(slot) if l.size == 4 => {
                    let mut at = Cursor::frame(cx, slot, t);
                    load_gpr_at(cx, Load::B, t, &mut at);
                    put(cx, mc::bfi(Size::X, g.zr(), t.zr(), l.first, 1));
                }
                LaneLoc::Frame(slot) => {
                    let mut at = Cursor::frame(cx, slot, t);
                    load_fpr_at(cx, l.view(), V31, &mut at);
                    lanes_to_bits(cx, l, V31, g, t);
                }
            }
        }
        LaneLoc::Frame(image) => {
            if l.size == 4 {
                // One element: its bit is or-ed into its byte of the
                // image, through `v30`/`v31`, after the part of element
                // 0 cleared the image.
                if l.first == 0 {
                    let mut at = Cursor::frame(cx, image, X16);
                    crate::common::zero_bytes(cx, &mut at, l.image_bytes());
                }
                match from {
                    LaneLoc::Reg(r) => {
                        put(cx, mc::ubfiz(Size::W, X17.zr(), xr(r).zr(), l.first % 8, 1));
                    }
                    LaneLoc::Frame(slot) => {
                        let mut at = Cursor::frame(cx, slot, X17);
                        load_gpr_at(cx, Load::B, X17, &mut at);
                        put(cx, mc::ubfiz(Size::W, X17.zr(), X17.zr(), l.first % 8, 1));
                    }
                }
                put(cx, mc::fmov_gpr_to_fp(Size::W, V30, X17.zr()));
                let byte = offset(image, l.first / 8);
                let mut at = Cursor::frame(cx, byte, X16);
                load_fpr_at(cx, VMemSize::B, V31, &mut at);
                put(cx, mc::orr_vec(VecBits::V64, V31, V31, V30));
                let mut at = Cursor::frame(cx, byte, X16);
                store_fpr_at(cx, VMemSize::B, V31, &mut at);
            } else {
                // Every element: the bits gathered in `x17` from zero,
                // then the image's bytes stored.
                let v = match from {
                    LaneLoc::Reg(r) => vr(r),
                    LaneLoc::Frame(slot) => {
                        let mut at = Cursor::frame(cx, slot, X17);
                        load_fpr_at(cx, l.view(), V31, &mut at);
                        V31
                    }
                };
                put(cx, mc::movz(Size::X, X17.zr(), 0, 0));
                lanes_to_bits(cx, l, v, X17, X16);
                put(cx, mc::fmov_gpr_to_fp(Size::X, V30, X17.zr()));
                let mut at = Cursor::frame(cx, image, X16);
                store_fpr_at(cx, vmem(l.image_bytes()), V30, &mut at);
            }
        }
    }
    Ok(())
}

/// Bit `first + i` of `g` set to bit 0 of lane `i` of `v`, through `t`.
fn lanes_to_bits(cx: &mut FnCx<'_, A64>, l: Lanes, v: VReg, g: Reg, t: Reg) {
    for i in 0..l.count {
        put(cx, mc::umov(l.elem(), t.zr(), v, i));
        put(cx, mc::bfi(Size::X, g.zr(), t.zr(), l.first + i, 1));
    }
}

/// The lanes at `from` narrowed into a promoted vector's image at `to`.
fn promoted_in(cx: &mut FnCx<'_, A64>, l: Lanes, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let LaneLoc::Frame(image) = to else {
        return cx.bug("a promoted vector that is not in memory");
    };
    let mut v = match from {
        LaneLoc::Reg(r) => vr(r),
        LaneLoc::Frame(slot) => {
            let mut at = Cursor::frame(cx, slot, X17);
            load_fpr_at(cx, l.view(), V31, &mut at);
            V31
        }
    };
    let mut width = 8 * l.lane;
    while width > l.bits {
        let narrow = if width == 32 { Elem::H } else { Elem::B };
        put(cx, mc::xtn(narrow, V31, v));
        v = V31;
        width /= 2;
    }
    let mut at = Cursor::frame(cx, image, X16);
    store_fpr_at(cx, vmem(l.image_bytes()), v, &mut at);
    Ok(())
}

/// WP-167: whether the value side of a mask's or a promoted vector's
/// lane move is one this module moves: a general register (a mask's
/// bits) or a frame place. `hod_cg` hands a register value in a scratch
/// register of its class; a vector register here is refused, never read
/// as a general one.
fn value_side_ok(value: LaneLoc) -> bool {
    match value {
        LaneLoc::Reg(r) => reg::is_gpr(r),
        LaneLoc::Frame(_) => true,
    }
}

/// WP-167: the limb (0 low, 1 high) a part of a lone `<1 x i128>`
/// carries; `None` for any other `Lanes` part.
fn limb_of(cx: &FnCx<'_, A64>, part: &Part) -> CgResult<Option<u32>> {
    let PartKind::Lanes { bits: 64, lane, first, len } = part.kind else { return Ok(None) };
    if lane != 8 || len != 2 || first > 1 || part.size != 8 {
        return cx.bug("a limb part whose shape abi::assign does not make");
    }
    Ok(Some(u32::from(first)))
}

/// WP-167: limb `limb` of the `Q`-register value at `from` (a vector
/// register, or the value's 16-byte image) to its place `to`: a general
/// register or an 8-byte stack slot. Writes `x16`, `x17` and `to` only.
fn limb_out(cx: &mut FnCx<'_, A64>, limb: u32, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let t = match to {
        LaneLoc::Reg(r) if reg::is_gpr(r) => xr(r),
        LaneLoc::Reg(_) => return cx.bug("a limb part in a vector register"),
        LaneLoc::Frame(_) => X16,
    };
    match from {
        LaneLoc::Reg(v) if reg::is_fpr(v) => put(cx, mc::umov(Elem::D, t.zr(), vr(v), limb)),
        LaneLoc::Reg(_) => return cx.bug("a Q-register value in a general register"),
        LaneLoc::Frame(image) => {
            let mut at = Cursor::frame(cx, offset(image, 8 * limb), other_scratch(t));
            load_gpr_at(cx, Load::X, t, &mut at);
        }
    }
    if let LaneLoc::Frame(slot) = to {
        let mut at = Cursor::frame(cx, slot, X17);
        store_gpr_at(cx, MemSize::X, t.zr(), &mut at);
    }
    Ok(())
}

/// WP-167: limb `limb` at its place `from` (a general register or an
/// incoming 8-byte stack slot) into the `Q`-register value at `to` (a
/// vector register, whose other limb is kept, or the value's image).
/// Writes `x16`, `x17` and `to` only.
fn limb_in(cx: &mut FnCx<'_, A64>, limb: u32, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let t = match from {
        LaneLoc::Reg(r) if reg::is_gpr(r) => xr(r),
        LaneLoc::Reg(_) => return cx.bug("a limb part in a vector register"),
        LaneLoc::Frame(slot) => {
            let mut at = Cursor::frame(cx, slot, X17);
            load_gpr_at(cx, Load::X, X16, &mut at);
            X16
        }
    };
    match to {
        LaneLoc::Reg(v) if reg::is_fpr(v) => put(cx, mc::ins_gpr(Elem::D, vr(v), limb, t.zr())),
        LaneLoc::Reg(_) => return cx.bug("a Q-register value in a general register"),
        LaneLoc::Frame(image) => {
            let mut at = Cursor::frame(cx, offset(image, 8 * limb), other_scratch(t));
            store_gpr_at(cx, MemSize::X, t.zr(), &mut at);
        }
    }
    Ok(())
}

/// `offset` bytes into frame place `of`.
fn offset(of: FrameRef, by: u32) -> FrameRef {
    of.offset_by(by)
}

/// The hook of `term::ret` for a function that returns a register value
/// in memory (`abi-a64.md` §5.2): a mask of 9 to 64 elements. Its image
/// (the bits after the last element zero, as a store of a mask writes
/// them) goes to scratch image 0, which `ret` copies into the buffer.
///
/// # Errors
/// `Internal` for a value that is no mask.
pub fn ret_image(cx: &mut FnCx<'_, A64>, value: Value) -> CgResult<FrameRef> {
    let ty = value_ty(cx, value);
    let len = match cx.types().kind(ty) {
        TypeKind::Vector { elem, len } if elem == hod_ir::TypeId::I1 && len <= 64 => len,
        _ => return cx.bug("a register value returned in memory that is no mask"),
    };
    let scratch = cx.scratch_reg(hod_cg::Class::Gpr);
    cx.put(value, scratch)?;
    let bits = xr(scratch);
    if len < 64 {
        put(cx, mc::ubfx(Size::X, bits.zr(), bits.zr(), 0, len));
    }
    let image = crate::vector::scratch(cx, 0)?;
    let mut at = Cursor::frame(cx, image, other_scratch(bits));
    store_gpr_at(cx, MemSize::X, bits.zr(), &mut at);
    Ok(image)
}
