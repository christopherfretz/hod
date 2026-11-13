//! Statements of AArch64 assembly text (`spec/asm.md` §2), as LLVM's
//! assembly lexer reads them for `aarch64-*-linux-*`:
//!
//! - statements are separated by a newline or `;`;
//! - `//` starts a comment to the end of the line anywhere outside a
//!   string, `#` does at the start of a statement (only white space
//!   before it), and `/* ... */` is a comment anywhere (it may span
//!   lines; it does not end the statement);
//! - a string (`"..."`, with backslash escapes) and a character literal
//!   (`'c'`) are kept whole: a separator or comment marker inside them is
//!   text.
//!
//! [`clean`] writes the text with its comments replaced by one space
//! each; [`statements`] then splits the result.

use crate::reason::{Refusal, other};

/// White space for the assembler.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c')
}

/// `text` without its comments (each replaced by one space, a line
/// comment's newline kept), into `out` (cleared first).
///
/// # Errors
/// [`other::LEXICAL`] for an unterminated block comment, string or
/// character literal.
pub fn clean(text: &str, out: &mut String) -> Result<(), Refusal> {
    out.clear();
    let t = text.as_bytes();
    let mut i = 0;
    // Only white space since the last separator.
    let mut at_start = true;
    let mut from = 0;
    while i < t.len() {
        let b = t[i];
        match b {
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
                at_start = false;
            }
            b'\'' => {
                // `'c'` or `'\c'`.
                let len = if t.get(i + 1) == Some(&b'\\') { 4 } else { 3 };
                if t.get(i + len - 1) != Some(&b'\'') {
                    return Err(Refusal(other::LEXICAL));
                }
                i += len;
                at_start = false;
            }
            b'/' if t.get(i + 1) == Some(&b'/') => {
                out.push_str(&text[from..i]);
                out.push(' ');
                while i < t.len() && t[i] != b'\n' {
                    i += 1;
                }
                from = i;
            }
            b'#' if at_start => {
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
            b'\n' | b';' => {
                at_start = true;
                i += 1;
            }
            _ => {
                if !is_space(b) {
                    at_start = false;
                }
                i += 1;
            }
        }
    }
    out.push_str(&text[from..]);
    Ok(())
}

/// The statements of `cleaned` (the output of [`clean`]), trimmed, empty
/// ones included, with separators inside strings and character literals
/// kept as text.
pub fn statements(cleaned: &str) -> Statements<'_> {
    Statements { text: cleaned, at: 0 }
}

/// The iterator of [`statements`].
#[derive(Debug)]
pub struct Statements<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Iterator for Statements<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let t = self.text.as_bytes();
        if self.at > t.len() {
            return None;
        }
        let start = self.at;
        let mut i = start;
        while i < t.len() {
            match t[i] {
                b'"' => {
                    i += 1;
                    while i < t.len() && t[i] != b'"' {
                        i += if t[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                b'\'' => i += if t.get(i + 1) == Some(&b'\\') { 4 } else { 3 },
                b'\n' | b';' => break,
                _ => i += 1,
            }
        }
        let end = i.min(t.len());
        self.at = end + 1;
        Some(trim(&self.text[start..end]))
    }
}

/// `s` without leading and trailing assembler white space.
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<String> {
        let mut out = String::new();
        clean(text, &mut out).unwrap();
        statements(&out).map(str::to_owned).collect()
    }

    /// `spec/asm.md` §2: separators and comments.
    #[test]
    fn statements_and_comments() {
        assert_eq!(split("nop; nop\n nop "), ["nop", "nop", "nop"]);
        assert_eq!(split("nop // x; y\nret"), ["nop", "ret"]);
        assert_eq!(split("# c ; d\nnop"), ["", "nop"]);
        assert_eq!(split("mov x0, #1 # no comment"), ["mov x0, #1 # no comment"]);
        assert_eq!(split("a /* x\n y */ b"), ["a   b"]);
        assert_eq!(split(".ascii \"a;b//c\"; nop"), [".ascii \"a;b//c\"", "nop"]);
        assert_eq!(split("mov w0, ';'"), ["mov w0, ';'"]);
        let mut out = String::new();
        assert!(clean("/* open", &mut out).is_err());
        assert!(clean("\"open", &mut out).is_err());
    }
}
