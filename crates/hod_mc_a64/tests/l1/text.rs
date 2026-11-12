//! Assembly-text formatting and operand sweeps for the L1 tests. Nothing
//! here uses the encoder's bit-level code: register names come from
//! register numbers, operation names from tables written out below.

use hod_mc_a64::{
    Arr, Barrier, Cond, Elem, Extend, FpArr, FpFormat, FpSize, Index, LitLoad, Load, MemSize,
    Order, Reg, RegSp, RegZr, Shift, Size, SysReg, VMemSize, VPairSize, VReg, VecBits,
};

/// Operand values from register numbers.
pub fn z(n: u32) -> RegZr {
    RegZr::new(n as u8).expect("register number 0..=31")
}

pub fn s(n: u32) -> RegSp {
    RegSp::new(n as u8).expect("register number 0..=31")
}

pub fn g(n: u32) -> Reg {
    Reg::new(n as u8).expect("register number 0..=30")
}

/// An FP/SIMD register from its number.
pub fn v(n: u32) -> VReg {
    VReg::new(n as u8).expect("register number 0..=31")
}

fn reg(prefix: char, n: u32, r31: &str) -> String {
    assert!(n <= 31);
    if n == 31 { r31.to_string() } else { format!("{prefix}{n}") }
}

/// `x<n>`, with 31 as `xzr`.
pub fn xz(n: u32) -> String {
    reg('x', n, "xzr")
}

/// `w<n>`, with 31 as `wzr`.
pub fn wz(n: u32) -> String {
    reg('w', n, "wzr")
}

/// `x<n>`, with 31 as `sp`.
pub fn xs(n: u32) -> String {
    reg('x', n, "sp")
}

/// `w<n>`, with 31 as `wsp`.
pub fn ws(n: u32) -> String {
    reg('w', n, "wsp")
}

/// A register of the given size in a position where 31 is the zero
/// register.
pub fn rz(sz: Size, n: u32) -> String {
    match sz {
        Size::W => wz(n),
        Size::X => xz(n),
    }
}

/// A register of the given size in a position where 31 is the stack
/// pointer.
pub fn rs(sz: Size, n: u32) -> String {
    match sz {
        Size::W => ws(n),
        Size::X => xs(n),
    }
}

/// A register named by a `w`/`x` prefix character, 31 as the zero register.
pub fn pz(prefix: char, n: u32) -> String {
    match prefix {
        'w' => wz(n),
        'x' => xz(n),
        _ => panic!("register prefix"),
    }
}

pub const CONDS: [(Cond, &str); 16] = [
    (Cond::Eq, "eq"),
    (Cond::Ne, "ne"),
    (Cond::Hs, "hs"),
    (Cond::Lo, "lo"),
    (Cond::Mi, "mi"),
    (Cond::Pl, "pl"),
    (Cond::Vs, "vs"),
    (Cond::Vc, "vc"),
    (Cond::Hi, "hi"),
    (Cond::Ls, "ls"),
    (Cond::Ge, "ge"),
    (Cond::Lt, "lt"),
    (Cond::Gt, "gt"),
    (Cond::Le, "le"),
    (Cond::Al, "al"),
    (Cond::Nv, "nv"),
];

pub const SHIFTS: [(Shift, &str); 4] =
    [(Shift::Lsl, "lsl"), (Shift::Lsr, "lsr"), (Shift::Asr, "asr"), (Shift::Ror, "ror")];

pub const EXTENDS: [(Extend, &str); 8] = [
    (Extend::Uxtb, "uxtb"),
    (Extend::Uxth, "uxth"),
    (Extend::Uxtw, "uxtw"),
    (Extend::Uxtx, "uxtx"),
    (Extend::Sxtb, "sxtb"),
    (Extend::Sxth, "sxth"),
    (Extend::Sxtw, "sxtw"),
    (Extend::Sxtx, "sxtx"),
];

/// (kind, mnemonic, unscaled mnemonic, register prefix, access bytes).
pub const LOADS: [(Load, &str, &str, char, i64); 9] = [
    (Load::B, "ldrb", "ldurb", 'w', 1),
    (Load::H, "ldrh", "ldurh", 'w', 2),
    (Load::W, "ldr", "ldur", 'w', 4),
    (Load::X, "ldr", "ldur", 'x', 8),
    (Load::SbW, "ldrsb", "ldursb", 'w', 1),
    (Load::ShW, "ldrsh", "ldursh", 'w', 2),
    (Load::SbX, "ldrsb", "ldursb", 'x', 1),
    (Load::ShX, "ldrsh", "ldursh", 'x', 2),
    (Load::SwX, "ldrsw", "ldursw", 'x', 4),
];

/// (width, mnemonic, unscaled mnemonic, register prefix, access bytes).
pub const STORES: [(MemSize, &str, &str, char, i64); 4] = [
    (MemSize::B, "strb", "sturb", 'w', 1),
    (MemSize::H, "strh", "sturh", 'w', 2),
    (MemSize::W, "str", "stur", 'w', 4),
    (MemSize::X, "str", "stur", 'x', 8),
];

/// (kind, mnemonic, register prefix).
pub const LIT_LOADS: [(LitLoad, &str, char); 3] =
    [(LitLoad::W, "ldr", 'w'), (LitLoad::X, "ldr", 'x'), (LitLoad::SwX, "ldrsw", 'x')];

/// (extension, name, index register prefix).
pub const INDEXES: [(Index, &str, char); 4] = [
    (Index::Uxtw, "uxtw", 'w'),
    (Index::Lsl, "lsl", 'x'),
    (Index::Sxtw, "sxtw", 'w'),
    (Index::Sxtx, "sxtx", 'x'),
];

/// (width, mnemonic suffix, register prefix).
pub const MEM_SIZES: [(MemSize, &str, char); 4] =
    [(MemSize::B, "b", 'w'), (MemSize::H, "h", 'w'), (MemSize::W, "", 'w'), (MemSize::X, "", 'x')];

/// (ordering, mnemonic infix).
pub const ORDERS: [(Order, &str); 4] =
    [(Order::Plain, ""), (Order::A, "a"), (Order::L, "l"), (Order::Al, "al")];

/// (precision, scalar register prefix).
pub const FP_SIZES: [(FpSize, char); 2] = [(FpSize::S, 's'), (FpSize::D, 'd')];

/// (format, scalar register prefix).
pub const FP_FORMATS: [(FpFormat, char); 3] =
    [(FpFormat::H, 'h'), (FpFormat::S, 's'), (FpFormat::D, 'd')];

/// (arrangement, its name after the dot, element prefix, elements).
pub const ARRS: [(Arr, &str, char, u32); 7] = [
    (Arr::B8, "8b", 'b', 8),
    (Arr::B16, "16b", 'b', 16),
    (Arr::H4, "4h", 'h', 4),
    (Arr::H8, "8h", 'h', 8),
    (Arr::S2, "2s", 's', 2),
    (Arr::S4, "4s", 's', 4),
    (Arr::D2, "2d", 'd', 2),
];

/// (arrangement, its name after the dot).
pub const FP_ARRS: [(FpArr, &str); 3] = [(FpArr::S2, "2s"), (FpArr::S4, "4s"), (FpArr::D2, "2d")];

/// (vector width, the byte arrangement of that width).
pub const VEC_BITS: [(VecBits, &str); 2] = [(VecBits::V64, "8b"), (VecBits::V128, "16b")];

/// (element width, its prefix, elements in a 128-bit register).
pub const ELEMS: [(Elem, char, u32); 4] =
    [(Elem::B, 'b', 16), (Elem::H, 'h', 8), (Elem::S, 's', 4), (Elem::D, 'd', 2)];

/// (width, scalar register prefix, access bytes).
pub const VMEM_SIZES: [(VMemSize, char, i64); 5] = [
    (VMemSize::B, 'b', 1),
    (VMemSize::H, 'h', 2),
    (VMemSize::S, 's', 4),
    (VMemSize::D, 'd', 8),
    (VMemSize::Q, 'q', 16),
];

/// (width, scalar register prefix, bytes per register).
pub const VPAIR_SIZES: [(VPairSize, char, i64); 3] =
    [(VPairSize::S, 's', 4), (VPairSize::D, 'd', 8), (VPairSize::Q, 'q', 16)];

/// The value an FMOV immediate field denotes, from the Arm ARM's prose
/// definition and without the crate's expansion: `(-1)^s * (16 + m) / 16 *
/// 2^e`, where the field is `s:b:cd:m`, and `e` is `cd + 1` when `b` is
/// clear and `cd - 3` when it is set. Every value is exact in single
/// precision.
pub fn fp_imm8_value(imm8: u8) -> f64 {
    let sign = if imm8 & 0x80 == 0 { 1.0 } else { -1.0 };
    let mantissa = f64::from(16 + (imm8 & 0xf)) / 16.0;
    let cd = i32::from((imm8 >> 4) & 3);
    let exponent = if imm8 & 0x40 == 0 { cd + 1 } else { cd - 3 };
    sign * mantissa * 2f64.powi(exponent)
}

/// The bit pattern of `value` at a precision. The conversion to single
/// precision is exact for the values the tests pass.
pub fn fp_bits(sz: FpSize, value: f64) -> u64 {
    match sz {
        FpSize::S => u64::from((value as f32).to_bits()),
        FpSize::D => value.to_bits(),
    }
}

pub const SYS_REGS: [(SysReg, &str); 4] = [
    (SysReg::Nzcv, "NZCV"),
    (SysReg::Fpcr, "FPCR"),
    (SysReg::Fpsr, "FPSR"),
    (SysReg::TpidrEl0, "TPIDR_EL0"),
];

pub const BARRIERS: [(Barrier, &str); 12] = [
    (Barrier::Oshld, "oshld"),
    (Barrier::Oshst, "oshst"),
    (Barrier::Osh, "osh"),
    (Barrier::Nshld, "nshld"),
    (Barrier::Nshst, "nshst"),
    (Barrier::Nsh, "nsh"),
    (Barrier::Ishld, "ishld"),
    (Barrier::Ishst, "ishst"),
    (Barrier::Ish, "ish"),
    (Barrier::Ld, "ld"),
    (Barrier::St, "st"),
    (Barrier::Sy, "sy"),
];

/// A small deterministic generator (xorshift64*), so sweeps are the same on
/// every run.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 11) % n
    }

    /// An element of `items`.
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Register-number tuples for `K` register positions: each position takes
/// every value below its limit (32 where encoding 31 is legal, 31 where it
/// is not) while the others hold fixed, distinct registers; then 16
/// pseudo-random tuples.
pub fn tuples<const K: usize>(limits: [u32; K]) -> Vec<[u32; K]> {
    const HOLD: [u32; 5] = [3, 17, 9, 26, 12];
    let mut out = Vec::new();
    for position in 0..K {
        for value in 0..limits[position] {
            let mut tuple = [0; K];
            tuple.copy_from_slice(&HOLD[..K]);
            tuple[position] = value;
            out.push(tuple);
        }
    }
    let mut rng = Rng::new(K as u64 + u64::from(limits.iter().sum::<u32>()));
    for _ in 0..16 {
        let mut tuple = [0; K];
        for (value, limit) in tuple.iter_mut().zip(limits) {
            *value = rng.below(u64::from(limit)) as u32;
        }
        out.push(tuple);
    }
    out
}

/// Every valid logical immediate of a `bits`-wide operand, ascending:
/// each rotation of each run of ones in each element size, replicated.
/// Built from the definition, without the encoder.
pub fn bitmask_patterns(bits: u32) -> Vec<u64> {
    let mut out = Vec::new();
    let mut esize = 2;
    while esize <= bits {
        let emask = (1u128 << esize) - 1;
        for ones in 1..esize {
            let run = (1u128 << ones) - 1;
            for rotation in 0..esize {
                let elem = ((run << rotation) | (run >> (esize - rotation))) & emask;
                let mut value = 0u128;
                let mut at = 0;
                while at < bits {
                    value |= elem << at;
                    at += esize;
                }
                out.push(value as u64);
            }
        }
        esize *= 2;
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The Arm ARM's `DecodeBitMasks` for a `bits`-wide operand, written from
/// the pseudocode and independent of the crate's decoder. `None` for a
/// reserved encoding.
pub fn decode_bitmask_reference(bits: u32, n: u32, immr: u32, imms: u32) -> Option<u64> {
    let key = (n << 6) | (!imms & 0x3f);
    let len = (0..7).rev().find(|bit| key >> bit & 1 == 1)?;
    if len < 1 || (1u32 << len) > bits {
        return None;
    }
    let esize = 1u32 << len;
    let levels = esize - 1;
    let (s, r) = (imms & levels, immr & levels);
    if s == levels {
        return None;
    }
    let emask = (1u128 << esize) - 1;
    let welem = (1u128 << (s + 1)) - 1;
    let elem = ((welem >> r) | (welem << (esize - r))) & emask;
    let mut value = 0u128;
    for copy in 0..bits / esize {
        value |= elem << (copy * esize);
    }
    Some(value as u64)
}
