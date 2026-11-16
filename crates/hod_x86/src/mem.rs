//! `load`, `store` and `ptradd` of class `Gpr` (`spec/codegen-x86.md`
//! "Memory"). Owned by WP-293.
//!
//! An address is a stack slot's frame place, a symbol's absolute
//! `disp32` (`R_386_32`), or a register. Nothing is fused: a `ptradd`
//! computes its address into a register, and a load or store of it
//! reads that register.

use hod_cg::{CgResult, FnCx, SlotPlace};
use hod_ir::{Inst, MemFlags, PtrAdd, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Scale, Size};

use crate::alu::{Ext, extend_to};
use crate::common::{
    EAX, ECX, EDX, const_int, frame_inst, gp, other_scratch, put, sym_inst, unsupported, width,
};
use crate::target::X86;

/// A load or store with an ordering: design 0008's I11.
pub const ATOMIC: &str = "x86:atomic";

/// Where an access goes.
#[derive(Clone, Copy, Debug)]
enum At {
    /// A frame place.
    Frame(hod_cg::FrameRef),
    /// A symbol, at an addend.
    Sym(hod_ir::SymId, i64),
    /// The address in a register.
    Reg(Reg),
}

/// The address of `ptr`. A pointer slot (an over-aligned slot) is
/// loaded into `ecx`/`edx`, the one `avoid` is not.
fn address(cx: &mut FnCx<'_, X86>, ptr: Value, avoid: Option<Reg>) -> CgResult<At> {
    match ptr.kind() {
        ValueKind::Slot(slot) => match cx.slot_place(slot)? {
            SlotPlace::Direct(place) => Ok(At::Frame(place)),
            SlotPlace::Indirect(pointer) => {
                let r = if avoid == Some(ECX) { EDX } else { ECX };
                frame_inst(cx, pointer, |m| mc::mov_rm(Size::D, r, m));
                Ok(At::Reg(r))
            }
        },
        ValueKind::Sym(sym)
            if !matches!(
                hod_cg::sym_access(cx.module, cx.options, sym),
                hod_cg::Access::Tls(_)
            ) =>
        {
            Ok(At::Sym(sym, 0))
        }
        _ => Ok(At::Reg(gp(cx.use_reg(ptr)?))),
    }
}

/// Emits the instruction `make` builds over byte `k` of `at`.
fn access(
    cx: &mut FnCx<'_, X86>,
    at: At,
    k: i32,
    make: impl FnOnce(Mem) -> mc::Inst,
) -> CgResult<()> {
    match at {
        At::Frame(place) => {
            frame_inst(cx, place.offset_by(k as u32), make);
            Ok(())
        }
        At::Sym(sym, addend) => sym_inst(cx, sym, addend + i64::from(k), make),
        At::Reg(base) => {
            put(cx, make(Mem::base(base, k)));
            Ok(())
        }
    }
}

/// The scratch register an access of `at` may use besides `value`.
fn aux_for(at: At, value: Reg) -> Reg {
    match at {
        At::Reg(base) if base == ECX || base == EDX => other_scratch(base),
        _ => other_scratch(value),
    }
}

/// Rejects an ordered access.
fn check(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() { unsupported(ATOMIC) } else { Ok(()) }
}

/// The size in bytes of an access of `inst`'s type `ty`: its store size.
fn store_size(cx: &FnCx<'_, X86>, ty: hod_ir::TypeId) -> CgResult<u32> {
    match cx.types().size_of(ty) {
        n @ 0..=4 => Ok(n as u32),
        _ => cx.bug("a Gpr-class access wider than a register"),
    }
}

/// `%r = load <ty>, ptr %ptr`, `<ty>` of class `Gpr`.
///
/// # Errors
/// [`ATOMIC`]; what the operands report.
pub fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    let bytes = store_size(cx, inst.ty())?;
    let at = address(cx, ptr, None)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    let aux = aux_for(at, r);
    // A three-byte load reads the high byte first, so `r` may be the base.
    match bytes {
        0 | 1 => access(cx, at, 0, |m| mc::movzx_rm(Size::D, r, Size::B, m))?,
        2 => access(cx, at, 0, |m| mc::movzx_rm(Size::D, r, Size::W, m))?,
        3 => {
            access(cx, at, 2, |m| mc::movzx_rm(Size::D, aux, Size::B, m))?;
            put(cx, mc::shl_ri(Size::D, aux, 16));
            access(cx, at, 0, |m| mc::movzx_rm(Size::D, r, Size::W, m))?;
            put(cx, mc::or_rr(Size::D, r, aux));
        }
        _ => access(cx, at, 0, |m| mc::mov_rm(Size::D, r, m))?,
    }
    cx.finish(inst, rd)
}

/// `store <ty> %value, ptr %ptr`, `<ty>` of class `Gpr`.
///
/// # Errors
/// [`ATOMIC`]; what the operands report.
pub fn store<'a>(
    cx: &mut FnCx<'a, X86>,
    _inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    check(mem)?;
    let bytes = store_size(cx, crate::common::value_ty(cx, value))?;
    let rs = if let Some(c) = const_int(cx, value) {
        let t = gp(cx.temp(hod_cg::Class::Gpr)?);
        crate::common::mov_imm(cx, t, c);
        t
    } else {
        gp(cx.use_reg(value)?)
    };
    let at = if ptr == value { At::Reg(rs) } else { address(cx, ptr, Some(rs))? };
    let aux = aux_for(at, rs);
    let needs_aux = bytes == 3 || (bytes <= 1 && !crate::reg::has_low_byte(rs));
    if needs_aux {
        put(cx, mc::mov_rr(Size::D, aux, rs));
    }
    match bytes {
        0 | 1 => {
            let src = if needs_aux { aux } else { rs };
            access(cx, at, 0, |m| mc::mov_mr(Size::B, m, src))
        }
        2 => access(cx, at, 0, |m| mc::mov_mr(Size::W, m, rs)),
        3 => {
            access(cx, at, 0, |m| mc::mov_mr(Size::W, m, rs))?;
            put(cx, mc::shr_ri(Size::D, aux, 16));
            access(cx, at, 2, |m| mc::mov_mr(Size::B, m, aux))
        }
        _ => access(cx, at, 0, |m| mc::mov_mr(Size::D, m, rs)),
    }
}

/// The `SIB` scale of `scale`, if it is one.
fn sib(scale: u64) -> Option<Scale> {
    match scale {
        1 => Some(Scale::S1),
        2 => Some(Scale::S2),
        4 => Some(Scale::S4),
        8 => Some(Scale::S8),
        _ => None,
    }
}

/// `%r = ptradd ptr %base, <offset> [+ index * scale ...]`: the base in
/// a register, each index sign-extended to 32 bits in `eax` and scaled
/// (`lea` for 1, 2, 4, 8, else `imul`), the constant offset added; all
/// modulo 2^32.
///
/// # Errors
/// `x86:vector` for a vector of pointers; the type refusal of an index
/// that is no `Gpr` value; what the operands report.
pub fn ptr_add<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    if cx.types().is_vector(inst.ty()) {
        return unsupported(crate::target::VECTOR);
    }
    let mut offset = add.offset as u32;
    for (index, scale) in add.indices() {
        if cx.class_of(index) != hod_cg::Class::Gpr {
            return Err(crate::target::refusal_of(cx, crate::common::value_ty(cx, index)));
        }
        if let Some(c) = const_int(cx, index) {
            let bits = width(cx, index);
            let c = crate::alu::ext_const(c, bits, Ext::Sign);
            offset = offset.wrapping_add(c.wrapping_mul(scale as u32));
        }
    }
    let rb = cx.use_reg(add.base)?;
    let b = gp(rb);
    for (index, scale) in add.indices() {
        if const_int(cx, index).is_some() {
            continue;
        }
        let bits = width(cx, index);
        cx.put(index, crate::reg::EAX)?;
        extend_to(cx, EAX, bits, Ext::Sign);
        let Some(eax) = EAX.index() else {
            return cx.bug("eax has no index form");
        };
        if let Some(s) = sib(scale) {
            put(cx, mc::lea(Size::D, b, Mem::base_index(b, eax, s, 0)));
        } else {
            put(cx, mc::imul_rri(Size::D, EAX, EAX, i64::from(scale as u32 as i32)));
            put(cx, mc::add_rr(Size::D, b, EAX));
        }
    }
    if offset != 0 {
        put(cx, mc::lea(Size::D, b, Mem::base(b, offset as i32)));
    }
    cx.finish(inst, rb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_of_a_sib() {
        assert_eq!(sib(4), Some(Scale::S4));
        assert_eq!(sib(12), None);
    }
}
