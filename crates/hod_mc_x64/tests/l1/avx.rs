//! Sweeps for `spec/mc-x64.md` "SSE4 and the extensions of the
//! `0F38`/`0F3A` maps" and "VEX" (WP-202): every member of every family,
//! each register position over registers on both sides of the REX/VEX
//! extension bit, both widths where the instruction has them (the other
//! refused by the checked form, and by `llvm-mc`), the immediates at
//! their boundaries, and the sampled memory forms.
//!
//! LLVM's assembler commutes a commutable VEX instruction whose second
//! source is `xmm8`..`xmm15` while its first is not, to reach the
//! two-byte prefix; the register-register sweeps pair a high second
//! source with a high first one only, where both encodings agree.

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{M, r, reg, some_mems, x, xmm};

/// Registers on both sides of each extension bit.
const SOME: [u8; 4] = [0, 7, 8, 15];

/// Immediates at the boundaries.
const IMM8: [u8; 6] = [0, 1, 0x10, 0x7f, 0x80, 0xff];

/// The name of vector register `n` at `width`.
fn vr(width: Width, n: u8) -> String {
    match width {
        Width::X => xmm(n),
        Width::Y => format!("ymm{n}"),
    }
}

/// `xmmword ptr`/`ymmword ptr` of `m`.
fn vm(width: Width, m: M) -> String {
    match width {
        Width::X => m.xmmword(),
        Width::Y => format!("ymmword ptr {}", m.text()),
    }
}

/// The register triples of the three-operand sweeps.
fn triples() -> Vec<(u8, u8, u8)> {
    let mut out = Vec::new();
    for d in SOME {
        for a in SOME {
            for b in SOME {
                // A high `b` only with a high `a` (see the module doc).
                if b < 8 || a >= 8 {
                    out.push((d, a, b));
                }
            }
        }
    }
    out
}

/// The width a member does not have, if any.
fn missing(allows: impl Fn(Width) -> bool) -> Option<Width> {
    Width::ALL.into_iter().find(|&w| !allows(w))
}

/// The legacy `0F38`/`0F3A` forms.
pub fn legacy(b: &mut Batch) {
    for op in Ext::ALL {
        let mn = op.mnemonic();
        for d in 0..16 {
            for s in SOME {
                emit_as!(b, mn, ext_rr(op, x(d), x(s)), "{mn} {}, {}", xmm(d), xmm(s));
            }
        }
        for m in some_mems() {
            emit_as!(b, mn, ext_rm(op, x(9), m.mem()), "{mn} {}, {}", xmm(9), m.xmmword());
        }
    }
    for op in ExtImm::ALL {
        let mn = op.mnemonic();
        for d in SOME {
            for s in SOME {
                for i in IMM8 {
                    emit_as!(b, mn, ext_rri(op, x(d), x(s), i), "{mn} {}, {}, {i}", xmm(d), xmm(s));
                }
            }
        }
        let size = match op.mem_bytes() {
            4 => "dword ptr",
            8 => "qword ptr",
            _ => "xmmword ptr",
        };
        for m in some_mems() {
            let t = format!("{size} {}", m.text());
            emit_as!(b, mn, ext_rmi(op, x(12), m.mem(), 0x21), "{mn} {}, {t}, 33", xmm(12));
        }
    }
}

/// The VEX forms.
#[allow(clippy::too_many_lines, reason = "one sweep per family")]
pub fn vex_forms(b: &mut Batch) {
    for &op in V3::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL.into_iter().filter(|&w| op.allows(w)) {
            // The shifts by an `xmm` count take an `xmm` at both widths.
            let count = matches!(
                op,
                V3::Psrlw
                    | V3::Psrld
                    | V3::Psrlq
                    | V3::Psraw
                    | V3::Psrad
                    | V3::Psllw
                    | V3::Pslld
                    | V3::Psllq
            );
            let bw = if count { Width::X } else { w };
            for (d, a, s) in triples() {
                emit_as!(
                    b,
                    mn,
                    try_v3_rrr(op, w, x(d), x(a), x(s)),
                    "{mn} {}, {}, {}",
                    vr(w, d),
                    vr(w, a),
                    vr(bw, s)
                );
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_v3_rrm(op, w, x(14), x(3), m.mem()),
                    "{mn} {}, {}, {}",
                    vr(w, 14),
                    vr(w, 3),
                    vm(bw, m)
                );
            }
        }
        if let Some(w) = missing(|w| op.allows(w)) {
            reject!(
                b,
                try_v3_rrr(op, w, x(0), x(1), x(2)),
                "{mn} {}, {}, {}",
                vr(w, 0),
                vr(w, 1),
                vr(w, 2)
            );
            reject!(b, try_v3_rrm(op, w, x(0), x(1), M::base(0, 0).mem()));
        }
    }
    for &op in V3Imm::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL.into_iter().filter(|&w| op.allows(w)) {
            for (d, a, s) in triples() {
                for i in [0u8, 0x11, 0xff] {
                    // `vcmpps` prints its predicate in the mnemonic.
                    if mn.starts_with("vcmp") {
                        continue;
                    }
                    emit_as!(
                        b,
                        mn,
                        try_v3_rrri(op, w, x(d), x(a), x(s), i),
                        "{mn} {}, {}, {}, {i}",
                        vr(w, d),
                        vr(w, a),
                        vr(w, s)
                    );
                }
            }
            for m in some_mems() {
                if mn.starts_with("vcmp") {
                    continue;
                }
                emit_as!(
                    b,
                    mn,
                    try_v3_rrmi(op, w, x(6), x(11), m.mem(), 0x31),
                    "{mn} {}, {}, {}, 49",
                    vr(w, 6),
                    vr(w, 11),
                    vm(w, m)
                );
            }
        }
        if let Some(w) = missing(|w| op.allows(w)) {
            reject!(
                b,
                try_v3_rrri(op, w, x(0), x(1), x(2), 0),
                "{mn} {}, {}, {}, 0",
                vr(w, 0),
                vr(w, 1),
                vr(w, 2)
            );
            reject!(b, try_v3_rrmi(op, w, x(0), x(1), M::base(0, 0).mem(), 0));
        }
    }
    // The compares: the predicate in the mnemonic, as LLVM prints it.
    const PREDS: [&str; 32] = [
        "eq", "lt", "le", "unord", "neq", "nlt", "nle", "ord", "eq_uq", "nge", "ngt", "false",
        "neq_oq", "ge", "gt", "true", "eq_os", "lt_oq", "le_oq", "unord_s", "neq_us", "nlt_uq",
        "nle_uq", "ord_s", "eq_us", "nge_uq", "ngt_uq", "false_os", "neq_os", "ge_oq", "gt_oq",
        "true_us",
    ];
    for (op, suffix) in
        [(V3Imm::Cmpps, "ps"), (V3Imm::Cmppd, "pd"), (V3Imm::Cmpss, "ss"), (V3Imm::Cmpsd, "sd")]
    {
        for w in Width::ALL.into_iter().filter(|&w| op.allows(w)) {
            for (p, pred) in (0u8..).zip(PREDS) {
                let mn = format!("vcmp{pred}{suffix}");
                for (d, a, s) in [(0, 1, 2), (9, 10, 3), (15, 8, 12)] {
                    emit_as!(
                        b,
                        mn.clone(),
                        try_v3_rrri(op, w, x(d), x(a), x(s), p),
                        "{mn} {}, {}, {}",
                        vr(w, d),
                        vr(w, a),
                        vr(w, s)
                    );
                }
                let m = M::base(5, 0x40);
                let mem = match suffix {
                    "ss" => format!("dword ptr {}", m.text()),
                    "sd" => format!("qword ptr {}", m.text()),
                    _ => vm(w, m),
                };
                emit_as!(
                    b,
                    mn.clone(),
                    try_v3_rrmi(op, w, x(4), x(13), m.mem(), p),
                    "{mn} {}, {}, {mem}",
                    vr(w, 4),
                    vr(w, 13)
                );
            }
        }
    }
    for &op in V2::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL.into_iter().filter(|&w| op.allows(w)) {
            // The narrowing conversions write an `xmm`; `vcvtph2ps`
            // reads one (64 bits from memory at 128).
            let narrow = matches!(op, V2::Cvtpd2ps | V2::Cvtpd2dq | V2::Cvttpd2dq);
            let widen = matches!(op, V2::Cvtph2ps);
            let dw = if narrow { Width::X } else { w };
            let sw = if widen { Width::X } else { w };
            for d in SOME {
                // LLVM writes `vmovdqu` from `xmm8`..`xmm15` as the store
                // form (`7F`), for the two-byte prefix.
                for s in SOME.into_iter().filter(|&s| s < 8 || op != V2::Movdqu) {
                    emit_as!(b, mn, v2_rr(op, w, x(d), x(s)), "{mn} {}, {}", vr(dw, d), vr(sw, s));
                }
            }
            for m in some_mems() {
                let mt = match (widen, w, narrow) {
                    (true, Width::X, _) => format!("qword ptr {}", m.text()),
                    (true, Width::Y, _) => m.xmmword(),
                    _ => vm(w, m),
                };
                emit_as!(b, mn, v2_rm(op, w, x(10), m.mem()), "{mn} {}, {mt}", vr(dw, 10));
            }
        }
    }
    for &op in V2Imm::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL.into_iter().filter(|&w| op.allows(w)) {
            for d in SOME {
                for s in SOME {
                    for i in IMM8 {
                        emit_as!(
                            b,
                            mn,
                            try_v2_rri(op, w, x(d), x(s), i),
                            "{mn} {}, {}, {i}",
                            vr(w, d),
                            vr(w, s)
                        );
                    }
                }
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_v2_rmi(op, w, x(1), m.mem(), 9),
                    "{mn} {}, {}, 9",
                    vr(w, 1),
                    vm(w, m)
                );
            }
        }
        if let Some(w) = missing(|w| op.allows(w)) {
            reject!(b, try_v2_rri(op, w, x(0), x(1), 0), "{mn} {}, {}, 0", vr(w, 0), vr(w, 1));
            reject!(b, try_v2_rmi(op, w, x(0), M::base(0, 0).mem(), 0));
        }
    }
    for &op in VBlend::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL {
            for (d, a, s) in triples() {
                for k in SOME {
                    emit_as!(
                        b,
                        mn,
                        vblend_rrrr(op, w, x(d), x(a), x(s), x(k)),
                        "{mn} {}, {}, {}, {}",
                        vr(w, d),
                        vr(w, a),
                        vr(w, s),
                        vr(w, k)
                    );
                }
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    vblend_rrmr(op, w, x(2), x(9), m.mem(), x(15)),
                    "{mn} {}, {}, {}, {}",
                    vr(w, 2),
                    vr(w, 9),
                    vm(w, m),
                    vr(w, 15)
                );
            }
        }
    }
    for &op in Fma::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL {
            for (d, a, s) in triples() {
                emit_as!(
                    b,
                    mn,
                    fma_rrr(op, w, x(d), x(a), x(s)),
                    "{mn} {}, {}, {}",
                    vr(w, d),
                    vr(w, a),
                    vr(w, s)
                );
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    fma_rrm(op, w, x(7), x(8), m.mem()),
                    "{mn} {}, {}, {}",
                    vr(w, 7),
                    vr(w, 8),
                    vm(w, m)
                );
            }
        }
    }
    for &op in VMask::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL {
            for (d, k) in [(0, 1), (8, 3), (5, 12), (15, 15)] {
                for m in some_mems() {
                    emit_as!(
                        b,
                        mn,
                        vmask_load(op, w, x(d), x(k), m.mem()),
                        "{mn} {}, {}, {}",
                        vr(w, d),
                        vr(w, k),
                        vm(w, m)
                    );
                    emit_as!(
                        b,
                        mn,
                        vmask_store(op, w, m.mem(), x(k), x(d)),
                        "{mn} {}, {}, {}",
                        vm(w, m),
                        vr(w, k),
                        vr(w, d)
                    );
                }
            }
        }
    }
    for op in VShift::ALL {
        let mn = op.mnemonic();
        for w in Width::ALL {
            for d in SOME {
                for s in SOME {
                    for i in IMM8 {
                        emit_as!(
                            b,
                            mn,
                            vshift_rri(op, w, x(d), x(s), i),
                            "{mn} {}, {}, {i}",
                            vr(w, d),
                            vr(w, s)
                        );
                    }
                }
            }
        }
    }
    for w in Width::ALL {
        for n in 0..16 {
            for m in some_mems() {
                emit!(b, vmovdqu_mr(w, m.mem(), x(n)), "vmovdqu {}, {}", vm(w, m), vr(w, n));
            }
            for g in SOME {
                let gr = reg(Size::D, g);
                emit!(b, vpmovmskb_rr(w, r(g), x(n)), "vpmovmskb {gr}, {}", vr(w, n));
                emit_as!(
                    b,
                    "vmovmskps",
                    vmovmskp_rr(false, w, r(g), x(n)),
                    "vmovmskps {gr}, {}",
                    vr(w, n)
                );
                emit_as!(
                    b,
                    "vmovmskpd",
                    vmovmskp_rr(true, w, r(g), x(n)),
                    "vmovmskpd {gr}, {}",
                    vr(w, n)
                );
            }
            for s in SOME {
                for i in IMM8 {
                    emit!(
                        b,
                        vcvtps2ph_rri(w, x(s), x(n), i),
                        "vcvtps2ph {}, {}, {i}",
                        xmm(s),
                        vr(w, n)
                    );
                }
            }
        }
    }
    emit!(b, vzeroupper(), "vzeroupper");
}
