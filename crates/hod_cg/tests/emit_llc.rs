//! Module emission against `llc` (`spec/codegen.md` §9.7): each snippet
//! is emitted by `hod_cg::emit_module` and compiled by `llc` of each LLVM
//! major present, and the two objects must have the same sections,
//! groups, symbols and relocations, up to the normalizations of
//! `elfview` (order, mapping symbols, the file symbol, where `llc` puts a
//! COMDAT signature it makes).
//!
//! Functions in a snippet compile to code without relocations; `hod`'s
//! chunk carries `llc`'s bytes for them, so the comparison is of the
//! placement, not of instruction selection.

mod borrow;
mod elfview;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use elfview::{Elf, diff};
use hod_cg::{Options, RelocModel, emit_module};
use hod_ir::{AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};

/// One case: a name, extra `llc` flags, and the module text after the
/// target header.
struct Snippet {
    name: &'static str,
    flags: &'static [&'static str],
    ir: &'static str,
}

const SNIPPETS: &[Snippet] = &[
    Snippet {
        name: "scalars",
        flags: &[],
        ir: r"
@a = global i32 5
@b = global i64 0
@c = internal global i32 0
@c1 = internal global i32 3
@d = constant i32 7
@e = unnamed_addr constant i32 7
@f8 = unnamed_addr constant i64 7
@f16 = unnamed_addr constant <4 x i32> <i32 1, i32 2, i32 3, i32 4>
@f32 = unnamed_addr constant [8 x i32] [i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8]
@f12 = unnamed_addr constant [3 x i32] [i32 1, i32 2, i32 3]
@g = common global i32 0, align 4
@g2 = common global [100 x i8] zeroinitializer, align 16
@h = weak global i32 1
@h2 = weak_odr global i32 1
@h3 = linkonce_odr global i32 1
@hid = hidden global i32 1
@pro = protected global i32 1
@big = global [100 x i64] zeroinitializer
@al = global i32 1, align 64
@u = global i32 undef
@z0 = global {} zeroinitializer
@z1 = internal global {} zeroinitializer
@z2 = internal global [0 x i8] zeroinitializer
@cz = constant i64 0
",
    },
    Snippet {
        name: "strings",
        flags: &[],
        ir: r#"
@s1 = private unnamed_addr constant [6 x i8] c"hello\00"
@s2 = internal unnamed_addr constant [6 x i8] c"world\00"
@s3 = unnamed_addr constant [3 x i16] [i16 104, i16 105, i16 0]
@s4 = unnamed_addr constant [2 x i32] [i32 104, i32 0]
@s5 = unnamed_addr constant [1 x i8] zeroinitializer
@s6 = unnamed_addr constant [5 x i8] c"a\00bc\00"
@s7 = constant [6 x i8] c"named\00"
@s8 = unnamed_addr constant [4 x i8] c"abcd"
@s9 = private unnamed_addr constant [8 x i8] c"align4\00\00", align 4
@s10 = private unnamed_addr constant [17 x i8] c"sixteen chars...\00"
@ps = global ptr getelementptr (i8, ptr @s1, i64 2)
@ps0 = global ptr @s2
"#,
    },
    Snippet {
        name: "pointers",
        flags: &[],
        ir: r"
@a = global i32 5
@l = internal global i32 6
@dl = dso_local global i32 7
@p = global ptr @a
@q = constant ptr @a
@r = internal constant ptr @l
@pp = global [3 x ptr] [ptr @a, ptr getelementptr (i8, ptr @l, i64 4), ptr null]
@i = global i64 ptrtoint (ptr @a to i64)
@i32 = global i32 ptrtoint (ptr @a to i32)
@st = global { i8, ptr, i16 } { i8 1, ptr @l, i16 2 }
@pk = global <{ i8, ptr }> <{ i8 1, ptr @dl }>
@rel = dso_local constant i32 trunc (i64 sub (i64 ptrtoint (ptr @dl to i64), i64 ptrtoint (ptr @rel to i64)) to i32)
@rel2 = constant [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @rel2 to i64)) to i32), i32 0]
@fold = internal global [2 x i64] [i64 sub (i64 ptrtoint (ptr getelementptr (i8, ptr @fold, i64 8) to i64), i64 ptrtoint (ptr @fold to i64)), i64 0]
@ext = external global i32
@pext = global ptr @ext
@w = extern_weak global i32
@pw = global ptr @w
@hd = external hidden global i32
@phd = global ptr @hd
",
    },
    Snippet {
        name: "pointers-static",
        flags: &["-relocation-model=static"],
        ir: r"
@a = global i32 5
@l = internal global i32 6
@q = constant ptr @a
@r = internal constant ptr @l
@cs = unnamed_addr constant ptr @a
",
    },
    Snippet {
        name: "tls",
        flags: &[],
        ir: r"
@t = thread_local global i32 0
@t1 = thread_local global i32 1
@t2 = internal thread_local global i64 0
@t3 = internal thread_local global i64 9
@t4 = thread_local(initialexec) global i32 5
@t5 = thread_local constant i32 0
@pt = global ptr @t1
",
    },
    Snippet {
        name: "sections",
        flags: &[],
        ir: r#"
@a = global i32 1, section "foo"
@b = global i32 2, section "foo"
@c = constant i32 3, section "foo"
@s = unnamed_addr constant [3 x i8] c"hi\00", section "msec"
@m = global i32 4, section "msec"
@z = global i32 0, section ".bss.mine"
@zt = thread_local global i32 0, section ".tbss.mine"
@x = global i32 0, section "xs"
@d = constant i32 5, section ".data"
@e = global i32 6, section ".data"
@r1 = unnamed_addr constant [3 x i8] c"ab\00", section ".rodata.str1.1"
@r2 = unnamed_addr constant [3 x i8] c"cd\00", section ".rodata.str1.1"
@n = global i32 7, section ".note.mine"
@meta = global i32 8, section "llvm.metadata"
@ia = global ptr null, section ".init_array.mine"
@al = global i64 1, section "foo", align 32
"#,
    },
    Snippet {
        name: "comdat",
        flags: &[],
        ir: r#"
$c1 = comdat any
$c2 = comdat any
$grp = comdat any
$nd = comdat nodeduplicate
@c1 = linkonce_odr global i32 1, comdat
@c2 = linkonce_odr global i32 0, comdat
@v = global i32 1, comdat($grp)
@v2 = internal global i32 2, comdat($grp)
@n = global i32 3, comdat($nd)
@nd = global i32 4, comdat
@s = linkonce_odr unnamed_addr constant [3 x i8] c"ab\00", comdat($c1)
@cp = linkonce_odr constant ptr @v, comdat($c1)
"#,
    },
    Snippet {
        name: "used",
        flags: &[],
        ir: r#"
@u1 = internal global i32 1
@u2 = global i32 0
@u3 = internal constant i32 3, section "us"
@cu = internal global i32 4
@llvm.used = appending global [3 x ptr] [ptr @u1, ptr @u2, ptr @u3], section "llvm.metadata"
@llvm.compiler.used = appending global [1 x ptr] [ptr @cu], section "llvm.metadata"
"#,
    },
    Snippet {
        name: "data-sections",
        flags: &["-data-sections"],
        ir: r#"
@a = global i32 1
@b = internal global i32 0
@c = constant i32 3
@s = private unnamed_addr constant [3 x i8] c"ab\00"
@e = unnamed_addr constant i32 7
@p = constant ptr @a
@t = thread_local global i32 1
@t0 = thread_local global i32 0
@sec = global i32 2, section "foo"
"#,
    },
    Snippet {
        name: "aliases",
        flags: &[],
        ir: r"
%S = type { i32, i64 }
@a = global i32 5
@st = global %S { i32 1, i64 2 }
@l = internal global i64 3
@priv = private global [2 x i32] [i32 1, i32 2]
@t = thread_local global i32 1
@al = alias i32, ptr @a
@al2 = alias i64, getelementptr (i8, ptr @st, i64 8)
@al3 = internal alias i64, ptr @l
@al4 = hidden alias i32, ptr @al
@al5 = alias i32, getelementptr (i8, ptr @priv, i64 4)
@al6 = weak alias i32, ptr @a
@alt = alias i32, ptr @t
@pal = global ptr @al3
@pal4 = global ptr @al4
",
    },
    Snippet {
        name: "functions",
        flags: &[],
        ir: r#"
$cf = comdat any
define void @f() {
  ret void
}
define internal void @g() {
  ret void
}
define linkonce_odr void @cf() comdat {
  ret void
}
define hidden void @h() section "ftext" {
  ret void
}
define weak void @wk() align 16 {
  ret void
}
define ptr @resolver() {
  ret ptr null
}
@fa = alias void (), ptr @f
@ga = internal alias void (), ptr @g
@if = ifunc void (), ptr @resolver
@iif = internal ifunc void (), ptr @resolver
@pf = global ptr @f
@pg = global ptr @g
@pga = global ptr @ga
@pif = global ptr @if
@piif = global ptr @iif
@llvm.used = appending global [1 x ptr] [ptr @h], section "llvm.metadata"
"#,
    },
    Snippet {
        name: "function-sections",
        flags: &["-function-sections"],
        ir: r#"
define void @f() {
  ret void
}
define internal void @g() {
  ret void
}
define void @k() section "ks" {
  ret void
}
@pg = global ptr @g
"#,
    },
    Snippet {
        name: "structors",
        flags: &[],
        ir: r"
$key = comdat any
@key = linkonce_odr global i32 0, comdat
@ext = external global i32
define void @c1() {
  ret void
}
define internal void @c2() {
  ret void
}
define void @d1() {
  ret void
}
@llvm.global_ctors = appending global [5 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @c1, ptr null }, { i32, ptr, ptr } { i32 101, ptr @c2, ptr null }, { i32, ptr, ptr } { i32 65535, ptr @c2, ptr @key }, { i32, ptr, ptr } { i32 101, ptr @c1, ptr null }, { i32, ptr, ptr } { i32 200, ptr @c1, ptr @ext }]
@llvm.global_dtors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @d1, ptr null }, { i32, ptr, ptr } { i32 300, ptr @d1, ptr null }]
",
    },
    Snippet {
        name: "linker-options",
        flags: &[],
        ir: r#"
@a = global i32 1
!llvm.linker.options = !{!0, !1}
!llvm.dependent-libraries = !{!2, !3}
!0 = !{!"lib", !"foo"}
!1 = !{!"-L", !"/x"}
!2 = !{!"m"}
!3 = !{!"pthread"}
"#,
    },
    Snippet {
        name: "anonymous",
        flags: &[],
        ir: r#"
@0 = private unnamed_addr constant [3 x i8] c"ab\00"
@1 = internal global i32 3
@2 = global i32 4
@p = global [3 x ptr] [ptr @0, ptr @1, ptr @2]
"#,
    },
    Snippet {
        name: "relative",
        flags: &[],
        ir: r"
@vt = dso_local unnamed_addr constant [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr @t1 to i64), i64 ptrtoint (ptr @vt to i64)) to i32), i32 trunc (i64 sub (i64 ptrtoint (ptr @t2 to i64), i64 ptrtoint (ptr @vt to i64)) to i32)]
@t1 = dso_local global i32 1
@t2 = internal global i32 2
@d64 = global i64 sub (i64 ptrtoint (ptr @t1 to i64), i64 ptrtoint (ptr @d64 to i64))
",
    },
    Snippet {
        name: "code-relocs",
        flags: &[],
        ir: r#"
@g = global i32 1
@l = internal global i32 2
@s = private unnamed_addr constant [3 x i8] c"ab\00"
@dl = dso_local global i32 3
@t = thread_local global i32 4
@tl = internal thread_local global i32 5
define i32 @f() {
  %a = load i32, ptr @g
  %b = load i32, ptr @l
  %c = add i32 %a, %b
  ret i32 %c
}
define ptr @h() {
  ret ptr @s
}
define ptr @k() {
  ret ptr getelementptr (i8, ptr @s, i64 1)
}
define ptr @m() {
  ret ptr @dl
}
define ptr @n() {
  ret ptr @t
}
define ptr @o() {
  ret ptr @tl
}
define ptr @q() {
  ret ptr @later
}
define internal void @later() {
  ret void
}
define dso_local void @dlf() {
  ret void
}
define ptr @r() {
  ret ptr @dlf
}
"#,
    },
];

fn header(major: u32) -> String {
    let layout =
        if major >= 20 { AARCH64_LINUX_DATA_LAYOUT } else { AARCH64_LINUX_DATA_LAYOUT_LLVM19 };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n")
}

fn llc_path(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    installed.is_file().then_some(installed)
}

/// `llc`'s object for `text`.
fn llc(scratch: &Scratch, major: u32, name: &str, text: &str, flags: &[&str]) -> Vec<u8> {
    let input = scratch.write(&format!("{name}-{major}.ll"), text);
    let output = scratch.join(format!("{name}-{major}.o"));
    let mut command = Command::new(llc_path(major).unwrap());
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    if !flags.iter().any(|f| f.starts_with("-relocation-model=")) {
        command.arg("-relocation-model=pic");
    }
    command.args(flags).arg(&input).arg("-o").arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: llc-{major} failed:\n{}", outcome.stderr_text());
    std::fs::read(&output).unwrap()
}

fn options(flags: &[&str]) -> Options {
    let mut options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    for flag in flags {
        match *flag {
            "-function-sections" => options.function_sections = true,
            "-data-sections" => options.data_sections = true,
            "-relocation-model=static" => options.reloc_model = RelocModel::Static,
            "-relocation-model=dynamic-no-pic" => options.reloc_model = RelocModel::DynamicNoPic,
            other => panic!("unknown flag {other}"),
        }
    }
    options
}

/// Sections whose contents are not compared: code `hod` copied, and
/// what `llc` writes that `hod` does not compare here.
fn ignore_data(_name: &str) -> bool {
    false
}

/// Sections not module emission's: the unwind tables `llc` makes for the
/// snippets' functions are `hod_eh`'s.
fn skip(name: &str) -> bool {
    name == ".eh_frame"
}

#[test]
fn snippets_agree_with_llc() {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc; skipping");
        return;
    }
    let scratch = Scratch::new("emit-llc");
    let mut failures = Vec::new();
    for major in &majors {
        for snippet in SNIPPETS {
            let text = format!("{}{}", header(*major), snippet.ir);
            let theirs = Elf::parse(llc(&scratch, *major, snippet.name, &text, snippet.flags));
            let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
                .unwrap_or_else(|e| panic!("{}: hod_ll: {e}", snippet.name));
            let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
            let align = |name: &str| if name == "wk" { 16 } else { 4 };
            let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &align);
            let emitted = emit_module(&parsed.module, &options(snippet.flags), &[chunk])
                .unwrap_or_else(|e| panic!("{}: emit_module: {e}", snippet.name));
            let ours = Elf::parse(hod_obj::elf::to_vec(&emitted.object).unwrap());
            let (a, b) = (
                ours.describe_without(&ignore_data, &skip),
                theirs.describe_without(&ignore_data, &skip),
            );
            let (only_ours, only_theirs) = diff(&a, &b);
            let verdict =
                if only_ours.is_empty() && only_theirs.is_empty() { "agree" } else { "DIFFER" };
            eprintln!("emit_llc: LLVM {major} {:<18} {verdict} ({} lines)", snippet.name, b.len());
            if verdict != "agree" {
                let mut report = format!("{} (LLVM {major}):\n", snippet.name);
                for line in &only_ours {
                    let _ = writeln!(report, "  hod: {line}");
                }
                for line in &only_theirs {
                    let _ = writeln!(report, "  llc: {line}");
                }
                failures.push(report);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What module emission cannot express is refused by name (§10): each
/// module here, read by `hod_ll`, must give exactly that error.
#[test]
fn unsupported_constructs_are_refused_by_name() {
    use hod_cg::CgError;
    use hod_cg::emit::unsupported as u;
    let cases: &[(&str, &str)] = &[
        (u::MODULE_ASM, "module asm \".globl x\"\n@v = global i32 1\n"),
        (u::COMDAT_KIND, "$c = comdat largest\n@c = global i32 1, comdat\n"),
        (u::FIELD_WIDTH, "@y = global i32 0\n@x = global i8 ptrtoint (ptr @y to i8)\n"),
        (
            u::SYM_DIFF,
            "@a = external global i32\n@b = external global i32\n@d = global i64 sub (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @b to i64))\n",
        ),
        (u::NOBITS_INIT, "@x = global i32 5, section \".bss.mine\"\n"),
        (u::DSO_LOCAL_EQUIVALENT, "declare void @f()\n@p = global ptr dso_local_equivalent @f\n"),
    ];
    let mut checked = 0;
    for &(name, ir) in cases {
        let text = format!("{}{ir}", header(22));
        let parsed = match hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()) {
            Ok(parsed) if parsed.module.unsupported().next().is_none() => parsed,
            Ok(_) | Err(_) => {
                eprintln!("note: {name}: hod_ll does not pass this module on");
                continue;
            }
        };
        let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
        match emit_module(&parsed.module, &options, &[]) {
            Err(CgError::Unsupported { construct }) => assert_eq!(construct, name),
            other => panic!("{name}: {other:?}"),
        }
        checked += 1;
    }
    assert!(checked >= 4, "only {checked} cases reached emission");
}
