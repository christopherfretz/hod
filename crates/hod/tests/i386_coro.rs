//! WP-327 (stage A): the coroutine handle operations on i386
//! (`spec/codegen-x86.md` §18). `coro/handles.ll` calls `coro.resume`,
//! `destroy`, `done`, `promise` (alignment 4 and 16, both directions)
//! and `noop` on frames built by hand with LLVM's layout, with a `double`
//! live across `resume` and `destroy` (the x87 barrier). hod compiles it
//! through the facade and under `Stress::ALL`; the oracle is the same
//! module through `opt -passes='default<O0>'` and `llc`. Static and PIE,
//! at the default CPU (no SSE) and `-mcpu=i686`, linked by GNU ld, lld
//! and mold, run under `qemu-i386`: every run prints what `llc`'s
//! prints.
//!
//! WP-335 (stages B to D): coroutine bodies, `esi` their frame base.
//! C++ programs by clang 19 and 22 before LLVM's passes, at CPU `generic`
//! and `i686`, static and PIE, plain and stressed, run as the oracle's;
//! `-g` bodies verify; the `asm` that writes `esi` is refused by name.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, data, hod_objects, refusal};
use i386_support::{checked, clang_module, llc_object, llvm_tool, run_with_features, tool};

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

/// The suspend barrier module (`spec/codegen-x86.md` §11, §18.1) in the
/// dialect of LLVM `major` (`coro.end` returns `i1` in 19, nothing in
/// 22), at `cpu` (none: `generic`): the body computes the 80-bit product
/// `p` of `1 + 2^-30` with itself before its first suspend and stores
/// `p + c` after it, `c = -(1 + 2^-29)`: `2^-60` while `p` keeps 80 bits,
/// 0 once it is rounded to `double`.
fn suspend_barrier_module(major: u32, cpu: Option<&str>) -> String {
    let (end_ty, end) = if major < 22 { ("i1", "%e = call i1") } else { ("void", "call void") };
    let attrs = match cpu {
        Some(cpu) => format!("\"target-cpu\"=\"{cpu}\""),
        None => String::new(),
    };
    format!(
        "target triple = \"i686-unknown-linux-gnu\"\n\
         @in = global [3 x i64] [i64 4607182418804211712, i64 4607182418804211712, \
         i64 -4616189618046369792]\n\
         @out = global i64 0, align 8\n\
         @fmt = private constant [6 x i8] c\"%llx\\0A\\00\"\n\
         declare i32 @printf(ptr, ...)\ndeclare ptr @malloc(i32)\ndeclare void @free(ptr)\n\
         declare token @llvm.coro.id(i32, ptr, ptr, ptr)\ndeclare i32 @llvm.coro.size.i32()\n\
         declare ptr @llvm.coro.begin(token, ptr)\ndeclare token @llvm.coro.save(ptr)\n\
         declare i8 @llvm.coro.suspend(token, i1)\ndeclare ptr @llvm.coro.free(token, ptr)\n\
         declare {end_ty} @llvm.coro.end(ptr, i1, token)\n\
         declare void @llvm.coro.resume(ptr)\ndeclare void @llvm.coro.destroy(ptr)\n\
         define ptr @co(ptr %in) #0 {{\nentry:\n  \
         %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  \
         %size = call i32 @llvm.coro.size.i32()\n  %mem = call ptr @malloc(i32 %size)\n  \
         %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)\n  \
         %a = load double, ptr %in\n  %gb = getelementptr double, ptr %in, i32 1\n  \
         %b = load double, ptr %gb\n  %gc = getelementptr double, ptr %in, i32 2\n  \
         %c = load double, ptr %gc\n  %p = fmul double %a, %b\n  \
         %save = call token @llvm.coro.save(ptr null)\n  \
         %s = call i8 @llvm.coro.suspend(token %save, i1 false)\n  \
         switch i8 %s, label %suspend [i8 0, label %resume\n    i8 1, label %cleanup]\n\
         resume:\n  %y = fadd double %p, %c\n  store double %y, ptr @out\n  \
         %save2 = call token @llvm.coro.save(ptr null)\n  \
         %s2 = call i8 @llvm.coro.suspend(token %save2, i1 true)\n  \
         switch i8 %s2, label %suspend [i8 0, label %cleanup\n    i8 1, label %cleanup]\n\
         cleanup:\n  %f = call ptr @llvm.coro.free(token %id, ptr %hdl)\n  \
         call void @free(ptr %f)\n  br label %suspend\n\
         suspend:\n  {end} @llvm.coro.end(ptr %hdl, i1 false, token none)\n  ret ptr %hdl\n}}\n\
         define i32 @main() #1 {{\n  %h = call ptr @co(ptr @in)\n  \
         call void @llvm.coro.resume(ptr %h)\n  call void @llvm.coro.destroy(ptr %h)\n  \
         %r = load i64, ptr @out\n  call i32 (ptr, ...) @printf(ptr @fmt, i64 %r)\n  \
         ret i32 0\n}}\n\
         attributes #0 = {{ presplitcoroutine nounwind {attrs} }}\n\
         attributes #1 = {{ nounwind {attrs} }}\n"
    )
}

/// A `double` computed before a suspend and used after it is rounded to
/// `double` across the suspend (§18.1: it lives in the heap frame, 8
/// bytes, and the suspend is an x87 barrier): hod prints 0, plain and
/// under `Stress::ALL`, at CPU `generic` and `pentiumpro`, in LLVM 19's
/// and 22's dialect; GNU ld. The oracle differs here, and the test
/// records how: `CoroSplit` rematerialises the `fmul` in the resume
/// clone from the two spilled (exact) operands, so `llc`'s code computes
/// `p` again in 80 bits and prints `2^-60` (`3c30000000000000`). The
/// IR's `double` semantics give 0: the excess precision is the oracle's
/// (`spec/harness.md`'s `x87-precision` class).
#[test]
fn a_double_across_a_suspend_is_rounded() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-coro-suspend-barrier");
    for major in LLVM_MAJORS {
        let (Some(opt), Some(llc)) = (llvm_tool(major, "opt"), llvm_tool(major, "llc")) else {
            continue;
        };
        for cpu in [None, Some("pentiumpro")] {
            let text = suspend_barrier_module(major, cpu);
            let label = format!("suspend-{major}-{}", cpu.unwrap_or("generic"));
            let source = scratch.write(&format!("{label}.ll"), text.as_bytes());
            let lowered = scratch.join(format!("{label}.o0.ll"));
            let mut command = Command::new(&opt);
            command.arg("-passes=default<O0>").arg("-S").arg(&source);
            checked(&format!("opt {label}"), command.arg("-o").arg(&lowered));
            let (plain, stress) = hod_objects(text.as_bytes(), STATIC, &label);
            let objects = [
                ("llc", llc_object(&scratch, &llc, &lowered, &[], &format!("{label}-llc"))),
                ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
            ];
            let mut results = Vec::new();
            for (who, object) in &objects {
                let exe = scratch.join(format!("{label}-{who}"));
                let mut command = Command::new(&gcc);
                command.arg("-no-pie").arg(object).arg("-o").arg(&exe);
                checked(&format!("link of {label} ({who})"), &mut command);
                let (status, out) = run_with_features(&qemu, &exe, &[]);
                assert_eq!(status, Some(0), "{label} ({who})");
                results.push(String::from_utf8_lossy(&out).into_owned());
            }
            let oracle = ["0\n", "3c30000000000000\n"];
            assert!(oracle.contains(&results[0].as_str()), "{label}: llc's {}", results[0]);
            for (who, got) in ["hod", "stress"].iter().zip(&results[1..]) {
                assert_eq!(got, "0\n", "{label}: {who}'s output");
            }
        }
    }
}

/// The C++ programs of the body run tests (WP-335, `spec/codegen-x86.md`
/// §18): generators of `int`, `long long` and `double`, an `sret` return
/// object and 12 mixed parameters (`gen.cpp`); a chain of 100,000
/// symmetric transfers in constant stack (`transfer.cpp`); exceptions
/// caught in a body and thrown out of a resumed one (`throws.cpp`).
const PROGRAMS: [&str; 3] = ["gen.cpp", "transfer.cpp", "throws.cpp"];

/// clang's pre-optimisation module of `source` (`-O0 -Xclang
/// -disable-llvm-passes`, what hod receives from a front end) for i686,
/// with `flags`, as `<name>.ll`.
fn pre_opt(scratch: &Scratch, clang: &Path, source: &Path, flags: &[&str], name: &str) -> PathBuf {
    let mut all = vec!["-std=c++20", "-Xclang", "-disable-llvm-passes"];
    all.extend_from_slice(flags);
    clang_module(scratch, clang, source, &all, name)
}

/// The module without its `target-cpu` attributes: CPU `generic` (no
/// `cmov`, no SSE) for `llc` and hod alike; clang writes `i686`.
fn generic(text: &str) -> String {
    text.replace("\"target-cpu\"=\"i686\"", "")
}

/// `gcc` links the C++ `object` (`-pie` when `pic`) into `exe`.
fn link_cxx(gcc: &Path, object: &Path, pic: bool, exe: &Path) {
    let mut command = Command::new(gcc);
    command.arg(if pic { "-pie" } else { "-no-pie" });
    command.arg(object).args(["-lstdc++", "-lm"]).arg("-o").arg(exe);
    checked(&format!("link of {}", exe.display()), &mut command);
}

/// Every body program, by clang 19 and 22, at CPU `generic` and at
/// clang's default `i686` (`cmov`), static and PIE, runs as the `opt
/// -passes='default<O0>'` + `llc` oracle's does: hod's object through
/// the facade, and `hod_x86`'s under `Stress::ALL` (two pool registers
/// left beside the frame base `esi`, far branches, displaced values).
/// GNU ld; `qemu-i386 -cpu pentium2,-mmx`.
#[test]
fn bodies_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-coro-bodies");
    let mut runs = 0;
    for program in PROGRAMS {
        let stem = program.trim_end_matches(".cpp");
        for major in LLVM_MAJORS {
            let (Some(clang), Some(opt), Some(llc)) =
                (llvm_tool(major, "clang"), llvm_tool(major, "opt"), llvm_tool(major, "llc"))
            else {
                continue;
            };
            for pic in [false, true] {
                let model = if pic { "pie" } else { "exe" };
                let flag = if pic { "-fPIE" } else { "-fno-pic" };
                let label = format!("{stem}-{major}-{model}");
                let ll = pre_opt(&scratch, &clang, &data("coro", program), &[flag], &label);
                let i686 = std::fs::read_to_string(&ll).unwrap();
                for (cpu, text) in [("i686", i686.clone()), ("generic", generic(&i686))] {
                    let label = format!("{label}-{cpu}");
                    let source = scratch.write(&format!("{label}.ll"), text.as_bytes());
                    let lowered = scratch.join(format!("{label}.o0.ll"));
                    let mut command = Command::new(&opt);
                    command.arg("-passes=default<O0>").arg("-S").arg(&source);
                    checked(&format!("opt {label}"), command.arg("-o").arg(&lowered));
                    let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
                    let build = Build { pic, ..STATIC };
                    let (plain, stress) = hod_objects(text.as_bytes(), build, &label);
                    let objects = [
                        (
                            "llc",
                            llc_object(&scratch, &llc, &lowered, flags, &format!("{label}-llc")),
                        ),
                        ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                        ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
                    ];
                    let mut results = Vec::new();
                    for (who, object) in &objects {
                        let exe = scratch.join(format!("{label}-{who}"));
                        link_cxx(&gcc, object, pic, &exe);
                        results.push(run_with_features(&qemu, &exe, &[]));
                        runs += 1;
                    }
                    let want = String::from_utf8_lossy(&results[0].1);
                    assert_eq!(results[0].0, Some(0), "{label}: llc's run failed: {want}");
                    for (who, (status, out)) in ["hod", "stress"].iter().zip(&results[1..]) {
                        let got = String::from_utf8_lossy(out);
                        assert_eq!(want, got, "{label}: {who}'s output");
                        assert_eq!(results[0].0, *status, "{label}: {who}'s status");
                    }
                    match stem {
                        "transfer" => assert!(want.contains("constant stack 1"), "{label}: {want}"),
                        "throws" => assert!(want.contains("caught 5"), "{label}: {want}"),
                        _ => assert!(want.contains("mixed: "), "{label}: {want}"),
                    }
                }
            }
        }
    }
    eprintln!("i386 coro bodies: {runs} runs equal llc's");
}

/// Debug information of a body (`spec/dwarf.md` §6.16, the locations
/// through the frame word, the same rule as the 64-bit targets'):
/// `tests/debug/coroutine.cpp` and `transfer.cpp` at `-g` compile, hod's
/// object verifies (`llvm-dwarfdump --verify` of the same major) and the
/// program runs as `llc`'s.
#[test]
fn bodies_with_debug_information_verify_and_run() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-coro-debug");
    let sources = [
        (
            "coroutine",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/debug/coroutine.cpp"),
        ),
        ("transfer", data("coro", "transfer.cpp")),
    ];
    for (stem, source) in &sources {
        for major in LLVM_MAJORS {
            let (Some(clang), Some(opt), Some(llc), Some(dwarfdump)) = (
                llvm_tool(major, "clang"),
                llvm_tool(major, "opt"),
                llvm_tool(major, "llc"),
                llvm_tool(major, "llvm-dwarfdump"),
            ) else {
                continue;
            };
            let label = format!("{stem}-{major}-g");
            let ll = pre_opt(&scratch, &clang, source, &["-g", "-fno-pic"], &label);
            let text = std::fs::read(&ll).unwrap();
            let (plain, _) = hod_objects(&text, STATIC, &label);
            let hod = scratch.write(&format!("{label}-hod.o"), plain);
            let mut command = Command::new(&dwarfdump);
            checked(&format!("verify {label}"), command.arg("--verify").arg(&hod));
            let lowered = scratch.join(format!("{label}.o0.ll"));
            let mut command = Command::new(&opt);
            command.arg("-passes=default<O0>").arg("-S").arg(&ll);
            checked(&format!("opt {label}"), command.arg("-o").arg(&lowered));
            let llc_o = llc_object(&scratch, &llc, &lowered, &[], &format!("{label}-llc"));
            let mut results = Vec::new();
            for (who, object) in [("llc", &llc_o), ("hod", &hod)] {
                let exe = scratch.join(format!("{label}-{who}"));
                link_cxx(&gcc, object, false, &exe);
                results.push(run_with_features(&qemu, &exe, &[]));
            }
            assert_eq!(results[0], results[1], "{label}");
        }
    }
}

/// A promise alignment that is no constant power of two is refused by
/// name; a coroutine body compiles (WP-335), and inline assembly that
/// writes its frame base `esi` is `x86:coro:asm-base`.
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
    let body = coroutine("");
    assert_eq!(refusal(&body, None), None);
    let asm = coroutine("call void asm sideeffect \"xor %esi, %esi\", \"~{esi}\"()");
    let error = refusal(&asm, None).unwrap_or_else(|| panic!("compiled:\n{asm}"));
    assert!(error.contains("x86:coro:asm-base"), "{error}");
    let operand = coroutine("call void asm sideeffect \"nop\", \"{esi}\"(i32 1)");
    let error = refusal(&operand, None).unwrap_or_else(|| panic!("compiled:\n{operand}"));
    assert!(error.contains("x86:coro:asm-base"), "{error}");
    // The same `asm` outside a coroutine compiles.
    let plain = "define void @g() {\n  call void asm sideeffect \"xor %esi, %esi\", \
                 \"~{esi}\"()\n  ret void\n}";
    assert_eq!(refusal(plain, None), None);
}

/// A minimal coroutine body (one final suspend, LLVM's pre-split form)
/// with `extra` before its suspend.
fn coroutine(extra: &str) -> String {
    format!(
        "define ptr @f() #0 {{\nentry:\n  \
         %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  \
         %size = call i32 @llvm.coro.size.i32()\n  %mem = call ptr @malloc(i32 %size)\n  \
         %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)\n  \
         %save = call token @llvm.coro.save(ptr null)\n  {extra}\n  \
         %s = call i8 @llvm.coro.suspend(token %save, i1 true)\n  \
         switch i8 %s, label %suspend [i8 0, label %cleanup\n    i8 1, label %cleanup]\n\
         cleanup:\n  %f = call ptr @llvm.coro.free(token %id, ptr %hdl)\n  \
         call void @free(ptr %f)\n  br label %suspend\n\
         suspend:\n  call void @llvm.coro.end(ptr %hdl, i1 false, token none)\n  ret ptr %hdl\n}}\n\
         declare token @llvm.coro.id(i32, ptr, ptr, ptr)\ndeclare i32 @llvm.coro.size.i32()\n\
         declare ptr @llvm.coro.begin(token, ptr)\ndeclare token @llvm.coro.save(ptr)\n\
         declare i8 @llvm.coro.suspend(token, i1)\n\
         declare ptr @llvm.coro.free(token, ptr)\ndeclare void @llvm.coro.end(ptr, i1, token)\n\
         declare ptr @malloc(i32)\ndeclare void @free(ptr)\n\
         attributes #0 = {{ presplitcoroutine }}"
    )
}
