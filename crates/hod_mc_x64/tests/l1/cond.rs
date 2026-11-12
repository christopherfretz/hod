//! Sweeps for `spec/mc-x64.md` "Conditional".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{CONDS, WIDE, r, reg, some_mems};

pub fn build(b: &mut Batch) {
    for (cond, cc) in CONDS {
        let set = format!("set{cc}");
        let cmov = format!("cmov{cc}");
        for n in 0..16 {
            emit_as!(b, &set, setcc_r(cond, r(n)), "{set} {}", reg(Size::B, n));
        }
        for m in some_mems() {
            emit_as!(b, &set, setcc_m(cond, m.mem()), "{set} {}", m.sized(Size::B));
        }
        for size in WIDE {
            for d in 0..16 {
                for s in 0..16 {
                    emit_as!(
                        b,
                        &cmov,
                        try_cmovcc_rr(cond, size, r(d), r(s)),
                        "{cmov} {}, {}",
                        reg(size, d),
                        reg(size, s)
                    );
                }
            }
            for m in some_mems() {
                for d in [0, 5, 12] {
                    emit_as!(
                        b,
                        &cmov,
                        try_cmovcc_rm(cond, size, r(d), m.mem()),
                        "{cmov} {}, {}",
                        reg(size, d),
                        m.sized(size)
                    );
                }
            }
        }
        reject!(b, try_cmovcc_rr(cond, Size::B, r(0), r(1)), "{cmov} al, cl");
        reject!(
            b,
            try_cmovcc_rm(cond, Size::B, r(0), Mem::base(Reg::Rax, 0)),
            "{cmov} al, byte ptr [rax]"
        );
    }
}
