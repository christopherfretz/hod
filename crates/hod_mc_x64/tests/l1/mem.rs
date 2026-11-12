//! Sweeps for `spec/mc-x64.md` "Memory operands": every ModRM/SIB/RIP
//! form, with every register in the reg field, through loads and stores.

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{all_mems, r, reg};

pub fn build(b: &mut Batch) {
    for m in all_mems() {
        for n in 0..16 {
            emit!(
                b,
                mov_rm(Size::Q, r(n), m.mem()),
                "mov {}, {}",
                reg(Size::Q, n),
                m.sized(Size::Q)
            );
            emit!(
                b,
                mov_mr(Size::B, m.mem(), r(n)),
                "mov {}, {}",
                m.sized(Size::B),
                reg(Size::B, n)
            );
        }
        if m.seg.is_none() {
            for n in [0, 5, 8, 15] {
                emit!(b, lea(Size::Q, r(n), m.mem()), "lea {}, {}", reg(Size::Q, n), m.text());
            }
        }
    }
}
