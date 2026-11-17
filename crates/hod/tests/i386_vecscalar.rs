//! WP-338: vectors on `i686-unknown-linux-gnu` in a function without SSE
//! (`spec/codegen-x86.md` §20 "Vectors without SSE").
//! `data/i386/vecscalar/lanes.c` (integer, `i64`, `float` and `double`
//! lanes with edge values, compares, inserts of wide lanes and at
//! variable indices, a shuffle) and `edge.c` (vectors passed lane by
//! lane, returned in `eax`/`edx`/`ecx`, `st0` and memory) are built by
//! clang 19 and 22 at `-O0` for clang's default i686 CPU (`cmov`, no SSE)
//! and for `i586` (no `cmov`); each module is compiled by `llc` (`-O0
//! -global-isel=0 -fast-isel=0`) and by hod, static and PIE, linked and
//! run under `qemu-i386 -cpu pentium2,-mmx`: the outputs are equal, also
//! with hod's callee and `llc`'s caller (and back) and under
//! `Stress::ALL`. The refusals keep their names.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/vecscalar").join(name)
}

fn options(pic: bool) -> Options {
    Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: pic.then_some(hod::RelocModel::Pic),
        ..Options::default()
    }
}

fn link_run(
    gcc: &Path,
    qemu: &Path,
    scratch: &Scratch,
    objects: &[&Path],
    name: &str,
    pic: bool,
) -> String {
    let exe = scratch.join(name);
    let mut command = Command::new(gcc);
    command.arg(if pic { "-pie" } else { "-no-pie" }).args(objects).arg("-o").arg(&exe);
    checked(&format!("link {name}"), &mut command);
    let (status, out) = run(qemu, &exe);
    assert_eq!(status, Some(0), "{name} exited with {status:?}");
    String::from_utf8(out).unwrap()
}

#[test]
fn lanes_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-vecscalar-lanes");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for march in ["i686", "i586"] {
            for pic in [false, true] {
                let tag = format!("{major}-{march}-{}", if pic { "pic" } else { "static" });
                let mut objects = Vec::new();
                for part in ["lanes", "edge"] {
                    let flags = [
                        &format!("-march={march}") as &str,
                        if pic { "-fPIE" } else { "-fno-pic" },
                    ];
                    let ll = clang_module(
                        &scratch,
                        &clang,
                        &data(&format!("{part}.c")),
                        &flags,
                        &format!("{part}-{tag}"),
                    );
                    let llc_flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
                    let l =
                        llc_object(&scratch, &llc, &ll, llc_flags, &format!("{part}-llc-{tag}"));
                    let text = std::fs::read(&ll).unwrap();
                    let h = hod::compile(&text, &options(pic))
                        .unwrap_or_else(|e| panic!("hod {part} {tag}: {e}"));
                    let h = scratch.write(&format!("{part}-hod-{tag}.o"), h);
                    let s = (!pic).then(|| {
                        scratch.write(&format!("{part}-stress-{tag}.o"), stressed_object(&text))
                    });
                    objects.push((l, h, s));
                }
                let [(ll, lh, ls), (el, eh, es)] = &objects[..] else { unreachable!() };
                let want = link_run(&gcc, &qemu, &scratch, &[ll, el], &format!("llc-{tag}"), pic);
                assert_eq!(want.lines().count(), 53, "{tag}: {want}");
                let got = link_run(&gcc, &qemu, &scratch, &[lh, eh], &format!("hod-{tag}"), pic);
                assert_eq!(want, got, "{tag}: hod runs differently from llc");
                for (a, b, name) in [(ll, eh, "hod-callee"), (lh, el, "hod-caller")] {
                    let got =
                        link_run(&gcc, &qemu, &scratch, &[a, b], &format!("{name}-{tag}"), pic);
                    assert_eq!(want, got, "{name} {tag}: the boundary differs from llc's");
                }
                if let (Some(ls), Some(es)) = (ls, es) {
                    let got =
                        link_run(&gcc, &qemu, &scratch, &[ls, es], &format!("stress-{tag}"), pic);
                    assert_eq!(want, got, "{tag}: hod under Stress::ALL runs differently");
                }
                runs += 1;
            }
        }
    }
    eprintln!("i386 vectors without SSE: {runs} configurations equal llc's");
}

/// One function of `body` at `cpu`, compiled: the outcome.
fn compile(cpu: &str, decls: &str, body: &str) -> Result<Vec<u8>, String> {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n{decls}\n{body}\n\
         attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}\n"
    );
    hod::compile(text.as_bytes(), &options(false)).map_err(|e| e.to_string())
}

#[test]
fn the_reduced_libcxx_module_compiles() {
    // `.build/findings/libcxx-i686/vector-no-sse2-v4i32.ll`.
    let body = "define void @f(ptr %p, ptr %q) #0 {\n %v = load <4 x i32>, ptr %p, align 16\n \
                store <4 x i32> %v, ptr %q, align 16\n ret void\n}";
    for cpu in ["i686", "generic", "i586"] {
        compile(cpu, "", body).unwrap_or_else(|e| panic!("{cpu}: {e}"));
    }
}

#[test]
fn refusals_keep_their_names() {
    let load2 = "%a = load <2 x double>, ptr %p\n %b = load <2 x double>, ptr %q\n";
    let cases: &[(&str, &str, String, &str)] = &[
        (
            "pentium3",
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x float>, ptr %p\n \
             %y = fadd <4 x float> %x, %x\n store <4 x float> %y, ptr %p\n ret void\n}"
                .into(),
            "x86:vector:no-sse2",
        ),
        (
            "i686",
            "",
            format!(
                "define void @f(ptr %p, ptr %q) #0 {{\n {load2} %s = fadd <2 x double> %a, %b\n \
                 %d = fsub <2 x double> %s, %a\n store <2 x double> %d, ptr %p\n ret void\n}}"
            ),
            "x86:vector:no-sse2:excess-precision",
        ),
        (
            "i686",
            "",
            format!(
                "define double @f(ptr %p, ptr %q) #0 {{\n {load2} %s = fmul <2 x double> %a, %b\n \
                 %e = extractelement <2 x double> %s, i32 1\n ret double %e\n}}"
            ),
            "x86:vector:no-sse2:excess-precision",
        ),
        (
            "i686",
            "",
            "define <1 x double> @f(<1 x double> %a) #0 {\n %s = fadd <1 x double> %a, %a\n \
             ret <1 x double> %s\n}"
                .into(),
            "x86:vector:no-sse2:excess-precision",
        ),
        (
            "i686",
            "",
            "define void @f(ptr %p) #0 {\n %s = fadd <2 x double> <double 1.0, double 2.0>, \
             <double 3.0, double 4.0>\n store <2 x double> %s, ptr %p\n ret void\n}"
                .into(),
            "x86:vector:no-sse2:fold",
        ),
        (
            "i686",
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x i32>, ptr %p\n \
             %y = sitofp <4 x i32> %x to <4 x float>\n store <4 x float> %y, ptr %p\n ret void\n}"
                .into(),
            "x86:vector:no-sse2:cast",
        ),
        (
            "generic",
            "",
            "define void @f(ptr %p, i1 %c) #0 {\n %x = load <4 x i32>, ptr %p\n \
             %y = select i1 %c, <4 x i32> %x, <4 x i32> zeroinitializer\n \
             store <4 x i32> %y, ptr %p\n ret void\n}"
                .into(),
            "x86:vector:no-sse2:select",
        ),
        (
            "generic",
            "declare i32 @llvm.vector.reduce.umax.v4i32(<4 x i32>)",
            "define i32 @f(ptr %p) #0 {\n %x = load <4 x i32>, ptr %p\n \
             %y = call i32 @llvm.vector.reduce.umax.v4i32(<4 x i32> %x)\n ret i32 %y\n}"
                .into(),
            "x86:vector:no-sse2:reduce",
        ),
        (
            "i686",
            "",
            "define void @f(<4 x i1> %a, ptr %p) #0 {\n store <4 x i1> %a, ptr %p\n ret void\n}"
                .into(),
            "x86:vector:no-sse2:abi",
        ),
        (
            "i686",
            "",
            "define <2 x float> @f(ptr %p) #0 {\n %x = load <2 x float>, ptr %p\n \
             ret <2 x float> %x\n}"
                .into(),
            "x86:abi:x87-pair",
        ),
    ];
    for (cpu, decls, body, name) in cases {
        let err = compile(cpu, decls, body).expect_err(body);
        assert!(err.contains(name), "{name} at {cpu}: {err}");
    }
}
