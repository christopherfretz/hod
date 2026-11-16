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
//! The line rows: hod's `epilogue_begin` rows follow the epilogue rule
//! (`spec/codegen.md` §4, `spec/dwarf.md` §5.4 and §5.2: one per
//! function, at its `ret`'s location, after the last block, `is_stmt` when
//! that block ends on another line, as after a landing pad); the other
//! `is_stmt` and `prologue_end` rows (line, column) equal `llc`'s; every (line,
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

/// The (line, column) of each defined function's one `ret`, in the order
/// of the module (the order hod places them): the `DILocation` the `ret`
/// carries. clang at `-O0` writes one `ret` per function.
fn ret_locations(ll: &str) -> Vec<(String, String)> {
    let field = |text: &str, key: &str| {
        text.split(key).nth(1).map_or("0".to_owned(), |rest| {
            rest.chars().take_while(char::is_ascii_digit).collect::<String>()
        })
    };
    let location = |id: &str| {
        let head = format!("{id} = !DILocation(");
        let text = ll.lines().find(|l| l.starts_with(&head)).expect("the ret's DILocation");
        (field(text, "line: "), field(text, "column: "))
    };
    let mut out = Vec::new();
    let mut rets: Option<Vec<String>> = None;
    for line in ll.lines() {
        if line.starts_with("define ") && line.contains("!dbg !") {
            rets = Some(Vec::new());
        } else if line == "}" {
            if let Some(rets) = rets.take() {
                assert_eq!(rets.len(), 1, "one `ret` per function: {rets:?}");
                out.push(location(&rets[0]));
            }
        } else if let Some(rets) = rets.as_mut()
            && line.trim_start().starts_with("ret ")
        {
            let id = line.rsplit("!dbg ").next().expect("a located `ret`").trim();
            rets.push(id.to_owned());
        }
    }
    out
}

/// The epilogue rule (`spec/codegen.md` §4: the one epilogue is emitted
/// at the function's end and every `ret` but the last block's branches to
/// it; `spec/dwarf.md` §5.4: with one `ret`, the epilogue row has its
/// location; §5.2: `epilogue_begin` on it, `is_stmt` when its line differs
/// from the last non-zero line written). hod's `epilogue_begin` rows, in
/// order, are at `rets` and each is `is_stmt` exactly by §5.2.
fn epilogue_rule(label: &str, rows: &[(String, String, String)], rets: &[(String, String)]) {
    let has = |flags: &str, flag: &str| flags.split(' ').any(|f| f == flag);
    let epilogues: Vec<usize> =
        (0..rows.len()).filter(|&i| has(&rows[i].2, "epilogue_begin")).collect();
    let at: Vec<(String, String)> =
        epilogues.iter().map(|&i| (rows[i].0.clone(), rows[i].1.clone())).collect();
    assert_eq!(at, rets, "{label}: hod's epilogue rows are not at the `ret`s' locations");
    for &i in &epilogues {
        let last = rows[..i].iter().rev().map(|r| &r.0).find(|l| *l != "0");
        let stmt = last != Some(&rows[i].0);
        assert_eq!(has(&rows[i].2, "is_stmt"), stmt, "{label}: epilogue row {:?}", rows[i]);
    }
}

/// The statement rows but those of an epilogue (whose rows follow the
/// epilogue rule in hod's and each `llc` target's own way).
fn stmts_but_epilogue(rows: &[(String, String, String)]) -> [Vec<(String, String)>; 2] {
    let kept: Vec<_> = rows.iter().filter(|r| !r.2.contains("epilogue_begin")).cloned().collect();
    stmts(&kept)
}

/// Every program, both majors, static and PIC: verified, the DIE trees
/// and line rows equal `llc`'s under the rules of the module comment.
#[test]
fn trees_and_rows_equal_llc() {
    let mut objects = 0;
    let mut epilogue_stmts = Vec::new();
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
                let rets = ret_locations(&String::from_utf8_lossy(&text));
                epilogue_rule(&label, &h, &rets);
                let (hs, ls) = (stmts_but_epilogue(&h), stmts_but_epilogue(&l));
                assert_eq!(hs, ls, "{label}: statement rows differ");
                if stmts(&h) != stmts(&l) {
                    epilogue_stmts.push(label);
                }
                objects += 1;
            }
        }
    }
    eprintln!(
        "i386 debug: {objects} objects verified, trees and rows equal llc's; an epilogue \
         row of hod's is a statement where llc's is not (§5.2/§5.4 rule): {epilogue_stmts:?}"
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
