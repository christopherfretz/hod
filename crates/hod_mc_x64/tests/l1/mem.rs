//! Sweeps for `spec/mc-x64.md` "Memory operands": every ModRM/SIB/RIP
//! form, with every register in the reg field, through loads and stores.

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{all_mems, r, reg};

/// Every memory form but RIP-relative with its displacement an absolute
/// field (`Mem::with_abs32`): always `disp32` (`{disp32}` makes LLVM do
/// the same), and the instruction's site.
pub fn abs32(b: &mut Batch) {
    for m in all_mems().into_iter().filter(|m| !m.is_rip()) {
        let field = m.mem().with_abs32().expect("only RIP-relative operands refuse");
        for n in 0..16 {
            let load = mov_rm(Size::Q, r(n), field);
            let site = load.site().expect("an absolute field is a site");
            assert_eq!(
                (site.offset, site.fixup, site.addend),
                (load.len() - 4, Fixup::Abs32, 0),
                "{m:?}"
            );
            emit!(
                b,
                mov_rm(Size::Q, r(n), field),
                "{{disp32}} mov {}, {}",
                reg(Size::Q, n),
                m.sized(Size::Q)
            );
        }
        emit!(b, mov_mi(Size::D, field, -2), "{{disp32}} mov {}, -2", m.sized(Size::D));
        for n in [0, 4, 12] {
            emit!(b, lea(Size::Q, r(n), field), "{{disp32}} lea {}, {}", reg(Size::Q, n), m.text());
        }
    }
}

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
        for n in [0, 5, 8, 15] {
            emit!(b, lea(Size::Q, r(n), m.mem()), "lea {}, {}", reg(Size::Q, n), m.text());
        }
    }
}
