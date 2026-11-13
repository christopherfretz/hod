//! `phi` and `select` of every value class (WP-066; `spec/codegen.md`
//! §4.5, §2.2): random diamonds and loops over values of class `Gpr`,
//! `Fpr` (`half`, `float`, `double`, `fp128`, vectors) and `Mem` (`i128`,
//! `i256`, aggregates), with critical edges, a `phi` that reads another
//! `phi` of its block, rotations and swap cycles of any length on a back
//! edge, constant incoming values and `select` of every class. A function
//! folds what it ends with into an `i64`. The types `hod_interp` models
//! are checked against it and `llc -O0 -global-isel=0 -fast-isel=0`
//! (`common::control`'s batch check); the others against `llc` under
//! every linker (`common::native`).
//!
//! `HOD_PHI_SEEDS=<n>` sets the batches of `many_phi_graphs_agree`.

mod common;
#[path = "common/control.rs"]
#[allow(dead_code, reason = "only the batch check is used here")]
mod graphs;
#[path = "common/native.rs"]
mod native;

use std::fmt::Write as _;

use graphs::{Call, Func, Mode, Rng, Ty, check, text_of};
use hod_cg::{Options, RelocModel, Stress};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

const MODES: [Mode; 3] = [
    Mode { name: "plain", options: DEFAULT },
    Mode { name: "stress", options: Options { stress: Stress::ALL, ..DEFAULT } },
    Mode {
        name: "far",
        options: Options {
            stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
            ..DEFAULT
        },
    },
];

/// A value type: its spelling, a constant of it, and whether
/// `hod_interp` models it.
#[derive(Clone, Copy, Debug)]
struct Kind {
    ll: &'static str,
    constant: &'static str,
    interp: bool,
}

const KINDS: [Kind; 15] = [
    Kind { ll: "i64", constant: "-81985529216486896", interp: true },
    Kind { ll: "i33", constant: "-3", interp: true },
    Kind { ll: "i128", constant: "-170141183460469231731687303715884105727", interp: true },
    Kind { ll: "i256", constant: "12345678901234567890123456789012345678901", interp: false },
    Kind { ll: "half", constant: "0xH7E01", interp: false },
    Kind { ll: "float", constant: "-0.0", interp: true },
    Kind { ll: "double", constant: "0x7FF4000000000001", interp: true },
    Kind { ll: "fp128", constant: "0xL0000000000000001FFFF800000000000", interp: false },
    Kind { ll: "<4 x i32>", constant: "<i32 1, i32 -2, i32 3, i32 -4>", interp: false },
    Kind { ll: "<2 x double>", constant: "<double 1.5, double -0.0>", interp: false },
    Kind {
        ll: "<8 x i8>",
        constant: "<i8 1, i8 2, i8 3, i8 4, i8 5, i8 6, i8 7, i8 -8>",
        interp: false,
    },
    Kind { ll: "{ i32, double }", constant: "{ i32 -7, double 2.5 }", interp: true },
    Kind { ll: "[3 x i16]", constant: "[i16 1, i16 -2, i16 300]", interp: true },
    Kind {
        ll: "{ i8, fp128 }",
        constant: "{ i8 9, fp128 0xL00000000000000003FFF000000000000 }",
        interp: false,
    },
    Kind { ll: "{ float, i128 }", constant: "{ float 1.0, i128 -5 }", interp: true },
];

/// Writes instructions that define `%{to}` of `kind` from the `i64`
/// values `%{a}` and `%{b}`, with names prefixed by `to`.
fn make(out: &mut String, kind: Kind, to: &str, a: &str, b: &str) {
    let t = kind.ll;
    let wide = |out: &mut String, bits: u32| {
        writeln!(
            out,
            "  %{to}.l = zext i64 %{a} to i{bits}\n  %{to}.h = zext i64 %{b} to i{bits}\n  \
             %{to}.s = shl i{bits} %{to}.h, {}\n  %{to}.w = or i{bits} %{to}.s, %{to}.l",
            bits / 2
        )
        .unwrap();
    };
    match t {
        "i64" => writeln!(out, "  %{to} = xor i64 %{a}, %{b}").unwrap(),
        "i33" => {
            writeln!(out, "  %{to}.x = xor i64 %{a}, %{b}\n  %{to} = trunc i64 %{to}.x to i33")
                .unwrap();
        }
        "i128" | "i256" => {
            let bits = if t == "i128" { 128 } else { 256 };
            wide(out, bits);
            writeln!(out, "  %{to} = add {t} %{to}.w, 0").unwrap();
        }
        "half" => writeln!(
            out,
            "  %{to}.t = trunc i64 %{a} to i16\n  %{to} = bitcast i16 %{to}.t to half"
        )
        .unwrap(),
        "float" => writeln!(
            out,
            "  %{to}.t = trunc i64 %{a} to i32\n  %{to} = bitcast i32 %{to}.t to float"
        )
        .unwrap(),
        "double" => writeln!(out, "  %{to} = bitcast i64 %{a} to double").unwrap(),
        "fp128" | "<4 x i32>" | "<2 x double>" => {
            wide(out, 128);
            writeln!(out, "  %{to} = bitcast i128 %{to}.w to {t}").unwrap();
        }
        "<8 x i8>" => writeln!(out, "  %{to} = bitcast i64 %{a} to <8 x i8>").unwrap(),
        "{ i32, double }" => writeln!(
            out,
            "  %{to}.t = trunc i64 %{a} to i32\n  %{to}.d = bitcast i64 %{b} to double\n  \
             %{to}.0 = insertvalue {t} poison, i32 %{to}.t, 0\n  \
             %{to} = insertvalue {t} %{to}.0, double %{to}.d, 1"
        )
        .unwrap(),
        "[3 x i16]" => writeln!(
            out,
            "  %{to}.t0 = trunc i64 %{a} to i16\n  %{to}.t1 = trunc i64 %{b} to i16\n  \
             %{to}.r = lshr i64 %{a}, 16\n  %{to}.t2 = trunc i64 %{to}.r to i16\n  \
             %{to}.0 = insertvalue {t} poison, i16 %{to}.t0, 0\n  \
             %{to}.1 = insertvalue {t} %{to}.0, i16 %{to}.t1, 1\n  \
             %{to} = insertvalue {t} %{to}.1, i16 %{to}.t2, 2"
        )
        .unwrap(),
        "{ i8, fp128 }" => {
            wide(out, 128);
            writeln!(
                out,
                "  %{to}.q = bitcast i128 %{to}.w to fp128\n  %{to}.t = trunc i64 %{b} to i8\n  \
                 %{to}.0 = insertvalue {t} poison, i8 %{to}.t, 0\n  \
                 %{to} = insertvalue {t} %{to}.0, fp128 %{to}.q, 1"
            )
            .unwrap();
        }
        "{ float, i128 }" => {
            wide(out, 128);
            writeln!(
                out,
                "  %{to}.t = trunc i64 %{b} to i32\n  %{to}.f = bitcast i32 %{to}.t to float\n  \
                 %{to}.0 = insertvalue {t} poison, float %{to}.f, 0\n  \
                 %{to} = insertvalue {t} %{to}.0, i128 %{to}.w, 1"
            )
            .unwrap();
        }
        _ => unreachable!("{t}"),
    }
}

/// Folds the `iN` value `%{v}` into the `i64` `%{to}`.
fn fold_int(out: &mut String, bits: u32, to: &str, v: &str) {
    if bits <= 64 {
        let op = if bits < 64 { "zext" } else { "add" };
        if bits < 64 {
            writeln!(out, "  %{to} = {op} i{bits} %{v} to i64").unwrap();
        } else {
            writeln!(out, "  %{to} = add i64 %{v}, 0").unwrap();
        }
        return;
    }
    writeln!(out, "  %{to}.c0 = trunc i{bits} %{v} to i64").unwrap();
    let mut last = format!("{to}.c0");
    for chunk in 1..bits / 64 {
        writeln!(
            out,
            "  %{to}.s{chunk} = lshr i{bits} %{v}, {}\n  %{to}.c{chunk} = trunc i{bits} %{to}.s{chunk} to i64\n  \
             %{to}.m{chunk} = mul i64 %{last}, 1000003\n  %{to}.x{chunk} = xor i64 %{to}.m{chunk}, %{to}.c{chunk}",
            chunk * 64
        )
        .unwrap();
        last = format!("{to}.x{chunk}");
    }
    writeln!(out, "  %{to} = add i64 %{last}, 0").unwrap();
}

/// Folds `%{v}` of `kind` into the `i64` `%{to}`: every bit counts.
fn fold(out: &mut String, kind: Kind, to: &str, v: &str) {
    let t = kind.ll;
    match t {
        "i64" => fold_int(out, 64, to, v),
        "i33" => fold_int(out, 33, to, v),
        "i128" => fold_int(out, 128, to, v),
        "i256" => fold_int(out, 256, to, v),
        "half" | "float" | "double" => {
            let bits = match t {
                "half" => 16,
                "float" => 32,
                _ => 64,
            };
            writeln!(out, "  %{to}.b = bitcast {t} %{v} to i{bits}").unwrap();
            fold_int(out, bits, to, &format!("{to}.b"));
        }
        "fp128" | "<4 x i32>" | "<2 x double>" => {
            writeln!(out, "  %{to}.b = bitcast {t} %{v} to i128").unwrap();
            fold_int(out, 128, to, &format!("{to}.b"));
        }
        "<8 x i8>" => writeln!(out, "  %{to} = bitcast <8 x i8> %{v} to i64").unwrap(),
        "{ i32, double }" => writeln!(
            out,
            "  %{to}.e0 = extractvalue {t} %{v}, 0\n  %{to}.e1 = extractvalue {t} %{v}, 1\n  \
             %{to}.z = zext i32 %{to}.e0 to i64\n  %{to}.b = bitcast double %{to}.e1 to i64\n  \
             %{to}.m = mul i64 %{to}.z, 1000003\n  %{to} = xor i64 %{to}.m, %{to}.b"
        )
        .unwrap(),
        "[3 x i16]" => {
            writeln!(out, "  %{to}.a = add i64 0, 0").unwrap();
            let mut last = format!("{to}.a");
            for i in 0..3 {
                writeln!(
                    out,
                    "  %{to}.e{i} = extractvalue {t} %{v}, {i}\n  %{to}.z{i} = zext i16 %{to}.e{i} to i64\n  \
                     %{to}.m{i} = mul i64 %{last}, 65599\n  %{to}.x{i} = xor i64 %{to}.m{i}, %{to}.z{i}"
                )
                .unwrap();
                last = format!("{to}.x{i}");
            }
            writeln!(out, "  %{to} = add i64 %{last}, 0").unwrap();
        }
        "{ i8, fp128 }" => {
            writeln!(
                out,
                "  %{to}.e0 = extractvalue {t} %{v}, 0\n  %{to}.e1 = extractvalue {t} %{v}, 1\n  \
                 %{to}.z = zext i8 %{to}.e0 to i64\n  %{to}.b = bitcast fp128 %{to}.e1 to i128"
            )
            .unwrap();
            fold_int(out, 128, &format!("{to}.f"), &format!("{to}.b"));
            writeln!(out, "  %{to}.m = mul i64 %{to}.f, 257\n  %{to} = xor i64 %{to}.m, %{to}.z")
                .unwrap();
        }
        "{ float, i128 }" => {
            writeln!(
                out,
                "  %{to}.e0 = extractvalue {t} %{v}, 0\n  %{to}.e1 = extractvalue {t} %{v}, 1\n  \
                 %{to}.b = bitcast float %{to}.e0 to i32\n  %{to}.z = zext i32 %{to}.b to i64"
            )
            .unwrap();
            fold_int(out, 128, &format!("{to}.f"), &format!("{to}.e1"));
            writeln!(out, "  %{to}.m = mul i64 %{to}.f, 257\n  %{to} = xor i64 %{to}.m, %{to}.z")
                .unwrap();
        }
        _ => unreachable!("{t}"),
    }
}

/// One random function over `kind`: a diamond with two critical edges
/// into its join block `m`, a self loop `h` whose `phi`s are rotated by a
/// random permutation on the back edge, and selects.
#[allow(clippy::many_single_char_names, reason = "block names")]
fn function(rng: &mut Rng, name: &str, kind: Kind) -> Func {
    let t = kind.ll;
    let c = kind.constant;
    let mut entry = String::new();
    for (i, (a, b)) in [("s0", "s1"), ("s1", "s2"), ("s2", "s3"), ("s3", "s0")].iter().enumerate() {
        make(&mut entry, kind, &format!("v{i}"), a, b);
    }
    entry.push_str(
        "  %c0 = trunc i64 %p to i1\n  %q1 = lshr i64 %p, 1\n  %c1 = trunc i64 %q1 to i1\n  \
         %q5 = lshr i64 %p, 5\n  %c3 = trunc i64 %q5 to i1\n",
    );
    let pick = |rng: &mut Rng, pool: &[&str]| -> String {
        if rng.chance(1, 6) { c.to_owned() } else { format!("%{}", rng.pick(pool)) }
    };
    // The blocks after the entry block, in a random order.
    let mut blocks: Vec<String> = Vec::new();
    let sa = format!(
        "  %sa = select i1 %c1, {t} {}, {t} {}\n",
        pick(rng, &["v2", "v3"]),
        pick(rng, &["v0", "v3"])
    );
    blocks.push(format!("a:\n{sa}  br i1 %c1, label %m, label %b\n"));
    blocks.push("b:\n  br label %m\n".to_owned());
    let mut m = String::from("m:\n");
    for phi in ["x", "y", "z"] {
        writeln!(
            m,
            "  %{phi} = phi {t} [ {}, %entry ], [ {}, %a ], [ {}, %b ]",
            pick(rng, &["v0", "v1", "v2", "v3"]),
            pick(rng, &["v0", "v1", "sa"]),
            pick(rng, &["v2", "v3", "sa"]),
        )
        .unwrap();
    }
    m.push_str("  br label %h\n");
    blocks.push(m);
    let n = 2 + rng.below(4);
    let mut perm: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        perm.swap(i, rng.below(i + 1));
    }
    let mut h = String::from("h:\n  %i = phi i64 [ 0, %m ], [ %i1, %h ]\n");
    for (j, &from) in perm.iter().enumerate() {
        let back = if rng.chance(1, 8) { c.to_owned() } else { format!("%l{from}") };
        writeln!(
            h,
            "  %l{j} = phi {t} [ {}, %m ], [ {back}, %h ]",
            pick(rng, &["x", "y", "z", "v0"])
        )
        .unwrap();
    }
    h.push_str(
        "  %i1 = add i64 %i, 1\n  %q2 = lshr i64 %p, 2\n  %n = and i64 %q2, 7\n  \
         %done = icmp uge i64 %i1, %n\n  br i1 %done, label %e, label %h\n",
    );
    blocks.push(h);
    let mut e = String::from("e:\n");
    writeln!(e, "  %sel = select i1 %c3, {t} %l0, {t} %l{}", n - 1).unwrap();
    writeln!(e, "  %sel2 = select i1 %c0, {t} %sel, {t} {c}").unwrap();
    let mut folded = vec!["sel2".to_owned(), "x".to_owned(), "y".to_owned(), "z".to_owned()];
    folded.extend((0..n).map(|j| format!("l{j}")));
    e.push_str("  %h0 = add i64 0, 0\n");
    for (i, v) in folded.iter().enumerate() {
        fold(&mut e, kind, &format!("f{i}"), v);
        writeln!(e, "  %hm{i} = mul i64 %h{i}, 31\n  %h{} = xor i64 %hm{i}, %f{i}", i + 1).unwrap();
    }
    writeln!(e, "  ret i64 %h{}", folded.len()).unwrap();
    blocks.push(e);
    for i in (1..blocks.len()).rev() {
        blocks.swap(i, rng.below(i + 1));
    }
    let mut text = format!(
        "define i64 @{name}(i64 %s0, i64 %s1, i64 %s2, i64 %s3, i64 %p) {{\nentry:\n{entry}  \
         br i1 %c0, label %a, label %m\n"
    );
    for b in blocks {
        text.push_str(&b);
    }
    text.push_str("}\n");
    Func {
        name: name.to_owned(),
        params: vec![Ty::Int(64); 5],
        ret: Ty::Int(64),
        text,
        interesting: vec![],
    }
}

/// Seeds that make interesting values of every type.
fn seed_value(rng: &mut Rng) -> u64 {
    match rng.below(6) {
        0 => 0,
        1 => u64::MAX,
        2 => 0x7ff8_0000_0000_0001,
        3 => 0x8000_0000_0000_0000,
        _ => rng.next(),
    }
}

/// `batches` batches of `count` functions from `first`, `per` calls each:
/// interpreter and `llc` for the types `hod_interp` models, `llc` for
/// the others. Returns the functions and calls checked.
fn check_batches(first: u64, batches: u64, count: usize, per: usize) -> (usize, usize) {
    let mut checked = (0, 0);
    for seed in first..first + batches {
        let mut rng = Rng(seed);
        let mut interp_funcs = Vec::new();
        let mut interp_calls = Vec::new();
        let mut body = String::new();
        let mut native_calls: Vec<native::Call> = Vec::new();
        for i in 0..count {
            let kind = KINDS[i % KINDS.len()];
            let f = function(&mut rng, &format!("p{seed}_{i}"), kind);
            for _ in 0..per {
                let args: Vec<u64> = (0..5)
                    .map(|k| if k == 4 { rng.next() } else { seed_value(&mut rng) })
                    .collect();
                if kind.interp {
                    interp_calls.push(Call { func: interp_funcs.len(), args });
                } else {
                    let mut six = [0u64; 6];
                    six[..5].copy_from_slice(&args);
                    native_calls.push((f.name.clone(), six));
                }
            }
            if kind.interp {
                interp_funcs.push(f);
            } else {
                body.push_str(&f.text);
            }
        }
        if let Some(wrong) = check("a64-phis", &interp_funcs, &interp_calls, &MODES, true, seed)
            && let Some(first) = wrong.first()
        {
            panic!(
                "{} disagreements; first {} call {} @{}: interpreter {} native {}\n{}",
                wrong.len(),
                first.mode,
                first.call,
                first.func,
                first.want,
                first.got,
                text_of(&interp_funcs, &first.func)
            );
        }
        let modes = MODES.map(|m| (m.name, m.options));
        native::run_table("a64-phis-llc", &body, &native_calls, &modes, None);
        checked.0 += count;
        checked.1 += interp_calls.len() + native_calls.len();
    }
    checked
}

/// Phis and selects of every class, a few hundred functions.
#[test]
fn phis_and_selects_of_every_class_agree() {
    let (funcs, calls) = check_batches(1, 2, 150, 6);
    eprintln!("a64 phis: {funcs} functions, {calls} calls agree");
}

/// Many more.
#[test]
#[ignore = "minutes; run with --ignored"]
fn many_phi_graphs_agree() {
    let batches: u64 =
        std::env::var("HOD_PHI_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(20);
    let (funcs, calls) = check_batches(1_000, batches, 300, 6);
    eprintln!("a64 phis: {funcs} functions, {calls} calls agree");
}
