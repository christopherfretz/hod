//! What line tables cost (`spec/dwarf.md` §8.5, §9): a generated module
//! of `FUNCTIONS` functions of `STATEMENTS` statements each (a volatile
//! store per source line, a branch every few lines, an inlined line
//! now and then), compiled and written by `hod_a64::compile_to_object`
//! with its debug information, with its units `NoDebug`, and with none
//! at all; and by `llc -O0` with and without. A measurement, not a gate:
//!
//! ```text
//! cargo test --release -p hod_a64 --test lines_throughput -- --ignored --nocapture
//! ```
//!
//! Time is the best of five in this process (the parse is not timed);
//! peak memory is the peak resident set of a child of this test that
//! parses and compiles once (GNU `time`), and of `llc`.

mod common;

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::Options;
use hod_testutil::{Scratch, run_with, tool};

const FUNCTIONS: u32 = 2_000;
const STATEMENTS: u32 = 100;

/// The generated module: (with debug information, without any).
fn generated() -> (String, String) {
    let mut code = String::new();
    let mut meta = String::new();
    let mut plain = String::new();
    let mut next = 100u32;
    let mut line = 1u32;
    for f in 0..FUNCTIONS {
        let sp = next;
        next += 1;
        writeln!(
            meta,
            "!{sp} = distinct !DISubprogram(name: \"f{f}\", scope: !1, file: !1, line: {line}, type: !4, scopeLine: {line}, spFlags: DISPFlagDefinition, unit: !0)"
        )
        .unwrap();
        let inlined = next;
        next += 1;
        writeln!(
            meta,
            "!{inlined} = distinct !DISubprogram(name: \"h{f}\", scope: !7, file: !7, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0)"
        )
        .unwrap();
        let mut body = String::new();
        let mut plain_body = String::new();
        writeln!(body, "define dso_local void @f{f}(i32 %x) !dbg !{sp} {{\nb0:").unwrap();
        writeln!(plain_body, "define dso_local void @f{f}(i32 %x) {{\nb0:").unwrap();
        for s in 0..STATEMENTS {
            line += 1;
            let loc = next;
            next += 1;
            if s % 17 == 5 {
                let at = next;
                next += 1;
                writeln!(meta, "!{at} = !DILocation(line: {line}, column: 3, scope: !{sp})")
                    .unwrap();
                writeln!(
                    meta,
                    "!{loc} = !DILocation(line: {}, column: 5, scope: !{inlined}, inlinedAt: !{at})",
                    2 + s % 7
                )
                .unwrap();
            } else {
                writeln!(
                    meta,
                    "!{loc} = !DILocation(line: {line}, column: {}, scope: !{sp})",
                    3 + s % 9
                )
                .unwrap();
            }
            let inst = format!("  store volatile i32 {s}, ptr @g, align 4");
            writeln!(body, "{inst}, !dbg !{loc}").unwrap();
            writeln!(plain_body, "{inst}").unwrap();
            if s % 10 == 9 {
                let (b, n) = (s / 10, s / 10 + 1);
                let cmp = format!("  %c{b} = icmp sgt i32 %x, {s}");
                let br = format!("  br i1 %c{b}, label %b{n}, label %b{n}");
                writeln!(body, "{cmp}, !dbg !{loc}\n{br}, !dbg !{loc}\nb{n}:").unwrap();
                writeln!(plain_body, "{cmp}\n{br}\nb{n}:").unwrap();
            }
        }
        writeln!(body, "  ret void, !dbg !{}\n}}", next - 1).unwrap();
        writeln!(plain_body, "  ret void\n}}").unwrap();
        code.push_str(&body);
        plain.push_str(&plain_body);
        line += 3;
    }
    let header = common::ll_header(22);
    let full = format!(
        "{header}@g = dso_local global i32 0, align 4\n{code}\n!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!2, !3}}\n\
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"gen\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)\n\
!1 = !DIFile(filename: \"gen.c\", directory: \"/hod/gen\")\n\
!2 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n!3 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
!4 = !DISubroutineType(types: !5)\n!5 = !{{null}}\n\
!7 = !DIFile(filename: \"inl.h\", directory: \"/hod/gen\")\n{meta}"
    );
    let plain = format!("{header}@g = dso_local global i32 0, align 4\n{plain}");
    (full, plain)
}

fn compile(text: &str) -> usize {
    let (module, bodies) = common::parse(text);
    hod_a64::compile_to_object(&module, &bodies, &Options::default()).unwrap().len()
}

/// The best of five of compiling `text`, the parse not timed; and the
/// object's size.
fn best(text: &str) -> (f64, usize) {
    let (module, bodies) = common::parse(text);
    let mut best = f64::MAX;
    let mut size = 0;
    for _ in 0..5 {
        let start = Instant::now();
        size = hod_a64::compile_to_object(&module, &bodies, &Options::default()).unwrap().len();
        best = best.min(start.elapsed().as_secs_f64());
    }
    (best, size)
}

/// Wall seconds and peak KiB of `command` under GNU `time`.
fn timed(command: &Command) -> (f64, u64) {
    let mut timed = Command::new("/usr/bin/time");
    timed.args(["-f", "%M"]).arg(command.get_program()).args(command.get_args());
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            timed.env(key, value);
        }
    }
    let start = Instant::now();
    let out = run_with(&mut timed, &[], Duration::from_mins(10));
    let wall = start.elapsed().as_secs_f64();
    assert!(out.success(), "{}", out.stderr_text());
    let kib = out.stderr_text().lines().last().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    (wall, kib)
}

/// The child: parses and compiles the module `HOD_LINES_CHILD` names.
#[test]
#[ignore = "a child process of `line_table_cost`"]
fn child() {
    let Some(path) = std::env::var_os("HOD_LINES_CHILD") else { return };
    let text = std::fs::read_to_string(path).unwrap();
    assert!(compile(&text) > 0);
}

fn peak(me: &Path, input: &Path) -> u64 {
    (0..3)
        .map(|_| {
            let mut command = Command::new(me);
            command.args(["--ignored", "--exact", "child", "--test-threads=1"]);
            command.env("HOD_LINES_CHILD", input);
            timed(&command).1
        })
        .min()
        .unwrap()
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn line_table_cost() {
    let scratch = Scratch::new("lines-throughput");
    let (full, plain) = generated();
    let no_debug = full.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
    let (t_full, s_full) = best(&full);
    let (t_nodebug, s_nodebug) = best(&no_debug);
    let (t_plain, s_plain) = best(&plain);
    // The rows written: count them in the object.
    let (module, bodies) = common::parse(&full);
    let rows: usize = {
        let emitted = hod_a64::compile_module(&module, &bodies, &Options::default()).unwrap();
        let path = scratch.write("full.o", hod_obj::elf::to_vec(&emitted.object).unwrap());
        match tool("llvm-dwarfdump") {
            Some(d) => {
                let mut c = Command::new(d);
                c.arg("--debug-line").arg(&path);
                let out = run_with(&mut c, &[], Duration::from_mins(5)).stdout_text();
                out.lines().filter(|l| l.starts_with("0x")).count()
            }
            None => 0,
        }
    };
    let me = std::env::current_exe().unwrap();
    let paths = [
        scratch.write("full.ll", &full),
        scratch.write("nodebug.ll", &no_debug),
        scratch.write("plain.ll", &plain),
    ];
    let peaks: Vec<u64> = paths.iter().map(|p| peak(&me, p)).collect();
    let llc = tool("llc-22").map(|llc| {
        paths
            .iter()
            .map(|p| {
                (0..3)
                    .map(|_| {
                        let mut c = Command::new(&llc);
                        c.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
                            .arg(p)
                            .arg("-o")
                            .arg(scratch.join("llc.o"));
                        timed(&c)
                    })
                    .fold((f64::MAX, u64::MAX), |(t, m), (a, b)| (t.min(a), m.min(b)))
            })
            .collect::<Vec<_>>()
    });
    let extra = t_full - t_plain;
    eprintln!(
        "{FUNCTIONS} functions x {STATEMENTS} statements, {rows} rows:\n\
         hod: full debug {:.1} ms ({s_full} bytes), NoDebug unit {:.1} ms ({s_nodebug} bytes), no debug {:.1} ms ({s_plain} bytes)\n\
         hod: line tables and DIEs add {:.1} ms: {:.1} ns/row, {:.0} ns/function\n\
         hod peak RSS (parse + compile): full {} KiB, NoDebug {} KiB, none {} KiB",
        t_full * 1e3,
        t_nodebug * 1e3,
        t_plain * 1e3,
        extra * 1e3,
        extra * 1e9 / rows.max(1) as f64,
        extra * 1e9 / f64::from(FUNCTIONS),
        peaks[0],
        peaks[1],
        peaks[2],
    );
    if let Some(llc) = llc {
        eprintln!(
            "llc -O0: full debug {:.0} ms, {} KiB; NoDebug {:.0} ms, {} KiB; none {:.0} ms, {} KiB",
            llc[0].0 * 1e3,
            llc[0].1,
            llc[1].0 * 1e3,
            llc[1].1,
            llc[2].0 * 1e3,
            llc[2].1
        );
    }
    assert!(s_full > s_plain);
}
