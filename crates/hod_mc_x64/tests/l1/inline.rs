//! Sweeps for `spec/mc-x64.md` "Inline assembly" (WP-199).

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{SIZES, WIDE, ptr, r, reg, some_mems};

pub fn build(b: &mut Batch) {
    for m in some_mems() {
        let t = m.text();
        for op in Prefetch::ALL {
            let mn = op.mnemonic();
            emit_as!(b, mn, prefetch_m(op, m.mem()), "{mn} byte ptr {t}");
        }
        for size in WIDE {
            emit!(b, try_nop_m(size, m.mem()), "nop {} {t}", ptr(size));
        }
        emit!(b, fnstsw_m(m.mem()), "fnstsw word ptr {t}");
        emit!(b, fnstenv_m(m.mem()), "fnstenv {t}");
        emit!(b, fldenv_m(m.mem()), "fldenv {t}");
        emit!(b, fnsave_m(m.mem()), "fnsave {t}");
        emit!(b, frstor_m(m.mem()), "frstor {t}");
    }
    reject!(b, try_nop_m(Size::B, Mem::base(r(0), 0)));
    for n in 0..16 {
        for size in WIDE {
            emit!(b, try_nop_r(size, r(n)), "nop {}", reg(size, n));
        }
        for dst in [0, 3, 7, 8, 15] {
            emit!(b, crc32_q_rb(r(dst), r(n)), "crc32 {}, {}", reg(Size::Q, dst), reg(Size::B, n));
        }
    }
    reject!(b, try_nop_r(Size::B, r(0)));
    for size in [Size::B, Size::W, Size::D] {
        let acc = reg(size, 0);
        emit!(b, try_in_dx(size), "in {acc}, dx");
        emit!(b, try_out_dx(size), "out dx, {acc}");
        for port in [0, 1, 0x60, 0x80, 0xff] {
            emit!(b, try_in_i(size, port), "in {acc}, {port}");
            emit!(b, try_out_i(size, port), "out {port}, {acc}");
        }
    }
    reject!(b, try_in_dx(Size::Q));
    reject!(b, try_out_dx(Size::Q));
    reject!(b, try_in_i(Size::Q, 0));
    reject!(b, try_out_i(Size::Q, 0));
    for op in StringOp::ALL {
        for (size, suffix) in SIZES.into_iter().zip(['b', 'w', 'd', 'q']) {
            let mn = format!("{}{suffix}", op.stem());
            emit_as!(b, &mn, string_op(op, size), "{mn}");
        }
    }
    emit!(b, rdmsr(), "rdmsr");
    emit!(b, wrmsr(), "wrmsr");
    emit!(b, enclu(), "enclu");
    emit!(b, encls(), "encls");
    emit!(b, enclv(), "enclv");
    emit!(b, pconfig(), "pconfig");
    for imm in [0, 1, 0x7f, 0x80, 0xff] {
        emit!(b, hreset(imm), "hreset {imm}");
    }
    emit!(b, clc(), "clc");
    emit!(b, stc(), "stc");
    emit!(b, cmc(), "cmc");
    emit!(b, lahf(), "lahf");
    emit!(b, sahf(), "sahf");
    emit!(b, cli(), "cli");
    emit!(b, sti(), "sti");
}
