//! Vectors of `i128` across calls on x86-64 (WP-171, `spec/abi-x64.md`
//! §4.3, `spec/codegen.md` §5.6.20): the signature family WP-167 wrote
//! for AArch64 (`crates/hod_a64/tests/vcall.rs`,
//! `lone_i128_vectors_cross_calls`), the same 81 signatures from the same
//! seeds, on x86-64 under the emulator.
//!
//! Every case is a signature. Its callee `f_K` stores each leaf of each
//! argument (`extractvalue`) to a global of its own and returns a value
//! built (`insertvalue`) from leaves loaded from globals; its caller
//! `call_K` loads each leaf of each argument, builds the arguments, calls
//! and stores each leaf of the result. A C driver fills the input globals
//! with random bytes, calls every `call_K` and prints what went in and
//! what came out. Every leaf must arrive as it left, byte for byte.
//!
//! Each case is first compiled alone by hod, its callers' module and its
//! callee's (the 162 one-case modules); the result of each is counted by
//! name. A case hod refuses as `x64:vector-i128-split` must be one `llc`
//! 19 and 22 place differently: its `llc` 19 caller against the `llc` 22
//! callee, or the reverse, must lose a leaf. The cases hod builds are
//! then linked as one program per pairing: hod (plain, and under every
//! stress option) against `llc -O0 -fast-isel=0` 22 both ways under bfd,
//! lld and mold, hod stressed against `llc` 19 both ways, hod against
//! hod, and `llc` against itself, each run under `qemu-x86_64`.
//!
//! The call styles: a direct `call`, a call through a pointer, an
//! `invoke`, and a `musttail` forwarder in the caller's module; a
//! variadic callee takes its named arguments (any shape) and hands its
//! `va_list` to a C walker that takes the rest with `va_arg`.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::format_collect,
    clippy::many_single_char_names,
    reason = "a test generator: `.ll` and C text built by hand"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use float_gen::{ll_header, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};

/// The bytes of every global the driver defines.
const SLOT: usize = 64;
/// How many times the driver runs every case, with fresh values.
const ROUNDS: u32 = 4;

// ---- types ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I8,
    I16,
    I32,
    I64,
    I128,
    Half,
    Float,
    Double,
    /// A pointer (WP-174: `<1 x ptr>`).
    Ptr,
}

impl E {
    fn ll(self) -> &'static str {
        match self {
            E::I8 => "i8",
            E::I16 => "i16",
            E::I32 => "i32",
            E::I64 => "i64",
            E::I128 => "i128",
            E::Half => "half",
            E::Float => "float",
            E::Double => "double",
            E::Ptr => "ptr",
        }
    }

    fn bits(self) -> u32 {
        match self {
            E::I8 => 8,
            E::I16 | E::Half => 16,
            E::I32 | E::Float => 32,
            E::I64 | E::Double | E::Ptr => 64,
            E::I128 => 128,
        }
    }

    /// The C type of an element, where C has one (variable arguments).
    fn c(self) -> Option<&'static str> {
        Some(match self {
            E::I8 => "signed char",
            E::I16 => "short",
            E::I32 => "int",
            E::I64 => "long",
            E::Float => "float",
            E::Double => "double",
            E::Ptr => "void *",
            E::I128 | E::Half => return None,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Ty {
    /// A scalar.
    S(E),
    /// A vector.
    V(E, u32),
    /// A struct.
    St(Vec<Ty>),
}

use Ty::{S, St, V};

impl Ty {
    fn ll(&self) -> String {
        match self {
            S(e) => e.ll().into(),
            V(e, n) => format!("<{n} x {}>", e.ll()),
            St(fields) => {
                format!("{{ {} }}", fields.iter().map(Ty::ll).collect::<Vec<_>>().join(", "))
            }
        }
    }

    /// The leaves (scalars and vectors) with their `extractvalue` paths.
    fn leaves(&self) -> Vec<(Vec<u32>, Ty)> {
        match self {
            S(_) | V(..) => vec![(Vec::new(), self.clone())],
            St(fields) => fields
                .iter()
                .enumerate()
                .flat_map(|(i, f)| {
                    f.leaves().into_iter().map(move |(mut p, t)| {
                        p.insert(0, i as u32);
                        (p, t)
                    })
                })
                .collect(),
        }
    }

    /// The bytes a leaf's store writes.
    fn bytes(&self) -> usize {
        match self {
            S(e) => e.bits() as usize / 8,
            V(e, n) => (n * e.bits()).div_ceil(8) as usize,
            St(_) => unreachable!("not a leaf"),
        }
    }

    /// The C type of a variable argument: a scalar, or a 16-byte vector.
    fn c_leaf(&self) -> String {
        match self {
            S(e) => e.c().unwrap().to_owned(),
            V(e, n) => format!("vc_{}x{n}", e.ll()),
            St(_) => unreachable!("no variable aggregates"),
        }
    }
}

// ---- cases ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    Direct,
    Indirect,
    Invoke,
    Musttail,
}

#[derive(Clone, Debug)]
struct Case {
    params: Vec<Ty>,
    ret: Option<Ty>,
    style: Style,
    /// For a variadic callee, the variable arguments (C-spellable).
    var: Option<Vec<Ty>>,
}

impl Case {
    fn new(params: Vec<Ty>, ret: Option<Ty>) -> Case {
        Case { params, ret, style: Style::Direct, var: None }
    }

    fn ret_ll(&self) -> String {
        self.ret.as_ref().map_or_else(|| "void".into(), Ty::ll)
    }

    fn fn_type(&self) -> String {
        let mut params: Vec<String> = self.params.iter().map(Ty::ll).collect();
        if self.var.is_some() {
            params.push("...".into());
        }
        format!("{} ({})", self.ret_ll(), params.join(", "))
    }

    /// The leaves the driver compares.
    fn leaf_count(&self) -> usize {
        let count = |ts: &[Ty]| ts.iter().map(|t| t.leaves().len()).sum::<usize>();
        count(&self.params) + count(self.var.as_deref().unwrap_or(&[])) + count(self.ret.as_slice())
    }
}

/// WP-167's generator, step for step, so that the signatures are its.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }
}

/// Vectors passed as bytes: legal ones, and those wider than a register
/// or of an odd length.
fn bytes() -> Vec<Ty> {
    vec![
        V(E::I32, 4),
        V(E::Float, 2),
        V(E::Double, 2),
        V(E::I8, 16),
        V(E::Half, 4),
        V(E::I64, 1),
        V(E::Double, 1),
        V(E::Float, 3),
        V(E::I32, 3),
        V(E::I32, 5),
        V(E::I32, 8),
        V(E::I64, 3),
        V(E::Half, 3),
        V(E::Float, 16),
        V(E::I16, 12),
    ]
}

/// A lone `<1 x i128>`, the longer `i128` vectors and the scalar.
fn wide() -> Vec<Ty> {
    vec![V(E::I128, 1), V(E::I128, 2), V(E::I128, 3), S(E::I128)]
}

fn scalars() -> Vec<Ty> {
    vec![S(E::I32), S(E::I64), S(E::Float), S(E::Double)]
}

/// Random signatures over `pool`; a quarter of them first fill the vector
/// or the general argument registers.
fn random(rng: &mut Rng, pool: &[Ty], count: usize) -> Vec<Case> {
    (0..count)
        .map(|_| {
            let mut params = Vec::new();
            match rng.below(8) {
                0 => params.extend((0..8).map(|_| V(E::I32, 4))),
                1 => params.extend((0..8).map(|_| S(E::I64))),
                2 => params.extend((0..7).map(|_| S(E::Double))),
                _ => {}
            }
            let n = 1 + rng.below(6);
            params.extend((0..n).map(|_| rng.pick(pool)));
            let ret = (rng.below(4) != 0).then(|| rng.pick(pool));
            Case::new(params, ret)
        })
        .collect()
}

/// Every leaf of `pool` by itself as an argument and as the result, and
/// after the argument registers are full.
fn each(pool: &[Ty]) -> Vec<Case> {
    let mut out = Vec::new();
    for t in pool {
        out.push(Case::new(vec![t.clone()], Some(t.clone())));
        let mut full: Vec<Ty> = (0..8).map(|_| V(E::I32, 4)).collect();
        full.extend((0..8).map(|_| S(E::I64)));
        full.push(t.clone());
        full.push(S(E::I32));
        full.push(t.clone());
        out.push(Case::new(full, None));
    }
    out
}

/// WP-167's 81 signatures.
fn wp167_cases() -> Vec<Case> {
    let v1 = V(E::I128, 1);
    let mut cases = each(&wide());
    for before in 0..=8 {
        let mut params: Vec<Ty> = (0..before).map(|_| S(E::I64)).collect();
        params.push(v1.clone());
        params.push(S(E::I32));
        params.push(v1.clone());
        cases.push(Case::new(params, Some(v1.clone())));
    }
    cases.push(Case::new(vec![V(E::I32, 4), v1.clone(), S(E::Double), v1.clone()], None));
    cases.push(Case::new(
        vec![St(vec![S(E::I64), v1.clone()]), v1.clone()],
        Some(St(vec![v1.clone()])),
    ));
    let mut rng = Rng(0x0167_0001);
    let mut pool = wide();
    pool.push(v1.clone());
    pool.extend(bytes());
    pool.extend(scalars());
    cases.extend(random(&mut rng, &pool, 40));
    let mut styled = random(&mut rng, &pool, 18);
    for (k, case) in styled.iter_mut().enumerate() {
        case.params.push(v1.clone());
        case.style = [Style::Indirect, Style::Invoke, Style::Musttail][k % 3];
        if case.style == Style::Musttail || k % 2 == 0 {
            case.ret = Some(v1.clone());
        }
    }
    cases.extend(styled);
    for before in [0, 1, 7, 8] {
        let mut params: Vec<Ty> = (0..before).map(|_| S(E::I64)).collect();
        params.push(v1.clone());
        let var = vec![S(E::I64), V(E::I32, 4), S(E::I32)];
        cases.push(Case { params, ret: None, style: Style::Direct, var: Some(var) });
    }
    cases
}

/// §4.3's placements one by one: every `i128` vector of 1 to 4 elements
/// after 0 to 10 general arguments (an `i32` or an `i64`; past six, on
/// the stack at offsets 0 mod 16 and 8 mod 16), followed by an `i64`, a
/// `double` and a second vector, around full vector registers and with an
/// `i128` before it.
fn placement_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for n in 1..=4 {
        let v = V(E::I128, n);
        for before in 0..=10 {
            let int = if before % 2 == 0 { E::I64 } else { E::I32 };
            let mut params: Vec<Ty> = (0..before).map(|_| S(int)).collect();
            params.extend([v.clone(), S(E::I64), S(E::Double), V(E::I128, 1 + before % 3)]);
            params.push(S(E::I64));
            cases.push(Case::new(params, (n == 1).then(|| v.clone())));
        }
        let mut params: Vec<Ty> = (0..8).map(|_| S(E::Double)).collect();
        params.extend([S(E::I128), v.clone(), S(E::I32)]);
        cases.push(Case::new(params, None));
        let mut params = vec![S(E::I64), S(E::I128), S(E::I64)];
        params.extend([v.clone(), V(E::I32, 4), v.clone()]);
        cases.push(Case::new(params, Some(V(E::I128, 1))));
    }
    cases
}

/// WP-174: a lone `<1 x i64>` or `<1 x ptr>` (a general piece whose
/// value is an `xmm`) by itself, after 0 to 8 general arguments (in
/// registers and on the stack), around `double`, `<1 x double>` and full
/// vector registers, beside an `i128` pair and the byte vectors, in a
/// structure; through a pointer, `invoke` and `musttail`; as a named
/// argument of a variadic callee; and random signatures over them.
fn lone_i64_cases() -> Vec<Case> {
    let lones = [V(E::I64, 1), V(E::Ptr, 1)];
    let mut cases = each(&lones);
    for v in &lones {
        for before in 0..=8 {
            let mut params: Vec<Ty> = (0..before).map(|_| S(E::I64)).collect();
            params.extend([v.clone(), S(E::Double), V(E::Double, 1), v.clone(), S(E::I32)]);
            cases.push(Case::new(params, Some(v.clone())));
        }
        let mut params: Vec<Ty> = (0..5).map(|_| S(E::I64)).collect();
        params.extend([v.clone(), S(E::I128), v.clone()]);
        cases.push(Case::new(params, Some(V(E::I64, 1))));
        cases.push(Case::new(
            vec![St(vec![S(E::I64), v.clone()]), v.clone(), St(vec![v.clone(), S(E::Double)])],
            Some(St(vec![v.clone()])),
        ));
        for before in [0, 5, 6, 8] {
            let mut params: Vec<Ty> = (0..before).map(|_| S(E::I64)).collect();
            params.push(v.clone());
            let var = vec![S(E::I64), V(E::I32, 4), S(E::Ptr), S(E::I32)];
            cases.push(Case { params, ret: None, style: Style::Direct, var: Some(var) });
        }
    }
    let mut rng = Rng(0x0174_0001);
    let mut pool = lones.to_vec();
    pool.extend(lones.iter().cloned());
    // Not the vectors passed element by element in general pieces
    // (`<5 x i32>`, `<3 x i64>`, `<12 x i16>`): where their pieces do not
    // all fit in registers `llc` 19 and 22 place them differently
    // (`spec/abi-x64.md` §4.2, "LLVM 19").
    let split = [V(E::I32, 5), V(E::I64, 3), V(E::I16, 12)];
    pool.extend(bytes().into_iter().filter(|t| !split.contains(t)));
    pool.extend(scalars());
    pool.push(S(E::Ptr));
    cases.extend(random(&mut rng, &pool, 40));
    let mut styled = random(&mut rng, &pool, 18);
    for (k, case) in styled.iter_mut().enumerate() {
        case.params.push(lones[k % 2].clone());
        case.style = [Style::Indirect, Style::Invoke, Style::Musttail][k % 3];
        if case.style == Style::Musttail || k % 2 == 0 {
            case.ret = Some(lones[(k / 2) % 2].clone());
        }
    }
    cases.extend(styled);
    cases
}

// ---- IR ----

fn slot(prefix: &str, k: usize, i: usize, j: usize) -> String {
    format!("vc_{prefix}_{k}_{i}_{j}")
}

/// Loads the leaves of `t` from the globals `prefix_k_i_*` and builds the
/// value; returns its name.
fn build(out: &mut String, n: &mut u32, t: &Ty, prefix: &str, k: usize, i: usize) -> String {
    let leaves = t.leaves();
    if let [(path, leaf)] = &leaves[..]
        && path.is_empty()
    {
        *n += 1;
        writeln!(out, "  %t{n} = load {}, ptr @{}", leaf.ll(), slot(prefix, k, i, 0)).unwrap();
        return format!("%t{n}");
    }
    let mut value = "poison".to_owned();
    for (j, (path, leaf)) in leaves.iter().enumerate() {
        *n += 1;
        let x = format!("%t{n}");
        writeln!(out, "  {x} = load {}, ptr @{}", leaf.ll(), slot(prefix, k, i, j)).unwrap();
        *n += 1;
        let indices: String = path.iter().map(|p| format!(", {p}")).collect();
        writeln!(out, "  %t{n} = insertvalue {} {value}, {} {x}{indices}", t.ll(), leaf.ll())
            .unwrap();
        value = format!("%t{n}");
    }
    value
}

/// Stores the leaves of `value` (of type `t`) to `prefix_k_i_*`.
fn scatter(out: &mut String, n: &mut u32, t: &Ty, value: &str, prefix: &str, k: usize, i: usize) {
    for (j, (path, leaf)) in t.leaves().iter().enumerate() {
        let x = if path.is_empty() {
            value.to_owned()
        } else {
            *n += 1;
            let indices: String = path.iter().map(|p| format!(", {p}")).collect();
            writeln!(out, "  %t{n} = extractvalue {} {value}{indices}", t.ll()).unwrap();
            format!("%t{n}")
        };
        writeln!(out, "  store {} {x}, ptr @{}", leaf.ll(), slot(prefix, k, i, j)).unwrap();
    }
}

fn externs(out: &mut String, t: &Ty, prefix: &str, k: usize, i: usize) {
    for j in 0..t.leaves().len() {
        writeln!(out, "@{} = external global [{SLOT} x i8], align 64", slot(prefix, k, i, j))
            .unwrap();
    }
}

/// The module of the callers (after the module header).
fn caller_ll(cases: &[Case]) -> String {
    let mut out = String::new();
    if cases.iter().any(|c| c.style == Style::Invoke) {
        out.push_str("declare i32 @vc_pers(...)\n");
    }
    for (k, case) in cases.iter().enumerate() {
        for (i, t) in case.params.iter().enumerate() {
            externs(&mut out, t, "in", k, i);
        }
        for (i, t) in case.var.iter().flatten().enumerate() {
            externs(&mut out, t, "vin", k, i);
        }
        if let Some(r) = &case.ret {
            externs(&mut out, r, "rout", k, 0);
        }
        let ty = case.fn_type();
        let ret = case.ret_ll();
        let named: Vec<String> = case.params.iter().map(Ty::ll).collect();
        if case.var.is_some() {
            writeln!(out, "declare void @f_{k}({}, ...)", named.join(", ")).unwrap();
        } else {
            writeln!(out, "declare {ret} @f_{k}({})", named.join(", ")).unwrap();
        }
        let mut n = 0;
        let mut body = String::new();
        let mut args: Vec<String> = Vec::new();
        for (i, t) in case.params.iter().enumerate() {
            let v = build(&mut body, &mut n, t, "in", k, i);
            args.push(format!("{} {v}", t.ll()));
        }
        for (i, t) in case.var.iter().flatten().enumerate() {
            let v = build(&mut body, &mut n, t, "vin", k, i);
            args.push(format!("{} {v}", t.ll()));
        }
        let args = args.join(", ");
        let result = if case.ret.is_some() { "%r = " } else { "" };
        let mut callee = format!("@f_{k}");
        if case.style == Style::Musttail {
            // A forwarder in this module: incoming and outgoing at once.
            let params: Vec<String> =
                case.params.iter().enumerate().map(|(i, t)| format!("{} %a{i}", t.ll())).collect();
            let forwarded = params.join(", ");
            writeln!(out, "define {ret} @mt_{k}({forwarded}) {{").unwrap();
            if case.ret.is_some() {
                writeln!(out, "  %r = musttail call {ret} @f_{k}({forwarded})\n  ret {ret} %r")
                    .unwrap();
            } else {
                writeln!(out, "  musttail call void @f_{k}({forwarded})\n  ret void").unwrap();
            }
            out.push_str("}\n");
            callee = format!("@mt_{k}");
        }
        let personality =
            if case.style == Style::Invoke { " personality ptr @vc_pers" } else { "" };
        writeln!(out, "define void @call_{k}(){personality} {{\nentry:").unwrap();
        out.push_str(&body);
        match case.style {
            Style::Direct | Style::Musttail => {
                writeln!(out, "  {result}call {ty} {callee}({args})").unwrap();
            }
            Style::Indirect => {
                writeln!(
                    out,
                    "  %slot = alloca ptr\n  store volatile ptr {callee}, ptr %slot\n  \
                     %fp = load volatile ptr, ptr %slot\n  {result}call {ty} %fp({args})"
                )
                .unwrap();
            }
            Style::Invoke => {
                writeln!(
                    out,
                    "  {result}invoke {ty} {callee}({args}) to label %ok unwind label %pad\n\
                     pad:\n  %l = landingpad {{ ptr, i32 }} cleanup\n  \
                     resume {{ ptr, i32 }} %l\nok:"
                )
                .unwrap();
            }
        }
        if let Some(r) = &case.ret {
            scatter(&mut out, &mut n, r, "%r", "rout", k, 0);
        }
        out.push_str("  ret void\n}\n");
    }
    out
}

/// The module of the callees (after the module header).
fn callee_ll(cases: &[Case]) -> String {
    let mut out = String::new();
    if cases.iter().any(|c| c.var.is_some()) {
        out.push_str("declare void @llvm.va_start.p0(ptr)\ndeclare void @llvm.va_end.p0(ptr)\n");
    }
    for (k, case) in cases.iter().enumerate() {
        for (i, t) in case.params.iter().enumerate() {
            externs(&mut out, t, "out", k, i);
        }
        if let Some(r) = &case.ret {
            externs(&mut out, r, "rin", k, 0);
        }
        let mut params: Vec<String> =
            case.params.iter().enumerate().map(|(i, t)| format!("{} %a{i}", t.ll())).collect();
        if case.var.is_some() {
            params.push("...".into());
            writeln!(out, "declare void @vc_walk_{k}(ptr)").unwrap();
        }
        writeln!(out, "define {} @f_{k}({}) {{", case.ret_ll(), params.join(", ")).unwrap();
        let mut n = 0;
        for (i, t) in case.params.iter().enumerate() {
            scatter(&mut out, &mut n, t, &format!("%a{i}"), "out", k, i);
        }
        if case.var.is_some() {
            writeln!(
                out,
                "  %ap = alloca [32 x i8], align 8\n  call void @llvm.va_start.p0(ptr %ap)\n  \
                 call void @vc_walk_{k}(ptr %ap)\n  call void @llvm.va_end.p0(ptr %ap)"
            )
            .unwrap();
        }
        match &case.ret {
            Some(r) => {
                let v = build(&mut out, &mut n, r, "rin", k, 0);
                writeln!(out, "  ret {} {v}", r.ll()).unwrap();
            }
            None => out.push_str("  ret void\n"),
        }
        out.push_str("}\n");
    }
    out
}

// ---- C ----

/// The driver: the globals, the variadic walkers, and `main`.
fn driver(cases: &[Case]) -> String {
    let mut out = String::from(
        "#include <stdarg.h>\n#include <stdio.h>\n#include <string.h>\n\
         typedef int vc_i32x4 __attribute__((vector_size(16)));\n",
    );
    let mut decls = String::new();
    let mut main = String::new();
    let mut slots = Vec::new();
    for (k, case) in cases.iter().enumerate() {
        writeln!(out, "void call_{k}(void);").unwrap();
        let groups: [(&str, &str, char, &[Ty]); 3] = [
            ("in", "out", 'p', &case.params),
            ("vin", "vout", 'v', case.var.as_deref().unwrap_or(&[])),
            ("rin", "rout", 'r', case.ret.as_slice()),
        ];
        for (from, to, _, types) in groups {
            for (i, t) in types.iter().enumerate() {
                for (j, (_, leaf)) in t.leaves().iter().enumerate() {
                    let (a, b) = (slot(from, k, i, j), slot(to, k, i, j));
                    slots.push(a.clone());
                    slots.push(b.clone());
                    writeln!(main, "  fill({a}, {}); memset({b}, 0x5a, {SLOT});", leaf.bytes())
                        .unwrap();
                }
            }
        }
        writeln!(main, "  call_{k}();").unwrap();
        for (from, to, tag, types) in groups {
            for (i, t) in types.iter().enumerate() {
                for (j, (_, leaf)) in t.leaves().iter().enumerate() {
                    writeln!(
                        main,
                        "  dump({k}, '{tag}', {i}, {j}, {}, {}, {});",
                        slot(from, k, i, j),
                        slot(to, k, i, j),
                        leaf.bytes()
                    )
                    .unwrap();
                }
            }
        }
        if let Some(var) = &case.var {
            let mut walk = format!("void vc_walk_{k}(va_list *ap) {{\n");
            for (i, t) in var.iter().enumerate() {
                let ct = t.c_leaf();
                writeln!(
                    walk,
                    "  {{ {ct} x = va_arg(*ap, {ct}); memcpy({}, &x, sizeof x); }}",
                    slot("vout", k, i, 0)
                )
                .unwrap();
            }
            walk.push_str("}\n");
            decls.push_str(&walk);
        }
    }
    for s in &slots {
        writeln!(out, "unsigned char {s}[{SLOT}] __attribute__((aligned(64)));").unwrap();
    }
    out.push_str(&decls);
    write!(
        out,
        "int vc_pers(void) {{ return 0; }}\n\
         static unsigned long long state;\n\
         static unsigned char next(void) {{\n  state ^= state << 13; state ^= state >> 7; \
         state ^= state << 17;\n  return (unsigned char)(state >> 29);\n}}\n\
         static void fill(unsigned char *p, int n) {{\n  memset(p, 0, {SLOT});\n  \
         for (int i = 0; i < n; i++) p[i] = next();\n}}\n\
         static void dump(int k, char tag, int i, int j, const unsigned char *a, \
         const unsigned char *b, int n) {{\n  printf(\"%d %c %d %d \", k, tag, i, j);\n  \
         for (int x = 0; x < n; x++) printf(\"%02x\", a[x]);\n  printf(\" \");\n  \
         for (int x = 0; x < n; x++) printf(\"%02x\", b[x]);\n  printf(\"\\n\");\n}}\n\
         int main(void) {{\n  for (int round = 0; round < {ROUNDS}; round++) {{\n  \
         state = 0x9e3779b97f4a7c15ULL * (round + 1);\n{main}  }}\n  return 0;\n}}\n"
    )
    .unwrap();
    out
}

// ---- running ----

/// hod's object of `text` (after the header), or its refusal.
fn hod_compile(text: &str, stress: Stress) -> Result<Vec<u8>, CgError> {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(stress)) {
        Ok(object) => Ok(object),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

fn hod_object(scratch: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let object = hod_compile(text, stress).unwrap_or_else(|e| panic!("hod {name}: {e}\n{text}"));
    scratch.write(&format!("{name}.o"), object)
}

fn llc_object(scratch: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    float_gen::llc(scratch, major, name, &format!("{}{text}", ll_header()))
}

/// The name hod refuses the module `text` with, if it does.
fn refusal(text: &str) -> Option<&'static str> {
    match hod_compile(text, Stress::default()) {
        Ok(_) => None,
        Err(CgError::Unsupported { construct }) => Some(construct),
        Err(other) => panic!("hod: {other}\n{text}"),
    }
}

/// Checks the driver's output against the cases; returns the number of
/// leaves compared, and pushes each disagreement (with its case number)
/// to `wrong`.
fn check_output(
    cases: &[Case],
    label: &str,
    stdout: &str,
    wrong: &mut Vec<(usize, String)>,
) -> usize {
    let want: usize = cases.iter().map(Case::leaf_count).sum::<usize>() * ROUNDS as usize;
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != want {
        wrong.push((usize::MAX, format!("{label}: {} lines, want {want}", lines.len())));
        return 0;
    }
    for line in &lines {
        let f: Vec<&str> = line.split(' ').collect();
        let (k, tag, i, j) = (
            f[0].parse::<usize>().unwrap(),
            f[1],
            f[2].parse::<usize>().unwrap(),
            f[3].parse::<usize>().unwrap(),
        );
        let case = &cases[k];
        let t = match tag {
            "p" => &case.params[i],
            "v" => &case.var.as_ref().unwrap()[i],
            _ => case.ret.as_ref().unwrap(),
        };
        let (_, leaf) = &t.leaves()[j];
        if f[4] != f[5] {
            wrong.push((
                k,
                format!(
                    "{label}: case {k} `{}` style {:?} var {:?}: {tag}{i} leaf {j} ({}): sent {} \
                     got {}",
                    case.fn_type(),
                    case.style,
                    case.var.as_ref().map(|v| v.iter().map(Ty::ll).collect::<Vec<_>>()),
                    leaf.ll(),
                    f[4],
                    f[5]
                ),
            ));
        }
    }
    lines.len()
}

/// The objects of the pairings, by side: (callers, callee).
type Sides = BTreeMap<String, (PathBuf, PathBuf)>;

fn llc_sides(scratch: &Scratch, tag: &str, cases: &[Case], sides: &mut Sides) {
    for major in [22, 19] {
        let caller =
            llc_object(scratch, major, &format!("{tag}-llc{major}-caller"), &caller_ll(cases));
        let callee =
            llc_object(scratch, major, &format!("{tag}-llc{major}-callee"), &callee_ll(cases));
        if let (Some(caller), Some(callee)) = (caller, callee) {
            sides.insert(format!("llc{major}"), (caller, callee));
        }
    }
}

/// Links and runs each pairing (callers, callee, every linker); returns
/// the leaves compared and the programs run.
fn run_pairs(
    scratch: &Scratch,
    toolchain: &linking::Toolchain,
    tag: &str,
    cases: &[Case],
    sides: &Sides,
    pairs: &[(&str, &str, bool)],
    wrong: &mut Vec<(usize, String)>,
) -> (usize, Vec<String>) {
    let driver = toolchain.compile_c(scratch, &format!("{tag}-driver"), &driver(cases), &[]);
    let mut legs = Vec::new();
    let mut compared = 0;
    for &(caller, callee, every) in pairs {
        let (Some((caller_o, _)), Some((_, callee_o))) = (sides.get(caller), sides.get(callee))
        else {
            continue;
        };
        let linkers = if every { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let label = format!("{caller}>{callee}/{}", linker.name);
            let program = scratch.join(format!("{tag}-{}", label.replace(['/', '>'], "-")));
            let inputs: [&Path; 3] = [&driver, caller_o, callee_o];
            toolchain.link_ok(linker, &[], &inputs, &program);
            let ran =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(3));
            if !ran.success() {
                wrong.push((
                    usize::MAX,
                    format!("{label}: {:?}\n{}", ran.status, ran.stderr_text()),
                ));
                continue;
            }
            compared += check_output(cases, &label, &ran.stdout_text(), wrong);
            legs.push(label);
        }
    }
    (compared, legs)
}

/// Compiles each case alone (callers' and callee's module), then builds
/// and runs every pairing of the cases hod builds; checks that each case
/// refused as `x64:vector-i128-split` is one `llc` 19 and 22 disagree on.
fn check(name: &str, cases: &[Case]) {
    let scratch = Scratch::new(&format!("x64-vcall-i128-{name}"));
    // The one-case modules.
    let mut tally: BTreeMap<&'static str, usize> = BTreeMap::new();
    let (mut built, mut refused) = (Vec::new(), Vec::new());
    // `HOD_VCALL_DUMP=<dir>`: the one-case modules as files too, for the
    // `hod` driver (whose `musttail` rule names its own refusal).
    let dump = std::env::var_os("HOD_VCALL_DUMP").map(PathBuf::from);
    for (k, case) in cases.iter().enumerate() {
        let one = std::slice::from_ref(case);
        if let Some(dir) = &dump {
            for (side, text) in [("caller", caller_ll(one)), ("callee", callee_ll(one))] {
                let path = dir.join(format!("{name}-{k:02}-{side}.ll"));
                std::fs::write(path, format!("{}{text}", ll_header())).unwrap();
            }
        }
        let caller = refusal(&caller_ll(one));
        let callee = refusal(&callee_ll(one));
        for r in [caller, callee] {
            *tally.entry(r.unwrap_or("built")).or_default() += 1;
        }
        match caller.or(callee) {
            None => built.push(case.clone()),
            Some(why) => refused.push((case.clone(), why)),
        }
    }
    eprintln!("x64 vcall {name}: {} one-case modules: {tally:?}", 2 * cases.len());
    for (case, why) in &refused {
        eprintln!("x64 vcall {name}: refused {why}: {} {:?}", case.fn_type(), case.style);
    }

    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let mut wrong = Vec::new();

    // The refused cases: `llc` 19 against 22 must lose a leaf.
    let split: Vec<Case> = refused
        .iter()
        .filter(|(_, why)| *why == hod_x64::abi::I128_VECTOR_SPLIT)
        .map(|(c, _)| c.clone())
        .collect();
    if !split.is_empty() {
        let mut sides = Sides::new();
        llc_sides(&scratch, "split", &split, &mut sides);
        let mut differ = Vec::new();
        let pairs = [("llc19", "llc22", false), ("llc22", "llc19", false)];
        let (compared, _) =
            run_pairs(&scratch, &toolchain, "split", &split, &sides, &pairs, &mut differ);
        let mut agreed: Vec<String> = Vec::new();
        for (k, case) in split.iter().enumerate() {
            if !differ.iter().any(|(at, _)| *at == k || *at == usize::MAX) {
                agreed.push(case.fn_type());
            }
        }
        eprintln!(
            "x64 vcall {name}: {} split refusals, llc 19 against 22 over {compared} leaves: \
             {} disagreements",
            split.len(),
            differ.len()
        );
        assert!(
            agreed.is_empty() || sides.len() < 2,
            "refused as {} although llc 19 and 22 agree: {agreed:#?}",
            hod_x64::abi::I128_VECTOR_SPLIT
        );
    }

    let mut sides = Sides::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        let caller = hod_object(&scratch, &format!("{leg}-caller"), &caller_ll(&built), stress);
        let callee = hod_object(&scratch, &format!("{leg}-callee"), &callee_ll(&built), stress);
        sides.insert(leg.into(), (caller, callee));
    }
    llc_sides(&scratch, "built", &built, &mut sides);
    let pairs = [
        ("hod-plain", "llc22", true),
        ("llc22", "hod-plain", true),
        ("hod-stress", "llc22", false),
        ("llc22", "hod-stress", false),
        ("hod-stress", "llc19", false),
        ("llc19", "hod-stress", false),
        ("hod-plain", "llc19", false),
        ("llc19", "hod-plain", false),
        ("hod-plain", "hod-stress", false),
        ("hod-stress", "hod-plain", false),
        ("llc22", "llc22", false),
        ("llc19", "llc22", false),
        ("llc22", "llc19", false),
    ];
    let (compared, legs) =
        run_pairs(&scratch, &toolchain, "built", &built, &sides, &pairs, &mut wrong);
    eprintln!(
        "x64 vcall {name}: {} cases built, {compared} leaves compared over {} programs {legs:?}",
        built.len(),
        legs.len()
    );
    if !wrong.is_empty() {
        let mut message = format!("{} disagreements; the first:\n", wrong.len());
        for (_, line) in wrong.iter().take(12) {
            writeln!(message, "{line}").unwrap();
        }
        panic!("{message}");
    }
}

// ---- tests ----

/// WP-167's 81 signatures (a lone `<1 x i128>` after 0 to 8 general
/// arguments, around `i32`, after full vector registers, in a structure,
/// beside `<2 x i128>`, `<3 x i128>`, `i128` and the byte vectors;
/// through a pointer, `invoke` and `musttail`; as a named argument of a
/// variadic callee).
#[test]
fn wp167_i128_signatures_cross_calls() {
    let cases = wp167_cases();
    assert_eq!(cases.len(), 81);
    check("wp167", &cases);
}

/// §4.3's placements of `<N x i128>`, N = 1..4, at every count of general
/// registers left and at both stack alignments.
#[test]
fn i128_vector_placements_cross_calls() {
    check("placements", &placement_cases());
}

/// WP-174: lone `<1 x i64>` and `<1 x ptr>` across calls; hod builds
/// every case (no `call:part-register-class`).
#[test]
fn lone_i64_vectors_cross_calls() {
    let cases = lone_i64_cases();
    for case in &cases {
        let one = std::slice::from_ref(case);
        for text in [caller_ll(one), callee_ll(one)] {
            assert_eq!(refusal(&text), None, "{} {:?}", case.fn_type(), case.style);
        }
    }
    check("lone-i64", &cases);
}

/// The generator itself (no tools needed): every module parses.
#[test]
fn the_generator_parses() {
    let mut cases = wp167_cases();
    cases.extend(placement_cases());
    cases.extend(lone_i64_cases());
    for text in [caller_ll(&cases), callee_ll(&cases)] {
        parse(&format!("{}{text}", ll_header()));
    }
}
