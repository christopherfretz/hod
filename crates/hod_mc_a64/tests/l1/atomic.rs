//! Sweeps for `spec/mc-a64.md` "Atomics".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{MEM_SIZES, ORDERS, g, pz, rz, s, tuples, wz, xs, z};

fn exclusive(b: &mut Batch) {
    macro_rules! load {
        ($($f:ident $stem:literal;)*) => {$(
            for (ms, suffix, prefix) in MEM_SIZES {
                let mn = format!("{}{suffix}", $stem);
                for [t, n] in tuples([32, 32]) {
                    emit_as!(b, &mn, $f(ms, z(t), s(n)), "{mn} {}, [{}]", pz(prefix, t), xs(n));
                }
            }
        )*};
    }
    load! {
        ldxr "ldxr";
        ldaxr "ldaxr";
        ldar "ldar";
        stlr "stlr";
    }

    // The status register must differ from the data register, and from the
    // base unless the base is SP.
    macro_rules! store {
        ($($f:ident $stem:literal;)*) => {$(
            for (ms, suffix, prefix) in MEM_SIZES {
                let mn = format!("{}{suffix}", $stem);
                for [st, t, n] in tuples([32, 32, 32]) {
                    let asm = format!("{mn} {}, {}, [{}]", wz(st), pz(prefix, t), xs(n));
                    if st == t || (st == n && n != 31) {
                        reject!(b, $f(ms, z(st), z(t), s(n)), "{asm}");
                    } else {
                        emit_as!(b, &mn, $f(ms, z(st), z(t), s(n)), "{asm}");
                    }
                }
                reject!(b, $f(ms, z(4), z(4), s(6)), "{mn} w4, {}, [x6]", pz(prefix, 4));
                reject!(b, $f(ms, z(6), z(4), s(6)), "{mn} w6, {}, [x6]", pz(prefix, 4));
                reject!(b, $f(ms, z(31), z(31), s(6)), "{mn} wzr, {}, [x6]", pz(prefix, 31));
                emit_as!(b, &mn, $f(ms, z(31), z(4), s(31)), "{mn} wzr, {}, [sp]", pz(prefix, 4));
            }
        )*};
    }
    store! {
        try_stxr "stxr";
        try_stlxr "stlxr";
    }

    macro_rules! load_pair {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for [t, t2, n] in tuples([32, 32, 32]) {
                    let asm = format!("{} {}, {}, [{}]", $mn, rz(sz, t), rz(sz, t2), xs(n));
                    if t == t2 {
                        // CONSTRAINED UNPREDICTABLE; LLVM 22 assembles it
                        // without a diagnostic, so there is no text to
                        // confirm the rejection with.
                        reject!(b, $f(sz, z(t), z(t2), s(n)));
                    } else {
                        emit!(b, $f(sz, z(t), z(t2), s(n)), "{asm}");
                    }
                }
                reject!(b, $f(sz, z(4), z(4), s(6)));
            }
        )*};
    }
    load_pair! {
        try_ldxp "ldxp";
        try_ldaxp "ldaxp";
    }

    macro_rules! store_pair {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for [st, t, t2, n] in tuples([32, 32, 32, 32]) {
                    let asm =
                        format!("{} {}, {}, {}, [{}]", $mn, wz(st), rz(sz, t), rz(sz, t2), xs(n));
                    if st == t || st == t2 || (st == n && n != 31) {
                        reject!(b, $f(sz, z(st), z(t), z(t2), s(n)), "{asm}");
                    } else {
                        emit!(b, $f(sz, z(st), z(t), z(t2), s(n)), "{asm}");
                    }
                }
                let (r4, r5) = (rz(sz, 4), rz(sz, 5));
                reject!(b, $f(sz, z(4), z(4), z(5), s(6)), "{} w4, {r4}, {r5}, [x6]", $mn);
                reject!(b, $f(sz, z(5), z(4), z(5), s(6)), "{} w5, {r4}, {r5}, [x6]", $mn);
                reject!(b, $f(sz, z(6), z(4), z(5), s(6)), "{} w6, {r4}, {r5}, [x6]", $mn);
                // Storing one register twice is allowed.
                emit!(b, $f(sz, z(7), z(4), z(4), s(6)), "{} w7, {r4}, {r4}, [x6]", $mn);
            }
        )*};
    }
    store_pair! {
        try_stxp "stxp";
        try_stlxp "stlxp";
    }

    emit!(b, clrex(), "clrex");
}

fn lse(b: &mut Batch) {
    macro_rules! three_operand {
        ($($f:ident $stem:literal;)*) => {$(
            for (ms, suffix, prefix) in MEM_SIZES {
                for (order, infix) in ORDERS {
                    let mn = format!("{}{infix}{suffix}", $stem);
                    for [rs, t, n] in tuples([32, 32, 32]) {
                        emit_as!(
                            b, &mn,
                            $f(ms, order, z(rs), z(t), s(n)),
                            "{mn} {}, {}, [{}]", pz(prefix, rs), pz(prefix, t), xs(n)
                        );
                    }
                }
            }
        )*};
    }
    three_operand! {
        cas "cas";
        swp "swp";
        ldadd "ldadd";
        ldclr "ldclr";
        ldeor "ldeor";
        ldset "ldset";
        ldsmax "ldsmax";
        ldsmin "ldsmin";
        ldumax "ldumax";
        ldumin "ldumin";
    }

    for sz in Size::ALL {
        for (order, infix) in ORDERS {
            let mn = format!("casp{infix}");
            for [rs, t, n] in tuples([31, 31, 32]) {
                if rs % 2 == 1 || t % 2 == 1 {
                    reject!(b, try_casp(sz, order, g(rs), g(t), s(n)));
                    continue;
                }
                emit_as!(
                    b,
                    &mn,
                    try_casp(sz, order, g(rs), g(t), s(n)),
                    "{mn} {}, {}, {}, {}, [{}]",
                    rz(sz, rs),
                    rz(sz, rs + 1),
                    rz(sz, t),
                    rz(sz, t + 1),
                    xs(n)
                );
            }
            let (r1, r2, r3, r4) = (rz(sz, 1), rz(sz, 2), rz(sz, 3), rz(sz, 4));
            reject!(b, try_casp(sz, order, g(1), g(2), s(6)), "{mn} {r1}, {r2}, {r2}, {r3}, [x6]");
            reject!(b, try_casp(sz, order, g(2), g(3), s(6)), "{mn} {r2}, {r3}, {r3}, {r4}, [x6]");
        }
    }
}

pub fn build(b: &mut Batch) {
    exclusive(b);
    lse(b);
}
