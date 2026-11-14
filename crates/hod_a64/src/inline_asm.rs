//! Inline assembly with a template of instructions (`spec/asm.md` §6,
//! `spec/codegen.md` §5.5.3 "WP-103"): the operands are placed as the
//! constraints ask, the template is substituted as LLVM's `AsmPrinter`
//! does and assembled by `hod_asm` (every word is one hod parsed and
//! encoded), and the outputs are taken back.
//!
//! **Owner: WP-103.** Tests: `tests/inline_asm.rs`. Hook: `call::call`
//! (every call, `invoke` and `callbr` of inline assembly comes here; an
//! empty template goes to `crate::barrier` first).
//!
//! # The sequence
//!
//! 1. Every input (and every pointer of an indirect operand) is
//!    evaluated and stored to a 16-byte slot of its own, so that the
//!    placing below reads only the frame.
//! 2. Every register value is displaced; every register the template
//!    may write (operand registers, clobbers) is taken; the callee-saved
//!    ones among them (`x19`..`x28`, the low halves of `v8`..`v15`) are
//!    saved to slots.
//! 3. The inputs are loaded into their registers (an integer narrower
//!    than 64 bits zero-extended), the memory operands' addresses
//!    computed into theirs.
//! 4. The template's words.
//! 5. The register outputs are stored to their slots (an indirect one
//!    through its pointer), the flag outputs (`={@cc<cond>}`) are
//!    `cset` and stored, the callee-saved registers restored.
//! 6. The result is loaded from the output slots: one scalar, or the
//!    fields of the struct of several. A `callbr` does 5 and 6 again at
//!    each indirect destination's trampoline (WP-112).
//!
//! # Registers
//!
//! Each operand gets a register of its own, distinct from every other
//! operand's and every clobber (always a valid choice: an output that
//! `llc` would share with an input is in a register of its own here),
//! except a tied input, which is in its output's register. General
//! registers are taken from `x9`..`x15`, `x0`..`x8`; vector registers
//! from `v16`..`v29`, `v0`..`v7`. An explicit register (`{x0}`) is that
//! register; the frame registers (`sp`, `x29`) are refused
//! ([`FRAME_REGISTER`]), as are the scratch registers hod's own code
//! around the template uses (`x16`, `x17`, `v30`, `v31`:
//! [`SCRATCH_REGISTER`]).
//!
//! # Constraint choice
//!
//! Of several alternatives (`r|m`) the one `llc` takes by weights, as in
//! `crate::barrier`. Within one alternative: an explicit register; a
//! tied input's output register; an immediate (`i`, `n`, `I`..`N`) for a
//! constant integer, a symbol for `i` and a symbol's address; memory
//! (`m`, `Q`, `o`) when the alternative has it and the operand is not a
//! tied output; else a register: `w`, `x`, `y` vector registers, `r`
//! and `X` (and anything with only memory or immediates left) general
//! registers, or vector ones for a floating-point or vector `X`.

use hod_asm::a64::{self as asm, Arg, Features};
use hod_asm::{Refusal, template};
use hod_cg::asm_constraints::{
    self as constraints, Clobber, ConstraintError, ConstraintSyntax, FLAG, Kind, MAX_OPS, SPEC,
    TIED,
};
use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef, PReg, SymRef};
use hod_ir::{
    BlockId, Const, FnStr, InlineAsm, Inst, Opcode, SymbolKind, TypeId, TypeKind, Value, ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Size, VMemSize};
use hod_obj::{A64Reloc, RelocKind};

use crate::common::{
    BRANCH19, Cursor, JUMP26, TEST14, X16, X17, branch, copy_bytes, deferred_first, fpr_view,
    frame_address, gpr_load_op, load_fpr_at, load_gpr_at, put, store_fpr_at, store_fpr_bytes,
    store_gpr_at, store_gpr_bytes, unsupported, value_ty, vr, xr,
};
use crate::reg;
use crate::target::A64;

/// An operand in `sp` or the frame pointer (`x29`, `fp`), or a clobber
/// of either: hod's frame is addressed from them.
pub const FRAME_REGISTER: &str = "a64:inline-asm-frame-register";
/// An operand in `x16`, `x17`, `v30` or `v31`: the scratch registers of
/// the sequence around the template.
pub const SCRATCH_REGISTER: &str = "a64:inline-asm-scratch-register";
/// An operand of a type its register class does not hold: an aggregate,
/// an integer wider than 64 bits, a vector wider than 16 bytes, a value
/// in a general register wider than 8 bytes.
pub const OPERAND_TYPE: &str = "a64:inline-asm-operand-type";
/// More operands, alternatives or registers than are kept (16 operands,
/// 4 alternatives, the registers of a class).
pub const TOO_MANY: &str = "a64:inline-asm-operands";
/// `unwind` inline assembly called by `invoke`: hod records no call
/// site for the template.
pub const UNWIND: &str = "a64:inline-asm-unwind";
/// An operand bundle on a call of inline assembly.
pub const FORM: &str = "a64:inline-asm-form";

/// Every name this module refuses with, besides `hod_asm`'s `asm:*`.
pub const UNSUPPORTED: &[&str] =
    &[FRAME_REGISTER, SCRATCH_REGISTER, OPERAND_TYPE, TOO_MANY, UNWIND, FORM];

/// A `hod_asm` refusal as a codegen refusal.
fn refused(r: Refusal) -> CgError {
    CgError::unsupported(r.0)
}

// ---- constraints ----

// The constraint string is read by `hod_cg::asm_constraints`; the codes
// of the letters, the register names and the clobbers are AArch64's
// (`A64Syntax`).

/// Codes of one alternative, as bits (`SPEC`, `TIED` and `FLAG` are the
/// shared parser's).
const R: u32 = 1 << 0; // r, X (general)
const W: u32 = 1 << 1; // w, x, y
const M: u32 = 1 << 2; // m, Q, o
const I: u32 = 1 << 3; // i, n, I..N, s
const XX: u32 = 1 << 4; // X
const LOW: u32 = 1 << 8; // x: v0..v15
const LOWEST: u32 = 1 << 9; // y: v0..v7

/// One alternative of an operand.
type Alt = constraints::Alt<mc::Cond>;
/// One operand.
type Op = constraints::Op<mc::Cond>;
/// The parsed constraints.
type Ops = constraints::Constraints<mc::Cond>;

/// What an operand became.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    /// A register.
    Reg(PReg),
    /// Memory whose address is in this general register.
    Mem(PReg),
    /// A constant.
    Imm(i64),
    /// A symbol's address.
    Sym(hod_ir::SymId),
    /// A flag output.
    Flag(mc::Cond),
    /// An `asm goto` target: the index of the indirect destination.
    Label(u8),
}

/// The register a name inside braces denotes (`x0`, `w0`, `v0`, `q0`,
/// `d0`, ..., `fp`, `lr`, `sp`); `Err` for a frame register.
fn named_reg(name: &[u8]) -> Result<Option<PReg>, ()> {
    match name {
        b"fp" | b"x29" | b"w29" | b"sp" | b"wsp" => return Err(()),
        b"lr" => return Ok(Some(reg::x(30))),
        _ => {}
    }
    let Some((&class, digits)) = name.split_first() else { return Ok(None) };
    let well_formed = !digits.is_empty()
        && digits.len() <= 2
        && digits.iter().all(u8::is_ascii_digit)
        && (digits.len() == 1 || digits[0] != b'0');
    if !well_formed {
        return Ok(None);
    }
    let n = digits.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0')) as u8;
    Ok(match class {
        b'x' | b'w' if n <= 30 => Some(reg::x(n)),
        b'v' | b'q' | b'd' | b's' | b'h' | b'b' if n <= 31 => Some(reg::v(n)),
        _ => None,
    })
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or("other")
}

/// AArch64's part of a constraint string.
struct A64Syntax;

impl ConstraintSyntax for A64Syntax {
    type Cond = mc::Cond;

    fn letters(&self, code: &[u8], _output: bool) -> Option<(u32, usize)> {
        let bits = match code.first()? {
            b'r' => R,
            b'w' => W,
            b'x' => W | LOW,
            b'y' => W | LOWEST,
            b'm' | b'Q' | b'o' => M,
            b'i' | b'n' | b's' | b'I' | b'J' | b'K' | b'L' | b'M' | b'N' => I,
            b'X' => XX,
            _ => return None,
        };
        Some((bits, 1))
    }

    fn flag(&self, cond: &[u8]) -> Option<mc::Cond> {
        asm::encode::cond_name(text(cond))
    }

    fn operand_reg(&self, name: &[u8]) -> Result<Option<PReg>, CgError> {
        match named_reg(name) {
            Err(()) => Err(CgError::unsupported(FRAME_REGISTER)),
            Ok(Some(r))
                if matches!(reg::number(r), 16 | 17) && reg::is_gpr(r)
                    || matches!(reg::number(r), 30 | 31) && reg::is_fpr(r) =>
            {
                Err(CgError::unsupported(SCRATCH_REGISTER))
            }
            Ok(r) => Ok(r),
        }
    }

    fn clobber(&self, name: &[u8]) -> Result<Clobber, CgError> {
        if let b"cc" | b"nzcv" = name {
            return Ok(Clobber::Ignored);
        }
        match named_reg(name) {
            Err(()) => Err(CgError::unsupported(FRAME_REGISTER)),
            Ok(None) => Ok(Clobber::Unknown),
            Ok(Some(r)) => Ok(Clobber::Reg(r)),
        }
    }
}

/// Reads the constraint string `constraints` (`hod_cg::asm_constraints`).
fn parse(constraints: &[u8]) -> CgResult<Ops> {
    constraints::parse(&A64Syntax, constraints).map_err(|e| match e {
        ConstraintError::Constraint(t) => refused(hod_asm::reason::constraint(text(t))),
        ConstraintError::Letter(c) => {
            refused(hod_asm::reason::constraint(&char::from(c).to_string()))
        }
        ConstraintError::Clobber(t) => refused(hod_asm::reason::clobber(text(t))),
        ConstraintError::TooMany => CgError::unsupported(TOO_MANY),
        ConstraintError::Target(e) => e,
    })
}

// ---- choosing ----

fn is_const_int(cx: &FnCx<'_, A64>, value: Value) -> Option<i64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => {
            let TypeKind::Int(bits) = cx.types().kind(ty) else { return None };
            Some(hod_ir::sext64(value, bits))
        }
        _ => None,
    }
}

fn is_fp_type(cx: &FnCx<'_, A64>, ty: TypeId) -> bool {
    matches!(
        cx.types().kind(ty),
        TypeKind::Half
            | TypeKind::BFloat
            | TypeKind::Float
            | TypeKind::Double
            | TypeKind::Fp128
            | TypeKind::Vector { .. }
    )
}

/// LLVM's weight of one code for an operand (as `crate::barrier`).
fn code_weight(cx: &FnCx<'_, A64>, operand: Option<Value>, code: u32) -> Option<i32> {
    let Some(value) = operand else { return Some(0) };
    let ty = value_ty(cx, value);
    match code {
        I => is_const_int(cx, value).map(|_| 3),
        M => Some(2),
        R => matches!(cx.types().kind(ty), TypeKind::Int(_)).then_some(1),
        W => is_fp_type(cx, ty).then_some(1),
        _ => Some(0),
    }
}

fn alt_of(op: &Op, index: usize) -> Alt {
    op.alt(index)
}

/// The alternative `llc` takes: the first of the greatest total weight
/// (`Constraints::choose`); an alternative weighs what its best code
/// weighs.
fn choose(cx: &FnCx<'_, A64>, ops: &Ops, args: &[Value]) -> usize {
    ops.choose(args, |operand, codes| {
        let mut w = None;
        for code in [R, M, I, W, XX, SPEC, TIED, FLAG] {
            if codes & code != 0 {
                w = w.max(code_weight(cx, operand, code));
            }
        }
        w
    })
}

const GPR_ORDER: [u8; 16] = [9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8];
const FPR_ORDER: [u8; 22] =
    [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 0, 1, 2, 3, 4, 5, 6, 7];

/// What the lowering decided for each operand.
struct Plan {
    places: [Option<Place>; MAX_OPS],
    /// The type an operand's register or memory holds: the value's type,
    /// an indirect operand's element type, a direct output's field type.
    tys: [TypeId; MAX_OPS],
    /// The slot of each operand: an input's value or pointer, an
    /// output's result.
    slots: [Option<FrameRef>; MAX_OPS],
    /// Every register the template may write.
    written: u64,
}

fn reg_set_has(set: u64, r: PReg) -> bool {
    set & (1 << r.index()) != 0
}

/// The type of each operand's register or memory.
fn operand_types(
    cx: &FnCx<'_, A64>,
    inst: Inst<'_>,
    site: hod_ir::CallSite<'_>,
    ops: &Ops,
) -> CgResult<[TypeId; MAX_OPS]> {
    let mut tys = [TypeId::VOID; MAX_OPS];
    let args = site.args();
    for (i, op) in ops.list[..ops.len].iter().enumerate() {
        tys[i] = match (op.kind, op.indirect, op.arg, op.field) {
            (Kind::Label, ..) => TypeId::VOID,
            (_, true, Some(a), _) => {
                // An indirect operand's register holds its element.
                site.arg_attrs(usize::from(a)).mem_type().unwrap_or(TypeId::VOID)
            }
            (_, false, Some(a), _) => value_ty(cx, args[usize::from(a)]),
            (Kind::Output, false, None, Some(f)) => {
                if ops.direct_outputs == 1 {
                    inst.ty()
                } else {
                    match cx.types().struct_fields(inst.ty()) {
                        Some(fields) if fields.len() == usize::from(ops.direct_outputs) => {
                            fields[usize::from(f)]
                        }
                        _ => return unsupported(OPERAND_TYPE),
                    }
                }
            }
            _ => return cx.bug("an inline-assembly operand without a type"),
        };
    }
    Ok(tys)
}

/// Whether a value of `ty` fits a register of `class` (by bytes): a
/// general register holds up to 8 bytes, a vector one up to 16; neither
/// holds an aggregate or a wide integer.
fn fits(cx: &FnCx<'_, A64>, ty: TypeId, fpr: bool) -> bool {
    let bytes = cx.types().alloc_size_of(ty);
    let scalar = match cx.types().kind(ty) {
        TypeKind::Int(bits) => bits <= 64,
        TypeKind::Ptr { addr_space: 0 }
        | TypeKind::Half
        | TypeKind::BFloat
        | TypeKind::Float
        | TypeKind::Double => true,
        TypeKind::Fp128 | TypeKind::Vector { .. } => fpr,
        _ => false,
    };
    scalar && bytes >= 1 && bytes <= if fpr { 16 } else { 8 }
}

#[allow(clippy::too_many_lines, reason = "the choice of the module documentation, in order")]
fn decide(cx: &FnCx<'_, A64>, inst: Inst<'_>, ops: &Ops) -> CgResult<Plan> {
    let site = inst.call().expect("checked by the caller");
    let args = site.args();
    let chosen = choose(cx, ops, args);
    let tys = operand_types(cx, inst, site, ops)?;
    let mut plan =
        Plan { places: [None; MAX_OPS], tys, slots: [None; MAX_OPS], written: ops.clobbers };
    // Which outputs an input is tied to.
    let mut tied_by = [false; MAX_OPS];
    for op in &ops.list[..ops.len] {
        let alt = alt_of(op, chosen);
        if op.kind == Kind::Input && alt.codes & TIED != 0 {
            tied_by[usize::from(alt.tied)] = true;
        }
    }
    // Registers named explicitly or clobbered are not chosen.
    let mut taken = ops.clobbers;
    for op in &ops.list[..ops.len] {
        if let Some(r) = alt_of(op, chosen).reg {
            taken |= 1 << r.index();
        }
    }
    let mut label = 0u8;
    // First the explicit, immediate, memory and label places; then the
    // registers in order; ties last.
    let mut wants = [None::<bool>; MAX_OPS]; // Some(fpr) for a register to choose
    for (i, op) in ops.list[..ops.len].iter().enumerate() {
        let alt = alt_of(op, chosen);
        let value = op.arg.map(|a| args[usize::from(a)]);
        let direct_value = value.filter(|_| !op.indirect);
        let place = if op.kind == Kind::Label {
            label += 1;
            Some(Place::Label(label - 1))
        } else if let Some(cond) = alt.cond {
            Some(Place::Flag(cond))
        } else if let Some(r) = alt.reg {
            Some(Place::Reg(r))
        } else if alt.codes & TIED != 0 && op.kind == Kind::Input {
            None // its output's register, below
        } else if let Some(c) =
            direct_value.and_then(|v| is_const_int(cx, v)).filter(|_| alt.codes & I != 0)
        {
            Some(Place::Imm(c))
        } else if let Some(ValueKind::Sym(s)) =
            direct_value.map(Value::kind).filter(|_| alt.codes & I != 0)
        {
            Some(Place::Sym(s))
        } else if alt.codes & M != 0
            && !(op.kind == Kind::Output && tied_by[i] && alt.codes & (R | W | XX) != 0)
        {
            // An address register, chosen below.
            wants[i] = Some(false);
            None
        } else if alt.codes & (R | W | XX) != 0 {
            let fpr =
                alt.codes & W != 0 || (alt.codes & (XX | R) == XX && is_fp_type(cx, plan.tys[i]));
            wants[i] = Some(fpr);
            None
        } else if alt.codes & I != 0 {
            // An immediate constraint on something that is no constant
            // integer or symbol: an `llc` error.
            return Err(refused(hod_asm::reason::constraint("i")));
        } else {
            return Err(refused(hod_asm::reason::constraint("other")));
        };
        plan.places[i] = place;
    }
    for i in 0..ops.len {
        let Some(fpr) = wants[i] else { continue };
        let op = &ops.list[i];
        let alt = alt_of(op, chosen);
        let memory = alt.codes & M != 0
            && !(op.kind == Kind::Output && tied_by[i] && alt.codes & (R | W | XX) != 0);
        let mask = if alt.codes & LOWEST != 0 {
            0xff
        } else if alt.codes & LOW != 0 {
            0xffff
        } else {
            u32::MAX
        };
        let pick = |order: &[u8], make: fn(u8) -> PReg| {
            order
                .iter()
                .map(|&n| (n, make(n)))
                .find(|&(n, r)| (mask >> n) & 1 != 0 && !reg_set_has(taken, r))
                .map(|(_, r)| r)
        };
        let r = if fpr && !memory { pick(&FPR_ORDER, reg::v) } else { pick(&GPR_ORDER, reg::x) };
        let Some(r) = r else { return unsupported(TOO_MANY) };
        taken |= 1 << r.index();
        plan.places[i] = Some(if memory { Place::Mem(r) } else { Place::Reg(r) });
    }
    for (i, op) in ops.list[..ops.len].iter().enumerate() {
        let alt = alt_of(op, chosen);
        if op.kind == Kind::Input && alt.codes & TIED != 0 && plan.places[i].is_none() {
            let out = usize::from(alt.tied);
            match plan.places[out] {
                Some(Place::Reg(r)) => plan.places[i] = Some(Place::Reg(r)),
                // Tied to a memory output: the same memory.
                Some(Place::Mem(r)) => plan.places[i] = Some(Place::Mem(r)),
                _ => return Err(refused(hod_asm::reason::constraint("tied"))),
            }
        }
    }
    // Types against places; the registers written.
    for (i, op) in ops.list[..ops.len].iter().enumerate() {
        match plan.places[i] {
            Some(Place::Reg(r)) => {
                let ty = plan.tys[i];
                if !fits(cx, ty, reg::is_fpr(r)) {
                    return unsupported(OPERAND_TYPE);
                }
                plan.written |= 1 << r.index();
            }
            Some(Place::Mem(r)) => {
                plan.written |= 1 << r.index();
                if !op.indirect && !fits(cx, plan.tys[i], true) {
                    return unsupported(OPERAND_TYPE);
                }
            }
            Some(Place::Flag(_)) => {
                if !matches!(cx.types().kind(plan.tys[i]), TypeKind::Int(_)) || op.indirect {
                    return unsupported(OPERAND_TYPE);
                }
            }
            Some(Place::Imm(_) | Place::Sym(_) | Place::Label(_)) => {}
            None => return cx.bug("an inline-assembly operand without a place"),
        }
    }
    Ok(plan)
}

// ---- the lowering ----

/// The features the function is compiled under.
fn features(cx: &FnCx<'_, A64>) -> Features {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return Features::BASE;
    };
    match cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) {
        Some(f) => Features::from_attribute(text(f)),
        None => Features::BASE,
    }
}

/// The callee-saved registers in `set`: `x19`..`x28`, `v8`..`v15`.
fn callee_saved(set: u64) -> impl Iterator<Item = PReg> {
    (19..=28).map(reg::x).chain((8..=15).map(reg::v)).filter(move |&r| reg_set_has(set, r))
}

/// A call, `invoke` or `callbr` of inline assembly (module
/// documentation).
///
/// # Errors
/// The names of this module and of `crate::barrier`; `hod_asm`'s
/// `asm:*` refusals.
pub fn call<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("an inline-assembly lowering of something that is no call");
    };
    let ValueKind::Asm(id) = site.callee_value().kind() else {
        return cx.bug("an inline-assembly lowering of another callee");
    };
    let asm = cx.module.asm(id);
    if inst.opcode() != Opcode::CallBr
        && hod_ir::asm_template_is_empty(hod_ir::Arch::Aarch64, asm.template)
    {
        match crate::barrier::call(cx, inst) {
            Err(CgError::Unsupported { construct })
                if crate::barrier::UNSUPPORTED.contains(&construct)
                    && construct != crate::barrier::CLOBBER
                    && construct != crate::barrier::BUNDLE =>
            {
                // The general sequence below handles what the barrier
                // module does not model; nothing was emitted.
            }
            other => return other,
        }
    }
    if asm.has(InlineAsm::INTEL_DIALECT) {
        return Err(refused(Refusal(hod_asm::reason::other::DIALECT)));
    }
    if site.bundle_count() != 0 {
        return unsupported(FORM);
    }
    if asm.has(InlineAsm::UNWIND) && inst.opcode() == Opcode::Invoke {
        return unsupported(UNWIND);
    }
    let ops = parse(asm.constraints)?;
    let args = site.args();
    let operands = ops.list[..ops.len].iter().filter(|op| op.arg.is_some()).count();
    let dests = site.indirect_dests().len();
    if operands != args.len()
        || (ops.direct_outputs == 0) != (inst.ty() == TypeId::VOID)
        || usize::from(ops.labels) != if inst.opcode() == Opcode::CallBr { dests } else { 0 }
    {
        return Err(refused(hod_asm::reason::constraint("count")));
    }
    let plan = decide(cx, inst, &ops)?;
    // The text first: nothing is emitted for a template that refuses.
    let mut body = String::new();
    let mut code = asm::Code::default();
    substitute(cx, inst, asm, &ops, &plan, &mut body)?;
    asm::assemble(&body, features(cx), &mut code, &mut String::new()).map_err(refused)?;
    let mut syms = Vec::new();
    for r in &code.refs {
        if let Some(n) = label_index(&r.name) {
            if usize::from(n) >= dests {
                return Err(refused(Refusal(hod_asm::reason::operand::LABEL)));
            }
            if r.addend != 0 || r.kind == asm::RefKind::Literal19 || r.kind == asm::RefKind::Adr21 {
                return Err(refused(Refusal(hod_asm::reason::operand::LABEL)));
            }
            syms.push(None);
        } else {
            let Some(sym) = cx.module.find_symbol(r.name.as_bytes()) else {
                return Err(refused(Refusal(hod_asm::reason::other::SYMBOL)));
            };
            syms.push(Some(sym));
        }
    }
    emit(cx, inst, &ops, plan, &code, &syms)
}

/// The name an `asm goto` target prints as: `.Lhod_asm_goto_<n>`.
const LABEL_PREFIX: &str = ".Lhod_asm_goto_";

fn label_index(name: &str) -> Option<u8> {
    name.strip_prefix(LABEL_PREFIX)?.parse().ok()
}

fn substitute(
    cx: &FnCx<'_, A64>,
    inst: Inst<'_>,
    asm: InlineAsm<'_>,
    ops: &Ops,
    plan: &Plan,
    out: &mut String,
) -> CgResult<()> {
    let template = std::str::from_utf8(asm.template)
        .map_err(|_| refused(Refusal(hod_asm::reason::other::LEXICAL)))?;
    let mut names = Vec::new();
    for place in &plan.places[..ops.len] {
        names.push(match place {
            Some(Place::Sym(s)) => std::str::from_utf8(cx.module.symbol(*s).name()).ok(),
            _ => None,
        });
    }
    let uid = inst.id().raw();
    template::substitute(template, ops.len as u32, uid, out, |r, out| {
        let i = r.index as usize;
        let arg = match plan.places[i] {
            Some(Place::Reg(p)) if reg::is_gpr(p) => Arg::Gp(reg::number(p)),
            Some(Place::Reg(p)) => Arg::Fp(reg::number(p)),
            Some(Place::Mem(p)) => Arg::Mem(reg::number(p)),
            Some(Place::Imm(v)) => Arg::Imm(v),
            Some(Place::Sym(_)) => match names[i] {
                Some(name) => Arg::Sym(name, 0),
                None => return Err(Refusal(hod_asm::reason::other::SYMBOL)),
            },
            Some(Place::Label(n)) => {
                // Printed with its own name; `asm::print_operand` takes a
                // borrowed one.
                let name = format!("{LABEL_PREFIX}{n}");
                return asm::print_operand(Arg::Label(&name), r.modifier, out);
            }
            // `llc` cannot print a flag output.
            Some(Place::Flag(_)) | None => return Err(Refusal(hod_asm::reason::other::TEMPLATE)),
        };
        asm::print_operand(arg, r.modifier, out)
    })
    .map_err(refused)
}

/// Stores the 64-bit view of general register `r` to `slot`.
fn store_x(cx: &mut FnCx<'_, A64>, r: mc::Reg, slot: FrameRef) {
    let mut at = Cursor::frame(cx, slot, X16);
    store_gpr_at(cx, MemSize::X, r.zr(), &mut at);
}

/// A 16-byte slot.
fn slot16(cx: &mut FnCx<'_, A64>) -> CgResult<FrameRef> {
    cx.alloc_slot(16, 16)
}

/// Zero-extends the integer in `r` of `bits` (< 64) to 64 bits.
fn zero_extend(cx: &mut FnCx<'_, A64>, r: mc::Reg, bits: u32) {
    if bits < 64 {
        put(cx, mc::ubfx(Size::X, r.zr(), r.zr(), 0, bits));
    }
}

#[allow(clippy::too_many_lines, reason = "the sequence of the module documentation, in order")]
fn emit<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ops: &Ops,
    mut plan: Plan,
    code: &asm::Code,
    syms: &[Option<hod_ir::SymId>],
) -> CgResult<()> {
    let site = inst.call().expect("checked by the caller");
    let args = site.args();
    deferred_first(cx, inst, None)?;
    // 1. Inputs and pointers to slots, one per distinct value.
    let mut stored: [(Value, Option<FrameRef>); MAX_OPS] = [(Value::NONE, None); MAX_OPS];
    let mut nstored = 0;
    for i in 0..ops.len {
        let op = ops.list[i];
        let Some(a) = op.arg else { continue };
        let needs = matches!(plan.places[i], Some(Place::Reg(_) | Place::Mem(_)));
        // An indirect output in memory needs its pointer; one in a
        // register needs it after the template; a direct input its value.
        if !needs {
            continue;
        }
        let value = args[usize::from(a)];
        if let Some(&(_, slot)) = stored[..nstored].iter().find(|(v, _)| *v == value) {
            plan.slots[i] = slot;
            continue;
        }
        let slot = slot16(cx)?;
        match cx.class_of(value) {
            Class::Gpr => {
                let r = cx.use_reg(value)?;
                store_x(cx, xr(r), slot);
                cx.free_temp(r);
            }
            Class::Fpr => {
                let r = cx.use_reg(value)?;
                let bytes = cx.types().alloc_size_of(value_ty(cx, value)) as u32;
                let mut at = Cursor::frame(cx, slot, X16);
                store_fpr_at(cx, fpr_view(bytes.max(1)), vr(r), &mut at);
                cx.free_temp(r);
            }
            Class::Mem => return unsupported(OPERAND_TYPE),
        }
        cx.release(value);
        stored[nstored] = (value, Some(slot));
        nstored += 1;
        plan.slots[i] = Some(slot);
    }
    // Direct outputs: a slot for each.
    for i in 0..ops.len {
        if ops.list[i].kind == Kind::Output
            && !ops.list[i].indirect
            && matches!(plan.places[i], Some(Place::Mem(_) | Place::Reg(_) | Place::Flag(_)))
        {
            plan.slots[i] = Some(slot16(cx)?);
        }
    }
    // 2. Registers.
    cx.displace_all()?;
    let scratch = [reg::x(16), reg::x(17), reg::v(30), reg::v(31), reg::x(18)];
    let mut taken = Vec::new();
    for n in 0..64u8 {
        let r = PReg::new(n);
        if reg_set_has(plan.written, r) && !scratch.contains(&r) && r != reg::SP && r != reg::FP {
            cx.take_reg(r)?;
            taken.push(r);
        }
    }
    let mut callee_slots = Vec::new();
    for r in callee_saved(plan.written) {
        let slot = cx.alloc_slot(8, 8)?;
        let mut at = Cursor::frame(cx, slot, X16);
        if reg::is_gpr(r) {
            store_gpr_at(cx, MemSize::X, xr(r).zr(), &mut at);
        } else {
            store_fpr_at(cx, VMemSize::D, vr(r), &mut at);
        }
        callee_slots.push((r, slot));
    }
    // 3. Inputs into place: vector registers first (their loads may
    // rebase into `x16`), then general ones.
    for pass_fpr in [true, false] {
        for i in 0..ops.len {
            let op = ops.list[i];
            let Some(place) = plan.places[i] else { continue };
            let input = op.kind == Kind::Input;
            match place {
                Place::Reg(r) if input && reg::is_fpr(r) == pass_fpr => {
                    let Some(slot) = plan.slots[i] else {
                        return cx.bug("an inline-assembly input without a slot");
                    };
                    let bytes = (cx.types().alloc_size_of(plan.tys[i]) as u32).max(1);
                    if op.indirect {
                        // The element at the pointer.
                        let mut at = Cursor::frame(cx, slot, X16);
                        load_gpr_at(cx, Load::X, X16, &mut at);
                        let mut at = Cursor::at_reg(X16, 0, X16);
                        load_into(cx, r, bytes, &mut at);
                    } else {
                        let mut at = Cursor::frame(cx, slot, X16);
                        load_into(cx, r, bytes, &mut at);
                    }
                    if let TypeKind::Int(bits) = cx.types().kind(plan.tys[i])
                        && reg::is_gpr(r)
                    {
                        zero_extend(cx, xr(r), bits);
                    }
                }
                Place::Mem(r) if !pass_fpr => {
                    let Some(slot) = plan.slots[i] else {
                        return cx.bug("an inline-assembly memory operand without a slot");
                    };
                    if op.indirect {
                        let mut at = Cursor::frame(cx, slot, xr(r));
                        load_gpr_at(cx, Load::X, xr(r), &mut at);
                    } else {
                        frame_address(cx, xr(r), slot);
                    }
                }
                _ => {}
            }
        }
    }
    // 4. The template.
    let start = cx.offset();
    let mut labels: Vec<(u32, asm::RefKind, u8)> = Vec::new();
    for word in &code.words {
        put(cx, *word);
    }
    for (r, sym) in code.refs.iter().zip(syms) {
        let at = start + r.offset;
        if let Some(sym) = sym {
            let kind = match r.kind {
                asm::RefKind::Jump26 => A64Reloc::Jump26,
                asm::RefKind::Call26 => A64Reloc::Call26,
                asm::RefKind::Branch19 => A64Reloc::CondBr19,
                asm::RefKind::Test14 => A64Reloc::TstBr14,
                asm::RefKind::Literal19 => A64Reloc::LdPrelLo19,
                asm::RefKind::Adr21 => A64Reloc::AdrPrelLo21,
            };
            cx.reloc(at, RelocKind::A64(kind), SymRef::Module(*sym), r.addend);
        } else {
            let n = label_index(&r.name).expect("checked");
            labels.push((at, r.kind, n));
        }
    }
    // 5. Outputs, flags, restores: on the path that falls through, and
    // (for `callbr`) at each indirect destination's trampoline.
    outputs(cx, ops, &plan, &callee_slots);
    for r in &taken {
        cx.free_temp(*r);
    }
    // 6. The result.
    result(cx, inst, ops, &plan)?;
    match inst.opcode() {
        Opcode::Invoke => {
            let Some(normal) = site.normal_dest() else {
                return cx.bug("an invoke without a normal destination");
            };
            continue_at(cx, normal)?;
        }
        Opcode::CallBr => {
            let Some(normal) = site.normal_dest() else {
                return cx.bug("a callbr without a normal destination");
            };
            // The trampolines follow: the normal destination is never
            // next.
            cx.phi_stores(normal)?;
            branch(cx, cx.block_label(normal))?;
            let dests: Vec<BlockId> = site.indirect_dests().collect();
            for (n, dest) in dests.iter().enumerate() {
                let here = cx.new_label();
                cx.bind(here)?;
                outputs(cx, ops, &plan, &callee_slots);
                // WP-112: the result is defined on every edge out, as
                // `llc` 22 writes it (an output is valid at an indirect
                // destination; `spec/codegen.md` §5.5.3 has LLVM 19's).
                result(cx, inst, ops, &plan)?;
                cx.phi_stores(*dest)?;
                // A register result no `phi` of this destination took
                // (its only uses were on another edge) is let go: no
                // register holds a value at the end of a block (§4.5).
                cx.release(inst.result());
                branch(cx, cx.block_label(*dest))?;
                for &(at, kind, _) in labels.iter().filter(|l| usize::from(l.2) == n) {
                    let fixup = match kind {
                        asm::RefKind::Jump26 | asm::RefKind::Call26 => JUMP26,
                        asm::RefKind::Branch19 => BRANCH19,
                        asm::RefKind::Test14 => TEST14,
                        _ => return cx.bug("a label reference of a data kind"),
                    };
                    cx.fixup(at, fixup, here)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Goes on at `dest` after the sequence: its phis, and a branch unless
/// it is next.
fn continue_at(cx: &mut FnCx<'_, A64>, dest: BlockId) -> CgResult<()> {
    cx.phi_stores(dest)?;
    if !cx.falls_through(dest) {
        branch(cx, cx.block_label(dest))?;
    }
    Ok(())
}

/// Loads `bytes` of the place at `at` into register `r` (zero-extended
/// for a general register).
fn load_into(cx: &mut FnCx<'_, A64>, r: PReg, bytes: u32, at: &mut Cursor) {
    if reg::is_gpr(r) {
        load_gpr_at(cx, gpr_load_op(bytes), xr(r), at);
    } else {
        load_fpr_at(cx, fpr_view(bytes), vr(r), at);
    }
}

/// Step 5: the register outputs to their slots or pointers, the flags,
/// the callee-saved registers back.
fn outputs(cx: &mut FnCx<'_, A64>, ops: &Ops, plan: &Plan, callee_slots: &[(PReg, FrameRef)]) {
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output {
            continue;
        }
        let (Some(Place::Reg(r)), Some(slot)) = (plan.places[i], plan.slots[i]) else {
            continue;
        };
        if op.indirect {
            let bytes = (cx.types().alloc_size_of(plan.tys[i]) as u32).max(1);
            let mut at = Cursor::frame(cx, slot, X17);
            load_gpr_at(cx, Load::X, X17, &mut at);
            let mut at = Cursor::at_reg(X17, 0, X17);
            if reg::is_gpr(r) {
                store_gpr_bytes(cx, xr(r), bytes, &mut at);
            } else {
                store_fpr_bytes(cx, vr(r), bytes, &mut at, mc::VReg::V30);
            }
        } else if reg::is_gpr(r) {
            store_x(cx, xr(r), slot);
        } else {
            let mut at = Cursor::frame(cx, slot, X16);
            store_fpr_at(cx, VMemSize::Q, vr(r), &mut at);
        }
    }
    for i in 0..ops.len {
        if let (Some(Place::Flag(cond)), Some(slot)) = (plan.places[i], plan.slots[i]) {
            put(cx, mc::cset(Size::X, X17.zr(), cond));
            store_x(cx, X17, slot);
        }
    }
    for &(r, slot) in callee_slots {
        let mut at = Cursor::frame(cx, slot, X16);
        if reg::is_gpr(r) {
            load_gpr_at(cx, Load::X, xr(r), &mut at);
        } else {
            load_fpr_at(cx, VMemSize::D, vr(r), &mut at);
        }
    }
}

/// Step 6: the call's result from the output slots.
fn result<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ops: &Ops, plan: &Plan) -> CgResult<()> {
    if inst.ty() == TypeId::VOID {
        return Ok(());
    }
    let fields: Vec<(usize, TypeId)> = (0..ops.len)
        .filter(|&i| ops.list[i].kind == Kind::Output && !ops.list[i].indirect)
        .map(|i| (i, plan.tys[i]))
        .collect();
    let class = cx.class_of(inst.result());
    if fields.len() == 1 && class != Class::Mem {
        let (i, ty) = fields[0];
        let Some(slot) = plan.slots[i] else { return cx.bug("an output without a slot") };
        let bytes = (cx.types().alloc_size_of(ty) as u32).max(1);
        let r = cx.def_reg(inst)?;
        let mut at = Cursor::frame(cx, slot, X16);
        load_into(cx, r, bytes, &mut at);
        cx.finish(inst, r)?;
        // A `callbr` defines its result again on each indirect edge: the
        // temporary is not kept (nothing happens to a value's register).
        cx.free_temp(r);
        return Ok(());
    }
    let dst = cx.def_mem(inst)?;
    for (field, &(i, ty)) in fields.iter().enumerate() {
        let Some(slot) = plan.slots[i] else { return cx.bug("an output without a slot") };
        let bytes = cx.types().size_of(ty) as u32;
        let offset =
            if fields.len() == 1 { 0 } else { cx.types().field_offset(inst.ty(), field as u32) };
        let mut to = Cursor::frame(cx, dst, X16);
        to.advance(offset as u32);
        let mut from = Cursor::frame(cx, slot, X17);
        copy_bytes(cx, &mut to, &mut from, bytes.max(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_parse() {
        let ops = parse(b"=r,={x10},=*m,0,{x9},~{x1},~{memory},~{cc}").unwrap();
        assert_eq!(ops.len, 5);
        assert_eq!(ops.direct_outputs, 2);
        assert!(reg_set_has(ops.clobbers, reg::x(1)));
        assert_eq!(ops.list[1].alts[0].reg, Some(reg::x(10)));
        assert_eq!(ops.list[3].alts[0].tied, 0);
        assert!(parse(b"={@cceq},r").unwrap().list[0].alts[0].cond == Some(mc::Cond::Eq));
        assert!(parse(b"{sp}").is_err());
        assert!(parse(b"~{x29}").is_err());
        assert!(parse(b"{x16}").is_err());
        assert!(parse(b"g").is_err());
        assert!(parse(b"r,=r").is_err());
    }
}
