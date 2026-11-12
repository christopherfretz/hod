//! Sweeps for `spec/mc-a64.md` "SIMD".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{
    ARRS, ELEMS, FP_ARRS, FP_SIZES, VEC_BITS, fp_bits, fp_imm8_value, rz, tuples, v, wz, xz, z,
};

/// The rows of [`ARRS`] whose elements are narrower than 64 bits.
const NOT_D: &[(Arr, &str, char, u32)] = ARRS.split_at(6).0;

/// `<mn> v<d>.<T>, v<n>.<T>, v<m>.<T>` for every row of a table whose
/// first two fields are the arrangement operand and its name.
macro_rules! three {
    ($b:ident, $rows:expr, $($f:ident $mn:literal;)*) => {$(
        for row in $rows {
            let (kind, t) = (row.0, row.1);
            for [d, n, m] in tuples([32, 32, 32]) {
                emit!($b, $f(kind, v(d), v(n), v(m)), "{} v{d}.{t}, v{n}.{t}, v{m}.{t}", $mn);
            }
        }
    )*};
}

/// `<mn> v<d>.<T>, v<n>.<T><tail>` likewise.
macro_rules! two {
    ($b:ident, $rows:expr, $tail:literal, $($f:ident $mn:literal;)*) => {$(
        for row in $rows {
            let (kind, t) = (row.0, row.1);
            for [d, n] in tuples([32, 32]) {
                emit!($b, $f(kind, v(d), v(n)), "{} v{d}.{t}, v{n}.{t}{}", $mn, $tail);
            }
        }
    )*};
}

/// The general-purpose register an element of this width moves to or
/// from: `X` for 64 bits, `W` below.
fn gpr(prefix: char, n: u32) -> String {
    if prefix == 'd' { xz(n) } else { wz(n) }
}

fn moves(b: &mut Batch) {
    two!(b, VEC_BITS, "", mov_vec "mov";);

    for (arr, t, p, _) in ARRS {
        for [d, n] in tuples([32, 32]) {
            emit!(b, dup_gpr(arr, v(d), z(n)), "dup v{d}.{t}, {}", gpr(p, n));
        }
        let lanes = arr.elem().lanes();
        for index in 0..lanes {
            for [d, n] in [[5, 6], [31, 0], [0, 31]] {
                emit!(b, try_dup_elem(arr, v(d), v(n), index), "dup v{d}.{t}, v{n}.{p}[{index}]");
            }
        }
        reject!(b, try_dup_elem(arr, v(5), v(6), lanes), "dup v5.{t}, v6.{p}[{lanes}]");
        reject!(b, try_dup_elem(arr, v(5), v(6), u32::MAX));
    }

    for (elem, p, lanes) in ELEMS {
        assert_eq!(elem.lanes(), lanes);
        for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
            let (index, other) = (i as u32 % lanes, (i as u32 / 3) % lanes);
            emit!(b, try_dup_scalar(elem, v(d), v(n), index), "mov {p}{d}, v{n}.{p}[{index}]");
            emit!(
                b,
                try_ins_elem(elem, v(d), index, v(n), other),
                "mov v{d}.{p}[{index}], v{n}.{p}[{other}]"
            );
            emit!(b, try_ins_gpr(elem, v(d), index, z(n)), "mov v{d}.{p}[{index}], {}", gpr(p, n));
            emit!(b, try_umov(elem, z(n), v(d), index), "umov {}, v{d}.{p}[{index}]", gpr(p, n));
        }
        for index in 0..lanes {
            emit!(b, try_dup_scalar(elem, v(5), v(6), index), "mov {p}5, v6.{p}[{index}]");
            emit!(b, try_ins_gpr(elem, v(5), index, z(6)), "mov v5.{p}[{index}], {}", gpr(p, 6));
            emit!(b, try_umov(elem, z(5), v(6), index), "umov {}, v6.{p}[{index}]", gpr(p, 5));
            for other in 0..lanes {
                emit!(
                    b,
                    try_ins_elem(elem, v(5), index, v(6), other),
                    "mov v5.{p}[{index}], v6.{p}[{other}]"
                );
            }
        }
        reject!(b, try_dup_scalar(elem, v(5), v(6), lanes), "mov {p}5, v6.{p}[{lanes}]");
        reject!(b, try_ins_elem(elem, v(5), lanes, v(6), 0), "mov v5.{p}[{lanes}], v6.{p}[0]");
        reject!(b, try_ins_elem(elem, v(5), 0, v(6), lanes), "mov v5.{p}[0], v6.{p}[{lanes}]");
        reject!(b, try_ins_gpr(elem, v(5), lanes, z(6)), "mov v5.{p}[{lanes}], {}", gpr(p, 6));
        reject!(b, try_umov(elem, z(5), v(6), lanes), "umov {}, v6.{p}[{lanes}]", gpr(p, 5));
        reject!(b, try_dup_scalar(elem, v(5), v(6), u32::MAX));
        reject!(b, try_ins_elem(elem, v(5), u32::MAX, v(6), 0));
        reject!(b, try_ins_elem(elem, v(5), 0, v(6), u32::MAX));
        reject!(b, try_ins_gpr(elem, v(5), u32::MAX, z(6)));
        reject!(b, try_umov(elem, z(5), v(6), u32::MAX));

        // SMOV: only to a wider register.
        for sz in Size::ALL {
            if elem.bits() >= sz.bits() {
                reject!(b, try_smov(sz, elem, z(5), v(6), 0), "smov {}, v6.{p}[0]", rz(sz, 5));
                continue;
            }
            for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
                let index = i as u32 % lanes;
                emit!(
                    b,
                    try_smov(sz, elem, z(d), v(n), index),
                    "smov {}, v{n}.{p}[{index}]",
                    rz(sz, d)
                );
            }
            for index in 0..lanes {
                emit!(
                    b,
                    try_smov(sz, elem, z(5), v(6), index),
                    "smov {}, v6.{p}[{index}]",
                    rz(sz, 5)
                );
            }
            reject!(
                b,
                try_smov(sz, elem, z(5), v(6), lanes),
                "smov {}, v6.{p}[{lanes}]",
                rz(sz, 5)
            );
            reject!(b, try_smov(sz, elem, z(5), v(6), u32::MAX));
        }
    }
}

fn immediates(b: &mut Batch) {
    for (arr, t, p, _) in ARRS {
        let shifts: &[u32] = match p {
            'b' => &[0],
            'h' => &[0, 8],
            's' => &[0, 8, 16, 24],
            _ => &[],
        };
        for &shift in shifts {
            // An explicit `lsl #0` and no shift at all are the same text
            // to LLVM; both are swept.
            let lsl = if shift == 0 { String::new() } else { format!(", lsl #{shift}") };
            for imm8 in 0..=255u8 {
                let d = u32::from(imm8) % 32;
                emit!(b, try_movi(arr, v(d), imm8, shift), "movi v{d}.{t}, #{imm8}{lsl}");
                if p != 'b' {
                    emit!(b, try_mvni(arr, v(d), imm8, shift), "mvni v{d}.{t}, #{imm8}{lsl}");
                }
            }
            if p != 'b' {
                emit!(b, try_movi(arr, v(5), 0x5a, shift), "movi v5.{t}, #90, lsl #{shift}");
                emit!(b, try_mvni(arr, v(5), 0x5a, shift), "mvni v5.{t}, #90, lsl #{shift}");
            }
        }
        // The first shift the element width does not have, a shift that is
        // not a whole byte, and (64-bit elements) any shift at all.
        let past = shifts.len() as u32 * 8;
        reject!(b, try_movi(arr, v(5), 1, past), "movi v5.{t}, #1, lsl #{past}");
        reject!(b, try_mvni(arr, v(5), 1, past), "mvni v5.{t}, #1, lsl #{past}");
        reject!(b, try_movi(arr, v(5), 1, 4), "movi v5.{t}, #1, lsl #4");
        reject!(b, try_mvni(arr, v(5), 1, 4), "mvni v5.{t}, #1, lsl #4");
        reject!(b, try_movi(arr, v(5), 1, u32::MAX));
        reject!(b, try_mvni(arr, v(5), 1, u32::MAX));
        if shifts.is_empty() {
            reject!(b, try_movi(arr, v(5), 1, 0), "movi v5.{t}, #1");
        }
        if p == 'b' || p == 'd' {
            reject!(b, try_mvni(arr, v(5), 1, 0), "mvni v5.{t}, #1");
        }
    }

    // The 64-bit byte masks: all 256 of them, in both forms.
    for imm8 in 0..=255u32 {
        let mask = (0..8).fold(0u64, |m, i| m | (u64::from(imm8 >> i & 1) * 0xff) << (i * 8));
        let d = imm8 % 32;
        emit!(b, try_movi_mask(VecBits::V64, v(d), mask), "movi d{d}, #{mask:#018x}");
        emit!(b, try_movi_mask(VecBits::V128, v(d), mask), "movi v{d}.2d, #{mask:#018x}");
    }
    for mask in [1u64, 0x80, 0xfe, 0x7f00, 0xff01, 0x0100_0000_0000_0000, u64::MAX - 1] {
        reject!(b, try_movi_mask(VecBits::V64, v(5), mask), "movi d5, #{mask:#x}");
        reject!(b, try_movi_mask(VecBits::V128, v(5), mask), "movi v5.2d, #{mask:#x}");
    }

    for (arr, t) in FP_ARRS {
        let sz = arr.fp_size();
        for imm8 in 0..=255u8 {
            let value = fp_imm8_value(imm8);
            let d = u32::from(imm8) % 32;
            emit!(b, try_fmov_imm_vec(arr, v(d), fp_bits(sz, value)), "fmov v{d}.{t}, #{value:?}");
        }
        for value in [0.1f64, 0.0625, 32.0, 1.03125] {
            let value = f64::from(value as f32);
            reject!(b, try_fmov_imm_vec(arr, v(5), fp_bits(sz, value)), "fmov v5.{t}, #{value:?}");
        }
        reject!(b, try_fmov_imm_vec(arr, v(5), 0), "fmov v5.{t}, #0.0");
        reject!(b, try_fmov_imm_vec(arr, v(5), fp_bits(sz, f64::NAN)));
        reject!(b, try_fmov_imm_vec(arr, v(5), fp_bits(sz, 1.0) + 1));
    }
}

fn integer(b: &mut Batch) {
    three! {
        b, ARRS,
        add_vec "add";
        sub_vec "sub";
        cmeq "cmeq";
        cmgt "cmgt";
        cmge "cmge";
        cmhi "cmhi";
        cmhs "cmhs";
        cmtst "cmtst";
        sshl "sshl";
        ushl "ushl";
    }
    macro_rules! narrower_than_d {
        ($($f:ident $mn:literal;)*) => {$(
            three!(b, NOT_D, $f $mn;);
            reject!(b, $f(Arr::D2, v(5), v(6), v(7)), "{} v5.2d, v6.2d, v7.2d", $mn);
        )*};
    }
    narrower_than_d! {
        try_mul_vec "mul";
        try_smax "smax";
        try_smin "smin";
        try_umax "umax";
        try_umin "umin";
    }

    two!(b, ARRS, "", abs "abs"; neg_vec "neg";);
    two! {
        b, ARRS, ", #0",
        cmeq_zero "cmeq";
        cmgt_zero "cmgt";
        cmge_zero "cmge";
        cmlt_zero "cmlt";
        cmle_zero "cmle";
    }
    two!(b, NOT_D, "", try_clz_vec "clz"; try_rev64 "rev64";);
    reject!(b, try_clz_vec(Arr::D2, v(5), v(6)), "clz v5.2d, v6.2d");
    reject!(b, try_rev64(Arr::D2, v(5), v(6)), "rev64 v5.2d, v6.2d");
    two!(b, NOT_D.split_at(4).0, "", try_rev32_vec "rev32";);
    for (arr, t, ..) in &ARRS[4..] {
        reject!(b, try_rev32_vec(*arr, v(5), v(6)), "rev32 v5.{t}, v6.{t}");
    }
    two! {
        b, VEC_BITS, "",
        rev16_vec "rev16";
        cnt "cnt";
        not "not";
        rbit_vec "rbit";
    }

    // UADDLP: the destination has half as many elements, twice as wide.
    let paired = ["4h", "8h", "2s", "4s", "1d", "2d"];
    for ((arr, t, ..), wide) in NOT_D.iter().zip(paired) {
        for [d, n] in tuples([32, 32]) {
            emit!(b, try_uaddlp(*arr, v(d), v(n)), "uaddlp v{d}.{wide}, v{n}.{t}");
        }
    }
    reject!(b, try_uaddlp(Arr::D2, v(5), v(6)), "uaddlp v5.2d, v6.2d");

    three! {
        b, VEC_BITS,
        and_vec "and";
        bic_vec "bic";
        orr_vec "orr";
        orn_vec "orn";
        eor_vec "eor";
        bsl "bsl";
        bit "bit";
        bif "bif";
    }
}

fn shifts(b: &mut Batch) {
    for (arr, t, ..) in ARRS {
        let bits = arr.elem().bits();
        for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
            let left = i as u32 % bits;
            let right = left + 1;
            emit!(b, try_shl(arr, v(d), v(n), left), "shl v{d}.{t}, v{n}.{t}, #{left}");
            emit!(b, try_sshr(arr, v(d), v(n), right), "sshr v{d}.{t}, v{n}.{t}, #{right}");
            emit!(b, try_ushr(arr, v(d), v(n), right), "ushr v{d}.{t}, v{n}.{t}, #{right}");
        }
        for left in 0..bits {
            let right = left + 1;
            emit!(b, try_shl(arr, v(5), v(6), left), "shl v5.{t}, v6.{t}, #{left}");
            emit!(b, try_sshr(arr, v(5), v(6), right), "sshr v5.{t}, v6.{t}, #{right}");
            emit!(b, try_ushr(arr, v(5), v(6), right), "ushr v5.{t}, v6.{t}, #{right}");
        }
        reject!(b, try_shl(arr, v(5), v(6), bits), "shl v5.{t}, v6.{t}, #{bits}");
        reject!(b, try_shl(arr, v(5), v(6), u32::MAX));
        for amount in [0, bits + 1] {
            reject!(b, try_sshr(arr, v(5), v(6), amount), "sshr v5.{t}, v6.{t}, #{amount}");
            reject!(b, try_ushr(arr, v(5), v(6), amount), "ushr v5.{t}, v6.{t}, #{amount}");
        }
        reject!(b, try_sshr(arr, v(5), v(6), u32::MAX));
        reject!(b, try_ushr(arr, v(5), v(6), u32::MAX));
    }

    // Widening and narrowing between (narrow, wide) arrangements.
    let widenings = [(Elem::B, "8b", "8h"), (Elem::H, "4h", "4s"), (Elem::S, "2s", "2d")];
    for (elem, narrow, wide) in widenings {
        let bits = elem.bits();
        for (i, [d, n]) in tuples([32, 32]).into_iter().enumerate() {
            let amount = i as u32 % bits;
            emit!(
                b,
                try_sshll(elem, v(d), v(n), amount),
                "sshll v{d}.{wide}, v{n}.{narrow}, #{amount}"
            );
            emit!(
                b,
                try_ushll(elem, v(d), v(n), amount),
                "ushll v{d}.{wide}, v{n}.{narrow}, #{amount}"
            );
            emit!(b, try_sxtl(elem, v(d), v(n)), "sxtl v{d}.{wide}, v{n}.{narrow}");
            emit!(b, try_uxtl(elem, v(d), v(n)), "uxtl v{d}.{wide}, v{n}.{narrow}");
            emit!(b, try_xtn(elem, v(d), v(n)), "xtn v{d}.{narrow}, v{n}.{wide}");
        }
        for amount in 0..bits {
            emit!(
                b,
                try_sshll(elem, v(5), v(6), amount),
                "sshll v5.{wide}, v6.{narrow}, #{amount}"
            );
            emit!(
                b,
                try_ushll(elem, v(5), v(6), amount),
                "ushll v5.{wide}, v6.{narrow}, #{amount}"
            );
        }
        reject!(b, try_sshll(elem, v(5), v(6), bits), "sshll v5.{wide}, v6.{narrow}, #{bits}");
        reject!(b, try_ushll(elem, v(5), v(6), bits), "ushll v5.{wide}, v6.{narrow}, #{bits}");
        reject!(b, try_sshll(elem, v(5), v(6), u32::MAX));
        reject!(b, try_ushll(elem, v(5), v(6), u32::MAX));
    }
    // There is no arrangement wider than 64-bit elements to write these
    // with.
    reject!(b, try_sshll(Elem::D, v(5), v(6), 0));
    reject!(b, try_ushll(Elem::D, v(5), v(6), 0));
    reject!(b, try_sxtl(Elem::D, v(5), v(6)));
    reject!(b, try_uxtl(Elem::D, v(5), v(6)));
    reject!(b, try_xtn(Elem::D, v(5), v(6)));
}

fn permutes(b: &mut Batch) {
    three! {
        b, ARRS,
        uzp1 "uzp1";
        uzp2 "uzp2";
        trn1 "trn1";
        trn2 "trn2";
        zip1 "zip1";
        zip2 "zip2";
    }
    for (bits, t) in VEC_BITS {
        let bytes = bits.bits() / 8;
        for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
            let index = i as u32 % bytes;
            emit!(
                b,
                try_ext(bits, v(d), v(n), v(m), index),
                "ext v{d}.{t}, v{n}.{t}, v{m}.{t}, #{index}"
            );
            let next = (n + 1) % 32;
            emit!(b, tbl1(bits, v(d), v(n), v(m)), "tbl v{d}.{t}, {{ v{n}.16b }}, v{m}.{t}");
            emit!(
                b,
                tbl2(bits, v(d), v(n), v(m)),
                "tbl v{d}.{t}, {{ v{n}.16b, v{next}.16b }}, v{m}.{t}"
            );
        }
        for index in 0..bytes {
            emit!(
                b,
                try_ext(bits, v(5), v(6), v(7), index),
                "ext v5.{t}, v6.{t}, v7.{t}, #{index}"
            );
        }
        // LLVM's assembler takes any index here and keeps its low bits, so
        // these have no text it refuses.
        for index in [bytes, bytes + 1, 16, 255, u32::MAX] {
            reject!(b, try_ext(bits, v(5), v(6), v(7), index));
        }
    }
}

fn reductions(b: &mut Batch) {
    // (arrangement, scalar result prefix, widened result prefix).
    const REDUCIBLE: [(Arr, &str, char, char); 5] = [
        (Arr::B8, "8b", 'b', 'h'),
        (Arr::B16, "16b", 'b', 'h'),
        (Arr::H4, "4h", 'h', 's'),
        (Arr::H8, "8h", 'h', 's'),
        (Arr::S4, "4s", 's', 'd'),
    ];
    macro_rules! reduce {
        ($($f:ident $mn:literal $result:tt;)*) => {$(
            for row in REDUCIBLE {
                let (arr, t, p) = (row.0, row.1, row.$result);
                for [d, n] in tuples([32, 32]) {
                    emit!(b, $f(arr, v(d), v(n)), "{} {p}{d}, v{n}.{t}", $mn);
                }
            }
            for (arr, t, p) in [(Arr::S2, "2s", 's'), (Arr::D2, "2d", 'd')] {
                reject!(b, $f(arr, v(5), v(6)), "{} {p}5, v6.{t}", $mn);
            }
        )*};
    }
    reduce! {
        try_addv "addv" 2;
        try_uaddlv "uaddlv" 3;
        try_smaxv "smaxv" 2;
        try_sminv "sminv" 2;
        try_umaxv "umaxv" 2;
        try_uminv "uminv" 2;
    }
}

fn floating_point(b: &mut Batch) {
    three! {
        b, FP_ARRS,
        fadd_vec "fadd";
        fsub_vec "fsub";
        fmul_vec "fmul";
        fdiv_vec "fdiv";
        fmax_vec "fmax";
        fmin_vec "fmin";
        fmaxnm_vec "fmaxnm";
        fminnm_vec "fminnm";
        fmla "fmla";
        fmls "fmls";
        fcmeq "fcmeq";
        fcmge "fcmge";
        fcmgt "fcmgt";
    }
    two! {
        b, FP_ARRS, "",
        fabs_vec "fabs";
        fneg_vec "fneg";
        fsqrt_vec "fsqrt";
        scvtf_vec "scvtf";
        ucvtf_vec "ucvtf";
        fcvtzs_vec "fcvtzs";
        fcvtzu_vec "fcvtzu";
        frintn_vec "frintn";
        frintm_vec "frintm";
        frintp_vec "frintp";
        frintz_vec "frintz";
        frinta_vec "frinta";
        frintx_vec "frintx";
        frinti_vec "frinti";
    }
    two! {
        b, FP_ARRS, ", #0.0",
        fcmeq_zero "fcmeq";
        fcmgt_zero "fcmgt";
        fcmge_zero "fcmge";
        fcmlt_zero "fcmlt";
        fcmle_zero "fcmle";
    }
    // (the wider precision, wide arrangement, narrow arrangement).
    for ((wide, _), (wt, nt)) in FP_SIZES.into_iter().zip([("4s", "4h"), ("2d", "2s")]) {
        for [d, n] in tuples([32, 32]) {
            emit!(b, fcvtl(wide, v(d), v(n)), "fcvtl v{d}.{wt}, v{n}.{nt}");
            emit!(b, fcvtn(wide, v(d), v(n)), "fcvtn v{d}.{nt}, v{n}.{wt}");
        }
    }
}

pub fn build(b: &mut Batch) {
    moves(b);
    immediates(b);
    integer(b);
    shifts(b);
    permutes(b);
    reductions(b);
    floating_point(b);
}
