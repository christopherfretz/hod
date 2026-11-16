//! WP-315 (`spec/codegen-x86.md` §12, `spec/landed/codegen/WP-315.md`):
//! first-class aggregates, the `*.with.overflow` intrinsics at every
//! width from 1 to 64, and the small intrinsics.
//!
//! Programs under `tests/data/i386/agg/`: `agg.ll` and `small.ll` written
//! by hand, `ovf.c` through clang 19 and 22 at CPU `i586` (no `cmov`) and
//! clang's default (`i686`), and a module generated here: the six
//! overflow intrinsics at each width over edge values, all pairs, as a
//! checksum and a count of the overflow flags. The hand-written and
//! generated modules at CPU `generic` and `i686`. Each compiled by `llc`,
//! hod and `hod_x86` under `hod_cg::Stress::ALL`, static and PIE, linked
//! by GNU ld (PIE also by lld and mold), run under `qemu-i386`: the
//! outputs are equal.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool,
};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/agg").join(path)
}

/// hod's object through the facade and `hod_x86`'s under `Stress::ALL`.
fn hod_objects(text: &[u8], pic: bool, label: &str) -> [Vec<u8>; 2] {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
        ..Options::default()
    };
    let plain = hod::compile(text, &options).unwrap_or_else(|e| panic!("hod {label}: {e}"));
    let stress = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    [plain, back_end_object(text, &stress)]
}

/// Runs `ll` (static or PIE) by `llc` and hod with every linker that
/// applies, against `llc`'s run with GNU ld. Answers the runs compared.
fn run_module(scratch: &Scratch, llc: &Path, ll: &Path, pic: bool, label: &str) -> usize {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return 0;
    };
    let text = std::fs::read(ll).unwrap();
    let reloc: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
    let [plain, stress] = hod_objects(&text, pic, label);
    let objects = [
        ("llc", llc_object(scratch, llc, ll, reloc, &format!("{label}-llc"))),
        ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
        ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
    ];
    let present = linkers::present(target);
    let mut reference = None;
    let mut runs = 0;
    for (who, object) in &objects {
        for linker in &present {
            if !pic && linker.0 != "bfd" {
                continue;
            }
            let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
            let mut command = Command::new(&gcc);
            let driver: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
            command.args(driver).arg(if pic { "-pie" } else { "-no-pie" }).arg(object);
            command.args(["-lm", "-o"]).arg(&exe);
            checked(&format!("link {label} {who} {}", linker.0), &mut command);
            let result = run(&qemu, &exe);
            let reference = reference.get_or_insert_with(|| result.clone());
            assert_eq!(result.0, Some(0), "{label}: {who} with {}: status", linker.0);
            assert!(result.1.ends_with(b"done\n"), "{label}: {who} did not finish");
            if result.1 != reference.1 {
                let (want, got) =
                    (String::from_utf8_lossy(&reference.1), String::from_utf8_lossy(&result.1));
                let diff: Vec<_> = want
                    .lines()
                    .zip(got.lines())
                    .filter(|(a, b)| a != b)
                    .take(12)
                    .map(|(a, b)| format!("  llc {a}\n  {who} {b}"))
                    .collect();
                panic!("{label}: {who} with {} differs from llc:\n{}", linker.0, diff.join("\n"));
            }
            runs += 1;
        }
    }
    runs
}

const OPS: [&str; 6] = ["sadd", "uadd", "ssub", "usub", "smul", "umul"];

/// The edge values of an `n`-bit operand: zero, one, the extremes of
/// both signs and their neighbours, the half-width powers around the
/// multiplication's overflow, a bit pattern.
fn edges(n: u32) -> Vec<u64> {
    let m: u128 = (1u128 << n) - 1;
    let half = 1u128 << (n / 2);
    let vals = [
        0,
        1,
        2,
        3,
        m,
        m - 1,
        m >> 1,
        (m >> 1) + 1,
        (m >> 1).wrapping_sub(1),
        (m >> 1) + 2,
        m / 3,
        half,
        half - 1,
        (1u128 << n.div_ceil(2)) + 1,
        half.wrapping_neg(),
        0x5A5A_5A5A_5A5A_5A5A,
    ];
    let mut out: Vec<u64> = vals.iter().map(|v| (v & m) as u64).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The generated module: per intrinsic and width a function looping over
/// all pairs of [`edges`], printing a checksum of the results and the
/// count of overflow flags.
fn overflow_module() -> String {
    let mut out = String::from(
        "target datalayout = \"e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128\"\n\
         target triple = \"i686-unknown-linux-gnu\"\n\
         @fmt = private constant [18 x i8] c\"%s %2d %16llx %d\\0A\\00\"\n\
         @done = private constant [5 x i8] c\"done\\00\"\n\
         declare i32 @printf(ptr, ...)\ndeclare i32 @puts(ptr)\n",
    );
    for op in OPS {
        writeln!(out, "@name.{op} = private constant [5 x i8] c\"{op}\\00\"").unwrap();
    }
    for n in 1..=64 {
        let vals = edges(n);
        let body: Vec<String> = vals.iter().map(|v| format!("i64 {v}")).collect();
        writeln!(out, "@v{n} = private constant [{} x i64] [{}]", vals.len(), body.join(", "))
            .unwrap();
    }
    for op in OPS {
        for n in 1..=64u32 {
            let k = edges(n).len();
            let (trunc, ext) = if n < 64 {
                (
                    format!("  %a = trunc i64 %a64 to i{n}\n  %b = trunc i64 %b64 to i{n}\n"),
                    format!("  %x64 = zext i{n} %x to i64\n"),
                )
            } else {
                (
                    "  %a = freeze i64 %a64\n  %b = freeze i64 %b64\n".into(),
                    "  %x64 = freeze i64 %x\n".into(),
                )
            };
            write!(
                out,
                "define internal void @t_{op}_{n}() #0 {{\n\
                 entry:\n  br label %loop\nloop:\n\
                 \x20 %idx = phi i32 [ 0, %entry ], [ %idx1, %loop ]\n\
                 \x20 %h = phi i64 [ 0, %entry ], [ %h3, %loop ]\n\
                 \x20 %f = phi i32 [ 0, %entry ], [ %f1, %loop ]\n\
                 \x20 %i = udiv i32 %idx, {k}\n  %j = urem i32 %idx, {k}\n\
                 \x20 %pa = getelementptr i64, ptr @v{n}, i32 %i\n\
                 \x20 %pb = getelementptr i64, ptr @v{n}, i32 %j\n\
                 \x20 %a64 = load i64, ptr %pa\n  %b64 = load i64, ptr %pb\n\
                 {trunc}\
                 \x20 %r = call {{i{n}, i1}} @llvm.{op}.with.overflow.i{n}(i{n} %a, i{n} %b)\n\
                 \x20 %x = extractvalue {{i{n}, i1}} %r, 0\n\
                 \x20 %o = extractvalue {{i{n}, i1}} %r, 1\n\
                 {ext}\
                 \x20 %o32 = zext i1 %o to i32\n  %f1 = add i32 %f, %o32\n\
                 \x20 %o64 = zext i1 %o to i64\n  %h1 = mul i64 %h, 1000003\n\
                 \x20 %h2 = xor i64 %h1, %x64\n  %h3 = add i64 %h2, %o64\n\
                 \x20 %idx1 = add i32 %idx, 1\n  %c = icmp ult i32 %idx1, {kk}\n\
                 \x20 br i1 %c, label %loop, label %out\nout:\n\
                 \x20 %p = call i32 (ptr, ...) @printf(ptr @fmt, ptr @name.{op}, i32 {n}, i64 %h3, i32 %f1)\n\
                 \x20 ret void\n}}\n\
                 declare {{i{n}, i1}} @llvm.{op}.with.overflow.i{n}(i{n}, i{n})\n",
                kk = k * k,
            )
            .unwrap();
        }
    }
    out.push_str("define i32 @main() #0 {\n");
    for op in OPS {
        for n in 1..=64 {
            writeln!(out, "  call void @t_{op}_{n}()").unwrap();
        }
    }
    out.push_str(
        "  %d = call i32 @puts(ptr @done)\n  ret i32 0\n}\nattributes #0 = { nounwind }\n",
    );
    out
}

/// `text` with every function at CPU `cpu` (`attributes #0`, or one
/// added to each `define` that has none).
fn at_cpu(text: &str, cpu: &str) -> String {
    if cpu == "generic" {
        return text.to_string();
    }
    let attr = format!("\"target-cpu\"=\"{cpu}\"");
    if text.contains("attributes #0 = {") {
        return text.replace("attributes #0 = {", &format!("attributes #0 = {{ {attr}"));
    }
    let mut out = text.replace(" nounwind {", " #9 {").replace(" nounwind\n{", " #9\n{");
    write!(out, "\nattributes #9 = {{ nounwind {attr} }}\n").unwrap();
    out
}

#[test]
fn aggregates_overflow_and_small_intrinsics_run_as_llc_s() {
    let scratch = Scratch::new("i386-agg");
    let generated = overflow_module();
    let hand = [
        ("agg", std::fs::read_to_string(data("agg.ll")).unwrap()),
        ("small", std::fs::read_to_string(data("small.ll")).unwrap()),
        ("overflow", generated),
    ];
    let (mut modules, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for pic in [false, true] {
            let kind = if pic { "pie" } else { "exe" };
            for (cpu, flags) in [("i586", &["-march=i586"][..]), ("i686", &[][..])] {
                let label = format!("ovf-{major}-{cpu}-{kind}");
                let mut all = vec![if pic { "-fPIE" } else { "-fno-pic" }];
                all.extend_from_slice(flags);
                let ll = clang_module(&scratch, &clang, &data("ovf.c"), &all, &label);
                runs += run_module(&scratch, &llc, &ll, pic, &label);
                modules += 1;
            }
            for (name, text) in &hand {
                for cpu in ["generic", "i686"] {
                    let label = format!("{name}-{major}-{cpu}-{kind}");
                    let ll = scratch.write(&format!("{label}.ll"), at_cpu(text, cpu));
                    runs += run_module(&scratch, &llc, &ll, pic, &label);
                    modules += 1;
                }
            }
        }
    }
    eprintln!("i386 agg: {modules} modules, {runs} runs equal to llc's");
}

/// The edge values hold what the generated module claims: every width's
/// extremes, sorted and distinct.
#[test]
fn the_edge_values_cover_each_width_s_extremes() {
    for n in 1..=64u32 {
        let e = edges(n);
        let m = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        assert!(e.contains(&0) && e.contains(&m) && e.contains(&(m >> 1)), "width {n}");
        assert!(e.windows(2).all(|w| w[0] < w[1]), "width {n}");
    }
}
