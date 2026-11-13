//! The memory intrinsics' programs on x86-64 (WP-128), adapted from the
//! AArch64 group's (`crates/hod_a64/tests/common/intrinsic_mem.rs`,
//! without its `double` values, which another group lowers): functions
//! of a 512-byte buffer that run `memcpy`, `memcpy.inline`, `memmove`,
//! `memset`, `memset.inline` on it.
//!
//! - [`systematic`]: one operation per function, every length 0 to 130
//!   at a destination offset (alignment) 0 to 15 and a source offset
//!   that walks the alignments too, constant and variable (`i64` and
//!   `i32`) lengths, overlapping `memmove`s in both directions at
//!   distances 1 to 17;
//! - [`generate`]: one to four random operations per function, constant
//!   lengths on both sides of the inline thresholds and past the
//!   `.inline` loops' steps, variable lengths, constant and variable
//!   fill bytes, both volatile flags.
//!
//! The first 64 bytes of the buffer are never written (a sentinel); the
//! buffer ends at a page boundary natively, so an access past it faults.
//! Each function keeps twelve integer values live across the operations
//! (more than the registers that survive a call) and returns their
//! combination, so a call or an inline sequence that clobbered a live
//! register shows.

#![allow(clippy::cast_possible_truncation)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::int_gen::Rng;

/// The buffer's size.
pub const BUF: u64 = 512;
/// The bytes at its start that are never written.
pub const SENTINEL: u64 = 64;
/// Live values kept across the operations.
const LIVE: u64 = 12;

/// A generated function.
#[derive(Clone, Debug)]
pub struct Func {
    /// Its name.
    pub name: String,
    /// Its text.
    pub text: String,
    /// The buffer's initial bytes.
    pub buf: Vec<u8>,
    /// The values of `%n` (i64), `%m` (i32), `%v` (i8), `%x` (i64).
    pub ints: [u64; 4],
}

/// The parameter list every function has.
pub const PARAMS: &str = "ptr %buf, i64 %n, i32 %m, i8 %v, i64 %x";

/// One operation: `memset` writes `byte`, the others copy from `src`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Copy,
    CopyInline,
    Move,
    Set,
    SetInline,
}

impl Op {
    fn name(self) -> &'static str {
        match self {
            Op::Copy => "memcpy",
            Op::CopyInline => "memcpy.inline",
            Op::Move => "memmove",
            Op::Set => "memset",
            Op::SetInline => "memset.inline",
        }
    }
}

/// How the length is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Len {
    Const,
    /// `%n`, an `i64` parameter.
    Wide,
    /// `%m`, an `i32` parameter (garbage above its width natively).
    Narrow,
}

/// The call of `op` on `%d<k>` (and `%s<k>`) of `len` bytes, declared.
#[allow(clippy::too_many_arguments, reason = "one call's parts")]
fn op_call(
    decls: &mut BTreeSet<String>,
    body: &mut String,
    k: usize,
    op: Op,
    len: Len,
    bytes: u64,
    byte: &str,
    volatile: bool,
) {
    let (ty, text) = match len {
        Len::Const => ("i64", bytes.to_string()),
        Len::Wide => ("i64", "%n".to_string()),
        Len::Narrow => ("i32", "%m".to_string()),
    };
    let f = op.name();
    if matches!(op, Op::Set | Op::SetInline) {
        decls.insert(format!("declare void @llvm.{f}.p0.{ty}(ptr, i8, {ty}, i1)"));
        writeln!(
            body,
            "  call void @llvm.{f}.p0.{ty}(ptr %d{k}, i8 {byte}, {ty} {text}, i1 {volatile})"
        )
        .unwrap();
    } else {
        decls.insert(format!("declare void @llvm.{f}.p0.p0.{ty}(ptr, ptr, {ty}, i1)"));
        writeln!(
            body,
            "  call void @llvm.{f}.p0.p0.{ty}(ptr %d{k}, ptr %s{k}, {ty} {text}, i1 {volatile})"
        )
        .unwrap();
    }
}

/// The function around `ops` (its body lines): the live values before,
/// their combination after.
fn wrap(name: &str, ops: &str, rng: &mut Rng, len: u64) -> Func {
    let mut body = String::new();
    for k in 0..LIVE {
        writeln!(body, "  %a{k} = add i64 %x, {}", (k + 1) * 0x1_0001).unwrap();
    }
    body.push_str(ops);
    let mut acc = "%a0".to_string();
    for k in 1..LIVE {
        writeln!(body, "  %c{k} = xor i64 {acc}, %a{k}").unwrap();
        writeln!(body, "  %e{k} = mul i64 %c{k}, 3").unwrap();
        acc = format!("%e{k}");
    }
    writeln!(body, "  ret i64 {acc}").unwrap();
    let text = format!("define i64 @{name}({PARAMS}) {{\n{body}}}\n");
    let buf = (0..BUF).map(|_| rng.next() as u8).collect();
    let ints = [len, len, rng.next() & 0xff, rng.next()];
    Func { name: name.to_string(), text, buf, ints }
}

/// The systematic functions: every operation and length kind, every
/// length 0 to 130, alignments and overlaps walked with the length.
pub fn systematic(decls: &mut BTreeSet<String>) -> Vec<Func> {
    let mut funcs = Vec::new();
    let mut rng = Rng(0x0128_0128);
    for op in [Op::Copy, Op::CopyInline, Op::Move, Op::Set, Op::SetInline] {
        for len in [Len::Const, Len::Wide, Len::Narrow] {
            // The `.inline` forms take constant lengths only.
            if len != Len::Const && matches!(op, Op::CopyInline | Op::SetInline) {
                continue;
            }
            for bytes in 0..=130u64 {
                let dst = SENTINEL + bytes % 16;
                let mut ops = format!("  %d0 = getelementptr i8, ptr %buf, i64 {dst}\n");
                if !matches!(op, Op::Set | Op::SetInline) {
                    let src = if op == Op::Move {
                        // Overlapping, below and above in turn.
                        let delta = 1 + bytes % 17;
                        if bytes % 2 == 0 { dst - delta } else { dst + delta }
                    } else {
                        // Disjoint: after the destination, at every
                        // alignment.
                        dst + bytes + (bytes * 7) % 16
                    };
                    writeln!(ops, "  %s0 = getelementptr i8, ptr %buf, i64 {src}").unwrap();
                }
                let byte = if bytes % 3 == 0 { "%v" } else { "-91" };
                op_call(decls, &mut ops, 0, op, len, bytes, byte, bytes % 5 == 0);
                let name = format!("s_{}_{len:?}_{bytes}", op.name().replace('.', "_"));
                funcs.push(wrap(&name, &ops, &mut rng, bytes));
            }
        }
    }
    funcs
}

/// A range of `len` bytes in the buffer: a start for a write (past the
/// sentinel) or a read (anywhere).
fn start(rng: &mut Rng, len: u64, write: bool) -> u64 {
    let low = if write { SENTINEL } else { 0 };
    low + rng.below(BUF - len - low + 1)
}

/// A constant length: mostly around the thresholds.
fn length(rng: &mut Rng, max: u64) -> u64 {
    let len = match rng.below(10) {
        0 => 0,
        1..=4 => 1 + rng.below(40),
        5 | 6 => 28 + rng.below(40),
        7 => *rng.pick(&[15, 16, 17, 31, 32, 33, 63, 64, 65, 80]),
        _ => rng.below(193),
    };
    len.min(max)
}

/// A random function of `seed`, named `name`; its declarations go to
/// `decls`.
pub fn generate(name: &str, seed: u64, decls: &mut BTreeSet<String>) -> Func {
    let mut rng = Rng(seed ^ 0x0128_0000_0000);
    let mut body = String::new();
    let ops = 1 + rng.below(4) as usize;
    let mut variable = None;
    for k in 0..ops {
        let volatile = rng.chance(20);
        let kind = rng.below(9);
        // A variable length: `%n` or `%m`, of a value fixed for the
        // function (one of them per function).
        let var_len = kind >= 7 && variable.is_none();
        let (bytes, len) = if var_len {
            let bytes = length(&mut rng, 160);
            let wide = rng.chance(50);
            variable = Some(bytes);
            (bytes, if wide { Len::Wide } else { Len::Narrow })
        } else {
            let max = if kind == 1 || kind == 6 { 192 } else { 96 };
            (length(&mut rng, max), Len::Const)
        };
        let dst = start(&mut rng, bytes, true);
        writeln!(body, "  %d{k} = getelementptr i8, ptr %buf, i64 {dst}").unwrap();
        let set = matches!(kind, 4..=6) || (var_len && rng.chance(30));
        if set {
            let op = if kind == 6 && !var_len { Op::SetInline } else { Op::Set };
            let byte = if rng.chance(50) {
                "%v".to_string()
            } else {
                rng.pick(&["0", "1", "-1", "85", "-128"]).to_string()
            };
            op_call(decls, &mut body, k, op, len, bytes, &byte, volatile);
            continue;
        }
        // A copy: memmove anywhere; memcpy from a disjoint range (or the
        // same one).
        let moving = kind == 2 || kind == 3 || (var_len && rng.chance(50));
        let src = if moving {
            if rng.chance(50) {
                let delta = 1 + rng.below(bytes.clamp(1, 40));
                let s = if rng.chance(50) { dst.saturating_sub(delta) } else { dst + delta };
                s.min(BUF - bytes)
            } else {
                start(&mut rng, bytes, false)
            }
        } else if rng.chance(5) {
            dst
        } else {
            let mut s = start(&mut rng, bytes, false);
            let mut tries = 0;
            while s < dst + bytes && dst < s + bytes && tries < 64 {
                s = start(&mut rng, bytes, false);
                tries += 1;
            }
            if s < dst + bytes && dst < s + bytes { dst } else { s }
        };
        writeln!(body, "  %s{k} = getelementptr i8, ptr %buf, i64 {src}").unwrap();
        let op = if moving {
            Op::Move
        } else if kind == 1 && !var_len {
            Op::CopyInline
        } else {
            Op::Copy
        };
        op_call(decls, &mut body, k, op, len, bytes, "", volatile);
    }
    wrap(name, &body, &mut rng, variable.unwrap_or(0))
}
