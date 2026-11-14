//! x86-64 AT&T-syntax instructions to bytes (`spec/asm.md` §10.3), each
//! accepted form one `hod_mc_x64` constructor call. Operands are written
//! source first; the constructors take them in Intel order.
//!
//! The operation size comes from the mnemonic's suffix (`b`, `w`, `l`,
//! `q`) or, without one, from the general-register operands that carry
//! it; the two must agree, and an instruction with neither (`inc (%rax)`)
//! is `asm:operand/form`, as LLVM's assembler refuses it as ambiguous.
//! Where LLVM's assembler chooses among encodings of one written form,
//! `hod_mc_x64` makes the same choice (`spec/mc-x64.md`), and
//! `tests/llvm_mc_x64.rs` holds it to LLVM's bytes.

use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Inst, Reg, Size, Xmm};

use crate::expr::{Name, Value};
use crate::reason::{self, Refusal, operand};
use crate::x64::operand::{Base, Gpr, MemOp, Op, Ops};

/// What one instruction statement assembles to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoded<'a> {
    /// Its bytes.
    Inst(Inst),
    /// A branch to a label (`jmp` when the condition is `None`): its
    /// form, short or near, is chosen once the labels are known.
    Branch(Option<Cond>, Value<'a>),
    /// `call` of a label or symbol (`spec/asm.md` §14): always the near
    /// form, `E8` and a 32-bit displacement.
    Call(Value<'a>),
}

/// Whether a branch target has no specifier or `@PLT` (which LLVM's
/// assembler writes as the plain target's relocation, `R_X86_64_PLT32`).
fn plt_or_none(m: &MemOp<'_>) -> bool {
    matches!(m.spec, None | Some(crate::x64::operand::Spec::Plt))
}

fn err<T>(what: &'static str) -> Result<T, Refusal> {
    Err(Refusal(what))
}

/// The condition named by `name` (`e`, `nz`, `nae`, ...).
pub fn cond_name(name: &str) -> Option<Cond> {
    Some(match name {
        "o" => Cond::O,
        "no" => Cond::No,
        "b" | "c" | "nae" => Cond::B,
        "ae" | "nb" | "nc" => Cond::Ae,
        "e" | "z" => Cond::E,
        "ne" | "nz" => Cond::Ne,
        "be" | "na" => Cond::Be,
        "a" | "nbe" => Cond::A,
        "s" => Cond::S,
        "ns" => Cond::Ns,
        "p" | "pe" => Cond::P,
        "np" | "po" => Cond::Np,
        "l" | "nge" => Cond::L,
        "ge" | "nl" => Cond::Ge,
        "le" | "ng" => Cond::Le,
        "g" | "nle" => Cond::G,
        _ => return None,
    })
}

/// The memory operand of `m` for the encoder.
///
/// # Errors
/// `asm:operand/symbol` for a displacement that names a label,
/// `asm:operand/range` for one that is not a signed 32-bit value. A
/// displacement that names a symbol is a relocatable field, zero here
/// (`spec/asm.md` §10.2 "Symbols"): its relocation is the caller's.
pub fn mem(m: &MemOp<'_>) -> Result<mc::Mem, Refusal> {
    let field = match m.disp.name {
        None => false,
        Some(Name::Sym(_)) => true,
        Some(Name::Local(..)) => return err(operand::SYMBOL),
    };
    let disp = if field {
        0
    } else {
        i32::try_from(m.disp.constant).map_err(|_| Refusal(operand::RANGE))?
    };
    let index = match m.index {
        Some((r, s)) => Some((r.index().ok_or(Refusal(operand::REGISTER))?, s)),
        None => None,
    };
    let out = match (m.base, index) {
        (None, None) => mc::Mem::abs(disp),
        (Some(Base::Reg(b)), None) => mc::Mem::base(b, disp),
        (Some(Base::Reg(b)), Some((i, s))) => mc::Mem::base_index(b, i, s, disp),
        (None, Some((i, s))) => mc::Mem::index(i, s, disp),
        (Some(Base::Rip), None) => mc::Mem::rip(disp),
        (Some(Base::Rip), Some(_)) => return err(operand::FORM),
    };
    let out = match m.seg {
        Some(seg) => out.with_seg(seg),
        None => out,
    };
    if field && m.base != Some(Base::Rip) {
        return out.with_abs32().ok_or(Refusal(operand::FORM));
    }
    Ok(out)
}

fn imm(v: &Value<'_>) -> Result<i64, Refusal> {
    if v.name.is_some() {
        return err(operand::SYMBOL);
    }
    Ok(v.constant)
}

/// A shift count or bit index: an immediate 0 to 255.
fn imm8(v: &Value<'_>) -> Result<u8, Refusal> {
    u8::try_from(imm(v)?).map_err(|_| Refusal(operand::RANGE))
}

/// The operation size: the suffix, or the size of the registers in
/// `regs`, which must agree.
fn size_of(suffix: Option<Size>, regs: &[Gpr]) -> Result<Size, Refusal> {
    let mut size = suffix;
    for r in regs {
        match size {
            None => size = Some(r.size),
            Some(s) if s != r.size => return err(operand::REGISTER),
            Some(_) => {}
        }
    }
    size.ok_or(Refusal(operand::FORM))
}

fn some(inst: Option<Inst>) -> Result<Inst, Refusal> {
    inst.ok_or(Refusal(operand::RANGE))
}

/// A register or memory operand.
enum Rm {
    R(Gpr),
    M(mc::Mem),
}

fn rm(op: Op<'_>) -> Result<Rm, Refusal> {
    match op {
        Op::Gpr(g) => Ok(Rm::R(g)),
        Op::Mem(m) => Ok(Rm::M(mem(&m)?)),
        Op::Xmm(_) => err(operand::REGISTER),
        Op::Imm(_) => err(operand::FORM),
    }
}

/// The instructions without operands.
fn fixed(name: &str) -> Option<Inst> {
    Some(match name {
        "nop" => mc::nop(1),
        "pause" => mc::pause(),
        "cpuid" => mc::cpuid(),
        "rdtsc" => mc::rdtsc(),
        "rdtscp" => mc::rdtscp(),
        "xgetbv" => mc::xgetbv(),
        "mfence" => mc::mfence(),
        "lfence" => mc::lfence(),
        "sfence" => mc::sfence(),
        "int3" => mc::int3(),
        "ud2" => mc::ud2(),
        "hlt" => mc::hlt(),
        "fwait" | "wait" => mc::fwait(),
        "cld" => mc::cld(),
        "std" => mc::std(),
        "syscall" => mc::syscall(),
        "pushf" | "pushfq" => mc::pushfq(),
        "popf" | "popfq" => mc::popfq(),
        "cbtw" | "cbw" => mc::cbw(),
        "cwtl" | "cwde" => mc::cwde(),
        "cltq" | "cdqe" => mc::cdqe(),
        "cwtd" | "cwd" => mc::cwd(),
        "cltd" | "cdq" => mc::cdq(),
        "cqto" | "cqo" => mc::cqo(),
        "rdmsr" => mc::rdmsr(),
        "wrmsr" => mc::wrmsr(),
        "rdpmc" => mc::rdpmc(),
        "xsetbv" => mc::xsetbv(),
        "xend" => mc::xend(),
        "xtest" => mc::xtest(),
        "rdpkru" => mc::rdpkru(),
        "wrpkru" => mc::wrpkru(),
        "serialize" => mc::serialize(),
        "endbr64" => mc::endbr64(),
        "enclu" => mc::enclu(),
        "encls" => mc::encls(),
        "enclv" => mc::enclv(),
        "pconfig" => mc::pconfig(),
        "fnclex" => mc::fnclex(),
        "fninit" => mc::fninit(),
        "clc" => mc::clc(),
        "stc" => mc::stc(),
        "cmc" => mc::cmc(),
        "lahf" => mc::lahf(),
        "sahf" => mc::sahf(),
        "cli" => mc::cli(),
        "sti" => mc::sti(),
        _ => return None,
    })
}

type Rr = fn(Size, Reg, Reg) -> Inst;
type RmFn = fn(Size, Reg, mc::Mem) -> Inst;
type Mr = fn(Size, mc::Mem, Reg) -> Inst;
type Ri = fn(Size, Reg, i64) -> Option<Inst>;
type Mi = fn(Size, mc::Mem, i64) -> Option<Inst>;

/// The two-operand ALU instructions: `rr`, `rm`, `mr`, `ri`, `mi`.
fn alu(name: &str) -> Option<(Rr, RmFn, Mr, Ri, Mi)> {
    Some(match name {
        "add" => (mc::add_rr, mc::add_rm, mc::add_mr, mc::try_add_ri, mc::try_add_mi),
        "or" => (mc::or_rr, mc::or_rm, mc::or_mr, mc::try_or_ri, mc::try_or_mi),
        "adc" => (mc::adc_rr, mc::adc_rm, mc::adc_mr, mc::try_adc_ri, mc::try_adc_mi),
        "sbb" => (mc::sbb_rr, mc::sbb_rm, mc::sbb_mr, mc::try_sbb_ri, mc::try_sbb_mi),
        "and" => (mc::and_rr, mc::and_rm, mc::and_mr, mc::try_and_ri, mc::try_and_mi),
        "sub" => (mc::sub_rr, mc::sub_rm, mc::sub_mr, mc::try_sub_ri, mc::try_sub_mi),
        "xor" => (mc::xor_rr, mc::xor_rm, mc::xor_mr, mc::try_xor_ri, mc::try_xor_mi),
        "cmp" => (mc::cmp_rr, mc::cmp_rm, mc::cmp_mr, mc::try_cmp_ri, mc::try_cmp_mi),
        _ => return None,
    })
}

type R1 = fn(Size, Reg) -> Inst;
type M1 = fn(Size, mc::Mem) -> Inst;

/// The one-operand group instructions.
fn unary(name: &str) -> Option<(R1, M1)> {
    Some(match name {
        "inc" => (mc::inc_r, mc::inc_m),
        "dec" => (mc::dec_r, mc::dec_m),
        "not" => (mc::not_r, mc::not_m),
        "neg" => (mc::neg_r, mc::neg_m),
        "mul" => (mc::mul_r, mc::mul_m),
        "div" => (mc::div_r, mc::div_m),
        "idiv" => (mc::idiv_r, mc::idiv_m),
        _ => return None,
    })
}

type Sri = fn(Size, Reg, u8) -> Inst;
type Smi = fn(Size, mc::Mem, u8) -> Inst;

/// The shifts and rotates: by an immediate, by `cl`.
fn shift(name: &str) -> Option<(Sri, Smi, R1, M1)> {
    Some(match name {
        "rol" => (mc::rol_ri, mc::rol_mi, mc::rol_rcl, mc::rol_mcl),
        "ror" => (mc::ror_ri, mc::ror_mi, mc::ror_rcl, mc::ror_mcl),
        "rcl" => (mc::rcl_ri, mc::rcl_mi, mc::rcl_rcl, mc::rcl_mcl),
        "rcr" => (mc::rcr_ri, mc::rcr_mi, mc::rcr_rcl, mc::rcr_mcl),
        "shl" | "sal" => (mc::shl_ri, mc::shl_mi, mc::shl_rcl, mc::shl_mcl),
        "shr" => (mc::shr_ri, mc::shr_mi, mc::shr_rcl, mc::shr_mcl),
        "sar" => (mc::sar_ri, mc::sar_mi, mc::sar_rcl, mc::sar_mcl),
        _ => return None,
    })
}

type CRr = fn(Size, Reg, Reg) -> Option<Inst>;
type CRm = fn(Size, Reg, mc::Mem) -> Option<Inst>;

/// The bit scans and counts: `rr`, `rm` (no byte form).
fn count(name: &str) -> Option<(CRr, CRm)> {
    Some(match name {
        "bsf" => (mc::try_bsf_rr, mc::try_bsf_rm),
        "bsr" => (mc::try_bsr_rr, mc::try_bsr_rm),
        "tzcnt" => (mc::try_tzcnt_rr, mc::try_tzcnt_rm),
        "lzcnt" => (mc::try_lzcnt_rr, mc::try_lzcnt_rm),
        "popcnt" => (mc::try_popcnt_rr, mc::try_popcnt_rm),
        _ => return None,
    })
}

type BRr = fn(Size, Reg, Reg) -> Option<Inst>;
type BMr = fn(Size, mc::Mem, Reg) -> Option<Inst>;
type BRi = fn(Size, Reg, u8) -> Option<Inst>;
type BMi = fn(Size, mc::Mem, u8) -> Option<Inst>;

/// The bit tests.
fn bit(name: &str) -> Option<(BRr, BMr, BRi, BMi)> {
    Some(match name {
        "bt" => (mc::try_bt_rr, mc::try_bt_mr, mc::try_bt_ri, mc::try_bt_mi),
        "bts" => (mc::try_bts_rr, mc::try_bts_mr, mc::try_bts_ri, mc::try_bts_mi),
        "btr" => (mc::try_btr_rr, mc::try_btr_mr, mc::try_btr_ri, mc::try_btr_mi),
        "btc" => (mc::try_btc_rr, mc::try_btc_mr, mc::try_btc_ri, mc::try_btc_mi),
        _ => return None,
    })
}

/// The AT&T zero and sign extensions with both sizes in the name:
/// `movzbl` is (`movzx`, from `b`, to `l`).
fn extension(name: &str) -> Option<(bool, Size, Size)> {
    let rest = name.strip_prefix("mov")?;
    let (signed, rest) = match rest.as_bytes().first()? {
        b'z' => (false, &rest[1..]),
        b's' => (true, &rest[1..]),
        _ => return None,
    };
    let size = |c: u8| match c {
        b'b' => Some(Size::B),
        b'w' => Some(Size::W),
        b'l' => Some(Size::D),
        b'q' => Some(Size::Q),
        _ => None,
    };
    let [from, to] = rest.as_bytes() else { return None };
    Some((signed, size(*from)?, size(*to)?))
}

/// The suffix letter's size.
fn suffix_size(c: u8) -> Option<Size> {
    match c {
        b'b' => Some(Size::B),
        b'w' => Some(Size::W),
        b'l' => Some(Size::D),
        b'q' => Some(Size::Q),
        _ => None,
    }
}

/// Assembles one instruction statement: `mnemonic` (any case) and its
/// operands.
///
/// # Errors
/// `asm:mnemonic/<name>` for an instruction not covered,
/// `asm:operand/<what>` for operands outside its forms.
pub fn encode<'a>(mnemonic: &str, ops: &Ops<'a>) -> Result<Encoded<'a>, Refusal> {
    let name = mnemonic.to_ascii_lowercase();
    if let Some(found) = sse(&name, ops) {
        return found.map(Encoded::Inst);
    }
    // `nop` with an operand is the multi-byte no-op, a family member.
    if let Some(inst) = fixed(&name).filter(|_| ops.is_empty() || name != "nop") {
        return if ops.is_empty() { Ok(Encoded::Inst(inst)) } else { err(operand::FORM) };
    }
    if let Some(inst) = string_op(&name) {
        return if ops.is_empty() { Ok(Encoded::Inst(inst)) } else { err(operand::FORM) };
    }
    if let Some(found) = family(&name, None, ops) {
        return found;
    }
    if let Some(&last) = name.as_bytes().last()
        && let Some(size) = suffix_size(last)
        && let Some(found) = family(&name[..name.len() - 1], Some(size), ops)
    {
        return found;
    }
    Err(reason::mnemonic(&name))
}

/// A string instruction without operands (`movsb`, `lodsq`, `cmpsl`;
/// `movsd` is `movsl`), as `rep` takes it: `None` for a name that is
/// none of them. `stosd`, `lodsd`, `scasd` and `cmpsd` are not names of
/// them to LLVM's assembler.
pub fn string_op(name: &str) -> Option<Inst> {
    if name == "movsd" {
        return Some(mc::string_op(mc::StringOp::Movs, Size::D));
    }
    let (&last, stem) = name.as_bytes().split_last()?;
    let op = mc::StringOp::ALL.into_iter().find(|op| op.stem().as_bytes() == stem)?;
    Some(mc::string_op(op, suffix_size(last)?))
}

type XRr = fn(FpSize, Xmm, Xmm) -> Inst;
type XRm = fn(FpSize, Xmm, mc::Mem) -> Inst;

/// The SSE instructions of two operands, source first, whose destination
/// is a register: arithmetic and logic.
fn sse_arith(name: &str) -> Option<(XRr, XRm, FpSize)> {
    let (stem, fp) = match name.as_bytes().last()? {
        b's' => (&name[..name.len() - 1], FpSize::S),
        b'd' => (&name[..name.len() - 1], FpSize::D),
        _ => return None,
    };
    let ops: (XRr, XRm) = match stem {
        "adds" => (mc::adds_rr, mc::adds_rm),
        "subs" => (mc::subs_rr, mc::subs_rm),
        "muls" => (mc::muls_rr, mc::muls_rm),
        "divs" => (mc::divs_rr, mc::divs_rm),
        "sqrts" => (mc::sqrts_rr, mc::sqrts_rm),
        "mins" => (mc::mins_rr, mc::mins_rm),
        "maxs" => (mc::maxs_rr, mc::maxs_rm),
        "andp" => (mc::andp_rr, mc::andp_rm),
        "andnp" => (mc::andnp_rr, mc::andnp_rm),
        "orp" => (mc::orp_rr, mc::orp_rm),
        "xorp" => (mc::xorp_rr, mc::xorp_rm),
        "ucomis" => (mc::ucomis_rr, mc::ucomis_rm),
        "comis" => (mc::comis_rr, mc::comis_rm),
        "addp" => (mc::addp_rr, mc::addp_rm),
        "subp" => (mc::subp_rr, mc::subp_rm),
        "mulp" => (mc::mulp_rr, mc::mulp_rm),
        "divp" => (mc::divp_rr, mc::divp_rm),
        "sqrtp" => (mc::sqrtp_rr, mc::sqrtp_rm),
        "minp" => (mc::minp_rr, mc::minp_rm),
        "maxp" => (mc::maxp_rr, mc::maxp_rm),
        "unpcklp" => (mc::unpcklp_rr, mc::unpcklp_rm),
        "unpckhp" => (mc::unpckhp_rr, mc::unpckhp_rm),
        _ => return None,
    };
    Some((ops.0, ops.1, fp))
}

/// The SSE instructions (`spec/asm.md` §10.3): `None` for a name that is
/// none of them, or `movq`/`movsd` without an SSE register (the general
/// `movq`, the string `movsd`).
#[allow(clippy::many_single_char_names, reason = "operands a, b; source s, destination d")]
fn sse(name: &str, ops: &Ops<'_>) -> Option<Result<Inst, Refusal>> {
    let (n, a, b) = (ops.len(), ops.get(0), ops.get(1));
    let any_xmm = matches!(a, Some(Op::Xmm(_))) || matches!(b, Some(Op::Xmm(_)));
    if let Some((rr, rmf, fp)) = sse_arith(name) {
        return Some(match (n, a, b) {
            (2, Some(Op::Xmm(s)), Some(Op::Xmm(d))) => Ok(rr(fp, d, s)),
            (2, Some(Op::Mem(m)), Some(Op::Xmm(d))) => mem(&m).map(|m| rmf(fp, d, m)),
            _ => err(operand::FORM),
        });
    }
    let moves =
        |rr: fn(Xmm, Xmm) -> Inst, rmf: fn(Xmm, mc::Mem) -> Inst, mr: fn(mc::Mem, Xmm) -> Inst| {
            Some(match (n, a, b) {
                (2, Some(Op::Xmm(s)), Some(Op::Xmm(d))) => Ok(rr(d, s)),
                (2, Some(Op::Mem(m)), Some(Op::Xmm(d))) => mem(&m).map(|m| rmf(d, m)),
                (2, Some(Op::Xmm(s)), Some(Op::Mem(m))) => mem(&m).map(|m| mr(m, s)),
                _ => err(operand::FORM),
            })
        };
    match name {
        "movss" | "movsd" if any_xmm || name == "movss" => {
            let fp = if name == "movss" { FpSize::S } else { FpSize::D };
            Some(match (n, a, b) {
                (2, Some(Op::Xmm(s)), Some(Op::Xmm(d))) => Ok(mc::movs_rr(fp, d, s)),
                (2, Some(Op::Mem(m)), Some(Op::Xmm(d))) => mem(&m).map(|m| mc::movs_rm(fp, d, m)),
                (2, Some(Op::Xmm(s)), Some(Op::Mem(m))) => mem(&m).map(|m| mc::movs_mr(fp, m, s)),
                _ => err(operand::FORM),
            })
        }
        "movaps" => moves(
            |d, s| mc::movap_rr(FpSize::S, d, s),
            |d, m| mc::movap_rm(FpSize::S, d, m),
            |m, s| mc::movap_mr(FpSize::S, m, s),
        ),
        "movapd" => moves(
            |d, s| mc::movap_rr(FpSize::D, d, s),
            |d, m| mc::movap_rm(FpSize::D, d, m),
            |m, s| mc::movap_mr(FpSize::D, m, s),
        ),
        "movups" => moves(
            |d, s| mc::movup_rr(FpSize::S, d, s),
            |d, m| mc::movup_rm(FpSize::S, d, m),
            |m, s| mc::movup_mr(FpSize::S, m, s),
        ),
        "movupd" => moves(
            |d, s| mc::movup_rr(FpSize::D, d, s),
            |d, m| mc::movup_rm(FpSize::D, d, m),
            |m, s| mc::movup_mr(FpSize::D, m, s),
        ),
        "movdqa" => moves(mc::movdqa_rr, mc::movdqa_rm, mc::movdqa_mr),
        "movdqu" => moves(mc::movdqu_rr, mc::movdqu_rm, mc::movdqu_mr),
        "movd" | "movq" if any_xmm => {
            let size = if name == "movd" { Size::D } else { Size::Q };
            Some(match (n, a, b) {
                (2, Some(Op::Xmm(s)), Some(Op::Xmm(d))) if size == Size::Q => Ok(mc::movq_rr(d, s)),
                (2, Some(Op::Gpr(s)), Some(Op::Xmm(d))) if s.size == size => {
                    Ok(mc::movd_xr(size, d, s.reg))
                }
                (2, Some(Op::Xmm(s)), Some(Op::Gpr(d))) if d.size == size => {
                    Ok(mc::movd_rx(size, d.reg, s))
                }
                (2, Some(Op::Gpr(_)), _) | (2, _, Some(Op::Gpr(_))) => err(operand::REGISTER),
                (2, Some(Op::Mem(m)), Some(Op::Xmm(d))) => mem(&m).map(|m| mc::movd_rm(size, d, m)),
                (2, Some(Op::Xmm(s)), Some(Op::Mem(m))) => mem(&m).map(|m| mc::movd_mr(size, m, s)),
                _ => err(operand::FORM),
            })
        }
        "cvtss2sd" | "cvtsd2ss" => {
            let up = name == "cvtss2sd";
            Some(match (n, a, b) {
                (2, Some(Op::Xmm(s)), Some(Op::Xmm(d))) => {
                    Ok(if up { mc::cvtss2sd_rr(d, s) } else { mc::cvtsd2ss_rr(d, s) })
                }
                (2, Some(Op::Mem(m)), Some(Op::Xmm(d))) => {
                    mem(&m).map(|m| if up { mc::cvtss2sd_rm(d, m) } else { mc::cvtsd2ss_rm(d, m) })
                }
                _ => err(operand::FORM),
            })
        }
        _ => None,
    }
}

/// The instruction families: `None` for a name that is none of them.
#[allow(clippy::too_many_lines, reason = "one arm per family, in the order of spec/asm.md §10.3")]
#[allow(clippy::many_single_char_names, reason = "operands a, b, c; source s, destination d")]
fn family<'a>(
    name: &str,
    sfx: Option<Size>,
    ops: &Ops<'a>,
) -> Option<Result<Encoded<'a>, Refusal>> {
    let n = ops.len();
    let a = ops.get(0);
    let b = ops.get(1);
    let c = ops.get(2);
    let inst = |r: Result<Inst, Refusal>| Some(r.map(Encoded::Inst));
    if let Some((rr, rmf, mr, ri, mi)) = alu(name) {
        return inst((|| match (n, a, b) {
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => Ok(rr(size_of(sfx, &[s, d])?, d.reg, s.reg)),
            (2, Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
                Ok(rmf(size_of(sfx, &[d])?, d.reg, mem(&m)?))
            }
            (2, Some(Op::Gpr(s)), Some(Op::Mem(m))) => Ok(mr(size_of(sfx, &[s])?, mem(&m)?, s.reg)),
            (2, Some(Op::Imm(i)), Some(Op::Gpr(d))) => {
                some(ri(size_of(sfx, &[d])?, d.reg, imm(&i)?))
            }
            (2, Some(Op::Imm(i)), Some(Op::Mem(m))) => {
                some(mi(size_of(sfx, &[])?, mem(&m)?, imm(&i)?))
            }
            _ => err(operand::FORM),
        })());
    }
    if name == "test" {
        return inst((|| match (n, a, b) {
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
                Ok(mc::test_rr(size_of(sfx, &[s, d])?, d.reg, s.reg))
            }
            (2, Some(Op::Gpr(r)), Some(Op::Mem(m))) | (2, Some(Op::Mem(m)), Some(Op::Gpr(r))) => {
                Ok(mc::test_mr(size_of(sfx, &[r])?, mem(&m)?, r.reg))
            }
            (2, Some(Op::Imm(i)), Some(Op::Gpr(d))) => {
                some(mc::try_test_ri(size_of(sfx, &[d])?, d.reg, imm(&i)?))
            }
            (2, Some(Op::Imm(i)), Some(Op::Mem(m))) => {
                some(mc::try_test_mi(size_of(sfx, &[])?, mem(&m)?, imm(&i)?))
            }
            _ => err(operand::FORM),
        })());
    }
    if let Some((r1, m1)) = unary(name) {
        return inst((|| match (n, a) {
            (1, Some(Op::Gpr(r))) => Ok(r1(size_of(sfx, &[r])?, r.reg)),
            (1, Some(Op::Mem(m))) => Ok(m1(size_of(sfx, &[])?, mem(&m)?)),
            _ => err(operand::FORM),
        })());
    }
    if name == "imul" {
        return inst((|| match (n, a, b, c) {
            (1, Some(Op::Gpr(r)), ..) => Ok(mc::imul_r(size_of(sfx, &[r])?, r.reg)),
            (1, Some(Op::Mem(m)), ..) => Ok(mc::imul_m(size_of(sfx, &[])?, mem(&m)?)),
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d)), _) => {
                some(mc::try_imul_rr(size_of(sfx, &[s, d])?, d.reg, s.reg))
            }
            (2, Some(Op::Mem(m)), Some(Op::Gpr(d)), _) => {
                some(mc::try_imul_rm(size_of(sfx, &[d])?, d.reg, mem(&m)?))
            }
            (2, Some(Op::Imm(i)), Some(Op::Gpr(d)), _) => {
                some(mc::try_imul_rri(size_of(sfx, &[d])?, d.reg, d.reg, imm(&i)?))
            }
            (3, Some(Op::Imm(i)), Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
                some(mc::try_imul_rri(size_of(sfx, &[s, d])?, d.reg, s.reg, imm(&i)?))
            }
            (3, Some(Op::Imm(i)), Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
                some(mc::try_imul_rmi(size_of(sfx, &[d])?, d.reg, mem(&m)?, imm(&i)?))
            }
            _ => err(operand::FORM),
        })());
    }
    if let Some((ri, mi, rcl, mcl)) = shift(name) {
        return inst((|| {
            let is_cl = |op: Option<Op<'_>>| {
                matches!(op, Some(Op::Gpr(Gpr { reg: Reg::Rcx, size: Size::B })))
            };
            match (n, a, b) {
                (1, Some(Op::Gpr(r)), _) => Ok(ri(size_of(sfx, &[r])?, r.reg, 1)),
                (1, Some(Op::Mem(m)), _) => Ok(mi(size_of(sfx, &[])?, mem(&m)?, 1)),
                (2, Some(Op::Imm(i)), Some(Op::Gpr(r))) => {
                    Ok(ri(size_of(sfx, &[r])?, r.reg, imm8(&i)?))
                }
                (2, Some(Op::Imm(i)), Some(Op::Mem(m))) => {
                    Ok(mi(size_of(sfx, &[])?, mem(&m)?, imm8(&i)?))
                }
                (2, _, Some(Op::Gpr(r))) if is_cl(a) => Ok(rcl(size_of(sfx, &[r])?, r.reg)),
                (2, _, Some(Op::Mem(m))) if is_cl(a) => Ok(mcl(size_of(sfx, &[])?, mem(&m)?)),
                _ => err(operand::FORM),
            }
        })());
    }
    if name == "shld" || name == "shrd" {
        let left = name == "shld";
        return inst((|| {
            let is_cl = matches!(a, Some(Op::Gpr(Gpr { reg: Reg::Rcx, size: Size::B })));
            let (Some(Op::Gpr(s)), Some(dst), 3) = (b, c, n) else { return err(operand::FORM) };
            match (a, rm(dst)?) {
                (Some(Op::Imm(i)), Rm::R(d)) => {
                    let size = size_of(sfx, &[s, d])?;
                    let f = if left { mc::try_shld_rri } else { mc::try_shrd_rri };
                    some(f(size, d.reg, s.reg, imm8(&i)?))
                }
                (Some(Op::Imm(i)), Rm::M(m)) => {
                    let f = if left { mc::try_shld_mri } else { mc::try_shrd_mri };
                    some(f(size_of(sfx, &[s])?, m, s.reg, imm8(&i)?))
                }
                (_, Rm::R(d)) if is_cl => {
                    let f = if left { mc::try_shld_rrcl } else { mc::try_shrd_rrcl };
                    some(f(size_of(sfx, &[s, d])?, d.reg, s.reg))
                }
                (_, Rm::M(m)) if is_cl => {
                    let f = if left { mc::try_shld_mrcl } else { mc::try_shrd_mrcl };
                    some(f(size_of(sfx, &[s])?, m, s.reg))
                }
                _ => err(operand::FORM),
            }
        })());
    }
    if let Some((rr, rmf)) = count(name) {
        return inst((|| match (n, a, b) {
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
                some(rr(size_of(sfx, &[s, d])?, d.reg, s.reg))
            }
            (2, Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
                some(rmf(size_of(sfx, &[d])?, d.reg, mem(&m)?))
            }
            _ => err(operand::FORM),
        })());
    }
    if let Some((rr, mr, ri, mi)) = bit(name) {
        return inst((|| match (n, a, b) {
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
                some(rr(size_of(sfx, &[s, d])?, d.reg, s.reg))
            }
            (2, Some(Op::Gpr(s)), Some(Op::Mem(m))) => {
                some(mr(size_of(sfx, &[s])?, mem(&m)?, s.reg))
            }
            (2, Some(Op::Imm(i)), Some(Op::Gpr(d))) => {
                some(ri(size_of(sfx, &[d])?, d.reg, imm8(&i)?))
            }
            (2, Some(Op::Imm(i)), Some(Op::Mem(m))) => {
                some(mi(size_of(sfx, &[])?, mem(&m)?, imm8(&i)?))
            }
            _ => err(operand::FORM),
        })());
    }
    match name {
        "mov" => return inst(mov(sfx, ops)),
        "movabs" => {
            return inst((|| match (n, a, b) {
                (2, Some(Op::Imm(i)), Some(Op::Gpr(d))) if size_of(sfx, &[d])? == Size::Q => {
                    Ok(mc::movabs_ri(d.reg, imm(&i)?))
                }
                _ => err(operand::FORM),
            })());
        }
        "lea" => {
            return inst((|| match (n, a, b) {
                (2, Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
                    some(mc::try_lea(size_of(sfx, &[d])?, d.reg, mem(&m)?))
                }
                _ => err(operand::FORM),
            })());
        }
        "xchg" => {
            return inst((|| match (n, a, b) {
                (2, Some(Op::Gpr(x)), Some(Op::Gpr(y))) => {
                    Ok(mc::xchg_rr(size_of(sfx, &[x, y])?, y.reg, x.reg))
                }
                (2, Some(Op::Gpr(r)), Some(Op::Mem(m)))
                | (2, Some(Op::Mem(m)), Some(Op::Gpr(r))) => {
                    Ok(mc::xchg_mr(size_of(sfx, &[r])?, mem(&m)?, r.reg))
                }
                _ => err(operand::FORM),
            })());
        }
        "xadd" | "cmpxchg" => {
            let f = if name == "xadd" { mc::xadd_mr } else { mc::cmpxchg_mr };
            return inst((|| match (n, a, b) {
                (2, Some(Op::Gpr(r)), Some(Op::Mem(m))) => {
                    Ok(f(size_of(sfx, &[r])?, mem(&m)?, r.reg))
                }
                _ => err(operand::FORM),
            })());
        }
        "cmpxchg8b" | "cmpxchg16b" if sfx.is_none() => {
            let f = if name == "cmpxchg8b" { mc::cmpxchg8b_m } else { mc::cmpxchg16b_m };
            return inst((|| match (n, a) {
                (1, Some(Op::Mem(m))) => Ok(f(mem(&m)?)),
                _ => err(operand::FORM),
            })());
        }
        "bswap" => {
            return inst((|| match (n, a) {
                (1, Some(Op::Gpr(r))) => some(mc::try_bswap(size_of(sfx, &[r])?, r.reg)),
                _ => err(operand::FORM),
            })());
        }
        "push" | "pop" => {
            let push = name == "push";
            return inst((|| match (n, a) {
                (1, Some(Op::Gpr(r))) if size_of(sfx, &[r])? == Size::Q => {
                    Ok(if push { mc::push_r(r.reg) } else { mc::pop_r(r.reg) })
                }
                (1, Some(Op::Mem(m))) if sfx == Some(Size::Q) => {
                    let m = mem(&m)?;
                    Ok(if push { mc::push_m(m) } else { mc::pop_m(m) })
                }
                (1, Some(Op::Imm(i))) if push && matches!(sfx, None | Some(Size::Q)) => {
                    let v = i32::try_from(imm(&i)?).map_err(|_| Refusal(operand::RANGE))?;
                    Ok(mc::push_i(v))
                }
                _ => err(operand::FORM),
            })());
        }
        "int" if sfx.is_none() => {
            return inst((|| match (n, a) {
                (1, Some(Op::Imm(i))) => Ok(mc::int_i(imm8(&i)?)),
                _ => err(operand::FORM),
            })());
        }
        "fnstcw" | "fldcw" if sfx.is_none() => {
            return inst((|| match (n, a) {
                (1, Some(Op::Mem(m))) => {
                    let m = mem(&m)?;
                    Ok(if name == "fnstcw" { mc::fnstcw_m(m) } else { mc::fldcw_m(m) })
                }
                _ => err(operand::FORM),
            })());
        }
        "jmp" if sfx.is_none() => {
            return Some(match (n, a) {
                (1, Some(Op::Mem(m))) if m.bare && plt_or_none(&m) => {
                    Ok(Encoded::Branch(None, m.disp))
                }
                _ => err(operand::FORM),
            });
        }
        // §14: `call sym`, `callq sym`, `call sym@PLT`.
        "call" if matches!(sfx, None | Some(Size::Q)) => {
            return Some(match (n, a) {
                (1, Some(Op::Mem(m))) if m.bare && plt_or_none(&m) => Ok(Encoded::Call(m.disp)),
                _ => err(operand::FORM),
            });
        }
        _ => {}
    }
    if let Some((signed, from, to)) = extension(name).filter(|_| sfx.is_none()) {
        return inst(extend(signed, Some((from, to)), ops));
    }
    if (name == "movzx" || name == "movsx") && sfx.is_none() {
        return inst(extend(name == "movsx", None, ops));
    }
    if sfx.is_none()
        && let Some(cond) = name.strip_prefix("set").and_then(cond_name)
    {
        return inst((|| match (n, a) {
            (1, Some(Op::Gpr(r))) if r.size == Size::B => Ok(mc::setcc_r(cond, r.reg)),
            (1, Some(Op::Gpr(_))) => err(operand::REGISTER),
            (1, Some(Op::Mem(m))) => Ok(mc::setcc_m(cond, mem(&m)?)),
            _ => err(operand::FORM),
        })());
    }
    if let Some(cond) = name.strip_prefix("cmov").and_then(cond_name) {
        return inst((|| match (n, a, b) {
            (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
                some(mc::try_cmovcc_rr(cond, size_of(sfx, &[s, d])?, d.reg, s.reg))
            }
            (2, Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
                some(mc::try_cmovcc_rm(cond, size_of(sfx, &[d])?, d.reg, mem(&m)?))
            }
            _ => err(operand::FORM),
        })());
    }
    if sfx.is_none()
        && let Some(cond) = name.strip_prefix('j').and_then(cond_name)
    {
        return Some(match (n, a) {
            (1, Some(Op::Mem(m))) if m.bare && plt_or_none(&m) => {
                Ok(Encoded::Branch(Some(cond), m.disp))
            }
            _ => err(operand::FORM),
        });
    }
    inline(name, sfx, ops).map(|r| r.map(Encoded::Inst))
}

/// The instructions only inline assembly writes (`spec/asm.md` §13):
/// `None` for a name that is none of them.
#[allow(clippy::many_single_char_names, reason = "operands a, b; source s, destination d")]
fn inline(name: &str, sfx: Option<Size>, ops: &Ops<'_>) -> Option<Result<Inst, Refusal>> {
    let (n, a, b) = (ops.len(), ops.get(0), ops.get(1));
    let one_mem = || match (n, a) {
        (1, Some(Op::Mem(m))) => mem(&m),
        _ => err(operand::FORM),
    };
    if sfx.is_none() {
        if let Some(op) = mc::Prefetch::ALL.into_iter().find(|p| p.mnemonic() == name) {
            return Some(one_mem().map(|m| mc::prefetch_m(op, m)));
        }
        if let Some(op) = mc::XState::ALL.into_iter().find(|x| x.mnemonic() == name) {
            return Some(one_mem().map(|m| mc::xstate_m(op, m)));
        }
        let mem_form: Option<fn(mc::Mem) -> Inst> = match name {
            "clflush" => Some(mc::clflush_m),
            "clflushopt" => Some(mc::clflushopt_m),
            "clwb" => Some(mc::clwb_m),
            "stmxcsr" => Some(mc::stmxcsr_m),
            "ldmxcsr" => Some(mc::ldmxcsr_m),
            "fnstenv" => Some(mc::fnstenv_m),
            "fldenv" => Some(mc::fldenv_m),
            "fnsave" => Some(mc::fnsave_m),
            "frstor" => Some(mc::frstor_m),
            _ => None,
        };
        if let Some(f) = mem_form {
            return Some(one_mem().map(f));
        }
    }
    let bare = sfx.is_none();
    Some(match (name, n, a, b) {
        ("fnstsw", 0, ..) if bare => Ok(mc::fnstsw_ax()),
        ("fnstsw", 1, Some(Op::Gpr(r)), _) if bare => {
            if r.reg == Reg::Rax && r.size == Size::W {
                Ok(mc::fnstsw_ax())
            } else {
                err(operand::REGISTER)
            }
        }
        ("fnstsw", 1, Some(Op::Mem(m)), _) if bare => mem(&m).map(mc::fnstsw_m),
        ("nop", 1, Some(Op::Gpr(r)), _) => {
            size_of(sfx, &[r]).and_then(|size| some(mc::try_nop_r(size, r.reg)))
        }
        ("nop", 1, Some(Op::Mem(m)), _) => {
            size_of(sfx, &[]).and_then(|size| some(mc::try_nop_m(size, mem(&m)?)))
        }
        ("hreset", 1, Some(Op::Imm(i)), _) if bare => imm8(&i).map(mc::hreset),
        ("in" | "out", 2, Some(x), Some(y)) => port_io(name == "in", sfx, x, y),
        ("crc32", 2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => (|| {
            let size = size_of(sfx, &[s])?;
            match (size, d.size) {
                (Size::B, Size::Q) => Ok(mc::crc32_q_rb(d.reg, s.reg)),
                (Size::Q, Size::Q) | (Size::B | Size::W | Size::D, Size::D) => {
                    Ok(mc::crc32_rr(size, d.reg, s.reg))
                }
                _ => err(operand::REGISTER),
            }
        })(),
        ("rdrand" | "rdseed", 1, Some(Op::Gpr(r)), _) => (|| {
            let size = size_of(sfx, &[r])?;
            let f = if name == "rdrand" { mc::try_rdrand_r } else { mc::try_rdseed_r };
            f(size, r.reg).ok_or(Refusal(operand::REGISTER))
        })(),
        ("rdpid", 1, Some(Op::Gpr(r)), _) if bare => {
            if r.size == Size::Q {
                Ok(mc::rdpid_r(r.reg))
            } else {
                err(operand::REGISTER)
            }
        }
        ("fnstsw" | "hreset" | "rdpid", ..) if bare => err(operand::FORM),
        ("nop" | "in" | "out" | "crc32" | "rdrand" | "rdseed", ..) => err(operand::FORM),
        _ => {
            let op = mc::SegBase::ALL.into_iter().find(|op| op.mnemonic() == name)?;
            match (n, a) {
                (1, Some(Op::Gpr(r))) => size_of(sfx, &[r]).and_then(|size| {
                    mc::try_segbase_r(op, size, r.reg).ok_or(Refusal(operand::REGISTER))
                }),
                _ => err(operand::FORM),
            }
        }
    })
}

/// `in` (`input`) from `%dx` or an 8-bit port `x` into the accumulator
/// `y`, or `out` of the accumulator `x` to `%dx` or a port `y`.
fn port_io(input: bool, sfx: Option<Size>, x: Op<'_>, y: Op<'_>) -> Result<Inst, Refusal> {
    let (acc, port) = if input { (y, x) } else { (x, y) };
    let Op::Gpr(acc) = acc else { return err(operand::FORM) };
    if acc.reg != Reg::Rax {
        return err(operand::REGISTER);
    }
    let size = size_of(sfx, &[acc])?;
    let inst = match port {
        Op::Gpr(d) if d.reg == Reg::Rdx && d.size == Size::W => {
            if input {
                mc::try_in_dx(size)
            } else {
                mc::try_out_dx(size)
            }
        }
        Op::Imm(i) => {
            let p = imm8(&i)?;
            if input { mc::try_in_i(size, p) } else { mc::try_out_i(size, p) }
        }
        Op::Gpr(_) => return err(operand::REGISTER),
        _ => return err(operand::FORM),
    };
    some(inst)
}

/// `mov` in its register, memory and immediate forms.
fn mov(sfx: Option<Size>, ops: &Ops<'_>) -> Result<Inst, Refusal> {
    match (ops.len(), ops.get(0), ops.get(1)) {
        (2, Some(Op::Gpr(s)), Some(Op::Gpr(d))) => {
            Ok(mc::mov_rr(size_of(sfx, &[s, d])?, d.reg, s.reg))
        }
        (2, Some(Op::Mem(m)), Some(Op::Gpr(d))) => {
            Ok(mc::mov_rm(size_of(sfx, &[d])?, d.reg, mem(&m)?))
        }
        (2, Some(Op::Gpr(s)), Some(Op::Mem(m))) => {
            Ok(mc::mov_mr(size_of(sfx, &[s])?, mem(&m)?, s.reg))
        }
        (2, Some(Op::Imm(i)), Some(Op::Gpr(d))) => {
            some(mc::try_mov_ri(size_of(sfx, &[d])?, d.reg, imm(&i)?))
        }
        (2, Some(Op::Imm(i)), Some(Op::Mem(m))) => {
            some(mc::try_mov_mi(size_of(sfx, &[])?, mem(&m)?, imm(&i)?))
        }
        _ => err(operand::FORM),
    }
}

/// `movzx`/`movsx` and their AT&T spellings: `sizes` is (from, to) when
/// the name gives them.
fn extend(signed: bool, sizes: Option<(Size, Size)>, ops: &Ops<'_>) -> Result<Inst, Refusal> {
    let (2, Some(src), Some(Op::Gpr(d))) = (ops.len(), ops.get(0), ops.get(1)) else {
        return err(operand::FORM);
    };
    let (from, to) = match (sizes, src) {
        (Some((from, to)), Op::Gpr(s)) if s.size == from && d.size == to => (from, to),
        (Some((from, to)), Op::Mem(_)) if d.size == to => (from, to),
        (None, Op::Gpr(s)) => (s.size, d.size),
        (Some(_), Op::Gpr(_) | Op::Mem(_)) => return err(operand::REGISTER),
        _ => return err(operand::FORM),
    };
    if signed && from == Size::D && to == Size::Q {
        return match src {
            Op::Gpr(s) => Ok(mc::movsxd_rr(d.reg, s.reg)),
            Op::Mem(m) => Ok(mc::movsxd_rm(d.reg, mem(&m)?)),
            _ => err(operand::FORM),
        };
    }
    let inst = match (src, signed) {
        (Op::Gpr(s), false) => mc::try_movzx_rr(to, d.reg, from, s.reg),
        (Op::Gpr(s), true) => mc::try_movsx_rr(to, d.reg, from, s.reg),
        (Op::Mem(m), false) => mc::try_movzx_rm(to, d.reg, from, mem(&m)?),
        (Op::Mem(m), true) => mc::try_movsx_rm(to, d.reg, from, mem(&m)?),
        _ => return err(operand::FORM),
    };
    inst.ok_or(Refusal(operand::REGISTER))
}

/// Whether `inst` takes `lock`: [`Inst::is_lockable`].
pub fn lockable(inst: &Inst) -> bool {
    inst.is_lockable()
}
