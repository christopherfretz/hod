//! AArch64 operands (`spec/asm.md` §5.1): registers, immediates,
//! shifts and extensions, memory operands, names.
//!
//! Register names are read without regard to case: `x0`..`x30`,
//! `w0`..`w30`, `xzr`, `wzr`, `sp`, `wsp`, the aliases `fp` (`x29`),
//! `lr` (`x30`), `x31`/`w31` (the zero register), and the scalar views
//! of the FP/SIMD registers `b0`..`q31`. A vector register with an
//! arrangement (`v0.4s`, [`Op::VReg`]), an element (`v0.s[1]`,
//! [`Op::VLane`]) and a register list (`{v0.4s, v1.4s}`,
//! `{v0.16b-v3.16b}`, `{v0.s, v1.s}[1]`, [`Op::VList`]) are read as
//! `llvm-mc` reads them (§20); any other vector form (`v0` with no
//! arrangement, an arrangement `llvm-mc` has no name for) parses as
//! [`Op::Vector`], so that the instruction refuses it by name.

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
    /// A vector register, element or register list of a form hod does
    /// not read.
    Vector,
    /// `v<n>.<count><view>`: a vector register with an arrangement
    /// (`v8.4s` is `(8, 4, S)`; `v8.1q` is `(8, 1, Q)`).
    VReg(u8, u8, View),
    /// `v<n>.<view>[<index>]`: one element (`v10.s[0]`).
    VLane(u8, View, u8),
    /// A register list.
    VList(VList),
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

/// A register list `{v<n>.<T>, ...}` or `{v<n>.<T> - v<m>.<T>}`, one to
/// four consecutive registers (`v31` is followed by `v0`) of one
/// arrangement, and for the single-structure forms an element index
/// after the brace (`{v0.s, v1.s}[1]`, the arrangement then a bare
/// element: `count` 0).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VList {
    /// The first register.
    pub first: u8,
    /// The number of registers, 1..=4.
    pub len: u8,
    /// The element count of the arrangement (0: an element, `v0.s`).
    pub count: u8,
    /// The element.
    pub view: View,
    /// The element index written after the list.
    pub index: Option<u8>,
}

impl Op<'_> {
    /// Whether it is a vector operand (a register with an arrangement, an
    /// element, a list, or a vector form hod does not read).
    pub const fn is_vector(&self) -> bool {
        matches!(self, Op::Vector | Op::VReg(..) | Op::VLane(..) | Op::VList(_))
    }

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

/// The number of a vector register name `v0`..`v31` (any case; the
/// caller has cut the arrangement at the `.`).
fn vector_number(name: &str) -> Option<u8> {
    let b = name.as_bytes();
    if b.len() >= 2 && b[0].eq_ignore_ascii_case(&b'v') { number(&b[1..], 31) } else { None }
}

/// An arrangement suffix as `llvm-mc` names them: `8b`, `16b`, `4b`,
/// `4h`, `8h`, `2h`, `2s`, `4s`, `1d`, `2d`, `1q` (`count`, `view`), or a
/// bare element `b`, `h`, `s`, `d`, `q` (count 0); any case.
fn arrangement(suffix: &str) -> Option<(u8, View)> {
    let (&last, digits) = suffix.as_bytes().split_last()?;
    let view = match last.to_ascii_lowercase() {
        b'b' => View::B,
        b'h' => View::H,
        b's' => View::S,
        b'd' => View::D,
        b'q' => View::Q,
        _ => return None,
    };
    let count = match digits {
        b"" => return Some((0, view)),
        b"1" => 1,
        b"2" => 2,
        b"4" => 4,
        b"8" => 8,
        b"16" => 16,
        _ => return None,
    };
    let named = matches!(
        (count, view),
        (4 | 8 | 16, View::B)
            | (2 | 4 | 8, View::H)
            | (2 | 4, View::S)
            | (1 | 2, View::D)
            | (1, View::Q)
    );
    named.then_some((count, view))
}

/// A vector register word (`v8.4s`, `v8.s`) as its number and
/// arrangement; `None` for one hod does not read.
fn vector_word(word: &str) -> Option<(u8, u8, View)> {
    let (base, suffix) = word.split_once('.')?;
    let n = vector_number(base)?;
    let (count, view) = arrangement(suffix)?;
    Some((n, count, view))
}

/// An element index `[i]`, 0..=255 (the instruction checks the range).
fn lane_index(c: &mut Cursor<'_>) -> Result<Option<u8>, Refusal> {
    if !c.eat(b'[') {
        return Ok(None);
    }
    let i = c.constant()?;
    if !c.eat(b']') {
        return err(operand::SYNTAX);
    }
    u8::try_from(i).map(Some).map_err(|_| Refusal(operand::RANGE))
}

/// The inside of `{...}` (the `{` consumed), the `}` and an index after
/// it; `Op::Vector` for a list `llvm-mc` rejects (registers that are not
/// consecutive, arrangements that differ, more than four).
fn vector_list<'a>(c: &mut Cursor<'a>) -> Result<Op<'a>, Refusal> {
    let mut regs = [(0u8, 0u8, View::B); 4];
    let mut len = 0usize;
    let mut range = false;
    loop {
        let Some((n, count, view)) = c.name().and_then(vector_word) else {
            return Ok(Op::Vector);
        };
        if range {
            // `first-last`: every register between them.
            let (first, fc, fv) = regs[0];
            let span = usize::from(n.wrapping_sub(first) & 31) + 1;
            if len != 1 || (count, view) != (fc, fv) || span > 4 {
                return Ok(Op::Vector);
            }
            for (k, r) in regs.iter_mut().enumerate().take(span).skip(1) {
                *r = ((first + k as u8) & 31, count, view);
            }
            len = span;
        } else {
            if len == 4 {
                return Ok(Op::Vector);
            }
            regs[len] = (n, count, view);
            len += 1;
        }
        if c.eat(b'}') {
            break;
        }
        if c.eat(b'-') && !range && len == 1 {
            range = true;
        } else if !c.eat(b',') || range {
            return Ok(Op::Vector);
        }
    }
    let (first, count, view) = regs[0];
    for (k, &(n, cn, vn)) in regs[..len].iter().enumerate() {
        if n != (first + k as u8) & 31 || (cn, vn) != (count, view) {
            return Ok(Op::Vector);
        }
    }
    let index = lane_index(c)?;
    if index.is_some() != (count == 0) {
        return Ok(Op::Vector);
    }
    Ok(Op::VList(VList { first, len: len as u8, count, view, index }))
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
        Some(b'{') => {
            c.at += 1;
            match vector_list(&mut c)? {
                Op::Vector => return Ok(Op::Vector),
                list => list,
            }
        }
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
                if vector_number(base).is_some() {
                    return match vector_word(word) {
                        Some((n, 0, view)) => match lane_index(&mut c)? {
                            Some(i) if c.at_end() => Ok(Op::VLane(n, view, i)),
                            _ => Ok(Op::Vector),
                        },
                        Some((n, count, view)) if c.at_end() => Ok(Op::VReg(n, count, view)),
                        _ => Ok(Op::Vector),
                    };
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
        assert_eq!(one("v1.4s"), Op::VReg(1, 4, View::S));
        assert_eq!(one("V31.16B"), Op::VReg(31, 16, View::B));
        assert_eq!(one("v8.1q"), Op::VReg(8, 1, View::Q));
        assert_eq!(one("v10.s[3]"), Op::VLane(10, View::S, 3));
        assert_eq!(one("v1"), Op::Vector);
        assert_eq!(one("v1.3s"), Op::Vector);
        assert_eq!(one("v1.s"), Op::Vector);
        assert_eq!(one("v1.4s[1]"), Op::Vector);
        let list = |first, len, count, view, index| VList { first, len, count, view, index };
        assert_eq!(one("{v0.16b}"), Op::VList(list(0, 1, 16, View::B, None)));
        assert_eq!(one("{ v30.4s, v31.4s, v0.4s }"), Op::VList(list(30, 3, 4, View::S, None)));
        assert_eq!(one("{v4.2d-v7.2d}"), Op::VList(list(4, 4, 2, View::D, None)));
        assert_eq!(one("{v1.s, v2.s}[3]"), Op::VList(list(1, 2, 0, View::S, Some(3))));
        assert_eq!(one("{v0.4s, v2.4s}"), Op::Vector);
        assert_eq!(one("{v0.4s, v1.2s}"), Op::Vector);
        assert_eq!(one("{v0.s}"), Op::Vector);
        assert_eq!(one("{v0.4s-v4.4s}"), Op::Vector);
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
