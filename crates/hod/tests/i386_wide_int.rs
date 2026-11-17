//! WP-346 (`spec/codegen-x86.md` §22): integers of 65 to 128 bits on
//! i386, run against `llc` (`-O0 -global-isel=0 -fast-isel=0`, LLVM 19 and
//! 22) under `qemu-i386`, plain and under `Stress::ALL`, static and PIE,
//! at CPU `generic` (no `cmov`), `i686` and `pentium4`:
//! - hand-written `.ll` at 65, 96 and 128 bits over every pair of boundary
//!   values (carries across each limb, the sign bit of the odd width, the
//!   largest and smallest values): `add`, `sub`, `mul`, the bitwise
//!   operations, every `icmp`, `select`, `freeze`, `zext` / `sext` /
//!   `trunc` to and from 32, 64 and the other wide widths, the six
//!   `with.overflow` intrinsics, a `phi` in a loop, `load` and `store`;
//! - clang's `__builtin_{add,sub,mul}_overflow` over every triple of
//!   `int`, `unsigned`, `long long`, `unsigned long long` (operands and
//!   result), which writes `i33` and `i65`;
//! - the two modules reduced from the full libc++ run compile;
//! - what stays refused, by name.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, lane, refusal};

/// QEMU's features of an SSE2 build (`pentium4`).
const P4: &[&str] = &["mmx", "sse", "sse2"];

/// The CPUs of the runs: the function attribute and the QEMU features.
const CPUS: [(Option<&str>, &[&str]); 3] =
    [(None, &[]), (Some("i686"), &[]), (Some("pentium4"), P4)];

/// The widths of the hand-written programs.
const WIDTHS: [u32; 3] = [65, 96, 128];

/// The mask of `w` bits.
fn mask(w: u32) -> u128 {
    if w == 128 { u128::MAX } else { (1 << w) - 1 }
}

/// The boundary values of `iW`, deduplicated.
fn values(w: u32) -> Vec<u128> {
    let m = mask(w);
    let min = 1u128 << (w - 1);
    let mut v = vec![
        0,
        1,
        2,
        0xffff_ffff,
        1 << 32,
        u128::from(u64::MAX),
        1 << 64,
        (1 << 64) | 0xffff_ffff,
        min - 1,
        min,
        min + 1,
        m,
        m - 1,
        0x89ab_cdef_0123_4567_fedc_ba98_7654_3210,
    ];
    if w > 96 {
        v.extend([(1 << 96) - 1, 1 << 96]);
    }
    let mut out = Vec::new();
    for x in v {
        let x = x & m;
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// `x` as a signed decimal literal of `iW`.
fn lit(w: u32, x: u128) -> String {
    if x >> (w - 1) & 1 == 1 {
        format!("-{}", x.wrapping_neg() & mask(w))
    } else {
        x.to_string()
    }
}

/// The shared part of every program: `@show` prints the four words of a
/// 16-byte buffer, `@flag` an `i1`.
const SHOW: &str = r#"
declare i32 @printf(ptr, ...)
@fmt = private constant [21 x i8] c"%08x %08x %08x %08x\0A\00"
@fmt1 = private constant [4 x i8] c"%d\0A\00"
define void @show(ptr %p) #0 {
  %q1 = getelementptr i8, ptr %p, i32 4
  %q2 = getelementptr i8, ptr %p, i32 8
  %q3 = getelementptr i8, ptr %p, i32 12
  %w0 = load i32, ptr %p
  %w1 = load i32, ptr %q1
  %w2 = load i32, ptr %q2
  %w3 = load i32, ptr %q3
  %u = call i32 (ptr, ...) @printf(ptr @fmt, i32 %w3, i32 %w2, i32 %w1, i32 %w0)
  store i32 0, ptr %p
  store i32 0, ptr %q1
  store i32 0, ptr %q2
  store i32 0, ptr %q3
  ret void
}
define void @flag(i1 %c) #0 {
  %z = zext i1 %c to i32
  %u = call i32 (ptr, ...) @printf(ptr @fmt1, i32 %z)
  ret void
}
"#;

/// A function body under construction: `%vN` names.
struct Body {
    text: String,
    n: u32,
}

impl Body {
    /// Emits `rhs` as a fresh value; its name.
    fn val(&mut self, rhs: &str) -> String {
        self.n += 1;
        let name = format!("%v{}", self.n);
        writeln!(self.text, "  {name} = {rhs}").unwrap();
        name
    }
    /// Stores the `iT` value `v` into `%buf` and prints it.
    fn show(&mut self, t: u32, v: &str) {
        writeln!(self.text, "  store i{t} {v}, ptr %buf\n  call void @show(ptr %buf)").unwrap();
    }
    /// Prints the `i1` value `c`.
    fn flag(&mut self, c: &str) {
        writeln!(self.text, "  call void @flag(i1 {c})").unwrap();
    }
}

/// `@tW(ptr %pa, ptr %pb)`: every operation on the pair at the pointers.
fn pair_function(w: u32) -> String {
    let mut b = Body { text: String::new(), n: 0 };
    let t = format!("i{w}");
    let x = b.val(&format!("load {t}, ptr %pa"));
    let y = b.val(&format!("load {t}, ptr %pb"));
    for op in ["add", "sub", "mul", "and", "or", "xor"] {
        let r = b.val(&format!("{op} {t} {x}, {y}"));
        b.show(w, &r);
    }
    for pred in ["eq", "ne", "ult", "ule", "ugt", "uge", "slt", "sle", "sgt", "sge"] {
        let c = b.val(&format!("icmp {pred} {t} {x}, {y}"));
        b.flag(&c);
    }
    let c = b.val(&format!("icmp slt {t} {x}, {y}"));
    let s = b.val(&format!("select i1 {c}, {t} {x}, {t} {y}"));
    b.show(w, &s);
    let f = b.val(&format!("freeze {t} {x}"));
    let g = b.val(&format!("add {t} {f}, 1"));
    b.show(w, &g);
    for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
        let r = b.val(&format!(
            "call {{ {t}, i1 }} @llvm.{op}.with.overflow.{t}({t} {x}, {t} {y})"
        ));
        let v = b.val(&format!("extractvalue {{ {t}, i1 }} {r}, 0"));
        let o = b.val(&format!("extractvalue {{ {t}, i1 }} {r}, 1"));
        b.show(w, &v);
        b.flag(&o);
    }
    // Truncations and extensions through 32, 64 and the other widths.
    for to in [32, 64] {
        let tr = b.val(&format!("trunc {t} {y} to i{to}"));
        for ext in ["zext", "sext"] {
            let back = b.val(&format!("{ext} i{to} {tr} to {t}"));
            b.show(w, &back);
        }
        if to == 32 {
            b.show(32, &tr);
        } else {
            b.show(64, &tr);
        }
    }
    for other in WIDTHS.into_iter().filter(|&o| o != w) {
        if other > w {
            for ext in ["zext", "sext"] {
                let e = b.val(&format!("{ext} {t} {x} to i{other}"));
                b.show(other, &e);
            }
        } else {
            let tr = b.val(&format!("trunc {t} {x} to i{other}"));
            let back = b.val(&format!("sext i{other} {tr} to {t}"));
            b.show(other, &tr);
            b.show(w, &back);
        }
    }
    // The sign bit of the width read through a compare with zero.
    let neg = b.val(&format!("icmp slt {t} {y}, 0"));
    b.flag(&neg);
    format!(
        "define void @t{w}(ptr %pa, ptr %pb) #0 {{\n  %buf = alloca [16 x i8], align 16\n  \
         store i128 0, ptr %buf\n{}  ret void\n}}\n",
        b.text
    )
}

/// `@sW()`: a running sum, product and minimum of the values in a loop
/// (`phi` of `iW`), printed at the end.
fn loop_function(w: u32, count: usize) -> String {
    let t = format!("i{w}");
    format!(
        "define void @s{w}() #0 {{
entry:
  %buf = alloca [16 x i8], align 16
  store i128 0, ptr %buf
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %sum = phi {t} [ 0, %entry ], [ %sum1, %loop ]
  %prod = phi {t} [ 1, %entry ], [ %prod1, %loop ]
  %min = phi {t} [ {max}, %entry ], [ %min1, %loop ]
  %p = getelementptr {t}, ptr @v{w}, i32 %i
  %x = load {t}, ptr %p
  %sum1 = add {t} %sum, %x
  %x1 = or {t} %x, 1
  %prod1 = mul {t} %prod, %x1
  %lt = icmp slt {t} %x, %min
  %min1 = select i1 %lt, {t} %x, {t} %min
  %i1 = add i32 %i, 1
  %done = icmp eq i32 %i1, {count}
  br i1 %done, label %out, label %loop
out:
  store {t} %sum1, ptr %buf
  call void @show(ptr %buf)
  store {t} %prod1, ptr %buf
  call void @show(ptr %buf)
  store {t} %min1, ptr %buf
  call void @show(ptr %buf)
  ret void
}}
",
        max = lit(w, mask(w) >> 1),
    )
}

/// The program for `cpu`: each width's pairs, then its loop.
fn program(cpu: Option<&str>) -> String {
    let mut s = format!("target triple = \"{}\"\n{SHOW}", i386_support::TRIPLE);
    let mut main = String::new();
    for w in WIDTHS {
        let vals = values(w);
        let n = vals.len();
        let items: Vec<String> = vals.iter().map(|&v| format!("i{w} {}", lit(w, v))).collect();
        writeln!(s, "@v{w} = private constant [{n} x i{w}] [{}]", items.join(", ")).unwrap();
        for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
            writeln!(s, "declare {{ i{w}, i1 }} @llvm.{op}.with.overflow.i{w}(i{w}, i{w})").unwrap();
        }
        s.push_str(&pair_function(w));
        s.push_str(&loop_function(w, n));
        write!(
            main,
            "  br label %o{w}
o{w}:
  %i{w} = phi i32 [ 0, %{prev} ], [ %i{w}n, %o{w}e ]
  br label %in{w}
in{w}:
  %j{w} = phi i32 [ 0, %o{w} ], [ %j{w}n, %in{w} ]
  %a{w} = getelementptr i{w}, ptr @v{w}, i32 %i{w}
  %b{w} = getelementptr i{w}, ptr @v{w}, i32 %j{w}
  call void @t{w}(ptr %a{w}, ptr %b{w})
  %j{w}n = add i32 %j{w}, 1
  %jd{w} = icmp eq i32 %j{w}n, {n}
  br i1 %jd{w}, label %o{w}e, label %in{w}
o{w}e:
  %i{w}n = add i32 %i{w}, 1
  %id{w} = icmp eq i32 %i{w}n, {n}
  br i1 %id{w}, label %d{w}, label %o{w}
d{w}:
  call void @s{w}()
",
            prev = main_prev(w),
        )
        .unwrap();
    }
    write!(s, "define i32 @main() #0 {{\nentry:\n{main}  ret i32 0\n}}\n").unwrap();
    match cpu {
        Some(c) => writeln!(s, "attributes #0 = {{ \"target-cpu\"=\"{c}\" }}").unwrap(),
        None => s.push_str("attributes #0 = { nounwind }\n"),
    }
    s
}

/// The block of `@main` that enters width `w`'s loops.
fn main_prev(w: u32) -> String {
    match WIDTHS.iter().position(|&x| x == w) {
        Some(0) | None => "entry".into(),
        Some(k) => format!("d{}", WIDTHS[k - 1]),
    }
}

#[test]
fn wide_operations_as_llc() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-wide-int-ll");
    let mut runs = 0;
    for (cpu, features) in CPUS {
        let text = program(cpu);
        for pic in [false, true] {
            let build = Build { pic, features, ..STATIC };
            let label = format!("wide-{}", cpu.unwrap_or("generic"));
            runs += lane.check(&scratch, &label, &Source::Ll(&text), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 wide integers: {runs} runs equal llc's");
}

/// The C types of the overflow builtins and their printf formats.
const C_TYPES: [(&str, &str); 4] = [
    ("int", "%d"),
    ("unsigned", "%u"),
    ("long long", "%lld"),
    ("unsigned long long", "%llu"),
];

/// clang's overflow builtins over every triple of the four types, each
/// over every pair of boundary values (converted to the operand types).
fn overflow_c() -> String {
    let mut s = String::from(
        "#include <stdio.h>\n\
         static const long long V[] = {0, 1, -1, 2, -2, 0x7fffffff, -0x7fffffff - 1,\n  \
         0xffffffffLL, 0x100000000LL, -0x100000000LL, 0x7fffffffffffffffLL,\n  \
         -0x7fffffffffffffffLL - 1, 0x3037abcd12345678LL, -0x3037abcd12345678LL};\n\
         #define N (sizeof V / sizeof V[0])\n",
    );
    let mut calls = String::new();
    let mut n = 0;
    for op in ["add", "sub", "mul"] {
        for (a, _) in C_TYPES {
            for (bt, _) in C_TYPES {
                for (r, fmt) in C_TYPES {
                    n += 1;
                    writeln!(
                        s,
                        "static void f{n}(void) {{\n  for (unsigned i = 0; i < N; i++)\n    \
                         for (unsigned j = 0; j < N; j++) {{\n      {r} res = 0;\n      \
                         int o = __builtin_{op}_overflow(({a})V[i], ({bt})V[j], &res);\n      \
                         printf(\"{n} %u %u {fmt} %d\\n\", i, j, res, o);\n    }}\n}}"
                    )
                    .unwrap();
                    writeln!(calls, "  f{n}();").unwrap();
                }
            }
        }
    }
    write!(s, "int main(void) {{\n{calls}  return 0;\n}}\n").unwrap();
    s
}

#[test]
fn overflow_builtins_from_c() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-wide-int-c");
    let source = scratch.write("overflow.c", &overflow_c());
    let mut runs = 0;
    for (flags, features) in
        [(&[][..], &[][..]), (&["-march=i586"][..], &[][..]), (&["-march=pentium4"][..], P4)]
    {
        for pic in [false, true] {
            let build = Build { pic, features, ..STATIC };
            let name = format!("overflow-{}", flags.first().unwrap_or(&"i686"));
            runs += lane.check(&scratch, &name, &Source::C(&source, flags), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 overflow builtins from C: {runs} runs equal llc's");
}

/// The two modules reduced from the full libc++ run for i686
/// (`num_get::__do_get_integral<long long>`) compile.
#[test]
fn reduced_libcxx_modules_compile() {
    let zext = "define i1 @f(i64 %a) {
  %x = zext i64 %a to i65
  %t = trunc i65 %x to i64
  %z = zext i64 %t to i65
  %c = icmp ne i65 %x, %z
  ret i1 %c
}";
    let sadd = "declare { i65, i1 } @llvm.sadd.with.overflow.i65(i65, i65)
define i1 @f(i64 %a, i32 %b) {
  %x = zext i64 %a to i65
  %y = sext i32 %b to i65
  %r = call { i65, i1 } @llvm.sadd.with.overflow.i65(i65 %x, i65 %y)
  %o = extractvalue { i65, i1 } %r, 1
  ret i1 %o
}";
    for text in [zext, sadd] {
        assert_eq!(refusal(text, None), None, "{text}");
    }
}

/// Each operation left out stays refused under its own name.
#[test]
fn what_stays_refused() {
    let cases = [
        ("%r = shl i96 %a, %a", "x86:i64:wide:shift"),
        ("%r = lshr i128 %a, %a", "x86:i64:wide:shift"),
        ("%r = ashr i65 %a, %a", "x86:i64:wide:shift"),
        ("%r = udiv i128 %a, %a", "x86:i64:wide:div"),
        // An odd width is the shared classifier's `inst:srem/int-wide`.
        ("%r = srem i128 %a, %a", "x86:i64:wide:div"),
        ("%r = call i128 @llvm.ctpop.i128(i128 %a)", "x86:i64:wide:intrinsic"),
        ("%r = call i128 @llvm.umax.i128(i128 %a, i128 %a)", "x86:i64:wide:intrinsic"),
    ];
    for (op, name) in cases {
        let ty = op.split_whitespace().find(|w| w.starts_with('i')).unwrap_or("i128");
        let text = format!(
            "define void @f(ptr %p) {{\n  %a = load {ty}, ptr %p\n  {op}\n  \
             store {ty} %r, ptr %p\n  ret void\n}}"
        );
        let error = refusal(&text, None).unwrap_or_else(|| panic!("compiled:\n{text}"));
        assert!(error.contains(name), "{text}\n{error}");
    }
    let switch = "define i32 @f(ptr %p) {\nentry:\n  %a = load i128, ptr %p\n  \
                  switch i128 %a, label %d [ i128 18446744073709551616, label %c ]\n\
                  c:\n  ret i32 1\nd:\n  ret i32 0\n}";
    let error = refusal(switch, None).expect("a wide switch compiles");
    assert!(error.contains("x86:i64:wide:switch"), "{error}");
    let abi = "define i128 @f(ptr %p) {\n  %a = load i128, ptr %p\n  ret i128 %a\n}";
    let error = refusal(abi, None).expect("a wide return compiles");
    assert!(error.contains("x86:i64:wide:abi"), "{error}");
    let width = "define void @f(ptr %p) {\n  %a = load i200, ptr %p\n  %r = add i200 %a, %a\n  \
                 store i200 %r, ptr %p\n  ret void\n}";
    let error = refusal(width, None).expect("an i200 add compiles");
    assert!(error.contains("x86:i64:wide:width"), "{error}");
}
