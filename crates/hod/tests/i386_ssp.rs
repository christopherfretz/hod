//! WP-324: stack protectors on i386 (`spec/codegen-x86.md` §16).
//! `ssp/smash.c` built with `-fstack-protector`, `-strong` and `-all`
//! (one buffer, several escaping slots, a dynamic area with the low
//! guard, `float`/`double` results, an `i64` result, a struct result,
//! recursion) at `-march=i586` and the default CPU, static and PIE,
//! linked by GNU ld, lld and mold, runs as `llc` 19 and 22 run it (also
//! under `Stress::ALL`); `ssp/tail.ll` adds `musttail` exits (arguments
//! in `ecx`/`edx` and on the stack) and a `float` result computed after a
//! call. With `-DOVERFLOW=k` case `k` writes past a protected buffer: the
//! program aborts with `*** stack smashing detected ***` (SIGABRT) under
//! hod as under `llc`.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, linkers, run_with};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, hod_objects, lane, refusal};
use i386_support::{checked, clang_module, llc_object, llvm_tool, tool};

#[test]
fn protected_programs_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-ssp");
    let source = data("ssp", "smash.c");
    let mut runs = 0;
    for (cpu, march) in [("i586", Some("-march=i586")), ("i686", None)] {
        for (level, flag) in [
            ("ssp", "-fstack-protector"),
            ("strong", "-fstack-protector-strong"),
            ("all", "-fstack-protector-all"),
        ] {
            let flags: Vec<&str> = march.into_iter().chain([flag]).collect();
            for pic in [false, true] {
                let build = Build { pic, all_linkers: true, ..STATIC };
                let name = format!("smash-{cpu}-{level}");
                runs += lane.check(&scratch, &name, &Source::C(&source, &flags), build);
            }
        }
    }
    let tail = std::fs::read_to_string(data("ssp", "tail.ll")).unwrap();
    for pic in [false, true] {
        for mcpu in [None, Some("i686")] {
            let build = Build { pic, mcpu, all_linkers: true, ..STATIC };
            runs += lane.check(&scratch, "tail", &Source::Ll(&tail), build);
        }
    }
    eprintln!("i386 ssp: {runs} runs equal llc's");
}

/// The tools of the overflow runs.
struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return None;
    };
    Some(Tools { gcc, qemu, linkers: linkers::present(target) })
}

const SMASHED: &str = "*** stack smashing detected ***";

/// Whether the program `exe` aborted on a smashed stack.
fn aborts_smashed(tools: &Tools, exe: &std::path::Path) -> Result<(), String> {
    let target = Target::I686;
    let mut command = Command::new(&tools.qemu);
    command.args(target.emulator_args(&target.sysroot())).arg(exe);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    let signal = outcome.status.and_then(|s| s.signal());
    let stderr = outcome.stderr_text();
    if signal == Some(6) && stderr.contains(SMASHED) {
        Ok(())
    } else {
        Err(format!("status {:?}, stderr {stderr:?}", outcome.status))
    }
}

#[test]
fn an_overflow_aborts_as_under_llc() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-ssp-overflow");
    let source = data("ssp", "smash.c");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for case in 1..=5 {
            for pic in [false, true] {
                let define = format!("-DOVERFLOW={case}");
                let pie = if pic { "-fPIE" } else { "-fno-pic" };
                let label = format!("over{case}-{major}-{}", if pic { "pie" } else { "exe" });
                let flags = ["-fstack-protector-strong", define.as_str(), pie];
                let ll = clang_module(&scratch, &clang, &source, &flags, &label);
                let text = std::fs::read(&ll).unwrap();
                let build = Build { pic, ..STATIC };
                let (plain, stressed) = hod_objects(&text, build, &label);
                let model: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
                let objects = [
                    ("llc", llc_object(&scratch, &llc, &ll, model, &format!("{label}-llc"))),
                    ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                    ("stress", scratch.write(&format!("{label}-stress.o"), stressed)),
                ];
                for (linker, program) in &tools.linkers {
                    for (who, object) in &objects {
                        let exe = scratch.join(format!("{label}-{who}-{linker}"));
                        let mut command = Command::new(&tools.gcc);
                        command.args(linkers::driver_flags(scratch.path(), linker, program));
                        command.arg(if pic { "-pie" } else { "-no-pie" });
                        command.arg(object).arg("-o").arg(&exe);
                        checked(&format!("{linker} link of {label} ({who})"), &mut command);
                        // Case 3 overflows a dynamic allocation: hod's low
                        // guard catches it; `llc` has none and its PIE may
                        // fault on what the overflow overwrote first.
                        if let Err(what) = aborts_smashed(&tools, &exe)
                            && (*who != "llc" || case != 3)
                        {
                            panic!("{label} ({who}, {linker}) did not abort: {what}");
                        }
                        runs += 1;
                    }
                }
            }
        }
    }
    eprintln!("i386 ssp: {runs} overflow runs abort as llc's");
}

#[test]
fn what_compiles_and_what_is_refused() {
    let fine = [
        "declare void @g(ptr)\ndefine void @f() sspstrong {\n  %b = alloca [8 x i8]\n  \
         call void @g(ptr %b)\n  ret void\n}",
        "define void @f() sspreq {\n  ret void\n}",
        "declare void @g(ptr)\ndefine void @f(i32 %n) ssp {\n  %b = alloca i8, i32 %n\n  \
         call void @g(ptr %b)\n  ret void\n}",
        "declare void @g(ptr)\ndefine void @f() sspstrong {\n  %b = alloca [8 x i8], align 64\n  \
         call void @g(ptr %b)\n  ret void\n}",
    ];
    for body in fine {
        assert_eq!(refusal(body, None), None, "{body}");
    }
    let safe = "define void @f() safestack {\n  ret void\n}";
    let error = refusal(safe, None).unwrap_or_else(|| panic!("compiled:\n{safe}"));
    assert!(error.contains("safestack"), "{error}");
    // The guard module flags are the front half's (`spec/ir.md` §2.1).
    let global = "define void @f() sspreq {\n  ret void\n}\n!llvm.module.flags = !{!0}\n\
                  !0 = !{i32 1, !\"stack-protector-guard\", !\"global\"}";
    let error = refusal(global, None).unwrap_or_else(|| panic!("compiled:\n{global}"));
    assert!(error.contains("module-flag:stack-protector-guard"), "{error}");
}
