//! Integer expressions as LLVM's GNU-syntax parser reads them
//! (`spec/asm.md` §3): literals (decimal, `0x`, `0b`, octal with a
//! leading `0`, `'c'`), unary `-` `~` `+` `!`, and the binary operators
//! with GNU precedence (`*` `/` `%` `<<` `>>` bind tightest, then `|`
//! `&` `^`, then `+` `-`, then `&&`, then `||`); `>>` is logical, as on
//! AArch64. A value may name one symbol or local label: `sym`,
//! `sym + c`, `sym - c`, `c + sym`, `1f`, `2b`. Everything else
//! (comparisons, a difference of symbols, `.`) is refused.

use crate::reason::{Refusal, operand};

/// A symbol or label an expression names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name<'a> {
    /// A symbol or named label.
    Sym(&'a str),
    /// A local numeric label: `n` and whether the reference is forward
    /// (`nf`) or backward (`nb`).
    Local(u32, bool),
}

/// The value of an expression: a constant, or a name plus a constant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Value<'a> {
    /// The name, if any.
    pub name: Option<Name<'a>>,
    /// The constant (the addend when there is a name).
    pub constant: i64,
}

impl Value<'_> {
    /// A constant.
    pub const fn constant(value: i64) -> Self {
        Value { name: None, constant: value }
    }
}

/// A cursor over operand text.
#[derive(Clone, Copy, Debug)]
pub struct Cursor<'a> {
    /// The text.
    pub text: &'a str,
    /// The byte offset of the next character.
    pub at: usize,
}

fn err<T>(what: &'static str) -> Result<T, Refusal> {
    Err(Refusal(what))
}

/// Whether `b` may start a symbol name.
pub fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || matches!(b, b'_' | b'.' | b'$')
}

/// Whether `b` may continue a symbol name.
pub fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$')
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `text`.
    pub fn new(text: &'a str) -> Self {
        Cursor { text, at: 0 }
    }

    /// Skips white space.
    pub fn skip_ws(&mut self) {
        let t = self.text.as_bytes();
        while self.at < t.len() && crate::text::is_space(t[self.at]) {
            self.at += 1;
        }
    }

    /// The next character after white space, not consumed.
    pub fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.text.as_bytes().get(self.at).copied()
    }

    /// Consumes `c` (after white space) if it is next.
    pub fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Consumes the two characters `s` if they are next.
    pub fn eat2(&mut self, s: &[u8; 2]) -> bool {
        self.skip_ws();
        if self.text.as_bytes()[self.at..].starts_with(s) {
            self.at += 2;
            true
        } else {
            false
        }
    }

    /// Whether only white space is left.
    pub fn at_end(&mut self) -> bool {
        self.peek().is_none()
    }

    /// The rest of the text.
    pub fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    /// A name (`[A-Za-z_.$][A-Za-z0-9_.$]*`), or `None` (nothing
    /// consumed).
    pub fn name(&mut self) -> Option<&'a str> {
        self.skip_ws();
        let t = self.text.as_bytes();
        if !t.get(self.at).copied().is_some_and(is_name_start) {
            return None;
        }
        let start = self.at;
        while self.at < t.len() && is_name_char(t[self.at]) {
            self.at += 1;
        }
        Some(&self.text[start..self.at])
    }

    /// An expression (module documentation).
    ///
    /// # Errors
    /// [`operand::EXPRESSION`], [`operand::SYNTAX`].
    pub fn expr(&mut self) -> Result<Value<'a>, Refusal> {
        self.binary(1)
    }

    /// A constant expression.
    ///
    /// # Errors
    /// As [`Cursor::expr`]; [`operand::SYMBOL`] for a value that names a
    /// symbol or label.
    pub fn constant(&mut self) -> Result<i64, Refusal> {
        let v = self.expr()?;
        if v.name.is_some() {
            return err(operand::SYMBOL);
        }
        Ok(v.constant)
    }

    fn binary(&mut self, min: u8) -> Result<Value<'a>, Refusal> {
        let mut lhs = self.unary()?;
        loop {
            let Some((op, prec, len)) = self.binop() else { return Ok(lhs) };
            if prec < min {
                return Ok(lhs);
            }
            self.at += len;
            let rhs = self.binary(prec + 1)?;
            lhs = apply(op, lhs, rhs)?;
        }
    }

    /// The binary operator next, its precedence and length.
    fn binop(&mut self) -> Option<(u8, u8, usize)> {
        let rest = {
            self.skip_ws();
            &self.text.as_bytes()[self.at..]
        };
        let two = |s: &[u8]| rest.starts_with(s);
        Some(if two(b"||") {
            (b'O', 1, 2)
        } else if two(b"&&") {
            (b'A', 2, 2)
        } else if two(b"<<") {
            (b'<', 6, 2)
        } else if two(b">>") {
            (b'>', 6, 2)
        } else {
            match rest.first()? {
                b'+' => (b'+', 4, 1),
                b'-' => (b'-', 4, 1),
                b'|' => (b'|', 5, 1),
                b'&' => (b'&', 5, 1),
                b'^' => (b'^', 5, 1),
                b'*' => (b'*', 6, 1),
                b'/' => (b'/', 6, 1),
                b'%' => (b'%', 6, 1),
                _ => return None,
            }
        })
    }

    fn unary(&mut self) -> Result<Value<'a>, Refusal> {
        match self.peek() {
            Some(b'-') => {
                self.at += 1;
                let v = self.unary()?;
                konst(v).map(|c| Value::constant(c.wrapping_neg()))
            }
            Some(b'~') => {
                self.at += 1;
                let v = self.unary()?;
                konst(v).map(|c| Value::constant(!c))
            }
            Some(b'!') => {
                self.at += 1;
                let v = self.unary()?;
                konst(v).map(|c| Value::constant(i64::from(c == 0)))
            }
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            Some(b'(') => {
                self.at += 1;
                let v = self.binary(1)?;
                if !self.eat(b')') {
                    return err(operand::SYNTAX);
                }
                Ok(v)
            }
            Some(b'\'') => {
                let t = self.text.as_bytes();
                let (c, len) = match (t.get(self.at + 1), t.get(self.at + 2), t.get(self.at + 3)) {
                    (Some(b'\\'), Some(&e), Some(b'\'')) => {
                        let c = match e {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'r' => b'\r',
                            b'0' => 0,
                            b'\\' | b'\'' | b'"' => e,
                            _ => return err(operand::SYNTAX),
                        };
                        (c, 4)
                    }
                    (Some(&c), Some(b'\''), _) if c != b'\\' => (c, 3),
                    _ => return err(operand::SYNTAX),
                };
                self.at += len;
                Ok(Value::constant(i64::from(c)))
            }
            Some(b) if b.is_ascii_digit() => self.number(),
            Some(b) if is_name_start(b) => {
                let name = self.name().unwrap_or_default();
                if name == "." {
                    return err(operand::EXPRESSION);
                }
                Ok(Value { name: Some(Name::Sym(name)), constant: 0 })
            }
            _ => err(operand::SYNTAX),
        }
    }

    /// A literal or a local label reference.
    fn number(&mut self) -> Result<Value<'a>, Refusal> {
        let t = self.text.as_bytes();
        let start = self.at;
        let mut end = start;
        while end < t.len() && t[end].is_ascii_alphanumeric() {
            end += 1;
        }
        let word = &self.text[start..end];
        self.at = end;
        let digits = |s: &str, radix: u32| -> Result<i64, Refusal> {
            if s.is_empty() {
                return err(operand::SYNTAX);
            }
            // LLVM keeps the low 64 bits of a literal that fits in 64
            // bits unsigned.
            u64::from_str_radix(s, radix)
                .map(|v| v as i64)
                .map_err(|_| Refusal(operand::EXPRESSION))
        };
        let lower = word.to_ascii_lowercase();
        // A local label reference: digits then `b` or `f`.
        if let Some(n) = lower.strip_suffix('b').or_else(|| lower.strip_suffix('f')) {
            let binary = lower.starts_with("0b") && lower.len() > 2;
            if !binary && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                let n: u32 = n.parse().map_err(|_| Refusal(operand::EXPRESSION))?;
                let forward = lower.ends_with('f');
                return Ok(Value { name: Some(Name::Local(n, forward)), constant: 0 });
            }
        }
        let value = if let Some(hex) = lower.strip_prefix("0x") {
            digits(hex, 16)?
        } else if let Some(bin) = lower.strip_prefix("0b") {
            digits(bin, 2)?
        } else if lower.len() > 1 && lower.starts_with('0') {
            digits(&lower[1..], 8)?
        } else if lower.bytes().all(|b| b.is_ascii_digit()) {
            digits(&lower, 10)?
        } else {
            return err(operand::SYNTAX);
        };
        Ok(Value::constant(value))
    }
}

fn konst(v: Value<'_>) -> Result<i64, Refusal> {
    if v.name.is_some() { err(operand::EXPRESSION) } else { Ok(v.constant) }
}

fn apply<'a>(op: u8, lhs: Value<'a>, rhs: Value<'a>) -> Result<Value<'a>, Refusal> {
    // A name survives `+` and `-` of a constant only.
    match (op, lhs.name, rhs.name) {
        (b'+', Some(_), None) => {
            return Ok(Value { name: lhs.name, constant: lhs.constant.wrapping_add(rhs.constant) });
        }
        (b'+', None, Some(_)) => {
            return Ok(Value { name: rhs.name, constant: lhs.constant.wrapping_add(rhs.constant) });
        }
        (b'-', Some(_), None) => {
            return Ok(Value { name: lhs.name, constant: lhs.constant.wrapping_sub(rhs.constant) });
        }
        (_, None, None) => {}
        _ => return err(operand::EXPRESSION),
    }
    let (a, b) = (lhs.constant, rhs.constant);
    let v = match op {
        b'+' => a.wrapping_add(b),
        b'-' => a.wrapping_sub(b),
        b'*' => a.wrapping_mul(b),
        b'/' => a.checked_div(b).ok_or(Refusal(operand::EXPRESSION))?,
        b'%' => a.checked_rem(b).ok_or(Refusal(operand::EXPRESSION))?,
        // LLVM shifts by the amount modulo nothing: an amount of 64 or
        // more is refused rather than guessed.
        b'<' if (0..64).contains(&b) => a.wrapping_shl(b as u32),
        b'>' if (0..64).contains(&b) => ((a as u64) >> b) as i64,
        b'|' => a | b,
        b'&' => a & b,
        b'^' => a ^ b,
        b'A' => i64::from(a != 0 && b != 0),
        b'O' => i64::from(a != 0 || b != 0),
        _ => return err(operand::EXPRESSION),
    };
    Ok(Value::constant(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str) -> Result<Value<'_>, Refusal> {
        let mut c = Cursor::new(s);
        let v = c.expr()?;
        if c.at_end() { Ok(v) } else { Err(Refusal(operand::SYNTAX)) }
    }

    fn k(s: &str) -> i64 {
        eval(s).unwrap().constant
    }

    /// `spec/asm.md` §3: literals and GNU precedence.
    #[test]
    fn constants() {
        assert_eq!(k("42"), 42);
        assert_eq!(k("0x2A"), 42);
        assert_eq!(k("0b101010"), 42);
        assert_eq!(k("052"), 42);
        assert_eq!(k("'*'"), 42);
        assert_eq!(k("-1"), -1);
        assert_eq!(k("1 + 2 * 3"), 7);
        // `|` binds tighter than `+` in GNU syntax.
        assert_eq!(k("1 + 2 | 4"), 7);
        assert_eq!(k("8 | 1 - 1"), 8);
        assert_eq!(k("(1 << 4) - 1"), 15);
        assert_eq!(k("-8 >> 60"), 15);
        assert_eq!(k("~0"), -1);
        assert_eq!(k("0xffffffffffffffff"), -1);
        assert!(eval("1 / 0").is_err());
        assert!(eval("1 << 64").is_err());
        assert!(eval("0x1ffffffffffffffff").is_err());
    }

    /// Names: one symbol plus a constant; local label references.
    #[test]
    fn names() {
        assert_eq!(eval("foo").unwrap().name, Some(Name::Sym("foo")));
        assert_eq!(eval("foo + 8").unwrap(), Value { name: Some(Name::Sym("foo")), constant: 8 });
        assert_eq!(eval("4 + foo - 1").unwrap().constant, 3);
        assert_eq!(eval("1f").unwrap().name, Some(Name::Local(1, true)));
        assert_eq!(eval("12b").unwrap().name, Some(Name::Local(12, false)));
        assert_eq!(eval("0b").unwrap().name, Some(Name::Local(0, false)));
        assert_eq!(k("0b1"), 1);
        assert!(eval("foo - bar").is_err());
        assert!(eval("2 * foo").is_err());
        assert!(eval(".").is_err());
    }
}
