//! Vectors as values (WP-079, `spec/codegen.md` §5.5.1 "Vectors, the
//! base (WP-079)"): `extractelement`, `insertelement` and
//! `shufflevector` over every shape of byte-sized elements, and the
//! whole-vector forms (`select`, `phi`, `freeze`, constants, `bitcast`,
//! aggregates holding vectors), checked natively.
//!
//! The generator below builds one function per shape × operation ×
//! index, `void f(ptr out, ptr a, ptr b, i64 i)`, which loads its
//! vectors from `a` and `b` and stores its result at `out`. Each is
//! built by hod (plain and under every stress option), by
//! `llc -O0 -global-isel=0 -fast-isel=0`, and, where C can say it, as C
//! with `ext_vector_type` compiled by clang; one C driver runs every case
//! with sentinels around `out`, linked under bfd, lld and mold. Every
//! leg is compared with the model written here, byte by byte, on the
//! bytes that are defined (a lane index out of range, an undefined mask
//! lane and a `poison` lane define nothing), and every leg must leave
//! the sentinels alone (hod's stores write the store size).
//!
//! `hod_interp` does not model vectors (`spec/interp.md`), so it is not
//! a leg here.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::format_collect,
    clippy::many_single_char_names,
    reason = "a test generator: `.ll` and C text built by hand"
)]

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, compile_ll, ll_header, llc, llc_major, parse, write_object};
use hod_a64::{compile_module, vector};
use hod_cg::{Options, Stress};
use hod_testutil::{Scratch, run_with, tool};

/// Bytes of each input image.
const IN: usize = 64;
/// Bytes of the output buffer: sentinels, at most 128 result bytes at
/// offset [`AT`], sentinels.
const OUT: usize = 256;
/// Where the result goes in the output buffer.
const AT: usize = 64;
/// What the driver fills the output buffer with.
const SENTINEL: u8 = 0xA5;

// ---- shapes ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I8,
    I16,
    I32,
    I64,
    Ptr,
    Half,
    BFloat,
    Float,
    Double,
}

impl E {
    fn bytes(self) -> usize {
        match self {
            E::I8 => 1,
            E::I16 | E::Half | E::BFloat => 2,
            E::I32 | E::Float => 4,
            E::I64 | E::Ptr | E::Double => 8,
        }
    }

    fn ll(self) -> &'static str {
        match self {
            E::I8 => "i8",
            E::I16 => "i16",
            E::I32 => "i32",
            E::I64 => "i64",
            E::Ptr => "ptr",
            E::Half => "half",
            E::BFloat => "bfloat",
            E::Float => "float",
            E::Double => "double",
        }
    }

    /// The C element type of the same bits, for the clang leg.
    fn c(self) -> Option<&'static str> {
        match self {
            E::I8 => Some("signed char"),
            E::I16 => Some("short"),
            E::I32 => Some("int"),
            E::I64 | E::Ptr => Some("long long"),
            E::Float => Some("float"),
            E::Double => Some("double"),
            E::Half | E::BFloat => None,
        }
    }

    /// A constant element of `bits` (the element's own width) as `.ll`
    /// text; never a NaN.
    fn lit(self, bits: u64) -> String {
        match self {
            E::I8 => format!("{}", bits as u8 as i8),
            E::I16 => format!("{}", bits as u16 as i16),
            E::I32 => format!("{}", bits as u32 as i32),
            E::I64 => format!("{}", bits as i64),
            E::Ptr => "null".into(),
            E::Half => format!("0xH{:04X}", bits as u16),
            E::BFloat => format!("0xR{:04X}", bits as u16),
            E::Float => format!("0x{:016X}", f64::from(f32::from_bits(bits as u32)).to_bits()),
            E::Double => format!("0x{bits:016X}"),
        }
    }

    /// `bits` made a valid constant: no NaN, and `null` for a pointer.
    fn tame(self, bits: u64) -> u64 {
        match self {
            E::Ptr => 0,
            E::Half | E::BFloat if self.exp_all_ones(bits) => bits & !0x4000,
            E::Float | E::Double if self.exp_all_ones(bits) => {
                bits & !(1 << (self.bytes() * 8 - 2))
            }
            _ => bits & (u64::MAX >> (64 - 8 * self.bytes())),
        }
    }

    fn exp_all_ones(self, bits: u64) -> bool {
        let mask: u64 = match self {
            E::Half => 0x7C00,
            E::BFloat => 0x7F80,
            E::Float => 0x7F80_0000,
            E::Double => 0x7FF0_0000_0000_0000,
            _ => return false,
        };
        bits & mask == mask
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct V {
    e: E,
    n: u32,
}

impl V {
    const fn new(e: E, n: u32) -> Self {
        Self { e, n }
    }

    fn ll(self) -> String {
        format!("<{} x {}>", self.n, self.e.ll())
    }

    fn store(self) -> usize {
        self.n as usize * self.e.bytes()
    }

    fn c_type(self) -> Option<String> {
        self.e.c().map(|_| format!("v{}_{}", self.n, self.e.ll()))
    }
}

/// Every shape the generator covers: each `Fpr` shape of a byte-sized
/// element, and `Mem` shapes (short, odd and long) of each element.
const SHAPES: &[V] = &[
    // Fpr: 64 and 128 bits.
    V::new(E::I8, 8),
    V::new(E::I8, 16),
    V::new(E::I16, 4),
    V::new(E::I16, 8),
    V::new(E::I32, 2),
    V::new(E::I32, 4),
    V::new(E::I64, 1),
    V::new(E::I64, 2),
    V::new(E::Ptr, 1),
    V::new(E::Ptr, 2),
    V::new(E::Half, 4),
    V::new(E::Half, 8),
    V::new(E::BFloat, 4),
    V::new(E::BFloat, 8),
    V::new(E::Float, 2),
    V::new(E::Float, 4),
    V::new(E::Double, 1),
    V::new(E::Double, 2),
    // Mem: shorter than 64 bits, not a power of two, longer than 128.
    V::new(E::I8, 1),
    V::new(E::I8, 2),
    V::new(E::I8, 3),
    V::new(E::I8, 4),
    V::new(E::I8, 7),
    V::new(E::I8, 32),
    V::new(E::I8, 64),
    V::new(E::I16, 1),
    V::new(E::I16, 2),
    V::new(E::I16, 3),
    V::new(E::I16, 16),
    V::new(E::I32, 1),
    V::new(E::I32, 3),
    V::new(E::I32, 5),
    V::new(E::I32, 8),
    V::new(E::I64, 3),
    V::new(E::I64, 4),
    V::new(E::Ptr, 3),
    V::new(E::Half, 3),
    V::new(E::Half, 2),
    V::new(E::BFloat, 3),
    V::new(E::Float, 1),
    V::new(E::Float, 3),
    V::new(E::Float, 8),
    V::new(E::Double, 3),
];

// ---- operations ----

/// The second operand of a shuffle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Second {
    B,
    Same,
    Poison,
    Zero,
}

#[derive(Clone, Debug)]
enum Op {
    /// `extractelement` with a constant index of `ix` bits.
    ExtractConst {
        v: V,
        ix: u32,
        k: u64,
    },
    /// `extractelement` with `%i` truncated to `ix` bits.
    ExtractVar {
        v: V,
        ix: u32,
    },
    InsertConst {
        v: V,
        ix: u32,
        k: u64,
    },
    InsertVar {
        v: V,
        ix: u32,
    },
    Shuffle {
        v: V,
        second: Second,
        mask: Vec<Option<u32>>,
    },
    /// The same, with a `zeroinitializer` mask of `m` lanes.
    ShuffleZeroMask {
        v: V,
        m: u32,
    },
    Select {
        v: V,
    },
    Phi {
        v: V,
    },
    Freeze {
        v: V,
    },
    /// The vector into and out of `{ i8, V, i16 }`.
    Struct {
        v: V,
    },
    /// The vector into and out of `[2 x V]`.
    Array {
        v: V,
    },
    Constant {
        v: V,
        lanes: Vec<Option<u64>>,
    },
    Bitcast {
        from: V,
        to: V,
    },
    /// `insertelement (shufflevector a, b, mask), (extractelement b, %i), k`.
    Chain {
        v: V,
        mask: Vec<Option<u32>>,
        k: u32,
    },
}

impl Op {
    /// Bytes the function stores at `out`.
    fn store(&self) -> usize {
        match self {
            Op::ExtractConst { v, .. } | Op::ExtractVar { v, .. } => v.e.bytes(),
            Op::Shuffle { v, mask, .. } => mask.len() * v.e.bytes(),
            Op::ShuffleZeroMask { v, m } => *m as usize * v.e.bytes(),
            Op::Bitcast { to, .. } => to.store(),
            Op::InsertConst { v, .. }
            | Op::InsertVar { v, .. }
            | Op::Select { v }
            | Op::Phi { v }
            | Op::Freeze { v }
            | Op::Struct { v }
            | Op::Array { v }
            | Op::Constant { v, .. }
            | Op::Chain { v, .. } => v.store(),
        }
    }

    /// Whether the cases should range the variable index over its edges.
    fn var(&self) -> Option<(V, u32)> {
        match self {
            Op::ExtractVar { v, ix } | Op::InsertVar { v, ix } => Some((*v, *ix)),
            Op::Chain { v, .. } => Some((*v, 64)),
            _ => None,
        }
    }
}

/// `%name = trunc i64 %i to i<ix>` (nothing for 64 bits) and the index
/// operand.
fn index_var(ix: u32) -> (String, String) {
    if ix == 64 {
        (String::new(), "i64 %i".into())
    } else {
        (format!("  %ix = trunc i64 %i to i{ix}\n"), format!("i{ix} %ix"))
    }
}

fn mask_ll(mask: &[Option<u32>]) -> String {
    let entries: Vec<String> = mask
        .iter()
        .map(|e| e.map_or_else(|| "i32 poison".to_string(), |e| format!("i32 {e}")))
        .collect();
    format!("<{} x i32> <{}>", mask.len(), entries.join(", "))
}

/// The `.ll` definition of `op` as `@name`.
fn ll(op: &Op, name: &str) -> String {
    let mut s = format!("define void @{name}(ptr %out, ptr %a, ptr %b, i64 %i) {{\nentry:\n");
    let load = |s: &mut String, v: V| {
        let t = v.ll();
        writeln!(s, "  %va = load {t}, ptr %a\n  %vb = load {t}, ptr %b").unwrap();
    };
    match op {
        Op::ExtractConst { v, ix, k } => {
            load(&mut s, *v);
            let (t, e) = (v.ll(), v.e.ll());
            writeln!(s, "  %e = extractelement {t} %va, i{ix} {k}\n  store {e} %e, ptr %out")
                .unwrap();
        }
        Op::ExtractVar { v, ix } => {
            load(&mut s, *v);
            let (pre, idx) = index_var(*ix);
            let (t, e) = (v.ll(), v.e.ll());
            write!(s, "{pre}  %e = extractelement {t} %va, {idx}\n  store {e} %e, ptr %out\n")
                .unwrap();
        }
        Op::InsertConst { v, ix, k } => {
            load(&mut s, *v);
            let (t, e) = (v.ll(), v.e.ll());
            writeln!(
                s,
                "  %e = load {e}, ptr %b\n  %r = insertelement {t} %va, {e} %e, i{ix} {k}\n  \
                 store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::InsertVar { v, ix } => {
            load(&mut s, *v);
            let (pre, idx) = index_var(*ix);
            let (t, e) = (v.ll(), v.e.ll());
            write!(
                s,
                "{pre}  %e = load {e}, ptr %b\n  %r = insertelement {t} %va, {e} %e, {idx}\n  \
                 store {t} %r, ptr %out\n"
            )
            .unwrap();
        }
        Op::Shuffle { v, second, mask } => {
            load(&mut s, *v);
            let t = v.ll();
            let other = match second {
                Second::B => "%vb",
                Second::Same => "%va",
                Second::Poison => "poison",
                Second::Zero => "zeroinitializer",
            };
            let out = V::new(v.e, mask.len() as u32).ll();
            writeln!(
                s,
                "  %r = shufflevector {t} %va, {t} {other}, {}\n  store {out} %r, ptr %out",
                mask_ll(mask)
            )
            .unwrap();
        }
        Op::ShuffleZeroMask { v, m } => {
            load(&mut s, *v);
            let t = v.ll();
            let out = V::new(v.e, *m).ll();
            writeln!(
                s,
                "  %r = shufflevector {t} %va, {t} %vb, <{m} x i32> zeroinitializer\n  \
                 store {out} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Select { v } => {
            load(&mut s, *v);
            let t = v.ll();
            writeln!(
                s,
                "  %c = trunc i64 %i to i1\n  %r = select i1 %c, {t} %va, {t} %vb\n  \
                 store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Phi { v } => {
            let t = v.ll();
            writeln!(
                s,
                "  %c = trunc i64 %i to i1\n  br i1 %c, label %one, label %two\n\
                 one:\n  %x = load {t}, ptr %a\n  br label %join\n\
                 two:\n  %y = load {t}, ptr %b\n  br label %join\n\
                 join:\n  %r = phi {t} [ %x, %one ], [ %y, %two ]\n  store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Freeze { v } => {
            load(&mut s, *v);
            let t = v.ll();
            writeln!(s, "  %r = freeze {t} %va\n  store {t} %r, ptr %out").unwrap();
        }
        Op::Struct { v } => {
            load(&mut s, *v);
            let t = v.ll();
            let st = format!("{{ i8, {t}, i16 }}");
            writeln!(
                s,
                "  %s0 = insertvalue {st} poison, {t} %vb, 1\n  \
                 %s1 = insertvalue {st} %s0, i8 7, 0\n  \
                 %s2 = insertvalue {st} %s1, {t} %va, 1\n  \
                 %r = extractvalue {st} %s2, 1\n  store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Array { v } => {
            load(&mut s, *v);
            let t = v.ll();
            let at = format!("[2 x {t}]");
            writeln!(
                s,
                "  %s0 = insertvalue {at} poison, {t} %va, 1\n  \
                 %s1 = insertvalue {at} %s0, {t} %vb, 0\n  \
                 %r = extractvalue {at} %s1, 1\n  store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Constant { v, lanes } => {
            let t = v.ll();
            let e = v.e.ll();
            let items: Vec<String> = lanes
                .iter()
                .map(|l| l.map_or_else(|| format!("{e} poison"), |b| format!("{e} {}", v.e.lit(b))))
                .collect();
            writeln!(s, "  store {t} <{}>, ptr %out", items.join(", ")).unwrap();
        }
        Op::Bitcast { from, to } => {
            let (f, t) = (from.ll(), to.ll());
            writeln!(
                s,
                "  %va = load {f}, ptr %a\n  %r = bitcast {f} %va to {t}\n  store {t} %r, ptr %out"
            )
            .unwrap();
        }
        Op::Chain { v, mask, k } => {
            load(&mut s, *v);
            let (t, e) = (v.ll(), v.e.ll());
            writeln!(
                s,
                "  %s = shufflevector {t} %va, {t} %vb, {}\n  \
                 %e = extractelement {t} %vb, i64 %i\n  \
                 %r = insertelement {t} %s, {e} %e, i32 {k}\n  store {t} %r, ptr %out",
                mask_ll(mask)
            )
            .unwrap();
        }
    }
    s.push_str("  ret void\n}\n\n");
    s
}

/// The C definition of `op` as `name` for the clang leg, if C can say
/// it; else a function that stores nothing (and is not compared).
fn c_fn(op: &Op, name: &str) -> (String, bool) {
    let head =
        format!("void {name}(void *o, const void *a, const void *b, unsigned long long i) {{\n");
    let body = c_body(op);
    let compared = body.is_some();
    (format!("{head}{}}}\n", body.unwrap_or_default()), compared)
}

fn c_body(op: &Op) -> Option<String> {
    let loads = |v: V| {
        let t = v.c_type()?;
        let st = v.store();
        Some(format!("  {t} va, vb;\n  memcpy(&va, a, {st});\n  memcpy(&vb, b, {st});\n"))
    };
    let shuffle_idx = |mask: &[Option<u32>], n: u32, poison_second: bool| {
        mask.iter()
            .map(|e| match e {
                Some(e) if !(poison_second && *e >= n) => format!(", {e}"),
                _ => ", -1".into(),
            })
            .collect::<String>()
    };
    let wmask = |ix: u32| if ix == 64 { u64::MAX } else { (1u64 << ix) - 1 };
    Some(match op {
        Op::ExtractConst { v, k, .. } => {
            let mut s = loads(*v)?;
            if *k < u64::from(v.n) {
                writeln!(s, "  {} e = va[{k}];\n  memcpy(o, &e, sizeof e);", v.e.c()?).unwrap();
            }
            s
        }
        Op::ExtractVar { v, ix } => {
            let mut s = loads(*v)?;
            writeln!(
                s,
                "  unsigned long long k = i & {}ULL;\n  if (k >= {}) return;\n  \
                 {} e = va[k];\n  memcpy(o, &e, sizeof e);",
                wmask(*ix),
                v.n,
                v.e.c()?
            )
            .unwrap();
            s
        }
        Op::InsertConst { v, k, .. } => {
            let mut s = loads(*v)?;
            if *k < u64::from(v.n) {
                writeln!(
                    s,
                    "  {} e;\n  memcpy(&e, b, sizeof e);\n  va[{k}] = e;\n  memcpy(o, &va, {});",
                    v.e.c()?,
                    v.store()
                )
                .unwrap();
            }
            s
        }
        Op::InsertVar { v, ix } => {
            let mut s = loads(*v)?;
            writeln!(
                s,
                "  unsigned long long k = i & {}ULL;\n  if (k >= {}) return;\n  \
                 {} e;\n  memcpy(&e, b, sizeof e);\n  va[k] = e;\n  memcpy(o, &va, {});",
                wmask(*ix),
                v.n,
                v.e.c()?,
                v.store()
            )
            .unwrap();
            s
        }
        Op::Shuffle { v, second, mask } => {
            let mut s = loads(*v)?;
            let t = v.c_type()?;
            let other = match second {
                Second::B => "vb",
                Second::Same | Second::Poison => "va",
                Second::Zero => {
                    writeln!(s, "  {t} z = {{0}};").unwrap();
                    "z"
                }
            };
            let idx = shuffle_idx(mask, v.n, *second == Second::Poison);
            writeln!(
                s,
                "  __auto_type r = __builtin_shufflevector(va, {other}{idx});\n  \
                 memcpy(o, &r, {});",
                mask.len() * v.e.bytes()
            )
            .unwrap();
            s
        }
        Op::Chain { v, mask, k } => {
            let mut s = loads(*v)?;
            let idx = shuffle_idx(mask, v.n, false);
            writeln!(
                s,
                "  __auto_type r = __builtin_shufflevector(va, vb{idx});\n  \
                 if (i < {}) r[{k}] = vb[i];\n  memcpy(o, &r, {});",
                v.n,
                v.store()
            )
            .unwrap();
            s
        }
        _ => return None,
    })
}

// ---- the model ----

fn lane(img: &[u8], e: E, j: usize) -> &[u8] {
    &img[j * e.bytes()..(j + 1) * e.bytes()]
}

/// What `op` stores for inputs `a`, `b`, `i`: the bytes and which of
/// them are defined.
fn model(op: &Op, a: &[u8], b: &[u8], i: u64) -> (Vec<u8>, Vec<bool>) {
    let n = op.store();
    let mut out = vec![0u8; n];
    let mut def = vec![true; n];
    let undefine = |def: &mut Vec<bool>, from: usize, len: usize| {
        def[from..from + len].iter_mut().for_each(|d| *d = false);
    };
    let wrap = |ix: u32, i: u64| if ix == 64 { i } else { i & ((1u64 << ix) - 1) };
    let shuffle =
        |v: V, second: Second, mask: &[Option<u32>], out: &mut [u8], def: &mut Vec<bool>| {
            let w = v.e.bytes();
            for (k, entry) in mask.iter().enumerate() {
                let src: Option<&[u8]> = match entry {
                    None => None,
                    Some(e) if *e < v.n => Some(lane(a, v.e, *e as usize)),
                    Some(e) => {
                        let j = (*e - v.n) as usize;
                        match second {
                            Second::B => Some(lane(b, v.e, j)),
                            Second::Same => Some(lane(a, v.e, j)),
                            Second::Poison => None,
                            Second::Zero => Some(&[0u8; 8][..w]),
                        }
                    }
                };
                match src {
                    Some(bytes) => out[k * w..(k + 1) * w].copy_from_slice(bytes),
                    None => undefine(def, k * w, w),
                }
            }
        };
    match op {
        Op::ExtractConst { v, ix, k } => {
            let k = wrap(*ix, *k);
            if k < u64::from(v.n) {
                out.copy_from_slice(lane(a, v.e, k as usize));
            } else {
                undefine(&mut def, 0, n);
            }
        }
        Op::ExtractVar { v, ix } => {
            let k = wrap(*ix, i);
            if k < u64::from(v.n) {
                out.copy_from_slice(lane(a, v.e, k as usize));
            } else {
                undefine(&mut def, 0, n);
            }
        }
        Op::InsertConst { v, ix, .. } | Op::InsertVar { v, ix } => {
            let k = match op {
                Op::InsertConst { k, .. } => wrap(*ix, *k),
                _ => wrap(*ix, i),
            };
            out.copy_from_slice(&a[..n]);
            if k < u64::from(v.n) {
                let w = v.e.bytes();
                let k = k as usize;
                out[k * w..(k + 1) * w].copy_from_slice(lane(b, v.e, 0));
            } else {
                undefine(&mut def, 0, n);
            }
        }
        Op::Shuffle { v, second, mask } => shuffle(*v, *second, mask, &mut out, &mut def),
        Op::ShuffleZeroMask { v, m } => {
            let mask = vec![Some(0); *m as usize];
            shuffle(*v, Second::B, &mask, &mut out, &mut def);
        }
        Op::Select { .. } | Op::Phi { .. } => {
            out.copy_from_slice(if i & 1 == 1 { &a[..n] } else { &b[..n] });
        }
        Op::Freeze { .. } | Op::Struct { .. } | Op::Array { .. } | Op::Bitcast { .. } => {
            out.copy_from_slice(&a[..n]);
        }
        Op::Constant { v, lanes } => {
            let w = v.e.bytes();
            for (k, l) in lanes.iter().enumerate() {
                match l {
                    Some(bits) => out[k * w..(k + 1) * w].copy_from_slice(&bits.to_le_bytes()[..w]),
                    None => undefine(&mut def, k * w, w),
                }
            }
        }
        Op::Chain { v, mask, k } => {
            shuffle(*v, Second::B, mask, &mut out, &mut def);
            let w = v.e.bytes();
            let at = *k as usize * w;
            if i < u64::from(v.n) {
                out[at..at + w].copy_from_slice(lane(b, v.e, i as usize));
                def[at..at + w].iter_mut().for_each(|d| *d = true);
            } else {
                undefine(&mut def, at, w);
            }
        }
    }
    (out, def)
}

// ---- the generator ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn bytes(&mut self) -> Vec<u8> {
        (0..IN).map(|_| self.next() as u8).collect()
    }
}

struct Case {
    func: usize,
    i: u64,
    a: Vec<u8>,
    b: Vec<u8>,
}

#[derive(Default)]
struct Suite {
    ops: Vec<Op>,
    cases: Vec<Case>,
}

/// A random mask of `m` entries over `2 * n` source lanes, about one in
/// five undefined.
fn random_mask(rng: &mut Rng, n: u32, m: u32) -> Vec<Option<u32>> {
    (0..m).map(|_| (rng.below(5) != 0).then(|| rng.below(u64::from(2 * n)) as u32)).collect()
}

/// The shuffles of `v`: identity, reverse, splats of the first and last
/// lane, interleave, concatenation, halves, all-undefined, and random
/// masks widening and narrowing (into `Fpr` and `Mem` shapes alike).
fn shuffles(rng: &mut Rng, v: V) -> Vec<Op> {
    let n = v.n;
    let mut out = Vec::new();
    let mut add = |second: Second, mask: Vec<Option<u32>>| {
        if !mask.is_empty() && mask.len() * v.e.bytes() <= 128 && mask.len() <= 256 {
            out.push(Op::Shuffle { v, second, mask });
        }
    };
    add(Second::Poison, (0..n).map(Some).collect());
    add(Second::B, (0..n).rev().map(Some).collect());
    add(Second::Poison, vec![Some(0); n as usize]);
    add(Second::B, vec![Some(2 * n - 1); n as usize]);
    add(Second::B, (0..n).map(|k| Some(k / 2 + (k % 2) * n)).collect());
    add(Second::B, (0..2 * n).map(Some).collect());
    add(Second::Same, (n / 2..n).chain(0..n / 2).map(Some).collect());
    add(Second::B, vec![None; n as usize]);
    add(Second::Zero, (0..n).map(|k| Some(if k % 2 == 0 { k } else { n + k })).collect());
    for second in [Second::B, Second::Poison, Second::Same, Second::Zero] {
        let m = 1 + rng.below(u64::from(2 * n)) as u32;
        add(second, random_mask(rng, n, m));
    }
    // Into the other class: 64 and 128 bits from anything, and an odd count.
    for bits in [64, 128] {
        let m = bits / (8 * v.e.bytes() as u32);
        add(Second::B, random_mask(rng, n, m));
    }
    add(Second::B, random_mask(rng, n, 3));
    if n >= 2 {
        add(Second::Poison, (0..n - 1).map(Some).collect());
    }
    out.push(Op::ShuffleZeroMask { v, m: n });
    out
}

/// The bitcasts between shapes of one size (and to and from the integer
/// and scalar of that size).
const BITCASTS: &[(V, V)] = &[
    (V::new(E::I32, 4), V::new(E::I8, 16)),
    (V::new(E::I8, 16), V::new(E::Double, 2)),
    (V::new(E::Float, 2), V::new(E::I64, 1)),
    (V::new(E::I16, 4), V::new(E::Half, 4)),
    (V::new(E::I32, 3), V::new(E::I8, 12)),
    (V::new(E::I16, 6), V::new(E::I32, 3)),
    (V::new(E::I8, 4), V::new(E::I16, 2)),
    (V::new(E::I64, 4), V::new(E::I32, 8)),
    (V::new(E::I8, 32), V::new(E::Float, 8)),
    (V::new(E::Half, 2), V::new(E::I32, 1)),
    (V::new(E::I8, 8), V::new(E::BFloat, 4)),
];

fn suite(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for &v in SHAPES {
        let n = u64::from(v.n);
        let mut ops = Vec::new();
        for (j, k) in [0, n - 1, n, 1 << 40].into_iter().enumerate() {
            let ix = [64, 32, 8, 64][j];
            let k = if ix == 8 { k.min(127) } else { k };
            ops.push(Op::ExtractConst { v, ix, k });
            ops.push(Op::InsertConst { v, ix: [32, 64, 64, 64][j], k });
        }
        ops.push(Op::ExtractConst { v, ix: 64, k: u64::MAX });
        for ix in [64, 32, 16, 8, 1] {
            ops.push(Op::ExtractVar { v, ix });
            ops.push(Op::InsertVar { v, ix });
        }
        ops.extend(shuffles(rng, v));
        ops.push(Op::Select { v });
        ops.push(Op::Phi { v });
        ops.push(Op::Freeze { v });
        ops.push(Op::Struct { v });
        ops.push(Op::Array { v });
        let lanes = (0..v.n).map(|_| (rng.below(4) != 0).then(|| v.e.tame(rng.next()))).collect();
        ops.push(Op::Constant { v, lanes });
        ops.push(Op::Constant { v, lanes: vec![Some(0); v.n as usize] });
        ops.push(Op::Chain { v, mask: random_mask(rng, v.n, v.n), k: rng.below(n) as u32 });
        s.ops.extend(ops);
    }
    for &(from, to) in BITCASTS {
        s.ops.push(Op::Bitcast { from, to });
        s.ops.push(Op::Bitcast { from: to, to: from });
    }
    for func in 0..s.ops.len() {
        let indices: Vec<u64> = match s.ops[func].var() {
            Some((v, ix)) => {
                let n = u64::from(v.n);
                let edges = [0, n - 1, rng.below(n), n, (1u64 << ix.min(40)) - 1, u64::MAX];
                edges
                    .into_iter()
                    .map(|k| {
                        let k = if ix == 64 { k } else { k & ((1u64 << ix) - 1) };
                        // Garbage above the index's width.
                        if ix == 64 { k } else { (rng.next() << ix) | k }
                    })
                    .collect()
            }
            None => vec![rng.next(), rng.next()],
        };
        for i in indices {
            s.cases.push(Case { func, i, a: rng.bytes(), b: rng.bytes() });
        }
    }
    s
}

fn module_text(suite: &Suite, header: &str) -> String {
    let mut text = header.to_string();
    for (k, op) in suite.ops.iter().enumerate() {
        text.push_str(&ll(op, &format!("f{k}")));
    }
    text
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn driver(suite: &Suite) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n\
         typedef void fn(void *, const void *, const void *, unsigned long long);\n",
    );
    for k in 0..suite.ops.len() {
        writeln!(c, "extern fn f{k};").unwrap();
    }
    c.push_str("static fn *const funcs[] = {");
    for k in 0..suite.ops.len() {
        write!(c, "f{k},").unwrap();
    }
    c.push_str("};\nstruct case_ { unsigned f; unsigned long long i; const char *a, *b; };\n");
    c.push_str("static const struct case_ cases[] = {\n");
    for case in &suite.cases {
        let esc = |img: &[u8]| img.iter().map(|b| format!("\\x{b:02x}")).collect::<String>();
        writeln!(
            c,
            "{{{}, {:#x}ULL, \"{}\", \"{}\"}},",
            case.func,
            case.i,
            esc(&case.a),
            esc(&case.b)
        )
        .unwrap();
    }
    write!(
        c,
        "}};\nint main(void) {{\n  \
         static unsigned char a[{IN}] __attribute__((aligned(64)));\n  \
         static unsigned char b[{IN}] __attribute__((aligned(64)));\n  \
         static unsigned char out[{OUT}] __attribute__((aligned(64)));\n  \
         for (unsigned c = 0; c < sizeof cases / sizeof cases[0]; c++) {{\n    \
         memcpy(a, cases[c].a, {IN});\n    memcpy(b, cases[c].b, {IN});\n    \
         memset(out, {SENTINEL}, {OUT});\n    \
         funcs[cases[c].f](out + {AT}, a, b, cases[c].i);\n    \
         for (int k = 0; k < {OUT}; k++) printf(\"%02x\", out[k]);\n    \
         putchar('\\n');\n  }}\n  return 0;\n}}\n"
    )
    .unwrap();
    c
}

fn c_module(suite: &Suite) -> (String, Vec<bool>) {
    let mut c = String::from("#include <string.h>\n");
    let mut types: Vec<V> = Vec::new();
    for op in &suite.ops {
        let v = match op {
            Op::ExtractConst { v, .. }
            | Op::ExtractVar { v, .. }
            | Op::InsertConst { v, .. }
            | Op::InsertVar { v, .. }
            | Op::Shuffle { v, .. }
            | Op::Chain { v, .. } => *v,
            _ => continue,
        };
        if !types.contains(&v) {
            types.push(v);
        }
    }
    for v in types {
        if let (Some(t), Some(e)) = (v.c_type(), v.e.c()) {
            writeln!(c, "typedef {e} {t} __attribute__((ext_vector_type({})));", v.n).unwrap();
        }
    }
    let mut compared = Vec::new();
    for (k, op) in suite.ops.iter().enumerate() {
        let (text, yes) = c_fn(op, &format!("f{k}"));
        c.push_str(&text);
        compared.push(yes);
    }
    (c, compared)
}

fn parse_line(line: &str) -> Vec<u8> {
    (0..line.len() / 2).map(|i| u8::from_str_radix(&line[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// What a run found.
#[derive(Default)]
struct Report {
    legs: Vec<String>,
    compared: usize,
    wrong: Vec<String>,
}

fn check_leg(
    suite: &Suite,
    leg: &str,
    stdout: &str,
    compared: Option<&[bool]>,
    report: &mut Report,
) {
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), suite.cases.len(), "{leg}: one line per case");
    for (case, line) in suite.cases.iter().zip(lines) {
        if compared.is_some_and(|c| !c[case.func]) {
            continue;
        }
        let op = &suite.ops[case.func];
        let got = parse_line(line);
        let (want, def) = model(op, &case.a, &case.b, case.i);
        let n = want.len();
        let sentinels = got[..AT].iter().chain(&got[AT + n..]).all(|&b| b == SENTINEL);
        let bytes = (0..n).all(|k| !def[k] || got[AT + k] == want[k]);
        report.compared += 1;
        if !sentinels || !bytes {
            report.wrong.push(format!(
                "{leg}: f{} i={:#x} a={} b={}\n  {op:?}\n  want {} (defined {})\n  got  {}{}",
                case.func,
                case.i,
                hex(&case.a),
                hex(&case.b),
                hex(&want),
                def.iter().map(|&d| if d { '1' } else { '0' }).collect::<String>(),
                hex(&got[AT..AT + n]),
                if sentinels { "" } else { " (sentinels overwritten)" },
            ));
        }
    }
}

fn clang_object(scratch: &Scratch, name: &str, source: &str) -> Option<PathBuf> {
    let clang = tool("clang")?;
    let input = scratch.write(&format!("{name}.c"), source);
    let output = scratch.join(format!("{name}.o"));
    let mut command = Command::new(clang);
    command.args(["-c", "-O1", "-w"]).arg(&input).arg("-o").arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "clang {name}.c:\n{}", outcome.stderr_text());
    Some(output)
}

fn run(name: &str, suite: &Suite) -> Report {
    let major = llc_major();
    let text = module_text(suite, &ll_header(major));
    if let Ok(dir) = std::env::var("HOD_VECTOR_DUMP") {
        std::fs::write(format!("{dir}/{name}.ll"), &text).unwrap();
    }
    let (module, bodies) = parse(&text);
    let scratch = Scratch::new(&format!("a64-vector-{name}"));
    let mut legs: Vec<(String, PathBuf, Option<Vec<bool>>)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        legs.push((
            format!("hod-{mode}"),
            write_object(&scratch, &format!("hod-{mode}.o"), &emitted),
            None,
        ));
    }
    if let Some(object) = llc(&scratch, "llc", &text, &[]) {
        legs.push(("llc".into(), object, None));
    }
    let (c, compared) = c_module(suite);
    if let Some(object) = clang_object(&scratch, "cvec", &c) {
        legs.push(("clang".into(), object, Some(compared)));
    }
    let mut report = Report::default();
    let Some(toolchain) = Toolchain::host(&scratch) else { return report };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite), &[]);
    for (leg, object, compared) in &legs {
        // hod's plain object under every linker; the others under one.
        let linkers =
            if leg == "hod-plain" { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let label = format!("{leg}/{}", linker.name);
            let program = scratch.join(label.replace('/', "-"));
            toolchain.link_ok(linker, &[], &[&driver, object], &program);
            let ran = run_with(&mut Command::new(&program), &[], Duration::from_mins(2));
            assert!(ran.success(), "{label}: {:?}\n{}", ran.status, ran.stderr_text());
            check_leg(suite, &label, &ran.stdout_text(), compared.as_deref(), &mut report);
            report.legs.push(label);
        }
    }
    report
}

fn assert_agrees(name: &str, suite: &Suite, report: &Report) {
    eprintln!(
        "a64 vector {name}: {} functions, {} cases, {} comparisons over legs {:?}",
        suite.ops.len(),
        suite.cases.len(),
        report.compared,
        report.legs
    );
    if report.wrong.is_empty() {
        return;
    }
    let mut message = format!("{} disagreements; the first:\n", report.wrong.len());
    for line in report.wrong.iter().take(8) {
        writeln!(message, "{line}").unwrap();
    }
    panic!("{message}");
}

// ---- tests ----

/// Every shape × operation × index edge, against the model, `llc` and
/// clang, under bfd, lld and mold.
#[test]
fn every_shape_operation_and_index_agrees() {
    let suite = suite(&mut Rng(0x0790_0790_0790_0790));
    let report = run("all", &suite);
    assert_agrees("all", &suite, &report);
}

/// The model is the C semantics of `ext_vector_type` where C has one;
/// a sanity check of the generator itself (no tools needed).
#[test]
fn the_generator_covers_its_edges() {
    let s = suite(&mut Rng(0x0790_0790_0790_0790));
    for &v in SHAPES {
        let has = |f: &dyn Fn(&Op) -> bool| s.ops.iter().any(f);
        assert!(has(
            &|op| matches!(op, Op::ExtractConst { v: w, k, .. } if *w == v && *k >= u64::from(v.n))
        ));
        assert!(has(&|op| matches!(op, Op::InsertVar { v: w, ix: 8 } if *w == v)));
        assert!(has(
            &|op| matches!(op, Op::Shuffle { v: w, mask, .. } if *w == v && mask.len() > v.n as usize)
        ));
    }
    // Every case of a variable index ranges over lane 0, the last lane,
    // N and an index out of range.
    let (text, _) = c_module(&s);
    assert!(text.contains("__builtin_shufflevector"));
    let (m, bodies) = parse(&module_text(&s, &ll_header(22)));
    assert_eq!(bodies.len(), s.ops.len());
    drop(m);
}

/// The calling convention of 64- and 128-bit vectors (class `Fpr`),
/// both ways between hod and gcc's `vector_size` vectors: in `v0`..`v7`,
/// then on the stack, and returned in `v0`.
#[test]
fn register_vectors_cross_calls_both_ways() {
    let functions = "\
declare <4 x i32> @c_mix(<4 x i32>, <2 x double>, <8 x i8>, <4 x half>, i32)
declare <2 x float> @c_many(<2 x float>, <2 x float>, <2 x float>, <2 x float>, <2 x float>, <2 x float>, <2 x float>, <2 x float>, <2 x float>, <4 x i32>)

define <4 x i32> @h_mix(<4 x i32> %a, <2 x double> %b, <8 x i8> %c, <4 x half> %d, i32 %k) {
  %r = call <4 x i32> @c_mix(<4 x i32> %a, <2 x double> %b, <8 x i8> %c, <4 x half> %d, i32 %k)
  %e = extractelement <8 x i8> %c, i32 %k
  %z = sext i8 %e to i32
  %r2 = insertelement <4 x i32> %r, i32 %z, i32 3
  ret <4 x i32> %r2
}

define <2 x float> @h_many(<2 x float> %v0, <2 x float> %v1, <2 x float> %v2, <2 x float> %v3, <2 x float> %v4, <2 x float> %v5, <2 x float> %v6, <2 x float> %v7, <2 x float> %v8, <4 x i32> %v9) {
  %x = shufflevector <2 x float> %v0, <2 x float> %v8, <2 x i32> <i32 3, i32 0>
  %y = shufflevector <2 x float> %v7, <2 x float> %x, <2 x i32> <i32 1, i32 2>
  %w = extractelement <4 x i32> %v9, i64 2
  %f = sitofp i32 %w to float
  %r = insertelement <2 x float> %y, float %f, i64 1
  ret <2 x float> %r
}

define <2 x float> @h_calls_many(<2 x float> %p, <4 x i32> %q) {
  %r = call <2 x float> @c_many(<2 x float> %p, <2 x float> zeroinitializer, <2 x float> zeroinitializer, <2 x float> zeroinitializer, <2 x float> zeroinitializer, <2 x float> zeroinitializer, <2 x float> zeroinitializer, <2 x float> <float 7.0, float 8.0>, <2 x float> %p, <4 x i32> %q)
  ret <2 x float> %r
}
";
    let driver = r#"
#include <stdio.h>
typedef int v4si __attribute__((vector_size(16)));
typedef double v2df __attribute__((vector_size(16)));
typedef signed char v8qi __attribute__((vector_size(8)));
typedef _Float16 v4hf __attribute__((vector_size(8)));
typedef float v2sf __attribute__((vector_size(8)));
v4si c_mix(v4si a, v2df b, v8qi c, v4hf d, int k) {
  v4si r = a;
  r[0] += (int)b[1];
  r[1] += c[k];
  r[2] += (int)d[3];
  return r;
}
v2sf c_many(v2sf a, v2sf b, v2sf c, v2sf d, v2sf e, v2sf f, v2sf g, v2sf h, v2sf i, v4si j) {
  v2sf r = { a[0] + h[0] + i[1], h[1] + (float)j[3] };
  return r + b + c + d + e + f + g;
}
v4si h_mix(v4si, v2df, v8qi, v4hf, int);
v2sf h_many(v2sf, v2sf, v2sf, v2sf, v2sf, v2sf, v2sf, v2sf, v2sf, v4si);
v2sf h_calls_many(v2sf, v4si);
int main(void) {
  int bad = 0;
  v4si a = {10, 20, 30, 40};
  v2df b = {1.5, -6.0};
  v8qi c = {1, -2, 3, -4, 5, -6, 7, -8};
  v4hf d = {1, 2, 3, 9};
  for (int k = 0; k < 8; k++) {
    v4si r = h_mix(a, b, c, d, k);
    v4si want = {4, 20 + c[k], 39, c[k]};
    for (int l = 0; l < 4; l++) bad |= r[l] != want[l];
  }
  v2sf z = {0, 0}, p = {1, 2}, q = {3, 4}, s = {5, 6};
  v4si j = {0, 0, -9, 77};
  v2sf m = h_many(p, z, z, z, z, z, z, q, s, j);
  bad |= m[0] != 4 || m[1] != -9;
  v2sf n = h_calls_many(p, j);
  bad |= n[0] != 1 + 7 + 2 || n[1] != 8 + 77;
  printf("%s\n", bad ? "bad" : "ok");
  return bad;
}
"#;
    let (module, bodies) = parse(&format!("{}{functions}", ll_header(22)));
    let scratch = Scratch::new("a64-vector-calls");
    let mut objects = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let emitted = compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
        objects.push(write_object(&scratch, &format!("hod-{mode}.o"), &emitted));
    }
    if let Some(object) =
        llc(&scratch, "llc", &format!("{}{functions}", ll_header(llc_major())), &[])
    {
        objects.push(object);
    }
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "driver", driver, &[]);
    for (k, object) in objects.iter().enumerate() {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("calls-{k}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            assert!(ran.success(), "{}: {}", object.display(), ran.stdout_text());
            assert_eq!(ran.stdout_text(), "ok\n");
        }
    }
}

/// Lane counts up to [`vector::MAX_LANES`] compile (one access per
/// lane, unrolled).
#[test]
fn the_longest_vectors_compile() {
    let reverse: Vec<String> = (0..256).rev().map(|k| format!("i32 {k}")).collect();
    let functions = format!(
        "define void @f(ptr %p, i64 %i) {{
  %v = load <256 x i8>, ptr %p
  %r = shufflevector <256 x i8> %v, <256 x i8> poison, <256 x i32> <{}>
  %e = extractelement <256 x i8> %r, i64 %i
  %s = insertelement <256 x i8> %r, i8 %e, i64 %i
  store <256 x i8> %s, ptr %p
  ret void
}}\n",
        reverse.join(", ")
    );
    assert_eq!(compile_ll(&functions), Ok(()));
}

/// Every shape a lane operation does not handle is refused by its name
/// (design 0003 §4.4), one construct per function.
#[test]
fn what_is_refused_is_refused_by_name() {
    let extract = |t: &str, e: &str| {
        format!(
            "define void @f(ptr %p) {{\n  %v = load {t}, ptr %p\n  \
             %e = extractelement {t} %v, i32 1\n  store {e} %e, ptr %p\n  ret void\n}}\n"
        )
    };
    let cases = [
        (extract("<65 x i1>", "i1"), vector::MASK_WIDE),
        (extract("<4 x i4>", "i4"), vector::ODD_ELEMENT),
        (extract("<2 x i24>", "i24"), vector::ODD_ELEMENT),
        (extract("<2 x fp128>", "fp128"), vector::WIDE_ELEMENT),
        (extract("<257 x i8>", "i8"), vector::LENGTH),
        (
            "define void @f(ptr %p, i128 %i) {\n  %v = load <4 x i32>, ptr %p\n  \
             %e = extractelement <4 x i32> %v, i128 %i\n  store i32 %e, ptr %p\n  ret void\n}\n"
                .into(),
            vector::INDEX,
        ),
        (
            "define void @f(ptr %p, i4 %x) {\n  %v = load <4 x i4>, ptr %p\n  \
             %r = insertelement <4 x i4> %v, i4 %x, i32 0\n  \
             store <4 x i4> %r, ptr %p\n  ret void\n}\n"
                .into(),
            vector::ODD_ELEMENT,
        ),
        // Vectors across calls pass since WP-082 (`tests/vcall.rs`).
        (
            "define void @f(<2 x ptr> %p, ptr %o) {\n  \
             %q = getelementptr i8, <2 x ptr> %p, <2 x i64> <i64 1, i64 2>\n  \
             store <2 x ptr> %q, ptr %o\n  ret void\n}\n"
                .into(),
            vector::PTRADD_VECTOR,
        ),
    ];
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(f, r)| (f.as_str(), *r)).collect();
    assert_refused(&cases);
}
