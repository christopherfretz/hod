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
                let [want, got] = runs(&tools, &scratch, &llc, &text, &[&x87], &name);
                assert!(want == got, "{name}: {}", first_difference(&want, &got));
                assert!(got.contains("\nmixed "), "{name}: no mixed line");
                runs_n += 3;
            }
        }
    }
    eprintln!("i386 sse ops: {runs_n} runs equal llc's");
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
/// in one module: `float`/`double` through `st0` both ways, and a
/// callee-pop convention (`sret`: `retl $4`) with floating arguments.
const MIXED: &str = r#"
target triple = "i686-unknown-linux-gnu"
declare i32 @printf(ptr, ...)
@fmt = private constant [25 x i8] c"%a %a %a %a %a %a %a %a\0A\00"

define internal float @p3(double %a, double %b) #2 {
  %m = fmul double %a, %b
  %r = fptrunc double %m to float
  ret float %r
}

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
define internal void @popper(ptr sret(double) %o, double %a, float %b) #1 {
  %e = fpext float %b to double
  %r = fsub double %a, %e
  store double %r, ptr %o
  ret void
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
  %cs = alloca double
  call void @popper(ptr sret(double) %cs, double 1.0e+00, float 0x3F50624DE0000000)
  %c = load double, ptr %cs
  %d = call double @sse2(double %c, float %b, i32 7)
  %x = fpext double %d to x86_fp80
  %y = fmul x86_fp80 %x, %x
  %yd = fptrunc x86_fp80 %y to double
  %r = call double @from_x87(double 0x3FB999999999999A)
  %rt = fptrunc double %r to float
  %rtd = fpext float %rt to double
  %rx = fpext double %r to x86_fp80
  %rxd = fmul x86_fp80 %rx, 0xK3FFF8000000000000001
  %rxt = fptrunc x86_fp80 %rxd to double
  %e = call float @p3(double 0x3FB999999999999A, double 0x3FE6666666666666)
  %ed = fpext float %e to double
  call i32 (ptr, ...) @printf(ptr @fmt, double %a, double %bd, double %c, double %d, double %yd, double %rtd, double %rxt, double %ed)
  ret i32 0
}
attributes #0 = { nounwind "target-cpu"="i686" }
attributes #1 = { nounwind "target-cpu"="pentium4" }
attributes #2 = { nounwind "target-cpu"="pentium3" }
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

/// The fold of a call result's 80-bit value (`spec/codegen-x86.md` §12
/// "The 80-bit value of a call result"): x87 callees (`i686`) return
/// `1/3` and `(1 + 2^-24) + 2^-60` computed at 80 bits in `st0`; each
/// shape (at the SSE CPU) reads its call result through `fpext`,
/// `fptrunc`, chains of both and `ret`, across a call, a libm call, a
/// `memcpy`, a branch-lowered `select`, in another block, and through
/// uses that read the rounded value (`fneg`, `freeze`, `fadd`). Every
/// value is printed with `%a` / `%La` by an x87 `main`.
fn fold_module(cpu: &str, p3_x87: bool) -> String {
    // At `pentium3` these two read an `fpext` of the call result, an x87
    // `double`, in x87 arithmetic, where `llc` has the 80-bit value and
    // §11's copies do not (the FINDING of WP-317's record): run only by
    // the ignored test below.
    const P3_X87: [&str; 2] = ["trunc_other_add", "ext_sub"];
    // (name, return type of the shape or "void", body). A void shape
    // stores its doubles to `%o` (8 bytes apart) and `x86_fp80` values to
    // `%x` (16 bytes apart); a shape that returns is read by `main` at 80
    // bits (`fpext` of the call result in `main`'s x87 run).
    const SHAPES: &[(&str, &str, &str)] = &[
        (
            "ext",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to double\nstore double %e, ptr %o",
        ),
        (
            "ext_call",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\ncall void @g()\n%e = fpext float %r to double\nstore double %e, ptr %o",
        ),
        (
            "ext_libm",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%l = call float @floorf(float 2.5)\n%e = fpext float %r to double\nstore double %e, ptr %o\n%le = fpext float %l to double\n%o1 = getelementptr i8, ptr %o, i32 8\nstore double %le, ptr %o1",
        ),
        (
            "ext_memcpy",
            "void",
            "%a = alloca [1000 x i8]\n%b = alloca [1000 x i8]\ncall void @llvm.memset.p0.i32(ptr %b, i8 1, i32 1000, i1 false)\n%r = call float @df(float 1.0, float 3.0)\ncall void @llvm.memcpy.p0.p0.i32(ptr %a, ptr %b, i32 1000, i1 false)\n%e = fpext float %r to double\nstore double %e, ptr %o",
        ),
        (
            "ext_select",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%c = load volatile i1, ptr @flag\n%p = fadd float %r, 1.0\n%q = fsub float %r, 1.0\n%s = select i1 %c, float %p, float %q\n%sd = fpext float %s to double\n%e = fpext float %r to double\nstore double %e, ptr %o\n%o1 = getelementptr i8, ptr %o, i32 8\nstore double %sd, ptr %o1",
        ),
        (
            "ext_block",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\nbr label %next\nnext:\n%e = fpext float %r to double\nstore double %e, ptr %o",
        ),
        (
            "ext_and_use",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%s = fadd float %r, %r\n%e = fpext float %r to double\n%t = fpext float %s to double\n%u = fadd double %e, %t\nstore double %u, ptr %o\n%o1 = getelementptr i8, ptr %o, i32 8\nstore double %e, ptr %o1",
        ),
        (
            "ext_ld",
            "void",
            "%r = call double @dd(double 1.0, double 3.0)\n%e = fpext double %r to x86_fp80\nstore x86_fp80 %e, ptr %x",
        ),
        (
            "ext_ld_call",
            "void",
            "%r = call double @dd(double 1.0, double 3.0)\ncall void @g()\n%e = fpext double %r to x86_fp80\nstore x86_fp80 %e, ptr %x",
        ),
        (
            "ext_f_ld",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\ncall void @g()\n%e = fpext float %r to x86_fp80\nstore x86_fp80 %e, ptr %x",
        ),
        (
            "trunc",
            "void",
            "%r = call double @ad(double 0x3FF0000010000000, double 0x3C30000000000000)\n%t = fptrunc double %r to float\n%e = fpext float %t to double\nstore double %e, ptr %o",
        ),
        (
            "trunc_call",
            "void",
            "%r = call double @ad(double 0x3FF0000010000000, double 0x3C30000000000000)\ncall void @g()\n%t = fptrunc double %r to float\n%e = fpext float %t to double\nstore double %e, ptr %o",
        ),
        (
            "trunc_back",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to double\n%t = fptrunc double %e to float\n%y = fpext float %t to double\nstore double %y, ptr %o\n%o1 = getelementptr i8, ptr %o, i32 8\nstore double %e, ptr %o1",
        ),
        (
            "trunc_other",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to x86_fp80\n%t = fptrunc x86_fp80 %e to double\nstore double %t, ptr %o",
        ),
        (
            "trunc_other_d",
            "void",
            "%r = call double @ad(double 0x3FF0000010000000, double 0x3C30000000000000)\n%e = fpext double %r to x86_fp80\n%t = fptrunc x86_fp80 %e to float\n%y = fpext float %t to double\nstore double %y, ptr %o",
        ),
        (
            "trunc_other_add",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to x86_fp80\n%t = fptrunc x86_fp80 %e to double\n%s = fadd double %t, 0.0\nstore double %s, ptr %o",
        ),
        (
            "ext_sub",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to double\n%s = fsub double %e, 0x3FD5555555555555\nstore double %s, ptr %o",
        ),
        (
            "fneg",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%n = fneg float %r\n%e = fpext float %n to double\nstore double %e, ptr %o",
        ),
        (
            "freeze",
            "void",
            "%r = call float @df(float 1.0, float 3.0)\n%z = freeze float %r\n%e = fpext float %z to double\nstore double %e, ptr %o",
        ),
        ("ret", "float", "%r = call float @df(float 1.0, float 3.0)\nret float %r"),
        (
            "ret_call",
            "float",
            "%r = call float @df(float 1.0, float 3.0)\ncall void @g()\nret float %r",
        ),
        (
            "ret_ext",
            "double",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to double\nret double %e",
        ),
        (
            "ret_d",
            "double",
            "%r = call double @dd(double 1.0, double 3.0)\ncall void @g()\nret double %r",
        ),
        (
            "ret_back",
            "float",
            "%r = call float @df(float 1.0, float 3.0)\n%e = fpext float %r to double\n%t = fptrunc double %e to float\nret float %t",
        ),
        (
            "ret_block",
            "float",
            "%r = call float @df(float 1.0, float 3.0)\nbr label %next\nnext:\nret float %r",
        ),
    ];
    let mut m = format!(
        "target triple = \"{TRIPLE}\"\n\
         declare i32 @printf(ptr, ...)\ndeclare float @floorf(float)\n\
         declare void @llvm.memcpy.p0.p0.i32(ptr, ptr, i32, i1)\n\
         declare void @llvm.memset.p0.i32(ptr, i8, i32, i1)\n\
         @flag = global i1 true\n\
         @fd = private constant [10 x i8] c\"%s %d %a\\0A\\00\"\n\
         @fx = private constant [11 x i8] c\"%s %d %La\\0A\\00\"\n\
         define float @df(float %a, float %b) #1 {{\n  %q = fdiv float %a, %b\n  ret float %q\n}}\n\
         define double @dd(double %a, double %b) #1 {{\n  %q = fdiv double %a, %b\n  ret double %q\n}}\n\
         define double @ad(double %a, double %b) #1 {{\n  %q = fadd double %a, %b\n  ret double %q\n}}\n\
         define void @g() #1 {{\n  ret void\n}}\n"
    );
    let mut main = String::from("define i32 @main() #1 {\n");
    for (k, (name, ret, body)) in SHAPES.iter().enumerate() {
        if cpu == "pentium3" && P3_X87.contains(name) != p3_x87 {
            continue;
        }
        let body = body.replace('\n', "\n  ");
        let tail = if *ret == "void" { "\n  ret void" } else { "" };
        writeln!(m, "@n{k} = private constant [{}x i8] c\"{name}\\00\"", name.len() + 1).unwrap();
        writeln!(m, "define {ret} @s{k}(ptr %o, ptr %x) #0 {{\nentry:\n  {body}{tail}\n}}")
            .unwrap();
        writeln!(main, "  %o{k} = alloca [2 x double]\n  %x{k} = alloca [2 x x86_fp80]").unwrap();
        writeln!(main, "  call void @llvm.memset.p0.i32(ptr %o{k}, i8 0, i32 16, i1 false)")
            .unwrap();
        writeln!(main, "  call void @llvm.memset.p0.i32(ptr %x{k}, i8 0, i32 24, i1 false)")
            .unwrap();
        if *ret == "void" {
            writeln!(main, "  call void @s{k}(ptr %o{k}, ptr %x{k})").unwrap();
        } else {
            writeln!(main, "  %r{k} = call {ret} @s{k}(ptr %o{k}, ptr %x{k})").unwrap();
            writeln!(main, "  %e{k} = fpext {ret} %r{k} to x86_fp80").unwrap();
            writeln!(main, "  store x86_fp80 %e{k}, ptr %x{k}").unwrap();
        }
        for j in 0..2 {
            writeln!(
                main,
                "  %od{k}_{j} = getelementptr [2 x double], ptr %o{k}, i32 0, i32 {j}\n\
                 \x20 %vd{k}_{j} = load double, ptr %od{k}_{j}\n\
                 \x20 call i32 (ptr, ...) @printf(ptr @fd, ptr @n{k}, i32 {j}, double %vd{k}_{j})\n\
                 \x20 %ox{k}_{j} = getelementptr [2 x x86_fp80], ptr %x{k}, i32 0, i32 {j}\n\
                 \x20 %vx{k}_{j} = load x86_fp80, ptr %ox{k}_{j}\n\
                 \x20 call i32 (ptr, ...) @printf(ptr @fx, ptr @n{k}, i32 {j}, x86_fp80 %vx{k}_{j})"
            )
            .unwrap();
        }
    }
    m.push_str(&main);
    m.push_str("  ret i32 0\n}\n");
    writeln!(m, "attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}").unwrap();
    writeln!(m, "attributes #1 = {{ nounwind \"target-cpu\"=\"i686\" }}").unwrap();
    m
}

#[test]
fn call_result_folds_run_as_llc_s() {
    fold_runs(&CPUS.map(|c| c.0), false);
}

/// The FINDING: at `pentium3` an `fpext` of a `float` call result to the
/// x87 `double` is the 80-bit value in `llc` for every x87 use in the
/// block; hod's x87 operations read its home (rounded). Fixing it needs
/// `x87::push` (I9's) to read `sse`'s view.
#[test]
#[ignore = "pentium3: x87 uses of an fpext of an SSE call result read the rounded home (x87.rs)"]
fn call_result_folds_into_x87_doubles_at_pentium3() {
    fold_runs(&["pentium3"], true);
}

fn fold_runs(cpus: &[&str], p3_x87: bool) {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-sse-fold");
    let mut equal = 0;
    let mut failures = String::new();
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for &cpu in cpus {
            let name = format!("fold-{major}-{cpu}-{p3_x87}");
            let text = fold_module(cpu, p3_x87);
            let [want, got] = runs(&tools, &scratch, &llc, &text, &[], &name);
            let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
            assert_eq!(w.len(), g.len(), "{name}");
            for (a, b) in w.iter().zip(&g) {
                if a == b {
                    equal += 1;
                } else {
                    writeln!(failures, "{name}: hod {b} != llc {a}").unwrap();
                }
            }
        }
    }
    assert!(failures.is_empty(), "fold shapes that differ:\n{failures}");
    eprintln!("i386 sse fold: {equal} lines equal llc's");
}

#[test]
fn half_bfloat_fp128_and_vectors_stay_refused_with_sse2() {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    for (ty, name) in [
        ("half", "x86:float:half"),
        ("bfloat", "x86:float:bfloat"),
        ("fp128", "x86:float:fp128"),
        ("<4 x float>", "x86:vector"),
    ] {
        let text = format!(
            "target triple = \"{TRIPLE}\"\n\
             define void @f(ptr %p) #0 {{\n  %x = load {ty}, ptr %p\n\
             \x20 %y = fadd {ty} %x, %x\n  store {ty} %y, ptr %p\n  ret void\n}}\n\
             attributes #0 = {{ nounwind \"target-cpu\"=\"pentium4\" }}\n"
        );
        let err = hod::compile(text.as_bytes(), &options).expect_err(ty);
        assert!(err.to_string().contains(name), "{ty}: {err}");
    }
}
