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
        // WP-188: LLVM does not inline a body with a `returns_twice`
        // call; out of line is `opt`'s result.
        if why == inline_reason::RETURNS_TWICE {
            for linkage in ["linkonce_odr", "internal", "", "available_externally"] {
                let module = sensitive(body, extra, linkage, "alwaysinline nounwind");
                compile(&module).unwrap_or_else(|e| panic!("{linkage}: {e}"));
                // WP-197: but `opt` inlines a `returns_twice` body's
                // `setjmp` call: declined.
                let module = sensitive(body, extra, linkage, "alwaysinline returns_twice");
                assert_eq!(declined(&module), why, "returns_twice {linkage}");
            }
            continue;
        }
        // WP-197: a local body (or a copy) that only inlined calls reach
        // gets its frame intrinsics' depth raised; one that may also run
        // out of line (another object's call) cannot have both meanings.
        let biased = [inline_reason::FRAME_ADDRESS, inline_reason::RETURN_ADDRESS].contains(&why);
        for linkage in ["linkonce_odr", "internal", "", "available_externally"] {
            let module = sensitive(body, extra, linkage, "alwaysinline nounwind");
            if biased && ["internal", "available_externally"].contains(&linkage) {
                compile(&module).unwrap_or_else(|e| panic!("{why} {linkage}: {e}"));
            } else {
                assert_eq!(declined(&module), why, "{linkage}");
            }
        }
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
    // `opt` inlines at a call site marked `alwaysinline` whatever the
    // callee says (WP-188).
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

/// What `hod::compile` makes of `text` and of its bitcode by each
/// LLVM's `llvm-as`: the object, or the decline's name. Text and
/// bitcode agree (the two readers decode call-site attributes apart).
fn outcome(text: &str) -> Result<Vec<u8>, String> {
    let module = format!("{HEAD}{text}");
    let of = |input: &[u8]| match hod::compile(input, &options()) {
        Ok(object) => Ok(object),
        Err(CompileError::Unsupported(u)) => Err(u.construct.to_string()),
        Err(other) => panic!("{other}\n{text}"),
    };
    let from_text = of(module.as_bytes());
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
        let mut command = Command::new(llvm_as);
        command.args(["-o", "-", "-"]);
        let ran = run_with(&mut command, module.as_bytes(), Duration::from_mins(1));
        assert!(ran.success(), "llvm-as-{major}: {}\n{module}", ran.stderr_text());
        assert!(of(&ran.stdout) == from_text, "LLVM {major} bitcode and text differ:\n{text}");
    }
    from_text
}

/// WP-188, `spec/cli.md` "Always-inline": a direct call declines exactly
/// when `opt -passes=always-inline` (19 and 22) inlines it: the callee or
/// the call site is `alwaysinline` and the site is not `noinline`, and
/// the body is one LLVM can inline.
#[test]
fn the_decision_is_opt_s_per_call_site() {
    let ra = "declare ptr @llvm.returnaddress(i32 immarg)";
    let body_of = |attrs: &str, params: &str, body: &str| {
        format!("define linkonce_odr ptr @s({params}) {attrs} {{\n{body}\n}}\n{ra}\n")
    };
    let plain = "  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p";
    let caller = |call: &str| format!("define ptr @caller() {{\n  %p = {call}\n  ret ptr %p\n}}\n");
    let marker = "define void @other() #9 {\n  ret void\n}\nattributes #9 = { alwaysinline }\n";
    let why = inline_reason::RETURN_ADDRESS;
    let cases: Vec<(String, Option<&str>)> = vec![
        // `noinline` callee, plain call, `alwaysinline` elsewhere: kept.
        (body_of("noinline", "", plain) + &caller("call ptr @s()") + marker, None),
        // `noinline` callee, call site `alwaysinline` (group and in place).
        (
            body_of("noinline", "", plain)
                + &caller("call ptr @s() #8")
                + "attributes #8 = { alwaysinline }\n",
            Some(why),
        ),
        (body_of("noinline", "", plain) + &caller("call ptr @s() alwaysinline"), Some(why)),
        // `alwaysinline` callee, call site `noinline` (group, in place,
        // a group that also holds other attributes): kept.
        (
            body_of("alwaysinline", "", plain)
                + &caller("call ptr @s() #8")
                + "attributes #8 = { noinline }\n",
            None,
        ),
        (body_of("alwaysinline", "", plain) + &caller("call ptr @s() noinline"), None),
        (
            body_of("alwaysinline", "", plain)
                + &caller("call ptr @s() #8")
                + "attributes #8 = { nounwind noinline }\n",
            None,
        ),
        // Called once kept, once inlined: declined.
        (
            body_of("alwaysinline", "", plain)
                + &caller("call ptr @s() noinline")
                + "define ptr @second() {\n  %p = call ptr @s()\n  ret ptr %p\n}\n",
            Some(why),
        ),
        // Bodies LLVM does not inline: variadic, recursive, `indirectbr`,
        // `llvm.localescape`.
        (body_of("alwaysinline", "...", plain) + &caller("call ptr (...) @s()"), None),
        (
            body_of(
                "alwaysinline",
                "",
                "  %p = call ptr @llvm.returnaddress(i32 0)\n  %q = call ptr @s()\n  ret ptr %p",
            ) + &caller("call ptr @s()"),
            None,
        ),
        (
            body_of(
                "alwaysinline",
                "",
                "  %p = call ptr @llvm.returnaddress(i32 0)\n  indirectbr ptr blockaddress(@s, %l), [label %l]\nl:\n  ret ptr %p",
            ) + &caller("call ptr @s()"),
            None,
        ),
        (
            body_of(
                "alwaysinline",
                "",
                "  %a = alloca i32\n  call void (...) @llvm.localescape(ptr %a)\n  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p",
            ) + &caller("call ptr @s()")
                + "declare void @llvm.localescape(...)\n",
            None,
        ),
        // A plain call of an `alwaysinline` callee: declined.
        (body_of("alwaysinline", "", plain) + &caller("call ptr @s()"), Some(why)),
    ];
    for (text, want) in &cases {
        match (outcome(text), want) {
            (Ok(_), None) => {}
            // Kept, and refused for a reason of its own (`localescape`).
            (Err(name), None) if !inline_reason::ALL.contains(&name.as_str()) => {}
            (Err(name), Some(want)) if name == *want => {}
            (got, want) => panic!("{want:?}, got {:?}:\n{text}", got.map(|_| "an object")),
        }
    }
}

/// WP-197 (`spec/cli.md` "Always-inline", frame bias): inlined,
/// `llvm.returnaddress(n)` and `llvm.frameaddress(n)` speak of the frame
/// the inlined code runs in; hod gives a body that runs only there, `b`
/// frames below it, depth `n + b`. The object is the one of the same
/// module with the deeper depth written in and every call kept.
#[test]
fn a_frame_bias_gives_the_inlined_frame() {
    let decls = "declare ptr @llvm.returnaddress(i32 immarg)\n\
                 declare ptr @llvm.frameaddress.p0(i32 immarg)\n";
    // `@s` reads both intrinsics at depths `d` and `d + 1`; `@t` calls it;
    // `@caller` calls `@t` (or `@s` when `via` is false).
    let module = |d: u32, via: bool, site: &str| {
        let e = d + 1;
        let target = if via { "t" } else { "s" };
        format!(
            "define internal ptr @s() alwaysinline {{\n  \
               %r = call ptr @llvm.returnaddress(i32 {d})\n  \
               %f = call ptr @llvm.frameaddress.p0(i32 {e})\n  \
               %x = ptrtoint ptr %f to i64\n  %p = getelementptr i8, ptr %r, i64 %x\n  ret ptr %p\n}}\n\
             define internal ptr @t() alwaysinline {{\n  %p = call ptr @s(){site}\n  ret ptr %p\n}}\n\
             define ptr @caller() {{\n  %p = call ptr @{target}(){site}\n  ret ptr %p\n}}\n{decls}"
        )
    };
    // Bias 1: `@s` called by `@caller`. `@t` is local and nothing names
    // it: it runs nowhere, so its call adds no frame.
    let biased = compile(&module(0, false, "")).expect("bias 1");
    let written = compile(&module(1, false, " noinline")).expect("written deeper");
    assert_eq!(biased, written, "bias 1");
    // Bias 2: `@t` runs only inlined in `@caller`, `@s` only inlined in
    // `@t`.
    let biased = compile(&module(0, true, "")).expect("bias 2");
    let written = compile(&module(2, true, " noinline")).expect("written deeper");
    assert_eq!(biased, written, "bias 2");
    // An `available_externally` copy that only inlined calls reach.
    let copy = |d: u32, site: &str| {
        module(d, false, site)
            .replace("define internal ptr @s()", "define available_externally ptr @s()")
    };
    let biased = compile(&copy(0, "")).expect("copy, bias 1");
    let written = compile(&copy(1, " noinline")).expect("copy written deeper");
    assert_eq!(biased, written, "copy");
}

/// WP-197: where a body may run in more than one frame relative to the
/// inlined one, or hod cannot tell that LLVM inlines it, it keeps the
/// decline.
#[test]
fn no_frame_bias_where_the_frame_is_not_one() {
    let why = inline_reason::RETURN_ADDRESS;
    let s = "define internal ptr @s() #0 {\n  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p\n}\n\
             declare ptr @llvm.returnaddress(i32 immarg)\nattributes #0 = { alwaysinline }\n";
    let caller = |call: &str| format!("define ptr @caller() {{\n  %p = {call}\n  ret ptr %p\n}}\n");
    let cases: Vec<(&str, String)> = vec![
        // Also called where LLVM keeps the call: two meanings.
        (
            "kept call",
            format!(
                "{s}{}define ptr @other() {{\n  %p = call ptr @s() noinline\n  ret ptr %p\n}}\n",
                caller("call ptr @s()")
            ),
        ),
        // Its address taken in code, in data.
        (
            "address in code",
            format!(
                "{s}{}@g = global ptr null\ndefine void @h() {{\n  store ptr @s, ptr @g\n  ret void\n}}\n",
                caller("call ptr @s()")
            ),
        ),
        ("address in data", format!("{s}{}@g = global ptr @s\n", caller("call ptr @s()"))),
        (
            "used",
            format!(
                "{s}{}@llvm.used = appending global [1 x ptr] [ptr @s], section \"llvm.metadata\"\n",
                caller("call ptr @s()")
            ),
        ),
        // Not certainly inlined: another function type, a bundle, a
        // musttail call, another personality.
        ("function type", format!("{s}{}", caller("call ptr @s(i32 1)"))),
        ("bundle", format!("{s}{}", caller("call ptr @s() [ \"x\"(i32 1) ]"))),
        ("musttail", format!("{s}{}", caller("musttail call ptr @s()"))),
        // Reached at two depths: directly and through `@t`, which is also
        // inlined.
        (
            "two depths",
            format!(
                "{s}define internal ptr @t() #0 {{\n  %p = call ptr @s()\n  ret ptr %p\n}}\n{}\
                 define ptr @d() {{\n  %p = call ptr @t()\n  ret ptr %p\n}}\n",
                caller("call ptr @s()")
            ),
        ),
        // Through an external `@t`, which other objects call out of line.
        (
            "external intermediate",
            format!(
                "{s}define ptr @t() #0 {{\n  %p = call ptr @s()\n  ret ptr %p\n}}\n{}",
                caller("call ptr @t()")
            ),
        ),
        // A cycle of inlined calls.
        (
            "cycle",
            format!(
                "define internal ptr @s() #0 {{\n  %p = call ptr @llvm.returnaddress(i32 0)\n  \
                   %q = call ptr @t()\n  ret ptr %p\n}}\n\
                 define internal ptr @t() #0 {{\n  %p = call ptr @s()\n  ret ptr %p\n}}\n\
                 declare ptr @llvm.returnaddress(i32 immarg)\nattributes #0 = {{ alwaysinline }}\n{}",
                caller("call ptr @s()")
            ),
        ),
        // A block address of its own: LLVM inlines it only for `callbr`.
        (
            "block address",
            format!(
                "define internal ptr @s() #0 {{\n  br label %l\nl:\n  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p\n}}\n\
                 @b = global ptr blockaddress(@s, %l)\n\
                 declare ptr @llvm.returnaddress(i32 immarg)\nattributes #0 = {{ alwaysinline }}\n{}",
                caller("call ptr @s()")
            ),
        ),
        // Another inlining-sensitive construct beside it.
        (
            "and a stack pointer",
            format!(
                "define internal ptr @s() #0 {{\n  %p = call ptr @llvm.returnaddress(i32 0)\n  \
                   %q = call ptr @llvm.stacksave.p0()\n  ret ptr %q\n}}\n\
                 declare ptr @llvm.returnaddress(i32 immarg)\ndeclare ptr @llvm.stacksave.p0()\n\
                 attributes #0 = {{ alwaysinline }}\n{}",
                caller("call ptr @s()")
            ),
        ),
    ];
    for (what, text) in &cases {
        match outcome(text) {
            Err(name)
                if name == why
                    || (*what == "and a stack pointer"
                        && name == inline_reason::RETURN_ADDRESS) => {}
            Err(name)
                if *what == "function type" && !inline_reason::ALL.contains(&name.as_str()) => {}
            // `classify` refuses every operand bundle for now.
            Err(name) if *what == "bundle" && name.starts_with("bundle:") => {}
            got => panic!("{what}: {:?}\n{text}", got.map(|_| "an object")),
        }
    }
    // A personality the callee has and the caller has another of.
    let pers = format!(
        "declare i32 @__gxx_personality_v0(...)\ndeclare i32 @__gcc_personality_v0(...)\n\
         define internal ptr @s() #0 personality ptr @__gxx_personality_v0 {{\n  %p = call ptr @llvm.returnaddress(i32 0)\n  ret ptr %p\n}}\n\
         declare ptr @llvm.returnaddress(i32 immarg)\nattributes #0 = {{ alwaysinline }}\n\
         define ptr @caller() personality ptr @__gcc_personality_v0 {{\n  %p = call ptr @s()\n  ret ptr %p\n}}\n"
    );
    assert_eq!(outcome(&pers).err().as_deref(), Some(why), "personality");
    // The same personality: biased.
    outcome(
        &pers.replace(
            "personality ptr @__gcc_personality_v0",
            "personality ptr @__gxx_personality_v0",
        ),
    )
    .expect("one personality");
}

/// WP-197: `opt` (19 and 22) inlines an `alwaysinline` body whose
/// `setjmp` is an `invoke`, or that is `returns_twice` itself; hod does
/// not inline, so it declines. A `call` of `setjmp` in a plain body keeps
/// the call (WP-188).
#[test]
fn a_setjmp_opt_inlines_is_declined() {
    let why = inline_reason::RETURNS_TWICE;
    let invoke = "declare i32 @setjmp(ptr) returns_twice\ndeclare i32 @__gxx_personality_v0(...)\n\
                  @buf = global [64 x i64] zeroinitializer\n\
                  define internal i32 @a() alwaysinline personality ptr @__gxx_personality_v0 {\n  \
                    %r = invoke i32 @setjmp(ptr @buf) to label %ok unwind label %lp\nok:\n  ret i32 %r\n\
                  lp:\n  %l = landingpad { ptr, i32 } cleanup\n  resume { ptr, i32 } %l\n}\n\
                  define i32 @caller() {\n  %r = call i32 @a()\n  ret i32 %r\n}\n";
    assert_eq!(outcome(invoke).err().as_deref(), Some(why), "invoke");
    let call = invoke.replace(
        "%r = invoke i32 @setjmp(ptr @buf) to label %ok unwind label %lp",
        "%r = call i32 @setjmp(ptr @buf)\n  br label %ok",
    );
    outcome(&call).expect("a call of setjmp: kept");
    let own = call.replace(
        "define internal i32 @a() alwaysinline",
        "define internal i32 @a() alwaysinline returns_twice",
    );
    assert_eq!(outcome(&own).err().as_deref(), Some(why), "returns_twice body");
    // A `returns_twice` attribute on the call site alone, indirect.
    let indirect = "@buf = global [64 x i64] zeroinitializer\n@fp = global ptr null\n\
                    define internal i32 @a() alwaysinline {\n  %f = load ptr, ptr @fp\n  \
                      %r = call i32 %f(ptr @buf) returns_twice\n  ret i32 %r\n}\n\
                    define i32 @caller() {\n  %r = call i32 @a()\n  ret i32 %r\n}\n";
    outcome(indirect).expect("an indirect returns_twice call: kept");
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
        // Coroutine bodies are compiled since WP-106: what the plan
        // refuses is refused by its `coro:*` name (here, no `coro.id`).
        ("presplitcoroutine", "coro:begin-shape"),
    ];
    for (attr, name) in cases {
        let module =
            format!("define void @f() #0 {{\n  ret void\n}}\nattributes #0 = {{ {attr} }}\n");
        assert_eq!(declined(&module), name);
    }
    let coro = "declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n\
                define void @f() {\n  %t = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  ret void\n}\n";
    assert_eq!(declined(coro), "coro:outside-coroutine");
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

/// WP-197: the frame intrinsics of `always_inline` bodies (C, clang's
/// pre-opt module): each inlined value equals the caller's own (`same`),
/// and each return address lies in `main` (by the linked program's
/// symbol table).
const FRAME_C: &str = r#"
#include <stdio.h>
#include <stdint.h>

/* Inlined by opt's always-inliner: the intrinsics speak of the caller. */
static inline __attribute__((always_inline)) void *ra0(void) { return __builtin_return_address(0); }
static inline __attribute__((always_inline)) void *fa0(void) { return __builtin_frame_address(0); }
static inline __attribute__((always_inline)) void *ra1(void) { return __builtin_return_address(1); }
static inline __attribute__((always_inline)) void *fa1(void) { return __builtin_frame_address(1); }
/* Two levels: a always_inline calls b always_inline. */
static inline __attribute__((always_inline)) void *ra_leaf(void) { return __builtin_return_address(0); }
static inline __attribute__((always_inline)) void *ra_nested(void) { return ra_leaf(); }
static inline __attribute__((always_inline)) void *fa_leaf(void) { return __builtin_frame_address(0); }
static inline __attribute__((always_inline)) void *fa_nested2(void) { return fa_leaf(); }
static inline __attribute__((always_inline)) void *fa_nested(void) { return fa_nested2(); }
/* A body with locals and a loop around the intrinsic. */
static inline __attribute__((always_inline)) int ra_sum(void **out, int n) {
  int s = 0;
  for (int i = 0; i < n; i++) { s += i; out[i] = __builtin_return_address(0); }
  return s;
}

static void report(const char *what, void *got, void *want) {
  printf("%s %s %ld\n", what, got == want ? "same" : "DIFFERENT", (long)((char *)got - (char *)want));
}

/* Each prints whether the inlined value equals the caller's own. */
__attribute__((noinline)) void *f_ra0(void) { void *own = __builtin_return_address(0); report("ra0", ra0(), own); return ra0(); }
__attribute__((noinline)) void f_fa0(void) { report("fa0", fa0(), __builtin_frame_address(0)); }
__attribute__((noinline)) void *f_ra1(void) { report("ra1", ra1(), __builtin_return_address(1)); return ra1(); }
__attribute__((noinline)) void f_fa1(void) { report("fa1", fa1(), __builtin_frame_address(1)); }
__attribute__((noinline)) void *f_nested(void) { report("ra-nested", ra_nested(), __builtin_return_address(0)); return ra_nested(); }
__attribute__((noinline)) void f_fa_nested(void) { report("fa-nested", fa_nested(), __builtin_frame_address(0)); }
__attribute__((noinline)) void f_sum(void) {
  void *out[4];
  int s = ra_sum(out, 4);
  printf("sum %d\n", s);
  for (int i = 0; i < 4; i++) report("ra-loop", out[i], __builtin_return_address(0));
}
__attribute__((noinline)) void *g_ra1(void) { return f_ra1(); }

/* Addresses relative to main, for the symbol-range check. */
int main(void) {
  void *r = f_ra0();
  printf("off main ra0 %ld\n", (long)((char *)r - (char *)main));
  f_fa0();
  r = g_ra1();
  printf("off main ra1 %ld\n", (long)((char *)r - (char *)main));
  f_fa1();
  r = f_nested();
  printf("off main nested %ld\n", (long)((char *)r - (char *)main));
  f_fa_nested();
  f_sum();
  return 0;
}
"#;

/// WP-197: `setjmp`/`longjmp` round trips with locals changed between,
/// `always_inline` helpers inlined into the `setjmp` caller, and an
/// `always_inline` body with a `setjmp` call (`opt` keeps that call).
const JMP_C: &str = r#"
#include <setjmp.h>
#include <stdio.h>

static jmp_buf buf;

/* opt inlines these into the setjmp caller; hod calls them. */
static inline __attribute__((always_inline)) void bump(int *p, volatile int *q) { *p += 10; *q += 100; }
static inline __attribute__((always_inline)) int twice(int x) { return 2 * x; }

__attribute__((noinline)) void jump(int v) { longjmp(buf, v); }

/* opt keeps the call (a body with a setjmp `call` is not inlined). */
static inline __attribute__((always_inline)) int guarded(void (*cb)(int), int v) {
  volatile int seen = 0;
  if (setjmp(buf) == 0) { seen = 1; cb(v); return -1; }
  return seen + 1000;
}

__attribute__((noinline)) int round_trip(int n) {
  int plain = 1;
  volatile int vol = 2;
  int arr[3] = {1, 2, 3};
  int r = setjmp(buf);
  if (r == 0) {
    plain = twice(plain) + 5;
    vol = twice(vol);
    bump(&plain, &vol);
    arr[1] = 42;
    jump(n);
  }
  printf("r=%d plain=%d vol=%d arr=%d,%d,%d twice=%d\n", r, plain, vol, arr[0], arr[1], arr[2], twice(r));
  return r;
}

__attribute__((noinline)) int loop_jumps(void) {
  volatile int count = 0;
  int local = 0;
  setjmp(buf);
  count = count + 1;
  local = local + twice(1);
  if (count < 5) jump(count);
  printf("count=%d local=%d\n", count, local);
  return count;
}

static void cb(int v) { jump(v); }

int main(void) {
  round_trip(7);
  loop_jumps();
  printf("guarded=%d\n", guarded(cb, 3));
  return 0;
}
"#;

/// WP-197 (`spec/cli.md` "Always-inline", frame bias and
/// `returns_twice`): [`FRAME_C`] and [`JMP_C`] built from clang's pre-opt
/// module two ways, by hod and by `opt -passes=always-inline` then
/// `llc -O0`, for AArch64 and x86-64 (under qemu off its host), with and
/// without `-g`: the same output, every `same` and no `DIFFERENT`, every
/// return address inside `main`.
#[test]
fn frame_intrinsics_and_setjmp_behave_as_opt_inlined_them() {
    let (Some(clang), Some(opt), Some(llc), Some(nm)) =
        (tool("clang"), tool("opt"), tool("llc"), tool("llvm-nm"))
    else {
        return;
    };
    for target in hod_testutil::Target::ALL {
        let dir = Scratch::new(&format!("hod-inlining-frame-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let linker = &toolchain.linkers[0];
        for (name, source) in [("frame", FRAME_C), ("jmp", JMP_C)] {
            for g in [false, true] {
                let label = format!("{name}-{}{}", target.name(), if g { "-g" } else { "" });
                let c = dir.write(&format!("{label}.c"), source);
                let ll = dir.join(format!("{label}.ll"));
                let mut front = Command::new(&clang);
                front.arg(format!("--target={}-unknown-linux-gnu", target.name()));
                front.args(["-S", "-emit-llvm", "-O0", "-Xclang", "-disable-llvm-passes"]);
                front.args(g.then_some("-g")).arg(&c).arg("-o").arg(&ll);
                run_ok(&mut front, &label);
                let hod_o = Tools::hod(&dir, &ll);
                let bc = dir.join(format!("{label}.opt.bc"));
                let mut inline = Command::new(&opt);
                inline.arg("-passes=always-inline").arg(&ll).arg("-o").arg(&bc);
                run_ok(&mut inline, "opt");
                let llc_o = dir.join(format!("{label}.llc.o"));
                let mut back = Command::new(&llc);
                back.args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"]);
                back.args(["-filetype=obj", "-o"]).arg(&llc_o).arg(&bc);
                run_ok(&mut back, "llc");
                let mut outputs = Vec::new();
                for (how, object) in [("hod", &hod_o), ("llc", &llc_o)] {
                    let exe = dir.join(format!("{label}-{how}"));
                    let ran = toolchain.link_and_run(linker, &["-pie"], &[object], &exe);
                    assert!(ran.success(), "{label} {how}: {}", ran.stderr_text());
                    let out = ran.stdout_text();
                    assert!(!out.contains("DIFFERENT"), "{label} {how}:\n{out}");
                    // `off main <what> <n>`: `n` bytes after `main`, inside it.
                    let mut symbols = Command::new(&nm);
                    symbols.arg("-S").arg(&exe);
                    let table = run_ok(&mut symbols, "llvm-nm");
                    let main_size = table
                        .lines()
                        .find_map(|line| match line.split_whitespace().collect::<Vec<_>>()[..] {
                            [_, size, _, "main"] => u64::from_str_radix(size, 16).ok(),
                            _ => None,
                        })
                        .expect("main in the symbol table");
                    let mut kept = String::new();
                    for line in out.lines() {
                        if let Some(rest) = line.strip_prefix("off main ") {
                            let n: i64 = rest.rsplit(' ').next().unwrap().parse().unwrap();
                            assert!(
                                n > 0 && n.unsigned_abs() < main_size,
                                "{label} {how}: {line} outside main ({main_size} bytes)"
                            );
                        } else {
                            kept.push_str(line);
                            kept.push('\n');
                        }
                    }
                    outputs.push(kept);
                }
                assert_eq!(outputs[0], outputs[1], "{label}: hod and opt+llc differ");
                assert!(outputs[0].lines().count() >= 3, "{label}");
            }
        }
    }
}
