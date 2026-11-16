//! WP-317, design 0008 I12: scalar SSE floating point on
//! `i686-unknown-linux-gnu` (`spec/codegen-x86.md` "SSE"). Programs are
//! compiled by `llc` 19 and 22 (`-O0 -global-isel=0 -fast-isel=0`), by
//! hod, and by `hod_x86` under `hod_cg::Stress::ALL`, linked by GNU ld
//! with the cross C library and `-lm`, and run under `qemu-i386` with a
//! CPU that has SSE2: the outputs are equal bit for bit (`%a`), at
//! `pentium4` (SSE2: `float` and `double` in `xmm`) and `pentium3` (SSE:
//! `float` in `xmm`, `double` on the x87).
//!
//! - `ops.c` (`data/i386/sse/`): every scalar operation, compare and
//!   conversion (64-bit integers too), the libm calls, `long double`
//!   mixed in, calls into and back from `x87.c` (built by the cross C
//!   compiler without SSE), static and PIC.
//! - the x87 package's `arith.c` built for SSE.
//! - the sixteen `fcmp` predicates (`float`, `double`, `x86_fp80`).
//! - the x87 package's `shapes-{float,double}.ll` at the SSE CPUs: an
//!   SSE value has no 80-bit copy, so every shape runs as `llc`'s.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{
    TRIPLE, checked, clang_module, cpu_features, llc_object, llvm_tool, run_with_features,
    stressed_object, tool,
};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(name)
}

struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let gcc = tool(&target.c_driver())?;
    let qemu = tool(&target.emulator())?;
    Some(Tools { gcc, qemu })
}

/// A C object of the cross compiler (its default: x87, no SSE).
fn driver(tools: &Tools, scratch: &Scratch, source: &str, defines: &[&str], name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(&tools.gcc);
    command.args(["-O0", "-c"]).args(defines).arg(data(source)).arg("-o").arg(&out);
    checked(&format!("cc {name}"), &mut command);
    out
}

/// The output of `objects` linked by GNU ld with `-lm` and run on a CPU
/// with SSE2.
fn link_run(tools: &Tools, scratch: &Scratch, objects: &[&Path], name: &str) -> Vec<u8> {
    let exe = scratch.join(name);
    let mut command = Command::new(&tools.gcc);
    command.arg("-no-pie").args(objects).arg("-o").arg(&exe).arg("-lm");
    checked(&format!("link {name}"), &mut command);
    let (status, out) = run_with_features(&tools.qemu, &exe, cpu_features("p4"));
    assert_eq!(status, Some(0), "{name} exited with {status:?}");
    out
}

/// hod's object of `text` (the facade) and the `Stress::ALL` one.
fn hod_objects(scratch: &Scratch, text: &str, name: &str) -> [PathBuf; 2] {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let object =
        hod::compile(text.as_bytes(), &options).unwrap_or_else(|e| panic!("hod {name}: {e}"));
    [
        scratch.write(&format!("{name}-hod.o"), object),
        scratch.write(&format!("{name}-stress.o"), stressed_object(text.as_bytes())),
    ]
}

/// The outputs of module `text` by `llc`, hod and hod under stress, each
/// linked with `extra`: `[llc, hod]` (the stressed run equal to hod's).
fn runs(
    tools: &Tools,
    scratch: &Scratch,
    llc: &Path,
    text: &str,
    extra: &[&Path],
    name: &str,
) -> [String; 2] {
    let ll = scratch.write(&format!("{name}.ll"), text.as_bytes());
    let llc_o = llc_object(scratch, llc, &ll, &[], &format!("{name}-llc"));
    let [hod_o, stress_o] = hod_objects(scratch, text, name);
    let outs = [&llc_o, &hod_o, &stress_o].map(|o| {
        let mut objects = vec![o.as_path()];
        objects.extend_from_slice(extra);
        let stem = o.file_stem().unwrap().to_string_lossy().into_owned();
        String::from_utf8(link_run(tools, scratch, &objects, &stem)).unwrap()
    });
    assert_eq!(outs[1], outs[2], "{name}: hod under Stress::ALL runs differently from hod");
    let [want, got, _] = outs;
    [want, got]
}

/// The first line where `want` and `got` differ.
fn first_difference(want: &str, got: &str) -> String {
    let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
    for (k, (a, b)) in w.iter().zip(&g).enumerate() {
        if a != b {
            return format!("line {k}:\n  llc {a}\n  hod {b}");
        }
    }
    format!("{} lines against {}", w.len(), g.len())
}

/// The SSE CPUs and clang's flags for them.
const CPUS: [(&str, &[&str]); 2] = [
    ("pentium4", &["-march=pentium4", "-msse2", "-mfpmath=sse"]),
    ("pentium3", &["-march=pentium3", "-mfpmath=sse"]),
];

/// Whether clang wrote the CPU's attribute (a C test exercises SSE only
/// if it did).
fn check_cpu(text: &str, cpu: &str) {
    assert!(text.contains(&format!("\"target-cpu\"=\"{cpu}\"")), "clang wrote no {cpu}");
}

#[test]
fn ops_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-ops");
    let x87 = driver(&tools, &scratch, "sse/x87.c", &[], "x87");
    let mut runs_n = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for (cpu, flags) in CPUS {
            for pic in ["-fno-pic", "-fPIC"] {
                let name = format!("ops-{major}-{cpu}{pic}");
                let mut all = vec![pic, "-fno-math-errno"];
                all.extend_from_slice(flags);
                let ll = clang_module(&scratch, &clang, &data("sse/ops.c"), &all, &name);
                let text = std::fs::read_to_string(&ll).unwrap();
                check_cpu(&text, cpu);
                if cpu == "pentium3" && !hod_compiles(&text) {
                    eprintln!("{name}: refused at SSE without SSE2 (x86:float:sse1-fptrunc)");
                    continue;
                }
                let [want, got] = runs(&tools, &scratch, &llc, &text, &[&x87], &name);
                assert!(want == got, "{name}: {}", first_difference(&want, &got));
                assert!(got.contains("\nmixed "), "{name}: no mixed line");
                runs_n += 3;
            }
        }
    }
    eprintln!("i386 sse ops: {runs_n} runs equal llc's");
}

/// Whether hod compiles `text`; a refusal must be the one this package
/// keeps at SSE without SSE2.
fn hod_compiles(text: &str) -> bool {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    match hod::compile(text.as_bytes(), &options) {
        Ok(_) => true,
        Err(e) => {
            assert!(e.to_string().contains("x86:float:sse1-fptrunc"), "{e}");
            false
        }
    }
}

#[test]
fn x87_arith_runs_as_llc_s_with_sse() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-arith");
    let mut runs_n = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for (cpu, flags) in CPUS {
            let name = format!("arith-{major}-{cpu}");
            let mut all = vec!["-fno-pic", "-fno-pie", "-fno-math-errno"];
            all.extend_from_slice(flags);
            let ll = clang_module(&scratch, &clang, &data("x87/arith.c"), &all, &name);
            let text = std::fs::read_to_string(&ll).unwrap();
            check_cpu(&text, cpu);
            if cpu == "pentium3" && !hod_compiles(&text) {
                eprintln!("{name}: refused at SSE without SSE2 (x86:float:sse1-fptrunc)");
                continue;
            }
            let [want, got] = runs(&tools, &scratch, &llc, &text, &[], &name);
            assert!(want == got, "{name}: {}", first_difference(&want, &got));
            runs_n += 3;
        }
    }
    eprintln!("i386 sse arith: {runs_n} runs equal llc's");
}

/// The `fcmp` module of the x87 package's `fcmp.c` driver: per type and
/// predicate `@cmp_<t>_<n>`, its result used by `zext`, an integer and a
/// floating `select`, a branch, and of an `fneg` operand.
fn fcmp_module(cpu: &str) -> String {
    const PREDS: [&str; 16] = [
        "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult",
        "ule", "une", "uno", "true",
    ];
    let mut m = format!("target triple = \"{TRIPLE}\"\n\n");
    for (t, ty) in [("f", "float"), ("d", "double"), ("x", "x86_fp80")] {
        for (n, p) in PREDS.iter().enumerate() {
            writeln!(
                m,
                "define i32 @cmp_{t}_{n}(ptr %a, ptr %b, ptr %o) #0 {{\n\
                 entry:\n  %x = load {ty}, ptr %a\n  %y = load {ty}, ptr %b\n\
                 \x20 %c = fcmp {p} {ty} %x, %y\n  %z = zext i1 %c to i32\n\
                 \x20 %s = select i1 %c, i32 2, i32 0\n  %f = select i1 %c, {ty} %x, {ty} %y\n\
                 \x20 store {ty} %f, ptr %o\n  %n = fneg {ty} %x\n\
                 \x20 %c2 = fcmp {p} {ty} %n, %y\n  %z2 = zext i1 %c2 to i32\n\
                 \x20 %h = shl i32 %z2, 8\n  %r0 = or i32 %z, %s\n  %r1 = or i32 %r0, %h\n\
                 \x20 br i1 %c, label %t, label %e\n\
                 t:\n  %r = or i32 %r1, 4\n  ret i32 %r\n\
                 e:\n  ret i32 %r1\n}}\n"
            )
            .unwrap();
        }
        let list: Vec<String> = (0..16).map(|n| format!("ptr @cmp_{t}_{n}")).collect();
        writeln!(m, "@cmp_{t} = constant [16 x ptr] [{}]\n", list.join(", ")).unwrap();
    }
    writeln!(m, "attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}").unwrap();
    m
}

#[test]
fn fcmp_predicates_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-fcmp");
    let drv = driver(&tools, &scratch, "x87/fcmp.c", &[], "fcmp-driver");
    let mut runs_n = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for (cpu, _) in CPUS {
            let name = format!("fcmp-{major}-{cpu}");
            let [want, got] = runs(&tools, &scratch, &llc, &fcmp_module(cpu), &[&drv], &name);
            assert!(want == got, "{name}: {}", first_difference(&want, &got));
            assert_eq!(got.lines().count(), 363, "{name}");
            runs_n += 3;
        }
    }
    eprintln!("i386 sse fcmp: {runs_n} runs (16 predicates x 3 types x 121 pairs) equal llc's");
}

/// The x87 shapes file of `ty` at `cpu`.
fn with_cpu(text: &str, cpu: &str) -> String {
    let mut t = text.replace("(ptr %in, ptr %out) nounwind {", "(ptr %in, ptr %out) #90 {");
    writeln!(t, "attributes #90 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}").unwrap();
    t
}

#[test]
fn x87_shapes_run_as_llc_s_with_sse() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-shapes");
    let mut equal = 0;
    let mut failures = String::new();
    for (ty, defines) in [("float", &["-DFLT"][..]), ("double", &[][..])] {
        let drv = driver(&tools, &scratch, "x87/shapes.c", defines, &format!("shapes-{ty}-d"));
        let source = std::fs::read_to_string(data(&format!("x87/shapes-{ty}.ll"))).unwrap();
        for major in LLVM_MAJORS {
            let Some(llc) = llvm_tool(major, "llc") else { continue };
            for (cpu, _) in CPUS {
                // `double` at `pentium3` is an x87 type: the x87 package's.
                if ty == "double" && cpu == "pentium3" {
                    continue;
                }
                let name = format!("shapes-{ty}-{major}-{cpu}");
                let text = with_cpu(&source, cpu);
                let [want, got] = runs(&tools, &scratch, &llc, &text, &[&drv], &name);
                let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
                assert_eq!(w.len(), g.len(), "{name}");
                for (k, (a, b)) in w.iter().zip(&g).enumerate() {
                    if a == b {
                        equal += 1;
                    } else {
                        writeln!(failures, "{name} shape {k}: hod {b} != llc {a}").unwrap();
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "shapes that differ:\n{failures}");
    eprintln!("i386 sse shapes: {equal} equal llc's");
}

/// A function without SSE calling one with SSE2 and back, by attribute,
/// in one module: `float`/`double` through `st0` both ways, a `fastcc`
/// free C convention, `x86_stdcallcc` callee pop with float arguments.
const MIXED: &str = r#"
target triple = "i686-unknown-linux-gnu"
declare i32 @printf(ptr, ...)
@fmt = private constant [16 x i8] c"%a %a %a %a %a\0A\00"

define internal double @sse2(double %a, float %b, i32 %k) #1 {
  %e = fpext float %b to double
  %s = fadd double %a, %e
  %c = sitofp i32 %k to double
  %r = fdiv double %s, %c
  ret double %r
}
define internal float @plain(float %a, double %b) #0 {
  %t = fptrunc double %b to float
  %r = fmul float %a, %t
  ret float %r
}
define internal x86_stdcallcc double @popper(double %a, float %b) #1 {
  %e = fpext float %b to double
  %r = fsub double %a, %e
  ret double %r
}
define internal double @from_x87(double %a) #0 {
  %r = call double @sse2(double %a, float 0x3FB99999A0000000, i32 3)
  %m = fmul double %r, %a
  ret double %m
}
define i32 @main() #1 {
  %a = call double @from_x87(double 0x3FD3333333333333)
  %b = call float @plain(float 1.0e+01, double 0x3FB999999999999A)
  %bd = fpext float %b to double
  %c = call x86_stdcallcc double @popper(double 1.0e+00, float 0x3F50624DE0000000)
  %d = call double @sse2(double %c, float %b, i32 7)
  %x = fpext double %d to x86_fp80
  %y = fmul x86_fp80 %x, %x
  %yd = fptrunc x86_fp80 %y to double
  call i32 (ptr, ...) @printf(ptr @fmt, double %a, double %bd, double %c, double %d, double %yd)
  ret i32 0
}
attributes #0 = { nounwind "target-cpu"="i686" }
attributes #1 = { nounwind "target-cpu"="pentium4" }
"#;

#[test]
fn mixed_feature_calls_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-mixed");
    let mut runs_n = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        let name = format!("mixed-{major}");
        let [want, got] = runs(&tools, &scratch, &llc, MIXED, &[], &name);
        assert_eq!(want, got, "{name}");
        runs_n += 3;
    }
    eprintln!("i386 sse mixed: {runs_n} runs equal llc's");
}

#[test]
fn fptrunc_of_a_computed_double_at_sse_without_sse2_is_refused() {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n\
         define float @f(double %a, double %b) #0 {{\n\
         \x20 %m = fmul double %a, %b\n  %r = fptrunc double %m to float\n  ret float %r\n}}\n\
         define float @g(double %a) #0 {{\n  %r = fptrunc double %a to float\n  ret float %r\n}}\n\
         attributes #0 = {{ nounwind \"target-cpu\"=\"pentium3\" }}\n"
    );
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let err = hod::compile(text.as_bytes(), &options).expect_err("refused");
    assert!(err.to_string().contains("x86:float:sse1-fptrunc"), "{err}");
    // An argument has no 80-bit copy: `@g` alone compiles.
    let start = text.find("define float @g").unwrap();
    let only_g = format!("target triple = \"{TRIPLE}\"\n{}", &text[start..]);
    hod::compile(only_g.as_bytes(), &options).expect("an fptrunc of an argument compiles");
}
