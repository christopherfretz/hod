//! Sweeps for `spec/mc-x64.md` "Shifts".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{SIZES, WIDE, r, reg, some_mems};

const COUNTS: [u8; 12] = [0, 1, 2, 3, 7, 8, 15, 16, 31, 32, 63, 255];

pub fn build(b: &mut Batch) {
    macro_rules! shifts {
        ($($ri:ident $mi:ident $rcl:ident $mcl:ident $mn:literal;)*) => {$(
            for size in SIZES {
                for n in 0..16 {
                    for c in COUNTS {
                        emit!(b, $ri(size, r(n), c), "{} {}, {c}", $mn, reg(size, n));
                    }
                    emit!(b, $rcl(size, r(n)), "{} {}, cl", $mn, reg(size, n));
                }
                for m in some_mems() {
                    for c in [0, 1, 5, 255] {
                        emit!(b, $mi(size, m.mem(), c), "{} {}, {c}", $mn, m.sized(size));
                    }
                    emit!(b, $mcl(size, m.mem()), "{} {}, cl", $mn, m.sized(size));
                }
            }
        )*};
    }
    shifts! {
        rol_ri rol_mi rol_rcl rol_mcl "rol";
        ror_ri ror_mi ror_rcl ror_mcl "ror";
        rcl_ri rcl_mi rcl_rcl rcl_mcl "rcl";
        rcr_ri rcr_mi rcr_rcl rcr_mcl "rcr";
        shl_ri shl_mi shl_rcl shl_mcl "shl";
        shr_ri shr_mi shr_rcl shr_mcl "shr";
        sar_ri sar_mi sar_rcl sar_mcl "sar";
    }

    macro_rules! double {
        ($($rri:ident $rrcl:ident $mri:ident $mrcl:ident $mn:literal;)*) => {$(
            for size in WIDE {
                for d in 0..16 {
                    for s in 0..16 {
                        let c = COUNTS[(d as usize * 3 + s as usize) % COUNTS.len()];
                        emit!(b, $rri(size, r(d), r(s), c), "{} {}, {}, {c}", $mn, reg(size, d), reg(size, s));
                        emit!(b, $rrcl(size, r(d), r(s)), "{} {}, {}, cl", $mn, reg(size, d), reg(size, s));
                    }
                }
                for m in some_mems() {
                    for s in [0, 6, 11] {
                        emit!(b, $mri(size, m.mem(), r(s), 9), "{} {}, {}, 9", $mn, m.sized(size), reg(size, s));
                        emit!(b, $mrcl(size, m.mem(), r(s)), "{} {}, {}, cl", $mn, m.sized(size), reg(size, s));
                    }
                }
            }
            let m = Mem::base(Reg::Rax, 0);
            reject!(b, $rri(Size::B, r(0), r(1), 1), "{} al, cl, 1", $mn);
            reject!(b, $rrcl(Size::B, r(0), r(1)), "{} al, cl, cl", $mn);
            reject!(b, $mri(Size::B, m, r(1), 1), "{} byte ptr [rax], cl, 1", $mn);
            reject!(b, $mrcl(Size::B, m, r(1)), "{} byte ptr [rax], cl, cl", $mn);
        )*};
    }
    double! {
        try_shld_rri try_shld_rrcl try_shld_mri try_shld_mrcl "shld";
        try_shrd_rri try_shrd_rrcl try_shrd_mri try_shrd_mrcl "shrd";
    }
}
