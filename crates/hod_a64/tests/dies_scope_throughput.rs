//! What WP-059's DIEs cost (`spec/dwarf.md` §6.9, §7.2): a generated
//! module of `N` class template instances `C<i>`, each with a type and a
//! value parameter and a static member, the member's definition (a
//! global with `DW_AT_specification`), and a `using` declaration of it
//! in the unit: six DIEs per instance.
//!
//! The gate test emits it at `N` = 20,000 and counts the DIEs (no time
//! is measured). The measurement (ignored) times the module emission at
//! `N` and `2N`, best of five, with and without debug information, and
//! prints ns per DIE and the ratio of the two:
//!
//! ```text
//! cargo test --release -p hod_a64 --test dies_scope_throughput -- --ignored --nocapture
//! ```

mod common;

use std::fmt::Write as _;
use std::time::Instant;

use common::{ll_header, parse};
use hod_cg::{Chunk, Options, emit_module};

const N: usize = 20_000;

/// The DIEs per instance: the definition, the class, its member, `T`,
/// `N` and the import.
const PER: usize = 6;

fn module(n: usize) -> String {
    let (mut code, mut meta, mut globals, mut imports) =
        (String::new(), String::new(), String::new(), String::new());
    for i in 0..n {
        let b = 10 + 10 * i;
        writeln!(code, "@g{i} = global i32 {i}, align 4, !dbg !{b}").unwrap();
        writeln!(
            meta,
            "!{b} = !DIGlobalVariableExpression(var: !{}, expr: !DIExpression())\n\
             !{} = distinct !DIGlobalVariable(name: \"s\", linkageName: \"_ZN1CILi{i}EE1sE\", scope: !0, file: !1, line: 3, type: !6, isLocal: false, isDefinition: true, declaration: !{})\n\
             !{} = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"C<{i}>\", file: !1, line: 1, size: 8, elements: !{}, templateParams: !{}, identifier: \"_ZTS1CILi{i}EE\")\n\
             !{} = !{{!{}}}\n\
             !{} = !DIDerivedType(tag: DW_TAG_variable, name: \"s\", scope: !{}, file: !1, line: 2, baseType: !6, flags: DIFlagStaticMember)\n\
             !{} = !{{!{}, !{}}}\n\
             !{} = !DITemplateTypeParameter(name: \"T\", type: !6)\n\
             !{} = !DITemplateValueParameter(name: \"N\", type: !6, value: i32 {i})\n\
             !{} = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !0, entity: !{}, file: !1, line: 4)",
            b + 1,
            b + 1,
            b + 4,
            b + 2,
            b + 3,
            b + 5,
            b + 3,
            b + 4,
            b + 4,
            b + 2,
            b + 5,
            b + 6,
            b + 7,
            b + 6,
            b + 7,
            b + 8,
            b + 1,
        )
        .unwrap();
        let sep = if i == 0 { "" } else { ", " };
        write!(globals, "{sep}!{b}").unwrap();
        write!(imports, "{sep}!{}", b + 8).unwrap();
    }
    format!(
        "{}{code}\n!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!2, !3}}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"gen\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !4, imports: !5, splitDebugInlining: false, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"gen.cpp\", directory: \"/hod/gen\")\n\
         !2 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n!3 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !4 = !{{{globals}}}\n!5 = !{{{imports}}}\n\
         !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n{meta}",
        ll_header(22)
    )
}

/// The module emission (which writes the DIEs), best of `runs`.
fn emit_timed(text: &str, runs: usize) -> (f64, Vec<u8>) {
    let (module, bodies) = parse(text);
    assert!(bodies.is_empty());
    let options = Options::default();
    let chunks = [Chunk::new()];
    let mut best = f64::MAX;
    let mut bytes = Vec::new();
    for _ in 0..runs {
        let start = Instant::now();
        let emitted = emit_module(&module, &options, &chunks).unwrap();
        best = best.min(start.elapsed().as_secs_f64());
        bytes = hod_obj::elf::to_vec(&emitted.object).unwrap();
    }
    (best, bytes)
}

/// At `N` the module is written; its DIEs are counted with
/// `llvm-dwarfdump` when there is one.
#[test]
fn twenty_thousand_scale() {
    let (_, bytes) = emit_timed(&module(N), 1);
    assert!(!bytes.is_empty());
    let Some(dwarfdump) = hod_testutil::tool("llvm-dwarfdump") else { return };
    let scratch = hod_testutil::Scratch::new("dies-scope-scale");
    let path = scratch.write("o.o", &bytes);
    let out =
        hod_testutil::run(&dwarfdump, [std::ffi::OsString::from("--debug-info"), path.into()]);
    assert!(out.success(), "{}", out.stderr_text());
    let text = out.stdout_text();
    let count = |tag: &str| text.lines().filter(|l| l.trim_end().ends_with(tag)).count();
    for tag in [
        "DW_TAG_structure_type",
        "DW_TAG_template_type_parameter",
        "DW_TAG_template_value_parameter",
        "DW_TAG_imported_declaration",
    ] {
        assert_eq!(count(tag), N, "{tag}");
    }
    // The members' declarations and the definitions.
    assert_eq!(count("DW_TAG_variable"), 2 * N);
    assert_eq!(text.matches("DW_AT_specification").count(), N);
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn scope_die_cost() {
    let mut per = Vec::new();
    for n in [N, 2 * N] {
        let text = module(n);
        let (with, bytes) = emit_timed(&text, 5);
        let bare = text.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
        let (without, _) = emit_timed(&bare, 5);
        let dies = PER * n;
        let ns = (with - without).max(0.0) * 1e9 / dies as f64;
        per.push(ns);
        eprintln!(
            "n={n}: emission {:.2} ms with DIEs, {:.2} ms without; {ns:.0} ns per DIE; object {} bytes",
            with * 1e3,
            without * 1e3,
            bytes.len()
        );
    }
    eprintln!("ns per DIE at 2N / at N = {:.2}", per[1] / per[0].max(1e-9));
}
