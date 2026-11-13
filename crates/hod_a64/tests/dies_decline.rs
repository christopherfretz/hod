//! Function DIEs that were declined before WP-094 (`spec/dwarf.md`
//! §6.13), and three of §6.7's permitted differences held to exactly
//! their shapes.
//!
//! The sources in `data/dies_decline/` (imports scoped in lexical blocks,
//! the order of the unit's types, a function's own early DIE, a
//! function's thread-local `static`, line-tables-only units with inlined
//! code), by clang 19 (`llc` 19), clang 22 and kiln++ (`llc` 22): hod's DIE
//! trees are `llc`'s, both objects verify, and the shapes are there. gdb
//! and lldb sessions on hod's programs (imports in blocks, thread-local
//! statics) print what they print on `llc`'s under GNU ld, lld and mold.
//!
//! The permitted differences (§6.7): an inlined call whose only code is
//! a branch hod falls through (`fallthrough.cpp`), the blocks under an
//! `llvm.is.constant` `llc` folds to false (`isconst.cpp`), and the gap
//! in `llc`'s location list of an `sret` result at the function's first
//! instruction (`sret.cpp`).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count_tag, first_difference, hod_object, kiln, llc_object, render, trees};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_decline")
}

/// The (name, compiler, `llc`) triples present for `source`: clang 19's
/// IR by `llc` 19, clang 22's and kiln++'s (C++ only) by `llc` 22.
fn pairs(source: &str) -> Vec<(&'static str, PathBuf, PathBuf)> {
    let c = support::has_ext(source, "c");
    let (c19, c22) = if c { ("clang-19", "clang-22") } else { ("clang++-19", "clang++-22") };
    let mut out = Vec::new();
    for (name, compiler, llc) in [("c19", c19, "llc-19"), ("c22", c22, "llc-22")] {
        if let (Some(cc), Some(l)) = (tool(compiler), tool(llc)) {
            out.push((name, cc, l));
        }
    }
    if !c && let (Some(k), Some(l)) = (kiln(), tool("llc-22")) {
        out.push(("kiln", k, l));
    }
    out
}

/// `-g -O0` IR of `source` by `compiler`, with `flags` after `-g`.
fn ir(compiler: &Path, scratch: &Scratch, name: &str, source: &str, flags: &[&str]) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "--target=aarch64-linux-gnu", "-S", "-emit-llvm", "-w"]).args(flags);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// hod's and `llc`'s objects of `source` by one pair, both verified.
fn objects(
    scratch: &Scratch,
    dwarfdump: &Path,
    tag: &str,
    pair: &(&str, PathBuf, PathBuf),
    source: &str,
    flags: &[&str],
) -> (String, PathBuf, PathBuf) {
    let text = ir(&pair.1, scratch, tag, source, flags);
    let hod = hod_object(scratch, &format!("{tag}-hod"), &text);
    let llc = llc_object(scratch, &pair.2, &format!("{tag}-llc"), &text);
    support::verify(dwarfdump, &hod);
    support::verify(dwarfdump, &llc);
    (text, hod, llc)
}

/// The DIEs of `tree` with `tag` whose attributes include `attr` (any,
/// when empty), with their parents' tags.
fn find<'a>(
    tree: &'a Node,
    tag: &str,
    attr: &str,
    parent: &'a str,
    out: &mut Vec<(&'a str, &'a Node)>,
) {
    if tree.tag == tag && (attr.is_empty() || tree.attrs.iter().any(|a| a.contains(attr))) {
        out.push((parent, tree));
    }
    for c in &tree.children {
        find(c, tag, attr, &tree.tag, out);
    }
}

/// Removes the DIEs under `tree` that `drop` names; how many.
fn strip(tree: &mut Node, drop: &dyn Fn(&Node) -> bool) -> usize {
    let before = tree.children.len();
    tree.children.retain(|c| !drop(c));
    let mut n = before - tree.children.len();
    for c in &mut tree.children {
        n += strip(c, drop);
    }
    n
}

fn has(n: &Node, attr: &str) -> bool {
    n.attrs.iter().any(|a| a == attr)
}

fn rendered(n: &Node) -> String {
    let mut s = String::new();
    render(n, 0, &mut s);
    s
}

/// The sources compared whole with `llc`: source, flags, the pairs left
/// out.
const TREES: &[(&str, &[&str], &[&str])] = &[
    ("imports.cpp", &[], &[]),
    ("imports2.cpp", &[], &[]),
    ("order.cpp", &[], &[]),
    ("own_early.cpp", &[], &[]),
    ("tls_static.c", &[], &[]),
    // `llc` 19 puts the static of a function that is only inlined under
    // an empty subprogram (§6.7).
    ("tls_static.cpp", &[], &["c19"]),
    ("lines_only.cpp", &["-gline-tables-only"], &[]),
];

#[test]
fn trees_match_llc() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-decline-trees");
    let mut compared = 0;
    for &(source, flags, skip) in TREES {
        for pair in pairs(source) {
            if skip.contains(&pair.0) {
                continue;
            }
            let tag = format!("{}-{}", source.replace('.', "_"), pair.0);
            let (_, hod, llc) = objects(&scratch, &dwarfdump, &tag, &pair, source, flags);
            let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc));
            assert_eq!(th.len(), 1, "{tag}");
            if let Some(d) = first_difference(&th[0], &tl[0]) {
                panic!("{tag}: hod's tree, then llc's: {d}");
            }
            let unit = &th[0];
            let mut v = Vec::new();
            match source {
                "imports.cpp" | "imports2.cpp" => {
                    for t in ["DW_TAG_imported_module", "DW_TAG_imported_declaration"] {
                        find(unit, t, "", "", &mut v);
                    }
                    let in_blocks = v.iter().filter(|(p, _)| *p == "DW_TAG_lexical_block").count();
                    // kiln++ writes no imported entities.
                    let wants = if pair.0 == "kiln" { 0 } else { 3 };
                    assert!(in_blocks >= wants, "{tag}: {in_blocks} imports in blocks");
                }
                "tls_static.c" | "tls_static.cpp" => {
                    let names: &[&str] = if source == "tls_static.c" {
                        &["counter"]
                    } else {
                        &["total", "inner", "lazy"]
                    };
                    for name in names {
                        v.clear();
                        find(unit, "DW_TAG_variable", &format!("(\"{name}\")"), "", &mut v);
                        assert_eq!(v.len(), 1, "{tag}: {name}");
                        let (parent, die) = v[0];
                        assert!(
                            matches!(parent, "DW_TAG_subprogram" | "DW_TAG_lexical_block"),
                            "{tag}: {name} under {parent}"
                        );
                        // AArch64's `llc` writes no location (§6.11).
                        assert!(
                            !die.attrs.iter().any(|a| a.starts_with("DW_AT_location")),
                            "{tag}: {name}"
                        );
                    }
                }
                "lines_only.cpp" => {
                    // `two` is out of line and inlined: a concrete DIE.
                    find(unit, "DW_TAG_subprogram", "DW_AT_abstract_origin", "", &mut v);
                    assert_eq!(v.len(), 1, "{tag}: the concrete subprogram");
                    assert!(count_tag(unit, "DW_TAG_inlined_subroutine") >= 4, "{tag}");
                }
                _ => {}
            }
            compared += 1;
        }
    }
    eprintln!("{compared} trees equal to llc's");
}

// ------------------------------------------ permitted differences ----

/// The lines of the DIE with `tag` whose name is `name`, in
/// `llvm-dwarfdump --debug-info` output.
fn die_lines<'a>(dump: &'a str, tag: &str, attr: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in dump.lines().chain([""]) {
        if line.trim().is_empty() {
            if current.first().is_some_and(|l| l.ends_with(tag))
                && current.iter().any(|l| l.contains(attr))
            {
                assert!(out.is_empty(), "two DIEs {tag} with {attr}");
                out = std::mem::take(&mut current);
            }
            current.clear();
        } else {
            current.push(line);
        }
    }
    out
}

fn dump(dwarfdump: &Path, object: &Path) -> String {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success());
    out.stdout_text()
}

/// The value of `attr`, an address, in the lines of a DIE.
fn address(lines: &[&str], attr: &str) -> u64 {
    let line = lines.iter().find(|l| l.trim_start().starts_with(attr)).unwrap();
    let hex = line.split("(0x").nth(1).unwrap().trim_end_matches(')');
    u64::from_str_radix(hex, 16).unwrap()
}

#[test]
fn permitted_fallthrough_inlined() {
    let (Some(dwarfdump), Some(objdump)) = (tool("llvm-dwarfdump"), tool("llvm-objdump")) else {
        return;
    };
    let scratch = Scratch::new("dies-decline-fallthrough");
    for pair in pairs("fallthrough.cpp") {
        let tag = format!("fallthrough-{}", pair.0);
        let (_, hod, llc) = objects(&scratch, &dwarfdump, &tag, &pair, "fallthrough.cpp", &[]);
        let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc));
        let (h, mut l) = (th[0].clone(), tl[0].clone());
        assert_eq!(count_tag(&h, "DW_TAG_inlined_subroutine"), 0, "{tag}");
        // llc's: the inlined call, its abstract instance, and the two
        // types only its parameter names. Nothing else differs.
        let call = |n: &Node| {
            n.tag == "DW_TAG_inlined_subroutine"
                && has(n, "DW_AT_abstract_origin (\"_Z8categoryRKi\")")
        };
        assert_eq!(strip(&mut l, &call), 1, "{tag}");
        let only_llc = |n: &Node| {
            (n.tag == "DW_TAG_subprogram"
                && has(n, "DW_AT_linkage_name (\"_Z8categoryRKi\")")
                && has(n, "DW_AT_inline (DW_INL_inlined)"))
                || (n.tag == "DW_TAG_reference_type" && n.attrs == ["DW_AT_type (\"const int\")"])
                || (n.tag == "DW_TAG_const_type" && n.attrs == ["DW_AT_type (\"int\")"])
        };
        let before = l.children.len();
        l.children.retain(|c| !only_llc(c));
        assert_eq!(before - l.children.len(), 3, "{tag}");
        // The unit's children in `llc`'s order of type creation, which
        // the abstract instance changes.
        assert_eq!(h.attrs, l.attrs, "{tag}");
        let mut hs: Vec<String> = h.children.iter().map(rendered).collect();
        let mut ls: Vec<String> = l.children.iter().map(rendered).collect();
        hs.sort();
        ls.sort();
        assert_eq!(hs, ls, "{tag}: the unit's children, hod's then llc's");
        // The call's only code: one branch to the next instruction.
        let text = dump(&dwarfdump, &llc);
        let die = die_lines(&text, "DW_TAG_inlined_subroutine", "\"_Z8categoryRKi\"");
        let (low, high) = (address(&die, "DW_AT_low_pc"), address(&die, "DW_AT_high_pc"));
        assert_eq!(high - low, 4, "{tag}");
        let mut command = Command::new(&objdump);
        command.args(["-d", "--no-show-raw-insn"]).arg(&llc);
        let disasm = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
        let body = disasm.split("<_Z3usei>:").nth(1).unwrap().split("\n\n").next().unwrap();
        let insn: Vec<&str> = body
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            .find(|t| t.first() == Some(&format!("{low:x}:").as_str()))
            .unwrap_or_default();
        assert_eq!(insn.get(1..3), Some(&["b", format!("{high:#x}").as_str()][..]), "{tag}");
    }
}

#[test]
fn permitted_is_constant_blocks() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-decline-isconst");
    for pair in pairs("isconst.cpp") {
        let tag = format!("isconst-{}", pair.0);
        let (text, hod, llc) = objects(&scratch, &dwarfdump, &tag, &pair, "isconst.cpp", &[]);
        assert!(text.contains("@llvm.is.constant"), "{tag}");
        let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc));
        let mut h = th[0].clone();
        // hod's only: the block whose code is under the folded call.
        let block = |n: &Node| {
            n.tag == "DW_TAG_lexical_block"
                && n.children.len() == 1
                && n.children[0].tag == "DW_TAG_variable"
                && has(&n.children[0], "DW_AT_name (\"folded\")")
        };
        assert_eq!(strip(&mut h, &block), 1, "{tag}");
        if let Some(d) = first_difference(&h, &tl[0]) {
            panic!("{tag}: hod's tree without the block, then llc's: {d}");
        }
    }
}

#[test]
fn permitted_sret_gap() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-decline-sret");
    for pair in pairs("sret.cpp") {
        let tag = format!("sret-{}", pair.0);
        let (_, hod, llc) = objects(&scratch, &dwarfdump, &tag, &pair, "sret.cpp", &[]);
        let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc));
        // `llc`'s unit names its location lists' base; hod has none.
        let mut l = tl[0].clone();
        let before = l.attrs.len();
        l.attrs.retain(|a| a != "DW_AT_loclists_base");
        assert_eq!(before - l.attrs.len(), 1, "{tag}");
        if let Some(d) = first_difference(&th[0], &l) {
            panic!("{tag}: hod's tree, then llc's: {d}");
        }
        // hod: the slot holding the result's address, over the whole
        // function.
        let text = dump(&dwarfdump, &hod);
        let ret = die_lines(&text, "DW_TAG_variable", "(\"ret\")");
        let loc = ret.iter().find(|l| l.contains("DW_AT_location")).unwrap();
        assert!(loc.contains("(DW_OP_fbreg +") && loc.ends_with(", DW_OP_deref)"), "{tag}: {loc}");
        // llc: two ranges from the first instruction's end to the
        // function's: no location at its first instruction only.
        let text = dump(&dwarfdump, &llc);
        let make = die_lines(&text, "DW_TAG_subprogram", "(\"make\")");
        let (low, high) = (address(&make, "DW_AT_low_pc"), address(&make, "DW_AT_high_pc"));
        let ret = die_lines(&text, "DW_TAG_variable", "(\"ret\")");
        let ranges: Vec<(u64, u64)> = ret
            .iter()
            .filter_map(|l| l.trim_start().strip_prefix('['))
            .map(|r| {
                let (a, b) = r.split_once(", ").unwrap();
                let b = b.split(')').next().unwrap();
                let hex = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
                (hex(a), hex(b))
            })
            .collect();
        assert_eq!(ranges, [(low + 4, low + 8), (low + 8, high)], "{tag}:\n{}", ret.join("\n"));
    }
}

// --------------------------------------------------------- sessions ----

/// Hex numbers, process ids, and offsets in functions masked; the
/// program's directory named `DIR`.
fn masked(text: &str, dir: &Path) -> String {
    let dir = dir.display().to_string();
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.replace(&dir, "DIR");
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i..].starts_with(b"0x") {
                out.push_str("0x?");
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                continue;
            }
            let after = |p: &[u8]| b[i..].starts_with(p);
            if after(b"rocess ") || after(b" + ") {
                let skip = if after(b" + ") { 3 } else { 7 };
                let digits = b[i + skip..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 {
                    out.push_str(&line[i..i + skip]);
                    out.push('?');
                    i += skip + digits;
                    continue;
                }
            }
            out.push(b[i] as char);
            i += 1;
        }
        out.push('\n');
    }
    out
}

/// A session: the source, gdb's commands and what its output must
/// contain, lldb's commands and what its output must contain.
struct Session {
    source: &'static str,
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const SESSIONS: &[Session] = &[
    Session {
        source: "imports.cpp",
        gdb: &[
            "break imports.cpp:7",
            "break imports.cpp:15",
            "break imports.cpp:22",
            "break imports.cpp:30",
            "run",
            "print g",
            "print h(3)",
            "continue",
            "print g",
            "print k",
            "continue",
            "ptype S",
            "print s",
            "delete 3",
            "continue",
            "print N::g",
            "print k",
            "continue",
        ],
        gdb_wants: &[
            "$1 = 1\n$2 = 4",
            "$3 = 1\n$4 = 2",
            "type = struct ns::S {",
            "f2 (a=1) at imports.cpp:30",
            "$6 = 1\n$7 = 2",
            "exited normally",
        ],
        lldb: &[
            "breakpoint set --file imports.cpp --line 7",
            "breakpoint set --file imports.cpp --line 15",
            "breakpoint set --file imports.cpp --line 22",
            "breakpoint set --file imports.cpp --line 30",
            "run",
            "expr g",
            "expr h(3)",
            "continue",
            "expr g",
            "expr k",
            "continue",
            "frame variable s",
            "breakpoint delete 3",
            "continue",
            "expr N::g",
            "expr k",
            "continue",
        ],
        lldb_wants: &[
            "(int) $0 = 1",
            "(int) $1 = 4",
            "(int) $3 = 2",
            "(ns::S) s = (v = 0)",
            "(int) $5 = 2",
            "exited with status = 0",
        ],
    },
    Session {
        source: "tls_static.c",
        gdb: &["break tls_static.c:5", "run", "print counter", "print file_tls", "continue"],
        gdb_wants: &["$1 = <optimized out>\n$2 = 7", "exited normally"],
        // lldb reads no thread-local variable without a location (nor
        // `file_tls`, under either writer).
        lldb: &[
            "breakpoint set --file tls_static.c --line 5",
            "run",
            "frame variable counter",
            "continue",
        ],
        lldb_wants: &[
            "(int) counter = <no location, value may have been optimized out>",
            "exited with status = 0",
        ],
    },
    Session {
        source: "tls_static.cpp",
        gdb: &[
            "break tls_static.cpp:6",
            "break tls_static.cpp:19",
            "break tls_static.cpp:25",
            "run",
            "info locals",
            "continue",
            "print inner",
            "continue",
            "print lazy",
            "continue",
        ],
        gdb_wants: &[
            "blocky (v=1) at tls_static.cpp:19",
            "$1 = <optimized out>",
            "$2 = <optimized out>",
            "exited normally",
        ],
        lldb: &[
            "breakpoint set --file tls_static.cpp --line 6",
            "breakpoint set --file tls_static.cpp --line 19",
            "breakpoint set --file tls_static.cpp --line 25",
            "run",
            "frame variable",
            "continue",
            "frame variable inner",
            "continue",
            "frame variable lazy",
            "continue",
        ],
        lldb_wants: &[
            "(int) total = <no location, value may have been optimized out>",
            "(int) inner = <no location, value may have been optimized out>",
            "(int) lazy = <no location, value may have been optimized out>",
            "exited with status = 0",
        ],
    },
];

fn run_session(debugger: &Path, commands: &[&str], lldb: bool, program: &Path) -> String {
    let mut command = Command::new(debugger);
    if lldb {
        command
            .args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
            .args(["-o", "settings set target.disable-aslr false"]);
        for c in commands {
            command.arg("-o").arg(c);
        }
    } else {
        command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
        command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
        for c in commands {
            command.arg("-ex").arg(c);
        }
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "{} on {}", debugger.display(), program.display());
    out.stdout_text().replace(&format!("{}/", data_dir().display()), "")
}

#[test]
fn sessions_match_llc() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb), Some(dwarfdump)) =
        (tool("gdb"), tool("lldb"), tool("llvm-dwarfdump"))
    else {
        return;
    };
    let scratch = Scratch::new("dies-decline-sessions");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let mut sessions = 0;
    for s in SESSIONS {
        // clang 22's IR, by `llc` 22.
        let Some(pair) = pairs(s.source).into_iter().find(|p| p.0 == "c22") else { continue };
        let name = s.source.replace('.', "_");
        let (_, hod, llc) = objects(&scratch, &dwarfdump, &name, &pair, s.source, &[]);
        for linker in &toolchain.linkers {
            let tag = format!("{}, {}", s.source, linker.name);
            let mut programs = Vec::new();
            for (who, object) in [("hod", &hod), ("llc", &llc)] {
                let dir = scratch.join(format!("{who}-{name}-{}", linker.name));
                std::fs::create_dir_all(&dir).unwrap();
                let program = dir.join("prog");
                let inputs = [object.as_path(), Path::new("-lstdc++")];
                let ran = toolchain.link_and_run(linker, &["-no-pie"], &inputs, &program);
                assert_eq!(exit_code(&ran), Some(0), "{tag}: {who}'s program");
                programs.push((dir, program));
            }
            for (debugger, is_lldb, commands, wants) in
                [(&gdb, false, s.gdb, s.gdb_wants), (&lldb, true, s.lldb, s.lldb_wants)]
            {
                let which = if is_lldb { "lldb" } else { "gdb" };
                let [(dh, ph), (dl, pl)] = &programs[..] else { unreachable!() };
                let h = masked(&run_session(debugger, commands, is_lldb, ph), dh);
                let l = masked(&run_session(debugger, commands, is_lldb, pl), dl);
                assert_eq!(h, l, "{tag}: {which}: hod's session, then llc's");
                for w in wants {
                    assert!(h.contains(w), "{tag}: {which}: {w:?} in\n{h}");
                }
                sessions += 2;
            }
        }
    }
    eprintln!("{sessions} sessions, hod's equal to llc's");
}
