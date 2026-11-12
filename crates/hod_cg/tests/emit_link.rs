//! Link and run (`spec/codegen.md` §9.7): a data-heavy module (variables
//! of every section kind, pointers, differences, TLS, COMDAT, common,
//! weak, aliases, an IFUNC, constructors with priorities) is emitted by
//! `hod` and, as the reference, compiled by `llc`. Each object is linked
//! with an `llc`-compiled `main` that prints the data, by GNU ld, lld and
//! mold, and run: `hod`'s program must print what `llc`'s prints.
//!
//! The module's few functions carry `llc`'s code (`borrow`), placed and
//! relocated by `hod`.

mod borrow;
mod elfview;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel, emit_module};
use hod_ir::{AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};
use linking::{Toolchain, exit_code};

const FLAGS: &str = r#"
!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
"#;

const DATA: &str = r#"
$cd = comdat any
%S = type { i8, i64, i16 }
@i32 = global i32 42
@i64 = global i64 -7
@zero = global i32 0
@local = internal global i32 5
@str = private unnamed_addr constant [6 x i8] c"hello\00"
@pstr = global ptr @str
@pstr2 = global ptr getelementptr (i8, ptr @str, i64 1)
@str16 = unnamed_addr constant [3 x i16] [i16 104, i16 105, i16 0]
@ro = constant i32 77
@roptr = constant ptr @i32
@arr = global [3 x i32] [i32 1, i32 2, i32 3]
@parr = global ptr getelementptr (i8, ptr @arr, i64 8)
@plocal = global ptr @local
@st = global %S { i8 3, i64 4, i16 5 }
@pst = constant ptr getelementptr (i8, ptr @st, i64 8)
@cd = linkonce_odr global i32 9, comdat
@com = common global i32 0, align 4
@wk = weak global i32 11
@t = thread_local global i32 13
@tz = thread_local global i32 0
@tl = internal thread_local global i32 21
@al = alias i32, ptr @i32
@al2 = alias i32, getelementptr (i8, ptr @arr, i64 4)
@rel = constant i32 trunc (i64 sub (i64 ptrtoint (ptr @i32 to i64), i64 ptrtoint (ptr @rel to i64)) to i32)
@fold = global i64 sub (i64 ptrtoint (ptr getelementptr (i8, ptr @arr, i64 8) to i64), i64 ptrtoint (ptr @arr to i64))
@cst8 = unnamed_addr constant i64 1234567890123
@big = global [1000 x i64] zeroinitializer
@sec = global i32 99, section "mydata"
@log = global i32 0
@ext = external global i32
@pext = global ptr @ext
@w = extern_weak global i32
@pw = global ptr @w
@pf = global ptr @impl

define internal i32 @impl() {
  ret i32 42
}
define ptr @resolver() {
  ret ptr @impl
}
@answer = ifunc i32 (), ptr @resolver
define internal void @ctor_a() {
  %v = load i32, ptr @log
  %m = mul i32 %v, 10
  %a = add i32 %m, 1
  store i32 %a, ptr @log
  ret void
}
define internal void @ctor_b() {
  %v = load i32, ptr @log
  %m = mul i32 %v, 10
  %a = add i32 %m, 2
  store i32 %a, ptr @log
  ret void
}
define i32 @read_tl() {
  %v = load i32, ptr @tl
  ret i32 %v
}
@llvm.global_ctors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @ctor_b, ptr null }, { i32, ptr, ptr } { i32 101, ptr @ctor_a, ptr null }]
"#;

/// What `main` prints: a label and an `i64` expression over the data.
const PROBES: &[(&str, &str)] = &[
    ("i32", "%v = load i32, ptr @i32\n%x = sext i32 %v to i64"),
    ("i64", "%x = load i64, ptr @i64"),
    ("zero", "%v = load i32, ptr @zero\n%x = sext i32 %v to i64"),
    ("pstr", "%p = load ptr, ptr @pstr\n%c = load i8, ptr %p\n%x = sext i8 %c to i64"),
    ("pstr2", "%p = load ptr, ptr @pstr2\n%c = load i8, ptr %p\n%x = sext i8 %c to i64"),
    ("str16", "%p = getelementptr i8, ptr @str16, i64 2\n%c = load i16, ptr %p\n%x = sext i16 %c to i64"),
    ("ro", "%v = load i32, ptr @ro\n%x = sext i32 %v to i64"),
    ("roptr", "%p = load ptr, ptr @roptr\n%e = icmp eq ptr %p, @i32\n%x = zext i1 %e to i64"),
    ("parr", "%p = load ptr, ptr @parr\n%v = load i32, ptr %p\n%x = sext i32 %v to i64"),
    ("plocal", "%p = load ptr, ptr @plocal\n%v = load i32, ptr %p\n%x = sext i32 %v to i64"),
    ("pst", "%p = load ptr, ptr @pst\n%x = load i64, ptr %p"),
    ("st.c", "%p = getelementptr i8, ptr @st, i64 16\n%v = load i16, ptr %p\n%x = sext i16 %v to i64"),
    ("cd", "%v = load i32, ptr @cd\n%x = sext i32 %v to i64"),
    ("com", "%v = load i32, ptr @com\n%x = sext i32 %v to i64"),
    ("wk", "%v = load i32, ptr @wk\n%x = sext i32 %v to i64"),
    ("t", "%v = load i32, ptr @t\n%x = sext i32 %v to i64"),
    ("tz", "%v = load i32, ptr @tz\n%x = sext i32 %v to i64"),
    ("tl", "%v = call i32 @read_tl()\n%x = sext i32 %v to i64"),
    ("al", "%e = icmp eq ptr @al, @i32\n%x = zext i1 %e to i64"),
    ("al2", "%v = load i32, ptr @al2\n%x = sext i32 %v to i64"),
    (
        "rel",
        "%r = load i32, ptr @rel\n%r64 = sext i32 %r to i64\n%b = ptrtoint ptr @rel to i64\n%s = add i64 %b, %r64\n%t = ptrtoint ptr @i32 to i64\n%x = sub i64 %s, %t",
    ),
    ("fold", "%x = load i64, ptr @fold"),
    ("cst8", "%x = load i64, ptr @cst8"),
    ("big", "%p = getelementptr i8, ptr @big, i64 7992\n%x = load i64, ptr %p"),
    ("sec", "%v = load i32, ptr @sec\n%x = sext i32 %v to i64"),
    ("log", "%v = load i32, ptr @log\n%x = sext i32 %v to i64"),
    ("pext", "%p = load ptr, ptr @pext\n%v = load i32, ptr %p\n%x = sext i32 %v to i64"),
    ("pw", "%p = load ptr, ptr @pw\n%e = icmp eq ptr %p, null\n%x = zext i1 %e to i64"),
    ("pf", "%p = load ptr, ptr @pf\n%v = call i32 %p()\n%x = sext i32 %v to i64"),
    ("answer", "%v = call i32 @answer()\n%x = sext i32 %v to i64"),
];

/// `main`, which prints every probe.
fn main_module() -> String {
    let mut text = String::from(
        r#"
%S = type { i8, i64, i16 }
@i32 = external global i32
@i64 = external global i64
@zero = external global i32
@pstr = external global ptr
@pstr2 = external global ptr
@str16 = external global [3 x i16]
@ro = external constant i32
@roptr = external constant ptr
@parr = external global ptr
@plocal = external global ptr
@st = external global %S
@pst = external constant ptr
@cd = external global i32
@com = external global i32
@wk = external global i32
@t = external thread_local global i32
@tz = external thread_local global i32
@al = external global i32
@al2 = external global i32
@rel = external constant i32
@fold = external global i64
@cst8 = external constant i64
@big = external global [1000 x i64]
@sec = external global i32
@log = external global i32
@pext = external global ptr
@pw = external global ptr
@pf = external global ptr
@ext = global i32 31
declare i32 @answer()
declare i32 @read_tl()
declare i32 @printf(ptr, ...)
@fmt = private unnamed_addr constant [8 x i8] c"%s %ld\0A\00"
"#,
    );
    for (i, (label, _)) in PROBES.iter().enumerate() {
        let len = label.len() + 1;
        text.push_str(&format!(
            "@l{i} = private unnamed_addr constant [{len} x i8] c\"{label}\\00\"\n"
        ));
    }
    text.push_str("define i32 @main() {\n");
    for (i, (_, expr)) in PROBES.iter().enumerate() {
        text.push_str(&format!("  br label %p{i}\np{i}:\n"));
        for line in expr.lines() {
            // Number the probe's names apart.
            let line = line.replace('%', &format!("%q{i}_"));
            text.push_str(&format!("  {line}\n"));
        }
        text.push_str(&format!(
            "  call i32 (ptr, ...) @printf(ptr @fmt, ptr @l{i}, i64 %q{i}_x)\n"
        ));
    }
    text.push_str("  ret i32 0\n}\n");
    text
}

fn header(major: u32) -> String {
    let layout =
        if major >= 20 { AARCH64_LINUX_DATA_LAYOUT } else { AARCH64_LINUX_DATA_LAYOUT_LLVM19 };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n")
}

fn llc(scratch: &Scratch, major: u32, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let output = scratch.join(format!("{name}.o"));
    let mut command = Command::new(format!("/usr/lib/llvm-{major}/bin/llc"));
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: llc failed:\n{}", outcome.stderr_text());
    output
}

fn link_run(toolchain: &Toolchain, scratch: &Scratch, name: &str, inputs: &[&Path]) -> Vec<String> {
    let mut outputs = Vec::new();
    for linker in &toolchain.linkers {
        let program = scratch.join(format!("{name}-{}", linker.name));
        let outcome = toolchain.link_and_run(linker, &[], inputs, &program);
        assert_eq!(exit_code(&outcome), Some(0), "{name} with {}", linker.name);
        outputs.push(format!("{}:\n{}", linker.name, outcome.stdout_text()));
    }
    outputs
}

#[test]
fn data_links_and_runs_like_llc() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: not an AArch64 host; skipping");
        return;
    }
    let scratch = Scratch::new("emit-link");
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|m| Path::new(&format!("/usr/lib/llvm-{m}/bin/llc")).is_file())
        .collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    for major in majors {
        let data = format!("{}{DATA}{FLAGS}", header(major));
        let main = format!("{}{}{FLAGS}", header(major), main_module());
        let main_o = llc(&scratch, major, &format!("main-{major}"), &main);
        let theirs_o = llc(&scratch, major, &format!("data-llc-{major}"), &data);
        let theirs = elfview::Elf::parse(std::fs::read(&theirs_o).unwrap());
        let parsed = hod_ll::parse_module(data.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("hod_ll: {e}"));
        let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
        let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| 4);
        let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
        let emitted = emit_module(&parsed.module, &options, &[chunk]).expect("emission");
        let ours_o = linking::write(&scratch, &format!("data-hod-{major}.o"), &emitted.object);
        let expected = link_run(&toolchain, &scratch, &format!("llc-{major}"), &[&main_o, &theirs_o]);
        let got = link_run(&toolchain, &scratch, &format!("hod-{major}"), &[&main_o, &ours_o]);
        assert_eq!(got, expected, "LLVM {major}: hod's data runs differently");
        assert_eq!(expected[0].lines().count() - 1, PROBES.len());
        for probe in ["log 12", "answer 42", "pf 42", "rel 0", "fold 8", "pext 31", "pw 1"] {
            assert!(expected[0].lines().any(|l| l == probe), "llc's program: {}", expected[0]);
        }
        eprintln!(
            "emit_link: LLVM {major}: {} probes agree with llc's under {}",
            PROBES.len(),
            toolchain.linkers.iter().map(|l| l.name).collect::<Vec<_>>().join(", ")
        );
    }
}
