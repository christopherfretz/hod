//! The under-aligned atomics suite of both targets (WP-231, WP-232;
//! `spec/codegen.md` §5.5.1 and §5.6.10): one function per access whose
//! alignment is below its size, which `llc` and hod send to libatomic's
//! generic routines (`__atomic_load`, `__atomic_store`,
//! `__atomic_exchange`, `__atomic_compare_exchange`, and the
//! compare-exchange loop of the other `atomicrmw` operations), and the C
//! driver that runs each on a misaligned cell and works out by itself
//! what the cell and the result must be.
//!
//! Shared by `crates/hod_x64/tests/leftovers.rs` and
//! `crates/hod_a64/tests/atomic.rs`; the module's header and attributes
//! are the caller's.

use std::fmt::Write as _;

/// What a function of the suite does to its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generic {
    Load,
    Store,
    Xchg,
    Cas,
    /// `atomicrmw` of the operation named.
    Rmw(&'static str),
    /// A store of the cell's own address to it.
    SelfStore,
    /// `atomicrmw xchg` of the cell's own address.
    SelfXchg,
    /// `cmpxchg` with the cell's own address as the new value.
    SelfCas,
}

/// A function of the suite: `void g<k>(cell, in, out)`.
#[derive(Clone, Copy, Debug)]
pub struct GenericFn {
    pub op: Generic,
    pub ty: &'static str,
    /// The type's size in bytes.
    pub size: u32,
    /// The access's alignment, below `size`.
    pub align: u32,
    /// The ordering text (two orderings for a `cmpxchg`).
    pub order: &'static str,
    pub weak: bool,
}

/// How many times the driver calls each function.
pub const GENERIC_ROUNDS: usize = 4;

/// The integer `atomicrmw` operations other than `xchg`.
pub const INT_OPS: [&str; 14] = [
    "add",
    "sub",
    "and",
    "nand",
    "or",
    "xor",
    "max",
    "min",
    "umax",
    "umin",
    "uinc_wrap",
    "udec_wrap",
    "usub_cond",
    "usub_sat",
];

/// The floating-point `atomicrmw` operations.
pub const FP_OPS: [&str; 6] = ["fadd", "fsub", "fmax", "fmin", "fmaximum", "fminimum"];

/// The orderings the `atomicrmw` functions take in turn.
const RMW_ORDERS: [&str; 5] = ["monotonic", "acquire", "release", "acq_rel", "seq_cst"];

/// Whether `llc` 19 lacks `f`'s operation (added in LLVM 20).
pub fn newer_than_19(f: &GenericFn) -> bool {
    matches!(f.op, Generic::Rmw("usub_cond" | "usub_sat" | "fmaximum" | "fminimum"))
}

/// Every operation that has a generic routine, for each type at each
/// alignment below its size that is listed; `cmpxchg` for the integers
/// and the pointer only (LLVM's verifier). With `rmw`, every other
/// `atomicrmw` operation of each type too; with `self_ptr`, the accesses
/// whose value operand is the pointer itself.
pub fn generic_table(rmw: bool, self_ptr: bool) -> Vec<GenericFn> {
    let types: [(&'static str, u32, &'static [u32], bool); 8] = [
        ("i16", 2, &[1], true),
        ("i32", 4, &[1, 2], true),
        ("i64", 8, &[1, 4], true),
        ("ptr", 8, &[2], true),
        ("half", 2, &[1], false),
        ("float", 4, &[2], false),
        ("double", 8, &[1, 4], false),
        ("i128", 16, &[1, 8], true),
    ];
    let mut table = Vec::new();
    let mut turn = 0;
    for (ty, size, aligns, cas) in types {
        for &align in aligns {
            let low = align == 1;
            let f = |op, order, weak| GenericFn { op, ty, size, align, order, weak };
            table.push(f(Generic::Load, if low { "seq_cst" } else { "acquire" }, false));
            table.push(f(Generic::Store, if low { "seq_cst" } else { "release" }, false));
            table.push(f(Generic::Xchg, if low { "seq_cst" } else { "acq_rel" }, false));
            if cas {
                table.push(f(Generic::Cas, "seq_cst seq_cst", false));
                table.push(f(Generic::Cas, "acq_rel monotonic", true));
            }
            let ops: &[&'static str] = match (rmw, ty) {
                (false, _) | (true, "ptr") => &[],
                (true, "half" | "float" | "double") => &FP_OPS,
                (true, _) => &INT_OPS,
            };
            for &op in ops {
                table.push(f(Generic::Rmw(op), RMW_ORDERS[turn % RMW_ORDERS.len()], false));
                turn += 1;
            }
            if self_ptr && ty == "ptr" {
                table.push(f(Generic::SelfStore, "seq_cst", false));
                table.push(f(Generic::SelfXchg, "acq_rel", false));
                table.push(f(Generic::SelfCas, "seq_cst acquire", false));
            }
        }
    }
    table
}

/// The module: each function reads its operands from `in` (the value, or
/// the expected value and, 16 bytes on, the new one), does its one atomic
/// access to `cell` and writes what it read to `out` (the value, then at
/// 16 a `cmpxchg`'s flag). Only the functions `keep` accepts are
/// defined; `attrs` follows each signature (`""` or `" #0"`), `footer`
/// ends the module.
pub fn generic_module(
    table: &[GenericFn],
    header: &str,
    attrs: &str,
    footer: &str,
    keep: impl Fn(&GenericFn) -> bool,
) -> String {
    let mut text = header.to_owned();
    for (k, f) in table.iter().enumerate() {
        if !keep(f) {
            continue;
        }
        let GenericFn { ty, align, order, .. } = *f;
        let weak = if f.weak { "weak " } else { "" };
        let cas = |expected: &str, new: &str| {
            format!(
                "  %r = cmpxchg {weak}ptr %cell, {ty} {expected}, {ty} {new} {order}, align {align}\n  \
                 %old = extractvalue {{ {ty}, i1 }} %r, 0\n  \
                 %ok = extractvalue {{ {ty}, i1 }} %r, 1\n  \
                 store {ty} %old, ptr %out, align 1\n  \
                 %flag = zext i1 %ok to i8\n  \
                 %out.k = getelementptr i8, ptr %out, i64 16\n  \
                 store i8 %flag, ptr %out.k, align 1\n"
            )
        };
        let rmw = |op: &str, x: &str| {
            format!(
                "  %v = atomicrmw {op} ptr %cell, {ty} {x} {order}, align {align}\n  \
                 store {ty} %v, ptr %out, align 1\n"
            )
        };
        let load_x = format!("  %x = load {ty}, ptr %in, align 1\n");
        let core = match f.op {
            Generic::Load => format!(
                "  %v = load atomic {ty}, ptr %cell {order}, align {align}\n  \
                 store {ty} %v, ptr %out, align 1\n"
            ),
            Generic::Store => {
                format!("{load_x}  store atomic {ty} %x, ptr %cell {order}, align {align}\n")
            }
            Generic::Xchg => format!("{load_x}{}", rmw("xchg", "%x")),
            Generic::Rmw(op) => format!("{load_x}{}", rmw(op, "%x")),
            Generic::Cas => format!(
                "  %e = load {ty}, ptr %in, align 1\n  \
                 %in.n = getelementptr i8, ptr %in, i64 16\n  \
                 %n = load {ty}, ptr %in.n, align 1\n{}",
                cas("%e", "%n")
            ),
            Generic::SelfStore => {
                format!("  store atomic ptr %cell, ptr %cell {order}, align {align}\n")
            }
            Generic::SelfXchg => rmw("xchg", "%cell"),
            Generic::SelfCas => {
                format!("  %e = load ptr, ptr %in, align 1\n{}", cas("%e", "%cell"))
            }
        };
        write!(
            text,
            "define void @g{k}(ptr %cell, ptr %in, ptr %out){attrs} {{\nentry:\n{core}  ret void\n}}\n\n"
        )
        .unwrap();
    }
    text.push_str(footer);
    text
}

/// The driver's code for `f`'s operation.
fn op_code(f: &GenericFn) -> String {
    match f.op {
        Generic::Load => "LOAD".into(),
        Generic::Store => "STORE".into(),
        Generic::Xchg => "XCHG".into(),
        Generic::Cas => "CAS".into(),
        Generic::SelfStore => "SELF_STORE".into(),
        Generic::SelfXchg => "SELF_XCHG".into(),
        Generic::SelfCas => "SELF_CAS".into(),
        Generic::Rmw(op) => format!("R_{}", op.to_uppercase()),
    }
}

/// The C reference of the `atomicrmw` operations: integers on `unsigned
/// __int128` (zero-extended reads, masked to the size; the signed
/// compares on the sign-extended values), floating point in its own type
/// on inputs that are finite, not zero and not NaN (so `maxnum` and
/// `maximum` agree, and every result is exact).
const REFERENCE: &str = r"
typedef unsigned __int128 u128;
typedef __int128 s128;
static u128 rd(const unsigned char *p, int size) { u128 v = 0; memcpy(&v, p, size); return v; }
static s128 sx(u128 v, int size) { int sh = 128 - 8 * size; return ((s128)(v << sh)) >> sh; }
static u128 int_op(int op, u128 o, u128 v, int size) {
  u128 r;
  switch (op) {
  case R_ADD: r = o + v; break;
  case R_SUB: r = o - v; break;
  case R_AND: r = o & v; break;
  case R_NAND: r = ~(o & v); break;
  case R_OR: r = o | v; break;
  case R_XOR: r = o ^ v; break;
  case R_MAX: r = sx(o, size) > sx(v, size) ? o : v; break;
  case R_MIN: r = sx(o, size) < sx(v, size) ? o : v; break;
  case R_UMAX: r = o > v ? o : v; break;
  case R_UMIN: r = o < v ? o : v; break;
  case R_UINC_WRAP: r = o >= v ? 0 : o + 1; break;
  case R_UDEC_WRAP: r = (o == 0 || o > v) ? v : o - 1; break;
  case R_USUB_COND: r = o >= v ? o - v : o; break;
  default: r = o >= v ? o - v : 0; break;
  }
  return size == 16 ? r : r & (((u128)1 << (8 * size)) - 1);
}
#define FP_OP(T, op, a, b) \
  ((op) == R_FADD ? (T)((a) + (b)) : (op) == R_FSUB ? (T)((a) - (b)) : \
   ((op) == R_FMAX || (op) == R_FMAXIMUM) ? ((a) > (b) ? (a) : (b)) : ((a) < (b) ? (a) : (b)))
static void fp_value(unsigned char *p, int kind, unsigned seed) {
  double d = ((int)(seed % 2001u) - 1000) / 8.0 + 0.0625;
  if (kind == 1) { _Float16 h = (_Float16)d; memcpy(p, &h, 2); }
  else if (kind == 2) { float f = (float)d; memcpy(p, &f, 4); }
  else { memcpy(p, &d, 8); }
}
static void fp_op(int op, int kind, const unsigned char *o, const unsigned char *v, unsigned char *r) {
  if (kind == 1) { _Float16 a, b, c; memcpy(&a, o, 2); memcpy(&b, v, 2); c = FP_OP(_Float16, op, a, b); memcpy(r, &c, 2); }
  else if (kind == 2) { float a, b, c; memcpy(&a, o, 4); memcpy(&b, v, 4); c = FP_OP(float, op, a, b); memcpy(r, &c, 4); }
  else { double a, b, c; memcpy(&a, o, 8); memcpy(&b, v, 8); c = FP_OP(double, op, a, b); memcpy(r, &c, 8); }
}
";

/// The C driver: each function [`GENERIC_ROUNDS`] times on a cell placed
/// at its alignment past a 64-byte boundary (so it honors the access's
/// alignment and no more), every other `cmpxchg` with the expected value
/// equal to the cell's. It works out what the cell and `out` must hold by
/// copying bytes (and, for `atomicrmw`, by its reference), checks the
/// bytes around the cell, and prints `compared N wrong M`.
pub fn generic_driver(table: &[GenericFn]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n\
         typedef void (*fn)(void *, const void *, void *);\n\
         enum { LOAD, STORE, XCHG, CAS, SELF_STORE, SELF_XCHG, SELF_CAS, ",
    );
    for op in INT_OPS.iter().chain(&FP_OPS) {
        write!(c, "R_{}, ", op.to_uppercase()).unwrap();
    }
    c.push_str("};\n");
    c.push_str(REFERENCE);
    for k in 0..table.len() {
        writeln!(c, "void g{k}(void *, const void *, void *);").unwrap();
    }
    c.push_str("static const struct { fn f; int op, size, align, kind; } fs[] = {\n");
    for (k, f) in table.iter().enumerate() {
        let kind = match f.ty {
            "half" => 1,
            "float" => 2,
            "double" => 3,
            _ => 0,
        };
        writeln!(c, "  {{ g{k}, {}, {}, {}, {kind} }},", op_code(f), f.size, f.align).unwrap();
    }
    write!(
        c,
        "}};\nint main(void) {{\n  unsigned n = 0, wrong = 0;\n  \
         for (unsigned i = 0; i < sizeof fs / sizeof fs[0]; i++) {{\n    \
         for (int round = 0; round < {GENERIC_ROUNDS}; round++) {{\n      \
         _Alignas(64) unsigned char buf[64];\n      \
         unsigned char in[32], out[32], was[16], cell_want[16], out_want[32];\n      \
         int size = fs[i].size, at = fs[i].align, op = fs[i].op;\n      \
         unsigned char *cell = buf + at;\n      \
         memset(buf, 0xee, sizeof buf);\n      \
         for (int b = 0; b < size; b++) was[b] = (unsigned char)(0x11 * (i + 1) + 7 * b + 31 * round);\n      \
         for (int b = 0; b < 32; b++) in[b] = (unsigned char)(0xc3 ^ (5 * b + 13 * i + round));\n      \
         if (op >= R_FADD) {{ fp_value(was, fs[i].kind, 977u * i + 131u * round); fp_value(in, fs[i].kind, 613u * i + 89u * round + 7u); }}\n      \
         if (op == R_UDEC_WRAP && round == 1) memset(was, 0, size);\n      \
         if (op >= R_ADD && op < R_FADD && round == 2) memcpy(in, was, size);\n      \
         if (op == SELF_CAS && (round & 1)) memcpy(was, &cell, size);\n      \
         memcpy(cell, was, size);\n      \
         if ((op == CAS || op == SELF_CAS) && (round & 1)) memcpy(in, was, size);\n      \
         memset(out, 0xa5, sizeof out);\n      \
         memset(out_want, 0xa5, sizeof out_want);\n      \
         memcpy(cell_want, was, size);\n      \
         switch (op) {{\n      \
         case LOAD: memcpy(out_want, was, size); break;\n      \
         case STORE: memcpy(cell_want, in, size); break;\n      \
         case XCHG: memcpy(out_want, was, size); memcpy(cell_want, in, size); break;\n      \
         case SELF_STORE: memcpy(cell_want, &cell, size); break;\n      \
         case SELF_XCHG: memcpy(out_want, was, size); memcpy(cell_want, &cell, size); break;\n      \
         case CAS: case SELF_CAS: {{\n        \
         int same = memcmp(was, in, size) == 0;\n        \
         memcpy(out_want, was, size);\n        \
         out_want[16] = (unsigned char)same;\n        \
         if (same) {{ if (op == CAS) memcpy(cell_want, in + 16, size); else memcpy(cell_want, &cell, size); }}\n        \
         break;\n      \
         }}\n      \
         default:\n        \
         memcpy(out_want, was, size);\n        \
         if (op >= R_FADD) fp_op(op, fs[i].kind, was, in, cell_want);\n        \
         else {{ u128 r = int_op(op, rd(was, size), rd(in, size), size); memcpy(cell_want, &r, size); }}\n      \
         }}\n      \
         fs[i].f(cell, in, out);\n      \
         int bad = memcmp(cell, cell_want, size) != 0 || memcmp(out, out_want, sizeof out) != 0;\n      \
         for (int b = 0; b < 64; b++)\n        \
         if ((b < at || b >= at + size) && buf[b] != 0xee) bad = 1;\n      \
         n++;\n      \
         if (bad) {{ wrong++; printf(\"wrong: g%u round %d\\n\", i, round); }}\n    \
         }}\n  }}\n  \
         printf(\"compared %u wrong %u\\n\", n, wrong);\n  return wrong != 0;\n}}\n"
    )
    .unwrap();
    c
}
