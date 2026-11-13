//! AArch64 operands (`spec/asm.md` §5.1): registers, immediates,
//! shifts and extensions, memory operands, names.
//!
//! Register names are read without regard to case: `x0`..`x30`,
//! `w0`..`w30`, `xzr`, `wzr`, `sp`, `wsp`, the aliases `fp` (`x29`),
//! `lr` (`x30`), `x31`/`w31` (the zero register), and the scalar views
//! of the FP/SIMD registers `b0`..`q31`. A vector register (`v0`,
//! `v0.4s`, `v0.s[1]`) and a register list (`{...}`) parse, as
//! [`Op::Vector`], so that the instruction refuses them by name.

use hod_mc_a64::{Extend, Shift};

use crate::expr::{Cursor, Name, Value};
use crate::reason::{Refusal, operand};

/// A general register operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gp {
    /// `x<n>`, `n` in 0..=30.
    X(u8),
    /// `w<n>`, `n` in 0..=30.
    W(u8),
    /// `xzr`.
    Xzr,
    /// `wzr`.
    Wzr,
    /// `sp`.
    Sp,
    /// `wsp`.
    Wsp,
}

impl Gp {
    /// Whether it is a 64-bit name.
    pub const fn is_x(self) -> bool {
        matches!(self, Gp::X(_) | Gp::Xzr | Gp::Sp)
    }

    /// Whether it is the stack pointer (`sp` or `wsp`).
    pub const fn is_sp(self) -> bool {
        matches!(self, Gp::Sp | Gp::Wsp)
    }

    /// Whether it is the zero register.
    pub const fn is_zr(self) -> bool {
        matches!(self, Gp::Xzr | Gp::Wzr)
    }
}

/// The scalar view of an FP/SIMD register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// `b<n>`: 8 bits.
    B,
    /// `h<n>`: 16 bits.
    H,
    /// `s<n>`: 32 bits.
    S,
    /// `d<n>`: 64 bits.
    D,
    /// `q<n>`: 128 bits.
    Q,
}

/// The register-offset index of a memory operand: the register and its
/// extension or shift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Index {
    /// The index register.
    pub reg: Gp,
    /// `lsl`, `uxtw`, `sxtw` or `sxtx` (`None`: no extension written).
    pub ext: Option<IndexExt>,
    /// The amount written after the extension (`None`: none written).
    pub amount: Option<u32>,
}

/// The extension of an index register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IndexExt {
    /// `lsl`.
    Lsl,
    /// `uxtw`.
    Uxtw,
    /// `sxtw`.
    Sxtw,
    /// `sxtx`.
    Sxtx,
}

/// What follows the base register of a memory operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Offset {
    /// `[xn]`.
    None,
    /// `[xn, #imm]`.
    Imm(i64),
    /// `[xn, rm{, ext #amount}]`.
    Reg(Index),
}

/// A memory operand `[base{, offset}]{!}`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mem {
    /// The base: `x0`..`x30` or `sp` (a `Gp`; any other is refused by
    /// the instruction).
    pub base: Gp,
    /// The offset.
    pub offset: Offset,
    /// `!`: pre-index write-back.
    pub writeback: bool,
}

/// One operand.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op<'a> {
    /// A general register.
    Gp(Gp),
    /// A scalar view of an FP/SIMD register: `(n, view)`.
    Fp(u8, View),
    /// A vector register, element or register list.
    Vector,
    /// A constant: `#expr` (`true`) or a bare constant expression.
    Imm(i64, bool),
    /// A floating-point immediate `#1.5`.
    FpImm(f64),
    /// An expression that names a symbol or label, or a bare name (a
    /// condition, a system register, a barrier option).
    Name(Value<'a>),
    /// `lsl #n`, `lsr #n`, `asr #n`, `ror #n`.
    Shift(Shift, u32),
    /// `uxtb`.. `sxtx`, with the amount if written.
    Extend(Extend, Option<u32>),
    /// A memory operand.
    Mem(Mem),
    /// `=value` (a literal pool load).
    Literal,
}

impl Op<'_> {
    /// The bare name of an operand (`eq`, `ish`, `tpidr_el0`), lower
    /// case is the caller's.
    pub fn ident(&self) -> Option<&str> {
        match self {
            Op::Name(Value { name: Some(Name::Sym(s)), constant: 0 }) => Some(s),
            _ => None,
        }
    }
}

/// The most operands a statement has.
pub const MAX_OPS: usize = 6;

/// The operands of a statement, without allocating.
#[derive(Clone, Copy, Debug)]
pub struct Ops<'a> {
    list: [Op<'a>; MAX_OPS],
    len: usize,
}

impl<'a> Ops<'a> {
    /// The operands.
    pub fn as_slice(&self) -> &[Op<'a>] {
        &self.list[..self.len]
    }
}

fn err<T>(what: &'static str) -> Result<T, Refusal> {
    Err(Refusal(what))
}

/// A general register name.
pub fn gp_name(name: &str) -> Option<Gp> {
    let lower = name.as_bytes();
    let eq = |s: &str| name.eq_ignore_ascii_case(s);
    if eq("sp") {
        return Some(Gp::Sp);
    }
    if eq("wsp") {
        return Some(Gp::Wsp);
    }
    if eq("xzr") || eq("x31") {
        return Some(Gp::Xzr);
    }
    if eq("wzr") || eq("w31") {
        return Some(Gp::Wzr);
    }
    if eq("fp") {
        return Some(Gp::X(29));
    }
    if eq("lr") {
        return Some(Gp::X(30));
    }
    let (&first, digits) = lower.split_first()?;
    let n = number(digits, 30)?;
    match first.to_ascii_lowercase() {
        b'x' => Some(Gp::X(n)),
        b'w' => Some(Gp::W(n)),
        _ => None,
    }
}

/// A register number `0..=max` written without leading zeros.
fn number(digits: &[u8], max: u32) -> Option<u8> {
    if digits.is_empty() || digits.len() > 2 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits.len() == 2 && digits[0] == b'0' {
        return None;
    }
    let n = digits.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0'));
    (n <= max).then_some(n as u8)
}

/// An FP/SIMD scalar register name.
pub fn fp_name(name: &str) -> Option<(u8, View)> {
    let (&first, digits) = name.as_bytes().split_first()?;
    let n = number(digits, 31)?;
    let view = match first.to_ascii_lowercase() {
        b'b' => View::B,
        b'h' => View::H,
        b's' => View::S,
        b'd' => View::D,
        b'q' => View::Q,
        _ => return None,
    };
    Some((n, view))
}

/// Whether `name` is a vector register `v0`..`v31` (with or without an
/// arrangement, which the caller has cut at the `.`).
fn is_vector_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 2 && b[0].eq_ignore_ascii_case(&b'v') && number(&b[1..], 31).is_some()
}

fn shift_name(name: &str) -> Option<Shift> {
    Some(match name.to_ascii_lowercase().as_str() {
        "lsl" => Shift::Lsl,
        "lsr" => Shift::Lsr,
        "asr" => Shift::Asr,
        "ror" => Shift::Ror,
        _ => return None,
    })
}

fn extend_name(name: &str) -> Option<Extend> {
    Some(match name.to_ascii_lowercase().as_str() {
        "uxtb" => Extend::Uxtb,
        "uxth" => Extend::Uxth,
        "uxtw" => Extend::Uxtw,
        "uxtx" => Extend::Uxtx,
        "sxtb" => Extend::Sxtb,
        "sxth" => Extend::Sxth,
        "sxtw" => Extend::Sxtw,
        "sxtx" => Extend::Sxtx,
        _ => return None,
    })
}

/// Splits `text` at the commas outside brackets and braces and parses
/// each operand.
///
/// # Errors
/// [`operand::SYNTAX`], [`operand::FORM`] (more than [`MAX_OPS`]), and
/// what [`parse_one`] reports.
pub fn parse(text: &str) -> Result<Ops<'_>, Refusal> {
    let mut ops = Ops { list: [Op::Literal; MAX_OPS], len: 0 };
    let text = crate::text::trim(text);
    if text.is_empty() {
        return Ok(ops);
    }
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i <= b.len() {
        let c = b.get(i).copied();
        match c {
            Some(b'[' | b'{' | b'(') => depth += 1,
            Some(b']' | b'}' | b')') => depth -= 1,
            Some(b'\'') => {
                i += if b.get(i + 1) == Some(&b'\\') { 4 } else { 3 };
                continue;
            }
            Some(b',') | None if depth == 0 => {
                if ops.len == MAX_OPS {
                    return err(operand::FORM);
                }
                ops.list[ops.len] = parse_one(crate::text::trim(&text[start..i.min(b.len())]))?;
                ops.len += 1;
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return err(operand::SYNTAX);
        }
        i += 1;
    }
    if depth != 0 {
        return err(operand::SYNTAX);
    }
    Ok(ops)
}

/// One operand.
///
/// # Errors
/// [`operand::SYNTAX`], [`operand::SPECIFIER`], and what the expression
/// reader reports.
pub fn parse_one(text: &str) -> Result<Op<'_>, Refusal> {
    let mut c = Cursor::new(text);
    let op = match c.peek() {
        None => return err(operand::SYNTAX),
        Some(b'[') => {
            c.at += 1;
            Op::Mem(mem(&mut c)?)
        }
        Some(b'{') => return Ok(Op::Vector),
        Some(b'=') => return Ok(Op::Literal),
        Some(b'#') => {
            c.at += 1;
            if c.peek() == Some(b':') {
                return err(operand::SPECIFIER);
            }
            if let Some(f) = fp_literal(c.rest()) {
                return Ok(Op::FpImm(f));
            }
            Op::Imm(c.constant()?, true)
        }
        Some(b':') => return err(operand::SPECIFIER),
        Some(_) => {
            let save = c;
            if let Some(word) = c.name() {
                // A register, possibly with an arrangement or element.
                let base = word.split('.').next().unwrap_or(word);
                if is_vector_name(base) {
                    return Ok(Op::Vector);
                }
                if let Some(g) = gp_name(word) {
                    Op::Gp(g)
                } else if let Some((n, v)) = fp_name(word) {
                    Op::Fp(n, v)
                } else if let Some(s) = shift_name(word) {
                    Op::Shift(s, amount(&mut c)?.ok_or(Refusal(operand::SYNTAX))?)
                } else if let Some(e) = extend_name(word) {
                    Op::Extend(e, amount(&mut c)?)
                } else {
                    c = save;
                    name_or_imm(c.expr()?)
                }
            } else {
                c = save;
                name_or_imm(c.expr()?)
            }
        }
    };
    if !c.at_end() {
        return err(operand::SYNTAX);
    }
    Ok(op)
}

fn name_or_imm(v: Value<'_>) -> Op<'_> {
    if v.name.is_none() { Op::Imm(v.constant, false) } else { Op::Name(v) }
}

/// A floating-point literal (`1.0`, `-0.5`, `1e3`, `1.5e-2`): digits
/// with a `.` or an exponent, nothing else after.
fn fp_literal(text: &str) -> Option<f64> {
    let t = crate::text::trim(text);
    let body = t.strip_prefix(['-', '+']).unwrap_or(t);
    let decimal = body.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && !body.starts_with("0x")
        && !body.starts_with("0X")
        && (body.contains('.') || body.contains(['e', 'E']));
    if !decimal || !body.bytes().all(|b| b.is_ascii_digit() || b".eE+-".contains(&b)) {
        return None;
    }
    t.parse().ok()
}

/// An optional `#n` or `n` after a shift or extension name.
fn amount(c: &mut Cursor<'_>) -> Result<Option<u32>, Refusal> {
    if c.at_end() || c.peek() == Some(b']') || c.peek() == Some(b',') {
        return Ok(None);
    }
    c.eat(b'#');
    let v = c.constant()?;
    u32::try_from(v).map(Some).map_err(|_| Refusal(operand::RANGE))
}

/// The inside of `[...]`, then `]` and `!`.
fn mem(c: &mut Cursor<'_>) -> Result<Mem, Refusal> {
    let Some(base) = c.name().and_then(gp_name) else { return err(operand::FORM) };
    let mut offset = Offset::None;
    if c.eat(b',') {
        if c.peek() == Some(b'#') {
            c.at += 1;
            if c.peek() == Some(b':') {
                return err(operand::SPECIFIER);
            }
            offset = Offset::Imm(c.constant()?);
        } else if c.peek() == Some(b':') {
            return err(operand::SPECIFIER);
        } else {
            let save = *c;
            if let Some(reg) = c.name().and_then(gp_name) {
                let mut index = Index { reg, ext: None, amount: None };
                if c.eat(b',') {
                    let ext = match c.name().map(str::to_ascii_lowercase).as_deref() {
                        Some("lsl") => IndexExt::Lsl,
                        Some("uxtw") => IndexExt::Uxtw,
                        Some("sxtw") => IndexExt::Sxtw,
                        Some("sxtx") => IndexExt::Sxtx,
                        _ => return err(operand::FORM),
                    };
                    index.ext = Some(ext);
                    index.amount = amount(c)?;
                    if ext == IndexExt::Lsl && index.amount.is_none() {
                        return err(operand::SYNTAX);
                    }
                }
                offset = Offset::Reg(index);
            } else {
                *c = save;
                offset = Offset::Imm(c.constant()?);
            }
        }
    }
    if !c.eat(b']') {
        return err(operand::SYNTAX);
    }
    let writeback = c.eat(b'!');
    Ok(Mem { base, offset, writeback })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(s: &str) -> Op<'_> {
        parse_one(s).unwrap()
    }

    /// `spec/asm.md` §5.1.
    #[test]
    fn operands() {
        assert_eq!(one("X3"), Op::Gp(Gp::X(3)));
        assert_eq!(one("w30"), Op::Gp(Gp::W(30)));
        assert_eq!(one("fp"), Op::Gp(Gp::X(29)));
        assert_eq!(one("x31"), Op::Gp(Gp::Xzr));
        assert_eq!(one("wsp"), Op::Gp(Gp::Wsp));
        assert_eq!(one("d7"), Op::Fp(7, View::D));
        assert_eq!(one("v1.4s"), Op::Vector);
        assert_eq!(one("#-8"), Op::Imm(-8, true));
        assert_eq!(one("42"), Op::Imm(42, false));
        assert_eq!(one("#1.5"), Op::FpImm(1.5));
        assert_eq!(one("lsl #12"), Op::Shift(Shift::Lsl, 12));
        assert_eq!(one("sxtw"), Op::Extend(Extend::Sxtw, None));
        assert_eq!(one("uxtb #2"), Op::Extend(Extend::Uxtb, Some(2)));
        assert_eq!(
            one("[sp, #16]!"),
            Op::Mem(Mem { base: Gp::Sp, offset: Offset::Imm(16), writeback: true })
        );
        assert_eq!(
            one("[x1, w2, sxtw #3]"),
            Op::Mem(Mem {
                base: Gp::X(1),
                offset: Offset::Reg(Index {
                    reg: Gp::W(2),
                    ext: Some(IndexExt::Sxtw),
                    amount: Some(3)
                }),
                writeback: false,
            })
        );
        assert_eq!(one("eq").ident(), Some("eq"));
        assert_eq!(one("1f"), Op::Name(Value { name: Some(Name::Local(1, true)), constant: 0 }));
        assert!(parse_one("#:lo12:x").is_err());
        assert!(parse_one("[x1").is_err());
        let ops = parse("x0, [x1], #8").unwrap();
        assert_eq!(ops.as_slice().len(), 3);
        assert!(parse("x0, x1, x2, x3, x4, x5, x6").is_err());
    }
}
