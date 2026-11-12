//! `spec/dwarf.md` §8.6: hod's type graph under a real debugger.
//!
//! The oracle input of `tests/data/types/types.cpp` (clang 19, see
//! `common/types.rs`) is compiled by `llc` 19 and 22; its debug sections
//! are replaced by hod's, every other byte unchanged. Each object is
//! linked into a shared library (its deleted functions stay undefined)
//! with GNU ld, lld and mold. gdb (`ptype`, `ptype /o`, `whatis`, `print
//! sizeof`, `info types`) and lldb (`type lookup`, `image lookup -t`)
//! read the types without running anything, and must print the same for
//! hod's library as for `llc`'s.

mod common;

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::dies::{die_section_list, splice};
use common::session::{linkers, masked};
use common::types::{cxx, extract, module_sections, oracle_input, read};
use common::{Elf, compile, llc};
use hod_obj::Arch;
use hod_testutil::{Scratch, find_tool, run, run_with, tool};

const TRIPLE: &str = "aarch64-unknown-linux-gnu";

/// The types gdb prints with `ptype` and lldb with `type lookup`.
const TYPES: [&str; 20] = [
    "Bits",
    "Over",
    "Access",
    "Derived",
    "Diamond",
    "Priv",
    "U",
    "WithAnon",
    "Color",
    "Wide",
    "Small",
    "Big",
    "ns::Outer",
    "ns::Outer::Inner",
    "ns::deeper::Leaf",
    "Node",
    "Other",
    "NonTrivial",
    "Trivial",
    "Unsigned",
];

/// The layouts `ptype /o` and `print sizeof` print.
const LAYOUTS: [&str; 8] =
    ["Bits", "Over", "Access", "Derived", "Diamond", "U", "WithAnon", "Priv"];

/// The variables `whatis` and `ptype` print.
const VARIABLES: [&str; 43] = [
    "b", "wc", "c8", "c16", "c32", "i128", "ld", "f16", "cf", "bi7", "ubi13", "np", "pci", "pcvu",
    "rp", "ai", "ref", "rref", "cref", "sz", "str", "tap", "cvp", "pmd", "pmf", "pmf2", "arr",
    "matrix", "punk", "text", "names", "empties", "vec", "vf", "fp", "vfp", "nfp", "fref", "fnp",
    "fwdp", "fwdu", "opaque", "tmpl",
];

fn gdb_session(gdb: &Path, library: &Path) -> String {
    let mut cmd = Command::new(gdb);
    cmd.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"])
        .args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"])
        .args(["-ex", "set width 0"]);
    for t in TYPES {
        cmd.arg("-ex").arg(format!("ptype {t}"));
    }
    for t in LAYOUTS {
        cmd.arg("-ex").arg(format!("ptype /o {t}"));
        cmd.arg("-ex").arg(format!("print sizeof({t})"));
    }
    for v in VARIABLES {
        cmd.arg("-ex").arg(format!("whatis {v}"));
        cmd.arg("-ex").arg(format!("ptype {v}"));
    }
    for e in ["Access::limit", "Access::ulimit", "Access::ratio", "Access::neg", "Access::letter"] {
        cmd.arg("-ex").arg(format!("print {e}"));
    }
    cmd.args(["-ex", "info types Color", "-ex", "info types ^ns::"]);
    cmd.arg(library);
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "gdb on {}", library.display());
    masked(&(out.stdout_text() + &out.stderr_text()), library)
}

fn lldb_session(lldb: &Path, library: &Path) -> String {
    let mut cmd = Command::new(lldb);
    cmd.args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"]);
    cmd.arg(library);
    // Each a type lldb finds on `llc`'s library: the batch stops at the
    // first failed command.
    for t in TYPES {
        cmd.arg("-o").arg(format!("type lookup {t}"));
    }
    for t in LAYOUTS {
        cmd.arg("-o").arg(format!("image lookup -t {t}"));
    }
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "lldb on {}", library.display());
    masked(&(out.stdout_text() + &out.stderr_text()), library)
}

fn debugger(major: u32) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the libraries are built for AArch64; skipping on this host");
        return;
    }
    let Some(llc) = llc(major) else { return };
    let Some(clang) = tool("clang++-19") else { return };
    let Some(extract_tool) = tool("llvm-extract") else { return };
    let Some(driver) = tool("g++") else { return };
    let Some(gdb) = tool("gdb") else { return };
    let lldb = find_tool("lldb");
    if lldb.is_none() {
        eprintln!("note: `lldb` not found; the gdb sessions only");
    }
    let scratch = Scratch::new(&format!("types-debugger{major}"));
    let source = format!("{}/tests/data/types/types.cpp", env!("CARGO_MANIFEST_DIR"));
    let ir = cxx(&clang, &scratch, "types", Path::new(&source), Some(TRIPLE));
    let ir = extract(&extract_tool, &scratch, "types", &oracle_input(&ir));
    let module = read(&ir);
    let original = compile(&llc, &scratch, "types", &ir, TRIPLE);
    let elf = Elf::parse(original.clone());
    let (obj, line, dies, _) = module_sections(&elf, Arch::AArch64, &module).expect("hod's DIEs");
    let ours = splice(&elf, &obj, &die_section_list(line, &dies));

    for (linker, flags) in linkers(&scratch) {
        let mut gdb_out = Vec::new();
        let mut lldb_out = Vec::new();
        for (name, bytes) in [("orig", &original), ("hod", &ours)] {
            // One name in a directory per variant: debuggers print it.
            let dir = scratch.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let object = dir.join(format!("types-{major}-{linker}.o"));
            std::fs::write(&object, bytes).unwrap();
            let library = dir.join(format!("libtypes-{major}-{linker}.so"));
            let mut args: Vec<OsString> = flags.clone();
            args.extend(["-shared".into(), object.into(), "-o".into()]);
            args.push(library.clone().into());
            let linked = run(&driver, &args);
            assert!(linked.success(), "link {}: {}", library.display(), linked.stderr_text());
            gdb_out.push(gdb_session(&gdb, &library));
            if let Some(lldb) = &lldb {
                lldb_out.push(lldb_session(lldb, &library));
            }
        }
        let tag = format!("llc {major}, {linker}");
        for (debugger, out) in [("gdb", &gdb_out), ("lldb", &lldb_out)] {
            if out.is_empty() {
                continue;
            }
            if std::env::var_os("HOD_DWARF_SHOW_SESSION").is_some() {
                eprintln!("{tag}: {debugger}:\n{}", out[0]);
            }
            let (orig, hod) = (&out[0], &out[1]);
            // The session did what it says.
            let wants: &[&str] = if debugger == "gdb" {
                &[
                    "unsigned int a : 3;",
                    "/* offset      |    size */  type = struct Bits {",
                    "virtual void v() const;",
                    "type = int Access::*",
                    "type = enum class Wide {Wide::Zero, Wide::Max = -1}",
                    "= 4000000000",
                    "\tns::Outer::Inner;",
                ]
            } else {
                &["struct Bits {", "class Access {", "struct Diamond : public VB, public VC {"]
            };
            for w in wants {
                assert!(orig.contains(w), "{tag}: {debugger} on the original: no {w:?}:\n{orig}");
            }
            assert_eq!(hod, orig, "{tag}: {debugger}: hod's types against llc's");
            eprintln!("{tag}: {debugger}: {} lines of session identical", orig.lines().count());
        }
    }
}

/// §8.6 with LLVM 19's objects.
#[test]
fn llc19_debugger_sees_the_same_types() {
    debugger(19);
}

/// §8.6 with LLVM 22's objects.
#[test]
fn llc22_debugger_sees_the_same_types() {
    debugger(22);
}
