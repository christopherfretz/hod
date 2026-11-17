//! WP-317, design 0008 I12 step C: vectors on `i686-unknown-linux-gnu`
//! with SSE2 (`spec/codegen-x86.md` §17 "Vectors"). `data/i386/vector/
//! lanes.ll` (integer and float lanes with edge values, masks, select,
//! shuffles, variable indices, casts, reductions, `i64` lanes, a call of
//! six vector arguments mixed with `i32` and `i64`) is built by `llc` 19
//! and 22 (`-O0 -global-isel=0 -fast-isel=0`), by hod and by hod under
//! `Stress::ALL`, static and PIC, and run under `qemu-i386` with SSE2:
//! the outputs are equal. The callee of six vectors is also built apart
//! and linked with the other side's caller (both ways). The refusals
//! keep their names.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{
    TRIPLE, checked, cpu_features, llc_object, llvm_tool, run_with_features, stressed_object, tool,
};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/vector").join(name)
}

struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    Some(Tools { gcc: tool(&target.c_driver())?, qemu: tool(&target.emulator())? })
}

fn options() -> Options {
    Options { output_kind: OutputKind::Object, triple: Some(TRIPLE.into()), ..Options::default() }
}

fn link_run(tools: &Tools, scratch: &Scratch, objects: &[&Path], name: &str, pic: bool) -> String {
    let exe = scratch.join(name);
    let mut command = Command::new(&tools.gcc);
    command.arg(if pic { "-pie" } else { "-no-pie" }).args(objects).arg("-o").arg(&exe);
    checked(&format!("link {name}"), &mut command);
    let (status, out) = run_with_features(&tools.qemu, &exe, cpu_features("p4"));
    assert_eq!(status, Some(0), "{name} exited with {status:?}");
    String::from_utf8(out).unwrap()
}

fn hod_object(scratch: &Scratch, text: &str, name: &str, pic: bool) -> PathBuf {
    let mut options = options();
    if pic {
        options.relocation_model = Some(hod::RelocModel::Pic);
    }
    let object =
        hod::compile(text.as_bytes(), &options).unwrap_or_else(|e| panic!("hod {name}: {e}"));
    scratch.write(&format!("{name}.o"), object)
}

/// The module split into the six-vector callee and the rest.
fn split(text: &str) -> (String, String) {
    let i = text.find("define <4 x i32> @five").unwrap();
    let j = text.find("define i32 @main").unwrap();
    let head = format!("target triple = \"{TRIPLE}\"\n");
    let attrs = "attributes #0 = { nounwind \"target-cpu\"=\"pentium4\" }\n";
    let callee = format!("{head}{}{attrs}", &text[i..j]);
    let decl = "declare <4 x i32> @five(<4 x i32>, i32, <2 x double>, i64, <4 x float>, \
                <16 x i8>, <8 x i16>)\n";
    let program = format!("{}{decl}{}", &text[..i], &text[j..]);
    (callee, program)
}

#[test]
fn lanes_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-vector-lanes");
    let text = std::fs::read_to_string(data("lanes.ll")).unwrap();
    let ll = data("lanes.ll");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for pic in [false, true] {
            let tag = format!("{major}-{}", if pic { "pic" } else { "static" });
            let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
            let llc_o = llc_object(&scratch, &llc, &ll, flags, &format!("llc-{tag}"));
            let want = link_run(&tools, &scratch, &[&llc_o], &format!("llc-{tag}"), pic);
            assert_eq!(want.lines().count(), 13, "{tag}: {want}");
            let hod_o = hod_object(&scratch, &text, &format!("hod-{tag}"), pic);
            let got = link_run(&tools, &scratch, &[&hod_o], &format!("hod-{tag}"), pic);
            assert_eq!(want, got, "{tag}: hod runs differently from llc");
            if !pic {
                let stress =
                    scratch.write(&format!("stress-{tag}.o"), stressed_object(text.as_bytes()));
                let s = link_run(&tools, &scratch, &[&stress], &format!("stress-{tag}"), pic);
                assert_eq!(want, s, "{tag}: hod under Stress::ALL runs differently");
            }
            runs += 1;
        }
        // The boundary both ways: hod callee with llc caller and back.
        let (callee, program) = split(&text);
        let callee_ll = scratch.write(&format!("callee-{major}.ll"), callee.as_bytes());
        let program_ll = scratch.write(&format!("program-{major}.ll"), program.as_bytes());
        let lc = llc_object(&scratch, &llc, &callee_ll, &[], &format!("callee-llc-{major}"));
        let lr = llc_object(&scratch, &llc, &program_ll, &[], &format!("program-llc-{major}"));
        let hc = hod_object(&scratch, &callee, &format!("callee-hod-{major}"), false);
        let hr = hod_object(&scratch, &program, &format!("program-hod-{major}"), false);
        let want = link_run(&tools, &scratch, &[&lc, &lr], &format!("ll-{major}"), false);
        for (a, b, name) in [(&hc, &lr, "hod-callee"), (&lc, &hr, "hod-caller")] {
            let got = link_run(&tools, &scratch, &[a, b], &format!("{name}-{major}"), false);
            assert_eq!(want, got, "{name} {major}: the boundary differs from llc's");
        }
        runs += 2;
    }
    eprintln!("i386 vector lanes: {runs} runs equal llc's");
}

/// One function of `body` at `cpu`, compiled: the error.
fn refusal(cpu: &str, decls: &str, body: &str) -> String {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n{decls}\n{body}\n\
         attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}\n"
    );
    hod::compile(text.as_bytes(), &options()).expect_err(body).to_string()
}

#[test]
fn vector_refusals_keep_their_names() {
    let p4 = "pentium4";
    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "pentium3",
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x float>, ptr %p\n \
             %y = fadd <4 x float> %x, %x\n store <4 x float> %y, ptr %p\n ret void\n}",
            "x86:vector:no-sse2",
        ),
        (
            // Without any SSE, §20 (WP-338) passes it lane by lane.
            "pentium3",
            "",
            "define <4 x i32> @f(<4 x i32> %a) #0 {\n ret <4 x i32> %a\n}",
            "x86:vector:no-sse2",
        ),
        (p4, "", "define <8 x i32> @f(<8 x i32> %a) #0 {\n ret <8 x i32> %a\n}", "x86:vector:wide"),
        (p4, "", "define <4 x i1> @f(<4 x i1> %a) #0 {\n ret <4 x i1> %a\n}", "x86:vector:abi"),
        (p4, "", "define <1 x i32> @f(<1 x i32> %a) #0 {\n ret <1 x i32> %a\n}", "x86:vector:abi"),
        (
            p4,
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x half>, ptr %p\n \
             store <4 x half> %x, ptr %p\n ret void\n}",
            "x86:vector:element",
        ),
        (
            p4,
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x float>, ptr %p\n \
             %y = frem <4 x float> %x, %x\n store <4 x float> %y, ptr %p\n ret void\n}",
            "x86:vector:frem",
        ),
        (
            p4,
            "",
            "define void @f(ptr %p) #0 {\n %x = load <2 x i64>, ptr %p\n \
             %y = mul <2 x i64> %x, %x\n store <2 x i64> %y, ptr %p\n ret void\n}",
            "x86:vector:i64-op",
        ),
        (
            p4,
            "",
            "define void @f(ptr %p) #0 {\n %x = load <4 x i32>, ptr %p\n \
             %y = uitofp <4 x i32> %x to <4 x float>\n store <4 x float> %y, ptr %p\n ret void\n}",
            "x86:vector:cast",
        ),
        (
            p4,
            "declare float @llvm.vector.reduce.fadd.v4f32(float, <4 x float>)",
            "define float @f(ptr %p) #0 {\n %x = load <4 x float>, ptr %p\n \
             %y = call float @llvm.vector.reduce.fadd.v4f32(float 0.0, <4 x float> %x)\n \
             ret float %y\n}",
            "x86:vector:intrinsic",
        ),
    ];
    for &(cpu, decls, body, name) in cases {
        let err = refusal(cpu, decls, body);
        assert!(err.contains(name), "{name} at {cpu}: {err}");
    }
}
