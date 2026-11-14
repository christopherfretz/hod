//! System registers in inline assembly (WP-188, `spec/asm.md` §5.4): the
//! seeds of `tests/data/sysreg_asm/functions.ll` (`mrs`/`msr` of named
//! registers and of the generic form) are compiled by hod and by
//! `llc -O0 -global-isel=0 -fast-isel=0` of each LLVM major present;
//! each object is linked with `tests/data/sysreg_asm/driver.c`, which
//! checks every function against the C compiler's own `mrs`, under every
//! linker, and run. The system-register words of hod's code are `llc`'s.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{code_words, emit, ll_header, parse, write_object};
use hod_cg::Options;
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tool};

const FUNCTIONS: &str = include_str!("data/sysreg_asm/functions.ll");
const DRIVER: &str = include_str!("data/sysreg_asm/driver.c");

fn llc(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
    let input = dir.write(&format!("llc-{major}.ll"), format!("{}{FUNCTIONS}", ll_header(major)));
    let output = dir.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

/// The `mrs`/`msr` (register) words of `.text` of `object`, with the
/// general register cleared (the allocators choose differently), sorted.
fn sysreg_words(dir: &Scratch, object: &Path, major: u32) -> Option<Vec<u32>> {
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let bin = dir.join(format!("text-{major}.bin"));
    let mut command = Command::new(objcopy);
    command.args(["-O", "binary", "--only-section=.text"]).arg(object).arg(&bin);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "objcopy: {}", outcome.stderr_text());
    let bytes = std::fs::read(&bin).unwrap();
    let words = bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    Some(sorted(words))
}

fn sorted(words: impl Iterator<Item = u32>) -> Vec<u32> {
    let mut found: Vec<u32> =
        words.filter(|w| w & 0xffd0_0000 == 0xd510_0000).map(|w| w & !0x1f).collect();
    found.sort_unstable();
    found
}

#[test]
fn seeds_run_as_llc_builds_them() {
    let dir = Scratch::new("a64-sysreg-asm");
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header(22)));
    let ours = sorted(code_words(&m, &bodies, &Options::default()).into_iter());
    assert_eq!(ours.len(), 17, "{ours:08x?}");
    let mut objects = vec![(
        "hod".to_owned(),
        write_object(&dir, "hod.o", &emit(&m, &bodies, &Options::default())),
    )];
    for major in LLVM_MAJORS {
        if let Some(theirs) = llc(&dir, major) {
            if let Some(words) = sysreg_words(&dir, &theirs, major) {
                assert_eq!(ours, words, "the system-register words of llc-{major}");
            }
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &[]);
    for (who, object) in &objects {
        for linker in &toolchain.linkers {
            let program = dir.join(format!("{who}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            assert_eq!(
                exit_code(&ran),
                Some(0),
                "{who} {}: {}{}",
                linker.name,
                ran.stdout_text(),
                ran.stderr_text()
            );
            assert_eq!(ran.stdout_text(), "checked, 0 failures\n", "{who} {}", linker.name);
        }
    }
}

/// The shapes kiln writes for abseil's `UnscaledCycleClock::Now` and
/// `Frequency` and `crc_internal::GetCpuType` (WP-196): `optnone`
/// functions with kiln's AArch64 `target-features`, the counter read
/// through a `sideeffect` template, `MIDR_EL1` in upper case through one
/// without. They compile, and their system-register words are `llc`'s.
/// (kiln writes `"frame-pointer"="non-leaf-no-reserve"`, which LLVM 19
/// does not read; `non-leaf` stands in for it.)
const KILN_SHAPES: &str = r#"
define dso_local noundef i64 @now() #0 {
  %1 = alloca i64, align 8
  %2 = call i64 asm sideeffect "mrs $0, cntvct_el0", "=r"() #1
  store i64 %2, ptr %1, align 8
  %3 = load i64, ptr %1, align 8
  ret i64 %3
}

define dso_local noundef double @frequency() #0 {
  %1 = alloca i64, align 8
  %2 = call i64 asm sideeffect "mrs $0, cntfrq_el0", "=r"() #1
  store i64 %2, ptr %1, align 8
  %3 = load i64, ptr %1, align 8
  %4 = uitofp i64 %3 to double
  ret double %4
}

define dso_local i32 @cpu_part() #0 {
  %1 = call i64 asm "mrs $0, MIDR_EL1", "=r"() #2
  %2 = lshr i64 %1, 4
  %3 = and i64 %2, 4095
  %4 = trunc i64 %3 to i32
  ret i32 %4
}

attributes #0 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nounwind }
attributes #2 = { nounwind memory(none) }
"#;

#[test]
fn kiln_shapes_compile_with_llc_s_words() {
    let dir = Scratch::new("a64-sysreg-kiln");
    let (m, bodies) = parse(&format!("{}{KILN_SHAPES}", ll_header(22)));
    let ours = sorted(code_words(&m, &bodies, &Options::default()).into_iter());
    assert_eq!(ours.len(), 3, "{ours:08x?}");
    for major in LLVM_MAJORS {
        let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
        if !llc.is_file() {
            continue;
        }
        let input =
            dir.write(&format!("llc-{major}.ll"), format!("{}{KILN_SHAPES}", ll_header(major)));
        let output = dir.join(format!("llc-{major}.o"));
        let mut command = Command::new(llc);
        command
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .arg(&input)
            .arg("-o")
            .arg(&output);
        let outcome = run_with(&mut command, &[], Duration::from_mins(2));
        assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
        if let Some(words) = sysreg_words(&dir, &output, major) {
            assert_eq!(ours, words, "the system-register words of llc-{major}");
        }
    }
}
