//! `spec/mc-x64.md` "Registers and operand kinds" (design 0006 2.5 point
//! 2): an 8-bit operand numbered 4 to 7 is `spl`, `bpl`, `sil`, `dil`,
//! which exist only with a REX prefix; without one the same numbers are
//! `ah`, `ch`, `dh`, `bh`. Every byte-register form of the crate must put
//! a REX prefix on such an operand, in every position it can take, so
//! that no constructor ever produces a high-byte register.

use std::fmt::Write as _;

use hod_mc_x64::*;

use crate::harness::Tools;
use crate::text::{CONDS, r};

/// A memory operand that needs no REX of its own.
const M: Mem = Mem::base(Reg::Rax, 0);

/// A form with one byte register operand.
type One = (&'static str, fn(Reg) -> Inst);
/// A form with two.
type Two = (&'static str, fn(Reg, Reg) -> Inst);

macro_rules! one {
    ($($name:literal => |$x:ident| $e:expr;)*) => {
        vec![$(($name, (|$x: Reg| $e) as fn(Reg) -> Inst)),*]
    };
}

macro_rules! two {
    ($($f:ident),* $(,)?) => {
        vec![$((stringify!($f), (|a: Reg, b: Reg| $f(Size::B, a, b)) as fn(Reg, Reg) -> Inst)),*]
    };
}

fn one_operand() -> Vec<One> {
    let mut forms = one! {
        "add_rm" => |x| add_rm(Size::B, x, M);
        "or_rm" => |x| or_rm(Size::B, x, M);
        "adc_rm" => |x| adc_rm(Size::B, x, M);
        "sbb_rm" => |x| sbb_rm(Size::B, x, M);
        "and_rm" => |x| and_rm(Size::B, x, M);
        "sub_rm" => |x| sub_rm(Size::B, x, M);
        "xor_rm" => |x| xor_rm(Size::B, x, M);
        "cmp_rm" => |x| cmp_rm(Size::B, x, M);
        "add_mr" => |x| add_mr(Size::B, M, x);
        "or_mr" => |x| or_mr(Size::B, M, x);
        "adc_mr" => |x| adc_mr(Size::B, M, x);
        "sbb_mr" => |x| sbb_mr(Size::B, M, x);
        "and_mr" => |x| and_mr(Size::B, M, x);
        "sub_mr" => |x| sub_mr(Size::B, M, x);
        "xor_mr" => |x| xor_mr(Size::B, M, x);
        "cmp_mr" => |x| cmp_mr(Size::B, M, x);
        "add_ri" => |x| add_ri(Size::B, x, 1);
        "or_ri" => |x| or_ri(Size::B, x, 1);
        "adc_ri" => |x| adc_ri(Size::B, x, 1);
        "sbb_ri" => |x| sbb_ri(Size::B, x, 1);
        "and_ri" => |x| and_ri(Size::B, x, 1);
        "sub_ri" => |x| sub_ri(Size::B, x, 1);
        "xor_ri" => |x| xor_ri(Size::B, x, 1);
        "cmp_ri" => |x| cmp_ri(Size::B, x, 1);
        "test_mr" => |x| test_mr(Size::B, M, x);
        "test_ri" => |x| test_ri(Size::B, x, 1);
        "inc_r" => |x| inc_r(Size::B, x);
        "dec_r" => |x| dec_r(Size::B, x);
        "not_r" => |x| not_r(Size::B, x);
        "neg_r" => |x| neg_r(Size::B, x);
        "mul_r" => |x| mul_r(Size::B, x);
        "imul_r" => |x| imul_r(Size::B, x);
        "div_r" => |x| div_r(Size::B, x);
        "idiv_r" => |x| idiv_r(Size::B, x);
        "mov_rm" => |x| mov_rm(Size::B, x, M);
        "mov_mr" => |x| mov_mr(Size::B, M, x);
        "mov_ri" => |x| mov_ri(Size::B, x, 1);
        "movzx_rr" => |x| movzx_rr(Size::D, Reg::Rax, Size::B, x);
        "movsx_rr" => |x| movsx_rr(Size::D, Reg::Rcx, Size::B, x);
        "movzx_rr w" => |x| movzx_rr(Size::W, Reg::Rdx, Size::B, x);
        "rol_ri" => |x| rol_ri(Size::B, x, 3);
        "ror_ri" => |x| ror_ri(Size::B, x, 1);
        "rcl_ri" => |x| rcl_ri(Size::B, x, 3);
        "rcr_ri" => |x| rcr_ri(Size::B, x, 1);
        "shl_ri" => |x| shl_ri(Size::B, x, 3);
        "shr_ri" => |x| shr_ri(Size::B, x, 1);
        "sar_ri" => |x| sar_ri(Size::B, x, 7);
        "rol_rcl" => |x| rol_rcl(Size::B, x);
        "ror_rcl" => |x| ror_rcl(Size::B, x);
        "rcl_rcl" => |x| rcl_rcl(Size::B, x);
        "rcr_rcl" => |x| rcr_rcl(Size::B, x);
        "shl_rcl" => |x| shl_rcl(Size::B, x);
        "shr_rcl" => |x| shr_rcl(Size::B, x);
        "sar_rcl" => |x| sar_rcl(Size::B, x);
        "xchg_mr" => |x| xchg_mr(Size::B, M, x);
        "xadd_mr" => |x| xadd_mr(Size::B, M, x);
        "cmpxchg_mr" => |x| cmpxchg_mr(Size::B, M, x);
    };
    // Every condition of `setcc`.
    let setcc: [fn(Reg) -> Inst; 16] = [
        |x| setcc_r(CONDS[0].0, x),
        |x| setcc_r(CONDS[1].0, x),
        |x| setcc_r(CONDS[2].0, x),
        |x| setcc_r(CONDS[3].0, x),
        |x| setcc_r(CONDS[4].0, x),
        |x| setcc_r(CONDS[5].0, x),
        |x| setcc_r(CONDS[6].0, x),
        |x| setcc_r(CONDS[7].0, x),
        |x| setcc_r(CONDS[8].0, x),
        |x| setcc_r(CONDS[9].0, x),
        |x| setcc_r(CONDS[10].0, x),
        |x| setcc_r(CONDS[11].0, x),
        |x| setcc_r(CONDS[12].0, x),
        |x| setcc_r(CONDS[13].0, x),
        |x| setcc_r(CONDS[14].0, x),
        |x| setcc_r(CONDS[15].0, x),
    ];
    forms.extend(setcc.into_iter().map(|f| ("setcc_r", f)));
    forms
}

fn two_operand() -> Vec<Two> {
    two![add_rr, or_rr, adc_rr, sbb_rr, and_rr, sub_rr, xor_rr, cmp_rr, test_rr, mov_rr, xchg_rr]
}

/// Every byte-register instruction with a register numbered 4 to 7 in
/// some position, the other operands numbered 0 to 3: `(form, the
/// instruction, the names of its byte registers)`.
fn high_cases() -> Vec<(&'static str, Inst, Vec<u8>)> {
    let mut out = Vec::new();
    for (name, f) in one_operand() {
        for n in 4..8 {
            out.push((name, f(r(n)), vec![n]));
        }
    }
    for (name, f) in two_operand() {
        for n in 4..8 {
            for low in 0..4 {
                out.push((name, f(r(n), r(low)), vec![n, low]));
                out.push((name, f(r(low), r(n)), vec![low, n]));
            }
            for m in 4..8 {
                out.push((name, f(r(n), r(m)), vec![n, m]));
            }
        }
    }
    out
}

/// The same forms with registers numbered 0 to 3 only.
fn low_cases() -> Vec<(&'static str, Inst)> {
    let mut out = Vec::new();
    for (name, f) in one_operand() {
        for n in 0..4 {
            out.push((name, f(r(n))));
        }
    }
    for (name, f) in two_operand() {
        for a in 0..4 {
            for b in 0..4 {
                out.push((name, f(r(a), r(b))));
            }
        }
    }
    out
}

/// The first byte after an operand-size prefix: where REX goes.
fn after_66(inst: Inst) -> u8 {
    inst.bytes().iter().copied().find(|&b| b != 0x66).expect("an opcode")
}

/// A byte operand numbered 4 to 7 gets exactly `40` (no other REX bit is
/// needed), first after any `66`; one numbered 0 to 3 gets none. So the REX comes from
/// the operand alone, and no form can produce `ah`..`bh`.
#[test]
fn byte_registers_4_to_7_always_get_rex() {
    let high = high_cases();
    assert!(high.len() > 500, "{} cases", high.len());
    for (name, inst, regs) in &high {
        assert!(inst.has_rex(), "{name} {regs:?}: no REX in {:02x?}", inst.bytes());
        assert_eq!(after_66(*inst), 0x40, "{name} {regs:?}: {:02x?}", inst.bytes());
    }
    for (name, inst) in low_cases() {
        assert!(!inst.has_rex(), "{name}: REX in {:02x?}", inst.bytes());
        assert!(!(0x40..0x50).contains(&after_66(inst)), "{name}: {:02x?}", inst.bytes());
    }
}

const BYTE_NAMES: [&str; 8] = ["al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil"];
const HIGH_BYTES: [&str; 4] = ["ah", "ch", "dh", "bh"];

/// LLVM's disassembler reads each of those instructions with the
/// registers intended, and never a high-byte register: hod's bytes, one
/// `.byte` line each, through `llvm-objdump` of every installed major.
#[test]
fn byte_registers_4_to_7_disassemble_as_spl_to_dil() {
    let cases = high_cases();
    let mut source = String::new();
    for (_, inst, _) in &cases {
        let bytes: Vec<String> = inst.bytes().iter().map(|b| format!("{b:#04x}")).collect();
        writeln!(source, ".byte {}", bytes.join(",")).expect("writing to a String");
    }
    for tools in Tools::all() {
        let obj = tools.assemble_object("byte-regs", &source).expect("llvm-mc assembles .byte");
        let decoded = tools.disassemble(&obj);
        assert_eq!(decoded.len(), cases.len(), "LLVM {}: one instruction per case", tools.major);
        for ((_, text), (name, inst, regs)) in decoded.iter().zip(&cases) {
            let words: Vec<&str> = text
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect();
            for high in HIGH_BYTES {
                assert!(
                    !words.contains(&high),
                    "LLVM {}: {name} {:02x?} is `{text}`",
                    tools.major,
                    inst.bytes()
                );
            }
            for &n in regs {
                let want = BYTE_NAMES[n as usize];
                assert!(
                    words.contains(&want),
                    "LLVM {}: {name} {:02x?} is `{text}`, without {want}",
                    tools.major,
                    inst.bytes()
                );
            }
        }
    }
}
