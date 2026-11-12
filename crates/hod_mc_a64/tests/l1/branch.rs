//! Sweeps for `spec/mc-a64.md` "Branches" and "Address generation". The
//! displacement is written as an immediate here; `fixups.rs` checks the
//! same fields through labels.

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{CONDS, Rng, rz, wz, xz, z};

/// Displacements for a field: both ends, the values next to them and to
/// zero, and a pseudo-random spread, all multiples of the field's unit.
pub fn displacements(kind: Fixup, extra: usize) -> Vec<i64> {
    let unit = unit(kind);
    let (min, max) = (kind.min(), kind.max());
    let mut out = vec![min, min + unit, min + 2 * unit, -2 * unit, -unit, 0, unit, 2 * unit];
    out.extend([max - 2 * unit, max - unit, max]);
    let mut rng = Rng::new(max as u64);
    for _ in 0..extra {
        out.push(min + rng.below(((max - min) / unit + 1) as u64) as i64 * unit);
    }
    out
}

/// The unit a field counts in, in bytes, stated independently of the
/// crate.
pub fn unit(kind: Fixup) -> i64 {
    match kind {
        Fixup::Jump26 | Fixup::Branch19 | Fixup::Test14 | Fixup::Literal19 => 4,
        Fixup::Adr21 => 1,
        Fixup::Adrp21 => 4096,
    }
}

/// Displacements no field holds, too large to write in assembly text.
pub const HUGE: [i64; 4] = [i64::MIN, i64::MAX, 1 << 40, -(1 << 40)];

/// Displacements outside a field: one unit past each end, far past, and
/// (where the unit is not 1) in range but not a multiple of the unit.
pub fn bad_displacements(kind: Fixup) -> Vec<i64> {
    let unit = unit(kind);
    let (min, max) = (kind.min(), kind.max());
    let mut out = vec![min - unit, max + unit, min * 2, max * 2 + unit];
    if unit > 1 {
        out.extend([1, 2, -1, unit / 2, unit + 1, max - 1, min + 1, max - unit / 2]);
    }
    out
}

fn direct(b: &mut Batch) {
    for disp in displacements(Fixup::Jump26, 200) {
        emit!(b, try_b(disp), "b #{disp}");
        emit!(b, try_bl(disp), "bl #{disp}");
    }
    for disp in bad_displacements(Fixup::Jump26) {
        reject!(b, try_b(disp), "b #{disp}");
        reject!(b, try_bl(disp), "bl #{disp}");
    }
    for disp in HUGE {
        reject!(b, try_b(disp));
        reject!(b, try_bl(disp));
        reject!(b, try_b_cond(Cond::Ne, disp));
        reject!(b, try_cbz(Size::X, z(7), disp));
        reject!(b, try_cbnz(Size::X, z(7), disp));
        reject!(b, try_tbz(z(7), 0, disp));
        reject!(b, try_tbnz(z(7), 0, disp));
        reject!(b, try_adr(z(7), disp));
        reject!(b, try_adrp(z(7), disp));
    }

    let disps = displacements(Fixup::Branch19, 60);
    for (cond, name) in CONDS {
        for &disp in &disps {
            emit_as!(b, format!("b.{name}"), try_b_cond(cond, disp), "b.{name} #{disp}");
        }
    }
    for disp in bad_displacements(Fixup::Branch19) {
        reject!(b, try_b_cond(Cond::Ne, disp), "b.ne #{disp}");
    }
    macro_rules! compare_branch {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for t in 0..32 {
                    let disp = disps[t as usize % disps.len()];
                    emit!(b, $f(sz, z(t), disp), "{} {}, #{disp}", $mn, rz(sz, t));
                }
                for &disp in &disps {
                    emit!(b, $f(sz, z(7), disp), "{} {}, #{disp}", $mn, rz(sz, 7));
                }
                for disp in bad_displacements(Fixup::Branch19) {
                    reject!(b, $f(sz, z(7), disp), "{} {}, #{disp}", $mn, rz(sz, 7));
                }
            }
        )*};
    }
    compare_branch! {
        try_cbz "cbz";
        try_cbnz "cbnz";
    }

    // The register of a test-and-branch is written `W` for bits 0..=31 and
    // `X` above.
    let bit_reg = |bit: u32, t: u32| if bit < 32 { wz(t) } else { xz(t) };
    let disps = displacements(Fixup::Test14, 40);
    macro_rules! test_branch {
        ($($f:ident $mn:literal;)*) => {$(
            for bit in 0..64 {
                for t in [0, 7, 30, 31] {
                    let disp = disps[(bit + t) as usize % disps.len()];
                    emit!(b, $f(z(t), bit, disp), "{} {}, #{bit}, #{disp}", $mn, bit_reg(bit, t));
                }
            }
            for t in 0..32 {
                emit!(b, $f(z(t), 3, 8), "{} {}, #3, #8", $mn, wz(t));
                emit!(b, $f(z(t), 3, 8), "{} {}, #3, #8", $mn, xz(t));
                emit!(b, $f(z(t), 42, -8), "{} {}, #42, #-8", $mn, xz(t));
            }
            for &disp in &disps {
                emit!(b, $f(z(7), 63, disp), "{} x7, #63, #{disp}", $mn);
            }
            for disp in bad_displacements(Fixup::Test14) {
                reject!(b, $f(z(7), 0, disp), "{} w7, #0, #{disp}", $mn);
            }
            reject!(b, $f(z(7), 64, 0), "{} x7, #64, #0", $mn);
            reject!(b, $f(z(7), u32::MAX, 0));
        )*};
    }
    test_branch! {
        try_tbz "tbz";
        try_tbnz "tbnz";
    }
}

fn indirect(b: &mut Batch) {
    for n in 0..32 {
        emit!(b, br(z(n)), "br {}", xz(n));
        emit!(b, blr(z(n)), "blr {}", xz(n));
        emit!(b, ret(z(n)), "ret {}", xz(n));
    }
    emit!(b, ret(Reg::LR.zr()), "ret");
}

fn addresses(b: &mut Batch) {
    let disps = displacements(Fixup::Adr21, 200);
    for (i, &disp) in disps.iter().enumerate() {
        let d = (i % 32) as u32;
        emit!(b, try_adr(z(d), disp), "adr {}, #{disp}", xz(d));
    }
    for disp in bad_displacements(Fixup::Adr21) {
        reject!(b, try_adr(z(7), disp), "adr x7, #{disp}");
    }
    let disps = displacements(Fixup::Adrp21, 200);
    for (i, &disp) in disps.iter().enumerate() {
        let d = (i % 32) as u32;
        emit!(b, try_adrp(z(d), disp), "adrp {}, #{disp}", xz(d));
    }
    for disp in bad_displacements(Fixup::Adrp21) {
        reject!(b, try_adrp(z(7), disp), "adrp x7, #{disp}");
    }
}

pub fn build(b: &mut Batch) {
    direct(b);
    indirect(b);
    addresses(b);
}
