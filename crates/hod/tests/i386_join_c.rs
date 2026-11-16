//! The i386 join C (`spec/landed/codegen/i386-join-c.md`): what the
//! integer packages (WP-306, WP-309) and the x87 package (WP-300) left
//! for the join. Programs under `tests/data/i386/join_c/`, C through
//! clang 19 and 22 (`-fno-math-errno`: `frem` and the libm intrinsics),
//! at CPU `i586` (no `cmov`) and `pentiumpro` (`cmov`, no SSE),
//! compiled by `llc`, hod and `hod_x86` under `hod_cg::Stress::ALL`,
//! linked by the cross C compiler's driver and run under `qemu-i386`:
//! the outputs are equal byte for byte.
//!
//! - `fptoui.c`: `fptoui` to `i64` from `float`, `double`, `long double`
//!   (`spec/codegen-x86.md` §11 "Conversions"), also of a product
//!   computed in the same expression (its 80 bits, or rounded where
//!   `llc` branches).
//! - `libm.c`: `double` `frem`, `fma`, `minnum`/`maxnum` and the libm
//!   routines over edge values (§11 "libm").
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{
    TRIPLE, checked, clang_module, llc_object, llvm_tool, run, stressed_object, tool,
};

const PROGRAMS: [&str; 2] = ["fptoui.c", "libm.c"];

/// The CPU variants and clang's flags for them. Not clang's default:
/// for `i686-linux-gnu` it writes `"target-cpu"="i686"`, which has
/// `cmov`; `i586` has none.
const CPUS: [(&str, &[&str]); 2] =
    [("i586", &["-march=i586"]), ("pentiumpro", &["-march=pentiumpro"])];

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/join_c").join(path)
}

fn hod_object(text: &[u8], label: &str) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    hod::compile(text, &options).unwrap_or_else(|e| panic!("hod {label}: {e}"))
}

#[test]
fn conversions_and_libm_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-join-c");
    let (mut modules, mut lines) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in PROGRAMS {
            for (cpu, cpu_flags) in CPUS {
                let label = format!("{program}-{major}-{cpu}");
                let mut flags = vec!["-fno-pic", "-fno-pie", "-fno-math-errno"];
                flags.extend_from_slice(cpu_flags);
                let ll = clang_module(&scratch, &clang, &data(program), &flags, &label);
                let text = std::fs::read(&ll).unwrap();
                let objects = [
                    ("llc", llc_object(&scratch, &llc, &ll, &[], &format!("{label}-llc"))),
                    ("hod", scratch.write(&format!("{label}-hod.o"), hod_object(&text, &label))),
                    ("stress", scratch.write(&format!("{label}-stress.o"), stressed_object(&text))),
                ];
                let mut outputs = Vec::new();
                for (who, object) in &objects {
                    let exe = scratch.join(format!("{label}-{who}"));
                    let mut command = Command::new(&gcc);
                    command.arg("-no-pie").arg(object).args(["-lm", "-o"]).arg(&exe);
                    checked(&format!("link {label} {who}"), &mut command);
                    outputs.push((*who, run(&qemu, &exe)));
                }
                let (_, (status, reference)) = &outputs[0];
                assert_eq!(*status, Some(0), "{label}: llc's status");
                assert!(reference.ends_with(b"done\n"), "{label}: llc's run did not finish");
                for (who, result) in &outputs[1..] {
                    if result.1 != *reference || result.0 != *status {
                        let (want, got) = (
                            String::from_utf8_lossy(reference),
                            String::from_utf8_lossy(&result.1),
                        );
                        let diff: Vec<_> = want
                            .lines()
                            .zip(got.lines())
                            .filter(|(a, b)| a != b)
                            .take(20)
                            .map(|(a, b)| format!("  llc {a}\n  {who} {b}"))
                            .collect();
                        panic!(
                            "{label}: {who}'s run differs from llc's ({:?}):\n{}",
                            result.0,
                            diff.join("\n")
                        );
                    }
                }
                modules += 1;
                lines += String::from_utf8_lossy(reference).lines().count();
            }
        }
    }
    eprintln!(
        "i386 join C: {modules} modules, {lines} lines of llc's output, hod's and stress's equal"
    );
}

/// `llvm.threadlocal.address` of an operand that is no global (IR the
/// verifier rejects) gives on i686 the diagnostic x86-64 and AArch64 give
/// for the same module.
#[test]
fn threadlocal_address_of_no_global_is_diagnosed_as_on_the_other_targets() {
    let text = "define ptr @f(ptr %p) {\n  %a = call ptr @llvm.threadlocal.address.p0(ptr %p)\n  \
                ret ptr %a\n}\ndeclare ptr @llvm.threadlocal.address.p0(ptr)\n";
    let error = |triple: &str| {
        let options = Options {
            output_kind: OutputKind::Object,
            triple: Some(triple.into()),
            ..Options::default()
        };
        hod::compile(text.as_bytes(), &options).expect_err(triple).to_string()
    };
    let i686 = error(TRIPLE);
    assert!(i686.contains("llvm.threadlocal.address of something that is no symbol"), "{i686}");
    for other in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        assert_eq!(error(other), i686, "{other}");
    }
}
