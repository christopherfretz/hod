//! Named registers (WP-064, `spec/codegen.md` §5.5.1 "WP-064"):
//! `llvm.read_register`, `llvm.read_volatile_register` and
//! `llvm.write_register` of an `i64`, whose register is named by a
//! metadata operand `!{!"name"}` (`spec/ir.md` §5.3), lowered as
//! `llc -O0 -global-isel=0 -fast-isel=0` of LLVM 19 and 22 lowers them.
//!
//! - `sp`, `x29` and `x30` (exactly so spelled; `llc` refuses `SP`, `fp`,
//!   `lr`), read: `mov xd, sp` (`add xd, sp, #0`) or `mov xd, x29`.
//! - A system register, read with `mrs` and written with `msr`: by name
//!   from [`SYSREGS`] (any case, as `llc`), or as
//!   `op0:op1:CRn:CRm:op2` in decimal with `op0` 2 or 3 and every field in
//!   range (`llc` takes any five numbers and encodes garbage outside
//!   them). A write of a register [`SYSREGS`] marks read-only is refused,
//!   as `llc` 22 refuses it (`llc` 19 aborts).
//!
//! Refused by name: [`REGISTER`] for any other name or form (the
//! general registers `llc` takes only when reserved, `x0` and `xzr`
//! whose reads mean nothing, the `w` names `llc` misreads as `NZCV`, a
//! system register outside the table), [`WIDTH`] for another type than
//! `i64` (`llc` emits an unencodable `mrs w0`), [`WRITE_GPR`] for a write
//! of `sp`, `x29` or `x30` (it breaks the frame `llc` and hod address
//! slots through).
//!
//! A body that reads `sp`, `x29` or `x30` means something else inlined;
//! the facade declines it in a module that may still inline it
//! (`crates/hod/src/session/inlining.rs`, WP-064).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, TypeId, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{RegSp, Size};

use crate::common::{deferred_first, put, unsupported, xr};
use crate::target::A64;

/// A register name this target does not read or write by name.
pub const REGISTER: &str = "a64:named-register";
/// A named-register intrinsic of another type than `i64`.
pub const WIDTH: &str = "a64:named-register-width";
/// `llvm.write_register` of `sp`, `x29` or `x30`.
pub const WRITE_GPR: &str = "a64:write-register-gpr";

/// The system registers read and written by name: the name (upper case),
/// `op0:op1:CRn:CRm:op2`, and whether `msr` may write it. Each is checked
/// against `llc` 19 and 22 (`tests/named_reg.rs`).
pub const SYSREGS: &[(&str, [u16; 5], bool)] = &[
    ("CNTFRQ_EL0", [3, 3, 14, 0, 0], true),
    ("CNTPCT_EL0", [3, 3, 14, 0, 1], false),
    ("CNTVCT_EL0", [3, 3, 14, 0, 2], false),
    ("CNTV_CTL_EL0", [3, 3, 14, 3, 1], true),
    ("CNTV_CVAL_EL0", [3, 3, 14, 3, 2], true),
    ("CTR_EL0", [3, 3, 0, 0, 1], false),
    ("DAIF", [3, 3, 4, 2, 1], true),
    ("DCZID_EL0", [3, 3, 0, 0, 7], false),
    ("FPCR", [3, 3, 4, 4, 0], true),
    ("FPSR", [3, 3, 4, 4, 1], true),
    ("MIDR_EL1", [3, 0, 0, 0, 0], false),
    ("MPIDR_EL1", [3, 0, 0, 0, 5], false),
    ("NZCV", [3, 3, 4, 2, 0], true),
    ("REVIDR_EL1", [3, 0, 0, 0, 6], false),
    ("TPIDRRO_EL0", [3, 3, 13, 0, 3], true),
    ("TPIDR_EL0", [3, 3, 13, 0, 2], true),
];

/// What a register name names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    /// `sp`, or general register `x29`/`x30` by number.
    Gpr(u8),
    /// A system register: its 15-bit `o0:op1:CRn:CRm:op2` field, and
    /// whether it may be written.
    Sys(u16, bool),
}

/// What `name` names, `None` for a name this target refuses.
pub fn named(name: &str) -> Option<Named> {
    match name {
        "sp" => return Some(Named::Gpr(31)),
        "x29" => return Some(Named::Gpr(29)),
        "x30" => return Some(Named::Gpr(30)),
        _ => {}
    }
    let field = |[op0, op1, crn, crm, op2]: [u16; 5]| {
        (op0 - 2) << 14 | op1 << 11 | crn << 7 | crm << 3 | op2
    };
    if let Some(&(_, ops, writable)) =
        SYSREGS.iter().find(|(known, ..)| known.eq_ignore_ascii_case(name))
    {
        return Some(Named::Sys(field(ops), writable));
    }
    let mut ops = [0u16; 5];
    let mut parts = name.split(':');
    for (op, limit) in ops.iter_mut().zip([4u16, 8, 16, 16, 8]) {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *op = part.parse().ok().filter(|&value| value < limit)?;
    }
    if parts.next().is_some() || ops[0] < 2 {
        return None;
    }
    Some(Named::Sys(field(ops), true))
}

/// `llvm.read_register`, `llvm.read_volatile_register` (`which` reads)
/// or `llvm.write_register` with arguments `args`.
///
/// # Errors
/// [`REGISTER`], [`WIDTH`], [`WRITE_GPR`]; what placing the operands
/// reports.
pub fn lower<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let write = which == Intrinsic::WriteRegister;
    let ty = if write {
        args.get(1).map_or(TypeId::VOID, |&value| cx.body.value_type(cx.module, value))
    } else {
        inst.ty()
    };
    if ty != TypeId::I64 {
        return unsupported(WIDTH);
    }
    let name = args
        .first()
        .and_then(|value| value.as_meta())
        .and_then(|meta| cx.body.metadata(meta))
        .and_then(|meta| meta.single_node_string());
    let Some(named) = name.and_then(named) else { return unsupported(REGISTER) };
    deferred_first(cx, inst, None)?;
    match (named, write, args) {
        (Named::Gpr(_), true, _) => unsupported(WRITE_GPR),
        (Named::Sys(_, false), true, _) => unsupported(REGISTER),
        (Named::Sys(field, _), true, &[_, value]) => {
            let rt = xr(cx.use_reg(value)?);
            put(cx, 0xd510_0000 | u32::from(field) << 5 | rt.zr().code());
            Ok(())
        }
        (Named::Gpr(31), false, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_sp(Size::X, xr(rd).sp(), RegSp::SP));
            cx.finish(inst, rd)
        }
        (Named::Gpr(number), false, _) => {
            let rd = cx.def_reg(inst)?;
            let rn = mc::Reg::new(number).expect("x29 or x30");
            put(cx, mc::mov(Size::X, xr(rd).zr(), rn.zr()));
            cx.finish(inst, rd)
        }
        (Named::Sys(field, _), false, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, 0xd530_0000 | u32::from(field) << 5 | xr(rd).zr().code());
            cx.finish(inst, rd)
        }
        (Named::Sys(..), true, _) => unsupported(REGISTER),
    }
}
