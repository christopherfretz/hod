//! The 32-bit x86 CPU table (`hod_ir::I386_CPUS`, WP-286) against `llc`
//! 19 and 22.
//!
//! - The table's names are exactly the CPUs `llc -mtriple=i686-linux-gnu
//!   -mcpu=help` lists for LLVM 22, and include LLVM 19's.
//! - Each CPU's features are those `llc` shows in code: one module with a
//!   set of probe functions per CPU, each set with `"target-cpu"` of that
//!   CPU (and `-fma4`, so that AMD's FMA4 and XOP instructions do not hide
//!   FMA and SSE4.2), compiled once per version. A feature is seen when
//!   its probe uses the instruction or register that needs it.
//! - `-mcpu` reaches a function without `target-cpu`, its own
//!   `target-cpu` wins over `-mcpu`, and `-mattr` comes after the
//!   function's own features, as [`X86Features::of_function`] says.

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hod_ir::{I386_CPUS, X86Features};
use hod_testutil::Scratch;

/// One probe: the function's body, and the feature its code shows by
/// containing one of `marks`.
struct Probe {
    name: &'static str,
    feature: X86Features,
    marks: &'static [&'static str],
    signature: &'static str,
    body: &'static str,
}

const PROBES: [Probe; 14] = [
    Probe {
        name: "sel",
        feature: X86Features::CMOV,
        marks: &["cmov"],
        signature: "i32 (i32 %a, i32 %b, i32 %c)",
        body: "%t = icmp slt i32 %a, %b\n  %r = select i1 %t, i32 %c, i32 %b\n  ret i32 %r",
    },
    Probe {
        name: "ld",
        feature: X86Features::X87,
        marks: &["fadd"],
        signature: "x86_fp80 (x86_fp80 %a, x86_fp80 %b)",
        body: "%r = fadd x86_fp80 %a, %b\n  ret x86_fp80 %r",
    },
    Probe {
        name: "cas",
        feature: X86Features::CX8,
        marks: &["cmpxchg8b"],
        signature: "i64 (ptr %p, i64 %a, i64 %b)",
        body: "%r = cmpxchg ptr %p, i64 %a, i64 %b seq_cst seq_cst\n  \
               %v = extractvalue {i64, i1} %r, 0\n  ret i64 %v",
    },
    Probe {
        name: "fs",
        feature: X86Features::SSE,
        marks: &["mulss"],
        signature: "float (float %a, float %b)",
        body: "%r = fmul float %a, %b\n  ret float %r",
    },
    Probe {
        name: "fd",
        feature: X86Features::SSE2,
        marks: &["mulsd"],
        signature: "double (double %a, double %b)",
        body: "%r = fmul double %a, %b\n  ret double %r",
    },
    Probe {
        name: "hadd",
        feature: X86Features::SSE3,
        marks: &["movddup", "haddpd"],
        signature: "<2 x double> (<2 x double> %a)",
        body: "%l = shufflevector <2 x double> %a, <2 x double> poison, <2 x i32> zeroinitializer\n  \
               %h = shufflevector <2 x double> %a, <2 x double> poison, <2 x i32> <i32 1, i32 1>\n  \
               %r = fadd <2 x double> %l, %h\n  ret <2 x double> %r",
    },
    Probe {
        name: "abs",
        feature: X86Features::SSSE3,
        marks: &["pabsd"],
        signature: "<4 x i32> (<4 x i32> %a)",
        body: "%r = call <4 x i32> @llvm.abs.v4i32(<4 x i32> %a, i1 false)\n  ret <4 x i32> %r",
    },
    Probe {
        name: "mull",
        feature: X86Features::SSE4_1,
        marks: &["pmulld"],
        signature: "<4 x i32> (<4 x i32> %a, <4 x i32> %b)",
        body: "%r = mul <4 x i32> %a, %b\n  ret <4 x i32> %r",
    },
    Probe {
        name: "cmpgt",
        feature: X86Features::SSE4_2,
        marks: &["pcmpgtq"],
        signature: "<2 x i64> (<2 x i64> %a, <2 x i64> %b)",
        body: "%c = icmp sgt <2 x i64> %a, %b\n  %r = sext <2 x i1> %c to <2 x i64>\n  \
               ret <2 x i64> %r",
    },
    Probe {
        name: "pop",
        feature: X86Features::POPCNT,
        marks: &["popcnt"],
        signature: "i32 (i32 %a)",
        body: "%r = call i32 @llvm.ctpop.i32(i32 %a)\n  ret i32 %r",
    },
    Probe {
        name: "v8",
        feature: X86Features::AVX,
        marks: &["ymm"],
        signature: "<8 x float> (<8 x float> %a, <8 x float> %b)",
        body: "%r = fadd <8 x float> %a, %b\n  ret <8 x float> %r",
    },
    Probe {
        name: "v8i",
        feature: X86Features::AVX2,
        marks: &["vpaddd\t%ymm"],
        signature: "<8 x i32> (<8 x i32> %a, <8 x i32> %b)",
        body: "%r = add <8 x i32> %a, %b\n  ret <8 x i32> %r",
    },
    Probe {
        name: "fma",
        feature: X86Features::FMA,
        marks: &["vfmadd1", "vfmadd2", "vfmadd3"],
        signature: "double (double %a, double %b, double %c)",
        body: "%r = call double @llvm.fma.f64(double %a, double %b, double %c)\n  ret double %r",
    },
    Probe {
        name: "v16",
        feature: X86Features::AVX512F,
        marks: &["zmm"],
        signature: "<16 x float> (<16 x float> %a, <16 x float> %b)",
        body: "%r = fadd <16 x float> %a, %b\n  ret <16 x float> %r",
    },
];

const DECLARATIONS: &str = "declare <4 x i32> @llvm.abs.v4i32(<4 x i32>, i1)\n\
     declare i32 @llvm.ctpop.i32(i32)\n\
     declare double @llvm.fma.f64(double, double, double)\n";

const HEADER: &str = "target datalayout = \
     \"e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128\"\n\
     target triple = \"i686-unknown-linux-gnu\"\n";

/// The probe functions of `prefix`, with attribute group `group` (or none).
fn probes(out: &mut String, prefix: &str, group: Option<usize>) {
    let attrs = group.map(|g| format!(" #{g}")).unwrap_or_default();
    for probe in &PROBES {
        let (ret, params) = probe.signature.split_once(" (").expect("a signature");
        let _ = writeln!(
            out,
            "define {ret} @{prefix}_{}({params}{attrs} {{\n  {}\n}}",
            probe.name, probe.body
        );
    }
}

/// `llc`'s assembly of `ir`, function by function.
fn functions(
    tools: &common::Tools,
    scratch: &Scratch,
    ir: &str,
    args: &[&str],
) -> BTreeMap<String, String> {
    let input = scratch.write("cpus.ll", ir);
    let mut command = std::process::Command::new(&tools.llc);
    command.args(["-O0", "-o", "-"]).args(args).arg(&input);
    let outcome = hod_testutil::run_with(&mut command, &[], hod_testutil::DEFAULT_TIMEOUT);
    assert!(outcome.success(), "llc {}: {}", tools.major, outcome.stderr_text());
    let mut map = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in outcome.stdout_text().lines() {
        if let Some(label) = line.split_whitespace().next().and_then(|w| w.strip_suffix(':'))
            && !label.starts_with('.')
            && !line.starts_with(char::is_whitespace)
        {
            current = Some(label.to_owned());
            continue;
        }
        if line.starts_with(".Lfunc_end") {
            current = None;
        }
        if let Some(name) = &current {
            let text: &mut String = map.entry(name.clone()).or_default();
            text.push_str(line);
            text.push('\n');
        }
    }
    map
}

/// What `prefix`'s probes show.
fn seen(code: &BTreeMap<String, String>, prefix: &str) -> X86Features {
    PROBES.iter().fold(X86Features::NONE, |set, probe| {
        let body = code
            .get(&format!("{prefix}_{}", probe.name))
            .unwrap_or_else(|| panic!("{prefix}_{} is not in llc's output", probe.name));
        if probe.marks.iter().any(|mark| body.contains(mark)) {
            set.union(probe.feature)
        } else {
            set
        }
    })
}

fn cpu_list(tools: &common::Tools) -> Vec<String> {
    let mut command = std::process::Command::new(&tools.llc);
    command.args(["-mtriple=i686-unknown-linux-gnu", "-mcpu=help"]);
    let outcome = hod_testutil::run_with(&mut command, &[], hod_testutil::DEFAULT_TIMEOUT);
    let text = format!("{}{}", outcome.stdout_text(), outcome.stderr_text());
    let start = text.find("Available CPUs for this target:").expect("llc's CPU list");
    let end = text.find("Available features for this target:").expect("llc's feature list");
    text[start..end]
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

fn check(major: u32) {
    let Some(tools) = common::tools(major) else { return };
    let listed = cpu_list(&tools);
    let names: Vec<&str> = I386_CPUS.iter().map(|&(name, _)| name).collect();
    for cpu in &listed {
        assert!(names.contains(&cpu.as_str()), "llc {major} lists {cpu}, the table does not");
    }
    if major >= 22 {
        assert_eq!(listed.len(), names.len(), "llc {major}'s CPUs are the table's");
    }

    let mut ir = String::from(HEADER);
    let mut groups = String::new();
    let mut checked = Vec::new();
    for (index, &(cpu, features)) in I386_CPUS.iter().enumerate() {
        if !listed.iter().any(|name| name == cpu) {
            continue;
        }
        probes(&mut ir, &format!("c{index}"), Some(index));
        let _ = writeln!(
            groups,
            "attributes #{index} = {{ \"target-cpu\"=\"{cpu}\" \"target-features\"=\"-fma4\" }}"
        );
        checked.push((index, cpu, features));
    }
    ir.push_str(DECLARATIONS);
    ir.push_str(&groups);
    let scratch = Scratch::new(&format!("x86-cpus-{major}"));
    let code = functions(&tools, &scratch, &ir, &[]);
    let mut failures = Vec::new();
    for (index, cpu, features) in &checked {
        let llc = seen(&code, &format!("c{index}"));
        if llc != *features {
            failures.push(format!("{cpu}: hod {features:?}, llc {major} {llc:?}"));
        }
    }
    eprintln!("x86_cpus llc {major}: {} CPUs, {} probes each", checked.len(), PROBES.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cpu_table_matches_llc22() {
    check(22);
}

#[test]
fn cpu_table_matches_llc19() {
    check(19);
}

/// `-mcpu` for a function with no `target-cpu`, the function's own first;
/// `-mattr` after the function's `target-features`.
#[test]
fn mcpu_and_mattr_as_llc_applies_them() {
    let Some(tools) = common::tools(22) else { return };
    let mut ir = String::from(HEADER);
    probes(&mut ir, "plain", None);
    probes(&mut ir, "own", Some(0));
    probes(&mut ir, "minus", Some(1));
    ir.push_str(DECLARATIONS);
    ir.push_str(
        "attributes #0 = { \"target-cpu\"=\"i686\" \"target-features\"=\"+cmov,+cx8,+x87\" }\n",
    );
    ir.push_str("attributes #1 = { \"target-cpu\"=\"core2\" \"target-features\"=\"-ssse3\" }\n");
    let scratch = Scratch::new("x86-cpus-options");
    for (mcpu, mattr) in [
        (None, None),
        (Some("pentium4"), None),
        (Some("haswell"), Some("-avx,+popcnt")),
        (None, Some("+sse2")),
        (Some("pentium4"), Some("-cmov")),
        (Some("i686"), Some("-cmov,-cx8")),
        (Some("skx"), Some("-avx2")),
    ] {
        let mut args = vec![];
        let (cpu_arg, attr_arg) =
            (mcpu.map(|c| format!("-mcpu={c}")), mattr.map(|a| format!("-mattr={a}")));
        args.extend(cpu_arg.as_deref());
        args.extend(attr_arg.as_deref());
        let code = functions(&tools, &scratch, &ir, &args);
        let mattr: Vec<String> = mattr.map(|a| vec![a.to_owned()]).unwrap_or_default();
        for (prefix, cpu, own) in [
            ("plain", None, None),
            ("own", Some("i686"), Some("+cmov,+cx8,+x87")),
            ("minus", Some("core2"), Some("-ssse3")),
        ] {
            let hod = X86Features::of_function(cpu, own, mcpu, &mattr).unwrap();
            assert_eq!(hod, seen(&code, prefix), "{prefix} with {args:?}");
        }
    }
}
