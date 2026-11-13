//! The x86-64 vector group (WP-132, `spec/codegen.md` §5.6.13): what
//! compiles, what is refused by name, and the lane edge cases
//! (`extractelement`/`insertelement` indices, `shufflevector` entries)
//! checked against `hod_interp` and `llc` under the emulator.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/vector_gen.rs"]
mod vector_gen;

use aggr_gen::{header, parse, pic};
use hod_cg::{CgError, Stress};

/// hod's result on `body` (functions and declarations after the header).
fn compile(body: &str) -> Result<Vec<u8>, String> {
    let text = format!("{}\n{body}", header());
    let (module, bodies) = parse(&text);
    let mut last = Ok(Vec::new());
    for stress in [Stress::default(), Stress::ALL] {
        last = match hod_x64::compile_to_object(&module, &bodies, &pic(stress)) {
            Ok(bytes) => Ok(bytes),
            Err(hod_x64::ModuleError::Function {
                error: CgError::Unsupported { construct },
                ..
            }) => Err(construct.to_string()),
            Err(e) => panic!("hod: {e}\n{text}"),
        };
    }
    last
}

#[track_caller]
fn compiles(body: &str) {
    if let Err(e) = compile(body) {
        panic!("refused {e}:\n{body}");
    }
}

#[test]
fn rounding_lanes_call_the_library() {
    compiles(
        "declare <4 x float> @llvm.round.v4f32(<4 x float>)\n\
         define <4 x float> @f(<4 x float> %a) {\n\
           %r = call <4 x float> @llvm.round.v4f32(<4 x float> %a)\n\
           ret <4 x float> %r\n}\n",
    );
}

/// A vector type: element, lanes.
#[derive(Clone, Copy)]
struct V(&'static str, u32);

impl V {
    fn ir(self) -> String {
        format!("<{} x {}>", self.1, self.0)
    }

    fn elem_bytes(self) -> u32 {
        match self.0 {
            "i8" => 1,
            "i16" | "half" => 2,
            "i32" | "float" => 4,
            _ => 8,
        }
    }

    fn bytes(self) -> u32 {
        self.1 * self.elem_bytes()
    }

    /// A constant with distinct bytes in every lane (integers; the
    /// floating-point types by their bits through a bitcast).
    fn init(self, k: u32) -> String {
        let int = match self.elem_bytes() {
            1 => "i8",
            2 => "i16",
            4 => "i32",
            _ => "i64",
        };
        let lanes: Vec<String> = (0..self.1)
            .map(|i| {
                let mut v: u64 = 0;
                for b in 0..self.elem_bytes() {
                    v |= u64::from((k * 37 + i * 11 + b * 3 + 1) as u8 | 0x10) << (8 * b);
                }
                let shift = 64 - 8 * self.elem_bytes();
                format!("{int} {}", ((v << shift) as i64) >> shift)
            })
            .collect();
        format!("<{} x {int}> <{}>", self.1, lanes.join(", "))
    }
}

/// The shapes the edge cases run on: `Fpr` (8 and 16 bytes) and `Mem`.
const SHAPES: [V; 11] = [
    V("i8", 16),
    V("i8", 8),
    V("i16", 8),
    V("i16", 4),
    V("i32", 4),
    V("i32", 2),
    V("i64", 2),
    V("float", 4),
    V("double", 2),
    V("i32", 3),
    V("i16", 5),
];

/// The integer type of the lanes of `v` (the bitcast's).
fn int_of(v: V) -> V {
    let e = match v.elem_bytes() {
        1 => "i8",
        2 => "i16",
        4 => "i32",
        _ => "i64",
    };
    V(e, v.1)
}

/// A function `@t<k>` loading a vector of `a` (and of `b`) from globals,
/// running `body` (which defines `%r` of type `r` from `%a`, `%b`, `%i`,
/// `%x`) and emitting `%r`; `%i` is the `i32` from a global, `%x` an
/// element.
#[allow(clippy::many_single_char_names, reason = "the names of the IR values")]
fn edge(k: u64, a: V, b: V, i: i64, r: &str, r_bytes: u32, body: &str) -> (u64, Option<String>) {
    let (ia, ib) = (int_of(a), int_of(b));
    let text = format!(
        "@t{k}.a = global {}, align 16\n@t{k}.b = global {}, align 16\n\
         @t{k}.i = global i32 {i}\n@t{k}.o = global [{r_bytes} x i8] zeroinitializer, align 16\n\
         define void @t{k}() {{\nentry:\n  %ai = load {}, ptr @t{k}.a\n  %a = bitcast {} %ai to {}\n  \
         %bi = load {}, ptr @t{k}.b\n  %b = bitcast {} %bi to {}\n  %i = load i32, ptr @t{k}.i\n  \
         %x = extractelement {} %b, i32 0\n{body}  store {r} %r, ptr @t{k}.o\n  \
         call void @emit(ptr @t{k}.o, i64 {r_bytes})\n  ret void\n}}\n\n",
        a.init(k as u32),
        b.init(k as u32 + 7),
        ia.ir(),
        ia.ir(),
        a.ir(),
        ib.ir(),
        ib.ir(),
        b.ir(),
        b.ir(),
    );
    (k, Some(text))
}

/// `extractelement`, `insertelement` and `shufflevector` on every shape:
/// constant indices in and out of range, variable ones in range, past
/// the end (masked or clamped) and negative; shuffles of one source, of
/// two, with undefined entries, changing the length; checked against the
/// interpreter and `llc` under the emulator.
#[test]
#[allow(clippy::too_many_lines, reason = "one table of cases")]
fn element_access_and_shuffles_agree() {
    let mut functions = Vec::new();
    let mut k = 0;
    let mut add = |f: (u64, Option<String>)| functions.push(f);
    for v in SHAPES {
        let (n, e) = (v.1, v.0);
        let vt = v.ir();
        let eb = v.elem_bytes();
        let constants = [0, 1, n - 1, n, n + 5];
        for c in constants {
            k += 1;
            add(edge(k, v, v, 0, e, eb, &format!("  %r = extractelement {vt} %a, i32 {c}\n")));
            k += 1;
            add(edge(
                k,
                v,
                v,
                0,
                &vt,
                v.bytes(),
                &format!("  %r = insertelement {vt} %a, {e} %x, i32 {c}\n"),
            ));
        }
        for i in [0, 1, i64::from(n) - 1, i64::from(n), i64::from(n) + 3, -1] {
            k += 1;
            add(edge(k, v, v, i, e, eb, &format!("  %r = extractelement {vt} %a, i32 %i\n")));
            k += 1;
            add(edge(
                k,
                v,
                v,
                i,
                &vt,
                v.bytes(),
                &format!("  %r = insertelement {vt} %a, {e} %x, i32 %i\n"),
            ));
            k += 1;
            add(edge(
                k,
                v,
                v,
                i,
                e,
                eb,
                &format!("  %w = zext i32 %i to i64\n  %r = extractelement {vt} %a, i64 %w\n"),
            ));
        }
        // Shuffles: reversed, a broadcast, two sources interleaved, with
        // undefined entries, `%a, %a`, longer and shorter results.
        let entries = |list: &[Option<u32>]| {
            list.iter()
                .map(|e| e.map_or("i32 poison".to_string(), |e| format!("i32 {e}")))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let rev: Vec<Option<u32>> = (0..n).rev().map(Some).collect();
        let bcast: Vec<Option<u32>> = (0..n).map(|_| Some(1)).collect();
        let zip: Vec<Option<u32>> =
            (0..n).map(|j| Some(if j % 2 == 0 { j / 2 } else { n + j / 2 })).collect();
        let holes: Vec<Option<u32>> =
            (0..n).map(|j| (j % 3 != 1).then_some((j * 5) % (2 * n))).collect();
        let long: Vec<Option<u32>> = (0..2 * n).map(|j| Some((j * 3 + 1) % (2 * n))).collect();
        let short: Vec<Option<u32>> = (0..n.div_ceil(2)).map(|j| Some(n + j)).collect();
        for (list, same) in [
            (&rev, false),
            (&bcast, false),
            (&zip, false),
            (&holes, false),
            (&zip, true),
            (&long, false),
            (&short, false),
        ] {
            let len = list.len() as u32;
            let rt = V(v.0, len);
            let second = if same { "%a" } else { "%b" };
            k += 1;
            add(edge(
                k,
                v,
                v,
                0,
                &rt.ir(),
                rt.bytes(),
                &format!(
                    "  %r = shufflevector {vt} %a, {vt} {second}, <{len} x i32> <{}>\n",
                    entries(list)
                ),
            ));
        }
    }
    let mut tally = vector_gen::Tally::default();
    vector_gen::check_functions("edges", functions, &mut tally);
    eprintln!(
        "x64 vector edges: {} generated, {} compared on {:?}, {} undefined {:?}, {} poison bytes, \
         refused {:?}, llc disagreements {:?}",
        tally.generated,
        tally.run,
        tally.legs,
        tally.trapped,
        tally.traps,
        tally.poison_bytes,
        tally.refused,
        tally.llc_wrong
    );
    vector_gen::assert_agrees(&tally);
    assert!(tally.refused.is_empty(), "{:?}", tally.refused);
}

/// What is refused, by name, one construct per function.
#[test]
fn what_is_refused_is_refused_by_name() {
    let extract = |t: &str, e: &str| {
        format!(
            "define void @f(ptr %p) {{\n  %v = load {t}, ptr %p\n  \
             %e = extractelement {t} %v, i32 1\n  store {e} %e, ptr %p\n  ret void\n}}\n"
        )
    };
    let cases = [
        (extract("<65 x i1>", "i1"), hod_x64::vector::MASK_WIDE),
        (extract("<4 x i4>", "i4"), hod_x64::vector::ODD_ELEMENT),
        (extract("<2 x i24>", "i24"), hod_x64::vector::ODD_ELEMENT),
        (extract("<2 x i128>", "i128"), hod_x64::vector::WIDE_ELEMENT),
        (extract("<257 x i8>", "i8"), hod_x64::vector::LENGTH),
        (
            "define void @f(ptr %p, i128 %i) {\n  %v = load <4 x i32>, ptr %p\n  \
             %e = extractelement <4 x i32> %v, i128 %i\n  store i32 %e, ptr %p\n  ret void\n}\n"
                .into(),
            hod_x64::vector::INDEX,
        ),
        (
            "define void @f(<2 x ptr> %p, ptr %o) {\n  \
             %q = getelementptr i8, <2 x ptr> %p, <2 x i64> <i64 1, i64 2>\n  \
             store <2 x ptr> %q, ptr %o\n  ret void\n}\n"
                .into(),
            hod_x64::vector::PTRADD_VECTOR,
        ),
        (
            "define void @f(ptr %p) {\n  %v = load <4 x i64>, ptr %p\n  \
             %r = uitofp <4 x i64> %v to <4 x bfloat>\n  store <4 x bfloat> %r, ptr %p\n  \
             ret void\n}\n"
                .into(),
            hod_x64::vector::BFLOAT,
        ),
    ];
    for (text, name) in &cases {
        assert_eq!(compile(text).err().as_deref(), Some(*name), "{text}");
    }
}

/// The longest vectors compile: 256 bytes, a variable index.
#[test]
fn the_longest_vectors_compile() {
    let reverse: Vec<String> = (0..256).rev().map(|i| format!("i32 {i}")).collect();
    compiles(&format!(
        "define void @f(ptr %p, i64 %i) {{
  %v = load <256 x i8>, ptr %p
  %r = shufflevector <256 x i8> %v, <256 x i8> poison, <256 x i32> <{}>
  %e = extractelement <256 x i8> %r, i64 %i
  %s = insertelement <256 x i8> %r, i8 %e, i64 %i
  store <256 x i8> %s, ptr %p
  ret void
}}\n",
        reverse.join(", ")
    ));
}

/// Three-operand lanes fit the minimal pool (two vector registers).
#[test]
fn three_operand_lanes_fit_the_minimal_pool() {
    for t in ["<4 x float>", "<3 x double>", "<8 x half>", "<3 x half>"] {
        for f in ["fma", "fmuladd"] {
            let s = match t {
                "<4 x float>" => "v4f32",
                "<3 x double>" => "v3f64",
                "<3 x half>" => "v3f16",
                _ => "v8f16",
            };
            compiles(&format!(
                "declare {t} @llvm.{f}.{s}({t}, {t}, {t})\n\
                 define void @f(ptr %p) {{\n  %a = load {t}, ptr %p\n  \
                 %q = getelementptr i8, ptr %p, i64 64\n  %b = load {t}, ptr %q\n  \
                 %c = load volatile {t}, ptr %p\n  \
                 %r = call {t} @llvm.{f}.{s}({t} %a, {t} %b, {t} %c)\n  \
                 store {t} %r, ptr %p\n  ret void\n}}\n"
            ));
        }
    }
}

/// Shifts by a constant splat (`psllw`/`pslld`/`psllq`, `psrlw`/...,
/// `psraw`/`psrad`) and by everything else (the lane loop), on every
/// lane width, checked against the interpreter and `llc`.
#[test]
fn shifts_by_constants_agree() {
    let mut functions = Vec::new();
    let mut k = 0;
    for (bits, n) in [(8u32, 16u32), (16, 8), (32, 4), (64, 2), (16, 4), (32, 2), (32, 3)] {
        let t = format!("<{n} x i{bits}>");
        let lanes: Vec<String> = (0..n)
            .map(|i| {
                let v = 0x8d3c_71a5_e62b_f049u64.rotate_left(i * 13 + bits);
                let shift = 64 - bits;
                format!("i{bits} {}", ((v << shift) as i64) >> shift)
            })
            .collect();
        let init = format!("{t} <{}>", lanes.join(", "));
        let mut amounts: Vec<String> =
            [0, 1, bits / 2 + 1, bits - 1].iter().map(|a| format!("splat (i{bits} {a})")).collect();
        let same: Vec<String> = (0..n).map(|_| format!("i{bits} 3")).collect();
        let mixed: Vec<String> = (0..n).map(|i| format!("i{bits} {}", i % bits)).collect();
        amounts.push(format!("<{}>", same.join(", ")));
        amounts.push(format!("<{}>", mixed.join(", ")));
        amounts.push("zeroinitializer".into());
        for op in ["shl", "lshr", "ashr"] {
            for amount in &amounts {
                k += 1;
                functions.push((
                    k,
                    Some(format!(
                        "@t{k}.a = global {init}, align 16\n\
                         @t{k}.o = global {t} zeroinitializer, align 16\n\
                         define void @t{k}() {{\nentry:\n  %a = load {t}, ptr @t{k}.a\n  \
                         %r = {op} {t} %a, {amount}\n  store {t} %r, ptr @t{k}.o\n  \
                         call void @emit(ptr @t{k}.o, i64 {})\n  ret void\n}}\n\n",
                        n * bits / 8
                    )),
                ));
            }
        }
    }
    let mut tally = vector_gen::Tally::default();
    vector_gen::check_functions("shifts", functions, &mut tally);
    vector_gen::assert_agrees(&tally);
    assert!(tally.refused.is_empty(), "{:?}", tally.refused);
    assert_eq!(tally.run, tally.generated, "{tally:?}");
}
