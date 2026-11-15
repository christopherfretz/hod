//! `win64cc` and `aarch64_vector_pcs` on AArch64 Linux (WP-247,
//! `spec/abi-a64.md` §13), built by hod, linked by every linker and run.
//!
//! - What the callee keeps (`data/cconv/keep.ll`, `keep.c`): `x18` for
//!   `win64cc`, `q8`..`q23` whole for `aarch64_vector_pcs`, checked by C's
//!   inline assembly around a `bl`, and `x18` as the unwinder recovers it
//!   in the caller; callees by hod (plain, stressed, far) and by `llc` 22
//!   and 19, whose CFI rules for those registers hod's must equal.
//! - Calls in both directions (`caller.ll`, `callee.ll`, `va.ll`): each
//!   module by hod and by `llc` 22 and 19, linked in every pairing;
//!   direct calls and calls through pointers, stack arguments, `sret`,
//!   arrays of `float`s and `double`s, `i128`, vectors, a return in
//!   memory, and variadic `win64cc` calls to `llc`'s functions (hod
//!   refuses their definition).
//! - Exceptions through such frames and into their landing pads (`eh.ll`,
//!   `eh-catch.ll`, `eh.cpp`).
//! - The corpus module `dcl.attr.cconv/cc-run.cpp` (`cc-run.ll`).
//! - The refusals by name.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, ll_header, parse, write_object};
use hod_a64::{cconv, compile_module};
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::Scratch;

const KEEP: &str = include_str!("data/cconv/keep.ll");
const KEEP_C: &str = include_str!("data/cconv/keep.c");
const CALLER: &str = include_str!("data/cconv/caller.ll");
const CALLEE: &str = include_str!("data/cconv/callee.ll");
const VA: &str = include_str!("data/cconv/va.ll");
const CALLS_C: &str = include_str!("data/cconv/calls.c");
const EH: &str = include_str!("data/cconv/eh.ll");
const EH_CATCH: &str = include_str!("data/cconv/eh-catch.ll");
const EH_CPP: &str = include_str!("data/cconv/eh.cpp");
const CC_RUN: &str = include_str!("data/cconv/cc-run.ll");

const PIC: Options = Options {
    reloc_model: RelocModel::Pic,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
    tls_descriptors: false,
};

/// hod's builds: plain, every stress option, far mode.
const MODES: [(&str, Options); 3] = [
    ("hod", PIC),
    ("hod-stress", Options { stress: Stress::ALL, ..PIC }),
    (
        "hod-far",
        Options {
            stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
            ..PIC
        },
    ),
];

fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    let found = hod_testutil::find_tool(&format!("llc-{major}"));
    assert!(
        found.is_some() || !hod_testutil::tools_required(),
        "llc {major} is not installed (HOD_REQUIRE_TOOLS=1)"
    );
    found
}

/// `llc -O0 -global-isel=0 -fast-isel=0` of LLVM `major` (the trusted
/// configuration) on `body` into `dir/name.o`.
fn llc_build(dir: &Scratch, major: u32, name: &str, body: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), format!("{}{body}", ll_header(major)));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc {major} on {name}:\n{}", outcome.stderr_text());
    Some(output)
}

/// hod's build of `body` with `options` into `dir/name.o`.
fn hod_build(dir: &Scratch, name: &str, body: &str, options: &Options) -> PathBuf {
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let emitted =
        compile_module(&module, &bodies, options).unwrap_or_else(|e| panic!("{name}: {e}"));
    write_object(dir, &format!("{name}.o"), &emitted)
}

/// Every build of `body`: hod's [`MODES`] (all of them, or the plain one
/// alone) and `llc` 22's and 19's.
fn builds(dir: &Scratch, name: &str, body: &str, every_mode: bool) -> Vec<(String, PathBuf)> {
    let modes = if every_mode { &MODES[..] } else { &MODES[..1] };
    let mut out: Vec<(String, PathBuf)> = modes
        .iter()
        .map(|(mode, options)| {
            ((*mode).to_owned(), hod_build(dir, &format!("{name}-{mode}"), body, options))
        })
        .collect();
    for major in [22, 19] {
        if let Some(object) = llc_build(dir, major, &format!("{name}-llc{major}"), body) {
            out.push((format!("llc{major}"), object));
        }
    }
    out
}

/// Links `inputs` under every linker and runs the program: it must
/// succeed and print `want` (the first run's output when `None`), which
/// is returned.
fn run_everywhere(
    toolchain: &Toolchain,
    dir: &Scratch,
    label: &str,
    inputs: &[&Path],
    want: &mut Option<String>,
) {
    for linker in &toolchain.linkers {
        let exe = dir.join(format!("{label}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], inputs, &exe);
        let got = ran.stdout_text();
        assert!(ran.success(), "{label} under {}: {got}", linker.name);
        assert_eq!(&got, want.get_or_insert_with(|| got.clone()), "{label} under {}", linker.name);
    }
}

/// The registers some CFI row of `object` gives a rule (`W18`, `B8`, ..)
/// by `llvm-dwarfdump`, less the frame record's (`W29`, `W30`), whose
/// saving differs between hod and `llc`; `None` without the tool.
fn kept_rules(object: &Path) -> Option<std::collections::BTreeSet<String>> {
    let tool = hod_testutil::find_tool("llvm-dwarfdump")?;
    let mut command = Command::new(tool);
    command.arg("--eh-frame").arg(object);
    let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-dwarfdump: {}", outcome.stderr_text());
    let text = outcome.stdout_text();
    let rows = text.lines().filter_map(|line| line.trim().split_once(": CFA=")).map(|(_, r)| r);
    let rules = rows.flat_map(|row| row.split(": ").nth(1).unwrap_or_default().split(", "));
    let names = rules.filter_map(|rule| Some(rule.split_once('=')?.0.to_owned()));
    Some(names.filter(|name| name != "W29" && name != "W30").collect())
}

/// §13.4, §13.5: a `win64cc` callee keeps `x18`, an `aarch64_vector_pcs`
/// callee `q8`..`q23` whole, as `llc` 22's and 19's do; `ccc` callers
/// call them.
#[test]
fn callees_keep_what_llc_keeps() {
    let dir = Scratch::new("a64-cconv-keep");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let c = toolchain.compile_c(&dir, "keep-c", KEEP_C, &[]);
    let builds = builds(&dir, "keep", KEEP, true);
    let mut want = Some("46 4625196817309499400\n".to_owned());
    for (name, object) in &builds {
        run_everywhere(&toolchain, &dir, name, &[c.as_path(), object.as_path()], &mut want);
    }
    let rules: Vec<_> = builds.iter().filter_map(|(n, o)| Some((n, kept_rules(o)?))).collect();
    assert!(!rules.is_empty() || !hod_testutil::tools_required(), "no llvm-dwarfdump");
    for (name, set) in &rules {
        assert!(set.contains("W18"), "{name}: no rule for x18: {set:?}");
        assert_eq!(set, &rules[0].1, "{name} describes other registers than {}", rules[0].0);
    }
}

/// §13.2, §13.3: hod's and `llc`'s builds call each other's functions of
/// both conventions, and hod's make variadic `win64cc` calls to `llc`'s.
#[test]
fn calls_in_both_directions() {
    let dir = Scratch::new("a64-cconv-calls");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let c = toolchain.compile_c(&dir, "calls-c", CALLS_C, &[]);
    let calling = builds(&dir, "caller", CALLER, true);
    let called = builds(&dir, "callee", CALLEE, false);
    let Some(va) = llc_build(&dir, 22, "va-llc22", VA) else { return };
    let mut want = None;
    for (caller, a) in &calling {
        for (callee, b) in &called {
            if caller.starts_with("llc") && callee.starts_with("llc") && caller != callee {
                continue;
            }
            let label = format!("{caller}-{callee}");
            let inputs = [c.as_path(), a.as_path(), b.as_path(), va.as_path()];
            run_everywhere(&toolchain, &dir, &label, &inputs, &mut want);
        }
    }
    let want = want.unwrap_or_default();
    assert!(want.starts_with("many 13462041186 "), "{want}");
    assert!(want.contains("\nva 136865576 268913708 400961840\n"), "{want}");
}

/// §13.4, §13.5: exceptions thrown through hod's frames of both
/// conventions reach `llc`'s and hod's catchers, and landing pads in
/// such frames catch, as with `llc`'s builds.
#[test]
fn exceptions_through_such_frames() {
    let dir = Scratch::new("a64-cconv-eh");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let Some(cpp) = toolchain.compile_cxx(&dir, "eh-cpp", EH_CPP, &[]) else { return };
    let throwers = builds(&dir, "eh", EH, true);
    let catchers = builds(&dir, "eh-catch", EH_CATCH, false);
    let stdcxx = Path::new("-lstdc++");
    let mut want = Some("0 7 7 9\n505 -5 1005 2005\n-909 9 991 1991\n".to_owned());
    for (thrower, a) in &throwers {
        for (catcher, b) in &catchers {
            let label = format!("eh-{thrower}-{catcher}");
            let inputs = [cpp.as_path(), a.as_path(), b.as_path(), stdcxx];
            run_everywhere(&toolchain, &dir, &label, &inputs, &mut want);
        }
    }
}

/// The corpus module `dcl.attr.cconv/cc-run.cpp` (kiln, LLVM 22), which
/// uses both conventions: built by hod, linked by every linker, run; its
/// output is the one the corpus records, and `llc`'s build's.
#[test]
fn corpus_module_runs() {
    let dir = Scratch::new("a64-cconv-corpus");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let (module, bodies) = parse(CC_RUN);
    let emitted = compile_module(&module, &bodies, &PIC).unwrap_or_else(|e| panic!("cc-run: {e}"));
    let hod = write_object(&dir, "cc-run-hod.o", &emitted);
    let mut objects = vec![("hod".to_owned(), hod)];
    if let Some(llc) = llc_of(22) {
        let input = dir.write("cc-run.ll", CC_RUN);
        let output = dir.join("cc-run-llc.o");
        let mut command = Command::new(llc);
        command
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"])
            .args(["-filetype=obj"])
            .arg(&input)
            .arg("-o")
            .arg(&output);
        let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(2));
        assert!(outcome.success(), "llc on cc-run: {}", outcome.stderr_text());
        objects.push(("llc22".to_owned(), output));
    }
    let (stdcxx, libm) = (Path::new("-lstdc++"), Path::new("-lm"));
    let mut want = Some("pm 21 pa 3.5 ms 60 msr 44 v 7 l 9\nt 42\n".to_owned());
    for (leg, object) in &objects {
        let inputs = [object.as_path(), stdcxx, libm];
        run_everywhere(&toolchain, &dir, &format!("cc-run-{leg}"), &inputs, &mut want);
    }
}

/// §13.6: what hod does not build of the two conventions is refused by
/// name.
#[test]
fn refusals_are_named() {
    assert_refused(&[
        (
            "declare void @llvm.va_start.p0(ptr)\n\
             define win64cc i64 @f(i64 %a, ...) {\n  %ap = alloca ptr\n  \
             call void @llvm.va_start.p0(ptr %ap)\n  ret i64 %a\n}\n",
            cconv::WIN64_VARIADIC_DEFINITION,
        ),
        (
            "declare win64cc i64 @g(i64)\n\
             define win64cc i64 @f(i64 %a) {\n  \
             %r = musttail call win64cc i64 @g(i64 %a)\n  ret i64 %r\n}\n",
            cconv::WIN64_MUSTTAIL,
        ),
        (
            "declare win64cc void @g(ptr nest)\n\
             define void @f(ptr %p) {\n  call win64cc void @g(ptr nest %p)\n  ret void\n}\n",
            cconv::WIN64_NEST,
        ),
        (
            "declare win64cc void @g(i32, ...)\n\
             define void @f() {\n  call win64cc void (i32, ...) @g(i32 1, half 2.0)\n  \
             ret void\n}\n",
            cconv::WIN64_VARIADIC_HALF,
        ),
        (
            "declare win64cc void @g(i32, ...)\n\
             define void @f() {\n  call win64cc void (i32, ...) @g(i32 1, [2 x bfloat] zeroinitializer)\n  \
             ret void\n}\n",
            cconv::WIN64_VARIADIC_HALF,
        ),
        (
            "define aarch64_vector_pcs i64 @f(i64 %a, ...) {\n  ret i64 %a\n}\n",
            cconv::VPCS_VARIADIC_DEFINITION,
        ),
        (
            "declare aarch64_vector_pcs i64 @g(i64)\n\
             define aarch64_vector_pcs i64 @f(i64 %a) {\n  \
             %r = musttail call aarch64_vector_pcs i64 @g(i64 %a)\n  ret i64 %r\n}\n",
            cconv::VPCS_MUSTTAIL,
        ),
        (
            "declare void @llvm.eh.unwind.init()\n\
             define win64cc void @f() {\n  call void @llvm.eh.unwind.init()\n  ret void\n}\n",
            hod_a64::unwind::UNWIND_INIT_CONVENTION,
        ),
    ]);
    // Every name is in the list of §13.6 once.
    let names: std::collections::BTreeSet<_> = cconv::UNSUPPORTED.into_iter().collect();
    assert_eq!(names.len(), cconv::UNSUPPORTED.len());
}
