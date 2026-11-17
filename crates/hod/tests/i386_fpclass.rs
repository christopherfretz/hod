//! WP-337 (`spec/codegen-x86.md` "Floating-point classes and `modf`"):
//! `llvm.is.fpclass` and `llvm.modf` of `float`, `double` and `x86_fp80`
//! on i386, run against `llc` (`-O0 -global-isel=0 -fast-isel=0`) under
//! `qemu-i386`, plain and under `Stress::ALL`, static and PIE:
//! - every one of the 1024 class masks against bit patterns of every
//!   class and both signs (for `x86_fp80` also pseudo-denormals,
//!   unnormals, pseudo-zeros, pseudo-infinities and pseudo-NaNs), at CPU
//!   `generic`, `i686` (`cmov`) and `pentium4` (SSE2), `llc` 19 and 22;
//! - the classes of products that round at their type (overflow,
//!   underflow to a subnormal or to zero) with a use of the 80-bit
//!   product in the same run;
//! - `modf` (LLVM 22 only: LLVM 19 has no `llvm.modf`), whose call is an
//!   x87 barrier.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;
use std::process::Command;

use hod_testutil::Scratch;
use hod_testutil::target::Target;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, hod_objects, lane, refusal};
use i386_support::{checked, llc_object, llvm_tool, run_with_features, tool};

/// QEMU's features of an SSE2 build (`pentium4`).
const P4: &[&str] = &["mmx", "sse", "sse2"];

/// The CPUs of the runs: the function attribute (so `Stress::ALL`
/// applies too) and the QEMU features its code needs.
const CPUS: [(Option<&str>, &[&str]); 3] =
    [(None, &[]), (Some("i686"), &[]), (Some("pentium4"), P4)];

/// `float` patterns: every class, both signs.
fn f32_patterns() -> Vec<u32> {
    let pos = [
        0,
        1,
        0x007f_ffff,
        0x0080_0000,
        0x3f80_0000,
        0x7f7f_ffff,
        0x7f80_0000,
        0x7f80_0001,
        0x7fbf_ffff,
        0x7fc0_0000,
        0x7fff_ffff,
    ];
    pos.iter().flat_map(|&p| [p, p | 0x8000_0000]).collect()
}

/// `double` patterns: every class, both signs, payloads in each word.
fn f64_patterns() -> Vec<u64> {
    let pos = [
        0,
        1,
        0x0000_0001_0000_0000,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x3ff0_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000,
        0x7ff0_0000_0000_0001,
        0x7ff4_0000_0000_0000,
        0x7ff7_ffff_ffff_ffff,
        0x7ff8_0000_0000_0000,
        0x7fff_ffff_ffff_ffff,
    ];
    pos.iter().flat_map(|&p| [p, p | (1 << 63)]).collect()
}

/// `x86_fp80` patterns (sign and exponent, significand): every class
/// and every encoding only the x87 has, both signs.
fn f80_patterns(pseudo_denormals: bool) -> Vec<(u16, u64)> {
    const J: u64 = 1 << 63;
    let pos = [
        (0, 0),
        (0, 1),
        (0, J - 1),
        (0, J),
        (0, J | 1),
        (1, J),
        (0x3fff, J),
        (0x7ffe, u64::MAX),
        (0x3fff, 0),
        (0x3fff, 1 << 62),
        (0x7fff, J),
        (0x7fff, 0),
        (0x7fff, 1),
        (0x7fff, 1 << 62),
        (0x7fff, J | 1),
        (0x7fff, J | ((1 << 62) - 1)),
        (0x7fff, J | (1 << 62)),
        (0x7fff, u64::MAX),
    ];
    // Pseudo-denormals (exponent 0, the integer bit set): `llc` 19 and
    // 22 differ for them (masks 7, 243, 515, 780), so only the LLVM 22
    // table has them.
    let pseudo_denormal = |&(e, m): &(u16, u64)| e == 0 && m & J != 0;
    pos.iter()
        .filter(|p| pseudo_denormals || !pseudo_denormal(p))
        .flat_map(|&(e, m)| [(e, m), (e | 0x8000, m)])
        .collect()
}

/// The attribute group of the functions for `cpu`.
fn attrs(cpu: Option<&str>) -> String {
    cpu.map(|c| format!("attributes #0 = {{ \"target-cpu\"=\"{c}\" }}\n")).unwrap_or_default()
}

/// The function that prints `r` (an `i32` of `n` result bits) as `n`
/// digits and a newline.
const PRINT_BITS: &str = "
declare i32 @putchar(i32)
define void @bits(i32 %r, i32 %n) #0 {
entry:
  br label %l
l:
  %i = phi i32 [ 0, %entry ], [ %i1, %l ]
  %s = lshr i32 %r, %i
  %b = and i32 %s, 1
  %c = add i32 %b, 48
  %u = call i32 @putchar(i32 %c)
  %i1 = add i32 %i, 1
  %more = icmp ult i32 %i1, %n
  br i1 %more, label %l, label %done
done:
  %v = call i32 @putchar(i32 10)
  ret void
}
";

/// The program of the mask table: for each type a function per mask
/// (`ptr` to the pattern, the class test of the loaded value), a table
/// of them, and `@sweep`, which prints one line per mask with the result
/// of every pattern.
fn table_program(cpu: Option<&str>) -> String {
    table(cpu, false)
}

/// [`table_program`] with pseudo-denormals among the `x86_fp80` patterns.
fn table_program_22(cpu: Option<&str>) -> String {
    table(cpu, true)
}

/// The mask table, with pseudo-denormals or without.
fn table(cpu: Option<&str>, pseudo_denormals: bool) -> String {
    let mut s = format!("target triple = \"{}\"\n", i386_support::TRIPLE);
    let f32s = f32_patterns();
    let f64s = f64_patterns();
    let f80s = f80_patterns(pseudo_denormals);
    let list = |items: Vec<String>| items.join(", ");
    writeln!(
        s,
        "@p_f32 = global [{} x i32] [{}]",
        f32s.len(),
        list(f32s.iter().map(|p| format!("i32 {}", *p as i32)).collect())
    )
    .unwrap();
    writeln!(
        s,
        "@p_f64 = global [{} x i64] [{}]",
        f64s.len(),
        list(f64s.iter().map(|p| format!("i64 {}", *p as i64)).collect())
    )
    .unwrap();
    writeln!(
        s,
        "@p_f80 = global [{} x [3 x i32]] [{}]",
        f80s.len(),
        list(
            f80s.iter()
                .map(|&(e, m)| {
                    format!(
                        "[3 x i32] [i32 {}, i32 {}, i32 {}]",
                        m as u32 as i32,
                        (m >> 32) as u32 as i32,
                        e
                    )
                })
                .collect()
        )
    )
    .unwrap();
    for (sfx, ty) in [("f32", "float"), ("f64", "double"), ("f80", "x86_fp80")] {
        writeln!(s, "declare i1 @llvm.is.fpclass.{sfx}({ty}, i32)").unwrap();
        for mask in 0..1024 {
            writeln!(
                s,
                "define i32 @c_{sfx}_{mask}(ptr %p) #0 {{\n  %x = load {ty}, ptr %p\n  \
                 %c = call i1 @llvm.is.fpclass.{sfx}({ty} %x, i32 {mask})\n  \
                 %r = zext i1 %c to i32\n  ret i32 %r\n}}"
            )
            .unwrap();
        }
        let fns: Vec<String> = (0..1024).map(|m| format!("ptr @c_{sfx}_{m}")).collect();
        writeln!(s, "@t_{sfx} = global [1024 x ptr] [{}]", fns.join(", ")).unwrap();
    }
    s.push_str(
        "
define void @sweep(ptr %tab, ptr %pat, i32 %n, i32 %stride) #0 {
entry:
  br label %m
m:
  %mi = phi i32 [ 0, %entry ], [ %mn, %mend ]
  %fp = getelementptr ptr, ptr %tab, i32 %mi
  %fn = load ptr, ptr %fp
  br label %i
i:
  %ii = phi i32 [ 0, %m ], [ %in, %i ]
  %off = mul i32 %ii, %stride
  %pp = getelementptr i8, ptr %pat, i32 %off
  %r = call i32 %fn(ptr %pp)
  %ch = add i32 %r, 48
  %u = call i32 @putchar(i32 %ch)
  %in = add i32 %ii, 1
  %c = icmp ult i32 %in, %n
  br i1 %c, label %i, label %mend
mend:
  %nl = call i32 @putchar(i32 10)
  %mn = add i32 %mi, 1
  %c2 = icmp ult i32 %mn, 1024
  br i1 %c2, label %m, label %done
done:
  ret void
}
",
    );
    writeln!(
        s,
        "define i32 @main() #0 {{\n  \
         call void @sweep(ptr @t_f32, ptr @p_f32, i32 {}, i32 4)\n  \
         call void @sweep(ptr @t_f64, ptr @p_f64, i32 {}, i32 8)\n  \
         call void @sweep(ptr @t_f80, ptr @p_f80, i32 {}, i32 12)\n  ret i32 0\n}}",
        f32s.len(),
        f64s.len(),
        f80s.len()
    )
    .unwrap();
    s.push_str(PRINT_BITS);
    s.push_str(&attrs(cpu));
    s
}

/// The program of products: `x * y` classified (it rounds at its type),
/// then `x * y / y` (computed from the product in the same run: at 80
/// bits on the x87) classified, for operands that overflow, underflow to
/// a subnormal and to zero, and a plain one.
fn product_program(cpu: Option<&str>) -> String {
    let mut s = format!("target triple = \"{}\"\n", i386_support::TRIPLE);
    let pairs32: [(f32, f32); 4] = [(1e30, 1e30), (1e-30, 1e-15), (1e-30, 1e-30), (2.0, 3.0)];
    let pairs64: [(f64, f64); 4] = [(1e300, 1e300), (1e-160, 1e-160), (1e-200, 1e-200), (2.0, 3.0)];
    for (sfx, ty, int, bits) in [
        (
            "f32",
            "float",
            "i32",
            pairs32
                .iter()
                .map(|p| (p.0.to_bits().into(), p.1.to_bits().into()))
                .collect::<Vec<(u64, u64)>>(),
        ),
        ("f64", "double", "i64", pairs64.iter().map(|p| (p.0.to_bits(), p.1.to_bits())).collect()),
    ] {
        let words: Vec<String> =
            bits.iter().flat_map(|&(a, b)| [a, b]).map(|w| format!("{int} {}", w as i64)).collect();
        writeln!(s, "@q_{sfx} = global [{} x {int}] [{}]", words.len(), words.join(", ")).unwrap();
        writeln!(s, "declare i1 @llvm.is.fpclass.{sfx}({ty}, i32)").unwrap();
        let mut body = format!(
            "define i32 @prod_{sfx}(ptr %a, ptr %b) #0 {{\n  %x = load {ty}, ptr %a\n  \
             %y = load {ty}, ptr %b\n  %p = fmul {ty} %x, %y\n"
        );
        let masks = [516, 264, 144, 96, 3, 1020];
        let mut k = 0;
        for v in ["p", "q"] {
            if v == "q" {
                writeln!(body, "  %q = fdiv {ty} %p, %y").unwrap();
            }
            for m in masks {
                writeln!(
                    body,
                    "  %c{k} = call i1 @llvm.is.fpclass.{sfx}({ty} %{v}, i32 {m})\n  \
                     %z{k} = zext i1 %c{k} to i32\n  %s{k} = shl i32 %z{k}, {k}"
                )
                .unwrap();
                k += 1;
            }
        }
        body.push_str("  %o0 = or i32 %s0, 0\n");
        for i in 1..k {
            writeln!(body, "  %o{i} = or i32 %o{}, %s{i}", i - 1).unwrap();
        }
        writeln!(body, "  ret i32 %o{}\n}}", k - 1).unwrap();
        s.push_str(&body);
    }
    s.push_str("define i32 @main() #0 {\n");
    for (sfx, int) in [("f32", "i32"), ("f64", "i64")] {
        for i in 0..4 {
            writeln!(
                s,
                "  %a_{sfx}_{i} = getelementptr {int}, ptr @q_{sfx}, i32 {}\n  \
                 %b_{sfx}_{i} = getelementptr {int}, ptr @q_{sfx}, i32 {}\n  \
                 %r_{sfx}_{i} = call i32 @prod_{sfx}(ptr %a_{sfx}_{i}, ptr %b_{sfx}_{i})\n  \
                 call void @bits(i32 %r_{sfx}_{i}, i32 12)",
                2 * i,
                2 * i + 1
            )
            .unwrap();
        }
    }
    s.push_str("  ret i32 0\n}\n");
    s.push_str(PRINT_BITS);
    s.push_str(&attrs(cpu));
    s
}

/// `make(cpu)` at every CPU of [`CPUS`], static and PIE, run as `llc`'s
/// with `llc` 19 and 22.
fn run_everywhere(name: &str, make: fn(Option<&str>) -> String) {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new(&format!("i386-fpclass-{name}"));
    let mut runs = 0;
    for (cpu, features) in CPUS {
        let text = make(cpu);
        for pic in [false, true] {
            let build = Build { pic, features, ..STATIC };
            let label = format!("{name}-{}", cpu.unwrap_or("generic"));
            runs += lane.check(&scratch, &label, &Source::Ll(&text), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 fpclass {name}: {runs} runs equal llc's");
}

#[test]
fn every_mask_against_every_class() {
    run_everywhere("table", table_program);
}

#[test]
fn every_mask_against_pseudo_denormals_as_llc_22() {
    let runs: usize = CPUS
        .iter()
        .map(|&(cpu, features)| {
            run_22(
                &format!("table22-{}", cpu.unwrap_or("generic")),
                &table_program_22(cpu),
                features,
            )
        })
        .sum();
    eprintln!("i386 fpclass table with pseudo-denormals: {runs} runs equal llc 22's");
}

#[test]
fn products_classified_rounded_with_an_80_bit_use() {
    run_everywhere("products", product_program);
}

#[test]
fn vectors_and_half_stay_refused() {
    // A vector operand keeps the classifier's vector refusal.
    let vector = "declare <2 x i1> @llvm.is.fpclass.v2f64(<2 x double>, i32)
define <2 x i1> @f(<2 x double> %x) {
  %r = call <2 x i1> @llvm.is.fpclass.v2f64(<2 x double> %x, i32 3)
  ret <2 x i1> %r
}";
    let error = refusal(vector, None).expect("a vector is.fpclass compiles");
    assert!(error.contains("intrinsic:llvm.is.fpclass/vector"), "{error}");
    // `half` keeps its refusal.
    let half = "declare i1 @llvm.is.fpclass.f16(half, i32)
define i1 @f(ptr %p) {
  %x = load half, ptr %p
  %r = call i1 @llvm.is.fpclass.f16(half %x, i32 3)
  ret i1 %r
}";
    assert!(refusal(half, None).is_some(), "a half is.fpclass compiles");
}

// ---- modf (LLVM 22 only) ----

/// `modf` of each type through the pair, a product in the run before the
/// call and used after it (read rounded: the call is a barrier), and the
/// libm call under PIC.
fn modf_program() -> String {
    let mut s = format!("target triple = \"{}\"\n", i386_support::TRIPLE);
    s.push_str(
        "declare i32 @printf(ptr, ...)
@fmt = private constant [11 x i8] c\"%a %a %La\\0A\\00\"
@fmtl = private constant [13 x i8] c\"%La %La %La\\0A\\00\"
declare { float, float } @llvm.modf.f32(float)
declare { double, double } @llvm.modf.f64(double)
declare { x86_fp80, x86_fp80 } @llvm.modf.f80(x86_fp80)
define void @df(double %x, double %y) {
  %p = fmul double %x, %y
  %r = call { double, double } @llvm.modf.f64(double %p)
  %f = extractvalue { double, double } %r, 0
  %i = extractvalue { double, double } %r, 1
  %e = fpext double %p to x86_fp80
  %u = call i32 (ptr, ...) @printf(ptr @fmt, double %f, double %i, x86_fp80 %e)
  ret void
}
define void @ff(float %x) {
  %r = call { float, float } @llvm.modf.f32(float %x)
  %f = extractvalue { float, float } %r, 0
  %i = extractvalue { float, float } %r, 1
  %fd = fpext float %f to double
  %id = fpext float %i to double
  %u = call i32 (ptr, ...) @printf(ptr @fmt, double %fd, double %id, x86_fp80 0xK00000000000000000000)
  ret void
}
define void @fl(x86_fp80 %x) {
  %r = call { x86_fp80, x86_fp80 } @llvm.modf.f80(x86_fp80 %x)
  %f = extractvalue { x86_fp80, x86_fp80 } %r, 0
  %i = extractvalue { x86_fp80, x86_fp80 } %r, 1
  %u = call i32 (ptr, ...) @printf(ptr @fmtl, x86_fp80 %f, x86_fp80 %i, x86_fp80 %x)
  ret void
}
define i32 @main() {
  call void @df(double 2.5, double 3.0)
  call void @df(double -1.25, double 0x3FB999999999999A)
  call void @df(double 0x7FE0000000000000, double 4.0)
  call void @df(double 1.0e300, double 1.0e10)
  call void @ff(float 7.75)
  call void @ff(float -0.5)
  call void @fl(x86_fp80 0xK4000C000000000000000)
  call void @fl(x86_fp80 0xKC03EFFFFFFFFFFFFFFFF)
  call void @fl(x86_fp80 0xK7FFF8000000000000000)
  ret i32 0
}
",
    );
    s
}

#[test]
fn modf_of_each_type_as_llc_22() {
    let runs = run_22("modf", &modf_program(), &[]);
    eprintln!("i386 modf: {runs} runs equal llc 22's");
}

/// `text` built by `llc` 22 and by hod (plain and under `Stress::ALL`),
/// static and PIE, linked by GNU ld with libm, run with QEMU's
/// `features`: every run prints what `llc`'s prints. The number of runs.
fn run_22(name: &str, text: &str, features: &[&str]) -> usize {
    let target = Target::I686;
    let (Some(llc), Some(gcc), Some(qemu)) =
        (llvm_tool(22, "llc"), tool(&target.c_driver()), tool(&target.emulator()))
    else {
        return 0;
    };
    let scratch = Scratch::new(&format!("i386-fpclass-{name}"));
    let ll = scratch.write(&format!("{name}.ll"), text);
    let mut runs = 0;
    for pic in [false, true] {
        let label = format!("{name}-{}", if pic { "pie" } else { "exe" });
        let (plain, stress) = hod_objects(text.as_bytes(), Build { pic, ..STATIC }, &label);
        let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
        let objects = [
            llc_object(&scratch, &llc, &ll, flags, &format!("{label}-llc")),
            scratch.write(&format!("{label}-hod.o"), plain),
            scratch.write(&format!("{label}-stress.o"), stress),
        ];
        let mut outputs = Vec::new();
        for (n, object) in objects.iter().enumerate() {
            let exe = scratch.join(format!("{label}-{n}"));
            let mut command = Command::new(&gcc);
            command.arg(if pic { "-pie" } else { "-no-pie" }).arg(object).arg("-lm");
            command.arg("-o").arg(&exe);
            checked(&format!("link of {label} ({n})"), &mut command);
            let (status, out) = run_with_features(&qemu, &exe, features);
            assert_eq!(status, Some(0), "{label} ({n})");
            outputs.push(out);
            runs += 1;
        }
        assert!(!outputs[0].is_empty());
        for out in &outputs[1..] {
            assert_eq!(
                String::from_utf8_lossy(&outputs[0]),
                String::from_utf8_lossy(out),
                "{label}: hod's output differs from llc's"
            );
        }
    }
    runs
}

// ---- `bitcast` between `x86_fp80` and `i80` ----

/// Every predicate of `icmp i80 %x, 0` on the bits of the `x86_fp80`
/// patterns (both signs, zeros, NaNs, the x87-only encodings), and the
/// round trip `x86_fp80` to `i80` and back.
fn i80_program(cpu: Option<&str>) -> String {
    let mut s = format!("target triple = \"{}\"\n", i386_support::TRIPLE);
    let pats = f80_patterns(true);
    let words: Vec<String> = pats
        .iter()
        .map(|&(e, m)| {
            format!(
                "[3 x i32] [i32 {}, i32 {}, i32 {}]",
                m as u32 as i32,
                (m >> 32) as u32 as i32,
                e
            )
        })
        .collect();
    writeln!(s, "@p = global [{} x [3 x i32]] [{}]", pats.len(), words.join(", ")).unwrap();
    s.push_str("define i32 @preds(ptr %p, ptr %out) #0 {\n  %x = load x86_fp80, ptr %p\n  %b = bitcast x86_fp80 %x to i80\n");
    let preds = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];
    for (k, pred) in preds.iter().enumerate() {
        writeln!(
            s,
            "  %c{k} = icmp {pred} i80 %b, 0\n  %z{k} = zext i1 %c{k} to i32\n  %s{k} = shl i32 %z{k}, {k}"
        )
        .unwrap();
    }
    s.push_str("  %o0 = or i32 %s0, 0\n");
    for k in 1..preds.len() {
        writeln!(s, "  %o{k} = or i32 %o{}, %s{k}", k - 1).unwrap();
    }
    s.push_str(
        "  %back = bitcast i80 %b to x86_fp80
  store x86_fp80 %back, ptr %out
  ret i32 %o9
}
define i32 @main() #0 {
entry:
  %out = alloca [3 x i32], align 4
  br label %l
l:
  %i = phi i32 [ 0, %entry ], [ %i1, %l ]
  store [3 x i32] [i32 -1, i32 -1, i32 -1], ptr %out
  %pp = getelementptr [3 x i32], ptr @p, i32 %i
  %r = call i32 @preds(ptr %pp, ptr %out)
  call void @bits(i32 %r, i32 10)
  %w0 = load i32, ptr %out
  %o1 = getelementptr i32, ptr %out, i32 1
  %w1 = load i32, ptr %o1
  %o2 = getelementptr i32, ptr %out, i32 2
  %w2 = load i32, ptr %o2
  call void @bits(i32 %w0, i32 32)
  call void @bits(i32 %w1, i32 32)
  call void @bits(i32 %w2, i32 32)
  %i1 = add i32 %i, 1
  %more = icmp ult i32 %i1, ",
    );
    writeln!(s, "{}\n  br i1 %more, label %l, label %done\ndone:\n  ret i32 0\n}}", pats.len())
        .unwrap();
    s.push_str(PRINT_BITS);
    s.push_str(&attrs(cpu));
    s
}

#[test]
fn i80_compares_with_zero_and_the_round_trip() {
    run_everywhere("i80", i80_program);
}

/// `signbit`, `isnan`, `isinf`, `isfinite`, `isnormal` and `fpclassify`
/// of `long double` as clang writes them (`bitcast` to `i80` for
/// `signbit`; `is.fpclass` with clang 22), over the `x86_fp80` patterns.
const LONG_DOUBLE_C: &str = r#"#include <math.h>
#include <stdio.h>
#include <string.h>
static const unsigned short se[] = {0, 0x8000, 0, 0x3fff, 0xbfff, 0x7ffe, 0x7fff, 0xffff, 0x7fff, 0x7fff, 0xffff, 0x3fff, 0x7fff, 0};
static const unsigned long long m[] = {0, 0, 1, 1ull << 63, 1ull << 63, ~0ull, 1ull << 63, 1ull << 63, 3ull << 62, (1ull << 63) | 1, 3ull << 62, 1ull << 62, 0, 1ull << 63};
int main(void) {
  for (unsigned i = 0; i < sizeof se / sizeof se[0]; i++) {
    long double x;
    unsigned char b[sizeof x];
    memset(b, 0, sizeof b);
    memcpy(b, &m[i], 8);
    memcpy(b + 8, &se[i], 2);
    memcpy(&x, b, sizeof x);
    printf("%d %d %d %d %d %d\n", signbit(x) != 0, isnan(x) != 0, isinf(x) != 0,
           isfinite(x) != 0, isnormal(x) != 0, fpclassify(x));
  }
  return 0;
}
"#;

#[test]
fn long_double_classification_from_c() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-fpclass-long-double");
    let source = scratch.write("long_double.c", LONG_DOUBLE_C);
    let mut runs = 0;
    for (flags, features) in
        [(&[][..], &[][..]), (&["-march=i586"][..], &[][..]), (&["-march=pentium4"][..], P4)]
    {
        for pic in [false, true] {
            let build = Build { pic, features, ..STATIC };
            let name = format!("long-double-{}", flags.first().unwrap_or(&"i686"));
            runs += lane.check(&scratch, &name, &Source::C(&source, flags), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 long double classification: {runs} runs equal llc's");
}

#[test]
fn other_i80_operations_stay_refused() {
    let shift = "define i32 @f(x86_fp80 %x) {
  %b = bitcast x86_fp80 %x to i80
  %s = lshr i80 %b, 64
  %t = trunc i80 %s to i32
  ret i32 %t
}";
    let error = refusal(shift, None).expect("an i80 shift compiles");
    assert!(error.contains("x86:i64:wide"), "{error}");
}
