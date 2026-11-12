//! Sweeps for `spec/mc-x64.md` "Branches and calls", "Stack".
//!
//! A branch's text names its target as an offset from a label at the
//! branch itself (`$HERE`): the instruction's length plus the
//! displacement. `{disp32}` makes LLVM keep the near form.

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{CONDS, Rng, r, reg, some_mems};

/// `$HERE+n` for a target `disp` bytes past the end of an instruction of
/// `len` bytes.
pub fn target(len: usize, disp: i64) -> String {
    format!("$HERE{:+}", len as i64 + disp)
}

/// Near displacements: the boundaries, small values, and a sample.
pub fn near_disps() -> Vec<i32> {
    let mut out = vec![0, 1, -1, 127, -128, 128, -129, -5, -6, i32::MAX, i32::MIN, i32::MAX - 5];
    let mut rng = Rng::new(0xb7);
    out.extend((0..24).map(|_| rng.next() as i32));
    out
}

pub fn build(b: &mut Batch) {
    for d in i8::MIN..=i8::MAX {
        emit!(b, jmp_short(d), "jmp {}", target(2, d.into()));
    }
    for d in near_disps() {
        emit!(b, jmp_near(d), "{{disp32}} jmp {}", target(5, d.into()));
        emit!(b, call(d), "call {}", target(5, d.into()));
    }
    for (cond, cc) in CONDS {
        let j = format!("j{cc}");
        for d in [i8::MIN, -2, -1, 0, 1, 2, 100, i8::MAX] {
            emit_as!(b, &j, jcc_short(cond, d), "{j} {}", target(2, d.into()));
        }
        for d in near_disps() {
            emit_as!(b, &j, jcc_near(cond, d), "{{disp32}} {j} {}", target(6, d.into()));
        }
    }
    for n in 0..16 {
        emit!(b, jmp_r(r(n)), "jmp {}", reg(Size::Q, n));
        emit!(b, call_r(r(n)), "call {}", reg(Size::Q, n));
        emit!(b, push_r(r(n)), "push {}", reg(Size::Q, n));
        emit!(b, pop_r(r(n)), "pop {}", reg(Size::Q, n));
    }
    for m in some_mems() {
        emit!(b, jmp_m(m.mem()), "jmp {}", m.sized(Size::Q));
        emit!(b, call_m(m.mem()), "call {}", m.sized(Size::Q));
        emit!(b, push_m(m.mem()), "push {}", m.sized(Size::Q));
        emit!(b, pop_m(m.mem()), "pop {}", m.sized(Size::Q));
    }
    for v in [0, 1, -1, 127, -128, 128, -129, 0x1234_5678, i32::MAX, i32::MIN] {
        emit!(b, push_i(v), "push {v}");
    }
    emit!(b, ret(), "ret");
    for n in [1, 8, 0x100, u16::MAX] {
        emit!(b, ret_imm(n), "ret {n}");
    }
    emit!(b, leave(), "leave");
}
