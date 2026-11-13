//! Pre-opt modules with coroutines (WP-109; `spec/cli.md` "Pre-opt
//! modules", design 0005 §6 C6): a front end hands hod the module before
//! LLVM's `-O0` pipeline when coroutines (and `alwaysinline`) are all that
//! pipeline would lower, and runs `opt` + `llc` itself when hod declines.
//!
//! - A coroutine hod compiles is compiled from the pre-opt module.
//! - Every coroutine refusal is decided BEFORE code generation: the
//!   session's answer comes without the `bodies` phase (which ends
//!   judging and starts code generation), and the name is the one code
//!   generation itself gives the same body (`hod::generate`, which skips
//!   the session), so the early check and the backstop agree.
//! - A target without coroutine bodies declines the module at its module
//!   level, before any body is read: a body that does not even parse
//!   after it is never seen.

use hod::{CompileError, OptLevel, Options, OutputKind, RelocModel, Report, Settings};
use hod_ir::{AARCH64_LINUX_DATA_LAYOUT, Arch, Support, X86_64_LINUX_DATA_LAYOUT};

fn options() -> Options {
    Options {
        output_kind: OutputKind::Object,
        opt_level: OptLevel::O0,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    }
}

fn header(arch: Arch) -> String {
    let (layout, triple) = match arch {
        Arch::Aarch64 => (AARCH64_LINUX_DATA_LAYOUT, "aarch64-unknown-linux-gnu"),
        Arch::X86_64 => (X86_64_LINUX_DATA_LAYOUT, "x86_64-unknown-linux-gnu"),
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n")
}

const DECLS: &str = r"
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare ptr @malloc(i64)
declare void @free(ptr)
declare void @use(ptr)
";

/// A coroutine of LLVM 22's dialect with calling convention `{cc}` and a
/// stack slot aligned to `{align}` that no suspend crosses (the shape of
/// `hod_a64/tests/coro_body.rs`).
fn coroutine(cc: &str, align: u32) -> String {
    format!(
        r"{DECLS}
define {cc} ptr @co(i32 %n) presplitcoroutine {{
entry:
  %slot = alloca i32, align {align}
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
  store i32 %n, ptr %slot
  %v = load i32, ptr %slot
  call void @use(ptr null)
  %save = call token @llvm.coro.save(ptr null)
  %s = call i8 @llvm.coro.suspend(token %save, i1 true)
  switch i8 %s, label %out [
    i8 0, label %cleanup
    i8 1, label %cleanup
  ]
cleanup:
  %f = call ptr @llvm.coro.free(token %id, ptr %h)
  call void @free(ptr %f)
  br label %out
out:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret ptr %h
}}
"
    )
}

/// A coroutine whose ramp return block runs both before `coro.begin`
/// (a failed allocation) and after it, and names the promise, which is
/// copied at `coro.begin`.
const SHARED_BLOCK: &str = r"
define ptr @co() presplitcoroutine {
entry:
  %p = alloca i32, align 4
  %id = call token @llvm.coro.id(i32 16, ptr %p, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %null = icmp eq ptr %mem, null
  br i1 %null, label %out, label %begin
begin:
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %save = call token @llvm.coro.save(ptr null)
  %s = call i8 @llvm.coro.suspend(token %save, i1 true)
  switch i8 %s, label %out [
    i8 0, label %cleanup
    i8 1, label %cleanup
  ]
cleanup:
  %f = call ptr @llvm.coro.free(token %id, ptr %h)
  call void @free(ptr %f)
  br label %out
out:
  %r = phi ptr [ null, %entry ], [ %h, %begin ], [ %h, %cleanup ]
  call void @use(ptr %p)
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret ptr %r
}
";

/// The session's answer for `text`, with its report.
fn judged(text: &str, settings: &Settings) -> (Result<Vec<u8>, CompileError>, Report) {
    let mut report = Report::default();
    let result = hod::compile_with(text.as_bytes(), &options(), settings, &mut report);
    (result, report)
}

fn construct(result: &Result<Vec<u8>, CompileError>, text: &str) -> String {
    match result {
        Err(CompileError::Unsupported(unsupported)) => unsupported.construct.to_string(),
        Err(other) => panic!("not a decline: {other}\n{text}"),
        Ok(_) => panic!("compiled:\n{text}"),
    }
}

fn ended(report: &Report, phase: &str) -> bool {
    report.phases.iter().any(|(name, _)| *name == phase)
}

/// What code generation alone answers for `text` (no session: the
/// backstop's name).
fn generated(text: &str) -> Result<(), String> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &parsed.text, |_, read| {
        bodies.push(hod_ir::Body::clone(read.body.expect("a body hod_ll builds")));
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    match hod::generate(&module, &bodies, &options()) {
        Ok(_) => Ok(()),
        Err(CompileError::Unsupported(unsupported)) => Err(unsupported.construct.to_string()),
        Err(other) => panic!("not a decline: {other}\n{text}"),
    }
}

// spec/cli.md "Pre-opt modules": a coroutine hod compiles is compiled from
// the module as the front end wrote it, through every phase.
#[test]
fn a_pre_opt_coroutine_is_compiled() {
    let text = format!("{}{}", header(Arch::Aarch64), coroutine("", 4));
    let (result, report) = judged(&text, &Settings::default());
    assert!(result.is_ok(), "{result:?}");
    assert!(ended(&report, "bodies") && ended(&report, "object"), "{:?}", report.phases);
}

// spec/cli.md "Pre-opt modules", "Declined early": each coroutine refusal
// is the answer before code generation starts, under the name code
// generation gives the same module.
#[test]
fn every_coroutine_refusal_is_decided_before_code_generation() {
    let a64 = header(Arch::Aarch64);
    let outside = "declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n\
                   define void @f() {\n  %t = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  ret void\n}\n";
    let cases = [
        (format!("{a64}{}", coroutine("preserve_mostcc", 4)), "a64:coro-preserve-cc"),
        (format!("{a64}{}", coroutine("preserve_allcc", 4)), "a64:coro-preserve-cc"),
        (format!("{a64}{}", coroutine("", 32)), "coro:overaligned-stack-slot"),
        (format!("{a64}{DECLS}{SHARED_BLOCK}"), "coro:begin-shared-block"),
        (format!("{a64}{outside}"), "coro:outside-coroutine"),
        // Design 0005's post-sync: the plan's refusal, in both dialects
        // (code generation runs the same plan).
        (
            include_str!("../../hod_xform/tests/data/seeds/llvm19/post-sync-19.ll").to_owned(),
            "coro:frame-access-after-suspend",
        ),
        (
            include_str!("../../hod_xform/tests/data/seeds/llvm22/post-sync-22.ll").to_owned(),
            "coro:frame-access-after-suspend",
        ),
    ];
    for (text, name) in cases {
        let (result, report) = judged(&text, &Settings::default());
        assert_eq!(construct(&result, &text), name, "{text}");
        assert!(!ended(&report, "bodies"), "{name}: judged before code generation");
        assert_eq!(generated(&text), Err(name.to_owned()), "code generation agrees: {text}");
    }
}

// spec/cli.md "Pre-opt modules", "Declined early": on a target whose
// matrix does not accept `presplitcoroutine` (x86-64 today), or whose code
// generator compiles no coroutine body, the module is declined by its
// module level: the body after it, which does not parse, is never read.
#[test]
fn a_target_without_coroutines_declines_at_the_module_level() {
    let broken = "define void @g() {\n  this is not a body\n}\n";
    let text = format!("{}{}{broken}", header(Arch::X86_64), coroutine("", 4));
    let (result, report) = judged(&text, &Settings::default());
    assert_eq!(construct(&result, &text), "attr:presplitcoroutine");
    assert!(ended(&report, "classify") && !ended(&report, "bodies"), "{:?}", report.phases);

    // The attribute accepted by the matrix, the code generator still has
    // no coroutine bodies: `coro:target`, as code generation says it.
    let support = Support::shipped(Arch::X86_64).clone().with("attr:presplitcoroutine");
    let settings = Settings { support, ..Settings::default() };
    let (result, report) = judged(&text, &settings);
    assert_eq!(construct(&result, &text), "coro:target");
    assert!(!ended(&report, "bodies"));

    // Without the broken body the AArch64 module compiles.
    let text = format!("{}{}", header(Arch::Aarch64), coroutine("", 4));
    assert!(judged(&text, &Settings::default()).0.is_ok());
}

// spec/cli.md "Pre-opt modules": the coroutine checks look only at
// modules with coroutines; a module that declares no `llvm.coro.*`
// function and no `presplitcoroutine` set is judged as before.
#[test]
fn modules_without_coroutines_are_not_affected() {
    for arch in [Arch::Aarch64, Arch::X86_64] {
        let text = format!("{}define i32 @f(i32 %x) {{\n  ret i32 %x\n}}\n", header(arch));
        let (result, _) = judged(&text, &Settings::default());
        assert!(result.is_ok(), "{arch:?}: {result:?}");
    }
}
