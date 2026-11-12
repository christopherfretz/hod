//! Sweeps for `spec/mc-x64.md` "Bit tests".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{WIDE, r, reg, some_mems};

/// Bit indices: each end of each operand size and beyond it.
const BITS: [u8; 13] = [0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 255];

pub fn build(b: &mut Batch) {
    macro_rules! bits {
        ($($rr:ident $mr:ident $ri:ident $mi:ident $mn:literal;)*) => {$(
            for size in WIDE {
                for d in 0..16 {
                    for s in 0..16 {
                        emit!(b, $rr(size, r(d), r(s)), "{} {}, {}", $mn, reg(size, d), reg(size, s));
                    }
                    for bit in BITS {
                        emit!(b, $ri(size, r(d), bit), "{} {}, {bit}", $mn, reg(size, d));
                    }
                }
                for m in some_mems() {
                    for s in [0, 5, 8, 15] {
                        emit!(b, $mr(size, m.mem(), r(s)), "{} {}, {}", $mn, m.sized(size), reg(size, s));
                    }
                    for bit in [0, 9, 255] {
                        emit!(b, $mi(size, m.mem(), bit), "{} {}, {bit}", $mn, m.sized(size));
                    }
                }
            }
            let m = Mem::base(Reg::Rax, 0);
            reject!(b, $rr(Size::B, r(0), r(1)), "{} al, cl", $mn);
            reject!(b, $mr(Size::B, m, r(1)), "{} byte ptr [rax], cl", $mn);
            reject!(b, $ri(Size::B, r(0), 1), "{} al, 1", $mn);
            reject!(b, $mi(Size::B, m, 1), "{} byte ptr [rax], 1", $mn);
        )*};
    }
    bits! {
        try_bt_rr try_bt_mr try_bt_ri try_bt_mi "bt";
        try_bts_rr try_bts_mr try_bts_ri try_bts_mi "bts";
        try_btr_rr try_btr_mr try_btr_ri try_btr_mi "btr";
        try_btc_rr try_btc_mr try_btc_ri try_btc_mi "btc";
    }
}
