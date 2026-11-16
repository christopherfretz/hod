//! WP-324: CET on i386 (`spec/codegen-x86.md` §17). `cet/cet.c`
//! (function pointers, `nocf_check`, `setjmp`/`longjmp`, a computed goto,
//! a libc callback) and `cet/throw.cpp` (landing pads), compiled by clang
//! 19 and 22 with `-fcf-protection=full`:
//!
//! - the programs run as `llc`'s do (static and PIE, GNU ld, lld, mold);
//! - the `.note.gnu.property` section is `llc`'s byte for byte (its
//!   header too) and `llvm-readelf -n` reads the same properties;
//! - `endbr32` is where `llc` puts it: per function, at least as many,
//!   at the entry of every function where `llc` has one, after each
//!   `setjmp` call, none at a `nocf_check` function's entry;
//! - a freestanding program whose objects all carry the property keeps
//!   it through each linker (and loses it with one object without).

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, back_end_refusal, data, lane, refusal};
use i386_support::{TRIPLE, checked, clang_module, llc_object, llvm_tool, run, tool};

const CET: &[&str] = &["-fcf-protection=full"];

#[test]
fn protected_programs_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-cet-run");
    let mut runs = 0;
    for pic in [false, true] {
        let build = Build { pic, all_linkers: true, ..STATIC };
        runs += lane.check(&scratch, "cet", &Source::C(&data("cet", "cet.c"), CET), build);
        let cpp = Build { link: &["-lstdc++", "-lm"], ..build };
        runs += lane.check(&scratch, "throw", &Source::C(&data("cet", "throw.cpp"), CET), cpp);
    }
    eprintln!("i386 cet: {runs} runs equal llc's");
}

/// The section's bytes and header fields (type, flags, alignment).
fn note(readelf: &Path, objdump: &Path, object: &Path) -> (String, String) {
    let out = checked("readelf -S", Command::new(readelf).arg("-S").arg(object));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let header = text
        .lines()
        .find(|l| l.contains(".note.gnu.property"))
        .unwrap_or_else(|| panic!("{}: no property note", object.display()));
    // `[ n] name type address offset size es flags lk inf al`: leave out
    // the index, address and offset.
    let fields: Vec<&str> = header.split(']').nth(1).unwrap().split_whitespace().collect();
    let header = format!("{} {} {}", fields[1], fields[4], fields[5..].join(" "));
    let out = checked(
        "objdump -s",
        Command::new(objdump).args(["-s", "-j", ".note.gnu.property"]).arg(object),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let bytes: Vec<&str> = text.lines().filter(|l| l.starts_with(" 0")).collect();
    (header, bytes.join("\n"))
}

/// The property lines of `llvm-readelf -n`.
fn properties(readelf: &Path, file: &Path) -> Vec<String> {
    let out = checked("readelf -n", Command::new(readelf).arg("-n").arg(file));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("Properties:") || l.contains("x86 feature"))
        .map(|l| l.trim().to_owned())
        .collect()
}

/// Per function, the number of `endbr32`, whether the first instruction
/// is one, and the number of `setjmp` calls not followed by one.
fn landings(objdump: &Path, object: &Path) -> Vec<(String, usize, bool, usize)> {
    let out = checked(
        "objdump -d",
        Command::new(objdump).args(["-d", "-r", "--no-show-raw-insn"]).arg(object),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let mut functions: Vec<(String, usize, bool, usize)> = Vec::new();
    let mut first = false;
    let mut pending_setjmp = false;
    for line in text.lines() {
        if let Some(name) = line.strip_suffix(">:") {
            let name = name.rsplit('<').next().unwrap().to_owned();
            functions.push((name, 0, false, 0));
            first = true;
            pending_setjmp = false;
            continue;
        }
        let Some(current) = functions.last_mut() else { continue };
        let relocation = line.contains("R_386_");
        if relocation {
            if line.contains("setjmp") {
                pending_setjmp = true;
            }
            continue;
        }
        if !line.contains(":\t") {
            continue;
        }
        let is_endbr = line.contains("endbr32");
        if first {
            current.2 = is_endbr;
            first = false;
        }
        if is_endbr {
            current.1 += 1;
        }
        if pending_setjmp && !line.contains("calll") {
            if !is_endbr {
                current.3 += 1;
            }
            pending_setjmp = false;
        }
    }
    functions
}

#[test]
fn the_note_and_the_landings_are_llc_s() {
    let scratch = Scratch::new("i386-cet-obj");
    let mut compared = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc), Some(readelf), Some(objdump)) = (
            llvm_tool(major, "clang"),
            llvm_tool(major, "llc"),
            llvm_tool(major, "llvm-readelf"),
            llvm_tool(major, "llvm-objdump"),
        ) else {
            continue;
        };
        for (file, pic) in [("cet.c", false), ("cet.c", true), ("throw.cpp", false)] {
            let label = format!("{file}-{major}-{}", if pic { "pic" } else { "static" });
            let mut flags = CET.to_vec();
            flags.push(if pic { "-fPIE" } else { "-fno-pic" });
            let ll = clang_module(&scratch, &clang, &data("cet", file), &flags, &label);
            let llc_flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
            let theirs = llc_object(&scratch, &llc, &ll, llc_flags, &format!("{label}-llc"));
            let build = Build { pic, ..STATIC };
            let text = std::fs::read(&ll).unwrap();
            let (plain, stressed) = i386_i11b::hod_objects(&text, build, &label);
            for (who, object) in [("hod", plain), ("stress", stressed)] {
                let ours = scratch.write(&format!("{label}-{who}.o"), object);
                let what = format!("{label} ({who})");
                assert_eq!(
                    note(&readelf, &objdump, &ours),
                    note(&readelf, &objdump, &theirs),
                    "{what}"
                );
                let props = properties(&readelf, &ours);
                assert_eq!(props, properties(&readelf, &theirs), "{what}");
                assert!(props.iter().any(|l| l.contains("IBT, SHSTK")), "{what}: {props:?}");
                let (want, got) = (landings(&objdump, &theirs), landings(&objdump, &ours));
                let mut names: Vec<&String> = want.iter().map(|f| &f.0).collect();
                names.sort();
                let mut ours_names: Vec<&String> = got.iter().map(|f| &f.0).collect();
                ours_names.sort();
                assert_eq!(names, ours_names, "{what}: functions");
                for (name, count, entry, missed) in &want {
                    let g = got.iter().find(|f| &f.0 == name).unwrap();
                    assert!(g.1 >= *count, "{what}: {name}: {} endbr32, llc {count}", g.1);
                    assert!(g.2 || !entry, "{what}: {name}: no endbr32 at the entry");
                    assert_eq!(*missed, 0, "{what}: {name}: llc without endbr32 after setjmp");
                    assert_eq!(g.3, 0, "{what}: {name}: no endbr32 after setjmp");
                    if name == "plain" {
                        assert!(!entry && !g.2, "{what}: nocf_check function with endbr32");
                    }
                }
                compared += 1;
            }
        }
    }
    eprintln!("i386 cet: {compared} objects compared with llc's");
}

/// A freestanding program (no C runtime, whose objects lack the
/// property): `_start` calls through a pointer and exits by `int 0x80`.
const START: &str = "@fp = global ptr @seven\n\
    define i32 @seven() {\n  ret i32 7\n}\n\
    define void @_start() noreturn nounwind {\n  %f = load ptr, ptr @fp\n  \
    %r = call i32 %f()\n  %s = call i32 @other()\n  %t = add i32 %r, %s\n  \
    call void asm sideeffect \"int $$0x80\", \"{ax},{bx},~{dirflag},~{fpsr},~{flags}\"(i32 1, i32 %t)\n  \
    unreachable\n}\ndeclare i32 @other()\n";
const OTHER: &str = "define i32 @other() {\n  ret i32 35\n}\n";
const FLAGS: &str = "!llvm.module.flags = !{!0, !1}\n\
    !0 = !{i32 8, !\"cf-protection-branch\", i32 1}\n\
    !1 = !{i32 8, !\"cf-protection-return\", i32 1}\n";

#[test]
fn each_linker_keeps_the_property() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-cet-link");
    let mut links = 0;
    for major in LLVM_MAJORS {
        let (Some(llc), Some(readelf)) =
            (llvm_tool(major, "llc"), llvm_tool(major, "llvm-readelf"))
        else {
            continue;
        };
        for marked in [true, false] {
            let flags = if marked { FLAGS } else { "" };
            let start = format!("target triple = \"{TRIPLE}\"\n{START}{FLAGS}");
            let other = format!("target triple = \"{TRIPLE}\"\n{OTHER}{flags}");
            let label = format!("{major}-{}", if marked { "all" } else { "one-without" });
            let mut objects: Vec<(&str, PathBuf, PathBuf)> = Vec::new();
            let start_ll = scratch.write(&format!("{label}-start.ll"), &start);
            let other_ll = scratch.write(&format!("{label}-other.ll"), &other);
            objects.push((
                "llc",
                llc_object(&scratch, &llc, &start_ll, &[], &format!("{label}-start-llc")),
                llc_object(&scratch, &llc, &other_ll, &[], &format!("{label}-other-llc")),
            ));
            let (s, _) = i386_i11b::hod_objects(start.as_bytes(), STATIC, &label);
            let (o, _) = i386_i11b::hod_objects(other.as_bytes(), STATIC, &label);
            objects.push((
                "hod",
                scratch.write(&format!("{label}-start-hod.o"), s),
                scratch.write(&format!("{label}-other-hod.o"), o),
            ));
            for linker in linkers::present(target) {
                let mut seen = Vec::new();
                for (who, a, b) in &objects {
                    let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                    let mut command = Command::new(&gcc);
                    command.args(linkers::driver_flags(scratch.path(), linker.0, &linker.1));
                    command.args(["-nostdlib", "-static", "-no-pie"]).arg(a).arg(b);
                    command.arg("-o").arg(&exe);
                    checked(&format!("{} link of {label} ({who})", linker.0), &mut command);
                    let (status, _) = run(&qemu, &exe);
                    assert_eq!(status, Some(42), "{label} ({who}, {})", linker.0);
                    seen.push(properties(&readelf, &exe));
                    links += 1;
                }
                let what = format!("{label} ({})", linker.0);
                assert_eq!(seen[1], seen[0], "{what}: hod's against llc's");
                let kept = seen[1].iter().any(|l| l.contains("IBT, SHSTK"));
                assert_eq!(kept, marked, "{what}: {:?}", seen[1]);
            }
        }
    }
    eprintln!("i386 cet: {links} freestanding links checked");
}

const BRANCH: &str = "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-branch\", i32 1}";

/// What compiles, and the indirect `nocf_check` call refused: it needs a
/// `notrack` prefix (`x86:nocf-check-indirect-call`).
#[test]
fn what_compiles_and_what_is_refused() {
    let fine = [
        "define i32 @f(ptr %p) {\n  %r = call i32 %p()\n  ret i32 %r\n}",
        "define i32 @f() nocf_check {\n  ret i32 1\n}",
        "declare i32 @g()\ndefine i32 @f() {\n  %r = call i32 @g() nocf_check\n  ret i32 %r\n}",
        "declare i32 @setjmp(ptr) returns_twice\n\
         define i32 @f(ptr %b) {\n  %r = call i32 @setjmp(ptr %b)\n  ret i32 %r\n}",
    ];
    for body in fine {
        let module = format!("{body}\n{BRANCH}");
        assert_eq!(refusal(&module, None), None, "{body}");
        assert_eq!(back_end_refusal(&module), None, "{body}");
    }
    let body = "define i32 @f(ptr %p) {\n  %r = call i32 %p() nocf_check\n  ret i32 %r\n}";
    let error = back_end_refusal(&format!("{body}\n{BRANCH}")).expect("refused");
    assert!(error.contains("x86:nocf-check-indirect-call"), "{error}");
    // Without the module flag, `nocf_check` is a no-op.
    assert_eq!(back_end_refusal(body), None);
}
