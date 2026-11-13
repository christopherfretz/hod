//! x86-64 AT&T-syntax assembly (`spec/asm.md` §10): operand printing for
//! inline-assembly templates ([`print_operand`]), and the assembler of
//! an inline-assembly body ([`assemble`]): statements, prefixes, data
//! directives, numeric local labels and branches to them, and
//! instructions ([`encode`]) whose bytes come from `hod_mc_x64`'s
//! constructors.
//!
//! As on AArch64, an inline body defines no symbol: a named label, a
//! directive other than the data directives, or a reference to a symbol
//! is refused (`asm:label`, `asm:directive/<name>`, `asm:operand/symbol`).
//! x86-64 bodies have no symbol references at all: every byte is known
//! here.

pub mod encode;
pub mod operand;

use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Size};

use crate::expr::{Cursor, Name, Value};
use crate::reason::{self, Refusal, operand as op_reason, other};
use crate::text::{is_space, statements, trim};
use encode::Encoded;

/// Writes `text` with its comments replaced by one space each into `out`
/// (cleared first), as LLVM's lexer reads `x86_64-*-linux-*` assembly:
/// `#` and `//` start a comment to the end of the line anywhere outside
/// a string or character literal, `/* ... */` is a comment anywhere.
///
/// # Errors
/// [`other::LEXICAL`] for an unterminated block comment, string or
/// character literal.
pub fn clean(text: &str, out: &mut String) -> Result<(), Refusal> {
    out.clear();
    let t = text.as_bytes();
    let mut i = 0;
    let mut from = 0;
    while i < t.len() {
        match t[i] {
            b'"' => {
                let mut j = i + 1;
                loop {
                    match t.get(j) {
                        None | Some(b'\n') => return Err(Refusal(other::LEXICAL)),
                        Some(b'\\') => j += 2,
                        Some(b'"') => break,
                        Some(_) => j += 1,
                    }
                }
                i = j + 1;
            }
            b'\'' => {
                let len = if t.get(i + 1) == Some(&b'\\') { 4 } else { 3 };
                if t.get(i + len - 1) != Some(&b'\'') {
                    return Err(Refusal(other::LEXICAL));
                }
                i += len;
            }
            b'#' => {
                out.push_str(&text[from..i]);
                out.push(' ');
                while i < t.len() && t[i] != b'\n' {
                    i += 1;
                }
                from = i;
            }
            b'/' if t.get(i + 1) == Some(&b'/') => {
                out.push_str(&text[from..i]);
                out.push(' ');
                while i < t.len() && t[i] != b'\n' {
                    i += 1;
                }
                from = i;
            }
            b'/' if t.get(i + 1) == Some(&b'*') => {
                out.push_str(&text[from..i]);
                out.push(' ');
                let Some(close) = text[i + 2..].find("*/") else {
                    return Err(Refusal(other::LEXICAL));
                };
                i += 2 + close + 2;
                from = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&text[from..]);
    Ok(())
}

/// One operand of an inline-assembly call as the template prints it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arg {
    /// A general register, by number, and the bytes of the operand's
    /// type (1, 2, 4 or 8): the view printed without a modifier.
    Gp(u8, u8),
    /// An SSE register `xmm<n>`.
    Xmm(u8),
    /// A constant.
    Imm(i64),
    /// A memory operand whose address is in general register `n`.
    Mem(u8),
}

/// Writes `arg` with `modifier` as LLVM's `X86AsmPrinter` does in AT&T
/// syntax (`PrintAsmOperand`, `PrintAsmMemoryOperand`):
///
/// - a general register is `%` and its name in the view of the operand's
///   type; `b`, `w`, `k`, `q` select the 8-, 16-, 32-, 64-bit view; `c`
///   prints it plainly; `a` prints `(%reg)` (the 64-bit view);
/// - an SSE register is `%xmm<n>` (no modifier, `x`, `c`);
/// - a constant is `$<value>`; `c`, `P` and `a` print it without `$`,
///   `n` its negation; `b`, `w`, `k`, `q` print it as no modifier does;
/// - a memory operand is `(%reg)`; `b`, `w`, `k`, `q` change nothing, `H`
///   is `8(%reg)`.
///
/// # Errors
/// `asm:modifier/<m>` for a modifier the operand does not take (`h`, the
/// high-byte view, is one: hod has no `ah`..`dh`).
pub fn print_operand(arg: Arg, modifier: Option<u8>, out: &mut String) -> Result<(), Refusal> {
    use std::fmt::Write as _;
    let bad = |m: u8| Err(reason::modifier(&char::from(m).to_string()));
    let gp = |n: u8| mc::Reg::new(n).ok_or(Refusal(other::TEMPLATE));
    let size = |bytes: u8| match bytes {
        1 => Size::B,
        2 => Size::W,
        4 => Size::D,
        _ => Size::Q,
    };
    match (arg, modifier) {
        (Arg::Gp(n, bytes), None | Some(b'c')) => {
            out.push('%');
            out.push_str(&operand::gpr_name(gp(n)?, size(bytes)));
        }
        (Arg::Gp(n, _), Some(m @ (b'b' | b'w' | b'k' | b'q'))) => {
            let s = match m {
                b'b' => Size::B,
                b'w' => Size::W,
                b'k' => Size::D,
                _ => Size::Q,
            };
            out.push('%');
            out.push_str(&operand::gpr_name(gp(n)?, s));
        }
        (Arg::Gp(n, _), Some(b'a')) | (Arg::Mem(n), None | Some(b'b' | b'w' | b'k' | b'q')) => {
            out.push_str("(%");
            out.push_str(&operand::gpr_name(gp(n)?, Size::Q));
            out.push(')');
        }
        (Arg::Mem(n), Some(b'H')) => {
            out.push_str("8(%");
            out.push_str(&operand::gpr_name(gp(n)?, Size::Q));
            out.push(')');
        }
        (Arg::Xmm(n), None | Some(b'x' | b'c')) => {
            write!(out, "%xmm{n}").map_err(|_| Refusal(other::TEMPLATE))?;
        }
        (Arg::Imm(v), None | Some(b'b' | b'w' | b'k' | b'q')) => {
            write!(out, "${v}").map_err(|_| Refusal(other::TEMPLATE))?;
        }
        (Arg::Imm(v), Some(b'c' | b'P' | b'a')) => {
            write!(out, "{v}").map_err(|_| Refusal(other::TEMPLATE))?;
        }
        (Arg::Imm(v), Some(b'n')) => {
            write!(out, "{}", v.wrapping_neg()).map_err(|_| Refusal(other::TEMPLATE))?;
        }
        (_, Some(m)) => return bad(m),
    }
    Ok(())
}

/// One item of a body: bytes, a branch to a local label, a label.
#[derive(Clone, Copy, Debug)]
enum Item {
    /// Bytes `at..at + len` of the byte buffer.
    Bytes { at: usize, len: usize },
    /// A branch to local label `n`; `forward` for `nf`.
    Branch { cond: Option<Cond>, n: u32, forward: bool, near: bool },
    /// A numeric label definition.
    Label(u32),
}

/// Splits a leading label definition `name:` off a statement.
fn split_label(stmt: &str) -> Option<(&str, &str)> {
    crate::a64::split_label(stmt)
}

/// Splits a statement into its mnemonic (or directive) and its operand
/// text.
fn split_mnemonic(stmt: &str) -> (&str, &str) {
    match stmt.find(|c: char| c.is_ascii() && is_space(c as u8)) {
        Some(i) => (&stmt[..i], trim(&stmt[i..])),
        None => (stmt, ""),
    }
}

/// A prefix statement: `lock`, or `rep` and its synonyms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prefix {
    Lock,
    Rep,
}

fn prefix(name: &str) -> Option<Prefix> {
    match name {
        "lock" => Some(Prefix::Lock),
        "rep" | "repe" | "repz" => Some(Prefix::Rep),
        _ => None,
    }
}

/// The string instruction `rep` takes: `movs` or `stos` of a size.
fn string_op(name: &str) -> Option<mc::Inst> {
    let (op, size): (fn(Size) -> mc::Inst, u8) = match name.strip_prefix("movs") {
        Some(rest) => (mc::rep_movs, *rest.as_bytes().first()?),
        None => (mc::rep_stos, *name.strip_prefix("stos")?.as_bytes().first()?),
    };
    if name.len() != 5 {
        return None;
    }
    let size = match size {
        b'b' => Size::B,
        b'w' => Size::W,
        b'l' | b'd' => Size::D,
        b'q' => Size::Q,
        _ => return None,
    };
    Some(op(size))
}

/// The data directives: their field size.
fn data_size(name: &str) -> Option<usize> {
    match name {
        ".byte" => Some(1),
        ".short" | ".value" | ".word" | ".2byte" => Some(2),
        ".long" | ".int" | ".4byte" => Some(4),
        ".quad" | ".8byte" => Some(8),
        _ => None,
    }
}

/// Appends the fields of a data directive of `size` bytes.
fn data(size: usize, operands: &str, out: &mut Vec<u8>) -> Result<(), Refusal> {
    for part in operands.split(',') {
        let mut c = Cursor::new(trim(part));
        let v = c.constant()?;
        if !c.at_end() {
            return Err(Refusal(op_reason::SYNTAX));
        }
        let bits = 8 * size as u32;
        let fits = bits == 64 || {
            let min = -(1i64 << (bits - 1));
            let max = (1i64 << bits) - 1;
            (min..=max).contains(&v)
        };
        if !fits {
            return Err(Refusal(op_reason::RANGE));
        }
        out.extend_from_slice(&v.to_le_bytes()[..size]);
    }
    Ok(())
}

/// Assembles an inline-assembly body (the template after operand
/// substitution) into `code` (cleared first). `scratch` is reused text
/// space.
///
/// # Errors
/// The `asm:*` refusals of the statement that is not accepted.
pub fn assemble(body: &str, code: &mut Vec<u8>, scratch: &mut String) -> Result<(), Refusal> {
    code.clear();
    clean(body, scratch)?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut pending: Option<Prefix> = None;
    for stmt in statements(scratch) {
        let mut stmt = stmt;
        while let Some((label, rest)) = split_label(stmt) {
            let Ok(n) = label.parse::<u32>() else { return Err(Refusal(other::LABEL)) };
            if pending.is_some() {
                return Err(Refusal(op_reason::FORM));
            }
            items.push(Item::Label(n));
            stmt = rest;
        }
        if stmt.is_empty() {
            continue;
        }
        let (mnemonic, mut rest) = split_mnemonic(stmt);
        let mut name = mnemonic.to_ascii_lowercase();
        if name.starts_with('.') {
            if pending.is_some() {
                return Err(Refusal(op_reason::FORM));
            }
            let Some(size) = data_size(&name) else { return Err(reason::directive(&name)) };
            let at = bytes.len();
            data(size, rest, &mut bytes)?;
            items.push(Item::Bytes { at, len: bytes.len() - at });
            continue;
        }
        // A prefix with its instruction on the same statement is part of
        // the instruction (`lock xadd`: `F0` after `66`, as LLVM writes
        // it); a prefix alone is a byte of its own before the next one.
        let mut joined = None;
        if let Some(p) = prefix(&name) {
            if rest.is_empty() {
                if pending.is_some() {
                    return Err(Refusal(op_reason::FORM));
                }
                pending = Some(p);
                continue;
            }
            joined = Some(p);
            let (m, r) = split_mnemonic(rest);
            name = m.to_ascii_lowercase();
            rest = r;
        }
        let separate = pending.take();
        let prefix = joined.or(separate);
        let inst = match prefix {
            Some(Prefix::Rep) => {
                if !rest.is_empty() {
                    return Err(Refusal(op_reason::FORM));
                }
                match name.as_str() {
                    "nop" => mc::pause(),
                    _ => string_op(&name).ok_or_else(|| reason::mnemonic(&name))?,
                }
            }
            Some(Prefix::Lock) | None => {
                let ops = operand::parse(rest)?;
                match encode::encode(&name, &ops)? {
                    Encoded::Inst(inst) => match prefix {
                        Some(_) if !inst.is_lockable() => return Err(Refusal(op_reason::FORM)),
                        Some(_) if separate.is_some() => {
                            bytes.push(0xf0);
                            inst
                        }
                        Some(_) => mc::lock(inst),
                        None => inst,
                    },
                    Encoded::Branch(cond, target) => {
                        if prefix.is_some() {
                            return Err(Refusal(op_reason::FORM));
                        }
                        let (n, forward) = match target {
                            Value { name: Some(Name::Local(n, f)), constant: 0 } => (n, f),
                            Value { name: Some(Name::Local(..)), .. } => {
                                return Err(Refusal(op_reason::EXPRESSION));
                            }
                            _ => return Err(Refusal(op_reason::SYMBOL)),
                        };
                        items.push(Item::Branch { cond, n, forward, near: false });
                        continue;
                    }
                }
            }
        };
        let at = bytes.len() - usize::from(separate == Some(Prefix::Lock));
        bytes.extend_from_slice(inst.bytes());
        items.push(Item::Bytes { at, len: bytes.len() - at });
    }
    if pending.is_some() {
        return Err(Refusal(op_reason::FORM));
    }
    lay_out(&mut items, &bytes, code)
}

/// The bytes of a branch.
fn branch_len(cond: Option<Cond>, near: bool) -> usize {
    match (cond, near) {
        (_, false) => 2,
        (None, true) => 5,
        (Some(_), true) => 6,
    }
}

/// The target offset of each branch, given the offsets of the items.
fn target(items: &[Item], offsets: &[usize], at: usize) -> Result<usize, Refusal> {
    let Item::Branch { n, forward, .. } = items[at] else { unreachable!("a branch") };
    let found = if forward {
        (at + 1..items.len()).find(|&i| matches!(items[i], Item::Label(m) if m == n))
    } else {
        (0..at).rev().find(|&i| matches!(items[i], Item::Label(m) if m == n))
    };
    found.map(|i| offsets[i]).ok_or(Refusal(op_reason::LABEL))
}

/// Chooses each branch's form as LLVM's relaxation does (short unless
/// the target is out of `rel8` reach, until nothing changes) and writes
/// the code.
fn lay_out(items: &mut [Item], bytes: &[u8], code: &mut Vec<u8>) -> Result<(), Refusal> {
    let mut offsets = vec![0usize; items.len() + 1];
    loop {
        let mut at = 0;
        for (i, item) in items.iter().enumerate() {
            offsets[i] = at;
            at += match *item {
                Item::Bytes { len, .. } => len,
                Item::Branch { cond, near, .. } => branch_len(cond, near),
                Item::Label(_) => 0,
            };
        }
        offsets[items.len()] = at;
        let mut changed = false;
        for i in 0..items.len() {
            if let Item::Branch { cond, near: false, .. } = items[i] {
                let end = offsets[i] + branch_len(cond, false);
                let disp = target(items, &offsets, i)? as i64 - end as i64;
                if i8::try_from(disp).is_err() {
                    if let Item::Branch { near, .. } = &mut items[i] {
                        *near = true;
                    }
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for i in 0..items.len() {
        match items[i] {
            Item::Bytes { at, len } => code.extend_from_slice(&bytes[at..at + len]),
            Item::Label(_) => {}
            Item::Branch { cond, near, .. } => {
                let end = offsets[i] + branch_len(cond, near);
                let disp = target(items, &offsets, i)? as i64 - end as i64;
                let inst = match (cond, near) {
                    (None, false) => mc::jmp_short(disp as i8),
                    (Some(c), false) => mc::jcc_short(c, disp as i8),
                    (None, true) => {
                        mc::jmp_near(i32::try_from(disp).map_err(|_| Refusal(op_reason::REACH))?)
                    }
                    (Some(c), true) => {
                        mc::jcc_near(c, i32::try_from(disp).map_err(|_| Refusal(op_reason::REACH))?)
                    }
                };
                code.extend_from_slice(inst.bytes());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(text: &str) -> Result<Vec<u8>, Refusal> {
        let mut code = Vec::new();
        assemble(text, &mut code, &mut String::new())?;
        Ok(code)
    }

    /// `spec/asm.md` §10.1, §10.4: statements, comments, prefixes, data,
    /// local labels, and what an inline body refuses.
    #[test]
    fn bodies() {
        assert_eq!(asm("cpuid").unwrap(), [0x0f, 0xa2]);
        assert_eq!(asm("nop # c ; nop\nnop // d\n nop /* e */ ; nop").unwrap(), [0x90; 4]);
        assert_eq!(asm("rep; nop").unwrap(), [0xf3, 0x90]);
        assert_eq!(asm("rep movsb").unwrap(), [0xf3, 0xa4]);
        assert_eq!(asm("lock xaddw %ax, (%rdi)").unwrap(), [0x66, 0xf0, 0x0f, 0xc1, 0x07]);
        assert_eq!(asm("lock; xaddw %ax, (%rdi)").unwrap(), [0xf0, 0x66, 0x0f, 0xc1, 0x07]);
        assert_eq!(asm(".byte 0x0f, 0x01, 0xd0").unwrap(), [0x0f, 0x01, 0xd0]);
        assert_eq!(asm(".word -1").unwrap(), [0xff, 0xff]);
        assert_eq!(asm("1: jmp 1b").unwrap(), [0xeb, 0xfe]);
        assert_eq!(asm("jne 1f\n1:").unwrap(), [0x75, 0x00]);
        let far = format!("jmp 1f\n{}1:", ".quad 0\n".repeat(16));
        assert_eq!(asm(&far).unwrap()[..5], [0xe9, 0x80, 0, 0, 0]);
        assert_eq!(asm("x: nop").err(), Some(Refusal(other::LABEL)));
        assert_eq!(asm(".p2align 4").err(), Some(reason::directive(".p2align")));
        assert_eq!(asm("jmp 2f").err(), Some(Refusal(op_reason::LABEL)));
        assert_eq!(asm("jmp foo").err(), Some(Refusal(op_reason::SYMBOL)));
        assert_eq!(asm("frob %eax").err(), Some(reason::mnemonic("frob")));
        assert_eq!(asm("lock").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(asm("lock movl %eax, (%rdi)").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(asm(".byte 256").err(), Some(Refusal(op_reason::RANGE)));
        assert_eq!(asm("rep cmpsb").err(), Some(reason::mnemonic("cmpsb")));
    }

    #[test]
    fn printing() {
        let p = |a, m| {
            let mut s = String::new();
            print_operand(a, m, &mut s).map(|()| s)
        };
        assert_eq!(p(Arg::Gp(0, 4), None).unwrap(), "%eax");
        assert_eq!(p(Arg::Gp(6, 1), None).unwrap(), "%sil");
        assert_eq!(p(Arg::Gp(9, 8), Some(b'k')).unwrap(), "%r9d");
        assert_eq!(p(Arg::Gp(3, 4), Some(b'q')).unwrap(), "%rbx");
        assert_eq!(p(Arg::Gp(3, 8), Some(b'b')).unwrap(), "%bl");
        assert_eq!(p(Arg::Gp(7, 8), Some(b'a')).unwrap(), "(%rdi)");
        assert_eq!(p(Arg::Imm(5), None).unwrap(), "$5");
        assert_eq!(p(Arg::Imm(5), Some(b'c')).unwrap(), "5");
        assert_eq!(p(Arg::Imm(5), Some(b'n')).unwrap(), "-5");
        assert_eq!(p(Arg::Mem(1), None).unwrap(), "(%rcx)");
        assert_eq!(p(Arg::Mem(1), Some(b'H')).unwrap(), "8(%rcx)");
        assert_eq!(p(Arg::Xmm(3), None).unwrap(), "%xmm3");
        assert_eq!(p(Arg::Gp(0, 4), Some(b'h')).err(), Some(reason::modifier("h")));
        assert!(p(Arg::Mem(0), Some(b'c')).is_err());
        assert!(p(Arg::Xmm(0), Some(b'k')).is_err());
    }
}
