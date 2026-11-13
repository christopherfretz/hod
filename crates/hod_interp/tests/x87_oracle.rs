//! `spec/interp.md` §5.11: the `x86_fp80` arithmetic of `hod_interp::x87`
//! against the x87 itself: a C program compiled for x86-64 computes the
//! same operations on `long double` (run under `qemu-x86_64` when the
//! host is not x86-64), and every result must have the same bits; a NaN
//! result only has to be a NaN on both sides (the interpreter traps it,
//! §5.1). A missing tool skips the test with a note, or fails under
//! `HOD_REQUIRE_TOOLS=1`.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use hod_interp::x87::{self, Format, Op, Rounding};
use hod_testutil::{DEFAULT_TIMEOUT, Scratch, run_with, tool};

const PROGRAM: &str = r#"#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <math.h>

static long double get(const char *hex) {
  /* 20 hex digits, most significant first. */
  unsigned char b[16] = {0};
  for (int i = 0; i < 10; i++) {
    unsigned v; sscanf(hex + 2 * (9 - i), "%2x", &v); b[i] = (unsigned char)v;
  }
  long double x; memcpy(&x, b, sizeof x); return x;
}
static void put(long double x) {
  unsigned char b[16] = {0}; memcpy(b, &x, 10);
  for (int i = 9; i >= 0; i--) printf("%02x", b[i]);
  printf("\n");
}
int main(void) {
  char op[16], a[32], c[32];
  while (scanf("%15s %31s %31s", op, a, c) == 3) {
    long double x = get(a), y = get(c);
    if (!strcmp(op, "add")) put(x + y);
    else if (!strcmp(op, "sub")) put(x - y);
    else if (!strcmp(op, "mul")) put(x * y);
    else if (!strcmp(op, "div")) put(x / y);
    else if (!strcmp(op, "rem")) put(fmodl(x, y));
    else if (!strcmp(op, "cmp")) printf("%d%d%d\n", x < y, x == y, x > y);
    else if (!strcmp(op, "tod")) { double d = (double)x; uint64_t u; memcpy(&u, &d, 8); printf("%016llx\n", (unsigned long long)u); }
    else if (!strcmp(op, "tof")) { float f = (float)x; uint32_t u; memcpy(&u, &f, 4); printf("%08x\n", u); }
    else if (!strcmp(op, "toi")) printf("%016llx\n", (unsigned long long)(int64_t)x);
    else if (!strcmp(op, "fromi")) { int64_t i; sscanf(c, "%llx", (unsigned long long *)&i); put((long double)i); }
    else if (!strcmp(op, "fromd")) { uint64_t u; double d; sscanf(c, "%llx", (unsigned long long *)&u); memcpy(&d, &u, 8); put((long double)d); }
    else if (!strcmp(op, "sqrt")) put(sqrtl(x));
    else if (!strcmp(op, "floor")) put(floorl(x));
    else if (!strcmp(op, "ceil")) put(ceill(x));
    else if (!strcmp(op, "trunc")) put(truncl(x));
    else if (!strcmp(op, "round")) put(roundl(x));
    else if (!strcmp(op, "roundeven")) put(roundevenl(x));
    else if (!strcmp(op, "rint")) put(rintl(x));
    else if (!strcmp(op, "nearbyint")) put(nearbyintl(x));
    else if (!strcmp(op, "fmax")) put(fmaxl(x, y));
    else if (!strcmp(op, "fmin")) put(fminl(x, y));
    else if (!strcmp(op, "fma")) { char d[32]; if (scanf("%31s", d) != 1) return 1; put(fmal(x, y, get(d))); }
    else printf("?\n");
  }
  return 0;
}
"#;

/// A C compiler for x86-64 and, off x86-64, the emulator to run with.
fn toolchain() -> Option<(PathBuf, Option<PathBuf>)> {
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        return Some((tool("gcc")?, None));
    }
    Some((tool("x86_64-linux-gnu-gcc")?, Some(tool("qemu-x86_64")?)))
}

/// xorshift64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// An operand: mostly normals near 1, some anywhere, some special.
    fn operand(&mut self) -> u128 {
        let sign = u128::from(self.next() & 1) << 79;
        let mant = self.next() | (1 << 63);
        let pick = self.next() % 16;
        let exp: u64 = match pick {
            0 => 0,                         // denormal or zero
            1 => 0x7fff,                    // infinity or NaN
            2 => 1 + self.next() % 70,      // near the denormals
            3 => 0x7ffe - self.next() % 70, // near overflow
            4 => self.next() % 0x7fff,
            _ => 0x3fff - 40 + self.next() % 80,
        };
        let mant = match pick {
            0 if self.next().is_multiple_of(3) => 0,
            0 => mant >> 1 >> (self.next() % 63),
            1 if self.next().is_multiple_of(2) => 1 << 63,
            // Short significands make exact sums and remainders.
            _ if self.next().is_multiple_of(4) => mant & !((1 << (self.next() % 60)) - 1),
            _ => mant,
        };
        sign | (u128::from(exp) << 64) | u128::from(mant)
    }
}

impl Rng {
    /// An encoding the 387 refuses: an unnormal, a pseudo-NaN or a
    /// pseudo-infinity.
    fn invalid(&mut self) -> u128 {
        let sign = u128::from(self.next() & 1) << 79;
        let mant = u128::from(self.next() >> 1);
        let exp = match self.next() % 3 {
            0 => 0x7fff,
            _ => 1 + self.next() % 0x7ffe,
        };
        sign | (u128::from(exp) << 64) | mant
    }
}

fn hex80(x: u128) -> String {
    format!("{x:020x}")
}

#[test]
#[allow(
    clippy::many_single_char_names,
    reason = "operands a, b and results d, f, i, r as in the C program"
)]
fn x87_arithmetic_agrees_with_the_hardware() {
    let Some((cc, runner)) = toolchain() else { return };
    let scratch = Scratch::new("interp-x87");
    let source = scratch.join("x87.c");
    std::fs::write(&source, PROGRAM).unwrap();
    let program = scratch.join("x87");
    let built = run_with(
        Command::new(&cc).arg("-O0").arg(&source).arg("-o").arg(&program).arg("-lm"),
        b"",
        DEFAULT_TIMEOUT,
    );
    assert!(built.success(), "{}", built.stderr_text());

    // Each case: its input line and what hod's x87 says.
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut input = String::new();
    let mut want: Vec<(String, String)> = Vec::new();
    let nan = |bits: u128| x87::is_nan(Format::X87, bits);
    for _ in 0..4000 {
        let (a, b) = (rng.operand(), rng.operand());
        for (name, op) in [
            ("add", Op::Add),
            ("sub", Op::Sub),
            ("mul", Op::Mul),
            ("div", Op::Div),
            ("rem", Op::Rem),
        ] {
            let r = x87::binary(op, a, b);
            let line = format!("{name} {} {}", hex80(a), hex80(b));
            want.push((line.clone(), if nan(r) { "nan".into() } else { hex80(r) }));
            input.push_str(&line);
            input.push('\n');
        }
        let order = x87::compare(Format::X87, a, b);
        let cmp = match order {
            None => "000",
            Some(Ordering::Less) => "100",
            Some(Ordering::Equal) => "010",
            Some(Ordering::Greater) => "001",
        };
        let line = format!("cmp {} {}", hex80(a), hex80(b));
        let _ = writeln!(input, "{line}");
        want.push((line, cmp.into()));
        let d = x87::convert(Format::X87, Format::DOUBLE, a);
        let line = format!("tod {} 0", hex80(a));
        let _ = writeln!(input, "{line}");
        want.push((line, if nan(a) { "nan-d".into() } else { format!("{d:016x}") }));
        let f = x87::convert(Format::X87, Format::FLOAT, a);
        let line = format!("tof {} 0", hex80(a));
        let _ = writeln!(input, "{line}");
        want.push((line, if nan(a) { "nan-f".into() } else { format!("{f:08x}") }));
        if let Ok(i) = x87::to_int(Format::X87, a, 64, true) {
            let line = format!("toi {} 0", hex80(a));
            let _ = writeln!(input, "{line}");
            want.push((line, format!("{i:016x}")));
        }
        // The intrinsics of §5.11 through glibc's `long double` functions
        // (`llc` calls them, or emits the instruction they use). Invalid
        // encodings only for those the 387 itself computes: glibc's
        // `roundl` and `roundevenl` look at the bits as a number.
        let invalid = rng.invalid();
        for (name, mode) in [
            ("sqrt", None),
            ("floor", Some(Rounding::Floor)),
            ("ceil", Some(Rounding::Ceil)),
            ("trunc", Some(Rounding::Trunc)),
            ("round", Some(Rounding::NearestAway)),
            ("roundeven", Some(Rounding::NearestEven)),
            ("rint", Some(Rounding::NearestEven)),
            ("nearbyint", Some(Rounding::NearestEven)),
        ] {
            let bitwise = matches!(name, "round" | "roundeven");
            for x in [a, invalid] {
                if x == invalid && bitwise {
                    continue;
                }
                let r = match mode {
                    None => x87::sqrt(x),
                    Some(mode) => x87::round_integral(x, mode),
                };
                let line = format!("{name} {} 0", hex80(x));
                let _ = writeln!(input, "{line}");
                want.push((line, if nan(r) { "nan".into() } else { hex80(r) }));
            }
        }
        let c = rng.operand();
        let r = x87::fma(a, b, c);
        let line = format!("fma {} {} {}", hex80(a), hex80(b), hex80(c));
        let _ = writeln!(input, "{line}");
        want.push((line, if nan(r) { "nan".into() } else { hex80(r) }));
        // A product near `c`, so that the sum cancels.
        let near = x87::binary(Op::Mul, a, b) ^ (1 << 79);
        let near = near ^ u128::from(rng.next() % 4);
        let r = x87::fma(a, b, near);
        let line = format!("fma {} {} {}", hex80(a), hex80(b), hex80(near));
        let _ = writeln!(input, "{line}");
        want.push((line, if nan(r) { "nan".into() } else { hex80(r) }));
        for (name, max) in [("fmax", true), ("fmin", false)] {
            // Without `host_nan`: what traps is not compared.
            if let Ok(r) = x87::min_max(Format::X87, a, b, max, false) {
                let line = format!("{name} {} {}", hex80(a), hex80(b));
                let _ = writeln!(input, "{line}");
                want.push((line, hex80(r)));
            }
        }
        let i = rng.next() >> (rng.next() % 64);
        let i = if rng.next().is_multiple_of(2) { i.wrapping_neg() } else { i };
        let line = format!("fromi 0 {i:x}");
        let _ = writeln!(input, "{line}");
        want.push((line, hex80(x87::from_int(Format::X87, u128::from(i), 64, true))));
        let dbits = rng.next();
        if !f64::from_bits(dbits).is_nan() {
            let line = format!("fromd 0 {dbits:x}");
            let _ = writeln!(input, "{line}");
            let wide = x87::convert(Format::DOUBLE, Format::X87, u128::from(dbits));
            want.push((line, hex80(wide)));
        }
    }

    let ran = match &runner {
        Some(qemu) => run_with(
            Command::new(qemu).args(["-L", "/usr/x86_64-linux-gnu"]).arg(&program),
            input.as_bytes(),
            DEFAULT_TIMEOUT,
        ),
        None => run_with(&mut Command::new(&program), input.as_bytes(), DEFAULT_TIMEOUT),
    };
    assert!(ran.success(), "{}", ran.stderr_text());
    let got: Vec<String> = ran.stdout_text().lines().map(str::to_owned).collect();
    assert_eq!(got.len(), want.len(), "one line per case");
    let mut wrong = Vec::new();
    for ((line, want), got) in want.iter().zip(&got) {
        let agrees = match want.as_str() {
            "nan" => u128::from_str_radix(got, 16).is_ok_and(|bits| x87::is_nan(Format::X87, bits)),
            "nan-d" => u64::from_str_radix(got, 16).is_ok_and(|b| f64::from_bits(b).is_nan()),
            "nan-f" => u32::from_str_radix(got, 16).is_ok_and(|b| f32::from_bits(b).is_nan()),
            _ => got == want,
        };
        if !agrees {
            wrong.push(format!("{line}: hod {want}, x87 {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} differ:\n{}",
        wrong.len(),
        want.len(),
        wrong[..wrong.len().min(20)].join("\n")
    );
}
