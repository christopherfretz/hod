//! `win64cc` (`__attribute__((ms_abi))`) on x86-64 Linux (WP-246,
//! `spec/abi-x64.md` §13), built by hod, linked by every linker and run.
//!
//! - What a `win64cc` callee keeps: each callee calls `clobber`
//!   (`tests/data/win64cc/snap.c`), a System V function that writes every
//!   register its convention lets it write; an assembly routine loads a
//!   distinct value into every register, calls the callee as a `win64cc`
//!   caller does and stores them all, and the driver checks the result,
//!   `rbx`, `rsi`, `rdi`, `r12`..`r15` and `xmm6`..`xmm15`. During the
//!   call the driver walks the stack with libgcc's unwinder: the caller's
//!   general registers, recovered through the callee's rows, must be the
//!   values loaded, and each `xmm` must be where the rows say (read off
//!   `llvm-dwarfdump --eh-frame`). Callees by hod (plain and stressed) and
//!   by `llc` 22 and 19 (which also checks the expected sets against
//!   `llc`'s); a `ccc` callee is the control. A System V caller built by
//!   hod and one by `llc` 22 call every callee of each build.
//! - Calls in both directions (`calls.ll`): hod's System V functions call
//!   `llc`'s `win64cc` functions and the reverse, directly, through a
//!   pointer, a variadic one (`llc`'s `va_arg`), with stack arguments,
//!   `i128` halves split between `r9` and the stack, aggregates, `sret`,
//!   a return in memory; GCC's own `ms_abi` calls each build.
//! - Exceptions through `win64cc` frames (`eh.ll`, `eh.cpp`).
//! - The corpus module `dcl.attr.cconv/cc-run.cpp` (`cc-run.ll`).
//! - The refusals by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use float_gen::{ll_header, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};

const SNAP: &str = include_str!("data/win64cc/snap.c");
const DRIVER: &str = include_str!("data/win64cc/driver.c");
const CALLS: &str = include_str!("data/win64cc/calls.ll");
const CALLS_C: &str = include_str!("data/win64cc/calls.c");
const VA: &str = include_str!("data/win64cc/va.ll");
const EH: &str = include_str!("data/win64cc/eh.ll");
const EH_CPP: &str = include_str!("data/win64cc/eh.cpp");
const EH_CATCH: &str = include_str!("data/win64cc/eh-catch.ll");
const CC_RUN: &str = include_str!("data/win64cc/cc-run.ll");

/// A result kind: its name in the driver, the result type, the
/// parameters and the body after `%x = call i64 @clobber(i64 %a)`.
struct Kind {
    name: &'static str,
    ret: &'static str,
    body: &'static str,
}

const KINDS: &[Kind] = &[
    Kind { name: "VOID", ret: "void", body: "store i64 %b, ptr @sink, align 8\n  ret void" },
    Kind { name: "I64", ret: "i64", body: "%s = add i64 %x, %b\n  ret i64 %s" },
    Kind {
        name: "I32",
        ret: "i32",
        body: "%s = add i64 %x, %b\n  %t = trunc i64 %s to i32\n  ret i32 %t",
    },
    Kind {
        name: "I128",
        ret: "i128",
        body: "%h = zext i64 %x to i128\n  %h2 = shl i128 %h, 64\n  %l = zext i64 %b to i128\n  \
               %r = or i128 %h2, %l\n  ret i128 %r",
    },
    Kind {
        name: "F64",
        ret: "double",
        body: "%s = add i64 %x, %b\n  %f = sitofp i64 %s to double\n  ret double %f",
    },
    Kind {
        name: "F32",
        ret: "float",
        body: "%s = add i64 %x, %b\n  %f = sitofp i64 %s to float\n  ret float %f",
    },
    Kind {
        name: "DD",
        ret: "{ double, double }",
        body: "%f = sitofp i64 %x to double\n  %g = sitofp i64 %b to double\n  \
               %r0 = insertvalue { double, double } poison, double %f, 0\n  \
               %r1 = insertvalue { double, double } %r0, double %g, 1\n  \
               ret { double, double } %r1",
    },
    Kind {
        name: "ID",
        ret: "{ i64, double }",
        body: "%g = sitofp i64 %b to double\n  \
               %r0 = insertvalue { i64, double } poison, i64 %x, 0\n  \
               %r1 = insertvalue { i64, double } %r0, double %g, 1\n  ret { i64, double } %r1",
    },
    Kind {
        name: "III",
        ret: "{ i64, i64, i64 }",
        body: "%s = add i64 %x, %b\n  \
               %r0 = insertvalue { i64, i64, i64 } poison, i64 %x, 0\n  \
               %r1 = insertvalue { i64, i64, i64 } %r0, i64 %b, 1\n  \
               %r2 = insertvalue { i64, i64, i64 } %r1, i64 %s, 2\n  ret { i64, i64, i64 } %r2",
    },
    Kind {
        name: "V4F",
        ret: "<4 x float>",
        body: "%s = add i64 %x, %b\n  %d = sub i64 %x, %b\n  %fa = sitofp i64 %x to float\n  \
               %fb = sitofp i64 %b to float\n  %fs = sitofp i64 %s to float\n  \
               %fd = sitofp i64 %d to float\n  \
               %v0 = insertelement <4 x float> poison, float %fa, i32 0\n  \
               %v1 = insertelement <4 x float> %v0, float %fb, i32 1\n  \
               %v2 = insertelement <4 x float> %v1, float %fs, i32 2\n  \
               %v3 = insertelement <4 x float> %v2, float %fd, i32 3\n  ret <4 x float> %v3",
    },
    Kind {
        name: "FP80",
        ret: "x86_fp80",
        body: "%s = add i64 %x, %b\n  %f = sitofp i64 %s to x86_fp80\n  ret x86_fp80 %f",
    },
    Kind {
        name: "SRET",
        ret: "void",
        body: "%s = add i64 %x, %b\n  store i64 %s, ptr %p, align 8\n  ret void",
    },
    Kind {
        name: "MEM",
        ret: "{ i64, i64, i64, i64 }",
        body: "%s = add i64 %x, %b\n  \
               %r0 = insertvalue { i64, i64, i64, i64 } poison, i64 %x, 0\n  \
               %r1 = insertvalue { i64, i64, i64, i64 } %r0, i64 %b, 1\n  \
               %r2 = insertvalue { i64, i64, i64, i64 } %r1, i64 %s, 2\n  \
               %r3 = insertvalue { i64, i64, i64, i64 } %r2, i64 7, 3\n  \
               ret { i64, i64, i64, i64 } %r3",
    },
];

/// The callee module, the callees in order (name, kind), and the caller
/// module for a caller named `@CALLER`.
fn modules() -> (String, Vec<(String, &'static str)>, String) {
    let mut callees = format!(
        "{}@sink = external global i64\ndeclare i64 @clobber(i64)\n\
         define void @c_plain(i64 %a, i64 %b) {{\n  %x = call i64 @clobber(i64 %a)\n  \
         store i64 %b, ptr @sink, align 8\n  ret void\n}}\n",
        ll_header()
    );
    let mut list = vec![("c_plain".to_owned(), "PLAIN")];
    let mut caller = String::from(
        "define i64 @CALLER(i64 %a, i64 %b) {\nentry:\n  %acc0 = add i64 0, 0\n  \
         %buf = alloca i64, align 8\n",
    );
    let mut decls = String::new();
    for (n, kind) in KINDS.iter().enumerate() {
        let name = format!("w_{}", kind.name.to_lowercase());
        let (params, ptypes) = match kind.name {
            "SRET" => ("ptr sret(i64) %p, i64 %a, i64 %b", "ptr sret(i64), i64, i64"),
            _ => ("i64 %a, i64 %b", "i64, i64"),
        };
        writeln!(
            callees,
            "define win64cc {} @{name}({params}) {{\nentry:\n  %x = call i64 @clobber(i64 %a)\n  {}\n}}",
            kind.ret, kind.body
        )
        .unwrap();
        writeln!(decls, "declare win64cc {} @{name}({ptypes})", kind.ret).unwrap();
        let call = |args: &str| format!("call win64cc {} @{name}({args})", kind.ret);
        let fold_ir = match kind.name {
            "VOID" => {
                format!("{}\n  %r{n} = load i64, ptr @sink, align 8", call("i64 %a, i64 %b"))
            }
            "SRET" => format!(
                "{}\n  %r{n} = load i64, ptr %buf, align 8",
                call("ptr sret(i64) %buf, i64 %a, i64 %b")
            ),
            _ => {
                let value = match kind.name {
                    "I64" => format!("%r{n} = add i64 %c{n}, 0"),
                    "I32" => format!("%r{n} = sext i32 %c{n} to i64"),
                    "I128" => format!(
                        "%h{n} = lshr i128 %c{n}, 64\n  %t{n} = trunc i128 %h{n} to i64\n  \
                         %l{n} = trunc i128 %c{n} to i64\n  %r{n} = mul i64 %t{n}, %l{n}"
                    ),
                    "F64" => format!("%r{n} = fptosi double %c{n} to i64"),
                    "F32" => format!("%r{n} = fptosi float %c{n} to i64"),
                    "DD" => format!(
                        "%e{n} = extractvalue {{ double, double }} %c{n}, 1\n  \
                         %r{n} = fptosi double %e{n} to i64"
                    ),
                    "ID" => format!(
                        "%e{n} = extractvalue {{ i64, double }} %c{n}, 0\n  %r{n} = mul i64 %e{n}, 3"
                    ),
                    "III" => format!("%r{n} = extractvalue {{ i64, i64, i64 }} %c{n}, 2"),
                    "V4F" => format!(
                        "%e{n} = extractelement <4 x float> %c{n}, i32 3\n  \
                         %r{n} = fptosi float %e{n} to i64"
                    ),
                    "FP80" => format!("%r{n} = fptosi x86_fp80 %c{n} to i64"),
                    "MEM" => format!(
                        "%e{n} = extractvalue {{ i64, i64, i64, i64 }} %c{n}, 2\n  \
                         %f{n} = extractvalue {{ i64, i64, i64, i64 }} %c{n}, 3\n  \
                         %r{n} = mul i64 %e{n}, %f{n}"
                    ),
                    _ => unreachable!(),
                };
                format!("%c{n} = {}\n  {value}", call("i64 %a, i64 %b"))
            }
        };
        writeln!(caller, "  {fold_ir}\n  %acc{} = add i64 %acc{n}, %r{n}", n + 1).unwrap();
        list.push((name, kind.name));
    }
    writeln!(caller, "  ret i64 %acc{}\n}}", KINDS.len()).unwrap();
    let caller = format!("{}@sink = external global i64\n{decls}{caller}", ll_header());
    (callees, list, caller)
}

/// Where each function's unwind rows (in object order) save `xmm0` to
/// `xmm15`, as offsets from the CFA, 0 for none: the body row with the
/// most rules of each FDE (as `tests/preserve_cc.rs` reads them).
fn xmm_offsets(object: &Path) -> Option<Vec<[i32; 16]>> {
    let dump = hod_testutil::tool("llvm-dwarfdump")?;
    let out = std::process::Command::new(dump).arg("--eh-frame").arg(object).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut fdes = Vec::new();
    let mut best: Option<(usize, [i32; 16])> = None;
    for line in text.lines() {
        if line.contains(" FDE ") {
            if let Some((_, row)) = best.take() {
                fdes.push(row);
            }
            best = Some((0, [0; 16]));
            continue;
        }
        let Some((rules, _)) = best.as_ref() else { continue };
        if !line.trim_start().starts_with("0x") || !line.contains("CFA=") {
            continue;
        }
        let count = line.matches("=[CFA").count();
        if count >= *rules {
            let mut row = [0; 16];
            for (k, slot) in row.iter_mut().enumerate() {
                let key = format!("XMM{k}=[CFA-");
                if let Some(at) = line.find(&key) {
                    let rest = &line[at + key.len()..];
                    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                    *slot = -digits.parse::<i32>().expect("an offset");
                }
            }
            best = Some((count, row));
        }
    }
    if let Some((_, row)) = best {
        fdes.push(row);
    }
    Some(fdes)
}

/// The C program for one build of the callees.
fn program(list: &[(String, &str)], offsets: &[[i32; 16]]) -> String {
    assert_eq!(offsets.len(), list.len(), "one FDE per function");
    let mut c = String::from("#include <stdint.h>\n");
    let mut snaps = String::from("#define SNAPS");
    c.push_str("enum { K_PLAIN,");
    for kind in KINDS {
        write!(c, " K_{},", kind.name).unwrap();
    }
    c.push_str(" };\n");
    c.push_str(
        "struct fn_case { const char *name; void (*snap)(const uint64_t *, uint64_t *); \
         const void *fn; int kind; int win64; int xoff[16]; };\n",
    );
    for (name, kind) in list {
        write!(snaps, " \"SNAP {name}, {}\\n\"", i32::from(*kind == "FP80")).unwrap();
        writeln!(c, "void snap_{name}(const uint64_t *, uint64_t *);\nextern char {name}[];")
            .unwrap();
    }
    c.push_str("long caller_hod(long, long);\nlong caller_llc(long, long);\n");
    writeln!(c, "static const struct fn_case CASES[] = {{").unwrap();
    for ((name, kind), xoff) in list.iter().zip(offsets) {
        let xoff: Vec<String> = xoff.iter().map(i32::to_string).collect();
        writeln!(
            c,
            "  {{\"{name}\", snap_{name}, {name}, K_{kind}, {}, {{{}}}}},",
            i32::from(*kind != "PLAIN"),
            xoff.join(", ")
        )
        .unwrap();
    }
    writeln!(c, "}};\n#define NCASES {}", list.len()).unwrap();
    format!("{snaps}\n{c}{SNAP}\n{DRIVER}")
}

fn hod_object(scratch: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let (m, bodies) = parse(text);
    let object = hod_x64::compile_to_object(&m, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), object)
}

/// §13.6: what a `win64cc` callee keeps, its unwind rows, and calls into
/// it from System V callers.
#[test]
fn win64_callees_keep_what_llc_keeps() {
    let (callees, list, caller) = modules();
    let scratch = Scratch::new("x64-win64cc-keep");
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        objects.push((leg.to_owned(), hod_object(&scratch, leg, &callees, stress)));
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    for major in [22, 19] {
        if let Some(object) = float_gen::llc(&scratch, major, &format!("llc{major}"), &callees) {
            objects.push((format!("llc{major}"), object));
        }
    }
    let hod_caller = caller.replace("@CALLER", "@caller_hod");
    let hod_caller = hod_object(&scratch, "caller-hod", &hod_caller, Stress::default());
    let Some(llc_caller) =
        float_gen::llc(&scratch, 22, "caller-llc", &caller.replace("@CALLER", "@caller_llc"))
    else {
        return;
    };
    let mut sums = None;
    let mut ran = Vec::new();
    for (leg, object) in &objects {
        let Some(offsets) = xmm_offsets(object) else { return };
        let driver =
            toolchain.compile_c(&scratch, &format!("driver-{leg}"), &program(&list, &offsets), &[]);
        for linker in &toolchain.linkers {
            let exe = scratch.join(format!("{leg}-{}", linker.name));
            let inputs: [&Path; 4] = [&driver, object, &hod_caller, &llc_caller];
            toolchain.link_ok(linker, &[], &inputs, &exe);
            let outcome =
                run_with(&mut toolchain.runner_command(&exe), &[], Duration::from_mins(2));
            let stdout = outcome.stdout_text();
            assert!(outcome.success(), "{leg}/{}: {stdout}{}", linker.name, outcome.stderr_text());
            let line = stdout.lines().find(|l| l.starts_with("callers ")).expect("the sums");
            let mut parts = line.split(' ').skip(1);
            let (a, b) = (parts.next().unwrap().to_owned(), parts.next().unwrap().to_owned());
            assert_eq!(a, b, "{leg}/{}: hod's caller and llc's disagree", linker.name);
            assert_eq!(*sums.get_or_insert_with(|| a.clone()), a, "{leg}/{}", linker.name);
            ran.push(format!("{leg}/{}", linker.name));
        }
    }
    eprintln!("x64 win64cc: {} callees checked over {ran:?}", list.len());
}

/// The numbers of the line of `stdout` that starts with `tag`.
fn numbers<'a>(stdout: &'a str, tag: &str) -> Vec<&'a str> {
    let line = stdout.lines().find(|l| l.starts_with(tag)).unwrap_or_else(|| panic!("{stdout}"));
    line.split(' ').skip(1).collect()
}

/// §13.2 to §13.5: hod's and `llc`'s builds of `calls.ll` call each
/// other's `win64cc` functions (and GCC's `ms_abi` calls each), under
/// every linker; the results equal `llc` 19's build calling itself.
#[test]
fn win64_calls_in_both_directions() {
    let scratch = Scratch::new("x64-win64cc-calls");
    // The reference build calls its own functions: no declarations of them.
    let build = |own: &str, other: &str| {
        let mut text = ll_header();
        for line in CALLS.lines() {
            if own != other || !(line.starts_with("declare") && line.contains("$O")) {
                writeln!(text, "{}", line.replace("$S", own).replace("$O", other)).unwrap();
            }
        }
        text
    };
    let mut inputs = vec![hod_object(&scratch, "calls-hod", &build("_h", "_l"), Stress::default())];
    let stressed = hod_object(&scratch, "calls-stress", &build("_h", "_l"), Stress::ALL);
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let llc = |major, name: &str, text: &str| float_gen::llc(&scratch, major, name, text);
    let (Some(l), Some(r), Some(va)) = (
        llc(22, "calls-llc", &build("_l", "_h")),
        llc(19, "calls-ref", &build("_r", "_r")),
        llc(22, "va", &format!("{}{VA}", ll_header())),
    ) else {
        return;
    };
    inputs.extend([l, r, va]);
    let driver = toolchain.compile_c(&scratch, "calls-driver", CALLS_C, &[]);
    let mut ran = 0;
    for (leg, hod) in [("plain", inputs[0].clone()), ("stress", stressed)] {
        let objects: Vec<&Path> =
            [&driver, &hod, &inputs[1], &inputs[2], &inputs[3]].map(PathBuf::as_path).to_vec();
        for linker in &toolchain.linkers {
            let exe = scratch.join(format!("calls-{leg}-{}", linker.name));
            let outcome = toolchain.link_and_run(linker, &[], &objects, &exe);
            let stdout = outcome.stdout_text();
            assert!(outcome.success(), "{leg}/{}: {stdout}{}", linker.name, outcome.stderr_text());
            for tag in ["many ", "drive "] {
                let values = numbers(&stdout, tag);
                assert!(
                    values.windows(2).all(|w| w[0] == w[1]),
                    "{leg}/{}: {tag}differ: {stdout}",
                    linker.name
                );
            }
            ran += 1;
        }
    }
    assert!(ran >= 2, "no linker ran");
}

/// §13.6: a C++ exception thrown through a hod-built `win64cc` frame (to
/// a System V catcher whose `rbx` the frame's rows give back) and one
/// caught in a landing pad of one, called by GCC's `ms_abi` code that
/// keeps values in `rsi`, `rdi`, `xmm6` and `xmm15` across the call.
#[test]
fn exceptions_through_win64_frames() {
    let scratch = Scratch::new("x64-win64cc-eh");
    let module = format!("{}{EH}", ll_header());
    let hod = hod_object(&scratch, "eh-hod", &module, Stress::default());
    let stressed = hod_object(&scratch, "eh-stress", &module, Stress::ALL);
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let Some(driver) = toolchain.compile_cxx(&scratch, "eh-driver", EH_CPP, &["-fPIC"]) else {
        return;
    };
    let catcher = format!("{}{EH_CATCH}", ll_header());
    let (Some(llc), Some(catcher)) = (
        float_gen::llc(&scratch, 22, "eh-llc", &module),
        float_gen::llc(&scratch, 22, "eh-catch", &catcher),
    ) else {
        return;
    };
    let stdcxx = Path::new("-lstdc++");
    let want = "pass 1042 catch 8 kept 1234 5678 0.75 1.5";
    for (leg, object) in [("hod", &hod), ("stress", &stressed), ("llc", &llc)] {
        for linker in &toolchain.linkers {
            let exe = scratch.join(format!("eh-{leg}-{}", linker.name));
            let inputs: [&Path; 4] = [&driver, &catcher, object, stdcxx];
            let outcome = toolchain.link_and_run(linker, &[], &inputs, &exe);
            let stdout = outcome.stdout_text();
            assert!(outcome.success(), "{leg}/{}: {stdout}{}", linker.name, outcome.stderr_text());
            assert_eq!(stdout.trim(), want, "{leg}/{}", linker.name);
        }
    }
}

/// The corpus module `dcl.attr.cconv/cc-run.cpp` (kiln, LLVM 22), less
/// the two conventions hod refuses by design: built by hod, linked by
/// every linker, run; its output is the one the corpus records, and
/// `llc`'s build's.
#[test]
fn corpus_module_runs() {
    let scratch = Scratch::new("x64-win64cc-corpus");
    let hod = hod_object(&scratch, "cc-run-hod", CC_RUN, Stress::default());
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let Some(llc) = float_gen::llc(&scratch, 22, "cc-run-llc", CC_RUN) else { return };
    let want = "pm 21 pa 3.5 ms 60 msr 44 v 7 l 9\nt 42";
    let (stdcxx, libm) = (Path::new("-lstdc++"), Path::new("-lm"));
    for (leg, object) in [("hod", &hod), ("llc", &llc)] {
        for linker in &toolchain.linkers {
            let exe = scratch.join(format!("cc-run-{leg}-{}", linker.name));
            let inputs: [&Path; 3] = [object, stdcxx, libm];
            let outcome = toolchain.link_and_run(linker, &[], &inputs, &exe);
            let stdout = outcome.stdout_text();
            assert!(outcome.success(), "{leg}/{}: {stdout}{}", linker.name, outcome.stderr_text());
            assert_eq!(stdout.trim(), want, "{leg}/{}", linker.name);
        }
    }
}

/// §13.7: what a `win64cc` function cannot be is refused by name.
#[test]
fn win64_functions_refuse_by_name() {
    let one = |text: &str| format!("{}{text}", ll_header());
    let cases = [
        (
            one("declare win64cc i64 @g(i64)\n\
                 define win64cc i64 @f(i64 %a) {\n  \
                 %r = musttail call win64cc i64 @g(i64 %a)\n  ret i64 %r\n}\n"),
            hod_x64::win64::MUSTTAIL,
        ),
        (
            one("declare void @llvm.va_start.p0(ptr)\n\
                 define win64cc i64 @f(i64 %a, ...) {\n  %ap = alloca ptr\n  \
                 call void @llvm.va_start.p0(ptr %ap)\n  ret i64 %a\n}\n"),
            hod_x64::win64::VARIADIC_DEFINITION,
        ),
        (one("define win64cc void @f(<4 x float> %v) {\n  ret void\n}\n"), hod_x64::win64::VECTOR),
        (
            one("declare win64cc void @g(x86_fp80)\n\
                 define void @f() {\n  call win64cc void @g(x86_fp80 0xK3FFF8000000000000000)\n  \
                 ret void\n}\n"),
            hod_x64::win64::X86_FP80,
        ),
        (
            one("declare win64cc void @g(ptr byval([3 x i64]))\n\
                 define void @f(ptr %p) {\n  call win64cc void @g(ptr byval([3 x i64]) %p)\n  \
                 ret void\n}\n"),
            hod_x64::win64::BYVAL,
        ),
        (
            one("declare win64cc void @g(i32, ...)\n\
                 define void @f() {\n  call win64cc void (i32, ...) @g(i32 1, float 2.0)\n  \
                 ret void\n}\n"),
            hod_x64::win64::VARIADIC_FLOAT,
        ),
    ];
    for (text, name) in &cases {
        let (m, bodies) = parse(text);
        match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported(name), "{text}");
            }
            other => panic!("not refused: {:?}\n{text}", other.map(|o| o.len())),
        }
    }
}
