//! The x86-64 intrinsic group's integer programs (WP-128), built on
//! `aggr_gen`'s [`Suite`]: cores over `%a`, `%b`, `%c`, run by
//! `hod_interp`, by hod plainly and stressed, by `llc` 22 and 19, in the
//! two mixed legs (hod's wrappers calling `llc`'s twins and the reverse),
//! from memory and from registers with garbage above the widths.
//! Adapted from the AArch64 group's generators
//! (`crates/hod_a64/tests/common/intrinsic_gen.rs` and
//! `intrinsic2_gen.rs`):
//!
//! - [`narrow_table`]: `ctlz`, `cttz` (both poison flags), `ctpop`,
//!   `bswap`, `bitreverse`, `abs`, `smax`/`smin`/`umax`/`umin`,
//!   `fshl`/`fshr` (variable amounts, constant amounts, rotates) at every
//!   width of [`WIDTHS`], register and constant operands;
//! - [`sat_table`]: `{s,u}{add,sub}.sat` at every width 1 to 64 and 128,
//!   exhaustive at the narrowest widths;
//! - [`three_way_table`]: `scmp`/`ucmp` over operand by result widths;
//! - [`wide_table`]: the unary, min/max and funnel intrinsics at `i128`;
//! - [`shl_sat_table`]: `{s,u}shl.sat` at every width, every amount below
//!   the width.

#![allow(dead_code, reason = "each test uses part of it")]
#![allow(clippy::cloned_ref_to_slice_refs, reason = "`.ll` cores built by hand, as `aggr_gen`")]

use crate::aggr_gen::{Rng, Suite, T, bytes_of, edges, lit, small, value};

/// The widths the register-class intrinsics are checked at.
pub const WIDTHS: [u32; 19] =
    [1, 2, 3, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 40, 48, 56, 63, 64, 128];

/// The saturating operations.
pub const SAT: [&str; 4] = ["sadd.sat", "uadd.sat", "ssub.sat", "usub.sat"];

fn ints(ws: &[Vec<u64>], bits: u32) -> Vec<Vec<u8>> {
    ws.iter().map(|w| bytes_of(w, bits)).collect()
}

/// A value of `bits`: an edge (60%) or random.
fn pick(rng: &mut Rng, e: &[Vec<u64>], bits: u32) -> Vec<u64> {
    if rng.chance(60) { rng.pick(e).clone() } else { value(rng, bits) }
}

/// `call iN @llvm.<op>.iN(<args>[, i1 <flag>])`, declared.
fn call(s: &mut Suite, op: &str, bits: u32, args: &[&str], flag: Option<bool>) -> String {
    let t = format!("i{bits}");
    let mut params: Vec<String> = args.iter().map(|_| t.clone()).collect();
    let mut list: Vec<String> = args.iter().map(|a| format!("{t} {a}")).collect();
    if let Some(flag) = flag {
        params.push("i1".into());
        list.push(format!("i1 {flag}"));
    }
    s.decls.insert(format!("declare {t} @llvm.{op}.{t}({})", params.join(", ")));
    format!("call {t} @llvm.{op}.{t}({})", list.join(", "))
}

/// `call iN @llvm.<op>.iN(<a>, <b>)` as the core defining `%r`, declared.
fn binary(s: &mut Suite, op: &str, bits: u32, a: &str, b: &str) -> String {
    format!("  %r = {}\n", call(s, op, bits, &[a, b], None))
}

/// The unary, min/max and funnel intrinsics at every width below 128
/// (`i128` is [`wide_table`]'s).
pub fn narrow_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS.into_iter().filter(|&b| b <= 64) {
        let ty = T::I(bits);
        let e = edges(bits);
        for (name, flags) in [
            ("ctlz", &[Some(false), Some(true)][..]),
            ("cttz", &[Some(false), Some(true)]),
            ("ctpop", &[None]),
            ("bswap", &[None]),
            ("bitreverse", &[None]),
            ("abs", &[Some(false), Some(true)]),
        ] {
            if name == "bswap" && !bits.is_multiple_of(16) {
                continue;
            }
            for &flag in flags {
                let core = format!("  %r = {}\n", call(&mut s, name, bits, &["%a"], flag));
                let f = s.add(&[ty.clone()], ty.clone(), &core);
                for a in &e {
                    s.case(f, ints(&[a.clone()], bits));
                }
                for k in 0..bits {
                    s.case(f, ints(&[small(bits, 1u128 << k)], bits));
                    s.case(f, ints(&[small(bits, u128::MAX >> (128 - bits + k))], bits));
                }
                for _ in 0..8 {
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
            for _ in 0..10 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b], bits));
            }
            // A constant on each side.
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
        for op in ["fshl", "fshr"] {
            let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%b", "%c"], None));
            let f = s.add(&[ty.clone(), ty.clone(), ty.clone()], ty.clone(), &core);
            let n = u128::from(bits);
            let mask = u128::MAX >> (128 - bits);
            let mut amounts: Vec<u128> = (0..=n + 2).collect();
            amounts.extend([2 * n + 3, mask, mask - 1]);
            for c in amounts {
                for _ in 0..2 {
                    let (a, b) = (value(rng, bits), value(rng, bits));
                    s.case(f, ints(&[a, b, small(bits, c & mask)], bits));
                }
            }
            for _ in 0..10 {
                let (a, b, c) = (value(rng, bits), value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b, c], bits));
            }
            // Constant amounts.
            for c in [0, 1, 2, n - 1, n, n + 1, 2 * n + 3, u128::from(rng.next())] {
                let c = lit(&small(bits, c & mask), bits);
                let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%b", &c], None));
                let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
                for _ in 0..12 {
                    let (a, b) = (value(rng, bits), value(rng, bits));
                    s.case(f, ints(&[a, b], bits));
                }
            }
            // A rotate: the same value twice.
            let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%a", "%b"], None));
            let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
            for _ in 0..12 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b], bits));
            }
        }
    }
    s
}

/// The saturating arithmetic at every width.
pub fn sat_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in [1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 48, 62, 63, 64, 128] {
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
    for bits in [1u32, 2, 7, 8, 9, 16, 17, 31, 32, 33, 63, 64, 128] {
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

/// The unary, min/max and funnel intrinsics at `i128`.
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
            let core = format!("  %r = {}\n", call(&mut s, name, bits, &["%a"], flag));
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
        let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%b", "%c"], None));
        let f = s.add(&[ty.clone(), ty.clone(), ty.clone()], ty.clone(), &core);
        let mut amounts: Vec<u128> = (0..=130).collect();
        amounts.extend([255, 256, 300, u128::MAX, u128::MAX - 63, 1u128 << 64 | 3]);
        for c in amounts {
            for _ in 0..2 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b, small(bits, c)], bits));
            }
        }
        for c in [0u128, 1, 63, 64, 65, 127] {
            let c = c.to_string();
            let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%b", &c], None));
            let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
            for _ in 0..8 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                s.case(f, ints(&[a, b], bits));
            }
        }
        // A rotate.
        let core = format!("  %r = {}\n", call(&mut s, op, bits, &["%a", "%a", "%b"], None));
        let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
        for _ in 0..40 {
            let a = value(rng, bits);
            let k = small(bits, u128::from(rng.below(260)));
            s.case(f, ints(&[a, k], bits));
        }
    }
    s
}

/// `sshl.sat`/`ushl.sat` at every width up to 64: every amount below
/// the width (an amount at or above it is poison), edge and random
/// values; a constant amount.
pub fn shl_sat_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS.into_iter().filter(|&b| b <= 64) {
        let ty = T::I(bits);
        let e = edges(bits);
        for op in ["sshl.sat", "ushl.sat"] {
            let core = binary(&mut s, op, bits, "%a", "%b");
            let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
            for k in 0..bits {
                for _ in 0..3 {
                    let a = pick(rng, &e, bits);
                    s.case(f, ints(&[a, small(bits, u128::from(k))], bits));
                }
            }
            for a in &e {
                let k = small(bits, u128::from(rng.below(u64::from(bits))));
                s.case(f, ints(&[a.clone(), k], bits));
            }
            let k = rng.below(u64::from(bits));
            let core = binary(&mut s, op, bits, "%a", &lit(&small(bits, k.into()), bits));
            let f = s.add(&[ty.clone()], ty.clone(), &core);
            for a in &e {
                s.case(f, ints(&[a.clone()], bits));
            }
        }
    }
    s
}
