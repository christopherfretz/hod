//! Sweeps for `spec/mc-x64.md` "Processor state and extensions"
//! (WP-190).

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{r, reg, some_mems};

pub fn build(b: &mut Batch) {
    for m in some_mems() {
        let t = m.text();
        for op in XState::ALL {
            let mn = op.mnemonic();
            emit_as!(b, mn, xstate_m(op, m.mem()), "{mn} {t}");
        }
        emit!(b, clflushopt_m(m.mem()), "clflushopt byte ptr {t}");
        emit!(b, clwb_m(m.mem()), "clwb byte ptr {t}");
    }
    for n in 0..16 {
        for size in [Size::W, Size::D, Size::Q] {
            let name = reg(size, n);
            emit!(b, try_rdrand_r(size, r(n)), "rdrand {name}");
            emit!(b, try_rdseed_r(size, r(n)), "rdseed {name}");
        }
        for size in [Size::D, Size::Q] {
            let name = reg(size, n);
            for op in SegBase::ALL {
                let mn = op.mnemonic();
                emit_as!(b, mn, try_segbase_r(op, size, r(n)), "{mn} {name}");
            }
            let q = if size == Size::Q { 'q' } else { 'd' };
            emit!(b, try_rdssp_r(size, r(n)), "rdssp{q} {name}");
            emit!(b, try_incssp_r(size, r(n)), "incssp{q} {name}");
        }
        emit!(b, rdpid_r(r(n)), "rdpid {}", reg(Size::Q, n));
        for (size, src) in [(Size::B, n), (Size::W, n), (Size::D, n), (Size::Q, n)] {
            for dst in [0, 3, 7, 8, 15] {
                let d = reg(if size == Size::Q { Size::Q } else { Size::D }, dst);
                emit!(b, crc32_rr(size, r(dst), r(src)), "crc32 {d}, {}", reg(size, src));
            }
        }
        for op in Bmi::ALL {
            let mn = op.mnemonic();
            for size in [Size::D, Size::Q] {
                for (x, y) in [(n, (n + 5) % 16), ((n + 9) % 16, n), (n, n)] {
                    let dst = (n + 3) % 16;
                    let text =
                        format!("{mn} {}, {}, {}", reg(size, dst), reg(size, x), reg(size, y));
                    emit_as!(b, mn, try_bmi_rrr(op, size, r(dst), r(x), r(y)), "{text}");
                }
            }
        }
    }
    reject!(b, try_rdrand_r(Size::B, r(0)));
    reject!(b, try_rdseed_r(Size::B, r(0)));
    for size in [Size::B, Size::W] {
        reject!(b, try_segbase_r(SegBase::RdFs, size, r(0)));
        reject!(b, try_rdssp_r(size, r(0)));
        reject!(b, try_incssp_r(size, r(0)));
        reject!(b, try_bmi_rrr(Bmi::Pdep, size, r(0), r(1), r(2)));
    }
    emit!(b, rdpmc(), "rdpmc");
    emit!(b, xgetbv(), "xgetbv");
    emit!(b, xsetbv(), "xsetbv");
    emit!(b, xend(), "xend");
    emit!(b, xtest(), "xtest");
    emit!(b, rdpkru(), "rdpkru");
    emit!(b, wrpkru(), "wrpkru");
    emit!(b, serialize(), "serialize");
    for imm in [0, 1, 0x7f, 0x80, 0xff] {
        emit!(b, xabort(imm), "xabort {imm}");
    }
    for d in [0, 1, -1, 127, -128, 128, -6, i32::MAX, i32::MIN] {
        emit!(b, xbegin(d), "xbegin {}", crate::branch::target(6, d.into()));
    }
}
