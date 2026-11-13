//! Variable arguments before a forwarded `...` in a `musttail` call
//! (WP-115, `spec/codegen.md` §7.1 "The `musttail` hook", `spec/abi-a64.md`
//! §8.5, §9.1), run natively against `llc` 19 and 22.
//!
//! Each generated case is a variadic thunk `t_k(named..., ...)` that
//! tail-calls `c_k(named..., extras..., ...)`: the named parameters are
//! `i64`s and `double`s (enough of them to put some on the stack), the
//! extras are 0 to 6 values of every class AArch64 passes (integers of
//! all widths, `float`, `double`, `fp128`, `[2 x i64]`, and the blocks
//! `[2 x double]`, `[3 x float]`), constants or computed from a named
//! parameter, and some thunks first call a function that sets every
//! argument register. `c_k` is C: it prints what it receives, named,
//! extras and then what is left of the forwarded arguments (registers by
//! the `va_list`'s own counters, then the stack). The forwarded stack
//! words, right after the named stack area, are canaries checked
//! exactly: that is the area a thunk must never write, so a tail call
//! that writes beyond its caller's named area is caught there (and one
//! that writes further clobbers `run_k`'s frame record). `run_k` also
//! keeps canaries in its frame and values in callee-saved registers
//! across the call and checks them after it.
//!
//! The named and extra values printed are checked against the
//! generator's; the whole output (forwarded values included) must equal
//! that of the same program with the thunks built by `llc`, under GNU
//! ld, lld and mold, with hod's plain and stress register pools.
//!
//! Whether a case compiles is decided by `classify` with
//! `hod_cg::abi::TargetStackArgs`: the extras fit in registers (they
//! compile) or some go to the stack, beyond the caller's area
//! (`musttail:stack-args-grow`). Which one is predicted here by the
//! AAPCS64 rules of `spec/abi-a64.md` §3 written out in the test.

#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "generated values of small ranges, named as the C side names them"
)]

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{emit, ll_header, parse, write_object};
use hod_cg::{Options, Stress};
use hod_ir::{Arch, Census, Classifier, Support};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The classes of extra arguments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    F128,
    L2,
    D2,
    F3,
}

const KINDS: [Kind; 11] = [
    Kind::I8,
    Kind::I16,
    Kind::I32,
    Kind::I64,
    Kind::I128,
    Kind::F32,
    Kind::F64,
    Kind::F128,
    Kind::L2,
    Kind::D2,
    Kind::F3,
];

/// One extra argument: its IR operand, any instruction computing it,
/// how C reads and prints it, and what it prints.
struct Extra {
    kind: Kind,
    operand: String,
    inst: Option<String>,
    expect: String,
}

struct Case {
    k: usize,
    /// The named parameters: `true` for `i64`, `false` for `double`. The
    /// first is an `i64`.
    named: Vec<bool>,
    extras: Vec<Extra>,
    /// Forwarded `i64`s main passes on the stack, right after the named
    /// stack area: canaries in the one place a thunk must not write.
    stack_longs: usize,
    clobber: bool,
}

/// The value main passes as named parameter `i` of case `k`.
fn named_long(k: usize, i: usize) -> u64 {
    0x10_0000 * (k as u64 + 1) + i as u64
}

fn named_double(k: usize, i: usize) -> f64 {
    (k * 16 + i) as f64 + 0.5
}

/// Forwarded stack word `j` of case `k`.
fn stack_canary(k: usize, j: usize) -> u64 {
    0xE000_0000 + (k as u64) * 256 + j as u64
}

fn f32_value(rng: &mut Rng) -> f32 {
    (rng.below(2000) as f32 - 1000.0) / 8.0
}

fn f64_value(rng: &mut Rng) -> f64 {
    (rng.below(200_000) as f64 - 100_000.0) / 64.0
}

/// A `float` constant in LLVM's hexadecimal form (the `double` of it).
fn float_ir(value: f32) -> String {
    format!("0x{:016X}", f64::from(value).to_bits())
}

fn extra(rng: &mut Rng, pool: &[Kind], case_named: &[bool], k: usize, n: usize) -> Extra {
    let kind = pool[rng.below(pool.len() as u64) as usize];
    let r = rng.next();
    let mut inst = None;
    let (operand, expect) = match kind {
        Kind::I8 => (format!("i8 {}", r as u8 as i8), format!("{:x}", r as u8)),
        Kind::I16 => (format!("i16 {}", r as u16 as i16), format!("{:x}", r as u16)),
        Kind::I32 => (format!("i32 {}", r as u32 as i32), format!("{:x}", r as u32)),
        Kind::I64 => {
            // Half of them computed from a named `i64`.
            let ints: Vec<usize> = (0..case_named.len()).filter(|&i| case_named[i]).collect();
            if r & 1 == 1 {
                let i = ints[rng.below(ints.len() as u64) as usize];
                let add = r >> 40;
                let name = format!("%e{n}");
                inst = Some(format!("  {name} = add i64 %p{i}, {add}\n"));
                (format!("i64 {name}"), format!("{:x}", named_long(k, i).wrapping_add(add)))
            } else {
                (format!("i64 {}", r as i64), format!("{r:x}"))
            }
        }
        Kind::I128 => {
            let hi = rng.next() >> 1;
            let value = (u128::from(hi) << 64) | u128::from(r);
            (format!("i128 {value}"), format!("{hi:x}:{r:x}"))
        }
        Kind::F32 => {
            let v = f32_value(rng);
            (format!("float {}", float_ir(v)), format!("{:x}", v.to_bits()))
        }
        Kind::F64 => {
            let v = f64_value(rng);
            (format!("double 0x{:016X}", v.to_bits()), format!("{:x}", v.to_bits()))
        }
        Kind::F128 => {
            let hi = rng.next();
            (format!("fp128 0xL{r:016X}{hi:016X}"), format!("{hi:x}:{r:x}"))
        }
        Kind::L2 => {
            let b = rng.next();
            (format!("[2 x i64] [i64 {}, i64 {}]", r as i64, b as i64), format!("{r:x}:{b:x}"))
        }
        Kind::D2 => {
            let (a, b) = (f64_value(rng), f64_value(rng));
            (
                format!(
                    "[2 x double] [double 0x{:016X}, double 0x{:016X}]",
                    a.to_bits(),
                    b.to_bits()
                ),
                format!("{:x}:{:x}", a.to_bits(), b.to_bits()),
            )
        }
        Kind::F3 => {
            let v = [f32_value(rng), f32_value(rng), f32_value(rng)];
            (
                format!(
                    "[3 x float] [float {}, float {}, float {}]",
                    float_ir(v[0]),
                    float_ir(v[1]),
                    float_ir(v[2])
                ),
                format!("{:x}:{:x}:{:x}", v[0].to_bits(), v[1].to_bits(), v[2].to_bits()),
            )
        }
    };
    Extra { kind, operand, inst, expect }
}

/// The extras that go in general registers.
const INT_KINDS: [Kind; 6] = [Kind::I8, Kind::I16, Kind::I32, Kind::I64, Kind::I128, Kind::L2];
/// The extras that go in vector registers.
const FP_KINDS: [Kind; 5] = [Kind::F32, Kind::F64, Kind::F128, Kind::D2, Kind::F3];

fn random_case(rng: &mut Rng, k: usize) -> Case {
    // A third of the cases have named arguments on the stack, of one
    // class, and extras of the other.
    let (ints, doubles, pool): (usize, usize, &[Kind]) = match rng.below(6) {
        0 => (9 + rng.below(3) as usize, rng.below(5) as usize, &FP_KINDS),
        1 => (1 + rng.below(4) as usize, 9 + rng.below(3) as usize, &INT_KINDS),
        _ => (1 + rng.below(10) as usize, rng.below(10) as usize, &KINDS),
    };
    let mut named = vec![true];
    let (mut i, mut d) = (1, 0);
    while i < ints || d < doubles {
        let int = d == doubles || (i < ints && rng.below(2) == 0);
        named.push(int);
        if int {
            i += 1;
        } else {
            d += 1;
        }
    }
    let count = rng.below(7) as usize;
    let mut extras = Vec::new();
    for n in 0..count {
        extras.push(extra(rng, pool, &named, k, n));
    }
    Case { k, named, extras, stack_longs: 1 + rng.below(3) as usize, clobber: rng.below(3) == 0 }
}

/// AAPCS64 (`spec/abi-a64.md` §3): whether every extra of `case` lands in
/// a register after the named parameters.
fn extras_in_registers(case: &Case) -> bool {
    let mut ngrn = case.named.iter().filter(|&&int| int).count().min(8);
    let mut nsrn = case.named.iter().filter(|&&int| !int).count().min(8);
    for extra in &case.extras {
        let fits = match extra.kind {
            Kind::I8 | Kind::I16 | Kind::I32 | Kind::I64 => {
                ngrn += 1;
                ngrn <= 8
            }
            Kind::I128 => {
                ngrn = ngrn.next_multiple_of(2) + 2;
                ngrn <= 8
            }
            Kind::L2 => {
                ngrn += 2;
                ngrn <= 8
            }
            Kind::F32 | Kind::F64 | Kind::F128 => {
                nsrn += 1;
                nsrn <= 8
            }
            Kind::D2 => {
                nsrn += 2;
                nsrn <= 8
            }
            Kind::F3 => {
                nsrn += 3;
                nsrn <= 8
            }
        };
        if !fits {
            return false;
        }
    }
    true
}

fn ir_type(int: bool) -> &'static str {
    if int { "i64" } else { "double" }
}

/// The thunks and the declarations they need.
fn thunks(cases: &[&Case]) -> String {
    let mut out = String::new();
    out.push_str(
        "declare void @clobber(i64, i64, i64, i64, i64, i64, i64, i64, double, double, double, \
         double, double, double, double, double)\n",
    );
    for case in cases {
        let k = case.k;
        let types: Vec<&str> = case.named.iter().map(|&int| ir_type(int)).collect();
        let fn_ty = format!("i64 ({}, ...)", types.join(", "));
        let params: Vec<String> =
            types.iter().enumerate().map(|(i, ty)| format!("{ty} %p{i}")).collect();
        writeln!(out, "declare i64 @c_{k}({}, ...)", types.join(", ")).unwrap();
        writeln!(out, "define i64 @t_{k}({}, ...) noinline {{", params.join(", ")).unwrap();
        if case.clobber {
            out.push_str(
                "  call void @clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, \
                 i64 -1, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, \
                 double -9.0, double -9.0, double -9.0)\n",
            );
        }
        for extra in &case.extras {
            if let Some(inst) = &extra.inst {
                out.push_str(inst);
            }
        }
        let args: Vec<String> = params
            .iter()
            .cloned()
            .chain(case.extras.iter().map(|extra| extra.operand.clone()))
            .collect();
        writeln!(out, "  %r = musttail call {fn_ty} @c_{k}({}, ...)", args.join(", ")).unwrap();
        out.push_str("  ret i64 %r\n}\n");
    }
    out
}

/// How C reads and prints one extra.
fn c_read(kind: Kind) -> &'static str {
    match kind {
        Kind::I8 => "{ int v = va_arg(ap, int); printf(\" %x\", v & 0xff); }",
        Kind::I16 => "{ int v = va_arg(ap, int); printf(\" %x\", v & 0xffff); }",
        Kind::I32 => "{ unsigned v = va_arg(ap, unsigned); printf(\" %x\", v); }",
        Kind::I64 => "{ unsigned long v = va_arg(ap, unsigned long); printf(\" %lx\", v); }",
        Kind::I128 => {
            "{ unsigned __int128 v = va_arg(ap, unsigned __int128); \
             printf(\" %lx:%lx\", (unsigned long)(v >> 64), (unsigned long)v); }"
        }
        Kind::F32 => {
            "{ double v = va_arg(ap, double); unsigned long b; memcpy(&b, &v, 8); \
             printf(\" %lx\", b & 0xffffffff); }"
        }
        Kind::F64 => "{ double v = va_arg(ap, double); printf(\" %lx\", bits(v)); }",
        Kind::F128 => {
            "{ long double v = va_arg(ap, long double); unsigned long b[2]; memcpy(b, &v, 16); \
             printf(\" %lx:%lx\", b[1], b[0]); }"
        }
        Kind::L2 => "{ struct l2 v = va_arg(ap, struct l2); printf(\" %lx:%lx\", v.a, v.b); }",
        Kind::D2 => {
            "{ struct d2 v = va_arg(ap, struct d2); printf(\" %lx:%lx\", bits(v.a), bits(v.b)); }"
        }
        Kind::F3 => {
            "{ struct f3 v = va_arg(ap, struct f3); \
             printf(\" %x:%x:%x\", fbits(v.a), fbits(v.b), fbits(v.c)); }"
        }
    }
}

/// The C side: `clobber`, every `c_k`, every `run_k`, `main`.
fn driver(cases: &[&Case]) -> String {
    let mut out = String::from(
        "#include <stdarg.h>\n#include <stdio.h>\n#include <string.h>\n\
         struct l2 { unsigned long a, b; };\nstruct d2 { double a, b; };\n\
         struct f3 { float a, b, c; };\n\
         static unsigned long bits(double d) { unsigned long b; memcpy(&b, &d, 8); return b; }\n\
         static unsigned fbits(float f) { unsigned b; memcpy(&b, &f, 4); return b; }\n\
         static long opaque(long x) { __asm__ volatile(\"\" : \"+r\"(x)); return x; }\n\
         static double opaqued(double x) { __asm__ volatile(\"\" : \"+w\"(x)); return x; }\n\
         static int bad;\n\
         __attribute__((noinline)) void clobber(long a, long b, long c, long d, long e, long f, \
         long g, long h, double i, double j, double k, double l, double m, double n, double o, \
         double p) { __asm__ volatile(\"\" ::: \"memory\"); }\n",
    );
    for case in cases {
        let k = case.k;
        let params: Vec<String> = case
            .named
            .iter()
            .enumerate()
            .map(|(i, &int)| format!("{} p{i}", if int { "unsigned long" } else { "double" }))
            .collect();
        let last = case.named.len() - 1;
        writeln!(out, "long t_{k}({}, ...);", params.join(", ")).unwrap();
        writeln!(
            out,
            "long c_{k}({}, ...) {{\n  va_list ap;\n  va_start(ap, p{last});",
            params.join(", ")
        )
        .unwrap();
        writeln!(out, "  printf(\"{k}:\");").unwrap();
        for (i, &int) in case.named.iter().enumerate() {
            if int {
                writeln!(out, "  printf(\" %lx\", p{i});").unwrap();
            } else {
                writeln!(out, "  printf(\" %lx\", bits(p{i}));").unwrap();
            }
        }
        out.push_str("  printf(\" |\");\n");
        for extra in &case.extras {
            writeln!(out, "  {}", c_read(extra.kind)).unwrap();
        }
        out.push_str(
            "  printf(\" |\");\n\
             while (ap.__gr_offs < 0) printf(\" %lx\", va_arg(ap, unsigned long));\n\
             while (ap.__vr_offs < 0) printf(\" %lx\", bits(va_arg(ap, double)));\n",
        );
        writeln!(
            out,
            "  for (int i = 0; i < {}; i++) printf(\" s%lx\", va_arg(ap, unsigned long));",
            case.stack_longs
        )
        .unwrap();
        writeln!(out, "  va_end(ap);\n  return {};\n}}", k * 7 + 1).unwrap();

        // The caller: canaries in its frame, values across the call.
        let mut args: Vec<String> = case
            .named
            .iter()
            .enumerate()
            .map(|(i, &int)| {
                if int {
                    format!("{:#x}UL", named_long(k, i))
                } else {
                    format!("{:?}", named_double(k, i))
                }
            })
            .collect();
        let ints = case.named.iter().filter(|&&int| int).count().min(8);
        let doubles = case.named.iter().filter(|&&int| !int).count().min(8);
        for j in 0..8 - ints {
            args.push(format!("{:#x}UL", 0xF000_0000u64 + (k as u64) * 256 + j as u64));
        }
        for j in 0..8 - doubles {
            args.push(format!("{:?}", 1000.25 + (k * 16 + j) as f64));
        }
        for j in 0..case.stack_longs {
            args.push(format!("{:#x}UL", stack_canary(k, j)));
        }
        writeln!(
            out,
            "__attribute__((noinline)) static void run_{k}(void) {{\n\
             \x20 volatile unsigned long canary[4] = {{ 0xc0ffee{k:x}, 1, 2, 3 }};\n\
             \x20 long keep = opaque({k}) * 3 + 1; double keepd = opaqued({k}.0) * 1.5;\n\
             \x20 long r = t_{k}({});\n\
             \x20 if (canary[0] != 0xc0ffee{k:x} || canary[1] != 1 || canary[2] != 2 || canary[3] != 3)\n\
             \x20   {{ printf(\" CANARY\"); bad = 1; }}\n\
             \x20 if (keep != {k} * 3 + 1 || keepd != {k}.0 * 1.5) {{ printf(\" SAVED\"); bad = 1; }}\n\
             \x20 printf(\" -> %lx\\n\", r);\n}}",
            args.join(", ")
        )
        .unwrap();
    }
    out.push_str("int main(void) {\n");
    for case in cases {
        writeln!(out, "  run_{}();", case.k).unwrap();
    }
    out.push_str("  printf(\"done\\n\");\n  return bad;\n}\n");
    out
}

/// The `musttail:` names `classify` with the stack-argument hook gives
/// the thunk of `case`.
fn musttail_rules(case: &Case) -> Vec<String> {
    let text = format!("{}{}", ll_header(22), thunks(&[case]));
    let (module, bodies) = parse(&text);
    let hook = hod_cg::abi::TargetStackArgs::new(&hod_a64::A64);
    let classifier = Classifier::new(Support::shipped(Arch::Aarch64)).with_stack_args(&hook);
    let mut census = Census::new();
    for body in &bodies {
        classifier.census_body(&module, body, &mut census);
    }
    let mut found: Vec<String> = census
        .entries()
        .iter()
        .map(|entry| entry.construct.clone())
        .filter(|name| name.starts_with("musttail:"))
        .collect();
    found.dedup();
    found
}

/// `llc` of `major`, `-O0 -global-isel=0 -fast-isel=0`, if installed.
fn llc_object(dir: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        assert!(!hod_testutil::tools_required(), "llc {major} is required");
        eprintln!("note: llc {major} not found");
        return None;
    }
    let input = dir.write(&format!("t-llc{major}.ll"), format!("{}{text}", ll_header(major)));
    let output = dir.join(format!("t-llc{major}.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(&input).arg("-o").arg(&output);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
    Some(output)
}

/// The expected start of case `k`'s line: named and extra values.
fn expected_prefix(case: &Case) -> String {
    let mut line = format!("{}:", case.k);
    for (i, &int) in case.named.iter().enumerate() {
        if int {
            write!(line, " {:x}", named_long(case.k, i)).unwrap();
        } else {
            write!(line, " {:x}", named_double(case.k, i).to_bits()).unwrap();
        }
    }
    line.push_str(" |");
    for extra in &case.extras {
        write!(line, " {}", extra.expect).unwrap();
    }
    line.push_str(" |");
    line
}

fn run(toolchain: &Toolchain, dir: &Scratch, tag: &str, inputs: &[&Path]) -> Vec<String> {
    let mut outputs = Vec::new();
    for linker in &toolchain.linkers {
        let exe = dir.join(format!("{tag}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], inputs, &exe);
        assert_eq!(exit_code(&ran), Some(0), "{tag} {}: {}", linker.name, ran.stdout_text());
        outputs.push(ran.stdout_text());
    }
    outputs
}

fn run_seed(seed: u64, count: usize, kinds: &mut [usize; KINDS.len()]) -> (usize, usize, usize) {
    let mut rng = Rng(seed);
    let cases: Vec<Case> = (0..count).map(|k| random_case(&mut rng, k)).collect();
    let mut compiled = Vec::new();
    let mut grown = 0;
    for case in &cases {
        let rules = musttail_rules(case);
        if extras_in_registers(case) {
            assert_eq!(rules, Vec::<String>::new(), "case {}:\n{}", case.k, thunks(&[case]));
            for extra in &case.extras {
                kinds[KINDS.iter().position(|&kind| kind == extra.kind).unwrap()] += 1;
            }
            compiled.push(case);
        } else {
            assert_eq!(
                rules,
                ["musttail:stack-args-grow"],
                "case {}:\n{}",
                case.k,
                thunks(&[case])
            );
            grown += 1;
        }
    }
    // Cases whose caller has named arguments on the stack.
    let stacked = compiled
        .iter()
        .filter(|case| {
            let ints = case.named.iter().filter(|&&int| int).count();
            (ints > 8 || case.named.len() - ints > 8) && !case.extras.is_empty()
        })
        .count();
    let counts = (compiled.len(), grown, stacked);
    let dir = Scratch::new("a64-musttail-varargs");
    let Some(toolchain) = Toolchain::host(&dir) else { return counts };
    let text = thunks(&compiled);
    let driver = toolchain.compile_c(&dir, "driver", &driver(&compiled), &["-w"]);
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let mut reference: Option<String> = None;
    let mut check = |tag: &str, outputs: Vec<String>| {
        for output in outputs {
            match &reference {
                None => {
                    let mut lines = output.lines();
                    for case in &compiled {
                        let line = lines.next().unwrap_or_default();
                        let mut canaries = String::new();
                        for j in 0..case.stack_longs {
                            write!(canaries, " s{:x}", stack_canary(case.k, j)).unwrap();
                        }
                        assert!(
                            line.starts_with(&expected_prefix(case))
                                && line.contains(&format!("{canaries} -> ")),
                            "{tag} case {}: {line}\nwant {}\n{}",
                            case.k,
                            expected_prefix(case),
                            thunks(&[case])
                        );
                    }
                    assert_eq!(lines.next(), Some("done"), "{tag}");
                    reference = Some(output);
                }
                Some(want) => {
                    for (got, want) in output.lines().zip(want.lines()) {
                        assert_eq!(got, want, "{tag}");
                    }
                    assert_eq!(output.lines().count(), want.lines().count(), "{tag}");
                }
            }
        }
    };
    for (tag, stress) in [("hod", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let object = write_object(&dir, &format!("t-{tag}.o"), &emit(&module, &bodies, &options));
        check(tag, run(&toolchain, &dir, tag, &[&driver, &object]));
    }
    for major in LLVM_MAJORS {
        if let Some(object) = llc_object(&dir, major, &text) {
            let tag = format!("llc{major}");
            check(&tag, run(&toolchain, &dir, &tag, &[&driver, &object]));
        }
    }
    counts
}

// `spec/codegen.md` §7.1, `spec/abi-a64.md` §8.5, §9.1.
#[test]
fn forwarding_thunks_with_variable_arguments_run_like_llc() {
    let mut totals = (0, 0, 0);
    let mut kinds = [0; KINDS.len()];
    for seed in [0x115_0001, 0x115_0002, 0x115_0003] {
        let (compiled, grown, stacked) = run_seed(seed, 60, &mut kinds);
        totals.0 += compiled;
        totals.1 += grown;
        totals.2 += stacked;
    }
    eprintln!(
        "{} thunks run ({} with extras and named stack arguments), {} refused as \
         musttail:stack-args-grow",
        totals.0, totals.2, totals.1
    );
    eprintln!("extras run, by class {KINDS:?}: {kinds:?}");
    assert!(totals.0 >= 60 && totals.1 >= 20 && totals.2 >= 5, "{totals:?}");
    // Every class of extra argument ran several times.
    assert!(kinds.iter().all(|&n| n >= 5), "{kinds:?}");
}
