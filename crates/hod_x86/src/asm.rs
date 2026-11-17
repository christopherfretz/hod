//! Inline assembly in 32-bit mode (`spec/asm.md` §22, design 0008's
//! I13, WP-301): the operands are placed as the constraints ask, the
//! template is substituted as LLVM's `X86AsmPrinter` prints it for
//! `i686` in AT&T syntax and assembled by `hod_asm::x86` (the x86-64
//! assembler through `Mode::admit`), and the outputs are taken back.
//!
//! **Owner: WP-301.** Tests: `crates/hod/tests/i386_asm.rs`. Hook:
//! `call::lower`'s asm arm. The shape is `hod_x64`'s
//! `barrier::template` (WP-187), restricted to general registers:
//!
//! 1. Every input placed in a register or memory (and every pointer of
//!    an indirect operand) is evaluated and stored to a frame slot of its
//!    own (8 bytes; an `A` pair's 8 bytes).
//! 2. Every register value is displaced; every holdable register the
//!    template may write (`ebx`, `esi`, `edi`: operand registers,
//!    clobbers) is taken. The prologue saves all three in every function.
//! 3. The inputs are loaded into their registers (an integer narrower
//!    than 32 bits zero-extended), the memory operands' addresses put in
//!    theirs (`lea` of the slot, or the pointer).
//! 4. The template's bytes and relocations.
//! 5. Every register output is stored to its slot; then each flag output
//!    (`={@cc<cond>}`) is `setcc al` and stored; then each indirect
//!    register output is stored through its pointer (`ecx`, the value in
//!    `eax`).
//! 6. The result is loaded from its output slot, or the fields of the
//!    struct of several are copied from theirs.
//!
//! # Registers
//!
//! Six: `eax`, `ecx`, `edx`, `esi`, `edi`, `ebx`, taken in that order
//! (`q` and `Q` from `eax`..`ebx`, the four with a byte form; `R` and
//! `r` from all six). Each operand gets one of its own, distinct from
//! every other operand's and every clobber, except a tied input, which
//! is in its output's register; a template that needs more is refused
//! ([`REGISTERS`]), never given a register twice. An explicit register
//! (`{eax}`, `{ax}`, `{al}`) is that register. `esp` and `ebp`, as an
//! operand or a clobber, are [`FRAME_REGISTER`]. `A` is `edx:eax`: a
//! 64-bit operand is its low half in `eax`, its high half in `edx`; a
//! narrower one is in `eax` with `edx` written.

use hod_asm::x64::Arg;
use hod_asm::{Refusal, reason, template, x86 as asm};
use hod_cg::asm_constraints::{
    self as constraints, Clobber, ConstraintError, ConstraintSyntax, Kind, MAX_OPS, TIED,
};
use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef, PReg, SymRef};
use hod_ir::{Const, InlineAsm, Inst, Opcode, TypeId, TypeKind, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Reg, Size};
use hod_obj::{I386Reloc, RelocKind};

use crate::common::{
    copy_frame, frame_address, gp, load_gpr, put, store_gpr, unsupported, value_ty,
};
use crate::reg;
use crate::target::X86;

/// An operand in `esp` or `ebp`, or a clobber of either: hod's frame is
/// addressed from them.
pub const FRAME_REGISTER: &str = "x86:inline-asm:frame-register";
/// More operands than registers: six general registers, fewer with
/// explicit registers and clobbers, four for `q`/`Q`.
pub const REGISTERS: &str = "x86:inline-asm:registers";
/// An operand of a type no general register holds: a floating-point or
/// 64-bit output or tied operand in a register (64 only with `A`), an
/// integer of other than 1, 8, 16, 32, 64 bits in a register, a memory
/// input of other than an integer of at most 64 bits, `float`, `double`,
/// `x86_fp80` (`i128`, `half`, `fp128`, a vector, an aggregate), an
/// indirect 64-bit register input, tied operands of different sizes.
pub const OPERAND_TYPE: &str = "x86:inline-asm:operand-type";
/// The x87 stack constraints `t`, `u`, `f` and the `st` registers as
/// operands.
pub const X87: &str = "x86:inline-asm:x87";
/// SSE and MMX constraints (`x`, `v`, `y`, `Y..`): I12's.
pub const VECTOR: &str = "x86:inline-asm:vector";
/// A symbol operand (`s`, `Ws`, `i` of an address): its name is printed
/// as an immediate (`$sym`), which the 32-bit assembler does not take.
pub const SYMBOL: &str = "x86:inline-asm:symbol";
/// Intel-dialect inline assembly (`inteldialect`): only AT&T is
/// assembled, as on x86-64.
pub const INTEL: &str = "x86:inline-asm:intel";
/// `asm goto` (`callbr`).
pub const GOTO: &str = "x86:inline-asm:goto";
/// Inline assembly called by `invoke`, or with operand bundles.
pub const CALL: &str = "x86:inline-asm:call";
/// Alternatives whose tied operands `hod_cg` cannot pair.
pub const ALTERNATIVES: &str = "x86:inline-asm:alternatives";

/// Every name this module refuses with, besides `hod_asm`'s `asm:*` and
/// the `x86-mode:*` names of `Mode::admit`.
pub const UNSUPPORTED: &[&str] = &[
    FRAME_REGISTER,
    REGISTERS,
    OPERAND_TYPE,
    X87,
    VECTOR,
    SYMBOL,
    INTEL,
    GOTO,
    CALL,
    ALTERNATIVES,
];

fn refused(r: Refusal) -> CgError {
    CgError::unsupported(r.0)
}

// ---- constraints ----

const R: u32 = 1 << 0; // r, l, R: any of the six
const ABCD: u32 = 1 << 1; // q, Q: eax, ecx, edx, ebx
const M: u32 = 1 << 2; // m, o
const XX: u32 = 1 << 3; // X: anything
const IMM_ANY: u32 = 1 << 4; // i, n
const IMM_E: u32 = 1 << 5; // e: a signed 32-bit constant
const IMM_Z: u32 = 1 << 6; // Z: an unsigned 32-bit constant
const IMM_I: u32 = 1 << 7; // I: 0..31
const IMM_J: u32 = 1 << 8; // J: 0..63
const IMM_K: u32 = 1 << 9; // K: a signed 8-bit constant
const IMM_L: u32 = 1 << 10; // L: 0xff, 0xffff, 0xffffffff
const IMM_M: u32 = 1 << 11; // M: 0..3
const IMM_N: u32 = 1 << 12; // N: 0..255
const IMM_O: u32 = 1 << 13; // O: 0..127
const PAIR: u32 = 1 << 14; // A: edx:eax
const STACK: u32 = 1 << 15; // t, u, f: refused (X87)
const VEC: u32 = 1 << 16; // x, v, y, Y.: refused (VECTOR)
const SYM: u32 = 1 << 17; // s, Ws: refused (SYMBOL)

const IMMS: [u32; 10] = [IMM_ANY, IMM_E, IMM_Z, IMM_I, IMM_J, IMM_K, IMM_L, IMM_M, IMM_N, IMM_O];
const ANY_IMM: u32 =
    IMM_ANY | IMM_E | IMM_Z | IMM_I | IMM_J | IMM_K | IMM_L | IMM_M | IMM_N | IMM_O;

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
    /// A general register.
    Reg(PReg),
    /// `edx:eax`, a 64-bit value.
    Pair,
    /// A 64-bit input in two general registers, its low half in the
    /// first (WP-347): `llc` gives an `i64` `r`/`q` operand the next two
    /// free registers of the order and prints the low one.
    Halves(PReg, PReg),
    /// Memory whose address is in this general register.
    Mem(PReg),
    /// A constant.
    Imm(i64),
    /// A flag output.
    Flag(Cond),
}

/// The general register a constraint name denotes (`eax`, `ax`, `al`;
/// `ah`..`dh` are `Ok(None)`, as no operand is placed there).
fn named_reg(name: &[u8]) -> Result<Option<PReg>, CgError> {
    if name.starts_with(b"st") {
        return Err(CgError::unsupported(X87));
    }
    if name.starts_with(b"xmm") || name.starts_with(b"mm") {
        return Err(CgError::unsupported(VECTOR));
    }
    let name = std::str::from_utf8(name).unwrap_or("");
    match hod_asm::x64::operand::reg_name(name) {
        Some(hod_asm::x64::operand::RegName::Gpr(g)) => {
            let ok = match g.size {
                Size::Q => false,
                Size::B => g.reg.code() < 4,
                _ => g.reg.code() < 8,
            };
            match g.reg {
                _ if !ok => Ok(None),
                Reg::Rsp | Reg::Rbp => Err(CgError::unsupported(FRAME_REGISTER)),
                r => Ok(Some(reg::gpr(r))),
            }
        }
        _ => Ok(None),
    }
}

/// i386's part of a constraint string.
struct X86Syntax;

impl ConstraintSyntax for X86Syntax {
    type Cond = Cond;

    fn letters(&self, code: &[u8], _output: bool) -> Option<(u32, usize)> {
        if code.starts_with(b"^Ws") {
            return Some((SYM, 3));
        }
        if code.starts_with(b"Ws") {
            return Some((SYM, 2));
        }
        if code.starts_with(b"Y") {
            return Some((VEC, code.len().min(2)));
        }
        let bits = match code.first()? {
            b's' => SYM,
            b'r' | b'l' | b'R' => R,
            b'q' | b'Q' => ABCD,
            b'A' => PAIR,
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
            b't' | b'u' | b'f' => STACK,
            b'x' | b'v' | b'y' => VEC,
            _ => return None,
        };
        Some((bits, 1))
    }

    fn flag(&self, cond: &[u8]) -> Option<Cond> {
        hod_asm::x64::encode::cond_name(std::str::from_utf8(cond).ok()?)
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
        // Nothing of hod's lives in the x87, MMX or SSE registers across
        // an instruction in this back end (I9 and I12 change that).
        if name.starts_with(b"st(") || name.starts_with(b"mm") || name.starts_with(b"xmm") {
            return Ok(Clobber::Ignored);
        }
        if let b"ah" | b"bh" | b"ch" | b"dh" = name {
            return Ok(Clobber::Reg(named_reg(&[name[0], b'x'])?.expect("a register")));
        }
        match named_reg(name)? {
            Some(r) => Ok(Clobber::Reg(r)),
            None => Ok(Clobber::Unknown),
        }
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or("other")
}

fn parse(constraints: &[u8]) -> CgResult<Ops> {
    constraints::parse(&X86Syntax, constraints).map_err(|e| match e {
        ConstraintError::Constraint(t) => refused(reason::constraint(text(t))),
        ConstraintError::Letter(c) => refused(reason::constraint(&char::from(c).to_string())),
        ConstraintError::Clobber(t) => refused(reason::clobber(text(t))),
        ConstraintError::TooMany => CgError::unsupported(REGISTERS),
        ConstraintError::Target(e) => e,
    })
}

/// The value of an integer constant, sign-extended.
fn const_int(cx: &FnCx<'_, X86>, value: Value) -> Option<i64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => {
            let TypeKind::Int(bits) = cx.types().kind(ty) else { return None };
            Some(hod_ir::sext64(value, bits))
        }
        _ => None,
    }
}

/// What code `code` weighs for `operand`, as `X86TargetLowering` weighs
/// it (`None`: it cannot take it).
fn code_weight(cx: &FnCx<'_, X86>, operand: Option<Value>, code: u32) -> Option<i32> {
    let Some(value) = operand else { return Some(0) };
    let integer = matches!(cx.types().kind(value_ty(cx, value)), TypeKind::Int(_));
    match code {
        IMM_E | IMM_Z | IMM_I | IMM_J | IMM_K | IMM_L | IMM_M | IMM_N | IMM_O => {
            const_int(cx, value).filter(|&v| imm_fits(code, v)).map(|_| 3)
        }
        _ if integer => Some(0),
        IMM_ANY => const_int(cx, value).map(|_| 3),
        M => Some(2),
        R | ABCD => None,
        _ => Some(0),
    }
}

/// The order registers are given out in: `eax`, `ecx`, `edx`, `esi`,
/// `edi`, `ebx`.
const GPR_ORDER: [u8; 6] = [0, 1, 2, 6, 7, 3];

/// The registers of codes `codes`, as a mask of encodings.
fn gpr_mask(codes: u32) -> u32 {
    if codes & (R | XX | M) != 0 { 0xcf } else { 0x0f }
}

struct Plan {
    places: [Option<Place>; MAX_OPS],
    tys: [TypeId; MAX_OPS],
    /// An input's slot; a direct output's slot; an indirect operand's
    /// pointer slot.
    slots: [Option<FrameRef>; MAX_OPS],
    /// An indirect register output's value slot.
    values: [Option<FrameRef>; MAX_OPS],
    /// The registers the template may write, by `PReg` index.
    written: u64,
    /// Inputs tied to an output in memory: their value is not read.
    unread: u32,
}

fn has(set: u64, r: PReg) -> bool {
    set & (1 << r.index()) != 0
}

fn operand_types(
    cx: &FnCx<'_, X86>,
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

/// The bytes of a general register operand of type `ty`.
fn gpr_bytes(cx: &FnCx<'_, X86>, ty: TypeId) -> Option<u8> {
    match cx.types().kind(ty) {
        TypeKind::Int(bits @ (8 | 16 | 32)) => Some((bits / 8) as u8),
        TypeKind::Int(1) => Some(1),
        TypeKind::Ptr { addr_space: 0 } => Some(4),
        _ => None,
    }
}

/// Whether a register operand `op` of type `ty` is a direct `i64` input,
/// placed in two registers ([`Place::Halves`]).
fn in_halves(cx: &FnCx<'_, X86>, op: &Op, ty: TypeId) -> bool {
    op.kind == Kind::Input && !op.indirect && matches!(cx.types().kind(ty), TypeKind::Int(64))
}

/// Whether a direct memory input of class `Mem` and type `ty` is copied
/// to a slot of its own whose address the template sees, as `llc` copies
/// it to a stack object (WP-347): an integer of 33 to 64 bits, `float`,
/// `double`, `x86_fp80`.
fn copied_to_memory(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    match cx.types().kind(ty) {
        TypeKind::Int(bits) => bits <= 64,
        TypeKind::Float | TypeKind::Double | TypeKind::X86Fp80 => true,
        _ => false,
    }
}

fn takes_memory(op: &Op, alt: Alt, tied_by: bool) -> bool {
    let output = op.kind == Kind::Output;
    let register = alt.codes & (R | ABCD | XX) != 0;
    alt.codes & M != 0 && (!output || op.indirect) && !(output && tied_by && register)
}

/// The place of an operand that needs no register choice, or `None`
/// with `wants` set when it needs one (`Some(false)` memory, `Some(true)`
/// a register).
fn first_place(
    cx: &FnCx<'_, X86>,
    op: &Op,
    alt: Alt,
    value: Option<Value>,
    tied_by: bool,
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
    if alt.codes & STACK != 0 {
        return unsupported(X87);
    }
    if alt.codes & VEC != 0 {
        return unsupported(VECTOR);
    }
    if alt.codes & SYM != 0 {
        return unsupported(SYMBOL);
    }
    if alt.codes & PAIR != 0 {
        return Ok(Some(Place::Pair));
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
            return Err(refused(reason::constraint("range")));
        }
    }
    if takes_memory(op, alt, tied_by) {
        *wants = Some(false);
        return Ok(None);
    }
    if alt.codes & (R | ABCD | XX) != 0 {
        *wants = Some(true);
        return Ok(None);
    }
    if alt.codes & ANY_IMM != 0 {
        if direct_value.is_some_and(|v| {
            matches!(v.kind(), ValueKind::Sym(_))
                || matches!(v.kind(), ValueKind::Const(id)
                    if matches!(cx.module.consts.get(id), Const::Addr { .. }))
        }) {
            return unsupported(SYMBOL);
        }
        return Err(refused(reason::constraint("i")));
    }
    Err(refused(reason::constraint(if alt.codes & M != 0 { "m" } else { "other" })))
}

#[allow(clippy::too_many_lines, reason = "the choice of the module documentation, in order")]
fn decide(cx: &FnCx<'_, X86>, inst: Inst<'_>, ops: &Ops) -> CgResult<Plan> {
    let site = inst.call().expect("checked by the caller");
    let args = site.args();
    let tys = operand_types(cx, inst, site, ops)?;
    let ties = ops.ties().map_err(|e| match e {
        constraints::AltError::Parse => refused(reason::constraint("tied")),
        constraints::AltError::Count => CgError::unsupported(ALTERNATIVES),
    })?;
    let chosen = ops.choose(args, |operand, codes| {
        let mut w = None;
        let mut bits = codes;
        while bits != 0 {
            let code = bits & bits.wrapping_neg();
            bits &= bits - 1;
            w = w.max(code_weight(cx, operand, code));
        }
        w
    });
    let mut plan = Plan {
        places: [None; MAX_OPS],
        tys,
        slots: [None; MAX_OPS],
        values: [None; MAX_OPS],
        written: ops.clobbers,
        unread: 0,
    };
    let mut wants = [None::<bool>; MAX_OPS];
    for (i, op) in ops.ops().iter().enumerate() {
        let tied_by = op.kind == Kind::Output && ties.matching(i, chosen).is_some();
        let alt = op.alt(chosen);
        let value = op.arg.map(|a| args[usize::from(a)]);
        plan.places[i] = first_place(cx, op, alt, value, tied_by, &mut wants[i])?;
        if tied_by && takes_memory(op, alt, tied_by) {
            wants[i] = Some(false);
        }
    }
    // Registers placed explicitly, in a pair, or clobbered are not chosen.
    let mut taken = ops.clobbers | crate::coro_body::asm_reserved(cx);
    for place in plan.places[..ops.len].iter().flatten() {
        match place {
            Place::Reg(r) => taken |= 1 << r.index(),
            Place::Pair => taken |= (1 << reg::EAX.index()) | (1 << reg::EDX.index()),
            _ => {}
        }
    }
    for (i, want) in wants.iter().enumerate().take(ops.len) {
        let Some(register) = *want else { continue };
        let alt = ops.list[i].alt(chosen);
        let mask = if register { gpr_mask(alt.codes) } else { gpr_mask(R) };
        let mut next = || {
            let r = GPR_ORDER
                .iter()
                .map(|&n| PReg::new(n))
                .find(|&r| (mask >> r.index()) & 1 != 0 && !has(taken, r));
            if let Some(r) = r {
                taken |= 1 << r.index();
            }
            r
        };
        let Some(r) = next() else { return unsupported(REGISTERS) };
        plan.places[i] = Some(if !register {
            Place::Mem(r)
        } else if in_halves(cx, &ops.list[i], plan.tys[i]) {
            let Some(high) = next() else { return unsupported(REGISTERS) };
            Place::Halves(r, high)
        } else {
            Place::Reg(r)
        });
    }
    for (i, op) in ops.ops().iter().enumerate() {
        let alt = op.alt(chosen);
        if op.kind == Kind::Input && alt.codes & TIED != 0 && plan.places[i].is_none() {
            let out = usize::from(alt.tied);
            plan.places[i] = match plan.places[out] {
                Some(Place::Reg(r)) => {
                    if gpr_bytes(cx, plan.tys[i]).is_none() {
                        return unsupported(OPERAND_TYPE);
                    }
                    Some(Place::Reg(r))
                }
                Some(Place::Pair) => Some(Place::Pair),
                Some(Place::Mem(r)) => {
                    plan.unread |= 1 << i;
                    Some(Place::Mem(r))
                }
                _ => return Err(refused(reason::constraint("tied"))),
            };
        }
    }
    for (i, op) in ops.ops().iter().enumerate() {
        let ty = plan.tys[i];
        match plan.places[i] {
            Some(Place::Reg(r)) => {
                if !reg::is_gpr(r) || gpr_bytes(cx, ty).is_none() {
                    return unsupported(OPERAND_TYPE);
                }
                plan.written |= 1 << r.index();
            }
            Some(Place::Pair) => {
                let bytes = cx.types().alloc_size_of(ty);
                let integer = matches!(cx.types().kind(ty), TypeKind::Int(_));
                if !(gpr_bytes(cx, ty).is_some() || (integer && bytes == 8)) {
                    return unsupported(OPERAND_TYPE);
                }
                plan.written |= (1 << reg::EAX.index()) | (1 << reg::EDX.index());
            }
            Some(Place::Halves(low, high)) => {
                plan.written |= (1 << low.index()) | (1 << high.index());
            }
            Some(Place::Mem(r)) => {
                plan.written |= 1 << r.index();
                if !op.indirect && op.kind == Kind::Input && plan.unread & (1 << i) == 0 {
                    let value = args[usize::from(op.arg.expect("an input has an operand"))];
                    if cx.class_of(value) != Class::Gpr && !copied_to_memory(cx, ty) {
                        return unsupported(OPERAND_TYPE);
                    }
                }
            }
            Some(Place::Flag(_)) => {
                if !matches!(cx.types().kind(ty), TypeKind::Int(8 | 16 | 32)) || op.indirect {
                    return unsupported(OPERAND_TYPE);
                }
            }
            Some(Place::Imm(_)) => {}
            None => return cx.bug("an inline-assembly operand without a place"),
        }
    }
    if has(plan.written, reg::ESP) || has(plan.written, reg::EBP) {
        return unsupported(FRAME_REGISTER);
    }
    crate::coro_body::check_asm(cx, plan.written)?;
    Ok(plan)
}

// ---- the lowering ----

/// The asm arm of `call::lower`: `inst` calls inline assembly.
///
/// # Errors
/// The names of [`UNSUPPORTED`], `hod_asm`'s `asm:*` and the
/// `x86-mode:*` names of the template's statements.
pub fn call<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
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
    if inst.opcode() == Opcode::CallBr {
        return unsupported(GOTO);
    }
    if site.bundle_count() != 0 || inst.opcode() == Opcode::Invoke {
        return unsupported(CALL);
    }
    let ops = parse(asm.constraints)?;
    if ops.labels != 0 {
        return unsupported(GOTO);
    }
    let args = site.args();
    let operands = ops.ops().iter().filter(|op| op.arg.is_some()).count();
    if operands != args.len() || (ops.direct_outputs == 0) != (inst.ty() == TypeId::VOID) {
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
        let Some(sym) = cx.module.find_symbol(r.name.as_bytes()) else {
            return Err(refused(Refusal(reason::other::SYMBOL)));
        };
        let kind = match r.kind {
            hod_asm::x64::RefKind::Pc32 => I386Reloc::Pc32,
            hod_asm::x64::RefKind::Plt32 => I386Reloc::Plt32,
            hod_asm::x64::RefKind::Abs32S => I386Reloc::Abs32,
            hod_asm::x64::RefKind::None => I386Reloc::None,
            _ => return Err(refused(Refusal(reason::operand::SPECIFIER))),
        };
        fields.push((r.offset, kind, sym, r.addend));
    }
    emit(cx, inst, &ops, plan, &code, &fields)
}

fn substitute(
    cx: &FnCx<'_, X86>,
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
            Some(Place::Reg(p)) => Arg::Gp(p.index(), gpr_bytes(cx, plan.tys[i]).unwrap_or(4)),
            // `X86AsmPrinter` prints an `A` operand as its first register.
            Some(Place::Pair) => Arg::Gp(0, 4),
            // `llc` prints a two-register operand as its low register.
            Some(Place::Halves(low, _)) => Arg::Gp(low.index(), 4),
            Some(Place::Mem(p)) => Arg::Mem(p.index()),
            Some(Place::Imm(v)) => Arg::Imm(v),
            Some(Place::Flag(_)) | None => return Err(Refusal(reason::other::TEMPLATE)),
        };
        asm::print_operand(arg, r.modifier, out)
    })
    .map_err(refused)
}

#[allow(clippy::too_many_lines, reason = "the sequence of the module documentation, in order")]
fn emit<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    ops: &Ops,
    mut plan: Plan,
    code: &[u8],
    fields: &[(u32, I386Reloc, hod_ir::SymId, i64)],
) -> CgResult<()> {
    let site = inst.call().expect("checked by the caller");
    let args = site.args();
    // Deferred operands are emitted before anything is placed.
    for &operand in inst.operands() {
        if cx.deferred(operand).is_some() {
            let r = cx.use_reg(operand)?;
            cx.free_temp(r);
        }
    }
    // 1. Inputs and pointers to slots, one per distinct value.
    let mut stored: [(Value, Option<FrameRef>); MAX_OPS] = [(Value::NONE, None); MAX_OPS];
    let mut nstored = 0;
    for i in 0..ops.len {
        let op = ops.list[i];
        let Some(a) = op.arg else { continue };
        if !matches!(
            plan.places[i],
            Some(Place::Reg(_) | Place::Mem(_) | Place::Pair | Place::Halves(..))
        ) || plan.unread & (1 << i) != 0
        {
            continue;
        }
        let value = args[usize::from(a)];
        if let Some(&(_, slot)) = stored[..nstored].iter().find(|(v, _)| *v == value) {
            plan.slots[i] = slot;
            continue;
        }
        let class = cx.class_of(value);
        // A memory input of class `Mem` (WP-347): a copy of its bytes.
        let copy = class == Class::Mem
            && !op.indirect
            && matches!(plan.places[i], Some(Place::Mem(_)))
            && copied_to_memory(cx, plan.tys[i]);
        let slot_bytes = if copy { cx.types().alloc_size_of(plan.tys[i]).max(8) } else { 8 };
        let slot = cx.alloc_slot(slot_bytes, 4)?;
        match class {
            Class::Gpr => {
                let r = cx.use_reg(value)?;
                store_gpr(cx, gp(r), slot, 4);
                cx.free_temp(r);
            }
            // A 64-bit `A` or two-register input: its 8 bytes.
            Class::Mem
                if matches!(plan.places[i], Some(Place::Pair | Place::Halves(..)))
                    && !op.indirect =>
            {
                let m = cx.use_mem(value)?;
                copy_frame(cx, slot, m, 8);
            }
            Class::Mem if copy => {
                let m = cx.use_mem(value)?;
                let bytes = u32::try_from(cx.types().alloc_size_of(plan.tys[i]))
                    .map_err(|_| CgError::unsupported(OPERAND_TYPE))?;
                copy_frame(cx, slot, m, bytes);
            }
            _ => return unsupported(OPERAND_TYPE),
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
            plan.slots[i] = Some(cx.alloc_slot(8, 4)?);
        } else if matches!(plan.places[i], Some(Place::Reg(_) | Place::Pair)) {
            plan.values[i] = Some(cx.alloc_slot(8, 4)?);
        }
    }
    // 2. Registers.
    cx.displace_all()?;
    let holdable = cx.reg_info().holdable;
    let mut taken = Vec::new();
    for n in 0..8u8 {
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
        let bytes = u32::from(gpr_bytes(cx, plan.tys[i]).unwrap_or(4));
        match plan.places[i] {
            Some(Place::Reg(r)) if op.indirect => {
                load_gpr(cx, gp(r), slot, 4);
                let m = mc::Mem::base(gp(r), 0);
                put(
                    cx,
                    match bytes {
                        1 => mc::movzx_rm(Size::D, gp(r), Size::B, m),
                        2 => mc::movzx_rm(Size::D, gp(r), Size::W, m),
                        _ => mc::mov_rm(Size::D, gp(r), m),
                    },
                );
            }
            Some(Place::Reg(r)) => {
                load_gpr(cx, gp(r), slot, bytes);
                if matches!(cx.types().kind(plan.tys[i]), TypeKind::Int(1)) {
                    put(cx, mc::and_ri(Size::D, gp(r), 1));
                }
            }
            Some(Place::Pair) if op.indirect => {
                // The pointer in `edx`, then the value through it.
                load_gpr(cx, crate::common::EDX, slot, 4);
                let m = |d| mc::Mem::base(crate::common::EDX, d);
                put(cx, mc::mov_rm(Size::D, crate::common::EAX, m(0)));
                if cx.types().alloc_size_of(plan.tys[i]) == 8 {
                    put(cx, mc::mov_rm(Size::D, crate::common::EDX, m(4)));
                }
            }
            Some(Place::Pair) => {
                if cx.types().alloc_size_of(plan.tys[i]) == 8 {
                    load_gpr(cx, crate::common::EAX, slot, 4);
                    load_gpr(cx, crate::common::EDX, slot.offset_by(4), 4);
                } else {
                    load_gpr(cx, crate::common::EAX, slot, bytes);
                }
            }
            Some(Place::Halves(low, high)) => {
                load_gpr(cx, gp(low), slot, 4);
                load_gpr(cx, gp(high), slot.offset_by(4), 4);
            }
            Some(Place::Mem(r)) if op.indirect => load_gpr(cx, gp(r), slot, 4),
            Some(Place::Mem(r)) => frame_address(cx, gp(r), slot),
            _ => {}
        }
    }
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind == Kind::Output
            && op.indirect
            && let (Some(Place::Mem(r)), Some(slot)) = (plan.places[i], plan.slots[i])
        {
            load_gpr(cx, gp(r), slot, 4);
        }
    }
    // 4. The template.
    let start = cx.offset();
    cx.emit(code);
    for &(offset, kind, sym, addend) in fields {
        cx.reloc(start + offset, RelocKind::I386(kind), SymRef::Module(sym), addend);
    }
    // 5. Outputs.
    outputs(cx, ops, &plan)?;
    for r in &taken {
        cx.free_temp(*r);
    }
    // 6. The result.
    result(cx, inst, ops, &plan)
}

fn outputs(cx: &mut FnCx<'_, X86>, ops: &Ops, plan: &Plan) -> CgResult<()> {
    use crate::common::{EAX, ECX, EDX};
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output {
            continue;
        }
        let to = if op.indirect { plan.values[i] } else { plan.slots[i] };
        match plan.places[i] {
            Some(Place::Reg(r)) => {
                let Some(to) = to else {
                    return cx.bug("an inline-assembly output without a slot");
                };
                store_gpr(cx, gp(r), to, 4);
            }
            Some(Place::Pair) => {
                let Some(to) = to else {
                    return cx.bug("an inline-assembly output without a slot");
                };
                store_gpr(cx, EAX, to, 4);
                store_gpr(cx, EDX, to.offset_by(4), 4);
            }
            _ => {}
        }
    }
    for i in 0..ops.len {
        if let (Some(Place::Flag(cond)), Some(slot)) = (plan.places[i], plan.slots[i]) {
            put(cx, mc::setcc_r(cond, EAX));
            put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
            store_gpr(cx, EAX, slot, 4);
        }
    }
    for i in 0..ops.len {
        let op = ops.list[i];
        if op.kind != Kind::Output || !op.indirect {
            continue;
        }
        let (Some(place @ (Place::Reg(_) | Place::Pair)), Some(ptr), Some(value)) =
            (plan.places[i], plan.slots[i], plan.values[i])
        else {
            continue;
        };
        load_gpr(cx, ECX, ptr, 4);
        let m = |d| mc::Mem::base(ECX, d);
        load_gpr(cx, EAX, value, 4);
        let bytes = cx.types().alloc_size_of(plan.tys[i]);
        if place == Place::Pair && bytes == 8 {
            put(cx, mc::mov_mr(Size::D, m(0), EAX));
            load_gpr(cx, EAX, value.offset_by(4), 4);
            put(cx, mc::mov_mr(Size::D, m(4), EAX));
            continue;
        }
        let width = match bytes {
            1 => Size::B,
            2 => Size::W,
            _ => Size::D,
        };
        put(cx, mc::mov_mr(width, m(0), EAX));
    }
    Ok(())
}

fn result<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ops: &Ops, plan: &Plan) -> CgResult<()> {
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
    if n == 1 && class == Class::Gpr {
        let (i, ty) = fields[0];
        let Some(slot) = plan.slots[i] else { return cx.bug("an output without a slot") };
        let bytes = (cx.types().alloc_size_of(ty) as u32).clamp(1, 4);
        let r = cx.def_reg(inst)?;
        load_gpr(cx, gp(r), slot, bytes);
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
        let ops = parse(b"={ax},={bx},={cx},={dx},{ax},~{dirflag},~{fpsr},~{flags}").unwrap();
        assert_eq!(ops.len, 5);
        assert_eq!(ops.list[1].alts[0].reg, Some(reg::EBX));
        let ops = parse(b"=r,=*m,0,*m,~{memory},~{cc},~{ebx},~{xmm1}").unwrap();
        assert!(has(ops.clobbers, reg::EBX));
        assert_eq!(ops.list[2].alts[0].tied, 0);
        assert_eq!(parse(b"=A").unwrap().list[0].alts[0].codes, PAIR);
        assert_eq!(parse(b"=q,Q").unwrap().list[1].alts[0].codes, ABCD);
        assert!(parse(b"{esp}").is_err());
        assert!(parse(b"~{ebp}").is_err());
        assert!(parse(b"{st}").is_err());
        assert!(parse(b"~{rax}").is_err());
        assert!(parse(b"~{r8d}").is_err());
        assert!(parse(b"r,=r").is_err());
    }

    #[test]
    fn names_are_stable() {
        for name in UNSUPPORTED {
            assert!(name.starts_with("x86:inline-asm:"), "{name}");
        }
    }
}
