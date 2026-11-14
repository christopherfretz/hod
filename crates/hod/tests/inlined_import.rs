//! WP-230 (`spec/dwarf.md` §6.35): an imported declaration (`using
//! n::map;`) whose entity is a function definition the module only
//! inlines was refused as `debug:import-entity`; one whose entity is
//! also placed out of line named the concrete DIE. `llc` 22's
//! `constructImportedEntityDIE` names the entity's abstract instance
//! whenever there is one (`getAbstractScopeDIEs().lookup(SP)`). WP-236
//! (§6.36): one whose entity is neither placed nor inlined was refused
//! too; `llc` 22 makes the entity's abstract instance, without children.
//!
//! - `trees_equal_llc`: the three shapes, both targets: the DIE tree (code
//!   addresses and frame offsets masked, references by name) equals
//!   `llc` 22's, both objects `--verify` clean, and the import names the
//!   abstract instance.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

/// `map`, placed out of line (the second shape only).
const MAP: &str = r"define i32 @map(i32 %x) !dbg !21 {
  %y = add i32 %x, 1, !dbg !25
  ret i32 %y, !dbg !25
}
";

/// `h`, which inlines `map`.
const H: &str = r"define i32 @h(i32 %a) !dbg !53 {
  %y = add i32 %a, 1, !dbg !59
  ret i32 %y, !dbg !56
}
";

const META: &str = r#"!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!9, !10}
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "hand", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug, imports: !70)
!3 = !DIFile(filename: "ii.cpp", directory: "/m")
!6 = !DISubroutineType(types: !7)
!7 = !{!8, !8}
!8 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!9 = !{i32 7, !"Dwarf Version", i32 5}
!10 = !{i32 2, !"Debug Info Version", i32 3}
!11 = !DINamespace(name: "n", scope: null)
!21 = distinct !DISubprogram(name: "map", linkageName: "map", scope: !11, file: !3, line: 2, type: !6, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, retainedNodes: !22)
!22 = !{!23}
!23 = !DILocalVariable(name: "x", arg: 1, scope: !21, file: !3, line: 2, type: !8)
!24 = !DILocation(line: 0, scope: !21)
!25 = !DILocation(line: 3, column: 3, scope: !21)
!53 = distinct !DISubprogram(name: "h", linkageName: "h", scope: !3, file: !3, line: 13, type: !6, scopeLine: 13, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, retainedNodes: !54)
!54 = !{!55}
!55 = !DILocalVariable(name: "a", arg: 1, scope: !53, file: !3, line: 13, type: !8)
!56 = !DILocation(line: 0, scope: !53)
!57 = !DILocation(line: 0, scope: !21, inlinedAt: !58)
!58 = distinct !DILocation(line: 13, column: 23, scope: !53)
!59 = !DILocation(line: 3, column: 3, scope: !21, inlinedAt: !58)
!70 = !{!71}
!71 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !2, entity: !21, file: !3, line: 9)
"#;

/// `h`, which does not inline `map` (the third shape, WP-236).
const H_ALONE: &str = r"define i32 @h(i32 %a) !dbg !53 {
  %y = add i32 %a, 1, !dbg !56
  ret i32 %y, !dbg !56
}
";

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// The shapes: `map` inlined only, also placed out of line, or (WP-236,
/// `spec/dwarf.md` §6.36) neither placed nor inlined: `llc` 22 makes its
/// abstract instance for the import, without its retained parameter.
const SHAPES: [&str; 3] = ["inlined-only", "also-out-of-line", "imported-only"];

fn module(triple: &str, shape: &str) -> String {
    let map = if shape == "also-out-of-line" { MAP } else { "" };
    let h = if shape == "imported-only" { H_ALONE } else { H };
    format!("target triple = \"{triple}\"\n{map}{h}{META}")
}

fn compile(text: &str) -> Vec<u8> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("{e}"))
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The values `spec/dwarf.md` §6.7 masks: code addresses and sizes,
/// section bases, and locations.
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

/// The DIE tree of `object`, one line per DIE or attribute; a reference
/// by the tag and name of its target.
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    // DIE offset -> (tag, whether it has `DW_AT_inline`).
    let mut tags = std::collections::HashMap::new();
    let mut last = None;
    for line in text.lines() {
        if let Some((addr, rest)) = line.split_once(':')
            && addr.starts_with("0x")
            && !addr.contains(' ')
        {
            last = Some(addr.to_owned());
            tags.insert(addr.to_owned(), (rest.trim().to_owned(), false));
        } else if line.trim_start().starts_with("DW_AT_inline")
            && let Some(t) = last.as_ref().and_then(|a| tags.get_mut(a))
        {
            t.1 = true;
        }
    }
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
        // `(0x0000004c "map")`: the target's tag, abstract or not, name.
        let v = match value.strip_prefix('(').and_then(|r| r.split_once(' ')) {
            Some((offset, named)) if offset.starts_with("0x") && named.starts_with('"') => {
                let key = format!("0x{:08x}", u64::from_str_radix(&offset[2..], 16).unwrap_or(0));
                let (tag, inline) = tags.get(&key).cloned().unwrap_or_default();
                format!("{tag}{} {named}", if inline { " (abstract)" } else { "" })
            }
            _ => {
                // A number as a number, whatever its form.
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
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

#[test]
fn trees_equal_llc() {
    let (Some(llc), Some(dwarfdump)) = (tool("llc-22"), tool("llvm-dwarfdump-22")) else {
        assert!(!tools_required(), "llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("inlined_import");
    for triple in TRIPLES {
        for shape in SHAPES {
            let name = format!("{}-{shape}", &triple[..triple.find('-').unwrap()]);
            let text = module(triple, shape);
            let ours = scratch.write(&format!("{name}.hod.o"), compile(&text));
            let input = scratch.write(&format!("{name}.ll"), &text);
            let theirs = scratch.join(format!("{name}.llc.o"));
            let mut command = Command::new(&llc);
            command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
            if triple.starts_with("aarch64") {
                command.arg("-global-isel=0");
            }
            output(command.arg(&input).arg("-o").arg(&theirs));
            verify(&dwarfdump, &ours);
            verify(&dwarfdump, &theirs);
            let (h, l) = (tree(&dwarfdump, &ours), tree(&dwarfdump, &theirs));
            assert_eq!(h.join("\n"), l.join("\n"), "{name}: hod's tree against llc's");
            let import = h.iter().position(|l| l.ends_with("DW_TAG_imported_declaration"));
            let entity = import.and_then(|i| h[i..].iter().find(|l| l.contains("DW_AT_import")));
            assert_eq!(
                entity.map(String::as_str),
                Some("    DW_AT_import DW_TAG_subprogram (abstract) \"map\")"),
                "{name}"
            );
        }
    }
}
