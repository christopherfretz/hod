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
