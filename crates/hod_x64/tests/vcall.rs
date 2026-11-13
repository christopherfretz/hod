//! Vectors across calls on x86-64 (WP-132, `spec/codegen.md` §5.6.13,
//! `spec/abi-x64.md` §4.3): every signature class (register vectors,
//! vectors in memory, two-register vectors, masks as the lanes of one
//! `xmm`, complex `float` as `<2 x float>`), with enough arguments that
//! some go on the stack, called between hod's objects and `llc`'s in both
//! directions and checked against the interpreter.

#![allow(
    clippy::format_push_string,
    clippy::similar_names,
    reason = "`.ll` text built by hand, of callers and callees"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/vector_gen.rs"]
mod vector_gen;

use aggr_gen::{header, parse, pic};
use hod_cg::{CgError, Stress};
use vector_gen::{Mixed, Tally, assert_agrees, check_mixed};

/// A lane type: its IR name and bits (`1` for a mask's).
#[derive(Clone, Copy)]
struct Ty {
    elem: &'static str,
    bits: u32,
    n: u32,
}

impl Ty {
    fn ir(self) -> String {
        format!("<{} x {}>", self.n, self.elem)
    }

    fn mask(self) -> bool {
        self.bits == 1
    }

    /// The type of the image a value is loaded as and stored as: its own,
    /// or `<n x i8>` for a mask.
    fn stored(self) -> String {
        if self.mask() { format!("<{} x i8>", self.n) } else { self.ir() }
    }

    /// A global initializer of lane values from `seed`: integers, or bit
    /// patterns of finite floats, or 0/1 for a mask's lanes.
    fn init(self, seed: u64) -> String {
        let int_bits = if self.mask() { 8 } else { self.bits };
        let lanes: Vec<String> = (0..self.n)
            .map(|i| {
                let h = (seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ u64::from(i))
                    .wrapping_mul(0xbf58_476d_1ce4_e5b9);
                let h = h ^ (h >> 29);
                let v = match (self.elem, self.bits) {
                    (_, 1) => h & 1,
                    ("half", _) => h & 0x3bff,
                    ("float", _) => h & 0x3fff_ffff,
                    ("double", _) => h & 0x3fff_ffff_ffff_ffff,
                    (_, 64) => h,
                    (_, b) => h & ((1 << b) - 1),
                };
                let shift = 64 - int_bits;
                format!("i{int_bits} {}", ((v << shift) as i64) >> shift)
            })
            .collect();
        format!("<{} x i{int_bits}> <{}>", self.n, lanes.join(", "))
    }

    /// Lines that load the global `@g` as `%name`.
    fn load(self, name: &str, g: &str) -> String {
        let n = self.n;
        if self.mask() {
            return format!(
                "  %{name}.i = load <{n} x i8>, ptr {g}\n  \
                 %{name} = icmp ne <{n} x i8> %{name}.i, zeroinitializer\n"
            );
        }
        let int = format!("<{n} x i{}>", self.bits);
        let op = if self.elem == "ptr" { "inttoptr" } else { "bitcast" };
        format!(
            "  %{name}.i = load {int}, ptr {g}\n  %{name} = {op} {int} %{name}.i to {}\n",
            self.ir()
        )
    }

    /// Lines that store `%name` at `at`.
    fn store(self, name: &str, at: &str) -> String {
        if self.mask() {
            format!(
                "  %{name}.z = zext {} %{name} to {}\n  store {} %{name}.z, ptr {at}\n",
                self.ir(),
                self.stored(),
                self.stored()
            )
        } else {
            format!("  store {} %{name}, ptr {at}\n", self.ir())
        }
    }

    /// Bytes of the stored image.
    fn bytes(self) -> u32 {
        if self.mask() { self.n } else { self.n * self.bits / 8 }
    }
}

const TYPES: [Ty; 22] = [
    Ty { elem: "i32", bits: 32, n: 4 },
    Ty { elem: "double", bits: 64, n: 2 },
    Ty { elem: "float", bits: 32, n: 4 },
    Ty { elem: "float", bits: 32, n: 2 },
    Ty { elem: "i16", bits: 16, n: 8 },
    Ty { elem: "i8", bits: 8, n: 16 },
    Ty { elem: "i64", bits: 64, n: 2 },
    Ty { elem: "half", bits: 16, n: 8 },
    Ty { elem: "half", bits: 16, n: 4 },
    Ty { elem: "i32", bits: 32, n: 2 },
    Ty { elem: "i8", bits: 8, n: 8 },
    Ty { elem: "ptr", bits: 64, n: 2 },
    Ty { elem: "i32", bits: 32, n: 3 },
    Ty { elem: "i16", bits: 16, n: 5 },
    Ty { elem: "i32", bits: 32, n: 8 },
    Ty { elem: "float", bits: 32, n: 3 },
    Ty { elem: "double", bits: 64, n: 4 },
    Ty { elem: "i8", bits: 8, n: 6 },
    Ty { elem: "i1", bits: 1, n: 2 },
    Ty { elem: "i1", bits: 1, n: 4 },
    Ty { elem: "i1", bits: 1, n: 8 },
    Ty { elem: "i1", bits: 1, n: 16 },
];

/// A parameter: a vector, or a scalar of the given IR type.
#[derive(Clone, Copy)]
enum P {
    V(Ty),
    S(&'static str, u32),
}

/// The call `@c<k>(params) -> ret`: the callee stores every argument and
/// emits them, then returns `ret_body` (lines defining `%r` from the
/// parameters `%p<i>`); the caller loads the arguments from globals,
/// calls, and emits what comes back.
fn case(k: u64, params: &[P], ret: Ty, ret_body: &str) -> Mixed {
    let stride = 64;
    let mut callee =
        format!("@c{k}.o = global [{} x i8] zeroinitializer, align 32\n", params.len() * stride);
    let list: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| match p {
            P::V(t) => format!("{} %p{i}", t.ir()),
            P::S(t, _) => format!("{t} %p{i}"),
        })
        .collect();
    let types: Vec<String> = params
        .iter()
        .map(|p| match p {
            P::V(t) => t.ir(),
            P::S(t, _) => (*t).to_string(),
        })
        .collect();
    let decl = format!("declare {} @c{k}({})", ret.ir(), types.join(", "));
    callee.push_str(&format!("define {} @c{k}({}) {{\nentry:\n", ret.ir(), list.join(", ")));
    for (i, p) in params.iter().enumerate() {
        let at = format!("%at{i}");
        callee.push_str(&format!("  {at} = getelementptr i8, ptr @c{k}.o, i64 {}\n", i * stride));
        match p {
            P::V(t) => callee.push_str(&t.store(&format!("p{i}"), &at)),
            P::S(t, _) => callee.push_str(&format!("  store {t} %p{i}, ptr {at}\n")),
        }
    }
    callee.push_str(&format!(
        "  call void @emit(ptr @c{k}.o, i64 {})\n{ret_body}  ret {} %r\n}}\n\n",
        params.len() * stride,
        ret.ir()
    ));

    let mut caller = String::new();
    let mut body = String::new();
    for (i, p) in params.iter().enumerate() {
        let g = format!("@t{k}.a{i}");
        match p {
            P::V(t) => {
                caller.push_str(&format!("{g} = global {}, align 32\n", t.init(k * 31 + i as u64)));
                body.push_str(&t.load(&format!("a{i}"), &g));
            }
            P::S(t, bits) => {
                let v = (k * 977 + i as u64 * 131) % 1000;
                let init = if t.contains("double") || t.contains("float") {
                    format!("{t} {v}.5")
                } else {
                    format!("{t} {}", v % (1 << (bits - 1).min(16)))
                };
                caller.push_str(&format!("{g} = global {init}, align 32\n"));
                body.push_str(&format!("  %a{i} = load {t}, ptr {g}\n"));
            }
        }
    }
    caller.push_str(&format!("@t{k}.o = global [64 x i8] zeroinitializer, align 32\n"));
    let args: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| match p {
            P::V(t) => format!("{} %a{i}", t.ir()),
            P::S(t, _) => format!("{t} %a{i}"),
        })
        .collect();
    caller.push_str(&format!("define void @t{k}() {{\nentry:\n{body}"));
    caller.push_str(&format!("  %r = call {} @c{k}({})\n", ret.ir(), args.join(", ")));
    caller.push_str(&ret.store("r", &format!("@t{k}.o")));
    caller.push_str(&format!(
        "  call void @emit(ptr @t{k}.o, i64 {})\n  ret void\n}}\n\n",
        ret.bytes()
    ));
    Mixed { seed: k, caller, decl, callee }
}

/// A shuffle of `%x` and `%y` of type `t` into `%r`: a permutation that
/// takes lanes of both.
fn mix(t: Ty, x: &str, y: &str) -> String {
    let n = t.n;
    let entries: Vec<String> = (0..n).map(|j| format!("i32 {}", (j * 3 + 1) % (2 * n))).collect();
    format!(
        "  %r = shufflevector {} %{x}, {} %{y}, <{n} x i32> <{}>\n",
        t.ir(),
        t.ir(),
        entries.join(", ")
    )
}

/// Every signature class, both directions, in registers and on the stack.
#[test]
fn vectors_cross_calls_both_ways() {
    let mut cases = Vec::new();
    let mut k = 0;
    for t in TYPES {
        k += 1;
        cases.push(case(k, &[P::V(t)], t, &mix(t, "p0", "p0")));
        k += 1;
        cases.push(case(k, &[P::V(t), P::V(t)], t, &mix(t, "p0", "p1")));
        // Nine vectors among scalars: the last ones on the stack.
        let mut params = vec![P::S("i64", 64)];
        for i in 0..9 {
            params.push(P::V(t));
            if i % 3 == 0 {
                params.push(P::S("double", 64));
            }
        }
        params.push(P::S("i32", 32));
        let last = params.len() - 2;
        k += 1;
        cases.push(case(k, &params, t, &mix(t, "p1", &format!("p{last}"))));
        // Six general-register scalars first, then the vectors.
        let mut params = vec![P::S("i64", 64); 6];
        params.extend([P::V(t), P::S("float", 32), P::V(t)]);
        k += 1;
        cases.push(case(k, &params, t, &mix(t, "p8", "p6")));
    }
    // Complex `float` as `<2 x float>`, beside `float`s.
    let c = Ty { elem: "float", bits: 32, n: 2 };
    let params = [P::V(c), P::S("float", 32), P::V(c), P::S("double", 64), P::V(c)];
    k += 1;
    cases.push(case(
        k,
        &params,
        c,
        "  %s = insertelement <2 x float> %p0, float %p1, i32 1\n  \
         %r = shufflevector <2 x float> %s, <2 x float> %p4, <2 x i32> <i32 1, i32 2>\n",
    ));
    // Masks and other vectors in one signature.
    let m = Ty { elem: "i1", bits: 1, n: 8 };
    let v = Ty { elem: "i16", bits: 16, n: 8 };
    k += 1;
    cases.push(case(
        k,
        &[P::V(m), P::V(v), P::V(m), P::S("i8", 8), P::V(v)],
        v,
        "  %r = select <8 x i1> %p2, <8 x i16> %p1, <8 x i16> %p4\n",
    ));

    let mut tally = Tally::default();
    for (i, chunk) in cases.chunks(24).enumerate() {
        check_mixed(&format!("calls{i}"), chunk, &mut tally);
    }
    eprintln!(
        "x64 vector calls: {} calls, {} compared, legs {}, {} undefined {:?}, llc disagreements \
         {:?}, llc failures {:?}, against llc {:?}; text hod/llc22 {}/{}",
        tally.generated,
        tally.run,
        tally.legs.len(),
        tally.trapped,
        tally.traps,
        tally.llc_wrong,
        tally.llc_crashed,
        tally.by_llc,
        tally.text.0,
        tally.text.1
    );
    assert_agrees(&tally);
    assert_eq!(tally.run, tally.generated, "every call is defined");
}

/// Masks the convention has no lanes for are refused by name.
#[test]
fn masks_without_lanes_are_refused_by_name() {
    for n in [3, 32, 64] {
        let text = format!(
            "{}define <{n} x i1> @f(<{n} x i1> %v) {{\n  ret <{n} x i1> %v\n}}\n",
            header()
        );
        let (module, bodies) = parse(&text);
        match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported("x64:mask-vector"), "{n}");
            }
            other => panic!("<{n} x i1> not refused: {:?}", other.map(|o| o.len())),
        }
    }
}
