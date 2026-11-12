//! `condbr` fused with a deferred compare (WP-066; `spec/codegen.md`
//! §4.6, §5.5.1 "The groups together"): every `icmp` predicate on every
//! integer width and on pointers, every `fcmp` predicate on `half`,
//! `float`, `double` and `fp128` (whose compare is calls), with the true
//! block, the false block or neither emitted next, a `phi` stored on the
//! edge, a compare whose result has another use (not fused), and a
//! compare separated from its branch by an instruction that sets the
//! flags itself (not fused). The oracle is `hod_interp`, the second
//! `llc -O0 -global-isel=0 -fast-isel=0`, through `common::control`'s
//! batch check (one object, one driver, one process per mode).

mod common;
#[path = "common/control.rs"]
#[allow(dead_code, reason = "the graph generator is not used here")]
mod graphs;
#[path = "common/native.rs"]
mod native;

use std::fmt::Write as _;

use common::{emit, ll_header, objdump, parse, write_object};
use graphs::{Call, Func, Mode, Rng, Ty, Wrong, check, mask, signed, text_of};
use hod_cg::{Options, RelocModel, Stress};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
};

/// Plain, every stress option, far mode alone.
const MODES: [Mode; 3] = [
    Mode { name: "plain", options: DEFAULT },
    Mode { name: "stress", options: Options { stress: Stress::ALL, ..DEFAULT } },
    Mode {
        name: "far",
        options: Options {
            stress: Stress { min_pool: false, far: true, displace: false },
            ..DEFAULT
        },
    },
];

const INT_PREDS: [&str; 10] = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];
const FP_PREDS: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult", "ule",
    "une", "uno", "true",
];

/// Where the compare sits and who else reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// `%c = cmp` right before the branch, its only use: fused.
    Fused,
    /// Fused, and the true edge goes straight to the join block, whose
    /// `phi` takes a value computed in the entry block: the phi store is
    /// written before the compare (before its calls, for `fp128`).
    PhiEdge,
    /// The result is also extended in the true block: not deferred.
    TwoUses,
    /// An `icmp` fused into a `zext` (a `cset`, which sets the flags)
    /// stands between the compare and the branch: not fused.
    Separated,
}

const SHAPES: [Shape; 4] = [Shape::Fused, Shape::PhiEdge, Shape::TwoUses, Shape::Separated];

/// The order of the blocks after the entry block: the true block next,
/// the false block next, or the join block next (neither falls through).
const LAYOUTS: [[&str; 3]; 3] = [["t", "f", "m"], ["f", "t", "m"], ["m", "t", "f"]];

/// One function: `prelude` defines `%x` and `%y` from the parameters
/// (the last parameter is `%k`, an `i64`), then the compare and the
/// branch in `shape` and `layout`.
fn function(
    name: &str,
    params: &[Ty],
    prelude: &str,
    compare: &str,
    shape: Shape,
    layout: [&str; 3],
) -> Func {
    let list: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            if i + 1 == params.len() {
                format!("{} %k", ty.ll())
            } else {
                format!("{} %p{i}", ty.ll())
            }
        })
        .collect();
    let mut text = format!("define i64 @{name}({}) {{\nentry:\n", list.join(", "));
    text.push_str("  %e = xor i64 %k, 12345\n");
    text.push_str(prelude);
    writeln!(text, "  %c = {compare}").unwrap();
    if shape == Shape::Separated {
        text.push_str("  %d = icmp eq i64 %k, 77\n  %dz = zext i1 %d to i64\n");
    }
    let true_target = if shape == Shape::PhiEdge { "m" } else { "t" };
    writeln!(text, "  br i1 %c, label %{true_target}, label %f").unwrap();
    for block in layout {
        match block {
            "t" => {
                text.push_str("t:\n");
                match shape {
                    Shape::TwoUses => {
                        text.push_str("  %z = zext i1 %c to i64\n  %tv = add i64 %z, 10\n");
                    }
                    Shape::Separated => text.push_str("  %tv = add i64 %dz, 100\n"),
                    _ => text.push_str("  %tv = add i64 0, 1\n"),
                }
                text.push_str("  br label %m\n");
            }
            "f" => {
                text.push_str("f:\n");
                if shape == Shape::Separated {
                    text.push_str("  %fv = add i64 %dz, 200\n");
                } else {
                    text.push_str("  %fv = add i64 0, 2\n");
                }
                text.push_str("  br label %m\n");
            }
            _ => {
                text.push_str("m:\n");
                if shape == Shape::PhiEdge {
                    text.push_str("  %r = phi i64 [ %e, %entry ], [ %fv, %f ]\n");
                } else {
                    text.push_str("  %r = phi i64 [ %tv, %t ], [ %fv, %f ]\n");
                }
                text.push_str("  ret i64 %r\n");
            }
        }
    }
    if shape == Shape::PhiEdge {
        // The true block is unreachable in this shape: keep it out.
        text = text.replace("t:\n  %tv = add i64 0, 1\n  br label %m\n", "");
    }
    text.push_str("}\n");
    Func {
        name: name.to_owned(),
        params: params.to_vec(),
        ret: Ty::Int(64),
        text,
        interesting: vec![],
    }
}

/// Panics with a report of the first disagreements.
fn assert_agree(wrong: &[Wrong], funcs: &[Func]) {
    if let Some(first) = wrong.first() {
        let mut report = String::new();
        for w in wrong.iter().take(8) {
            writeln!(
                report,
                "{} call {} @{}: interpreter {} native {}",
                w.mode, w.call, w.func, w.want, w.got
            )
            .unwrap();
        }
        panic!("{} disagreements\n{report}\n{}", wrong.len(), text_of(funcs, &first.func));
    }
}

/// Values worth comparing at width `bits`: the edges and `extra`.
fn int_values(bits: u32, extra: &[u64]) -> Vec<u64> {
    let m = mask(bits);
    let top = 1u64 << (bits - 1);
    let mut v = vec![0, 1, m, m - 1, top, top - 1, (top + 1) & m, 2 & m];
    v.extend(extra.iter().map(|e| e & m));
    v
}

/// `icmp` of every predicate at every width and on pointers, with the
/// operands two registers, one register twice, a register and an
/// immediate either way round, and constants no `cmp`/`cmn` immediate
/// takes.
#[test]
#[allow(clippy::many_single_char_names, reason = "widths, masks, types")]
fn condbr_fuses_every_icmp() {
    let mut rng = Rng(66_001);
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    let types = [1u32, 2, 7, 8, 9, 16, 17, 31, 32, 33, 48, 63, 64]
        .map(Ty::Int)
        .into_iter()
        .chain([Ty::Ptr]);
    for ty in types {
        let bits = ty.bits();
        let m = mask(bits);
        let t = ty.ll();
        let big = 0x1234_5678_9abc_def1u64 & m;
        let neg = 5u64.wrapping_neg() & m;
        let small = 0x25 & m;
        let constant = |v: u64| match ty {
            Ty::Ptr if v == 0 => "null".to_owned(),
            Ty::Ptr => format!("inttoptr (i64 {v} to ptr)"),
            Ty::Int(1) => {
                if v & 1 == 1 {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            Ty::Int(b) => signed(b, v).to_string(),
        };
        // Pointer constant expressions are not what this test is about;
        // the pointer forms compare registers and null.
        let forms: Vec<(&str, String)> = if ty == Ty::Ptr {
            vec![("rr", "%p0, %p1".into()), ("rx", "%p0, %p0".into()), ("r0", "%p0, null".into())]
        } else {
            vec![
                ("rr", "%p0, %p1".into()),
                ("rx", "%p0, %p0".into()),
                ("ri", format!("%p0, {}", constant(small))),
                ("ir", format!("{}, %p1", constant(small))),
                ("rn", format!("%p0, {}", constant(neg))),
                ("rb", format!("%p0, {}", constant(big))),
            ]
        };
        let values = int_values(bits, &[small, neg, big, small + 1, big.wrapping_sub(1)]);
        for pred in INT_PREDS {
            for (form, operands) in &forms {
                // Every shape with the true block next, then every
                // layout for the plain fused shape.
                let mut cases: Vec<(Shape, usize)> = SHAPES.iter().map(|&s| (s, 0)).collect();
                cases.extend([(Shape::Fused, 1), (Shape::Fused, 2), (Shape::PhiEdge, 1)]);
                for (shape, layout) in cases {
                    let name = format!("i_{}_{pred}_{form}_{shape:?}_{layout}", t.replace(' ', ""));
                    let f = function(
                        &name,
                        &[ty, ty, Ty::Int(64)],
                        "",
                        &format!("icmp {pred} {t} {operands}"),
                        shape,
                        LAYOUTS[layout],
                    );
                    for _ in 0..6 {
                        let a = rng.pick(&values);
                        let b = if rng.chance(1, 4) { a } else { rng.pick(&values) };
                        let k = if rng.chance(1, 3) { 77 } else { rng.next() };
                        calls.push(Call { func: funcs.len(), args: vec![a, b, k] });
                    }
                    funcs.push(f);
                }
            }
        }
    }
    let Some(wrong) = check("a64-fused-icmp", &funcs, &calls, &MODES, true, 66_001) else {
        return;
    };
    assert_agree(&wrong, &funcs);
    eprintln!("a64 fused icmp: {} functions, {} calls agree", funcs.len(), calls.len());
}

/// A floating-point format: its name, the prelude making `%x` and `%y`
/// from the parameters, the parameters, and the bit patterns worth
/// comparing (one `u64` per parameter word).
struct Format {
    name: &'static str,
    prelude: &'static str,
    words: usize,
    values: Vec<Vec<u64>>,
}

fn formats() -> Vec<Format> {
    let one = |v: &[u64]| v.iter().map(|&w| vec![w]).collect::<Vec<_>>();
    vec![
        Format {
            name: "float",
            prelude: "  %t0 = trunc i64 %p0 to i32\n  %x = bitcast i32 %t0 to float\n  \
                      %t1 = trunc i64 %p1 to i32\n  %y = bitcast i32 %t1 to float\n",
            words: 1,
            values: one(&[
                0,
                0x8000_0000,
                0x3f80_0000,
                0xbf80_0000,
                0x4000_0000,
                0x7f80_0000,
                0xff80_0000,
                0x7fc0_0000,
                0x7fa0_0001,
                1,
                0x7f7f_ffff,
                0x3f80_0001,
            ]),
        },
        Format {
            name: "double",
            prelude: "  %x = bitcast i64 %p0 to double\n  %y = bitcast i64 %p1 to double\n",
            words: 1,
            values: one(&[
                0,
                0x8000_0000_0000_0000,
                0x3ff0_0000_0000_0000,
                0xbff0_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x7ff0_0000_0000_0000,
                0xfff0_0000_0000_0000,
                0x7ff8_0000_0000_0000,
                0x7ff4_0000_0000_0001,
                1,
                0x7fef_ffff_ffff_ffff,
                0x3ff0_0000_0000_0001,
            ]),
        },
    ]
}

/// `fcmp` of every predicate on `float` and `double`,
/// with zeros of both signs, infinities, quiet and signalling NaNs,
/// subnormals and neighbours; and a constant operand.
#[test]
fn condbr_fuses_every_fcmp() {
    let mut rng = Rng(66_002);
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    for format in formats() {
        let params: Vec<Ty> =
            std::iter::repeat_n(Ty::Int(64), 2 * format.words + 1).collect::<Vec<_>>();
        let constant = "1.0";
        for pred in FP_PREDS {
            let mut cases: Vec<(Shape, usize, bool)> =
                SHAPES.iter().map(|&s| (s, 0, false)).collect();
            cases.extend([
                (Shape::Fused, 1, false),
                (Shape::Fused, 2, false),
                (Shape::PhiEdge, 1, false),
                (Shape::Fused, 0, true),
                (Shape::Fused, 1, true),
            ]);
            for (shape, layout, with_const) in cases {
                let rhs = if with_const { constant } else { "%y" };
                let name =
                    format!("f_{}_{pred}_{shape:?}_{layout}_{}", format.name, u8::from(with_const));
                let f = function(
                    &name,
                    &params,
                    format.prelude,
                    &format!("fcmp {pred} {} %x, {rhs}", format.name),
                    shape,
                    LAYOUTS[layout],
                );
                for _ in 0..6 {
                    let a = format.values[rng.below(format.values.len())].clone();
                    let b = if rng.chance(1, 4) {
                        a.clone()
                    } else {
                        format.values[rng.below(format.values.len())].clone()
                    };
                    let mut args = a;
                    args.extend(b);
                    args.push(if rng.chance(1, 3) { 77 } else { rng.next() });
                    calls.push(Call { func: funcs.len(), args });
                }
                funcs.push(f);
            }
        }
    }
    let Some(wrong) = check("a64-fused-fcmp", &funcs, &calls, &MODES, true, 66_002) else {
        return;
    };
    assert_agree(&wrong, &funcs);
    eprintln!("a64 fused fcmp: {} functions, {} calls agree", funcs.len(), calls.len());
}

/// The fusion is real: a fused compare is a `cmp` or `fcmp` and a
/// `b.<cond>`, with no `cset` and no `tbnz` of the `i1`; one that is
/// separated from its branch is not fused.
#[test]
fn a_fused_condbr_has_no_materialized_i1() {
    let fused =
        function("f", &[Ty::Int(64); 3], "", "icmp slt i64 %p0, %p1", Shape::Fused, LAYOUTS[0]);
    let fp = function(
        "g",
        &[Ty::Int(64); 3],
        "  %x = bitcast i64 %p0 to double\n  %y = bitcast i64 %p1 to double\n",
        "fcmp one double %x, %y",
        Shape::Fused,
        LAYOUTS[0],
    );
    let separated =
        function("h", &[Ty::Int(64); 3], "", "icmp slt i64 %p0, %p1", Shape::Separated, LAYOUTS[0]);
    let dir = hod_testutil::Scratch::new("a64-fused-shape");
    for (f, want_fused) in [(&fused, true), (&fp, true), (&separated, false)] {
        let (module, bodies) = parse(&format!("{}{}", ll_header(22), f.text));
        let emitted = emit(&module, &bodies, &DEFAULT);
        let object = write_object(&dir, &format!("{}.o", f.name), &emitted);
        let Some(lines) = objdump(&object) else { return };
        let has = |m: &str| lines.iter().any(|l| l.starts_with(m));
        let conds = lines.iter().filter(|l| l.starts_with("b.")).count();
        if want_fused {
            assert!(
                !has("cset") && !has("tbnz") && !has("tbz"),
                "{}:\n{}",
                f.name,
                lines.join("\n")
            );
            assert_eq!(
                conds,
                if f.name == "g" { 2 } else { 1 },
                "{}:\n{}",
                f.name,
                lines.join("\n")
            );
        } else {
            assert!(has("tbnz") || has("tbz"), "{}:\n{}", f.name, lines.join("\n"));
        }
    }
}

/// An IEEE value by its bits, `exp` exponent bits over `mant` fraction
/// bits: `None` for a NaN, else a key whose order is the value's (both
/// zeros are 0).
fn ieee_key(bits: u128, exp: u32, mant: u32) -> Option<i128> {
    let sign = bits >> (exp + mant) & 1 != 0;
    let mag = bits & ((1u128 << (exp + mant)) - 1);
    let exp_all = ((1u128 << exp) - 1) << mant;
    if mag & exp_all == exp_all && mag & ((1u128 << mant) - 1) != 0 {
        return None;
    }
    let mag = mag as i128;
    Some(if sign { -mag } else { mag })
}

/// What `fcmp <pred>` gives on two keys of [`ieee_key`].
fn fcmp_holds(pred: &str, a: Option<i128>, b: Option<i128>) -> bool {
    let Some((a, b)) = a.zip(b) else {
        return matches!(pred, "true" | "ueq" | "ugt" | "uge" | "ult" | "ule" | "une" | "uno");
    };
    match pred {
        "false" | "uno" => false,
        "true" | "ord" => true,
        "oeq" | "ueq" => a == b,
        "ogt" | "ugt" => a > b,
        "oge" | "uge" => a >= b,
        "olt" | "ult" => a < b,
        "ole" | "ule" => a <= b,
        "one" | "une" => a != b,
        _ => unreachable!("{pred}"),
    }
}

/// What a function of [`function`] returns when its compare `holds`.
fn expected(shape: Shape, holds: bool, k: u64) -> u64 {
    let dz = u64::from(k == 77);
    match (shape, holds) {
        (Shape::Fused, true) => 1,
        (Shape::PhiEdge, true) => k ^ 0x3039,
        (Shape::TwoUses, true) => 11,
        (Shape::Separated, true) => dz + 100,
        (Shape::Separated, false) => dz + 200,
        (_, false) => 2,
    }
}

/// A format `hod_interp` does not model (`half`, `fp128`): its bit
/// layout and a constant operand, as `.ll` and as bits.
struct BitFormat {
    format: Format,
    exp: u32,
    mant: u32,
    constant: (&'static str, Vec<u64>),
}

/// `fcmp` of every predicate on `b`'s format, checked against the IEEE
/// table of [`fcmp_holds`] (`hod_interp` models neither `half` nor
/// `fp128`), with `llc` as the second oracle, under every linker.
#[allow(clippy::many_single_char_names, reason = "the names of the IEEE table")]
fn check_bit_format(seed: u64, b: &BitFormat) {
    let f = &b.format;
    let bits = |words: &[u64]| {
        let lo = u128::from(words[0]);
        if f.words == 2 { lo | u128::from(words[1]) << 64 } else { lo & 0xffff }
    };
    let mut rng = Rng(seed);
    let mut body = String::new();
    let mut calls: Vec<native::Call> = Vec::new();
    let mut want = String::new();
    let params = vec![Ty::Int(64); 2 * f.words + 1];
    for pred in FP_PREDS {
        let mut cases: Vec<(Shape, usize, bool)> = SHAPES.iter().map(|&s| (s, 0, false)).collect();
        cases.extend([
            (Shape::Fused, 1, false),
            (Shape::Fused, 2, false),
            (Shape::PhiEdge, 1, false),
        ]);
        cases.extend([(Shape::Fused, 0, true), (Shape::Fused, 1, true)]);
        for (shape, layout, with_const) in cases {
            let rhs = if with_const { b.constant.0 } else { "%y" };
            let name = format!("b_{}_{pred}_{shape:?}_{layout}_{}", f.name, u8::from(with_const));
            let func = function(
                &name,
                &params,
                f.prelude,
                &format!("fcmp {pred} {} %x, {rhs}", f.name),
                shape,
                LAYOUTS[layout],
            );
            body.push_str(&func.text);
            for _ in 0..8 {
                let x = f.values[rng.below(f.values.len())].clone();
                let y = if rng.chance(1, 4) {
                    x.clone()
                } else {
                    f.values[rng.below(f.values.len())].clone()
                };
                let k = if rng.chance(1, 3) { 77 } else { rng.next() };
                let mut args = x.clone();
                args.extend(&y);
                if f.words == 1 {
                    // Junk above the 16 bits the function reads.
                    args[0] |= rng.next() << 16;
                    args[1] |= rng.next() << 16;
                }
                args.push(k);
                args.resize(6, 0);
                let y = if with_const { &b.constant.1 } else { &y };
                let holds = fcmp_holds(
                    pred,
                    ieee_key(bits(&x), b.exp, b.mant),
                    ieee_key(bits(y), b.exp, b.mant),
                );
                calls.push((name.clone(), args.try_into().expect("six arguments")));
                writeln!(want, "{}", expected(shape, holds, k)).unwrap();
            }
        }
    }
    let modes = MODES.map(|m| (m.name, m.options));
    let label = format!("a64-fused-{}", f.name);
    let count = native::run_table(&label, &body, &calls, &modes, Some(&want));
    eprintln!("a64 fused {}: {count} calls agree", f.name);
}

/// `fcmp` of every predicate on `half` (converted to `float` first).
#[test]
fn condbr_fuses_every_half_fcmp() {
    check_bit_format(
        66_003,
        &BitFormat { format: half_format(), exp: 5, mant: 10, constant: ("0xH3C00", vec![0x3c00]) },
    );
}

/// `fcmp` of every predicate on `fp128`: calls of `__eqtf2`, `__lttf2`,
/// `__letf2`, `__unordtf2` before the branch (WP-067), after a phi
/// store and with a phi value in a register before them.
#[test]
fn condbr_fuses_every_fp128_fcmp() {
    check_bit_format(
        66_004,
        &BitFormat {
            format: fp128_format(),
            exp: 15,
            mant: 112,
            constant: ("0xL00000000000000003FFF000000000000", vec![0, 0x3fff_0000_0000_0000]),
        },
    );
}

fn half_format() -> Format {
    Format {
        name: "half",
        prelude: "  %t0 = trunc i64 %p0 to i16\n  %x = bitcast i16 %t0 to half\n  \
                      %t1 = trunc i64 %p1 to i16\n  %y = bitcast i16 %t1 to half\n",
        words: 1,
        values: [0u64, 0x8000, 0x3c00, 0xbc00, 0x4000, 0x7c00, 0xfc00, 0x7e00, 0x7d01, 1, 0x7bff]
            .iter()
            .map(|&w| vec![w])
            .collect(),
    }
}

fn fp128_format() -> Format {
    Format {
        name: "fp128",
        prelude: "  %l0 = zext i64 %p0 to i128\n  %h0 = zext i64 %p1 to i128\n  \
                      %s0 = shl i128 %h0, 64\n  %w0 = or i128 %s0, %l0\n  \
                      %x = bitcast i128 %w0 to fp128\n  \
                      %l1 = zext i64 %p2 to i128\n  %h1 = zext i64 %p3 to i128\n  \
                      %s1 = shl i128 %h1, 64\n  %w1 = or i128 %s1, %l1\n  \
                      %y = bitcast i128 %w1 to fp128\n",
        words: 2,
        values: [
            (0, 0),
            (0, 0x8000_0000_0000_0000),
            (0, 0x3fff_0000_0000_0000),
            (0, 0xbfff_0000_0000_0000),
            (0, 0x4000_0000_0000_0000),
            (0, 0x7fff_0000_0000_0000),
            (0, 0xffff_0000_0000_0000),
            (0, 0x7fff_8000_0000_0000),
            (1, 0x7fff_0000_0000_0000),
            (1, 0),
            (1, 0x3fff_0000_0000_0000),
        ]
        .iter()
        .map(|&(lo, hi)| vec![lo, hi])
        .collect(),
    }
}
