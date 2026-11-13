//! Stable refusal names (`spec/asm.md` "Refusals").
//!
//! A refusal is a `&'static str`, as every construct name of hod is. The
//! names that carry a part of the input (`asm:mnemonic/<name>`,
//! `asm:directive/<name>`, `asm:constraint/<code>`) are interned once
//! per distinct name for the life of the process, so a refusal never
//! allocates twice for one name; past [`MAX_NAMES`] distinct names, or
//! for a part that is not a plain word, the name ends in `/other`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Why a statement, a template or a constraint is not assembled: a
/// stable construct name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Refusal(pub &'static str);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Refusal {}

/// The most distinct interned names.
pub const MAX_NAMES: usize = 4096;
/// The longest part kept in a name.
const MAX_PART: usize = 40;

/// `asm:operand/<what>` names: an operand of a known instruction that is
/// not one of the forms covered.
pub mod operand {
    /// An operand that does not parse: an unbalanced bracket, a stray
    /// character.
    pub const SYNTAX: &str = "asm:operand/syntax";
    /// An operand count or kind the instruction does not take.
    pub const FORM: &str = "asm:operand/form";
    /// An immediate out of the instruction's range, or misaligned.
    pub const RANGE: &str = "asm:operand/range";
    /// A register of the wrong size or class for the position.
    pub const REGISTER: &str = "asm:operand/register";
    /// A system register this crate does not encode.
    pub const SYSREG: &str = "asm:operand/sysreg";
    /// A barrier option or other named operand not encoded.
    pub const OPTION: &str = "asm:operand/option";
    /// A vector arrangement or element (SIMD forms are not covered).
    pub const VECTOR: &str = "asm:operand/vector";
    /// A floating-point immediate.
    pub const FP_IMMEDIATE: &str = "asm:operand/fp-immediate";
    /// A relocation specifier (`:lo12:`, `:got:`, ...).
    pub const SPECIFIER: &str = "asm:operand/specifier";
    /// An expression that is not a constant or one symbol plus a
    /// constant, a division by zero, an overflow.
    pub const EXPRESSION: &str = "asm:operand/expression";
    /// A reference to a symbol or label where only a constant is taken,
    /// or a symbol where only a local label is taken.
    pub const SYMBOL: &str = "asm:operand/symbol";
    /// A numeric label reference with no definition in that direction.
    pub const LABEL: &str = "asm:operand/label";
    /// A branch whose target is out of reach.
    pub const REACH: &str = "asm:operand/reach";
    /// A literal pool load (`ldr x0, =value`).
    pub const LITERAL_POOL: &str = "asm:operand/literal-pool";
}

/// `asm:...` names that are not about one operand.
pub mod other {
    /// A label definition not allowed where it is (a named label in
    /// inline assembly, a label defined twice).
    pub const LABEL: &str = "asm:label";
    /// An unterminated block comment or string.
    pub const LEXICAL: &str = "asm:lexical";
    /// An instruction that needs a target feature the function or the
    /// module does not have.
    pub const FEATURE: &str = "asm:feature";
    /// A template operand reference that does not parse, names an
    /// operand that does not exist, or has a modifier not covered.
    pub const TEMPLATE: &str = "asm:template";
    /// A template that uses the Intel dialect.
    pub const DIALECT: &str = "asm:dialect";
    /// A statement in a section that is not the one the text started in
    /// (inline assembly), or a section directive's operands not covered.
    pub const SECTION: &str = "asm:section";
    /// A symbol directive whose effect is not reproduced.
    pub const SYMBOL: &str = "asm:symbol";
}

fn names() -> &'static Mutex<HashSet<&'static str>> {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Whether `part` is kept in a name: a short word of lower-case letters,
/// digits and `._-`.
fn plain(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PART
        && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// The interned name `<prefix><part>` (part lower-cased), or
/// `<prefix>other`.
pub fn named(prefix: &'static str, part: &str) -> &'static str {
    let lower = part.to_ascii_lowercase();
    let mut set = names().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let full = if plain(&lower) { format!("{prefix}{lower}") } else { format!("{prefix}other") };
    if let Some(&found) = set.get(full.as_str()) {
        return found;
    }
    if set.len() >= MAX_NAMES {
        let other = format!("{prefix}other");
        if let Some(&found) = set.get(other.as_str()) {
            return found;
        }
        let leaked: &'static str = Box::leak(other.into_boxed_str());
        set.insert(leaked);
        return leaked;
    }
    let leaked: &'static str = Box::leak(full.into_boxed_str());
    set.insert(leaked);
    leaked
}

/// `asm:mnemonic/<name>`: an instruction this crate does not encode.
pub fn mnemonic(name: &str) -> Refusal {
    Refusal(named("asm:mnemonic/", name))
}

/// `asm:directive/<name>`: a directive this crate does not read (the
/// name without its dot).
pub fn directive(name: &str) -> Refusal {
    Refusal(named("asm:directive/", name.trim_start_matches('.')))
}

/// `asm:constraint/<code>`: an inline-assembly constraint code not
/// covered.
pub fn constraint(code: &str) -> Refusal {
    Refusal(named("asm:constraint/", code))
}

/// `asm:clobber/<name>`: a clobber not covered.
pub fn clobber(name: &str) -> Refusal {
    Refusal(named("asm:clobber/", name))
}

/// `asm:modifier/<m>`: a template operand modifier not covered for the
/// operand it is applied to.
pub fn modifier(m: &str) -> Refusal {
    Refusal(named("asm:modifier/", m))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/asm.md` "Refusals": one name per distinct part, lower case,
    /// `other` for what is not a plain word.
    #[test]
    fn interned_names() {
        let a = mnemonic("FOO");
        let b = mnemonic("foo");
        assert_eq!(a.0, "asm:mnemonic/foo");
        assert!(std::ptr::eq(a.0, b.0));
        assert_eq!(directive(".symver").0, "asm:directive/symver");
        assert_eq!(mnemonic("a b").0, "asm:mnemonic/other");
        assert_eq!(mnemonic(&"x".repeat(100)).0, "asm:mnemonic/other");
        assert_eq!(constraint("Ump").0, "asm:constraint/ump");
    }
}
