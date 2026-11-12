//! Sweeps for `spec/mc-a64.md` "Loads and stores".

use hod_mc_a64::*;

use crate::branch::{HUGE, bad_displacements, displacements};
use crate::harness::Batch;
use crate::text::{INDEXES, LIT_LOADS, LOADS, STORES, pz, rz, s, tuples, xs, xz, z};

/// Unsigned scaled offsets, in units of the access width.
pub const SCALED: [i64; 10] = [0, 1, 2, 3, 255, 256, 2047, 2048, 4094, 4095];
/// Signed 9-bit offsets.
pub const IMM9: [i64; 12] = [-256, -255, -128, -8, -1, 0, 1, 7, 8, 127, 254, 255];
pub const BAD_IMM9: [i64; 6] = [-257, 256, -4096, 4096, i64::MIN, i64::MAX];

/// The text after the index register: the extension and, when the index is
/// scaled, the shift amount. An unscaled `lsl` has no text at all (LLVM
/// refuses a bare `lsl`).
pub fn index_suffix(name: &str, amount: Option<u32>) -> String {
    match amount {
        Some(amount) => format!(", {name} #{amount}"),
        None if name == "lsl" => String::new(),
        None => format!(", {name}"),
    }
}

// Each sweep below runs once over `LOADS` and once over `STORES`, whose rows
// are `(kind, mnemonic, unscaled mnemonic, register prefix, bytes)`.

/// Unsigned scaled offsets.
fn scaled(b: &mut Batch) {
    macro_rules! sweep {
        ($kinds:ident: $off:ident) => {
            for (kind, mn, _, prefix, bytes) in $kinds {
                let (t5, n6) = (pz(prefix, 5), xs(6));
                for (i, [t, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let offset = SCALED[i % SCALED.len()] * bytes;
                    let (t_, n_) = (pz(prefix, t), xs(n));
                    emit_as!(b, mn, $off(kind, z(t), s(n), offset), "{mn} {t_}, [{n_}, #{offset}]");
                }
                for units in SCALED {
                    let offset = units * bytes;
                    emit_as!(b, mn, $off(kind, z(5), s(6), offset), "{mn} {t5}, [{n6}, #{offset}]");
                }
                // One past the end is an error for LLVM too; a negative or
                // misaligned offset LLVM re-expresses as the unscaled form
                // when it fits there, so those have no text.
                let past = 4096 * bytes;
                reject!(b, $off(kind, z(5), s(6), past), "{mn} {t5}, [{n6}, #{past}]");
                for offset in [-bytes, -1, -256, i64::MIN, i64::MAX, 1 << 32, past + bytes] {
                    reject!(b, $off(kind, z(5), s(6), offset));
                }
                if bytes > 1 {
                    for offset in [1, bytes - 1, bytes + 1, 4095 * bytes - 1] {
                        reject!(b, $off(kind, z(5), s(6), offset));
                    }
                    // Misaligned and outside the unscaled range: no form.
                    let odd = 256 * bytes + 1;
                    reject!(b, $off(kind, z(5), s(6), odd), "{mn} {t5}, [{n6}, #{odd}]");
                }
            }
        };
    }
    sweep!(LOADS: try_ldr_off);
    sweep!(STORES: try_str_off);
}

/// Signed unscaled offsets.
fn unscaled(b: &mut Batch) {
    macro_rules! sweep {
        ($kinds:ident: $unscaled:ident) => {
            for (kind, _, mn, prefix, _) in $kinds {
                let (t5, n6) = (pz(prefix, 5), xs(6));
                for (i, [t, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let offset = IMM9[i % IMM9.len()];
                    let (t_, n_) = (pz(prefix, t), xs(n));
                    emit_as!(
                        b,
                        mn,
                        $unscaled(kind, z(t), s(n), offset),
                        "{mn} {t_}, [{n_}, #{offset}]"
                    );
                }
                for offset in -256..=255 {
                    emit_as!(
                        b,
                        mn,
                        $unscaled(kind, z(5), s(6), offset),
                        "{mn} {t5}, [{n6}, #{offset}]"
                    );
                }
                for offset in BAD_IMM9 {
                    reject!(b, $unscaled(kind, z(5), s(6), offset));
                }
                for offset in [-257, 256] {
                    reject!(b, $unscaled(kind, z(5), s(6), offset), "{mn} {t5}, [{n6}, #{offset}]");
                }
            }
        };
    }
    sweep!(LOADS: try_ldur);
    sweep!(STORES: try_stur);
}

/// Pre- and post-indexed. The transferred register and the base must
/// differ, except that base 31 is SP while transferred 31 is ZR.
fn writeback(b: &mut Batch) {
    macro_rules! sweep {
        ($kinds:ident: $pre:ident $post:ident) => {
            for (kind, mn, _, prefix, _) in $kinds {
                let (t5, n6) = (pz(prefix, 5), xs(6));
                for (i, [t, n]) in tuples([32, 32]).into_iter().enumerate() {
                    let offset = IMM9[i % IMM9.len()];
                    let pre = format!("{mn} {}, [{}, #{offset}]!", pz(prefix, t), xs(n));
                    let post = format!("{mn} {}, [{}], #{offset}", pz(prefix, t), xs(n));
                    if t == n && n != 31 {
                        reject!(b, $pre(kind, z(t), s(n), offset), "{pre}");
                        reject!(b, $post(kind, z(t), s(n), offset), "{post}");
                    } else {
                        emit_as!(b, mn, $pre(kind, z(t), s(n), offset), "{pre}");
                        emit_as!(b, mn, $post(kind, z(t), s(n), offset), "{post}");
                    }
                }
                for offset in IMM9 {
                    emit_as!(
                        b,
                        mn,
                        $pre(kind, z(5), s(6), offset),
                        "{mn} {t5}, [{n6}, #{offset}]!"
                    );
                    emit_as!(
                        b,
                        mn,
                        $post(kind, z(5), s(6), offset),
                        "{mn} {t5}, [{n6}], #{offset}"
                    );
                }
                for offset in [-257, 256] {
                    reject!(b, $pre(kind, z(5), s(6), offset), "{mn} {t5}, [{n6}, #{offset}]!");
                    reject!(b, $post(kind, z(5), s(6), offset), "{mn} {t5}, [{n6}], #{offset}");
                }
                for offset in BAD_IMM9 {
                    reject!(b, $pre(kind, z(5), s(6), offset));
                    reject!(b, $post(kind, z(5), s(6), offset));
                }
            }
        };
    }
    sweep!(LOADS: try_ldr_pre try_ldr_post);
    sweep!(STORES: try_str_pre try_str_post);
}

/// Register offset: every extension, unscaled and scaled by the access
/// width.
fn register(b: &mut Batch) {
    macro_rules! sweep {
        ($kinds:ident: $reg:ident) => {
            for (kind, mn, _, prefix, bytes) in $kinds {
                let (t5, n6) = (pz(prefix, 5), xs(6));
                let amount = bytes.trailing_zeros();
                for (i, [t, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    let (index, name, m_prefix) = INDEXES[i % 4];
                    let scaled = i / 4 % 2 == 1;
                    emit_as!(
                        b,
                        mn,
                        $reg(kind, z(t), s(n), z(m), index, scaled),
                        "{mn} {}, [{}, {}{}]",
                        pz(prefix, t),
                        xs(n),
                        pz(m_prefix, m),
                        index_suffix(name, scaled.then_some(amount))
                    );
                }
                for (index, name, m_prefix) in INDEXES {
                    let m7 = pz(m_prefix, 7);
                    emit_as!(
                        b,
                        mn,
                        $reg(kind, z(5), s(6), z(7), index, false),
                        "{mn} {t5}, [{n6}, {m7}{}]",
                        index_suffix(name, None)
                    );
                    emit_as!(
                        b,
                        mn,
                        $reg(kind, z(5), s(6), z(7), index, true),
                        "{mn} {t5}, [{n6}, {m7}{}]",
                        index_suffix(name, Some(amount))
                    );
                }
                // An explicit `lsl #0` is the unscaled form, except on a
                // byte access, where LLVM takes it as "scaled by one byte"
                // and sets the S bit (swept above as `scaled`).
                if bytes > 1 {
                    emit_as!(
                        b,
                        mn,
                        $reg(kind, z(5), s(6), z(7), Index::Lsl, false),
                        "{mn} {t5}, [{n6}, {}, lsl #0]",
                        xz(7)
                    );
                }
            }
        };
    }
    sweep!(LOADS: ldr_reg);
    sweep!(STORES: str_reg);
}

fn literal(b: &mut Batch) {
    let disps = displacements(Fixup::Literal19, 60);
    for (kind, mn, prefix) in LIT_LOADS {
        for (i, &disp) in disps.iter().enumerate() {
            let t = (i % 32) as u32;
            emit_as!(b, mn, try_ldr_lit(kind, z(t), disp), "{mn} {}, #{disp}", pz(prefix, t));
        }
        for disp in bad_displacements(Fixup::Literal19) {
            reject!(b, try_ldr_lit(kind, z(7), disp), "{mn} {}, #{disp}", pz(prefix, 7));
        }
        for disp in HUGE {
            reject!(b, try_ldr_lit(kind, z(7), disp));
        }
    }
}

fn pairs(b: &mut Batch) {
    // `$sz` is the register size in the text and `$scale` the element
    // width; `$args` passes the size operand to the constructor (LDPSW has
    // none). `$load` says the two transferred registers must differ.
    macro_rules! pair {
        ($mn:literal $sz:expr, $scale:literal, $load:literal:
         $off:ident $pre:ident $post:ident ($($args:tt)*)) => {{
            let sz: Size = $sz;
            let scale: i64 = $scale;
            for (i, [t, t2, n]) in tuples([32, 32, 32]).into_iter().enumerate() {
                let offset = (i as i64 * 7 % 128 - 64) * scale;
                let (t_, t2_, n_) = (rz(sz, t), rz(sz, t2), xs(n));
                if $load && t == t2 {
                    reject!(
                        b,
                        $off($($args)* z(t), z(t2), s(n), offset),
                        "{} {t_}, {t2_}, [{n_}, #{offset}]", $mn
                    );
                    continue;
                }
                emit!(
                    b,
                    $off($($args)* z(t), z(t2), s(n), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]", $mn
                );
                if n != 31 && (t == n || t2 == n) {
                    reject!(
                        b,
                        $pre($($args)* z(t), z(t2), s(n), offset),
                        "{} {t_}, {t2_}, [{n_}, #{offset}]!", $mn
                    );
                    if $mn == "ldpsw" {
                        // LLVM 22 diagnoses the overlap on every pair load
                        // with writeback except post-indexed LDPSW.
                        reject!(b, $post($($args)* z(t), z(t2), s(n), offset));
                    } else {
                        reject!(
                            b,
                            $post($($args)* z(t), z(t2), s(n), offset),
                            "{} {t_}, {t2_}, [{n_}], #{offset}", $mn
                        );
                    }
                    continue;
                }
                emit!(
                    b,
                    $pre($($args)* z(t), z(t2), s(n), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]!", $mn
                );
                emit!(
                    b,
                    $post($($args)* z(t), z(t2), s(n), offset),
                    "{} {t_}, {t2_}, [{n_}], #{offset}", $mn
                );
            }
            let (t_, t2_, n_) = (rz(sz, 5), rz(sz, 6), xs(31));
            for units in -64..=63 {
                let offset = units * scale;
                emit!(
                    b,
                    $off($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]", $mn
                );
                emit!(
                    b,
                    $pre($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]!", $mn
                );
                emit!(
                    b,
                    $post($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}], #{offset}", $mn
                );
            }
            // Out of range by one element each way, and misaligned.
            for offset in [-65 * scale, 64 * scale, 1, scale / 2, scale + 1, -1] {
                reject!(
                    b,
                    $off($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]", $mn
                );
                reject!(
                    b,
                    $pre($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}, #{offset}]!", $mn
                );
                reject!(
                    b,
                    $post($($args)* z(5), z(6), s(31), offset),
                    "{} {t_}, {t2_}, [{n_}], #{offset}", $mn
                );
            }
            for offset in [i64::MIN, i64::MAX, 1 << 32, -(1 << 32)] {
                reject!(b, $off($($args)* z(5), z(6), s(31), offset));
                reject!(b, $pre($($args)* z(5), z(6), s(31), offset));
                reject!(b, $post($($args)* z(5), z(6), s(31), offset));
            }
        }};
    }
    pair!("ldp" Size::W, 4, true: try_ldp_off try_ldp_pre try_ldp_post (Size::W,));
    pair!("ldp" Size::X, 8, true: try_ldp_off try_ldp_pre try_ldp_post (Size::X,));
    pair!("stp" Size::W, 4, false: try_stp_off try_stp_pre try_stp_post (Size::W,));
    pair!("stp" Size::X, 8, false: try_stp_off try_stp_pre try_stp_post (Size::X,));
    pair!("ldpsw" Size::X, 4, true: try_ldpsw_off try_ldpsw_pre try_ldpsw_post ());
}

pub fn build(b: &mut Batch) {
    scaled(b);
    unscaled(b);
    writeback(b);
    register(b);
    literal(b);
    pairs(b);
}
