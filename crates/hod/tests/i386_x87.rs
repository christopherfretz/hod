//! WP-300, design 0008 I9: x87 floating point on `i686-unknown-linux-gnu`
//! (`spec/codegen-x86.md` "Floating point on the x87"). Programs are compiled by `llc` 19 and 22
//! (`-O0 -global-isel=0 -fast-isel=0`), by hod, and by `hod_x86` under
//! `hod_cg::Stress::ALL`, linked by GNU ld with the cross C library and
//! `-lm`, and run under `qemu-i386`: the outputs are equal bit for bit,
//! at two CPU levels (`generic`: no `cmov`, so `fucompp`/`fnstsw`/`sahf`
//! and branches for `select`; `pentiumpro`: `fucomip`, `fcmov`), both
//! without SSE. The linkers add nothing here (no relocation of this
//! package's own), so GNU ld alone links.
//!
//! - `arith.c`: every operation, compare, conversion and libm intrinsic of
//!   `float`, `double`, `long double` over edge values and a few hundred
//!   pseudo-random operands of a fixed generator; truncation toward zero
//!   and the rounding mode restored after it; the 80-bit `(a*b)+c`.
//! - the sixteen `fcmp` predicates (a module written here: C cannot
//!   spell most) over signed zeros, infinities, NaNs with payloads,
//!   subnormals, used by `zext`, an integer `select`, `br` and a floating
//!   `select`, also of an `fneg` result read through its 80-bit copy.
//! - `shapes-{float,double}.ll`: where a result stays at 80 bits (the
//!   spec's run rule: calls, inline and called `memcpy`/`memset`, `select`s
//!   that `llc` branches for, blocks, non-LIFO and far uses, the operations
//!   that pass 80 bits on). Every shape runs as `llc`'s but the `many*`
//!   ones, where `llc`'s allocator decides ([`divergent`]): those are
//!   compared with a model of hod's rule built by `llc` ([`model`]).
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
    TRIPLE, checked, clang_module, llc_object, llvm_tool, run, stressed_object, tool,
};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/x87").join(name)
}

/// `text` without the CPU attributes: the `generic` CPU.
fn generic(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = ["\"target-cpu\"=\"", "\"target-features\"=\"", "\"tune-cpu\"=\""]
        .iter()
        .filter_map(|k| rest.find(k).map(|i| (i, k.len())))
        .min()
    {
        out.push_str(&rest[..at.0]);
        let value = &rest[at.0 + at.1..];
        rest = &value[value.find('"').expect("closing quote") + 1..];
    }
    out.push_str(rest);
    out
}

/// The tools of a run: the cross C driver and the emulator.
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

/// A C object of the cross compiler (a driver).
fn driver(tools: &Tools, scratch: &Scratch, source: &str, defines: &[&str], name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(&tools.gcc);
    command.args(["-O0", "-c"]).args(defines).arg(data(source)).arg("-o").arg(&out);
    checked(&format!("cc {name}"), &mut command);
    out
}

/// The output of `objects` linked by GNU ld with `-lm` and run.
fn link_run(tools: &Tools, scratch: &Scratch, objects: &[&Path], name: &str) -> Vec<u8> {
    let exe = scratch.join(name);
    let mut command = Command::new(&tools.gcc);
    command.arg("-no-pie").args(objects).arg("-o").arg(&exe).arg("-lm");
    checked(&format!("link {name}"), &mut command);
    let (status, out) = run(&tools.qemu, &exe);
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
/// linked with `extra`: `[llc, hod, stress]`.
fn three_runs(
    tools: &Tools,
    scratch: &Scratch,
    llc: &Path,
    text: &str,
    extra: &[&Path],
    name: &str,
) -> [Vec<u8>; 3] {
    let ll = scratch.write(&format!("{name}.ll"), text.as_bytes());
    let llc_o = llc_object(scratch, llc, &ll, &[], &format!("{name}-llc"));
    let [hod_o, stress_o] = hod_objects(scratch, text, name);
    let mut outs = [&llc_o, &hod_o, &stress_o].map(|o| {
        let mut objects = vec![o.as_path()];
        objects.extend_from_slice(extra);
        let stem = o.file_stem().unwrap().to_string_lossy().into_owned();
        link_run(tools, scratch, &objects, &stem)
    });
    let stress = std::mem::take(&mut outs[2]);
    assert_eq!(
        String::from_utf8_lossy(&outs[1]),
        String::from_utf8_lossy(&stress),
        "{name}: hod under Stress::ALL runs differently from hod"
    );
    outs[2] = stress;
    outs
}

#[test]
fn arith_runs_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-x87-arith");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        let flags = ["-fno-pic", "-fno-pie", "-mno-sse", "-fno-math-errno", "-march=pentiumpro"];
        let ll =
            clang_module(&scratch, &clang, &data("arith.c"), &flags, &format!("arith-{major}"));
        let text = std::fs::read_to_string(&ll).unwrap();
        for (cpu, text) in [("pentiumpro", text.clone()), ("generic", generic(&text))] {
            let name = format!("arith-{major}-{cpu}");
            let [want, got, _] = three_runs(&tools, &scratch, &llc, &text, &[], &name);
            let got = String::from_utf8(got).unwrap();
            assert_eq!(String::from_utf8_lossy(&want), got, "{name}: hod's run differs from llc's");
            assert!(got.contains("chain 3c300000 0\n"), "{name}: (a*b)+c not at 80 bits:\n{got}");
            assert!(got.contains("nearest 3ff00000 2\n"), "{name}: rounding mode not restored");
            assert!(got.contains("trunc -2\ntrunc 2\ntrunc -2\n"), "{name}: truncation");
            runs += 3;
        }
    }
    eprintln!("i386 x87 arith: {runs} runs equal llc's");
}

/// The `fcmp` module: per type and predicate, `@cmp_<t>_<n>` and the
/// tables `@cmp_<t>`.
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
    let attr = if cpu == "generic" { String::new() } else { format!(" \"target-cpu\"=\"{cpu}\"") };
    writeln!(m, "attributes #0 = {{ nounwind{attr} }}").unwrap();
    m
}

#[test]
fn fcmp_predicates_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-x87-fcmp");
    let drv = driver(&tools, &scratch, "fcmp.c", &[], "fcmp-driver");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for cpu in ["generic", "pentiumpro"] {
            let name = format!("fcmp-{major}-{cpu}");
            let [want, got, _] =
                three_runs(&tools, &scratch, &llc, &fcmp_module(cpu), &[&drv], &name);
            let got = String::from_utf8(got).unwrap();
            assert_eq!(String::from_utf8_lossy(&want), got, "{name}: hod's run differs from llc's");
            // 3 types x 11 x 11 operand pairs.
            assert_eq!(got.lines().count(), 363, "{name}");
            runs += 3;
        }
    }
    eprintln!("i386 x87 fcmp: {runs} runs (16 predicates x 3 types x 121 pairs) equal llc's");
}

/// The shapes of `shapes-*.ll`, in order (the driver prints one line each).
const SHAPES: [&str; 45] = [
    "single",
    "twice",
    "twice_sep",
    "call",
    "call_between",
    "call_before",
    "block",
    "block_and_local",
    "stored",
    "stored_after",
    "nonlifo",
    "far_int",
    "far_fp",
    "many6_fifo",
    "many6_lifo",
    "many7_fifo",
    "many7_lifo",
    "many8_fifo",
    "many8_lifo",
    "many9_fifo",
    "many9_lifo",
    "many12_fifo",
    "many12_lifo",
    "sqrt_between",
    "frem_between",
    "chain_use_twice",
    "phi_use",
    "select_use",
    "cmp_twice",
    "select_var",
    "freeze_use",
    "fpext_use",
    "fptrunc_use",
    "sitofp_use",
    "sitofp_var",
    "fneg_use",
    "fmuladd",
    "intsel_between",
    "intsel_const",
    "memcpy_small",
    "memcpy_big",
    "memset_small",
    "memset_big",
    "fold_const",
    "frem_const_between",
];

/// The shapes where `llc`'s own result depends on its register
/// allocator, not on the IR (the spec's "The divergence"): six to twelve products live at once in
/// one block without a call. `llc` 19 and 22 keep some of them in x87
/// registers (80 bits) and spill others rounded to their type (which
/// ones follows its fast allocator's choices: measured, the first
/// product, and further the first and last ones in FIFO order, the
/// middle ones in LIFO order);
/// hod keeps every one at 80 bits (its run rule), so these lines compare
/// with a model of that rule, not with `llc`. (`many6_fifo`, where `llc`
/// spills nothing that is used again, compares with `llc`.)
fn divergent(shape: &str) -> bool {
    shape.starts_with("many") && shape != "many6_fifo"
}

/// The model of hod's rule for the divergent shapes, built by `llc`:
/// their loads extended to `x86_fp80`, the arithmetic at `x86_fp80`
/// (whose spills keep 80 bits), each store of the value rounded to `ty`
/// (`fptrunc`), as hod's home of an 80-bit result is.
fn model(text: &str, ty: &str) -> String {
    let mut out = String::new();
    let mut current: Option<&str> = None;
    let load = format!(" = load {ty}, ");
    let store = format!("  store {ty} %");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("define void @") {
            current = rest.split('(').next();
        }
        if !current.is_some_and(divergent) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let ops = [" = fmul ", " = fadd "];
        if let Some((name, rest)) = line.split_once(&load) {
            writeln!(
                out,
                "{name}.r{load}{rest}\n{name} = fpext {ty} {}.r to x86_fp80",
                name.trim()
            )
            .unwrap();
        } else if let Some(op) = ops.iter().find(|op| line.contains(*op)) {
            out.push_str(&line.replace(&format!("{op}{ty} "), &format!("{op}x86_fp80 ")));
            out.push('\n');
        } else if let Some(rest) = line.strip_prefix(&store) {
            let (v, ptr) = rest.split_once(", ").unwrap();
            writeln!(out, "  %{v}.t = fptrunc x86_fp80 %{v} to {ty}\n  store {ty} %{v}.t, {ptr}")
                .unwrap();
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `text` for `cpu`: the shapes files carry no CPU attribute.
fn with_cpu(text: &str, cpu: &str) -> String {
    if cpu == "generic" {
        return text.to_owned();
    }
    let mut t = text.replace("(ptr %in, ptr %out) nounwind {", "(ptr %in, ptr %out) #90 {");
    writeln!(t, "attributes #90 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}").unwrap();
    t
}

#[test]
fn rounding_shapes_run_as_llc_s_or_as_the_model() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-x87-shapes");
    let mut equal = 0;
    let mut modelled = 0;
    let mut failures = String::new();
    // The model lines that differ from `llc`'s (its allocator rounding).
    let mut apart = 0;
    for (ty, defines) in [("float", &["-DFLT"][..]), ("double", &[][..])] {
        let drv = driver(&tools, &scratch, "shapes.c", defines, &format!("shapes-{ty}-driver"));
        let source = std::fs::read_to_string(data(&format!("shapes-{ty}.ll"))).unwrap();
        for major in LLVM_MAJORS {
            let Some(llc) = llvm_tool(major, "llc") else { continue };
            for cpu in ["generic", "pentiumpro"] {
                let name = format!("shapes-{ty}-{major}-{cpu}");
                let text = with_cpu(&source, cpu);
                let [want, got, _] = three_runs(&tools, &scratch, &llc, &text, &[&drv], &name);
                let ll = scratch.write(&format!("{name}-model.ll"), model(&text, ty).as_bytes());
                let model_o = llc_object(&scratch, &llc, &ll, &[], &format!("{name}-model"));
                let modelled_out =
                    link_run(&tools, &scratch, &[&model_o, &drv], &format!("{name}-m"));
                let (want, got, by_model) = (
                    String::from_utf8(want).unwrap(),
                    String::from_utf8(got).unwrap(),
                    String::from_utf8(modelled_out).unwrap(),
                );
                let lines = |s: &str| s.lines().map(str::to_owned).collect::<Vec<_>>();
                let (want, got, by_model) = (lines(&want), lines(&got), lines(&by_model));
                assert_eq!(got.len(), SHAPES.len(), "{name}: {got:?}");
                for (k, shape) in SHAPES.iter().enumerate() {
                    let (reference, what) = if divergent(shape) {
                        modelled += 1;
                        apart += usize::from(want[k] != by_model[k]);
                        (&by_model[k], "the model")
                    } else {
                        equal += 1;
                        (&want[k], "llc")
                    };
                    if got[k] != *reference {
                        writeln!(failures, "{name} {shape}: hod {} != {what} {reference}", got[k])
                            .unwrap();
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "shapes that differ:\n{failures}");
    eprintln!(
        "i386 x87 shapes: {equal} equal llc's, {modelled} equal the model \
         ({apart} of them differ from llc's)"
    );
}

/// A `float` operation of clang's default i686 CPU (`pentium4`, SSE2) is
/// `llc`'s in `xmm` registers: refused by name.
#[test]
fn float_under_sse_is_refused() {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n\
         define void @f(ptr %p) #0 {{\n  %x = load float, ptr %p\n  %y = fadd float %x, %x\n\
         \x20 store float %y, ptr %p\n  ret void\n}}\n\
         attributes #0 = {{ nounwind \"target-cpu\"=\"pentium4\" }}\n"
    );
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let err = hod::compile(text.as_bytes(), &options).expect_err("SSE float must be refused");
    assert!(err.to_string().contains("x86:float:sse"), "{err}");
    // The same without SSE compiles.
    let plain = text.replace("\"target-cpu\"=\"pentium4\"", "");
    hod::compile(plain.as_bytes(), &options).expect("x87 float compiles");
}
