//! WP-214: what kiln (from `21b9110a4`) newly writes for driver options
//! reaches hod and is built (`spec/ir.md` §5.7, `spec/codegen.md`
//! §5.6.27, `spec/dwarf.md` §2).
//!
//! - `-fstack-clash-protection` (with and without `-mstack-probe-size=`):
//!   kiln writes `"probe-stack"="inline-asm"` (and `"stack-probe-size"`)
//!   on every definition and the module flags; hod builds the module
//!   strictly, and the program, with a frame of several pages and an
//!   `alloca`, prints what the `llc` build prints, on both targets
//!   (x86-64 under `qemu-x86_64`), with every linker.
//! - `-grecord-gcc-switches`: kiln writes the command line into the
//!   compile unit's `flags:`; `llc` appends it to `DW_AT_producer` (off
//!   Apple targets), and so does hod (WP-221).
//!
//! `-mtls-dialect=gnu2` makes kiln pass `-enable-tlsdesc` to its back
//! end; hod does not know that option yet and declines the module (kiln
//! then keeps its LLVM route), which WP-213 changes; it is not tested here.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use hod::{OutputKind, RelocModel};
use hod_testutil::{Scratch, Target, run_with, tool, tools_required};
use linking::Toolchain;

const PROGRAM: &str = r#"
#include <stdio.h>
#include <string.h>
#include <alloca.h>
__attribute__((noinline)) int big(int n) {
  volatile char buf[300000];
  for (int i = 0; i < 300000; i += 4096) buf[i] = (char)(i + n);
  char *d = (char *)alloca(n);
  memset(d, n & 0x7f, n);
  int s = 0;
  for (int i = 0; i < 300000; i += 4096) s += (unsigned char)buf[i];
  return s + d[n - 1];
}
int main() { printf("%d\n", big(100000)); return 0; }
"#;

/// kiln++, as the other kiln-dependent tests find it.
fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping");
            None
        }
    }
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}", ran.stderr_text());
    ran.stdout_text()
}

fn triple(target: Target) -> String {
    format!("--target={}-unknown-linux-gnu", target.name())
}

fn hod_options() -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..hod::Options::default()
    }
}

#[test]
fn stack_clash_protection_from_kiln_runs() {
    let scratch = Scratch::new("hod-wp214-clash");
    let Some(kiln) = kiln() else { return };
    let Some(llc) = tool("llc") else { return };
    let source = scratch.write("clash.cpp", PROGRAM);
    let mut runs = 0;
    // Not i686, whose back end refuses inline stack probing
    // (`x86:probe-stack`, which `-fstack-clash-protection` asks for).
    for target in [Target::Aarch64, Target::X86_64] {
        // A directory per target: each toolchain makes its linkers' own.
        let linkers = Scratch::new(&format!("hod-wp214-clash-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&linkers, target) else { continue };
        for (index, flags) in [
            &["-fstack-clash-protection"][..],
            &["-fstack-clash-protection", "-mstack-probe-size=8192"],
        ]
        .iter()
        .enumerate()
        {
            let tag = format!("{}-{index}", target.name());
            let ir = scratch.join(format!("{tag}.ll"));
            let mut command = Command::new(&kiln);
            command
                .arg(triple(target))
                .args(["-O0", "-fPIE", "-fno-exceptions", "-S", "-emit-llvm"])
                .args(*flags)
                .arg(&source)
                .arg("-o")
                .arg(&ir);
            run_ok(&mut command, &tag);
            let text = std::fs::read_to_string(&ir).expect("kiln's IR");
            assert!(text.contains("\"probe-stack\"=\"inline-asm\""), "{tag}: no probe-stack");
            assert!(text.contains("!\"probe-stack\""), "{tag}: no probe-stack module flag");
            if index == 1 {
                assert!(text.contains("\"stack-probe-size\"=\"8192\""), "{tag}: no probe size");
            }
            let hod_object = scratch.join(format!("{tag}-hod.o"));
            let bytes = hod::compile(text.as_bytes(), &hod_options())
                .unwrap_or_else(|e| panic!("hod, {tag}: {e}"));
            std::fs::write(&hod_object, bytes).expect("the object");
            let llc_object = scratch.join(format!("{tag}-llc.o"));
            let mut command = Command::new(&llc);
            command
                .args(["-O0", "-relocation-model=pic", "-filetype=obj"])
                .arg(&ir)
                .arg("-o")
                .arg(&llc_object);
            run_ok(&mut command, &tag);
            let mut expected: Option<String> = None;
            for object in [&llc_object, &hod_object] {
                for linker in &toolchain.linkers {
                    let exe = scratch.join(format!("{tag}-{}", linker.name));
                    let ran = toolchain.link_and_run(linker, &["-pie"], &[object.as_path()], &exe);
                    assert!(ran.success(), "{tag} ({}): {}", linker.name, ran.stderr_text());
                    let out = ran.stdout_text();
                    match &expected {
                        None => expected = Some(out),
                        Some(want) => assert_eq!(&out, want, "{tag} {}", object.display()),
                    }
                    runs += 1;
                }
            }
        }
    }
    eprintln!("wp214 stack clash: {runs} runs agree");
}

/// `-grecord-gcc-switches`: `llc` writes `DW_AT_producer` as the
/// producer, a space and the compile unit's `flags:` (LLVM
/// `DwarfDebug::constructDwarfCompileUnit`, off Apple targets); hod
/// writes the same (WP-221, `spec/dwarf.md` 6.30).
#[test]
fn recorded_command_line_is_in_the_producer() {
    let scratch = Scratch::new("hod-wp214-flags");
    let Some(kiln) = kiln() else { return };
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let source = scratch.write("flags.cpp", "int main() { return 0; }\n");
    let ir = scratch.join("flags.ll");
    let mut command = Command::new(&kiln);
    command
        .args(["-O0", "-g", "-grecord-gcc-switches", "-S", "-emit-llvm"])
        .arg(&source)
        .arg("-o")
        .arg(&ir);
    run_ok(&mut command, "kiln");
    let text = std::fs::read(&ir).expect("kiln's IR");
    let object = scratch.write(
        "flags.o",
        hod::compile(&text, &hod_options()).unwrap_or_else(|e| panic!("hod: {e}")),
    );
    let dump = run_ok(Command::new(&dwarfdump).arg("--debug-info").arg(&object), "dwarfdump");
    let producer = dump.lines().find(|l| l.contains("DW_AT_producer")).unwrap_or_default();
    assert!(producer.contains("-grecord-gcc-switches"), "{producer}");
}
