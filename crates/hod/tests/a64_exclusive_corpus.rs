//! The corpus entry `target.acle/exclusive-sysreg-run.cpp` (WP-245,
//! `spec/codegen.md` §5.6.39): the module kiln writes for it at `-O2`,
//! for LLVM 22 and LLVM 19 (saved as `tests/data/a64_exclusive/`), uses
//! `ldxr`/`stxr` at every size, `ldaxr`/`stlxr`, `ldaxp`/`stlxp`, `clrex`
//! and `cntvct_el0`. `hod::compile` (which refuses rather than falls
//! back) builds both; each is linked under every linker and run, and
//! prints what `llc`'s build of the same module prints, which is the
//! corpus entry's recorded output.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use hod::{Options, OptLevel, OutputKind, RelocModel};
use hod_testutil::{Scratch, run_with, tools_required};

const MODULES: [(u32, &str); 2] = [
    (22, include_str!("data/a64_exclusive/exclusive-sysreg-run.llvm22.ll")),
    (19, include_str!("data/a64_exclusive/exclusive-sysreg-run.llvm19.ll")),
];

/// What the corpus entry records as the program's output.
const STDOUT: &str = "ok 9\n";

/// `llc -O0 -global-isel=0 -fast-isel=0` of `major` on `text`.
fn llc(scratch: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        assert!(!tools_required(), "llc-{major} is not installed");
        return None;
    }
    let input = scratch.write(&format!("module-{major}.ll"), text);
    let output = scratch.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

#[test]
fn the_corpus_module_builds_and_runs_as_llc_s() {
    let scratch = Scratch::new("a64-exclusive-corpus");
    let Some(toolchain) = linking::Toolchain::aarch64(&scratch) else { return };
    let options = Options {
        opt_level: OptLevel::O0,
        relocation_model: Some(RelocModel::Pic),
        output_kind: OutputKind::Object,
        ..Options::default()
    };
    let mut runs = 0;
    for (major, text) in MODULES {
        let object =
            hod::compile(text.as_bytes(), &options).unwrap_or_else(|e| panic!("llvm{major}: {e}"));
        let mut objects = vec![(format!("hod-{major}"), scratch.write(&format!("hod-{major}.o"), object))];
        if let Some(theirs) = llc(&scratch, major, text) {
            objects.push((format!("llc-{major}"), theirs));
        }
        for (name, object) in &objects {
            for linker in &toolchain.linkers {
                let program = scratch.join(format!("{name}-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &["-lm"], &[object], &program);
                assert!(ran.success(), "{name} {}: {ran:?}", linker.name);
                assert_eq!(ran.stdout_text(), STDOUT, "{name} {}", linker.name);
                runs += 1;
            }
        }
    }
    eprintln!("a64 exclusive corpus: {runs} runs print the corpus entry's output");
}
