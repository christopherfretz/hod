//! Indirect branches (`spec/codegen-x86.md` §15, WP-324): `indirectbr`
//! and `blockaddress` of the function being compiled.
//!
//! `indirectbr ptr %a` is `jmp r`; on a `blockaddress` constant of this
//! function it is a direct branch to that block (as `hod_x64`).
//!
//! A block's address in a register is position independent in every
//! relocation model, with no relocation:
//!
//! ```text
//! call 0f            ; e8 00 00 00 00, pushes the address of 0
//! 0: pop rd
//! lea rd, [rd + disp32]  ; disp32 = label - (end of the field) = label - 0 - 7
//! lea rd, [rd + 7]
//! ```
//!
//! The `disp32` is a `REL32` fixup to the block's label (`patch_branch`
//! writes the label's offset minus the field's end, which is 7 bytes
//! past `0`). `llc` writes `movl $.Ltmp, %r` (`R_386_32`) in the static
//! model and `leal .Ltmp@GOTOFF(%ebx), %r` under PIC: the value is the
//! same, hod needs neither a relocation against a label nor the GOT
//! pointer. The sequence keeps the flags (`call`, `pop`, `lea`) and
//! writes `rd` only; the `call` is not a call of a function (no
//! `pic::emit_call`) and pushes below `esp`, where nothing lives on i386.
//! It is no x87 barrier: `llc` lowers a `blockaddress` with one `mov` or
//! `lea`, and `indirectbr` ends its block anyway.
//!
//! A `blockaddress` in a global's initializer is the module emitter's
//! (`R_386_32` against the function's section).

use hod_cg::{CgResult, FnCx};
use hod_ir::{BlockId, Const, Inst, SymId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Mode, Reg, Size};

use crate::common::{REL32, gp, put, unsupported};
use crate::control::BLOCKADDRESS;
use crate::target::X86;

/// The bytes from the address the `call` pushes to the end of the first
/// `lea`'s `disp32`: `pop` (1) and `lea r, [r + disp32]` (6).
const FIELD_END: i32 = 7;

/// `indirectbr ptr %addr, [labels]`.
///
/// # Errors
/// [`BLOCKADDRESS`] for a block of another function; what the branch's
/// fixup reports.
pub fn indirect_br<'a>(cx: &mut FnCx<'a, X86>, _inst: Inst<'a>, addr: Value) -> CgResult<()> {
    if let ValueKind::Const(id) = addr.kind()
        && let Const::BlockAddr { func, block } = cx.module.consts.get(id)
    {
        if func != cx.body.symbol() || block.index() >= cx.body.block_count() {
            return unsupported(BLOCKADDRESS);
        }
        return crate::control::br(cx, block);
    }
    let r = gp(cx.use_reg(addr)?);
    put(cx, mc::jmp_r(r));
    Ok(())
}

/// Puts the address of block `block` of function `func` in `rd` (module
/// doc); flags kept.
///
/// # Errors
/// [`BLOCKADDRESS`] for a block of another function (`classify` rejects
/// it first); what the fixup reports.
pub fn block_address(cx: &mut FnCx<'_, X86>, rd: Reg, func: SymId, block: BlockId) -> CgResult<()> {
    if func != cx.body.symbol() || block.index() >= cx.body.block_count() {
        return unsupported(BLOCKADDRESS);
    }
    put(cx, mc::call(0));
    put(cx, mc::pop_r(rd));
    let Some(m) = Mem::base(rd, 0).with_abs32() else {
        return cx.bug("a base register without a disp32 form");
    };
    let inst = mc::lea(Size::D, rd, m);
    let Some(site) = Mode::X86.admit(inst).ok().and_then(|i| i.site()) else {
        return cx.bug("a lea without a displacement field");
    };
    let at = cx.offset() + site.offset as u32;
    put(cx, inst);
    cx.fixup(at, REL32, cx.block_label(block))?;
    put(cx, mc::lea(Size::D, rd, Mem::base(rd, FIELD_END)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_ends_seven_bytes_past_the_pushed_address() {
        for rd in [Reg::Rax, Reg::Rcx, Reg::Rdx, Reg::Rbx, Reg::Rsi, Reg::Rdi] {
            let pop = Mode::X86.admit(mc::pop_r(rd)).unwrap();
            let m = Mem::base(rd, 0).with_abs32().unwrap();
            assert_eq!(Mode::X86.admit(mc::call(0)).unwrap().bytes().len(), 5);
            let lea = Mode::X86.admit(mc::lea(Size::D, rd, m)).unwrap();
            let site = lea.site().unwrap();
            assert_eq!(site.offset as usize + 4, lea.bytes().len(), "{rd:?}");
            assert_eq!(pop.bytes().len() + lea.bytes().len(), FIELD_END as usize, "{rd:?}");
        }
    }
}
