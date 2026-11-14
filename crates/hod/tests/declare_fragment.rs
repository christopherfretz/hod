//! WP-230 (`spec/dwarf.md` §6.32): a variable declared on slots in
//! fragments (`#dbg_declare(ptr %a, !v, !DIExpression(DW_OP_LLVM_fragment,
//! o, s))`, which SROA leaves when it splits part of an aggregate off) is
//! one location of pieces, by fragment offset, a `DW_OP_piece` of the
//! bytes no declaration covers before each, nothing after the last; its
//! value records are not used (`llc` keeps the frame table's entry). A
//! fragment declared at `poison` is dropped, and the variable's value
//! records describe it.
//!
//! - `locations_are_pieces`: both targets, each variable's location with
//!   frame offsets masked, as `llc` 19 and 22 write it in all three
//!   selectors (measured on this module);
//! - `locations_equal_llc`: the same against `llc` 22 (`-O0
//!   -fast-isel=0`, `-global-isel=0` on AArch64), both objects
//!   `--verify` clean;
//! - `refusals`: a fragment of bits, and a fragment beside a whole
//!   declaration of the same variable, stay `debug:fragment`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const MODULE: &str = r#"target triple = "TRIPLE"
declare void @g(i64)
declare void @h(ptr)
define i32 @main(i32 %n) !dbg !4 {
entry:
  %a = alloca [8 x i8], align 4
  %b = alloca [8 x i8], align 4
  %c = alloca [12 x i8], align 4
  %e = alloca [4 x i8], align 4
    #dbg_declare(ptr %a, !7, !DIExpression(DW_OP_LLVM_fragment, 0, 64), !11)
    #dbg_declare(ptr %b, !7, !DIExpression(DW_OP_LLVM_fragment, 64, 64), !11)
    #dbg_declare(ptr %c, !12, !DIExpression(DW_OP_LLVM_fragment, 32, 96), !11)
    #dbg_declare(ptr poison, !13, !DIExpression(DW_OP_LLVM_fragment, 32, 96), !11)
    #dbg_declare(ptr %e, !14, !DIExpression(DW_OP_LLVM_fragment, 64, 32), !11)
  call void @h(ptr %a), !dbg !11
  call void @h(ptr %b), !dbg !11
  call void @h(ptr %c), !dbg !11
  call void @h(ptr %e), !dbg !11
  %m = add i32 %n, 1, !dbg !11
    #dbg_value(i32 %m, !12, !DIExpression(DW_OP_LLVM_fragment, 0, 32), !11)
    #dbg_value(i32 %m, !13, !DIExpression(DW_OP_LLVM_fragment, 0, 32), !11)
  call void @g(i64 1), !dbg !11
  %z = zext i32 %m to i64, !dbg !11
  call void @g(i64 %z), !dbg !11
  ret i32 %m, !dbg !11
}
!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3, !20}
!20 = !{i32 7, !"Dwarf Version", i32 5}
!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "hand", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "fd.c", directory: "/m")
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 20, type: !6, scopeLine: 21, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !0, retainedNodes: !2)
!2 = !{}
!6 = !DISubroutineType(types: !2)
!7 = !DILocalVariable(name: "two", scope: !4, file: !1, line: 24, type: !9)
!9 = !DICompositeType(tag: DW_TAG_array_type, baseType: !10, size: 128, elements: !2)
!10 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!11 = !DILocation(line: 24, column: 11, scope: !4)
!12 = !DILocalVariable(name: "mix", scope: !4, file: !1, line: 25, type: !9)
!13 = !DILocalVariable(name: "pmix", scope: !4, file: !1, line: 26, type: !9)
!14 = !DILocalVariable(name: "part", scope: !4, file: !1, line: 27, type: !9)
"#;

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// Each variable's location as `llc` writes it, frame offsets masked;
/// `pmix` has a list (the slots' variables have expressions).
const EXPECTED: [(&str, &str); 4] = [
    ("two", "(DW_OP_fbreg o, DW_OP_piece 0x8, DW_OP_fbreg o, DW_OP_piece 0x8)"),
    ("mix", "(DW_OP_piece 0x4, DW_OP_fbreg o, DW_OP_piece 0xc)"),
    ("part", "(DW_OP_piece 0x8, DW_OP_fbreg o, DW_OP_piece 0x4)"),
    ("pmix", "list"),
];

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// `(name, location)` of each variable of `object`, frame offsets masked
/// and a location list as `list`.
fn locations(dwarfdump: &Path, object: &Path) -> Vec<(String, String)> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    let mut location = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("DW_AT_location") {
            let rest = rest.trim();
            // A list by index (`llc`) or by offset (the form is each
            // writer's own, §6.7).
            let list = rest.contains("loclist") || rest.ends_with(':');
            location = Some(if list { "list".to_owned() } else { mask(rest) });
        } else if let Some(rest) = line.strip_prefix("DW_AT_name")
            && let Some(at) = location.take()
        {
            out.push((
                rest.trim().trim_matches(|c| c == '(' || c == ')' || c == '"').to_owned(),
                at,
            ));
        } else if line.starts_with("DW_TAG_") || line.contains(": DW_TAG_") {
            location = None;
        }
    }
    out.sort();
    out
}

/// `DW_OP_fbreg N` as `DW_OP_fbreg o`.
fn mask(location: &str) -> String {
    let mut out = String::new();
    let mut rest = location;
    while let Some(i) = rest.find("DW_OP_fbreg ") {
        out.push_str(&rest[..i]);
        out.push_str("DW_OP_fbreg o");
        rest = &rest[i + "DW_OP_fbreg ".len()..];
        rest = &rest[rest.find([',', ')']).unwrap_or(rest.len())..];
    }
    out.push_str(rest);
    out
}

fn expected() -> Vec<(String, String)> {
    let mut v: Vec<_> = EXPECTED.iter().map(|&(n, l)| (n.to_owned(), l.to_owned())).collect();
    v.sort();
    v
}

#[test]
fn locations_are_pieces() {
    let Some(dwarfdump) = tool("llvm-dwarfdump-22") else {
        assert!(!tools_required(), "llvm-dwarfdump-22 is required");
        eprintln!("note: no llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("declare_fragment_pieces");
    for triple in TRIPLES {
        let object =
            compile(&MODULE.replace("TRIPLE", triple)).unwrap_or_else(|e| panic!("{triple}: {e}"));
        let path = scratch.write(&format!("{}.o", &triple[..6]), object);
        assert_eq!(locations(&dwarfdump, &path), expected(), "{triple}");
    }
}

fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

#[test]
fn locations_equal_llc() {
    let (Some(llc), Some(dwarfdump)) = (tool("llc-22"), tool("llvm-dwarfdump-22")) else {
        assert!(!tools_required(), "llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("declare_fragment_llc");
    for triple in TRIPLES {
        let name = &triple[..triple.find('-').unwrap()];
        let text = MODULE.replace("TRIPLE", triple);
        let ours = scratch.write(&format!("{name}.hod.o"), compile(&text).unwrap());
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
        assert_eq!(
            locations(&dwarfdump, &ours),
            locations(&dwarfdump, &theirs),
            "{name}: hod's locations against llc's"
        );
    }
}

#[test]
fn refusals() {
    let cases = [
        // A fragment of bits (`llc` writes `DW_OP_bit_piece`).
        (
            "DW_OP_LLVM_fragment, 64, 32), !11)\n  call",
            "DW_OP_LLVM_fragment, 64, 28), !11)\n  call",
        ),
        // A whole declaration of `part` beside its fragment.
        (
            "    #dbg_declare(ptr %e, !14,",
            "    #dbg_declare(ptr %b, !14, !DIExpression(), !11)\n    #dbg_declare(ptr %e, !14,",
        ),
        // Two fragments of `two` that overlap.
        ("DW_OP_LLVM_fragment, 64, 64)", "DW_OP_LLVM_fragment, 32, 64)"),
    ];
    for (from, to) in cases {
        let text = MODULE.replace("TRIPLE", TRIPLES[1]);
        assert!(text.contains(from), "{from}");
        match compile(&text.replace(from, to)) {
            Err(CompileError::Unsupported(u)) => match u.construct {
                hod::Construct::Named(name) => assert_eq!(name, "debug:fragment", "{to}"),
                other => panic!("{to}: declined, but not by name: {other:?}"),
            },
            other => panic!("{to}: expected a decline, got {:?}", other.map(|o| o.len())),
        }
    }
}
