//! Sweeps for `spec/mc-a64.md` "NEON intrinsics": every case of every
//! family on every shape it takes, against `llvm-mc` and `llvm-objdump`
//! of both LLVM 22 and LLVM 19, with the extensions the family needs.

use hod_mc_a64::*;

use crate::harness::{Batch, Tools};
use crate::text::{s as xsp, tuples, v, xs};

/// The extensions on top of the base features.
const EXTRA: &str =
    ",+fullfp16,+rdm,+dotprod,+i8mm,+bf16,+aes,+sha2,+sha3,+complxnum,+fp16fml,+v8.5a";

const BITS: [VecBits; 2] = [VecBits::V64, VecBits::V128];
const FORMATS: [FpFormat; 3] = [FpFormat::H, FpFormat::S, FpFormat::D];

/// A few register tuples per shape, and the full sweep of
/// [`tuples`] for one shape per constructor.
fn regs3(full: bool) -> Vec<[u32; 3]> {
    if full { tuples([32, 32, 32]) } else { vec![[0, 0, 0], [31, 1, 2], [5, 17, 30], [9, 31, 0]] }
}

fn regs2(full: bool) -> Vec<[u32; 2]> {
    regs3(full).into_iter().map(|[a, b, _]| [a, b]).collect()
}

/// The arrangement name of `bits` of `elem`.
fn arr(bits: VecBits, elem: Elem) -> &'static str {
    match (bits, elem) {
        (VecBits::V64, Elem::B) => "8b",
        (VecBits::V128, Elem::B) => "16b",
        (VecBits::V64, Elem::H) => "4h",
        (VecBits::V128, Elem::H) => "8h",
        (VecBits::V64, Elem::S) => "2s",
        (VecBits::V128, Elem::S) => "4s",
        (VecBits::V64, Elem::D) => "1d",
        (VecBits::V128, Elem::D) => "2d",
    }
}

fn elem_of(fmt: FpFormat) -> Elem {
    match fmt {
        FpFormat::H => Elem::H,
        FpFormat::S => Elem::S,
        FpFormat::D => Elem::D,
    }
}

fn wider(elem: Elem) -> Elem {
    match elem {
        Elem::B => Elem::H,
        Elem::H => Elem::S,
        Elem::S | Elem::D => Elem::D,
    }
}

/// The scalar view letter of `elem`.
fn view(elem: Elem) -> char {
    match elem {
        Elem::B => 'b',
        Elem::H => 'h',
        Elem::S => 's',
        Elem::D => 'd',
    }
}

fn int3(b: &mut Batch) {
    for (i, &op) in Int3::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            for elem in Elem::ALL {
                let t = arr(bits, elem);
                if !(op.vector_elem(elem) && t != "1d") {
                    reject!(
                        b,
                        try_neon_int3(op, bits, elem, v(0), v(1), v(2)),
                        "{mn} v0.{t}, v1.{t}, v2.{t}"
                    );
                    continue;
                }
                let full = i == 0 && t == "4s";
                for [d, n, m] in regs3(full) {
                    emit_as!(
                        b,
                        mn,
                        try_neon_int3(op, bits, elem, v(d), v(n), v(m)),
                        "{mn} v{d}.{t}, v{n}.{t}, v{m}.{t}"
                    );
                }
            }
        }
        for elem in Elem::ALL {
            let p = view(elem);
            if op.scalar_elem(elem) {
                for [d, n, m] in regs3(i == 2 && p == 'd') {
                    emit_as!(
                        b,
                        mn,
                        try_neon_int3_scalar(op, elem, v(d), v(n), v(m)),
                        "{mn} {p}{d}, {p}{n}, {p}{m}"
                    );
                }
            } else {
                reject!(
                    b,
                    try_neon_int3_scalar(op, elem, v(0), v(1), v(2)),
                    "{mn} {p}0, {p}1, {p}2"
                );
            }
        }
    }
}

fn fp3(b: &mut Batch) {
    for (i, &op) in Fp3::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            for fmt in FORMATS {
                let t = arr(bits, elem_of(fmt));
                if t == "1d" {
                    reject!(
                        b,
                        try_neon_fp3(op, bits, fmt, v(0), v(1), v(2)),
                        "{mn} v0.{t}, v1.{t}, v2.{t}"
                    );
                    continue;
                }
                for [d, n, m] in regs3(i == 0 && t == "8h") {
                    emit_as!(
                        b,
                        mn,
                        try_neon_fp3(op, bits, fmt, v(d), v(n), v(m)),
                        "{mn} v{d}.{t}, v{n}.{t}, v{m}.{t}"
                    );
                }
            }
        }
        for fmt in FORMATS {
            let p = view(elem_of(fmt));
            if op.has_scalar() {
                for [d, n, m] in regs3(i == 4 && p == 'h') {
                    emit_as!(
                        b,
                        mn,
                        try_neon_fp3_scalar(op, fmt, v(d), v(n), v(m)),
                        "{mn} {p}{d}, {p}{n}, {p}{m}"
                    );
                }
            } else {
                // The scalar FP data-processing class has some of these
                // (`FMAX <Sd>`), so the text is not an error to LLVM.
                reject!(b, try_neon_fp3_scalar(op, fmt, v(0), v(1), v(2)));
            }
        }
    }
}

fn int2(b: &mut Batch) {
    for (i, &op) in Int2::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            for elem in Elem::ALL {
                let t = arr(bits, elem);
                // The source of a narrowing case and the result of a
                // widening one.
                let other = if op.narrows() {
                    arr(VecBits::V128, wider(elem))
                } else if op.widens() {
                    arr(bits, wider(elem))
                } else {
                    t
                };
                let shape_ok = op.vector_shape(bits, elem) && t != "1d"
                    || op.vector_shape(bits, elem) && op.narrows();
                if !shape_ok {
                    // A `2` form of a narrowing case is another mnemonic.
                    let text =
                        (!op.narrows() && !op.widens()).then(|| format!("{mn} v0.{t}, v1.{t}"));
                    b.reject("try_neon_int2", try_neon_int2(op, bits, elem, v(0), v(1)), "", text);
                    continue;
                }
                for [d, n] in regs2(i == 0 && t == "16b") {
                    let asm = if op.narrows() {
                        format!("{mn} v{d}.{t}, v{n}.{other}")
                    } else if op.widens() {
                        format!("{mn} v{d}.{other}, v{n}.{t}")
                    } else {
                        format!("{mn} v{d}.{t}, v{n}.{t}")
                    };
                    b.push(
                        "try_neon_int2",
                        try_neon_int2(op, bits, elem, v(d), v(n)),
                        Some(mn.into()),
                        asm,
                    );
                }
            }
        }
        for elem in Elem::ALL {
            let p = view(elem);
            if op.scalar_elem(elem) {
                let q = if op.narrows() { view(wider(elem)) } else { p };
                for [d, n] in regs2(i == 0 && p == 'b') {
                    emit_as!(
                        b,
                        mn,
                        try_neon_int2_scalar(op, elem, v(d), v(n)),
                        "{mn} {p}{d}, {q}{n}"
                    );
                }
            } else {
                reject!(b, try_neon_int2_scalar(op, elem, v(0), v(1)));
            }
        }
    }
}

fn fp2(b: &mut Batch) {
    for (i, &op) in Fp2::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            for fmt in FORMATS {
                let t = arr(bits, elem_of(fmt));
                if !op.vector_shape(bits, fmt) {
                    reject!(b, try_neon_fp2(op, bits, fmt, v(0), v(1)), "{mn} v0.{t}, v1.{t}");
                    continue;
                }
                for [d, n] in regs2(i == 0 && t == "4h") {
                    emit_as!(
                        b,
                        mn,
                        try_neon_fp2(op, bits, fmt, v(d), v(n)),
                        "{mn} v{d}.{t}, v{n}.{t}"
                    );
                }
            }
        }
        for fmt in FORMATS {
            let p = view(elem_of(fmt));
            if op.scalar_format(fmt) {
                for [d, n] in regs2(i == 0 && p == 's') {
                    emit_as!(
                        b,
                        mn,
                        try_neon_fp2_scalar(op, fmt, v(d), v(n)),
                        "{mn} {p}{d}, {p}{n}"
                    );
                }
            } else {
                reject!(b, try_neon_fp2_scalar(op, fmt, v(0), v(1)));
            }
        }
    }
    for [d, n] in regs2(true) {
        emit!(b, bfcvtn(VecBits::V64, v(d), v(n)), "bfcvtn v{d}.4h, v{n}.4s");
        emit!(b, bfcvtn(VecBits::V128, v(d), v(n)), "bfcvtn2 v{d}.8h, v{n}.4s");
        emit!(b, bfcvt(v(d), v(n)), "bfcvt h{d}, s{n}");
        emit!(b, fcvtxn(v(d), v(n)), "fcvtxn v{d}.2s, v{n}.2d");
        emit!(b, fcvtxn_scalar(v(d), v(n)), "fcvtxn s{d}, d{n}");
    }
}

fn reductions(b: &mut Batch) {
    for bits in BITS {
        for elem in Elem::ALL {
            let t = arr(bits, elem);
            if matches!(t, "2s" | "1d" | "2d") {
                reject!(b, try_saddlv(bits, elem, v(0), v(1)), "saddlv d0, v1.{t}");
                continue;
            }
            let p = view(wider(elem));
            for [d, n] in regs2(t == "8b") {
                emit!(b, try_saddlv(bits, elem, v(d), v(n)), "saddlv {p}{d}, v{n}.{t}");
            }
        }
    }
    for &op in FpAcross::ALL {
        let mn = op.mnemonic();
        for bits in BITS {
            for fmt in FORMATS {
                let t = arr(bits, elem_of(fmt));
                let p = view(elem_of(fmt));
                if !matches!(t, "4h" | "8h" | "4s") {
                    reject!(b, try_neon_fp_across(op, bits, fmt, v(0), v(1)), "{mn} {p}0, v1.{t}");
                    continue;
                }
                for [d, n] in regs2(t == "4s") {
                    emit_as!(
                        b,
                        mn,
                        try_neon_fp_across(op, bits, fmt, v(d), v(n)),
                        "{mn} {p}{d}, v{n}.{t}"
                    );
                }
            }
        }
    }
    for [d, n] in regs2(true) {
        emit!(b, addp_scalar(v(d), v(n)), "addp d{d}, v{n}.2d");
    }
    for &op in FpPair::ALL {
        let mn = op.mnemonic();
        for fmt in FORMATS {
            let p = view(elem_of(fmt));
            for [d, n] in regs2(fmt == FpFormat::S) {
                emit_as!(b, mn, neon_fp_pair(op, fmt, v(d), v(n)), "{mn} {p}{d}, v{n}.2{p}");
            }
        }
    }
}

fn long3(b: &mut Batch) {
    for (i, &op) in Long3::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for elem in Elem::ALL {
            let narrow = arr(VecBits::V64, elem);
            let wide = if elem == Elem::D { "1q" } else { arr(VecBits::V128, wider(elem)) };
            if !op.takes(elem) {
                let text = if op.narrows() {
                    format!("{mn} v0.{narrow}, v1.{wide}, v2.{wide}")
                } else {
                    format!("{mn} v0.{wide}, v1.{narrow}, v2.{narrow}")
                };
                reject!(b, try_neon_long3(op, elem, v(0), v(1), v(2)), "{text}");
                continue;
            }
            for [d, n, m] in regs3(i == 0 && elem == Elem::H) {
                if op.narrows() {
                    emit_as!(
                        b,
                        mn,
                        try_neon_long3(op, elem, v(d), v(n), v(m)),
                        "{mn} v{d}.{narrow}, v{n}.{wide}, v{m}.{wide}"
                    );
                } else {
                    emit_as!(
                        b,
                        mn,
                        try_neon_long3(op, elem, v(d), v(n), v(m)),
                        "{mn} v{d}.{wide}, v{n}.{narrow}, v{m}.{narrow}"
                    );
                }
            }
        }
    }
    for elem in Elem::ALL {
        let (p, q) = (view(wider(elem)), view(elem));
        if matches!(elem, Elem::H | Elem::S) {
            for [d, n, m] in regs3(elem == Elem::S) {
                emit!(
                    b,
                    try_sqdmull_scalar(elem, v(d), v(n), v(m)),
                    "sqdmull {p}{d}, {q}{n}, {q}{m}"
                );
            }
        } else {
            reject!(b, try_sqdmull_scalar(elem, v(0), v(1), v(2)), "sqdmull {p}0, {q}1, {q}2");
        }
    }
}

fn shifts(b: &mut Batch) {
    for (i, &op) in ShiftImm::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            for elem in Elem::ALL {
                let t = arr(bits, elem);
                let src = if op.kind() == ShiftKind::Narrow {
                    arr(VecBits::V128, wider(elem))
                } else {
                    t
                };
                if !op.vector_shape(bits, elem) {
                    let text = (op.kind() != ShiftKind::Narrow)
                        .then(|| format!("{mn} v0.{t}, v1.{src}, #1"));
                    b.reject(
                        "try_neon_shift",
                        try_neon_shift(op, bits, elem, v(0), v(1), 1),
                        "",
                        text,
                    );
                    continue;
                }
                let (lo, hi) = match op.kind() {
                    ShiftKind::Left => (0, elem.bits() - 1),
                    ShiftKind::Right | ShiftKind::Narrow => (1, elem.bits()),
                };
                for amount in lo..=hi {
                    emit_as!(
                        b,
                        mn,
                        try_neon_shift(op, bits, elem, v(3), v(17), amount),
                        "{mn} v3.{t}, v17.{src}, #{amount}"
                    );
                }
                for [d, n] in regs2(i == 0 && t == "4s") {
                    emit_as!(
                        b,
                        mn,
                        try_neon_shift(op, bits, elem, v(d), v(n), lo),
                        "{mn} v{d}.{t}, v{n}.{src}, #{lo}"
                    );
                }
                let bad = if lo == 0 { hi + 1 } else { 0 };
                reject!(
                    b,
                    try_neon_shift(op, bits, elem, v(0), v(1), bad),
                    "{mn} v0.{t}, v1.{src}, #{bad}"
                );
                reject!(
                    b,
                    try_neon_shift(op, bits, elem, v(0), v(1), hi + 1),
                    "{mn} v0.{t}, v1.{src}, #{}",
                    hi + 1
                );
            }
        }
        for elem in Elem::ALL {
            let p = view(elem);
            let q = if op.kind() == ShiftKind::Narrow { view(wider(elem)) } else { p };
            if !op.scalar_elem(elem) {
                reject!(b, try_neon_shift_scalar(op, elem, v(0), v(1), 1));
                continue;
            }
            let (lo, hi) = match op.kind() {
                ShiftKind::Left => (0, elem.bits() - 1),
                ShiftKind::Right | ShiftKind::Narrow => (1, elem.bits()),
            };
            for amount in lo..=hi {
                emit_as!(
                    b,
                    mn,
                    try_neon_shift_scalar(op, elem, v(30), v(2), amount),
                    "{mn} {p}30, {q}2, #{amount}"
                );
            }
            reject!(
                b,
                try_neon_shift_scalar(op, elem, v(0), v(1), hi + 1),
                "{mn} {p}0, {q}1, #{}",
                hi + 1
            );
        }
    }
}

fn products(b: &mut Batch) {
    for (i, &op) in Dot::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for bits in BITS {
            let wide = arr(bits, Elem::S);
            let narrow = match op {
                Dot::Sdot | Dot::Udot | Dot::Usdot | Dot::Smmla | Dot::Ummla | Dot::Usmmla => {
                    arr(bits, Elem::B)
                }
                Dot::Fmlal | Dot::Fmlal2 | Dot::Fmlsl | Dot::Fmlsl2 => {
                    if bits == VecBits::V64 {
                        "2h"
                    } else {
                        "4h"
                    }
                }
                _ => arr(bits, Elem::H),
            };
            if bits == VecBits::V64 && op.only_128() {
                reject!(
                    b,
                    try_neon_dot(op, bits, v(0), v(1), v(2)),
                    "{mn} v0.{wide}, v1.{narrow}, v2.{narrow}"
                );
                continue;
            }
            for [d, n, m] in regs3(i == 0 && bits == VecBits::V128) {
                emit_as!(
                    b,
                    mn,
                    try_neon_dot(op, bits, v(d), v(n), v(m)),
                    "{mn} v{d}.{wide}, v{n}.{narrow}, v{m}.{narrow}"
                );
            }
        }
    }
    for bits in BITS {
        for fmt in FORMATS {
            let t = arr(bits, elem_of(fmt));
            if t == "1d" {
                reject!(
                    b,
                    try_fcmla(bits, fmt, v(0), v(1), v(2), 0),
                    "fcmla v0.1d, v1.1d, v2.1d, #0"
                );
                reject!(
                    b,
                    try_fcadd(bits, fmt, v(0), v(1), v(2), false),
                    "fcadd v0.1d, v1.1d, v2.1d, #90"
                );
                continue;
            }
            for rot in 0..4 {
                for [d, n, m] in regs3(rot == 1 && t == "4s") {
                    emit!(
                        b,
                        try_fcmla(bits, fmt, v(d), v(n), v(m), rot),
                        "fcmla v{d}.{t}, v{n}.{t}, v{m}.{t}, #{}",
                        rot * 90
                    );
                }
            }
            reject!(
                b,
                try_fcmla(bits, fmt, v(0), v(1), v(2), 4),
                "fcmla v0.{t}, v1.{t}, v2.{t}, #360"
            );
            for (rot270, deg) in [(false, 90), (true, 270)] {
                for [d, n, m] in regs3(rot270 && t == "8h") {
                    emit!(
                        b,
                        try_fcadd(bits, fmt, v(d), v(n), v(m), rot270),
                        "fcadd v{d}.{t}, v{n}.{t}, v{m}.{t}, #{deg}"
                    );
                }
            }
        }
    }
}

fn crypto(b: &mut Batch) {
    for (i, &op) in Aes::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for [d, n] in regs2(i == 0) {
            emit_as!(b, mn, aes(op, v(d), v(n)), "{mn} v{d}.16b, v{n}.16b");
        }
    }
    for (i, &op) in Sha3Reg::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for [d, n, m] in regs3(i == 0) {
            let asm = match op {
                Sha3Reg::Sha1c | Sha3Reg::Sha1p | Sha3Reg::Sha1m => {
                    format!("{mn} q{d}, s{n}, v{m}.4s")
                }
                Sha3Reg::Sha256h | Sha3Reg::Sha256h2 => format!("{mn} q{d}, q{n}, v{m}.4s"),
                Sha3Reg::Sha1su0 | Sha3Reg::Sha256su1 => format!("{mn} v{d}.4s, v{n}.4s, v{m}.4s"),
                Sha3Reg::Sha512h | Sha3Reg::Sha512h2 => format!("{mn} q{d}, q{n}, v{m}.2d"),
                Sha3Reg::Sha512su1 | Sha3Reg::Rax1 => format!("{mn} v{d}.2d, v{n}.2d, v{m}.2d"),
            };
            b.push("sha3reg", sha3reg(op, v(d), v(n), v(m)), Some(mn.into()), asm);
        }
    }
    for (i, &op) in Sha2Reg::ALL.iter().enumerate() {
        let mn = op.mnemonic();
        for [d, n] in regs2(i == 0) {
            let asm = match op {
                Sha2Reg::Sha1h => format!("{mn} s{d}, s{n}"),
                Sha2Reg::Sha1su1 | Sha2Reg::Sha256su0 => format!("{mn} v{d}.4s, v{n}.4s"),
                Sha2Reg::Sha512su0 => format!("{mn} v{d}.2d, v{n}.2d"),
            };
            b.push("sha2reg", sha2reg(op, v(d), v(n)), Some(mn.into()), asm);
        }
    }
    for [d, n, m] in regs3(true) {
        let a = (d + n + m) % 32;
        emit!(b, eor3(v(d), v(n), v(m), v(a)), "eor3 v{d}.16b, v{n}.16b, v{m}.16b, v{a}.16b");
        emit!(b, bcax(v(d), v(n), v(m), v(a)), "bcax v{d}.16b, v{n}.16b, v{m}.16b, v{a}.16b");
        emit!(b, try_xar(v(d), v(n), v(m), a * 2), "xar v{d}.2d, v{n}.2d, v{m}.2d, #{}", a * 2);
    }
    for amount in 0..64 {
        emit!(b, try_xar(v(1), v(2), v(3), amount), "xar v1.2d, v2.2d, v3.2d, #{amount}");
    }
    reject!(b, try_xar(v(1), v(2), v(3), 64), "xar v1.2d, v2.2d, v3.2d, #64");
}

/// `{v<r>.<t>, ...}` of `n` consecutive registers.
fn list(r: u32, n: u32, t: &str) -> String {
    let regs: Vec<String> = (0..n).map(|k| format!("v{}.{t}", (r + k) % 32)).collect();
    format!("{{{}}}", regs.join(", "))
}

fn tables(b: &mut Batch) {
    for extend in [false, true] {
        let mn = if extend { "tbx" } else { "tbl" };
        for len in 1..=4 {
            for bits in BITS {
                let t = arr(bits, Elem::B);
                for [d, n, m] in regs3(len == 4 && extend && bits == VecBits::V128) {
                    emit_as!(
                        b,
                        mn,
                        try_tbl_n(extend, len, bits, v(d), v(n), v(m)),
                        "{mn} v{d}.{t}, {}, v{m}.{t}",
                        list(n, len, "16b")
                    );
                }
            }
        }
        reject!(b, try_tbl_n(extend, 0, VecBits::V64, v(0), v(1), v(2)), "{mn} v0.8b, {{}}, v2.8b");
        reject!(
            b,
            try_tbl_n(extend, 5, VecBits::V64, v(0), v(1), v(2)),
            "{mn} v0.8b, {}, v2.8b",
            list(1, 5, "16b")
        );
    }
}

fn structures(b: &mut Batch) {
    for &op in Multi::ALL {
        let mn = op.mnemonic();
        for bits in BITS {
            for elem in Elem::ALL {
                let t = arr(bits, elem);
                let regs = list(7, op.registers(), t);
                if op.interleaves() && t == "1d" {
                    reject!(b, try_ldst_multi(op, bits, elem, v(7), xsp(3)), "{mn} {regs}, [x3]");
                    continue;
                }
                for [r, n] in regs2(op == Multi::Ld4 && t == "8h") {
                    let base = xs(n);
                    emit_as!(
                        b,
                        mn,
                        try_ldst_multi(op, bits, elem, v(r), xsp(n)),
                        "{mn} {}, [{base}]",
                        list(r, op.registers(), t)
                    );
                }
            }
        }
    }
    for load in [true, false] {
        for n in 1..=4 {
            let mn = format!("{}{n}", if load { "ld" } else { "st" });
            for elem in Elem::ALL {
                let p = view(elem);
                for index in 0..elem.lanes() {
                    emit_as!(
                        b,
                        mn.clone(),
                        try_ldst_lane(load, n, elem, index, v(30), xsp(2)),
                        "{mn} {}[{index}], [x2]",
                        list(30, n, &p.to_string())
                    );
                }
                for [r, x] in regs2(n == 2 && elem == Elem::H && load) {
                    let base = xs(x);
                    emit_as!(
                        b,
                        mn.clone(),
                        try_ldst_lane(load, n, elem, 1, v(r), xsp(x)),
                        "{mn} {}[1], [{base}]",
                        list(r, n, &p.to_string())
                    );
                }
                let lanes = elem.lanes();
                reject!(
                    b,
                    try_ldst_lane(load, n, elem, lanes, v(0), xsp(1)),
                    "{mn} {}[{lanes}], [x1]",
                    list(0, n, &p.to_string())
                );
            }
        }
        reject!(b, try_ldst_lane(load, 0, Elem::B, 0, v(0), xsp(1)));
        reject!(b, try_ldst_lane(load, 5, Elem::B, 0, v(0), xsp(1)));
    }
    for n in 1..=4 {
        let mn = format!("ld{n}r");
        for bits in BITS {
            for elem in Elem::ALL {
                let t = arr(bits, elem);
                for [r, x] in regs2(n == 3 && t == "4s") {
                    let base = xs(x);
                    emit_as!(
                        b,
                        mn.clone(),
                        try_ld_rep(n, bits, elem, v(r), xsp(x)),
                        "{mn} {}, [{base}]",
                        list(r, n, t)
                    );
                }
            }
        }
    }
    reject!(b, try_ld_rep(0, VecBits::V64, Elem::B, v(0), xsp(1)));
    reject!(b, try_ld_rep(5, VecBits::V64, Elem::B, v(0), xsp(1)));
}

pub fn build(b: &mut Batch) {
    int3(b);
    fp3(b);
    int2(b);
    fp2(b);
    reductions(b);
    long3(b);
    shifts(b);
    products(b);
    crypto(b);
    tables(b);
    structures(b);
}

fn check(major: u32) {
    let mut batch = Batch::default();
    build(&mut batch);
    batch.check_with(&format!("neon (LLVM {major})"), Tools::find_with(Some(major), EXTRA));
}

#[test]
fn neon_matches_llvm_22() {
    check(22);
}

#[test]
fn neon_matches_llvm_19() {
    check(19);
}
