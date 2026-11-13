//! Sweeps for `spec/mc-x64.md` "Packed SSE2": every `xmm` in every
//! register position, every general register of the mask, extract and
//! insert forms, both precisions, every compare predicate, the boundary
//! values of every immediate, the sampled memory forms, and every memory
//! form through three representative instructions (`packed-memory-operands`).
//!
//! The instructions are listed in tables (name, constructor, mnemonic); the
//! sweeps are generated from them.

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{FP_CMPS, FPS, M, all_mems, r, reg, some_mems, x, xmm};

/// Registers for memory forms: each REX.R value and each low field
/// boundary.
const SOME: [u8; 4] = [0, 7, 8, 15];

/// Immediates at the boundaries of the shift counts and shuffle orders.
const IMM8: [u8; 9] = [0, 1, 7, 8, 15, 16, 31, 64, 255];

type Xx = fn(Xmm, Xmm) -> Inst;
type Xm = fn(Xmm, Mem) -> Inst;
type FpXx = fn(FpSize, Xmm, Xmm) -> Inst;
type FpXm = fn(FpSize, Xmm, Mem) -> Inst;
type XiShift = fn(Xmm, u8) -> Inst;
type XxImm = fn(Xmm, Xmm, u8) -> Inst;
type XmImm = fn(Xmm, Mem, u8) -> Inst;

/// `(register name, register form, memory name, memory form, mnemonic)`.
type Pair<R, Mm> = (&'static str, R, &'static str, Mm, &'static str);

macro_rules! pairs {
    ($($rr:ident $rm:ident $mn:literal;)*) => {
        [$((stringify!($rr), $rr as _, stringify!($rm), $rm as _, $mn)),*]
    };
}

/// Every integer instruction with an `xmm, xmm/m128` form.
fn integer_ops() -> Vec<Pair<Xx, Xm>> {
    let ops: [Pair<Xx, Xm>; 57] = pairs! {
        paddb_rr paddb_rm "paddb";
        paddw_rr paddw_rm "paddw";
        paddd_rr paddd_rm "paddd";
        paddq_rr paddq_rm "paddq";
        psubb_rr psubb_rm "psubb";
        psubw_rr psubw_rm "psubw";
        psubd_rr psubd_rm "psubd";
        psubq_rr psubq_rm "psubq";
        paddsb_rr paddsb_rm "paddsb";
        paddsw_rr paddsw_rm "paddsw";
        paddusb_rr paddusb_rm "paddusb";
        paddusw_rr paddusw_rm "paddusw";
        psubsb_rr psubsb_rm "psubsb";
        psubsw_rr psubsw_rm "psubsw";
        psubusb_rr psubusb_rm "psubusb";
        psubusw_rr psubusw_rm "psubusw";
        pmullw_rr pmullw_rm "pmullw";
        pmulhw_rr pmulhw_rm "pmulhw";
        pmulhuw_rr pmulhuw_rm "pmulhuw";
        pmuludq_rr pmuludq_rm "pmuludq";
        pmaddwd_rr pmaddwd_rm "pmaddwd";
        pand_rr pand_rm "pand";
        pandn_rr pandn_rm "pandn";
        por_rr por_rm "por";
        pxor_rr pxor_rm "pxor";
        pcmpeqb_rr pcmpeqb_rm "pcmpeqb";
        pcmpeqw_rr pcmpeqw_rm "pcmpeqw";
        pcmpeqd_rr pcmpeqd_rm "pcmpeqd";
        pcmpgtb_rr pcmpgtb_rm "pcmpgtb";
        pcmpgtw_rr pcmpgtw_rm "pcmpgtw";
        pcmpgtd_rr pcmpgtd_rm "pcmpgtd";
        pminsw_rr pminsw_rm "pminsw";
        pmaxsw_rr pmaxsw_rm "pmaxsw";
        pminub_rr pminub_rm "pminub";
        pmaxub_rr pmaxub_rm "pmaxub";
        pavgb_rr pavgb_rm "pavgb";
        pavgw_rr pavgw_rm "pavgw";
        psadbw_rr psadbw_rm "psadbw";
        psllw_rr psllw_rm "psllw";
        pslld_rr pslld_rm "pslld";
        psllq_rr psllq_rm "psllq";
        psrlw_rr psrlw_rm "psrlw";
        psrld_rr psrld_rm "psrld";
        psrlq_rr psrlq_rm "psrlq";
        psraw_rr psraw_rm "psraw";
        psrad_rr psrad_rm "psrad";
        punpcklbw_rr punpcklbw_rm "punpcklbw";
        punpcklwd_rr punpcklwd_rm "punpcklwd";
        punpckldq_rr punpckldq_rm "punpckldq";
        punpcklqdq_rr punpcklqdq_rm "punpcklqdq";
        punpckhbw_rr punpckhbw_rm "punpckhbw";
        punpckhwd_rr punpckhwd_rm "punpckhwd";
        punpckhdq_rr punpckhdq_rm "punpckhdq";
        punpckhqdq_rr punpckhqdq_rm "punpckhqdq";
        packsswb_rr packsswb_rm "packsswb";
        packssdw_rr packssdw_rm "packssdw";
        packuswb_rr packuswb_rm "packuswb";
    };
    ops.to_vec()
}

/// The conversions between packed formats: `(.., width of the memory
/// source)`.
fn conversions() -> Vec<(Pair<Xx, Xm>, &'static str)> {
    let ops: [Pair<Xx, Xm>; 8] = pairs! {
        cvtdq2ps_rr cvtdq2ps_rm "cvtdq2ps";
        cvtps2dq_rr cvtps2dq_rm "cvtps2dq";
        cvttps2dq_rr cvttps2dq_rm "cvttps2dq";
        cvtdq2pd_rr cvtdq2pd_rm "cvtdq2pd";
        cvtpd2dq_rr cvtpd2dq_rm "cvtpd2dq";
        cvttpd2dq_rr cvttpd2dq_rm "cvttpd2dq";
        cvtps2pd_rr cvtps2pd_rm "cvtps2pd";
        cvtpd2ps_rr cvtpd2ps_rm "cvtpd2ps";
    };
    // The widening conversions read two 32-bit elements.
    ops.into_iter()
        .map(|op| (op, if matches!(op.4, "cvtdq2pd" | "cvtps2pd") { "qword" } else { "xmmword" }))
        .collect()
}

/// The floating-point instructions with an `xmm, xmm/m128` form, by stem
/// (`addp` + `s`/`d`).
fn fp_ops() -> Vec<Pair<FpXx, FpXm>> {
    let ops: [Pair<FpXx, FpXm>; 9] = pairs! {
        addp_rr addp_rm "add";
        subp_rr subp_rm "sub";
        mulp_rr mulp_rm "mul";
        divp_rr divp_rm "div";
        sqrtp_rr sqrtp_rm "sqrt";
        minp_rr minp_rm "min";
        maxp_rr maxp_rm "max";
        unpcklp_rr unpcklp_rm "unpckl";
        unpckhp_rr unpckhp_rm "unpckh";
    };
    ops.to_vec()
}

pub fn integer(b: &mut Batch) {
    for (rr_name, rr, rm_name, rm, mn) in integer_ops() {
        xx_and_xm(b, (rr_name, rr, rm_name, rm, mn), "xmmword");
    }
    for ((rr_name, rr, rm_name, rm, mn), width) in conversions() {
        xx_and_xm(b, (rr_name, rr, rm_name, rm, mn), width);
    }
    shifts(b);
    shuffles(b);
    masks(b);
}

/// Every register pair, and the sampled memory forms with every REX.R.
fn xx_and_xm(b: &mut Batch, (rr_name, rr, rm_name, rm, mn): Pair<Xx, Xm>, width: &str) {
    for d in 0..16 {
        for s in 0..16 {
            b.push(rr_name, rr(x(d), x(s)), None, format!("{mn} {}, {}", xmm(d), xmm(s)));
        }
    }
    for m in some_mems() {
        for n in SOME {
            let text = format!("{mn} {}, {width} ptr {}", xmm(n), m.text());
            b.push(rm_name, rm(x(n), m.mem()), None, text);
        }
    }
}

fn shifts(b: &mut Batch) {
    let ops: [(&str, XiShift, &str); 10] = [
        ("psrlw_ri", psrlw_ri, "psrlw"),
        ("psraw_ri", psraw_ri, "psraw"),
        ("psllw_ri", psllw_ri, "psllw"),
        ("psrld_ri", psrld_ri, "psrld"),
        ("psrad_ri", psrad_ri, "psrad"),
        ("pslld_ri", pslld_ri, "pslld"),
        ("psrlq_ri", psrlq_ri, "psrlq"),
        ("psrldq_ri", psrldq_ri, "psrldq"),
        ("psllq_ri", psllq_ri, "psllq"),
        ("pslldq_ri", pslldq_ri, "pslldq"),
    ];
    for (name, f, mn) in ops {
        for d in 0..16 {
            for n in IMM8 {
                b.push(name, f(x(d), n), None, format!("{mn} {}, {n}", xmm(d)));
            }
        }
    }
}

fn shuffles(b: &mut Batch) {
    let ops: [Pair<XxImm, XmImm>; 3] = [
        ("pshufd_rri", pshufd_rri, "pshufd_rmi", pshufd_rmi, "pshufd"),
        ("pshuflw_rri", pshuflw_rri, "pshuflw_rmi", pshuflw_rmi, "pshuflw"),
        ("pshufhw_rri", pshufhw_rri, "pshufhw_rmi", pshufhw_rmi, "pshufhw"),
    ];
    for (rr_name, rr, rm_name, rm, mn) in ops {
        for d in 0..16 {
            for s in 0..16 {
                let order = IMM8[(d + s) as usize % IMM8.len()];
                let text = format!("{mn} {}, {}, {order}", xmm(d), xmm(s));
                b.push(rr_name, rr(x(d), x(s), order), None, text);
            }
        }
        for m in some_mems() {
            for n in SOME {
                for order in [0, 0x1b, 0xff] {
                    let text = format!("{mn} {}, {}, {order}", xmm(n), m.xmmword());
                    b.push(rm_name, rm(x(n), m.mem(), order), None, text);
                }
            }
        }
    }
}

/// The forms with a general register: masks, `pextrw`, `pinsrw`.
fn masks(b: &mut Batch) {
    for g in 0..16 {
        for v in 0..16 {
            let (gt, xt) = (reg(Size::D, g), xmm(v));
            emit!(b, pmovmskb_rr(r(g), x(v)), "pmovmskb {gt}, {xt}");
            // WP-181: two `xmm` operands, swept here with the masks.
            let yt = xmm(g);
            emit!(b, rcpps_rr(x(g), x(v)), "rcpps {yt}, {xt}");
            emit!(b, rsqrtps_rr(x(g), x(v)), "rsqrtps {yt}, {xt}");
            emit!(b, maskmovdqu_rr(x(g), x(v)), "maskmovdqu {yt}, {xt}");
            for (fp, p) in FPS {
                let mn = format!("movmskp{p}");
                emit_as!(b, &mn, movmskp_rr(fp, r(g), x(v)), "{mn} {gt}, {xt}");
            }
            let lane = (g + v) % 8;
            emit!(b, pextrw_rri(r(g), x(v), lane), "pextrw {gt}, {xt}, {lane}");
            emit!(b, pinsrw_rri(x(v), r(g), lane), "pinsrw {xt}, {gt}, {lane}");
        }
    }
    for m in some_mems() {
        for n in SOME {
            for lane in [0, 7] {
                let text = format!("pinsrw {}, word ptr {}, {lane}", xmm(n), m.text());
                b.push("pinsrw_rmi", pinsrw_rmi(x(n), m.mem(), lane), None, text);
            }
        }
    }
}

pub fn floating_point(b: &mut Batch) {
    for (rr_name, rr, rm_name, rm, stem) in fp_ops() {
        for (fp, p) in FPS {
            let mn = format!("{stem}p{p}");
            for d in 0..16 {
                for s in 0..16 {
                    let text = format!("{mn} {}, {}", xmm(d), xmm(s));
                    b.push(rr_name, rr(fp, x(d), x(s)), Some(mn.clone()), text);
                }
            }
            for m in some_mems() {
                for n in SOME {
                    let text = format!("{mn} {}, {}", xmm(n), m.xmmword());
                    b.push(rm_name, rm(fp, x(n), m.mem()), Some(mn.clone()), text);
                }
            }
        }
    }
    for (fp, p) in FPS {
        for (pred, cc) in FP_CMPS {
            let mn = format!("cmp{cc}p{p}");
            let imm = pred as u8;
            for d in 0..16 {
                for s in 0..16 {
                    let (dt, st) = (xmm(d), xmm(s));
                    emit_as!(b, &mn, cmpp_rr(pred, fp, x(d), x(s)), "cmpp{p} {dt}, {st}, {imm}");
                }
            }
            for m in some_mems() {
                for n in [0, 9] {
                    let mt = m.xmmword();
                    emit_as!(b, &mn, cmpp_rm(pred, fp, x(n), m.mem()), "{mn} {}, {mt}", xmm(n));
                }
            }
        }
        let mn = format!("shufp{p}");
        for d in 0..16 {
            for s in 0..16 {
                let order = IMM8[(d * 3 + s) as usize % IMM8.len()];
                let (dt, st) = (xmm(d), xmm(s));
                emit_as!(b, &mn, shufp_rri(fp, x(d), x(s), order), "{mn} {dt}, {st}, {order}");
            }
        }
        for m in some_mems() {
            for n in SOME {
                let (xt, mt) = (xmm(n), m.xmmword());
                emit_as!(b, &mn, shufp_rmi(fp, x(n), m.mem(), 0xe4), "{mn} {xt}, {mt}, 228");
            }
        }
        let (lo, hi) = (format!("movlp{p}"), format!("movhp{p}"));
        for m in some_mems() {
            for n in SOME {
                let (xt, mt) = (xmm(n), format!("qword ptr {}", m.text()));
                emit_as!(b, &lo, movlp_rm(fp, x(n), m.mem()), "{lo} {xt}, {mt}");
                emit_as!(b, &lo, movlp_mr(fp, m.mem(), x(n)), "{lo} {mt}, {xt}");
                emit_as!(b, &hi, movhp_rm(fp, x(n), m.mem()), "{hi} {xt}, {mt}");
                emit_as!(b, &hi, movhp_mr(fp, m.mem(), x(n)), "{hi} {mt}, {xt}");
            }
        }
    }
    for d in 0..16 {
        for s in 0..16 {
            emit!(b, movlhps_rr(x(d), x(s)), "movlhps {}, {}", xmm(d), xmm(s));
            emit!(b, movhlps_rr(x(d), x(s)), "movhlps {}, {}", xmm(d), xmm(s));
        }
    }
}

/// Every memory form of "Memory operands" with every `xmm` in the reg
/// field through an integer operation, and through a shuffle and a compare
/// (an immediate after the displacement).
pub fn memory(b: &mut Batch) {
    let mems: Vec<M> = all_mems();
    for m in &mems {
        for n in 0..16 {
            emit!(b, paddd_rm(x(n), m.mem()), "paddd {}, {}", xmm(n), m.xmmword());
        }
        for n in SOME {
            emit!(b, pshufd_rmi(x(n), m.mem(), 0x4e), "pshufd {}, {}, 78", xmm(n), m.xmmword());
            emit_as!(
                b,
                "cmpltpd",
                cmpp_rm(FpCmp::Lt, FpSize::D, x(n), m.mem()),
                "cmpltpd {}, {}",
                xmm(n),
                m.xmmword()
            );
        }
    }
}
