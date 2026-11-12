//! Constant operands that hold addresses (WP-072, `spec/codegen.md`
//! §5.5.1 "Constant operands with addresses").
//!
//! Every address in an operand constant is computed in code, through the
//! symbol access of §7.4 ([`sym_address`]) or the function's own block
//! addresses ([`block_address`]), and the pieces are written where the
//! value goes: no constant pool, no data relocation, so the result is
//! right for PIC and non-PIC, preemptible, local and weak undefined
//! symbols alike (a GOT load gives 0 for a weak symbol that is not
//! defined), and a difference of two symbols in different sections, or a
//! truncated address, is an ordinary subtraction or a narrower store.
//!
//! - [`sym_diff`]: a `SymDiff` operand of class `Gpr`
//!   (`operand:sym-diff`), `a + addend - b` in the register; the high
//!   bits above the type's width are unspecified (§2.2).
//! - [`write_image`]: a `Mem`-class constant whose image has relocations
//!   other than aligned 8-byte symbol addresses (those `common` writes
//!   itself): a field of 1, 2, 4 or 8 bytes at any offset holding a
//!   symbol address (`Sym`, `Plt`), a block address, or a difference
//!   (`Diff`, `PltDiff`), the field's value truncated to its width.
//!
//! `dso_local_equivalent @f` is `f`'s own address: the equivalent the
//! function call through it needs, exactly as the scalar operand is
//! materialized (`common::materialize_gpr_const`).
//!
//! Hooks (one call each, at the places that refused):
//! `common::materialize_gpr_const` (`SymDiff`) and
//! `common::materialize_mem_const` (an image `write_image` takes).

use hod_cg::{CgResult, FnCx, FrameRef};
use hod_ir::{ConstId, FixupTarget, SymId, TypeId};
use hod_mc_a64 as mc;
use hod_mc_a64::{MemSize, Reg, RegZr, Size};

use crate::common::{
    CONSTANT_RELOC, Cursor, X16, X17, block_address, mov_imm, other_scratch, put, store_gpr_at,
    sym_address, unsupported,
};
use crate::target::A64;

/// Puts `a - b + addend` (`Const::SymDiff`) in `rd`, writing `rd` and
/// one scratch register other than it.
///
/// # Errors
/// What [`sym_address`] reports; `Internal` when `id` is no `SymDiff`.
pub fn sym_diff(cx: &mut FnCx<'_, A64>, rd: Reg, id: ConstId) -> CgResult<()> {
    let hod_ir::Const::SymDiff { a, b, addend, .. } = cx.module.consts.get(id) else {
        return cx.bug("sym_diff of a constant that is no difference");
    };
    difference(cx, rd, a, b, addend)
}

/// `rd = a + addend - b`, through one scratch register other than `rd`.
fn difference(cx: &mut FnCx<'_, A64>, rd: Reg, a: SymId, b: SymId, addend: i64) -> CgResult<()> {
    let tmp = other_scratch(rd);
    sym_address(cx, rd, a, addend)?;
    sym_address(cx, tmp, b, 0)?;
    put(cx, mc::sub_shifted(Size::X, rd.zr(), rd.zr(), tmp.zr(), mc::Shift::Lsl, 0));
    Ok(())
}

/// Whether `common` writes the image itself: every relocation is an
/// aligned 8-byte symbol address.
#[must_use]
pub fn is_plain(fixups: &[hod_ir::Fixup]) -> bool {
    fixups.iter().all(|f| {
        f.size == 8 && f.offset.is_multiple_of(8) && matches!(f.target, FixupTarget::Sym(_))
    })
}

/// Writes the flattened image of a `Mem`-class constant of type `ty` to
/// `dst`: the bytes between relocations as in `common` (zero by `xzr`,
/// others through `x17`), each relocated field computed in `x17` (with
/// `x16` as the second register of a difference, and as the cursor's
/// scratch) and stored with its width. Writes `x16` and `x17` only.
///
/// # Errors
/// [`CONSTANT_RELOC`] for a vector (vector constants with addresses
/// beyond what `common` writes are not this package's); what forming an
/// address reports.
pub fn write_image(
    cx: &mut FnCx<'_, A64>,
    dst: FrameRef,
    ty: TypeId,
    image: &[u8],
    fixups: &[hod_ir::Fixup],
) -> CgResult<()> {
    if matches!(cx.types().kind(ty), hod_ir::TypeKind::Vector { .. }) {
        return unsupported(CONSTANT_RELOC);
    }
    let mut pos = 0u64;
    for f in fixups {
        if f.offset < pos || f.offset + u64::from(f.size) > image.len() as u64 {
            return cx.bug("an image relocation outside its image or overlapping another");
        }
        bytes(cx, dst, image, pos, f.offset)?;
        match f.target {
            FixupTarget::Sym(sym) | FixupTarget::Plt(sym) => sym_address(cx, X17, sym, f.addend)?,
            FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
                difference(cx, X17, a, b, f.addend)?;
            }
            FixupTarget::Block { func, block } => {
                block_address(cx, X17, func, block)?;
                if f.addend != 0 {
                    crate::common::add_const(cx, X17.sp(), X17.sp(), f.addend);
                }
            }
            // WP-095: classify refuses a label difference in an operand
            // (`operand:block-diff`).
            FixupTarget::BlockDiff { .. } => return unsupported(CONSTANT_RELOC),
        }
        let ms = match f.size {
            1 => MemSize::B,
            2 => MemSize::H,
            4 => MemSize::W,
            8 => MemSize::X,
            _ => return cx.bug("an image relocation of a width other than 1, 2, 4 or 8"),
        };
        let mut at = Cursor::frame(cx, offset(cx, dst, f.offset)?, X16);
        store_gpr_at(cx, ms, X17.zr(), &mut at);
        pos = f.offset + u64::from(f.size);
    }
    bytes(cx, dst, image, pos, image.len() as u64)
}

/// `dst` moved on by `by` bytes.
fn offset(cx: &FnCx<'_, A64>, dst: FrameRef, by: u64) -> CgResult<FrameRef> {
    match u32::try_from(by) {
        Ok(by) => Ok(dst.offset_by(by)),
        Err(_) => cx.bug("an image larger than a frame"),
    }
}

/// Stores `image[from..to]` at `dst + from`: 8 bytes at a time, then 4,
/// 2 and 1 (unaligned stores are allowed on the frame's normal memory).
fn bytes(cx: &mut FnCx<'_, A64>, dst: FrameRef, image: &[u8], from: u64, to: u64) -> CgResult<()> {
    if from >= to {
        return Ok(());
    }
    let mut at = Cursor::frame(cx, offset(cx, dst, from)?, X16);
    let mut pos = from as usize;
    let end = to as usize;
    while pos < end {
        let take = match end - pos {
            8.. => 8,
            4..=7 => 4,
            2..=3 => 2,
            _ => 1,
        };
        let mut word = [0u8; 8];
        word[..take].copy_from_slice(&image[pos..pos + take]);
        let word = u64::from_le_bytes(word);
        let rt = if word == 0 {
            RegZr::ZR
        } else {
            mov_imm(cx, if take == 8 { Size::X } else { Size::W }, X17, word);
            X17.zr()
        };
        let ms = match take {
            8 => MemSize::X,
            4 => MemSize::W,
            2 => MemSize::H,
            _ => MemSize::B,
        };
        store_gpr_at(cx, ms, rt, &mut at);
        at.advance(take as u32);
        pos += take;
    }
    Ok(())
}
