//! Sweeps for `spec/mc-x64.md` "Arithmetic and logic", "Multiply and
//! divide".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{SIZES, WIDE, bad_imms, imms, llvm_checks_imm, r, reg, some_mems};

pub fn build(b: &mut Batch) {
    two_operand(b);
    test(b);
    unary(b);
    multiply(b);
}

fn two_operand(b: &mut Batch) {
    macro_rules! alu {
        ($($rr:ident $rm:ident $mr:ident $ri:ident $mi:ident $mn:literal;)*) => {$(
            for size in SIZES {
                for d in 0..16 {
                    for s in 0..16 {
                        emit!(b, $rr(size, r(d), r(s)), "{} {}, {}", $mn, reg(size, d), reg(size, s));
                    }
                    for m in some_mems() {
                        emit!(b, $rm(size, r(d), m.mem()), "{} {}, {}", $mn, reg(size, d), m.sized(size));
                        emit!(b, $mr(size, m.mem(), r(d)), "{} {}, {}", $mn, m.sized(size), reg(size, d));
                    }
                    for v in imms(size) {
                        emit!(b, $ri(size, r(d), v), "{} {}, {v}", $mn, reg(size, d));
                    }
                }
                for m in some_mems() {
                    for v in imms(size) {
                        emit!(b, $mi(size, m.mem(), v), "{} {}, {v}", $mn, m.sized(size));
                    }
                }
                for v in bad_imms(size) {
                    if llvm_checks_imm(size) { reject!(b, $ri(size, r(0), v), "{} {}, {v}", $mn, reg(size, 0)); } else { reject!(b, $ri(size, r(0), v)); }
                    if llvm_checks_imm(size) { reject!(b, $ri(size, r(9), v), "{} {}, {v}", $mn, reg(size, 9)); } else { reject!(b, $ri(size, r(9), v)); }
                    if llvm_checks_imm(size) { reject!(b, $mi(size, Mem::base(Reg::Rbx, 0), v), "{} {} [rbx], {v}", $mn, crate::text::ptr(size)); } else { reject!(b, $mi(size, Mem::base(Reg::Rbx, 0), v)); }
                }
            }
        )*};
    }
    alu! {
        add_rr add_rm add_mr try_add_ri try_add_mi "add";
        or_rr or_rm or_mr try_or_ri try_or_mi "or";
        adc_rr adc_rm adc_mr try_adc_ri try_adc_mi "adc";
        sbb_rr sbb_rm sbb_mr try_sbb_ri try_sbb_mi "sbb";
        and_rr and_rm and_mr try_and_ri try_and_mi "and";
        sub_rr sub_rm sub_mr try_sub_ri try_sub_mi "sub";
        xor_rr xor_rm xor_mr try_xor_ri try_xor_mi "xor";
        cmp_rr cmp_rm cmp_mr try_cmp_ri try_cmp_mi "cmp";
    }
}

fn test(b: &mut Batch) {
    for size in SIZES {
        for a in 0..16 {
            for s in 0..16 {
                emit!(b, test_rr(size, r(a), r(s)), "test {}, {}", reg(size, a), reg(size, s));
            }
            for v in imms(size) {
                emit!(b, try_test_ri(size, r(a), v), "test {}, {v}", reg(size, a));
            }
        }
        for m in some_mems() {
            for s in [0, 4, 7, 8, 15] {
                emit!(b, test_mr(size, m.mem(), r(s)), "test {}, {}", m.sized(size), reg(size, s));
            }
            for v in imms(size) {
                emit!(b, try_test_mi(size, m.mem(), v), "test {}, {v}", m.sized(size));
            }
        }
        for v in bad_imms(size) {
            if llvm_checks_imm(size) {
                reject!(b, try_test_ri(size, r(0), v), "test {}, {v}", reg(size, 0));
            } else {
                reject!(b, try_test_ri(size, r(0), v));
            }
            if llvm_checks_imm(size) {
                reject!(b, try_test_ri(size, r(3), v), "test {}, {v}", reg(size, 3));
            } else {
                reject!(b, try_test_ri(size, r(3), v));
            }
            if llvm_checks_imm(size) {
                reject!(
                    b,
                    try_test_mi(size, Mem::base(Reg::Rax, 0), v),
                    "test {} [rax], {v}",
                    crate::text::ptr(size)
                );
            } else {
                reject!(b, try_test_mi(size, Mem::base(Reg::Rax, 0), v));
            }
        }
    }
}

fn unary(b: &mut Batch) {
    macro_rules! unary {
        ($($fr:ident $fm:ident $mn:literal;)*) => {$(
            for size in SIZES {
                for n in 0..16 {
                    emit!(b, $fr(size, r(n)), "{} {}", $mn, reg(size, n));
                }
                for m in some_mems() {
                    emit!(b, $fm(size, m.mem()), "{} {}", $mn, m.sized(size));
                }
            }
        )*};
    }
    unary! {
        inc_r inc_m "inc";
        dec_r dec_m "dec";
        not_r not_m "not";
        neg_r neg_m "neg";
        mul_r mul_m "mul";
        imul_r imul_m "imul";
        div_r div_m "div";
        idiv_r idiv_m "idiv";
    }
}

fn multiply(b: &mut Batch) {
    for size in WIDE {
        for d in 0..16 {
            for s in 0..16 {
                emit!(b, try_imul_rr(size, r(d), r(s)), "imul {}, {}", reg(size, d), reg(size, s));
            }
            for (i, v) in imms(size).into_iter().enumerate() {
                let s = (d + i as u8 * 3) % 16;
                emit!(
                    b,
                    try_imul_rri(size, r(d), r(s), v),
                    "imul {}, {}, {v}",
                    reg(size, d),
                    reg(size, s)
                );
            }
            for m in some_mems() {
                emit!(
                    b,
                    try_imul_rm(size, r(d), m.mem()),
                    "imul {}, {}",
                    reg(size, d),
                    m.sized(size)
                );
            }
        }
        for m in some_mems() {
            for v in imms(size) {
                emit!(
                    b,
                    try_imul_rmi(size, r(10), m.mem(), v),
                    "imul {}, {}, {v}",
                    reg(size, 10),
                    m.sized(size)
                );
            }
        }
        for v in bad_imms(size) {
            if llvm_checks_imm(size) {
                reject!(
                    b,
                    try_imul_rri(size, r(1), r(2), v),
                    "imul {}, {}, {v}",
                    reg(size, 1),
                    reg(size, 2)
                );
            } else {
                reject!(b, try_imul_rri(size, r(1), r(2), v));
            }
            if llvm_checks_imm(size) {
                reject!(
                    b,
                    try_imul_rmi(size, r(1), Mem::base(Reg::Rax, 0), v),
                    "imul {}, {} [rax], {v}",
                    reg(size, 1),
                    crate::text::ptr(size)
                );
            } else {
                reject!(b, try_imul_rmi(size, r(1), Mem::base(Reg::Rax, 0), v));
            }
        }
    }
    let m = Mem::base(Reg::Rax, 0);
    reject!(b, try_imul_rr(Size::B, r(1), r(2)), "imul cl, dl");
    reject!(b, try_imul_rm(Size::B, r(1), m), "imul cl, byte ptr [rax]");
    reject!(b, try_imul_rri(Size::B, r(1), r(2), 3), "imul cl, dl, 3");
    reject!(b, try_imul_rmi(Size::B, r(1), m, 3), "imul cl, byte ptr [rax], 3");
}
