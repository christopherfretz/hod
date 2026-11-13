//! The hardening flags together (WP-194, `spec/codegen.md` §5.6.30):
//! stack protectors (§5.6.24), stack probes (§5.6.27), indirect-branch
//! tracking on x86-64 (§5.6.25), branch protection on AArch64 (§5.6.26)
//! and the preserving conventions on x86-64 (§5.6.23) were each tested
//! alone; distributions turn them all on at once, and they share the
//! prologue and the exit sequences.
//!
//! - `every_combination_runs_as_llc_s`: per target, one function per
//!   combination of the flags and per shape (frames from 24 bytes to
//!   1 MiB, `alloca`, VLAs, stack arguments, `byval`, exceptions thrown
//!   through the frame and caught in it, `invoke` with stack arguments,
//!   direct and indirect `musttail`, `nest` (called and tail-called),
//!   varargs, `setjmp`/`longjmp`; on x86-64 `preserve_mostcc` and
//!   `preserve_allcc` called through a checker that fills every register
//!   they keep and compares it after the call). Built by hod (plain and
//!   with every stress option) and by `llc` 19 and 22, linked with bfd,
//!   lld and mold, run (x86-64 under qemu): every output is the same.
//!   On AArch64 also a program with the branch-protection note on every
//!   object, so that the kernel enforces BTI (hod and `llc` 22: `llc` 19
//!   puts no `bti j` on a landing pad).
//! - `overflows_are_caught_with_every_flag`: a 1-byte overflow aborts
//!   ("stack smashing detected") with probes and branch protection on.
//! - `probe_faults_unwind_through_protected_signed_frames`: WP-186's
//!   guard-page run with the protector and the signing on: the fault is
//!   within a page below the guard's top and `backtrace()` from the
//!   signal handler (inside the probe loop) reaches the caller.
//! - `bti_is_enforced_at_every_landing_pad`: AArch64 with BTI enforced
//!   by the kernel.

#[path = "../../hod_obj/tests/common/mod.rs"]
mod common;

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use hod_testutil::{LLVM_MAJORS, Scratch, Target, run_with, tool};

fn triple(target: Target) -> &'static str {
    match target {
        Target::Aarch64 => "aarch64-unknown-linux-gnu",
        Target::X86_64 => "x86_64-unknown-linux-gnu",
    }
}

/// The module header: layout, triple, PIC/PIE levels (clang's `-fPIE`)
/// and the target's protection flags (`flags`, already numbered from 2).
fn header(target: Target, flags: &[&str]) -> String {
    let layout = match target {
        Target::Aarch64 => hod_ir::AARCH64_LINUX_DATA_LAYOUT,
        Target::X86_64 => hod_ir::X86_64_LINUX_DATA_LAYOUT,
    };
    let mut out =
        format!("target datalayout = \"{layout}\"\ntarget triple = \"{}\"\n", triple(target));
    let mut names = vec!["!0".to_owned(), "!1".to_owned()];
    let mut meta = String::from(
        "!0 = !{i32 8, !\"PIC Level\", i32 2}\n!1 = !{i32 7, !\"PIE Level\", i32 2}\n",
    );
    for (i, flag) in flags.iter().enumerate() {
        names.push(format!("!{}", i + 2));
        writeln!(meta, "!{} = !{{{flag}}}", i + 2).unwrap();
    }
    writeln!(out, "!llvm.module.flags = !{{{}}}\n{meta}", names.join(", ")).unwrap();
    out
}

/// Declarations every module shares (the C++ harness defines them).
const DECLS: &str = r"
%S = type { [6 x i64] }
@_ZTIi = external constant ptr
@tail_ptr = external global ptr
@nestval = internal global i64 77
declare void @fill(ptr, i64, i64)
declare i64 @check(ptr, i64, i64)
declare void @thrower(i64)
declare i64 @tail_target(i64)
declare i64 @tail12(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @vsum(i64, ptr)
declare i32 @_setjmp(ptr) returns_twice
declare void @jump(ptr, i64) noreturn
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
";

/// AArch64 only: x86-64 refuses `nest` itself (`x64:nest`).
const NEST_DECLS: &str = r"
define internal i64 @nest_target(ptr nest %c, i64 %y) {
  %v = load i64, ptr %c
  %r = add i64 %v, %y
  ret i64 %r
}
";

/// The frame sizes of the plain shape: below a page, a page with less
/// than 1024 bytes left (AArch64 probes it), several pages (unrolled),
/// above both unroll limits (a loop), 1 MiB.
const FIXED: [u64; 5] = [24, 3000, 13000, 40000, 1 << 20];
/// The frame sizes every other shape is built with: one below a page and
/// one in the loop (on AArch64 also both 12-bit halves of the `sub`).
const SIZES: [u64; 2] = [24, 40000];
/// The shapes built at every size of [`SIZES`], entered as `i64 (i64)`.
const SHAPES: [&str; 15] = [
    "dyn",
    "vla",
    "args",
    "ehargs",
    "byval",
    "eh",
    "ehthrough",
    "mt",
    "mti",
    "mtdyn",
    "mtargs",
    "nestcall",
    "nestmtcall",
    "vacall",
    "sj",
];
/// x86-64 only: called through the register checker.
const PRESERVE_SHAPES: [&str; 3] = ["pm", "pa", "pmdyn"];

/// The buffer every function fills first and checks last.
fn pre(n: u64) -> String {
    format!("  %b = alloca [{n} x i8], align 16\n  call void @fill(ptr %b, i64 {n}, i64 %x)\n")
}

fn post(n: u64, v: &str, tag: &str) -> String {
    format!(
        "  %bc{tag} = call i64 @check(ptr %b, i64 {n}, i64 %x)\n  %ret{tag} = add i64 {v}, \
         %bc{tag}\n  ret i64 %ret{tag}\n"
    )
}

fn twelve(names: &str) -> String {
    (0..12).map(|k| format!("i64 %{names}{k}")).collect::<Vec<_>>().join(", ")
}

/// The functions of one shape at size `n` for the prefix `p` and the
/// attribute group `g`.
#[allow(clippy::too_many_lines)]
fn shape(out: &mut String, shape: &str, p: &str, g: usize, n: u64) {
    let f = format!("{p}_{shape}_{n}");
    let (pre, post0) = (pre(n), post(n, "%v", ""));
    let pers = "personality ptr @__gxx_personality_v0";
    let args_callee = |out: &mut String, name: &str| {
        let mut body = format!("define i64 @{name}({}) #{g} {{\n", twelve("a"));
        body += "  %x = add i64 %a0, 0\n";
        body += &pre;
        body += "  call void @thrower(i64 %a0)\n  %s0 = add i64 %a0, 0\n";
        for k in 1..12 {
            writeln!(
                body,
                "  %m{k} = mul i64 %a{k}, {}\n  %s{k} = add i64 %s{}, %m{k}",
                k + 1,
                k - 1
            )
            .unwrap();
        }
        body += &post(n, "%s11", "");
        body += "}\n";
        out.push_str(&body);
    };
    let args = |x: &str| {
        let mut list = vec![format!("i64 {x}")];
        list.extend((1..12).map(|k| format!("i64 {}", k * 3)));
        list.join(", ")
    };
    match shape {
        "dyn" => write!(
            out,
            "define i64 @{f}(i64 %x) #{g} {{\n{pre}  %s = alloca i64, align 8\n  store i64 %x, ptr \
             %s\n  %n0 = mul i64 %x, 4099\n  %n = and i64 %n0, 16383\n  %d = alloca i8, i64 %n, \
             align 32\n  call void @fill(ptr %d, i64 %n, i64 %x)\n  %r = call i64 @check(ptr %d, \
             i64 %n, i64 %x)\n  %l = load i64, ptr %s\n  %a = ptrtoint ptr %d to i64\n  %m = and \
             i64 %a, 31\n  %t = add i64 %r, %l\n  %v = add i64 %t, %m\n{post0}}}\n"
        ),
        "vla" => write!(
            out,
            "define i64 @{f}(i64 %x) #{g} {{\nentry:\n{pre}  %acc = alloca i64, align 8\n  store \
             i64 %x, ptr %acc\n  br label %loop\nloop:\n  %i = phi i64 [0, %entry], [%i1, %loop]\n  \
             %sp = call ptr @llvm.stacksave.p0()\n  %n0 = mul i64 %i, 5000\n  %n = add i64 %n0, \
             16\n  %d = alloca i8, i64 %n, align 16\n  call void @fill(ptr %d, i64 %n, i64 %i)\n  \
             %r = call i64 @check(ptr %d, i64 %n, i64 %i)\n  %a = load i64, ptr %acc\n  %a1 = add \
             i64 %a, %r\n  %a2 = add i64 %a1, %i\n  store i64 %a2, ptr %acc\n  call void \
             @llvm.stackrestore.p0(ptr %sp)\n  %i1 = add i64 %i, 1\n  %c = icmp ult i64 %i1, 3\n  \
             br i1 %c, label %loop, label %done\ndone:\n  %v = load i64, ptr %acc\n{post0}}}\n"
        ),
        "args" => {
            args_callee(out, &format!("{f}_callee"));
            write!(
                out,
                "define i64 @{f}(i64 %x) #{g} {{\n{pre}  %x2 = shl i64 %x, 1\n  %v = call i64 \
                 @{f}_callee({})\n{post0}}}\n",
                args("%x2")
            )
        }
        "ehargs" => {
            args_callee(out, &format!("{f}_callee"));
            write!(
                out,
                "define i64 @{f}(i64 %x) #{g} {pers} {{\nentry:\n{pre}  %v = invoke i64 \
                 @{f}_callee({}) to label %ok unwind label %lp\nok:\n{post0}lp:\n  %l = landingpad \
                 {{ ptr, i32 }} catch ptr @_ZTIi\n  %e = extractvalue {{ ptr, i32 }} %l, 0\n  %q = \
                 call ptr @__cxa_begin_catch(ptr %e)\n  %w = load i32, ptr %q\n  call void \
                 @__cxa_end_catch()\n  %w1 = sext i32 %w to i64\n  %w2 = add i64 %w1, 1000\n{}}}\n",
                args("%x"),
                post(n, "%w2", "l")
            )
        }
        "byval" => {
            let mut callee = format!("define i64 @{f}_callee(ptr byval(%S) align 8 %s, i64 %x) #{g} {{\n{pre}  %t0 = add i64 0, 0\n");
            for k in 0..6 {
                writeln!(
                    callee,
                    "  %p{k} = getelementptr inbounds %S, ptr %s, i64 0, i32 0, i64 {k}\n  %l{k} \
                     = load i64, ptr %p{k}\n  %m{k} = mul i64 %l{k}, {}\n  %t{} = add i64 %t{k}, \
                     %m{k}\n  store i64 0, ptr %p{k}",
                    k + 2,
                    k + 1
                )
                .unwrap();
            }
            callee += &post(n, "%t6", "");
            callee += "}\n";
            out.push_str(&callee);
            let mut outer = format!("define i64 @{f}(i64 %x) #{g} {{\n{pre}  %s = alloca %S, align 8\n");
            for k in 0..6 {
                writeln!(
                    outer,
                    "  %p{k} = getelementptr inbounds %S, ptr %s, i64 0, i32 0, i64 {k}\n  %y{k} \
                     = add i64 %x, {k}\n  store i64 %y{k}, ptr %p{k}"
                )
                .unwrap();
            }
            outer += "  %v0 = call i64 @";
            write!(
                outer,
                "{f}_callee(ptr byval(%S) align 8 %s, i64 %x)\n  %l = load i64, ptr %p5\n  %v = \
                 add i64 %v0, %l\n{post0}}}\n"
            )
            .unwrap();
            out.push_str(&outer);
            Ok(())
        }
        "ehthrough" => write!(
            out,
            "define i64 @{f}(i64 %x) #{g} {{\n{pre}  call void @thrower(i64 %x)\n  %v = add i64 \
             %x, 5\n{post0}}}\n"
        ),
        "eh" => write!(
            out,
            "define i64 @{f}(i64 %x) #{g} {pers} {{\nentry:\n{pre}  %v = invoke i64 \
             @{p}_ehthrough_{n}(i64 %x) to label %ok unwind label %lp\nok:\n{post0}lp:\n  %l = \
             landingpad {{ ptr, i32 }} catch ptr @_ZTIi\n  %e = extractvalue {{ ptr, i32 }} %l, 0\n  \
             %q = call ptr @__cxa_begin_catch(ptr %e)\n  %w = load i32, ptr %q\n  call void \
             @__cxa_end_catch()\n  %w1 = sext i32 %w to i64\n  %w2 = add i64 %w1, 1000\n{}}}\n",
            post(n, "%w2", "l")
        ),
        "mt" | "mti" | "mtdyn" => {
            let mut body = format!("define i64 @{f}(i64 %x) #{g} {{\n{pre}");
            if shape == "mtdyn" {
                body += "  %n = and i64 %x, 255\n  %d = alloca i8, i64 %n, align 64\n  call void \
                         @fill(ptr %d, i64 %n, i64 %x)\n  %dc = call i64 @check(ptr %d, i64 %n, \
                         i64 %x)\n";
            } else {
                body += "  %dc = add i64 0, 0\n";
            }
            body += "  %bc = call i64 @check(ptr %b, i64 ";
            write!(body, "{n}, i64 %x)\n  %y0 = add i64 %x, %bc\n  %y = add i64 %y0, %dc\n").unwrap();
            if shape == "mti" {
                body += "  %fp = load ptr, ptr @tail_ptr\n  %r = musttail call i64 %fp(i64 %y)\n";
            } else {
                body += "  %r = musttail call i64 @tail_target(i64 %y)\n";
            }
            body += "  ret i64 %r\n}\n";
            out.push_str(&body);
            Ok(())
        }
        "mtargs" => {
            let mut body = format!("define i64 @{f}_callee({}) #{g} {{\n", twelve("a"));
            body += "  %x = add i64 %a0, 0\n";
            body += &pre;
            write!(
                body,
                "  %bc = call i64 @check(ptr %b, i64 {n}, i64 %x)\n  %y = add i64 %a11, %bc\n  %r \
                 = musttail call i64 @tail12({}, i64 %y)\n  ret i64 %r\n}}\n",
                (0..11).map(|k| format!("i64 %a{k}")).collect::<Vec<_>>().join(", ")
            )
            .unwrap();
            out.push_str(&body);
            write!(
                out,
                "define i64 @{f}(i64 %x) #{g} {{\n{pre}  %v = call i64 @{f}_callee({})\n{post0}}}\n",
                args("%x")
            )
        }
        "nestcall" => write!(
            out,
            "define i64 @{f}_callee(ptr nest %c, i64 %x) #{g} {{\n{pre}  %l = load i64, ptr %c\n  \
             %v = add i64 %l, %x\n{post0}}}\ndefine i64 @{f}(i64 %x) #{g} {{\n{pre}  %v = call i64 \
             @{f}_callee(ptr nest @nestval, i64 %x)\n{post0}}}\n"
        ),
        "nestmtcall" => write!(
            out,
            "define i64 @{f}_callee(ptr nest %c, i64 %x) #{g} {{\n{pre}  %bc = call i64 \
             @check(ptr %b, i64 {n}, i64 %x)\n  %y = add i64 %x, %bc\n  %r = musttail call i64 \
             @nest_target(ptr nest %c, i64 %y)\n  ret i64 %r\n}}\ndefine i64 @{f}(i64 %x) #{g} \
             {{\n{pre}  %v = call i64 @{f}_callee(ptr nest @nestval, i64 %x)\n{post0}}}\n"
        ),
        "vacall" => write!(
            out,
            "define i64 @{f}_callee(i64 %x, ...) #{g} {{\n{pre}  %ap = alloca [32 x i8], align \
             16\n  call void @llvm.va_start.p0(ptr %ap)\n  %v = call i64 @vsum(i64 %x, ptr %ap)\n  \
             call void @llvm.va_end.p0(ptr %ap)\n{post0}}}\ndefine i64 @{f}(i64 %x) #{g} \
             {{\n{pre}  %v = call i64 (i64, ...) @{f}_callee(i64 10, i64 %x, i64 2, i64 3, i64 \
             4, i64 5, i64 6, i64 7, i64 8, i64 9, i64 10)\n{post0}}}\n"
        ),
        "sj" => write!(
            out,
            "define i64 @{f}(i64 %x) #{g} {{\nentry:\n{pre}  %jb = alloca [512 x i8], align 16\n  \
             %s = alloca i64, align 8\n  store volatile i64 %x, ptr %s\n  %r = call i32 \
             @_setjmp(ptr %jb)\n  %z = icmp eq i32 %r, 0\n  br i1 %z, label %first, label \
             %second\nfirst:\n  store volatile i64 1000, ptr %s\n  call void @jump(ptr %jb, i64 \
             %x)\n  unreachable\nsecond:\n  %l = load volatile i64, ptr %s\n  %r64 = sext i32 %r \
             to i64\n  %v = add i64 %l, %r64\n{post0}}}\n"
        ),
        "pm" | "pmdyn" | "pa" => {
            let cc = if shape == "pa" { "preserve_allcc" } else { "preserve_mostcc" };
            let mut body = format!("define {cc} i64 @{f}(i64 %x) #{g} {{\n{pre}");
            if shape == "pmdyn" {
                body += "  %n = and i64 %x, 1023\n  %d = alloca i8, i64 %n, align 16\n  call void \
                         @fill(ptr %d, i64 %n, i64 %x)\n  %dc = call i64 @check(ptr %d, i64 %n, \
                         i64 %x)\n";
            } else {
                body += "  %dc = add i64 0, 0\n";
            }
            body += "  %fx = sitofp i64 %x to double\n  %fy = fmul double %fx, 2.5\n  %fz = \
                     fptosi double %fy to i64\n  %v = add i64 %fz, %dc\n";
            body += &post0;
            body += "}\n";
            out.push_str(&body);
            Ok(())
        }
        other => unreachable!("shape {other}"),
    }
    .unwrap();
}

/// One combination of flags: its name and its function attributes.
struct Combo {
    name: String,
    attrs: String,
}

fn combos(target: Target) -> Vec<Combo> {
    let ssp = [("none", ""), ("ssp", "ssp"), ("strong", "sspstrong"), ("req", "sspreq")];
    let probe = [("", ""), ("probe", "\"probe-stack\"=\"inline-asm\"")];
    let protect: &[(&str, &str)] = match target {
        Target::X86_64 => &[("", "")],
        Target::Aarch64 => &[
            ("", ""),
            ("bti", "\"branch-target-enforcement\"=\"true\""),
            ("nla", "\"sign-return-address\"=\"non-leaf\" \"sign-return-address-key\"=\"a_key\""),
            (
                "allabti",
                "\"sign-return-address\"=\"all\" \"sign-return-address-key\"=\"a_key\" \
                 \"branch-target-enforcement\"=\"true\"",
            ),
            (
                "nlbbti",
                "\"sign-return-address\"=\"non-leaf\" \"sign-return-address-key\"=\"b_key\" \
                 \"branch-target-enforcement\"=\"true\"",
            ),
            ("allb", "\"sign-return-address\"=\"all\" \"sign-return-address-key\"=\"b_key\""),
        ],
    };
    let mut out = Vec::new();
    for (sn, sa) in ssp {
        for (pn, pa) in probe {
            for &(bn, ba) in protect {
                out.push(Combo {
                    name: format!("{sn}{pn}{bn}"),
                    attrs: format!("uwtable \"frame-pointer\"=\"all\" {sa} {pa} {ba}"),
                });
            }
        }
    }
    out
}

/// One module: every combination whose name `keep` accepts, the
/// functions prefixed with `tag`; the entry points it defines (plain,
/// and those called through the register checker).
fn module(
    target: Target,
    tag: &str,
    flags: &[&str],
    keep: impl Fn(&str) -> bool,
) -> (String, Vec<String>, Vec<(String, &'static str)>) {
    let mut ir = header(target, flags);
    ir += DECLS;
    if target == Target::Aarch64 {
        ir += NEST_DECLS;
    }
    let (mut plain, mut checked) = (Vec::new(), Vec::new());
    let mut groups = String::new();
    for (g, combo) in combos(target).iter().enumerate() {
        if !keep(&combo.name) {
            continue;
        }
        let p = format!("{tag}{}", combo.name);
        writeln!(groups, "attributes #{g} = {{ {} }}", combo.attrs).unwrap();
        for n in FIXED {
            write!(
                ir,
                "define i64 @{p}_fixed_{n}(i64 %x) #{g} {{\n{}{}}}\n",
                pre(n),
                post(n, "%x", "")
            )
            .unwrap();
            plain.push(format!("{p}_fixed_{n}"));
        }
        for n in SIZES {
            for s in SHAPES {
                // x86-64 refuses `nest` itself (`x64:nest`, `spec/abi-x64.md`).
                if target == Target::X86_64 && s.starts_with("nest") {
                    continue;
                }
                shape(&mut ir, s, &p, g, n);
                plain.push(format!("{p}_{s}_{n}"));
            }
            if target == Target::X86_64 {
                for s in PRESERVE_SHAPES {
                    shape(&mut ir, s, &p, g, n);
                    checked.push((
                        format!("{p}_{s}_{n}"),
                        if s == "pa" { "pa_check" } else { "pm_check" },
                    ));
                }
            }
        }
    }
    ir += &groups;
    (ir, plain, checked)
}

/// The C++ side: the helpers the modules call, the register checkers
/// (x86-64), and `main`, which calls every entry with `x` = 1 (an odd
/// value: `thrower` throws) and 2, catching what escapes.
fn harness(target: Target, plain: &[String], checked: &[(String, &str)]) -> String {
    let mut c = String::from(HARNESS);
    if target == Target::X86_64 {
        c += CHECKERS_X64;
    }
    for name in plain {
        writeln!(c, "extern \"C\" long {name}(long);").unwrap();
    }
    for (name, _) in checked {
        writeln!(c, "extern \"C\" long {name}(long);").unwrap();
    }
    c += "int main() {\n  long r;\n";
    for name in plain {
        writeln!(
            c,
            "  for (long x = 1; x <= 2; x++) {{ try {{ r = {name}(x); }} catch (int v) {{ r = \
             2000000 + v; }} printf(\"{name} %ld %ld\\n\", x, r); }}"
        )
        .unwrap();
    }
    for (name, checker) in checked {
        writeln!(
            c,
            "  for (long x = 1; x <= 2; x++) {{ long bad = 0; r = {checker}({name}, x, &bad); \
             printf(\"{name} %ld %ld bad=%ld\\n\", x, r, bad); }}"
        )
        .unwrap();
    }
    c += "  return 0;\n}\n";
    c
}

const HARNESS: &str = r#"#include <csetjmp>
#include <cstdarg>
#include <cstdio>
extern "C" {
void fill(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++) p[i] = (unsigned char)(i * 7 + x);
}
long check(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++)
    if (p[i] != (unsigned char)(i * 7 + x)) {
      printf("OVERWRITTEN at %ld of %ld\n", i, n);
      return 1000000 + i;
    }
  return 0;
}
void thrower(long x) {
  if (x & 1) throw (int)(x * 3);
}
long tail_target(long y) { return y * 5 + 1; }
long (*tail_ptr)(long) = tail_target;
long tail12(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, long a8,
            long a9, long a10, long a11) {
  return a0 + 2 * a1 + 3 * a2 + 4 * a3 + 5 * a4 + 6 * a5 + 7 * a6 + 8 * a7 + 9 * a8 + 10 * a9 +
         11 * a10 + 12 * a11;
}
long vsum(long n, va_list *ap) {
  long s = 0;
  for (long i = 0; i < n; i++) s = s * 3 + va_arg(*ap, long);
  return s;
}
[[noreturn]] void jump(void *jb, long x) { longjmp(*(jmp_buf *)jb, (int)(x % 5) + 1); }
}
"#;

/// x86-64: `pm_check(fn, x, &bad)` calls `fn(x)` with every register
/// `preserve_mostcc` keeps holding a known value (`rdi` holds `x`) and
/// counts in `bad` those that changed; `pa_check` also fills and checks
/// `xmm0`..`xmm15`.
const CHECKERS_X64: &str = r#"
extern "C" long pm_check(long (*)(long), long, long *);
extern "C" long pa_check(long (*)(long), long, long *);
#define FILL_GPR \
  "mov %rdi, %rbx\n mov %rdx, %r13\n mov %rsi, %rdi\n mov %rsi, %r12\n" \
  "mov $0x1111, %rcx\n mov $0x2222, %rdx\n mov $0x3333, %rsi\n" \
  "mov $0x4444, %r8\n mov $0x5555, %r9\n mov $0x6666, %r10\n"
#define CHECK(reg, value) "cmp $" #value ", %" #reg "\n je 1f\n incq (%r13)\n1:\n"
#define CHECK_GPR \
  CHECK(rcx, 0x1111) CHECK(rdx, 0x2222) CHECK(rsi, 0x3333) CHECK(r8, 0x4444) \
  CHECK(r9, 0x5555) CHECK(r10, 0x6666) "cmp %r12, %rdi\n je 1f\n incq (%r13)\n1:\n"
#define SETX(k) "mov $0x770" #k ", %r11\n movq %r11, %xmm" #k "\n"
#define GETX(k) "movq %xmm" #k ", %r11\n cmp $0x770" #k ", %r11\n je 1f\n incq (%r13)\n1:\n"
#define ALLX(m) m(0) m(1) m(2) m(3) m(4) m(5) m(6) m(7) m(8) m(9)
asm(".text\n.globl pm_check\n.type pm_check,@function\npm_check:\n"
    "push %rbx\n push %r12\n push %r13\n" FILL_GPR "call *%rbx\n" CHECK_GPR
    "pop %r13\n pop %r12\n pop %rbx\n ret\n"
    ".globl pa_check\n.type pa_check,@function\npa_check:\n"
    "push %rbx\n push %r12\n push %r13\n" FILL_GPR ALLX(SETX)
    "mov $0x7710, %r11\n movq %r11, %xmm10\n mov $0x7715, %r11\n movq %r11, %xmm15\n"
    "call *%rbx\n" CHECK_GPR ALLX(GETX)
    "movq %xmm10, %r11\n cmp $0x7710, %r11\n je 1f\n incq (%r13)\n1:\n"
    "movq %xmm15, %r11\n cmp $0x7715, %r11\n je 1f\n incq (%r13)\n1:\n"
    "pop %r13\n pop %r12\n pop %rbx\n ret\n");
"#;

/// Which object a build is.
#[derive(Clone, Copy, Debug)]
enum Build {
    Hod,
    Stress,
    Llc(u32),
}

impl Build {
    fn name(self) -> String {
        match self {
            Build::Hod => "hod".into(),
            Build::Stress => "stress".into(),
            Build::Llc(major) => format!("llc{major}"),
        }
    }
}

/// hod's object, through the facade or below it with every stress
/// option.
fn hod_object(text: &str, target: Target, stress: bool) -> Result<Vec<u8>, String> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .map_err(|e| format!("hod_ll: {e}"))?;
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .map_err(|e| format!("hod_ll: {e}"))?;
    let options = hod_cg::Options {
        reloc_model: hod_cg::RelocModel::Pic,
        stress: if stress { hod_cg::Stress::ALL } else { hod_cg::Stress::default() },
        ..hod_cg::Options::default()
    };
    match target {
        Target::X86_64 => {
            hod_x64::compile_to_object(&module, &bodies, &options).map_err(|e| e.to_string())
        }
        Target::Aarch64 => {
            hod_a64::compile_to_object(&module, &bodies, &options).map_err(|e| e.to_string())
        }
    }
}

fn llc_object(dir: &Scratch, major: u32, text: &str, name: &str) -> Option<PathBuf> {
    let llc = tool(&format!("llc-{major}"))?;
    let object = dir.join(name);
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"]);
    command.args(["-filetype=obj", "-o"]).arg(&object);
    let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(5));
    assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
    Some(object)
}

/// The objects of `build` for the module texts `texts`.
fn objects(dir: &Scratch, target: Target, build: Build, texts: &[String]) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for (i, text) in texts.iter().enumerate() {
        let name = format!("{}-{}-{i}.o", target.name(), build.name());
        match build {
            Build::Hod | Build::Stress => {
                let object = hod_object(text, target, matches!(build, Build::Stress))
                    .unwrap_or_else(|e| panic!("{}: {}: {e}", target.name(), build.name()));
                out.push(dir.write(&name, object));
            }
            Build::Llc(major) => out.push(llc_object(dir, major, text, &name)?),
        }
    }
    Some(out)
}

/// Builds `texts` with every build, links each with the harness under
/// every linker and runs it; asserts every output is the first's and
/// returns it.
fn run_everywhere(
    dir: &Scratch,
    toolchain: &Toolchain,
    texts: &[String],
    harness: &str,
    harness_flags: &[&str],
    builds: &[Build],
) -> Option<String> {
    let target = toolchain.target;
    let mut flags = vec!["-O0"];
    flags.extend(harness_flags);
    let main =
        toolchain.compile_cxx(dir, &format!("harness-{}", target.name()), harness, &flags)?;
    let mut expected: Option<(String, String)> = None;
    for &build in builds {
        let Some(objects) = objects(dir, target, build, texts) else { continue };
        for linker in &toolchain.linkers {
            let program = dir.join(format!("{}-{}-{}", target.name(), build.name(), linker.name));
            let mut inputs: Vec<&Path> = vec![&main];
            inputs.extend(objects.iter().map(PathBuf::as_path));
            // After the objects that need it.
            inputs.push(Path::new("-lstdc++"));
            let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &program);
            let who = format!("{} {} {}", target.name(), build.name(), linker.name);
            assert!(ran.success(), "{who}: {ran:?}\n{}", ran.stderr_text());
            let out = ran.stdout_text();
            let changed = out.lines().any(|l| l.contains("bad=") && !l.ends_with("bad=0"));
            assert!(!changed, "{who}: a preserved register changed:\n{out}");
            assert!(!out.contains("OVERWRITTEN"), "{who}: a buffer was overwritten:\n{out}");
            match &expected {
                None => expected = Some((who, out)),
                Some((first, want)) => {
                    if *want != out {
                        let diff: Vec<_> = want
                            .lines()
                            .zip(out.lines())
                            .filter(|(a, b)| a != b)
                            .take(20)
                            .collect();
                        panic!("{who} differs from {first}: {diff:#?}");
                    }
                }
            }
        }
    }
    expected.map(|(_, out)| out)
}

fn all_builds() -> Vec<Build> {
    let mut builds: Vec<Build> = LLVM_MAJORS.map(Build::Llc).into();
    builds.extend([Build::Hod, Build::Stress]);
    builds
}

/// The module texts of `target`: on x86-64 one per value of the
/// `cf-protection` flags (none, branch, branch and return), each with its
/// own prefix; on AArch64 one, the policies on the functions.
fn texts(target: Target) -> (Vec<String>, Vec<String>, Vec<(String, &'static str)>) {
    let (mut texts, mut plain, mut checked) = (Vec::new(), Vec::new(), Vec::new());
    let sets: &[(&str, &[&str])] = match target {
        Target::X86_64 => &[
            ("n", &[]),
            ("b", &["i32 8, !\"cf-protection-branch\", i32 1"]),
            (
                "r",
                &[
                    "i32 8, !\"cf-protection-branch\", i32 1",
                    "i32 8, !\"cf-protection-return\", i32 1",
                ],
            ),
        ],
        Target::Aarch64 => &[("a", &[])],
    };
    for &(tag, flags) in sets {
        let (ir, p, c) = module(target, tag, flags, |_| true);
        texts.push(ir);
        plain.extend(p);
        checked.extend(c);
    }
    (texts, plain, checked)
}

/// Every combination of the flags and every shape: hod (plain and
/// stressed), `llc` 19 and 22, under bfd, lld and mold, print the same.
#[test]
fn every_combination_runs_as_llc_s() {
    for target in Target::ALL {
        let dir = Scratch::new(&format!("hardening-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let (texts, plain, checked) = texts(target);
        let harness = harness(target, &plain, &checked);
        let Some(out) = run_everywhere(&dir, &toolchain, &texts, &harness, &[], &all_builds())
        else {
            continue;
        };
        assert_eq!(out.lines().count(), 2 * (plain.len() + checked.len()), "{}", target.name());
        eprintln!("{}: {} entries, every build and linker the same", target.name(), plain.len());
    }
}

/// AArch64 with the branch-protection note on every object (the module
/// flags in clang 22's form, the harness built with
/// `-mbranch-protection=standard`), so that the kernel maps the program
/// with BTI on: every combination with `branch-target-enforcement` runs
/// as `llc` 22's (`llc` 19 puts no `bti j` on a landing pad, so its own
/// program faults; `spec/codegen.md` §5.6.26). A missing `bti c` or `bti
/// j` where an indirect branch lands (the tail call through `x16`, the
/// unwinder's branch to a landing pad, `longjmp`) is a `SIGILL`.
#[test]
fn bti_is_enforced_at_every_landing_pad() {
    let target = Target::Aarch64;
    if !target.is_host() {
        return; // BTI needs the hardware; qemu's baseline model has none.
    }
    let dir = Scratch::new("hardening-bti");
    let Some(toolchain) = Toolchain::for_target(&dir, target) else { return };
    let flags =
        ["i32 8, !\"branch-target-enforcement\", i32 2", "i32 8, !\"sign-return-address\", i32 2"];
    let (ir, plain, checked) = module(target, "e", &flags, |name| name.contains("bti"));
    let harness = harness(target, &plain, &checked);
    let builds = [Build::Llc(22), Build::Hod, Build::Stress];
    let std = ["-mbranch-protection=standard"];
    let out = run_everywhere(&dir, &toolchain, &[ir], &harness, &std, &builds);
    let Some(out) = out else { return };
    assert_eq!(out.lines().count(), 2 * plain.len());
    // The program really is marked: the kernel enforces BTI on it.
    let Some(readelf) = tool("readelf") else { return };
    let program = dir.join(format!("{}-hod-{}", target.name(), toolchain.linkers[0].name));
    let notes = hod_testutil::run(readelf, [OsStr::new("-n"), program.as_os_str()]).stdout_text();
    assert!(notes.contains("AArch64 feature: BTI, PAC"), "{notes}");
}

/// The protection attributes of one target, as attribute groups for the
/// overflow and the guard-page programs: every protector with probes on,
/// with each branch policy of the target.
fn protected_groups(target: Target) -> Vec<(String, String)> {
    combos(target)
        .into_iter()
        .filter(|c| c.name.contains("probe") && !c.name.starts_with("none"))
        .map(|c| (c.name, c.attrs))
        .collect()
}

/// §5.6.24 with §5.6.25 to §5.6.27: a one-byte overflow of a 16-byte
/// array, and of a 48-byte `alloca`, in a probed, branch-protected
/// function still reaches its guard: the program aborts with "stack
/// smashing detected" for every protector and policy; without the
/// overflow it returns.
#[test]
fn overflows_are_caught_with_every_flag() {
    use std::os::unix::process::ExitStatusExt as _;
    for target in Target::ALL {
        let dir = Scratch::new(&format!("hardening-smash-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let groups = protected_groups(target);
        let mut texts = Vec::new();
        let mut names = Vec::new();
        let sets: &[(&str, &[&str])] = match target {
            Target::X86_64 => &[("n", &[]), ("b", &["i32 8, !\"cf-protection-branch\", i32 1"])],
            Target::Aarch64 => &[("a", &[])],
        };
        for &(tag, flags) in sets {
            let mut ir = header(target, flags);
            ir += "declare void @overrun(ptr, i64)\n";
            for (g, (name, attrs)) in groups.iter().enumerate() {
                let f = format!("{tag}{name}");
                write!(
                    ir,
                    "define void @{f}_fixed(i64 %n) #{g} {{\n  %b = alloca [16 x i8], align 16\n  \
                     %l = add i64 %n, 16\n  call void @overrun(ptr %b, i64 %l)\n  ret void\n}}\n\
                     define void @{f}_dyn(i64 %n) #{g} {{\n  %k = add i64 %n, 0\n  %m = and i64 \
                     %k, 0\n  %s = add i64 %m, 48\n  %b = alloca i8, i64 %s, align 16\n  %l = add \
                     i64 %n, 48\n  call void @overrun(ptr %b, i64 %l)\n  ret void\n}}\n\
                     attributes #{g} = {{ {attrs} }}\n"
                )
                .unwrap();
                names.push(format!("{f}_fixed"));
                names.push(format!("{f}_dyn"));
            }
            texts.push(ir);
        }
        let mut c = String::from(
            "#include <cstdio>\n#include <cstdlib>\n#include <cstring>\n\
             extern \"C\" void overrun(char *p, long n) { memset(p, 0x41, n); }\n",
        );
        for name in &names {
            writeln!(c, "extern \"C\" void {name}(long);").unwrap();
        }
        c += "int main(int argc, char **argv) {\n  int which = atoi(argv[1]);\n  long n = \
              atol(argv[2]);\n  switch (which) {\n";
        for (i, name) in names.iter().enumerate() {
            writeln!(c, "  case {i}: {name}(n); break;").unwrap();
        }
        c += "  }\n  puts(\"returned\");\n  return 0;\n}\n";
        let Some(main) = toolchain.compile_cxx(&dir, "smash", &c, &["-O0"]) else { continue };
        let objects = objects(&dir, target, Build::Hod, &texts).expect("hod builds");
        let linker = &toolchain.linkers[0];
        let program = dir.join("smash");
        let mut inputs: Vec<&Path> = vec![&main];
        inputs.extend(objects.iter().map(PathBuf::as_path));
        inputs.push(Path::new("-lstdc++"));
        toolchain.link_ok(linker, &["-pie"], &inputs, &program);
        for (i, name) in names.iter().enumerate() {
            for n in [0, 1] {
                let mut run = toolchain.runner_command(&program);
                run.arg(i.to_string()).arg(n.to_string());
                let ran = run_with(&mut run, b"", Duration::from_mins(2));
                let who = format!("{}: {name} overflow {n}", target.name());
                if n == 0 {
                    assert!(ran.success() && ran.stdout_text() == "returned\n", "{who}: {ran:?}");
                } else {
                    let signal = ran.status.and_then(|s| s.signal());
                    assert_eq!(signal, Some(6), "{who}: {ran:?}");
                    assert!(ran.stderr_text().contains("stack smashing detected"), "{who}");
                }
            }
        }
        eprintln!("{}: {} protected functions abort at one byte", target.name(), names.len());
    }
}

/// §5.6.27 with §5.6.24 and the branch policies: WP-186's guard-page run
/// (`crates/hod/tests/stack_probe.rs`) on functions that are also
/// protected (`sspstrong`) and, on AArch64, signed with BTI (x86-64: the
/// module has `cf-protection-branch`). A 1 MiB frame and a 1 MiB `alloca`
/// fault within one page below the guard's top, and `backtrace()` in the
/// signal handler, which unwinds from inside the probe loop of a signed
/// frame through the CFI, reaches the caller; without probes the frame
/// lands below the guard. hod (plain and stressed) and `llc` 19 and 22.
#[test]
fn probe_faults_unwind_through_protected_signed_frames() {
    for target in Target::ALL {
        let dir = Scratch::new(&format!("hardening-guard-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let (probed, plain, flags): (&str, &str, &[&str]) = match target {
            Target::X86_64 => {
                ("strongprobe", "strong", &["i32 8, !\"cf-protection-branch\", i32 1"])
            }
            Target::Aarch64 => ("strongprobeallabti", "strongallabti", &[]),
        };
        let attrs = |name: &str| {
            combos(target).into_iter().find(|c| c.name == name).expect("a combination").attrs
        };
        let mut ir = header(target, flags);
        ir += "declare void @fill(ptr, i64, i64)\ndeclare i64 @check(ptr, i64, i64)\n";
        for (name, group) in [("guard", 0), ("plain", 1)] {
            write!(
                ir,
                "define i64 @{name}_fixed(i64 %x) #{group} {{\n{}{}}}\n\
                 define i64 @{name}_dyn(i64 %n, i64 %x) #{group} {{\n  %b = alloca i8, i64 %n, \
                 align 16\n  call void @fill(ptr %b, i64 %n, i64 %x)\n  %r = call i64 @check(ptr \
                 %b, i64 %n, i64 %x)\n  ret i64 %r\n}}\n",
                pre(1 << 20),
                post(1 << 20, "0", "")
            )
            .unwrap();
        }
        writeln!(
            ir,
            "attributes #0 = {{ {} }}\nattributes #1 = {{ {} }}",
            attrs(probed),
            attrs(plain)
        )
        .unwrap();
        let c = format!("{GUARD_HARNESS}{GUARD_MAIN}");
        let main = toolchain.compile_c(&dir, "guard", &c, &["-O0", "-w"]);
        for build in all_builds() {
            let Some(objects) = objects(&dir, target, build, std::slice::from_ref(&ir)) else {
                continue;
            };
            for linker in &toolchain.linkers {
                let program = dir.join(format!("guard-{}-{}", build.name(), linker.name));
                let inputs = [main.as_path(), objects[0].as_path(), Path::new("-pthread")];
                let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &program);
                let who = format!("{} {} {}", target.name(), build.name(), linker.name);
                assert!(ran.success(), "{who}: {ran:?}");
                assert_eq!(ran.stdout_text(), GUARD_EXPECTED, "{who}");
            }
        }
    }
}

/// WP-186's C side: `fill`, `check`, and the guard runs on a thread whose
/// stack is `[guard | stack]` above a mapped region the frames of the
/// unprobed functions land in.
const GUARD_HARNESS: &str = r#"#define _GNU_SOURCE
#include <execinfo.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>

long guard_fixed(long);
long plain_fixed(long);
long guard_dyn(long, long);
long plain_dyn(long, long);

void fill(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++) p[i] = (unsigned char)(i * 7 + x);
}
long check(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++)
    if (p[i] != (unsigned char)(i * 7 + x)) return i + 1;
  return 0;
}

#define VICTIM (4L << 20)
#define GUARD (64L << 10)
#define STACK (256L << 10)
static char *guard_top;
static sigjmp_buf back;
static volatile uintptr_t fault;
static volatile int unwound;
static int which;

__attribute__((noinline)) long caller_of_probe(void) {
  long r;
  switch (which) {
  case 0: r = guard_fixed(1); break;
  case 1: r = plain_fixed(1); break;
  case 2: r = guard_dyn(1L << 20, 1); break;
  default: r = plain_dyn(1L << 20, 1); break;
  }
  return r + 0;
}

static void on_fault(int sig, siginfo_t *info, void *context) {
  (void)sig;
  (void)context;
  fault = (uintptr_t)info->si_addr;
  void *frames[32];
  int n = backtrace(frames, 32);
  for (int i = 0; i < n; i++) {
    uintptr_t at = (uintptr_t)frames[i];
    if (at > (uintptr_t)caller_of_probe && at < (uintptr_t)caller_of_probe + 512) unwound = 1;
  }
  siglongjmp(back, 1);
}

static void *on_thread(void *arg) {
  (void)arg;
  static char alt[1 << 16];
  stack_t ss = {.ss_sp = alt, .ss_size = sizeof alt, .ss_flags = 0};
  sigaltstack(&ss, 0);
  if (sigsetjmp(back, 1) == 0) {
    long r = caller_of_probe();
    printf("%s: returned %ld\n", which & 1 ? "plain" : "probed", r);
  } else {
    uintptr_t top = (uintptr_t)guard_top;
    int near = fault < top && fault >= top - 4096;
    printf("%s: fault near=%d unwound=%d\n", which & 1 ? "plain" : "probed", near, unwound);
  }
  return 0;
}

static void guard_run(int what) {
  char *base = mmap(0, VICTIM + GUARD + STACK, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
  if (base == MAP_FAILED) { printf("mmap failed\n"); return; }
  mprotect(base + VICTIM, GUARD, PROT_NONE);
  guard_top = base + VICTIM + GUARD;
  which = what;
  fault = 0;
  unwound = 0;
  pthread_attr_t attr;
  pthread_attr_init(&attr);
  pthread_attr_setstack(&attr, guard_top, STACK);
  pthread_t thread;
  pthread_create(&thread, &attr, on_thread, 0);
  pthread_join(thread, 0);
  munmap(base, VICTIM + GUARD + STACK);
}
"#;

const GUARD_MAIN: &str = r#"
int main(void) {
  void *warm[4];
  backtrace(warm, 4);
  struct sigaction sa;
  memset(&sa, 0, sizeof sa);
  sa.sa_sigaction = on_fault;
  sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
  sigaction(SIGSEGV, &sa, 0);
  for (int what = 0; what < 4; what++) {
    printf("%s ", what < 2 ? "fixed" : "alloca");
    fflush(stdout);
    guard_run(what);
  }
  return 0;
}
"#;

const GUARD_EXPECTED: &str = "fixed probed: fault near=1 unwound=1
fixed plain: fault near=0 unwound=1
alloca probed: fault near=1 unwound=1
alloca plain: fault near=0 unwound=1
";
