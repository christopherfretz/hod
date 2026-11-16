//! WP-327 (stage A): the coroutine handle operations on i386
//! (`spec/codegen-x86.md` §15). `coro/handles.ll` calls `coro.resume`,
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
