//! WP-314 (design 0008's I10), part 2: i386 debug information
//! (`spec/codegen-x86.md` §13). `tests/data/i386/debug/`'s programs
//! (`vals.c`: `i64`, `float` and `double` parameters and locals, an
//! `sret` result, a struct expanded into its fields, a `byval` argument;
//! `tlsg.c`: thread-local and ordinary globals, external and static;
//! `eh.cpp`: a throw through three frames) built by clang 19 and 22 at
//! `-O0 -g` for `i686-linux-gnu`, static and PIC, then by `llc` of the
//! same major (the reference options) and by hod. Both objects verify
//! (`llvm-dwarfdump --verify` of that major) and hod's unwind tables
//! decode. hod's DIE trees equal `llc`'s in what matters (tag, name,
//! type, location, frame base, the presence of `low_pc`, `high_pc` and
//! `ranges`), under `spec/dwarf.md`'s rules only: a `DW_OP_fbreg` is
//! compared by its operation (§6.4), and a `byval` argument is described
//! through the pointer hod saved (`DW_OP_fbreg o, DW_OP_deref`, §6.17).
//! The line rows: the `is_stmt` and `prologue_end` rows (line, column)
//! equal `llc`'s, or differ exactly as the same source's x86-64 rows of
//! hod and `llc` differ (a 64-bit rule, not an i386 one); every (line,
//! column) of `llc`'s but line 0 is one of hod's (§5.7: hod has more
//! columns where `llc`'s selection folds instructions; §5.7 P2: `llc`'s
//! line-0 rows of the blocks it makes). The two refusals that remain
//! are each refused by name.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{CompileError, Options, OutputKind};
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, run_with};

mod i386_support;
use i386_support::{TRIPLE, checked, clang_module, llc_object, llvm_tool};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/debug").join(path)
}

/// hod's object of `text` for `triple`.
fn hod_object(text: &[u8], triple: &str, pic: bool) -> Result<Vec<u8>, CompileError> {
    let model = if pic { hod::RelocModel::Pic } else { hod::RelocModel::Static };
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(triple.into()),
        relocation_model: Some(model),
        ..Options::default()
    };
    hod::compile(text, &options)
}

/// `llvm-dwarfdump` with `args` on `object`: its output.
fn dump(dwarfdump: &Path, args: &[&str], object: &Path) -> String {
    let mut command = Command::new(dwarfdump);
    command.args(args).arg(object);
    let outcome = checked(&format!("llvm-dwarfdump {args:?} {}", object.display()), &mut command);
    String::from_utf8_lossy(&outcome.stdout).into_owned()
}

/// `--verify` of `object` must pass.
fn verifies(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    let text = String::from_utf8_lossy(&outcome.stdout);
    assert!(outcome.success() && text.contains("No errors."), "{}: {text}", object.display());
}

/// The value of an attribute: a reference's quoted name, a location with
/// each `DW_OP_fbreg`'s offset dropped.
fn value(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(start) = raw.find('"') {
        return raw[start..].to_owned(); // `0x00000032 "int"`
    }
    let ops: Vec<&str> = raw
        .split(", ")
        .map(|op| if op.starts_with("DW_OP_fbreg ") { "DW_OP_fbreg" } else { op })
        .collect();
    ops.join(", ")
}

/// The DIEs of `--debug-info` text in order (not the unit header, whose
/// length is the code's): depth, tag, and the attributes compared (only
/// the presence of the code addresses).
fn dies(text: &str) -> Vec<String> {
    const VALUES: [&str; 4] = ["DW_AT_name", "DW_AT_type", "DW_AT_location", "DW_AT_frame_base"];
    const PRESENT: [&str; 3] = ["DW_AT_low_pc", "DW_AT_high_pc", "DW_AT_ranges"];
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some((offset, rest)) = line.split_once(": ")
            && offset.starts_with("0x")
            && !rest.contains("Compile Unit:")
        {
            let tag = rest.trim_start();
            out.push(format!("{} {tag}", rest.len() - tag.len()));
            continue;
        }
        let (Some(last), Some((attr, raw))) =
            (out.last_mut(), line.trim_start().split_once(char::is_whitespace))
        else {
            continue;
        };
        if VALUES.contains(&attr) {
            last.push_str(&format!(" {attr}={}", value(raw)));
        } else if PRESENT.contains(&attr) {
            last.push_str(&format!(" {attr}"));
        }
    }
    out
}

/// §6.17: hod describes a `byval` argument through the pointer it saved.
fn byval_rule(hod: &str, llc: &str) -> bool {
    hod.contains("DW_TAG_formal_parameter")
        && hod.replace("DW_OP_fbreg, DW_OP_deref ", "DW_OP_fbreg ") == llc
}

/// The rows of `--debug-line` text: (line, column, flags).
fn rows(text: &str) -> Vec<(String, String, String)> {
    let row = |l: &str| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f[1].to_owned(), f[2].to_owned(), f[7..].join(" "))
    };
    text.lines().filter(|l| l.starts_with("0x0")).map(row).collect()
}

/// The (line, column) of the rows flagged `flag`.
fn flagged(rows: &[(String, String, String)], flag: &str) -> Vec<(String, String)> {
    let has = |flags: &str| flags.split(' ').any(|f| f == flag);
    rows.iter().filter(|r| has(&r.2)).map(|r| (r.0.clone(), r.1.clone())).collect()
}

/// The `is_stmt` and `prologue_end` rows.
fn stmts(rows: &[(String, String, String)]) -> [Vec<(String, String)>; 2] {
    [flagged(rows, "is_stmt"), flagged(rows, "prologue_end")]
}

/// The x86-64 objects of `source` by clang, `llc` and hod: their
/// statement rows, hod's and `llc`'s.
fn x86_64_stmts(
    scratch: &Scratch,
    tools: [&Path; 3],
    source: &Path,
    label: &str,
) -> [[Vec<(String, String)>; 2]; 2] {
    let [clang, llc, dwarfdump] = tools;
    let ll = scratch.join(format!("{label}-x64.ll"));
    let mut front = Command::new(clang);
    front.args(["--target=x86_64-linux-gnu", "-O0", "-g", "-S", "-emit-llvm", "-o"]);
    checked(&format!("clang {label} x86-64"), front.arg(&ll).arg(source));
    let llc_o = scratch.join(format!("{label}-x64-llc.o"));
    let mut back = Command::new(llc);
    back.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"]);
    checked(&format!("llc {label} x86-64"), back.arg(&llc_o).arg(&ll));
    let text = std::fs::read(&ll).unwrap();
    let hod = hod_object(&text, "x86_64-unknown-linux-gnu", false).unwrap();
    let hod_o = scratch.write(&format!("{label}-x64-hod.o"), &hod);
    [&hod_o, &llc_o].map(|o| stmts(&rows(&dump(dwarfdump, &["--debug-line"], o))))
}

/// Every program, both majors, static and PIC: verified, the DIE trees
/// and line rows equal `llc`'s under the rules of the module comment.
#[test]
fn trees_and_rows_equal_llc() {
    let mut objects = 0;
    let mut inherited = Vec::new();
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc), Some(dwarfdump)) = (
            llvm_tool(major, "clang"),
            llvm_tool(major, "llc"),
            llvm_tool(major, "llvm-dwarfdump"),
        ) else {
            continue;
        };
        let scratch = Scratch::new(&format!("i386-debug-{major}"));
        for source in ["vals.c", "tlsg.c", "eh.cpp"] {
            for pic in [false, true] {
                let model = if pic { "pic" } else { "static" };
                let label = format!("{}-{major}-{model}", source.replace('.', "_"));
                let flags: &[&str] = if pic { &["-g", "-fPIC"] } else { &["-g", "-fno-pic"] };
                let ll = clang_module(&scratch, &clang, &data(source), flags, &label);
                let llc_flags: &[&str] =
                    if pic { &["-relocation-model=pic"] } else { &["-relocation-model=static"] };
                let llc_o = llc_object(&scratch, &llc, &ll, llc_flags, &format!("{label}-llc"));
                let text = std::fs::read(&ll).unwrap();
                let hod =
                    hod_object(&text, TRIPLE, pic).unwrap_or_else(|e| panic!("hod {label}: {e}"));
                let hod_o = scratch.write(&format!("{label}-hod.o"), &hod);
                verifies(&dwarfdump, &llc_o);
                verifies(&dwarfdump, &hod_o);
                let frames = dump(&dwarfdump, &["--eh-frame", "--debug-frame"], &hod_o);
                assert!(frames.contains("CIE") && !frames.contains("error"), "{label}: {frames}");

                let h = dies(&dump(&dwarfdump, &["--debug-info"], &hod_o));
                let l = dies(&dump(&dwarfdump, &["--debug-info"], &llc_o));
                assert!(h.len() > 10 && h.len() == l.len(), "{label}: {h:#?}\n{l:#?}");
                for (h, l) in h.iter().zip(&l) {
                    assert!(h == l || byval_rule(h, l), "{label}: hod `{h}`, llc `{l}`");
                }

                let h = rows(&dump(&dwarfdump, &["--debug-line"], &hod_o));
                let l = rows(&dump(&dwarfdump, &["--debug-line"], &llc_o));
                let pairs = |r: &[(String, String, String)]| {
                    r.iter().map(|r| (r.0.clone(), r.1.clone())).collect::<Vec<_>>()
                };
                let hod_pairs = pairs(&h);
                // §5.7 P2: `llc`'s line-0 rows of the blocks it makes.
                for pair in pairs(&l).into_iter().filter(|p| p.0 != "0") {
                    assert!(hod_pairs.contains(&pair), "{label}: llc's row {pair:?} not in hod's");
                }
                let (h, l) = (stmts(&h), stmts(&l));
                if h != l {
                    let tools = [clang.as_path(), llc.as_path(), dwarfdump.as_path()];
                    let [h64, l64] = x86_64_stmts(&scratch, tools, &data(source), &label);
                    assert!(
                        h == h64 && l == l64,
                        "{label}: statement rows differ, not as on x86-64:\nhod {h:?}\nllc {l:?}"
                    );
                    inherited.push(label);
                }
                objects += 1;
            }
        }
    }
    eprintln!(
        "i386 debug: {objects} objects verified, trees and rows equal llc's; rows as on \
         x86-64: {inherited:?}"
    );
}

/// A DWARF 4 unit and split DWARF are refused by their names.
#[test]
fn dwarf_4_and_split_dwarf_are_refused_by_name() {
    let Some(clang) = llvm_tool(22, "clang") else { return };
    let scratch = Scratch::new("i386-debug-refusals");
    let ll = clang_module(&scratch, &clang, &data("vals.c"), &["-gdwarf-4"], "dwarf4");
    let err = hod_object(&std::fs::read(&ll).unwrap(), TRIPLE, false).expect_err("refused");
    assert!(err.to_string().contains("debug:dwarf-4-i386"), "{err}");

    let ll = clang_module(&scratch, &clang, &data("vals.c"), &["-g", "-gsplit-dwarf"], "split");
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        split_dwarf_file: Some(PathBuf::from("vals.dwo")), // clang's name of it
        split_dwarf_output: Some(scratch.join("split.dwo")),
        ..Options::default()
    };
    let err = hod::compile(&std::fs::read(&ll).unwrap(), &options).expect_err("refused");
    assert!(err.to_string().contains("debug:split-dwarf-i386"), "{err}");
}
