//! Sweeps for `spec/mc-a64.md` "System".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{BARRIERS, SYS_REGS, xz, z};

pub fn build(b: &mut Batch) {
    emit!(b, nop(), "nop");
    for imm in 0..128 {
        emit!(b, try_hint(imm), "hint #{imm}");
    }
    reject!(b, try_hint(128), "hint #128");
    reject!(b, try_hint(u32::MAX));
    emit!(b, bti_c(), "bti c");
    emit!(b, bti_j(), "bti j");
    emit!(b, bti_jc(), "bti jc");
    emit!(b, paciasp(), "paciasp");
    emit!(b, pacibsp(), "pacibsp");
    emit!(b, autiasp(), "autiasp");
    emit!(b, autibsp(), "autibsp");

    let mut imms: Vec<u16> = (0..16).map(|bit| 1 << bit).collect();
    imms.extend([0, 3, 0x1234, 0x7fff, 0xaaaa, 0xfffe, 0xffff]);
    imms.extend((0..200).map(|i| (i * 327) as u16));
    for imm in imms {
        emit!(b, brk(imm), "brk #{imm}");
        emit!(b, svc(imm), "svc #{imm}");
        emit!(b, udf(imm), "udf #{imm}");
    }

    for (reg, name) in SYS_REGS {
        for t in 0..32 {
            emit!(b, mrs(z(t), reg), "mrs {}, {name}", xz(t));
            emit!(b, msr(reg, z(t)), "msr {name}, {}", xz(t));
        }
    }
    for (option, name) in BARRIERS {
        emit!(b, dmb(option), "dmb {name}");
        emit!(b, dsb(option), "dsb {name}");
    }
    emit!(b, isb(), "isb");
}
