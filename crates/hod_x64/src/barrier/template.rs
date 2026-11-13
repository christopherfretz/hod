//! Inline assembly with a template of instructions on x86-64
//! (`spec/asm.md` §11, `spec/codegen.md` §5.6.28 "WP-187"): the
//! operands are placed as the constraints ask, the template is
//! substituted as LLVM's `X86AsmPrinter` does in AT&T syntax and
//! assembled by `hod_asm::x64` (every byte is one hod parsed and
//! encoded), and the outputs are taken back.
//!
//! **Owner: WP-187.** Tests: `tests/inline_asm.rs`. Hook:
//! `barrier::asm`, for a template that is not empty (a `callbr` with one
//! stays [`INLINE_ASM`](super::INLINE_ASM)).
//!
//! # The sequence
//!
//! 1. Every input that is placed in a register or memory (and every
//!    pointer of an indirect operand) is evaluated and stored to a
//!    16-byte slot of its own, so that the placing below reads only the
//!    frame.
//! 2. Every register value is displaced; every register the template may
//!    write (operand registers, clobbers) that can hold a value is taken.
//!    The callee-saved ones among them (`rbx`, `r12`..`r15`) are saved by
//!    the prologue (a taken register is a used one, §5.6.15).
//! 3. The inputs are loaded into their registers (an integer narrower
//!    than 32 bits zero-extended), the memory operands' addresses put in
//!    theirs (`lea` of the slot, or the pointer).
//! 4. The template's bytes.
//! 5. Every register output is stored to its slot (`mov`, which leaves
//!    the flags); then the flag outputs (`={@cc<cond>}`) are `setcc` into
//!    `r11` and stored; then each indirect register output is copied from
//!    its slot through its pointer (`r11`, the value in `r10` or
//!    `xmm15`).
//! 6. The result is loaded from its output slot, or the fields of the
//!    struct of several are copied from theirs.
//!
//! Nothing of the sequence writes the flags or a register between 3 and
//! 5 but the template; the code around it reads and writes only the
//! frame (`rbp`-relative, or `rbx`-relative in a coroutine body) and the
//! operand registers.
//!
//! # Registers
//!
//! Each operand gets a register of its own, distinct from every other
//! operand's and every clobber (always a valid choice: an output that
//! `llc` would share with an input is in a register of its own here,
//! which also satisfies `&`), except a tied input, which is in its
//! output's register. General registers are taken in the order `rax`,
//! `rcx`, `rdx`, `rsi`, `rdi`, `r8`..`r11`, `rbx`, `r12`..`r15` (within
//! the class: `Q` is `rax`..`rbx`, `R` the eight legacy registers);
//! vector registers `xmm0`..`xmm15`. An explicit register (`{rax}`) is
//! that register. (`llc` refuses the letters `a`, `b`, `c`, `d`, `S`,
//! `D`, `A` and `g` in IR, front ends write `{ax}` and `imr`; so does
//! hod: `asm:constraint/<letter>`.) `rsp` and `rbp` are refused as operands and
//! clobbers ([`FRAME_REGISTER`]); in a coroutine body `rbx` (the frame
//! base) is refused when the template may write it ([`FRAME_BASE`]).
//!
//! # Constraint choice
//!
//! Of several alternatives (`r|m`) the one `llc` takes by weights
//! (`X86TargetLowering::getSingleConstraintMatchWeight`, as in
//! `barrier`). Within one alternative, as `llc`'s
//! `ComputeConstraintToUse` orders the codes: a flag output; an explicit
//! or fixed register; a tied input's output register; an immediate code
//! (or `X`) for a constant integer in its range; memory (`m`, `o`) when the
//! operand is not a direct output or an output an input is tied to;
//! else a register.

use hod_asm::x64::{self as asm, Arg};
use hod_asm::{Refusal, reason, template};
use hod_cg::asm_constraints::{
    self as constraints, Clobber, ConstraintError, ConstraintSyntax, Kind, MAX_OPS, TIED,
};
use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef, PReg, SymRef};
use hod_ir::{Const, InlineAsm, Inst, Opcode, SymId, TypeId, TypeKind, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Size};
use hod_obj::{RelocKind, X64Reloc};

use crate::common::{
    R10, R11, XMM15, branch, copy_frame, deferred_first, frame_address, gp, gpr_load, load_fpr,
    load_gpr, put, store_fpr, store_gpr, unsupported, value_ty, xm,
};
use crate::reg;
use crate::target::X64;

/// An operand in `rsp` or `rbp`, or a clobber of either: hod's frame is
/// addressed from them.
pub const FRAME_REGISTER: &str = "x64:inline-asm-frame-register";
/// A template that may write `rbx` in a coroutine body, whose frame base
/// it is (WP-137).
pub const FRAME_BASE: &str = "x64:inline-asm-frame-base";
/// An operand of a type its place does not hold: an aggregate, an
/// integer wider than 64 bits or of other than 8, 16, 32, 64 bits in a
/// general register, a vector wider than 16 bytes, a tied pair of
/// different sizes, a flag output that is no integer.
pub const OPERAND_TYPE: &str = "x64:inline-asm-operand-type";
/// More operands, alternatives or registers than are kept (16 operands,
/// 4 alternatives, the registers of a class).
pub const TOO_MANY: &str = "x64:inline-asm-operands";
/// `unwind` inline assembly called by `invoke`: hod records no call site
/// for the template.
pub const UNWIND: &str = "x64:inline-asm-unwind";
/// Intel-dialect inline assembly (`inteldialect`): only AT&T is
/// assembled.
pub const INTEL: &str = "x64:inline-asm-intel";

/// Every name this module refuses with, besides `hod_asm`'s `asm:*` and
/// [`super::BUNDLE`].
pub const UNSUPPORTED: &[&str] =
    &[FRAME_REGISTER, FRAME_BASE, OPERAND_TYPE, TOO_MANY, UNWIND, INTEL];

fn refused(r: Refusal) -> CgError {
    CgError::unsupported(r.0)
}

// ---- constraints ----

/// Codes of one alternative, as bits (`SPEC`, `TIED` and `FLAG` are the
/// shared parser's).
const R: u32 = 1 << 0; // r, l, q: any general register
const ABCD: u32 = 1 << 1; // Q: rax, rcx, rdx, rbx
const LEGACY: u32 = 1 << 2; // R: the eight legacy registers
const X: u32 = 1 << 3; // x, v: an SSE register
const M: u32 = 1 << 4; // m, o
const XX: u32 = 1 << 6; // X: anything
const IMM_ANY: u32 = 1 << 7; // i, n
const IMM_E: u32 = 1 << 8; // e: a signed 32-bit constant
const IMM_Z: u32 = 1 << 9; // Z: an unsigned 32-bit constant
const IMM_I: u32 = 1 << 10; // I: 0..31
const IMM_J: u32 = 1 << 11; // J: 0..63
const IMM_K: u32 = 1 << 12; // K: a signed 8-bit constant
const IMM_L: u32 = 1 << 13; // L: 0xff, 0xffff, 0xffffffff
const IMM_M: u32 = 1 << 14; // M: 0..3
const IMM_N: u32 = 1 << 15; // N: 0..255
const IMM_O: u32 = 1 << 16; // O: 0..127

/// The immediate codes.
const IMMS: [u32; 10] = [IMM_ANY, IMM_E, IMM_Z, IMM_I, IMM_J, IMM_K, IMM_L, IMM_M, IMM_N, IMM_O];
const ANY_IMM: u32 =
    IMM_ANY | IMM_E | IMM_Z | IMM_I | IMM_J | IMM_K | IMM_L | IMM_M | IMM_N | IMM_O;
const GPR_CLASS: u32 = R | ABCD | LEGACY;

/// Whether `v` is in the range of immediate code `code`.
fn imm_fits(code: u32, v: i64) -> bool {
    match code {
        IMM_E => i32::try_from(v).is_ok(),
        IMM_Z => u32::try_from(v).is_ok(),
        IMM_I => (0..=31).contains(&v),
        IMM_J => (0..=63).contains(&v),
        IMM_K => (-128..=127).contains(&v),
        IMM_L => matches!(v, 0xff | 0xffff | 0xffff_ffff),
        IMM_M => (0..=3).contains(&v),
        IMM_N => (0..=255).contains(&v),
        IMM_O => (0..=127).contains(&v),
        _ => true,
    }
}

type Alt = constraints::Alt<Cond>;
type Op = constraints::Op<Cond>;
type Ops = constraints::Constraints<Cond>;

/// What an operand became.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    /// A register.
    Reg(PReg),
    /// Memory whose address is in this general register.
    Mem(PReg),
    /// A constant.
    Imm(i64),
    /// A flag output.
    Flag(Cond),
}

/// The register a name denotes, for an operand (`Ok(None)`: not one hod
/// places an operand in: `ah`..`dh`, x87, `xmm16` and up, `ymm`, `zmm`).
fn named_reg(name: &[u8]) -> Result<Option<PReg>, CgError> {
    let name = std::str::from_utf8(name).unwrap_or("");
    match asm::operand::reg_name(name) {
        Some(asm::operand::RegName::Gpr(g)) => match g.reg {
            mc::Reg::Rsp | mc::Reg::Rbp => Err(CgError::unsupported(FRAME_REGISTER)),
            r => Ok(Some(reg::gpr(r))),
        },
        Some(asm::operand::RegName::Xmm(x)) => Ok(Some(reg::xmm(x))),
        _ => Ok(None),
    }
}

/// The number of `prefix<n>` for `n` in `0..=31`, written without
/// leading zeros.
fn numbered(name: &[u8], prefix: &[u8]) -> Option<u8> {
    let digits = name.strip_prefix(prefix)?;
    let ok = !digits.is_empty()
        && digits.len() <= 2
        && digits.iter().all(u8::is_ascii_digit)
        && (digits.len() == 1 || digits[0] != b'0');
    let n = digits.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0'));
    (ok && n <= 31).then_some(n as u8)
}

/// x86-64's part of a constraint string.
struct X64Syntax;

impl ConstraintSyntax for X64Syntax {
    type Cond = Cond;

    fn letters(&self, code: &[u8], _output: bool) -> Option<(u32, usize)> {
        let bits = match code.first()? {
            b'r' | b'l' | b'q' => R,
            b'Q' => ABCD,
            b'R' => LEGACY,
            b'x' | b'v' => X,
            b'm' | b'o' => M,
            b'X' => XX,
            b'i' | b'n' => IMM_ANY,
            b'e' => IMM_E,
            b'Z' => IMM_Z,
            b'I' => IMM_I,
            b'J' => IMM_J,
            b'K' => IMM_K,
            b'L' => IMM_L,
            b'M' => IMM_M,
            b'N' => IMM_N,
            b'O' => IMM_O,
            _ => return None,
        };
        Some((bits, 1))
    }

    fn flag(&self, cond: &[u8]) -> Option<Cond> {
        asm::encode::cond_name(std::str::from_utf8(cond).ok()?)
    }

    fn operand_reg(&self, name: &[u8]) -> Result<Option<PReg>, CgError> {
        named_reg(name)
    }

    fn clobber(&self, name: &[u8]) -> Result<Clobber, CgError> {
        if let b"cc" | b"flags" | b"eflags" | b"dirflag" | b"fpsr" | b"fpcr" | b"mxcsr" | b"st" =
            name
        {
            return Ok(Clobber::Ignored);
        }
        // The x87 and MMX registers hold nothing of hod's across an
        // instruction; neither do the upper SSE registers.
        if name.starts_with(b"st(") || numbered(name, b"mm").is_some_and(|n| n < 8) {
            return Ok(Clobber::Ignored);
        }
        for prefix in [&b"xmm"[..], b"ymm", b"zmm"] {
            match numbered(name, prefix) {
                Some(n @ 0..=15) => {
                    return Ok(Clobber::Reg(PReg::new(reg::xmm(mc::Xmm::Xmm0).index() + n)));
                }
                Some(_) => return Ok(Clobber::Ignored),
                None => {}
            }
        }
        match named_reg(name)? {
            Some(r) => Ok(Clobber::Reg(r)),
            None => match name {
                b"ah" | b"bh" | b"ch" | b"dh" => {
                    Ok(Clobber::Reg(named_reg(&[name[0], b'x'])?.expect("a register")))
                }
                _ => Ok(Clobber::Unknown),
            },
        }
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or("other")
}

/// Reads the constraint string `constraints` (`hod_cg::asm_constraints`).
fn parse(constraints: &[u8]) -> CgResult<Ops> {
    constraints::parse(&X64Syntax, constraints).map_err(|e| match e {
        ConstraintError::Constraint(t) => refused(reason::constraint(text(t))),
        ConstraintError::Letter(c) => refused(reason::constraint(&char::from(c).to_string())),
        ConstraintError::Clobber(t) => refused(reason::clobber(text(t))),
        ConstraintError::TooMany => CgError::unsupported(TOO_MANY),
        ConstraintError::Target(e) => e,
    })
}

// ---- choosing ----

fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<i64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => {
            let TypeKind::Int(bits) = cx.types().kind(ty) else { return None };
            Some(hod_ir::sext64(value, bits))
        }
        _ => None,
    }
}

/// LLVM's weight of one code for an operand
/// (`X86TargetLowering::getSingleConstraintMatchWeight`; `barrier` has
/// the reading): for an integer every code weighs 0 but `x` (1 for 128
/// bits) and the ranged immediates (3 when the constant fits); for
/// another operand the generic weights (`m` 2, `i` 3 for a constant
/// integer, the register codes invalid).
fn code_weight(cx: &FnCx<'_, X64>, operand: Option<Value>, code: u32) -> Option<i32> {
    let Some(value) = operand else { return Some(0) };
    let ty = value_ty(cx, value);
    let integer = matches!(cx.types().kind(ty), TypeKind::Int(_));
    let wide = cx.types().size_in_bits_of(ty) == 128;
    match code {
        X => wide.then_some(1),
        IMM_E | IMM_Z | IMM_I | IMM_J | IMM_K | IMM_L | IMM_M | IMM_N | IMM_O => {
            const_int(cx, value).filter(|&v| imm_fits(code, v)).map(|_| 3)
        }
        _ if integer => Some(0),
        IMM_ANY => const_int(cx, value).map(|_| 3),
        M => Some(2),
        R | ABCD | LEGACY => None,
        _ => Some(0),
    }
}

/// The alternative `llc` takes (`Constraints::choose`).
fn choose(cx: &FnCx<'_, X64>, ops: &Ops, args: &[Value]) -> usize {
    ops.choose(args, |operand, codes| {
        let mut w = None;
        let mut bits = codes;
        while bits != 0 {
            let code = bits & bits.wrapping_neg();
            bits &= bits - 1;
            w = w.max(code_weight(cx, operand, code));
        }
        w
    })
}

/// The general registers in the order they are chosen.
const GPR_ORDER: [u8; 14] = [0, 1, 2, 6, 7, 8, 9, 10, 11, 3, 12, 13, 14, 15];

/// The general registers of a class code, as a mask over encodings.
fn gpr_mask(codes: u32) -> u32 {
    let mut mask = 0;
    if codes & (R | XX | M) != 0 {
        mask |= 0xffff & !0x30;
    }
    if codes & ABCD != 0 {
        mask |= 0x0f;
    }
    if codes & LEGACY != 0 {
        mask |= 0xcf;
    }
    mask
}

/// What the lowering decided for each operand.
struct Plan {
    places: [Option<Place>; MAX_OPS],
    /// The type an operand's register or memory holds: the value's type,
    /// an indirect operand's element type (`VOID` when the call does not
    /// say), a direct output's field type.
    tys: [TypeId; MAX_OPS],
    /// The slot of each operand: an input's value or pointer, an
    /// indirect output's pointer, a direct output's result.
    slots: [Option<FrameRef>; MAX_OPS],
    /// The slot an indirect register output's value waits in.
    values: [Option<FrameRef>; MAX_OPS],
    /// Every register the template may write.
    written: u64,
}

fn has(set: u64, r: PReg) -> bool {
    set & (1 << r.index()) != 0
}

/// The type of each operand's register or memory.
fn operand_types(
    cx: &FnCx<'_, X64>,
    inst: Inst<'_>,
    site: hod_ir::CallSite<'_>,
    ops: &Ops,
) -> CgResult<[TypeId; MAX_OPS]> {
    let mut tys = [TypeId::VOID; MAX_OPS];
    let args = site.args();
    for (i, op) in ops.ops().iter().enumerate() {
        tys[i] = match (op.indirect, op.arg, op.field) {
            (true, Some(a), _) => site.arg_attrs(usize::from(a)).mem_type().unwrap_or(TypeId::VOID),
            (false, Some(a), _) => value_ty(cx, args[usize::from(a)]),
            (false, None, Some(f)) => {
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

/// The bytes a value of `ty` takes in a general register (1, 2, 4, 8),
/// if one holds it.
fn gpr_bytes(cx: &FnCx<'_, X64>, ty: TypeId) -> Option<u8> {
    match cx.types().kind(ty) {
        TypeKind::Int(bits @ (8 | 16 | 32 | 64)) => Some((bits / 8) as u8),
        TypeKind::Ptr { addr_space: 0 } | TypeKind::Double => Some(8),
        TypeKind::Float => Some(4),
        _ => None,
    }
}

/// The bytes a value of `ty` takes in an SSE register (4, 8, 16), if one
/// holds it.
fn xmm_bytes(cx: &FnCx<'_, X64>, ty: TypeId) -> Option<u32> {
    let bytes = cx.types().alloc_size_of(ty) as u32;
    let ok = match cx.types().kind(ty) {
        TypeKind::Float | TypeKind::Double | TypeKind::Fp128 | TypeKind::Vector { .. } => true,
        TypeKind::Int(bits) => matches!(bits, 32 | 64 | 128),
        _ => false,
    };
    (ok && matches!(bytes, 4 | 8 | 16)).then_some(bytes)
}

fn is_fp_type(cx: &FnCx<'_, X64>, ty: TypeId) -> bool {
    matches!(
        cx.types().kind(ty),
        TypeKind::Float | TypeKind::Double | TypeKind::Fp128 | TypeKind::Vector { .. }
    ) || matches!(cx.types().kind(ty), TypeKind::Int(128))
}

/// Whether the alternative puts the operand in memory: it has a memory
/// code, and the operand is neither a direct output nor an output an
/// input is tied to that may take a register.
fn takes_memory(op: &Op, alt: Alt, tied_by: bool) -> bool {
    let output = op.kind == Kind::Output;
    let register = alt.codes & (GPR_CLASS | X | XX) != 0;
    alt.codes & M != 0 && (!output || op.indirect) && !(output && tied_by && register)
}

/// The place of operand `i` before registers are chosen: `Ok(None)` and
/// `wants` set for one to choose, `Ok(None)` alone for a tied input.
#[allow(clippy::too_many_arguments, reason = "the parts of the decision")]
fn first_place(
    cx: &FnCx<'_, X64>,
    op: &Op,
    alt: Alt,
    value: Option<Value>,
    tied_by: bool,
    ty: TypeId,
    wants: &mut Option<bool>,
) -> CgResult<Option<Place>> {
    let direct_value = value.filter(|_| !op.indirect);
    if let Some(cond) = alt.cond {
        return Ok(Some(Place::Flag(cond)));
    }
    if let Some(r) = alt.reg {
        return Ok(Some(Place::Reg(r)));
    }
    if alt.codes & TIED != 0 && op.kind == Kind::Input {
        return Ok(None);
    }
    let imm_codes = alt.codes & (ANY_IMM | XX);
    if imm_codes != 0
        && let Some(c) = direct_value.and_then(|v| const_int(cx, v))
    {
        let fits = IMMS.iter().any(|&code| imm_codes & code != 0 && imm_fits(code, c))
            || alt.codes & XX != 0;
        if fits {
            return Ok(Some(Place::Imm(c)));
        }
        if alt.codes & !ANY_IMM == 0 {
            // A constant out of every range of the alternative: an `llc`
            // error.
            return Err(refused(reason::constraint("range")));
        }
    }
    let register = alt.codes & (GPR_CLASS | X | XX) != 0;
    if takes_memory(op, alt, tied_by) {
        *wants = Some(false);
        return Ok(None);
    }
    if register {
        let fpr = alt.codes & X != 0
            || (alt.codes & GPR_CLASS == 0 && alt.codes & XX != 0 && is_fp_type(cx, ty));
        *wants = Some(fpr);
        return Ok(None);
    }
    if alt.codes & ANY_IMM != 0 {
        if direct_value.is_some_and(|v| matches!(v.kind(), ValueKind::Sym(_))) {
            return Err(refused(Refusal(reason::operand::SYMBOL)));
        }
        return Err(refused(reason::constraint("i")));
    }
    Err(refused(reason::constraint(if alt.codes & M != 0 { "m" } else { "other" })))
}

#[allow(clippy::too_many_lines, reason = "the choice of the module documentation, in order")]
fn decide(cx: &FnCx<'_, X64>, inst: Inst<'_>, ops: &Ops) -> CgResult<Plan> {
    let site = inst.call().expect("checked by the caller");
    let args = site.args();
    let chosen = choose(cx, ops, args);
    let tys = operand_types(cx, inst, site, ops)?;
    let mut plan = Plan {
        places: [None; MAX_OPS],
        tys,
        slots: [None; MAX_OPS],
        values: [None; MAX_OPS],
        written: ops.clobbers,
    };
    let mut tied_by = [false; MAX_OPS];
    for op in ops.ops() {
        let alt = op.alt(chosen);
        if op.kind == Kind::Input && alt.codes & TIED != 0 {
            tied_by[usize::from(alt.tied)] = true;
        }
    }
    let mut wants = [None::<bool>; MAX_OPS];
    for (i, op) in ops.ops().iter().enumerate() {
        let alt = op.alt(chosen);
        let value = op.arg.map(|a| args[usize::from(a)]);
        plan.places[i] = first_place(cx, op, alt, value, tied_by[i], plan.tys[i], &mut wants[i])?;
    }
    // Registers placed explicitly or clobbered are not chosen.
    let mut taken = ops.clobbers;
    for place in plan.places[..ops.len].iter().flatten() {
        if let Place::Reg(r) = place {
            taken |= 1 << r.index();
        }
    }
    for i in 0..ops.len {
        let Some(fpr) = wants[i] else { continue };
        let alt = ops.list[i].alt(chosen);
        let r = if fpr {
            (0..16u8).map(|n| PReg::new(16 + n)).find(|&r| !has(taken, r))
        } else {
            let mask = if alt.codes & GPR_CLASS != 0 && alt.codes & M == 0 {
                gpr_mask(alt.codes & GPR_CLASS)
            } else {
                gpr_mask(R)
            };
            GPR_ORDER
                .iter()
                .map(|&n| PReg::new(n))
                .find(|&r| (mask >> r.index()) & 1 != 0 && !has(taken, r))
        };
        let Some(r) = r else { return unsupported(TOO_MANY) };
        taken |= 1 << r.index();
        let memory = !fpr && takes_memory(&ops.list[i], alt, tied_by[i]);
        plan.places[i] = Some(if memory { Place::Mem(r) } else { Place::Reg(r) });
    }
    for (i, op) in ops.ops().iter().enumerate() {
        let alt = op.alt(chosen);
        if op.kind == Kind::Input && alt.codes & TIED != 0 && plan.places[i].is_none() {
            let out = usize::from(alt.tied);
            plan.places[i] = match plan.places[out] {
                Some(Place::Reg(r)) => {
                    let same = |ty| {
                        if reg::is_gpr(r) {
                            gpr_bytes(cx, ty)
                        } else {
                            xmm_bytes(cx, ty).map(|b| b as u8)
                        }
                    };
                    if same(plan.tys[i]) != same(plan.tys[out]) {
                        return unsupported(OPERAND_TYPE);
                    }
                    Some(Place::Reg(r))
                }
                Some(Place::Mem(r)) => Some(Place::Mem(r)),
                _ => return Err(refused(reason::constraint("tied"))),
            };
        }
    }
    // Types against places; the registers written.
    for (i, op) in ops.ops().iter().enumerate() {
        let ty = plan.tys[i];
        match plan.places[i] {
            Some(Place::Reg(r)) => {
                let fits = if reg::is_gpr(r) {
                    gpr_bytes(cx, ty).is_some()
                } else {
                    xmm_bytes(cx, ty).is_some()
                };
                if !fits {
                    return unsupported(OPERAND_TYPE);
                }
                plan.written |= 1 << r.index();
            }
            Some(Place::Mem(r)) => {
                plan.written |= 1 << r.index();
                if !op.indirect && op.kind == Kind::Input {
                    let value = args[usize::from(op.arg.expect("an input has an operand"))];
                    if cx.class_of(value) == Class::Mem
                        || (cx.class_of(value) == Class::Fpr && xmm_bytes(cx, ty).is_none())
                    {
                        return unsupported(OPERAND_TYPE);
                    }
                }
            }
            Some(Place::Flag(_)) => {
                if !matches!(cx.types().kind(ty), TypeKind::Int(8 | 16 | 32 | 64)) || op.indirect {
                    return unsupported(OPERAND_TYPE);
                }
            }
            Some(Place::Imm(_)) => {}
            None => return cx.bug("an inline-assembly operand without a place"),
        }
    }
    if has(plan.written, reg::RSP) || has(plan.written, reg::RBP) {
        return unsupported(FRAME_REGISTER);
    }
    if cx.has_coro_frame() && has(plan.written, reg::RBX) {
        return unsupported(FRAME_BASE);
    }
    Ok(plan)
}

// ---- the lowering ----

/// A call or `invoke` of inline assembly whose template is not empty
/// (module documentation).
///
/// # Errors
/// The names of this module, [`super::BUNDLE`]; `hod_asm`'s `asm:*`
/// refusals.
pub fn call<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("an inline-assembly lowering of something that is no call");
    };
    let ValueKind::Asm(id) = site.callee_value().kind() else {
        return cx.bug("an inline-assembly lowering of another callee");
    };
    let asm = cx.module.asm(id);
    if asm.has(InlineAsm::INTEL_DIALECT) {
        return unsupported(INTEL);
    }
    if site.bundle_count() != 0 {
        return unsupported(super::BUNDLE);
    }
    if asm.has(InlineAsm::UNWIND) && inst.opcode() == Opcode::Invoke {
        return unsupported(UNWIND);
    }
    let ops = parse(asm.constraints)?;
    let args = site.args();
    let operands = ops.ops().iter().filter(|op| op.arg.is_some()).count();
    if operands != args.len()
        || (ops.direct_outputs == 0) != (inst.ty() == TypeId::VOID)
        || ops.labels != 0
    {
        return Err(refused(reason::constraint("count")));
    }
    let plan = decide(cx, inst, &ops)?;
    // The text first: nothing is emitted for a template that refuses.
    let mut body = String::new();
    substitute(cx, inst, asm, &ops, &plan, &mut body)?;
    let (mut code, mut refs) = (Vec::new(), Vec::new());
    asm::assemble(&body, &mut code, &mut refs, &mut String::new()).map_err(refused)?;
    let mut fields = Vec::with_capacity(refs.len());
    for r in &refs {
        // A symbol of the module, by its object name, as LLVM's
        // assembler binds it; another is not written.
        let Some(sym) = cx.module.find_symbol(r.name.as_bytes()) else {
            return Err(refused(Refusal(reason::other::SYMBOL)));
        };
        let kind = match r.kind {
            asm::RefKind::Pc32 => X64Reloc::Pc32,
            asm::RefKind::Plt32 => X64Reloc::Plt32,
            asm::RefKind::GotPcRel => X64Reloc::GotPcrel,
            asm::RefKind::GotPcRelX => X64Reloc::GotPcrelX,
            asm::RefKind::RexGotPcRelX => X64Reloc::RexGotPcrelX,
            asm::RefKind::GotTpOff => X64Reloc::GotTpOff,
            asm::RefKind::TpOff32 => X64Reloc::TpOff32,
            asm::RefKind::DtpOff32 => X64Reloc::DtpOff32,
            asm::RefKind::Abs32S => X64Reloc::Abs32S,
        };
        fields.push((r.offset, kind, sym, r.addend));
    }
    emit(cx, inst, &ops, plan, &code, &fields)
}

/// A symbol field of a template's code: its offset in the code, the
/// relocation, the symbol and the addend.
type Field = (u32, X64Reloc, SymId, i64);

fn substitute(
    cx: &FnCx<'_, X64>,
    inst: Inst<'_>,
    asm: InlineAsm<'_>,
    ops: &Ops,
    plan: &Plan,
    out: &mut String,
) -> CgResult<()> {
    let template =
        std::str::from_utf8(asm.template).map_err(|_| refused(Refusal(reason::other::LEXICAL)))?;
    let uid = inst.id().raw();
    template::substitute(template, ops.len as u32, uid, out, |r, out| {
        let i = r.index as usize;
        let arg = match plan.places[i] {
            Some(Place::Reg(p)) if reg::is_gpr(p) => {
                Arg::Gp(reg::number(p), gpr_bytes(cx, plan.tys[i]).unwrap_or(8))
            }
            Some(Place::Reg(p)) => Arg::Xmm(reg::number(p)),
            Some(Place::Mem(p)) => Arg::Mem(reg::number(p)),
            Some(Place::Imm(v)) => Arg::Imm(v),
            // `llc` cannot print a flag output.
            Some(Place::Flag(_)) | None => return Err(Refusal(reason::other::TEMPLATE)),
        };
        asm::print_operand(arg, r.modifier, out)
    })
    .map_err(refused)
}

fn slot16(cx: &mut FnCx<'_, X64>) -> CgResult<FrameRef> {
    cx.alloc_slot(16, 16)
}

/// Loads `bytes` of `slot` into register `r` (a general one
/// zero-extended to 32 bits at least).
fn load_into(cx: &mut FnCx<'_, X64>, r: PReg, slot: FrameRef, bytes: u32) {
    if reg::is_gpr(r) {
        load_gpr(cx, gp(r), slot, bytes);
    } else {
        load_fpr(cx, xm(r), slot, bytes);
    }
}

/// The bytes of operand `i`'s register.
fn reg_bytes(cx: &FnCx<'_, X64>, r: PReg, ty: TypeId) -> u32 {
    if reg::is_gpr(r) {
        u32::from(gpr_bytes(cx, ty).unwrap_or(8))
    } else {
        xmm_bytes(cx, ty).unwrap_or(16)
    }
}

#[allow(clippy::too_many_lines, reason = "the sequence of the module documentation, in order")]
fn emit<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    ops: &Ops,
    mut plan: Plan,
    code: &[u8],
    fields: &[Field],
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
        if !matches!(plan.places[i], Some(Place::Reg(_) | Place::Mem(_))) {
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
                store_gpr(cx, gp(r), slot, 8);
                cx.free_temp(r);
            }
            Class::Fpr => {
                let Some(bytes) = xmm_bytes(cx, value_ty(cx, value)) else {
                    return unsupported(OPERAND_TYPE);
                };
                let r = cx.use_reg(value)?;
                store_fpr(cx, xm(r), slot, bytes);
                cx.free_temp(r);
            }
            Class::Mem => return unsupported(OPERAND_TYPE),
        }
        cx.release(value);
        stored[nstored] = (value, Some(slot));
        nstored += 1;
        plan.slots[i] = Some(slot);
    }
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output {
            continue;
        }
        if !op.indirect {
            plan.slots[i] = Some(slot16(cx)?);
        } else if matches!(plan.places[i], Some(Place::Reg(_))) {
            plan.values[i] = Some(slot16(cx)?);
        }
    }
    // 2. Registers.
    cx.displace_all()?;
    let holdable = cx.reg_info().holdable;
    let mut taken = Vec::new();
    for n in 0..32u8 {
        let r = PReg::new(n);
        if has(plan.written, r) && holdable.contains(r) {
            cx.take_reg(r)?;
            taken.push(r);
        }
    }
    // 3. Inputs into place.
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Input {
            continue;
        }
        let Some(slot) = plan.slots[i] else { continue };
        match plan.places[i] {
            Some(Place::Reg(r)) => {
                let bytes = reg_bytes(cx, r, plan.tys[i]);
                if op.indirect {
                    // The element at the pointer: the pointer into the
                    // register itself (a general one) or into `r11`.
                    let p = if reg::is_gpr(r) { gp(r) } else { R11 };
                    load_gpr(cx, p, slot, 8);
                    let m = mc::Mem::base(p, 0);
                    if reg::is_gpr(r) {
                        put(cx, gpr_load(gp(r), m, bytes));
                    } else {
                        put(cx, crate::common::fpr_load(xm(r), m, bytes));
                    }
                } else {
                    load_into(cx, r, slot, bytes);
                }
            }
            Some(Place::Mem(r)) => {
                if op.indirect {
                    load_gpr(cx, gp(r), slot, 8);
                } else {
                    frame_address(cx, gp(r), slot);
                }
            }
            _ => {}
        }
    }
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind == Kind::Output
            && op.indirect
            && let (Some(Place::Mem(r)), Some(slot)) = (plan.places[i], plan.slots[i])
        {
            load_gpr(cx, gp(r), slot, 8);
        }
    }
    // A memory output that is not indirect has no memory to point at:
    // `decide` never makes one.
    // 4. The template.
    let start = cx.offset();
    cx.emit(code);
    for &(offset, kind, sym, addend) in fields {
        cx.reloc(start + offset, RelocKind::X64(kind), SymRef::Module(sym), addend);
    }
    // 5. Outputs: registers to slots, flags, indirect stores.
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output {
            continue;
        }
        let Some(Place::Reg(r)) = plan.places[i] else { continue };
        let to = if op.indirect { plan.values[i] } else { plan.slots[i] };
        let Some(to) = to else { return cx.bug("an inline-assembly output without a slot") };
        if reg::is_gpr(r) {
            store_gpr(cx, gp(r), to, 8);
        } else {
            store_fpr(cx, xm(r), to, 16);
        }
    }
    for i in 0..ops.len {
        if let (Some(Place::Flag(cond)), Some(slot)) = (plan.places[i], plan.slots[i]) {
            put(cx, mc::setcc_r(cond, R11));
            put(cx, mc::movzx_rr(Size::D, R11, Size::B, R11));
            store_gpr(cx, R11, slot, 8);
        }
    }
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output || !op.indirect {
            continue;
        }
        let (Some(Place::Reg(r)), Some(ptr), Some(value)) =
            (plan.places[i], plan.slots[i], plan.values[i])
        else {
            continue;
        };
        let bytes = reg_bytes(cx, r, plan.tys[i]);
        load_gpr(cx, R11, ptr, 8);
        let m = mc::Mem::base(R11, 0);
        if reg::is_gpr(r) {
            load_gpr(cx, R10, value, 8);
            let width = crate::common::exact_size(bytes).unwrap_or(Size::Q);
            put(cx, mc::mov_mr(width, m, R10));
        } else {
            load_fpr(cx, XMM15, value, 16);
            put(
                cx,
                match bytes {
                    4 => mc::movs_mr(mc::FpSize::S, m, XMM15),
                    8 => mc::movs_mr(mc::FpSize::D, m, XMM15),
                    _ => mc::movup_mr(mc::FpSize::S, m, XMM15),
                },
            );
        }
    }
    for r in &taken {
        cx.free_temp(*r);
    }
    // 6. The result.
    result(cx, inst, ops, &plan)?;
    if inst.opcode() == Opcode::Invoke {
        let Some(normal) = site.normal_dest() else {
            return cx.bug("an invoke without a normal destination");
        };
        cx.phi_stores(normal)?;
        if !cx.falls_through(normal) {
            branch(cx, cx.block_label(normal))?;
        }
    }
    Ok(())
}

/// Step 6: the call's result from the output slots.
fn result<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ops: &Ops, plan: &Plan) -> CgResult<()> {
    if inst.ty() == TypeId::VOID {
        return Ok(());
    }
    let mut fields = [(0usize, TypeId::VOID); MAX_OPS];
    let mut n = 0;
    for i in 0..ops.len {
        if ops.list[i].kind == Kind::Output && !ops.list[i].indirect {
            fields[n] = (i, plan.tys[i]);
            n += 1;
        }
    }
    let class = cx.class_of(inst.result());
    if n == 1 && class != Class::Mem {
        let (i, ty) = fields[0];
        let Some(slot) = plan.slots[i] else { return cx.bug("an output without a slot") };
        let bytes = (cx.types().alloc_size_of(ty) as u32).max(1);
        let r = cx.def_reg(inst)?;
        load_into(cx, r, slot, bytes);
        cx.finish(inst, r)?;
        return Ok(());
    }
    let dst = cx.def_mem(inst)?;
    for (field, &(i, ty)) in fields[..n].iter().enumerate() {
        let Some(slot) = plan.slots[i] else { return cx.bug("an output without a slot") };
        let bytes = cx.types().size_of(ty) as u32;
        let offset = if n == 1 { 0 } else { cx.types().field_offset(inst.ty(), field as u32) };
        copy_frame(cx, dst.offset_by(offset as u32), slot, bytes.max(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_parse() {
        let ops = parse(b"={ax},={bx},={cx},={dx},{ax},{cx},~{dirflag},~{fpsr},~{flags}").unwrap();
        assert_eq!(ops.len, 6);
        assert_eq!(ops.direct_outputs, 4);
        assert_eq!(ops.list[1].alts[0].reg, Some(reg::RBX));
        let ops = parse(b"=r,=*m,0,*m,~{memory},~{cc},~{rbx},~{xmm8},~{st(1)}").unwrap();
        assert!(has(ops.clobbers, reg::RBX));
        assert!(has(ops.clobbers, reg::XMM8));
        assert_eq!(ops.list[2].alts[0].tied, 0);
        assert!(parse(b"={@ccz},r").unwrap().list[0].alts[0].cond == Some(Cond::E));
        assert!(parse(b"{rsp}").is_err());
        assert!(parse(b"~{rbp}").is_err());
        assert!(parse(b"{ah}").is_err());
        assert!(parse(b"t").is_err());
        assert!(parse(b"A").is_err());
        assert!(parse(b"~{k1}").is_err());
        assert!(parse(b"r,=r").is_err());
    }
}
