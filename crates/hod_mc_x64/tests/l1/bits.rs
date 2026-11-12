//! Sweeps for `spec/mc-x64.md` "Bit counting".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{WIDE, r, reg, some_mems};

pub fn build(b: &mut Batch) {
    macro_rules! count {
        ($($rr:ident $rm:ident $mn:literal;)*) => {$(
            for size in WIDE {
                for d in 0..16 {
                    for s in 0..16 {
                        emit!(b, $rr(size, r(d), r(s)), "{} {}, {}", $mn, reg(size, d), reg(size, s));
                    }
                }
                for m in some_mems() {
                    for d in [0, 3, 8, 14] {
                        emit!(b, $rm(size, r(d), m.mem()), "{} {}, {}", $mn, reg(size, d), m.sized(size));
                    }
                }
            }
            reject!(b, $rr(Size::B, r(0), r(1)), "{} al, cl", $mn);
            reject!(b, $rm(Size::B, r(0), Mem::base(Reg::Rax, 0)), "{} al, byte ptr [rax]", $mn);
        )*};
    }
    count! {
        try_bsf_rr try_bsf_rm "bsf";
        try_bsr_rr try_bsr_rm "bsr";
        try_tzcnt_rr try_tzcnt_rm "tzcnt";
        try_lzcnt_rr try_lzcnt_rm "lzcnt";
        try_popcnt_rr try_popcnt_rm "popcnt";
    }
    for size in [Size::D, Size::Q] {
        for n in 0..16 {
            emit!(b, try_bswap(size, r(n)), "bswap {}", reg(size, n));
        }
    }
    reject!(b, try_bswap(Size::B, r(0)), "bswap al");
    reject!(b, try_bswap(Size::W, r(0)), "bswap ax");
}
