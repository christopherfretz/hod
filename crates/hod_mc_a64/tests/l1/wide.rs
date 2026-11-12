//! Sweeps for `spec/mc-a64.md` "Moves": MOVZ, MOVN, MOVK.

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{rz, z};

const IMMS: [u16; 8] = [0, 1, 0x00ff, 0x1234, 0x8000, 0xaaaa, 0xfffe, 0xffff];

pub fn build(b: &mut Batch) {
    macro_rules! wide {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for shift in (0..sz.bits()).step_by(16) {
                    for d in 0..32 {
                        let imm = IMMS[(d as usize + shift as usize / 16) % IMMS.len()];
                        emit!(
                            b,
                            $f(sz, z(d), imm, shift),
                            "{} {}, #{imm}, lsl #{shift}", $mn, rz(sz, d)
                        );
                    }
                    for imm in IMMS {
                        emit!(
                            b,
                            $f(sz, z(11), imm, shift),
                            "{} {}, #{imm}, lsl #{shift}", $mn, rz(sz, 11)
                        );
                    }
                }
                for shift in [8, 12, 17, 64, 80] {
                    reject!(
                        b,
                        $f(sz, z(11), 1, shift),
                        "{} {}, #1, lsl #{shift}", $mn, rz(sz, 11)
                    );
                }
            }
            for shift in [32, 48] {
                reject!(b, $f(Size::W, z(11), 1, shift), "{} w11, #1, lsl #{shift}", $mn);
            }
        )*};
    }
    wide! {
        try_movz "movz";
        try_movn "movn";
        try_movk "movk";
    }
}
