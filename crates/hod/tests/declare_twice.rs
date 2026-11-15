//! WP-236 (`spec/dwarf.md` §6.36): a variable of an inlined function
//! declared twice, in two blocks, at the same pointer argument (an
//! `sret` result, kiln `-O2` of libc++'s `__format_buffer`, where the
//! inlined body is duplicated) was refused as `debug:variable-twice`.
//! Both records describe the variable alike; `llc` 22 writes one DIE
//! located through the pointer's spill slot, as for one record. hod
//! keeps the first record and skips the second.
//!
//! - `same_pointer_twice_matches_llc`: both targets: the DIE tree
//!   (locations masked) equals `llc` 22's, equals hod's for one record,
//!   one variable DIE, located through a stored pointer, `--verify` clean.
//! - `different_pointers_stay_refused`: the two records at different
//!   pointers: `llc` 22 writes one location for the whole scope, wrong in
//!   one of the blocks; hod still refuses it.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

/// `g` inlines `f`, whose `r` lives where `%p` points, on both paths.
const TWICE: &str = r#"target triple = "TRIPLE"
define void @g(ptr %p, ptr %q, i1 %c) !dbg !4 {
  br i1 %c, label %a, label %b, !dbg !18
a:
    #dbg_declare(ptr %p, !10, !DIExpression(), !14)
  store i32 1, ptr %p, align 4, !dbg !17
  ret void, !dbg !18
b:
    #dbg_declare(ptr %p, !10, !DIExpression(), !14)
  store i32 2, ptr %p, align 4, !dbg !17
  ret void, !dbg !18
}
!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}
!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "hand", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "t.cpp", directory: "/m")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = distinct !DISubprogram(name: "g", linkageName: "g", scope: !1, file: !1, line: 10, type: !5, scopeLine: 10, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!5 = !DISubroutineType(types: !6)
!6 = !{null}
!7 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!10 = !DILocalVariable(name: "r", scope: !11, file: !1, line: 3, type: !7)
!11 = distinct !DISubprogram(name: "f", linkageName: "f", scope: !1, file: !1, line: 2, type: !5, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0)
!14 = !DILocation(line: 3, column: 7, scope: !11, inlinedAt: !15)
!15 = distinct !DILocation(line: 11, column: 3, scope: !4)
!17 = !DILocation(line: 4, column: 3, scope: !11, inlinedAt: !15)
!18 = !DILocation(line: 12, column: 1, scope: !4)
"#;

const SECOND: &str =
    "b:\n    #dbg_declare(ptr %p, !10, !DIExpression(), !14)\n  store i32 2, ptr %p,";

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

fn module(triple: &str) -> String {
    TWICE.replace("TRIPLE", triple)
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The values `spec/dwarf.md` §6.7 masks: code addresses and sizes,
/// section bases, and locations (frames are laid out otherwise).
const MASKED: [&str; 8] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_location",
    "DW_AT_frame_base",
    "DW_AT_addr_base",
    "DW_AT_str_offsets_base",
    "DW_AT_stmt_list",
    "DW_AT_producer",
];

/// The DIE tree of `object`, one line per DIE or attribute: offsets and
/// masked values left out, a number as a number whatever its form.
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((addr, rest)) = line.split_once(':')
            && addr.starts_with("0x")
            && !addr.contains(' ')
        {
            let depth = rest.len() - rest.trim_start().len();
            let tag = rest.trim();
            if tag.starts_with("DW_TAG_") || tag == "NULL" {
                out.push(format!("{depth:>3} {tag}"));
            }
            continue;
        }
        if !trimmed.starts_with("DW_AT_") {
            continue;
        }
        let (name, value) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        if MASKED.contains(&name) {
            out.push(format!("    {name}"));
            continue;
        }
        let value = value.trim();
        // `(0x0000004c "r")`: a reference by the target's name.
        let v = match value.strip_prefix('(').and_then(|r| r.split_once(' ')) {
            Some((offset, named)) if offset.starts_with("0x") && named.starts_with('"') => {
                named.to_owned()
            }
            _ => {
                let num = value.strip_prefix('(').and_then(|r| r.strip_suffix(')'));
                let parsed = num.and_then(|n| {
                    n.strip_prefix("0x")
                        .map_or_else(|| n.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok())
                });
                parsed.map_or_else(|| value.to_owned(), |n| format!("({n})"))
            }
        };
        out.push(format!("    {name} {v}"));
    }
    out
}

fn verify(dwarfdump: &Path, object: &Path) {
    let text = output(Command::new(dwarfdump).arg("--verify").arg(object));
    assert!(text.contains("No errors."), "{}: {text}", object.display());
}

fn llc_object(llc: &Path, scratch: &Scratch, name: &str, text: &str) -> std::path::PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let theirs = scratch.join(format!("{name}.llc.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-fast-isel=0", "-global-isel=0", "-relocation-model=pic"]);
    output(command.arg("-filetype=obj").arg(&input).arg("-o").arg(&theirs));
    theirs
}

#[test]
fn same_pointer_twice_matches_llc() {
    let (Some(llc), Some(dwarfdump)) = (tool("llc-22"), tool("llvm-dwarfdump-22")) else {
        assert!(!tools_required(), "llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("declare_twice");
    for triple in TRIPLES {
        let name = &triple[..triple.find('-').unwrap()];
        let text = module(triple);
        assert!(text.contains(SECOND), "{name}: the second record");
        let once = text.replace(SECOND, "b:\n  store i32 2, ptr %p,");
        let ours = scratch.write(&format!("{name}.hod.o"), compile(&text).expect("compiles"));
        let single = scratch.write(&format!("{name}.once.o"), compile(&once).expect("compiles"));
        let theirs = llc_object(&llc, &scratch, name, &text);
        verify(&dwarfdump, &ours);
        verify(&dwarfdump, &theirs);
        let h = tree(&dwarfdump, &ours);
        assert_eq!(h.join("\n"), tree(&dwarfdump, &theirs).join("\n"), "{name}: against llc's");
        assert_eq!(h, tree(&dwarfdump, &single), "{name}: as one record");
        let vars = h.iter().filter(|l| l.ends_with("DW_TAG_variable")).count();
        assert_eq!(vars, 2, "{name}: the abstract variable and one concrete");
        // The place: the stored pointer, dereferenced (unmasked).
        let full = output(Command::new(&dwarfdump).arg("--debug-info").arg(&ours));
        let loc = full.lines().find(|l| l.contains("DW_AT_location")).unwrap_or_default();
        assert!(loc.contains("DW_OP_fbreg") && loc.contains("DW_OP_deref"), "{name}: {loc}");
    }
}

#[test]
fn different_pointers_stay_refused() {
    for triple in TRIPLES {
        let text = module(triple).replace(
            SECOND,
            "b:\n    #dbg_declare(ptr %q, !10, !DIExpression(), !14)\n  store i32 2, ptr %q,",
        );
        match compile(&text) {
            Err(CompileError::Unsupported(u)) => {
                let Construct::Named(name) = u.construct else { panic!("{:?}", u.construct) };
                assert_eq!(name, "debug:variable-twice", "{triple}");
            }
            other => panic!("{triple}: expected a refusal, got {:?}", other.map(|o| o.len())),
        }
    }
}
