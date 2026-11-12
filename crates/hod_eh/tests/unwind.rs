//! `spec/eh.md` §7.2: real unwinding. `tests/data/unwind.ll` (C++ compiled
//! by clang 19 at `-O0`; the source is `tests/data/unwind.cpp`) throws
//! through a large frame, a frame with dynamic stack allocation, a COMDAT
//! frame and frames with cleanups, catches and rethrows on the way, and
//! catches in `main`. `llc` compiles it; its `.eh_frame` and
//! `.rela.eh_frame` are replaced by the ones `hod_eh` writes from the
//! decoded rows (every other byte of the object unchanged, see
//! `common::Elf::replace`); the program is linked with GNU ld, lld and mold,
//! with and without `--eh-frame-hdr`, and must print what the original
//! prints.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use common::{Elf, compile, llc, round_trip};
use hod_obj::Arch;
use hod_testutil::{Outcome, Scratch, run, tool};

const IR: &str = include_str!("data/unwind.ll");
const EXPECTED: &str = "cleanup large\ncleanup template\nrethrowing 1\ncaught 101\n\
cleanup large\ncleanup template\nrethrowing 2\ncaught 102\n\
cleanup large\ncleanup template\nrethrowing 3\ncaught 103\nno throw\n";

/// The linkers, each as the `g++` flags that select it.
fn linkers(scratch: &Scratch) -> Vec<(&'static str, Vec<OsString>)> {
    let mut out = Vec::new();
    for (name, program) in [("bfd", "ld.bfd"), ("lld", "ld.lld"), ("mold", "ld.mold")] {
        let Some(found) = tool(program) else { continue };
        let dir = scratch.join(format!("use-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join(format!("ld.{name}"));
        if !link.exists() {
            std::os::unix::fs::symlink(&found, &link).unwrap();
        }
        let mut prefix = OsString::from("-B");
        prefix.push(&dir);
        out.push((name, vec![prefix, format!("-fuse-ld={name}").into()]));
    }
    out
}

fn link_and_run(
    driver: &Path,
    flags: &[OsString],
    hdr: bool,
    object: &Path,
    program: &Path,
) -> Outcome {
    let mut args: Vec<OsString> = flags.to_vec();
    args.push(object.into());
    if !hdr {
        args.push("-Wl,--no-eh-frame-hdr".into());
    }
    args.push("-o".into());
    args.push(program.into());
    let linked = run(driver, &args);
    assert!(linked.success(), "link {}: {}", program.display(), linked.stderr_text());
    let has_hdr = Elf::parse(std::fs::read(program).unwrap()).section(".eh_frame_hdr").is_some();
    assert_eq!(has_hdr, hdr, "{}: .eh_frame_hdr as asked", program.display());
    run(program, Vec::<OsString>::new())
}

fn unwind(major: u32) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the unwinding programs are AArch64; skipping on this host");
        return;
    }
    let Some(llc) = llc(major) else { return };
    let Some(driver) = tool("g++") else { return };
    let scratch = Scratch::new(&format!("eh-unwind{major}"));
    let label = format!("unwind-{major}");
    let object = compile(&llc, &scratch, &label, IR, "aarch64-unknown-linux-gnu");
    let Some(rt) = round_trip(&scratch, &label, Arch::AArch64, IR, &object, false) else { return };
    assert!(rt.cies == 2 && rt.used_state, "the program has functions with landing pads");
    let original = scratch.write(&format!("{label}.orig.o"), &object);
    let patched = scratch.write(&format!("{label}.hod.o"), &rt.patched);
    let mut report = Vec::new();
    for (name, flags) in linkers(&scratch) {
        for hdr in [true, false] {
            let tag = format!("{label}-{name}-{}", if hdr { "hdr" } else { "nohdr" });
            let want: PathBuf = scratch.join(format!("{tag}.orig"));
            let got: PathBuf = scratch.join(format!("{tag}.hod"));
            let want = link_and_run(&driver, &flags, hdr, &original, &want);
            let got = link_and_run(&driver, &flags, hdr, &patched, &got);
            if hdr {
                assert_eq!(want.stdout_text(), EXPECTED, "{tag}: the original program");
                assert!(want.success(), "{tag}: the original program");
            }
            // Without `PT_GNU_EH_FRAME` this libgcc finds no FDE at all (it
            // looks them up through `dl_iterate_phdr`), so the original
            // terminates at the first throw; the replacement must do the
            // same, no more and no less.
            assert_eq!(got.stdout_text(), want.stdout_text(), "{tag}: {}", got.stderr_text());
            assert_eq!(got.status.map(|s| s.code()), want.status.map(|s| s.code()), "{tag}");
            let caught = got.stdout_text() == EXPECTED;
            report.push(format!("{tag}: {}", if caught { "caught" } else { "terminated" }));
        }
    }
    eprintln!("as the original: {}", report.join(", "));
}

/// §7.2 with LLVM 19.
#[test]
fn llc19_objects_unwind_through_hod_eh_frame() {
    unwind(19);
}

/// §7.2 with LLVM 22.
#[test]
fn llc22_objects_unwind_through_hod_eh_frame() {
    unwind(22);
}
