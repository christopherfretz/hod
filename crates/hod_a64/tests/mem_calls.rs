//! `Mem`-class arguments and results across calls (WP-066; WP-040's
//! follow-up; `spec/codegen.md` §7.1, `spec/abi-a64.md` §3, §7): random
//! signatures whose arguments and results are wide integers (`i65` ..
//! `i256`), aggregates (odd-width leaves, `fp128` leaves, arrays, nests,
//! homogeneous blocks) and `fp128`, after fillers that push them onto the
//! stack, with `sret` results and `byval` arguments. A case is a caller
//! (module X) that builds the arguments from four seeds, calls the callee
//! (module Y) and folds the result into an `i64`; the callee folds every
//! argument (and, from memory, every `Mem`-class argument it stored and
//! the byte above each odd-width leaf, which §2.2 makes zero) and builds
//! its result from that. X and Y are built each by hod (plain and
//! stressed) and by `llc -O0 -global-isel=0 -fast-isel=0` in every
//! pairing, and for the signatures C can spell (`__int128`,
//! `long double`, structures) by gcc as well, in both directions; every
//! build must print what the `llc`-only build prints.
//!
//! `HOD_MEM_CALL_COUNT=<n>` sets the cases of `many_mem_calls_agree`.

#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "IR text generation: short names of values and blocks"
)]

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;

use common::linking::Toolchain;
use common::{ll_header, llc, llc_major, parse, write_object};
use hod_a64::compile_module;
use hod_cg::Options;
use hod_testutil::Scratch;

const K: u64 = 0x9e37_79b9_7f4a_7c15;

/// `SplitMix64`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(K);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A scalar leaf.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leaf {
    Int(u32),
    F32,
    F64,
    F128,
}

impl Leaf {
    fn ll(self) -> String {
        match self {
            Leaf::Int(n) => format!("i{n}"),
            Leaf::F32 => "float".into(),
            Leaf::F64 => "double".into(),
            Leaf::F128 => "fp128".into(),
        }
    }
}

/// A type: its `.ll` spelling, its leaves with their index paths, and its
/// C spelling when C has one that is passed alike.
#[derive(Clone, Debug)]
struct Ty {
    ll: String,
    leaves: Vec<(Leaf, Vec<u32>)>,
    c: Option<&'static str>,
}

fn scalar(leaf: Leaf) -> Ty {
    let c = match leaf {
        Leaf::Int(64) => Some("uint64_t"),
        Leaf::Int(128) => Some("unsigned __int128"),
        Leaf::F64 => Some("double"),
        Leaf::F128 => Some("long double"),
        _ => None,
    };
    Ty { ll: leaf.ll(), leaves: vec![(leaf, vec![])], c }
}

fn structure(fields: &[Ty]) -> Ty {
    let ll =
        format!("{{ {} }}", fields.iter().map(|f| f.ll.as_str()).collect::<Vec<_>>().join(", "));
    let mut leaves = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        for (leaf, path) in &f.leaves {
            let mut p = vec![i as u32];
            p.extend(path);
            leaves.push((*leaf, p));
        }
    }
    Ty { ll, leaves, c: None }
}

fn array(elem: &Ty, len: u32) -> Ty {
    let ll = format!("[{len} x {}]", elem.ll);
    let mut leaves = Vec::new();
    for i in 0..len {
        for (leaf, path) in &elem.leaves {
            let mut p = vec![i];
            p.extend(path);
            leaves.push((*leaf, p));
        }
    }
    Ty { ll, leaves, c: None }
}

/// The `Mem`-class (and `fp128`) types the generator draws.
fn pool() -> Vec<Ty> {
    let i = |n| scalar(Leaf::Int(n));
    let mut s2l = array(&i(64), 2);
    s2l.c = Some("struct s2l");
    let mut b5 = array(&i(64), 5);
    b5.c = Some("struct b5");
    vec![
        i(65),
        i(96),
        i(127),
        i(128),
        i(129),
        i(192),
        i(200),
        i(256),
        scalar(Leaf::F128),
        s2l,
        b5,
        structure(&[i(17), i(64)]),
        structure(&[i(7), i(17), i(33)]),
        array(&i(9), 2),
        structure(&[i(128), i(8)]),
        structure(&[scalar(Leaf::F128), i(64)]),
        array(&i(32), 3),
        structure(&[scalar(Leaf::F64), i(64)]),
        structure(&[scalar(Leaf::F32), scalar(Leaf::F32), scalar(Leaf::F32)]),
        array(&scalar(Leaf::F128), 2),
        structure(&[i(8), array(&i(65), 2)]),
        structure(&[array(&i(16), 2), i(48)]),
        structure(&[i(1), i(64)]),
        structure(&[i(7), array(&i(9), 2), i(33)]),
        array(&scalar(Leaf::F64), 4),
    ]
}

/// The fillers that use up argument registers.
fn fillers() -> [Ty; 2] {
    [scalar(Leaf::Int(64)), scalar(Leaf::F64)]
}

/// How a parameter is passed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pass {
    Value,
    ByVal,
}

/// One case: parameters, result (`sret` when `sret`).
#[derive(Clone, Debug)]
struct Case {
    params: Vec<(Ty, Pass)>,
    ret: Option<Ty>,
    sret: bool,
}

impl Case {
    fn c_ok(&self) -> bool {
        self.params.iter().all(|(t, p)| *p == Pass::Value && t.c.is_some())
            && self.ret.as_ref().is_none_or(|t| t.c.is_some())
    }
}

fn random_case(rng: &mut Rng, pool: &[Ty], c_only: bool) -> Case {
    let usable: Vec<&Ty> = pool.iter().filter(|t| !c_only || t.c.is_some()).collect();
    // C passes a structure of more than 16 bytes by reference, IR an
    // array of five `i64` in registers: `struct b5` is a C result only
    // (by `sret`).
    let args: Vec<&Ty> =
        usable.iter().copied().filter(|t| !c_only || t.c != Some("struct b5")).collect();
    let mut params = Vec::new();
    // Fillers: none, a few, or enough to push what follows to the stack.
    let fills = [0, 0, 1, 3, 6, 7, 8][rng.below(7)];
    let ffills = [0, 0, 2, 7, 8][rng.below(5)];
    let [int, fp] = fillers();
    for _ in 0..fills {
        params.push((int.clone(), Pass::Value));
    }
    for _ in 0..ffills {
        params.push((fp.clone(), Pass::Value));
    }
    for _ in 0..=rng.below(3) {
        let t = args[rng.below(args.len())].clone();
        let pass =
            if !c_only && t.c.is_none() && rng.below(5) == 0 { Pass::ByVal } else { Pass::Value };
        params.push((t, pass));
        if rng.below(3) == 0 {
            let f = if rng.below(2) == 0 { int.clone() } else { fp.clone() };
            params.push((f, Pass::Value));
        }
    }
    let ret = (rng.below(6) != 0).then(|| usable[rng.below(usable.len())].clone());
    let b5 = ret.as_ref().is_some_and(|t| t.c == Some("struct b5"));
    let sret = if c_only { b5 } else { ret.is_some() && rng.below(5) == 0 };
    Case { params, ret, sret }
}

/// A function body under construction: fresh names.
struct Body {
    text: String,
    n: usize,
}

impl Body {
    fn new() -> Body {
        Body { text: String::new(), n: 0 }
    }

    fn fresh(&mut self) -> String {
        self.n += 1;
        format!("%t{}", self.n)
    }

    fn line(&mut self, line: &str) {
        self.text.push_str("  ");
        self.text.push_str(line);
        self.text.push('\n');
    }

    /// `h = h * 1000003 ^ chunk`.
    fn mix(&mut self, h: &str, chunk: &str) -> String {
        let m = self.fresh();
        self.line(&format!("{m} = mul i64 {h}, 1000003"));
        let x = self.fresh();
        self.line(&format!("{x} = xor i64 {m}, {chunk}"));
        x
    }

    /// The seed of leaf `j` from seeds `s`: `s[j%4] * (2j+1) ^ s[(j+1)%4]`.
    fn seed(&mut self, s: &[String; 4], j: usize) -> String {
        let m = self.fresh();
        self.line(&format!("{m} = mul i64 {}, {}", s[j % 4], 2 * j + 1));
        let x = self.fresh();
        self.line(&format!("{x} = xor i64 {m}, {}", s[(j + 1) % 4]));
        x
    }

    /// A leaf from seed `x`: word `m` of a wide value is `x ^ m*K`.
    fn make_leaf(&mut self, leaf: Leaf, x: &str) -> String {
        let int = |b: &mut Body, n: u32| -> String {
            if n == 64 {
                return x.to_owned();
            }
            if n < 64 {
                let t = b.fresh();
                b.line(&format!("{t} = trunc i64 {x} to i{n}"));
                return t;
            }
            let words = n.div_ceil(64);
            let wide = words * 64;
            let mut acc = String::new();
            for m in 0..words {
                let w = b.fresh();
                b.line(&format!("{w} = xor i64 {x}, {}", (u64::from(m)).wrapping_mul(K) as i64));
                let z = b.fresh();
                b.line(&format!("{z} = zext i64 {w} to i{wide}"));
                let s = if m == 0 {
                    z
                } else {
                    let s = b.fresh();
                    b.line(&format!("{s} = shl i{wide} {z}, {}", 64 * m));
                    s
                };
                acc = if m == 0 {
                    s
                } else {
                    let o = b.fresh();
                    b.line(&format!("{o} = or i{wide} {acc}, {s}"));
                    o
                };
            }
            if wide == n {
                acc
            } else {
                let t = b.fresh();
                b.line(&format!("{t} = trunc i{wide} {acc} to i{n}"));
                t
            }
        };
        match leaf {
            Leaf::Int(n) => int(self, n),
            Leaf::F32 => {
                let t = int(self, 32);
                let f = self.fresh();
                self.line(&format!("{f} = bitcast i32 {t} to float"));
                f
            }
            Leaf::F64 => {
                let f = self.fresh();
                self.line(&format!("{f} = bitcast i64 {x} to double"));
                f
            }
            Leaf::F128 => {
                let t = int(self, 128);
                let f = self.fresh();
                self.line(&format!("{f} = bitcast i128 {t} to fp128"));
                f
            }
        }
    }

    /// A value of `ty`, its leaves seeded from `s` starting at leaf `*j`.
    fn make(&mut self, ty: &Ty, s: &[String; 4], j: &mut usize) -> String {
        if ty.leaves.len() == 1 && ty.leaves[0].1.is_empty() {
            let x = self.seed(s, *j);
            *j += 1;
            return self.make_leaf(ty.leaves[0].0, &x);
        }
        let mut acc = "poison".to_owned();
        for (leaf, path) in &ty.leaves {
            let x = self.seed(s, *j);
            *j += 1;
            let v = self.make_leaf(*leaf, &x);
            let n = self.fresh();
            let idx: Vec<String> = path.iter().map(u32::to_string).collect();
            self.line(&format!(
                "{n} = insertvalue {} {acc}, {} {v}, {}",
                ty.ll,
                leaf.ll(),
                idx.join(", ")
            ));
            acc = n;
        }
        acc
    }

    /// Folds leaf `v` into `h`.
    fn fold_leaf(&mut self, h: &str, leaf: Leaf, v: &str) -> String {
        let (n, bits) = match leaf {
            Leaf::Int(n) => (n, v.to_owned()),
            Leaf::F32 | Leaf::F64 | Leaf::F128 => {
                let n = match leaf {
                    Leaf::F32 => 32,
                    Leaf::F64 => 64,
                    _ => 128,
                };
                let b = self.fresh();
                self.line(&format!("{b} = bitcast {} {v} to i{n}", leaf.ll()));
                (n, b)
            }
        };
        if n <= 64 {
            let z = if n == 64 {
                bits
            } else {
                let z = self.fresh();
                self.line(&format!("{z} = zext i{n} {bits} to i64"));
                z
            };
            return self.mix(h, &z);
        }
        let mut h = h.to_owned();
        for m in 0..n.div_ceil(64) {
            let s = if m == 0 {
                bits.clone()
            } else {
                let s = self.fresh();
                self.line(&format!("{s} = lshr i{n} {bits}, {}", 64 * m));
                s
            };
            let t = self.fresh();
            self.line(&format!("{t} = trunc i{n} {s} to i64"));
            h = self.mix(&h, &t);
        }
        h
    }

    /// Folds `v` of `ty` into `h`.
    fn fold(&mut self, h: &str, ty: &Ty, v: &str) -> String {
        let mut h = h.to_owned();
        for (leaf, path) in &ty.leaves {
            let e = if path.is_empty() {
                v.to_owned()
            } else {
                let e = self.fresh();
                let idx: Vec<String> = path.iter().map(u32::to_string).collect();
                self.line(&format!("{e} = extractvalue {} {v}, {}", ty.ll, idx.join(", ")));
                e
            };
            h = self.fold_leaf(&h, *leaf, &e);
        }
        h
    }

    /// Folds into `h` the byte above each odd-width integer leaf of the
    /// image of `ty` at `ptr`, masked to the bits above the width (zero
    /// in memory, `spec/codegen.md` §2.2).
    fn fold_junk(&mut self, h: &str, ty: &Ty, ptr: &str) -> String {
        let mut h = h.to_owned();
        for (leaf, path) in &ty.leaves {
            let Leaf::Int(n) = *leaf else { continue };
            if n % 8 == 0 {
                continue;
            }
            let at = if path.is_empty() {
                ptr.to_owned()
            } else {
                let g = self.fresh();
                let idx: Vec<String> = path.iter().map(|i| format!("i32 {i}")).collect();
                self.line(&format!(
                    "{g} = getelementptr inbounds {}, ptr {ptr}, i32 0, {}",
                    ty.ll,
                    idx.join(", ")
                ));
                g
            };
            let byte = self.fresh();
            self.line(&format!("{byte} = getelementptr inbounds i8, ptr {at}, i64 {}", n / 8));
            let b = self.fresh();
            self.line(&format!("{b} = load i8, ptr {byte}"));
            let m = self.fresh();
            self.line(&format!("{m} = and i8 {b}, {}", (0xffu8 << (n % 8)) as i8));
            let z = self.fresh();
            self.line(&format!("{z} = zext i8 {m} to i64"));
            h = self.mix(&h, &z);
        }
        h
    }
}

fn param_decl(t: &Ty, pass: Pass) -> String {
    match pass {
        Pass::Value => t.ll.clone(),
        Pass::ByVal => format!("ptr byval({}) align 16", t.ll),
    }
}

/// The callee `@y{k}` (module Y) and the caller `@x{k}` (module X);
/// `through_memory` adds the callee's round trip of its `Mem`-class
/// arguments through globals, which the C versions do not mirror.
#[allow(clippy::too_many_lines, reason = "the callee, then the caller")]
fn ir(k: usize, case: &Case, through_memory: bool) -> (String, String) {
    let ret_ll = if case.sret || case.ret.is_none() {
        "void".to_owned()
    } else {
        case.ret.as_ref().map(|t| t.ll.clone()).unwrap_or_default()
    };
    let mut decl: Vec<String> = Vec::new();
    if case.sret {
        decl.push(format!(
            "ptr sret({}) align 16",
            case.ret.as_ref().map(|t| t.ll.as_str()).unwrap_or_default()
        ));
    }
    decl.extend(case.params.iter().map(|(t, p)| param_decl(t, *p)));
    // The callee.
    let mut y = Body::new();
    let mut h = "0".to_owned();
    let mut globals = String::new();
    for (i, (t, pass)) in case.params.iter().enumerate() {
        let p = format!("%p{i}");
        let v = match pass {
            Pass::Value => p.clone(),
            Pass::ByVal => {
                let v = y.fresh();
                y.line(&format!("{v} = load {}, ptr {p}, align 16", t.ll));
                // The callee owns its copy: the caller's must not change.
                let zero = "zeroinitializer";
                y.line(&format!("store {} {zero}, ptr {p}, align 16", t.ll));
                v
            }
        };
        h = y.fold(&h, t, &v);
        if through_memory && (t.leaves.len() > 1 || matches!(t.leaves[0].0, Leaf::Int(n) if n > 64))
        {
            // Through memory: the stored image, reloaded and its bytes
            // above odd widths.
            writeln!(globals, "@g{k}_{i} = internal global {} zeroinitializer, align 16", t.ll)
                .unwrap();
            y.line(&format!("store {} {v}, ptr @g{k}_{i}, align 16", t.ll));
            let l = y.fresh();
            y.line(&format!("{l} = load {}, ptr @g{k}_{i}, align 16", t.ll));
            h = y.fold(&h, t, &l);
            h = y.fold_junk(&h, t, &format!("@g{k}_{i}"));
        }
    }
    let mut params: Vec<String> = Vec::new();
    if case.sret {
        params.push(format!("{} %out", decl[0]));
    }
    let skip = usize::from(case.sret);
    for (i, d) in decl[skip..].iter().enumerate() {
        params.push(format!("{d} %p{i}"));
    }
    match &case.ret {
        None => {
            writeln!(globals, "@r{k} = global i64 0").unwrap();
            y.line(&format!("store i64 {h}, ptr @r{k}"));
            y.line("ret void");
        }
        Some(t) => {
            let s = [h.clone(), y.fresh(), y.fresh(), y.fresh()];
            for (i, name) in s.iter().enumerate().skip(1) {
                y.line(&format!("{name} = mul i64 {h}, {}", 2 * i + 1));
            }
            let mut j = 0;
            let r = y.make(t, &s, &mut j);
            if case.sret {
                y.line(&format!("store {} {r}, ptr %out, align 16", t.ll));
                y.line("ret void");
            } else {
                y.line(&format!("ret {} {r}", t.ll));
            }
        }
    }
    let callee = format!(
        "{globals}define {ret_ll} @y{k}({}) noinline {{\n{}}}\n",
        params.join(", "),
        y.text
    );
    // The caller.
    let mut x = Body::new();
    let s = ["%s0".to_owned(), "%s1".to_owned(), "%s2".to_owned(), "%s3".to_owned()];
    let mut j = 0;
    let mut args = Vec::new();
    let mut copies = Vec::new();
    if let (true, Some(t)) = (case.sret, &case.ret) {
        x.line(&format!("%out = alloca {}, align 16", t.ll));
        args.push(format!("ptr sret({}) align 16 %out", t.ll));
    }
    for (i, (t, pass)) in case.params.iter().enumerate() {
        let v = x.make(t, &s, &mut j);
        match pass {
            Pass::Value => args.push(format!("{} {v}", t.ll)),
            Pass::ByVal => {
                let a = format!("%bv{i}");
                x.line(&format!("{a} = alloca {}, align 16", t.ll));
                x.line(&format!("store {} {v}, ptr {a}, align 16", t.ll));
                args.push(format!("ptr byval({}) align 16 {a}", t.ll));
                copies.push((t, a));
            }
        }
    }
    let mut out = String::new();
    let call = format!("@y{k}({})", args.join(", "));
    let mut h = "0".to_owned();
    match &case.ret {
        None => {
            x.line(&format!("call void {call}"));
            let l = x.fresh();
            x.line(&format!("{l} = load i64, ptr @r{k}"));
            h = x.mix(&h, &l);
            writeln!(out, "@r{k} = external global i64").unwrap();
        }
        Some(t) if case.sret => {
            x.line(&format!("call void {call}"));
            let l = x.fresh();
            x.line(&format!("{l} = load {}, ptr %out, align 16", t.ll));
            h = x.fold(&h, t, &l);
            h = x.fold_junk(&h, t, "%out");
        }
        Some(t) => {
            let r = x.fresh();
            x.line(&format!("{r} = call {} {call}", t.ll));
            h = x.fold(&h, t, &r);
            // The result through memory, with the bytes above odd widths.
            let a = x.fresh();
            x.line(&format!("{a} = alloca {}, align 16", t.ll));
            x.line(&format!("store {} {r}, ptr {a}, align 16", t.ll));
            h = x.fold_junk(&h, t, &a);
        }
    }
    for (t, a) in copies {
        let l = x.fresh();
        x.line(&format!("{l} = load {}, ptr {a}, align 16", t.ll));
        h = x.fold(&h, t, &l);
    }
    x.line(&format!("ret i64 {h}"));
    let caller = format!(
        "{out}declare {ret_ll} @y{k}({})\ndefine i64 @x{k}(i64 %s0, i64 %s1, i64 %s2, i64 %s3) {{\n{}}}\n",
        decl.join(", "),
        x.text
    );
    (callee, caller)
}

/// C versions of the callee and the caller of a case C can spell, with
/// the same folds (the leaves in order, `h = h * 1000003 ^ chunk`).
fn c_source(k: usize, case: &Case, callee: bool) -> String {
    let mut c = String::new();
    let ret = case.ret.as_ref().map_or("void", |t| t.c.unwrap_or_default());
    let params: Vec<String> = case
        .params
        .iter()
        .enumerate()
        .map(|(i, (t, _))| format!("{} p{i}", t.c.unwrap_or_default()))
        .collect();
    let params = if params.is_empty() { "void".to_owned() } else { params.join(", ") };
    if callee {
        if case.ret.is_none() {
            writeln!(c, "uint64_t r{k};").unwrap();
        }
        writeln!(c, "{ret} y{k}({params}) {{\n  uint64_t h = 0;").unwrap();
        for (i, (t, _)) in case.params.iter().enumerate() {
            writeln!(c, "  h = fold_{}(h, p{i});", c_name(t)).unwrap();
        }
        match &case.ret {
            None => writeln!(c, "  r{k} = h;\n}}").unwrap(),
            Some(t) => writeln!(
                c,
                "  uint64_t s[4] = {{ h, h * 3, h * 5, h * 7 }};\n  unsigned j = 0;\n  \
                 return make_{}(s, &j);\n}}",
                c_name(t)
            )
            .unwrap(),
        }
    } else {
        if case.ret.is_none() {
            writeln!(c, "extern uint64_t r{k};").unwrap();
        }
        writeln!(c, "{ret} y{k}({params});").unwrap();
        writeln!(
            c,
            "uint64_t x{k}(uint64_t s0, uint64_t s1, uint64_t s2, uint64_t s3) {{\n  \
             uint64_t s[4] = {{ s0, s1, s2, s3 }};\n  unsigned j = 0;"
        )
        .unwrap();
        let mut args = Vec::new();
        for (i, (t, _)) in case.params.iter().enumerate() {
            writeln!(c, "  {} a{i} = make_{}(s, &j);", t.c.unwrap_or_default(), c_name(t)).unwrap();
            args.push(format!("a{i}"));
        }
        match &case.ret {
            None => writeln!(c, "  y{k}({});\n  return 0 * 1000003 ^ r{k};\n}}", args.join(", "))
                .unwrap(),
            Some(t) => writeln!(
                c,
                "  {} r = y{k}({});\n  return fold_{}(0, r);\n}}",
                t.c.unwrap_or_default(),
                args.join(", "),
                c_name(t)
            )
            .unwrap(),
        }
    }
    c
}

fn c_name(t: &Ty) -> &'static str {
    match t.c {
        Some("uint64_t") => "u64",
        Some("unsigned __int128") => "u128",
        Some("double") => "f64",
        Some("long double") => "f128",
        Some("struct s2l") => "s2l",
        Some("struct b5") => "b5",
        _ => unreachable!("{t:?}"),
    }
}

/// The C helpers `make_*` and `fold_*` of the C types.
const C_PRELUDE: &str = r"#include <stdint.h>
#include <string.h>
#define K 0x9e3779b97f4a7c15ULL
struct s2l { uint64_t a[2]; };
struct b5 { uint64_t a[5]; };
static uint64_t seed(const uint64_t *s, unsigned *j) {
  unsigned i = (*j)++;
  return s[i % 4] * (2 * i + 1) ^ s[(i + 1) % 4];
}
static uint64_t mix(uint64_t h, uint64_t c) { return h * 1000003 ^ c; }
static uint64_t make_u64(const uint64_t *s, unsigned *j) { return seed(s, j); }
static double make_f64(const uint64_t *s, unsigned *j) {
  uint64_t x = seed(s, j); double d; memcpy(&d, &x, 8); return d;
}
static unsigned __int128 make_u128(const uint64_t *s, unsigned *j) {
  uint64_t x = seed(s, j);
  return (unsigned __int128)x | (unsigned __int128)(x ^ K) << 64;
}
static long double make_f128(const uint64_t *s, unsigned *j) {
  unsigned __int128 v = make_u128(s, j); long double d; memcpy(&d, &v, 16); return d;
}
static struct s2l make_s2l(const uint64_t *s, unsigned *j) {
  struct s2l r; for (int i = 0; i < 2; i++) r.a[i] = seed(s, j); return r;
}
static struct b5 make_b5(const uint64_t *s, unsigned *j) {
  struct b5 r; for (int i = 0; i < 5; i++) r.a[i] = seed(s, j); return r;
}
static uint64_t fold_u64(uint64_t h, uint64_t v) { return mix(h, v); }
static uint64_t fold_f64(uint64_t h, double v) { uint64_t x; memcpy(&x, &v, 8); return mix(h, x); }
static uint64_t fold_u128(uint64_t h, unsigned __int128 v) {
  return mix(mix(h, (uint64_t)v), (uint64_t)(v >> 64));
}
static uint64_t fold_f128(uint64_t h, long double v) {
  unsigned __int128 x; memcpy(&x, &v, 16); return fold_u128(h, x);
}
static uint64_t fold_s2l(uint64_t h, struct s2l v) { for (int i = 0; i < 2; i++) h = mix(h, v.a[i]); return h; }
static uint64_t fold_b5(uint64_t h, struct b5 v) { for (int i = 0; i < 5; i++) h = mix(h, v.a[i]); return h; }
";

/// The driver: every case's caller with seeds, one line each.
fn driver(count: usize, seeds: &[[u64; 4]]) -> String {
    let mut c = String::from("#include <stdio.h>\n#include <stdint.h>\n");
    for k in 0..count {
        writeln!(c, "uint64_t x{k}(uint64_t, uint64_t, uint64_t, uint64_t);").unwrap();
    }
    c.push_str("int main(void) {\n");
    for k in 0..count {
        for s in seeds {
            writeln!(
                c,
                "  printf(\"%llu\\n\", (unsigned long long)x{k}({}ULL, {}ULL, {}ULL, {}ULL));",
                s[0], s[1], s[2], s[3]
            )
            .unwrap();
        }
    }
    c.push_str("  return 0;\n}\n");
    c
}

/// Who builds a module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum By {
    Hod,
    HodStress,
    Llc,
    Gcc,
}

/// Runs `count` cases from `seed` (only the ones C can spell when
/// `c_only`), every pairing of `builders` for X and Y. Returns the calls
/// checked per build.
fn run(label: &str, seed: u64, count: usize, c_only: bool, builders: &[By]) -> usize {
    let mut rng = Rng(seed);
    let pool = pool();
    let cases: Vec<Case> = (0..count).map(|_| random_case(&mut rng, &pool, c_only)).collect();
    let mut y_text = String::new();
    let mut x_text = String::new();
    let mut y_c = String::from(C_PRELUDE);
    let mut x_c = String::from(C_PRELUDE);
    for (k, case) in cases.iter().enumerate() {
        let (y, x) = ir(k, case, !c_only);
        y_text.push_str(&y);
        x_text.push_str(&x);
        if c_only {
            assert!(case.c_ok(), "{case:?}");
            y_c.push_str(&c_source(k, case, true));
            x_c.push_str(&c_source(k, case, false));
        }
    }
    let seeds: Vec<[u64; 4]> = (0..3)
        .map(|i| match i {
            0 => [0, 1, u64::MAX, 0x8000_0000_0000_0000],
            _ => [rng.next(), rng.next(), rng.next(), rng.next()],
        })
        .collect();
    let dir = Scratch::new(label);
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return 0 };
    let drv = toolchain.compile_c(&dir, "driver", &driver(count, &seeds), &[]);
    let build = |by: By, name: &str, text: &str, c: &str| -> Option<PathBuf> {
        match by {
            By::Llc => {
                llc(&dir, &format!("{name}-llc"), &format!("{}{text}", ll_header(llc_major())), &[])
            }
            By::Gcc => Some(toolchain.compile_c(&dir, &format!("{name}-gcc"), c, &["-O0"])),
            By::Hod | By::HodStress => {
                let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
                let options = if by == By::Hod {
                    Options::default()
                } else {
                    Options { stress: hod_cg::Stress::ALL, ..Options::default() }
                };
                let emitted = compile_module(&module, &bodies, &options)
                    .unwrap_or_else(|e| panic!("{label} {name}: {e}\n{text}"));
                Some(write_object(&dir, &format!("{name}-{by:?}.o"), &emitted))
            }
        }
    };
    let mut objects_x = Vec::new();
    let mut objects_y = Vec::new();
    for &by in builders {
        let Some(x) = build(by, "x", &x_text, &x_c) else { return 0 };
        let Some(y) = build(by, "y", &y_text, &y_c) else { return 0 };
        objects_x.push((by, x));
        objects_y.push((by, y));
    }
    let mut reference: Option<String> = None;
    // The `llc`-only pairing first: the reference.
    let order: Vec<(usize, usize)> = {
        let l = builders.iter().position(|&b| b == By::Llc).expect("llc is a builder");
        let mut o = vec![(l, l)];
        for i in 0..builders.len() {
            for j in 0..builders.len() {
                if (i, j) != (l, l) {
                    o.push((i, j));
                }
            }
        }
        o
    };
    for (i, j) in order {
        let (bx, x) = &objects_x[i];
        let (by, y) = &objects_y[j];
        // Every linker for the pairings of hod with hod, the first else.
        let all = *bx == By::Hod && *by == By::Hod;
        let linkers = if all { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let exe = dir.join(format!("p-{bx:?}-{by:?}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&drv, x, y], &exe);
            assert!(ran.success(), "{label} X {bx:?} Y {by:?}: {}", ran.stderr_text());
            let got = ran.stdout_text();
            let want = reference.get_or_insert_with(|| got.clone());
            if &got != want {
                let at = got.lines().zip(want.lines()).position(|(g, w)| g != w).unwrap_or(0);
                let k = at / seeds.len();
                let (yk, xk) = ir(k, &cases[k], !c_only);
                panic!(
                    "{label}: X by {bx:?}, Y by {by:?}, {}: case {k} differs\n{:?}\n{yk}\n{xk}",
                    linker.name, cases[k]
                );
            }
        }
    }
    count * seeds.len()
}

const IR_BUILDERS: [By; 3] = [By::Llc, By::Hod, By::HodStress];
const C_BUILDERS: [By; 4] = [By::Llc, By::Hod, By::HodStress, By::Gcc];

/// IR-only signatures, hod and `llc` in every pairing.
#[test]
fn mem_class_arguments_and_results_agree_with_llc() {
    let calls = run("a64-mem-calls", 0x66_0040, 150, false, &IR_BUILDERS);
    eprintln!("a64 mem calls: {calls} calls agree in every pairing");
}

/// What C can spell (`__int128`, `long double`, structures, `sret`),
/// hod, `llc` and gcc in every pairing.
#[test]
fn mem_class_arguments_and_results_agree_with_gcc() {
    let calls = run("a64-mem-calls-gcc", 0x66_0041, 100, true, &C_BUILDERS);
    eprintln!("a64 mem calls with gcc: {calls} calls agree in every pairing");
}

/// Many more.
#[test]
#[ignore = "a minute; run with --ignored"]
fn many_mem_calls_agree() {
    let count =
        std::env::var("HOD_MEM_CALL_COUNT").ok().and_then(|t| t.parse().ok()).unwrap_or(1500);
    let a = run("a64-mem-calls-many", 0x66_1040, count, false, &IR_BUILDERS);
    let b = run("a64-mem-calls-gcc-many", 0x66_1041, count, true, &C_BUILDERS);
    eprintln!("a64 mem calls: {a} + {b} calls agree in every pairing");
}
