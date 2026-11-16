//! 32-bit x86 AT&T-syntax assembly (`spec/asm.md` §22, WP-301): the
//! assembler of an inline-assembly body for `i686-*-linux-gnu`
//! ([`assemble`]) and the operand printing of its templates
//! ([`print_operand`]).
//!
//! One assembler, two modes (design 0008 §1.3): the statements are read
//! by [`crate::x64`]'s parser in [`Mode::X86`] (32-bit register names
//! only, 32-bit address registers) and encoded by its encoder, and every
//! instruction passes [`Mode::admit`], whose error is the refusal
//! (`x86-mode:extended-register`, `:operand-size-64`, `:rip-relative`,
//! `:x86-64-only`). What differs in the statements is [`statement`]'s.

use hod_mc_x64::{Mode, Size};

use crate::reason::{self, Refusal, other};
use crate::x64::operand::{self, Gpr, Op, Ops};
use crate::x64::{Arg, Ref};

/// Assembles the inline body `body` (after operand substitution) for
/// 32-bit mode into `code`, its symbol fields into `refs` (both cleared
/// first), as [`crate::x64::assemble`] does in 64-bit mode. A symbol
/// field's [`crate::x64::RefKind`] reads as the 32-bit relocation:
/// `Abs32S` is `R_386_32`, `Pc32` is `R_386_PC32`, `Plt32` is
/// `R_386_PLT32`, `None` is `R_386_NONE`; no other kind is returned.
///
/// # Errors
/// The `asm:*` refusals of the statement that is not accepted, and the
/// `x86-mode:*` names of [`Mode::admit`].
pub fn assemble(
    body: &str,
    code: &mut Vec<u8>,
    refs: &mut Vec<Ref>,
    scratch: &mut String,
) -> Result<(), Refusal> {
    crate::x64::assemble_in(body, code, refs, scratch, Mode::X86)
}

/// The mnemonics that exist only in 64-bit mode, or whose 64-bit form
/// [`Mode::admit`] would take for another instruction (`pushq %rax` is
/// `50`, `push %eax` in 32-bit mode): `asm:mnemonic/<name>`.
const X64_ONLY: &[&str] = &[
    "pushq",
    "popq",
    "pushfq",
    "popfq",
    "retq",
    "leaveq",
    "callq",
    "jmpq",
    "syscall",
    "sysretq",
    "swapgs",
    "cltq",
    "cdqe",
    "cqto",
    "cqo",
    "cmpxchg16b",
    "movslq",
    "movsxd",
    "movabsq",
    "movabs",
    "iretq",
    "endbr64",
];

/// Reads statement `name` with operands `ops` for 32-bit mode: the name
/// the 64-bit encoder knows it by, `ops` re-sized where the 64-bit form
/// is the same bytes.
///
/// - `push`/`pop` of a 32-bit register or of memory (`pushl`, `popl`, or
///   no suffix) is the 64-bit encoder's `push`/`pop` (`50+r`, `58+r`,
///   `FF /6`, `8F /0`: the operand size is the mode's default);
/// - `pushfl`, `popfl`, `retl`, `leavel`, `calll` are `pushf`, `popf`,
///   `ret`, `leave`, `call`;
/// - a relocation specifier other than `@PLT` is `asm:operand/specifier`
///   (`@ntpoff`, `@gotoff`, `@tlsgd` are PIC's and TLS's);
/// - the names of [`X64_ONLY`] are `asm:mnemonic/<name>`.
///
/// # Errors
/// As above.
pub(crate) fn statement(name: &str, ops: &mut Ops<'_>) -> Result<String, Refusal> {
    if X64_ONLY.contains(&name) {
        return Err(reason::mnemonic(name));
    }
    for i in 0..ops.len() {
        if let Some(Op::Mem(m)) = ops.get(i)
            && m.spec.is_some_and(|s| s != operand::Spec::Plt)
        {
            return Err(Refusal(reason::operand::SPECIFIER));
        }
    }
    Ok(match name {
        "push" | "pushl" | "pop" | "popl" => {
            let push = name.starts_with("push");
            let base = if push { "pushq" } else { "popq" };
            match ops.get(0) {
                Some(Op::Gpr(Gpr { reg, size: Size::D })) if ops.len() == 1 => {
                    ops.set(0, Op::Gpr(Gpr { reg, size: Size::Q }));
                    base.to_owned()
                }
                Some(Op::Mem(_)) => base.to_owned(),
                Some(Op::Imm(_)) if name == "pushl" => base.to_owned(),
                _ => name.to_owned(),
            }
        }
        "pushfl" => "pushf".to_owned(),
        "popfl" => "popf".to_owned(),
        "retl" => "ret".to_owned(),
        "leavel" => "leave".to_owned(),
        "calll" => "call".to_owned(),
        _ => name.to_owned(),
    })
}

/// Writes `arg` with `modifier` as LLVM's `X86AsmPrinter` does for
/// `i686` in AT&T syntax: [`crate::x64::print_operand`], except that a
/// general register's default view is at most 32 bits, an address is
/// in a 32-bit register (`(%eax)`), and `q` (the 64-bit view) and `h`
/// (`ah`..`dh`) are refused.
///
/// # Errors
/// `asm:modifier/<m>` for a modifier the operand does not take;
/// `asm:template` for a register `n` that is no 32-bit register or an
/// operand of 8 bytes in one register.
pub fn print_operand(arg: Arg, modifier: Option<u8>, out: &mut String) -> Result<(), Refusal> {
    let gp =
        |n: u8| hod_mc_x64::Reg::new(n).filter(|r| r.code() < 8).ok_or(Refusal(other::TEMPLATE));
    match (arg, modifier) {
        (_, Some(m @ (b'q' | b'h'))) => Err(reason::modifier(&char::from(m).to_string())),
        (Arg::Gp(n, bytes), _) if bytes > 4 || gp(n).is_err() => Err(Refusal(other::TEMPLATE)),
        (Arg::Gp(n, 1), None | Some(b'c' | b'b')) if n >= 4 => Err(Refusal(other::TEMPLATE)),
        (Arg::Gp(n, _), Some(b'b')) if n >= 4 => Err(Refusal(other::TEMPLATE)),
        (Arg::Gp(n, _), Some(b'a')) | (Arg::Mem(n), None | Some(b'b' | b'w' | b'k')) => {
            out.push_str("(%");
            out.push_str(&operand::gpr_name(gp(n)?, Size::D));
            out.push(')');
            Ok(())
        }
        (Arg::Mem(n), Some(b'H')) => {
            out.push_str("8(%");
            out.push_str(&operand::gpr_name(gp(n)?, Size::D));
            out.push(')');
            Ok(())
        }
        (Arg::Mem(_), Some(m)) => Err(reason::modifier(&char::from(m).to_string())),
        _ => crate::x64::print_operand(arg, modifier, out),
    }
}

/// The relocation of a symbol field of a 32-bit module instruction
/// (`spec/asm.md` §23), as llvm-mc 19 and 22 write it for `i686`: the
/// field is four bytes, its addend in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// `sym` (a displacement or an immediate): `R_386_32`.
    Abs32,
    /// A branch or call to `sym`: `R_386_PC32`.
    Pc32,
    /// A branch or call to `sym@PLT`: `R_386_PLT32`.
    Plt32,
    /// `sym@GOT` other than in [`RefKind::Got32X`]'s form: `R_386_GOT32`.
    Got32,
    /// `sym@GOT` as the displacement of a 32-bit `mov` load (`8B`) from a
    /// base register without an index (no SIB byte): `R_386_GOT32X`.
    Got32X,
    /// `sym@GOTOFF`: `R_386_GOTOFF`.
    GotOff,
    /// `_GLOBAL_OFFSET_TABLE_` (no specifier) in an instruction:
    /// `R_386_GOTPC`, the field's offset in the instruction added to the
    /// addend.
    GotPc,
    /// `sym@ntpoff`: `R_386_TLS_LE`.
    TlsLe,
    /// `sym@tpoff`: `R_386_TLS_LE_32`.
    TlsLe32,
    /// `sym@indntpoff`: `R_386_TLS_IE`.
    TlsIe,
    /// `sym@gottpoff`: `R_386_TLS_IE_32`.
    TlsIe32,
    /// `sym@gotntpoff`: `R_386_TLS_GOTIE`.
    TlsGotIe,
    /// `sym@tlsgd`: `R_386_TLS_GD`.
    TlsGd,
    /// `sym@tlsldm`: `R_386_TLS_LDM`.
    TlsLdm,
    /// `sym@dtpoff`: `R_386_TLS_LDO_32`.
    TlsLdo32,
}

impl RefKind {
    /// Whether the field is a thread-local one (`R_386_TLS_*`).
    pub fn is_tls(self) -> bool {
        matches!(
            self,
            Self::TlsLe
                | Self::TlsLe32
                | Self::TlsIe
                | Self::TlsIe32
                | Self::TlsGotIe
                | Self::TlsGd
                | Self::TlsLdm
                | Self::TlsLdo32
        )
    }
}

/// The name of the GOT's symbol: a field naming it is `R_386_GOTPC`.
pub const GOT_SYMBOL: &str = "_GLOBAL_OFFSET_TABLE_";

/// A symbol field of a 32-bit module instruction, at byte `at` of the
/// buffer the statement was appended to; its bytes are zero.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Field {
    pub(crate) at: usize,
    pub(crate) kind: RefKind,
    pub(crate) name: String,
    pub(crate) addend: i64,
}

/// A 32-bit module instruction statement once read.
pub(crate) enum ModuleStmt<'a> {
    /// Its bytes, appended, and its symbol field.
    Bytes(Option<Field>),
    /// A branch or call, nothing appended: the target, and the kind of a
    /// relocated field ([`RefKind::Pc32`] or [`RefKind::Plt32`]).
    Jump(crate::x64::Jump, crate::expr::Value<'a>, RefKind),
}

/// The relocation specifiers of 32-bit fields other than `@PLT`, read
/// without regard to case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spec {
    Got,
    GotOff,
    NtpOff,
    TpOff,
    GotTpOff,
    GotNtpOff,
    IndNtpOff,
    TlsGd,
    TlsLdm,
    DtpOff,
}

fn spec(word: &str) -> Option<Spec> {
    Some(match word.to_ascii_lowercase().as_str() {
        "got" => Spec::Got,
        "gotoff" => Spec::GotOff,
        "ntpoff" => Spec::NtpOff,
        "tpoff" => Spec::TpOff,
        "gottpoff" => Spec::GotTpOff,
        "gotntpoff" => Spec::GotNtpOff,
        "indntpoff" => Spec::IndNtpOff,
        "tlsgd" => Spec::TlsGd,
        "tlsldm" => Spec::TlsLdm,
        "dtpoff" => Spec::DtpOff,
        _ => return None,
    })
}

/// The immediate written in place of a symbol immediate: it takes the
/// 32-bit immediate form, whose four bytes end the instruction.
const PLACEHOLDER: u32 = 0x1234_5678;

/// `text` with its one specifier `@word` taken out (`sym@GOTOFF+4(%ebx)`
/// is `sym+4(%ebx)`), and the specifier; `None` without one.
///
/// # Errors
/// `asm:operand/specifier` for a specifier not of [`Spec`] (`@PLT` in a
/// field other than a branch's included) or a second one.
fn take_spec(text: &str) -> Result<Option<(String, Spec)>, Refusal> {
    let Some(at) = text.find('@') else { return Ok(None) };
    let after = &text[at + 1..];
    let end = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
    let spec = spec(&after[..end]).ok_or(Refusal(reason::operand::SPECIFIER))?;
    if after[end..].contains('@') {
        return Err(Refusal(reason::operand::SPECIFIER));
    }
    Ok(Some((format!("{}{}", &text[..at], &after[end..]), spec)))
}

/// What [`rewrite`] found: the operands with the symbol fields made
/// plain, the specifier of a memory field, and a symbol immediate (its
/// name, constant and specifier).
struct Rewritten {
    text: String,
    mem_spec: Option<Spec>,
    imm: Option<(String, i64, Option<Spec>)>,
}

/// The statement's operands `rest` (after a joined prefix's mnemonic,
/// if any) with a symbol immediate replaced by [`PLACEHOLDER`] and a
/// memory field's specifier taken out; `None` if there is neither.
fn rewrite(mnemonic: &str, rest: &str) -> Result<Option<Rewritten>, Refusal> {
    let (head, ops) = if crate::x64::prefix(&mnemonic.to_ascii_lowercase()).is_some() {
        crate::x64::split_mnemonic(rest)
    } else {
        ("", rest)
    };
    if ops.is_empty() {
        return Ok(None);
    }
    let mut out = Rewritten { text: String::new(), mem_spec: None, imm: None };
    if !head.is_empty() {
        out.text.push_str(head);
        out.text.push(' ');
    }
    let mut changed = false;
    for (i, part) in operand::split_operands(ops)?.into_iter().enumerate() {
        if i != 0 {
            out.text.push(',');
        }
        if let Some(imm) = part.strip_prefix('$') {
            let imm = crate::text::trim(imm);
            let (plain, spec) = match take_spec(imm)? {
                Some((plain, spec)) => (plain, Some(spec)),
                None => (imm.to_owned(), None),
            };
            let v = operand::whole_expr(crate::text::trim(&plain))?;
            match v.name {
                Some(crate::expr::Name::Sym(name)) => {
                    if out.imm.is_some() {
                        return Err(Refusal(reason::operand::SYMBOL));
                    }
                    out.imm = Some((name.to_owned(), v.constant, spec));
                    out.text.push('$');
                    out.text.push_str(&PLACEHOLDER.to_string());
                    changed = true;
                }
                _ if spec.is_some() => return Err(Refusal(reason::operand::SYMBOL)),
                _ => out.text.push_str(part),
            }
            continue;
        }
        match take_spec(part)? {
            Some((plain, spec)) => {
                if out.mem_spec.is_some() {
                    return Err(Refusal(reason::operand::SPECIFIER));
                }
                out.mem_spec = Some(spec);
                out.text.push_str(&plain);
                changed = true;
            }
            None => out.text.push_str(part),
        }
    }
    Ok(changed.then_some(out))
}

/// The kind of an absolute field with specifier `spec` (none: `sym`
/// alone) in the instruction `inst` (its bytes from its first), of
/// symbol `name`; `mem`: the field is a displacement.
fn field_kind(spec: Option<Spec>, name: &str, inst: &[u8], mem: bool) -> Result<RefKind, Refusal> {
    if name == GOT_SYMBOL {
        return match spec {
            None => Ok(RefKind::GotPc),
            Some(_) => Err(Refusal(reason::operand::SPECIFIER)),
        };
    }
    Ok(match spec {
        None => RefKind::Abs32,
        Some(Spec::Got) if mem && got32x(inst) => RefKind::Got32X,
        Some(Spec::Got) => RefKind::Got32,
        Some(Spec::GotOff) => RefKind::GotOff,
        Some(Spec::NtpOff) => RefKind::TlsLe,
        Some(Spec::TpOff) => RefKind::TlsLe32,
        Some(Spec::GotTpOff) => RefKind::TlsIe32,
        Some(Spec::GotNtpOff) => RefKind::TlsGotIe,
        Some(Spec::IndNtpOff) => RefKind::TlsIe,
        Some(Spec::TlsGd) => RefKind::TlsGd,
        Some(Spec::TlsLdm) => RefKind::TlsLdm,
        Some(Spec::DtpOff) => RefKind::TlsLdo32,
    })
}

/// Whether `inst` is the form llvm-mc writes `R_386_GOT32X` for: a
/// 32-bit `mov` load (`8B`, after any segment prefix) whose memory is a
/// base register and a 32-bit displacement, without a SIB byte.
fn got32x(inst: &[u8]) -> bool {
    let mut bytes =
        inst.iter().skip_while(|b| matches!(b, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65));
    bytes.next() == Some(&0x8b) && bytes.next().is_some_and(|m| m >> 6 == 2 && m & 7 != 4)
}

/// Reads instruction statement `mnemonic rest` of 32-bit module-level
/// assembly (`spec/asm.md` §23) and appends its bytes to `bytes`: the
/// inline assembler's statement ([`crate::x64::instruction_in`] in
/// [`Mode::X86`]) and, beyond it, a symbol immediate (`$sym`,
/// `$sym@ntpoff`) of a 32-bit immediate field and the specifiers of
/// [`RefKind`] on a displacement.
///
/// # Errors
/// The statement's `asm:*` refusal; an `x86-mode:*` name of
/// [`Mode::admit`].
pub(crate) fn module_instruction<'a>(
    mnemonic: &str,
    rest: &'a str,
    pending: &mut Option<crate::x64::Prefix>,
    bytes: &mut Vec<u8>,
) -> Result<ModuleStmt<'a>, Refusal> {
    use crate::x64::{RefKind as X, Stmt};
    let start = bytes.len();
    let saved = *pending;
    // The instruction's first byte, after the byte of a prefix statement
    // or of the `wait` of a waiting x87 form: each is an instruction of
    // its own to LLVM's assembler.
    let first = |bytes: &[u8]| {
        let at = start + usize::from(saved.is_some());
        at + usize::from(bytes.get(at) == Some(&0x9b))
    };
    let err = match crate::x64::instruction_in(mnemonic, rest, pending, bytes, Mode::X86) {
        Ok(Stmt::Jump(jump, target, kind)) => {
            let kind = if kind == X::Plt32 { RefKind::Plt32 } else { RefKind::Pc32 };
            return Ok(ModuleStmt::Jump(jump, target, kind));
        }
        Ok(Stmt::Bytes(None)) => return Ok(ModuleStmt::Bytes(None)),
        Ok(Stmt::Bytes(Some(p))) => {
            // In 32-bit mode a plain displacement is the one field here.
            if p.kind != X::Abs32S {
                return Err(Refusal(reason::operand::SPECIFIER));
            }
            let first = first(bytes);
            let kind = field_kind(None, &p.name, &bytes[first..], true)?;
            let addend =
                if kind == RefKind::GotPc { p.addend + (p.at - first) as i64 } else { p.addend };
            return Ok(ModuleStmt::Bytes(Some(Field { at: p.at, kind, name: p.name, addend })));
        }
        Err(e) => e,
    };
    if err.0 != reason::operand::SPECIFIER && err.0 != reason::operand::SYMBOL {
        return Err(err);
    }
    bytes.truncate(start);
    *pending = saved;
    let Some(new) = rewrite(mnemonic, rest)? else { return Err(err) };
    let restore = |bytes: &mut Vec<u8>, pending: &mut Option<crate::x64::Prefix>| {
        bytes.truncate(start);
        *pending = saved;
    };
    let field = match crate::x64::instruction_in(mnemonic, &new.text, pending, bytes, Mode::X86) {
        Ok(Stmt::Bytes(field)) => field,
        _ => {
            restore(bytes, pending);
            return Err(err);
        }
    };
    let inst = &bytes[first(bytes)..];
    let field = match (new.imm, field) {
        (Some((name, constant, spec)), None) => {
            let len = inst.len();
            if len < 4 || inst[len - 4..] != PLACEHOLDER.to_le_bytes() {
                restore(bytes, pending);
                return Err(err);
            }
            let kind = field_kind(spec, &name, inst, false)?;
            let addend =
                if kind == RefKind::GotPc { constant + (len - 4) as i64 } else { constant };
            let at = bytes.len() - 4;
            bytes[at..].fill(0);
            Field { at, kind, name, addend }
        }
        (None, Some(p)) if p.kind == X::Abs32S => {
            let kind = field_kind(new.mem_spec, &p.name, inst, true)?;
            Field { at: p.at, kind, name: p.name, addend: p.addend }
        }
        _ => {
            restore(bytes, pending);
            return Err(Refusal(reason::operand::SYMBOL));
        }
    };
    Ok(ModuleStmt::Bytes(Some(field)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(arg: Arg, m: Option<u8>) -> Result<String, Refusal> {
        let mut s = String::new();
        print_operand(arg, m, &mut s).map(|()| s)
    }

    #[test]
    fn operands_print_in_32_bit_views() {
        assert_eq!(printed(Arg::Gp(0, 4), None).unwrap(), "%eax");
        assert_eq!(printed(Arg::Gp(3, 4), Some(b'b')).unwrap(), "%bl");
        assert_eq!(printed(Arg::Gp(6, 4), Some(b'w')).unwrap(), "%si");
        assert_eq!(printed(Arg::Gp(1, 2), Some(b'k')).unwrap(), "%ecx");
        assert_eq!(printed(Arg::Mem(5), None).unwrap(), "(%ebp)");
        assert_eq!(printed(Arg::Gp(2, 4), Some(b'a')).unwrap(), "(%edx)");
        assert_eq!(printed(Arg::Imm(-3), Some(b'n')).unwrap(), "3");
        assert_eq!(printed(Arg::Gp(0, 4), Some(b'q')).unwrap_err().0, "asm:modifier/q");
        assert_eq!(printed(Arg::Gp(0, 4), Some(b'h')).unwrap_err().0, "asm:modifier/h");
        assert_eq!(printed(Arg::Gp(6, 4), Some(b'b')).unwrap_err().0, "asm:template");
        assert_eq!(printed(Arg::Gp(8, 4), None).unwrap_err().0, "asm:template");
    }
}
