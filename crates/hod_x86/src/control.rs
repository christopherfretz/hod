//! Terminators (`spec/codegen-x86.md` "Control flow"): `br`, `condbr`,
//! `switch`, `ret`, `unreachable`. Owned by WP-293.
//!
//! A `switch` is a chain of `cmp`/`je` in case order, then the default:
//! hod does not build tables or trees.

use hod_cg::{CgResult, Class, FnCx};
use hod_ir::{BlockId, Const, Switch, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Size};

use crate::alu::{Ext, ext_const, extend_to};
use crate::common::{branch, branch_cond, const_int, gp, put, unsupported, width};
use crate::target::X86;

/// `blockaddress`.
pub const BLOCKADDRESS: &str = "x86:blockaddress";
/// `indirectbr`.
pub const INDIRECT_BRANCH: &str = "x86:indirect-branch";

/// `br label %target`: nothing when the target is emitted next.
///
/// # Errors
/// What the branch's fixup reports.
pub fn br(cx: &mut FnCx<'_, X86>, target: BlockId) -> CgResult<()> {
    if cx.falls_through(target) {
        return Ok(());
    }
    branch(cx, cx.block_label(target))
}

/// `jcc if_true` then `br if_false`, or the inverse when `if_true` is
/// next.
fn branch_on(
    cx: &mut FnCx<'_, X86>,
    cond: Cond,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    if cx.falls_through(if_true) {
        let label = cx.block_label(if_false);
        return branch_cond(cx, cond.invert(), label);
    }
    let label = cx.block_label(if_true);
    branch_cond(cx, cond, label)?;
    br(cx, if_false)
}

/// `br i1 %cond, label %if_true, label %if_false`: `test r, 1`.
///
/// # Errors
/// What placing the condition and the branches report.
pub fn cond_br(
    cx: &mut FnCx<'_, X86>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    if let Some(c) = const_int(cx, cond) {
        return br(cx, if c & 1 != 0 { if_true } else { if_false });
    }
    if if_true == if_false {
        return br(cx, if_true);
    }
    let r = gp(cx.use_reg(cond)?);
    put(cx, mc::test_ri(Size::D, r, 1));
    branch_on(cx, Cond::Ne, if_true, if_false)
}

/// `switch`: the condition zero-extended to 32 bits, one `cmp`·`je` per
/// case, then the default.
///
/// # Errors
/// The type refusal of a condition that is no `Gpr` value; what the
/// branches report.
pub fn switch<'a>(cx: &mut FnCx<'a, X86>, switch: &Switch<'a>) -> CgResult<()> {
    if crate::int::is_wide(cx, switch.cond) {
        return crate::int::switch(cx, switch);
    }
    if cx.class_of(switch.cond) != Class::Gpr {
        let ty = crate::common::value_ty(cx, switch.cond);
        return Err(crate::target::refusal_of(cx, ty));
    }
    let bits = width(cx, switch.cond);
    let case_value = |cx: &FnCx<'a, X86>, id| match cx.module.consts.get(id) {
        Const::Int { value, .. } => ext_const(value as u32, bits, Ext::Zero),
        _ => 0,
    };
    if let Some(c) = const_int(cx, switch.cond) {
        let c = ext_const(c, bits, Ext::Zero);
        let target = switch
            .cases()
            .find(|&(case, _)| case_value(cx, case) == c)
            .map_or(switch.default, |(_, block)| block);
        return br(cx, target);
    }
    let r = gp(cx.use_reg(switch.cond)?);
    extend_to(cx, r, bits, Ext::Zero);
    for (case, block) in switch.cases() {
        let value = case_value(cx, case);
        put(cx, mc::cmp_ri(Size::D, r, i64::from(value as i32)));
        let label = cx.block_label(block);
        branch_cond(cx, Cond::E, label)?;
    }
    br(cx, switch.default)
}

/// `ret`: the value to `eax` (`FnCx::place_return`), then to the
/// epilogue unless it comes next.
///
/// # Errors
/// What placing the value or the branch reports.
pub fn ret(cx: &mut FnCx<'_, X86>, value: Option<Value>) -> CgResult<()> {
    if let Some(value) = value {
        if cx.fn_assignment().ret_memory.is_some() {
            return unsupported(crate::target::AGGREGATE);
        }
        cx.place_return(value)?;
    }
    if cx.ret_falls_through() {
        return Ok(());
    }
    branch(cx, cx.epilogue())
}

/// `unreachable`: `ud2`.
///
/// # Errors
/// None; a `Result` for the dispatch.
#[allow(clippy::unnecessary_wraps, reason = "every entry point of the dispatch returns CgResult")]
pub fn unreachable(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    cx.trap();
    Ok(())
}
