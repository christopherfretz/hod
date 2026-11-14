//! WP-221 part B (`spec/dwarf.md` §6.30): a `#dbg_declare` whose address
//! is an instruction result (a `phi`, a `getelementptr`) is a location
//! list of the ranges where the result's home holds the address: from its
//! definition on, over the blocks the definition dominates, never before
//! it is written. On both targets, DWARF 5 (`.debug_loclists`, split
//! `.debug_loclists.dwo`) and DWARF 4 (`.debug_loc`); the objects verify
//! under `llvm-dwarfdump` 19 and 22; the variables with lists are the
//! ones `llc`'s default `-O0` selectors (`-fast-isel=1`) describe by
//! lists, on hand-written IR and on clang 19's and 22's coroutine ramps.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

/// Declarations of each kind in one function: `both` on a slot and on a
/// `phi` (the slot is kept), `gep` on a `getelementptr` with an offset
/// expression, `dead` on one nothing uses (never computed: no DIE),
/// `deref` and `plus` on a loop's `phi` (recorded in that order).
const DECLARED: &str = r#"source_filename = "declared.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

declare void @use(ptr)
declare ptr @make()

define void @f(ptr %p) #0 !dbg !10 {
entry:
  %s = alloca ptr, align 8
  store ptr %p, ptr %s, align 8
    #dbg_declare(ptr %s, !34, !DIExpression(), !20)
  call void @use(ptr %p), !dbg !20
  %q = getelementptr i8, ptr %p, i64 16, !dbg !20
    #dbg_declare(ptr %q, !30, !DIExpression(DW_OP_plus_uconst, 8), !20)
  %dead = getelementptr i8, ptr %p, i64 32, !dbg !20
    #dbg_declare(ptr %dead, !31, !DIExpression(), !20)
  call void @use(ptr %q), !dbg !20
  br label %loop, !dbg !20

loop:
  %h = phi ptr [ %p, %entry ], [ %next, %loop ], !dbg !21
    #dbg_declare(ptr %h, !32, !DIExpression(DW_OP_deref), !21)
    #dbg_declare(ptr %h, !33, !DIExpression(DW_OP_plus_uconst, 16), !21)
    #dbg_declare(ptr %h, !34, !DIExpression(), !21)
  %next = call ptr @make(), !dbg !21
  %c = icmp eq ptr %next, null, !dbg !21
  br i1 %c, label %done, label %loop, !dbg !21

done:
  call void @use(ptr %h), !dbg !22
  ret void, !dbg !22
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, nameTableKind: None)
!1 = !DIFile(filename: "declared.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{null}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!20 = !DILocation(line: 2, column: 3, scope: !10)
!21 = !DILocation(line: 3, column: 3, scope: !10)
!22 = !DILocation(line: 4, column: 3, scope: !10)
!30 = !DILocalVariable(name: "gep", scope: !10, file: !1, line: 2, type: !13)
!31 = !DILocalVariable(name: "dead", scope: !10, file: !1, line: 2, type: !13)
!32 = !DILocalVariable(name: "deref", scope: !10, file: !1, line: 3, type: !13)
!33 = !DILocalVariable(name: "plus", scope: !10, file: !1, line: 3, type: !13)
!34 = !DILocalVariable(name: "both", scope: !10, file: !1, line: 2, type: !13)
"#;

/// `text` for x86-64.
fn x86_64(text: &str) -> String {
    text.replace(
        "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128",
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
    )
    .replace("aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu")
    .replace(r#""frame-pointer"="non-leaf""#, r#""frame-pointer"="all""#)
}

fn dwarf4(text: &str) -> String {
    text.replace(r#""Dwarf Version", i32 5"#, r#""Dwarf Version", i32 4"#)
}

/// `text` as a split unit's (`-gsplit-dwarf`).
fn split(text: &str) -> String {
    text.replace("nameTableKind", r#"splitDebugFilename: "declared.dwo", nameTableKind"#)
}

fn hod_object(input: &[u8], split: bool) -> Vec<u8> {
    let options = hod::Options {
        output_kind: OutputKind::Object,
        split_dwarf_file: split.then(|| "declared.dwo".into()),
        ..hod::Options::default()
    };
    compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

fn run_ok(command: &mut Command, what: &str) -> String {
    let out = run_with(command, b"", Duration::from_mins(2));
    assert!(out.success(), "{what}: {}", out.stderr_text());
    out.stdout_text()
}

/// Asserts that `object` verifies under every `llvm-dwarfdump` there is.
fn verify(object: &Path) {
    for dump in ["llvm-dwarfdump-19", "llvm-dwarfdump-22"].into_iter().filter_map(tool) {
        let out = run_ok(Command::new(&dump).arg("--verify").arg(object), "verify");
        assert!(out.contains("No errors."), "{} by {}: {out}", object.display(), dump.display());
    }
}

/// One variable DIE of a dump: its location (`None`: no attribute), and
/// its list's entries `(start, end, operations)` when it is a list.
#[derive(Debug, Default)]
struct Var {
    location: Option<String>,
    entries: Vec<(u64, u64, String)>,
}

/// One subprogram: its code range and its variables by name.
#[derive(Debug, Default)]
struct Func {
    low: u64,
    high: u64,
    vars: BTreeMap<String, Var>,
}

fn hex(s: &str) -> u64 {
    u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).unwrap_or(u64::MAX)
}

/// The subprograms of `llvm-dwarfdump --debug-info` output by linkage
/// name (else name), the second of a name (`CoroSplit`'s clones keep the
/// ramp's) as `name#1`, and so on. Variables of nested scopes are the
/// subprogram's.
fn functions(dump: &str) -> BTreeMap<String, Func> {
    let mut out = BTreeMap::new();
    let insert = |out: &mut BTreeMap<String, Func>, name: String, f: Func| {
        let mut key = name.clone();
        let mut n = 0;
        while out.contains_key(&key) {
            n += 1;
            key = format!("{name}#{n}");
        }
        out.insert(key, f);
    };
    let mut func: Option<(String, Func)> = None;
    let mut var: Option<Var> = None;
    let mut in_sub = false;
    let flush_var = |func: &mut Option<(String, Func)>, var: &mut Option<Var>, name: &str| {
        if let (Some((_, f)), Some(v)) = (func.as_mut(), var.take()) {
            f.vars.insert(name.to_owned(), v);
        }
    };
    let mut var_name = String::new();
    for line in dump.lines() {
        let t = line.trim();
        if t.starts_with("0x") && (t.contains("DW_TAG_") || t.ends_with("NULL")) {
            flush_var(&mut func, &mut var, &var_name);
            var_name.clear();
            in_sub = false;
            if t.ends_with("DW_TAG_subprogram") {
                if let Some((name, f)) = func.take() {
                    insert(&mut out, name, f);
                }
                func = Some((String::new(), Func::default()));
                in_sub = true;
            } else if t.ends_with("DW_TAG_variable") || t.ends_with("DW_TAG_formal_parameter") {
                var = Some(Var::default());
            }
            continue;
        }
        let value = |key: &str| {
            t.strip_prefix(key).map(|v| v.trim().trim_start_matches('(').trim_end_matches(')'))
        };
        if in_sub && let Some((name, f)) = func.as_mut() {
            if let Some(v) = value("DW_AT_low_pc") {
                f.low = hex(v);
            } else if let Some(v) = value("DW_AT_high_pc") {
                f.high = hex(v);
            } else if let Some(v) = value("DW_AT_linkage_name") {
                v.trim_matches('"').clone_into(name);
            } else if let Some(v) = value("DW_AT_name")
                && name.is_empty()
            {
                v.trim_matches('"').clone_into(name);
            }
        }
        if let Some(v) = var.as_mut() {
            if let Some(l) = value("DW_AT_location") {
                v.location = Some(l.to_owned());
            } else if let Some(n) = value("DW_AT_name") {
                n.trim_matches('"').clone_into(&mut var_name);
            } else if let Some(entry) = t.strip_prefix('[')
                && let Some((range, ops)) = entry.split_once("): ")
                && let Some((a, b)) = range.split_once(", ")
            {
                let ops = ops.trim_end_matches(')').to_owned();
                v.entries.push((hex(a), hex(b), ops));
            }
        }
    }
    flush_var(&mut func, &mut var, &var_name);
    if let Some((name, f)) = func {
        insert(&mut out, name, f);
    }
    out
}

fn dump(object: &Path) -> String {
    let dwarfdump = tool("llvm-dwarfdump").expect("llvm-dwarfdump");
    run_ok(Command::new(dwarfdump).arg("--debug-info").arg(object), "dwarfdump")
}

/// The names of the variables of `f` whose location is a list.
fn listed(f: &Func) -> Vec<&str> {
    f.vars.iter().filter(|(_, v)| !v.entries.is_empty()).map(|(n, _)| n.as_str()).collect()
}

/// The section names of `object`.
fn sections(object: &Path) -> String {
    let readelf = tool("llvm-readelf").expect("llvm-readelf");
    run_ok(Command::new(readelf).arg("-S").arg(object), "readelf")
}

fn llc_object(llc: &Path, scratch: &Scratch, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let object = scratch.join(format!("{name}.llc.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-fast-isel=1", "-filetype=obj"]).arg(&input).arg("-o").arg(&object);
    run_ok(&mut command, "llc");
    object
}

/// §6.30 on hand-written IR: every kind of declaration, both targets,
/// DWARF 5, 4 and split.
#[test]
fn declared_values_are_lists_from_their_definition() {
    if tool("llvm-dwarfdump").is_none() || tool("llvm-readelf").is_none() {
        return;
    }
    let scratch = Scratch::new("dwarf-declared-value");
    let mut checked = 0;
    for (target, base) in [("aarch64", DECLARED.to_owned()), ("x86_64", x86_64(DECLARED))] {
        for (shape, text, split) in
            [("5", base.clone(), false), ("4", dwarf4(&base), false), ("split", split(&base), true)]
        {
            let name = format!("{target}-{shape}");
            let object = scratch.write(&format!("{name}.o"), hod_object(text.as_bytes(), split));
            verify(&object);
            let names = sections(&object);
            match shape {
                "4" => assert!(names.contains(".debug_loc ") && !names.contains("loclists")),
                "split" => assert!(names.contains(".debug_loclists.dwo")),
                _ => assert!(names.contains(".debug_loclists ")),
            }
            let funcs = functions(&dump(&object));
            let f = &funcs["f"];
            let what = format!("{name}: {f:?}");
            // The slot is kept; the dead result has no DIE.
            assert!(f.vars["both"].entries.is_empty(), "{what}");
            assert!(
                f.vars["both"].location.as_deref().is_some_and(|l| l.starts_with("DW_OP_fbreg"))
            );
            assert!(!f.vars.contains_key("dead"), "{what}");
            assert_eq!(listed(f), ["deref", "gep", "plus"], "{what}");
            for (var, tail) in [
                ("gep", "DW_OP_deref, DW_OP_plus_uconst 0x8"),
                ("deref", "DW_OP_deref, DW_OP_deref"),
                ("plus", "DW_OP_deref, DW_OP_plus_uconst 0x10"),
            ] {
                for &(start, end, ref ops) in &f.vars[var].entries {
                    // Never the prologue, where nothing is stored yet;
                    // never past the function's code.
                    assert!(f.low < start && start < end && end <= f.high, "{what}: {var}");
                    assert!(ops.starts_with("DW_OP_fbreg ") && ops.ends_with(tail), "{what}");
                }
            }
            // The loop's `phi` is defined after `gep`: its ranges start
            // later.
            assert!(f.vars["gep"].entries[0].0 < f.vars["plus"].entries[0].0, "{what}");
            checked += 1;
        }
        // `llc`'s default `-O0` selectors describe the same variables by
        // lists (their ranges are their own code's).
        for llc in ["llc-19", "llc-22"].into_iter().filter_map(tool) {
            let theirs = functions(&dump(&llc_object(&llc, &scratch, target, &base)));
            let ours = hod_object(base.as_bytes(), false);
            let ours = functions(&dump(&scratch.write(&format!("{target}-ours.o"), ours)));
            assert_eq!(listed(&ours["f"]), listed(&theirs["f"]), "{target}, {}", llc.display());
        }
    }
    assert_eq!(checked, 6);
}

/// The test coroutine of `tests/debug/` through clang 19 and 22 at `-O0`
/// (which runs `CoroSplit`): the ramp declares the frame's variables on
/// the frame pointer's `phi`; hod's lists are on the variables `llc`'s
/// default selectors give lists, on both targets; DWARF 4 too.
#[test]
fn clang_coroutine_ramps() {
    if tool("llvm-dwarfdump").is_none() {
        return;
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/debug/coroutine.cpp");
    let scratch = Scratch::new("dwarf-declared-value-coro");
    let mut ramps = 0;
    for major in ["19", "22"] {
        let (Some(clang), Some(llc)) =
            (tool(&format!("clang++-{major}")), tool(&format!("llc-{major}")))
        else {
            continue;
        };
        for triple in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"] {
            for version in ["-gdwarf-5", "-gdwarf-4"] {
                let name = format!("{major}-{triple}{version}");
                let ll = scratch.join(format!("{name}.ll"));
                let mut command = Command::new(&clang);
                command.arg(format!("--target={triple}"));
                command.args(["-std=c++20", "-O0", "-g", version, "-S", "-emit-llvm"]);
                command.arg(&source).arg("-o").arg(&ll);
                run_ok(&mut command, "clang");
                let text = std::fs::read_to_string(&ll).unwrap();
                assert!(text.contains(".resume"), "{name}: CoroSplit has run");
                let object =
                    scratch.write(&format!("{name}.o"), hod_object(text.as_bytes(), false));
                verify(&object);
                let ours = functions(&dump(&object));
                let ramp = &ours["_Z5counti"];
                let lists = listed(ramp);
                for var in ["__coro_frame", "__promise", "local", "i"] {
                    assert!(lists.contains(&var), "{name}: {var} in {:?}", ramp.vars);
                }
                for &(start, end, _) in ramp.vars.values().flat_map(|v| &v.entries) {
                    assert!(ramp.low < start && start < end && end <= ramp.high, "{name}");
                }
                // The parameter keeps its slot.
                assert!(ramp.vars["n"].entries.is_empty(), "{name}: {:?}", ramp.vars["n"]);
                let theirs = functions(&dump(&llc_object(&llc, &scratch, &name, &text)));
                for (func, f) in theirs.iter().filter(|(func, _)| !func.is_empty()) {
                    let Some(o) = ours.get(func) else { continue };
                    assert_eq!(listed(o), listed(f), "{name}: {func}");
                }
                ramps += 1;
            }
        }
    }
    eprintln!("clang_coroutine_ramps: {ramps} modules");
}
