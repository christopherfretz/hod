//! AArch64 instruction statements to instruction words (`spec/asm.md`
//! §5.2, the mnemonic table): each accepted form is one call of a
//! `hod_mc_a64` encoder, through its checked (`try_`) form where the
//! encoder has one. The choices LLVM's assembler makes for a written
//! form are made the same way and are named where they are made (a
//! negative `add` immediate is a `sub`, an offset the scaled form does
//! not take is the unscaled form, a register operand next to `sp` is the
//! extended-register form); `tests/llvm_mc.rs` holds each of them to
//! LLVM 22's and 19's assembler.

use hod_mc_a64 as mc;
use hod_mc_a64::{
    Barrier, Cond, Extend, FpFormat, FpSize, Load, MemSize, Order, RegSp, RegZr, Shift, Size,
    VMemSize, VPairSize, VReg,
};

use super::operand::{Gp, IndexExt, Mem, Offset, Op, View};
use super::{Features, RefKind, sysreg};
use crate::expr::{Name, Value};
use crate::reason::{self, Refusal, operand, other};

type R<T> = Result<T, Refusal>;

/// One encoded instruction: its word, and the reference its
/// displacement or address field makes, if any (the field is zero in
/// `word`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Encoded<'a> {
    /// The instruction word.
    pub word: u32,
    /// The field to fill in, and what it refers to.
    pub fixup: Option<(RefKind, Value<'a>)>,
}

const fn word(word: u32) -> Encoded<'static> {
    Encoded { word, fixup: None }
}

fn fail<T>(what: &'static str) -> R<T> {
    Err(Refusal(what))
}

fn form<T>() -> R<T> {
    fail(operand::FORM)
}

fn checked(w: Option<u32>) -> R<Encoded<'static>> {
    w.map(word).ok_or(Refusal(operand::RANGE))
}

// ---- operand accessors ----

fn gp(op: &Op<'_>) -> R<Gp> {
    match op {
        Op::Gp(g) => Ok(*g),
        Op::Vector | Op::Fp(..) => fail(operand::REGISTER),
        _ => form(),
    }
}

fn size(g: Gp) -> Size {
    if g.is_x() { Size::X } else { Size::W }
}

/// A register in a position where 31 is the zero register.
fn zr(g: Gp) -> R<RegZr> {
    match g {
        Gp::X(n) | Gp::W(n) => Ok(mc::Reg::new(n).ok_or(Refusal(operand::REGISTER))?.zr()),
        Gp::Xzr | Gp::Wzr => Ok(RegZr::ZR),
        Gp::Sp | Gp::Wsp => fail(operand::REGISTER),
    }
}

/// A register in a position where 31 is the stack pointer.
fn sp(g: Gp) -> R<RegSp> {
    match g {
        Gp::X(n) | Gp::W(n) => Ok(mc::Reg::new(n).ok_or(Refusal(operand::REGISTER))?.sp()),
        Gp::Sp | Gp::Wsp => Ok(RegSp::SP),
        Gp::Xzr | Gp::Wzr => fail(operand::REGISTER),
    }
}

/// `g` as a general register of size `sz`.
fn sized(g: Gp, sz: Size) -> R<Gp> {
    if size(g) == sz { Ok(g) } else { fail(operand::REGISTER) }
}

fn zr_of(op: &Op<'_>, sz: Size) -> R<RegZr> {
    zr(sized(gp(op)?, sz)?)
}

/// An X-register base of a memory operand (`x0`..`x30`, `sp`).
fn base(m: &Mem) -> R<RegSp> {
    if !m.base.is_x() {
        return fail(operand::REGISTER);
    }
    sp(m.base)
}

fn imm(op: &Op<'_>) -> R<i64> {
    match op {
        Op::Imm(v, _) => Ok(*v),
        Op::Name(_) => fail(operand::SYMBOL),
        Op::FpImm(_) => fail(operand::FP_IMMEDIATE),
        _ => form(),
    }
}

/// An immediate in `0..=max`.
fn uimm(op: &Op<'_>, max: u64) -> R<u32> {
    let v = imm(op)?;
    if v < 0 || v as u64 > max {
        return fail(operand::RANGE);
    }
    Ok(v as u32)
}

/// A branch or address target: a label or a symbol.
fn target<'a>(op: &Op<'a>) -> R<Value<'a>> {
    match op {
        Op::Name(v) => Ok(*v),
        Op::Imm(..) => fail(operand::SYMBOL),
        _ => form(),
    }
}

fn fixed(w: u32, kind: RefKind, to: Value<'_>) -> Encoded<'_> {
    Encoded { word: w, fixup: Some((kind, to)) }
}

/// A condition name.
pub fn cond_name(name: &str) -> Option<Cond> {
    Some(match name.to_ascii_lowercase().as_str() {
        "eq" => Cond::Eq,
        "ne" => Cond::Ne,
        "cs" | "hs" => Cond::Hs,
        "cc" | "lo" => Cond::Lo,
        "mi" => Cond::Mi,
        "pl" => Cond::Pl,
        "vs" => Cond::Vs,
        "vc" => Cond::Vc,
        "hi" => Cond::Hi,
        "ls" => Cond::Ls,
        "ge" => Cond::Ge,
        "lt" => Cond::Lt,
        "gt" => Cond::Gt,
        "le" => Cond::Le,
        "al" => Cond::Al,
        "nv" => Cond::Nv,
        _ => return None,
    })
}

fn cond(op: &Op<'_>) -> R<Cond> {
    op.ident().and_then(cond_name).ok_or(Refusal(operand::FORM))
}

fn fp(op: &Op<'_>) -> R<(VReg, View)> {
    match op {
        Op::Fp(n, v) => Ok((VReg::new(*n).ok_or(Refusal(operand::REGISTER))?, *v)),
        Op::Vector => fail(operand::VECTOR),
        Op::Gp(_) => fail(operand::REGISTER),
        _ => form(),
    }
}

fn fp_size(v: View) -> R<FpSize> {
    match v {
        View::S => Ok(FpSize::S),
        View::D => Ok(FpSize::D),
        _ => fail(operand::REGISTER),
    }
}

/// FP registers of one view `S` or `D`, all the same.
fn fp_same(ops: &[Op<'_>]) -> R<(FpSize, [VReg; 4])> {
    let (_, view) = fp(&ops[0])?;
    let sz = fp_size(view)?;
    let mut regs = [VReg::V0; 4];
    for (i, op) in ops.iter().enumerate() {
        let (r, v) = fp(op)?;
        if v != view {
            return fail(operand::REGISTER);
        }
        regs[i] = r;
    }
    Ok((sz, regs))
}

// ---- the table ----

/// Encodes one instruction statement: `mnemonic` (any case) with its
/// operands.
///
/// # Errors
/// [`reason::mnemonic`] for a mnemonic not in the table; the
/// `asm:operand/*` names for operands it does not take;
/// [`other::FEATURE`] for an instruction `features` does not allow.
#[allow(clippy::too_many_lines, reason = "one arm per mnemonic family, read top to bottom")]
#[allow(clippy::many_single_char_names, reason = "operands named as in the instructions")]
pub fn encode<'a>(mnemonic: &str, ops: &[Op<'a>], features: Features) -> R<Encoded<'a>> {
    let mut buf = [0u8; 16];
    if mnemonic.len() > buf.len() {
        return Err(reason::mnemonic(mnemonic));
    }
    for (d, s) in buf.iter_mut().zip(mnemonic.bytes()) {
        *d = s.to_ascii_lowercase();
    }
    let m = std::str::from_utf8(&buf[..mnemonic.len()]).map_err(|_| reason::mnemonic("other"))?;
    let refuse = || Err(reason::mnemonic(m));
    if let Some(c) = m.strip_prefix("b.") {
        let Some(c) = cond_name(c) else { return refuse() };
        let [t] = ops else { return form() };
        return Ok(fixed(mc::b_cond(c, 0), RefKind::Branch19, target(t)?));
    }
    match m {
        "add" | "sub" | "adds" | "subs" => add_sub(m, ops),
        "cmp" | "cmn" => compare(m, ops),
        "neg" | "negs" => negate(m, ops),
        "mov" => mov(ops),
        "movz" | "movn" | "movk" => mov_wide(m, ops),
        "mvn" => logical_unary(ops),
        "and" | "orr" | "eor" | "ands" | "bic" | "bics" | "orn" | "eon" => logical(m, ops),
        "tst" => test(ops),
        "lsl" | "lsr" | "asr" | "ror" => shift(m, ops),
        "ubfx" | "sbfx" | "bfxil" | "ubfiz" | "sbfiz" | "bfi" | "ubfm" | "sbfm" | "bfm" => {
            bitfield(m, ops)
        }
        "sxtb" | "sxth" | "sxtw" | "uxtb" | "uxth" => extend(m, ops),
        "extr" => extr(ops),
        "rbit" | "rev" | "rev16" | "rev32" | "clz" | "cls" => unary(m, ops),
        "csel" | "csinc" | "csinv" | "csneg" => cond_select(m, ops),
        "cset" | "csetm" => cond_set(m, ops),
        "cinc" | "cinv" | "cneg" => cond_unary(m, ops),
        "ccmp" | "ccmn" => cond_compare(m, ops),
        "mul" | "mneg" | "sdiv" | "udiv" | "smulh" | "umulh" | "adc" | "adcs" | "sbc" | "sbcs"
        | "lslv" | "lsrv" | "asrv" | "rorv" => three(m, ops),
        "madd" | "msub" => four(m, ops),
        "smull" | "umull" | "smaddl" | "umaddl" | "smsubl" | "umsubl" => long_mul(m, ops),
        "crc32b" | "crc32h" | "crc32w" | "crc32x" | "crc32cb" | "crc32ch" | "crc32cw"
        | "crc32cx" => crc(m, ops, features),
        "b" | "bl" => {
            let [t] = ops else { return form() };
            let (w, kind) =
                if m == "b" { (mc::b(0), RefKind::Jump26) } else { (mc::bl(0), RefKind::Call26) };
            Ok(fixed(w, kind, target(t)?))
        }
        "cbz" | "cbnz" => {
            let [r, t] = ops else { return form() };
            let g = gp(r)?;
            let rt = zr(g)?;
            let w = if m == "cbz" { mc::cbz(size(g), rt, 0) } else { mc::cbnz(size(g), rt, 0) };
            Ok(fixed(w, RefKind::Branch19, target(t)?))
        }
        "tbz" | "tbnz" => {
            let [r, b, t] = ops else { return form() };
            let g = gp(r)?;
            let bit = uimm(b, if g.is_x() { 63 } else { 31 })?;
            let rt = zr(g)?;
            let w = if m == "tbz" { mc::try_tbz(rt, bit, 0) } else { mc::try_tbnz(rt, bit, 0) };
            Ok(fixed(w.ok_or(Refusal(operand::RANGE))?, RefKind::Test14, target(t)?))
        }
        "br" | "blr" => {
            let [r] = ops else { return form() };
            let rn = zr_of(r, Size::X)?;
            Ok(word(if m == "br" { mc::br(rn) } else { mc::blr(rn) }))
        }
        "ret" => match ops {
            [] => Ok(word(mc::ret(mc::Reg::LR.zr()))),
            [r] => Ok(word(mc::ret(zr_of(r, Size::X)?))),
            _ => form(),
        },
        "adr" => {
            let [r, t] = ops else { return form() };
            Ok(fixed(mc::adr(zr_of(r, Size::X)?, 0), RefKind::Adr21, target(t)?))
        }
        "ldr" | "str" | "ldrb" | "strb" | "ldrh" | "strh" | "ldrsb" | "ldrsh" | "ldrsw"
        | "ldur" | "stur" | "ldurb" | "sturb" | "ldurh" | "sturh" | "ldursb" | "ldursh"
        | "ldursw" => load_store(m, ops),
        "ldp" | "stp" | "ldpsw" => pair(m, ops),
        "ldxr" | "ldxrb" | "ldxrh" | "ldaxr" | "ldaxrb" | "ldaxrh" | "ldar" | "ldarb" | "ldarh"
        | "stlr" | "stlrb" | "stlrh" => exclusive_load(m, ops),
        "stxr" | "stxrb" | "stxrh" | "stlxr" | "stlxrb" | "stlxrh" => exclusive_store(m, ops),
        "ldxp" | "ldaxp" | "stxp" | "stlxp" => exclusive_pair(m, ops),
        "clrex" => match ops {
            [] => Ok(word(mc::clrex())),
            [i] if imm(i)? == 15 => Ok(word(mc::clrex())),
            _ => form(),
        },
        "nop" | "yield" | "wfe" | "wfi" | "sev" | "sevl" => {
            if !ops.is_empty() {
                return form();
            }
            let n = match m {
                "nop" => 0,
                "yield" => 1,
                "wfe" => 2,
                "wfi" => 3,
                "sev" => 4,
                _ => 5,
            };
            Ok(word(mc::hint(n)))
        }
        "hint" => {
            let [i] = ops else { return form() };
            checked(mc::try_hint(uimm(i, 127)?))
        }
        "isb" => match ops {
            [] => Ok(word(mc::isb())),
            [o] if o.ident().is_some_and(|s| s.eq_ignore_ascii_case("sy")) => Ok(word(mc::isb())),
            _ => fail(operand::OPTION),
        },
        "dmb" | "dsb" => {
            let [o] = ops else { return form() };
            let b = barrier(o)?;
            Ok(word(if m == "dmb" { mc::dmb(b) } else { mc::dsb(b) }))
        }
        "svc" | "brk" | "udf" => {
            let [i] = ops else { return form() };
            let v = uimm(i, 0xffff)? as u16;
            Ok(word(match m {
                "svc" => mc::svc(v),
                "brk" => mc::brk(v),
                _ => mc::udf(v),
            }))
        }
        "mrs" => {
            let [r, s] = ops else { return form() };
            let field = sysreg(s, sysreg::READ, features)?;
            Ok(word(sysreg::mrs(zr_of(r, Size::X)?.code(), field)))
        }
        "msr" => {
            let [s, r] = ops else { return form() };
            let field = sysreg(s, sysreg::WRITE, features)?;
            Ok(word(sysreg::msr(field, zr_of(r, Size::X)?.code())))
        }
        "bti" => match ops {
            [] => Ok(word(mc::hint(32))),
            [t] => match t.ident().map(str::to_ascii_lowercase).as_deref() {
                Some("c") => Ok(word(mc::bti_c())),
                Some("j") => Ok(word(mc::bti_j())),
                Some("jc") => Ok(word(mc::bti_jc())),
                _ => fail(operand::OPTION),
            },
            _ => form(),
        },
        "paciasp" | "pacibsp" | "autiasp" | "autibsp" => {
            if !ops.is_empty() {
                return form();
            }
            Ok(word(match m {
                "paciasp" => mc::paciasp(),
                "pacibsp" => mc::pacibsp(),
                "autiasp" => mc::autiasp(),
                _ => mc::autibsp(),
            }))
        }
        _ => {
            if let Some(e) = lse(m, ops, features)? {
                return Ok(e);
            }
            if let Some(e) = float(m, ops, features)? {
                return Ok(e);
            }
            refuse()
        }
    }
}

// ---- arithmetic ----

fn add_sub<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, rn, rest) = match ops {
        [rd, rn, rest @ ..] if !rest.is_empty() && rest.len() <= 2 => (gp(rd)?, gp(rn)?, rest),
        _ => return form(),
    };
    let sz = size(rd);
    sized(rn, sz)?;
    let flags = m.ends_with('s');
    let sub = m.starts_with("sub");
    if let Op::Imm(v, _) = rest[0] {
        let value = arith_imm(v, rest.get(1))?;
        // LLVM's assembler writes a negative immediate as the opposite
        // operation of its magnitude.
        let (sub, value) =
            if value < 0 { (!sub, value.unsigned_abs()) } else { (sub, value as u64) };
        let w = match (sub, flags) {
            (false, false) => mc::try_add_imm(sz, sp(rd)?, sp(rn)?, value),
            (true, false) => mc::try_sub_imm(sz, sp(rd)?, sp(rn)?, value),
            (false, true) => mc::try_adds_imm(sz, zr(rd)?, sp(rn)?, value),
            (true, true) => mc::try_subs_imm(sz, zr(rd)?, sp(rn)?, value),
        };
        return checked(w);
    }
    let rm = gp(&rest[0])?;
    let modifier = rest.get(1);
    let uses_sp = rn.is_sp() || (!flags && rd.is_sp());
    if uses_sp || matches!(modifier, Some(Op::Extend(..))) || size(rm) != sz {
        let (ext, amount) = ext_operand(sz, rm, modifier)?;
        let rm = zr(rm)?;
        let w = match (sub, flags) {
            (false, false) => mc::try_add_ext(sz, sp(rd)?, sp(rn)?, rm, ext, amount),
            (true, false) => mc::try_sub_ext(sz, sp(rd)?, sp(rn)?, rm, ext, amount),
            (false, true) => mc::try_adds_ext(sz, zr(rd)?, sp(rn)?, rm, ext, amount),
            (true, true) => mc::try_subs_ext(sz, zr(rd)?, sp(rn)?, rm, ext, amount),
        };
        return checked(w);
    }
    let (shift, amount) = arith_shift(modifier)?;
    let (rd, rn, rm) = (zr(rd)?, zr(rn)?, zr(rm)?);
    let w = match (sub, flags) {
        (false, false) => mc::try_add_shifted(sz, rd, rn, rm, shift, amount),
        (true, false) => mc::try_sub_shifted(sz, rd, rn, rm, shift, amount),
        (false, true) => mc::try_adds_shifted(sz, rd, rn, rm, shift, amount),
        (true, true) => mc::try_subs_shifted(sz, rd, rn, rm, shift, amount),
    };
    checked(w)
}

/// An add/subtract immediate with its optional `lsl #0` or `lsl #12`,
/// as a signed value (the caller turns a negative one around).
fn arith_imm(v: i64, shift: Option<&Op<'_>>) -> R<i64> {
    match shift {
        None => Ok(v),
        // An explicit shift takes a 12-bit unsigned field; `#0, lsl #12`
        // is a form of its own (sh = 1) that the encoder does not make.
        Some(Op::Shift(Shift::Lsl, 0)) if (0..4096).contains(&v) => Ok(v),
        Some(Op::Shift(Shift::Lsl, 12)) if (1..4096).contains(&v) => Ok(v << 12),
        Some(_) => fail(operand::RANGE),
    }
}

/// The shift of a shifted-register add/subtract (`lsl`, `lsr`, `asr`).
fn arith_shift(op: Option<&Op<'_>>) -> R<(Shift, u32)> {
    match op {
        None => Ok((Shift::Lsl, 0)),
        Some(Op::Shift(Shift::Ror, _)) => fail(operand::FORM),
        Some(Op::Shift(s, n)) => Ok((*s, *n)),
        Some(_) => form(),
    }
}

/// The extension of an extended-register add/subtract: written, or
/// `lsl` (allowed next to `sp`), or none (`uxtx`/`uxtw` #0). The index
/// register is `W` except for `uxtx`/`sxtx` (and `lsl`) at 64 bits.
fn ext_operand(sz: Size, rm: Gp, op: Option<&Op<'_>>) -> R<(Extend, u32)> {
    let natural = if sz == Size::X { Extend::Uxtx } else { Extend::Uxtw };
    let (ext, amount) = match op {
        None => (natural, 0),
        Some(Op::Shift(Shift::Lsl, n)) => (natural, *n),
        Some(Op::Extend(e, n)) => (*e, n.unwrap_or(0)),
        Some(_) => return form(),
    };
    let wide = matches!(ext, Extend::Uxtx | Extend::Sxtx);
    let want_x = sz == Size::X && wide;
    if rm.is_x() != want_x {
        return fail(operand::REGISTER);
    }
    if sz == Size::W && wide {
        return fail(operand::FORM);
    }
    Ok((ext, amount))
}

fn compare<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rn, rest) = match ops {
        [rn, rest @ ..] if !rest.is_empty() && rest.len() <= 2 => (gp(rn)?, rest),
        _ => return form(),
    };
    let sz = size(rn);
    let cmn = m == "cmn";
    if let Op::Imm(v, _) = rest[0] {
        let value = arith_imm(v, rest.get(1))?;
        let (cmn, value) =
            if value < 0 { (!cmn, value.unsigned_abs()) } else { (cmn, value as u64) };
        let w = if cmn {
            mc::try_cmn_imm(sz, sp(rn)?, value)
        } else {
            mc::try_cmp_imm(sz, sp(rn)?, value)
        };
        return checked(w);
    }
    let rm = gp(&rest[0])?;
    let modifier = rest.get(1);
    if rn.is_sp() || matches!(modifier, Some(Op::Extend(..))) || size(rm) != sz {
        let (ext, amount) = ext_operand(sz, rm, modifier)?;
        let rm = zr(rm)?;
        let w = if cmn {
            mc::try_cmn_ext(sz, sp(rn)?, rm, ext, amount)
        } else {
            mc::try_cmp_ext(sz, sp(rn)?, rm, ext, amount)
        };
        return checked(w);
    }
    let (shift, amount) = arith_shift(modifier)?;
    let w = if cmn {
        mc::try_cmn_shifted(sz, zr(rn)?, zr(rm)?, shift, amount)
    } else {
        mc::try_cmp_shifted(sz, zr(rn)?, zr(rm)?, shift, amount)
    };
    checked(w)
}

fn negate<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, rm, modifier) = match ops {
        [rd, rm] => (gp(rd)?, gp(rm)?, None),
        [rd, rm, s] => (gp(rd)?, gp(rm)?, Some(s)),
        _ => return form(),
    };
    let sz = size(rd);
    sized(rm, sz)?;
    let (shift, amount) = arith_shift(modifier)?;
    let w = if m == "neg" {
        mc::try_neg_shifted(sz, zr(rd)?, zr(rm)?, shift, amount)
    } else {
        mc::try_negs_shifted(sz, zr(rd)?, zr(rm)?, shift, amount)
    };
    checked(w)
}

fn mov<'a>(ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [d, s] = ops else { return form() };
    let rd = gp(d)?;
    let sz = size(rd);
    match s {
        Op::Imm(v, _) => {
            let v = *v;
            // A 32-bit move takes a value that is a 32-bit pattern,
            // signed or unsigned.
            if sz == Size::W && !(-(1i64 << 31)..(1i64 << 32)).contains(&v) {
                return fail(operand::RANGE);
            }
            let Some(rd) = zr(rd)?.reg() else { return fail(operand::REGISTER) };
            let seq = mc::mov_imm(sz, rd, v as u64);
            match seq.as_slice() {
                [w] => Ok(word(*w)),
                _ => fail(operand::RANGE),
            }
        }
        Op::Gp(rs) => {
            sized(*rs, sz)?;
            if rd.is_sp() || rs.is_sp() {
                Ok(word(mc::mov_sp(sz, sp(rd)?, sp(*rs)?)))
            } else {
                Ok(word(mc::mov(sz, zr(rd)?, zr(*rs)?)))
            }
        }
        _ => form(),
    }
}

fn mov_wide<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, v, shift) = match ops {
        [rd, v] => (gp(rd)?, v, 0),
        [rd, v, Op::Shift(Shift::Lsl, n)] => (gp(rd)?, v, *n),
        _ => return form(),
    };
    let sz = size(rd);
    let imm16 = uimm(v, 0xffff)? as u16;
    let rd = zr(rd)?;
    checked(match m {
        "movz" => mc::try_movz(sz, rd, imm16, shift),
        "movn" => mc::try_movn(sz, rd, imm16, shift),
        _ => mc::try_movk(sz, rd, imm16, shift),
    })
}

/// A logical immediate of size `sz`: a 32-bit value may be written
/// sign-extended.
fn logical_imm(sz: Size, v: i64) -> R<u64> {
    match sz {
        Size::X => Ok(v as u64),
        Size::W => {
            let high = v >> 32;
            if high == 0 || high == -1 { Ok(v as u64 & 0xffff_ffff) } else { fail(operand::RANGE) }
        }
    }
}

fn logical_shift(op: Option<&Op<'_>>) -> R<(Shift, u32)> {
    match op {
        None => Ok((Shift::Lsl, 0)),
        Some(Op::Shift(s, n)) => Ok((*s, *n)),
        Some(_) => form(),
    }
}

fn logical<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, rn, rest) = match ops {
        [rd, rn, rest @ ..] if !rest.is_empty() && rest.len() <= 2 => (gp(rd)?, gp(rn)?, rest),
        _ => return form(),
    };
    let sz = size(rd);
    sized(rn, sz)?;
    if let Op::Imm(v, _) = rest[0] {
        if rest.len() != 1 {
            return form();
        }
        let v = logical_imm(sz, v)?;
        let rn = zr(rn)?;
        return checked(match m {
            "and" => mc::try_and_imm(sz, sp(rd)?, rn, v),
            "orr" => mc::try_orr_imm(sz, sp(rd)?, rn, v),
            "eor" => mc::try_eor_imm(sz, sp(rd)?, rn, v),
            "ands" => mc::try_ands_imm(sz, zr(rd)?, rn, v),
            // `bic`, `orn`, `eon`, `bics` with an immediate are aliases
            // of the inverted immediate: not covered.
            _ => return fail(operand::FORM),
        });
    }
    let rm = zr_of(&rest[0], sz)?;
    let (shift, amount) = logical_shift(rest.get(1))?;
    let (rd, rn) = (zr(rd)?, zr(rn)?);
    checked(match m {
        "and" => mc::try_and_shifted(sz, rd, rn, rm, shift, amount),
        "orr" => mc::try_orr_shifted(sz, rd, rn, rm, shift, amount),
        "eor" => mc::try_eor_shifted(sz, rd, rn, rm, shift, amount),
        "ands" => mc::try_ands_shifted(sz, rd, rn, rm, shift, amount),
        "bic" => mc::try_bic_shifted(sz, rd, rn, rm, shift, amount),
        "bics" => mc::try_bics_shifted(sz, rd, rn, rm, shift, amount),
        "orn" => mc::try_orn_shifted(sz, rd, rn, rm, shift, amount),
        _ => mc::try_eon_shifted(sz, rd, rn, rm, shift, amount),
    })
}

fn logical_unary<'a>(ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, rm, modifier) = match ops {
        [rd, rm] => (gp(rd)?, rm, None),
        [rd, rm, s] => (gp(rd)?, rm, Some(s)),
        _ => return form(),
    };
    let sz = size(rd);
    let rm = zr_of(rm, sz)?;
    let (shift, amount) = logical_shift(modifier)?;
    checked(mc::try_mvn_shifted(sz, zr(rd)?, rm, shift, amount))
}

fn test<'a>(ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rn, rest) = match ops {
        [rn, rest @ ..] if !rest.is_empty() && rest.len() <= 2 => (gp(rn)?, rest),
        _ => return form(),
    };
    let sz = size(rn);
    if let Op::Imm(v, _) = rest[0] {
        if rest.len() != 1 {
            return form();
        }
        return checked(mc::try_tst_imm(sz, zr(rn)?, logical_imm(sz, v)?));
    }
    let rm = zr_of(&rest[0], sz)?;
    let (shift, amount) = logical_shift(rest.get(1))?;
    checked(mc::try_tst_shifted(sz, zr(rn)?, rm, shift, amount))
}

fn shift<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, x] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let rn = zr_of(rn, sz)?;
    let rd = zr(rd)?;
    if let Op::Imm(..) = x {
        let n = uimm(x, u64::from(sz.bits() - 1))?;
        return checked(match m {
            "lsl" => mc::try_lsl_imm(sz, rd, rn, n),
            "lsr" => mc::try_lsr_imm(sz, rd, rn, n),
            "asr" => mc::try_asr_imm(sz, rd, rn, n),
            _ => mc::try_ror_imm(sz, rd, rn, n),
        });
    }
    let rm = zr_of(x, sz)?;
    Ok(word(match m {
        "lsl" => mc::lslv(sz, rd, rn, rm),
        "lsr" => mc::lsrv(sz, rd, rn, rm),
        "asr" => mc::asrv(sz, rd, rn, rm),
        _ => mc::rorv(sz, rd, rn, rm),
    }))
}

fn bitfield<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, a, b] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let rn = zr_of(rn, sz)?;
    let rd = zr(rd)?;
    let max = u64::from(sz.bits());
    let (a, b) = (uimm(a, max)?, uimm(b, max)?);
    checked(match m {
        "ubfx" => mc::try_ubfx(sz, rd, rn, a, b),
        "sbfx" => mc::try_sbfx(sz, rd, rn, a, b),
        "bfxil" => mc::try_bfxil(sz, rd, rn, a, b),
        "ubfiz" => mc::try_ubfiz(sz, rd, rn, a, b),
        "sbfiz" => mc::try_sbfiz(sz, rd, rn, a, b),
        "bfi" => mc::try_bfi(sz, rd, rn, a, b),
        "ubfm" => mc::try_ubfm(sz, rd, rn, a, b),
        "sbfm" => mc::try_sbfm(sz, rd, rn, a, b),
        _ => mc::try_bfm(sz, rd, rn, a, b),
    })
}

fn extend<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn] = ops else { return form() };
    let rd = gp(rd)?;
    let rn = zr_of(rn, Size::W)?;
    let sz = size(rd);
    Ok(word(match m {
        "sxtb" => mc::sxtb(sz, zr(rd)?, rn),
        "sxth" => mc::sxth(sz, zr(rd)?, rn),
        "sxtw" if sz == Size::X => mc::sxtw(zr(rd)?, rn),
        "uxtb" if sz == Size::W => mc::uxtb(zr(rd)?, rn),
        "uxth" if sz == Size::W => mc::uxth(zr(rd)?, rn),
        _ => return fail(operand::REGISTER),
    }))
}

fn extr<'a>(ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, rm, lsb] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let (rn, rm) = (zr_of(rn, sz)?, zr_of(rm, sz)?);
    let lsb = uimm(lsb, u64::from(sz.bits() - 1))?;
    checked(mc::try_extr(sz, zr(rd)?, rn, rm, lsb))
}

fn unary<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let rn = zr_of(rn, sz)?;
    let rd = zr(rd)?;
    Ok(word(match m {
        "rbit" => mc::rbit(sz, rd, rn),
        "rev" => mc::rev(sz, rd, rn),
        "rev16" => mc::rev16(sz, rd, rn),
        "rev32" if sz == Size::X => mc::rev32(rd, rn),
        "clz" => mc::clz(sz, rd, rn),
        "cls" => mc::cls(sz, rd, rn),
        _ => return fail(operand::REGISTER),
    }))
}

fn cond_select<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, rm, c] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let (rn, rm, c) = (zr_of(rn, sz)?, zr_of(rm, sz)?, cond(c)?);
    let rd = zr(rd)?;
    Ok(word(match m {
        "csel" => mc::csel(sz, rd, rn, rm, c),
        "csinc" => mc::csinc(sz, rd, rn, rm, c),
        "csinv" => mc::csinv(sz, rd, rn, rm, c),
        _ => mc::csneg(sz, rd, rn, rm, c),
    }))
}

fn cond_set<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, c] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let c = cond(c)?;
    let rd = zr(rd)?;
    checked(if m == "cset" { mc::try_cset(sz, rd, c) } else { mc::try_csetm(sz, rd, c) })
}

fn cond_unary<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, c] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let rn_g = sized(gp(rn)?, sz)?;
    let c = cond(c)?;
    let rd = zr(rd)?;
    if m == "cneg" {
        return checked(mc::try_cneg(sz, rd, zr(rn_g)?, c));
    }
    // With the zero register these are `cset` and `csetm`.
    let Some(rn) = zr(rn_g)?.reg() else { return fail(operand::REGISTER) };
    checked(if m == "cinc" { mc::try_cinc(sz, rd, rn, c) } else { mc::try_cinv(sz, rd, rn, c) })
}

fn cond_compare<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rn, x, nzcv, c] = ops else { return form() };
    let rn = gp(rn)?;
    let sz = size(rn);
    let nzcv = uimm(nzcv, 15)?;
    let c = cond(c)?;
    let rn = zr(rn)?;
    if let Op::Imm(..) = x {
        let v = uimm(x, 31)?;
        return checked(if m == "ccmp" {
            mc::try_ccmp_imm(sz, rn, v, nzcv, c)
        } else {
            mc::try_ccmn_imm(sz, rn, v, nzcv, c)
        });
    }
    let rm = zr_of(x, sz)?;
    checked(if m == "ccmp" {
        mc::try_ccmp(sz, rn, rm, nzcv, c)
    } else {
        mc::try_ccmn(sz, rn, rm, nzcv, c)
    })
}

fn three<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, rm] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let (rn, rm) = (zr_of(rn, sz)?, zr_of(rm, sz)?);
    let rd = zr(rd)?;
    Ok(word(match m {
        "mul" => mc::mul(sz, rd, rn, rm),
        "mneg" => mc::mneg(sz, rd, rn, rm),
        "sdiv" => mc::sdiv(sz, rd, rn, rm),
        "udiv" => mc::udiv(sz, rd, rn, rm),
        "smulh" if sz == Size::X => mc::smulh(rd, rn, rm),
        "umulh" if sz == Size::X => mc::umulh(rd, rn, rm),
        "adc" => mc::adc(sz, rd, rn, rm),
        "adcs" => mc::adcs(sz, rd, rn, rm),
        "sbc" => mc::sbc(sz, rd, rn, rm),
        "sbcs" => mc::sbcs(sz, rd, rn, rm),
        "lslv" => mc::lslv(sz, rd, rn, rm),
        "lsrv" => mc::lsrv(sz, rd, rn, rm),
        "asrv" => mc::asrv(sz, rd, rn, rm),
        "rorv" => mc::rorv(sz, rd, rn, rm),
        _ => return fail(operand::REGISTER),
    }))
}

fn four<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rd, rn, rm, ra] = ops else { return form() };
    let rd = gp(rd)?;
    let sz = size(rd);
    let (rn, rm, ra) = (zr_of(rn, sz)?, zr_of(rm, sz)?, zr_of(ra, sz)?);
    let rd = zr(rd)?;
    Ok(word(if m == "madd" { mc::madd(sz, rd, rn, rm, ra) } else { mc::msub(sz, rd, rn, rm, ra) }))
}

fn long_mul<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (rd, rn, rm, ra) = match ops {
        [rd, rn, rm] if matches!(m, "smull" | "umull") => (rd, rn, rm, None),
        [rd, rn, rm, ra] if !matches!(m, "smull" | "umull") => (rd, rn, rm, Some(ra)),
        _ => return form(),
    };
    let rd = zr_of(rd, Size::X)?;
    let (rn, rm) = (zr_of(rn, Size::W)?, zr_of(rm, Size::W)?);
    let ra = ra.map(|ra| zr_of(ra, Size::X)).transpose()?;
    Ok(word(match (m, ra) {
        ("smull", None) => mc::smull(rd, rn, rm),
        ("umull", None) => mc::umull(rd, rn, rm),
        ("smaddl", Some(ra)) => mc::smaddl(rd, rn, rm, ra),
        ("umaddl", Some(ra)) => mc::umaddl(rd, rn, rm, ra),
        ("smsubl", Some(ra)) => mc::smsubl(rd, rn, rm, ra),
        ("umsubl", Some(ra)) => mc::umsubl(rd, rn, rm, ra),
        _ => return form(),
    }))
}

fn crc<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Encoded<'a>> {
    if !features.crc {
        return fail(other::FEATURE);
    }
    let [rd, rn, rm] = ops else { return form() };
    let kind = match m {
        "crc32b" => mc::Crc32::B,
        "crc32h" => mc::Crc32::H,
        "crc32w" => mc::Crc32::W,
        "crc32x" => mc::Crc32::X,
        "crc32cb" => mc::Crc32::Cb,
        "crc32ch" => mc::Crc32::Ch,
        "crc32cw" => mc::Crc32::Cw,
        _ => mc::Crc32::Cx,
    };
    let wide = matches!(kind, mc::Crc32::X | mc::Crc32::Cx);
    let rm = zr_of(rm, if wide { Size::X } else { Size::W })?;
    Ok(word(mc::crc32(kind, zr_of(rd, Size::W)?, zr_of(rn, Size::W)?, rm)))
}

// ---- loads and stores ----

/// What a load or store mnemonic transfers, from its name and its data
/// register.
#[derive(Clone, Copy)]
enum Access {
    Load(Load),
    Store(MemSize),
    LoadFp(VMemSize),
    StoreFp(VMemSize),
}

fn view_size(v: View) -> VMemSize {
    match v {
        View::B => VMemSize::B,
        View::H => VMemSize::H,
        View::S => VMemSize::S,
        View::D => VMemSize::D,
        View::Q => VMemSize::Q,
    }
}

fn access(m: &str, rt: &Op<'_>) -> R<(Access, Option<RegZr>, Option<VReg>)> {
    let base = m.replacen("ldur", "ldr", 1).replacen("stur", "str", 1);
    if let Op::Fp(..) = rt {
        let (r, v) = fp(rt)?;
        return match base.as_str() {
            "ldr" => Ok((Access::LoadFp(view_size(v)), None, Some(r))),
            "str" => Ok((Access::StoreFp(view_size(v)), None, Some(r))),
            _ => fail(operand::REGISTER),
        };
    }
    let g = gp(rt)?;
    let x = g.is_x();
    let a = match (base.as_str(), x) {
        ("ldr", false) => Access::Load(Load::W),
        ("ldr", true) => Access::Load(Load::X),
        ("str", false) => Access::Store(MemSize::W),
        ("str", true) => Access::Store(MemSize::X),
        ("ldrb", false) => Access::Load(Load::B),
        ("ldrh", false) => Access::Load(Load::H),
        ("strb", false) => Access::Store(MemSize::B),
        ("strh", false) => Access::Store(MemSize::H),
        ("ldrsb", false) => Access::Load(Load::SbW),
        ("ldrsb", true) => Access::Load(Load::SbX),
        ("ldrsh", false) => Access::Load(Load::ShW),
        ("ldrsh", true) => Access::Load(Load::ShX),
        ("ldrsw", true) => Access::Load(Load::SwX),
        _ => return fail(operand::REGISTER),
    };
    Ok((a, Some(zr(g)?), None))
}

fn log2_of(a: Access) -> u32 {
    match a {
        Access::Load(l) => l.mem_size().log2(),
        Access::Store(s) => s.log2(),
        Access::LoadFp(v) | Access::StoreFp(v) => v.log2(),
    }
}

/// The index of a register-offset address and whether it is scaled: a
/// shift amount is `0` or the access's log2; for a one-byte access a
/// written amount (`#0`) selects the scaled encoding, as LLVM writes it.
fn index(ix: super::operand::Index, log2: u32) -> R<(RegZr, mc::Index, bool)> {
    let (want_x, idx) = match ix.ext {
        None | Some(IndexExt::Lsl) => (true, mc::Index::Lsl),
        Some(IndexExt::Sxtx) => (true, mc::Index::Sxtx),
        Some(IndexExt::Uxtw) => (false, mc::Index::Uxtw),
        Some(IndexExt::Sxtw) => (false, mc::Index::Sxtw),
    };
    if ix.reg.is_x() != want_x || ix.reg.is_sp() {
        return fail(operand::REGISTER);
    }
    let scaled = match ix.amount {
        Some(n) if n == log2 => true,
        None | Some(0) => false,
        Some(_) => return fail(operand::RANGE),
    };
    Ok((zr(ix.reg)?, idx, scaled))
}

fn load_store<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let unscaled = m.starts_with("ldur") || m.starts_with("stur");
    let (rt, rest) = match ops {
        [rt, rest @ ..] if !rest.is_empty() && rest.len() <= 2 => (rt, rest),
        _ => return form(),
    };
    let (acc, gr, vr) = access(m, rt)?;
    let mem = match rest[0] {
        Op::Mem(mem) => mem,
        Op::Literal => return fail(operand::LITERAL_POOL),
        Op::Name(to) if rest.len() == 1 && !unscaled => {
            // A literal load from a label.
            let w = match (acc, gr, vr) {
                (Access::Load(Load::W), Some(rt), _) => mc::ldr_lit(mc::LitLoad::W, rt, 0),
                (Access::Load(Load::X), Some(rt), _) => mc::ldr_lit(mc::LitLoad::X, rt, 0),
                (Access::Load(Load::SwX), Some(rt), _) => mc::ldr_lit(mc::LitLoad::SwX, rt, 0),
                (Access::LoadFp(v), _, Some(rt)) => {
                    let sz = match v {
                        VMemSize::S => VPairSize::S,
                        VMemSize::D => VPairSize::D,
                        VMemSize::Q => VPairSize::Q,
                        _ => return fail(operand::REGISTER),
                    };
                    mc::ldr_fp_lit(sz, rt, 0)
                }
                _ => return form(),
            };
            return Ok(fixed(w, RefKind::Literal19, to));
        }
        _ => return form(),
    };
    let rn = base(&mem)?;
    let log2 = log2_of(acc);
    // Post-index: `[xn], #imm`.
    if let [_, Op::Imm(off, _)] = rest {
        if unscaled || mem.writeback || mem.offset != Offset::None {
            return form();
        }
        return checked(match (acc, gr, vr) {
            (Access::Load(l), Some(rt), _) => mc::try_ldr_post(l, rt, rn, *off),
            (Access::Store(s), Some(rt), _) => mc::try_str_post(s, rt, rn, *off),
            (Access::LoadFp(v), _, Some(rt)) => mc::try_ldr_fp_post(v, rt, rn, *off),
            (Access::StoreFp(v), _, Some(rt)) => mc::try_str_fp_post(v, rt, rn, *off),
            _ => return form(),
        });
    }
    if rest.len() != 1 {
        return form();
    }
    match mem.offset {
        Offset::Reg(ix) => {
            if unscaled || mem.writeback {
                return form();
            }
            let (rm, idx, scaled) = index(ix, log2)?;
            Ok(word(match (acc, gr, vr) {
                (Access::Load(l), Some(rt), _) => mc::ldr_reg(l, rt, rn, rm, idx, scaled),
                (Access::Store(s), Some(rt), _) => mc::str_reg(s, rt, rn, rm, idx, scaled),
                (Access::LoadFp(v), _, Some(rt)) => mc::ldr_fp_reg(v, rt, rn, rm, idx, scaled),
                (Access::StoreFp(v), _, Some(rt)) => mc::str_fp_reg(v, rt, rn, rm, idx, scaled),
                _ => return form(),
            }))
        }
        Offset::None | Offset::Imm(_) => {
            let off = if let Offset::Imm(o) = mem.offset { o } else { 0 };
            if mem.writeback {
                if unscaled || mem.offset == Offset::None {
                    return form();
                }
                return checked(match (acc, gr, vr) {
                    (Access::Load(l), Some(rt), _) => mc::try_ldr_pre(l, rt, rn, off),
                    (Access::Store(s), Some(rt), _) => mc::try_str_pre(s, rt, rn, off),
                    (Access::LoadFp(v), _, Some(rt)) => mc::try_ldr_fp_pre(v, rt, rn, off),
                    (Access::StoreFp(v), _, Some(rt)) => mc::try_str_fp_pre(v, rt, rn, off),
                    _ => return form(),
                });
            }
            // LLVM takes the scaled unsigned form when the offset fits it,
            // and the unscaled form otherwise; `ldur` is unscaled always.
            let scaled = !unscaled;
            let pick = |a: Option<u32>, b: Option<u32>| if scaled { a.or(b) } else { b };
            checked(match (acc, gr, vr) {
                (Access::Load(l), Some(rt), _) => {
                    pick(mc::try_ldr_off(l, rt, rn, off), mc::try_ldur(l, rt, rn, off))
                }
                (Access::Store(s), Some(rt), _) => {
                    pick(mc::try_str_off(s, rt, rn, off), mc::try_stur(s, rt, rn, off))
                }
                (Access::LoadFp(v), _, Some(rt)) => {
                    pick(mc::try_ldr_fp_off(v, rt, rn, off), mc::try_ldur_fp(v, rt, rn, off))
                }
                (Access::StoreFp(v), _, Some(rt)) => {
                    pick(mc::try_str_fp_off(v, rt, rn, off), mc::try_stur_fp(v, rt, rn, off))
                }
                _ => return form(),
            })
        }
    }
}

/// The addressing mode of a pair access.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Off,
    Pre,
    Post,
}

fn pair<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let (r1, r2, mem, post) = match ops {
        [a, b, Op::Mem(mem)] => (a, b, *mem, None),
        [a, b, Op::Mem(mem), Op::Imm(off, _)] => (a, b, *mem, Some(*off)),
        _ => return form(),
    };
    let rn = base(&mem)?;
    let off = match (mem.offset, post) {
        (Offset::None, Some(o)) if !mem.writeback => o,
        (Offset::None, None) if !mem.writeback => 0,
        (Offset::Imm(o), None) => o,
        _ => return form(),
    };
    let load = m.starts_with("ld");
    let mode = if post.is_some() {
        Mode::Post
    } else if mem.writeback {
        Mode::Pre
    } else {
        Mode::Off
    };
    if let Op::Fp(..) = r1 {
        if m == "ldpsw" {
            return fail(operand::REGISTER);
        }
        let (a, va) = fp(r1)?;
        let (b, vb) = fp(r2)?;
        if va != vb {
            return fail(operand::REGISTER);
        }
        let sz = match va {
            View::S => VPairSize::S,
            View::D => VPairSize::D,
            View::Q => VPairSize::Q,
            _ => return fail(operand::REGISTER),
        };
        return checked(match (load, mode) {
            (true, Mode::Off) => mc::try_ldp_fp_off(sz, a, b, rn, off),
            (true, Mode::Pre) => mc::try_ldp_fp_pre(sz, a, b, rn, off),
            (true, Mode::Post) => mc::try_ldp_fp_post(sz, a, b, rn, off),
            (false, Mode::Off) => mc::try_stp_fp_off(sz, a, b, rn, off),
            (false, Mode::Pre) => mc::try_stp_fp_pre(sz, a, b, rn, off),
            (false, Mode::Post) => mc::try_stp_fp_post(sz, a, b, rn, off),
        });
    }
    let a = gp(r1)?;
    let sz = size(a);
    let b = zr_of(r2, sz)?;
    let a = zr(a)?;
    if m == "ldpsw" {
        if sz != Size::X {
            return fail(operand::REGISTER);
        }
        return checked(match mode {
            Mode::Off => mc::try_ldpsw_off(a, b, rn, off),
            Mode::Pre => mc::try_ldpsw_pre(a, b, rn, off),
            Mode::Post => mc::try_ldpsw_post(a, b, rn, off),
        });
    }
    checked(match (load, mode) {
        (true, Mode::Off) => mc::try_ldp_off(sz, a, b, rn, off),
        (true, Mode::Pre) => mc::try_ldp_pre(sz, a, b, rn, off),
        (true, Mode::Post) => mc::try_ldp_post(sz, a, b, rn, off),
        (false, Mode::Off) => mc::try_stp_off(sz, a, b, rn, off),
        (false, Mode::Pre) => mc::try_stp_pre(sz, a, b, rn, off),
        (false, Mode::Post) => mc::try_stp_post(sz, a, b, rn, off),
    })
}

// ---- exclusives and atomics ----

/// The base of an exclusive or atomic access: `[xn]` or `[xn, #0]`.
fn plain_base(op: &Op<'_>) -> R<RegSp> {
    match op {
        Op::Mem(m) if !m.writeback && matches!(m.offset, Offset::None | Offset::Imm(0)) => base(m),
        _ => form(),
    }
}

/// The access size of a mnemonic ending in `b`/`h` (or neither) with
/// data register `g`.
fn suffix_size(m: &str, g: Gp) -> R<(MemSize, &str)> {
    if let Some(stem) = m.strip_suffix('b') {
        return if g.is_x() { fail(operand::REGISTER) } else { Ok((MemSize::B, stem)) };
    }
    if let Some(stem) = m.strip_suffix('h') {
        return if g.is_x() { fail(operand::REGISTER) } else { Ok((MemSize::H, stem)) };
    }
    Ok((if g.is_x() { MemSize::X } else { MemSize::W }, m))
}

fn exclusive_load<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rt, mem] = ops else { return form() };
    let g = gp(rt)?;
    let (ms, stem) = suffix_size(m, g)?;
    let (rt, rn) = (zr(g)?, plain_base(mem)?);
    Ok(word(match stem {
        "ldxr" => mc::ldxr(ms, rt, rn),
        "ldaxr" => mc::ldaxr(ms, rt, rn),
        "ldar" => mc::ldar(ms, rt, rn),
        _ => mc::stlr(ms, rt, rn),
    }))
}

fn exclusive_store<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [rs, rt, mem] = ops else { return form() };
    let g = gp(rt)?;
    let (ms, stem) = suffix_size(m, g)?;
    let (rs, rt, rn) = (zr_of(rs, Size::W)?, zr(g)?, plain_base(mem)?);
    checked(if stem == "stxr" {
        mc::try_stxr(ms, rs, rt, rn)
    } else {
        mc::try_stlxr(ms, rs, rt, rn)
    })
}

fn exclusive_pair<'a>(m: &str, ops: &[Op<'a>]) -> R<Encoded<'a>> {
    if m.starts_with("ld") {
        let [a, b, mem] = ops else { return form() };
        let g = gp(a)?;
        let sz = size(g);
        let (a, b, rn) = (zr(g)?, zr_of(b, sz)?, plain_base(mem)?);
        return checked(if m == "ldxp" {
            mc::try_ldxp(sz, a, b, rn)
        } else {
            mc::try_ldaxp(sz, a, b, rn)
        });
    }
    let [rs, a, b, mem] = ops else { return form() };
    let g = gp(a)?;
    let sz = size(g);
    let (rs, a, b, rn) = (zr_of(rs, Size::W)?, zr(g)?, zr_of(b, sz)?, plain_base(mem)?);
    checked(if m == "stxp" {
        mc::try_stxp(sz, rs, a, b, rn)
    } else {
        mc::try_stlxp(sz, rs, a, b, rn)
    })
}

/// The LSE atomics (`cas`, `swp`, `ld<op>` with their ordering and size
/// suffixes); `None` for another mnemonic.
fn lse<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Option<Encoded<'a>>> {
    const BASES: [&str; 10] =
        ["cas", "swp", "ldadd", "ldclr", "ldeor", "ldset", "ldsmax", "ldsmin", "ldumax", "ldumin"];
    let Some(&stem) = BASES.iter().find(|b| m.starts_with(**b)) else { return Ok(None) };
    let mut rest = &m[stem.len()..];
    let mut size_suffix = None;
    if let Some(r) = rest.strip_suffix('b') {
        size_suffix = Some(MemSize::B);
        rest = r;
    } else if let Some(r) = rest.strip_suffix('h') {
        size_suffix = Some(MemSize::H);
        rest = r;
    }
    let order = match rest {
        "" => Order::Plain,
        "a" => Order::A,
        "l" => Order::L,
        "al" => Order::Al,
        _ => return Ok(None),
    };
    if !features.lse {
        return fail(other::FEATURE);
    }
    let [rs, rt, mem] = ops else { return form() };
    let g = gp(rt)?;
    let ms = match size_suffix {
        Some(s) if !g.is_x() => s,
        Some(_) => return fail(operand::REGISTER),
        None if g.is_x() => MemSize::X,
        None => MemSize::W,
    };
    let reg_size = if ms == MemSize::X { Size::X } else { Size::W };
    let (rs, rt, rn) = (zr_of(rs, reg_size)?, zr(g)?, plain_base(mem)?);
    Ok(Some(word(match stem {
        "cas" => mc::cas(ms, order, rs, rt, rn),
        "swp" => mc::swp(ms, order, rs, rt, rn),
        "ldadd" => mc::ldadd(ms, order, rs, rt, rn),
        "ldclr" => mc::ldclr(ms, order, rs, rt, rn),
        "ldeor" => mc::ldeor(ms, order, rs, rt, rn),
        "ldset" => mc::ldset(ms, order, rs, rt, rn),
        "ldsmax" => mc::ldsmax(ms, order, rs, rt, rn),
        "ldsmin" => mc::ldsmin(ms, order, rs, rt, rn),
        "ldumax" => mc::ldumax(ms, order, rs, rt, rn),
        _ => mc::ldumin(ms, order, rs, rt, rn),
    })))
}

// ---- system ----

fn barrier(op: &Op<'_>) -> R<Barrier> {
    let Some(name) = op.ident() else { return fail(operand::OPTION) };
    Ok(match name.to_ascii_lowercase().as_str() {
        "sy" => Barrier::Sy,
        "st" => Barrier::St,
        "ld" => Barrier::Ld,
        "ish" => Barrier::Ish,
        "ishst" => Barrier::Ishst,
        "ishld" => Barrier::Ishld,
        "nsh" => Barrier::Nsh,
        "nshst" => Barrier::Nshst,
        "nshld" => Barrier::Nshld,
        "osh" => Barrier::Osh,
        "oshst" => Barrier::Oshst,
        "oshld" => Barrier::Oshld,
        _ => return fail(operand::OPTION),
    })
}

/// The `op0:op1:CRn:CRm:op2` field of a system-register operand that
/// allows `access` ([`sysreg::READ`] or [`sysreg::WRITE`]): a name of
/// [`sysreg::TABLE`] or the generic form (`spec/asm.md` §5.4).
fn sysreg(op: &Op<'_>, access: u8, features: Features) -> R<u16> {
    let Some(name) = op.ident() else { return fail(operand::SYSREG) };
    if let Some(field) = sysreg::generic(name) {
        return Ok(field);
    }
    match sysreg::named(name) {
        Some((_, field, bits)) if bits & access != 0 => {
            if bits & sysreg::RAND != 0 && !features.rand {
                return fail(other::FEATURE);
            }
            Ok(field)
        }
        _ => fail(operand::SYSREG),
    }
}

// ---- scalar floating point ----

/// The scalar floating-point instructions; `None` for another mnemonic.
#[allow(clippy::too_many_lines, reason = "one arm per mnemonic family, read top to bottom")]
#[allow(clippy::many_single_char_names, reason = "operands named as in the instructions")]
fn float<'a>(m: &str, ops: &[Op<'a>], features: Features) -> R<Option<Encoded<'a>>> {
    type Two = fn(FpSize, VReg, VReg) -> u32;
    type Three = fn(FpSize, VReg, VReg, VReg) -> u32;
    type Four = fn(FpSize, VReg, VReg, VReg, VReg) -> u32;
    let two: Option<Two> = match m {
        "fabs" => Some(mc::fabs),
        "fneg" => Some(mc::fneg),
        "fsqrt" => Some(mc::fsqrt),
        "frintn" => Some(mc::frintn),
        "frintp" => Some(mc::frintp),
        "frintm" => Some(mc::frintm),
        "frintz" => Some(mc::frintz),
        "frinta" => Some(mc::frinta),
        "frintx" => Some(mc::frintx),
        "frinti" => Some(mc::frinti),
        _ => None,
    };
    let three: Option<Three> = match m {
        "fadd" => Some(mc::fadd),
        "fsub" => Some(mc::fsub),
        "fmul" => Some(mc::fmul),
        "fdiv" => Some(mc::fdiv),
        "fnmul" => Some(mc::fnmul),
        "fmax" => Some(mc::fmax),
        "fmin" => Some(mc::fmin),
        "fmaxnm" => Some(mc::fmaxnm),
        "fminnm" => Some(mc::fminnm),
        _ => None,
    };
    let four: Option<Four> = match m {
        "fmadd" => Some(mc::fmadd),
        "fmsub" => Some(mc::fmsub),
        "fnmadd" => Some(mc::fnmadd),
        "fnmsub" => Some(mc::fnmsub),
        _ => None,
    };
    let known = two.is_some()
        || three.is_some()
        || four.is_some()
        || matches!(
            m,
            "fmov"
                | "fcmp"
                | "fcmpe"
                | "fccmp"
                | "fccmpe"
                | "fcsel"
                | "fcvt"
                | "scvtf"
                | "ucvtf"
                | "fcvtns"
                | "fcvtnu"
                | "fcvtas"
                | "fcvtau"
                | "fcvtps"
                | "fcvtpu"
                | "fcvtms"
                | "fcvtmu"
                | "fcvtzs"
                | "fcvtzu"
        );
    if !known {
        return Ok(None);
    }
    if !features.fp {
        return fail(other::FEATURE);
    }
    if ops.iter().any(|o| matches!(o, Op::Vector)) {
        return fail(operand::VECTOR);
    }
    if let Some(f) = two {
        let [_, _] = ops else { return form() };
        let (sz, r) = fp_same(ops)?;
        return Ok(Some(word(f(sz, r[0], r[1]))));
    }
    if let Some(f) = three {
        let [_, _, _] = ops else { return form() };
        let (sz, r) = fp_same(ops)?;
        return Ok(Some(word(f(sz, r[0], r[1], r[2]))));
    }
    if let Some(f) = four {
        let [_, _, _, _] = ops else { return form() };
        let (sz, r) = fp_same(ops)?;
        return Ok(Some(word(f(sz, r[0], r[1], r[2], r[3]))));
    }
    let e = match m {
        "fmov" => fmov(ops)?,
        "fcmp" | "fcmpe" => {
            let [a, b] = ops else { return form() };
            let (rn, v) = fp(a)?;
            let sz = fp_size(v)?;
            let zero = matches!(b, Op::FpImm(f) if *f == 0.0 && f.is_sign_positive());
            if zero {
                word(if m == "fcmp" { mc::fcmp_zero(sz, rn) } else { mc::fcmpe_zero(sz, rn) })
            } else {
                let (sz, r) = fp_same(ops)?;
                word(if m == "fcmp" { mc::fcmp(sz, r[0], r[1]) } else { mc::fcmpe(sz, r[0], r[1]) })
            }
        }
        "fccmp" | "fccmpe" => {
            let [a, b, nzcv, c] = ops else { return form() };
            let (sz, r) = fp_same(&[*a, *b])?;
            let (nzcv, c) = (uimm(nzcv, 15)?, cond(c)?);
            checked(if m == "fccmp" {
                mc::try_fccmp(sz, r[0], r[1], nzcv, c)
            } else {
                mc::try_fccmpe(sz, r[0], r[1], nzcv, c)
            })?
        }
        "fcsel" => {
            let [a, b, d, c] = ops else { return form() };
            let (sz, r) = fp_same(&[*a, *b, *d])?;
            word(mc::fcsel(sz, r[0], r[1], r[2], cond(c)?))
        }
        "fcvt" => {
            let [d, s] = ops else { return form() };
            let ((rd, vd), (rn, vn)) = (fp(d)?, fp(s)?);
            let format = |v| match v {
                View::H => Ok(FpFormat::H),
                View::S => Ok(FpFormat::S),
                View::D => Ok(FpFormat::D),
                _ => fail(operand::REGISTER),
            };
            checked(mc::try_fcvt(format(vd)?, rd, format(vn)?, rn))?
        }
        "scvtf" | "ucvtf" => {
            let [d, s] = ops else { return form() };
            let (rd, vd) = fp(d)?;
            let g = gp(s)?;
            let (fsz, rn) = (fp_size(vd)?, zr(g)?);
            word(if m == "scvtf" {
                mc::scvtf(fsz, rd, size(g), rn)
            } else {
                mc::ucvtf(fsz, rd, size(g), rn)
            })
        }
        _ => {
            // fcvt[nmpaz][su]
            let [d, s] = ops else { return form() };
            let g = gp(d)?;
            let (rn, vn) = fp(s)?;
            let (sz, rd, fsz) = (size(g), zr(g)?, fp_size(vn)?);
            let f: fn(Size, RegZr, FpSize, VReg) -> u32 = match m {
                "fcvtns" => mc::fcvtns,
                "fcvtnu" => mc::fcvtnu,
                "fcvtas" => mc::fcvtas,
                "fcvtau" => mc::fcvtau,
                "fcvtps" => mc::fcvtps,
                "fcvtpu" => mc::fcvtpu,
                "fcvtms" => mc::fcvtms,
                "fcvtmu" => mc::fcvtmu,
                "fcvtzs" => mc::fcvtzs,
                _ => mc::fcvtzu,
            };
            word(f(sz, rd, fsz, rn))
        }
    };
    Ok(Some(e))
}

fn fmov<'a>(ops: &[Op<'a>]) -> R<Encoded<'a>> {
    let [d, s] = ops else { return form() };
    match (d, s) {
        (Op::Fp(..), Op::Fp(..)) => {
            let (sz, r) = fp_same(ops)?;
            Ok(word(mc::fmov(sz, r[0], r[1])))
        }
        (Op::Fp(..), Op::Gp(g)) => {
            let (rd, v) = fp(d)?;
            let sz = match (v, g.is_x()) {
                (View::S, false) => Size::W,
                (View::D, true) => Size::X,
                _ => return fail(operand::REGISTER),
            };
            Ok(word(mc::fmov_gpr_to_fp(sz, rd, zr(*g)?)))
        }
        (Op::Gp(g), Op::Fp(..)) => {
            let (rn, v) = fp(s)?;
            let sz = match (v, g.is_x()) {
                (View::S, false) => Size::W,
                (View::D, true) => Size::X,
                _ => return fail(operand::REGISTER),
            };
            Ok(word(mc::fmov_fp_to_gpr(sz, zr(*g)?, rn)))
        }
        (Op::Fp(..), Op::FpImm(f)) => {
            let (rd, v) = fp(d)?;
            let sz = fp_size(v)?;
            let bits = match sz {
                FpSize::D => f.to_bits(),
                FpSize::S => {
                    let narrow = *f as f32;
                    if f64::from(narrow).to_bits() != f.to_bits() {
                        return fail(operand::FP_IMMEDIATE);
                    }
                    u64::from(narrow.to_bits())
                }
            };
            mc::try_fmov_imm(sz, rd, bits).map(word).ok_or(Refusal(operand::FP_IMMEDIATE))
        }
        _ => form(),
    }
}

/// Whether `name` is a local label reference.
pub fn is_local(v: &Value<'_>) -> bool {
    matches!(v.name, Some(Name::Local(..)))
}
