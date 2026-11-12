//! Contended atomics on real threads (WP-074): `.ll` workers compiled by
//! hod (LL/SC and `+outline-atomics` builds), driven from C on four
//! POSIX threads, every result exact.
//!
//! - **Counters**: every `atomicrmw` operation on a shared cell, at
//!   every width it takes, four threads at once. Each thread applies `n`
//!   operations; the result does not depend on the interleaving (the
//!   operations commute, or they are all the same operation, or, for an
//!   exchange, the sum of every value seen plus the final value is the
//!   sum of the values written). The expectation is computed by the same
//!   program with the same operations under a mutex, in C built by gcc.
//!   A `cmpxchg` increment loop (strong and weak) at every width too.
//! - **Spinlocks** on strong and weak `cmpxchg` and on exchange (the
//!   `atomic_flag` pattern), guarding plain increments.
//! - **A Treiber stack**: concurrent pushes, then concurrent pops; every
//!   node comes back exactly once.
//! - **128-bit tearing**: writers store and exchange values whose halves
//!   are equal; readers' atomic loads must never see unequal halves.
//! - **Message passing**: two threads alternate through a release store
//!   and an acquire load (and through fences); the data written before
//!   each release is what the acquire side reads.

#![allow(clippy::too_many_lines, reason = "a test generator: `.ll` and C text built by hand")]

use std::fmt::Write as _;
use std::path::PathBuf;

use hod_a64::compile_module;
use hod_cg::Options;
use hod_testutil::Scratch;

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, parse, write_object};

/// Threads per program.
const THREADS: u32 = 4;
/// Operations per thread.
const N: u32 = 2000;
/// Times each linked program runs.
const RUNS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    Int(u32),
    Ptr,
    Half,
    BFloat,
    Float,
    Double,
}

impl Ty {
    fn bits(self) -> u32 {
        match self {
            Ty::Int(b) => b,
            Ty::Half | Ty::BFloat => 16,
            Ty::Float => 32,
            Ty::Ptr | Ty::Double => 64,
        }
    }

    fn ll(self) -> String {
        match self {
            Ty::Int(b) => format!("i{b}"),
            Ty::Ptr => "ptr".into(),
            Ty::Half => "half".into(),
            Ty::BFloat => "bfloat".into(),
            Ty::Float => "float".into(),
            Ty::Double => "double".into(),
        }
    }

    /// The C type of its bits.
    fn c_bits(self) -> &'static str {
        match self.bits() {
            8 => "uint8_t",
            16 => "uint16_t",
            32 => "uint32_t",
            64 => "uint64_t",
            _ => "u128",
        }
    }

    fn c_signed(self) -> &'static str {
        match self.bits() {
            8 => "int8_t",
            16 => "int16_t",
            32 => "int32_t",
            64 => "int64_t",
            _ => "i128",
        }
    }

    /// The C floating type, for the floating formats with arithmetic.
    fn c_float(self) -> &'static str {
        match self {
            Ty::Half => "_Float16",
            Ty::Float => "float",
            _ => "double",
        }
    }
}

/// The operand each operation of a thread applies (`i` the operation's
/// index, `tid` the thread's).
#[derive(Clone, Copy)]
enum Val {
    /// These bits.
    Const(u128),
    /// `1 << tid`.
    Bit,
    /// `~(1 << tid)`.
    NotBit,
    /// `tid * n + i + 1` (truncated; both halves of an `i128`).
    Ramp,
    /// `tid * n + i + 1` converted to the floating type.
    RampF,
}

struct Spec {
    ty: Ty,
    /// The `atomicrmw` operation, or `casinc` (a `cmpxchg` loop adding
    /// one; `weak` picks the weak form).
    op: &'static str,
    val: Val,
    init: u128,
    order: &'static str,
    weak: bool,
}

impl Spec {
    /// What the result is: 0 the final value, 1 also the sum of the
    /// values the operations saw (independent of the interleaving when
    /// all operations are alike), 2 only the sum of those and the final
    /// value (an exchange: the final value depends on the interleaving).
    fn sums(&self) -> u8 {
        if self.op == "xchg" {
            2
        } else {
            u8::from(matches!(self.val, Val::Const(_)))
        }
    }
}

const ORDERS: [&str; 5] = ["monotonic", "acquire", "release", "acq_rel", "seq_cst"];

fn ones(bits: u32) -> u128 {
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

fn specs() -> Vec<Spec> {
    let mut out = Vec::new();
    let mut k = 0;
    let mut push = |ty: Ty, op: &'static str, val: Val, init: u128, weak: bool| {
        out.push(Spec { ty, op, val, init, order: ORDERS[k % ORDERS.len()], weak });
        k += 1;
    };
    for bits in [8, 16, 32, 64, 128] {
        let ty = Ty::Int(bits);
        let m = ones(bits);
        let high = 1u128 << (bits - 1);
        push(ty, "xchg", Val::Ramp, 0, false);
        push(ty, "add", Val::Const(1), m - 100, false);
        push(ty, "sub", Val::Const(3), 5, false);
        push(ty, "and", Val::NotBit, m, false);
        push(ty, "or", Val::Bit, high, false);
        push(ty, "xor", Val::Bit, 0x5a & m, false);
        push(ty, "nand", Val::Const(m), 0x1234_5678 & m, false);
        push(ty, "max", Val::Ramp, high, false);
        push(ty, "min", Val::Ramp, high - 1, false);
        push(ty, "umax", Val::Ramp, 0, false);
        push(ty, "umin", Val::Ramp, m, false);
        push(ty, "uinc_wrap", Val::Const(97), 0, false);
        push(ty, "udec_wrap", Val::Const(89), 3, false);
        push(ty, "usub_cond", Val::Const(7), 3000 & m, false);
        push(ty, "usub_sat", Val::Const(5), 7000 & m, false);
        push(ty, "casinc", Val::Const(1), m - 5, false);
        push(ty, "casinc", Val::Const(1), 7, true);
    }
    for ty in [Ty::Half, Ty::Float, Ty::Double] {
        let (one, big) = match ty {
            Ty::Half => (0x3c00, 0x7800),
            Ty::Float => (0x3f80_0000, 0x4f00_0000),
            _ => (0x3ff0_0000_0000_0000, 0x41e0_0000_0000_0000),
        };
        push(ty, "xchg", Val::Ramp, 0, false);
        push(ty, "fadd", Val::Const(one), 0, false);
        push(ty, "fsub", Val::Const(one), big, false);
        push(ty, "fmax", Val::RampF, one, false);
        push(ty, "fmin", Val::RampF, big, false);
        push(ty, "fmaximum", Val::RampF, one, false);
        push(ty, "fminimum", Val::RampF, big, false);
    }
    push(Ty::BFloat, "xchg", Val::Ramp, 0, false);
    push(Ty::Ptr, "xchg", Val::Ramp, 0, false);
    out
}

/// `.ll` that computes `%v` of the spec's type in a loop body with
/// `%i`, `%tid`, `%n` (all `i64`).
fn ll_value(spec: &Spec) -> String {
    let ty = spec.ty;
    let b = ty.bits();
    let mut s = String::new();
    let int = |s: &mut String, from: &str| match ty {
        Ty::Int(128) => write!(
            s,
            "  %vz = zext i64 {from} to i128\n  %vh = shl i128 %vz, 64\n  %v = or i128 %vh, %vz\n"
        )
        .unwrap(),
        Ty::Int(64) => writeln!(s, "  %v = add i64 {from}, 0").unwrap(),
        Ty::Int(_) => writeln!(s, "  %v = trunc i64 {from} to i{b}").unwrap(),
        Ty::Ptr => writeln!(s, "  %v = inttoptr i64 {from} to ptr").unwrap(),
        Ty::Double => writeln!(s, "  %v = bitcast i64 {from} to double").unwrap(),
        _ => write!(s, "  %vt = trunc i64 {from} to i{b}\n  %v = bitcast i{b} %vt to {}\n", ty.ll())
            .unwrap(),
    };
    match spec.val {
        Val::Const(c) => match ty {
            Ty::Int(_) => writeln!(s, "  %v = add i{b} {c}, 0").unwrap(),
            Ty::Ptr => writeln!(s, "  %v = inttoptr i64 {c} to ptr").unwrap(),
            _ => writeln!(s, "  %v = bitcast i{b} {c} to {}", ty.ll()).unwrap(),
        },
        Val::Bit => {
            s.push_str("  %v64 = shl i64 1, %tid\n");
            int(&mut s, "%v64");
        }
        Val::NotBit => {
            s.push_str("  %v1 = shl i64 1, %tid\n  %v64 = xor i64 %v1, -1\n");
            int(&mut s, "%v64");
        }
        Val::Ramp | Val::RampF => {
            s.push_str("  %v0 = mul i64 %tid, %n\n  %v1 = add i64 %v0, %i\n  %v64 = add i64 %v1, 1\n");
            if matches!(spec.val, Val::RampF) {
                writeln!(s, "  %v = uitofp i64 %v64 to {}", ty.ll()).unwrap();
            } else {
                int(&mut s, "%v64");
            }
        }
    }
    s
}

/// `.ll` folding `%old` of the type into `%f` (`i64`): the bits, an
/// `i128` as `lo + 3 * hi`.
fn ll_fold(ty: Ty) -> String {
    let b = ty.bits();
    match ty {
        Ty::Int(128) => "  %flo = trunc i128 %old to i64\n  %fs = lshr i128 %old, 64\n  \
                         %fhi = trunc i128 %fs to i64\n  %f3 = mul i64 %fhi, 3\n  \
                         %f = add i64 %flo, %f3\n"
            .into(),
        Ty::Int(64) => "  %f = add i64 %old, 0\n".into(),
        Ty::Int(_) => format!("  %f = zext i{b} %old to i64\n"),
        Ty::Ptr => "  %f = ptrtoint ptr %old to i64\n".into(),
        Ty::Double => "  %f = bitcast double %old to i64\n".into(),
        _ => format!("  %fb = bitcast {} %old to i{b}\n  %f = zext i{b} %fb to i64\n", ty.ll()),
    }
}

/// The worker `@w<k>(ptr %p, i64 %n, i64 %tid) -> i64`: `n` operations
/// on the cell, returning the sum of the folded values each saw.
fn ll_worker(k: usize, spec: &Spec) -> String {
    let (t, a) = (spec.ty.ll(), spec.ty.bits() / 8);
    let mut s = format!(
        "define i64 @w{k}(ptr %p, i64 %n, i64 %tid) {{\nentry:\n  br label %loop\n\
         loop:\n  %i = phi i64 [ 0, %entry ], [ %i1, %next ]\n  \
         %acc = phi i64 [ 0, %entry ], [ %acc1, %next ]\n"
    );
    s.push_str(&ll_value(spec));
    if spec.op == "casinc" {
        let weak = if spec.weak { "weak " } else { "" };
        let failure = match spec.order {
            "acquire" | "acq_rel" => "acquire",
            "seq_cst" => "seq_cst",
            _ => "monotonic",
        };
        write!(
            s,
            "  %o0 = load atomic {t}, ptr %p monotonic, align {a}\n  br label %retry\n\
             retry:\n  %old = phi {t} [ %o0, %loop ], [ %cur, %retry ]\n  \
             %new = add {t} %old, %v\n  \
             %x = cmpxchg {weak}ptr %p, {t} %old, {t} %new {} {failure}, align {a}\n  \
             %cur = extractvalue {{ {t}, i1 }} %x, 0\n  \
             %ok = extractvalue {{ {t}, i1 }} %x, 1\n  \
             br i1 %ok, label %next, label %retry\n\
             next:\n",
            spec.order
        )
        .unwrap();
    } else {
        write!(
            s,
            "  %old = atomicrmw {} ptr %p, {t} %v {}, align {a}\n  br label %next\nnext:\n",
            spec.op, spec.order
        )
        .unwrap();
    }
    s.push_str(&ll_fold(spec.ty));
    s.push_str(
        "  %acc1 = add i64 %acc, %f\n  %i1 = add i64 %i, 1\n  %more = icmp ult i64 %i1, %n\n  \
         br i1 %more, label %loop, label %done\ndone:\n  ret i64 %acc1\n}\n\n",
    );
    s
}

/// The structural workers (spinlocks, the stack, tearing, message
/// passing).
const STRUCTURES: &str = r#"
define void @spin_strong(ptr %lock, ptr %data, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %crit ]
  br label %acquire
acquire:
  %x = cmpxchg ptr %lock, i32 0, i32 1 acquire monotonic, align 4
  %ok = extractvalue { i32, i1 } %x, 1
  br i1 %ok, label %crit, label %acquire
crit:
  %c = load i64, ptr %data, align 8
  %c1 = add i64 %c, 1
  store i64 %c1, ptr %data, align 8
  %d = getelementptr i8, ptr %data, i64 8
  %e = load i64, ptr %d, align 8
  %e1 = add i64 %e, 2
  store i64 %e1, ptr %d, align 8
  store atomic i32 0, ptr %lock release, align 4
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define void @spin_weak(ptr %lock, ptr %data, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %crit ]
  br label %acquire
acquire:
  %x = cmpxchg weak ptr %lock, i8 0, i8 1 seq_cst seq_cst, align 1
  %ok = extractvalue { i8, i1 } %x, 1
  br i1 %ok, label %crit, label %acquire
crit:
  %c = load i64, ptr %data, align 8
  %c1 = add i64 %c, 1
  store i64 %c1, ptr %data, align 8
  %d = getelementptr i8, ptr %data, i64 8
  %e = load i64, ptr %d, align 8
  %e1 = add i64 %e, 2
  store i64 %e1, ptr %d, align 8
  store atomic i8 0, ptr %lock seq_cst, align 1
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define void @spin_wide(ptr %lock, ptr %data, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %crit ]
  br label %acquire
acquire:
  %x = cmpxchg weak ptr %lock, i128 0, i128 18446744073709551617 acq_rel acquire, align 16
  %ok = extractvalue { i128, i1 } %x, 1
  br i1 %ok, label %crit, label %acquire
crit:
  %c = load i64, ptr %data, align 8
  %c1 = add i64 %c, 1
  store i64 %c1, ptr %data, align 8
  %d = getelementptr i8, ptr %data, i64 8
  %e = load i64, ptr %d, align 8
  %e1 = add i64 %e, 2
  store i64 %e1, ptr %d, align 8
  store atomic i128 0, ptr %lock release, align 16
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define void @spin_flag(ptr %lock, ptr %data, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %crit ]
  br label %acquire
acquire:
  %was = atomicrmw xchg ptr %lock, i8 1 acquire, align 1
  %free = icmp eq i8 %was, 0
  br i1 %free, label %crit, label %wait
wait:
  %seen = load atomic i8, ptr %lock monotonic, align 1
  %clear = icmp eq i8 %seen, 0
  br i1 %clear, label %acquire, label %wait
crit:
  %c = load i64, ptr %data, align 8
  %c1 = add i64 %c, 1
  store i64 %c1, ptr %data, align 8
  %d = getelementptr i8, ptr %data, i64 8
  %e = load i64, ptr %d, align 8
  %e1 = add i64 %e, 2
  store i64 %e1, ptr %d, align 8
  store atomic i8 0, ptr %lock release, align 1
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define void @push(ptr %head, ptr %node) {
entry:
  %h0 = load atomic ptr, ptr %head monotonic, align 8
  br label %retry
retry:
  %h = phi ptr [ %h0, %entry ], [ %cur, %retry ]
  store ptr %h, ptr %node, align 8
  %x = cmpxchg weak ptr %head, ptr %h, ptr %node release monotonic, align 8
  %cur = extractvalue { ptr, i1 } %x, 0
  %ok = extractvalue { ptr, i1 } %x, 1
  br i1 %ok, label %done, label %retry
done:
  ret void
}

define ptr @pop(ptr %head) {
entry:
  %h0 = load atomic ptr, ptr %head acquire, align 8
  br label %retry
retry:
  %h = phi ptr [ %h0, %entry ], [ %cur, %try ]
  %empty = icmp eq ptr %h, null
  br i1 %empty, label %none, label %try
try:
  %next = load ptr, ptr %h, align 8
  %x = cmpxchg ptr %head, ptr %h, ptr %next acq_rel acquire, align 8
  %cur = extractvalue { ptr, i1 } %x, 0
  %ok = extractvalue { ptr, i1 } %x, 1
  br i1 %ok, label %got, label %retry
got:
  ret ptr %h
none:
  ret ptr null
}

define i64 @tear_write(ptr %p, i64 %n, i64 %tid) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %bad = phi i64 [ 0, %entry ], [ %bad1, %loop ]
  %v0 = mul i64 %tid, 1000003
  %v1 = add i64 %v0, %i
  %vz = zext i64 %v1 to i128
  %vh = shl i128 %vz, 64
  %v = or i128 %vh, %vz
  store atomic i128 %v, ptr %p seq_cst, align 16
  %old = atomicrmw xchg ptr %p, i128 %v acq_rel, align 16
  %lo = trunc i128 %old to i64
  %s = lshr i128 %old, 64
  %hi = trunc i128 %s to i64
  %torn = icmp ne i64 %lo, %hi
  %t = zext i1 %torn to i64
  %bad1 = add i64 %bad, %t
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %bad1
}

define i64 @tear_read(ptr %p, i64 %n, i64 %tid) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %bad = phi i64 [ 0, %entry ], [ %bad1, %loop ]
  %old = load atomic i128, ptr %p acquire, align 16
  %lo = trunc i128 %old to i64
  %s = lshr i128 %old, 64
  %hi = trunc i128 %s to i64
  %torn = icmp ne i64 %lo, %hi
  %t = zext i1 %torn to i64
  %bad1 = add i64 %bad, %t
  %i1 = add i64 %i, 1
  %more = icmp ult i64 %i1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %bad1
}

; Message passing: %side 0 writes data 7k at its turn 2k then releases
; 2k+1; %side 1 waits for 2k+1, checks the data, releases 2k+2.
define i64 @pass(ptr %flag, ptr %data, i64 %n, i64 %side) {
entry:
  br label %loop
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %turned ]
  %bad = phi i64 [ 0, %entry ], [ %bad1, %turned ]
  %k2 = shl i64 %k, 1
  %turn = add i64 %k2, %side
  br label %wait
wait:
  %f = load atomic i64, ptr %flag acquire, align 8
  %mine = icmp eq i64 %f, %turn
  br i1 %mine, label %go, label %wait
go:
  %want = mul i64 %k, 7
  %is0 = icmp eq i64 %side, 0
  br i1 %is0, label %write, label %read
write:
  store i64 %want, ptr %data, align 8
  br label %release
read:
  %got = load i64, ptr %data, align 8
  %diff = icmp ne i64 %got, %want
  %d = zext i1 %diff to i64
  br label %release
release:
  %b = phi i64 [ 0, %write ], [ %d, %read ]
  %bad1 = add i64 %bad, %b
  %t1 = add i64 %turn, 1
  store atomic i64 %t1, ptr %flag release, align 8
  br label %turned
turned:
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %bad1
}

; The same through fences around relaxed accesses of an i16 flag.
define i64 @pass_fenced(ptr %flag, ptr %data, i64 %n, i64 %side) {
entry:
  br label %loop
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %release ]
  %bad = phi i64 [ 0, %entry ], [ %bad1, %release ]
  %k2 = shl i64 %k, 1
  %turn = add i64 %k2, %side
  %turn16 = trunc i64 %turn to i16
  br label %wait
wait:
  %f = load atomic i16, ptr %flag monotonic, align 2
  %mine = icmp eq i16 %f, %turn16
  br i1 %mine, label %go, label %wait
go:
  fence acquire
  %want = mul i64 %k, 7
  %is0 = icmp eq i64 %side, 0
  br i1 %is0, label %write, label %read
write:
  store i64 %want, ptr %data, align 8
  br label %release
read:
  %got = load i64, ptr %data, align 8
  %diff = icmp ne i64 %got, %want
  %d = zext i1 %diff to i64
  br label %release
release:
  %b = phi i64 [ 0, %write ], [ %d, %read ]
  %bad1 = add i64 %bad, %b
  %t1 = add i16 %turn16, 1
  fence release
  store atomic i16 %t1, ptr %flag monotonic, align 2
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %bad1
}
"#;

/// The workers' module.
fn module(specs: &[Spec], outline: bool) -> String {
    let mut text = ll_header(22);
    for (k, spec) in specs.iter().enumerate() {
        text.push_str(&ll_worker(k, spec));
    }
    text.push_str(STRUCTURES);
    if outline {
        text = text.replace(") {\nentry:", ") #0 {\nentry:");
        text.push_str("\nattributes #0 = { \"target-features\"=\"+outline-atomics,+v8a\" }\n");
    }
    text
}

fn c_lit(v: u128) -> String {
    format!("(((u128){:#x}ull << 64) | {:#x}ull)", (v >> 64) as u64, v as u64)
}

/// The C expression of the operation on `o` and `v` (bits).
fn c_op(spec: &Spec) -> String {
    let (u, s) = (spec.ty.c_bits(), spec.ty.c_signed());
    let fl = |e: &str| format!("f2b_{u}(({}){e})", spec.ty.c_float());
    let (a, b) = (format!("b2f_{u}(o)"), format!("b2f_{u}(v)"));
    match spec.op {
        "xchg" => "v".into(),
        "add" | "casinc" => "o + v".into(),
        "sub" => "o - v".into(),
        "and" => "o & v".into(),
        "nand" => "~(o & v)".into(),
        "or" => "o | v".into(),
        "xor" => "o ^ v".into(),
        "max" => format!("({s})o > ({s})v ? o : v"),
        "min" => format!("({s})o < ({s})v ? o : v"),
        "umax" => "o > v ? o : v".into(),
        "umin" => "o < v ? o : v".into(),
        "uinc_wrap" => format!("o >= v ? ({u})0 : ({u})(o + 1)"),
        "udec_wrap" => format!("(o == 0 || o > v) ? v : ({u})(o - 1)"),
        "usub_cond" => format!("o >= v ? ({u})(o - v) : o"),
        "usub_sat" => format!("o >= v ? ({u})(o - v) : ({u})0"),
        "fadd" => fl(&format!("{a} + {b}")),
        "fsub" => fl(&format!("{a} - {b}")),
        // The values are positive and never NaN: every flavour of
        // maximum and minimum is the comparison.
        "fmax" | "fmaximum" => fl(&format!("{a} > {b} ? {a} : {b}")),
        "fmin" | "fminimum" => fl(&format!("{a} < {b} ? {a} : {b}")),
        other => unreachable!("{other}"),
    }
}

fn c_value(spec: &Spec) -> String {
    let u = spec.ty.c_bits();
    let v64 = match spec.val {
        Val::Const(c) => return format!("({u}){}", c_lit(c)),
        Val::Bit => "((uint64_t)1 << tid)".to_string(),
        Val::NotBit => "~((uint64_t)1 << tid)".to_string(),
        Val::Ramp | Val::RampF => "((uint64_t)tid * (uint64_t)n + (uint64_t)i + 1)".to_string(),
    };
    match (spec.val, spec.ty) {
        (Val::RampF, ty) => format!("f2b_{u}(({}){v64})", ty.c_float()),
        (_, Ty::Int(128)) => format!("(((u128){v64} << 64) | (u128){v64})"),
        _ => format!("({u}){v64}"),
    }
}

/// The C driver: the reference workers under a mutex, and the runs.
fn driver(specs: &[Spec]) -> String {
    let mut c = String::from(
        r#"#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
typedef unsigned __int128 u128;
typedef __int128 i128;
#define FLOATS(U, F) \
  static inline F b2f_##U(U b) { F f; memcpy(&f, &b, sizeof f); return f; } \
  static inline U f2b_##U(F f) { U b; memcpy(&b, &f, sizeof b); return b; }
FLOATS(uint16_t, _Float16)
FLOATS(uint32_t, float)
FLOATS(uint64_t, double)
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static inline uint64_t fold_u128(u128 o) { return (uint64_t)o + 3 * (uint64_t)(o >> 64); }
static inline uint64_t fold_uint64_t(uint64_t o) { return o; }
static inline uint64_t fold_uint32_t(uint32_t o) { return o; }
static inline uint64_t fold_uint16_t(uint16_t o) { return o; }
static inline uint64_t fold_uint8_t(uint8_t o) { return o; }
typedef uint64_t (*worker)(void *, int64_t, int64_t);
"#,
    );
    for (k, spec) in specs.iter().enumerate() {
        let u = spec.ty.c_bits();
        writeln!(
            c,
            "uint64_t w{k}(void *, int64_t, int64_t);\n\
             static uint64_t r{k}(void *cell, int64_t n, int64_t tid) {{\n  \
             {u} *p = cell; uint64_t acc = 0;\n  \
             for (int64_t i = 0; i < n; i++) {{\n    {u} v = {};\n    \
             pthread_mutex_lock(&mu); {u} o = *p; *p = ({u})({}); pthread_mutex_unlock(&mu);\n    \
             acc += fold_{u}(o);\n  }}\n  return acc;\n}}",
            c_value(spec),
            c_op(spec)
        )
        .unwrap();
    }
    c.push_str("static const struct { worker hod, ref; const char *name; unsigned size, sums; u128 init; } specs[] = {\n");
    for (k, spec) in specs.iter().enumerate() {
        writeln!(
            c,
            "  {{ w{k}, r{k}, \"{} {} {}{}\", {}, {}, {} }},",
            spec.op,
            spec.ty.ll(),
            spec.order,
            if spec.weak { " weak" } else { "" },
            spec.ty.bits() / 8,
            spec.sums(),
            c_lit(spec.init)
        )
        .unwrap();
    }
    write!(
        c,
        r#"}};
#define THREADS {THREADS}
#define N {N}
static pthread_barrier_t start;
struct job {{ worker f; void *cell; int64_t tid; uint64_t acc; }};
static void *run_job(void *arg) {{
  struct job *j = arg;
  pthread_barrier_wait(&start);
  j->acc = j->f(j->cell, N, j->tid);
  return 0;
}}
static uint64_t fold(const void *cell, unsigned size) {{
  u128 v = 0; memcpy(&v, cell, size);
  return size == 16 ? fold_u128(v) : (uint64_t)v;
}}
static void hex(char *out, const void *cell, unsigned size) {{
  const unsigned char *b = cell;
  for (unsigned i = 0; i < size; i++) sprintf(out + 2 * i, "%02x", b[i]);
}}
/* Runs the workers of one spec on THREADS threads; writes the result. */
static void counter(worker f, unsigned size, unsigned sums, u128 init, char *out) {{
  static _Alignas(16) unsigned char cell[16];
  memset(cell, 0, sizeof cell); memcpy(cell, &init, size);
  pthread_t t[THREADS]; struct job jobs[THREADS];
  for (int i = 0; i < THREADS; i++) {{
    jobs[i] = (struct job){{ f, cell, i, 0 }};
    pthread_create(&t[i], 0, run_job, &jobs[i]);
  }}
  uint64_t acc = 0;
  for (int i = 0; i < THREADS; i++) {{ pthread_join(t[i], 0); acc += jobs[i].acc; }}
  out[0] = 0;
  if (sums != 2) hex(out, cell, size);
  if (sums) sprintf(out + strlen(out), " sum %llx", (unsigned long long)(acc + fold(cell, size)));
}}
typedef void (*spinner)(void *, void *, int64_t);
void spin_strong(void *, void *, int64_t);
void spin_weak(void *, void *, int64_t);
void spin_wide(void *, void *, int64_t);
void spin_flag(void *, void *, int64_t);
struct spin_job {{ spinner f; void *lock; uint64_t *data; }};
static void *run_spin(void *arg) {{
  struct spin_job *j = arg;
  pthread_barrier_wait(&start);
  j->f(j->lock, j->data, N);
  return 0;
}}
static void spin(const char *name, spinner f) {{
  static _Alignas(16) unsigned char lock[16];
  static uint64_t data[2];
  memset(lock, 0, sizeof lock); data[0] = data[1] = 0;
  pthread_t t[THREADS]; struct spin_job jobs[THREADS];
  for (int i = 0; i < THREADS; i++) {{
    jobs[i] = (struct spin_job){{ f, lock, data }};
    pthread_create(&t[i], 0, run_spin, &jobs[i]);
  }}
  for (int i = 0; i < THREADS; i++) pthread_join(t[i], 0);
  printf("%s %llu %llu\n", name, (unsigned long long)data[0], (unsigned long long)data[1]);
}}
struct node {{ struct node *next; int64_t id; }};
void push(void *, struct node *);
struct node *pop(void *);
static struct node *head;
static struct node nodes[THREADS * N];
static unsigned char seen[THREADS * N];
static void *pusher(void *arg) {{
  int64_t tid = (int64_t)(intptr_t)arg;
  pthread_barrier_wait(&start);
  for (int64_t i = 0; i < N; i++) push(&head, &nodes[tid * N + i]);
  return 0;
}}
static void *popper(void *arg) {{
  (void)arg;
  pthread_barrier_wait(&start);
  struct node *x;
  while ((x = pop(&head))) __atomic_add_fetch(&seen[x->id], 1, __ATOMIC_RELAXED);
  return 0;
}}
static void stack(void) {{
  for (int64_t i = 0; i < THREADS * N; i++) nodes[i].id = i;
  pthread_t t[THREADS];
  for (int i = 0; i < THREADS; i++) pthread_create(&t[i], 0, pusher, (void *)(intptr_t)i);
  for (int i = 0; i < THREADS; i++) pthread_join(t[i], 0);
  for (int i = 0; i < THREADS; i++) pthread_create(&t[i], 0, popper, 0);
  for (int i = 0; i < THREADS; i++) pthread_join(t[i], 0);
  unsigned once = 0;
  for (int64_t i = 0; i < THREADS * N; i++) once += seen[i] == 1;
  printf("stack %u of %u once, head %s\n", once, THREADS * N, head ? "set" : "null");
}}
uint64_t tear_write(void *, int64_t, int64_t);
uint64_t tear_read(void *, int64_t, int64_t);
static void tear(void) {{
  static _Alignas(16) unsigned char cell[16];
  memset(cell, 0, sizeof cell);
  pthread_t t[THREADS]; struct job jobs[THREADS];
  for (int i = 0; i < THREADS; i++) {{
    jobs[i] = (struct job){{ i % 2 ? tear_read : tear_write, cell, i, 0 }};
    pthread_create(&t[i], 0, run_job, &jobs[i]);
  }}
  uint64_t bad = 0;
  for (int i = 0; i < THREADS; i++) {{ pthread_join(t[i], 0); bad += jobs[i].acc; }}
  printf("tear %llu\n", (unsigned long long)bad);
}}
typedef uint64_t (*passer)(void *, void *, int64_t, int64_t);
uint64_t pass(void *, void *, int64_t, int64_t);
uint64_t pass_fenced(void *, void *, int64_t, int64_t);
struct pass_job {{ passer f; void *flag; void *data; int64_t side; uint64_t bad; }};
static void *run_pass(void *arg) {{
  struct pass_job *j = arg;
  j->bad = j->f(j->flag, j->data, N, j->side);
  return 0;
}}
static void passing(const char *name, passer f) {{
  static _Alignas(16) uint64_t flag, data;
  flag = data = 0;
  pthread_t t[2]; struct pass_job jobs[2];
  for (int i = 0; i < 2; i++) {{
    jobs[i] = (struct pass_job){{ f, &flag, &data, i, 0 }};
    pthread_create(&t[i], 0, run_pass, &jobs[i]);
  }}
  for (int i = 0; i < 2; i++) pthread_join(t[i], 0);
  printf("%s %llu %llu\n", name, (unsigned long long)jobs[1].bad, (unsigned long long)flag);
}}
int main(void) {{
  pthread_barrier_init(&start, 0, THREADS);
  char got[80], want[80];
  int wrong = 0;
  for (unsigned k = 0; k < sizeof specs / sizeof specs[0]; k++) {{
    counter(specs[k].hod, specs[k].size, specs[k].sums, specs[k].init, got);
    counter(specs[k].ref, specs[k].size, specs[k].sums, specs[k].init, want);
    if (strcmp(got, want)) {{ wrong++; printf("WRONG %s: %s, reference %s\n", specs[k].name, got, want); }}
    else printf("%s: %s\n", specs[k].name, got);
  }}
  spin("spin_strong", spin_strong);
  spin("spin_weak", spin_weak);
  spin("spin_wide", spin_wide);
  spin("spin_flag", spin_flag);
  stack();
  tear();
  passing("pass", pass);
  passing("pass_fenced", pass_fenced);
  return wrong;
}}
"#
    )
    .unwrap();
    c
}

/// The lines every build must print after the counters.
fn expected_tail() -> String {
    let n = u64::from(THREADS * N);
    let mut s = String::new();
    for name in ["spin_strong", "spin_weak", "spin_wide", "spin_flag"] {
        writeln!(s, "{name} {n} {}", 2 * n).unwrap();
    }
    writeln!(s, "stack {n} of {n} once, head null").unwrap();
    writeln!(s, "tear 0").unwrap();
    writeln!(s, "pass 0 {}", 2 * u64::from(N)).unwrap();
    writeln!(s, "pass_fenced 0 {}", 2 * u64::from(N)).unwrap();
    s
}

/// Builds the workers with hod (both sequences, stressed and not) and
/// with `llc`, links each with the driver under every linker, runs each
/// [`RUNS`] times; every run must agree with the reference and print the
/// same.
pub fn check() {
    let scratch = Scratch::new("a64-atomic-threads");
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let specs = specs();
    let driver = toolchain.compile_c(&scratch, "driver", &driver(&specs), &["-pthread"]);
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for outline in [false, true] {
        let seq = if outline { "outline" } else { "llsc" };
        let text = module(&specs, outline);
        let (m, bodies) = parse(&text);
        for (mode, stress) in [("plain", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)]
        {
            let options = Options { stress, ..Options::default() };
            let emitted = compile_module(&m, &bodies, &options)
                .unwrap_or_else(|e| panic!("{seq} {mode}: {e}"));
            let name = format!("hod-{seq}-{mode}");
            objects.push((name.clone(), write_object(&scratch, &format!("{name}.o"), &emitted)));
        }
        if let Some(object) = llc(&scratch, &format!("llc-{seq}"), &text, &[]) {
            objects.push((format!("llc-{seq}"), object));
        }
    }
    let tail = expected_tail();
    let mut first: Option<String> = None;
    let mut runs = 0;
    for (name, object) in &objects {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("{name}-{}", linker.name));
            toolchain.link_ok(linker, &["-pthread"], &[&driver, object], &program);
            for _ in 0..RUNS {
                let ran = toolchain.run(linker, &program);
                let out = ran.stdout_text();
                assert!(ran.success(), "{name} {}: {:?}\n{out}", linker.name, ran.status);
                assert!(out.ends_with(&tail), "{name} {}:\n{out}", linker.name);
                match &first {
                    None => first = Some(out),
                    Some(f) => assert_eq!(&out, f, "{name} {}", linker.name),
                }
                runs += 1;
            }
        }
    }
    eprintln!(
        "a64 atomic threads: {} counters + 8 structures, {THREADS} threads x {N} operations, \
         {} builds, {runs} runs, all exact",
        specs.len(),
        objects.len()
    );
}
