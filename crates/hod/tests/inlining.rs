//! Pre-opt modules (WP-021; `spec/cli.md` "Always-inline",
//! `spec/codegen.md` §9.7.1, design 0001 point 6): a front end's module
//! before LLVM's `-O0` pipeline, with its `available_externally`
//! definitions and its `alwaysinline` calls still in it.
//!
//! - The object: an `available_externally` function is referenced
//!   externally; one that is `alwaysinline` and called directly gets a
//!   local copy `<name>.alwaysinline` that the direct calls go to, while
//!   its address stays the external symbol; copies are transitive; one
//!   nothing calls is never compiled.
//! - The declines: each body whose meaning depends on being inlined is
//!   declined by its stable name when it may have been inlined, and is
//!   not when nothing in the module is `alwaysinline`; the other triggers
//!   of `-O0`'s pipeline are refused by name.
//! - End to end: a libstdc++-style extern template whose
//!   `always_inline` members no library defines, written by clang++ and
//!   by kiln++ with `-Xclang -disable-llvm-passes`, compiled by hod,
//!   linked under bfd, lld and mold (alone: it links only because of the
//!   copies; with a second object that instantiates the template: the
//!   function pointers compare equal), mixed with the objects
//!   `opt -passes='default<O0>'` + `llc -O0` write for the same input in
//!   both roles and both link orders, and with `-g` (the copies' DIEs
//!   pass `llvm-dwarfdump --verify`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/mod.rs"]
mod common;

use common::Elf;
use common::linking::Toolchain;
use hod::{CompileError, OutputKind, RelocModel, inline_reason};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const R_AARCH64_JUMP26: u32 = 282;
const R_AARCH64_CALL26: u32 = 283;

const HEAD: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
                    target triple = \"aarch64-unknown-linux-gnu\"\n";

fn options() -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..hod::Options::default()
    }
}

fn compile(body: &str) -> Result<Vec<u8>, CompileError> {
    hod::compile(format!("{HEAD}{body}").as_bytes(), &options())
}

/// The construct `body`'s module is declined by.
fn declined(body: &str) -> String {
    match compile(body) {
        Err(CompileError::Unsupported(unsupported)) => unsupported.construct.to_string(),
        Err(other) => panic!("not a decline: {other}\n{body}"),
        Ok(_) => panic!("compiled:\n{body}"),
    }
}

/// The relocations of every section to symbol `name`, as their kinds.
fn reloc_kinds_to(elf: &Elf, name: &str) -> Vec<u32> {
    let target = elf.symbol(name) as u32;
    let relas = (0..elf.sections.len()).filter(|&i| elf.sections[i].kind == common::SHT_RELA);
    relas.flat_map(|i| elf.relas_in(i)).filter(|r| r.sym == target).map(|r| r.kind).collect()
}

fn has_symbol(elf: &Elf, name: &str) -> bool {
    elf.symbols.iter().any(|s| s.name == name)
}

const LAYOUT: &str = r"
@fp = global ptr @ai

define available_externally i32 @plain(i32 %x) {
  ret i32 %x
}

define available_externally i32 @ai(i32 %x) #0 {
  %y = call i32 @ai2(i32 %x)
  ret i32 %y
}

define available_externally i32 @ai2(i32 %x) #0 {
  %y = add i32 %x, 1
  ret i32 %y
}

define available_externally i32 @tail(i32 %x) #0 {
  ret i32 %x
}

define available_externally void @unused() #0 {
  call void @llvm.x86.sse2.pause()
  ret void
}
declare void @llvm.x86.sse2.pause()

define available_externally i32 @aliased(i32 %x) #0 {
  ret i32 %x
}
@al = available_externally alias i32 (i32), ptr @aliased

define i32 @user(i32 %x) {
  %a = call i32 @ai(i32 %x)
  %b = call i32 @plain(i32 %a)
  %c = call i32 @al(i32 %b)
  %d = tail call i32 @tail(i32 %c)
  ret i32 %d
}

define ptr @addr() {
  ret ptr @ai
}

attributes #0 = { alwaysinline nounwind }
";

#[test]
fn copies_are_local_and_take_the_direct_calls() {
    let elf = Elf::parse(&compile(LAYOUT).expect("the layout module compiles"));
    // The copies: local functions with code, for the called functions and
    // for what a copy calls.
    for name in ["ai", "ai2", "tail"] {
        let copy = &elf.symbols[elf.symbol(&format!("{name}{}", hod_cg::COPY_SUFFIX))];
        assert_eq!((copy.bind, copy.kind), (0, 2), "{name}: STB_LOCAL STT_FUNC");
        assert!(copy.size > 0 && copy.shndx != 0, "{name}: defined with code");
    }
    // The external symbols: undefined, never defined here.
    for name in ["ai", "plain", "al"] {
        assert_eq!(elf.symbols[elf.symbol(name)].shndx, 0, "{name} is undefined");
    }
    // Nothing for what is not alwaysinline, not called, or reached only
    // through an alias.
    for name in ["plain", "unused", "aliased"] {
        assert!(!has_symbol(&elf, &format!("{name}{}", hod_cg::COPY_SUFFIX)), "{name}");
    }
    assert!(!has_symbol(&elf, "unused") && !has_symbol(&elf, "aliased"));
    // The direct calls go to the copy; the address is the external symbol.
    let calls = reloc_kinds_to(&elf, "ai.alwaysinline");
    assert!(!calls.is_empty(), "no call to the copy");
    assert!(calls.iter().all(|&k| k == R_AARCH64_CALL26 || k == R_AARCH64_JUMP26), "{calls:?}");
    assert_eq!(reloc_kinds_to(&elf, "ai2.alwaysinline"), [R_AARCH64_CALL26]);
    let address = reloc_kinds_to(&elf, "ai");
    assert!(address.len() >= 2, "the address in @fp and in @addr: {address:?}");
    assert!(address.iter().all(|&k| k != R_AARCH64_CALL26 && k != R_AARCH64_JUMP26));
    // A call to `plain` is a call to the external symbol, as llc's.
    assert_eq!(reloc_kinds_to(&elf, "plain"), [R_AARCH64_CALL26]);
}

#[test]
fn an_uncalled_copy_is_never_compiled() {
    // `unused` holds a construct hod refuses; nothing calls it above, so
    // the module compiles. Called, it is judged like any body.
    let called = LAYOUT.replace(
        "define ptr @addr() {",
        "define void @g() {\n  call void @unused()\n  ret void\n}\n\ndefine ptr @addr() {",
    );
    assert_eq!(declined(&called), "intrinsic:llvm.x86.sse2.pause");
}

#[test]
fn a_malformed_discarded_body_is_still_malformed() {
    // Verified although it is never compiled.
    let body = "define available_externally i32 @f(i32 %x) {\n  ret i64 0\n}\n";
    let error = compile(body).expect_err("a return of the wrong type");
    assert!(matches!(error, CompileError::MalformedInput(_)), "{error}");
}

/// A function `@s` of linkage `linkage` with attributes `#0` and body
/// `body` (returning a `ptr`), and a caller.
fn sensitive(body: &str, extra: &str, linkage: &str, attrs: &str) -> String {
    format!(
        "define {linkage} ptr @s() #0 {{\n{body}\n}}\n\
         define ptr @caller() {{\n  %p = call ptr @s()\n  ret ptr %p\n}}\n\
         {extra}\nattributes #0 = {{ {attrs} }}\n"
    )
}

/// Each body whose meaning depends on being inlined, with its reason.
const SENSITIVE: [(&str, &str, &str); 6] = [
    (
        inline_reason::FRAME_ADDRESS,
        "  %p = call ptr @llvm.frameaddress.p0(i32 0)\n  ret ptr %p",
        "declare ptr @llvm.frameaddress.p0(i32 immarg)",
    ),
    (
        inline_reason::RETURN_ADDRESS,
        "  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p",
        "declare ptr @llvm.returnaddress(i32 immarg)",
    ),
    (
        inline_reason::STACKSAVE,
        "  %p = call ptr @llvm.stacksave.p0()\n  ret ptr %p",
        "declare ptr @llvm.stacksave.p0()",
    ),
    (
        inline_reason::RETURNS_TWICE,
        "  %b = alloca [32 x i64]\n  %r = call i32 @setjmp(ptr %b)\n  ret ptr null",
        "declare i32 @setjmp(ptr) #1\nattributes #1 = { returns_twice }",
    ),
    (inline_reason::LOCAL_ADDRESS, "  %a = alloca i32\n  ret ptr %a", ""),
    (
        inline_reason::FRAME_REGISTER_ASM,
        // An empty template: independent of what `hod_asm` assembles.
        "  call void asm sideeffect \"\", \"~{x29}\"()\n  ret ptr null",
        "",
    ),
];

#[test]
fn bodies_that_need_inlining_are_declined_by_name() {
    assert_eq!(SENSITIVE.map(|(why, _, _)| why), inline_reason::ALL);
    for (why, body, extra) in SENSITIVE {
        for linkage in ["linkonce_odr", "internal", ""] {
            let module = sensitive(body, extra, linkage, "alwaysinline nounwind");
            assert_eq!(declined(&module), why, "{linkage}");
        }
        // available_externally: its copy would be the body out of line.
        let module = sensitive(body, extra, "available_externally", "alwaysinline nounwind");
        assert_eq!(declined(&module), why, "available_externally");
        // Without `alwaysinline` anywhere, `-O0` inlined nothing: the body
        // means what it says out of line.
        let module = sensitive(body, extra, "linkonce_odr", "nounwind");
        match compile(&module) {
            Ok(_) => {}
            Err(CompileError::Unsupported(u)) => {
                assert!(!inline_reason::ALL.contains(&u.construct.to_string().as_str()), "{u}");
            }
            Err(other) => panic!("{why}: {other}"),
        }
    }
}

#[test]
fn an_alwaysinline_call_site_counts() {
    // The readers keep a call site's attributes without saying which call
    // has them: any `alwaysinline` in the module makes every direct call
    // to a sensitive body suspect.
    let (_, body, extra) = SENSITIVE[0];
    let module = sensitive(
        body,
        &format!("{extra}\nattributes #2 = {{ alwaysinline }}"),
        "linkonce_odr",
        "nounwind",
    )
    .replace("%p = call ptr @s()", "%p = call ptr @s() #2");
    assert_eq!(declined(&module), inline_reason::FRAME_ADDRESS);
}

#[test]
fn what_cannot_have_been_inlined_is_accepted() {
    let (_, body, extra) = SENSITIVE[0];
    // Only its address is taken.
    let module = sensitive(body, extra, "linkonce_odr", "alwaysinline nounwind")
        .replace("%p = call ptr @s()", "%p = getelementptr i8, ptr @s, i64 0");
    compile(&module).expect("an address is not a call");
    // A stack pointer that only `llvm.stackrestore` sees is the
    // variable-length array idiom: freed at the return either way.
    let body = "  %p = call ptr @llvm.stacksave.p0()\n  %a = alloca i8, i64 16\n  \
                call void @llvm.stackrestore.p0(ptr %p)\n  ret ptr null";
    let extra = "declare ptr @llvm.stacksave.p0()\ndeclare void @llvm.stackrestore.p0(ptr)";
    compile(&sensitive(body, extra, "linkonce_odr", "alwaysinline nounwind"))
        .expect("stacksave restored in the same body");
    // A dead available_externally copy candidate never declines.
    let (_, body, extra) = SENSITIVE[0];
    let module = format!(
        "define available_externally ptr @s() #0 {{\n{body}\n}}\n{extra}\n\
         define ptr @f() {{\n  ret ptr @s\n}}\nattributes #0 = {{ alwaysinline }}\n"
    );
    compile(&module).expect("an address of an available_externally body");
}

#[test]
fn the_other_triggers_of_the_o0_pipeline_are_refused_by_name() {
    let cases = [
        (
            "\"instrument-function-entry\"=\"__cyg_profile_func_enter\"",
            "attr:instrument-function-entry",
        ),
        (
            "\"instrument-function-entry-inlined\"=\"mcount\"",
            "attr:instrument-function-entry-inlined",
        ),
        (
            "\"instrument-function-exit\"=\"__cyg_profile_func_exit\"",
            "attr:instrument-function-exit",
        ),
        ("sanitize_alloc_token", "attr:sanitize_alloc_token"),
        ("presplitcoroutine", "attr:presplitcoroutine"),
    ];
    for (attr, name) in cases {
        let module =
            format!("define void @f() #0 {{\n  ret void\n}}\nattributes #0 = {{ {attr} }}\n");
        assert_eq!(declined(&module), name);
    }
    let coro = "declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n\
                define void @f() {\n  %t = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  ret void\n}\n";
    assert_eq!(declined(coro), "intrinsic:llvm.coro.id");
}

// End to end.

const BOX_H: &str = r"
template <class T> struct Box {
  T v;
  [[gnu::always_inline]] inline T get() const { return v + helper(); }
  [[gnu::always_inline]] static inline T helper() { return 1; }
  [[gnu::always_inline]] static int twice(int x);
};
template <class T> [[gnu::always_inline]] inline int Box<T>::twice(int x) { return 2 * x; }
extern template struct Box<int>;
";

/// Links only because the object has copies of `get`, `helper` and
/// `twice`: no library and no other object defines them.
const ALONE: &str = r#"
#include "box.h"
int main() {
  Box<int> b{41};
  return b.get() + Box<int>::twice(4) == 50 ? 0 : 1;
}
"#;

const A: &str = r#"
#include "box.h"
#include <cstdio>
using Fn = int (*)(int);
Fn from_b();
int main() {
  Box<int> b{41};
  Fn mine = &Box<int>::twice;
  std::printf("%d %d %d %d\n", b.get(), Box<int>::twice(21), mine(5), mine == from_b());
  return mine == from_b() ? 0 : 1;
}
"#;

const B: &str = r#"
#include "box.h"
template struct Box<int>;
using Fn = int (*)(int);
Fn from_b() { return &Box<int>::twice; }
"#;

fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its half");
            None
        }
    }
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}{}", ran.stdout_text(), ran.stderr_text());
    ran.stdout_text()
}

struct Tools {
    gxx: PathBuf,
    opt: PathBuf,
    llc: PathBuf,
    dwarfdump: PathBuf,
    toolchain: Toolchain,
}

impl Tools {
    fn find(dir: &Scratch) -> Option<Self> {
        let toolchain = Toolchain::host(dir)?;
        Some(Self {
            gxx: tool("g++")?,
            opt: tool("opt")?,
            llc: tool("llc")?,
            dwarfdump: tool("llvm-dwarfdump")?,
            toolchain,
        })
    }

    /// The pre-opt module `front` writes for `source`.
    fn pre_opt(dir: &Scratch, front: &Path, name: &str, source: &str, g: bool) -> PathBuf {
        let cpp = dir.write(&format!("{name}.cpp"), source);
        let ll = dir.join(format!("{name}.ll"));
        let mut command = Command::new(front);
        command.args(["-S", "-emit-llvm", "-O0", "-Xclang", "-disable-llvm-passes"]);
        command.args(g.then_some("-g")).arg(format!("-I{}", dir.path().display()));
        command.arg(&cpp).arg("-o").arg(&ll);
        run_ok(&mut command, name);
        let text = std::fs::read_to_string(&ll).unwrap();
        let explicit = source.contains("template struct");
        assert!(explicit || text.contains("available_externally"), "{name}: not pre-opt");
        ll
    }

    fn hod(dir: &Scratch, ll: &Path) -> PathBuf {
        let text = std::fs::read(ll).unwrap();
        let object =
            hod::compile(&text, &options()).unwrap_or_else(|e| panic!("{}: {e}", ll.display()));
        let name = ll.file_stem().unwrap().to_string_lossy();
        dir.write(&format!("{name}.hod.o"), object)
    }

    /// Today's route: `opt -passes='default<O0>'`, then `llc -O0`.
    fn llvm(&self, dir: &Scratch, ll: &Path) -> PathBuf {
        let name = ll.file_stem().unwrap().to_string_lossy().into_owned();
        let bc = dir.join(format!("{name}.opt.bc"));
        let mut opt = Command::new(&self.opt);
        opt.arg("-passes=default<O0>").arg(ll).arg("-o").arg(&bc);
        run_ok(&mut opt, "opt");
        let object = dir.join(format!("{name}.llc.o"));
        let mut llc = Command::new(&self.llc);
        llc.args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"]);
        llc.args(["-filetype=obj", "-o"]).arg(&object).arg(&bc);
        run_ok(&mut llc, "llc");
        object
    }

    /// Links `objects` under every linker and runs the program: its
    /// output under each.
    fn link_run(&self, dir: &Scratch, label: &str, objects: &[&Path]) -> Vec<String> {
        let mut outputs = Vec::new();
        for linker in &self.toolchain.linkers {
            let exe = dir.join(format!("{label}-{}", linker.name));
            let mut link = Command::new(&self.gxx);
            link.arg(format!("-fuse-ld={}", linker.name))
                .arg(format!("-B{}", dir.join(format!("use-{}", linker.name)).display()))
                .arg("-pie")
                .args(objects)
                .arg("-o")
                .arg(&exe);
            run_ok(&mut link, &format!("{label}: {} link", linker.name));
            outputs.push(run_ok(&mut Command::new(&exe), &format!("{label}: {}", linker.name)));
        }
        outputs
    }
}

fn fronts() -> Vec<(&'static str, PathBuf)> {
    let mut fronts: Vec<_> = tool("clang++").map(|c| ("clang", c)).into_iter().collect();
    fronts.extend(kiln().map(|k| ("kiln", k)));
    fronts
}

#[test]
fn extern_template_always_inline_members_link_and_keep_their_identity() {
    let dir = Scratch::new("hod-inlining-e2e");
    let Some(tools) = Tools::find(&dir) else { return };
    dir.write("box.h", BOX_H);
    for (who, front) in fronts() {
        let alone = Tools::pre_opt(&dir, &front, &format!("alone-{who}"), ALONE, false);
        let alone = Tools::hod(&dir, &alone);
        for out in tools.link_run(&dir, &format!("alone-{who}"), &[&alone]) {
            assert_eq!(out, "", "{who}");
        }
        let a = Tools::pre_opt(&dir, &front, &format!("a-{who}"), A, false);
        let b = Tools::pre_opt(&dir, &front, &format!("b-{who}"), B, false);
        let (a_hod, b_hod) = (Tools::hod(&dir, &a), Tools::hod(&dir, &b));
        let (a_llc, b_llc) = (tools.llvm(&dir, &a), tools.llvm(&dir, &b));
        let expected = "42 42 10 1\n";
        let mixes: [(&str, [&Path; 2]); 5] = [
            ("hod-hod", [&a_hod, &b_hod]),
            ("hod-llc", [&a_hod, &b_llc]),
            ("llc-hod", [&a_llc, &b_hod]),
            ("llc-hod-reversed", [&b_hod, &a_llc]),
            ("hod-llc-reversed", [&b_llc, &a_hod]),
        ];
        for (label, objects) in mixes {
            for out in tools.link_run(&dir, &format!("{label}-{who}"), &objects) {
                assert_eq!(out, expected, "{who} {label}");
            }
        }
    }
}

#[test]
fn copies_carry_valid_debug_information() {
    let dir = Scratch::new("hod-inlining-debug");
    let Some(tools) = Tools::find(&dir) else { return };
    dir.write("box.h", BOX_H);
    for (who, front) in fronts() {
        let a = Tools::pre_opt(&dir, &front, &format!("ga-{who}"), A, true);
        let b = Tools::pre_opt(&dir, &front, &format!("gb-{who}"), B, true);
        let (a_hod, b_hod) = (Tools::hod(&dir, &a), Tools::hod(&dir, &b));
        for object in [&a_hod, &b_hod] {
            let mut verify = Command::new(&tools.dwarfdump);
            verify.arg("--verify").arg(object);
            run_ok(&mut verify, &format!("{who}: llvm-dwarfdump --verify"));
        }
        // The copy of `get` has a subprogram whose code is the copy's.
        let mut dump = Command::new(&tools.dwarfdump);
        dump.args(["--debug-info", "--name=get"]).arg(&a_hod);
        let info = run_ok(&mut dump, "llvm-dwarfdump --name");
        assert!(info.contains("DW_AT_low_pc"), "{who}: no code for get's copy:\n{info}");
        for out in tools.link_run(&dir, &format!("g-{who}"), &[&a_hod, &b_hod]) {
            assert_eq!(out, "42 42 10 1\n", "{who}");
        }
    }
}
