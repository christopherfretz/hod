//! Constants that hold addresses (`spec/codegen.md` §5.6, the AArch64
//! rule of §5.5.1 "Constant operands with addresses"). **Owner: WP-126.**
//! Tests: `tests/aggr.rs`.
//!
//! Every address in a constant is computed in code through the symbol
//! access of §7.4 (`common::sym_address`) and written where the value
//! goes: no constant pool, no data relocation, so the result is right for
//! preemptible, local and weak undefined symbols alike, and a difference
//! of symbols in different sections, or a truncated address, is an
//! ordinary subtraction or a narrower store.
//!
//! - [`gpr`]: a `SymDiff` (`a + addend - b`) or a number minus an address
//!   (`hod_ir::ConstPool::neg_addr`) as a `Gpr` operand. Hook: one call
//!   in `common::materialize_gpr_const`.
//! - [`write_image`]: the flattened image of a `Mem`-class constant
//!   (`aggr::materialize_mem`), each relocated field of 1, 2, 4 or 8 bytes
//!   at any offset (`Sym`, `Plt`, `Diff`, `PltDiff`) computed in `rax`
//!   and stored with its width, the bytes between them as immediates.
//!
//! Everything here keeps the flags (`lea`, `not`, `mov`): glue may run
//! between a flag-setter and its reader.

use hod_cg::{CgResult, FnCx, FrameRef};
use hod_ir::{Const, ConstId, Fixup, FixupTarget, SymId};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Scale, Size};

use crate::aggr::store_word;
use crate::common::{frame_inst, other_scratch, put, sym_address, unsupported};
use crate::target::X64;

/// The skeleton's refusal name, kept for what nothing here covers.
pub const CONSTADDR: &str = "x64:todo:constaddr";
/// A `Mem`-class constant with a relocation this target does not compute:
/// a difference of block addresses, or a vector's image with a relocation
/// other than an aligned 8-byte symbol address (AArch64's
/// `a64:constant-relocation`).
pub const CONSTANT_RELOC: &str = "x64:constant-relocation";

/// Puts the `Gpr` constant `id`, a `SymDiff` or a number minus an
/// address, in `rd`, writing `rd` and the general scratch register other
/// than it; flags kept.
///
/// # Errors
/// `x64:todo:constant` for another constant expression; what
/// `common::sym_address` reports.
pub fn gpr(cx: &mut FnCx<'_, X64>, rd: Reg, id: ConstId) -> CgResult<()> {
    match cx.module.consts.get(id) {
        Const::SymDiff { a, b, addend, .. } => difference(cx, rd, a, b, addend),
        Const::Expr { .. } => neg_addr(cx, rd, id),
        _ => unsupported(crate::common::CONSTANT),
    }
}

/// `rd = a + addend - b`: `t = !b`, then `rd + t + 1`. Writes `rd` and
/// `t`, the general scratch register other than `rd`.
pub fn difference(
    cx: &mut FnCx<'_, X64>,
    rd: Reg,
    a: SymId,
    b: SymId,
    addend: i64,
) -> CgResult<()> {
    let t = other_scratch(rd);
    sym_address(cx, rd, a, addend)?;
    sym_address(cx, t, b, 0)?;
    put(cx, mc::not_r(Size::Q, t));
    if let Some(index) = t.index() {
        put(cx, mc::lea(Size::Q, rd, Mem::base_index(rd, index, Scale::S1, 1)));
    }
    Ok(())
}

/// `rd = addend - (sym + offset)` (`hod_ir::ConstPool::neg_addr`): the
/// address of `sym` with `offset - addend` added, negated as `!x + 1`.
///
/// # Errors
/// `x64:todo:constant` for an expression of another form.
pub fn neg_addr(cx: &mut FnCx<'_, X64>, rd: Reg, id: ConstId) -> CgResult<()> {
    let Some(neg) = cx.module.consts.neg_addr(&cx.module.types, id) else {
        return unsupported(crate::common::CONSTANT);
    };
    sym_address(cx, rd, neg.sym, neg.addend.wrapping_neg())?;
    put(cx, mc::not_r(Size::Q, rd));
    put(cx, mc::lea(Size::Q, rd, Mem::base(rd, 1)));
    Ok(())
}

/// Whether every relocation of an image is an aligned 8-byte symbol
/// address (what AArch64's `common` writes without `write_image`).
#[must_use]
pub fn is_plain(fixups: &[Fixup]) -> bool {
    fixups.iter().all(|f| {
        f.size == 8 && f.offset.is_multiple_of(8) && matches!(f.target, FixupTarget::Sym(_))
    })
}

/// Writes the flattened image `image` (relocations `fixups`, in offset
/// order) of a `Mem`-class constant of type `ty` to `dst`: the bytes
/// between relocations as immediate stores (through `r11` for an 8-byte
/// word beyond 32 bits), each relocated field computed in `rax` (with
/// `r11` the second register of a difference) and stored with its width,
/// truncated. Writes `rax` and `r11` only; flags kept.
///
/// # Errors
/// [`CONSTANT_RELOC`] for a block-address difference and for a vector
/// image whose relocations are not plain ([`is_plain`]);
/// `x64:todo:control` for a block address (WP-124); what forming an
/// address reports.
pub fn write_image(
    cx: &mut FnCx<'_, X64>,
    dst: FrameRef,
    ty: hod_ir::TypeId,
    image: &[u8],
    fixups: &[Fixup],
) -> CgResult<()> {
    if !fixups.is_empty() && cx.types().is_vector(ty) && !is_plain(fixups) {
        return unsupported(CONSTANT_RELOC);
    }
    let rax = Reg::Rax;
    let mut pos = 0u64;
    for f in fixups {
        if f.offset < pos || f.offset + u64::from(f.size) > image.len() as u64 {
            return cx.bug("an image relocation outside its image or overlapping another");
        }
        bytes(cx, dst, image, pos, f.offset)?;
        match f.target {
            FixupTarget::Sym(sym) | FixupTarget::Plt(sym) => sym_address(cx, rax, sym, f.addend)?,
            FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
                difference(cx, rax, a, b, f.addend)?;
            }
            FixupTarget::Block { .. } => return unsupported(crate::control::CONTROL),
            FixupTarget::BlockDiff { .. } => return unsupported(CONSTANT_RELOC),
        }
        let size = match f.size {
            1 => Size::B,
            2 => Size::W,
            4 => Size::D,
            8 => Size::Q,
            _ => return cx.bug("an image relocation of a width other than 1, 2, 4 or 8"),
        };
        let at = offset(cx, dst, f.offset)?;
        frame_inst(cx, at, |m| mc::mov_mr(size, m, rax));
        pos = f.offset + u64::from(f.size);
    }
    bytes(cx, dst, image, pos, image.len() as u64)
}

/// `dst` moved on by `by` bytes.
fn offset(cx: &FnCx<'_, X64>, dst: FrameRef, by: u64) -> CgResult<FrameRef> {
    match u32::try_from(by) {
        Ok(by) => Ok(dst.offset_by(by)),
        Err(_) => cx.bug("an image larger than a frame"),
    }
}

/// Stores `image[from..to]` at `dst + from`, 8 bytes at a time, then 4,
/// 2 and 1.
fn bytes(cx: &mut FnCx<'_, X64>, dst: FrameRef, image: &[u8], from: u64, to: u64) -> CgResult<()> {
    let mut pos = from;
    while pos < to {
        let take = match to - pos {
            8.. => 8,
            4..=7 => 4,
            2..=3 => 2,
            _ => 1,
        };
        let start = pos as usize;
        let mut word = [0u8; 8];
        word[..take].copy_from_slice(&image[start..start + take]);
        let at = offset(cx, dst, pos)?;
        store_word(cx, at, u64::from_le_bytes(word), take as u32);
        pos += take as u64;
    }
    Ok(())
}
