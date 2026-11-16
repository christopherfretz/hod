//! WP-327 (stage A): the coroutine handle operations on i386
//! (`spec/codegen-x86.md` §18). `coro/handles.ll` calls `coro.resume`,
//! `destroy`, `done`, `promise` (alignment 4 and 16, both directions)
//! and `noop` on frames built by hand with LLVM's layout, with a `double`
//! live across `resume` and `destroy` (the x87 barrier). hod compiles it
//! through the facade and under `Stress::ALL`; the oracle is the same
//! module through `opt -passes='default<O0>'` and `llc`. Static and PIE,
//! at the default CPU (no SSE) and `-mcpu=i686`, linked by GNU ld, lld
//! and mold, run under `qemu-i386`: every run prints what `llc`'s
//! prints. Coroutine bodies are still refused (`coro:target`).

use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, data, hod_objects, refusal};
use i386_support::{checked, llc_object, llvm_tool, run_with_features, tool};

#[test]
fn handle_operations_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let present = linkers::present(target);
    let scratch = Scratch::new("i386-coro");
    let source = data("coro", "handles.ll");
    let text = std::fs::read(&source).unwrap();
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(opt), Some(llc)) = (llvm_tool(major, "opt"), llvm_tool(major, "llc")) else {
            continue;
        };
        let lowered = scratch.join(format!("handles-{major}.o0.ll"));
        let mut command = Command::new(&opt);
        command.arg("-passes=default<O0>").arg("-S").arg(&source).arg("-o").arg(&lowered);
        checked(&format!("opt {major}"), &mut command);
        for mcpu in [None, Some("i686")] {
            for pic in [false, true] {
                let build = Build { pic, mcpu, ..STATIC };
                let label = format!(
                    "handles-{major}-{}-{}",
                    mcpu.unwrap_or("default"),
                    if pic { "pie" } else { "exe" }
                );
                let (plain, stress) = hod_objects(&text, build, &label);
                let mut flags = Vec::new();
                if pic {
                    flags.push("-relocation-model=pic");
                }
                if mcpu.is_some() {
                    flags.push("-mcpu=i686");
                }
                let objects = [
                    ("llc", llc_object(&scratch, &llc, &lowered, &flags, &format!("{label}-llc"))),
                    ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                    ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
                ];
                for (name, linker) in &present {
                    let mut results = Vec::new();
                    for (who, object) in &objects {
                        let exe = scratch.join(format!("{label}-{who}-{name}"));
                        let mut command = Command::new(&gcc);
                        command.args(linkers::driver_flags(scratch.path(), name, linker));
                        command.arg(if pic { "-pie" } else { "-no-pie" });
                        command.arg(object).arg("-o").arg(&exe);
                        checked(&format!("{name} link of {label} ({who})"), &mut command);
                        results.push(run_with_features(&qemu, &exe, &[]));
                        runs += 1;
                    }
                    let what = format!("{label} ({name})");
                    assert_eq!(results[0].0, Some(0), "{what}: llc's run failed");
                    assert!(
                        String::from_utf8_lossy(&results[0].1).contains("noop done 0 same 1"),
                        "{what}: llc's output"
                    );
                    for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                        assert_eq!(
                            String::from_utf8_lossy(&results[0].1),
                            String::from_utf8_lossy(&result.1),
                            "{what}: {who}'s output"
                        );
                        assert_eq!(results[0].0, result.0, "{what}: {who}'s status");
                    }
                }
            }
        }
    }
    eprintln!("i386 coro: {runs} runs of handles.ll equal llc's");
}

/// The x87 barrier shapes of the handle operations (`spec/codegen-x86.md`
/// §11, §18): `coro.resume` / `coro.destroy` of the noop frame between
/// an 80-bit product and its use.
const BARRIER_SHAPES: [(&str, &str); 3] = [
    ("resume_noop", "%h = call ptr @llvm.coro.noop()\n  call void @llvm.coro.resume(ptr %h)"),
    ("destroy_noop", "%h = call ptr @llvm.coro.noop()\n  call void @llvm.coro.destroy(ptr %h)"),
    (
        "done_noop",
        "%h = call ptr @llvm.coro.noop()\n  %d = call i1 @llvm.coro.done(ptr %h)\n  \
         %z = zext i1 %d to i64\n  store i64 %z, ptr %o2",
    ),
];

/// The barrier module of [`BARRIER_SHAPES`] for `ty` (`float` or
/// `double`) at `cpu` (none: `generic`), in the shape of the atomic
/// package's (`i386_atomic.rs`): one function per shape computes the
/// product `p` of `a` and `b` (80 bits: `1 + 2^-k` squared), stores
/// `p - n`, runs its construct, then stores `p + c`, which is `2^-2k`
/// while `p` keeps 80 bits and 0 once it is rounded. `main` prints one
/// line per shape.
fn barrier_module(ty: &str, cpu: Option<&str>) -> String {
    use std::fmt::Write as _;
    let (bits, a, c, nc) = if ty == "float" {
        ("i32", "1065355264", "-1082126336", "1065357312")
    } else {
        ("i64", "4607182418804211712", "-4616189618046369792", "4607182418808406016")
    };
    let mut t = format!(
        "target triple = \"i686-unknown-linux-gnu\"\n\
         @in = global [4 x {bits}] [{bits} {a}, {bits} {a}, {bits} {c}, {bits} {nc}]\n\
         @out = global [3 x i64] zeroinitializer, align 8\n\
         @fmt = private constant [21 x i8] c\"%2d: %llx %llx %llx\\0A\\00\"\n\
         declare i32 @printf(ptr, ...)\ndeclare ptr @llvm.coro.noop()\n\
         declare void @llvm.coro.resume(ptr)\ndeclare void @llvm.coro.destroy(ptr)\n\
         declare i1 @llvm.coro.done(ptr)\n"
    );
    let mut main = String::from("define i32 @main() #0 {\nentry:\n");
    for (k, (name, construct)) in BARRIER_SHAPES.iter().enumerate() {
        writeln!(
            t,
            "define void @{name}(ptr %in, ptr %out) #0 {{\nentry:\n  \
             %a = load {ty}, ptr %in\n  %gb = getelementptr {ty}, ptr %in, i32 1\n  \
             %b = load {ty}, ptr %gb\n  %gc = getelementptr {ty}, ptr %in, i32 2\n  \
             %c = load {ty}, ptr %gc\n  %gn = getelementptr {ty}, ptr %in, i32 3\n  \
             %n = load {ty}, ptr %gn\n  %o1 = getelementptr i8, ptr %out, i32 8\n  \
             %o2 = getelementptr i8, ptr %out, i32 16\n  %p = fmul {ty} %a, %b\n  \
             %x = fsub {ty} %p, %n\n  store {ty} %x, ptr %out\n  {construct}\n  \
             %y = fadd {ty} %p, %c\n  store {ty} %y, ptr %o1\n  ret void\n}}"
        )
        .unwrap();
        writeln!(
            main,
            "  store i64 0, ptr @out\n  \
             store i64 0, ptr getelementptr (i8, ptr @out, i32 8)\n  \
             store i64 0, ptr getelementptr (i8, ptr @out, i32 16)\n  \
             call void @{name}(ptr @in, ptr @out)\n  \
             %r0.{k} = load i64, ptr @out\n  \
             %r1.{k} = load i64, ptr getelementptr (i8, ptr @out, i32 8)\n  \
             %r2.{k} = load i64, ptr getelementptr (i8, ptr @out, i32 16)\n  \
             call i32 (ptr, ...) @printf(ptr @fmt, i32 {k}, i64 %r0.{k}, i64 %r1.{k}, \
             i64 %r2.{k})"
        )
        .unwrap();
    }
    main.push_str("  ret i32 0\n}\n");
    t.push_str(&main);
    match cpu {
        Some(cpu) => writeln!(t, "attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}"),
        None => writeln!(t, "attributes #0 = {{ nounwind }}"),
    }
    .unwrap();
    t
}

/// Every handle barrier shape runs as the `opt` + `llc` oracle's, in
/// `float` and `double`, at CPU `generic` and `pentiumpro` (`cmov`), hod
/// plain and under `Stress::ALL`; GNU ld, static.
#[test]
fn barrier_shapes_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-coro-barriers");
    let mut lines = 0;
    for ty in ["float", "double"] {
        for cpu in [None, Some("pentiumpro")] {
            let text = barrier_module(ty, cpu);
            let label = format!("barriers-{ty}-{}", cpu.unwrap_or("generic"));
            let source = scratch.write(&format!("{label}.ll"), text.as_bytes());
            let (plain, stress) = hod_objects(text.as_bytes(), STATIC, &label);
            let hod = [
                ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
            ];
            for major in LLVM_MAJORS {
                let (Some(opt), Some(llc)) = (llvm_tool(major, "opt"), llvm_tool(major, "llc"))
                else {
                    continue;
                };
                let lowered = scratch.join(format!("{label}-{major}.o0.ll"));
                let mut command = Command::new(&opt);
                command.arg("-passes=default<O0>").arg("-S").arg(&source).arg("-o").arg(&lowered);
                checked(&format!("opt {major}"), &mut command);
                let llc_o = llc_object(&scratch, &llc, &lowered, &[], &format!("{label}-{major}"));
                let mut results = Vec::new();
                for (who, object) in [("llc", &llc_o), (hod[0].0, &hod[0].1), (hod[1].0, &hod[1].1)]
                {
                    let exe = scratch.join(format!("{label}-{major}-{who}"));
                    let mut command = Command::new(&gcc);
                    command.arg("-no-pie").arg(object).arg("-o").arg(&exe);
                    checked(&format!("link of {label} ({who})"), &mut command);
                    let (status, out) = run_with_features(&qemu, &exe, &[]);
                    assert_eq!(status, Some(0), "{label}-{major} ({who}) exited with {status:?}");
                    results.push(String::from_utf8_lossy(&out).into_owned());
                }
                assert_eq!(results[0].lines().count(), BARRIER_SHAPES.len(), "{label}-{major}");
                for (who, got) in ["hod", "stress"].iter().zip(&results[1..]) {
                    assert_eq!(&results[0], got, "{label}-{major}: {who}'s output");
                }
                lines += BARRIER_SHAPES.len();
            }
        }
    }
    eprintln!("i386 coro barrier shapes: {lines} shape lines equal llc's");
}

/// A promise alignment that is no constant power of two is refused by
/// name; a coroutine body is still refused as `coro:target`.
#[test]
fn what_is_refused() {
    let odd = "define ptr @f(ptr %h, i32 %a) {\n  \
               %p = call ptr @llvm.coro.promise(ptr %h, i32 %a, i1 false)\n  ret ptr %p\n}\n\
               declare ptr @llvm.coro.promise(ptr, i32, i1)";
    let error = refusal(odd, None).unwrap_or_else(|| panic!("compiled:\n{odd}"));
    assert!(error.contains("x86:coro-promise"), "{error}");
    let fine = "define i1 @f(ptr %h) {\n  call void @llvm.coro.resume(ptr %h)\n  \
                %d = call i1 @llvm.coro.done(ptr %h)\n  ret i1 %d\n}\n\
                declare void @llvm.coro.resume(ptr)\ndeclare i1 @llvm.coro.done(ptr)";
    assert_eq!(refusal(fine, None), None);
    let body = "define void @f() #0 {\n  ret void\n}\nattributes #0 = { presplitcoroutine }";
    let error = refusal(body, None).unwrap_or_else(|| panic!("compiled:\n{body}"));
    assert!(error.contains("coro:target"), "{error}");
}
