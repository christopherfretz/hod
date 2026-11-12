//! The memory intrinsics' programs (WP-041): functions of a 256-byte
//! buffer that run one to four of `memcpy`, `memcpy.inline`, `memmove`,
//! `memset`, `memset.inline` on it, with constant lengths on both sides
//! of the inline thresholds and past the `.inline` loop's 16-byte steps,
//! variable lengths of `i64` and `i32` parameters (garbage above the
//! width), constant and variable fill bytes, both volatile flags, and
//! overlapping `memmove`s in both directions. The first 64 bytes of the
//! buffer are never written (a sentinel); the buffer ends at a page
//! boundary natively, so a write past it faults.
//!
//! Each function also keeps integer and `double` values live across the
//! operations (more than the register pool holds) and returns a
//! combination of them, so a call that clobbered a live register shows.

#![allow(clippy::too_many_lines, clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::int_gen::Rng;

/// The buffer's size.
pub const BUF: u64 = 256;
/// The bytes at its start that are never written.
pub const SENTINEL: u64 = 64;

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
    /// The value of `%d`.
    pub double: f64,
}

/// The parameter list every function has.
pub const PARAMS: &str = "ptr %buf, i64 %n, i32 %m, i8 %v, i64 %x, double %d";

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

/// A function of `seed`, named `name`; its declarations go to `decls`.
pub fn generate(name: &str, seed: u64, decls: &mut BTreeSet<String>) -> Func {
    let mut rng = Rng(seed ^ 0x0041_0000_0000);
    let mut body = String::new();
    // Live values: eight integers and three doubles, used at the end.
    for k in 0..8 {
        writeln!(body, "  %a{k} = add i64 %x, {}", (k + 1) * 0x1_0001).unwrap();
    }
    for k in 0..3 {
        writeln!(body, "  %f{k} = fadd double %d, {}.5", k + 1).unwrap();
    }
    let ops = 1 + rng.below(4);
    let mut variable = None;
    for op in 0..ops {
        let volatile = rng.chance(20);
        let kind = rng.below(9);
        // A variable length: `%n` or `%m`, of a value fixed for the
        // function (one of them per function).
        let var_len = kind >= 7 && variable.is_none();
        let (len, len_ty, len_text) = if var_len {
            let len = length(&mut rng, 160);
            let wide = rng.chance(50);
            variable = Some((len, wide));
            if wide { (len, "i64", "%n".to_string()) } else { (len, "i32", "%m".to_string()) }
        } else {
            let max = if kind == 1 || kind == 6 { 192 } else { 96 };
            let len = length(&mut rng, max);
            let ty = if rng.chance(15) { "i32" } else { "i64" };
            (len, ty, len.to_string())
        };
        let dst = start(&mut rng, len, true);
        writeln!(body, "  %d{op} = getelementptr i8, ptr %buf, i64 {dst}").unwrap();
        let flag = if volatile { "true" } else { "false" };
        let memset = matches!(kind, 4..=6) || (var_len && rng.chance(30));
        if memset {
            let inline = kind == 6 && !var_len;
            let byte = if rng.chance(50) {
                "%v".to_string()
            } else {
                rng.pick(&["0", "1", "-1", "85", "-128"]).to_string()
            };
            let f = if inline { "memset.inline" } else { "memset" };
            decls.insert(format!("declare void @llvm.{f}.p0.{len_ty}(ptr, i8, {len_ty}, i1)"));
            writeln!(
                body,
                "  call void @llvm.{f}.p0.{len_ty}(ptr %d{op}, i8 {byte}, {len_ty} {len_text}, i1 {flag})"
            )
            .unwrap();
            continue;
        }
        // A copy: memmove anywhere; memcpy from a disjoint range (or the
        // same one).
        let moving = kind == 2 || kind == 3 || (var_len && rng.chance(50));
        let src = if moving {
            if rng.chance(50) {
                // Overlapping, either direction.
                let delta = 1 + rng.below(len.clamp(1, 40));
                let s = if rng.chance(50) { dst.saturating_sub(delta) } else { dst + delta };
                s.min(BUF - len)
            } else {
                start(&mut rng, len, false)
            }
        } else if rng.chance(5) {
            dst
        } else {
            let mut s = start(&mut rng, len, false);
            let mut tries = 0;
            while s < dst + len && dst < s + len && tries < 64 {
                s = start(&mut rng, len, false);
                tries += 1;
            }
            if s < dst + len && dst < s + len { dst } else { s }
        };
        writeln!(body, "  %s{op} = getelementptr i8, ptr %buf, i64 {src}").unwrap();
        let f = if moving {
            "memmove"
        } else if kind == 1 && !var_len {
            "memcpy.inline"
        } else {
            "memcpy"
        };
        decls.insert(format!("declare void @llvm.{f}.p0.p0.{len_ty}(ptr, ptr, {len_ty}, i1)"));
        writeln!(
            body,
            "  call void @llvm.{f}.p0.p0.{len_ty}(ptr %d{op}, ptr %s{op}, {len_ty} {len_text}, i1 {flag})"
        )
        .unwrap();
    }
    // The live values, combined.
    let mut acc = "%a0".to_string();
    for k in 1..8 {
        writeln!(body, "  %c{k} = xor i64 {acc}, %a{k}").unwrap();
        acc = format!("%c{k}");
    }
    writeln!(body, "  %g0 = fadd double %f0, %f1\n  %g1 = fmul double %g0, %f2").unwrap();
    writeln!(body, "  %gi = fptosi double %g1 to i64\n  %r = add i64 {acc}, %gi").unwrap();
    writeln!(body, "  ret i64 %r").unwrap();
    let text = format!("define i64 @{name}({PARAMS}) {{\n{body}}}\n");
    let buf = (0..BUF).map(|_| rng.next() as u8).collect();
    let (len, _) = variable.unwrap_or((0, true));
    // The variable lengths, with garbage above `%m`'s and `%v`'s widths
    // natively (the interpreter gets them at their width).
    let ints = [len, len, rng.next() & 0xff, rng.next()];
    let double = (rng.below(2000) as f64 - 1000.0) / 8.0;
    Func { name: name.to_string(), text, buf, ints, double }
}
