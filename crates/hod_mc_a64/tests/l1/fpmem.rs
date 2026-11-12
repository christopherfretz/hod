//! Sweeps for `spec/mc-a64.md` "FP/SIMD loads and stores".

use hod_mc_a64::*;

use crate::branch::{HUGE, bad_displacements, displacements};
use crate::harness::Batch;
use crate::mem::{BAD_IMM9, IMM9, SCALED, index_suffix};
use crate::text::{INDEXES, VMEM_SIZES, VPAIR_SIZES, pz, s, tuples, v, xs, xz, z};

/// Unsigned scaled offsets.
fn scaled(b: &mut Batch) {
    macro_rules! sweep {
        ($mn:literal $off:ident) => {
            for (sz, p, bytes) in VMEM_SIZES {
                let n6 = xs(6);
                for (i, [t, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let offset = SCALED[i % SCALED.len()] * bytes;
                    emit!(
                        b,
                        $off(sz, v(t), s(n), offset),
                        "{} {p}{t}, [{}, #{offset}]",
                        $mn,
                        xs(n)
                    );
                }
                for units in SCALED {
                    let offset = units * bytes;
                    emit!(b, $off(sz, v(5), s(6), offset), "{} {p}5, [{n6}, #{offset}]", $mn);
                }
                // One past the end is an error for LLVM too; a negative or
                // misaligned offset LLVM re-expresses as the unscaled form
                // when it fits there, so those have no text.
                let past = 4096 * bytes;
                reject!(b, $off(sz, v(5), s(6), past), "{} {p}5, [{n6}, #{past}]", $mn);
                for offset in [-bytes, -1, -256, i64::MIN, i64::MAX, 1 << 32, past + bytes] {
                    reject!(b, $off(sz, v(5), s(6), offset));
                }
                if bytes > 1 {
                    for offset in [1, bytes - 1, bytes + 1, 4095 * bytes - 1] {
                        reject!(b, $off(sz, v(5), s(6), offset));
                    }
                    // Misaligned and outside the unscaled range: no form.
                    let odd = 256 * bytes + 1;
                    reject!(b, $off(sz, v(5), s(6), odd), "{} {p}5, [{n6}, #{odd}]", $mn);
                }
            }
        };
    }
    sweep!("ldr" try_ldr_fp_off);
    sweep!("str" try_str_fp_off);
}

/// Signed unscaled offsets, and the two writeback forms, which share the
/// offset field. An FP/SIMD register is never the base, so writeback has
/// no register constraint.
fn imm9(b: &mut Batch) {
    macro_rules! sweep {
        ($mn:literal $unscaled:literal $f:ident $pre:ident $post:ident) => {
            for (sz, p, _) in VMEM_SIZES {
                let n6 = xs(6);
                for (i, [t, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let offset = IMM9[i % IMM9.len()];
                    let n_ = xs(n);
                    emit!(b, $f(sz, v(t), s(n), offset), "{} {p}{t}, [{n_}, #{offset}]", $unscaled);
                    emit!(b, $pre(sz, v(t), s(n), offset), "{} {p}{t}, [{n_}, #{offset}]!", $mn);
                    emit!(b, $post(sz, v(t), s(n), offset), "{} {p}{t}, [{n_}], #{offset}", $mn);
                }
                for offset in -256..=255 {
                    emit!(b, $f(sz, v(5), s(6), offset), "{} {p}5, [{n6}, #{offset}]", $unscaled);
                    emit!(b, $pre(sz, v(5), s(6), offset), "{} {p}5, [{n6}, #{offset}]!", $mn);
                    emit!(b, $post(sz, v(5), s(6), offset), "{} {p}5, [{n6}], #{offset}", $mn);
                }
                for offset in [-257, 256] {
                    reject!(b, $f(sz, v(5), s(6), offset), "{} {p}5, [{n6}, #{offset}]", $unscaled);
                    reject!(b, $pre(sz, v(5), s(6), offset), "{} {p}5, [{n6}, #{offset}]!", $mn);
                    reject!(b, $post(sz, v(5), s(6), offset), "{} {p}5, [{n6}], #{offset}", $mn);
                }
                for offset in BAD_IMM9 {
                    reject!(b, $f(sz, v(5), s(6), offset));
                    reject!(b, $pre(sz, v(5), s(6), offset));
                    reject!(b, $post(sz, v(5), s(6), offset));
                }
            }
        };
    }
    sweep!("ldr" "ldur" try_ldur_fp try_ldr_fp_pre try_ldr_fp_post);
    sweep!("str" "stur" try_stur_fp try_str_fp_pre try_str_fp_post);
}

/// Register offset: every extension, unscaled and scaled by the access
/// width.
fn register(b: &mut Batch) {
    macro_rules! sweep {
        ($mn:literal $reg:ident) => {
            for (sz, p, bytes) in VMEM_SIZES {
                let n6 = xs(6);
                let amount = bytes.trailing_zeros();
                for (i, [t, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    let (index, name, m_prefix) = INDEXES[i % 4];
                    let scaled = i / 4 % 2 == 1;
                    emit!(
                        b,
                        $reg(sz, v(t), s(n), z(m), index, scaled),
                        "{} {p}{t}, [{}, {}{}]",
                        $mn,
                        xs(n),
                        pz(m_prefix, m),
                        index_suffix(name, scaled.then_some(amount))
                    );
                }
                for (index, name, m_prefix) in INDEXES {
                    let m7 = pz(m_prefix, 7);
                    emit!(
                        b,
                        $reg(sz, v(5), s(6), z(7), index, false),
                        "{} {p}5, [{n6}, {m7}{}]",
                        $mn,
                        index_suffix(name, None)
                    );
                    emit!(
                        b,
                        $reg(sz, v(5), s(6), z(7), index, true),
                        "{} {p}5, [{n6}, {m7}{}]",
                        $mn,
                        index_suffix(name, Some(amount))
                    );
                }
                // An explicit `lsl #0` is the unscaled form, except on a
                // byte access, where LLVM takes it as "scaled by one byte"
                // and sets the S bit (swept above as `scaled`).
                if bytes > 1 {
                    emit!(
                        b,
                        $reg(sz, v(5), s(6), z(7), Index::Lsl, false),
                        "{} {p}5, [{n6}, {}, lsl #0]",
                        $mn,
                        xz(7)
                    );
                }
            }
        };
    }
    sweep!("ldr" ldr_fp_reg);
    sweep!("str" str_fp_reg);
}

fn literal(b: &mut Batch) {
    let disps = displacements(Fixup::Literal19, 60);
    for (sz, p, _) in VPAIR_SIZES {
        for (i, &disp) in disps.iter().enumerate() {
            let t = (i % 32) as u32;
            emit!(b, try_ldr_fp_lit(sz, v(t), disp), "ldr {p}{t}, #{disp}");
        }
        for disp in bad_displacements(Fixup::Literal19) {
            reject!(b, try_ldr_fp_lit(sz, v(7), disp), "ldr {p}7, #{disp}");
        }
        for disp in HUGE {
            reject!(b, try_ldr_fp_lit(sz, v(7), disp));
        }
    }
}

/// Pairs. A load pair into one register twice is refused; the base is a
/// general-purpose register and cannot overlap.
fn pairs(b: &mut Batch) {
    macro_rules! pair {
        ($mn:literal $load:literal: $off:ident $pre:ident $post:ident) => {
            for (sz, p, scale) in VPAIR_SIZES {
                for (i, [t, t2, n]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    let offset = (i as i64 * 7 % 128 - 64) * scale;
                    let n_ = xs(n);
                    let off = format!("{} {p}{t}, {p}{t2}, [{n_}, #{offset}]", $mn);
                    let pre = format!("{} {p}{t}, {p}{t2}, [{n_}, #{offset}]!", $mn);
                    let post = format!("{} {p}{t}, {p}{t2}, [{n_}], #{offset}", $mn);
                    if $load && t == t2 {
                        reject!(b, $off(sz, v(t), v(t2), s(n), offset), "{off}");
                        reject!(b, $pre(sz, v(t), v(t2), s(n), offset), "{pre}");
                        reject!(b, $post(sz, v(t), v(t2), s(n), offset), "{post}");
                        continue;
                    }
                    emit!(b, $off(sz, v(t), v(t2), s(n), offset), "{off}");
                    emit!(b, $pre(sz, v(t), v(t2), s(n), offset), "{pre}");
                    emit!(b, $post(sz, v(t), v(t2), s(n), offset), "{post}");
                }
                let n_ = xs(31);
                for units in -64..=63 {
                    let offset = units * scale;
                    emit!(
                        b,
                        $off(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}, #{offset}]",
                        $mn
                    );
                    emit!(
                        b,
                        $pre(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}, #{offset}]!",
                        $mn
                    );
                    emit!(
                        b,
                        $post(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}], #{offset}",
                        $mn
                    );
                }
                // Out of range by one element each way, and misaligned.
                for offset in [-65 * scale, 64 * scale, 1, scale / 2, scale + 1, -1] {
                    reject!(
                        b,
                        $off(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}, #{offset}]",
                        $mn
                    );
                    reject!(
                        b,
                        $pre(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}, #{offset}]!",
                        $mn
                    );
                    reject!(
                        b,
                        $post(sz, v(5), v(6), s(31), offset),
                        "{} {p}5, {p}6, [{n_}], #{offset}",
                        $mn
                    );
                }
                for offset in [i64::MIN, i64::MAX, 1 << 32, -(1 << 32)] {
                    reject!(b, $off(sz, v(5), v(6), s(31), offset));
                    reject!(b, $pre(sz, v(5), v(6), s(31), offset));
                    reject!(b, $post(sz, v(5), v(6), s(31), offset));
                }
            }
        };
    }
    pair!("ldp" true: try_ldp_fp_off try_ldp_fp_pre try_ldp_fp_post);
    pair!("stp" false: try_stp_fp_off try_stp_fp_pre try_stp_fp_post);
}

pub fn build(b: &mut Batch) {
    scaled(b);
    imm9(b);
    register(b);
    literal(b);
    pairs(b);
}
