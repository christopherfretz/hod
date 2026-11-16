//! 32-bit x86 through the facade (WP-286, design 0008 §1.9; WP-293): a
//! module for `i686-*-linux-gnu` (and `i386`, `i486`, `i586`) is read,
//! verified and classified like any other, and compiled by `hod_x86`,
//! which refuses what it does not lower under an `x86:` name
//! (`spec/codegen-x86.md`). What is not 32-bit x86 Linux stays
//! `unsupported-target`.
//!
//! The clang-built modules are design 0008's C probes compiled by clang 19
//! and 22 for `i686-linux-gnu` (text and bitcode, with and without
//! `-march=pentium4`); a missing clang is skipped with a note unless
//! `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{CompileError, Construct, Options, OutputKind, Unsupported};
use hod_ir::I386_LINUX_DATA_LAYOUT;
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, run_with, tools_required};

fn obj() -> Options {
    Options { output_kind: OutputKind::Object, ..Options::default() }
}

/// A 32-bit ELF relocatable object for `EM_386`.
fn is_i386_object(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF\x01\x01") && bytes.get(18..20) == Some(&[3, 0][..])
}

/// Whether `result` is a refusal by `hod_x86` (some `x86:` name; which one
/// is not pinned for a module with several constructs).
fn refused_by_x86(result: &Result<Vec<u8>, CompileError>) -> bool {
    matches!(result, Err(CompileError::Unsupported(Unsupported { construct, .. }))
        if construct.name().starts_with("x86:"))
}

/// A module of `i1`..`i32` and `ptr` only.
const INTEGERS: &str = r"
define i32 @f(i32 %a, ptr %p) {
  %v = load i32, ptr %p
  %s = add i32 %a, %v
  ret i32 %s
}
";

fn header(triple: &str) -> String {
    format!("target datalayout = \"{I386_LINUX_DATA_LAYOUT}\"\ntarget triple = \"{triple}\"\n")
}

/// Integer, `i64`, `double`, `x86_fp80`, pointer arithmetic and a
/// `size_t`-sized `memcpy` of 32 bits: the front half accepts them all.
const BODY: &str = r#"
@g = global [4 x i32] zeroinitializer, align 4
@p = global ptr getelementptr (i8, ptr @g, i64 4294967300), align 4
@n = global i32 ptrtoint (ptr getelementptr (i32, ptr null, i32 1) to i32), align 4

define i64 @add(i64 %a, i64 %b) #0 {
  %s = add i64 %a, %b
  ret i64 %s
}

define double @fd(double %a, float %b) #0 {
  %c = fpext float %b to double
  %r = fmul double %a, %c
  ret double %r
}

define x86_fp80 @ld(x86_fp80 %a) #0 {
  %r = fadd x86_fp80 %a, 0xK3FFF8000000000000000
  ret x86_fp80 %r
}

define ptr @at(ptr %p, i32 %i) #0 {
  %q = getelementptr inbounds i32, ptr %p, i32 %i
  %c = icmp ult ptr %q, %p
  %r = select i1 %c, ptr %p, ptr %q
  ret ptr %r
}

define i32 @diff(ptr %a, ptr %b) #0 {
  %x = ptrtoint ptr %a to i32
  %y = ptrtoint ptr %b to i32
  %d = sub i32 %x, %y
  ret i32 %d
}

define void @copy(ptr %d, ptr %s, i32 %n) #0 {
  call void @llvm.memcpy.p0.p0.i32(ptr %d, ptr %s, i32 %n, i1 false)
  call void @llvm.memset.p0.i32(ptr %d, i8 0, i32 %n, i1 false)
  ret void
}

declare void @llvm.memcpy.p0.p0.i32(ptr, ptr, i32, i1)
declare void @llvm.memset.p0.i32(ptr, i8, i32, i1)

attributes #0 = { noinline nounwind "target-cpu"="i686" "target-features"="+cmov,+cx8,+x87" }
"#;

/// Every 32-bit x86 triple compiles an integer module to an i386 object
/// (and a module of declarations only); `BODY`, with `i64`, `double` and
/// `x86_fp80`, is refused by `hod_x86`, after the front half read and
/// classified it (its report names the target).
#[test]
fn every_i386_triple_reaches_hod_x86() {
    for triple in
        ["i686-unknown-linux-gnu", "i686-pc-linux-gnu", "i586-linux-gnu", "i386-linux-gnu"]
    {
        let text = format!("{}{BODY}", header(triple));
        let mut report = hod::Report::default();
        let result =
            hod::compile_with(text.as_bytes(), &obj(), &hod::Settings::default(), &mut report);
        assert!(refused_by_x86(&result), "{triple}: {result:?}");
        assert_eq!(report.target.as_deref(), Some(triple));
        let ints = format!("{}{INTEGERS}", header(triple));
        let object = hod::compile(ints.as_bytes(), &obj()).expect("an integer module");
        assert!(is_i386_object(&object), "{triple}");
        let decls = format!("{}declare i32 @f(i64)\n", header(triple));
        let object = hod::compile(decls.as_bytes(), &obj()).expect("declarations only");
        assert!(is_i386_object(&object), "{triple}");
    }
    // With `-mtriple` naming it too, and with `-mcpu` and `-mattr`, which
    // change nothing `hod_x86` emits.
    let ints = format!("{}{INTEGERS}", header("i686-unknown-linux-gnu"));
    let plain = hod::compile(ints.as_bytes(), &obj()).unwrap();
    let options = Options { triple: Some("i686-unknown-linux-gnu".into()), ..obj() };
    assert_eq!(hod::compile(ints.as_bytes(), &options).unwrap(), plain);
    let options = Options { cpu: Some("pentium4".into()), features: vec!["+sse2".into()], ..obj() };
    assert_eq!(hod::compile(ints.as_bytes(), &options).unwrap(), plain);
    let text = format!("{}{BODY}", header("i686-unknown-linux-gnu"));
    assert!(refused_by_x86(&hod::compile(text.as_bytes(), &options)));
}

/// `-relocation-model=pic` compiles (design 0008's I8, WP-299: it was
/// refused as `x86:pic`; the run tests are `i386_pic.rs`).
#[test]
fn pic_compiles() {
    let ints = format!("{}{INTEGERS}", header("i686-unknown-linux-gnu"));
    let options = Options { relocation_model: Some(hod::RelocModel::Pic), ..obj() };
    let result = hod::compile(ints.as_bytes(), &options);
    assert!(result.is_ok(), "{result:?}");
}

/// What is not 32-bit x86 Linux with the GNU environment, a layout that is
/// not i686's, and retargeting stay `unsupported-target`.
#[test]
fn other_32_bit_targets_stay_unsupported() {
    let target = || -> Result<Vec<u8>, CompileError> {
        Err(Unsupported::module(Construct::UnsupportedTarget).into())
    };
    for triple in [
        "x86_64-unknown-linux-gnux32",
        "i686-pc-windows-msvc",
        "i686-unknown-linux-musl",
        "i686-linux-android",
        "armv7-unknown-linux-gnueabihf",
        "i786-unknown-linux-gnu",
    ] {
        let text = format!("target triple = \"{triple}\"\ndeclare void @f()\n");
        assert_eq!(hod::compile(text.as_bytes(), &obj()), target(), "{triple}");
    }
    // The x86-64 layout under the i686 triple.
    let text = format!(
        "target datalayout = \"{}\"\ntarget triple = \"i686-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    );
    assert_eq!(hod::compile(text.as_bytes(), &obj()), target());
    // An x86-64 module with `-mtriple=i686-...` and the reverse.
    let options = Options { triple: Some("i686-unknown-linux-gnu".into()), ..obj() };
    let x64 = b"target triple = \"x86_64-unknown-linux-gnu\"\ndeclare void @f()\n";
    assert_eq!(hod::compile(x64, &options), target());
    let options = Options { triple: Some("x86_64-unknown-linux-gnu".into()), ..obj() };
    let i686 = header("i686-unknown-linux-gnu");
    assert_eq!(hod::compile(i686.as_bytes(), &options), target());
}

/// The front half still refuses what it refuses on x86-64 by its own
/// name before the back half is reached.
#[test]
fn front_half_refusals_come_first() {
    let text = format!(
        "{}define void @f() {{\n  call void @llvm.x86.foo.bar()\n  ret void\n}}\n\
         declare void @llvm.x86.foo.bar()\n",
        header("i686-unknown-linux-gnu")
    );
    let result = hod::compile(text.as_bytes(), &obj());
    let Err(CompileError::Unsupported(Unsupported { construct, .. })) = &result else {
        panic!("{result:?}");
    };
    assert!(!construct.name().starts_with("x86:"), "classify's own name: {}", construct.name());
}

fn clang(major: u32) -> Option<PathBuf> {
    let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/clang"));
    if path.exists() {
        return Some(path);
    }
    assert!(!tools_required(), "no clang {major} (HOD_REQUIRE_TOOLS=1)");
    eprintln!("note: no clang {major}; skipping its part");
    None
}

fn probe(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/0008-data/abi").join(name)
}

/// clang's own i686 modules (design 0008's `a.c`, `dbg.c`, `eh.cpp`), text and
/// bitcode, default CPU and `pentium4`, read by both readers.
#[test]
fn clang_modules_reach_the_back_half() {
    let scratch = Scratch::new("i386-clang");
    let mut compiled = 0;
    for major in LLVM_MAJORS {
        let Some(clang) = clang(major) else { continue };
        // `b.c`'s `regparm` and `fastcall` are refused by x86-64's
        // column (`inreg`), which 32-bit x86 is classified with until it
        // has its own.
        for source in ["a.c", "dbg.c", "eh.cpp"] {
            for march in [None, Some("-march=pentium4")] {
                for emit in ["-S", "-c"] {
                    let out = scratch.join(format!("{major}-{source}-{}{emit}", march.is_some()));
                    let mut command = Command::new(&clang);
                    command.args(["--target=i686-linux-gnu", "-O0", "-g", "-emit-llvm", emit]);
                    command.args(march).arg("-o").arg(&out).arg(probe(source));
                    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
                    assert!(outcome.success(), "clang {major} {source}: {}", outcome.stderr_text());
                    let input = std::fs::read(&out).expect("clang's module");
                    // Compiled, or refused by `hod_x86` (`-g`: debug
                    // information is I10's).
                    let result = hod::compile(&input, &obj());
                    assert!(
                        result.as_ref().is_ok_and(|o| is_i386_object(o)) || refused_by_x86(&result),
                        "clang {major} {source} {march:?} {emit}: {result:?}"
                    );
                    compiled += 1;
                }
            }
        }
    }
    eprintln!("i386: {compiled} clang modules read, classified and compiled or refused by hod_x86");
}
