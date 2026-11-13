//! `preserve_mostcc` and `preserve_allcc` on x86-64 (WP-181,
//! `spec/codegen.md` §5.6.23, `spec/abi-x64.md` §1.2): arguments and
//! results travel as for `ccc`; the callee also keeps `rax`, `rcx`,
//! `rdx`, `rsi`, `rdi`, `r8`..`r10` and, for `preserve_allcc`,
//! `xmm0`..`xmm15`, except the registers its result is returned in.
//!
//! Each callee calls `clobber` (`tests/data/preserve_cc/snap.c`), which
//! writes every caller-saved register; an assembly routine loads a
//! distinct value into every register, calls the callee and stores them
//! all, and the driver checks the result and every register the callee
//! must keep. During the call `clobber` walks the stack with libgcc's
//! unwinder: the caller's general registers, recovered through the
//! callee's unwind rows, must be the values loaded, and each `xmm`
//! register must be where the callee's rows say (read off
//! `llvm-dwarfdump --eh-frame`). The callees are built by hod (plain and
//! stressed) and by `llc -O0 -fast-isel=0` of LLVM 22 and 19, which also
//! checks the expected sets against `llc`'s; a `ccc` callee is the
//! control. A caller built by hod and one built by `llc` 22 call every
//! callee of each build (the caller side: hod treats them as `ccc`, which
//! assumes more registers clobbered) and must print the same sums.

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

const SNAP: &str = include_str!("data/preserve_cc/snap.c");
const DRIVER: &str = include_str!("data/preserve_cc/driver.c");

/// A result kind: its name in the driver, the result type, and the body
/// after `%x = call i64 @clobber(i64 %a)` (`%b` the second argument).
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
    Kind { name: "VA", ret: "i64", body: "" },
];

/// The convention of each callee prefix.
const CCS: [(&str, &str); 2] = [("m", "preserve_mostcc"), ("a", "preserve_allcc")];

/// The callee's parameters.
fn params(kind: &Kind) -> &'static str {
    match kind.name {
        "SRET" => "ptr sret(i64) %p, i64 %a, i64 %b",
        "VA" => "i64 %n, ...",
        _ => "i64 %a, i64 %b",
    }
}

/// The callee module, the callees in order (name, kind, allcc), and the
/// caller module for a caller named `caller`.
fn modules() -> (String, Vec<(String, &'static str, bool)>, String) {
    let mut callees = format!(
        "{}@sink = external global i64\ndeclare i64 @clobber(i64)\n\
         declare void @llvm.va_start.p0(ptr)\ndeclare void @llvm.va_end.p0(ptr)\n\
         define void @c_plain(i64 %a, i64 %b) {{\n  %x = call i64 @clobber(i64 %a)\n  \
         store i64 %b, ptr @sink, align 8\n  ret void\n}}\n",
        ll_header()
    );
    let mut list = vec![("c_plain".to_owned(), "PLAIN", false)];
    let mut caller = String::from("define i64 @CALLER(i64 %a, i64 %b) {\nentry:\n");
    let mut sum = 0;
    let mut decls = String::new();
    let fold = |caller: &mut String, value: String, sum: &mut usize| {
        writeln!(caller, "  %acc{} = add i64 %acc{}, {value}", *sum + 1, *sum).unwrap();
        *sum += 1;
    };
    caller.push_str("  %acc0 = add i64 0, 0\n  %buf = alloca i64, align 8\n");
    for (prefix, cc) in CCS {
        for kind in KINDS {
            let name = format!("{prefix}_{}", kind.name.to_lowercase());
            let body = if kind.name == "VA" {
                "%ap = alloca [24 x i8], align 16\n  %x = call i64 @clobber(i64 %n)\n  \
                 call void @llvm.va_start.p0(ptr %ap)\n  %gpo = load i32, ptr %ap, align 16\n  \
                 %rsap = getelementptr i8, ptr %ap, i64 16\n  %rsa = load ptr, ptr %rsap, align 8\n  \
                 %o0 = zext i32 %gpo to i64\n  %p0 = getelementptr i8, ptr %rsa, i64 %o0\n  \
                 %v0 = load i64, ptr %p0, align 8\n  %p1 = getelementptr i8, ptr %p0, i64 8\n  \
                 %v1 = load i64, ptr %p1, align 8\n  %p2 = getelementptr i8, ptr %p0, i64 16\n  \
                 %v2 = load i64, ptr %p2, align 8\n  \
                 call void @llvm.va_end.p0(ptr %ap)\n  %s0 = add i64 %v0, %v1\n  \
                 %s1 = add i64 %s0, %v2\n  ret i64 %s1"
                    .to_owned()
            } else {
                format!("%x = call i64 @clobber(i64 %a)\n  {}", kind.body)
            };
            writeln!(
                callees,
                "define {cc} {} @{name}({}) {{\nentry:\n  {body}\n}}",
                kind.ret,
                params(kind)
            )
            .unwrap();
            let ptypes = match kind.name {
                "SRET" => "ptr sret(i64), i64, i64",
                "VA" => "i64, ...",
                _ => "i64, i64",
            };
            writeln!(decls, "declare {cc} {} @{name}({ptypes})", kind.ret).unwrap();
            let call = |args: &str| format!("call {cc} {} @{name}({args})", kind.ret);
            let n = list.len();
            match kind.name {
                "VOID" => {
                    writeln!(caller, "  {}", call("i64 %a, i64 %b")).unwrap();
                    writeln!(caller, "  %r{n} = load i64, ptr @sink, align 8").unwrap();
                }
                "SRET" => {
                    writeln!(caller, "  {}", call("ptr sret(i64) %buf, i64 %a, i64 %b")).unwrap();
                    writeln!(caller, "  %r{n} = load i64, ptr %buf, align 8").unwrap();
                }
                "VA" => {
                    writeln!(
                        caller,
                        "  %r{n} = call {cc} i64 (i64, ...) @{name}(i64 3, i64 %a, i64 %b, i64 77)"
                    )
                    .unwrap();
                }
                _ => {
                    writeln!(caller, "  %c{n} = {}", call("i64 %a, i64 %b")).unwrap();
                    let fold_ir = match kind.name {
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
                        _ => unreachable!(),
                    };
                    writeln!(caller, "  {fold_ir}").unwrap();
                }
            }
            fold(&mut caller, format!("%r{n}"), &mut sum);
            list.push((name, kind.name, prefix == "a"));
        }
    }
    writeln!(caller, "  ret i64 %acc{sum}\n}}").unwrap();
    let caller = format!("{}@sink = external global i64\n{decls}{caller}", ll_header());
    (callees, list, caller)
}

/// Where each function's unwind rows (in object order) save `xmm0` to
/// `xmm15`, as offsets from the CFA, 0 for none: the body row with the
/// most rules of each FDE.
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
fn program(list: &[(String, &str, bool)], offsets: &[[i32; 16]]) -> String {
    assert_eq!(offsets.len(), list.len(), "one FDE per function");
    let mut c = String::from("#include <stdint.h>\n");
    let mut snaps = String::from("#define SNAPS");
    let kinds = ["PLAIN"].into_iter().chain(KINDS.iter().map(|k| k.name));
    c.push_str("enum {");
    for kind in kinds {
        write!(c, " K_{kind},").unwrap();
    }
    c.push_str(" };\n");
    c.push_str(
        "struct fn_case { const char *name; void (*snap)(const uint64_t *, uint64_t *); \
         const void *fn; int kind; int all; int xoff[16]; };\n",
    );
    for (name, kind, _) in list {
        write!(snaps, " \"SNAP {name}, {}\\n\"", i32::from(*kind == "FP80")).unwrap();
        writeln!(c, "void snap_{name}(const uint64_t *, uint64_t *);\nextern char {name}[];")
            .unwrap();
    }
    c.push_str("long caller_hod(long, long);\nlong caller_llc(long, long);\n");
    writeln!(c, "static const struct fn_case CASES[] = {{").unwrap();
    for ((name, kind, all), xoff) in list.iter().zip(offsets) {
        let xoff: Vec<String> = xoff.iter().map(i32::to_string).collect();
        writeln!(
            c,
            "  {{\"{name}\", snap_{name}, {name}, K_{kind}, {}, {{{}}}}},",
            i32::from(*all),
            xoff.join(", ")
        )
        .unwrap();
    }
    writeln!(c, "}};\n#define NCASES {}", list.len()).unwrap();
    format!("{snaps}\n{c}{SNAP}\n{DRIVER}")
}

#[test]
fn preserving_callees_keep_what_llc_keeps() {
    let (callees, list, caller) = modules();
    let scratch = Scratch::new("x64-preserve-cc");
    let (m, bodies) = parse(&callees);
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&m, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{leg}: {e}"));
        objects.push((leg.to_owned(), scratch.write(&format!("{leg}.o"), object)));
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    for major in [22, 19] {
        if let Some(object) = float_gen::llc(&scratch, major, &format!("llc{major}"), &callees) {
            objects.push((format!("llc{major}"), object));
        }
    }
    let hod_caller = caller.replace("@CALLER", "@caller_hod");
    let (cm, cbodies) = parse(&hod_caller);
    let hod_caller = scratch.write(
        "caller-hod.o",
        hod_x64::compile_to_object(&cm, &cbodies, &pic(Stress::default())).expect("the caller"),
    );
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
    eprintln!("x64 preserve_cc: {} callees checked over {ran:?}", list.len());
}

/// What a preserving function cannot be is refused by name: a `musttail`
/// exit (the teardown would restore the argument registers), a result
/// returned in memory (`llc`'s saved set there is not the rule's). (A
/// coroutine body, `x64:coro-preserve-cc`, has no case here.)
#[test]
fn preserving_functions_refuse_by_name() {
    let one = |text: &str| format!("{}{text}", ll_header());
    let cases = [
        (
            one("declare preserve_mostcc i64 @g(i64)\n\
                 define preserve_mostcc i64 @f(i64 %a) {\n  \
                 %r = musttail call preserve_mostcc i64 @g(i64 %a)\n  ret i64 %r\n}\n"),
            hod_x64::frame::PRESERVE_TAIL_CALL,
        ),
        (
            one("define preserve_allcc { i64, i64, i64, i64 } @f() {\n  \
                 ret { i64, i64, i64, i64 } zeroinitializer\n}\n"),
            hod_x64::frame::PRESERVE_RETURN_MEMORY,
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
