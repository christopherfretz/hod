//! Sweeps for `spec/mc-a64.md` "Logical" (the immediate forms' operand is
//! swept exhaustively in `bitmask.rs`).

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{SHIFTS, bitmask_patterns, rs, rz, s, tuples, z};

/// A spread of valid logical immediates for an operand size.
fn some_patterns(sz: Size) -> Vec<u64> {
    let all = bitmask_patterns(sz.bits());
    let step = all.len() / 37;
    all.into_iter().step_by(step).collect()
}

/// Values that are not logical immediates and that LLVM refuses as written.
fn bad_patterns(sz: Size) -> Vec<u64> {
    let mut bad = vec![0, 0x5, 0x1234_5678, 0x00ff_00fe, 0x8000_0101];
    bad.push(match sz {
        Size::W => 0xffff_ffff,
        Size::X => u64::MAX,
    });
    bad
}

fn immediate(b: &mut Batch) {
    macro_rules! two_reg {
        ($($f:ident $mn:literal $rd:ident $rd_text:ident;)*) => {$(
            for sz in Size::ALL {
                let patterns = some_patterns(sz);
                for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let imm = patterns[i % patterns.len()];
                    emit!(
                        b,
                        $f(sz, $rd(d), z(n), imm),
                        "{} {}, {}, #{imm:#x}", $mn, $rd_text(sz, d), rz(sz, n)
                    );
                }
                for imm in bad_patterns(sz) {
                    reject!(
                        b,
                        $f(sz, $rd(5), z(6), imm),
                        "{} {}, {}, #{imm:#x}", $mn, $rd_text(sz, 5), rz(sz, 6)
                    );
                }
            }
            // A 32-bit operation takes a 32-bit value: bits above are not
            // ignored.
            reject!(b, $f(Size::W, $rd(5), z(6), 0xffff_ffff_0000_00ff_u64));
            reject!(b, $f(Size::W, $rd(5), z(6), 0x0000_0001_0000_0001_u64));
        )*};
    }
    two_reg! {
        try_and_imm "and" s rs;
        try_orr_imm "orr" s rs;
        try_eor_imm "eor" s rs;
        try_ands_imm "ands" z rz;
    }
    for sz in Size::ALL {
        let patterns = some_patterns(sz);
        for (i, r) in (0..32).chain(0..32).enumerate() {
            let imm = patterns[i % patterns.len()];
            emit!(b, try_tst_imm(sz, z(r), imm), "tst {}, #{imm:#x}", rz(sz, r));
            emit!(
                b,
                try_mov_bitmask(sz, s(r), imm),
                "orr {}, {}, #{imm:#x}",
                rs(sz, r),
                rz(sz, 31)
            );
        }
        for imm in bad_patterns(sz) {
            reject!(b, try_tst_imm(sz, z(6), imm), "tst {}, #{imm:#x}", rz(sz, 6));
            reject!(
                b,
                try_mov_bitmask(sz, s(6), imm),
                "orr {}, {}, #{imm:#x}",
                rs(sz, 6),
                rz(sz, 31)
            );
        }
    }
    // `mov` is the ORR form when neither MOVZ nor MOVN can express the
    // value.
    for imm in [0x5555_5555_5555_5555u64, 0xff00_ff00_ff00_ff00, 0x0000_ffff_ffff_0000, 0x1_fffe] {
        emit!(b, try_mov_bitmask(Size::X, s(9), imm), "mov x9, #{imm:#x}");
    }
    for imm in [0x5555_5555u64, 0xff00_ff00, 0x0ff0_0ff0, 0x1_fffe] {
        emit!(b, try_mov_bitmask(Size::W, s(9), imm), "mov w9, #{imm:#x}");
    }
}

fn shifted(b: &mut Batch) {
    macro_rules! three_reg {
        ($($plain:ident $f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                let amounts = [0, 1, 7, sz.bits() / 2, sz.bits() - 2, sz.bits() - 1];
                for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    emit!(
                        b,
                        $plain(sz, z(d), z(n), z(m)),
                        "{} {}, {}, {}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                    let (shift, name) = SHIFTS[i % 4];
                    let amount = amounts[i % 6];
                    emit!(
                        b,
                        $f(sz, z(d), z(n), z(m), shift, amount),
                        "{} {}, {}, {}, {name} #{amount}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                }
                for (shift, name) in SHIFTS {
                    for amount in amounts {
                        emit!(
                            b,
                            $f(sz, z(5), z(6), z(7), shift, amount),
                            "{} {}, {}, {}, {name} #{amount}", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7)
                        );
                    }
                    reject!(
                        b,
                        $f(sz, z(5), z(6), z(7), shift, sz.bits()),
                        "{} {}, {}, {}, {name} #{}", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7), sz.bits()
                    );
                    reject!(b, $f(sz, z(5), z(6), z(7), shift, 64));
                }
            }
        )*};
    }
    three_reg! {
        and try_and_shifted "and";
        bic try_bic_shifted "bic";
        orr try_orr_shifted "orr";
        orn try_orn_shifted "orn";
        eor try_eor_shifted "eor";
        eon try_eon_shifted "eon";
        ands try_ands_shifted "ands";
        bics try_bics_shifted "bics";
    }

    // MVN names `rd, rm`; TST names `rn, rm`.
    macro_rules! two_reg {
        ($($plain:ident $f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                let amounts = [0, 1, 7, sz.bits() / 2, sz.bits() - 2, sz.bits() - 1];
                for (i, [p, q]) in tuples([32, 32]).into_iter().enumerate() {
                    emit!(b, $plain(sz, z(p), z(q)), "{} {}, {}", $mn, rz(sz, p), rz(sz, q));
                    let (shift, name) = SHIFTS[i % 4];
                    let amount = amounts[i % 6];
                    emit!(
                        b,
                        $f(sz, z(p), z(q), shift, amount),
                        "{} {}, {}, {name} #{amount}", $mn, rz(sz, p), rz(sz, q)
                    );
                }
                for (shift, name) in SHIFTS {
                    reject!(
                        b,
                        $f(sz, z(5), z(6), shift, sz.bits()),
                        "{} {}, {}, {name} #{}", $mn, rz(sz, 5), rz(sz, 6), sz.bits()
                    );
                }
            }
        )*};
    }
    two_reg! {
        mvn try_mvn_shifted "mvn";
        tst try_tst_shifted "tst";
    }
    for sz in Size::ALL {
        for [d, m] in tuples([32, 32]) {
            emit!(b, mov(sz, z(d), z(m)), "mov {}, {}", rz(sz, d), rz(sz, m));
            emit!(b, mov(sz, z(d), z(m)), "orr {}, {}, {}", rz(sz, d), rz(sz, 31), rz(sz, m));
        }
    }
}

pub fn build(b: &mut Batch) {
    immediate(b);
    shifted(b);
}
