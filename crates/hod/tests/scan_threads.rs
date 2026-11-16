//! `spec/ll.md` §4.4 and `spec/cli.md` "Threads": the index scan of a
//! large text runs on the threads granted, and the object is the same
//! bytes at every thread count, debug information included; tokens come
//! from the compile's one request to the pool.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod::{Options, OutputKind, Report, Settings, ThreadTokens, Threads};
use hod_ll::{SCAN_BYTES_PER_THREAD, SCAN_MAX_THREADS, scan_threads};

#[test]
fn the_scanning_policy() {
    let large = 100 * SCAN_BYTES_PER_THREAD;
    assert_eq!(scan_threads(0, 8), 1);
    assert_eq!(scan_threads(large, 1), 1);
    assert_eq!(scan_threads(large, 0), 1);
    assert_eq!(scan_threads(large, 3), 3);
    assert_eq!(scan_threads(large, 64), SCAN_MAX_THREADS);
    assert_eq!(scan_threads(2 * SCAN_BYTES_PER_THREAD - 1, 8), 1);
    assert_eq!(scan_threads(2 * SCAN_BYTES_PER_THREAD, 8), 2);
}

/// A module of `count` small functions, each with its subprogram, its
/// locations and ten debug nodes nothing names: mostly metadata, as a
/// `-g` translation unit is.
fn module(count: usize) -> String {
    let mut out = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
    for index in 0..count {
        let sp = 10 + 12 * index;
        let _ = write!(
            out,
            "define i32 @f{index}(i32 %a) !dbg !{sp} {{\nentry:\n  \
             %r = add i32 %a, {index}, !dbg !{loc}\n  ret i32 %r, !dbg !{loc}\n}}\n",
            loc = sp + 1
        );
    }
    out.push_str(
        "!llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!1, !2}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !3, producer: \"scan test\", \
         isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)\n\
         !1 = !{i32 7, !\"Dwarf Version\", i32 5}\n!2 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
         !3 = !DIFile(filename: \"scan.c\", directory: \"/scan\")\n\
         !4 = !DISubroutineType(types: !5)\n!5 = !{!6, !6}\n\
         !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n",
    );
    for index in 0..count {
        let sp = 10 + 12 * index;
        let line = index + 1;
        let _ = write!(
            out,
            "!{sp} = distinct !DISubprogram(name: \"f{index}\", scope: !3, file: !3, line: {line}, \
             type: !4, scopeLine: {line}, spFlags: DISPFlagDefinition, unit: !0)\n\
             !{loc} = !DILocation(line: {line}, column: 3, scope: !{sp})\n",
            loc = sp + 1
        );
        for extra in 0..10 {
            let _ = writeln!(
                out,
                "!{n} = !DIDerivedType(tag: DW_TAG_pointer_type, name: \"pointer_{index}_{extra}\", \
                 baseType: !6, size: 64)",
                n = sp + 2 + extra
            );
        }
    }
    out
}

fn compile(input: &str, threads: Threads) -> Vec<u8> {
    let settings = Settings { threads, ..Settings::default() };
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input.as_bytes(), &options, &settings, &mut Report::default())
        .expect("the module compiles")
}

/// Counts requests and holds the idle tokens.
#[derive(Debug)]
struct Pool {
    idle: AtomicUsize,
    asked: AtomicUsize,
}

impl ThreadTokens for Pool {
    fn take(&self, wanted: usize) -> usize {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let idle = self.idle.load(Ordering::SeqCst);
        let got = idle.min(wanted);
        self.idle.store(idle - got, Ordering::SeqCst);
        got
    }

    fn give_back(&self, count: usize) {
        self.idle.fetch_add(count, Ordering::SeqCst);
    }
}

#[test]
fn the_object_is_the_same_at_every_thread_count() {
    let text = module(4000);
    assert!(text.len() > 4 * SCAN_BYTES_PER_THREAD, "large enough to scan on four threads");
    let alone = compile(&text, Threads { max: 1, ..Threads::default() });
    assert!(alone.windows(11).any(|w| w == b".debug_info"), "with debug information");
    for max in [2, 3, 4, 8, 24] {
        let object = compile(&text, Threads { max, ..Threads::default() });
        assert!(object == alone, "{max} threads: the same bytes");
    }
    for idle in [0, 1, 3, 7] {
        let pool = Arc::new(Pool { idle: AtomicUsize::new(idle), asked: AtomicUsize::new(0) });
        let threads = Threads { max: 8, tokens: Some(pool.clone()), per_thread: 0 };
        assert!(compile(&text, threads) == alone, "{idle} idle tokens: the same bytes");
        assert_eq!(pool.asked.load(Ordering::SeqCst), 1, "{idle} idle: one request");
        assert_eq!(pool.idle.load(Ordering::SeqCst), idle, "{idle} idle: every token back");
    }
}
