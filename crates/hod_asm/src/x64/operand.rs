//! x86-64 AT&T-syntax operands (`spec/asm.md` §10.2): registers,
//! immediates, memory operands, and the bare expressions of branch
//! targets and absolute addresses.
//!
//! Register names are read without regard to case, after a `%`: the
//! general registers in each of their views (`rax`, `eax`, `ax`, `al`,
//! `sil`, `r8`, `r8d`, `r8w`, `r8b`), `xmm0`..`xmm15`, and `rip` as a
//! base. The high-byte registers `ah`..`dh`, the segment registers as
//! operands, the x87, MMX, AVX and control registers are
//! `asm:operand/register`.

use hod_mc_x64::{Reg, Scale, Seg, Size, Xmm};

use crate::expr::{Cursor, Value};
use crate::reason::{Refusal, operand};

/// A general register in one of its views.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gpr {
    /// The register.
    pub reg: Reg,
    /// The view: `al` is `Rax` with `Size::B`.
    pub size: Size,
}

/// A memory operand's base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    /// A 64-bit general register.
    Reg(Reg),
    /// `%rip`.
    Rip,
}

/// A memory operand: `seg:disp(base, index, scale)`, any part but one
/// optional.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemOp<'a> {
    /// `%fs:` or `%gs:`.
    pub seg: Option<Seg>,
    /// The displacement.
    pub disp: Value<'a>,
    /// The base register.
    pub base: Option<Base>,
    /// The index register and its scale.
    pub index: Option<(Reg, Scale)>,
    /// Written as an expression alone, without parentheses or segment:
    /// a branch target, or an absolute address.
    pub bare: bool,
}

/// One operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op<'a> {
    /// A general register.
    Gpr(Gpr),
    /// An SSE register.
    Xmm(Xmm),
    /// `$expr`.
    Imm(Value<'a>),
    /// A memory operand or a bare expression.
    Mem(MemOp<'a>),
}

/// The operands of one statement, at most four.
#[derive(Clone, Copy, Default, Debug)]
pub struct Ops<'a> {
    list: [Option<Op<'a>>; 4],
    len: usize,
}

impl<'a> Ops<'a> {
    /// The number of operands.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Operand `i`.
    pub fn get(&self, i: usize) -> Option<Op<'a>> {
        self.list.get(i).copied().flatten()
    }
}

const GPR64: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];
const GPR32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const GPR16: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
const GPR8: [&str; 8] = ["al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil"];

/// The name of general register `reg` in view `size`, without `%`.
pub fn gpr_name(reg: Reg, size: Size) -> String {
    let n = usize::from(reg.code());
    match (size, n) {
        (Size::Q, _) => GPR64[n].to_owned(),
        (Size::D, 0..=7) => GPR32[n].to_owned(),
        (Size::W, 0..=7) => GPR16[n].to_owned(),
        (Size::B, 0..=7) => GPR8[n].to_owned(),
        (Size::D, _) => format!("r{n}d"),
        (Size::W, _) => format!("r{n}w"),
        (Size::B, _) => format!("r{n}b"),
    }
}

/// What a register name (without `%`, lower case) denotes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegName {
    /// A general register view.
    Gpr(Gpr),
    /// An SSE register.
    Xmm(Xmm),
    /// `rip`.
    Rip,
    /// `fs`.
    Fs,
    /// `gs`.
    Gs,
}

/// The register `name` names (lower case, no `%`), or `None`.
pub fn reg_name(name: &str) -> Option<RegName> {
    let gpr = |n: usize, size| {
        Some(RegName::Gpr(Gpr { reg: Reg::new(n as u8).expect("a register number"), size }))
    };
    if let Some(n) = GPR64.iter().position(|&r| r == name) {
        return gpr(n, Size::Q);
    }
    if let Some(n) = GPR32.iter().position(|&r| r == name) {
        return gpr(n, Size::D);
    }
    if let Some(n) = GPR16.iter().position(|&r| r == name) {
        return gpr(n, Size::W);
    }
    if let Some(n) = GPR8.iter().position(|&r| r == name) {
        return gpr(n, Size::B);
    }
    match name {
        "rip" => return Some(RegName::Rip),
        "fs" => return Some(RegName::Fs),
        "gs" => return Some(RegName::Gs),
        _ => {}
    }
    let number = |digits: &str| -> Option<usize> {
        let ok = !digits.is_empty()
            && digits.len() <= 2
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits.len() == 1 || !digits.starts_with('0'));
        if ok { digits.parse().ok() } else { None }
    };
    if let Some(digits) = name.strip_prefix("xmm") {
        let n = number(digits)?;
        return Xmm::new(u8::try_from(n).ok()?).map(RegName::Xmm);
    }
    let rest = name.strip_prefix('r')?;
    let (digits, size) = match rest.as_bytes().last()? {
        b'd' => (&rest[..rest.len() - 1], Size::D),
        b'w' => (&rest[..rest.len() - 1], Size::W),
        b'b' => (&rest[..rest.len() - 1], Size::B),
        _ => (rest, Size::Q),
    };
    let n = number(digits)?;
    if (8..=15).contains(&n) { gpr(n, size) } else { None }
}

/// Splits `text` at the commas outside parentheses.
fn split_operands(text: &str) -> Result<Vec<&str>, Refusal> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return Err(Refusal(operand::SYNTAX));
                }
            }
            b',' if depth == 0 => {
                out.push(crate::text::trim(&text[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(Refusal(operand::SYNTAX));
    }
    out.push(crate::text::trim(&text[start..]));
    Ok(out)
}

/// Parses the operand text of a statement.
///
/// # Errors
/// `asm:operand/syntax`, `register`, `form`, `expression`.
pub fn parse(text: &str) -> Result<Ops<'_>, Refusal> {
    let mut ops = Ops::default();
    if crate::text::trim(text).is_empty() {
        return Ok(ops);
    }
    for part in split_operands(text)? {
        if ops.len == ops.list.len() {
            return Err(Refusal(operand::FORM));
        }
        ops.list[ops.len] = Some(one(part)?);
        ops.len += 1;
    }
    Ok(ops)
}

/// `%name` at the start of `text`: the register and the rest.
fn register(text: &str) -> Result<(RegName, &str), Refusal> {
    let body = text.strip_prefix('%').ok_or(Refusal(operand::SYNTAX))?;
    let end = body.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(body.len());
    if end == 0 {
        return Err(Refusal(operand::SYNTAX));
    }
    let name = body[..end].to_ascii_lowercase();
    let r = reg_name(&name).ok_or(Refusal(operand::REGISTER))?;
    Ok((r, crate::text::trim(&body[end..])))
}

/// A constant expression that is all of `text`.
fn whole_expr(text: &str) -> Result<Value<'_>, Refusal> {
    let mut c = Cursor::new(text);
    let v = c.expr()?;
    if !c.at_end() {
        return Err(Refusal(operand::SYNTAX));
    }
    Ok(v)
}

fn one(text: &str) -> Result<Op<'_>, Refusal> {
    if text.is_empty() {
        return Err(Refusal(operand::SYNTAX));
    }
    if text.starts_with('*') {
        // An indirect branch target.
        return Err(Refusal(operand::FORM));
    }
    if let Some(imm) = text.strip_prefix('$') {
        return Ok(Op::Imm(whole_expr(crate::text::trim(imm))?));
    }
    let mut rest = text;
    let mut seg = None;
    if rest.starts_with('%') {
        let (r, after) = register(rest)?;
        match r {
            RegName::Gpr(g) if after.is_empty() => return Ok(Op::Gpr(g)),
            RegName::Xmm(x) if after.is_empty() => return Ok(Op::Xmm(x)),
            RegName::Fs | RegName::Gs => {
                let Some(after) = after.strip_prefix(':') else {
                    return Err(Refusal(operand::REGISTER));
                };
                seg = Some(if r == RegName::Fs { Seg::Fs } else { Seg::Gs });
                rest = crate::text::trim(after);
            }
            _ => return Err(Refusal(operand::SYNTAX)),
        }
    }
    memory(rest, seg)
}

/// `disp(base, index, scale)` or `disp` alone, after any segment.
fn memory(text: &str, seg: Option<Seg>) -> Result<Op<'_>, Refusal> {
    // The parenthesised part is the last one whose content starts with
    // a register or a comma (an expression may have parentheses too).
    let regs_at = if text.ends_with(')') {
        let mut depth = 0;
        let mut open = None;
        for (i, b) in text.bytes().enumerate().rev() {
            match b {
                b')' => depth += 1,
                b'(' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let open = open.ok_or(Refusal(operand::SYNTAX))?;
        let inner = crate::text::trim(&text[open + 1..text.len() - 1]);
        (inner.starts_with('%') || inner.starts_with(',')).then_some((open, inner))
    } else {
        None
    };
    let (disp_text, inner) = match regs_at {
        Some((open, inner)) => (crate::text::trim(&text[..open]), Some(inner)),
        None => (text, None),
    };
    let disp = if disp_text.is_empty() {
        if inner.is_none() {
            return Err(Refusal(operand::SYNTAX));
        }
        Value::constant(0)
    } else {
        whole_expr(disp_text)?
    };
    let mut mem =
        MemOp { seg, disp, base: None, index: None, bare: seg.is_none() && inner.is_none() };
    let Some(inner) = inner else { return Ok(Op::Mem(mem)) };
    let parts: Vec<&str> = inner.split(',').map(crate::text::trim).collect();
    if parts.len() > 3 {
        return Err(Refusal(operand::SYNTAX));
    }
    let reg64 = |t: &str| -> Result<Option<RegName>, Refusal> {
        if t.is_empty() {
            return Ok(None);
        }
        let (r, after) = register(t)?;
        if !after.is_empty() {
            return Err(Refusal(operand::SYNTAX));
        }
        Ok(Some(r))
    };
    match reg64(parts[0])? {
        None => {}
        Some(RegName::Gpr(Gpr { reg, size: Size::Q })) => mem.base = Some(Base::Reg(reg)),
        Some(RegName::Rip) if parts.len() == 1 => mem.base = Some(Base::Rip),
        // A 32-bit address (`addr32`) or another register.
        Some(_) => return Err(Refusal(operand::REGISTER)),
    }
    if parts.len() >= 2 {
        let index = match reg64(parts[1])? {
            Some(RegName::Gpr(Gpr { reg, size: Size::Q })) if reg != Reg::Rsp => reg,
            None => return Err(Refusal(operand::SYNTAX)),
            Some(_) => return Err(Refusal(operand::REGISTER)),
        };
        let scale = if parts.len() == 3 {
            let v = whole_expr(parts[2])?;
            if v.name.is_some() {
                return Err(Refusal(operand::EXPRESSION));
            }
            match v.constant {
                1 => Scale::S1,
                2 => Scale::S2,
                4 => Scale::S4,
                8 => Scale::S8,
                _ => return Err(Refusal(operand::RANGE)),
            }
        } else {
            Scale::S1
        };
        mem.index = Some((index, scale));
    }
    if mem.base.is_none() && mem.index.is_none() {
        return Err(Refusal(operand::SYNTAX));
    }
    Ok(Op::Mem(mem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Name;

    fn op(text: &str) -> Result<Op<'_>, Refusal> {
        let ops = parse(text)?;
        assert_eq!(ops.len(), 1, "{text}");
        Ok(ops.get(0).unwrap())
    }

    /// `spec/asm.md` §10.2: every operand kind.
    #[test]
    fn operands() {
        assert_eq!(op("%eax").unwrap(), Op::Gpr(Gpr { reg: Reg::Rax, size: Size::D }));
        assert_eq!(op("%R9B").unwrap(), Op::Gpr(Gpr { reg: Reg::R9, size: Size::B }));
        assert_eq!(op("%sil").unwrap(), Op::Gpr(Gpr { reg: Reg::Rsi, size: Size::B }));
        assert_eq!(op("%xmm15").unwrap(), Op::Xmm(Xmm::Xmm15));
        assert_eq!(op("$-3").unwrap(), Op::Imm(Value::constant(-3)));
        assert_eq!(op("$(1 << 4)").unwrap(), Op::Imm(Value::constant(16)));
        let Op::Mem(m) = op("-8(%rbp,%rcx,4)").unwrap() else { panic!() };
        assert_eq!(
            (m.disp.constant, m.base, m.index, m.bare),
            (-8, Some(Base::Reg(Reg::Rbp)), Some((Reg::Rcx, Scale::S4)), false)
        );
        let Op::Mem(m) = op("%fs:0x28").unwrap() else { panic!() };
        assert_eq!((m.seg, m.disp.constant, m.base, m.bare), (Some(Seg::Fs), 40, None, false));
        let Op::Mem(m) = op("%gs:(%rax)").unwrap() else { panic!() };
        assert_eq!((m.seg, m.base), (Some(Seg::Gs), Some(Base::Reg(Reg::Rax))));
        let Op::Mem(m) = op("(,%rdi,8)").unwrap() else { panic!() };
        assert_eq!((m.base, m.index), (None, Some((Reg::Rdi, Scale::S8))));
        let Op::Mem(m) = op("1f").unwrap() else { panic!() };
        assert_eq!((m.disp.name, m.bare), (Some(Name::Local(1, true)), true));
        let Op::Mem(m) = op("(2*4)(%rax)").unwrap() else { panic!() };
        assert_eq!(m.disp.constant, 8);
        assert_eq!(parse("%eax, (%rdi)").unwrap().len(), 2);
        assert_eq!(op("%ah").err(), Some(Refusal(operand::REGISTER)));
        assert_eq!(op("(%eax)").err(), Some(Refusal(operand::REGISTER)));
        assert_eq!(op("(%rax,%rsp)").err(), Some(Refusal(operand::REGISTER)));
        assert_eq!(op("(%rax,%rcx,3)").err(), Some(Refusal(operand::RANGE)));
        assert_eq!(op("*%rax").err(), Some(Refusal(operand::FORM)));
        assert_eq!(op("%st(1)").err(), Some(Refusal(operand::REGISTER)));
        assert!(op("(%rax").is_err());
        assert_eq!(gpr_name(Reg::R10, Size::D), "r10d");
        assert_eq!(gpr_name(Reg::Rdi, Size::B), "dil");
    }
}
