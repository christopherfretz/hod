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

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel, emit_module};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, X86_64_LINUX_DATA_LAYOUT,
};
use hod_obj::Arch;
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
    (
        "str16",
        "%p = getelementptr i8, ptr @str16, i64 2\n%c = load i16, ptr %p\n%x = sext i16 %c to i64",
    ),
    ("ro", "%v = load i32, ptr @ro\n%x = sext i32 %v to i64"),
    ("roptr", "%p = load ptr, ptr @roptr\n%e = icmp eq ptr %p, @i32\n%x = zext i1 %e to i64"),
    ("parr", "%p = load ptr, ptr @parr\n%v = load i32, ptr %p\n%x = sext i32 %v to i64"),
    ("plocal", "%p = load ptr, ptr @plocal\n%v = load i32, ptr %p\n%x = sext i32 %v to i64"),
    ("pst", "%p = load ptr, ptr @pst\n%x = load i64, ptr %p"),
    (
        "st.c",
        "%p = getelementptr i8, ptr @st, i64 16\n%v = load i16, ptr %p\n%x = sext i16 %v to i64",
    ),
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
        let _ =
            writeln!(text, "@l{i} = private unnamed_addr constant [{len} x i8] c\"{label}\\00\"");
    }
    text.push_str("define i32 @main() {\n");
    for (i, (_, expr)) in PROBES.iter().enumerate() {
        let _ = writeln!(text, "  br label %p{i}\np{i}:");
        for line in expr.lines() {
            // Number the probe's names apart.
            let line = line.replace('%', &format!("%q{i}_"));
            let _ = writeln!(text, "  {line}");
        }
        let _ = writeln!(text, "  call i32 (ptr, ...) @printf(ptr @fmt, ptr @l{i}, i64 %q{i}_x)");
    }
    text.push_str("  ret i32 0\n}\n");
    text
}

/// The module header of `arch` for `llc` of LLVM `major`.
fn header(arch: Arch, major: u32) -> String {
    let (layout, triple) = match arch {
        Arch::AArch64 if major >= 20 => (AARCH64_LINUX_DATA_LAYOUT, "aarch64-unknown-linux-gnu"),
        Arch::AArch64 => (AARCH64_LINUX_DATA_LAYOUT_LLVM19, "aarch64-unknown-linux-gnu"),
        Arch::X86_64 => (X86_64_LINUX_DATA_LAYOUT, "x86_64-unknown-linux-gnu"),
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n")
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
    links_and_runs(&scratch, &toolchain, Arch::AArch64);
}

/// The same program for x86-64 (WP-143), linked by the cross toolchain
/// and run under the emulator when the host is not x86-64.
#[test]
fn x86_64_data_links_and_runs_like_llc() {
    let scratch = Scratch::new("emit-link-x64");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else {
        assert!(!tools_required(), "no x86-64 toolchain (HOD_REQUIRE_TOOLS=1)");
        return;
    };
    links_and_runs(&scratch, &toolchain, Arch::X86_64);
}

fn links_and_runs(scratch: &Scratch, toolchain: &Toolchain, arch: Arch) {
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|m| Path::new(&format!("/usr/lib/llvm-{m}/bin/llc")).is_file())
        .collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    // `llc` aligns x86-64 functions to 16 bytes.
    let align = if arch == Arch::X86_64 { 16 } else { 4 };
    for major in majors {
        let data = format!("{}{DATA}{FLAGS}", header(arch, major));
        let main = format!("{}{}{FLAGS}", header(arch, major), main_module());
        let main_o = llc(scratch, major, &format!("main-{major}"), &main);
        let theirs_o = llc(scratch, major, &format!("data-llc-{major}"), &data);
        let theirs = elfview::Elf::parse(std::fs::read(&theirs_o).unwrap());
        let parsed = hod_ll::parse_module(data.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("hod_ll: {e}"));
        let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
        let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| align);
        let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
        let emitted = emit_module(&parsed.module, &options, &[chunk]).expect("emission");
        let ours_o = linking::write(scratch, &format!("data-hod-{major}.o"), &emitted.object);
        let expected = link_run(toolchain, scratch, &format!("llc-{major}"), &[&main_o, &theirs_o]);
        let got = link_run(toolchain, scratch, &format!("hod-{major}"), &[&main_o, &ours_o]);
        assert_eq!(got, expected, "LLVM {major}: hod's data runs differently");
        assert_eq!(expected[0].lines().count() - 1, PROBES.len());
        for probe in ["log 12", "answer 42", "pf 42", "rel 0", "fold 8", "pext 31", "pw 1"] {
            assert!(expected[0].lines().any(|l| l == probe), "llc's program: {}", expected[0]);
        }
        eprintln!(
            "emit_link: {arch:?} LLVM {major}: {} probes agree with llc's under {}",
            PROBES.len(),
            toolchain.linkers.iter().map(|l| l.name).collect::<Vec<_>>().join(", ")
        );
    }
}

/// A data module whose code is all in a C driver (WP-143): tables of the
/// driver's functions, constructors and a destructor of the driver's with
/// priorities and an associated COMDAT, an IFUNC resolved to a driver
/// function, thread-local definitions the driver reads, aliases with
/// offsets, visibility, weak, common, `extern_weak`, an explicit section,
/// `llvm.used`, a 32-bit difference against a driver function.
const C_DATA: &str = r#"
$cw = comdat any
$grp = comdat any
%S = type { i32, ptr, [2 x i16] }
declare i32 @c_one()
declare i32 @c_two()
declare i32 @c_impl()
declare void @c_ctor_a()
declare void @c_ctor_b()
declare void @c_ctor_c()
declare void @c_dtor()
@table = global [3 x ptr] [ptr @c_one, ptr @c_two, ptr null]
@rtable = internal constant [2 x ptr] [ptr @c_two, ptr @c_one]
@ptable = global ptr getelementptr (i8, ptr @rtable, i64 8)
@s = global %S { i32 5, ptr getelementptr (i8, ptr @table, i64 8), [2 x i16] [i16 -1, i16 2] }
@msg = private unnamed_addr constant [6 x i8] c"hello\00"
@pmsg = global ptr getelementptr (i8, ptr @msg, i64 1)
@cw = linkonce_odr global i32 17, comdat
@grpv = global i32 18, comdat($grp)
@wk = weak global i32 19
@com = common global i64 0, align 8
@hid = hidden global i32 20
@pro = protected global i32 21
@aliasv = alias i32, ptr @hid
@aoff = alias i16, getelementptr (i8, ptr @s, i64 18)
@sec = global i32 22, section "hod_sec"
@kept = internal global i32 23
@pkept = global ptr @kept
@reltab = constant [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr @c_two to i64), i64 ptrtoint (ptr @reltab to i64)) to i32), i32 trunc (i64 sub (i64 ptrtoint (ptr @kept to i64), i64 ptrtoint (ptr @reltab to i64)) to i32)]
@ew = extern_weak global i32
@pew = global ptr @ew
@t = thread_local global i32 24
@tz = thread_local global [4 x i32] zeroinitializer
@str8 = unnamed_addr constant [4 x i8] c"abc\00"
@cst16 = unnamed_addr constant <4 x i32> <i32 1, i32 2, i32 3, i32 4>
define internal ptr @resolver() {
  ret ptr @c_impl
}
@answer = ifunc i32 (), ptr @resolver
@panswer = global ptr @answer
@llvm.used = appending global [1 x ptr] [ptr @kept], section "llvm.metadata"
@llvm.global_ctors = appending global [3 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @c_ctor_b, ptr null }, { i32, ptr, ptr } { i32 101, ptr @c_ctor_a, ptr null }, { i32, ptr, ptr } { i32 300, ptr @c_ctor_c, ptr @cw }]
@llvm.global_dtors = appending global [1 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @c_dtor, ptr null }]
"#;

const C_DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct S { int a; void *p; short h[2]; };
extern int (*table[3])(void);
extern int (**ptable)(void);
extern struct S s;
extern const char *pmsg;
extern int cw, grpv, wk, hid, pro, aliasv, sec;
extern short aoff;
extern int64_t com;
extern int *pkept, *pew;
extern const int32_t reltab[2];
extern __thread int t;
extern __thread int tz[4];
extern const char str8[4];
extern const int32_t cst16[4];
extern int answer(void);
extern int (*panswer)(void);
static int log_v;
int c_one(void) { return 1; }
int c_two(void) { return 2; }
int c_impl(void) { return 42; }
void c_ctor_a(void) { log_v = log_v * 10 + 1; }
void c_ctor_b(void) { log_v = log_v * 10 + 2; }
void c_ctor_c(void) { log_v = log_v * 10 + 3; }
void c_dtor(void) { printf("dtor %d\n", log_v); }
int main(void) {
  printf("log %d\n", log_v);
  printf("table %d %d %d\n", table[0](), table[1](), table[2] == 0);
  printf("ptable %d\n", (*ptable)());
  printf("s %d %d %d %d\n", s.a, s.p == (void *)&table[1], s.h[0], s.h[1]);
  printf("pmsg %s\n", pmsg);
  printf("cw %d grpv %d wk %d com %ld\n", cw, grpv, wk, (long)com);
  printf("hid %d pro %d aliasv %d same %d aoff %d\n", hid, pro, aliasv, &aliasv == &hid, aoff);
  printf("sec %d kept %d\n", sec, *pkept);
  printf("reltab %d %d\n", (char *)reltab + reltab[0] == (char *)c_two,
         (char *)reltab + 4 + reltab[1] == (char *)pkept + 4);
  printf("pew %d\n", pew == 0);
  printf("t %d tz %d %d\n", t, tz[0], tz[3]);
  t += 1;
  tz[3] = 5;
  printf("t %d tz %d\n", t, tz[3]);
  printf("str8 %s cst16 %d %d\n", str8, cst16[0], cst16[3]);
  printf("answer %d %d\n", answer(), panswer());
  return 0;
}
"#;

/// [`C_DATA`] by `hod` and by `llc`, each linked with [`C_DRIVER`] by
/// every linker and run: the outputs must agree, for both targets.
#[test]
fn data_with_a_c_driver_runs_like_llc() {
    let scratch = Scratch::new("emit-link-c");
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|m| Path::new(&format!("/usr/lib/llvm-{m}/bin/llc")).is_file())
        .collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    // Each toolchain chooses its linkers in a scratch directory of its own.
    let x64 = Scratch::new("emit-link-c-x64");
    let mut targets = vec![(Arch::X86_64, Toolchain::x86_64(&x64))];
    let host = Scratch::new("emit-link-c-host");
    if cfg!(target_arch = "aarch64") {
        targets.push((Arch::AArch64, Toolchain::host(&host)));
    }
    for (arch, toolchain) in targets {
        let Some(toolchain) = toolchain else {
            assert!(!tools_required(), "no toolchain for {arch:?} (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let tag = format!("{arch:?}");
        let driver = toolchain.compile_c(&scratch, &format!("driver-{tag}"), C_DRIVER, &[]);
        let align = if arch == Arch::X86_64 { 16 } else { 4 };
        for &major in &majors {
            let data = format!("{}{C_DATA}{FLAGS}", header(arch, major));
            let theirs_o = llc(&scratch, major, &format!("cdata-llc-{tag}-{major}"), &data);
            let theirs = elfview::Elf::parse(std::fs::read(&theirs_o).unwrap());
            let parsed = hod_ll::parse_module(data.as_bytes(), &hod_ll::ParseOptions::default())
                .unwrap_or_else(|e| panic!("hod_ll: {e}"));
            let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
            let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| align);
            let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
            let emitted = emit_module(&parsed.module, &options, &[chunk]).expect("emission");
            let ours_o =
                linking::write(&scratch, &format!("cdata-hod-{tag}-{major}.o"), &emitted.object);
            let name = format!("c-llc-{tag}-{major}");
            let expected = link_run(&toolchain, &scratch, &name, &[&driver, &theirs_o]);
            let name = format!("c-hod-{tag}-{major}");
            let got = link_run(&toolchain, &scratch, &name, &[&driver, &ours_o]);
            assert_eq!(got, expected, "{tag} LLVM {major}: hod's data runs differently");
            for line in [
                "log 132",
                "table 1 2 1",
                "ptable 1",
                "s 5 1 -1 2",
                "pmsg ello",
                "reltab 1 1",
                "pew 1",
                "t 24 tz 0 0",
                "answer 42 42",
                "dtor 132",
            ] {
                assert!(expected[0].lines().any(|l| l == line), "{line}: {}", expected[0]);
            }
            eprintln!("emit_link: {tag} LLVM {major}: the C driver agrees under every linker");
        }
    }
}

/// An `available_externally` `alwaysinline` function called directly
/// (`spec/codegen.md` §9.7.2, WP-021) on x86-64 (WP-143): its code,
/// placed as the local copy `inl.alwaysinline`, takes the direct call
/// (`R_X86_64_PLT32`); its address in data stays the external symbol,
/// which the C driver defines; an `available_externally` variable is not
/// written.
const AVAILABLE: &str = r"
@av = available_externally global i32 9
@pav = global ptr @av
define available_externally i32 @inl(i32 %x) #0 {
  %r = add i32 %x, 100
  ret i32 %r
}
define i32 @caller() {
  %v = call i32 @inl(i32 41)
  %r = add i32 %v, 1
  ret i32 %r
}
@pinl = global ptr @inl
attributes #0 = { alwaysinline }
";

const AVAILABLE_DRIVER: &str = r#"
#include <stdio.h>
int av = 9;
int inl(int x) { return x + 1000; }
extern int caller(void);
extern int *pav;
extern int (*pinl)(int);
int main(void) {
  printf("caller %d pinl %d pav %d\n", caller(), pinl(1), pav == &av);
  return 0;
}
"#;

#[test]
fn x86_64_available_externally_copy_links_and_runs() {
    let scratch = Scratch::new("emit-link-avail");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else {
        assert!(!tools_required(), "no x86-64 toolchain (HOD_REQUIRE_TOOLS=1)");
        return;
    };
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|m| Path::new(&format!("/usr/lib/llvm-{m}/bin/llc")).is_file())
        .collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    let driver = toolchain.compile_c(&scratch, "avail-driver", AVAILABLE_DRIVER, &[]);
    for major in majors {
        let text = format!("{}{AVAILABLE}{FLAGS}", header(Arch::X86_64, major));
        // `llc` writes no code for `inl`: its code is borrowed from the
        // same module with the function made `linkonce_odr`.
        let defined = text.replace("available_externally i32 @inl", "linkonce_odr i32 @inl");
        let code_o = llc(&scratch, major, &format!("avail-code-{major}"), &defined);
        let code = elfview::Elf::parse(std::fs::read(&code_o).unwrap());
        let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("hod_ll: {e}"));
        let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
        let chunk = borrow::chunk_from(&parsed.module, &functions, &code, &|_| 16);
        let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
        let emitted = emit_module(&parsed.module, &options, &[chunk]).expect("emission");
        let ours_o = linking::write(&scratch, &format!("avail-hod-{major}.o"), &emitted.object);
        let ours = elfview::Elf::parse(std::fs::read(&ours_o).unwrap());
        let calls = ours.relocs_of(".text");
        assert!(
            calls.iter().any(|(kind, name, _)| *kind == 4 && name == "inl.alwaysinline"),
            "the direct call goes to the copy: {calls:?}"
        );
        assert!(ours.symbols.iter().all(|s| s.name != "av" || s.shndx == 0), "av is not written");
        let got = link_run(&toolchain, &scratch, &format!("avail-{major}"), &[&driver, &ours_o]);
        for output in &got {
            assert!(output.contains("caller 142 pinl 1001 pav 1"), "{output}");
        }
        eprintln!("emit_link: x86-64 LLVM {major}: the available_externally copy runs");
    }
}
