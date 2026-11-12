//! Sweeps for `spec/mc-a64.md` "Shifts and bitfields" and "Bit operations".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{rz, tuples, wz, xz, z};

/// (lsb, width) pairs: every one for 32 bits; for 64 bits every `lsb` with
/// the widths at and near the ends and the middle.
fn fields(sz: Size) -> Vec<(u32, u32)> {
    let bits = sz.bits();
    let mut out = Vec::new();
    for lsb in 0..bits {
        let max = bits - lsb;
        for width in 1..=max {
            let near_end = width <= 2 || width + 2 > max;
            if bits == 32 || near_end || width == max / 2 || width == 17 {
                out.push((lsb, width));
            }
        }
    }
    out
}

fn raw(b: &mut Batch) {
    macro_rules! raw {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                let bits = sz.bits();
                for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let (immr, imms) = (i as u32 * 7 % bits, i as u32 * 11 % bits);
                    emit!(
                        b,
                        $f(sz, z(d), z(n), immr, imms),
                        "{} {}, {}, #{immr}, #{imms}", $mn, rz(sz, d), rz(sz, n)
                    );
                }
                for immr in 0..bits {
                    for imms in 0..bits {
                        let ends = |v: u32| v < 2 || v + 2 >= bits || v == bits / 2;
                        if bits == 32 || ends(immr) || ends(imms) || immr == imms {
                            emit!(
                                b,
                                $f(sz, z(5), z(6), immr, imms),
                                "{} {}, {}, #{immr}, #{imms}", $mn, rz(sz, 5), rz(sz, 6)
                            );
                        }
                    }
                }
                reject!(
                    b,
                    $f(sz, z(5), z(6), bits, 0),
                    "{} {}, {}, #{bits}, #0", $mn, rz(sz, 5), rz(sz, 6)
                );
                reject!(
                    b,
                    $f(sz, z(5), z(6), 0, bits),
                    "{} {}, {}, #0, #{bits}", $mn, rz(sz, 5), rz(sz, 6)
                );
                reject!(b, $f(sz, z(5), z(6), 64, 0));
                reject!(b, $f(sz, z(5), z(6), 0, 64));
                reject!(b, $f(sz, z(5), z(6), u32::MAX, u32::MAX));
            }
        )*};
    }
    raw! {
        try_ubfm "ubfm";
        try_sbfm "sbfm";
        try_bfm "bfm";
    }
}

fn aliases(b: &mut Batch) {
    macro_rules! shift {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                let bits = sz.bits();
                for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let amount = i as u32 * 5 % bits;
                    emit!(
                        b,
                        $f(sz, z(d), z(n), amount),
                        "{} {}, {}, #{amount}", $mn, rz(sz, d), rz(sz, n)
                    );
                }
                for amount in 0..bits {
                    emit!(
                        b,
                        $f(sz, z(5), z(6), amount),
                        "{} {}, {}, #{amount}", $mn, rz(sz, 5), rz(sz, 6)
                    );
                }
                reject!(
                    b,
                    $f(sz, z(5), z(6), bits),
                    "{} {}, {}, #{bits}", $mn, rz(sz, 5), rz(sz, 6)
                );
                reject!(b, $f(sz, z(5), z(6), 64));
                reject!(b, $f(sz, z(5), z(6), u32::MAX));
            }
        )*};
    }
    shift! {
        try_lsl_imm "lsl";
        try_lsr_imm "lsr";
        try_asr_imm "asr";
        try_ror_imm "ror";
    }

    macro_rules! field {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                let bits = sz.bits();
                let fields = fields(sz);
                for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let (lsb, width) = fields[i * 13 % fields.len()];
                    emit!(
                        b,
                        $f(sz, z(d), z(n), lsb, width),
                        "{} {}, {}, #{lsb}, #{width}", $mn, rz(sz, d), rz(sz, n)
                    );
                }
                for &(lsb, width) in &fields {
                    emit!(
                        b,
                        $f(sz, z(5), z(6), lsb, width),
                        "{} {}, {}, #{lsb}, #{width}", $mn, rz(sz, 5), rz(sz, 6)
                    );
                }
                // Zero width, a start outside the register, a field that
                // runs past the top.
                for (lsb, width) in [(0, 0), (bits, 1), (0, bits + 1), (1, bits), (bits - 1, 2)] {
                    reject!(
                        b,
                        $f(sz, z(5), z(6), lsb, width),
                        "{} {}, {}, #{lsb}, #{width}", $mn, rz(sz, 5), rz(sz, 6)
                    );
                }
                reject!(b, $f(sz, z(5), z(6), u32::MAX, 1));
                reject!(b, $f(sz, z(5), z(6), 1, u32::MAX));
                reject!(b, $f(sz, z(5), z(6), u32::MAX, u32::MAX));
            }
        )*};
    }
    field! {
        try_ubfx "ubfx";
        try_sbfx "sbfx";
        try_bfxil "bfxil";
        try_ubfiz "ubfiz";
        try_sbfiz "sbfiz";
        try_bfi "bfi";
    }

    for [d, n] in tuples([32, 32]) {
        emit!(b, sxtb(Size::W, z(d), z(n)), "sxtb {}, {}", wz(d), wz(n));
        emit!(b, sxtb(Size::X, z(d), z(n)), "sxtb {}, {}", xz(d), wz(n));
        emit!(b, sxth(Size::W, z(d), z(n)), "sxth {}, {}", wz(d), wz(n));
        emit!(b, sxth(Size::X, z(d), z(n)), "sxth {}, {}", xz(d), wz(n));
        emit!(b, sxtw(z(d), z(n)), "sxtw {}, {}", xz(d), wz(n));
        emit!(b, uxtb(z(d), z(n)), "uxtb {}, {}", wz(d), wz(n));
        emit!(b, uxth(z(d), z(n)), "uxth {}, {}", wz(d), wz(n));
    }
}

fn extract(b: &mut Batch) {
    for sz in Size::ALL {
        let bits = sz.bits();
        for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
            let lsb = i as u32 * 5 % bits;
            emit!(
                b,
                try_extr(sz, z(d), z(n), z(m), lsb),
                "extr {}, {}, {}, #{lsb}",
                rz(sz, d),
                rz(sz, n),
                rz(sz, m)
            );
        }
        for lsb in 0..bits {
            emit!(
                b,
                try_extr(sz, z(5), z(6), z(7), lsb),
                "extr {}, {}, {}, #{lsb}",
                rz(sz, 5),
                rz(sz, 6),
                rz(sz, 7)
            );
        }
        reject!(
            b,
            try_extr(sz, z(5), z(6), z(7), bits),
            "extr {}, {}, {}, #{bits}",
            rz(sz, 5),
            rz(sz, 6),
            rz(sz, 7)
        );
        reject!(b, try_extr(sz, z(5), z(6), z(7), 64));
    }
}

fn register_shifts_and_bit_operations(b: &mut Batch) {
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
        lslv "lslv";
        lsrv "lsrv";
        asrv "asrv";
        rorv "rorv";
    }
    // The preferred spelling of the same four.
    for sz in Size::ALL {
        let (d, n, m) = (rz(sz, 5), rz(sz, 6), rz(sz, 7));
        emit!(b, lslv(sz, z(5), z(6), z(7)), "lsl {d}, {n}, {m}");
        emit!(b, lsrv(sz, z(5), z(6), z(7)), "lsr {d}, {n}, {m}");
        emit!(b, asrv(sz, z(5), z(6), z(7)), "asr {d}, {n}, {m}");
        emit!(b, rorv(sz, z(5), z(6), z(7)), "ror {d}, {n}, {m}");
    }

    macro_rules! two_reg {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for [d, n] in tuples([32, 32]) {
                    emit!(b, $f(sz, z(d), z(n)), "{} {}, {}", $mn, rz(sz, d), rz(sz, n));
                }
            }
        )*};
    }
    two_reg! {
        rbit "rbit";
        rev16 "rev16";
        rev "rev";
        clz "clz";
        cls "cls";
    }
    for [d, n] in tuples([32, 32]) {
        emit!(b, rev32(z(d), z(n)), "rev32 {}, {}", xz(d), xz(n));
    }
}

pub fn build(b: &mut Batch) {
    raw(b);
    aliases(b);
    extract(b);
    register_shifts_and_bit_operations(b);
}
