//! DWARF version 4 (WP-114, `spec/dwarf.md` §6.20) against `llc` 19 and
//! 22 with the real tools, on both targets: the `-gdwarf-4` IR of clang
//! 19, clang 22 and kiln++.
//!
//! - the DIE trees and the relocations of `.debug_info`
//!   (`cargo xtask debug trees`): each module's outcome with
//!   `-gdwarf-4` is its outcome with `-gdwarf-5` (DWARF 4 adds no
//!   difference), and on x86-64, where the trees of these sources are
//!   `llc`'s, every one is the same or permitted;
//! - what the trees do not show, compared with `llc`'s object: the forms
//!   of every attribute (the widths of constants folded, §6.7), the set
//!   of `.debug_*` sections, the line table's header and the sections
//!   `.debug_ranges` relocates against;
//! - the language's lower bound (`-1` from `DW_LANG_OpenCL` on);
//! - gdb and lldb on hod's programs print what they print on `llc`'s,
//!   under every linker found (`tests/debug/dwarf4_*`).
//!
//! Tools-gated like every oracle test: without the tools the test skips
//! with a note; with `HOD_REQUIRE_TOOLS=1` a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::Scratch;
use xtask::debug::sessions::{self, Options, Outcome};
use xtask::debug::trees;
use xtask::irdiff;
use xtask::irdiff::directives::Arch;

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

fn debug_test(name: &str) -> PathBuf {
    irdiff::workspace_root().join("tests").join("debug").join(name)
}

fn skip(why: &str) {
    assert!(!hod_testutil::tools_required(), "required tools are missing: {why}");
    eprintln!("note: skipping this tool-backed test: {why}");
}

fn hod() -> PathBuf {
    irdiff::build_workspace_hod(&build_dir()).expect("hod builds")
}

/// The sources compared, of `tests/debug/`; kiln++ is given the C++ ones.
const SOURCES: [&str; 8] = [
    "classes.cpp",
    "comdat.cpp",
    "inlined.cpp",
    "dwarf4_shapes.cpp",
    "labels.c",
    "globals.c",
    "tls.c",
    "x64_places.c",
];

const TARGETS: [(Arch, &str); 2] =
    [(Arch::Aarch64, "aarch64-unknown-linux-gnu"), (Arch::X86_64, "x86_64-unknown-linux-gnu")];

/// A front end: its name, its C driver and its C++ driver (kiln++ for
/// both).
type Frontend = (&'static str, PathBuf, PathBuf);

/// The front ends: clang 19, clang 22, kiln++ (which writes LLVM 22).
fn frontends() -> Vec<Frontend> {
    let mut out = Vec::new();
    for (name, c, cxx) in
        [("clang-19", "clang-19", "clang++-19"), ("clang-22", "clang-22", "clang++-22")]
    {
        if let (Some(c), Some(cxx)) = (hod_testutil::tool(c), hod_testutil::tool(cxx)) {
            out.push((name, c, cxx));
        }
    }
    let kiln = xtask::corpus::capture_defaults(&build_dir()).kiln;
    if kiln.is_file() {
        out.push(("kiln", kiln.clone(), kiln));
    } else {
        eprintln!("note: no kiln++ at {}; its part is skipped", kiln.display());
    }
    out
}

/// The `-g -O0 -gdwarf-<version>` IR of `source` by `frontend` for
/// `triple`, written to `out`.
fn ir(frontend: &Frontend, triple: &str, source: &str, version: u32, out: &Path) {
    let cxx = Path::new(source).extension().is_some_and(|e| e == "cpp");
    let compiler = if cxx { &frontend.2 } else { &frontend.1 };
    let mut command = Command::new(compiler);
    if frontend.0 == "kiln" {
        // kiln writes LLVM 22's IR; it is pointed at that installation.
        let llvm = Path::new("/usr/lib/llvm-22");
        if llvm.is_dir() {
            command.arg(format!("--llvm-dir={}", llvm.display()));
        }
        command.arg("-fno-crash-diagnostics");
    }
    command.arg(format!("--target={triple}"));
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w", &format!("-gdwarf-{version}")]);
    if cxx {
        command.arg("-std=c++20");
    }
    command.arg(source).arg("-o").arg(out).current_dir(debug_test(""));
    let done = hod_testutil::run_with(&mut command, b"", hod_testutil::DEFAULT_TIMEOUT);
    assert!(done.success(), "{command:?}: {}", done.stderr_text());
}

/// Every IR of the matrix for `version` into `dir` (one file per front
/// end, target and source); `None` without any front end.
fn matrix(dir: &Path, version: u32) -> Option<Vec<PathBuf>> {
    let frontends = frontends();
    if frontends.is_empty() {
        return None;
    }
    std::fs::create_dir_all(dir).unwrap();
    let mut paths = Vec::new();
    for fe in &frontends {
        for (arch, triple) in TARGETS {
            for source in SOURCES {
                if fe.0 == "kiln" && Path::new(source).extension().is_none_or(|e| e != "cpp") {
                    continue;
                }
                let stem = Path::new(source).file_stem().unwrap().to_string_lossy().into_owned();
                let out = dir.join(format!("{}-{}-{stem}.ll", fe.0, arch.name()));
                ir(fe, triple, source, version, &out);
                paths.push(out);
            }
        }
    }
    Some(paths)
}

fn tree_outcomes(
    paths: Vec<PathBuf>,
    hod: &Path,
) -> Result<Vec<(PathBuf, trees::Outcome)>, String> {
    let options = trees::Options {
        paths,
        jobs: 4,
        hod: Some(hod.to_path_buf()),
        build_dir: build_dir(),
        llc: None,
        verbose: true,
    };
    trees::run(&options)
}

/// A module's outcome with `-gdwarf-4` is its outcome with `-gdwarf-5`;
/// none is declined or failed (both objects verify); on x86-64 every
/// one is the same as `llc`'s or differs by a permitted class only.
#[test]
fn dwarf4_trees_are_dwarf5s() {
    let scratch = Scratch::new("debug-dwarf4-trees");
    let (Some(v4), Some(v5)) = (matrix(&scratch.join("v4"), 4), matrix(&scratch.join("v5"), 5))
    else {
        return skip("no front end");
    };
    let hod = hod();
    let (four, five) = match (tree_outcomes(v4, &hod), tree_outcomes(v5, &hod)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return skip(&e),
    };
    let summary = trees::summary(&four, true);
    assert_eq!(four.len(), five.len(), "{summary}");
    for ((module, a), (_, b)) in four.iter().zip(&five) {
        let name = module.file_name().unwrap_or_default().to_string_lossy();
        assert!(
            !matches!(a, trees::Outcome::Declined(_) | trees::Outcome::Failed(_)),
            "{name}: {a:?}"
        );
        if name.contains("-x86_64-") {
            assert!(
                matches!(a, trees::Outcome::Same(_) | trees::Outcome::Permitted(_)),
                "{name}: {a:?}"
            );
        }
        // Where version 5 differs (AArch64's `sp`-relative locations,
        // §6.7), the first difference is the same DIE or, for `llc`'s
        // `sret` list, the unit's `loclists_base` version 4 does not have.
        match (a, b) {
            (trees::Outcome::Differs(x), trees::Outcome::Differs(y)) => {
                let path = |d: &str| d.split(':').next().unwrap_or_default().to_owned();
                assert!(
                    path(x) == path(y) || path(y) == "DW_TAG_compile_unit",
                    "{name}: version 4 differs at {x}, version 5 at {y}"
                );
            }
            _ => assert_eq!(a, b, "{name}: version 4, then version 5"),
        }
    }
}

fn tool_output(tool: &Path, args: &[&str], object: &Path) -> String {
    let mut command = Command::new(tool);
    command.args(args).arg(object);
    let done = hod_testutil::run_with(&mut command, b"", hod_testutil::DEFAULT_TIMEOUT);
    assert!(done.success(), "{command:?}: {}", done.stderr_text());
    done.stdout_text()
}

/// The tag, attribute and form sequence of `.debug_info`, values
/// dropped, the constant forms folded (§6.7: hod's `udata` where `llc`
/// picks a width), and the unit headers' versions.
fn forms(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = tool_output(dwarfdump, &["--debug-info", "--show-form"], object);
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(at) = t.find("version = ") {
            out.push(t[at..].split_whitespace().take(3).collect::<Vec<_>>().join(" "));
        } else if let Some(tag) = t.split_whitespace().nth(1).filter(|w| w.starts_with("DW_TAG_")) {
            out.push(tag.to_owned());
        } else if t.starts_with("DW_AT_")
            && let (Some(name), Some(open), Some(close)) =
                (t.split_whitespace().next(), t.find('['), t.find(']'))
        {
            let form = &t[open + 1..close];
            let form = match form {
                "DW_FORM_data1"
                | "DW_FORM_data2"
                | "DW_FORM_data4"
                | "DW_FORM_data8"
                | "DW_FORM_udata"
                | "DW_FORM_sdata"
                | "DW_FORM_implicit_const" => "const",
                "DW_FORM_flag" | "DW_FORM_flag_present" => "flag",
                f => f,
            };
            out.push(format!("{name} {form}"));
        }
    }
    out
}

fn sections(readelf: &Path, object: &Path) -> BTreeSet<String> {
    tool_output(readelf, &["-S", "-W"], object)
        .split_whitespace()
        .filter(|w| w.starts_with(".debug_") || w.starts_with(".rela.debug_"))
        .map(str::to_owned)
        .collect()
}

/// The line table's header up to its directories, the lengths dropped.
fn line_header(dwarfdump: &Path, object: &Path) -> Vec<String> {
    tool_output(dwarfdump, &["--debug-line"], object)
        .lines()
        .skip_while(|l| !l.contains("Line table prologue"))
        .take_while(|l| !l.contains("include_directories") && !l.contains("file_names"))
        .filter(|l| !l.contains("_length:") || l.contains("min_inst_length"))
        .map(|l| l.trim().to_owned())
        .collect()
}

/// The relocations of `.debug_ranges`: (offset, symbol, addend).
fn range_relocations(readelf: &Path, object: &Path) -> Vec<(u64, String, u64)> {
    let text = tool_output(readelf, &["-r", "-W"], object);
    let hex = |w: &str| u64::from_str_radix(w, 16).ok();
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("Relocation section") {
            inside = line.contains("'.rela.debug_ranges'");
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if inside
            && words.len() == 7
            && words[5] == "+"
            && let (Some(offset), Some(addend)) = (hex(words[0]), hex(words[6]))
        {
            out.push((offset, words[4].to_owned(), addend));
        }
    }
    out
}

/// The sections `.debug_ranges`' relocations name.
fn range_targets(readelf: &Path, object: &Path) -> BTreeSet<String> {
    range_relocations(readelf, object).into_iter().map(|(_, symbol, _)| symbol).collect()
}

/// Every function of `object` is inside a relocated range of its section
/// in `.debug_ranges` (an entry relocated at `o` is a start, the one at
/// `o + 8` its end), when the section has any.
fn functions_in_ranges(readelf: &Path, object: &Path, name: &str) {
    let relocations = range_relocations(readelf, object);
    let mut ranges: Vec<(String, u64, u64)> = Vec::new();
    for (offset, symbol, start) in &relocations {
        if let Some((_, _, end)) = relocations.iter().find(|r| r.0 == offset + 8 && &r.1 == symbol)
            && offset % 16 == 0
        {
            ranges.push((symbol.clone(), *start, *end));
        }
    }
    let mut sections = std::collections::BTreeMap::new();
    for line in tool_output(readelf, &["-S", "-W"], object).lines() {
        if let Some(rest) = line.trim_start().strip_prefix('[')
            && let Some((index, tail)) = rest.split_once(']')
            && let (Ok(index), Some(section)) =
                (index.trim().parse::<u32>(), tail.split_whitespace().next())
        {
            sections.insert(index.to_string(), section.to_owned());
        }
    }
    for line in tool_output(readelf, &["-s", "-W"], object).lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 8 || words[3] != "FUNC" {
            continue;
        }
        let (Ok(value), Ok(size)) = (u64::from_str_radix(words[1], 16), words[2].parse::<u64>())
        else {
            continue;
        };
        let Some(section) = sections.get(words[6]) else { continue };
        if size == 0 || !ranges.iter().any(|r| &r.0 == section) {
            continue;
        }
        assert!(
            ranges.iter().any(|r| &r.0 == section && r.1 <= value && value + size <= r.2),
            "{name}: {} ({section} {value:#x}+{size}) is in no range of {ranges:?}",
            words[7]
        );
    }
}

/// What the trees do not show is `llc`'s: forms, sections, the line
/// table's header (version 4, `include_directories` and `file_names`
/// inline), and `.debug_ranges`' targets; every function is in a range
/// of its unit's list. `x64_places.c` is left out:
/// `llc`'s location lists there are §6.7's `sret` class.
#[test]
fn dwarf4_forms_sections_and_line_headers_are_llcs() {
    let scratch = Scratch::new("debug-dwarf4-forms");
    let Some(paths) = matrix(&scratch.join("v4"), 4) else { return skip("no front end") };
    let tools =
        ["llvm-dwarfdump-22", "llvm-readelf-22", "llc-19", "llc-22"].map(hod_testutil::tool);
    let [Some(dwarfdump), Some(readelf), Some(llc19), Some(llc22)] = tools else {
        return skip("no llvm-dwarfdump, llvm-readelf or llc 19 and 22");
    };
    let hod = hod();
    let mut compared = 0;
    for path in paths.iter().filter(|p| !p.to_string_lossy().ends_with("x64_places.ll")) {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let llc = if name.starts_with("clang-19") { &llc19 } else { &llc22 };
        let objects = [("llc", llc.as_path()), ("hod", hod.as_path())].map(|(who, tool)| {
            let out = scratch.join(format!("{name}.{who}.o"));
            let mut command = Command::new(tool);
            if who == "llc" {
                command.args(["-global-isel=0", "-fast-isel=0"]);
            } else {
                command.arg("--strict");
            }
            command.args(["-O0", "-filetype=obj", "-relocation-model=pic"]);
            command.arg(path).arg("-o").arg(&out);
            let done = hod_testutil::run_with(&mut command, b"", hod_testutil::DEFAULT_TIMEOUT);
            assert!(done.success(), "{command:?}: {}", done.stderr_text());
            out
        });
        let [l, h] = &objects;
        assert_eq!(forms(&dwarfdump, h), forms(&dwarfdump, l), "{name}: hod's forms, then llc's");
        assert_eq!(sections(&readelf, h), sections(&readelf, l), "{name}: hod's sections, llc's");
        let header = line_header(&dwarfdump, h);
        assert!(header.iter().any(|l| l == "version: 4"), "{name}: {header:?}");
        assert_eq!(header, line_header(&dwarfdump, l), "{name}: hod's line header, then llc's");
        assert_eq!(range_targets(&readelf, h), range_targets(&readelf, l), "{name}: .debug_ranges");
        functions_in_ranges(&readelf, l, &format!("{name} (llc)"));
        functions_in_ranges(&readelf, h, &name);
        compared += 1;
    }
    assert!(compared > 0, "nothing compared");
}

/// A subrange with a `lowerBound` in a C++ unit: version 4's default is
/// -1 (`DW_LANG_C_plus_plus_14` is above `DW_LANG_OpenCL`), so `llc`
/// writes the bound 0 too; version 5's is 0, so it writes only the 1.
#[test]
fn dwarf4_lower_bounds_are_llcs() {
    let scratch = Scratch::new("debug-dwarf4-bounds");
    let Some(fe) = frontends().into_iter().find(|f| f.0 == "clang-22") else {
        return skip("no clang 22");
    };
    let mut paths = Vec::new();
    for version in [4, 5] {
        let raw = scratch.join(format!("raw{version}.ll"));
        ir(&fe, TARGETS[1].1, "dwarf4_shapes.cpp", version, &raw);
        let text = std::fs::read_to_string(&raw).unwrap();
        let text = text
            .replace("DISubrange(count: 4)", "DISubrange(count: 4, lowerBound: 0)")
            .replace("DISubrange(count: 3)", "DISubrange(count: 3, lowerBound: 1)");
        assert!(text.contains("lowerBound: 0") && text.contains("lowerBound: 1"), "the subranges");
        let path = scratch.join(format!("bounds{version}.ll"));
        std::fs::write(&path, text).unwrap();
        paths.push(path);
    }
    let outcomes = match tree_outcomes(paths, &hod()) {
        Ok(outcomes) => outcomes,
        Err(e) => return skip(&e),
    };
    let summary = trees::summary(&outcomes, true);
    assert_eq!(outcomes.len(), 2, "{summary}");
    for (_, outcome) in &outcomes {
        assert!(matches!(outcome, trees::Outcome::Same(_)), "{summary}");
    }
}

fn sessions_pass(target: Option<Arch>, frontends: &[&str], tests: &[&str], label: &str) {
    let scratch = Scratch::new(label);
    let mut options = Options::new(build_dir());
    options.hod = Some(hod());
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options.target = target;
    options.frontends = frontends.iter().map(|f| (*f).to_owned()).collect();
    options.paths = tests.iter().map(|t| debug_test(t)).collect();
    let report = match sessions::run(&options) {
        Ok(report) => report,
        Err(e) if e.contains("--target") => return skip(&e),
        Err(e) => panic!("the run cannot be made: {e}"),
    };
    let summary = report.summary(false);
    if report.records.iter().all(|r| r.outcome == Outcome::Skip) {
        return skip(&summary);
    }
    for bad in [Outcome::Fail, Outcome::Xpass, Outcome::Crash, Outcome::Error] {
        assert_eq!(report.count(bad), 0, "{summary}");
    }
    assert!(report.count(Outcome::Pass) >= tests.len(), "{summary}");
}

/// The version 4 twins on this machine: static members, deleted
/// members, arrays, a unit of several sections (`.debug_ranges`), and a
/// program of one hod object and one `llc` object (`comdat`), by every
/// front end, linker and debugger found.
#[test]
fn dwarf4_sessions_match_llc() {
    sessions_pass(
        None,
        &["clang-19", "clang-22", "kiln"],
        &["dwarf4_shapes.cpp", "dwarf4_comdat.cpp", "dwarf4_labels.c"],
        "debug-dwarf4-sessions",
    );
}

/// The same on x86-64, through qemu's gdb stub.
#[test]
fn dwarf4_x86_64_sessions_match_llc() {
    if Arch::host() == Some(Arch::X86_64) {
        // Not a missing tool: the native sessions are the other tests'.
        eprintln!("note: this test is of the remote sessions; the host is x86-64");
        return;
    }
    sessions_pass(
        Some(Arch::X86_64),
        &["clang-22"],
        &["dwarf4_shapes.cpp", "dwarf4_x64_places.c"],
        "debug-dwarf4-x64",
    );
}
