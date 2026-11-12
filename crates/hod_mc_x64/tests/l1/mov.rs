//! Sweeps for `spec/mc-x64.md` "Moves and extensions".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{SIZES, WIDE, bad_imms, imms, llvm_checks_imm, ptr, r, reg, some_mems};

/// 64-bit `mov` immediates that are not sign-extended 32-bit values.
const WIDE_IMMS: [i64; 8] = [
    0x8000_0000,
    0xffff_ffff,
    0x1_0000_0000,
    -0x8000_0001,
    0x1234_5678_9abc_def0,
    i64::MAX,
    i64::MIN,
    -0x1_0000_0000,
];

pub fn build(b: &mut Batch) {
    moves(b);
    extensions(b);
    lea_sweep(b);
}

fn moves(b: &mut Batch) {
    for size in SIZES {
        for d in 0..16 {
            for s in 0..16 {
                emit!(b, mov_rr(size, r(d), r(s)), "mov {}, {}", reg(size, d), reg(size, s));
            }
            for m in some_mems() {
                emit!(b, mov_rm(size, r(d), m.mem()), "mov {}, {}", reg(size, d), m.sized(size));
                emit!(b, mov_mr(size, m.mem(), r(d)), "mov {}, {}", m.sized(size), reg(size, d));
            }
            for v in imms(size) {
                emit!(b, try_mov_ri(size, r(d), v), "mov {}, {v}", reg(size, d));
            }
        }
        for m in some_mems() {
            for v in imms(size) {
                emit!(b, try_mov_mi(size, m.mem(), v), "mov {}, {v}", m.sized(size));
            }
        }
        for v in bad_imms(size) {
            if size != Size::Q {
                reject!(b, try_mov_ri(size, r(2), v));
            }
            let m = Mem::base(Reg::Rcx, 0);
            if llvm_checks_imm(size) {
                reject!(b, try_mov_mi(size, m, v), "mov {} [rcx], {v}", ptr(size));
            } else {
                reject!(b, try_mov_mi(size, m, v));
            }
        }
    }
    for d in 0..16 {
        for v in WIDE_IMMS {
            emit!(b, try_mov_ri(Size::Q, r(d), v), "mov {}, {v}", reg(Size::Q, d));
        }
    }
}

/// `(destination, source)` sizes of `movzx`/`movsx`, stated independently.
const EXTENDS: [(Size, Size); 5] = [
    (Size::W, Size::B),
    (Size::D, Size::B),
    (Size::Q, Size::B),
    (Size::D, Size::W),
    (Size::Q, Size::W),
];

fn extensions(b: &mut Batch) {
    macro_rules! extend {
        ($($rr:ident $rm:ident $mn:literal;)*) => {$(
            for (ds, ss) in EXTENDS {
                for d in 0..16 {
                    for s in 0..16 {
                        emit!(b, $rr(ds, r(d), ss, r(s)), "{} {}, {}", $mn, reg(ds, d), reg(ss, s));
                    }
                }
                for m in some_mems() {
                    for d in [0, 4, 9, 15] {
                        emit!(b, $rm(ds, r(d), ss, m.mem()), "{} {}, {}", $mn, reg(ds, d), m.sized(ss));
                    }
                }
            }
            for ds in SIZES {
                for ss in SIZES {
                    if EXTENDS.contains(&(ds, ss)) {
                        continue;
                    }
                    // `movsx r64, r32` is LLVM's name for MOVSXD; hod spells
                    // that instruction `movsxd_*`.
                    if $mn == "movsx" && (ds, ss) == (Size::Q, Size::D) {
                        reject!(b, $rr(ds, r(1), ss, r(2)));
                        reject!(b, $rm(ds, r(1), ss, Mem::base(Reg::Rax, 0)));
                        continue;
                    }
                    reject!(b, $rr(ds, r(1), ss, r(2)), "{} {}, {}", $mn, reg(ds, 1), reg(ss, 2));
                    reject!(b, $rm(ds, r(1), ss, Mem::base(Reg::Rax, 0)), "{} {}, {} [rax]", $mn, reg(ds, 1), ptr(ss));
                }
            }
        )*};
    }
    extend! {
        try_movzx_rr try_movzx_rm "movzx";
        try_movsx_rr try_movsx_rm "movsx";
    }
    for d in 0..16 {
        for s in 0..16 {
            emit!(b, movsxd_rr(r(d), r(s)), "movsxd {}, {}", reg(Size::Q, d), reg(Size::D, s));
        }
    }
    for m in some_mems() {
        for d in [0, 7, 8, 13] {
            emit!(b, movsxd_rm(r(d), m.mem()), "movsxd {}, {}", reg(Size::Q, d), m.sized(Size::D));
        }
    }
    emit!(b, cbw(), "cbw");
    emit!(b, cwde(), "cwde");
    emit!(b, cdqe(), "cdqe");
    emit!(b, cwd(), "cwd");
    emit!(b, cdq(), "cdq");
    emit!(b, cqo(), "cqo");
}

fn lea_sweep(b: &mut Batch) {
    for size in WIDE {
        for d in 0..16 {
            for m in some_mems() {
                emit!(b, try_lea(size, r(d), m.mem()), "lea {}, {}", reg(size, d), m.text());
            }
        }
    }
    reject!(b, try_lea(Size::B, r(0), Mem::base(Reg::Rax, 0)), "lea al, [rax]");
    reject!(b, try_lea(Size::B, r(9), Mem::rip(0)), "lea r9b, [rip]");
}
