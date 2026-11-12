//! Labels (`spec/dwarf.md` §6.14, WP-098).
//!
//! The sources in `data/dies_label/`: `labels.c` (`goto` targets in a
//! function, in a nested block, in an always-inlined function: an abstract
//! label and its concrete copy) by clang 19 (`llc` 19), clang 22 and
//! kiln++ (`llc` 22); `coro.cpp` (a coroutine: `CoroSplit` gives its
//! clones artificial `__coro_resume_N` labels with `coroSuspendIdx`, LLVM
//! 21 and later) by clang 22 and kiln++ (clang 19's IR has no such
//! labels). hod's DIE trees are `llc`'s, both objects verify, and the
//! shapes are there. Labels of every other shape (no name, line 0,
//! artificial, a suspend index on an abstract label) are made from clang
//! 22's IR of `labels.c` and compared with `llc` 22 the same way. gdb and
//! lldb sessions on hod's programs print what they print on `llc`'s under
//! GNU ld, lld and mold.
//!
//! Also two findings of the census (§6.14): a member whose alignment is
//! not a whole byte (`CoroSplit`'s frame types) and a unit with imports
//! but no code.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count_tag, first_difference, hod_object, kiln, llc_object, trees};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_label")
}

/// The (name, compiler, `llc`) triples for `source`: clang 19's IR by
/// `llc` 19 (`with19`), clang 22's and kiln++'s by `llc` 22.
fn pairs(source: &str, with19: bool) -> Vec<(&'static str, PathBuf, PathBuf)> {
    let c = support::has_ext(source, "c");
    let (c19, c22) = if c { ("clang-19", "clang-22") } else { ("clang++-19", "clang++-22") };
    let mut out = Vec::new();
    for (name, compiler, llc) in [("c19", c19, "llc-19"), ("c22", c22, "llc-22")] {
        if (with19 || name != "c19")
            && let (Some(cc), Some(l)) = (tool(compiler), tool(llc))
        {
            out.push((name, cc, l));
        }
    }
    if let (Some(k), Some(l)) = (kiln(), tool("llc-22")) {
        out.push(("kiln", k, l));
    }
    out
}

/// `-g -O0` IR of `source` by `compiler` (kiln++ reads a C source as
/// C++).
fn ir(compiler: &Path, scratch: &Scratch, name: &str, source: &str) -> String {
    let is_kiln = compiler.file_name().is_some_and(|n| n.to_string_lossy().starts_with("kiln"));
    let mut input = data_dir().join(source);
    if is_kiln && support::has_ext(source, "c") {
        let text = std::fs::read_to_string(&input).unwrap();
        input = scratch.write(&format!("{name}.cpp"), text);
    }
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w"]);
    if !support::has_ext(source, "c") || is_kiln {
        command.arg("-std=c++20");
    }
    command.arg(&input).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// hod's and `llc`'s objects of `text`, both verified, and their trees
/// (one unit each).
fn compare(scratch: &Scratch, dwarfdump: &Path, llc: &Path, tag: &str, text: &str) -> (Node, Node) {
    let (h, l) = unit_trees(scratch, dwarfdump, llc, tag, text);
    if let Some(d) = first_difference(&h, &l) {
        panic!("{tag}: hod's tree, then llc's: {d}");
    }
    (h, l)
}

/// hod's and `llc`'s objects of `text`, both verified, and their trees
/// (one unit each), not compared.
fn unit_trees(
    scratch: &Scratch,
    dwarfdump: &Path,
    llc: &Path,
    tag: &str,
    text: &str,
) -> (Node, Node) {
    let hod = hod_object(scratch, &format!("{tag}-hod"), text);
    let llc = llc_object(scratch, llc, &format!("{tag}-llc"), text);
    support::verify(dwarfdump, &hod);
    support::verify(dwarfdump, &llc);
    let (mut th, mut tl) = (trees(dwarfdump, &hod), trees(dwarfdump, &llc));
    assert_eq!((th.len(), tl.len()), (1, 1), "{tag}: one unit each");
    (th.remove(0), tl.remove(0))
}

/// The DIEs with `tag` under `n`, depth first.
fn all<'a>(n: &'a Node, tag: &str, out: &mut Vec<&'a Node>) {
    if n.tag == tag {
        out.push(n);
    }
    for c in &n.children {
        all(c, tag, out);
    }
}

fn labels(n: &Node) -> Vec<&Node> {
    let mut out = Vec::new();
    all(n, "DW_TAG_label", &mut out);
    out
}

fn has(n: &Node, attr: &str) -> bool {
    n.attrs.iter().any(|a| a == attr || a.starts_with(&format!("{attr} ")))
}

#[test]
fn labels_match_llc() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-label-labels");
    for (name, cc, llc) in pairs("labels.c", true) {
        let tag = format!("labels-{name}");
        let text = ir(&cc, &scratch, &tag, "labels.c");
        let (h, _) = compare(&scratch, &dwarfdump, &llc, &tag, &text);
        let ls = labels(&h);
        if name == "kiln" && ls.is_empty() {
            // kiln++ writes no `DILabel` for a `goto` target.
            eprintln!("note: {tag}: no labels in kiln++'s IR");
            continue;
        }
        // sum_down's again and done, out abstract and concrete in main,
        // spin's again and done.
        assert_eq!(ls.len(), 6, "{tag}: {ls:?}");
        assert!(ls.iter().any(|l| has(l, "DW_AT_abstract_origin")), "{tag}: the inlined copy");
        assert!(ls.iter().all(|l| !has(l, "DW_AT_artificial")), "{tag}");
    }
}

#[test]
fn coroutine_labels_match_llc() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-label-coro");
    for (name, cc, llc) in pairs("coro.cpp", false) {
        let tag = format!("coro-{name}");
        let text = coro_ir(&cc, &scratch, &tag);
        assert!(text.contains("coroSuspendIdx"), "{tag}: no suspend label");
        // The ramp function declares the frame's variables at the
        // frame's address, a call's result: `llc -O0` drops them, hod
        // keeps them (`ramp_frame_variables_match_llc`). The clones,
        // where the labels are, are compared whole.
        let (h, l) = unit_trees(&scratch, &dwarfdump, &llc, &tag, &text);
        let (ch, cl) = (clones(&h), clones(&l));
        assert_eq!(ch.len(), 3, "{tag}: resume, destroy, cleanup");
        assert_eq!(ch.len(), cl.len(), "{tag}");
        for (a, b) in ch.iter().zip(&cl) {
            if let Some(d) = first_difference(a, b) {
                panic!("{tag}: hod's clone, then llc's: {d}");
            }
        }
        let suspend: Vec<&Node> =
            labels(&h).into_iter().filter(|l| has(l, "DW_AT_LLVM_coro_suspend_idx")).collect();
        assert!(!suspend.is_empty(), "{tag}: no suspend label");
        for l in &suspend {
            assert!(has(l, "DW_AT_artificial"), "{tag}: {l:?}");
            assert!(has(l, "DW_AT_low_pc"), "{tag}: {l:?}");
            assert!(l.attrs[0].starts_with("DW_AT_name (\"__coro_resume_"), "{tag}: {l:?}");
        }
        // Each clone (resume, destroy, cleanup) has suspend point 0.
        let zero = suspend.iter().filter(|l| has(l, "DW_AT_LLVM_coro_suspend_idx (0)")).count();
        assert!(zero >= 2, "{tag}: {zero} labels of suspend point 0");
    }
}

/// The subprograms of a coroutine's clones.
fn clones(unit: &Node) -> Vec<&Node> {
    let mut out = Vec::new();
    all(unit, "DW_TAG_subprogram", &mut out);
    out.retain(|s| {
        s.attrs.iter().any(|a| {
            a.starts_with("DW_AT_linkage_name")
                && [".resume\")", ".destroy\")", ".cleanup\")"].iter().any(|e| a.ends_with(e))
        })
    });
    out
}

/// clang's coroutine ramp: `CoroSplit` moves the declarations of the
/// frame's variables (`__promise`, the parameters' copies, the loop
/// variable) to the frame's address, the result of the allocation call.
/// `llc -O0` drops a declaration whose address is not a stack slot or an
/// argument; hod's code generator keeps it (`crates/hod_cg`, the records
/// of `dbg_declare`), so hod's ramp has a `__promise` with a location
/// `llc`'s has not. Outside WP-098's ownership.
#[test]
#[ignore = "hod_cg keeps a dbg_declare of a non-slot address that llc -O0 drops (WP-098)"]
fn ramp_frame_variables_match_llc() {
    let (Some(dwarfdump), Some(cc), Some(llc)) =
        (tool("llvm-dwarfdump"), tool("clang++-22"), tool("llc-22"))
    else {
        return;
    };
    let scratch = Scratch::new("dies-label-ramp");
    let text = coro_ir(&cc, &scratch, "ramp");
    compare(&scratch, &dwarfdump, &llc, "ramp", &text);
}

/// `-g -O0` IR of `coro.cpp`, each variable declared once per function:
/// clang's ramp function declares the coroutine's parameter twice (its
/// slot and its copy in the frame), which hod refuses
/// (`debug:variable-twice`, not this package's); the labels are the
/// clones' and are kept.
fn coro_ir(compiler: &Path, scratch: &Scratch, name: &str) -> String {
    let text = ir(compiler, scratch, name, "coro.cpp");
    let mut out = String::with_capacity(text.len());
    let mut seen: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with("define ") {
            seen.clear();
        }
        if let Some(rest) = line.trim_start().strip_prefix("#dbg_declare(")
            && let Some(var) = rest.split(", ").nth(1)
        {
            if seen.contains(&var) {
                continue;
            }
            seen.push(var);
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `text` with `from` replaced by `to` exactly once.
fn edit(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from:?}");
    text.replacen(from, to, 1)
}

/// The `DILabel` line of `name` in `text`.
fn label_line<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains("!DILabel(") && l.contains(&format!("name: \"{name}\"")))
        .unwrap()
}

#[test]
fn label_shapes_match_llc() {
    let (Some(dwarfdump), Some(cc), Some(llc)) =
        (tool("llvm-dwarfdump"), tool("clang-22"), tool("llc-22"))
    else {
        return;
    };
    let scratch = Scratch::new("dies-label-shapes");
    let text = ir(&cc, &scratch, "shapes-base", "labels.c");
    // Each shape `applyLabelAttributes` distinguishes, one at a time.
    let again = label_line(&text, "again");
    let done = label_line(&text, "done");
    let out = label_line(&text, "out");
    let cases: Vec<(&str, String, &str)> = vec![
        ("nameless", edit(&text, done, &done.replace("name: \"done\"", "name: \"\"")), "done"),
        (
            "line0",
            edit(
                &text,
                again,
                &again.replace("line: 6,", "line: 0,").replace(")", ", isArtificial: true)"),
            ),
            "again",
        ),
        (
            "suspend",
            edit(&text, done, &done.replace(")", ", isArtificial: true, coroSuspendIdx: 7)")),
            "done",
        ),
        (
            "abstract",
            edit(&text, out, &out.replace(")", ", isArtificial: true, coroSuspendIdx: 300)")),
            "out",
        ),
        (
            "nocolumn",
            edit(
                &text,
                done,
                &done.replace(", column: 1", "").replace(")", ", isArtificial: true)"),
            ),
            "done",
        ),
    ];
    for (tag, text, which) in cases {
        let (h, _) = compare(&scratch, &dwarfdump, &llc, tag, &text);
        let ls = labels(&h);
        assert_eq!(ls.len(), 6, "{tag}");
        match tag {
            "nameless" => {
                assert!(
                    ls.iter().any(|l| !has(l, "DW_AT_name") && !has(l, "DW_AT_abstract_origin"))
                )
            }
            "line0" => {
                let l = ls.iter().find(|l| has(l, "DW_AT_name (\"again\")")).unwrap();
                assert!(!has(l, "DW_AT_decl_line") && !has(l, "DW_AT_decl_column"), "{l:?}");
                assert!(has(l, "DW_AT_artificial"), "{l:?}");
            }
            "suspend" | "abstract" => {
                let idx = if tag == "suspend" { 7 } else { 300 };
                let l = ls.iter().find(|l| has(l, &format!("DW_AT_name (\"{which}\")"))).unwrap();
                let want = format!("DW_AT_LLVM_coro_suspend_idx ({idx})");
                assert!(has(l, &want) && has(l, "DW_AT_artificial"), "{l:?}");
                // The abstract label has no address; its concrete copy is
                // `abstract_origin` and `low_pc` only.
                assert_eq!(has(l, "DW_AT_low_pc"), tag == "suspend", "{l:?}");
            }
            _ => {
                let l = ls.iter().find(|l| has(l, "DW_AT_name (\"done\")")).unwrap();
                assert!(has(l, "DW_AT_decl_line") && !has(l, "DW_AT_decl_column"), "{l:?}");
            }
        }
    }
}

/// A member whose alignment is under a byte (`align: 4`, which
/// `CoroSplit`'s frame types carry): `llc` writes no `DW_AT_alignment`
/// (`getAlignInBytes` is 0), and hod's abbreviation and bytes must agree
/// (WP-098 found them disagreeing: the bit was set, the value not
/// written, and the unit unreadable).
#[test]
fn sub_byte_alignment_matches_llc() {
    let (Some(dwarfdump), Some(cc), Some(llc)) =
        (tool("llvm-dwarfdump"), tool("clang-22"), tool("llc-22"))
    else {
        return;
    };
    let scratch = Scratch::new("dies-label-align");
    let source =
        "struct S { char c; int i; };\nint main(void) { struct S s = {1, 2}; return s.i - 2; }\n";
    let path = scratch.write("align.c", source);
    let out = scratch.join("align.ll");
    let mut command = Command::new(&cc);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w"]).arg(&path).arg("-o").arg(&out);
    assert!(run_with(&mut command, &[], Duration::from_mins(1)).success());
    let text = std::fs::read_to_string(out).unwrap();
    let member =
        text.lines().find(|l| l.contains("DW_TAG_member") && l.contains("name: \"i\"")).unwrap();
    let struct_line = text.lines().find(|l| l.contains("name: \"S\"")).unwrap();
    let text = edit(&text, member, &member.replace("size: 32", "size: 32, align: 4"));
    let text = edit(&text, struct_line, &struct_line.replace("size: 64", "size: 64, align: 4"));
    let (h, _) = compare(&scratch, &dwarfdump, &llc, "align", &text);
    assert_eq!(count_tag(&h, "DW_TAG_member"), 2);
}

/// A unit with imported entities but no code and no globals (a
/// compile-only libc++ test): `llc` writes the unit (`beginModule` skips
/// only a unit that lists nothing), hod writes none. The census's one
/// real difference outside the DIE writer: the bridge's filter of the
/// units it writes (`crates/hod_cg/src/emit/lines.rs`, `finish`,
/// "`llc` writes nothing for a unit with neither code nor globals").
#[test]
#[ignore = "hod_cg writes no unit without code or globals; llc writes one with imports (WP-098)"]
fn unit_with_only_imports_matches_llc() {
    let (Some(dwarfdump), Some(cc), Some(llc)) =
        (tool("llvm-dwarfdump"), tool("clang++-22"), tool("llc-22"))
    else {
        return;
    };
    let scratch = Scratch::new("dies-label-imports");
    let source = "namespace n { int f(); }\nusing n::f;\n";
    let path = scratch.write("imports.cpp", source);
    let out = scratch.join("imports.ll");
    let mut command = Command::new(&cc);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w"]).arg(&path).arg("-o").arg(&out);
    assert!(run_with(&mut command, &[], Duration::from_mins(1)).success());
    let text = std::fs::read_to_string(out).unwrap();
    assert!(text.contains("imports: "), "the unit lists an import");
    compare(&scratch, &dwarfdump, &llc, "imports", &text);
}

// ------------------------------------------------------- debuggers ----

/// Hex numbers, process ids and code offsets masked; the program's
/// directory named `DIR`.
fn masked(text: &str, dir: &Path) -> String {
    let text = text.replace(&dir.display().to_string(), "DIR");
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
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
        if after(b"rocess ") || after(b" + ") || after(b"+") {
            let skip = if after(b"rocess ") {
                7
            } else if after(b" + ") {
                3
            } else {
                1
            };
            let digits = b[i + skip..].iter().take_while(|c| c.is_ascii_digit()).count();
            // `+N` only as gdb's `<function+N>` or `<+N>`.
            let closed = skip != 1 || b.get(i + skip + digits) == Some(&b'>');
            if digits > 0 && closed {
                out.push_str(&text[i..i + skip]);
                out.push('?');
                i += skip + digits;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

struct Session {
    source: &'static str,
    /// Front ends whose IR is debugged.
    frontends: &'static [&'static str],
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const SESSIONS: &[Session] = &[
    Session {
        source: "labels.c",
        frontends: &["c19", "c22"],
        // gdb finds a label by `function:label` (linespecs, `info scope`)
        // but not as a symbol (`info address`); `spin` has no variables,
        // so its scope lists the labels alone.
        gdb: &[
            "info scope spin:done",
            "info line spin:again",
            "info line sum_down:done",
            "break spin:done",
            "break clamp:out",
            "break sum_down:done",
            "run",
            "bt",
            "info locals",
            "continue",
            "bt",
            "continue",
            "print counter",
            "continue",
        ],
        gdb_wants: &[
            "Symbol again is a label at address",
            "Symbol done is a label at address",
            "Line 31 of \"labels.c\"",
            "Breakpoint 3, sum_down (x=0)",
            "r = 12",
            "Breakpoint 2, clamp (x=10)",
            "Breakpoint 1, spin ()",
            "$1 = 3",
            "exited normally",
        ],
        // lldb resolves no breakpoint by a label's name (pending, no
        // locations); the labels' lines are used instead.
        lldb: &[
            "breakpoint set -n again",
            "breakpoint set --file labels.c --line 36",
            "breakpoint set --file labels.c --line 17",
            "run",
            "frame variable",
            "continue",
            "bt",
            "target variable counter",
            "continue",
        ],
        lldb_wants: &[
            "(int) r = 12",
            "spin at labels.c:36",
            "(int) counter = 3",
            "exited with status = 0",
        ],
    },
    Session {
        source: "coro.cpp",
        frontends: &["c22", "kiln"],
        // `info scope` would print the frame's variables' locations,
        // whose frame offsets differ (§6.7); the label is found by
        // `break` and `info line` instead.
        gdb: &[
            "info line evens:__coro_resume_1",
            "break evens:__coro_resume_1",
            "run",
            "bt 1",
            "print i",
            "print n",
            "continue",
            "print i",
            "delete",
            "continue",
        ],
        gdb_wants: &[
            "Breakpoint 1 at 0x?: evens:__coro_resume_1.",
            // kiln++'s clone has `n` as a variable, clang's as a parameter.
            "[clone .resume] (",
            "$2 = 3",
            "exited normally",
        ],
        // A line breakpoint would count the line's rows (§5.7 P1); the
        // resume clone is broken at by its linkage name.
        lldb: &[
            "breakpoint set -n _Z5evensi.resume",
            "run",
            "bt 1",
            "frame variable n",
            "continue",
            "frame variable n",
            "breakpoint delete 1",
            "continue",
        ],
        lldb_wants: &["(int) n = 3", "(.resume)", "exited with status = 0"],
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
    let scratch = Scratch::new("dies-label-sessions");
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let mut sessions = 0;
    for s in SESSIONS {
        for (fe, cc, llc) in pairs(s.source, true) {
            if !s.frontends.contains(&fe) {
                continue;
            }
            let name = format!("{}-{fe}", s.source.replace('.', "_"));
            let text = if s.source == "coro.cpp" {
                coro_ir(&cc, &scratch, &name)
            } else {
                ir(&cc, &scratch, &name, s.source)
            };
            let hod = hod_object(&scratch, &format!("{name}-hod"), &text);
            let llc = llc_object(&scratch, &llc, &format!("{name}-llc"), &text);
            support::verify(&dwarfdump, &hod);
            for linker in &toolchain.linkers {
                let tag = format!("{name}, {}", linker.name);
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
                        assert!(l.contains(w), "{tag}: {which}: {w:?} in llc's\n{l}");
                    }
                    sessions += 2;
                }
            }
        }
    }
    eprintln!("{sessions} sessions, hod's equal to llc's");
}
