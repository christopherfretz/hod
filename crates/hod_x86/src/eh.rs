//! Exceptions (`spec/codegen-x86.md` §12, WP-314, design 0008 I10):
//! `landingpad`, `resume`, `llvm.eh.typeid.for`, and the `extractvalue` /
//! `insertvalue` of a landing pad's `{ ptr, i32 }` value.
//!
//! An `invoke` is `hod_cg`'s call sequence (`FnCx::call`): it records the
//! call site, the pad and the normal edge, and honours the constant
//! fold's rewrites of `spec/codegen.md` §5.6.8 (a call without its unwind
//! edge, or an `invoke` that keeps only its unwind edge). `hod_cg` writes
//! the tables from the rows (`hod_eh::I386`); this module lowers the
//! instructions that read what the unwinder hands over.
//!
//! The unwinder enters a landing pad with the exception pointer in `eax`
//! and the selector in `edx` (`RegInfo::eh_data`, as `llc` 19 and 22
//! read them: `movl %eax, ...; movl %edx, ...` at the pad's first
//! instruction). The pad's value `{ ptr, i32 }` is class `Mem`: an 8-byte
//! home, the pointer at 0 and the selector at 4. `eax` never holds a
//! value across instructions and `edx` is scratch (§2), so both are still
//! the unwinder's when the pad's first instruction runs.

use hod_cg::{CallArg, CgResult, FnCx, RuntimeFn};
use hod_ir::{Clause, Const, ConstId, Inst, LandingPad, TypeId, TypeKind, Value, ValueKind};

use crate::common::{gp, load_gpr, mov_imm, store_gpr, unsupported, value_ty};
use crate::reg::{EAX, EDX};
use crate::target::X86;

/// A `landingpad` whose type is not `{ ptr, i32 }`.
pub const LANDING_PAD: &str = "x86:eh:landingpad";
/// A `landingpad` clause whose type information is not a symbol's address
/// or `null`, or a `filter` that is not an array of them.
pub const LANDING_PAD_CLAUSE: &str = "x86:eh:landingpad-clause";
/// `resume` of a value that is not `{ ptr, i32 }`.
pub const RESUME: &str = "x86:eh:resume";
/// `llvm.eh.typeid.for` of an operand that is not a symbol.
pub const TYPEID: &str = "x86:eh:typeid";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[LANDING_PAD, LANDING_PAD_CLAUSE, RESUME, TYPEID];

/// Whether `ty` is `{ ptr, i32 }`, the value of a landing pad.
pub fn is_pad_type(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    matches!(
        cx.types().kind(ty),
        TypeKind::Struct { fields: [ptr, TypeId::I32], packed: false }
            if matches!(cx.types().kind(*ptr), TypeKind::Ptr { addr_space: 0 })
    )
}

/// Whether `inst` is an `extractvalue` or `insertvalue` of a landing
/// pad's value, which this module lowers ([`extract_value`],
/// [`insert_value`]); every other aggregate is `x86:aggregate`'s.
pub fn owns_aggregate(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    match inst.kind() {
        hod_ir::InstKind::ExtractValue { agg, .. } => is_pad_type(cx, value_ty(cx, agg)),
        hod_ir::InstKind::InsertValue { .. } => is_pad_type(cx, inst.ty()),
        _ => false,
    }
}

/// The type-information symbol of a `catch` clause or `filter` element:
/// `None` for `null` (catch-all).
fn type_info(cx: &FnCx<'_, X86>, id: ConstId) -> CgResult<Option<hod_ir::SymId>> {
    match cx.module.consts.get(id) {
        Const::Null(_) => Ok(None),
        Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
        _ => unsupported(LANDING_PAD_CLAUSE),
    }
}

/// `%r = landingpad { ptr, i32 } [cleanup] [clauses]` (`spec/codegen.md`
/// §4.8, §9.4): the clauses go to the pad's row in order; `eax` and `edx`
/// are stored into the result's home.
///
/// # Errors
/// [`LANDING_PAD`], [`LANDING_PAD_CLAUSE`].
pub fn landing_pad<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pad: &LandingPad<'a>,
) -> CgResult<()> {
    if !is_pad_type(cx, inst.ty()) {
        return unsupported(LANDING_PAD);
    }
    for &clause in pad.clauses() {
        match clause {
            Clause::Catch(id) => {
                let sym = type_info(cx, id)?;
                cx.pad_clause(sym)?;
            }
            Clause::Filter(id) => {
                let elems: &[ConstId] = match cx.module.consts.get(id) {
                    Const::Aggregate { elems, .. } => elems,
                    Const::Zero(ty) if cx.types().alloc_size_of(ty) == 0 => &[],
                    _ => return unsupported(LANDING_PAD_CLAUSE),
                };
                let syms = elems.iter().map(|&e| type_info(cx, e)).collect::<CgResult<Vec<_>>>()?;
                cx.pad_filter(&syms)?;
            }
        }
    }
    let home = cx.def_mem(inst)?;
    store_gpr(cx, gp(EAX), home, 4);
    store_gpr(cx, gp(EDX), home.offset_by(4), 4);
    Ok(())
}

/// `resume { ptr, i32 } %value`: `_Unwind_Resume` of the exception
/// pointer through the call sequence (a call-site row, `@PLT` with `ebx`
/// under PIC), then the trap.
///
/// # Errors
/// [`RESUME`]; what the call sequence reports.
pub fn resume(cx: &mut FnCx<'_, X86>, value: Value) -> CgResult<()> {
    if !is_pad_type(cx, value_ty(cx, value)) {
        return unsupported(RESUME);
    }
    let from = cx.use_mem(value)?;
    let exception = cx.temp(hod_cg::Class::Gpr)?;
    load_gpr(cx, gp(exception), from, 4);
    cx.call_runtime(RuntimeFn::UnwindResume, &[CallArg::Reg(exception)], None)?;
    cx.trap();
    Ok(())
}

/// `%r = extractvalue { ptr, i32 } %agg, 0|1`: a load of the field.
///
/// # Errors
/// What the operand reports.
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    let from = cx.use_mem(agg)?;
    let rd = cx.def_reg(inst)?;
    load_gpr(cx, gp(rd), from.offset_by(offset as u32), 4);
    cx.finish(inst, rd)
}

/// `%r = insertvalue { ptr, i32 } %agg, %value, 0|1`: the aggregate
/// copied (`poison` and `undef` copy nothing, `zeroinitializer` writes
/// zeros), then the field stored.
///
/// # Errors
/// `x86:aggregate` for another constant aggregate; what the operands
/// report.
pub fn insert_value<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    let src = cx.use_reg(value)?;
    let home = cx.def_mem(inst)?;
    if let ValueKind::Const(id) = agg.kind() {
        match cx.module.consts.get(id) {
            Const::Poison(_) | Const::Undef(_) => {}
            Const::Zero(_) => {
                let other = home.offset_by(4 - offset as u32);
                mov_imm(cx, gp(EAX), 0);
                store_gpr(cx, gp(EAX), other, 4);
            }
            _ => return unsupported(crate::target::AGGREGATE),
        }
    } else {
        let from = cx.use_mem(agg)?;
        if from != home {
            crate::common::copy_frame(cx, home, from, 8);
        }
    }
    store_gpr(cx, gp(src), home.offset_by(offset as u32), 4);
    Ok(())
}

/// `llvm.eh.typeid.for(ptr @sym)`: the type id of `sym` in the
/// function's table, as an `i32` constant.
///
/// # Errors
/// [`TYPEID`] for an operand that is not a symbol.
pub fn typeid_for<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, info: Value) -> CgResult<()> {
    let ValueKind::Sym(sym) = info.kind() else { return unsupported(TYPEID) };
    let id = cx.type_id(Some(sym));
    let rd = cx.def_reg(inst)?;
    mov_imm(cx, gp(rd), id);
    cx.finish(inst, rd)
}
