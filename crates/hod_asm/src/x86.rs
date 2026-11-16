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
