//! x86-64 AT&T-syntax assembly (`spec/asm.md` §10): operand printing for
//! inline-assembly templates ([`print_operand`]), and the assembler of
//! an inline-assembly body ([`assemble`]): statements, prefixes, data
//! directives, numeric local labels and branches to them, and
//! instructions ([`encode`]) whose bytes come from `hod_mc_x64`'s
//! constructors.
//!
//! As on AArch64, an inline body defines no symbol: a named label or a
//! directive other than the data directives is refused (`asm:label`,
//! `asm:directive/<name>`). A memory operand may name a symbol, with or
//! without a relocation specifier (`sym@tpoff`, `sym@gotpcrel(%rip)`):
//! its field is left zero and returned as a [`Ref`] for the caller to
//! relocate, of the kind LLVM's assembler writes (`spec/asm.md` §10.2).
//! A symbol anywhere else (an immediate, a branch target) is
//! `asm:operand/symbol`.

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

/// Writes the symbol operand `name` + `addend` (a `Ws` or `s` operand,
/// `spec/asm.md` §13) with `modifier` as `X86AsmPrinter` does: `$name+8`
/// without one, `name+8` with `c`, `p` or `P` (`name`, `name-4` for the
/// other addends).
///
/// # Errors
/// `asm:modifier/<m>` for another modifier (`a`, which LLVM 19 and 22
/// print differently, is one).
pub fn print_symbol(
    name: &str,
    addend: i64,
    modifier: Option<u8>,
    out: &mut String,
) -> Result<(), Refusal> {
    use std::fmt::Write as _;
    match modifier {
        None => out.push('$'),
        Some(b'c' | b'p' | b'P') => {}
        Some(m) => return Err(reason::modifier(&char::from(m).to_string())),
    }
    out.push_str(name);
    if addend != 0 {
        write!(out, "{addend:+}").map_err(|_| Refusal(other::TEMPLATE))?;
    }
    Ok(())
}

/// The relocation of a symbol field, as LLVM's assembler chooses it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// `sym(%rip)`: `R_X86_64_PC32`.
    Pc32,
    /// `sym@plt(%rip)`: `R_X86_64_PLT32`.
    Plt32,
    /// `sym@gotpcrel(%rip)` where the linker may not relax it:
    /// `R_X86_64_GOTPCREL`.
    GotPcRel,
    /// `sym@gotpcrel(%rip)` as the source of a 32- or 64-bit `mov`,
    /// `adc add and cmp or sbb sub xor`, or the memory of `test`, no
    /// addend, without a REX prefix: `R_X86_64_GOTPCRELX`.
    GotPcRelX,
    /// The same with a REX prefix: `R_X86_64_REX_GOTPCRELX`.
    RexGotPcRelX,
    /// `sym@gottpoff(%rip)`: `R_X86_64_GOTTPOFF`.
    GotTpOff,
    /// `sym@tpoff` without `%rip`: `R_X86_64_TPOFF32`.
    TpOff32,
    /// `sym@dtpoff` without `%rip`: `R_X86_64_DTPOFF32`.
    DtpOff32,
    /// `sym` without `%rip` or specifier: `R_X86_64_32S`.
    Abs32S,
    /// `.reloc ., BFD_RELOC_NONE, sym`: `R_X86_64_NONE`, a field of no
    /// bytes (a reference that keeps `sym` linked).
    None,
}

/// A symbol field of assembled code, zero in the code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ref {
    /// The byte offset of the field in the code.
    pub offset: u32,
    /// The relocation.
    pub kind: RefKind,
    /// The symbol's name.
    pub name: String,
    /// The addend: the expression's constant, and for a field relative
    /// to `%rip` minus the bytes from the field to the instruction's end.
    pub addend: i64,
}

/// The mnemonics whose `@gotpcrel` load LLVM marks relaxable.
const RELAXABLE: [&str; 9] = ["mov", "adc", "add", "and", "cmp", "or", "sbb", "sub", "xor"];

/// The relocation of the symbol field of `inst`, statement `name` with
/// operands `ops`, memory operand `m`.
fn ref_kind(
    name: &str,
    ops: &operand::Ops<'_>,
    m: &operand::MemOp<'_>,
    inst: &mc::Inst,
) -> Result<RefKind, Refusal> {
    use operand::{Base, Gpr, Op, Spec};
    let rip = m.base == Some(Base::Rip);
    Ok(match (rip, m.spec) {
        (true, None) => RefKind::Pc32,
        (true, Some(Spec::Plt)) => RefKind::Plt32,
        (true, Some(Spec::GotTpOff)) => RefKind::GotTpOff,
        (true, Some(Spec::GotPcRel)) => {
            let base = if RELAXABLE.contains(&name) || name == "test" || name == "lea" {
                name
            } else {
                name.strip_suffix(['l', 'q']).unwrap_or(name)
            };
            // LLVM 19 and 22 disagree on `lea`.
            if base == "lea" {
                return Err(Refusal(op_reason::SPECIFIER));
            }
            let wide =
                |o: Option<Op<'_>>| matches!(o, Some(Op::Gpr(Gpr { size: Size::D | Size::Q, .. })));
            let mem = |o: Option<Op<'_>>| matches!(o, Some(Op::Mem(_)));
            let relaxable = m.disp.constant == 0
                && ops.len() == 2
                && ((RELAXABLE.contains(&base) && mem(ops.get(0)) && wide(ops.get(1)))
                    || (base == "test" && wide(ops.get(0)) && mem(ops.get(1))));
            let rex = inst
                .bytes()
                .iter()
                .find(|&&b| b != 0x64 && b != 0x65)
                .is_some_and(|b| b & 0xf0 == 0x40);
            match (relaxable, rex) {
                (false, _) => RefKind::GotPcRel,
                (true, false) => RefKind::GotPcRelX,
                (true, true) => RefKind::RexGotPcRelX,
            }
        }
        (false, None) => RefKind::Abs32S,
        (false, Some(Spec::TpOff)) => RefKind::TpOff32,
        (false, Some(Spec::DtpOff)) => RefKind::DtpOff32,
        _ => return Err(Refusal(op_reason::SPECIFIER)),
    })
}

/// A symbol field before layout: its offset in the byte buffer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Pending {
    /// The field's offset in the byte buffer.
    pub(crate) at: usize,
    /// The relocation.
    pub(crate) kind: RefKind,
    /// The symbol.
    pub(crate) name: String,
    /// The addend (for a `%rip` field, minus the bytes from the field to
    /// the instruction's end).
    pub(crate) addend: i64,
}

/// A transfer whose form or target is decided once labels are known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Jump {
    /// `jmp`: `EB rel8` or `E9 rel32`.
    Jmp,
    /// `j<cc>`: `7x rel8` or `0F 8x rel32`.
    Jcc(Cond),
    /// `call`: `E8 rel32`, never short.
    Call,
}

impl Jump {
    /// The bytes of the short (`near` false) or near form.
    pub(crate) fn len(self, near: bool) -> usize {
        match (self, near) {
            (Jump::Call, _) | (Jump::Jmp, true) => 5,
            (_, false) => 2,
            (Jump::Jcc(_), true) => 6,
        }
    }

    /// The instruction with displacement `disp` (short when it fits
    /// `near` false).
    pub(crate) fn inst(self, near: bool, disp: i64) -> Result<mc::Inst, Refusal> {
        let rel32 = || i32::try_from(disp).map_err(|_| Refusal(op_reason::REACH));
        let rel8 = || i8::try_from(disp).map_err(|_| Refusal(op_reason::REACH));
        Ok(match (self, near) {
            (Jump::Call, _) => mc::call(rel32()?),
            (Jump::Jmp, false) => mc::jmp_short(rel8()?),
            (Jump::Jcc(c), false) => mc::jcc_short(c, rel8()?),
            (Jump::Jmp, true) => mc::jmp_near(rel32()?),
            (Jump::Jcc(c), true) => mc::jcc_near(c, rel32()?),
        })
    }
}

/// One item of a body: bytes, a branch to a local label, a label.
#[derive(Clone, Copy, Debug)]
enum Item {
    /// Bytes `at..at + len` of the byte buffer.
    Bytes { at: usize, len: usize },
    /// A branch or call to local label `n`; `forward` for `nf`.
    Branch { jump: Jump, n: u32, forward: bool, near: bool },
    /// A numeric label definition.
    Label(u32),
}

/// Splits a leading label definition `name:` off a statement.
fn split_label(stmt: &str) -> Option<(&str, &str)> {
    crate::a64::split_label(stmt)
}

/// Splits a statement into its mnemonic (or directive) and its operand
/// text.
pub(crate) fn split_mnemonic(stmt: &str) -> (&str, &str) {
    match stmt.find(|c: char| c.is_ascii() && is_space(c as u8)) {
        Some(i) => (&stmt[..i], trim(&stmt[i..])),
        None => (stmt, ""),
    }
}

/// A prefix statement: `lock`, or `rep` and `repne` and their synonyms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Prefix {
    Lock,
    /// `rep`, `repe`, `repz`: `F3`.
    Rep,
    /// `repne`, `repnz`: `F2`.
    Repne,
}

fn prefix(name: &str) -> Option<Prefix> {
    match name {
        "lock" => Some(Prefix::Lock),
        "rep" | "repe" | "repz" => Some(Prefix::Rep),
        "repne" | "repnz" => Some(Prefix::Repne),
        _ => None,
    }
}

/// The x87 waiting forms: each is `wait` (`9B`) and then the form that
/// does not wait, two instructions to LLVM's assembler.
fn waiting(name: &str) -> Option<&'static str> {
    Some(match name {
        "fstcw" => "fnstcw",
        "fstsw" => "fnstsw",
        "fstenv" => "fnstenv",
        "fsave" => "fnsave",
        "fclex" => "fnclex",
        "finit" => "fninit",
        _ => return None,
    })
}

/// A repeat prefix and its instruction, an instruction of its own to
/// LLVM's assembler, joined or not: its byte (into `bytes`), then the
/// string instruction's (`F3 66 A5` for `rep movsw`).
fn repeated(p: Prefix, name: &str, rest: &str, bytes: &mut Vec<u8>) -> Result<mc::Inst, Refusal> {
    if !rest.is_empty() {
        return Err(Refusal(op_reason::FORM));
    }
    let inst = if name == "nop" {
        mc::nop(1)
    } else {
        encode::string_op(name).ok_or_else(|| reason::mnemonic(name))?
    };
    bytes.push(if p == Prefix::Rep { 0xf3 } else { 0xf2 });
    Ok(inst)
}

/// The data directives: their field size.
pub(crate) fn data_size(name: &str) -> Option<usize> {
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
/// substitution) into `code`, its symbol fields into `refs` (both
/// cleared first). `scratch` is reused text space.
///
/// # Errors
/// The `asm:*` refusals of the statement that is not accepted.
pub fn assemble(
    body: &str,
    code: &mut Vec<u8>,
    refs: &mut Vec<Ref>,
    scratch: &mut String,
) -> Result<(), Refusal> {
    code.clear();
    refs.clear();
    let mut pending_refs: Vec<Pending> = Vec::new();
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
        let (mnemonic, rest) = split_mnemonic(stmt);
        let name = mnemonic.to_ascii_lowercase();
        if name.starts_with('.') {
            if pending.is_some() {
                return Err(Refusal(op_reason::FORM));
            }
            if name == ".reloc" {
                let at = bytes.len();
                pending_refs.push(reloc(rest, at)?);
                items.push(Item::Bytes { at, len: 0 });
                continue;
            }
            let Some(size) = data_size(&name) else { return Err(reason::directive(&name)) };
            let at = bytes.len();
            data(size, rest, &mut bytes)?;
            items.push(Item::Bytes { at, len: bytes.len() - at });
            continue;
        }
        let at = bytes.len();
        match instruction(mnemonic, rest, &mut pending, &mut bytes)? {
            Stmt::Bytes(field) => {
                if let Some(p) = field {
                    pending_refs.push(p);
                }
                if bytes.len() != at {
                    items.push(Item::Bytes { at, len: bytes.len() - at });
                }
            }
            Stmt::Jump(jump, target, kind) => match target {
                Value { name: Some(Name::Local(n, forward)), constant: 0 } => {
                    items.push(Item::Branch { jump, n, forward, near: jump == Jump::Call });
                }
                Value { name: Some(Name::Local(..)), .. } => {
                    return Err(Refusal(op_reason::EXPRESSION));
                }
                // §14: a symbol, through its PLT entry, near.
                Value { name: Some(Name::Sym(sym)), constant } => {
                    pending_refs.push(symbol_jump(jump, sym, constant, kind, &mut bytes)?);
                    items.push(Item::Bytes { at, len: bytes.len() - at });
                }
                Value { name: None, .. } => return Err(Refusal(op_reason::SYMBOL)),
            },
        }
    }
    if pending.is_some() {
        return Err(Refusal(op_reason::FORM));
    }
    let offsets = lay_out(&mut items, &bytes, code)?;
    place_refs(&items, &offsets, pending_refs, refs)
}

/// The near form of `jump` to symbol `sym` plus `constant`, appended to
/// `bytes`, and its field: `R_X86_64_PLT32` with the addend minus 4, as
/// LLVM's assembler writes a branch or call to a symbol whatever its
/// binding, static or PIC (`spec/asm.md` §14).
pub(crate) fn symbol_jump(
    jump: Jump,
    sym: &str,
    constant: i64,
    kind: RefKind,
    bytes: &mut Vec<u8>,
) -> Result<Pending, Refusal> {
    let inst = jump.inst(true, 0)?;
    let site = inst.site().ok_or(Refusal(op_reason::SYMBOL))?;
    let at = bytes.len() + site.offset;
    bytes.extend_from_slice(inst.bytes());
    Ok(Pending {
        at,
        kind,
        name: sym.to_owned(),
        addend: constant.checked_add(site.addend).ok_or(Refusal(op_reason::EXPRESSION))?,
    })
}

/// The relocation of a branch target written as `rest` (§14): llvm-mc
/// keeps `R_X86_64_PLT32` for a symbol alone or one with `@PLT` (and an
/// addend), and writes `R_X86_64_PC32` for any other expression, `S+0`
/// included.
fn jump_kind(rest: &str) -> RefKind {
    let t = rest.trim();
    let alone = !t.is_empty()
        && !t.starts_with(|c: char| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'));
    if alone || t.contains('@') { RefKind::Plt32 } else { RefKind::Pc32 }
}

/// What an instruction statement is once read.
pub(crate) enum Stmt<'a> {
    /// Its bytes, appended to the buffer (none for a prefix alone, which
    /// waits for the next statement), and its symbol field.
    Bytes(Option<Pending>),
    /// A branch or call: its target is decided by the caller, nothing is
    /// appended. The kind is that of a relocated field (§14):
    /// `R_X86_64_PLT32` for a symbol alone or with `@PLT`, `R_X86_64_PC32`
    /// for any other expression (`S+0` included), as llvm-mc writes.
    Jump(Jump, Value<'a>, RefKind),
}

/// Reads instruction statement `mnemonic rest` (§10.4: prefixes, alone
/// in `pending` or joined, the waiting x87 forms, the repeat prefixes)
/// and appends its bytes to `bytes`.
///
/// # Errors
/// The statement's `asm:*` refusal.
pub(crate) fn instruction<'a>(
    mnemonic: &str,
    rest: &'a str,
    pending: &mut Option<Prefix>,
    bytes: &mut Vec<u8>,
) -> Result<Stmt<'a>, Refusal> {
    let mut name = mnemonic.to_ascii_lowercase();
    let mut rest = rest;
    // A prefix with its instruction on the same statement is part of
    // the instruction (`lock xadd`: `F0` after `66`, as LLVM writes it);
    // a prefix alone is a byte of its own before the next one.
    let mut joined = None;
    if let Some(p) = prefix(&name) {
        if rest.is_empty() {
            if pending.is_some() {
                return Err(Refusal(op_reason::FORM));
            }
            *pending = Some(p);
            return Ok(Stmt::Bytes(None));
        }
        joined = Some(p);
        let (m, r) = split_mnemonic(rest);
        name = m.to_ascii_lowercase();
        rest = r;
    }
    let separate = pending.take();
    let prefix = joined.or(separate);
    let inst = match prefix {
        Some(p @ (Prefix::Rep | Prefix::Repne)) => repeated(p, &name, rest, bytes)?,
        Some(Prefix::Lock) | None => {
            if let Some(form) = waiting(&name) {
                if prefix.is_some() {
                    return Err(Refusal(op_reason::FORM));
                }
                bytes.push(0x9b);
                name = form.to_owned();
            }
            let ops = operand::parse(rest)?;
            match encode::encode(&name, &ops)? {
                Encoded::Inst(inst) => {
                    let inst = match prefix {
                        Some(_) if !inst.is_lockable() => return Err(Refusal(op_reason::FORM)),
                        Some(_) if separate.is_some() => {
                            bytes.push(0xf0);
                            inst
                        }
                        Some(_) => mc::lock(inst),
                        None => inst,
                    };
                    let field = symbol_field(&name, &ops, &inst, bytes.len())?;
                    bytes.extend_from_slice(inst.bytes());
                    return Ok(Stmt::Bytes(field));
                }
                Encoded::Branch(cond, target) => {
                    if prefix.is_some() {
                        return Err(Refusal(op_reason::FORM));
                    }
                    let kind = jump_kind(rest);
                    return Ok(Stmt::Jump(cond.map_or(Jump::Jmp, Jump::Jcc), target, kind));
                }
                Encoded::Call(target) => {
                    if prefix.is_some() {
                        return Err(Refusal(op_reason::FORM));
                    }
                    return Ok(Stmt::Jump(Jump::Call, target, jump_kind(rest)));
                }
            }
        }
    };
    bytes.extend_from_slice(inst.bytes());
    Ok(Stmt::Bytes(None))
}

/// Moves each symbol field from its byte-buffer offset to its offset in
/// the laid-out code.
fn place_refs(
    items: &[Item],
    offsets: &[usize],
    pending_refs: Vec<Pending>,
    refs: &mut Vec<Ref>,
) -> Result<(), Refusal> {
    for p in pending_refs {
        // A `.reloc` field is the empty item at its offset.
        let Some((i, start)) = items.iter().enumerate().find_map(|(i, item)| match *item {
            Item::Bytes { at, len }
                if (at..at + len).contains(&p.at) || (len == 0 && at == p.at) =>
            {
                Some((i, at))
            }
            _ => None,
        }) else {
            return Err(Refusal(op_reason::SYMBOL));
        };
        let offset =
            u32::try_from(offsets[i] + (p.at - start)).map_err(|_| Refusal(op_reason::RANGE))?;
        refs.push(Ref { offset, kind: p.kind, name: p.name, addend: p.addend });
    }
    Ok(())
}

/// `.reloc ., BFD_RELOC_NONE, sym[+-c]` (or `R_X86_64_NONE`), the one
/// form of the directive inline assembly writes (`spec/asm.md` §13): an
/// `R_X86_64_NONE` field of no bytes at byte `at` of the buffer.
///
/// # Errors
/// `asm:operand/expression` for an offset other than `.`;
/// `asm:directive/reloc` for another relocation; `asm:operand/symbol` for
/// a target that names no symbol.
fn reloc(operands: &str, at: usize) -> Result<Pending, Refusal> {
    let mut parts = operands.splitn(3, ',').map(trim);
    let (Some(offset), Some(kind), Some(target)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal(op_reason::SYNTAX));
    };
    if offset != "." {
        return Err(Refusal(op_reason::EXPRESSION));
    }
    if kind != "BFD_RELOC_NONE" && kind != "R_X86_64_NONE" {
        return Err(reason::directive(".reloc"));
    }
    let mut c = Cursor::new(target);
    let v = c.expr()?;
    if !c.at_end() {
        return Err(Refusal(op_reason::SYNTAX));
    }
    let Some(Name::Sym(name)) = v.name else { return Err(Refusal(op_reason::SYMBOL)) };
    Ok(Pending { at, kind: RefKind::None, name: name.to_string(), addend: v.constant })
}

/// The symbol field of `inst` (statement `name`, operands `ops`), placed
/// at byte `at` of the buffer, if an operand names a symbol.
pub(crate) fn symbol_field(
    name: &str,
    ops: &operand::Ops<'_>,
    inst: &mc::Inst,
    at: usize,
) -> Result<Option<Pending>, Refusal> {
    let Some((sym, m)) = symbol_operand(ops) else { return Ok(None) };
    let site = inst.site().ok_or(Refusal(op_reason::SYMBOL))?;
    Ok(Some(Pending {
        at: at + site.offset,
        kind: ref_kind(name, ops, &m, inst)?,
        name: sym.to_owned(),
        addend: m.disp.constant.checked_add(site.addend).ok_or(Refusal(op_reason::EXPRESSION))?,
    }))
}

/// The memory operand of `ops` whose displacement names a symbol, and
/// the symbol.
fn symbol_operand<'a>(ops: &operand::Ops<'a>) -> Option<(&'a str, operand::MemOp<'a>)> {
    (0..ops.len()).find_map(|i| match ops.get(i) {
        Some(operand::Op::Mem(m)) => match m.disp.name {
            Some(Name::Sym(s)) => Some((s, m)),
            _ => None,
        },
        _ => None,
    })
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
/// the code. Returns each item's offset in the code.
fn lay_out(items: &mut [Item], bytes: &[u8], code: &mut Vec<u8>) -> Result<Vec<usize>, Refusal> {
    let mut offsets = vec![0usize; items.len() + 1];
    loop {
        let mut at = 0;
        for (i, item) in items.iter().enumerate() {
            offsets[i] = at;
            at += match *item {
                Item::Bytes { len, .. } => len,
                Item::Branch { jump, near, .. } => jump.len(near),
                Item::Label(_) => 0,
            };
        }
        offsets[items.len()] = at;
        let mut changed = false;
        for i in 0..items.len() {
            if let Item::Branch { jump, near: false, .. } = items[i] {
                let end = offsets[i] + jump.len(false);
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
            Item::Branch { jump, near, .. } => {
                let end = offsets[i] + jump.len(near);
                let disp = target(items, &offsets, i)? as i64 - end as i64;
                code.extend_from_slice(jump.inst(near, disp)?.bytes());
            }
        }
    }
    Ok(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(text: &str) -> Result<Vec<u8>, Refusal> {
        let (code, refs) = with_refs(text)?;
        assert!(refs.is_empty(), "{text}");
        Ok(code)
    }

    fn with_refs(text: &str) -> Result<(Vec<u8>, Vec<Ref>), Refusal> {
        let (mut code, mut refs) = (Vec::new(), Vec::new());
        assemble(text, &mut code, &mut refs, &mut String::new())?;
        Ok((code, refs))
    }

    /// `spec/asm.md` §10.2 "Symbols": the fields and relocations of
    /// llvm-mc 19 and 22.
    #[test]
    fn symbols() {
        let one = |text: &str| {
            let (code, refs) = with_refs(text).unwrap();
            assert_eq!(refs.len(), 1, "{text}");
            let r = &refs[0];
            assert_eq!(r.name, "s", "{text}");
            (code, r.offset, r.kind, r.addend)
        };
        let lea = [0x48, 0x8d, 0x04, 0x25, 0, 0, 0, 0];
        assert_eq!(one("leaq s@tpoff, %rax"), (lea.to_vec(), 4, RefKind::TpOff32, 0));
        assert_eq!(one("leaq s@TPOFF+16, %rax").3, 16);
        let fs = one("movq %fs:s@tpoff, %rax");
        assert_eq!((fs.0[0], fs.1, fs.2), (0x64, 5, RefKind::TpOff32));
        assert_eq!(
            one("movl s@tpoff(%rax), %ecx"),
            (vec![0x8b, 0x88, 0, 0, 0, 0], 2, RefKind::TpOff32, 0)
        );
        assert_eq!(one("leaq s@dtpoff(%rax), %rcx").2, RefKind::DtpOff32);
        assert_eq!(
            one("movq s@gottpoff(%rip), %rax"),
            (vec![0x48, 0x8b, 0x05, 0, 0, 0, 0], 3, RefKind::GotTpOff, -4)
        );
        assert_eq!(
            one("leaq s+4(%rip), %rax").2..=one("leaq s+4(%rip), %rax").2,
            RefKind::Pc32..=RefKind::Pc32
        );
        assert_eq!(one("leaq s+4(%rip), %rax").3, 0);
        assert_eq!(one("movl $1, s(%rip)").3, -8);
        assert_eq!(one("leaq s@PLT(%rip), %rax").2, RefKind::Plt32);
        assert_eq!(one("movq s@GOTPCREL(%rip), %rax").2, RefKind::RexGotPcRelX);
        assert_eq!(one("movl s@GOTPCREL(%rip), %eax").2, RefKind::GotPcRelX);
        assert_eq!(one("movl s@GOTPCREL(%rip), %r8d").2, RefKind::RexGotPcRelX);
        assert_eq!(one("testl %eax, s@GOTPCREL(%rip)").2, RefKind::GotPcRelX);
        assert_eq!(one("orl s@GOTPCREL(%rip), %eax").2, RefKind::GotPcRelX);
        assert_eq!(one("movq %rax, s@GOTPCREL(%rip)").2, RefKind::GotPcRel);
        assert_eq!(one("movw s@GOTPCREL(%rip), %ax").2, RefKind::GotPcRel);
        assert_eq!(one("movq s+8@GOTPCREL(%rip), %rax").2, RefKind::GotPcRel);
        assert_eq!(one("cmpq $1, s@GOTPCREL(%rip)").3, -5);
        assert_eq!(one("movl s, %eax").2, RefKind::Abs32S);
        // After a relaxed branch, the field moves with its instruction.
        let far = format!("jmp 1f\n{}1: movq s@gottpoff(%rip), %rax", ".quad 0\n".repeat(16));
        assert_eq!(one(&far).1, 5 + 128 + 3);
        let sp = Some(Refusal(op_reason::SPECIFIER));
        assert_eq!(with_refs("leaq s@GOTPCREL(%rip), %rax").err(), sp);
        assert_eq!(with_refs("movq s@tpoff(%rip), %rax").err(), sp);
        assert_eq!(with_refs("leaq s@gottpoff, %rax").err(), sp);
        assert_eq!(with_refs("movq $s@tpoff, %rax").err(), sp);
        assert_eq!(with_refs("movq s@got(%rip), %rax").err(), sp);
        assert_eq!(with_refs("movl $s, %eax").err(), Some(Refusal(op_reason::SYMBOL)));
        // §14: a branch or call to a symbol, near, through its PLT entry.
        let plt = |text: &str| {
            let (code, refs) = with_refs(text).unwrap();
            (code, refs[0].offset, refs[0].kind, refs[0].addend)
        };
        assert_eq!(plt("jmp s@plt"), (vec![0xe9, 0, 0, 0, 0], 1, RefKind::Plt32, -4));
        assert_eq!(plt("call s"), (vec![0xe8, 0, 0, 0, 0], 1, RefKind::Plt32, -4));
        assert_eq!(plt("callq s+8"), (vec![0xe8, 0, 0, 0, 0], 1, RefKind::Pc32, 4));
        assert_eq!(plt("call s+0").2, RefKind::Pc32);
        assert_eq!(plt("call s@PLT+8").2, RefKind::Plt32);
        assert_eq!(plt("nop; jne s").0, [0x90, 0x0f, 0x85, 0, 0, 0, 0]);
        assert_eq!(plt("nop; jne s").1, 3);
        assert_eq!(asm("call 1f\n1:").unwrap(), [0xe8, 0, 0, 0, 0]);
        assert_eq!(asm("1: call 1b").unwrap(), [0xe8, 0xfb, 0xff, 0xff, 0xff]);
        assert_eq!(with_refs("call s@gotpcrel").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(with_refs("lock call s").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(with_refs("callw s").err(), Some(reason::mnemonic("callw")));
        assert_eq!(asm("fwait; wait").unwrap(), [0x9b, 0x9b]);
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
        assert_eq!(with_refs("jmp $5").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(asm("frob %eax").err(), Some(reason::mnemonic("frob")));
        assert_eq!(asm("lock").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(asm("lock movl %eax, (%rdi)").err(), Some(Refusal(op_reason::FORM)));
        assert_eq!(asm(".byte 256").err(), Some(Refusal(op_reason::RANGE)));
        assert_eq!(asm("rep stosd").err(), Some(reason::mnemonic("stosd")));
        assert_eq!(asm("rep movsl %eax").err(), Some(Refusal(op_reason::FORM)));
        // §13: the repeat prefixes of every string instruction, the
        // waiting x87 forms.
        assert_eq!(asm("repe cmpsb").unwrap(), [0xf3, 0xa6]);
        assert_eq!(asm("repne; scasw").unwrap(), [0xf2, 0x66, 0xaf]);
        assert_eq!(asm("rep movsw").unwrap(), [0xf3, 0x66, 0xa5]);
        assert_eq!(asm("rep movsd").unwrap(), [0xf3, 0xa5]);
        assert_eq!(asm("fstcw (%rax)").unwrap(), [0x9b, 0xd9, 0x38]);
        assert_eq!(asm("fstsw").unwrap(), [0x9b, 0xdf, 0xe0]);
        assert_eq!(asm("lock fstsw").err(), Some(Refusal(op_reason::FORM)));
        // §13: `.reloc`, a field of no bytes.
        let (code, refs) = with_refs("nop\n.reloc ., BFD_RELOC_NONE, foo+8\nnop").unwrap();
        assert_eq!(code, [0x90, 0x90]);
        assert_eq!(refs, [Ref { offset: 1, kind: RefKind::None, name: "foo".into(), addend: 8 }]);
        let far = format!("jmp 1f\n{}.reloc ., BFD_RELOC_NONE, g\n1:", ".quad 0\n".repeat(16));
        assert_eq!(with_refs(&far).unwrap().1[0].offset, 5 + 128);
    }

    #[test]
    fn printing() {
        let p = |a, m| {
            let mut s = String::new();
            print_operand(a, m, &mut s).map(|()| s)
        };
        assert_eq!(p(Arg::Gp(0, 4), None).unwrap(), "%eax");
        let sym = |a, m| {
            let mut s = String::new();
            print_symbol("g", a, m, &mut s).map(|()| s)
        };
        assert_eq!(sym(0, None).unwrap(), "$g");
        assert_eq!(sym(8, Some(b'p')).unwrap(), "g+8");
        assert_eq!(sym(-4, Some(b'c')).unwrap(), "g-4");
        assert_eq!(sym(4, Some(b'P')).unwrap(), "g+4");
        assert_eq!(sym(0, Some(b'a')).err(), Some(reason::modifier("a")));
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
