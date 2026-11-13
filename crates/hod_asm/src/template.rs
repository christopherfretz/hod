//! Operand substitution in an inline-assembly template (`spec/asm.md`
//! §4), as LLVM's `AsmPrinter` (`EmitInlineAsmStr`, GNU dialect)
//! writes the text its assembler then reads:
//!
//! - text is copied as it is (`{`, `|`, `}` included);
//! - `$$` is `$`; `$(` opens a group of variants, `$|` starts the next
//!   variant, `$)` closes the group: only variant 0 (the dialect of
//!   `aarch64-*-linux-*`) is written; `$|` outside a group is `|`;
//! - `$N` and `${N}` are operand `N`, `${N:m}` is operand `N` with the
//!   one-character modifier `m`; what an operand prints as is the
//!   caller's (for AArch64, [`crate::a64::print_operand`]);
//! - `${:uid}` is a number unique to this call, `${:comment}` is the
//!   comment string (`//`), `${:private}` is the private prefix (`.L`).
//!
//! An operand reference that does not parse or is out of range is
//! [`other::TEMPLATE`], as it is a fatal error to `llc`. An operand in a
//! variant that is not written is still checked (LLVM checks its number
//! before the variant).

use crate::reason::{Refusal, other};

/// One operand reference: the operand's number and its modifier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OperandRef {
    /// The operand number (outputs first, then inputs, then labels).
    pub index: u32,
    /// The modifier character of `${N:m}`.
    pub modifier: Option<u8>,
}

/// Writes `template` with its references substituted into `out`
/// (appended). `operands` is the number of operands; `print` writes one
/// operand; `uid` is what `${:uid}` prints.
///
/// # Errors
/// [`other::TEMPLATE`]; what `print` returns.
pub fn substitute(
    template: &str,
    operands: u32,
    uid: u32,
    out: &mut String,
    mut print: impl FnMut(OperandRef, &mut String) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    let t = template.as_bytes();
    let bad = || Err(Refusal(other::TEMPLATE));
    // The variant being read: `None` outside a group.
    let mut variant: Option<u32> = None;
    let mut i = 0;
    while i < t.len() {
        let writing = variant.is_none_or(|v| v == 0);
        if t[i] != b'$' {
            let end = t[i + 1..].iter().position(|&b| b == b'$').map_or(t.len(), |p| i + 1 + p);
            if writing {
                out.push_str(&template[i..end]);
            }
            i = end;
            continue;
        }
        i += 1;
        match t.get(i) {
            Some(b'$') => {
                if writing {
                    out.push('$');
                }
                i += 1;
                continue;
            }
            Some(b'(') => {
                if variant.is_some() {
                    return bad();
                }
                variant = Some(0);
                i += 1;
                continue;
            }
            Some(b'|') => {
                match &mut variant {
                    None => out.push('|'),
                    Some(v) => *v += 1,
                }
                i += 1;
                continue;
            }
            Some(b')') => {
                variant = None;
                i += 1;
                continue;
            }
            _ => {}
        }
        let braces = t.get(i) == Some(&b'{');
        if braces {
            i += 1;
        }
        if braces && t.get(i) == Some(&b':') {
            let Some(close) = t[i..].iter().position(|&b| b == b'}') else { return bad() };
            let special = &template[i + 1..i + close];
            if writing {
                match special {
                    "uid" => out.push_str(&uid.to_string()),
                    "comment" => out.push_str("//"),
                    "private" => out.push_str(".L"),
                    _ => return bad(),
                }
            }
            i += close + 1;
            continue;
        }
        let start = i;
        while i < t.len() && t[i].is_ascii_digit() {
            i += 1;
        }
        let Ok(index) = template[start..i].parse::<u32>() else { return bad() };
        if index >= operands {
            return bad();
        }
        let mut modifier = None;
        if braces {
            if t.get(i) == Some(&b':') {
                let Some(&m) = t.get(i + 1) else { return bad() };
                modifier = Some(m);
                i += 2;
            }
            if t.get(i) != Some(&b'}') {
                return bad();
            }
            i += 1;
        }
        if writing {
            print(OperandRef { index, modifier }, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(template: &str) -> Result<String, Refusal> {
        let mut out = String::new();
        substitute(template, 3, 7, &mut out, |r, out| {
            out.push('<');
            out.push_str(&r.index.to_string());
            out.push(r.modifier.map_or(' ', char::from));
            out.push('>');
            Ok(())
        })?;
        Ok(out)
    }

    /// `spec/asm.md` §4: references, escapes, variants, specials.
    #[test]
    fn substitution() {
        assert_eq!(sub("add $0, $1, #1").unwrap(), "add <0 >, <1 >, #1");
        assert_eq!(sub("${0:w} ${2}").unwrap(), "<0w> <2 >");
        assert_eq!(sub("$$5 a|b {c}").unwrap(), "$5 a|b {c}");
        assert_eq!(sub("$(x$|y$|z$) $|").unwrap(), "x |");
        assert_eq!(sub("${:uid} ${:comment} ${:private}").unwrap(), "7 // .L");
        assert_eq!(sub("$10").err(), Some(Refusal(other::TEMPLATE)));
        for bad in ["$x", "${0", "${0:}", "${:foo}", "$($(", "$3", "${0:w"] {
            assert!(sub(bad).is_err(), "{bad}");
        }
        // A reference in a variant not written is still checked.
        assert!(sub("$(a$|$9$)").is_err());
    }
}
