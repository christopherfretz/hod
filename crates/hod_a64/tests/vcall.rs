//! Vectors across calls (WP-082, `spec/abi-a64.md` §4.3,
//! `spec/codegen.md` §5.5.1 "Vectors across calls (WP-082)"): masks,
//! promoted vectors, vectors passed as bytes, and aggregates of them, as
//! arguments and results, checked natively against `llc` and gcc.
//!
//! Every case is a signature. Its callee `f_K` stores each leaf of each
//! argument (`extractvalue`) to a global of its own and returns a value
//! built (`insertvalue`) from leaves loaded from globals; its caller
//! `call_K` loads each leaf of each argument, builds the arguments, calls
//! and stores each leaf of the result. A C driver fills the input globals
//! with random bytes (a mask's bits from N up zero), calls every `call_K`
//! and prints what went in and what came out. Every leaf must arrive as
//! it left: every bit of a mask's N, every byte of anything else.
//!
//! Callers and callees are separate modules, each built by hod (plain and
//! under every stress option) and by `llc -O0 -global-isel=0 -fast-isel=0`
//! of LLVM 22 and 19; pairs of them are linked, hod against `llc` both
//! ways, and the hod-against-`llc` 22 pairs under bfd, lld and mold. The
//! signatures C can spell (power-of-two `vector_size` vectors and arrays of
//! them in a struct) are also built by gcc, both sides.
//!
//! The call styles: a direct `call`, a call through a pointer, an
//! `invoke`, and a `musttail` forwarder in the caller's module. A variadic
//! case passes C-spellable vectors as variable arguments to a callee that
//! takes its named arguments (any shape) and hands its `va_list` to a C
//! walker that takes the rest with `va_arg`.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::format_collect,
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "a test generator: `.ll` and C text built by hand"
)]

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, ll_header, parse, write_object};
use hod_a64::{abi, compile_module};
use hod_cg::{Options, Stress};
use hod_testutil::{Scratch, run_with, tool};

/// The bytes of every global the driver defines.
const SLOT: usize = 64;
/// How many times the driver runs every case, with fresh values.
const ROUNDS: u32 = 4;

// ---- types ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I1,
    I8,
    I16,
    I32,
    I64,
    /// WP-167: `i128`, as a scalar and as the element of a vector.
    I128,
    Half,
    Float,
    Double,
}

impl E {
    fn ll(self) -> &'static str {
        match self {
            E::I1 => "i1",
            E::I8 => "i8",
            E::I16 => "i16",
            E::I32 => "i32",
            E::I64 => "i64",
            E::I128 => "i128",
            E::Half => "half",
            E::Float => "float",
            E::Double => "double",
        }
    }

    fn bits(self) -> u32 {
        match self {
            E::I1 => 1,
            E::I8 => 8,
            E::I16 | E::Half => 16,
            E::I32 | E::Float => 32,
            E::I64 | E::Double => 64,
            E::I128 => 128,
        }
    }

    /// The C type of an element, where C has one.
    fn c(self) -> Option<&'static str> {
        Some(match self {
            E::I8 => "signed char",
            E::I16 => "short",
            E::I32 => "int",
            E::I64 => "long",
            E::Float => "float",
            E::Double => "double",
            E::I1 | E::I128 | E::Half => return None,
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
    /// An array.
    Ar(u32, Box<Ty>),
}

use Ty::{Ar, S, St, V};

fn ar(n: u32, t: Ty) -> Ty {
    Ar(n, Box::new(t))
}

impl Ty {
    fn ll(&self) -> String {
        match self {
            S(e) => e.ll().into(),
            V(e, n) => format!("<{n} x {}>", e.ll()),
            St(fields) => {
                format!("{{ {} }}", fields.iter().map(Ty::ll).collect::<Vec<_>>().join(", "))
            }
            Ar(n, t) => format!("[{n} x {}]", t.ll()),
        }
    }

    /// The leaves (scalars and vectors) with their `extractvalue` paths.
    fn leaves(&self) -> Vec<(Vec<u32>, Ty)> {
        fn walk(t: &Ty, path: &mut Vec<u32>, out: &mut Vec<(Vec<u32>, Ty)>) {
            match t {
                S(_) | V(..) => out.push((path.clone(), t.clone())),
                St(fields) => {
                    for (i, f) in fields.iter().enumerate() {
                        path.push(i as u32);
                        walk(f, path, out);
                        path.pop();
                    }
                }
                Ar(n, e) => {
                    for i in 0..*n {
                        path.push(i);
                        walk(e, path, out);
                        path.pop();
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &mut Vec::new(), &mut out);
        out
    }

    /// The bytes a leaf's store writes.
    fn bytes(&self) -> usize {
        match self {
            S(e) => e.bits() as usize / 8,
            V(e, n) => (n * e.bits()).div_ceil(8) as usize,
            _ => unreachable!("not a leaf"),
        }
    }

    /// The mask of the leaf's last byte that carries value bits.
    fn last(&self) -> u8 {
        match self {
            V(E::I1, n) if n % 8 != 0 => (1u8 << (n % 8)) - 1,
            _ => 0xff,
        }
    }

    /// Whether a leaf travels in one register (or, a mask, in up to
    /// eight).
    fn small(&self) -> bool {
        match self {
            V(E::I1, n) => *n <= 8 || *n == 16,
            S(_) | V(..) => self.bytes() <= 16,
            _ => false,
        }
    }

    /// The C type of a leaf, where C spells it as `llc` passes it.
    fn c_leaf(&self) -> Option<String> {
        match self {
            S(e) => e.c().map(str::to_owned),
            V(e, n) => {
                let bytes = n * e.bits() / 8;
                e.c()?;
                (bytes == 8 || bytes == 16).then(|| format!("vc_{}x{n}", e.ll()))
            }
            _ => None,
        }
    }

    /// Whether C spells the type: a leaf, or an array of up to four of a
    /// vector leaf (a homogeneous vector aggregate) as a struct.
    fn c_ok(&self) -> bool {
        match self {
            S(_) | V(..) => self.c_leaf().is_some(),
            Ar(n, e) => *n <= 4 && matches!(**e, V(..)) && e.c_leaf().is_some(),
            St(_) => false,
        }
    }
}

/// Every vector typedef the C sources may use.
fn c_typedefs() -> String {
    let mut out = String::new();
    for e in [E::I8, E::I16, E::I32, E::I64, E::Float, E::Double] {
        for bytes in [8, 16] {
            let n = bytes * 8 / e.bits();
            writeln!(
                out,
                "typedef {} vc_{}x{n} __attribute__((vector_size({bytes})));",
                e.c().unwrap(),
                e.ll()
            )
            .unwrap();
        }
    }
    out
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

    fn c_ok(&self) -> bool {
        self.var.is_none()
            && self.style == Style::Direct
            && self.params.iter().all(Ty::c_ok)
            && self.ret.as_ref().is_none_or(Ty::c_ok)
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
}

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

const MASK_LENGTHS: [u32; 18] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 15, 16, 17, 24, 31, 32, 33, 64];

fn masks() -> Vec<Ty> {
    MASK_LENGTHS.iter().map(|&n| V(E::I1, n)).collect()
}

/// Vectors `llc` widens to lanes, or passes element by element.
fn promoted() -> Vec<Ty> {
    vec![
        V(E::I8, 1),
        V(E::I8, 2),
        V(E::I8, 3),
        V(E::I8, 4),
        V(E::I16, 1),
        V(E::I16, 2),
        V(E::I16, 3),
        V(E::I8, 6),
    ]
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

/// WP-167: a lone `<1 x i128>` (a `Q`-register value passed as an
/// `i128`), the longer `i128` vectors and the scalar beside it.
fn wide() -> Vec<Ty> {
    vec![V(E::I128, 1), V(E::I128, 2), V(E::I128, 3), S(E::I128)]
}

fn scalars() -> Vec<Ty> {
    vec![S(E::I32), S(E::I64), S(E::Float), S(E::Double)]
}

/// The vectors C spells.
fn c_vectors() -> Vec<Ty> {
    let mut out = Vec::new();
    for e in [E::I8, E::I16, E::I32, E::I64, E::Float, E::Double] {
        for bytes in [8, 16] {
            out.push(V(e, bytes * 8 / e.bits()));
        }
    }
    out
}

/// A random argument or result over `pool`: a leaf, or (`aggregates`) a
/// struct or an array of leaves.
fn shape(rng: &mut Rng, pool: &[Ty], aggregates: bool) -> Ty {
    let leaf = rng.pick(pool);
    if !aggregates {
        return leaf;
    }
    match rng.below(4) {
        0 | 1 => leaf,
        2 => St((0..=rng.below(3)).map(|_| rng.pick(pool)).collect()),
        // An array of a vector passed in several registers is refused
        // (`abi:vector-array`: LLVM's selectors disagree).
        _ if leaf.small() => ar(2 + rng.below(3) as u32, leaf),
        _ => leaf,
    }
}

/// Random signatures over `pool`; a quarter of them first fill the vector
/// or the general argument registers.
fn random(rng: &mut Rng, pool: &[Ty], count: usize, aggregates: bool) -> Vec<Case> {
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
            params.extend((0..n).map(|_| shape(rng, pool, aggregates)));
            let ret = (rng.below(4) != 0).then(|| shape(rng, pool, aggregates));
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

/// The module of the callers.
fn caller_ll(cases: &[Case], major: u32) -> String {
    let mut out = ll_header(major);
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
            let mut tail = String::new();
            scatter(&mut tail, &mut n, r, "%r", "rout", k, 0);
            out.push_str(&tail);
        }
        out.push_str("  ret void\n}\n");
    }
    out
}

/// The module of the callees.
fn callee_ll(cases: &[Case], major: u32) -> String {
    let mut out = ll_header(major);
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

/// The C type of an argument (`c_ok`), declaring a struct for an array.
fn c_type(decls: &mut String, t: &Ty, name: &str) -> String {
    match t {
        Ar(n, e) => {
            writeln!(decls, "struct {name} {{ {} a[{n}]; }};", e.c_leaf().unwrap()).unwrap();
            format!("struct {name}")
        }
        _ => t.c_leaf().unwrap(),
    }
}

/// The C lvalue of leaf `j` of `t` held in `var`.
fn c_leaf_of(t: &Ty, var: &str, j: usize) -> String {
    match t {
        Ar(..) => format!("{var}.a[{j}]"),
        _ => var.to_owned(),
    }
}

/// The driver: the globals, the variadic walkers, and `main`.
fn driver(cases: &[Case]) -> String {
    let mut out =
        format!("#include <stdarg.h>\n#include <stdio.h>\n#include <string.h>\n{}", c_typedefs());
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
        for (from, to, tag, types) in groups {
            for (i, t) in types.iter().enumerate() {
                for (j, (_, leaf)) in t.leaves().iter().enumerate() {
                    let (a, b) = (slot(from, k, i, j), slot(to, k, i, j));
                    slots.push(a.clone());
                    slots.push(b.clone());
                    writeln!(
                        main,
                        "  fill({a}, {}, {}); memset({b}, 0x5a, {SLOT});",
                        leaf.bytes(),
                        leaf.last()
                    )
                    .unwrap();
                    let _ = tag;
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
                let ct = c_type(&mut decls, t, &format!("vc_v{k}_{i}"));
                writeln!(walk, "  {{ {ct} x = va_arg(*ap, {ct});").unwrap();
                for j in 0..t.leaves().len() {
                    writeln!(
                        walk,
                        "    memcpy({}, &{}, sizeof {});",
                        slot("vout", k, i, j),
                        c_leaf_of(t, "x", j),
                        c_leaf_of(t, "x", j)
                    )
                    .unwrap();
                }
                walk.push_str("  }\n");
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
         static void fill(unsigned char *p, int n, int last) {{\n  memset(p, 0, {SLOT});\n  \
         for (int i = 0; i < n; i++) p[i] = next();\n  p[n - 1] &= last;\n}}\n\
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

/// gcc's side of the C-spellable cases: callees (`callee`) or callers.
fn c_side(cases: &[Case], callee: bool) -> String {
    let mut out = format!("#include <string.h>\n{}", c_typedefs());
    let mut body = String::new();
    for (k, case) in cases.iter().enumerate() {
        for (from, to, types) in
            [("in", "out", &case.params[..]), ("rin", "rout", case.ret.as_slice())]
        {
            for (i, t) in types.iter().enumerate() {
                for j in 0..t.leaves().len() {
                    writeln!(
                        out,
                        "extern unsigned char {}[], {}[];",
                        slot(from, k, i, j),
                        slot(to, k, i, j)
                    )
                    .unwrap();
                }
            }
        }
        let params: Vec<String> = case
            .params
            .iter()
            .enumerate()
            .map(|(i, t)| c_type(&mut out, t, &format!("vc_p{k}_{i}")))
            .collect();
        let ret = case
            .ret
            .as_ref()
            .map_or_else(|| "void".to_owned(), |r| c_type(&mut out, r, &format!("vc_r{k}")));
        let list: Vec<String> =
            params.iter().enumerate().map(|(i, p)| format!("{p} a{i}")).collect();
        let proto = format!(
            "{ret} f_{k}({})",
            if list.is_empty() { "void".into() } else { list.join(", ") }
        );
        if callee {
            writeln!(body, "{proto} {{").unwrap();
            for (i, t) in case.params.iter().enumerate() {
                for j in 0..t.leaves().len() {
                    let x = c_leaf_of(t, &format!("a{i}"), j);
                    writeln!(body, "  memcpy({}, &{x}, sizeof {x});", slot("out", k, i, j))
                        .unwrap();
                }
            }
            if let Some(r) = &case.ret {
                writeln!(body, "  {ret} r;").unwrap();
                for j in 0..r.leaves().len() {
                    let x = c_leaf_of(r, "r", j);
                    writeln!(body, "  memcpy(&{x}, {}, sizeof {x});", slot("rin", k, 0, j))
                        .unwrap();
                }
                body.push_str("  return r;\n");
            }
            body.push_str("}\n");
        } else {
            writeln!(body, "{proto};\nvoid call_{k}(void) {{").unwrap();
            for (i, t) in case.params.iter().enumerate() {
                writeln!(body, "  {} a{i};", params[i]).unwrap();
                for j in 0..t.leaves().len() {
                    let x = c_leaf_of(t, &format!("a{i}"), j);
                    writeln!(body, "  memcpy(&{x}, {}, sizeof {x});", slot("in", k, i, j)).unwrap();
                }
            }
            let args: Vec<String> = (0..case.params.len()).map(|i| format!("a{i}")).collect();
            match &case.ret {
                Some(r) => {
                    writeln!(body, "  {ret} r = f_{k}({});", args.join(", ")).unwrap();
                    for j in 0..r.leaves().len() {
                        let x = c_leaf_of(r, "r", j);
                        writeln!(body, "  memcpy({}, &{x}, sizeof {x});", slot("rout", k, 0, j))
                            .unwrap();
                    }
                }
                None => writeln!(body, "  f_{k}({});", args.join(", ")).unwrap(),
            }
            body.push_str("}\n");
        }
    }
    out.push_str(&body);
    out
}

// ---- running ----

fn llc_object(scratch: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    let llc = if llc.is_file() { llc } else { tool(&format!("llc-{major}"))? };
    let input = scratch.write(&format!("{name}.ll"), text);
    let output = scratch.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} {name}:\n{}", outcome.stderr_text());
    Some(output)
}

fn hod_object(scratch: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let (module, bodies) = parse(text);
    let options = Options { stress, ..Options::default() };
    let emitted = compile_module(&module, &bodies, &options)
        .unwrap_or_else(|e| panic!("hod {name}: {e}\n{text}"));
    write_object(scratch, &format!("{name}.o"), &emitted)
}

/// Checks the driver's output against the cases; returns the number of
/// leaves compared.
fn check_output(cases: &[Case], label: &str, stdout: &str, wrong: &mut Vec<String>) -> usize {
    let mut want = 0;
    for case in cases {
        let count = |ts: &[Ty]| ts.iter().map(|t| t.leaves().len()).sum::<usize>();
        want += count(&case.params)
            + count(case.var.as_deref().unwrap_or(&[]))
            + count(case.ret.as_slice());
    }
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() != want * ROUNDS as usize {
        wrong.push(format!("{label}: {} lines, want {}", lines.len(), want * ROUNDS as usize));
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
        let (a, b) = (f[4].as_bytes(), f[5].as_bytes());
        let n = leaf.bytes();
        let byte = |s: &[u8], x: usize| {
            u8::from_str_radix(std::str::from_utf8(&s[2 * x..2 * x + 2]).unwrap(), 16).unwrap()
        };
        let same = (0..n).all(|x| {
            let care = if x + 1 == n { leaf.last() } else { 0xff };
            (byte(a, x) ^ byte(b, x)) & care == 0
        });
        if !same {
            wrong.push(format!(
                "{label}: case {k} `{}` style {:?} var {:?}: {tag}{i} leaf {j} ({}): sent {} got {}",
                case.fn_type(),
                case.style,
                case.var.as_ref().map(|v| v.iter().map(Ty::ll).collect::<Vec<_>>()),
                leaf.ll(),
                f[4],
                f[5]
            ));
        }
    }
    lines.len()
}

/// Builds every side, links the pairs, runs them and compares.
fn check(name: &str, cases: &[Case]) {
    let scratch = Scratch::new(&format!("a64-vcall-{name}"));
    let mut objects: BTreeMap<String, (PathBuf, PathBuf)> = BTreeMap::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        let caller = hod_object(&scratch, &format!("{leg}-caller"), &caller_ll(cases, 22), stress);
        let callee = hod_object(&scratch, &format!("{leg}-callee"), &callee_ll(cases, 22), stress);
        objects.insert(leg.into(), (caller, callee));
    }
    for major in [22, 19] {
        let caller =
            llc_object(&scratch, major, &format!("llc{major}-caller"), &caller_ll(cases, major));
        let callee =
            llc_object(&scratch, major, &format!("llc{major}-callee"), &callee_ll(cases, major));
        if let (Some(caller), Some(callee)) = (caller, callee) {
            objects.insert(format!("llc{major}"), (caller, callee));
        }
    }
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let gcc = cases.iter().all(Case::c_ok);
    if gcc {
        let caller = toolchain.compile_c(&scratch, "gcc-caller", &c_side(cases, false), &[]);
        let callee = toolchain.compile_c(&scratch, "gcc-callee", &c_side(cases, true), &[]);
        objects.insert("gcc".into(), (caller, callee));
    }
    let driver = toolchain.compile_c(&scratch, "driver", &driver(cases), &[]);
    let mut pairs = vec![
        ("hod-plain", "llc22", true),
        ("llc22", "hod-plain", true),
        ("hod-stress", "llc19", false),
        ("llc19", "hod-stress", false),
        ("hod-plain", "hod-stress", false),
        ("hod-stress", "hod-plain", false),
        ("llc22", "llc22", false),
        ("llc19", "llc22", false),
    ];
    if gcc {
        pairs.extend([
            ("hod-plain", "gcc", true),
            ("gcc", "hod-plain", true),
            ("hod-stress", "gcc", false),
            ("gcc", "hod-stress", false),
            ("llc22", "gcc", false),
            ("gcc", "llc22", false),
        ]);
    }
    let mut wrong = Vec::new();
    let mut legs = Vec::new();
    let mut compared = 0;
    for (caller, callee, every) in pairs {
        let (Some((caller_o, _)), Some((_, callee_o))) = (objects.get(caller), objects.get(callee))
        else {
            continue;
        };
        let linkers = if every { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let label = format!("{caller}>{callee}/{}", linker.name);
            let program = scratch.join(label.replace(['/', '>'], "-"));
            let inputs: [&Path; 3] = [&driver, caller_o, callee_o];
            toolchain.link_ok(linker, &[], &inputs, &program);
            let ran =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(3));
            if !ran.success() {
                wrong.push(format!("{label}: {:?}\n{}", ran.status, ran.stderr_text()));
                continue;
            }
            compared += check_output(cases, &label, &ran.stdout_text(), &mut wrong);
            legs.push(label);
        }
    }
    eprintln!("a64 vcall {name}: {} cases, {compared} leaves compared over {legs:?}", cases.len());
    if !wrong.is_empty() {
        let mut message = format!("{} disagreements; the first:\n", wrong.len());
        for line in wrong.iter().take(12) {
            writeln!(message, "{line}").unwrap();
        }
        panic!("{message}");
    }
}

// ---- tests ----

/// Masks of every length up to 64: lanes of a D or Q register, or one
/// element per general register, then on the stack; results in memory.
#[test]
fn masks_cross_calls() {
    let mut cases = each(&masks());
    let mut rng = Rng(0x0820_0001);
    let mut pool = masks();
    pool.extend(scalars());
    cases.extend(random(&mut rng, &pool, 24, false));
    check("masks", &cases);
}

/// Vectors `llc` widens to lanes and the ones it passes as bytes, wide
/// and odd ones included (in memory).
#[test]
fn promoted_and_byte_vectors_cross_calls() {
    let mut pool = promoted();
    pool.extend(bytes());
    let mut cases = each(&pool);
    let mut rng = Rng(0x0820_0002);
    pool.extend(scalars());
    cases.extend(random(&mut rng, &pool, 24, false));
    check("vectors", &cases);
}

/// Structs and arrays of every kind of leaf: homogeneous aggregates,
/// arrays of lanes, and aggregates in memory.
#[test]
fn aggregates_of_vectors_cross_calls() {
    let mut pool = masks();
    pool.extend(promoted());
    pool.extend(bytes());
    pool.extend(scalars());
    let mut rng = Rng(0x0820_0003);
    let mut cases = random(&mut rng, &pool, 40, true);
    for t in [V(E::I1, 4), V(E::I1, 3), V(E::I8, 2), V(E::I16, 2), V(E::Float, 2), V(E::I32, 4)] {
        for n in [2, 4, 5] {
            cases.push(Case::new(vec![ar(n, t.clone())], Some(ar(n, t.clone()))));
            let mut full: Vec<Ty> = (0..6).map(|_| V(E::I32, 4)).collect();
            full.extend((0..7).map(|_| S(E::I64)));
            full.push(ar(n, t.clone()));
            full.push(t.clone());
            cases.push(Case::new(full, None));
        }
    }
    check("aggregates", &cases);
}

/// The same shapes through a pointer, an `invoke` and a `musttail`
/// forwarder.
#[test]
fn call_styles_pass_vectors() {
    let mut pool = masks();
    pool.extend(promoted());
    pool.extend(bytes());
    let mut rng = Rng(0x0820_0004);
    let mut cases = random(&mut rng, &pool, 36, true);
    for (k, case) in cases.iter_mut().enumerate() {
        case.style = [Style::Indirect, Style::Invoke, Style::Musttail][k % 3];
        if case.style == Style::Musttail && !case.ret.as_ref().is_none_or(Ty::small) {
            // A result in memory without `sret` cannot be forwarded
            // (`call:musttail-ret-memory`).
            case.ret = Some(V(E::I1, 4));
        }
    }
    check("styles", &cases);
}

/// A variadic callee: named arguments of every shape, variable vectors
/// and homogeneous vector aggregates C takes with `va_arg`.
#[test]
fn variadic_calls_pass_vectors() {
    let mut named = masks();
    named.extend(promoted());
    named.extend(bytes());
    let mut var_pool = c_vectors();
    // Not `float`: C promotes a variable `float` to `double`.
    var_pool.extend([S(E::I32), S(E::I64), S(E::Double)]);
    let mut rng = Rng(0x0820_0005);
    let mut cases = Vec::new();
    for _ in 0..30 {
        let mut params: Vec<Ty> =
            (0..=rng.below(4)).map(|_| shape(&mut rng, &named, true)).collect();
        if rng.below(4) == 0 {
            params.splice(0..0, (0..8).map(|_| V(E::I32, 4)));
        }
        let var: Vec<Ty> = (0..=rng.below(8))
            .map(|_| {
                let leaf = rng.pick(&var_pool);
                if matches!(leaf, V(..)) && rng.below(3) == 0 {
                    ar(2 + rng.below(3) as u32, leaf)
                } else {
                    leaf
                }
            })
            .collect();
        cases.push(Case { params, ret: None, style: Style::Direct, var: Some(var) });
    }
    check("variadic", &cases);
}

/// What gcc spells: D and Q vectors and homogeneous vector aggregates,
/// against gcc on both sides.
#[test]
fn vectors_agree_with_gcc() {
    let pool = c_vectors();
    let mut cases = each(&pool);
    let mut rng = Rng(0x0820_0006);
    let mut agg_pool = pool.clone();
    agg_pool.extend(scalars());
    cases.extend((0..30).map(|_| {
        let mut params = Vec::new();
        if rng.below(4) == 0 {
            params.extend((0..7).map(|_| V(E::I32, 4)));
        }
        params.extend((0..=rng.below(6)).map(|_| {
            let leaf = rng.pick(&agg_pool);
            if matches!(leaf, V(..)) && rng.below(3) == 0 {
                ar(2 + rng.below(3) as u32, leaf)
            } else {
                leaf
            }
        }));
        let ret = match rng.below(3) {
            0 => None,
            1 => Some(rng.pick(&agg_pool)),
            _ => Some(ar(2 + rng.below(3) as u32, rng.pick(&pool))),
        };
        Case::new(params, ret)
    }));
    assert!(cases.iter().all(Case::c_ok));
    check("gcc", &cases);
}

/// WP-167: a lone `<1 x i128>` as an argument and as the result: in two
/// general registers at every register number (the even-register rule
/// skips an odd one), split nowhere, on the stack (16-aligned), between
/// other vectors and scalars; in a structure and beside the other `i128`
/// shapes; through a pointer, an `invoke`, a `musttail` forwarder and as
/// a named argument of a variadic function.
#[test]
fn lone_i128_vectors_cross_calls() {
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
    cases.extend(random(&mut rng, &pool, 40, false));
    let mut styled = random(&mut rng, &pool, 18, false);
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
    check("i128", &cases);
}

/// A mask wider than 64 elements is refused by its name, as argument and
/// as result.
#[test]
fn wide_masks_are_refused_by_name() {
    assert_refused(&[
        ("define void @f(<65 x i1> %m) {\n  ret void\n}\n", abi::MASK_WIDE),
        ("define <128 x i1> @f() {\n  ret <128 x i1> zeroinitializer\n}\n", abi::MASK_WIDE),
        (
            "declare void @g({ i32, <100 x i1> })\ndefine void @f() {\n  \
             call void @g({ i32, <100 x i1> } zeroinitializer)\n  ret void\n}\n",
            abi::MASK_WIDE,
        ),
    ]);
}

/// The generator itself (no tools needed): every module parses.
#[test]
fn the_generator_parses() {
    let mut rng = Rng(7);
    let mut pool = masks();
    pool.extend(promoted());
    pool.extend(bytes());
    let mut cases = random(&mut rng, &pool, 20, true);
    cases[0].style = Style::Invoke;
    cases[1].style = Style::Musttail;
    cases[2].style = Style::Indirect;
    cases[3].var = Some(vec![V(E::I32, 4)]);
    cases[3].ret = None;
    cases[3].style = Style::Direct;
    let (_, bodies) = parse(&caller_ll(&cases, 22));
    assert!(bodies.len() >= cases.len());
    let (_, bodies) = parse(&callee_ll(&cases, 22));
    assert_eq!(bodies.len(), cases.len());
    assert_eq!(V(E::I1, 13).bytes(), 2);
    assert_eq!(V(E::I1, 13).last(), 0x1f);
}
