//! `spec/ll.md` §4.5: the debug nodes of a large text are lexed ahead
//! on a granted thread while the calling one reads them, and the object
//! is the same bytes at every thread count and from run to run.

use std::fmt::Write as _;

use hod::{Options, OutputKind, Report, Settings, Threads};
use hod_ll::SCAN_BYTES_PER_THREAD;

/// A module of `count` functions whose debug metadata is most of the
/// text: per function a subprogram, a variable, locations (one written
/// in place), a structure with members whose element tuple stands
/// before it, a retained-nodes tuple after it, and strings with escapes;
/// the compile unit names a tuple at the very end.
fn module(count: usize) -> String {
    let mut out = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
    for index in 0..count {
        let sp = 20 + 10 * index;
        let _ = write!(
            out,
            "define i32 @f{index}(i32 %a) !dbg !{sp} {{\nentry:\n  \
             #dbg_value(i32 %a, !{var}, !DIExpression(), !{loc})\n  \
             %r = add i32 %a, {index}, !dbg !{loc}\n  ret i32 %r, !dbg !{loc}\n}}\n",
            var = sp + 1,
            loc = sp + 2
        );
    }
    let last = 20 + 10 * count;
    let _ = write!(
        out,
        "!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!1, !2}}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, \
         producer: \"lex \\22ahead\\22\", isOptimized: false, runtimeVersion: 0, \
         emissionKind: FullDebug, retainedTypes: !{last})\n\
         !1 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
         !2 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !3 = !DIFile(filename: \"ahead.cpp\", directory: \"/ahead\")\n\
         !4 = !DISubroutineType(types: !5)\n!5 = !{{!6, !6}}\n\
         !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n"
    );
    for index in 0..count {
        let sp = 20 + 10 * index;
        let line = index + 1;
        let _ = write!(
            out,
            "!{sp} = distinct !DISubprogram(name: \"f{index}\", linkageName: \"_Z1f{index}i\", \
             scope: !3, file: !3, line: {line}, type: !4, scopeLine: {line}, \
             spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !{nodes})\n\
             !{var} = !DILocalVariable(name: \"a\", arg: 1, scope: !{sp}, file: !3, \
             line: {line}, type: !6)\n\
             !{loc} = !DILocation(line: {line}, column: 7, scope: !{block})\n\
             ; a comment between nodes\n\
             !{elems} = !{{!{m1}, !{m2}}}\n\
             !{ty} = distinct !DICompositeType(tag: DW_TAG_structure_type, \
             name: \"S{index}\\09x\", file: !3, line: {line}, size: 64, elements: !{elems}, \
             identifier: \"_ZTS1S{index}\")\n\
             !{m1} = !DIDerivedType(tag: DW_TAG_member, name: \"x\", scope: !{ty}, \
             baseType: !6, size: 32)\n\
             !{m2} = !DIDerivedType(tag: DW_TAG_member, name: \"y\", scope: !{ty}, \
             baseType: !6, size: 32, offset: 32)\n\
             !{nodes} = !{{!{var}}}\n\
             !{block} = distinct !DILexicalBlock(scope: !{sp}, file: !3, line: {line}, \
             column: 1)\n",
            var = sp + 1,
            loc = sp + 2,
            elems = sp + 3,
            ty = sp + 4,
            m1 = sp + 5,
            m2 = sp + 6,
            nodes = sp + 7,
            block = sp + 8,
        );
    }
    let types: Vec<String> = (0..count).map(|index| format!("!{}", 24 + 10 * index)).collect();
    let _ = writeln!(out, "!{last} = !{{{}}}", types.join(", "));
    out
}

fn compile(input: &str, max: usize) -> Vec<u8> {
    let settings =
        Settings { threads: Threads { max, ..Threads::default() }, ..Settings::default() };
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input.as_bytes(), &options, &settings, &mut Report::default())
        .expect("the module compiles")
}

#[test]
fn the_object_is_the_same_with_the_debug_nodes_lexed_ahead() {
    let text = module(4500);
    assert!(text.len() > 4 * SCAN_BYTES_PER_THREAD, "large enough for threads");
    let metadata = text.find("!llvm.dbg.cu").expect("the metadata");
    assert!(metadata < text.len() / 3, "mostly debug metadata");
    let alone = compile(&text, 1);
    assert!(alone.windows(11).any(|w| w == b".debug_info"), "with debug information");
    for max in [2, 3, 4, 8, 24] {
        assert!(compile(&text, max) == alone, "{max} threads: the same bytes");
    }
    for run in 0..3 {
        assert!(compile(&text, 2) == alone, "run {run} at two threads: the same bytes");
    }
}
