//! AArch64 GNU-syntax assembly (`spec/asm.md` §5): operand printing for
//! inline-assembly templates ([`print_operand`]), and the assembler of
//! an inline-assembly body ([`assemble`]): statements, numeric local
//! labels, and instructions ([`encode`]) whose words come from
//! `hod_mc_a64`'s checked encoders.
//!
//! An inline body defines no symbol: a named label, a directive other
//! than the alignment directives (§17, [`assemble_at`]) or a reference to
//! a numeric label the body does not define is refused (`asm:label`,
//! `asm:directive/<name>`, `asm:operand/label`). A
//! reference to a symbol is returned as a [`Ref`], for the caller to make
//! a relocation of (or to resolve, for an `asm goto` target).

pub mod encode;
pub mod neon;
pub mod operand;
pub mod prefetch;
pub mod sysreg;

use hod_mc_a64::Fixup;

use crate::expr::{Name, Value, is_name_char, is_name_start};
use crate::reason::{self, Refusal, operand as op_reason, other};

/// The architecture features an instruction may need, from the
/// function's (or the module's) `target-features`.
// One field per `target-features` name, as written in the attribute.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Features {
    /// `+lse`: the large-system-extension atomics (`cas`, `swp`, `ld<op>`).
    pub lse: bool,
    /// `+crc`: the `crc32` instructions.
    pub crc: bool,
    /// `+fp-armv8`: the scalar floating-point instructions.
    pub fp: bool,
    /// `+rand`: the `rndr` and `rndrrs` system registers.
    pub rand: bool,
    /// `+neon`: the Advanced SIMD instructions, vector and scalar
    /// (§20).
    pub neon: bool,
    /// `+aes`: `pmull` and `pmull2` of 64-bit elements (§20).
    pub aes: bool,
    /// `+fullfp16`: the half-precision arithmetic, vector forms on `4h`
    /// and `8h` among them (§20).
    pub fp16: bool,
}

impl Features {
    /// The features of `aarch64-*-linux-*` with no attribute: the base
    /// architecture with floating point and Advanced SIMD.
    pub const BASE: Features = Features {
        lse: false,
        crc: false,
        fp: true,
        rand: false,
        neon: true,
        aes: false,
        fp16: false,
    };

    /// Every feature hod reads.
    pub const ALL: Features =
        Features { lse: true, crc: true, fp: true, rand: true, neon: true, aes: true, fp16: true };

    /// The features a `target-features` attribute value
    /// (`+lse,+crc,-fp-armv8,+v8.1a,...`) gives over [`Features::BASE`]:
    /// an architecture version from `v8.1a` on has `lse` and `crc`.
    ///
    /// The items apply in order, each with what LLVM's feature table
    /// implies (§20, measured with `llc` 19 and 22): turning on `neon`,
    /// `fullfp16`, `fp16fml` or `sve` turns on `fp-armv8`; `aes` and
    /// `crypto` turn on `neon` too; `fp16fml` and `sve` turn on
    /// `fullfp16`. Turning a feature off turns off those that need it:
    /// `-fp-armv8` clears `neon`, `aes` and `fullfp16`, `-neon` clears
    /// `aes`.
    pub fn from_attribute(value: &str) -> Features {
        let mut f = Features::BASE;
        for item in value.split(',') {
            let item = item.trim();
            let (on, name) = match item.as_bytes().first() {
                Some(b'+') => (true, &item[1..]),
                Some(b'-') => (false, &item[1..]),
                _ => continue,
            };
            match (name, on) {
                ("fp-armv8", true) => f.fp = true,
                ("fp-armv8", false) => {
                    f.fp = false;
                    f.neon = false;
                    f.aes = false;
                    f.fp16 = false;
                }
                ("neon", true) => (f.neon, f.fp) = (true, true),
                ("neon", false) => (f.neon, f.aes) = (false, false),
                ("aes" | "crypto", true) => (f.aes, f.neon, f.fp) = (true, true, true),
                ("aes", false) => f.aes = false,
                ("fullfp16" | "fp16fml" | "sve", true) => (f.fp16, f.fp) = (true, true),
                ("fullfp16", false) => f.fp16 = false,
                _ => {}
            }
            match name {
                "lse" => f.lse = on,
                "crc" => f.crc = on,
                "rand" => f.rand = on,
                _ => {
                    if on && is_v81_or_later(name) {
                        f.lse = true;
                        f.crc = true;
                    }
                }
            }
        }
        f
    }
}

/// `v8.Na` for N ≥ 1, or `v9a`/`v9.Na`.
fn is_v81_or_later(name: &str) -> bool {
    let Some(v) = name.strip_prefix('v').and_then(|v| v.strip_suffix('a')) else { return false };
    match v.split_once('.') {
        None => v == "9",
        Some(("8", minor)) => minor.parse::<u32>().is_ok_and(|m| m >= 1),
        Some(("9", minor)) => minor.parse::<u32>().is_ok(),
        Some(_) => false,
    }
}

/// The kind of a PC-relative field that refers to a label or symbol.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RefKind {
    /// `b`: `R_AARCH64_JUMP26`.
    Jump26,
    /// `bl`: `R_AARCH64_CALL26`.
    Call26,
    /// `b.cond`, `cbz`, `cbnz`: `R_AARCH64_CONDBR19`.
    Branch19,
    /// `tbz`, `tbnz`: `R_AARCH64_TSTBR14`.
    Test14,
    /// `ldr` (literal): `R_AARCH64_LD_PREL_LO19`.
    Literal19,
    /// `adr`: `R_AARCH64_ADR_PREL_LO21`.
    Adr21,
}

impl RefKind {
    /// The instruction field.
    pub const fn fixup(self) -> Fixup {
        match self {
            RefKind::Jump26 | RefKind::Call26 => Fixup::Jump26,
            RefKind::Branch19 => Fixup::Branch19,
            RefKind::Test14 => Fixup::Test14,
            RefKind::Literal19 => Fixup::Literal19,
            RefKind::Adr21 => Fixup::Adr21,
        }
    }
}

/// A reference from assembled code to a symbol.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ref {
    /// The byte offset of the instruction in the code.
    pub offset: u32,
    /// The field.
    pub kind: RefKind,
    /// The symbol's name.
    pub name: String,
    /// The addend.
    pub addend: i64,
}

/// Assembled code: instruction words, and the symbol references left
/// for the caller.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Code {
    /// The instruction words, in order.
    pub words: Vec<u32>,
    /// The references to symbols (fields zero in `words`).
    pub refs: Vec<Ref>,
    /// The largest alignment an alignment directive of the body asked
    /// for, in bytes (0 when there is none, §17): the code must start at
    /// an offset of its section that is `start` modulo it.
    pub align: u32,
}

impl Code {
    /// The code as little-endian bytes, appended to `out`.
    pub fn bytes_into(&self, out: &mut Vec<u8>) {
        for w in &self.words {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
}

/// One operand of an inline-assembly call as the template prints it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arg<'a> {
    /// A general register `x0`..`x30`.
    Gp(u8),
    /// An FP/SIMD register `v0`..`v31`, holding a value of this many
    /// bytes (2, 4, 8 or 16).
    Fp(u8, u8),
    /// A constant.
    Imm(i64),
    /// A memory operand whose address is in general register `n`.
    Mem(u8),
    /// A symbol plus an offset.
    Sym(&'a str, i64),
    /// An `asm goto` target, by the name the caller resolves.
    Label(&'a str),
}

/// Writes `arg` with `modifier` as LLVM's AArch64 `AsmPrinter` does
/// (`PrintAsmOperand`, `PrintAsmMemoryOperand`):
///
/// - a general register is `x<n>`; `w` and `x` select the view; `b`,
///   `h`, `s`, `d`, `q` print the FP/SIMD register of that number;
/// - an FP/SIMD register is `v<n>`; `b`..`q` select the scalar view;
///   `w` and `x` select the view of the value's size (`h`, `s`, `d`,
///   `q`: `llc` 19 and 22, §20);
/// - a constant is decimal with no `#`; `w`/`x` print `wzr`/`xzr` for
///   zero; `c` is the constant, `n` its negation, `s` is
///   `(32 - c) & 31`;
/// - a memory operand is `[x<n>]` (modifier `a` or none);
/// - a symbol is its name with a `+`/`-` offset; `c` and `a` print it
///   the same;
/// - an `asm goto` target is its name (`l` or none).
///
/// # Errors
/// `asm:modifier/<m>` for a modifier the operand does not take.
pub fn print_operand(arg: Arg<'_>, modifier: Option<u8>, out: &mut String) -> Result<(), Refusal> {
    use std::fmt::Write as _;
    let bad = |m: u8| Err(reason::modifier(&char::from(m).to_string()));
    match (arg, modifier) {
        (Arg::Gp(n), None | Some(b'x')) => write!(out, "x{n}"),
        (Arg::Gp(n), Some(b'w')) => write!(out, "w{n}"),
        (Arg::Gp(n) | Arg::Fp(n, _), Some(m @ (b'b' | b'h' | b's' | b'd' | b'q'))) => {
            write!(out, "{}{n}", char::from(m))
        }
        (Arg::Fp(n, _), None) => write!(out, "v{n}"),
        (Arg::Fp(n, bytes), Some(m @ (b'w' | b'x'))) => {
            let view = match bytes {
                2 => 'h',
                4 => 's',
                8 => 'd',
                16 => 'q',
                _ => return bad(m),
            };
            write!(out, "{view}{n}")
        }
        (Arg::Imm(0), Some(b'w')) => write!(out, "wzr"),
        (Arg::Imm(0), Some(b'x')) => write!(out, "xzr"),
        (Arg::Imm(v), None | Some(b'w' | b'x' | b'c')) => write!(out, "{v}"),
        (Arg::Imm(v), Some(b'n')) => write!(out, "{}", v.wrapping_neg()),
        (Arg::Imm(v), Some(b's')) => write!(out, "{}", 32i64.wrapping_sub(v) & 31),
        (Arg::Mem(n), None | Some(b'a')) | (Arg::Gp(n), Some(b'a')) => write!(out, "[x{n}]"),
        (Arg::Sym(name, off), None | Some(b'c' | b'a')) => {
            out.push_str(name);
            match off {
                0 => Ok(()),
                o if o > 0 => write!(out, "+{o}"),
                o => write!(out, "{o}"),
            }
        }
        (Arg::Label(name), None | Some(b'l')) => {
            out.push_str(name);
            Ok(())
        }
        (_, Some(m)) => return bad(m),
    }
    .map_err(|_| Refusal(other::TEMPLATE))
}

/// A numeric local label definition: the statement it is on, and the
/// byte offset it stands for.
#[derive(Clone, Copy)]
struct LocalDef {
    n: u32,
    seq: u32,
    offset: u32,
}

/// A field to fill in once the labels are known.
#[derive(Clone, Copy)]
struct Pending {
    seq: u32,
    offset: u32,
    kind: RefKind,
    n: u32,
    forward: bool,
}

/// Splits a leading label definition `name:` off a statement.
pub fn split_label(stmt: &str) -> Option<(&str, &str)> {
    let b = stmt.as_bytes();
    let first = *b.first()?;
    let end = if first.is_ascii_digit() {
        b.iter().position(|c| !c.is_ascii_digit()).unwrap_or(b.len())
    } else if is_name_start(first) {
        b.iter().position(|&c| !is_name_char(c)).unwrap_or(b.len())
    } else {
        return None;
    };
    (b.get(end) == Some(&b':')).then(|| (&stmt[..end], crate::text::trim(&stmt[end + 1..])))
}

/// Splits a statement into its mnemonic (or directive) and its operand
/// text.
pub fn split_mnemonic(stmt: &str) -> (&str, &str) {
    match stmt.find(|c: char| c.is_ascii() && crate::text::is_space(c as u8)) {
        Some(i) => (&stmt[..i], crate::text::trim(&stmt[i..])),
        None => (stmt, ""),
    }
}

/// Assembles an inline-assembly body (the template after operand
/// substitution) into `code` (cleared first). `scratch` is reused text
/// space.
///
/// # Errors
/// The `asm:*` refusals of the statement that is not accepted.
pub fn assemble(
    body: &str,
    features: Features,
    code: &mut Code,
    scratch: &mut String,
) -> Result<(), Refusal> {
    assemble_at(body, features, 0, code, scratch)
}

/// The `nop` word, the padding of an alignment directive without a fill.
pub const NOP: u32 = 0xd503_201f;

/// [`assemble`] for a body whose first word is at byte offset `start`
/// (a multiple of 4) of a code origin aligned to at least
/// [`Code::align`]: `.p2align`, `.balign` and `.align` (a power of two)
/// pad to their boundary counted from that origin (§17), with `nop`
/// words, or with the fill byte when one is written; a maximum the
/// padding would pass leaves it out.
///
/// # Errors
/// As [`assemble`]; [`crate::align::parse`]'s refusals of a directive's
/// operands.
pub fn assemble_at(
    body: &str,
    features: Features,
    start: u32,
    code: &mut Code,
    scratch: &mut String,
) -> Result<(), Refusal> {
    debug_assert!(start.is_multiple_of(4), "a body at an offset that is not a multiple of 4");
    code.words.clear();
    code.refs.clear();
    code.align = 0;
    crate::text::clean(body, scratch)?;
    let mut defs: Vec<LocalDef> = Vec::new();
    let mut pending: Vec<Pending> = Vec::new();
    for (seq, stmt) in crate::text::statements(scratch).enumerate() {
        let seq = seq as u32;
        let mut stmt = stmt;
        while let Some((label, rest)) = split_label(stmt) {
            let Ok(n) = label.parse::<u32>() else { return Err(Refusal(other::LABEL)) };
            defs.push(LocalDef { n, seq, offset: offset_of(code)? });
            stmt = rest;
        }
        if stmt.is_empty() {
            continue;
        }
        let (mnemonic, rest) = split_mnemonic(stmt);
        if mnemonic.starts_with('.') {
            let is = |name: &str| mnemonic.eq_ignore_ascii_case(name);
            let log2 = if is(".p2align") || is(".align") {
                true
            } else if is(".balign") {
                false
            } else {
                return Err(reason::directive(mnemonic));
            };
            let a = crate::align::parse(rest, log2)?;
            code.align = code.align.max(a.align as u32);
            let pad = a.pad(u64::from(start) + u64::from(offset_of(code)?));
            let word = a.fill.map_or(NOP, |f| u32::from_le_bytes([f; 4]));
            code.words.extend(std::iter::repeat_n(word, (pad / 4) as usize));
            continue;
        }
        let ops = operand::parse(rest)?;
        let e = encode::encode(mnemonic, ops.as_slice(), features)?;
        let offset = offset_of(code)?;
        code.words.push(e.word);
        if let Some((kind, to)) = e.fixup {
            match to {
                Value { name: Some(Name::Local(n, forward)), constant: 0 } => {
                    pending.push(Pending { seq, offset, kind, n, forward });
                }
                Value { name: Some(Name::Local(..)), .. } => {
                    return Err(Refusal(op_reason::EXPRESSION));
                }
                Value { name: Some(Name::Sym(s)), constant } => {
                    code.refs.push(Ref { offset, kind, name: s.to_owned(), addend: constant });
                }
                Value { name: None, .. } => return Err(Refusal(op_reason::SYMBOL)),
            }
        }
    }
    for p in pending {
        let def = if p.forward {
            defs.iter().find(|d| d.n == p.n && d.seq > p.seq)
        } else {
            defs.iter().rev().find(|d| d.n == p.n && d.seq <= p.seq)
        };
        let Some(def) = def else { return Err(Refusal(op_reason::LABEL)) };
        let at = (p.offset / 4) as usize;
        let disp = i64::from(def.offset) - i64::from(p.offset);
        code.words[at] =
            p.kind.fixup().patch(code.words[at], disp).ok_or(Refusal(op_reason::REACH))?;
    }
    Ok(())
}

fn offset_of(code: &Code) -> Result<u32, Refusal> {
    u32::try_from(code.words.len() * 4).map_err(|_| Refusal(op_reason::REACH))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(text: &str) -> Result<Vec<u32>, Refusal> {
        let mut code = Code::default();
        assemble(text, Features::BASE, &mut code, &mut String::new())?;
        Ok(code.words)
    }

    /// `spec/asm.md` §5.3: local labels and what an inline body refuses.
    #[test]
    fn bodies() {
        assert_eq!(asm("nop; nop").unwrap(), [0xd503_201f, 0xd503_201f]);
        // `1: cbnz x0, 1b` branches to itself.
        assert_eq!(asm("1: cbnz x0, 1b").unwrap(), [0xb500_0000]);
        assert_eq!(asm("b 1f\n1:").unwrap(), [0x1400_0001]);
        assert_eq!(asm("x: nop").err(), Some(Refusal(other::LABEL)));
        assert_eq!(asm(".inst 0").err(), Some(reason::directive(".inst")));
        assert_eq!(asm("nop\n.p2align 2").unwrap(), [NOP]);
        assert_eq!(asm("b 2f").err(), Some(Refusal(op_reason::LABEL)));
        assert_eq!(asm("frob x0").err(), Some(reason::mnemonic("frob")));
        let mut code = Code::default();
        assemble("bl dump", Features::BASE, &mut code, &mut String::new()).unwrap();
        assert_eq!(
            code.refs,
            [Ref { offset: 0, kind: RefKind::Call26, name: "dump".into(), addend: 0 }]
        );
    }

    /// §17: alignment directives pad from the origin `start` counts from.
    #[test]
    fn alignment() {
        const B: u32 = 0x1400_0000;
        let at = |text: &str, start: u32| {
            let mut code = Code::default();
            assemble_at(text, Features::BASE, start, &mut code, &mut String::new())
                .map(|()| (code.words, code.align))
        };
        // `b 1f` over the padding to the label after it.
        assert_eq!(
            at("b 1f\n.p2align 4\n1: nop", 0).unwrap(),
            (vec![B | 4, NOP, NOP, NOP, NOP], 16)
        );
        assert_eq!(at("b 1f\n.p2align 4\n1: nop", 8).unwrap(), (vec![B | 2, NOP, NOP], 16));
        assert_eq!(at("b 1f\n.p2align 4\n1: nop", 12).unwrap(), (vec![B | 1, NOP], 16));
        assert_eq!(at(".balign 16, 0xff\nnop", 4).unwrap(), (vec![!0, !0, !0, NOP], 16));
        assert_eq!(at(".P2ALIGN 6,,8\nnop", 52).unwrap(), (vec![NOP], 64));
        assert_eq!(at(".p2align 6,,8\nnop", 56).unwrap(), (vec![NOP, NOP, NOP], 64));
        assert_eq!(at(".align 3\n.balign 2\n.p2align 0\nnop", 4).unwrap(), (vec![NOP, NOP], 8));
        assert_eq!(at("nop", 4).unwrap(), (vec![NOP], 0));
        assert_eq!(at(".balign 3", 0).err(), Some(Refusal(op_reason::RANGE)));
        assert_eq!(at(".balignw 4", 0).err(), Some(reason::directive(".balignw")));
    }

    #[test]
    fn printing() {
        let p = |a, m| {
            let mut s = String::new();
            print_operand(a, m, &mut s).map(|()| s)
        };
        assert_eq!(p(Arg::Gp(3), None).unwrap(), "x3");
        assert_eq!(p(Arg::Gp(3), Some(b'w')).unwrap(), "w3");
        assert_eq!(p(Arg::Fp(3, 16), Some(b'd')).unwrap(), "d3");
        assert_eq!(p(Arg::Fp(3, 16), None).unwrap(), "v3");
        // §20: `w` and `x` print the view of the value's size.
        assert_eq!(p(Arg::Fp(3, 2), Some(b'w')).unwrap(), "h3");
        assert_eq!(p(Arg::Fp(3, 4), Some(b'x')).unwrap(), "s3");
        assert_eq!(p(Arg::Fp(3, 8), Some(b'w')).unwrap(), "d3");
        assert_eq!(p(Arg::Fp(3, 16), Some(b'x')).unwrap(), "q3");
        assert!(p(Arg::Fp(1, 16), Some(b'v')).is_err());
        assert!(p(Arg::Fp(1, 16), Some(b'a')).is_err());
        assert_eq!(p(Arg::Imm(0), Some(b'w')).unwrap(), "wzr");
        assert_eq!(p(Arg::Imm(-5), None).unwrap(), "-5");
        assert_eq!(p(Arg::Imm(5), Some(b'n')).unwrap(), "-5");
        assert_eq!(p(Arg::Mem(9), None).unwrap(), "[x9]");
        assert_eq!(p(Arg::Sym("g", -4), None).unwrap(), "g-4");
        assert!(p(Arg::Mem(1), Some(b'w')).is_err());
    }

    #[test]
    fn features() {
        assert!(Features::from_attribute("+neon,+v8.2a").lse);
        assert!(!Features::from_attribute("+v8a,+neon").lse);
        assert!(Features::from_attribute("+crc").crc);
        assert!(!Features::from_attribute("-fp-armv8").fp);
        // §20: in order, with LLVM's implications.
        assert!(Features::from_attribute("+aes").neon);
        assert!(!Features::from_attribute("+aes,-neon").aes);
        assert!(Features::from_attribute("-neon,+aes").aes);
        assert!(Features::from_attribute("-fp-armv8,+neon").fp);
        assert!(!Features::from_attribute("+fullfp16,-fp-armv8").fp16);
        assert!(!Features::from_attribute("-neon,+fullfp16").neon);
        assert!(Features::from_attribute("+v8.2a,+crypto").aes);
        assert!(!Features::from_attribute("+v9a").fp16);
    }
}
