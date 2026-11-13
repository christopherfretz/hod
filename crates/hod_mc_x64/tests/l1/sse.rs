//! Sweeps for `spec/mc-x64.md` "Scalar floating point": every `xmm` in
//! every register position, every general register of the conversions and
//! `movd`/`movq`, both precisions, every compare predicate, the sampled
//! memory forms, and every memory form through a scalar load and store
//! (`xmm-memory-operands`).

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{FP_CMPS, FPS, all_mems, r, reg, some_mems, x, xmm};

/// Registers for memory forms: each REX.R value and each low field
/// boundary.
const SOME: [u8; 4] = [0, 7, 8, 15];

pub fn build(b: &mut Batch) {
    moves(b);
    arithmetic(b);
    compares(b);
    conversions(b);
}

fn moves(b: &mut Batch) {
    for (fp, p) in FPS {
        let (movs, movap, movup) = (format!("movs{p}"), format!("movap{p}"), format!("movup{p}"));
        for d in 0..16 {
            for s in 0..16 {
                emit_as!(b, &movs, movs_rr(fp, x(d), x(s)), "{movs} {}, {}", xmm(d), xmm(s));
                emit_as!(b, &movap, movap_rr(fp, x(d), x(s)), "{movap} {}, {}", xmm(d), xmm(s));
                emit_as!(b, &movup, movup_rr(fp, x(d), x(s)), "{movup} {}, {}", xmm(d), xmm(s));
            }
        }
        for m in some_mems() {
            for n in SOME {
                let (mt, xt) = (m.fp(fp), xmm(n));
                emit_as!(b, &movs, movs_rm(fp, x(n), m.mem()), "{movs} {xt}, {mt}");
                emit_as!(b, &movs, movs_mr(fp, m.mem(), x(n)), "{movs} {mt}, {xt}");
                let mt = m.xmmword();
                emit_as!(b, &movap, movap_rm(fp, x(n), m.mem()), "{movap} {xt}, {mt}");
                emit_as!(b, &movap, movap_mr(fp, m.mem(), x(n)), "{movap} {mt}, {xt}");
                emit_as!(b, &movup, movup_rm(fp, x(n), m.mem()), "{movup} {xt}, {mt}");
                emit_as!(b, &movup, movup_mr(fp, m.mem(), x(n)), "{movup} {mt}, {xt}");
            }
        }
    }
    for d in 0..16 {
        for s in 0..16 {
            emit!(b, movdqa_rr(x(d), x(s)), "movdqa {}, {}", xmm(d), xmm(s));
            emit!(b, movdqu_rr(x(d), x(s)), "movdqu {}, {}", xmm(d), xmm(s));
            emit!(b, movq_rr(x(d), x(s)), "movq {}, {}", xmm(d), xmm(s));
        }
    }
    for m in some_mems() {
        for n in SOME {
            let (mt, xt) = (m.xmmword(), xmm(n));
            emit!(b, movdqa_rm(x(n), m.mem()), "movdqa {xt}, {mt}");
            emit!(b, movdqa_mr(m.mem(), x(n)), "movdqa {mt}, {xt}");
            emit!(b, movdqu_rm(x(n), m.mem()), "movdqu {xt}, {mt}");
            emit!(b, movdqu_mr(m.mem(), x(n)), "movdqu {mt}, {xt}");
        }
    }
    for (size, mn) in [(Size::D, "movd"), (Size::Q, "movq")] {
        for v in 0..16 {
            for g in 0..16 {
                let (xt, gt) = (xmm(v), reg(size, g));
                emit_as!(b, mn, try_movd_xr(size, x(v), r(g)), "{mn} {xt}, {gt}");
                emit_as!(b, mn, try_movd_rx(size, r(g), x(v)), "{mn} {gt}, {xt}");
            }
        }
    }
    for (size, mn) in [(Size::D, "movd"), (Size::Q, "movq")] {
        for m in some_mems() {
            for n in SOME {
                let (xt, mt) = (xmm(n), m.sized(size));
                emit_as!(b, mn, try_movd_rm(size, x(n), m.mem()), "{mn} {xt}, {mt}");
                emit_as!(b, mn, try_movd_mr(size, m.mem(), x(n)), "{mn} {mt}, {xt}");
            }
        }
    }
    for size in [Size::B, Size::W] {
        let mt = format!("{} [rax]", crate::text::ptr(size));
        let m = Mem::base(Reg::Rax, 0);
        reject!(b, try_movd_rm(size, x(0), m), "movd xmm0, {mt}");
        reject!(b, try_movd_mr(size, m, x(0)), "movd {mt}, xmm0");
        let gt = reg(size, 1);
        reject!(b, try_movd_xr(size, x(0), r(1)), "movd xmm0, {gt}");
        reject!(b, try_movd_rx(size, r(1), x(0)), "movd {gt}, xmm0");
        reject!(b, try_movd_xr(size, x(9), r(12)));
        reject!(b, try_movd_rx(size, r(12), x(9)));
    }
}

fn arithmetic(b: &mut Batch) {
    macro_rules! ops {
        ($($rr:ident $rm:ident $stem:literal $kind:literal;)*) => {$(
            for (fp, p) in FPS {
                let mn = format!("{}{}{p}", $stem, $kind);
                for d in 0..16 {
                    for s in 0..16 {
                        emit_as!(b, &mn, $rr(fp, x(d), x(s)), "{mn} {}, {}", xmm(d), xmm(s));
                    }
                }
                for m in some_mems() {
                    // Scalars read an element; the packed logical forms
                    // read 128 bits.
                    let mt = if $kind == "s" { m.fp(fp) } else { m.xmmword() };
                    for n in SOME {
                        emit_as!(b, &mn, $rm(fp, x(n), m.mem()), "{mn} {}, {mt}", xmm(n));
                    }
                }
            }
        )*};
    }
    ops! {
        adds_rr adds_rm "add" "s";
        subs_rr subs_rm "sub" "s";
        muls_rr muls_rm "mul" "s";
        divs_rr divs_rm "div" "s";
        sqrts_rr sqrts_rm "sqrt" "s";
        mins_rr mins_rm "min" "s";
        maxs_rr maxs_rm "max" "s";
        andp_rr andp_rm "and" "p";
        andnp_rr andnp_rm "andn" "p";
        orp_rr orp_rm "or" "p";
        xorp_rr xorp_rm "xor" "p";
    }
}

fn compares(b: &mut Batch) {
    for (fp, p) in FPS {
        for (rr, rm, stem) in [
            (
                ucomis_rr as fn(FpSize, Xmm, Xmm) -> Inst,
                ucomis_rm as fn(FpSize, Xmm, Mem) -> Inst,
                "ucomis",
            ),
            (comis_rr, comis_rm, "comis"),
        ] {
            let mn = format!("{stem}{p}");
            let name = if stem == "ucomis" {
                ("ucomis_rr", "ucomis_rm")
            } else {
                ("comis_rr", "comis_rm")
            };
            for d in 0..16 {
                for s in 0..16 {
                    b.push(
                        name.0,
                        rr(fp, x(d), x(s)),
                        Some(mn.clone()),
                        format!("{mn} {}, {}", xmm(d), xmm(s)),
                    );
                }
            }
            for m in some_mems() {
                for n in SOME {
                    b.push(
                        name.1,
                        rm(fp, x(n), m.mem()),
                        Some(mn.clone()),
                        format!("{mn} {}, {}", xmm(n), m.fp(fp)),
                    );
                }
            }
        }
        for (pred, cc) in FP_CMPS {
            let mn = format!("cmp{cc}s{p}");
            let imm = pred as u8;
            for d in 0..16 {
                for s in 0..16 {
                    emit_as!(
                        b,
                        &mn,
                        cmps_rr(pred, fp, x(d), x(s)),
                        "cmps{p} {}, {}, {imm}",
                        xmm(d),
                        xmm(s)
                    );
                }
            }
            for m in some_mems() {
                for n in [0, 9] {
                    emit_as!(
                        b,
                        &mn,
                        cmps_rm(pred, fp, x(n), m.mem()),
                        "{mn} {}, {}",
                        xmm(n),
                        m.fp(fp)
                    );
                }
            }
        }
    }
}

fn conversions(b: &mut Batch) {
    let mems = some_mems();
    for (fp, p) in FPS {
        let (to, from) = (format!("cvtsi2s{p}"), format!("cvtts{p}2si"));
        let round = format!("cvts{p}2si");
        for size in [Size::D, Size::Q] {
            for v in 0..16 {
                for g in 0..16 {
                    let (xt, gt) = (xmm(v), reg(size, g));
                    emit_as!(b, &to, try_cvtsi2s_rr(fp, x(v), size, r(g)), "{to} {xt}, {gt}");
                    emit_as!(b, &from, try_cvtts2si_rr(size, r(g), fp, x(v)), "{from} {gt}, {xt}");
                    emit_as!(b, &round, try_cvts2si_rr(size, r(g), fp, x(v)), "{round} {gt}, {xt}");
                }
            }
            for m in &mems {
                for n in SOME {
                    let (xt, gt) = (xmm(n), reg(size, n));
                    let it = m.sized(size);
                    emit_as!(b, &to, try_cvtsi2s_rm(fp, x(n), size, m.mem()), "{to} {xt}, {it}");
                    let ft = m.fp(fp);
                    emit_as!(
                        b,
                        &from,
                        try_cvtts2si_rm(size, r(n), fp, m.mem()),
                        "{from} {gt}, {ft}"
                    );
                    emit_as!(
                        b,
                        &round,
                        try_cvts2si_rm(size, r(n), fp, m.mem()),
                        "{round} {gt}, {ft}"
                    );
                }
            }
        }
        for size in [Size::B, Size::W] {
            let (gt, it) = (reg(size, 2), crate::text::ptr(size));
            let m = Mem::base(Reg::Rax, 0);
            reject!(b, try_cvtsi2s_rr(fp, x(1), size, r(2)), "{to} xmm1, {gt}");
            reject!(b, try_cvtsi2s_rm(fp, x(1), size, m), "{to} xmm1, {it} [rax]");
            reject!(b, try_cvtts2si_rr(size, r(2), fp, x(1)), "{from} {gt}, xmm1");
            reject!(
                b,
                try_cvtts2si_rm(size, r(2), fp, m),
                "{from} {gt}, {} [rax]",
                crate::text::fp_ptr(fp)
            );
            reject!(b, try_cvtsi2s_rr(fp, x(13), size, r(10)));
            reject!(b, try_cvtts2si_rr(size, r(10), fp, x(13)));
            reject!(b, try_cvts2si_rr(size, r(2), fp, x(1)), "{round} {gt}, xmm1");
            reject!(
                b,
                try_cvts2si_rm(size, r(2), fp, m),
                "{round} {gt}, {} [rax]",
                crate::text::fp_ptr(fp)
            );
        }
    }
    for d in 0..16 {
        for s in 0..16 {
            emit!(b, cvtss2sd_rr(x(d), x(s)), "cvtss2sd {}, {}", xmm(d), xmm(s));
            emit!(b, cvtsd2ss_rr(x(d), x(s)), "cvtsd2ss {}, {}", xmm(d), xmm(s));
        }
    }
    for m in &mems {
        for n in SOME {
            emit!(b, cvtss2sd_rm(x(n), m.mem()), "cvtss2sd {}, {}", xmm(n), m.fp(FpSize::S));
            emit!(b, cvtsd2ss_rm(x(n), m.mem()), "cvtsd2ss {}, {}", xmm(n), m.fp(FpSize::D));
        }
    }
}

/// Every memory form of "Memory operands" through a scalar load and store
/// with every `xmm` in the reg field, and through a 128-bit load.
pub fn memory(b: &mut Batch) {
    for m in all_mems() {
        for n in 0..16 {
            emit_as!(
                b,
                "movsd",
                movs_rm(FpSize::D, x(n), m.mem()),
                "movsd {}, {}",
                xmm(n),
                m.fp(FpSize::D)
            );
            emit_as!(
                b,
                "movss",
                movs_mr(FpSize::S, m.mem(), x(n)),
                "movss {}, {}",
                m.fp(FpSize::S),
                xmm(n)
            );
        }
        for n in SOME {
            emit_as!(
                b,
                "movaps",
                movap_rm(FpSize::S, x(n), m.mem()),
                "movaps {}, {}",
                xmm(n),
                m.xmmword()
            );
        }
    }
}
