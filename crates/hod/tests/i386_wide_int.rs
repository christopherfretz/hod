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

#![allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "IR text built from short value names"
)]

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
    if x >> (w - 1) & 1 == 1 { format!("-{}", x.wrapping_neg() & mask(w)) } else { x.to_string() }
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
        let r =
            b.val(&format!("call {{ {t}, i1 }} @llvm.{op}.with.overflow.{t}({t} {x}, {t} {y})"));
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
            writeln!(s, "declare {{ i{w}, i1 }} @llvm.{op}.with.overflow.i{w}(i{w}, i{w})")
                .unwrap();
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
const C_TYPES: [(&str, &str); 4] =
    [("int", "%d"), ("unsigned", "%u"), ("long long", "%lld"), ("unsigned long long", "%llu")];

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
    let source = scratch.write("overflow.c", overflow_c());
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
        // The intrinsics' declarations (the parser wants them).
        let declare = match op.split_once('@') {
            Some((_, rest)) => {
                let name = rest.split('(').next().unwrap_or_default();
                let arity = rest.split('(').nth(1).unwrap_or_default().split(',').count();
                let params = vec![ty; arity].join(", ");
                format!("declare {ty} @{name}({params})\n")
            }
            None => String::new(),
        };
        let text = format!(
            "{declare}define void @f(ptr %p) {{\n  %a = load {ty}, ptr %p\n  {op}\n  \
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
    // An `i128` result is returned in memory as `llc` does (`abi.rs`);
    // another wide result is refused, as is a wide argument.
    let abi = "define i96 @f(ptr %p) {\n  %a = load i96, ptr %p\n  ret i96 %a\n}";
    let error = refusal(abi, None).expect("a wide return compiles");
    assert!(error.contains("x86:i64:wide:abi"), "{error}");
    let arg = "define void @f(i128 %a, ptr %p) {\n  store i128 %a, ptr %p\n  ret void\n}";
    // The convention refuses an `i128` argument first (`x86:abi:wide-arg`).
    let error = refusal(arg, None).expect("a wide argument compiles");
    assert!(error.contains("x86:abi:wide-arg"), "{error}");
    let width = "define void @f(ptr %p) {\n  %a = load i200, ptr %p\n  %r = add i200 %a, %a\n  \
                 store i200 %r, ptr %p\n  ret void\n}";
    let error = refusal(width, None).expect("an i200 add compiles");
    assert!(error.contains("x86:i64:wide:width"), "{error}");
}

// ---- the edges of a limb-wise lowering (WP-346 review) ----

/// The odd widths of the high-bits programs: a top limb of 1, 8, 4 and 24
/// bits.
const ODD: [u32; 4] = [65, 72, 100, 120];

/// The two builds of the edge programs: generic static, `pentium4` PIE.
const EDGE_BUILDS: [(Option<&str>, bool); 2] = [(None, false), (Some("pentium4"), true)];

/// The attribute group `#0` for `cpu`.
fn attributes(cpu: Option<&str>) -> String {
    match cpu {
        Some(c) => format!("attributes #0 = {{ \"target-cpu\"=\"{c}\" }}\n"),
        None => "attributes #0 = { nounwind }\n".into(),
    }
}

/// Runs `text(cpu)` in both edge builds; the number of runs.
fn check_edges(name: &str, text: impl Fn(Option<&str>) -> String) -> usize {
    let Some(lane) = lane() else { return 1 };
    let scratch = Scratch::new(&format!("i386-wide-int-{name}"));
    let mut runs = 0;
    for (cpu, pic) in EDGE_BUILDS {
        let features = if cpu.is_some() { P4 } else { &[] };
        let build = Build { pic, features, ..STATIC };
        let label = format!("{name}-{}", cpu.unwrap_or("generic"));
        runs += lane.check(&scratch, &label, &Source::Ll(&text(cpu)), build);
    }
    runs
}

/// The right-hand side that narrows the `i128` `v` to `iW` (`freeze` at
/// 128 bits, where a `trunc` is malformed).
fn narrow(v: &str, w: u32) -> String {
    if w == 128 { format!("freeze i128 {v}") } else { format!("trunc i128 {v} to i{w}") }
}

/// `x` (an `iW` value) with ones and a pattern above bit `w`: the `i128`
/// whose `trunc` to `iW` has garbage in the bits above the width.
fn garbage(w: u32, x: u128, k: usize) -> u128 {
    let above =
        if k.is_multiple_of(2) { u128::MAX } else { 0x5a5a_c3c3_f0f0_9696_a5a5_3c3c_0f0f_6969 };
    if w == 128 { x } else { (x & mask(w)) | (above & !mask(w)) | (1 << w) }
}

/// The `iW` value `x` sign-extended to `i128`.
fn sx(w: u32, x: u128) -> i128 {
    ((x << (128 - w)) as i128) >> (128 - w)
}

/// A loop in `@main` calling `@func(ptr, ptr)` on every pair of the `n`
/// elements of type `ty` of `@table`; `tag` keeps its labels apart.
fn pair_loop(tag: &str, func: &str, table: &str, ty: &str, n: usize) -> String {
    format!(
        "  br label %{tag}e
{tag}e:
  br label %{tag}o
{tag}o:
  %{tag}i = phi i32 [ 0, %{tag}e ], [ %{tag}i1, %{tag}x ]
  br label %{tag}l
{tag}l:
  %{tag}j = phi i32 [ 0, %{tag}o ], [ %{tag}j1, %{tag}l ]
  %{tag}a = getelementptr {ty}, ptr @{table}, i32 %{tag}i
  %{tag}b = getelementptr {ty}, ptr @{table}, i32 %{tag}j
  call void @{func}(ptr %{tag}a, ptr %{tag}b)
  %{tag}j1 = add i32 %{tag}j, 1
  %{tag}jd = icmp eq i32 %{tag}j1, {n}
  br i1 %{tag}jd, label %{tag}x, label %{tag}l
{tag}x:
  %{tag}i1 = add i32 %{tag}i, 1
  %{tag}id = icmp eq i32 %{tag}i1, {n}
  br i1 %{tag}id, label %{tag}d, label %{tag}o
{tag}d:
"
    )
}

/// `@gW(ptr %pa, ptr %pb)`: two `iW` values truncated from `i128`s with
/// garbage above the width, through every consumer that reads the bits
/// above it.
fn garbage_function(w: u32) -> String {
    let mut b = Body { text: String::new(), n: 0 };
    let t = format!("i{w}");
    let xx = b.val("load i128, ptr %pa");
    let yy = b.val("load i128, ptr %pb");
    let x = b.val(&format!("trunc i128 {xx} to {t}"));
    let y = b.val(&format!("trunc i128 {yy} to {t}"));
    for pred in ["eq", "ne", "ult", "ule", "ugt", "uge", "slt", "sle", "sgt", "sge"] {
        let c = b.val(&format!("icmp {pred} {t} {x}, {y}"));
        b.flag(&c);
    }
    for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
        let r =
            b.val(&format!("call {{ {t}, i1 }} @llvm.{op}.with.overflow.{t}({t} {x}, {t} {y})"));
        let v = b.val(&format!("extractvalue {{ {t}, i1 }} {r}, 0"));
        let o = b.val(&format!("extractvalue {{ {t}, i1 }} {r}, 1"));
        b.show(w, &v);
        b.flag(&o);
    }
    for ext in ["zext", "sext"] {
        let e = b.val(&format!("{ext} {t} {x} to i128"));
        b.show(128, &e);
    }
    if w < 120 {
        let e = b.val(&format!("sext {t} {y} to i120"));
        b.show(120, &e);
    }
    let m = b.val(&format!("mul {t} {x}, {y}"));
    b.show(w, &m);
    let mz = b.val(&format!("icmp eq {t} {m}, 0"));
    b.flag(&mz);
    let s = b.val(&format!("sub {t} {x}, {y}"));
    let sn = b.val(&format!("icmp slt {t} {s}, 0"));
    b.flag(&sn);
    let a = b.val(&format!("add {t} {x}, {y}"));
    b.show(w, &a);
    b.show(w, &x);
    if w > 65 {
        let tr = b.val(&format!("trunc {t} {x} to i65"));
        let tn = b.val(&format!("icmp slt i65 {tr}, 0"));
        b.flag(&tn);
        let te = b.val(&format!("icmp eq i65 {tr}, 0"));
        b.flag(&te);
    }
    let c = b.val(&format!("icmp ult {t} {x}, {y}"));
    let sel = b.val(&format!("select i1 {c}, {t} {x}, {t} {y}"));
    let sp = b.val(&format!("icmp sgt {t} {sel}, 0"));
    b.flag(&sp);
    b.show(w, &sel);
    format!(
        "define void @g{w}(ptr %pa, ptr %pb) #0 {{\nentry:\n  %buf = alloca [16 x i8], align 16\n  \
         store i128 0, ptr %buf\n{}  br i1 {c}, label %l, label %r\nl:\n  br label %j\nr:\n  \
         br label %j\nj:\n  %ph = phi {t} [ {x}, %l ], [ {y}, %r ]\n  \
         %pc = icmp sle {t} %ph, {x}\n  call void @flag(i1 %pc)\n  \
         %pz = icmp eq {t} %ph, 0\n  call void @flag(i1 %pz)\n  ret void\n}}\n",
        b.text
    )
}

/// `@ix()`: `getelementptr` with wide indices whose high limbs are not
/// zero, or negative as wide values: the index is truncated to 32 bits.
fn index_function() -> (String, String) {
    let cases: [(u32, &str, u128); 7] = [
        (65, "i8", (1 << 64) | 5),
        (96, "i8", 3u128.wrapping_neg()),
        (128, "i8", (0xdead_beef_u128 << 64) | 0xffff_fff0),
        (128, "i16", (1u128 << 100).wrapping_add(7).wrapping_neg()),
        (100, "i32", (1 << 99) | 6),
        (72, "i8", (0xab << 64) | (7 << 32) | 1),
        (120, "i16", (1 << 119) | (0x8000_0000 << 32) | 0xffff_fffe),
    ];
    let table: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(k, &(w, _, v))| format!("i128 {}", lit(128, garbage(w, v, k))))
        .collect();
    let mut globals = format!(
        "@ixv = private constant [{} x i128] [{}]\n@bytes = private constant [64 x i8] [{}]\n",
        cases.len(),
        table.join(", "),
        (0..64).map(|i| format!("i8 {i}")).collect::<Vec<_>>().join(", ")
    );
    globals.push_str(
        "define void @byte(i8 %b) #0 {\n  %z = zext i8 %b to i32\n  \
         %u = call i32 (ptr, ...) @printf(ptr @fmt1, i32 %z)\n  ret void\n}\n",
    );
    let mut f =
        String::from("define void @ix() #0 {\n  %base = getelementptr i8, ptr @bytes, i32 32\n");
    for (k, &(w, scale, _)) in cases.iter().enumerate() {
        writeln!(
            f,
            "  %K{k}p = getelementptr i128, ptr @ixv, i32 {k}\n  %K{k} = load i128, ptr %K{k}p\n  \
             %k{k} = {}\n  %p{k} = getelementptr {scale}, ptr %base, i{w} %k{k}\n  \
             %b{k} = load i8, ptr %p{k}\n  call void @byte(i8 %b{k})",
            narrow(&format!("%K{k}"), w)
        )
        .unwrap();
    }
    // Constant wide indices.
    f.push_str(
        "  %c0 = getelementptr i8, ptr %base, i128 -5\n  %d0 = load i8, ptr %c0\n  \
         call void @byte(i8 %d0)\n  %c1 = getelementptr i8, ptr %base, i96 18446744073709551619\n  \
         %d1 = load i8, ptr %c1\n  call void @byte(i8 %d1)\n  ret void\n}\n",
    );
    (globals, f)
}

/// The widths of the memory programs.
const MEM_WIDTHS: [u32; 6] = [65, 72, 96, 100, 120, 128];

/// `@mem()`: for each width, a value with garbage above it stored (at an
/// odd offset and at a 16-byte one) into a 48-byte buffer of `0xaa` guard
/// bytes, reloaded, stored again, the buffer printed; then a store, load
/// and store again at the end of a mapping whose next 64 KiB are
/// `PROT_NONE`. Returns 2 when the protection fails.
fn mem_function() -> (String, String) {
    let values: Vec<String> = MEM_WIDTHS
        .iter()
        .enumerate()
        .map(|(k, &w)| {
            let x = 0xfedc_ba98_7654_3210_0f1e_2d3c_4b5a_6978_u128 ^ ((k as u128) << (w - 1));
            format!("i128 {}", lit(128, garbage(w, x, k)))
        })
        .collect();
    let globals = format!(
        "@memv = private constant [{} x i128] [{}]\ndeclare ptr @mmap(ptr, i32, i32, i32, i32, i32)\n\
         declare i32 @mprotect(ptr, i32, i32)\n",
        MEM_WIDTHS.len(),
        values.join(", ")
    );
    let mut f = String::from(
        "define i32 @mem() #0 {\nentry:\n  %gb = alloca [48 x i8], align 16\n  \
         %m = call ptr @mmap(ptr null, i32 196608, i32 3, i32 34, i32 -1, i32 0)\n  \
         %mi = ptrtoint ptr %m to i32\n  %q0 = add i32 %mi, 65536\n  \
         %q1 = and i32 %q0, -65536\n  %q = inttoptr i32 %q1 to ptr\n  \
         %pr = call i32 @mprotect(ptr %q, i32 65536, i32 0)\n  %bad = icmp ne i32 %pr, 0\n  \
         br i1 %bad, label %fail, label %go\nfail:\n  ret i32 2\ngo:\n  \
         %g16 = getelementptr i8, ptr %gb, i32 16\n  %g32 = getelementptr i8, ptr %gb, i32 32\n  \
         %g3 = getelementptr i8, ptr %gb, i32 3\n  %g24 = getelementptr i8, ptr %gb, i32 24\n",
    );
    for (k, &w) in MEM_WIDTHS.iter().enumerate() {
        let t = format!("i{w}");
        let size = w.div_ceil(8);
        for word in 0..12 {
            writeln!(
                f,
                "  %f{k}_{word} = getelementptr i32, ptr %gb, i32 {word}\n  \
                 store i32 -1431655766, ptr %f{k}_{word}"
            )
            .unwrap();
        }
        writeln!(
            f,
            "  %X{k}p = getelementptr i128, ptr @memv, i32 {k}\n  %X{k} = load i128, ptr %X{k}p\n  \
             %x{k} = {}\n  store {t} %x{k}, ptr %g3, align 1\n  \
             store {t} %x{k}, ptr %g24, align 8\n  %l{k} = load {t}, ptr %g3, align 1\n  \
             %l{k}a = add {t} %l{k}, 1\n  store {t} %l{k}a, ptr %g24, align 8\n  \
             call void @show(ptr %gb)\n  call void @show(ptr %g16)\n  call void @show(ptr %g32)\n  \
             %e{k} = getelementptr i8, ptr %q, i32 -{size}\n  store {t} %x{k}, ptr %e{k}, align 1\n  \
             %r{k} = load {t}, ptr %e{k}, align 1\n  %r{k}a = sub {t} %r{k}, 3\n  \
             store {t} %r{k}a, ptr %e{k}, align 1\n  %s{k} = load {t}, ptr %e{k}, align 1\n  \
             store i128 0, ptr %gb\n  store {t} %s{k}, ptr %gb\n  call void @show(ptr %gb)",
            narrow(&format!("%X{k}"), w)
        )
        .unwrap();
    }
    f.push_str("  ret i32 0\n}\n");
    (globals, f)
}

/// `@cW()`: an `add` of a value to itself, a `sub` of a `phi` from that,
/// and two `phi`s that swap each iteration, printed every iteration.
fn carry_function(w: u32) -> String {
    let t = format!("i{w}");
    let m = mask(w);
    let c1 = lit(w, 0x8000_0001_ffff_ffff_8000_0001_ffff_ffff & m);
    let c2 = lit(w, 0xffff_ffff_0000_0000_ffff_ffff_0000_0001 & m);
    let c3 = lit(w, 0x0000_0001_ffff_ffff_ffff_ffff_ffff_ffff & m);
    format!(
        "define void @c{w}() #0 {{
entry:
  %buf = alloca [16 x i8], align 16
  store i128 0, ptr %buf
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %a = phi {t} [ {c1}, %entry ], [ %a2, %loop ]
  %x = phi {t} [ {c2}, %entry ], [ %y, %loop ]
  %y = phi {t} [ {c3}, %entry ], [ %x, %loop ]
  %a1 = add {t} %a, %a
  %a2 = sub {t} %a1, %x
  %s = add {t} %x, %y
  %d = sub {t} %y, %y
  store {t} %a2, ptr %buf
  call void @show(ptr %buf)
  store {t} %s, ptr %buf
  call void @show(ptr %buf)
  store {t} %d, ptr %buf
  call void @show(ptr %buf)
  %i1 = add i32 %i, 1
  %done = icmp eq i32 %i1, 6
  br i1 %done, label %out, label %loop
out:
  ret void
}}
"
    )
}

/// The program of the edge checks a, b, c, e and the wide index.
fn edges_program(cpu: Option<&str>) -> String {
    let mut s = format!("target triple = \"{}\"\n{SHOW}", i386_support::TRIPLE);
    let mut main = String::new();
    for w in ODD {
        let vals = values(w);
        let n = vals.len();
        let items: Vec<String> = vals
            .iter()
            .enumerate()
            .map(|(k, &v)| format!("i128 {}", lit(128, garbage(w, v, k))))
            .collect();
        writeln!(s, "@gv{w} = private constant [{n} x i128] [{}]", items.join(", ")).unwrap();
        for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
            writeln!(s, "declare {{ i{w}, i1 }} @llvm.{op}.with.overflow.i{w}(i{w}, i{w})")
                .unwrap();
        }
        s.push_str(&garbage_function(w));
        main.push_str(&pair_loop(&format!("q{w}"), &format!("g{w}"), &format!("gv{w}"), "i128", n));
    }
    let (ig, ifn) = index_function();
    let (mg, mfn) = mem_function();
    s.push_str(&ig);
    s.push_str(&ifn);
    s.push_str(&mg);
    s.push_str(&mfn);
    for w in [65, 96, 100, 128] {
        s.push_str(&carry_function(w));
        writeln!(main, "  call void @c{w}()").unwrap();
    }
    write!(
        s,
        "define i32 @main() #0 {{\nentry:\n{main}  call void @ix()\n  %r = call i32 @mem()\n  \
         ret i32 %r\n}}\n{}",
        attributes(cpu)
    )
    .unwrap();
    s
}

/// a. memory extent and guard bytes, b. garbage above the width through
/// every consumer, c. carries with aliased operands and swapping `phi`s,
/// e. compares at the limb edges at odd widths, and a wide `getelementptr`
/// index: equal to llc's.
#[test]
fn limb_edges_as_llc() {
    let runs = check_edges("edges", edges_program);
    assert!(runs > 0, "no run was made");
    eprintln!("i386 wide integer edges: {runs} runs equal llc's");
}

/// The `iW` product of `a` and `b` and the `smul` / `umul` overflow flags.
fn mul_truth(w: u32, a: u128, b: u128) -> (u128, bool, bool) {
    let m = mask(w);
    let r = a.wrapping_mul(b) & m;
    let s = match sx(w, a).checked_mul(sx(w, b)) {
        None => true,
        Some(p) => sx(w, p as u128 & m) != p,
    };
    let u = a.checked_mul(b).is_none_or(|p| p > m);
    (r, s, u)
}

/// The operand pairs of width `w`: every pair of the boundary values, then
/// 2,048 pairs of a fixed-seed generator over every magnitude and sign.
fn mul_pairs(w: u32) -> Vec<(u128, u128)> {
    let vals = values(w);
    let mut pairs: Vec<(u128, u128)> =
        vals.iter().flat_map(|&a| vals.iter().map(move |&b| (a, b))).collect();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::from(w);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let one = |next: &mut dyn FnMut() -> u64| {
        let r = (u128::from(next()) << 64) | u128::from(next());
        let v = r >> (next() % 128);
        let v = if next().is_multiple_of(4) { v.wrapping_neg() } else { v };
        v & mask(w)
    };
    for _ in 0..2048 {
        let a = one(&mut next);
        let b = one(&mut next);
        pairs.push((a, b));
    }
    pairs
}

/// d. `mul`, `smul.with.overflow`, `umul.with.overflow` over the pairs of
/// [`mul_pairs`] at 65, 96 and 128 bits, each checked against the exact
/// result in a table: the first mismatching index (exit 1), else a checksum
/// of the products and flags.
fn mul_program(cpu: Option<&str>) -> String {
    let mut s = format!(
        "target triple = \"{}\"\ndeclare i32 @printf(ptr, ...)\n\
         @fmtm = private constant [9 x i8] c\"%d %08x\\0A\\00\"\n\
         @fmtb = private constant [11 x i8] c\"bad %d %d\\0A\\00\"\n",
        i386_support::TRIPLE
    );
    let mut main = String::new();
    for w in WIDTHS {
        let t = format!("i{w}");
        let pairs = mul_pairs(w);
        let n = pairs.len();
        let (mut a, mut b, mut r, mut f) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &(x, y) in &pairs {
            let (p, so, uo) = mul_truth(w, x, y);
            a.push(format!("{t} {}", lit(w, x)));
            b.push(format!("{t} {}", lit(w, y)));
            r.push(format!("{t} {}", lit(w, p)));
            f.push(format!("i8 {}", u8::from(so) | (u8::from(uo) << 1)));
        }
        for (name, items, ty) in [("a", &a, &t), ("b", &b, &t), ("r", &r, &t)] {
            writeln!(s, "@m{name}{w} = private constant [{n} x {ty}] [{}]", items.join(", "))
                .unwrap();
        }
        writeln!(s, "@mf{w} = private constant [{n} x i8] [{}]", f.join(", ")).unwrap();
        for op in ["smul", "umul"] {
            writeln!(s, "declare {{ {t}, i1 }} @llvm.{op}.with.overflow.{t}({t}, {t})").unwrap();
        }
        write!(
            s,
            "define i32 @m{w}() #0 {{
entry:
  %buf = alloca [16 x i8], align 16
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %next ]
  %h = phi i32 [ 0, %entry ], [ %h5, %next ]
  %pa = getelementptr {t}, ptr @ma{w}, i32 %i
  %pb = getelementptr {t}, ptr @mb{w}, i32 %i
  %pr = getelementptr {t}, ptr @mr{w}, i32 %i
  %pf = getelementptr i8, ptr @mf{w}, i32 %i
  %a = load {t}, ptr %pa
  %b = load {t}, ptr %pb
  %r = load {t}, ptr %pr
  %f = load i8, ptr %pf
  %m = mul {t} %a, %b
  %so = call {{ {t}, i1 }} @llvm.smul.with.overflow.{t}({t} %a, {t} %b)
  %uo = call {{ {t}, i1 }} @llvm.umul.with.overflow.{t}({t} %a, {t} %b)
  %sv = extractvalue {{ {t}, i1 }} %so, 0
  %sf = extractvalue {{ {t}, i1 }} %so, 1
  %uv = extractvalue {{ {t}, i1 }} %uo, 0
  %uf = extractvalue {{ {t}, i1 }} %uo, 1
  %e1 = icmp eq {t} %m, %r
  %e2 = icmp eq {t} %sv, %r
  %e3 = icmp eq {t} %uv, %r
  %fs = zext i1 %sf to i8
  %fu = zext i1 %uf to i8
  %fu2 = shl i8 %fu, 1
  %fl = or i8 %fs, %fu2
  %e4 = icmp eq i8 %fl, %f
  %o1 = and i1 %e1, %e2
  %o2 = and i1 %e3, %e4
  %ok = and i1 %o1, %o2
  store i128 0, ptr %buf
  store {t} %m, ptr %buf
  %q1 = getelementptr i8, ptr %buf, i32 4
  %q2 = getelementptr i8, ptr %buf, i32 8
  %q3 = getelementptr i8, ptr %buf, i32 12
  %w0 = load i32, ptr %buf
  %w1 = load i32, ptr %q1
  %w2 = load i32, ptr %q2
  %w3 = load i32, ptr %q3
  %fl32 = zext i8 %fl to i32
  %h0 = mul i32 %h, 31
  %h1 = xor i32 %h0, %w0
  %h2 = add i32 %h1, %w1
  %h3 = xor i32 %h2, %w2
  %h4 = add i32 %h3, %w3
  %h5 = xor i32 %h4, %fl32
  br i1 %ok, label %next, label %bad
bad:
  %u0 = call i32 (ptr, ...) @printf(ptr @fmtb, i32 {w}, i32 %i)
  ret i32 1
next:
  %i1 = add i32 %i, 1
  %done = icmp eq i32 %i1, {n}
  br i1 %done, label %out, label %loop
out:
  %u1 = call i32 (ptr, ...) @printf(ptr @fmtm, i32 {w}, i32 %h5)
  ret i32 0
}}
"
        )
        .unwrap();
        writeln!(
            main,
            "  %r{w} = call i32 @m{w}()\n  %z{w} = icmp ne i32 %r{w}, 0\n  \
             br i1 %z{w}, label %fail, label %ok{w}\nok{w}:"
        )
        .unwrap();
    }
    write!(
        s,
        "define i32 @main() #0 {{\nentry:\n{main}  ret i32 0\nfail:\n  ret i32 1\n}}\n{}",
        attributes(cpu)
    )
    .unwrap();
    s
}

/// d. a random differential test of the multiplications against the exact
/// result (llc's run must exit 0) and llc's output.
#[test]
fn multiply_random_as_llc() {
    for w in WIDTHS {
        assert!(mul_pairs(w).len() >= 2000);
    }
    let runs = check_edges("mul", mul_program);
    assert!(runs > 0, "no run was made");
    eprintln!("i386 wide multiplication: {runs} runs equal llc's");
}
