//! Frame size against `llc -O0`, the tracked number of WP-198
//! (`spec/codegen.md` §5.1 "Value slots"), shared by `hod_x64` and
//! `hod_a64`: the seeds of `data/frame_size.ll`, compiled by hod and by
//! `llc -O0`, and each function's stack bytes read from its prologue by
//! `llvm-objdump`. The table is printed; the check fails when hod's total
//! exceeds `max_total` times `llc`'s, or one function's `max_function`
//! times.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod_ir::{Body, Module};
use hod_testutil::{Scratch, run_with, tool};

/// The seeds, without target lines.
pub const SEEDS: &str = include_str!("../data/frame_size.ll");

/// One target's run of the seeds.
pub struct Target<'a> {
    /// `target datalayout` and `target triple` lines.
    pub header: String,
    /// `llc`'s arguments besides `-O0`, the output and the input.
    pub llc_args: &'a [&'a str],
    /// hod's object of a module.
    pub compile: &'a dyn Fn(&Module, &[Body]) -> Vec<u8>,
    /// The bounds.
    pub max_total: f64,
    pub max_function: f64,
}

/// A module and its bodies from `.ll` text.
pub fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        bodies.push(parsed.body.expect("a body hod reads").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// A hexadecimal immediate (`$0x40`, `#0x40`).
fn hex(text: &str) -> Option<u64> {
    let digits = text.trim().trim_start_matches(['$', '#']).strip_prefix("0x")?;
    u64::from_str_radix(digits, 16).ok()
}

/// The stack bytes one prologue instruction allocates, `Some(0)` for one
/// that allocates none, `None` past the prologue. x86-64: `pushq` and
/// `subq $n, %rsp`; AArch64: `sub sp, sp, #n` (`lsl #12` too) and the
/// pre-indexed `stp`/`str` to `[sp, #-n]!`.
fn prologue_bytes(mnemonic: &str, operands: &str) -> Option<u64> {
    let operands = operands.split('<').next().unwrap_or(operands).trim();
    match mnemonic {
        "pushq" => Some(8),
        "subq" => operands.strip_suffix(", %rsp").and_then(hex),
        "endbr64" | "paciasp" | "bti" => Some(0),
        "movq" if operands == "%rsp, %rbp" => Some(0),
        "mov" if operands == "x29, sp" => Some(0),
        "add" if operands.starts_with("x29, sp,") => Some(0),
        "sub" => {
            let rest = operands.strip_prefix("sp, sp, ")?;
            let (imm, shifted) = match rest.strip_suffix(", lsl #12") {
                Some(imm) => (imm, true),
                None => (rest, false),
            };
            hex(imm).map(|n| if shifted { n << 12 } else { n })
        }
        "stp" | "str" | "stur" => {
            let (_, address) = operands.split_once("[sp")?;
            match address.strip_suffix("]!") {
                Some(pre) => pre.trim_start_matches(", ").strip_prefix("#-").and_then(hex),
                None => address.ends_with(']').then_some(0),
            }
        }
        _ => None,
    }
}

/// Each function's stack bytes, from `llvm-objdump -d --no-show-raw-insn`
/// text.
fn frames(disassembly: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, u64, bool)> = None;
    for line in disassembly.lines() {
        if let Some(name) = line.strip_suffix(">:").and_then(|l| l.split_once(" <")).map(|(_, n)| n)
        {
            if let Some((name, bytes, _)) = current.take() {
                out.insert(name, bytes);
            }
            current = Some((name.to_owned(), 0, true));
            continue;
        }
        let Some((_, bytes, in_prologue)) = current.as_mut() else { continue };
        let mut fields = line.split('\t');
        let (Some(_), Some(mnemonic)) = (fields.next(), fields.next()) else { continue };
        if !*in_prologue {
            continue;
        }
        let operands = fields.next().unwrap_or("");
        match prologue_bytes(mnemonic.trim(), operands) {
            Some(n) => *bytes += n,
            None => *in_prologue = false,
        }
    }
    if let Some((name, bytes, _)) = current {
        out.insert(name, bytes);
    }
    out
}

fn disassemble(objdump: &Path, object: &Path) -> BTreeMap<String, u64> {
    let outcome = run_with(
        Command::new(objdump).args(["-d", "--no-show-raw-insn"]).arg(object),
        &[],
        Duration::from_mins(2),
    );
    assert!(outcome.success(), "llvm-objdump failed:\n{}", outcome.stderr_text());
    frames(&outcome.stdout_text())
}

/// A byte count as a real number.
fn real(bytes: u64) -> f64 {
    f64::from(u32::try_from(bytes).expect("a frame under 4 GiB"))
}

/// Compiles the seeds both ways, prints the table and checks the bounds.
pub fn check(target: &Target<'_>) {
    let (Some(llc), Some(objdump)) = (tool("llc"), tool("llvm-objdump")) else {
        return;
    };
    let dir = Scratch::new("frame_size");
    let text = format!("{}{SEEDS}", target.header);
    let input = dir.write("seeds.ll", &text);
    let (module, bodies) = parse(&text);
    let hod_object = dir.write("hod.o", (target.compile)(&module, &bodies));
    let llc_object = dir.join("llc.o");
    let outcome = run_with(
        Command::new(llc).arg("-O0").args(target.llc_args).arg("-o").arg(&llc_object).arg(&input),
        &[],
        Duration::from_mins(2),
    );
    assert!(outcome.success(), "llc failed:\n{}", outcome.stderr_text());
    let hod = disassemble(&objdump, &hod_object);
    let llc = disassemble(&objdump, &llc_object);
    assert!(!llc.is_empty(), "no function in llc's object");

    let mut table = String::from("function             hod    llc\n");
    let mut over = Vec::new();
    let (mut hod_total, mut llc_total) = (0, 0);
    for (name, &theirs) in &llc {
        let ours = *hod.get(name).unwrap_or_else(|| panic!("hod's object has no `{name}`"));
        writeln!(table, "{name:<18} {ours:>6} {theirs:>6}").unwrap();
        hod_total += ours;
        llc_total += theirs;
        if real(ours) > real(theirs) * target.max_function {
            over.push(name.clone());
        }
    }
    let ratio = real(hod_total) / real(llc_total.max(1));
    writeln!(table, "{:<18} {hod_total:>6} {llc_total:>6}  ratio {ratio:.3}", "total").unwrap();
    println!("{table}");
    assert!(over.is_empty(), "over {} times llc's frame: {over:?}\n{table}", target.max_function);
    assert!(
        ratio <= target.max_total,
        "hod's frames are {ratio:.3} times llc's, at most {}\n{table}",
        target.max_total
    );
}

#[test]
fn prologues_are_read() {
    let x64 = "0000000000000000 <f>:\n       0:\tpushq\t%rbp\n       1:\tmovq\t%rsp, %rbp\n       4:\tpushq\t%rbx\n       5:\tsubq\t$0x18, %rsp\n       9:\tmovq\t%rdi, -0x10(%rbp)\n       d:\tsubq\t$0x10, %rsp\n";
    assert_eq!(frames(x64), BTreeMap::from([("f".to_owned(), 40)]));
    let a64 = "0000000000000000 <g>:\n       0:\tsub\tsp, sp, #0x1, lsl #12\n       4:\tsub\tsp, sp, #0x30\n       8:\tstp\tx29, x30, [sp, #-0x10]!\n       c:\tmov\tx29, sp\n      10:\tstr\tx19, [sp, #0x18]\n      14:\tbl\t0x0 <h>\n0000000000000020 <h>:\n      20:\tret\n";
    assert_eq!(
        frames(a64),
        BTreeMap::from([("g".to_owned(), 4096 + 0x30 + 0x10), ("h".to_owned(), 0)])
    );
}

/// A program in which a block slot would be wrong were it shared after a
/// `returns_twice` call (§5.1 "Value slots" 3): `%a` and `%b` are used only
/// in their block, after `_setjmp`; the `clobber` block computes values of
/// the same size, used only there, and `longjmp`s back. The function must
/// be compiled without block slots, so `%c` sees the first `%a` and `%b`.
/// `main` exits 0 when it does.
pub const SETJMP_PROGRAM: &str = r"
declare i32 @_setjmp(ptr) returns_twice
declare void @longjmp(ptr, i32) noreturn

@buf = internal global [512 x i8] zeroinitializer, align 16
@counter = internal global i32 0

define internal i32 @run(i32 %seed) noinline {
entry:
  %a = add i32 %seed, 11
  %b = mul i32 %a, %a
  %r = call i32 @_setjmp(ptr @buf) returns_twice
  %c = add i32 %a, %b
  %again = icmp ne i32 %r, 0
  br i1 %again, label %done, label %clobber

clobber:
  %x = load volatile i32, ptr @counter
  %y = add i32 %x, 1000
  %z = mul i32 %y, %y
  %w = add i32 %z, %y
  store volatile i32 %w, ptr @counter
  call void @longjmp(ptr @buf, i32 1)
  unreachable

done:
  ret i32 %c
}

define i32 @main() {
  %v = call i32 @run(i32 5)
  %ok = icmp eq i32 %v, 272
  %e = select i1 %ok, i32 0, i32 1
  ret i32 %e
}
";
