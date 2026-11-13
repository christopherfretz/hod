//! Coroutine debug information (WP-108, `spec/dwarf.md` §6.16): the
//! program of `tests/debug/coroutine.cpp`, its `-g` IR before LLVM's
//! passes by clang++ 19, clang++ 22 and kiln++, compiled by hod.
//!
//! - `llvm-dwarfdump --verify` is clean;
//! - the subprogram ends where the `F.resume` stub starts (R1), and the
//!   prologue stores 0 to the frame word every heap location starts from
//!   (R3) before the first call;
//! - gdb, on the linked program: the parameter before the first suspend,
//!   the parameter, a loop variable, a local and the promise after a
//!   resume (`bt` and `finish` from a callee), and the parameter and the
//!   promise in the destroy path, all with the values the program has.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{objdump, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const SOURCE: &str = include_str!("../../../tests/debug/coroutine.cpp");

const PIC: Options = Options {
    reloc_model: RelocModel::Pic,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

/// The `-g` IR before LLVM's passes of [`SOURCE`] by `compiler`.
fn pre_opt(dir: &Scratch, compiler: &Path, name: &str) -> PathBuf {
    let cpp = dir.write("coroutine.cpp", SOURCE);
    let ir = dir.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command
        .current_dir(dir.path())
        .args(["-std=c++20", "-g", "-O0", "-S", "-emit-llvm", "-w"])
        .args(["-Xclang", "-disable-llvm-passes"])
        .arg(cpp.file_name().unwrap_or_default())
        .arg("-o")
        .arg(&ir);
    let ran = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{name}: {}", ran.stderr_text());
    ir
}

/// The front ends found: clang++ 19 and 22, kiln++.
fn front_ends() -> Vec<(&'static str, PathBuf)> {
    let mut found = Vec::new();
    for (name, major) in [("clang-19", 19), ("clang-22", 22)] {
        let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/clang++"));
        match installed.is_file().then_some(installed).or_else(|| tool(&format!("clang++-{major}")))
        {
            Some(path) => found.push((name, path)),
            None => assert!(!tools_required(), "no clang++-{major} (HOD_REQUIRE_TOOLS=1)"),
        }
    }
    if let Some(kiln) = support::kiln() {
        found.push(("kiln", kiln));
    }
    found
}

/// `llvm-dwarfdump` `args` of `object`.
fn dwarfdump(dwarfdump: &Path, args: &[&str], object: &Path) -> String {
    let mut command = Command::new(dwarfdump);
    command.args(args).arg(object);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "llvm-dwarfdump: {}", ran.stderr_text());
    ran.stdout_text()
}

/// The value of the `(0x…)` after `attr` in `text`.
fn hex_after(text: &str, attr: &str) -> Option<u64> {
    let at = text.find(attr)? + attr.len();
    let open = text[at..].find("(0x")? + at + 3;
    let len = text[open..].find(|c: char| !c.is_ascii_hexdigit())?;
    u64::from_str_radix(&text[open..open + len], 16).ok()
}

/// The structure of hod's object: R1 and the frame word of R3.
fn check_structure(name: &str, object: &Path, tool_path: &Path) {
    let info = dwarfdump(tool_path, &["--debug-info", "--name=count", "-c"], object);
    let low = hex_after(&info, "DW_AT_low_pc").unwrap_or_else(|| panic!("{name}: {info}"));
    let high = hex_after(&info, "DW_AT_high_pc").unwrap_or_else(|| panic!("{name}: {info}"));
    let Some(nm) = tool("llvm-nm") else { return };
    let mut command = Command::new(nm);
    command.arg(object);
    let symbols = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
    let address = |symbol: &str| {
        symbols
            .lines()
            .find(|l| l.ends_with(symbol))
            .and_then(|l| u64::from_str_radix(l.split_whitespace().next().unwrap_or(""), 16).ok())
    };
    assert_eq!(address(" _Z5counti"), Some(low), "{name}: the subprogram starts at count");
    assert_eq!(address(" _Z5counti.resume"), Some(high), "{name}: R1, it ends at the stubs");
    // `local` is in the heap frame: `DW_OP_fbreg W, DW_OP_deref, ...`.
    let at = info.find("(\"local\")").unwrap_or_else(|| panic!("{name}: no local: {info}"));
    let before = &info[..at];
    let loc = &before[before.rfind("DW_AT_location").unwrap_or_else(|| panic!("{name}"))..];
    let word: i64 = loc
        .split("DW_OP_fbreg ")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|w| w.trim().trim_start_matches('+').parse().ok())
        .unwrap_or_else(|| panic!("{name}: local's location: {loc}"));
    assert!(loc.contains(", DW_OP_deref, DW_OP_plus_uconst"), "{name}: {loc}");
    let Some(lines) = objdump(object) else { return };
    let start = lines.iter().position(|l| l == "<_Z5counti>:" || l.contains("<_Z5counti>"));
    let body = &lines[start.unwrap_or(0)..];
    let call = body.iter().position(|l| l.starts_with("bl ")).unwrap_or(body.len());
    let zero = format!("str xzr, [x29, #0x{word:x}]");
    assert!(
        body[..call].contains(&zero),
        "{name}: the frame word ({zero}) is not 0 before the first call: {:#?}",
        &body[..call]
    );
}

/// gdb's session on `program`, the script of the L7 test.
fn gdb_session(gdb: &Path, program: &Path) -> String {
    let mut command = Command::new(gdb);
    command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
    command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
    for line in SOURCE.lines() {
        if let Some(c) = line.strip_prefix("//@ gdb: ") {
            command.arg("-ex").arg(c);
        }
    }
    command.arg(program).current_dir(program.parent().unwrap_or(Path::new(".")));
    let ran = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!ran.timed_out(), "gdb on {}", program.display());
    ran.stdout_text()
}

/// What hod's program prints under the script, in order.
const GDB_WANTS: [&str; 14] = [
    "count (n=4) at coroutine.cpp:41",
    "$1 = 4",
    "#1  0x",
    "in count (n=4) at coroutine.cpp:45",
    "in main () at coroutine.cpp:54",
    "Value returned is $2 = 0",
    "$3 = 4",
    "$4 = 0",
    "$5 = 7",
    "$6 = {cur = 4}",
    "Value returned is $7 = 2",
    "in count (n=4) at coroutine.cpp:48",
    "$10 = 4",
    "$11 = {cur = 9}",
];

#[test]
fn coroutine_variables_hold_in_every_activation() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let Some(llvm_dwarfdump) = tool("llvm-dwarfdump") else { return };
    let dir = Scratch::new("coro-debug");
    let toolchain = Toolchain::host(&dir);
    let gdb = tool("gdb");
    let mut built = 0;
    for (name, compiler) in front_ends() {
        let ir = pre_opt(&dir, &compiler, name);
        let text = std::fs::read_to_string(&ir).unwrap();
        let (module, bodies) = parse(&text);
        let emitted =
            compile_module(&module, &bodies, &PIC).unwrap_or_else(|e| panic!("{name}: {e}"));
        let object = write_object(&dir, &format!("{name}.o"), &emitted);
        support::verify(&llvm_dwarfdump, &object);
        check_structure(name, &object, &llvm_dwarfdump);
        built += 1;
        let (Some(toolchain), Some(gdb)) = (&toolchain, &gdb) else { continue };
        let Some(linker) = toolchain.linkers.first() else { continue };
        let program = dir.join(format!("{name}-prog"));
        toolchain.link_ok(linker, &[], &[object.as_path(), Path::new("-lstdc++")], &program);
        let printed = gdb_session(gdb, &program);
        let mut rest = printed.as_str();
        for want in GDB_WANTS {
            let at =
                rest.find(want).unwrap_or_else(|| panic!("{name}: no '{want}' in:\n{printed}"));
            rest = &rest[at + want.len()..];
        }
        assert!(printed.contains("exited normally"), "{name}: {printed}");
    }
    assert!(built > 0 || !tools_required(), "no front end");
}
