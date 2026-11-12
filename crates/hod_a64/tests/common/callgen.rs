//! The call generator of `tests/call.rs` (WP-038): random signatures over
//! the classes `hod_a64::abi::assign` accepts, as chains of calls that
//! cross between C and IR in both directions.
//!
//! A case `k` is three functions:
//!
//! ```text
//! C driver --> h_k (IR, module A) --> m_k (IR, module B) --> c_k (C)
//! ```
//!
//! The driver (gcc) calls `h_k` with values it chose; `h_k` calls `m_k`,
//! and `m_k` calls `c_k`, each passing a random selection of its own
//! parameters (permuted, duplicated), constants and symbol addresses, in
//! registers and on the stack, directly or through a function pointer.
//! `c_k` checks every argument against what the generator knows it must
//! be, with the bits beyond a narrow integer checked exactly as far as the
//! ABI promises them (`abi-a64.md` §7, so an extension attribute is
//! observed through a wider C type), and returns a value that travels
//! back through `m_k` and `h_k` to the driver, which checks it the same
//! way. A case may instead make `c_k` variadic (hod's caller of a C
//! variadic function), or `m_k` variadic: it then calls `va_start` (and
//! `va_copy`) on module variables and hands the lists to `c_k`, which
//! walks them with `va_arg`. Returns cover registers of both classes,
//! aggregates, `sret` and results in memory.
//!
//! Modules A and B are compiled each by hod or by `llc`, so every
//! combination of caller and callee is linked and run.

#![allow(dead_code, reason = "each test uses part of the generator")]

use std::fmt::Write as _;

/// A small deterministic generator (`SplitMix64`).
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'t, T>(&mut self, items: &'t [T]) -> &'t T {
        &items[self.below(items.len() as u64) as usize]
    }

    pub fn u128(&mut self) -> u128 {
        u128::from(self.next()) | (u128::from(self.next()) << 64)
    }
}

/// An integer's extension attribute.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ext {
    No,
    Z,
    S,
}

impl Ext {
    pub fn ir(self) -> &'static str {
        match self {
            Ext::No => "",
            Ext::Z => " zeroext",
            Ext::S => " signext",
        }
    }
}

/// A short vector type (class `Fpr`): IR, C typedef, C element, lanes.
pub const VECS: &[(&str, &str, &str, u32)] = &[
    ("<4 x i32>", "v4si", "int", 4),
    ("<2 x i32>", "v2si", "int", 2),
    ("<8 x i16>", "v8hi", "short", 8),
    ("<16 x i8>", "v16qi", "signed char", 16),
    ("<2 x i64>", "v2di", "long", 2),
    ("<1 x i64>", "v1di", "long", 1),
    ("<2 x float>", "v2sf", "float", 2),
    ("<4 x float>", "v4sf", "float", 4),
    ("<2 x double>", "v2df", "double", 2),
    ("<4 x half>", "v4hf", "_Float16", 4),
];

/// A scalar leaf of an aggregate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Leaf {
    I64,
    F32,
    F64,
    F128,
    V4I32,
}

impl Leaf {
    fn bytes(self) -> usize {
        match self {
            Leaf::I64 | Leaf::F64 => 8,
            Leaf::F32 => 4,
            Leaf::F128 | Leaf::V4I32 => 16,
        }
    }
}

/// A C structure and the IR clang 22 passes and returns it as on this
/// target, with the `alignstack` clang puts on an HFA argument (which
/// moves a block of `float`s on the stack, `abi-a64.md` §3.3):
/// `(C name, C definition, argument IR or "" (not an argument),
/// return IR, the leaves of the IR value, which are also the C fields'
/// bytes in order)`. `ret` "" is a structure returned in memory.
pub struct AggTy {
    pub c: &'static str,
    pub def: &'static str,
    pub arg: &'static str,
    pub ret: &'static str,
    pub leaves: &'static [Leaf],
}

pub const AGGS: &[AggTy] = &[
    AggTy {
        c: "struct s2l",
        def: "struct s2l { long a, b; };",
        arg: "[2 x i64]",
        ret: "[2 x i64]",
        leaves: &[Leaf::I64, Leaf::I64],
    },
    AggTy {
        c: "struct mix",
        def: "struct mix { long a; double b; };",
        arg: "[2 x i64]",
        ret: "[2 x i64]",
        leaves: &[Leaf::I64, Leaf::I64],
    },
    AggTy {
        c: "struct hfa1",
        def: "struct hfa1 { float a; };",
        arg: "[1 x float] alignstack(8)",
        ret: "{ float }",
        leaves: &[Leaf::F32],
    },
    AggTy {
        c: "struct hfa3",
        def: "struct hfa3 { float a, b, c; };",
        arg: "[3 x float] alignstack(8)",
        ret: "{ float, float, float }",
        leaves: &[Leaf::F32, Leaf::F32, Leaf::F32],
    },
    AggTy {
        c: "struct hda2",
        def: "struct hda2 { double a, b; };",
        arg: "[2 x double] alignstack(8)",
        ret: "{ double, double }",
        leaves: &[Leaf::F64, Leaf::F64],
    },
    AggTy {
        c: "struct hda4",
        def: "struct hda4 { double a, b, c, d; };",
        arg: "[4 x double] alignstack(8)",
        ret: "{ double, double, double, double }",
        leaves: &[Leaf::F64, Leaf::F64, Leaf::F64, Leaf::F64],
    },
    AggTy {
        c: "struct hq2",
        def: "struct hq2 { long double a, b; };",
        arg: "[2 x fp128] alignstack(16)",
        ret: "{ fp128, fp128 }",
        leaves: &[Leaf::F128, Leaf::F128],
    },
    AggTy {
        c: "struct hva2",
        def: "struct hva2 { v4si a, b; };",
        arg: "[2 x <4 x i32>]",
        ret: "{ <4 x i32>, <4 x i32> }",
        leaves: &[Leaf::V4I32, Leaf::V4I32],
    },
    // Returned in memory (`x8`), not by `sret`: nine leaves.
    AggTy {
        c: "struct b9",
        def: "struct b9 { long a[9]; };",
        arg: "",
        ret: "[9 x i64]",
        leaves: &[Leaf::I64; 9],
    },
];

/// The structure passed by reference and returned by `sret`.
pub const BIG: &str = "struct big";
pub const BIG_BYTES: usize = 40;

/// A type as both sides see it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum T {
    /// `iN` with an attribute; the C side holds it in an integer of `k`
    /// bits (exactly `intN_t` when `k == n`).
    Int {
        n: u32,
        ext: Ext,
        k: u32,
    },
    /// `i1` as `_Bool`.
    Bool,
    Ptr,
    Half,
    BFloat,
    Float,
    Double,
    Quad,
    Vec(u8),
    Agg(u8),
    /// `struct big`: a pointer in IR, the structure in C (passed by
    /// reference to a copy).
    BigRef,
}

impl T {
    /// The IR type (an argument's, or a return value's with `ret`).
    pub fn ir(self, ret: bool) -> String {
        match self {
            T::Int { n, .. } => format!("i{n}"),
            T::Bool => "i1".into(),
            T::Ptr | T::BigRef => "ptr".into(),
            T::Half => "half".into(),
            T::BFloat => "bfloat".into(),
            T::Float => "float".into(),
            T::Double => "double".into(),
            T::Quad => "fp128".into(),
            T::Vec(v) => VECS[v as usize].0.into(),
            T::Agg(a) => {
                let agg = &AGGS[a as usize];
                let text = if ret { agg.ret } else { agg.arg };
                text.split(" alignstack").next().unwrap_or(text).into()
            }
        }
    }

    /// The attributes of a parameter or return value of this type.
    pub fn attrs(self, ret: bool) -> String {
        match self {
            T::Int { ext, .. } => ext.ir().into(),
            T::Agg(a) if !ret => {
                let arg = AGGS[a as usize].arg;
                arg.find(" alignstack").map_or(String::new(), |at| arg[at..].into())
            }
            _ => String::new(),
        }
    }

    /// Two types agree when a value of one can be passed as the other:
    /// the same IR type, and both or neither `struct big`.
    pub fn agrees(self, other: T, ret: bool) -> bool {
        self.ir(ret) == other.ir(ret) && (self == T::BigRef) == (other == T::BigRef)
    }

    /// The C type.
    pub fn c(self) -> String {
        match self {
            T::Int { n, ext, k } => {
                let signed = ext == Ext::S;
                match (k, k == n) {
                    (128, _) => if signed { "__int128" } else { "unsigned __int128" }.into(),
                    (_, _) => format!("{}int{k}_t", if signed { "" } else { "u" }),
                }
            }
            T::Bool => "_Bool".into(),
            T::Ptr => "void *".into(),
            T::Half => "_Float16".into(),
            T::BFloat => "__bf16".into(),
            T::Float => "float".into(),
            T::Double => "double".into(),
            T::Quad => "long double".into(),
            T::Vec(v) => VECS[v as usize].1.into(),
            T::Agg(a) => AGGS[a as usize].c.into(),
            T::BigRef => BIG.into(),
        }
    }

    /// The size of the C value.
    pub fn bytes(self) -> usize {
        match self {
            T::Int { k, .. } => k as usize / 8,
            T::Bool => 1,
            T::Ptr | T::Double => 8,
            T::Half | T::BFloat => 2,
            T::Float => 4,
            T::Quad => 16,
            T::Vec(v) => {
                let ir = VECS[v as usize].0;
                if ir == "<2 x i32>"
                    || ir == "<1 x i64>"
                    || ir == "<2 x float>"
                    || ir == "<4 x half>"
                {
                    8
                } else {
                    16
                }
            }
            T::Agg(a) => AGGS[a as usize].leaves.iter().map(|l| l.bytes()).sum(),
            T::BigRef => BIG_BYTES,
        }
    }

    /// The width of an integer type.
    pub fn int_bits(self) -> Option<u32> {
        match self {
            T::Int { n, .. } => Some(n),
            T::Bool => Some(1),
            _ => None,
        }
    }

    /// The `ParamAttrs` flags of the extension.
    pub fn ext(self) -> Ext {
        match self {
            T::Int { ext, .. } => ext,
            _ => Ext::No,
        }
    }
}

/// The integer widths the generator draws, and the edge values.
const WIDTHS: &[u32] = &[1, 7, 8, 15, 16, 17, 31, 32, 33, 48, 63, 64, 65, 100, 127, 128];

/// The C integer that holds an `iN`: exactly `intN_t` when C has it and
/// the extension the ABI promises fits (gcc does not extend a narrow
/// value it passes or returns), else the carrier.
fn carrier_for(rng: &mut Rng, n: u32, ext: Ext, carrier: u32) -> u32 {
    let exact = matches!(n, 8 | 16 | 32 | 64 | 128) && (ext == Ext::No || n >= 32);
    if exact && rng.chance(50) { n } else { carrier }
}

/// A random type for an argument (`ret` false) or a return value.
pub fn random_type(rng: &mut Rng, ret: bool, variable: bool) -> T {
    if variable {
        // What C passes as a variable argument (after the default
        // promotions).
        return match rng.below(12) {
            0 | 1 => T::Int { n: 32, ext: *rng.pick(&[Ext::No, Ext::S]), k: 32 },
            2 | 3 => T::Int { n: 64, ext: Ext::No, k: 64 },
            4 | 5 => T::Double,
            6 => T::Ptr,
            7 => T::Quad,
            8 => T::Int { n: 128, ext: Ext::No, k: 128 },
            9 => T::Agg(*rng.pick(&[0, 1, 4, 6, 7])),
            10 => T::Vec(0),
            _ => T::BigRef,
        };
    }
    match rng.below(20) {
        0..=6 => {
            let n = *rng.pick(WIDTHS);
            let ext = *rng.pick(&[Ext::No, Ext::No, Ext::Z, Ext::S]);
            // The C side: exactly the width when C has it, else the
            // next carrier.
            let carrier = if n <= 32 {
                32
            } else if n <= 64 {
                64
            } else {
                128
            };
            let k = carrier_for(rng, n, ext, carrier);
            if n == 1 && ext == Ext::No && rng.chance(50) {
                return T::Bool;
            }
            T::Int { n, ext, k }
        }
        7 | 8 => T::Ptr,
        9 => T::Half,
        10 => T::BFloat,
        11 => T::Float,
        12 => T::Double,
        13 => T::Quad,
        14 | 15 => T::Vec(rng.below(VECS.len() as u64) as u8),
        16..=18 => loop {
            let a = rng.below(AGGS.len() as u64) as u8;
            if ret || !AGGS[a as usize].arg.is_empty() {
                break T::Agg(a);
            }
        },
        _ => {
            if ret {
                T::Double
            } else {
                T::BigRef
            }
        }
    }
}

/// A value: an integer's low bits, bytes, or a symbol's address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum V {
    Int(u128),
    Bytes(Vec<u8>),
    /// A symbol of the C driver plus an addend; a function when the name
    /// starts with `c_` or `m_`.
    Sym(String, i64),
}

fn mask(bits: u32) -> u128 {
    if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

/// `value`, of `n` bits, extended to `to` bits by `ext` (zero for none).
fn extend(value: u128, n: u32, to: u32, signed: bool) -> u128 {
    let value = value & mask(n);
    if signed && n < 128 && value >> (n - 1) & 1 == 1 {
        (value | !mask(n)) & mask(to)
    } else {
        value & mask(to)
    }
}

/// A finite `float` with a random exponent.
fn random_f32(rng: &mut Rng) -> u32 {
    match rng.below(8) {
        0 => 0,
        1 => 0x8000_0000,
        2 => 0x3f80_0000,
        3 => 0x7f7f_ffff,
        4 => 0x0000_0001,
        _ => {
            let bits = rng.next() as u32;
            if bits & 0x7f80_0000 == 0x7f80_0000 { bits & 0xbfff_ffff } else { bits }
        }
    }
}

fn random_f64(rng: &mut Rng) -> u64 {
    match rng.below(8) {
        0 => 0,
        1 => 0x8000_0000_0000_0000,
        2 => 0x3ff0_0000_0000_0000,
        3 => 0x7fef_ffff_ffff_ffff,
        _ => {
            let bits = rng.next();
            if bits & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000 {
                bits & 0xbfff_ffff_ffff_ffff
            } else {
                bits
            }
        }
    }
}

fn random_f16(rng: &mut Rng) -> u16 {
    let bits = rng.next() as u16;
    if bits & 0x7c00 == 0x7c00 { bits & 0xbfff } else { bits }
}

fn random_bf16(rng: &mut Rng) -> u16 {
    let bits = rng.next() as u16;
    if bits & 0x7f80 == 0x7f80 { bits & 0xbfff } else { bits }
}

fn leaf_bytes(rng: &mut Rng, leaf: Leaf, out: &mut Vec<u8>) {
    match leaf {
        Leaf::I64 => out.extend_from_slice(&rng.next().to_le_bytes()),
        Leaf::F32 => out.extend_from_slice(&random_f32(rng).to_le_bytes()),
        Leaf::F64 => out.extend_from_slice(&random_f64(rng).to_le_bytes()),
        Leaf::F128 | Leaf::V4I32 => {
            out.extend_from_slice(&rng.next().to_le_bytes());
            out.extend_from_slice(&rng.next().to_le_bytes());
        }
    }
}

/// The C symbols a pointer may hold the address of: data objects of the
/// driver.
pub const DATA_SYMS: &[&str] = &["g_data", "g_more"];

/// A random value of `t`.
pub fn random_value(rng: &mut Rng, t: T) -> V {
    match t {
        T::Int { n, .. } => {
            let value = match rng.below(6) {
                0 => 0,
                1 => 1,
                2 => mask(n),
                3 => 1u128 << (n - 1),
                4 => mask(n) >> 1,
                _ => rng.u128(),
            };
            V::Int(value & mask(n))
        }
        T::Bool => V::Int(u128::from(rng.chance(50))),
        T::Ptr => match rng.below(4) {
            0 => V::Sym((*rng.pick(DATA_SYMS)).into(), rng.below(4) as i64 * 8),
            1 => V::Bytes(vec![0; 8]),
            _ => V::Bytes(rng.next().to_le_bytes().to_vec()),
        },
        T::Half => V::Bytes(random_f16(rng).to_le_bytes().to_vec()),
        T::BFloat => V::Bytes(random_bf16(rng).to_le_bytes().to_vec()),
        T::Float => V::Bytes(random_f32(rng).to_le_bytes().to_vec()),
        T::Double => V::Bytes(random_f64(rng).to_le_bytes().to_vec()),
        T::Quad | T::Vec(_) => {
            let mut bytes: Vec<u8> = Vec::new();
            for _ in 0..t.bytes().div_ceil(8) {
                bytes.extend_from_slice(&rng.next().to_le_bytes());
            }
            bytes.truncate(t.bytes());
            if rng.chance(15) {
                bytes.fill(0);
            }
            V::Bytes(bytes)
        }
        T::Agg(a) => {
            let mut bytes = Vec::new();
            for &leaf in AGGS[a as usize].leaves {
                leaf_bytes(rng, leaf, &mut bytes);
            }
            V::Bytes(bytes)
        }
        T::BigRef => {
            let mut bytes = Vec::new();
            for _ in 0..BIG_BYTES / 8 {
                bytes.extend_from_slice(&rng.next().to_le_bytes());
            }
            V::Bytes(bytes)
        }
    }
}

/// Whether `v` of `t` can be written as an IR constant that hod's
/// lowerings on this branch materialize.
pub fn has_const(t: T, v: &V) -> bool {
    match (t, v) {
        (T::Vec(_), V::Bytes(bytes)) => bytes.iter().all(|&b| b == 0),
        // Vector leaves: only zero.
        (T::Agg(a), V::Bytes(bytes)) => {
            !AGGS[a as usize].leaves.contains(&Leaf::V4I32) || bytes.iter().all(|&b| b == 0)
        }
        (T::BigRef, _) => false,
        _ => true,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "\\x{b:02x}");
        s
    })
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(word)
}

fn leaf_const(leaf: Leaf, bytes: &[u8]) -> String {
    match leaf {
        Leaf::I64 => format!("i64 {}", u64_at(bytes, 0) as i64),
        Leaf::F32 => {
            let bits = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            format!("float 0x{:016X}", f64::from(f32::from_bits(bits)).to_bits())
        }
        Leaf::F64 => format!("double 0x{:016X}", u64_at(bytes, 0)),
        Leaf::F128 => format!("fp128 0xL{:016X}{:016X}", u64_at(bytes, 0), u64_at(bytes, 8)),
        Leaf::V4I32 => "<4 x i32> zeroinitializer".into(),
    }
}

/// `v` of `t` as an IR operand: type and constant.
pub fn ir_const(t: T, v: &V, ret: bool) -> String {
    let ty = t.ir(ret);
    let value = match (t, v) {
        (T::Int { n, .. }, V::Int(x)) => {
            // Signed decimal, which LLVM reads for every width.
            let x = extend(*x, n, 128, true) as i128;
            format!("{x}")
        }
        (T::Bool, V::Int(x)) => if *x != 0 { "true" } else { "false" }.into(),
        (T::Ptr | T::BigRef, V::Sym(name, 0)) => format!("@{name}"),
        (T::Ptr, V::Sym(name, addend)) => format!("getelementptr (i8, ptr @{name}, i64 {addend})"),
        (T::Ptr, V::Bytes(bytes)) => match u64_at(bytes, 0) {
            0 => "null".into(),
            x => format!("inttoptr (i64 {} to ptr)", x as i64),
        },
        (T::Half, V::Bytes(b)) => format!("0xH{:04X}", u16::from_le_bytes([b[0], b[1]])),
        (T::BFloat, V::Bytes(b)) => format!("0xR{:04X}", u16::from_le_bytes([b[0], b[1]])),
        (T::Float, V::Bytes(b)) => leaf_const(Leaf::F32, b)["float ".len()..].into(),
        (T::Double, V::Bytes(b)) => format!("0x{:016X}", u64_at(b, 0)),
        (T::Quad, V::Bytes(b)) => format!("0xL{:016X}{:016X}", u64_at(b, 0), u64_at(b, 8)),
        (T::Vec(_), _) => "zeroinitializer".into(),
        (T::Agg(a), V::Bytes(bytes)) => {
            let agg = &AGGS[a as usize];
            let mut at = 0;
            let mut elems = Vec::new();
            for &leaf in agg.leaves {
                elems.push(leaf_const(leaf, &bytes[at..]));
                at += leaf.bytes();
            }
            if ty.starts_with('[') {
                format!("[{}]", elems.join(", "))
            } else {
                format!("{{ {} }}", elems.join(", "))
            }
        }
        _ => panic!("no constant of {t:?} for {v:?}"),
    };
    format!("{ty} {value}")
}

/// `v` as the driver passes or `c_k` returns it, a C expression of
/// `t.c()`. Bits the ABI leaves unspecified are filled from `rng`.
pub fn c_value(rng: &mut Rng, t: T, v: &V) -> String {
    match (t, v) {
        (T::Int { n, ext, k }, V::Int(x)) => {
            // Extended as far as the consumer may assume it (registers),
            // junk above.
            let to = guaranteed(t, Place::Reg).min(k);
            let mut image = extend(*x, n, to, ext == Ext::S);
            if to < k {
                image |= (rng.u128() << to) & mask(k);
            }
            int_literal(&t.c(), image, k)
        }
        (T::Bool, V::Int(x)) => format!("(_Bool){x}"),
        (_, V::Sym(name, addend)) => {
            if name.starts_with("c_") || name.starts_with("m_") {
                format!("(void *){name}")
            } else {
                format!("(void *)((char *)&{name} + {addend})")
            }
        }
        (_, V::Bytes(bytes)) => format!("MK({}, \"{}\")", t.c(), hex(bytes)),
        _ => panic!("no C value of {t:?} for {v:?}"),
    }
}

fn int_literal(ty: &str, image: u128, k: u32) -> String {
    if k == 128 {
        format!(
            "(({ty})(((unsigned __int128)0x{:x}ULL << 64) | 0x{:x}ULL))",
            (image >> 64) as u64,
            image as u64
        )
    } else {
        format!("(({ty})0x{:x}ULL)", image as u64)
    }
}

/// Where a value travels, for what its consumer may assume.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Reg,
    Stack,
}

/// How many low bits of an integer of type `t` its consumer may rely on
/// (`abi-a64.md` §7), at `place`.
pub fn guaranteed(t: T, place: Place) -> u32 {
    let Some(n) = t.int_bits() else { return 0 };
    let attrs = match t.ext() {
        Ext::No => hod_ir::ParamAttrs::NONE,
        Ext::Z => hod_ir::ParamAttrs::NONE.with(hod_ir::ParamAttrs::ZEROEXT).expect("flag"),
        Ext::S => hod_ir::ParamAttrs::NONE.with(hod_ir::ParamAttrs::SIGNEXT).expect("flag"),
    };
    let place = match place {
        Place::Reg => hod_cg::abi::Place::Reg(hod_cg::PReg::new(0)),
        Place::Stack => hod_cg::abi::Place::Stack(0),
    };
    hod_a64::abi::assumed_extension(n, attrs, place).map_or(n, |e| e.to.max(n))
}

/// The check that the C variable `name` of type `t` holds `v`, which a
/// compiled IR function produced at `place`: a C boolean expression.
pub fn c_check(t: T, v: &V, name: &str, place: Place) -> String {
    match (t, v) {
        (T::Int { n, ext, k }, V::Int(x)) => {
            let to = if k == n { k } else { guaranteed(t, place).min(k) };
            let want = extend(*x, n, to, ext == Ext::S);
            let m = mask(to);
            if k == 128 {
                format!(
                    "((((unsigned __int128){name}) & {}) == {})",
                    int_literal("unsigned __int128", m, 128),
                    int_literal("unsigned __int128", want, 128)
                )
            } else {
                format!("((((uint64_t){name}) & 0x{m:x}ULL) == 0x{want:x}ULL)")
            }
        }
        (T::Bool, V::Int(x)) => format!("({name} == {x})"),
        (_, V::Sym(..)) => format!("({name} == {})", c_value(&mut Rng(0), t, v)),
        (_, V::Bytes(bytes)) => {
            format!("(memcmp(&{name}, \"{}\", {}) == 0)", hex(bytes), bytes.len())
        }
        _ => panic!("no check of {t:?} for {v:?}"),
    }
}

/// The C definitions every driver starts with.
pub const C_PRELUDE: &str = r#"
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
typedef int v4si __attribute__((vector_size(16)));
typedef int v2si __attribute__((vector_size(8)));
typedef short v8hi __attribute__((vector_size(16)));
typedef signed char v16qi __attribute__((vector_size(16)));
typedef long v2di __attribute__((vector_size(16)));
typedef long v1di __attribute__((vector_size(8)));
typedef float v2sf __attribute__((vector_size(8)));
typedef float v4sf __attribute__((vector_size(16)));
typedef double v2df __attribute__((vector_size(16)));
typedef _Float16 v4hf __attribute__((vector_size(8)));
struct big { long a[5]; };
#define MK(T, s) ({ T v_; memcpy(&v_, s, sizeof v_); v_; })
static int fails, checks;
#define CHK(k, what, ok) do { checks++; if (!(ok)) { fails++; printf("FAIL case %d %s\n", k, what); } } while (0)
long g_data[8] = { 1, 2, 3, 4, 5, 6, 7, 8 };
long g_more[8];
"#;

/// A parameter or argument source in a caller: a parameter of the
/// caller, or a constant.
#[derive(Clone, Debug)]
pub enum Src {
    Param(usize),
    Const(V),
}

/// One call: what each argument is, and the value it has.
#[derive(Clone, Debug)]
pub struct Call {
    /// Named arguments, then variable ones: source, the callee's type of
    /// the argument, and its value.
    pub args: Vec<(Src, T, V)>,
    /// Through a pointer: the source of the callee's address.
    pub indirect: Option<Src>,
}

/// How the variable arguments of a case are used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variadic {
    None,
    /// `c_k` is a C variadic function.
    C,
    /// `m_k` is a variadic IR function that hands its `va_list` (and a
    /// copy) to `c_k`.
    M,
}

/// The return of a case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ret {
    Void,
    Value(T),
    /// `struct big` through `sret` at every level.
    Sret,
}

/// A case: the three signatures and the two calls.
#[derive(Clone, Debug)]
pub struct Case {
    pub k: usize,
    pub variadic: Variadic,
    /// `h_k`'s parameters, with the driver's values.
    pub h_params: Vec<(T, V)>,
    /// `m_k`'s named parameters.
    pub m_params: Vec<T>,
    /// `c_k`'s named parameters.
    pub c_params: Vec<T>,
    /// The variable arguments, with their values (`h_k` passes them to
    /// `m_k` for `Variadic::M`, `m_k` to `c_k` for `Variadic::C`).
    pub variable: Vec<(T, V)>,
    /// What `h_k` passes to `m_k`.
    pub h_call: Call,
    /// What `m_k` passes to `c_k` (named ones only for `Variadic::M`).
    pub m_call: Call,
    /// The return types of `h_k`, `m_k`, `c_k` (equal IR types).
    pub ret: [Ret; 3],
    /// The calling convention of `m_k` (" fastcc", ...; "" for `ccc`),
    /// which `abi-a64.md` §1.2 assigns as `ccc`.
    pub cc: &'static str,
    /// The value `c_k` returns.
    pub ret_value: Option<V>,
}

/// Builds the arguments of a call to a callee with parameters `params`
/// (and then `variable`) from a caller with parameters `have`, whose
/// values are `values`: each argument is a fitting parameter of the
/// caller or a constant. Returns the call, and adds to `have` the new
/// parameters the caller needs.
fn make_call(
    rng: &mut Rng,
    params: &[(T, V)],
    have: &mut Vec<(T, V)>,
    indirect: Option<V>,
) -> Call {
    let mut args = Vec::new();
    for (t, v) in params {
        let fits: Vec<usize> =
            (0..have.len()).filter(|&p| have[p].0.agrees(*t, false) && have[p].1 == *v).collect();
        let src = if !fits.is_empty() && rng.chance(60) {
            Src::Param(*rng.pick(&fits))
        } else if has_const(*t, v) && rng.chance(35) {
            Src::Const(v.clone())
        } else {
            // A new parameter of the caller, of an agreeing type.
            have.push((variant(rng, *t), v.clone()));
            Src::Param(have.len() - 1)
        };
        args.push((src, *t, v.clone()));
    }
    let indirect = indirect.map(|target| {
        if rng.chance(50) {
            Src::Const(target)
        } else {
            have.push((T::Ptr, target));
            Src::Param(have.len() - 1)
        }
    });
    Call { args, indirect }
}

/// A type that agrees with `t` (the same IR type), with another
/// attribute or C type where integers allow it.
fn variant(rng: &mut Rng, t: T) -> T {
    match t {
        T::Int { n, .. } => {
            let ext = *rng.pick(&[Ext::No, Ext::Z, Ext::S]);
            let carrier = if n <= 32 {
                32
            } else if n <= 64 {
                64
            } else {
                128
            };
            let ext = if n > 64 && !n.is_multiple_of(64) { Ext::No } else { ext };
            let k = carrier_for(rng, n, ext, carrier);
            T::Int { n, ext, k }
        }
        T::Bool => {
            if rng.chance(50) {
                T::Bool
            } else {
                T::Int { n: 1, ext: Ext::No, k: 32 }
            }
        }
        other => other,
    }
}

/// Shuffles the caller's parameters, renumbering the sources.
fn shuffle(rng: &mut Rng, have: &mut Vec<(T, V)>, calls: &mut [&mut Call]) {
    let n = have.len();
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    // order[new] = old.
    let mut new_of = vec![0; n];
    for (new, &old) in order.iter().enumerate() {
        new_of[old] = new;
    }
    let old = std::mem::take(have);
    *have = order.iter().map(|&o| old[o].clone()).collect();
    for call in calls.iter_mut() {
        for (src, _, _) in &mut call.args {
            if let Src::Param(p) = src {
                *p = new_of[*p];
            }
        }
        if let Some(Src::Param(p)) = &mut call.indirect {
            *p = new_of[*p];
        }
    }
}

/// A random case.
pub fn random_case(rng: &mut Rng, k: usize) -> Case {
    let variadic = match rng.below(10) {
        0 | 1 => Variadic::C,
        2 | 3 => Variadic::M,
        _ => Variadic::None,
    };
    let count = |rng: &mut Rng| -> usize {
        match rng.below(10) {
            0..=5 => rng.below(7) as usize,
            6..=8 => 6 + rng.below(8) as usize,
            _ => 12 + rng.below(10) as usize,
        }
    };
    let mut c_named: Vec<(T, V)> = (0..count(rng))
        .map(|_| {
            let t = random_type(rng, false, false);
            let v = random_value(rng, t);
            (t, v)
        })
        .collect();
    if variadic == Variadic::C && c_named.is_empty() {
        let t = T::Int { n: 32, ext: Ext::No, k: 32 };
        c_named.push((t, random_value(rng, t)));
    }
    let variable: Vec<(T, V)> = if variadic == Variadic::None {
        Vec::new()
    } else {
        (0..rng.below(12))
            .map(|_| {
                let t = random_type(rng, false, true);
                let v = random_value(rng, t);
                (t, v)
            })
            .collect()
    };
    let ret = match rng.below(10) {
        0 => Ret::Void,
        1 if variadic != Variadic::M => Ret::Sret,
        _ => Ret::Value(random_type(rng, true, false)),
    };
    let ret_value = match ret {
        Ret::Value(t) => Some(random_value(rng, t)),
        Ret::Sret => Some(random_value(rng, T::BigRef)),
        Ret::Void => None,
    };
    let rets = match ret {
        Ret::Value(t) => [Ret::Value(variant(rng, t)), Ret::Value(variant(rng, t)), ret],
        other => [other; 3],
    };

    // m_k -> c_k: the callee's arguments are c_named (and, for a C
    // variadic callee, the variable ones).
    let mut m_have: Vec<(T, V)> = Vec::new();
    let c_target = (rng.chance(25)).then(|| V::Sym(format!("c_{k}"), 0));
    let mut passed = c_named.clone();
    if variadic == Variadic::C {
        passed.extend(variable.iter().cloned());
    }
    let mut m_call = make_call(rng, &passed, &mut m_have, c_target);
    // Unused parameters.
    for _ in 0..rng.below(3) {
        let t = random_type(rng, false, false);
        let v = random_value(rng, t);
        m_have.push((t, v));
    }
    shuffle(rng, &mut m_have, &mut [&mut m_call]);

    // h_k -> m_k.
    let mut h_have: Vec<(T, V)> = Vec::new();
    let m_target = (rng.chance(25)).then(|| V::Sym(format!("m_{k}"), 0));
    let mut passed = m_have.clone();
    if variadic == Variadic::M {
        passed.extend(variable.iter().cloned());
    }
    let mut h_call = make_call(rng, &passed, &mut h_have, m_target);
    for _ in 0..rng.below(2) {
        let t = random_type(rng, false, false);
        let v = random_value(rng, t);
        h_have.push((t, v));
    }
    shuffle(rng, &mut h_have, &mut [&mut h_call]);

    let m_params: Vec<T> = m_have.into_iter().map(|(t, _)| t).collect();
    let c_params: Vec<T> = c_named.into_iter().map(|(t, _)| t).collect();
    let h_params = h_have;
    let retype = |call: &mut Call, named: &[T]| {
        for (index, (_, t, _)) in call.args.iter_mut().enumerate() {
            *t = named.get(index).copied().unwrap_or_else(|| variable[index - named.len()].0);
        }
    };
    retype(&mut h_call, &m_params);
    retype(&mut m_call, &c_params);

    let cc = if variadic == Variadic::M {
        ""
    } else {
        *rng.pick(&["", "", "", " fastcc", " swiftcc", " cxx_fast_tlscc"])
    };
    Case {
        k,
        variadic,
        h_params,
        m_params,
        c_params,
        variable,
        h_call,
        m_call,
        ret: rets,
        ret_value,
        cc,
    }
}

// ---- IR text ----

fn ret_ir(ret: Ret) -> (String, String) {
    match ret {
        Ret::Void | Ret::Sret => ("void".into(), String::new()),
        Ret::Value(t) => (t.ir(true), t.attrs(true)),
    }
}

/// The parameter list of a signature, with `sret` first when asked.
fn param_list(params: &[T], sret: bool, named: bool) -> String {
    let mut list: Vec<String> = Vec::new();
    if sret {
        list.push(format!("ptr sret({BIG_IR}) align 8"));
    }
    for t in params {
        list.push(format!("{}{}", t.ir(false), t.attrs(false)));
    }
    if named {
        list.push("...".into());
    }
    list.join(", ")
}

/// The IR of `struct big`.
pub const BIG_IR: &str = "[5 x i64]";

/// One call instruction's text: `%r = call ...` or `call void ...`.
fn call_ir(
    out: &mut String,
    callee: &str,
    call: &Call,
    ret: Ret,
    named: &[T],
    variadic: bool,
    sret_from: Option<&str>,
    cc: &str,
) {
    let (rty, rattrs) = ret_ir(ret);
    let mut types: Vec<String> = Vec::new();
    if ret == Ret::Sret {
        types.push("ptr".into());
    }
    types.extend(named.iter().map(|t| t.ir(false)));
    if variadic {
        types.push("...".into());
    }
    let fn_ty = format!("{rty} ({})", types.join(", "));
    let callee = match &call.indirect {
        Some(Src::Param(p)) => format!("%p{p}"),
        Some(Src::Const(_)) | None => format!("@{callee}"),
    };
    let mut args: Vec<String> = Vec::new();
    if let Some(sret) = sret_from {
        args.push(format!("ptr sret({BIG_IR}) align 8 {sret}"));
    }
    for (index, (src, t, _)) in call.args.iter().enumerate() {
        let attrs = if index < named.len() { t.attrs(false) } else { variable_attrs(*t) };
        let value = match src {
            Src::Param(p) => format!("{}{attrs} %p{p}", t.ir(false)),
            Src::Const(v) => {
                let text = ir_const(*t, v, false);
                let ty = t.ir(false);
                let rest = text.strip_prefix(&format!("{ty} ")).expect("type and value");
                format!("{ty}{attrs} {rest}")
            }
        };
        args.push(value);
    }
    let result = if matches!(ret, Ret::Value(_)) { "%r = " } else { "" };
    let _ = writeln!(out, "  {result}call{cc}{rattrs} {fn_ty} {callee}({})", args.join(", "));
}

/// The attributes of a variable argument: as clang writes them.
fn variable_attrs(t: T) -> String {
    t.attrs(false)
}

/// Module A: the `h_k`, with declarations of what they call.
pub fn module_a(cases: &[Case], header: &str) -> String {
    let mut out = String::from(header);
    out.push_str("@g_data = external global [8 x i64]\n@g_more = external global [8 x i64]\n");
    for case in cases {
        let k = case.k;
        // Only its address is taken here.
        let _ = writeln!(out, "declare void @c_{k}()");
        let (rty, rattrs) = ret_ir(case.ret[1]);
        let m_variadic = case.variadic == Variadic::M;
        let _ = writeln!(
            out,
            "declare{}{rattrs} {rty} @m_{k}({})",
            case.cc,
            param_list(&case.m_params, case.ret[1] == Ret::Sret, m_variadic)
        );
        let (rty, rattrs) = ret_ir(case.ret[0]);
        let mut params: Vec<String> = Vec::new();
        if case.ret[0] == Ret::Sret {
            params.push(format!("ptr sret({BIG_IR}) align 8 %out"));
        }
        for (p, (t, _)) in case.h_params.iter().enumerate() {
            params.push(format!("{}{} %p{p}", t.ir(false), t.attrs(false)));
        }
        let _ = writeln!(out, "define{rattrs} {rty} @h_{k}({}) {{", params.join(", "));
        let sret = (case.ret[0] == Ret::Sret).then_some("%out");
        call_ir(
            &mut out,
            &format!("m_{k}"),
            &case.h_call,
            case.ret[1],
            &case.m_params,
            m_variadic,
            sret,
            case.cc,
        );
        finish_ret(&mut out, case.ret[0]);
    }
    out
}

fn finish_ret(out: &mut String, ret: Ret) {
    match ret {
        Ret::Value(t) => {
            let _ = writeln!(out, "  ret {} %r\n}}", t.ir(true));
        }
        _ => out.push_str("  ret void\n}\n"),
    }
}

/// Module B: the `m_k`, the `va_list` variables, and declarations of the
/// `c_k`.
pub fn module_b(cases: &[Case], header: &str) -> String {
    let mut out = String::from(header);
    out.push_str("declare void @llvm.va_start.p0(ptr)\ndeclare void @llvm.va_end.p0(ptr)\n");
    out.push_str("declare void @llvm.va_copy.p0(ptr, ptr)\n");
    out.push_str("@g_data = external global [8 x i64]\n@g_more = external global [8 x i64]\n");
    for case in cases {
        let k = case.k;
        let m_variadic = case.variadic == Variadic::M;
        let mut c_named = case.c_params.clone();
        if m_variadic {
            c_named.push(T::Ptr);
            c_named.push(T::Ptr);
            let _ = writeln!(out, "@ap_{k} = internal global [4 x i64] zeroinitializer, align 8");
            let _ = writeln!(out, "@aq_{k} = internal global [4 x i64] zeroinitializer, align 8");
        }
        let (rty, rattrs) = ret_ir(case.ret[2]);
        let _ = writeln!(
            out,
            "declare{rattrs} {rty} @c_{k}({})",
            param_list(&c_named, case.ret[2] == Ret::Sret, case.variadic == Variadic::C)
        );
        let (rty, rattrs) = ret_ir(case.ret[1]);
        let mut params: Vec<String> = Vec::new();
        if case.ret[1] == Ret::Sret {
            params.push(format!("ptr sret({BIG_IR}) align 8 %out"));
        }
        for (p, t) in case.m_params.iter().enumerate() {
            params.push(format!("{}{} %p{p}", t.ir(false), t.attrs(false)));
        }
        if m_variadic {
            params.push("...".into());
        }
        let _ = writeln!(out, "define{}{rattrs} {rty} @m_{k}({}) {{", case.cc, params.join(", "));
        let mut call = case.m_call.clone();
        if m_variadic {
            let _ = writeln!(out, "  call void @llvm.va_start.p0(ptr @ap_{k})");
            let _ = writeln!(out, "  call void @llvm.va_copy.p0(ptr @aq_{k}, ptr @ap_{k})");
            for list in ["ap", "aq"] {
                call.args.push((Src::Const(V::Sym(format!("{list}_{k}"), 0)), T::Ptr, V::Int(0)));
            }
        }
        let sret = (case.ret[1] == Ret::Sret).then_some("%out");
        call_ir(
            &mut out,
            &format!("c_{k}"),
            &call,
            case.ret[2],
            &c_named,
            case.variadic == Variadic::C,
            sret,
            "",
        );
        if m_variadic {
            let _ = writeln!(out, "  call void @llvm.va_end.p0(ptr @aq_{k})");
            let _ = writeln!(out, "  call void @llvm.va_end.p0(ptr @ap_{k})");
        }
        finish_ret(&mut out, case.ret[1]);
    }
    out
}

// ---- C text ----

/// Where each argument of a call by the signature `params` (named, then
/// variable) arrives: on the stack or in a register, by `assign`.
pub type Places = Vec<Place>;

/// The C driver: the `c_k`, and `main` calling every `h_k`. `places(k)`
/// gives where each argument of `c_k` arrives.
pub fn driver(cases: &[Case], places: &dyn Fn(usize) -> Places) -> String {
    let mut out = String::from(C_PRELUDE);
    let mut rng = Rng(0x00c0_ffee);
    for agg in AGGS {
        let _ = writeln!(out, "{}", agg.def);
    }
    for case in cases {
        let k = case.k;
        let at = places(k);
        // c_k.
        let ret_c = |r: Ret| -> String {
            match r {
                Ret::Void => "void".into(),
                Ret::Sret => BIG.into(),
                Ret::Value(t) => t.c(),
            }
        };
        let mut params: Vec<String> =
            case.c_params.iter().enumerate().map(|(i, t)| format!("{} a{i}", t.c())).collect();
        if case.variadic == Variadic::M {
            params.push("va_list ap".into());
            params.push("va_list aq".into());
        }
        if case.variadic == Variadic::C {
            params.push("...".into());
        }
        if params.is_empty() {
            params.push("void".into());
        }
        let _ = writeln!(out, "{} c_{k}({}) {{", ret_c(case.ret[2]), params.join(", "));
        for (i, (t, v)) in case
            .m_call
            .args
            .iter()
            .enumerate()
            .take(case.c_params.len())
            .map(|(i, (_, t, v))| (i, (t, v)))
        {
            let place = at.get(i).copied().unwrap_or(Place::Reg);
            let _ = writeln!(
                out,
                "  CHK({k}, \"arg {i}\", {});",
                c_check(*t, v, &format!("a{i}"), place)
            );
        }
        let walk = |out: &mut String, list: &str| {
            for (j, (t, v)) in case.variable.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "  {{ {} x = va_arg({list}, {}); CHK({k}, \"{list} {j}\", {}); }}",
                    t.c(),
                    t.c(),
                    c_check(*t, v, "x", Place::Reg)
                );
            }
        };
        match case.variadic {
            Variadic::C => {
                let last = case.c_params.len() - 1;
                let _ = writeln!(out, "  va_list ap;\n  va_start(ap, a{last});");
                walk(&mut out, "ap");
                out.push_str("  va_end(ap);\n");
            }
            Variadic::M => {
                walk(&mut out, "ap");
                walk(&mut out, "aq");
            }
            Variadic::None => {}
        }
        match (case.ret[2], &case.ret_value) {
            (Ret::Value(t), Some(v)) => {
                let _ = writeln!(out, "  return {};", c_value(&mut rng, t, v));
            }
            (Ret::Sret, Some(v)) => {
                let _ = writeln!(out, "  return {};", c_value(&mut rng, T::BigRef, v));
            }
            _ => {}
        }
        out.push_str("}\n");
        // The prototype of h_k.
        let mut params: Vec<String> = case.h_params.iter().map(|(t, _)| t.c()).collect();
        if params.is_empty() {
            params.push("void".into());
        }
        let _ = writeln!(out, "{} h_{k}({});", ret_c(case.ret[0]), params.join(", "));
        // m_k is only called through pointers C takes the address of.
        let _ = writeln!(out, "void m_{k}(void);");
    }
    out.push_str("int main(void) {\n");
    for case in cases {
        let k = case.k;
        let args: Vec<String> =
            case.h_params.iter().map(|(t, v)| c_value(&mut rng, *t, v)).collect();
        let call = format!("h_{k}({})", args.join(", "));
        match (case.ret[0], &case.ret_value) {
            (Ret::Value(t), Some(v)) => {
                let _ = writeln!(
                    out,
                    "  {{ {} r = {call}; CHK({k}, \"result\", {}); }}",
                    t.c(),
                    c_check(t, v, "r", Place::Reg)
                );
            }
            (Ret::Sret, Some(v)) => {
                let _ = writeln!(
                    out,
                    "  {{ {BIG} r = {call}; CHK({k}, \"result\", {}); }}",
                    c_check(T::BigRef, v, "r", Place::Reg)
                );
            }
            _ => {
                let _ = writeln!(out, "  {call};");
            }
        }
    }
    out.push_str(
        "  printf(\"checked %d, %d failures\\n\", checks, fails);\n  return fails != 0;\n}\n",
    );
    out
}
