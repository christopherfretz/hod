//! Sweeps for `spec/mc-x64.md` "Atomics".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{SIZES, WIDE, r, reg, some_mems};

pub fn build(b: &mut Batch) {
    for size in SIZES {
        for a in 0..16 {
            for c in 0..16 {
                emit!(b, xchg_rr(size, r(a), r(c)), "xchg {}, {}", reg(size, a), reg(size, c));
            }
        }
        for m in some_mems() {
            for n in [0, 3, 6, 9, 15] {
                let (mt, rt) = (m.sized(size), reg(size, n));
                emit!(b, xchg_mr(size, m.mem(), r(n)), "xchg {mt}, {rt}");
                emit!(b, xadd_mr(size, m.mem(), r(n)), "xadd {mt}, {rt}");
                emit!(b, cmpxchg_mr(size, m.mem(), r(n)), "cmpxchg {mt}, {rt}");
            }
        }
    }
    for m in some_mems() {
        emit!(b, cmpxchg8b_m(m.mem()), "cmpxchg8b qword ptr {}", m.text());
        emit!(b, cmpxchg16b_m(m.mem()), "cmpxchg16b xmmword ptr {}", m.text());
    }
    locked(b);
    emit!(b, mfence(), "mfence");
    emit!(b, lfence(), "lfence");
    emit!(b, sfence(), "sfence");
    emit!(b, pause(), "pause");
}

/// Every lockable constructor, locked; everything else refused.
fn locked(b: &mut Batch) {
    macro_rules! lock_mr {
        ($size:expr, $m:expr, $n:expr; $($f:ident $mn:literal),*) => {$(
            emit_as!(b, concat!("lock ", $mn), try_lock($f($size, $m.mem(), r($n))),
                "lock {} {}, {}", $mn, $m.sized($size), reg($size, $n));
        )*};
    }
    macro_rules! lock_mi {
        ($size:expr, $m:expr, $v:expr; $($f:ident $mn:literal),*) => {$(
            emit_as!(b, concat!("lock ", $mn), try_lock($f($size, $m.mem(), $v)),
                "lock {} {}, {}", $mn, $m.sized($size), $v);
        )*};
    }
    macro_rules! lock_m {
        ($size:expr, $m:expr; $($f:ident $mn:literal),*) => {$(
            emit_as!(b, concat!("lock ", $mn), try_lock($f($size, $m.mem())),
                "lock {} {}", $mn, $m.sized($size));
        )*};
    }
    for size in SIZES {
        for m in some_mems() {
            for n in [1, 4, 10] {
                lock_mr!(size, m, n; add_mr "add", or_mr "or", adc_mr "adc", sbb_mr "sbb",
                    and_mr "and", sub_mr "sub", xor_mr "xor", xchg_mr "xchg", xadd_mr "xadd",
                    cmpxchg_mr "cmpxchg");
            }
            for v in [1i64, -1, 100] {
                lock_mi!(size, m, v; add_mi "add", or_mi "or", adc_mi "adc", sbb_mi "sbb",
                    and_mi "and", sub_mi "sub", xor_mi "xor");
            }
            lock_m!(size, m; inc_m "inc", dec_m "dec", not_m "not", neg_m "neg");
        }
    }
    for size in WIDE {
        for m in some_mems() {
            for n in [2, 11] {
                lock_mr!(size, m, n; bts_mr "bts", btr_mr "btr", btc_mr "btc");
            }
            for v in [0, 63] {
                lock_mi!(size, m, v; bts_mi "bts", btr_mi "btr", btc_mi "btc");
            }
        }
    }
    for m in some_mems() {
        emit_as!(
            b,
            "lock cmpxchg8b",
            try_lock(cmpxchg8b_m(m.mem())),
            "lock cmpxchg8b qword ptr {}",
            m.text()
        );
        emit_as!(
            b,
            "lock cmpxchg16b",
            try_lock(cmpxchg16b_m(m.mem())),
            "lock cmpxchg16b xmmword ptr {}",
            m.text()
        );
    }
    // Not lockable: register destinations, CMP and TEST, loads and plain
    // stores, and an instruction that already has the prefix. LLVM's
    // assembler adds `lock` to any instruction, so these have no text it
    // refuses.
    let m = Mem::base(Reg::Rax, 0);
    let refused = [
        add_rr(Size::Q, r(0), r(1)),
        add_rm(Size::Q, r(0), m),
        cmp_mr(Size::D, m, r(1)),
        cmp_mi(Size::D, m, 1),
        test_mr(Size::D, m, r(1)),
        mov_mr(Size::Q, m, r(1)),
        mov_rm(Size::Q, r(1), m),
        inc_r(Size::D, r(0)),
        mul_m(Size::D, m),
        xchg_rr(Size::D, r(1), r(2)),
        lock(add_mr(Size::D, m, r(1))),
        nop(1),
        bt_mr(Size::D, m, r(1)),
        bt_mi(Size::Q, m, 3),
        bts_rr(Size::D, r(0), r(1)),
        btr_ri(Size::Q, r(0), 1),
        lock(btc_mi(Size::W, m, 1)),
        movs_mr(FpSize::D, m, Xmm::Xmm0),
        tls_gd_call(),
    ];
    for inst in refused {
        reject!(b, try_lock(inst));
    }
}
