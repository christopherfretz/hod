//! `spec/interp.md` §5.12 and §5.11 against LLVM: generated C programs
//! are compiled by clang 19 and 22 for `x86_64-unknown-linux-gnu` to LLVM
//! text; the interpreter runs that text, and `llc` of the same major
//! compiles it to a program run under `qemu-x86_64` (natively on an
//! x86-64 host). Both report what the program passes to `void
//! sink(long)`, which must be the same sequence.
//!
//! - `variadic_calls_agree_with_llc`: variadic functions walked with
//!   `va_arg` (open-coded by clang), with integer, pointer, `double`,
//!   `long double` and structure arguments of every psABI class, beyond
//!   the register counts so that the overflow area is used, `va_copy`
//!   in the middle of a walk, a `va_list` handed to another function,
//!   and variadic calls made from inside a variadic function.
//! - `x87_intrinsics_agree_with_llc`: `llvm.sqrt`, the rounding family,
//!   `llvm.fma`, `llvm.minnum`/`llvm.maxnum` and `llvm.is.fpclass` on
//!   `x86_fp80`, as clang emits them for `long double` (`-fno-math-errno`).
//!
//! A missing tool skips with a note, or fails under `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod_interp::{Config, Interp, NanPolicy, Program, Stop, Val};
use hod_ll::{BodyParser, ParseOptions, parse_module};
use hod_testutil::{DEFAULT_TIMEOUT, Scratch, run_with, tool, tools_required};

const SINK_C: &str = "#include <stdio.h>\nvoid sink(long v) { printf(\"%ld\\n\", v); }\n";

/// A tool of LLVM `major`.
fn versioned(name: &str, major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if installed.is_file() {
        return Some(installed);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}-{major}")))
        .find(|p| p.is_file());
    if found.is_none() {
        assert!(!tools_required(), "no {name} {major} (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no {name} {major}; skipping its part");
    }
    found
}

/// The x86-64 C compiler (to link) and, off x86-64, the emulator.
fn toolchain() -> Option<(PathBuf, Option<PathBuf>)> {
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        return Some((tool("gcc")?, None));
    }
    Some((tool("x86_64-linux-gnu-gcc")?, Some(tool("qemu-x86_64")?)))
}

struct Oracle {
    scratch: Scratch,
    cc: PathBuf,
    runner: Option<PathBuf>,
    sink_o: PathBuf,
    /// `(major, clang, llc)`.
    llvm: Vec<(u32, PathBuf, PathBuf)>,
}

impl Oracle {
    fn new(label: &str) -> Option<Oracle> {
        let (cc, runner) = toolchain()?;
        let mut llvm = Vec::new();
        for major in [19, 22] {
            if let (Some(clang), Some(llc)) = (versioned("clang", major), versioned("llc", major)) {
                llvm.push((major, clang, llc));
            }
        }
        let scratch = Scratch::new(label);
        let sink_c = scratch.write("sink.c", SINK_C);
        let sink_o = scratch.join("sink.o");
        let built = run_with(
            Command::new(&cc).arg("-c").arg(&sink_c).arg("-o").arg(&sink_o),
            b"",
            DEFAULT_TIMEOUT,
        );
        assert!(built.success(), "{}", built.stderr_text());
        Some(Oracle { scratch, cc, runner, sink_o, llvm })
    }

    /// clang's LLVM text for `source`.
    fn ir(&self, clang: &Path, name: &str, source: &str) -> String {
        let c = self.scratch.write(&format!("{name}.c"), source);
        let ll = self.scratch.join(format!("{name}.ll"));
        let built = run_with(
            Command::new(clang)
                .args(["-O0", "-S", "-emit-llvm", "-fno-math-errno"])
                .arg("--target=x86_64-unknown-linux-gnu")
                .arg(&c)
                .arg("-o")
                .arg(&ll),
            b"",
            DEFAULT_TIMEOUT,
        );
        assert!(built.success(), "clang: {}", built.stderr_text());
        std::fs::read_to_string(&ll).unwrap()
    }

    /// What the program `llc` makes of `ir` sinks.
    fn native(&self, llc: &Path, name: &str, ir: &str) -> Vec<i64> {
        let ll = self.scratch.write(&format!("{name}.native.ll"), ir);
        let obj = self.scratch.join(format!("{name}.o"));
        let exe = self.scratch.join(name);
        let compiled = run_with(
            Command::new(llc)
                .args(["-O0", "-mtriple=x86_64-unknown-linux-gnu", "-relocation-model=pic"])
                .arg("-filetype=obj")
                .arg(&ll)
                .arg("-o")
                .arg(&obj),
            b"",
            DEFAULT_TIMEOUT,
        );
        assert!(compiled.success(), "llc: {}", compiled.stderr_text());
        let linked = run_with(
            Command::new(&self.cc).arg(&obj).arg(&self.sink_o).arg("-o").arg(&exe).arg("-lm"),
            b"",
            DEFAULT_TIMEOUT,
        );
        assert!(linked.success(), "link: {}", linked.stderr_text());
        let ran = match &self.runner {
            Some(qemu) => run_with(
                Command::new(qemu).args(["-L", "/usr/x86_64-linux-gnu"]).arg(&exe),
                b"",
                DEFAULT_TIMEOUT,
            ),
            None => run_with(&mut Command::new(&exe), b"", DEFAULT_TIMEOUT),
        };
        assert!(ran.success(), "{name}: {}", ran.stderr_text());
        ran.stdout_text().lines().map(|line| line.parse().expect("a number")).collect()
    }
}

/// What the interpreter's run of `@main` in `ir` sinks.
fn interpret(ir: &str, nan: NanPolicy) -> Result<Vec<i64>, (Stop, Vec<i64>)> {
    let mut parsed = parse_module(ir.as_bytes(), &ParseOptions::default()).expect("module");
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        if let Some(body) = parser.parse(&mut parsed.module, function).expect("body") {
            bodies.push(body);
        }
    }
    let module = &parsed.module;
    let program = Program::with_bodies(module, &bodies).expect("program");
    program.verify().expect("verifies");
    let main = module.find_symbol(b"main").expect("main");
    let config = Config { nan, ..Config::default() };
    let mut interp = Interp::new(&program).with_config(config);
    hod_interp::host::register(&mut interp);
    let outcome = interp.run(main, &[]);
    let sunk: Vec<i64> = outcome
        .trace
        .iter()
        .filter(|event| &*event.name == "sink")
        .filter_map(|event| match event.args.first() {
            Some(Val::Int { value, .. }) => Some(*value as u64 as i64),
            _ => None,
        })
        .collect();
    match outcome.result {
        Ok(_) => Ok(sunk),
        Err(stop) => Err((stop, sunk)),
    }
}

/// Runs `source` both ways under every LLVM major whose `skip` is false.
fn compare(oracle: &Oracle, name: &str, source: &str, nan: NanPolicy, skip: impl Fn(u32) -> bool) {
    for (major, clang, llc) in &oracle.llvm {
        if skip(*major) {
            continue;
        }
        let tag = format!("{name}-{major}");
        let ir = oracle.ir(clang, &tag, source);
        let want = oracle.native(llc, &tag, &ir);
        match interpret(&ir, nan) {
            Ok(got) => {
                if got != want {
                    let at = got.iter().zip(&want).position(|(a, b)| a != b);
                    let dir = oracle.scratch.path();
                    panic!(
                        "{tag}: the interpreter sinks {} values, llc {}; first difference at {at:?} \
                         (interp {:?}, llc {:?}); files in {}",
                        got.len(),
                        want.len(),
                        at.map(|i| got[i]),
                        at.map(|i| want[i]),
                        dir.display()
                    );
                }
            }
            Err((stop, sunk)) => {
                panic!("{tag}: the interpreter stopped with {stop:?} after {} sinks", sunk.len())
            }
        }
    }
}

// ---- variadic programs ----

/// xorshift64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The C types variable arguments have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Int,
    Long,
    Double,
    LongDouble,
    Ptr,
    /// `{ int, int }`: one general eightbyte (`i64`).
    S8,
    /// `{ int, int, int }`: two general eightbytes (`i64, i32`).
    S12,
    /// `{ double, double }`: two vector eightbytes.
    Sdd,
    /// `{ long, double }`, `{ double, long }`: mixed.
    Sld,
    Sdl,
    /// `{ float, float }`: one vector eightbyte (`<2 x float>`).
    Sff,
    /// `{ long, long, long }`: memory (`byval`).
    S24,
    /// `{ long double }`: memory.
    Sx,
    /// `__int128`.
    I128,
}

const TYS: &[Ty] = &[
    Ty::Int,
    Ty::Long,
    Ty::Double,
    Ty::LongDouble,
    Ty::Ptr,
    Ty::S8,
    Ty::S12,
    Ty::Sdd,
    Ty::Sld,
    Ty::Sdl,
    Ty::Sff,
    Ty::S24,
    Ty::Sx,
    Ty::I128,
];

impl Ty {
    fn c(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Long => "long",
            Ty::Double => "double",
            Ty::LongDouble => "long double",
            Ty::Ptr => "long *",
            Ty::S8 => "S8",
            Ty::S12 => "S12",
            Ty::Sdd => "Sdd",
            Ty::Sld => "Sld",
            Ty::Sdl => "Sdl",
            Ty::Sff => "Sff",
            Ty::S24 => "S24",
            Ty::Sx => "Sx",
            Ty::I128 => "__int128",
        }
    }
}

const PRELUDE: &str = r"#include <stdarg.h>
#include <string.h>
void sink(long);
typedef struct { int a, b; } S8;
typedef struct { int a, b, c; } S12;
typedef struct { double a, b; } Sdd;
typedef struct { long a; double b; } Sld;
typedef struct { double a; long b; } Sdl;
typedef struct { float a, b; } Sff;
typedef struct { long a, b, c; } S24;
typedef struct { long double x; } Sx;
long g[4];
static double D(unsigned long u) { double d; memcpy(&d, &u, 8); return d; }
static float F(unsigned u) { float f; memcpy(&f, &u, 4); return f; }
static long double X(unsigned long lo, unsigned hi) {
  unsigned char b[16] = {0}; memcpy(b, &lo, 8); memcpy(b + 8, &hi, 2);
  long double x; memcpy(&x, b, 10); return x;
}
static void sd(double d) { long u; memcpy(&u, &d, 8); sink(u); }
static void sf(float f) { unsigned u; memcpy(&u, &f, 4); sink(u); }
static void sx(long double x) {
  unsigned char b[16] = {0}; memcpy(b, &x, 10);
  long lo, hi = 0; memcpy(&lo, b, 8); memcpy(&hi, b + 8, 2); sink(lo); sink(hi);
}
static void sp(long *p) { sink(p == &g[0] ? 0 : p == &g[1] ? 1 : p == &g[2] ? 2 : p == &g[3] ? 3 : 99); }
";

/// C statements that sink `v` of type `ty`.
fn sink_of(ty: Ty, v: &str) -> String {
    match ty {
        Ty::Int | Ty::Long => format!("sink({v});"),
        Ty::Double => format!("sd({v});"),
        Ty::LongDouble => format!("sx({v});"),
        Ty::Ptr => format!("sp({v});"),
        Ty::S8 => format!("sink({v}.a); sink({v}.b);"),
        Ty::Sdd => format!("sd({v}.a); sd({v}.b);"),
        Ty::Sld => format!("sink({v}.a); sd({v}.b);"),
        Ty::Sdl => format!("sd({v}.a); sink({v}.b);"),
        Ty::Sff => format!("sf({v}.a); sf({v}.b);"),
        Ty::S12 | Ty::S24 => format!("sink({v}.a); sink({v}.b); sink({v}.c);"),
        Ty::Sx => format!("sx({v}.x);"),
        Ty::I128 => format!("sink((long){v}); sink((long)({v} >> 64));"),
    }
}

fn int_value(rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => "0".into(),
        1 => "-1".into(),
        2 => "(-2147483647 - 1)".into(),
        3 => "2147483647".into(),
        _ => format!("{}", rng.next() as i32),
    }
}

fn long_value(rng: &mut Rng) -> String {
    match rng.below(5) {
        0 => "(-9223372036854775807L - 1)".into(),
        1 => "-1L".into(),
        _ => format!("{}L", (rng.next() >> 1) as i64 * if rng.below(2) == 0 { 1 } else { -1 }),
    }
}

fn double_value(rng: &mut Rng) -> String {
    let bits: u64 = match rng.below(8) {
        0 => 0,
        1 => 1 << 63,
        2 => 0x7ff0_0000_0000_0000,
        3 => 0x7ff8_0000_0000_0001,
        4 => 0x0000_0000_0000_0001,
        _ => rng.next() & !(0x7ff << 52) | (u64::from(rng.below(0x7fe) as u16) << 52),
    };
    format!("D(0x{bits:x}UL)")
}

fn float_value(rng: &mut Rng) -> String {
    let bits = (rng.next() as u32 & 0x807f_ffff) | ((1 + rng.below(0xfe) as u32) << 23);
    format!("F(0x{bits:x}U)")
}

fn x87_value(rng: &mut Rng) -> String {
    let (lo, hi): (u64, u32) = match rng.below(6) {
        0 => (0, 0x8000),
        1 => (1 << 63, 0x7fff),
        2 => (0xc000_0000_0000_0001, 0xffff),
        3 => (rng.next() >> 1, rng.below(2) as u32 * 0x8000),
        _ => (rng.next() | 1 << 63, (1 + rng.below(0x7ffe) as u32) | ((rng.below(2) as u32) << 15)),
    };
    format!("X(0x{lo:x}UL, 0x{hi:x}U)")
}

/// A C expression of type `ty`.
fn value(rng: &mut Rng, ty: Ty) -> String {
    match ty {
        Ty::Int => int_value(rng),
        Ty::Long => long_value(rng),
        Ty::Double => double_value(rng),
        Ty::LongDouble => x87_value(rng),
        Ty::Ptr => format!("&g[{}]", rng.below(4)),
        Ty::S8 => format!("(S8){{{}, {}}}", int_value(rng), int_value(rng)),
        Ty::S12 => format!("(S12){{{}, {}, {}}}", int_value(rng), int_value(rng), int_value(rng)),
        Ty::Sdd => format!("(Sdd){{{}, {}}}", double_value(rng), double_value(rng)),
        Ty::Sld => format!("(Sld){{{}, {}}}", long_value(rng), double_value(rng)),
        Ty::Sdl => format!("(Sdl){{{}, {}}}", double_value(rng), long_value(rng)),
        Ty::Sff => format!("(Sff){{{}, {}}}", float_value(rng), float_value(rng)),
        Ty::S24 => {
            format!("(S24){{{}, {}, {}}}", long_value(rng), long_value(rng), long_value(rng))
        }
        Ty::Sx => format!("(Sx){{{}}}", x87_value(rng)),
        Ty::I128 => {
            format!("(((__int128){}) << 64 | (unsigned long){})", long_value(rng), long_value(rng))
        }
    }
}

/// One generated variadic function.
struct Callee {
    named: Vec<Ty>,
    varargs: Vec<Ty>,
    /// Where the walk takes a `va_copy` and walks the rest of the copy.
    copy_at: Option<usize>,
    /// Where the walk hands the list to `rest_N` and stops.
    hand_off_at: Option<usize>,
    /// Where the walk calls the next callee.
    nested_at: Option<usize>,
}

/// A program of `count` variadic functions and a `main` that calls each.
fn variadic_program(rng: &mut Rng, count: usize, with_i128: bool) -> String {
    let tys: Vec<Ty> = TYS.iter().copied().filter(|&t| with_i128 || t != Ty::I128).collect();
    let named_tys = [Ty::Int, Ty::Long, Ty::Double, Ty::LongDouble, Ty::S24, Ty::Sdd, Ty::Ptr];
    let callees: Vec<Callee> = (0..count)
        .map(|_| {
            let named = (0..=rng.below(4))
                .map(|_| named_tys[rng.below(named_tys.len() as u64) as usize])
                .collect();
            let n = rng.below(20) as usize;
            let varargs: Vec<Ty> =
                (0..n).map(|_| tys[rng.below(tys.len() as u64) as usize]).collect();
            let pick =
                |rng: &mut Rng| (n > 0 && rng.below(3) == 0).then(|| rng.below(n as u64) as usize);
            Callee {
                named,
                copy_at: pick(rng),
                hand_off_at: pick(rng),
                nested_at: pick(rng),
                varargs,
            }
        })
        .collect();
    let mut out = String::from(PRELUDE);
    // Prototypes, so that nested calls can go forward.
    for (k, callee) in callees.iter().enumerate() {
        let params: Vec<String> = callee.named.iter().map(|t| t.c().to_owned()).collect();
        let _ = writeln!(out, "static void v{k}({}, ...);", params.join(", "));
    }
    for (k, callee) in callees.iter().enumerate() {
        let params: Vec<String> =
            callee.named.iter().enumerate().map(|(i, t)| format!("{} n{i}", t.c())).collect();
        // The walk of the rest from `from`, over list `l`.
        let walk = |from: usize, l: &str, tag: usize| -> String {
            let mut s = format!("  sink({tag});\n");
            for &ty in callee.varargs.iter().skip(from) {
                let _ = writeln!(
                    s,
                    "  {{ {} x = va_arg({l}, {}); {} }}",
                    ty.c(),
                    ty.c(),
                    sink_of(ty, "x")
                );
            }
            s
        };
        // `rest_k` walks a list handed to it.
        if let Some(at) = callee.hand_off_at {
            let _ =
                writeln!(out, "static void rest{k}(va_list l) {{\n{}}}", walk(at, "l", 2000 + k));
        }
        let _ = writeln!(out, "static void v{k}({}, ...) {{", params.join(", "));
        for (i, &ty) in callee.named.iter().enumerate() {
            let _ = writeln!(out, "  {}", sink_of(ty, &format!("n{i}")));
        }
        let _ = writeln!(out, "  va_list ap;\n  va_start(ap, n{});", callee.named.len() - 1);
        for (i, &ty) in callee.varargs.iter().enumerate() {
            if callee.copy_at == Some(i) {
                let _ = writeln!(
                    out,
                    "  {{ va_list aq; va_copy(aq, ap);\n{}  va_end(aq); }}",
                    walk(i, "aq", 1000 + k)
                );
            }
            if callee.nested_at == Some(i) && k + 1 < callees.len() {
                let _ = writeln!(out, "  {}", call_of(rng, k + 1, &callees[k + 1]));
            }
            if callee.hand_off_at == Some(i) {
                let _ = writeln!(out, "  rest{k}(ap);\n  va_end(ap);\n  return;");
                break;
            }
            let _ = writeln!(
                out,
                "  {{ {} x = va_arg(ap, {}); {} }}",
                ty.c(),
                ty.c(),
                sink_of(ty, "x")
            );
        }
        out.push_str("  va_end(ap);\n}\n");
    }
    out.push_str("int main(void) {\n");
    for (k, callee) in callees.iter().enumerate() {
        let _ = writeln!(out, "  sink({});\n  {}", 3000 + k, call_of(rng, k, callee));
    }
    out.push_str("  return 0;\n}\n");
    out
}

fn call_of(rng: &mut Rng, k: usize, callee: &Callee) -> String {
    let args: Vec<String> =
        callee.named.iter().chain(&callee.varargs).map(|&ty| value(rng, ty)).collect();
    format!("v{k}({});", args.join(", "))
}

/// §5.12: generated variadic programs sink the same values in the
/// interpreter and compiled by `llc`, under LLVM 19 and 22. `__int128`
/// arguments only run against LLVM 22 (`spec/abi-x64.md` §4.2: LLVM 19
/// spends the registers an `i128` on the stack leaves).
#[test]
fn variadic_calls_agree_with_llc() {
    let Some(oracle) = Oracle::new("interp-x64-variadic") else { return };
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for index in 0..12 {
        let with_i128 = index % 3 == 2;
        let source = variadic_program(&mut rng, 6, with_i128);
        compare(&oracle, &format!("va{index}"), &source, NanPolicy::Trap, |major| {
            with_i128 && major < 22
        });
    }
}

// ---- x87 intrinsics ----

/// Operands for the `long double` intrinsics: specials, edges of the
/// rounding (halves, the largest non-integers), denormals, random.
fn x87_operands(rng: &mut Rng) -> Vec<(u64, u32)> {
    let mut out = vec![
        (0, 0),
        (0, 0x8000),
        (1 << 63, 0x7fff),
        (1 << 63, 0xffff),
        (0xc000_0000_0000_0000, 0x7fff),
        (1 << 63, 0x3ffe),               // 0.5
        (0xc000_0000_0000_0000, 0x3fff), // 1.5
        (0xa000_0000_0000_0000, 0x4000), // 2.5
        (0xa000_0000_0000_0000, 0xc000), // -2.5
        (u64::MAX, 0x403d),              // 2^63 - 0.5
        (u64::MAX, 0x403e),              // 2^64 - 1
        (u64::MAX, 0x3ffe),              // just below 1
        (1, 0),                          // the smallest denormal
        (0x4000_0000_0000_0000, 0x8000),
        (1 << 63, 0x0001),  // the smallest normal
        (u64::MAX, 0x7ffe), // the largest finite
    ];
    for _ in 0..40 {
        let hi = match rng.below(4) {
            0 => rng.below(0x7fff) as u32,
            _ => 0x3fff - 70 + rng.below(140) as u32,
        } | (rng.below(2) as u32) << 15;
        let lo = if hi.trailing_zeros() >= 15 { rng.next() >> 1 } else { rng.next() | 1 << 63 };
        out.push((lo, hi));
    }
    out
}

/// §5.11: the `long double` intrinsics clang emits agree with `llc`
/// bit for bit. A NaN result is compared as "a NaN" (the interpreter
/// runs with `NanPolicy::Host`, whose NaNs are not covered, §5.1), and
/// `fmaxl`/`fminl` of zeros of opposite signs is left out (it traps
/// `zero-sign`).
#[test]
fn x87_intrinsics_agree_with_llc() {
    let Some(oracle) = Oracle::new("interp-x64-x87") else { return };
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let ops = x87_operands(&mut rng);
    let mut source = String::from(PRELUDE);
    source.push_str(
        "static void sr(long double x) { if (x != x) { sink(-7); sink(-7); } else sx(x); }\n\
         int main(void) {\n",
    );
    let mut lines = String::new();
    for (i, &(lo, hi)) in ops.iter().enumerate() {
        let a = format!("X(0x{lo:x}UL, 0x{hi:x}U)");
        let (blo, bhi) = ops[(i * 7 + 3) % ops.len()];
        let b = format!("X(0x{blo:x}UL, 0x{bhi:x}U)");
        let (clo, chi) = ops[(i * 13 + 5) % ops.len()];
        let c = format!("X(0x{clo:x}UL, 0x{chi:x}U)");
        let _ = writeln!(lines, "  {{ long double a = {a}, b = {b}, c = {c};");
        for f in ["sqrtl", "floorl", "ceill", "truncl", "roundl", "rintl", "nearbyintl"] {
            let _ = writeln!(lines, "    sr(__builtin_{f}(a));");
        }
        lines.push_str("    sr(__builtin_roundevenl(a));\n    sr(__builtin_fmal(a, b, c));\n");
        let zero = |lo: u64, hi: u32| lo == 0 && hi.trailing_zeros() >= 15;
        if !(zero(lo, hi) && zero(blo, bhi) && hi != bhi) {
            lines.push_str("    sr(__builtin_fmaxl(a, b));\n    sr(__builtin_fminl(a, b));\n");
        }
        for bit in 0..10 {
            let _ = writeln!(lines, "    sink(__builtin_isfpclass(a, {}));", 1 << bit);
        }
        lines.push_str("  }\n");
    }
    source.push_str(&lines);
    source.push_str("  return 0;\n}\n");
    compare(&oracle, "x87", &source, NanPolicy::Host, |_| false);
}
