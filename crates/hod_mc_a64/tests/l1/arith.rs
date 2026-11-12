//! Sweeps for `spec/mc-a64.md` "Arithmetic".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{EXTENDS, SHIFTS, rs, rz, s, tuples, wz, xz, z};

/// `#imm`, or `#imm12, lsl #12` for a shifted immediate.
fn imm_text(imm: u64) -> String {
    if imm < 0x1000 { format!("#{imm}") } else { format!("#{}, lsl #12", imm >> 12) }
}

const IMMS: [u64; 12] =
    [0, 1, 2, 0x555, 0xaaa, 0xffe, 0xfff, 0x1000, 0x2000, 0x55_5000, 0xffe_000, 0xfff_000];
/// Not ADD/SUB immediates, and not re-expressed by LLVM as another
/// instruction.
const BAD_IMMS: [u64; 5] = [0x1001, 0x1fff, 0xfff_001, 0x100_0000, 0x7fff_f000];
/// Not ADD/SUB immediates; LLVM reads these as negative and flips ADD/SUB.
const BAD_IMMS_NO_TEXT: [u64; 3] = [u64::MAX, 0xffff_ffff_ffff_f000, 1 << 63];

fn immediate(b: &mut Batch) {
    macro_rules! two_reg {
        ($($f:ident $mn:literal $rd:ident $rd_text:ident;)*) => {$(
            for sz in Size::ALL {
                for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                    for imm in [IMMS[i % IMMS.len()], IMMS[(i / 5 + 7) % IMMS.len()]] {
                        emit!(
                            b,
                            $f(sz, $rd(d), s(n), imm),
                            "{} {}, {}, {}", $mn, $rd_text(sz, d), rs(sz, n), imm_text(imm)
                        );
                    }
                }
                for imm in IMMS {
                    emit!(
                        b,
                        $f(sz, $rd(5), s(6), imm),
                        "{} {}, {}, {}", $mn, $rd_text(sz, 5), rs(sz, 6), imm_text(imm)
                    );
                }
                for imm in BAD_IMMS {
                    reject!(
                        b,
                        $f(sz, $rd(5), s(6), imm),
                        "{} {}, {}, #{imm}", $mn, $rd_text(sz, 5), rs(sz, 6)
                    );
                }
                for imm in BAD_IMMS_NO_TEXT {
                    reject!(b, $f(sz, $rd(5), s(6), imm));
                }
            }
        )*};
    }
    two_reg! {
        try_add_imm "add" s rs;
        try_adds_imm "adds" z rz;
        try_sub_imm "sub" s rs;
        try_subs_imm "subs" z rz;
    }

    macro_rules! compare {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for n in 0..32 {
                    for imm in [IMMS[n as usize % IMMS.len()], IMMS[(n as usize + 7) % IMMS.len()]] {
                        emit!(b, $f(sz, s(n), imm), "{} {}, {}", $mn, rs(sz, n), imm_text(imm));
                    }
                }
                for imm in IMMS {
                    emit!(b, $f(sz, s(6), imm), "{} {}, {}", $mn, rs(sz, 6), imm_text(imm));
                }
                for imm in BAD_IMMS {
                    reject!(b, $f(sz, s(6), imm), "{} {}, #{imm}", $mn, rs(sz, 6));
                }
                for imm in BAD_IMMS_NO_TEXT {
                    reject!(b, $f(sz, s(6), imm));
                }
            }
        )*};
    }
    compare! {
        try_cmp_imm "cmp";
        try_cmn_imm "cmn";
    }

    for sz in Size::ALL {
        for [d, n] in tuples([32, 32]) {
            emit!(b, mov_sp(sz, s(d), s(n)), "add {}, {}, #0", rs(sz, d), rs(sz, n));
        }
        // The alias text exists only when one side is the stack pointer.
        for r in 0..31 {
            emit!(b, mov_sp(sz, s(31), s(r)), "mov {}, {}", rs(sz, 31), rs(sz, r));
            emit!(b, mov_sp(sz, s(r), s(31)), "mov {}, {}", rs(sz, r), rs(sz, 31));
        }
    }
}

/// Shift amounts worth checking for an operand size: the ends and a few
/// interior values.
fn amounts(sz: Size) -> [u32; 6] {
    [0, 1, 7, sz.bits() / 2, sz.bits() - 2, sz.bits() - 1]
}

fn shifted(b: &mut Batch) {
    macro_rules! three_reg {
        ($($plain:ident $f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    emit!(
                        b,
                        $plain(sz, z(d), z(n), z(m)),
                        "{} {}, {}, {}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                    let (shift, name) = SHIFTS[i % 3];
                    let amount = amounts(sz)[i % 6];
                    emit!(
                        b,
                        $f(sz, z(d), z(n), z(m), shift, amount),
                        "{} {}, {}, {}, {name} #{amount}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                }
                for (shift, name) in &SHIFTS[..3] {
                    for amount in amounts(sz) {
                        emit!(
                            b,
                            $f(sz, z(5), z(6), z(7), *shift, amount),
                            "{} {}, {}, {}, {name} #{amount}", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7)
                        );
                    }
                    reject!(
                        b,
                        $f(sz, z(5), z(6), z(7), *shift, sz.bits()),
                        "{} {}, {}, {}, {name} #{}", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7), sz.bits()
                    );
                    reject!(b, $f(sz, z(5), z(6), z(7), *shift, 64));
                    reject!(b, $f(sz, z(5), z(6), z(7), *shift, u32::MAX));
                }
                reject!(
                    b,
                    $f(sz, z(5), z(6), z(7), Shift::Ror, 1),
                    "{} {}, {}, {}, ror #1", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7)
                );
            }
        )*};
    }
    three_reg! {
        add try_add_shifted "add";
        adds try_adds_shifted "adds";
        sub try_sub_shifted "sub";
        subs try_subs_shifted "subs";
    }

    // Two-register aliases: CMP/CMN name `rn, rm`; NEG/NEGS name `rd, rm`.
    macro_rules! two_reg {
        ($($plain:ident $f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [p, q]) in tuples([32, 32]).into_iter().enumerate() {
                    emit!(b, $plain(sz, z(p), z(q)), "{} {}, {}", $mn, rz(sz, p), rz(sz, q));
                    let (shift, name) = SHIFTS[i % 3];
                    let amount = amounts(sz)[i % 6];
                    emit!(
                        b,
                        $f(sz, z(p), z(q), shift, amount),
                        "{} {}, {}, {name} #{amount}", $mn, rz(sz, p), rz(sz, q)
                    );
                }
                for (shift, name) in &SHIFTS[..3] {
                    for amount in amounts(sz) {
                        emit!(
                            b,
                            $f(sz, z(5), z(6), *shift, amount),
                            "{} {}, {}, {name} #{amount}", $mn, rz(sz, 5), rz(sz, 6)
                        );
                    }
                    reject!(
                        b,
                        $f(sz, z(5), z(6), *shift, sz.bits()),
                        "{} {}, {}, {name} #{}", $mn, rz(sz, 5), rz(sz, 6), sz.bits()
                    );
                }
                reject!(
                    b,
                    $f(sz, z(5), z(6), Shift::Ror, 1),
                    "{} {}, {}, ror #1", $mn, rz(sz, 5), rz(sz, 6)
                );
            }
        )*};
    }
    two_reg! {
        cmp try_cmp_shifted "cmp";
        cmn try_cmn_shifted "cmn";
        neg try_neg_shifted "neg";
        negs try_negs_shifted "negs";
    }
}

/// The index register of an extended-register form: `W` unless the
/// extension uses all 64 bits of a 64-bit operation.
fn ext_rm(sz: Size, ext: Extend, m: u32) -> String {
    match (sz, ext) {
        (Size::X, Extend::Uxtx | Extend::Sxtx) => xz(m),
        _ => wz(m),
    }
}

fn extended(b: &mut Batch) {
    macro_rules! three_reg {
        ($($f:ident $mn:literal $rd:ident $rd_text:ident;)*) => {$(
            for sz in Size::ALL {
                for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    let (ext, name) = EXTENDS[i % 8];
                    let amount = (i / 8 % 5) as u32;
                    emit!(
                        b,
                        $f(sz, $rd(d), s(n), z(m), ext, amount),
                        "{} {}, {}, {}, {name} #{amount}",
                        $mn, $rd_text(sz, d), rs(sz, n), ext_rm(sz, ext, m)
                    );
                }
                for (ext, name) in EXTENDS {
                    for amount in 0..=4 {
                        emit!(
                            b,
                            $f(sz, $rd(5), s(31), z(7), ext, amount),
                            "{} {}, {}, {}, {name} #{amount}",
                            $mn, $rd_text(sz, 5), rs(sz, 31), ext_rm(sz, ext, 7)
                        );
                    }
                    reject!(
                        b,
                        $f(sz, $rd(5), s(31), z(7), ext, 5),
                        "{} {}, {}, {}, {name} #5",
                        $mn, $rd_text(sz, 5), rs(sz, 31), ext_rm(sz, ext, 7)
                    );
                    reject!(b, $f(sz, $rd(5), s(31), z(7), ext, 8));
                }
            }
        )*};
    }
    three_reg! {
        try_add_ext "add" s rs;
        try_adds_ext "adds" z rz;
        try_sub_ext "sub" s rs;
        try_subs_ext "subs" z rz;
    }
    // The stack-pointer spellings of the plain forms.
    emit!(b, try_add_ext(Size::X, s(0), s(31), z(2), Extend::Uxtx, 0), "add x0, sp, x2");
    emit!(b, try_add_ext(Size::X, s(31), s(31), z(2), Extend::Uxtx, 3), "add sp, sp, x2, lsl #3");
    emit!(b, try_sub_ext(Size::X, s(31), s(31), z(9), Extend::Uxtx, 0), "sub sp, sp, x9");
    emit!(b, try_add_ext(Size::W, s(0), s(31), z(2), Extend::Uxtw, 0), "add w0, wsp, w2");
    emit!(b, try_cmp_ext(Size::X, s(31), z(4), Extend::Uxtx, 0), "cmp sp, x4");

    macro_rules! compare {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [n, m]) in tuples([32, 32]).into_iter().enumerate() {
                    let (ext, name) = EXTENDS[i % 8];
                    let amount = (i / 8 % 5) as u32;
                    emit!(
                        b,
                        $f(sz, s(n), z(m), ext, amount),
                        "{} {}, {}, {name} #{amount}", $mn, rs(sz, n), ext_rm(sz, ext, m)
                    );
                }
                for (ext, name) in EXTENDS {
                    for amount in 0..=4 {
                        emit!(
                            b,
                            $f(sz, s(31), z(7), ext, amount),
                            "{} {}, {}, {name} #{amount}", $mn, rs(sz, 31), ext_rm(sz, ext, 7)
                        );
                    }
                    reject!(
                        b,
                        $f(sz, s(31), z(7), ext, 5),
                        "{} {}, {}, {name} #5", $mn, rs(sz, 31), ext_rm(sz, ext, 7)
                    );
                }
            }
        )*};
    }
    compare! {
        try_cmp_ext "cmp";
        try_cmn_ext "cmn";
    }
}

fn carry_multiply_divide(b: &mut Batch) {
    macro_rules! three_reg {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for [d, n, m] in tuples([32, 32, 32]) {
                    emit!(
                        b,
                        $f(sz, z(d), z(n), z(m)),
                        "{} {}, {}, {}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                }
            }
        )*};
    }
    three_reg! {
        adc "adc";
        adcs "adcs";
        sbc "sbc";
        sbcs "sbcs";
        mul "mul";
        mneg "mneg";
        udiv "udiv";
        sdiv "sdiv";
    }
    // WP-084: CRC32 and CRC32C (FEAT_CRC32).
    for (kind, mn) in [
        (Crc32::B, "crc32b"),
        (Crc32::H, "crc32h"),
        (Crc32::W, "crc32w"),
        (Crc32::X, "crc32x"),
        (Crc32::Cb, "crc32cb"),
        (Crc32::Ch, "crc32ch"),
        (Crc32::Cw, "crc32cw"),
        (Crc32::Cx, "crc32cx"),
    ] {
        let wide = matches!(kind, Crc32::X | Crc32::Cx);
        for [d, n, m] in tuples([32, 32, 32]) {
            let m_ = if wide { xz(m) } else { wz(m) };
            emit_as!(b, mn, crc32(kind, z(d), z(n), z(m)), "{mn} {}, {}, {m_}", wz(d), wz(n));
        }
    }
    for sz in Size::ALL {
        for [d, n, m, a] in tuples([32, 32, 32, 32]) {
            let (d_, n_, m_, a_) = (rz(sz, d), rz(sz, n), rz(sz, m), rz(sz, a));
            emit!(b, madd(sz, z(d), z(n), z(m), z(a)), "madd {d_}, {n_}, {m_}, {a_}");
            emit!(b, msub(sz, z(d), z(n), z(m), z(a)), "msub {d_}, {n_}, {m_}, {a_}");
        }
    }
    macro_rules! long {
        ($($f:ident $mn:literal;)*) => {$(
            for [d, n, m, a] in tuples([32, 32, 32, 32]) {
                emit!(
                    b,
                    $f(z(d), z(n), z(m), z(a)),
                    "{} {}, {}, {}, {}", $mn, xz(d), wz(n), wz(m), xz(a)
                );
            }
        )*};
    }
    long! {
        smaddl "smaddl";
        smsubl "smsubl";
        umaddl "umaddl";
        umsubl "umsubl";
    }
    for [d, n, m] in tuples([32, 32, 32]) {
        emit!(b, smull(z(d), z(n), z(m)), "smull {}, {}, {}", xz(d), wz(n), wz(m));
        emit!(b, umull(z(d), z(n), z(m)), "umull {}, {}, {}", xz(d), wz(n), wz(m));
        emit!(b, smulh(z(d), z(n), z(m)), "smulh {}, {}, {}", xz(d), xz(n), xz(m));
        emit!(b, umulh(z(d), z(n), z(m)), "umulh {}, {}, {}", xz(d), xz(n), xz(m));
    }
}

pub fn build(b: &mut Batch) {
    immediate(b);
    shifted(b);
    extended(b);
    carry_multiply_divide(b);
}
