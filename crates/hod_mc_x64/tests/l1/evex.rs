//! Sweeps for `spec/mc-x64.md` "EVEX" (WP-208): every member of every
//! family at each length it has (the others refused, by the checked form
//! and by `llvm-mc`), register positions on both sides of bits 3 and 4,
//! the three maskings, broadcast where allowed, every rounding mode on
//! the 512-bit register forms (accepted or refused), the immediates at
//! their boundaries, the sampled memory forms, and, for the compressed
//! displacement, every member's memory form at displacements around its
//! `N` (`evex-memory-operands`). The opmask instructions are their own
//! group (`opmask`).
//!
//! Every EVEX line is written with `{evex}`: LLVM's assembler would
//! otherwise choose a VEX form where one exists. The opmask instructions
//! are VEX-encoded and written without it (`{evex} kmovw` is APX's EVEX
//! form).

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::{M, Rng, r, reg, some_mems};

/// Registers on both sides of bits 3 and 4.
const SOME: [u8; 8] = [0, 7, 8, 15, 16, 23, 24, 31];

/// Immediates at the boundaries.
const IMM8: [u8; 4] = [0, 1, 0x7f, 0xff];

fn z(n: u8) -> Zmm {
    Zmm::new(n).expect("register 0..=31")
}

fn k(n: u8) -> K {
    K::new(n).expect("opmask 0..=7")
}

/// The name of vector register `n` at length `vl`.
fn vr(vl: Vl, n: u8) -> String {
    match vl {
        Vl::X => format!("xmm{n}"),
        Vl::Y => format!("ymm{n}"),
        Vl::Z => format!("zmm{n}"),
    }
}

/// Half the length, as a register (an `xmm` at least).
fn half(vl: Vl) -> Vl {
    match vl {
        Vl::Z => Vl::Y,
        _ => Vl::X,
    }
}

/// `xmmword ptr` and the like, by bytes.
fn ptr(bytes: u8) -> &'static str {
    match bytes {
        1 => "byte ptr",
        2 => "word ptr",
        4 => "dword ptr",
        8 => "qword ptr",
        16 => "xmmword ptr",
        32 => "ymmword ptr",
        64 => "zmmword ptr",
        _ => panic!("no operand of {bytes} bytes"),
    }
}

/// ` {k1}` / ` {k1} {z}` / nothing.
fn mk(m: Masking) -> String {
    match (m.k.code(), m.zero) {
        (0, false) => String::new(),
        (n, false) => format!(" {{k{n}}}"),
        (n, true) => format!(" {{k{n}}} {{z}}"),
    }
}

/// The maskings every member is swept with.
fn maskings() -> [Masking; 3] {
    [Masking::NONE, Masking::merge(k(1)), Masking::zero(k(7))]
}

/// The rounding operand's text.
fn rnd(round: Round) -> &'static str {
    match round {
        Round::Rn => "{rn-sae}",
        Round::Rd => "{rd-sae}",
        Round::Ru => "{ru-sae}",
        Round::Rz => "{rz-sae}",
        Round::Sae => "{sae}",
    }
}

/// The register pairs of the sweeps: every pair of [`SOME`].
fn pairs() -> Vec<(u8, u8)> {
    SOME.iter().flat_map(|&a| SOME.iter().map(move |&b| (a, b))).collect()
}

/// The memory operands of the compressed-displacement sweep for `n`:
/// both sides of each boundary of the scaled byte, and values that are
/// not multiples.
fn disp_mems(n: i32) -> Vec<M> {
    let mut disps = vec![0, n, -n, 127 * n, -128 * n, 128 * n, -129 * n, n + 1, 1, -1, 64];
    if n > 1 {
        disps.push(n / 2);
        disps.push(-(n / 2));
    }
    let mut out = Vec::new();
    for d in disps {
        out.push(M::base(0, d));
        out.push(M::base(13, d));
        out.push(M::bi(4, 9, 8, d));
    }
    out
}

/// `N` as the test states it, from the SDM's tables (not from the
/// encoder): `elem` the broadcast element, `div` the tuple's divisor of
/// the length, `fixed` a size that does not depend on the length.
fn n_of(vl: Vl, div: i32, fixed: i32, bcst: Option<u8>) -> i32 {
    match bcst {
        Some(e) => i32::from(e),
        None if fixed > 0 => fixed,
        None => i32::from(vl.bytes()) / div,
    }
}

/// `dword ptr [..]{1toN}`: `count` elements of `elem` bytes.
fn bcst(m: M, elem: u8, count: u8) -> String {
    format!("{} {}{{1to{count}}}", ptr(elem), m.text())
}

/// The second source of an [`E3`] member: 128 bits for the shift
/// counts and the scalar forms.
fn e3_b(op: E3, vl: Vl) -> Vl {
    use E3::*;
    match op {
        Psllw | Pslld | Psllq | Psrlw | Psrld | Psrlq | Psraw | Psrad | Psraq | Rcp14sd
        | Rsqrt14sd => Vl::X,
        _ => vl,
    }
}

/// `(divisor, fixed)` of an [`E3`] member's memory operand.
fn e3_mem(op: E3) -> (i32, i32) {
    use E3::*;
    match op {
        Psllw | Pslld | Psllq | Psrlw | Psrld | Psrlq | Psraw | Psrad | Psraq => (1, 16),
        Rcp14sd | Rsqrt14sd => (1, 8),
        _ => (1, 0),
    }
}

/// The `E3` sweep.
#[allow(clippy::too_many_lines, reason = "one sweep per form of the family")]
fn e3(b: &mut Batch) {
    for &op in E3::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            if !op.allows(vl) {
                reject!(
                    b,
                    try_e3_rrr(op, vl, Masking::NONE, z(0), z(1), z(2)),
                    "{{evex}} {mn} {}, {}, {}",
                    vr(vl, 0),
                    vr(vl, 1),
                    vr(e3_b(op, vl), 2)
                );
                reject!(b, try_e3_rrm(op, vl, Masking::NONE, z(0), z(1), M::base(0, 0).mem()));
                continue;
            }
            let bv = e3_b(op, vl);
            for (i, (d, a)) in pairs().into_iter().enumerate() {
                let s = SOME[(i * 3 + 1) % 8];
                emit_as!(
                    b,
                    mn,
                    try_e3_rrr(op, vl, Masking::NONE, z(d), z(a), z(s)),
                    "{{evex}} {mn} {}, {}, {}",
                    vr(vl, d),
                    vr(vl, a),
                    vr(bv, s)
                );
            }
            for m in maskings() {
                if op.masks() {
                    emit_as!(
                        b,
                        mn,
                        try_e3_rrr(op, vl, m, z(17), z(9), z(30)),
                        "{{evex}} {mn} {}{}, {}, {}",
                        vr(vl, 17),
                        mk(m),
                        vr(vl, 9),
                        vr(bv, 30)
                    );
                } else if m != Masking::NONE {
                    reject!(
                        b,
                        try_e3_rrr(op, vl, m, z(17), z(9), z(30)),
                        "{{evex}} {mn} {}{}, {}, {}",
                        vr(vl, 17),
                        mk(m),
                        vr(vl, 9),
                        vr(bv, 30)
                    );
                }
            }
            reject!(b, try_e3_rrr(op, vl, Masking::zero(K::K0), z(1), z(2), z(3)));
            let none = M::base(0, 0).mem();
            reject!(b, try_e3_rrm(op, vl, Masking::zero(K::K0), z(1), z(2), none));
            reject!(b, try_e3_rrb(op, vl, Masking::zero(K::K0), z(1), z(2), none));
            let (div, fixed) = e3_mem(op);
            let size = ptr(n_of(vl, div, fixed, None) as u8);
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_e3_rrm(op, vl, Masking::NONE, z(20), z(3), m.mem()),
                    "{{evex}} {mn} {}, {}, {size} {}",
                    vr(vl, 20),
                    vr(vl, 3),
                    m.text()
                );
            }
            let e = op.broadcast();
            if e == 0 {
                reject!(b, try_e3_rrb(op, vl, Masking::NONE, z(0), z(1), M::base(0, 0).mem()));
            } else {
                for m in [M::base(0, 0), M::base(12, -8), M::bi(5, 9, 4, 0x100)] {
                    emit_as!(
                        b,
                        mn,
                        try_e3_rrb(op, vl, Masking::merge(k(3)), z(4), z(25), m.mem()),
                        "{{evex}} {mn} {} {{k3}}, {}, {}",
                        vr(vl, 4),
                        vr(vl, 25),
                        bcst(m, e, vl.bytes() / e)
                    );
                }
            }
        }
        for round in Round::ALL {
            let text = format!("{{evex}} {mn} zmm1 {{k2}}, zmm18, zmm9, {}", rnd(round));
            if op.allows(Vl::Z) && op.rounds(round) {
                emit_as!(
                    b,
                    mn,
                    try_e3_rrr_round(op, Masking::merge(k(2)), z(1), z(18), z(9), round),
                    "{text}"
                );
            } else if op.allows(Vl::Z) {
                reject!(
                    b,
                    try_e3_rrr_round(op, Masking::merge(k(2)), z(1), z(18), z(9), round),
                    "{text}"
                );
            }
        }
    }
    for &op in E3Imm::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            for (i, (d, a)) in pairs().into_iter().enumerate() {
                let s = SOME[(i * 5 + 2) % 8];
                let imm = IMM8[i % 4];
                emit_as!(
                    b,
                    mn,
                    try_e3_rrri(op, vl, maskings()[i % 3], z(d), z(a), z(s), imm),
                    "{{evex}} {mn} {}{}, {}, {}, {imm}",
                    vr(vl, d),
                    mk(maskings()[i % 3]),
                    vr(vl, a),
                    vr(vl, s)
                );
            }
            let none = M::base(0, 0).mem();
            reject!(b, try_e3_rrri(op, vl, Masking::zero(K::K0), z(1), z(2), z(3), 0));
            reject!(b, try_e3_rrmi(op, vl, Masking::zero(K::K0), z(1), z(2), none, 0));
            reject!(b, try_e3_rrbi(op, vl, Masking::zero(K::K0), z(1), z(2), none, 0));
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_e3_rrmi(op, vl, Masking::NONE, z(2), z(29), m.mem(), 0x96),
                    "{{evex}} {mn} {}, {}, {} {}, 150",
                    vr(vl, 2),
                    vr(vl, 29),
                    ptr(vl.bytes()),
                    m.text()
                );
            }
            let e = op.broadcast();
            if e == 0 {
                reject!(b, try_e3_rrbi(op, vl, Masking::NONE, z(0), z(1), M::base(0, 0).mem(), 0));
            } else {
                for m in [M::base(0, 0), M::base(12, -8)] {
                    emit_as!(
                        b,
                        mn,
                        try_e3_rrbi(op, vl, Masking::zero(k(5)), z(31), z(8), m.mem(), 7),
                        "{{evex}} {mn} {} {{k5}} {{z}}, {}, {}, 7",
                        vr(vl, 31),
                        vr(vl, 8),
                        bcst(m, e, vl.bytes() / e)
                    );
                }
            }
        }
    }
}

/// The length of an [`E2`] member's destination and source at `vl`.
fn e2_lengths(op: E2, vl: Vl) -> (Vl, Vl) {
    use E2::*;
    match op {
        Cvtpd2dq | Cvtpd2ps | Cvttpd2dq | Cvttpd2udq | Cvtqq2ps | Cvtuqq2ps => (half(vl), vl),
        Cvttps2qq | Cvttps2uqq => (vl, half(vl)),
        _ => (vl, vl),
    }
}

/// The source's size in bytes of an [`E2`] member's memory form, and
/// the element count of its broadcast.
fn e2_mem(op: E2, vl: Vl) -> (u8, u8) {
    use E2::*;
    let bytes = vl.bytes();
    let e = op.broadcast();
    match op {
        Cvttps2qq | Cvttps2uqq => (bytes / 2, bytes / 8),
        _ => (bytes, bytes / e),
    }
}

/// The source text of a narrowing conversion from memory: LLVM's Intel
/// syntax needs the size to tell `xmm` from `ymm` sources apart.
#[allow(clippy::too_many_lines, reason = "one sweep per form of the family")]
fn e2(b: &mut Batch) {
    for &op in E2::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            let (dv, sv) = e2_lengths(op, vl);
            for (i, (d, s)) in pairs().into_iter().enumerate() {
                let m = maskings()[i % 3];
                emit_as!(
                    b,
                    mn,
                    try_e2_rr(op, vl, m, z(d), z(s)),
                    "{{evex}} {mn} {}{}, {}",
                    vr(dv, d),
                    mk(m),
                    vr(sv, s)
                );
            }
            reject!(b, try_e2_rr(op, vl, Masking::zero(K::K0), z(1), z(2)));
            reject!(b, try_e2_rm(op, vl, Masking::zero(K::K0), z(1), M::base(0, 0).mem()));
            reject!(b, try_e2_rb(op, vl, Masking::zero(K::K0), z(1), M::base(0, 0).mem()));
            let (bytes, count) = e2_mem(op, vl);
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_e2_rm(op, vl, Masking::merge(k(6)), z(19), m.mem()),
                    "{{evex}} {mn} {} {{k6}}, {} {}",
                    vr(dv, 19),
                    ptr(bytes),
                    m.text()
                );
            }
            for m in [M::base(0, 0), M::base(12, -8)] {
                emit_as!(
                    b,
                    mn,
                    try_e2_rb(op, vl, Masking::NONE, z(11), m.mem()),
                    "{{evex}} {mn} {}, {}",
                    vr(dv, 11),
                    bcst(m, op.broadcast(), count)
                );
            }
        }
        let (dv, sv) = e2_lengths(op, Vl::Z);
        for round in Round::ALL {
            let text =
                format!("{{evex}} {mn} {} {{k1}} {{z}}, {}, {}", vr(dv, 22), vr(sv, 6), rnd(round));
            if op.rounds(round) {
                emit_as!(
                    b,
                    mn,
                    try_e2_rr_round(op, Masking::zero(k(1)), z(22), z(6), round),
                    "{text}"
                );
            } else {
                reject!(b, try_e2_rr_round(op, Masking::zero(k(1)), z(22), z(6), round), "{text}");
            }
        }
    }
    for &op in E2Imm::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            if !op.allows(vl) {
                reject!(
                    b,
                    try_e2_rri(op, vl, Masking::NONE, z(0), z(1), 0),
                    "{{evex}} {mn} {}, {}, 0",
                    vr(vl, 0),
                    vr(vl, 1)
                );
                reject!(b, try_e2_rmi(op, vl, Masking::NONE, z(0), M::base(0, 0).mem(), 0));
                reject!(b, try_e2_rbi(op, vl, Masking::NONE, z(0), M::base(0, 0).mem(), 0));
                continue;
            }
            let none = M::base(0, 0).mem();
            reject!(b, try_e2_rri(op, vl, Masking::zero(K::K0), z(1), z(2), 0));
            reject!(b, try_e2_rmi(op, vl, Masking::zero(K::K0), z(1), none, 0));
            reject!(b, try_e2_rbi(op, vl, Masking::zero(K::K0), z(1), none, 0));
            for (i, (d, s)) in pairs().into_iter().enumerate() {
                let m = maskings()[i % 3];
                let imm = IMM8[i % 4];
                emit_as!(
                    b,
                    mn,
                    try_e2_rri(op, vl, m, z(d), z(s), imm),
                    "{{evex}} {mn} {}{}, {}, {imm}",
                    vr(vl, d),
                    mk(m),
                    vr(vl, s)
                );
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_e2_rmi(op, vl, Masking::NONE, z(28), m.mem(), 3),
                    "{{evex}} {mn} {}, {} {}, 3",
                    vr(vl, 28),
                    ptr(vl.bytes()),
                    m.text()
                );
            }
            let e = op.broadcast();
            for m in [M::base(0, 0), M::base(12, -8)] {
                emit_as!(
                    b,
                    mn,
                    try_e2_rbi(op, vl, Masking::merge(k(4)), z(5), m.mem(), 0x80),
                    "{{evex}} {mn} {} {{k4}}, {}, 128",
                    vr(vl, 5),
                    bcst(m, e, vl.bytes() / e)
                );
            }
        }
        for round in Round::ALL {
            if !op.allows(Vl::Z) {
                continue;
            }
            let text = format!("{{evex}} {mn} zmm3, zmm27, {}, 9", rnd(round));
            if op.rounds(round) {
                emit_as!(
                    b,
                    mn,
                    try_e2_rri_round(op, Masking::NONE, z(3), z(27), round, 9),
                    "{text}"
                );
            } else {
                reject!(b, try_e2_rri_round(op, Masking::NONE, z(3), z(27), round, 9), "{text}");
            }
        }
    }
}

/// The shifts by an immediate, the opmask compares, the stores, the moves.
#[allow(clippy::too_many_lines, reason = "one sweep per family")]
fn others(b: &mut Batch) {
    for &op in EShift::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            for (i, (d, s)) in pairs().into_iter().enumerate() {
                let m = maskings()[i % 3];
                let imm = IMM8[i % 4];
                emit_as!(
                    b,
                    mn,
                    try_eshift_rri(op, vl, m, z(d), z(s), imm),
                    "{{evex}} {mn} {}{}, {}, {imm}",
                    vr(vl, d),
                    mk(m),
                    vr(vl, s)
                );
            }
            reject!(b, try_eshift_rri(op, vl, Masking::zero(K::K0), z(1), z(2), 0));
            let none = M::base(0, 0).mem();
            reject!(b, try_eshift_rmi(op, vl, Masking::zero(K::K0), z(1), none, 0));
            reject!(b, try_eshift_rbi(op, vl, Masking::zero(K::K0), z(1), none, 0));
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    try_eshift_rmi(op, vl, Masking::NONE, z(21), m.mem(), 5),
                    "{{evex}} {mn} {}, {} {}, 5",
                    vr(vl, 21),
                    ptr(vl.bytes()),
                    m.text()
                );
            }
            let e = op.broadcast();
            if e == 0 {
                reject!(b, try_eshift_rbi(op, vl, Masking::NONE, z(0), M::base(0, 0).mem(), 0));
            } else {
                emit_as!(
                    b,
                    mn,
                    try_eshift_rbi(op, vl, Masking::NONE, z(12), M::base(3, 8).mem(), 63),
                    "{{evex}} {mn} {}, {}, 63",
                    vr(vl, 12),
                    bcst(M::base(3, 8), e, vl.bytes() / e)
                );
            }
        }
    }
    for &op in ECmp::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            for dk in 0..8 {
                for (mask, s) in [(0, 0), (1, 15), (7, 16), (dk, 31)] {
                    emit_as!(
                        b,
                        mn,
                        ecmp_kri(op, vl, k(mask), k(dk), z(s), 0x22),
                        "{{evex}} {mn} k{dk}{}, {}, 34",
                        mk(Masking::merge(k(mask))),
                        vr(vl, s)
                    );
                }
            }
            for m in some_mems() {
                emit_as!(
                    b,
                    mn,
                    ecmp_kmi(op, vl, k(2), k(5), m.mem(), 1),
                    "{{evex}} {mn} k5 {{k2}}, {} {}, 1",
                    ptr(vl.bytes()),
                    m.text()
                );
            }
            let e = op.broadcast();
            emit_as!(
                b,
                mn,
                ecmp_kbi(op, vl, K::K0, k(1), M::base(0, 4).mem(), 255),
                "{{evex}} {mn} k1, {}, 255",
                bcst(M::base(0, 4), e, vl.bytes() / e)
            );
        }
    }
    for &op in EStore::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            let (dv, bytes) = store_dst(op, vl);
            for (i, (d, s)) in pairs().into_iter().enumerate() {
                let m = maskings()[i % 3];
                emit_as!(
                    b,
                    mn,
                    try_estore_rr(op, vl, m, z(d), z(s)),
                    "{{evex}} {mn} {}{}, {}",
                    vr(dv, d),
                    mk(m),
                    vr(vl, s)
                );
            }
            reject!(b, try_estore_rr(op, vl, Masking::zero(K::K0), z(1), z(2)));
            for (i, m) in some_mems().into_iter().enumerate() {
                let mask = k([0, 1, 7][i % 3]);
                emit_as!(
                    b,
                    mn,
                    estore_mr(op, vl, mask, m.mem(), z(26)),
                    "{{evex}} {mn} {} {}{}, {}",
                    ptr(bytes),
                    m.text(),
                    mk(Masking::merge(mask)),
                    vr(vl, 26)
                );
            }
        }
    }
    for vl in Vl::ALL {
        let mn = "vcvtps2ph";
        for (i, (d, s)) in pairs().into_iter().enumerate() {
            let m = maskings()[i % 3];
            emit_as!(
                b,
                mn,
                try_evcvtps2ph_rri(vl, m, z(d), z(s), IMM8[i % 4]),
                "{{evex}} {mn} {}{}, {}, {}",
                vr(half(vl), d),
                mk(m),
                vr(vl, s),
                IMM8[i % 4]
            );
        }
        reject!(b, try_evcvtps2ph_rri(vl, Masking::zero(K::K0), z(0), z(1), 0));
        for (i, m) in some_mems().into_iter().enumerate() {
            let mask = k([0, 3][i % 2]);
            emit_as!(
                b,
                mn,
                evcvtps2ph_mri(vl, mask, m.mem(), z(17), 4),
                "{{evex}} {mn} {} {}{}, {}, 4",
                ptr(vl.bytes() / 2),
                m.text(),
                mk(Masking::merge(mask)),
                vr(vl, 17)
            );
        }
    }
    for &op in EMov::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            for (i, (d, s)) in pairs().into_iter().enumerate() {
                let m = maskings()[i % 3];
                emit_as!(
                    b,
                    mn,
                    try_emov_rr(op, vl, m, z(d), z(s)),
                    "{{evex}} {mn} {}{}, {}",
                    vr(vl, d),
                    mk(m),
                    vr(vl, s)
                );
            }
            reject!(b, try_emov_rr(op, vl, Masking::zero(K::K0), z(1), z(2)));
            reject!(b, try_emov_rm(op, vl, Masking::zero(K::K0), z(1), M::base(0, 0).mem()));
            for (i, m) in some_mems().into_iter().enumerate() {
                let mask = maskings()[i % 3];
                emit_as!(
                    b,
                    mn,
                    try_emov_rm(op, vl, mask, z(SOME[i % 8]), m.mem()),
                    "{{evex}} {mn} {}{}, {} {}",
                    vr(vl, SOME[i % 8]),
                    mk(mask),
                    ptr(vl.bytes()),
                    m.text()
                );
                let kk = k([0, 2][i % 2]);
                emit_as!(
                    b,
                    mn,
                    emov_mr(op, vl, kk, m.mem(), z(SOME[(i + 3) % 8])),
                    "{{evex}} {mn} {} {}{}, {}",
                    ptr(vl.bytes()),
                    m.text(),
                    mk(Masking::merge(kk)),
                    vr(vl, SOME[(i + 3) % 8])
                );
            }
        }
    }
}

/// The destination length of an [`EStore`] member's register form and
/// the bytes of its memory form.
fn store_dst(op: EStore, vl: Vl) -> (Vl, u8) {
    use EStore::*;
    let bytes = vl.bytes();
    match op {
        Pmovsqb | Pmovusqb => (Vl::X, bytes / 8),
        Pmovsqw | Pmovusqw | Pmovusdb => (Vl::X, bytes / 4),
        Pmovsqd | Pmovusqd => (half(vl), bytes / 2),
        _ => (vl, bytes),
    }
}

/// The EVEX families.
pub fn evex_forms(b: &mut Batch) {
    e3(b);
    e2(b);
    others(b);
}

/// The compressed displacement: every member's memory form at every
/// length, at displacements around its `N` (stated here from the SDM's
/// tuple tables), plain and broadcast; absolute fields stay `disp32`.
#[allow(clippy::too_many_lines, reason = "one sweep per family")]
pub fn memory(b: &mut Batch) {
    for &op in E3::ALL {
        let mn = op.mnemonic();
        let (div, fixed) = e3_mem(op);
        for vl in Vl::ALL.into_iter().filter(|&vl| op.allows(vl)) {
            let n = n_of(vl, div, fixed, None);
            for m in disp_mems(n) {
                emit_as!(
                    b,
                    mn,
                    try_e3_rrm(op, vl, Masking::NONE, z(16), z(1), m.mem()),
                    "{{evex}} {mn} {}, {}, {} {}",
                    vr(vl, 16),
                    vr(vl, 1),
                    ptr(n as u8),
                    m.text()
                );
            }
            let e = op.broadcast();
            if e != 0 {
                for m in disp_mems(n_of(vl, 1, 0, Some(e))) {
                    emit_as!(
                        b,
                        mn,
                        try_e3_rrb(op, vl, Masking::NONE, z(16), z(1), m.mem()),
                        "{{evex}} {mn} {}, {}, {}",
                        vr(vl, 16),
                        vr(vl, 1),
                        bcst(m, e, vl.bytes() / e)
                    );
                }
            }
        }
    }
    for &op in E2::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            let (dv, _) = e2_lengths(op, vl);
            let (bytes, count) = e2_mem(op, vl);
            for m in disp_mems(i32::from(bytes)) {
                emit_as!(
                    b,
                    mn,
                    try_e2_rm(op, vl, Masking::NONE, z(9), m.mem()),
                    "{{evex}} {mn} {}, {} {}",
                    vr(dv, 9),
                    ptr(bytes),
                    m.text()
                );
            }
            for m in disp_mems(i32::from(op.broadcast())) {
                emit_as!(
                    b,
                    mn,
                    try_e2_rb(op, vl, Masking::NONE, z(9), m.mem()),
                    "{{evex}} {mn} {}, {}",
                    vr(dv, 9),
                    bcst(m, op.broadcast(), count)
                );
            }
        }
    }
    for &op in EStore::ALL {
        let mn = op.mnemonic();
        for vl in Vl::ALL {
            let (_, bytes) = store_dst(op, vl);
            // The compresses' `N` is the element, whatever the length.
            let n = match op {
                EStore::Pcompressb => 1,
                EStore::Pcompressw => 2,
                EStore::Pcompressd | EStore::Compressps => 4,
                EStore::Pcompressq | EStore::Compresspd => 8,
                _ => i32::from(bytes),
            };
            for m in disp_mems(n) {
                emit_as!(
                    b,
                    mn,
                    estore_mr(op, vl, K::K0, m.mem(), z(24)),
                    "{{evex}} {mn} {} {}, {}",
                    ptr(bytes),
                    m.text(),
                    vr(vl, 24)
                );
            }
        }
    }
    for vl in Vl::ALL {
        for m in disp_mems(i32::from(vl.bytes())) {
            emit_as!(
                b,
                "vmovdqu64",
                try_emov_rm(EMov::Movdqu64, vl, Masking::NONE, z(30), m.mem()),
                "{{evex}} vmovdqu64 {}, {} {}",
                vr(vl, 30),
                ptr(vl.bytes()),
                m.text()
            );
            emit_as!(
                b,
                "vmovups",
                emov_mr(EMov::Movups, vl, K::K0, m.mem(), z(7)),
                "{{evex}} vmovups {} {}, {}",
                ptr(vl.bytes()),
                m.text(),
                vr(vl, 7)
            );
        }
        for &op in EShift::ALL {
            let mn = op.mnemonic();
            for m in disp_mems(i32::from(vl.bytes())) {
                emit_as!(
                    b,
                    mn,
                    try_eshift_rmi(op, vl, Masking::NONE, z(2), m.mem(), 1),
                    "{{evex}} {mn} {}, {} {}, 1",
                    vr(vl, 2),
                    ptr(vl.bytes()),
                    m.text()
                );
            }
        }
        for m in disp_mems(i32::from(vl.bytes() / 2)) {
            emit_as!(
                b,
                "vcvtps2ph",
                evcvtps2ph_mri(vl, K::K0, m.mem(), z(17), 4),
                "{{evex}} vcvtps2ph {} {}, {}, 4",
                ptr(vl.bytes() / 2),
                m.text(),
                vr(vl, 17)
            );
        }
    }
    // Absolute fields: `disp32` whatever `N`.
    for m in [M::base(0, 64), M::base(5, 0), M::bi(4, 9, 8, 128), M::abs(64)] {
        let field = m.mem().with_abs32().expect("not RIP-relative");
        emit_as!(
            b,
            "vpaddd",
            try_e3_rrm(E3::Paddd, Vl::Z, Masking::NONE, z(0), z(1), field),
            "{{disp32}} {{evex}} vpaddd zmm0, zmm1, zmmword ptr {}",
            m.text()
        );
    }
}

/// The opmask instructions.
pub fn opmask(b: &mut Batch) {
    let sizes = [(Size::B, 'b'), (Size::W, 'w'), (Size::D, 'd'), (Size::Q, 'q')];
    let mut rng = Rng::new(208);
    for (size, c) in sizes {
        let gsize = if size == Size::Q { Size::Q } else { Size::D };
        for d in 0..8 {
            for s in 0..8 {
                emit_as!(b, format!("kmov{c}"), kmov_kk(size, k(d), k(s)), "kmov{c} k{d}, k{s}");
                let t = (d + s * 3) % 8;
                emit_as!(
                    b,
                    format!("kand{c}"),
                    try_k3_kkk(K3::And, size, k(d), k(s), k(t)),
                    "kand{c} k{d}, k{s}, k{t}"
                );
            }
            for n in 0..16 {
                emit_as!(
                    b,
                    format!("kmov{c}"),
                    kmov_kr(size, k(d), r(n)),
                    "kmov{c} k{d}, {}",
                    reg(gsize, n)
                );
                emit_as!(
                    b,
                    format!("kmov{c}"),
                    kmov_rk(size, r(n), k(d)),
                    "kmov{c} {}, k{d}",
                    reg(gsize, n)
                );
            }
        }
        let bytes = match size {
            Size::B => 1,
            Size::W => 2,
            Size::D => 4,
            Size::Q => 8,
        };
        for (i, m) in some_mems().into_iter().enumerate() {
            let n = (i % 8) as u8;
            emit_as!(
                b,
                format!("kmov{c}"),
                kmov_km(size, k(n), m.mem()),
                "kmov{c} k{n}, {} {}",
                ptr(bytes),
                m.text()
            );
            emit_as!(
                b,
                format!("kmov{c}"),
                kmov_mk(size, m.mem(), k(n)),
                "kmov{c} {} {}, k{n}",
                ptr(bytes),
                m.text()
            );
        }
        for op in K3::ALL {
            let Some(mn) = op.mnemonic(size) else {
                reject!(b, try_k3_kkk(op, size, k(1), k(2), k(3)), "kunpckb k1, k2, k3");
                continue;
            };
            for _ in 0..16 {
                let (d, a, s) =
                    ((rng.next() % 8) as u8, (rng.next() % 8) as u8, (rng.next() % 8) as u8);
                emit_as!(b, mn, try_k3_kkk(op, size, k(d), k(a), k(s)), "{mn} k{d}, k{a}, k{s}");
            }
        }
        for op in K2::ALL {
            let mn = format!("{}{c}", op.stem());
            for d in 0..8 {
                let s = (d * 5 + 3) % 8;
                emit_as!(b, mn.clone(), k2_kk(op, size, k(d), k(s)), "{mn} k{d}, k{s}");
            }
        }
        for op in KShift::ALL {
            let mn = format!("{}{c}", op.stem());
            for (d, s) in [(0, 7), (7, 0), (3, 4)] {
                for imm in IMM8 {
                    emit_as!(
                        b,
                        mn.clone(),
                        kshift_kki(op, size, k(d), k(s), imm),
                        "{mn} k{d}, k{s}, {imm}"
                    );
                }
            }
        }
    }
}
