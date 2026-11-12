//! Sweeps for `spec/mc-a64.md` "Conditional".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{CONDS, g, rz, tuples, z};

fn select(b: &mut Batch) {
    macro_rules! select {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
                    let (cond, name) = CONDS[i % 16];
                    emit!(
                        b,
                        $f(sz, z(d), z(n), z(m), cond),
                        "{} {}, {}, {}, {name}", $mn, rz(sz, d), rz(sz, n), rz(sz, m)
                    );
                }
                for (cond, name) in CONDS {
                    emit!(
                        b,
                        $f(sz, z(5), z(6), z(7), cond),
                        "{} {}, {}, {}, {name}", $mn, rz(sz, 5), rz(sz, 6), rz(sz, 7)
                    );
                }
            }
        )*};
    }
    select! {
        csel "csel";
        csinc "csinc";
        csinv "csinv";
        csneg "csneg";
    }

    macro_rules! set {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (cond, name) in &CONDS[..14] {
                    for d in 0..32 {
                        emit!(b, $f(sz, z(d), *cond), "{} {}, {name}", $mn, rz(sz, d));
                    }
                }
                for (cond, name) in &CONDS[14..] {
                    reject!(b, $f(sz, z(5), *cond), "{} {}, {name}", $mn, rz(sz, 5));
                }
            }
        )*};
    }
    set! {
        try_cset "cset";
        try_csetm "csetm";
    }

    // CINC and CINV take a real register as the source; CNEG also ZR.
    macro_rules! increment {
        ($($f:ident $mn:literal $rn:ident $limit:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [d, n]) in tuples([32, $limit]).into_iter().enumerate() {
                    let (cond, name) = CONDS[i % 14];
                    emit!(
                        b,
                        $f(sz, z(d), $rn(n), cond),
                        "{} {}, {}, {name}", $mn, rz(sz, d), rz(sz, n)
                    );
                }
                for (cond, name) in &CONDS[..14] {
                    emit!(
                        b,
                        $f(sz, z(5), $rn(6), *cond),
                        "{} {}, {}, {name}", $mn, rz(sz, 5), rz(sz, 6)
                    );
                }
                for (cond, name) in &CONDS[14..] {
                    reject!(
                        b,
                        $f(sz, z(5), $rn(6), *cond),
                        "{} {}, {}, {name}", $mn, rz(sz, 5), rz(sz, 6)
                    );
                }
            }
        )*};
    }
    increment! {
        try_cinc "cinc" g 31;
        try_cinv "cinv" g 31;
        try_cneg "cneg" z 32;
    }
}

fn compare(b: &mut Batch) {
    macro_rules! register {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for (i, [n, m]) in tuples([32, 32]).into_iter().enumerate() {
                    let (cond, name) = CONDS[i % 16];
                    let nzcv = (i * 7 % 16) as u32;
                    emit!(
                        b,
                        $f(sz, z(n), z(m), nzcv, cond),
                        "{} {}, {}, #{nzcv}, {name}", $mn, rz(sz, n), rz(sz, m)
                    );
                }
                for (cond, name) in CONDS {
                    for nzcv in 0..16 {
                        emit!(
                            b,
                            $f(sz, z(6), z(7), nzcv, cond),
                            "{} {}, {}, #{nzcv}, {name}", $mn, rz(sz, 6), rz(sz, 7)
                        );
                    }
                }
                reject!(
                    b,
                    $f(sz, z(6), z(7), 16, Cond::Eq),
                    "{} {}, {}, #16, eq", $mn, rz(sz, 6), rz(sz, 7)
                );
                reject!(b, $f(sz, z(6), z(7), u32::MAX, Cond::Eq));
            }
        )*};
    }
    register! {
        try_ccmp "ccmp";
        try_ccmn "ccmn";
    }

    macro_rules! immediate {
        ($($f:ident $mn:literal;)*) => {$(
            for sz in Size::ALL {
                for n in 0..32 {
                    let (cond, name) = CONDS[n as usize % 16];
                    let (imm, nzcv) = (n * 5 % 32, n * 3 % 16);
                    emit!(
                        b,
                        $f(sz, z(n), imm, nzcv, cond),
                        "{} {}, #{imm}, #{nzcv}, {name}", $mn, rz(sz, n)
                    );
                }
                for imm in 0..32 {
                    for nzcv in 0..16 {
                        let (cond, name) = CONDS[(imm + nzcv) as usize % 16];
                        emit!(
                            b,
                            $f(sz, z(6), imm, nzcv, cond),
                            "{} {}, #{imm}, #{nzcv}, {name}", $mn, rz(sz, 6)
                        );
                    }
                }
                reject!(
                    b,
                    $f(sz, z(6), 32, 0, Cond::Eq),
                    "{} {}, #32, #0, eq", $mn, rz(sz, 6)
                );
                reject!(
                    b,
                    $f(sz, z(6), 0, 16, Cond::Eq),
                    "{} {}, #0, #16, eq", $mn, rz(sz, 6)
                );
                reject!(b, $f(sz, z(6), u32::MAX, 0, Cond::Eq));
                reject!(b, $f(sz, z(6), 0, u32::MAX, Cond::Eq));
            }
        )*};
    }
    immediate! {
        try_ccmp_imm "ccmp";
        try_ccmn_imm "ccmn";
    }
}

pub fn build(b: &mut Batch) {
    select(b);
    compare(b);
}
