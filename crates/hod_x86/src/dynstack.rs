//! The dynamic stack on i386 (`spec/codegen-x86.md` §12; design 0008's
//! I11b, WP-316): `alloca` of a variable size or outside the entry block,
//! over-aligned slots, `llvm.stacksave` and `llvm.stackrestore`.
//!
//! The shape is `hod_x64`'s. `esp` is moved down by the size rounded up
//! to 16 (so it stays 16-aligned for the calls that follow) and, for an
//! alignment above 16, rounded down with `and`; the new `esp` is the
//! address. The frame is `ebp`-based (`spec/codegen-x86.md` §3): locals,
//! arguments and the epilogue (`lea esp, [ebp - 12]`) never read `esp`,
//! and a call's outgoing area is reserved below `esp` by the call
//! sequence when the call is made, so it is always below every dynamic
//! area. An over-aligned slot of the body (`hod_cg` gives it a pointer
//! slot, `Target::dynamic_slot`) is allocated the same way in the entry
//! code: where `llc` realigns the whole frame (`and esp, -align` in the
//! prologue, the locals addressed from `esp` and the arguments from
//! `ebp`), hod gives that one slot an aligned dynamic area, which runs the
//! same. No stack probes: `llc` for i686 Linux probes only under
//! `"probe-stack"="inline-asm"` (refused, [`PROBE`]).

use hod_cg::{CgError, CgResult, FnCx, FrameRef, LIMIT_FRAME_SIZE};
use hod_ir::{Inst, Intrinsic, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::Size;

use crate::common::{ECX, ESP, const_int, frame_inst, gp, put, width};
use crate::target::X86;

/// A function with a `"probe-stack"` attribute (`llc` writes inline
/// probes for `"inline-asm"`, and calls the named function otherwise).
pub const PROBE: &str = "x86:probe-stack";

/// The entry check (`Target::begin_function`): a function with
/// `"probe-stack"="inline-asm"` is refused ([`PROBE`]): `llc` 19 and 22
/// probe the i386 frame page by page (`sub esp, 4096` · `mov dword
/// [esp], 0`, measured) and the dynamic areas, which needs the prologue.
/// Another value is the classifier's refusal.
///
/// # Errors
/// [`PROBE`].
pub fn check(cx: &FnCx<'_, X86>) -> CgResult<()> {
    let hod_ir::SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind()
    else {
        return Ok(());
    };
    match cx.module.fn_attrs(attrs).stack_probe() {
        hod_ir::StackProbe::None => Ok(()),
        _ => crate::common::unsupported(PROBE),
    }
}

/// The largest dynamic area: what `lea` with a 32-bit displacement and
/// the `and` mask can express.
const MAX: u64 = 1 << 30;

/// `esp` moved down by `ecx` (rounded up to 16) and aligned down to
/// `align`; the address left in `ecx`.
fn lower_esp(cx: &mut FnCx<'_, X86>, align: u64) {
    put(cx, mc::add_ri(Size::D, ECX, 15));
    put(cx, mc::and_ri(Size::D, ECX, -16));
    put(cx, mc::neg_r(Size::D, ECX));
    put(cx, mc::add_rr(Size::D, ECX, ESP));
    if align > 16 {
        put(cx, mc::and_ri(Size::D, ECX, -(align as i64)));
    }
    put(cx, mc::mov_rr(Size::D, ESP, ECX));
}

/// `Target::dynamic_slot`: an over-aligned slot of `size` bytes, its
/// address stored in the pointer slot `ptr`.
///
/// # Errors
/// `limit:frame-size` for a size or an alignment above 2^30.
pub fn dynamic_slot(cx: &mut FnCx<'_, X86>, ptr: FrameRef, size: u64, align: u64) -> CgResult<()> {
    if size > MAX || !align.is_power_of_two() || align > MAX {
        return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
    }
    put(cx, mc::mov_ri(Size::D, ECX, size as i64));
    lower_esp(cx, align);
    frame_inst(cx, ptr, |m| mc::mov_mr(Size::D, m, ECX));
    Ok(())
}

/// `%p = alloca <ty>, <iN> %count, align <align>` that the frame does not
/// hold: `count` (zero-extended: a negative count is undefined) times
/// `elem_size`, modulo 2^32 as `llc` computes it.
///
/// # Errors
/// `limit:frame-size` for a constant size or an alignment above 2^30; the
/// refusal of a count of more than 32 bits; what the operands report.
pub fn alloca<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    count: Value,
    elem_size: u64,
    align: u64,
) -> CgResult<()> {
    if !align.is_power_of_two() || align > MAX {
        return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
    }
    if let Some(n) = const_int(cx, count) {
        let size = u64::from(n).checked_mul(elem_size).filter(|&s| s <= MAX);
        let Some(size) = size else { return Err(CgError::unsupported(LIMIT_FRAME_SIZE)) };
        put(cx, mc::mov_ri(Size::D, ECX, size as i64));
    } else {
        if width(cx, count) > 32 {
            return Err(crate::target::refusal_of(cx, crate::common::value_ty(cx, count)));
        }
        let bits = width(cx, count);
        let n = gp(cx.use_reg(count)?);
        put(cx, mc::mov_rr(Size::D, ECX, n));
        crate::alu::extend_to(cx, ECX, bits, crate::alu::Ext::Zero);
        let elem = elem_size as u32;
        if elem == 0 {
            put(cx, mc::mov_ri(Size::D, ECX, 0));
        } else if elem.is_power_of_two() {
            if elem > 1 {
                put(cx, mc::shl_ri(Size::D, ECX, elem.trailing_zeros() as u8));
            }
        } else {
            put(cx, mc::imul_rri(Size::D, ECX, ECX, i64::from(elem as i32)));
        }
    }
    lower_esp(cx, align);
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), ECX));
    cx.finish(inst, rd)
}

/// `llvm.stacksave` (the current `esp`) and `llvm.stackrestore`.
///
/// # Errors
/// What the operands report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    match (which, args) {
        (Intrinsic::StackSave, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::D, gp(rd), ESP));
            cx.finish(inst, rd)
        }
        (Intrinsic::StackRestore, &[ptr]) => {
            let r = gp(cx.use_reg(ptr)?);
            put(cx, mc::mov_rr(Size::D, ESP, r));
            Ok(())
        }
        _ => cx.bug("a stack intrinsic of another shape"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_stable() {
        assert_eq!(super::PROBE, "x86:probe-stack");
    }
}
