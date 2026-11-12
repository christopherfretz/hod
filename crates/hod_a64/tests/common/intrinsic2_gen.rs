//! The second intrinsic set's integer programs (WP-070), built on
//! `aggr_gen`'s [`Suite`] (cores over `%a`, `%b`, `%c`, run by
//! `hod_interp`, by hod plainly and stressed, and by `llc -O0
//! -global-isel=0 -fast-isel=0`, from memory and from registers with
//! garbage above the widths):
//!
//! - `{s,u}{add,sub}.sat` at every width 1 to 64 and 128, exhaustive at
//!   the narrowest widths, edges and random values elsewhere, a constant
//!   on either side;
//! - `scmp`/`ucmp` for operand widths 1 to 128 by result widths 2 to 128;
//! - the `i128` forms of WP-041's integer intrinsics.

#![allow(dead_code, reason = "each test uses part of it")]

use crate::aggr_gen::{Rng, Suite, T, bytes_of, edges, lit, small, value};

/// The widths the saturating arithmetic is checked at.
pub const SAT_WIDTHS: [u32; 19] =
    [1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 48, 62, 63, 64, 128];

/// The saturating operations.
pub const SAT: [&str; 4] = ["sadd.sat", "uadd.sat", "ssub.sat", "usub.sat"];

fn ints(ws: &[Vec<u64>], bits: u32) -> Vec<Vec<u8>> {
    ws.iter().map(|w| bytes_of(w, bits)).collect()
}

/// A value of `bits`: an edge (60%) or random.
fn pick(rng: &mut Rng, e: &[Vec<u64>], bits: u32) -> Vec<u64> {
    if rng.chance(60) { rng.pick(e).clone() } else { value(rng, bits) }
}

/// `call iN @llvm.<op>.iN(<a>, <b>)` as the core defining `%r`, declared.
fn binary(s: &mut Suite, op: &str, bits: u32, a: &str, b: &str) -> String {
    let t = format!("i{bits}");
    s.decls.insert(format!("declare {t} @llvm.{op}.{t}({t}, {t})"));
    format!("  %r = call {t} @llvm.{op}.{t}({t} {a}, {t} {b})\n")
}

/// The saturating arithmetic at every width.
pub fn sat_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in SAT_WIDTHS {
        let ty = T::I(bits);
        let e = edges(bits);
        for op in SAT {
            let core = binary(&mut s, op, bits, "%a", "%b");
            let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
            if bits <= 5 {
                for a in 0..1u128 << bits {
                    for b in 0..1u128 << bits {
                        s.case(f, ints(&[small(bits, a), small(bits, b)], bits));
                    }
                }
            } else {
                for a in &e {
                    for _ in 0..3 {
                        let b = rng.pick(&e).clone();
                        s.case(f, ints(&[a.clone(), b], bits));
                    }
                }
                for _ in 0..30 {
                    let (a, b) = (value(rng, bits), value(rng, bits));
                    s.case(f, ints(&[a, b], bits));
                }
            }
            // A constant on each side.
            for _ in 0..2 {
                let k = rng.pick(&e).clone();
                let left = binary(&mut s, op, bits, &lit(&k, bits), "%a").replace("%r =", "%x =");
                let right = binary(&mut s, op, bits, "%a", &lit(&k, bits)).replace("%r =", "%y =");
                let core = format!("{left}{right}  %r = xor i{bits} %x, %y\n");
                let f = s.add(&[ty.clone()], ty.clone(), &core);
                for _ in 0..10 {
                    let a = pick(rng, &e, bits);
                    s.case(f, ints(&[a], bits));
                }
            }
            // The same value twice.
            let core = binary(&mut s, op, bits, "%a", "%a");
            let f = s.add(&[ty.clone()], ty.clone(), &core);
            for _ in 0..6 {
                let a = pick(rng, &e, bits);
                s.case(f, ints(&[a], bits));
            }
        }
    }
    s
}

/// `scmp`/`ucmp` over operand widths by result widths.
pub fn three_way_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in [1u32, 2, 7, 8, 9, 16, 31, 32, 33, 63, 64, 128] {
        let ty = T::I(bits);
        let e = edges(bits);
        for ret in [2u32, 8, 32, 64, 128] {
            for op in ["scmp", "ucmp"] {
                let (t, r) = (format!("i{bits}"), format!("i{ret}"));
                s.decls.insert(format!("declare {r} @llvm.{op}.{r}.{t}({t}, {t})"));
                let call =
                    |a: &str, b: &str| format!("call {r} @llvm.{op}.{r}.{t}({t} {a}, {t} {b})");
                let f = s.add(
                    &[ty.clone(), ty.clone()],
                    T::I(ret),
                    &format!("  %r = {}\n", call("%a", "%b")),
                );
                for a in &e {
                    s.case(f, ints(&[a.clone(), a.clone()], bits));
                    for _ in 0..2 {
                        let b = rng.pick(&e).clone();
                        s.case(f, ints(&[a.clone(), b], bits));
                    }
                }
                for _ in 0..10 {
                    let (a, b) = (value(rng, bits), value(rng, bits));
                    s.case(f, ints(&[a, b], bits));
                }
                let k = rng.pick(&e).clone();
                let f = s.add(
                    &[ty.clone()],
                    T::I(ret),
                    &format!(
                        "  %x = {}\n  %y = {}\n  %r = add {r} %x, %y\n",
                        call(&lit(&k, bits), "%a"),
                        call("%a", &lit(&k, bits))
                    ),
                );
                for _ in 0..8 {
                    let a = pick(rng, &e, bits);
                    s.case(f, ints(&[a], bits));
                }
            }
        }
    }
    s
}

/// WP-041's integer intrinsics at `i128`.
pub fn wide_table(rng: &mut Rng) -> Suite {
    let bits = 128;
    let ty = T::I(bits);
    let e = edges(bits);
    let mut s = Suite::default();
    for (name, flags) in [
        ("ctlz", &[Some(false), Some(true)][..]),
        ("cttz", &[Some(false), Some(true)]),
        ("ctpop", &[None]),
        ("bswap", &[None]),
        ("bitreverse", &[None]),
        ("abs", &[Some(false), Some(true)]),
    ] {
        for &flag in flags {
            let (decl, arg) = match flag {
                Some(flag) => (", i1", format!(", i1 {flag}")),
                None => ("", String::new()),
            };
            s.decls.insert(format!("declare i128 @llvm.{name}.i128(i128{decl})"));
            let core = format!("  %r = call i128 @llvm.{name}.i128(i128 %a{arg})\n");
            let f = s.add(&[ty.clone()], ty.clone(), &core);
            for a in &e {
                s.case(f, ints(&[a.clone()], bits));
            }
            for k in 0..128 {
                s.case(f, ints(&[small(bits, 1u128 << k)], bits));
                s.case(f, ints(&[small(bits, u128::MAX >> k)], bits));
            }
            for _ in 0..20 {
                let a = value(rng, bits);
                s.case(f, ints(&[a], bits));
            }
        }
    }
    for op in ["smax", "smin", "umax", "umin"] {
        let core = binary(&mut s, op, bits, "%a", "%b");
        let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
        for a in &e {
            s.case(f, ints(&[a.clone(), a.clone()], bits));
            for _ in 0..3 {
                let b = rng.pick(&e).clone();
                s.case(f, ints(&[a.clone(), b], bits));
            }
        }
        // Equal high limbs, the low ones deciding.
        for _ in 0..10 {
            let (a, mut b) = (value(rng, bits), value(rng, bits));
            b[1] = a[1];
            s.case(f, ints(&[a, b], bits));
        }
        let k = rng.pick(&e).clone();
        let core = binary(&mut s, op, bits, "%a", &lit(&k, bits));
        let f = s.add(&[ty.clone()], ty.clone(), &core);
        for _ in 0..10 {
            let a = pick(rng, &e, bits);
            s.case(f, ints(&[a], bits));
        }
    }
    for op in ["fshl", "fshr"] {
        s.decls.insert(format!("declare i128 @llvm.{op}.i128(i128, i128, i128)"));
        let f = s.add(
            &[ty.clone(), ty.clone(), ty.clone()],
            ty.clone(),
            &format!("  %r = call i128 @llvm.{op}.i128(i128 %a, i128 %b, i128 %c)\n"),
        );
        let mut amounts: Vec<u128> = (0..=130).collect();
        amounts.extend([255, 256, 300, u128::MAX, u128::MAX - 63, 1u128 << 64 | 3]);
        for c in amounts {
            for _ in 0..2 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b, small(bits, c)], bits));
            }
        }
        for c in [0u128, 1, 63, 64, 65, 127] {
            let f = s.add(
                &[ty.clone(), ty.clone()],
                ty.clone(),
                &format!("  %r = call i128 @llvm.{op}.i128(i128 %a, i128 %b, i128 {c})\n"),
            );
            for _ in 0..8 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b], bits));
            }
        }
        // A rotate.
        let f = s.add(
            &[ty.clone(), ty.clone()],
            ty.clone(),
            &format!("  %r = call i128 @llvm.{op}.i128(i128 %a, i128 %a, i128 %b)\n"),
        );
        for _ in 0..40 {
            let a = value(rng, bits);
            let k = small(bits, u128::from(rng.below(260)));
            s.case(f, ints(&[a, k], bits));
        }
    }
    s
}
