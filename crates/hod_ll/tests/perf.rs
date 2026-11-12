//! A measurement, not a gate (`spec/ll.md` §9): megabytes per second and
//! nanoseconds per byte of the lexer alone, of the index scan, of the
//! whole module-level parse and of stepping over function bodies; the
//! peak memory of the parse; and, where LLVM is installed, the same on
//! what clang makes of a C++ source full of the standard library, beside
//! the wall time of `llvm-as` on that text. The numbers are printed
//! (`cargo test --release -p hod_ll --test perf -- --nocapture`); what
//! the test asserts is only that the parser found what the text has.
#![allow(clippy::cast_precision_loss)] // Sizes and rates for display.

mod common;

use std::fmt::Write as _;
use std::process::Command;
use std::time::{Duration, Instant};

use hod_ir::Arch;
use hod_ll::{BodyParser, Lexer, ParseOptions, ParsedModule, Tok, parse_module, scan_only};
use hod_testutil::{DEFAULT_TIMEOUT, Scratch, run_with};

use common::{Lcg, Llvm};

/// Functions in the synthetic module: about 19 MiB of text when
/// optimized, a quarter of that otherwise.
const FUNCTIONS: usize = if cfg!(debug_assertions) { 500 } else { 2_000 };
/// Timed runs of each step; the fastest counts.
const RUNS: usize = if cfg!(debug_assertions) { 1 } else { 5 };

/// A name shaped like an Itanium mangling, distinct for each `serial`.
fn mangled(rng: &mut Lcg, serial: usize) -> String {
    const WORDS: [&str; 10] = [
        "std",
        "__1",
        "basic_string",
        "allocator",
        "char_traits",
        "vector",
        "__tree_node",
        "unique_ptr",
        "value_type",
        "detail",
    ];
    let mut name = String::from("_ZN");
    for _ in 0..2 + rng.below(6) {
        let word = WORDS[rng.below(WORDS.len())];
        let _ = write!(name, "{}{word}", word.len());
        if rng.below(4) == 0 {
            name.push_str("IcNS_");
        }
    }
    let _ = write!(name, "C2Ev{serial}");
    name
}

/// A module shaped like what clang writes for C++ at `-O0`: for each
/// function a definition with a body of loads, stores, calls and
/// branches, now and then a string, a table of pointers, a declaration;
/// attribute groups and module flags at the end.
fn synthetic(debug_info: bool) -> (Vec<u8>, usize) {
    let mut rng = Lcg(0x11_5eed);
    let mut text = String::with_capacity(FUNCTIONS * 1500);
    text.push_str("; ModuleID = 'synthetic.cpp'\nsource_filename = \"synthetic.cpp\"\n");
    text.push_str(&common::header(Arch::X86_64, 22));
    text.push_str(
        "\n%struct.S = type { ptr, i64, [4 x i32] }\n%class.C = type { %struct.S, i8 }\n\n",
    );
    let mut body_bytes = 0;
    let mut nodes = 10;
    for serial in 0..FUNCTIONS {
        let name = mangled(&mut rng, serial);
        if serial % 5 == 0 {
            let _ = writeln!(
                text,
                "@.str.{serial} = private unnamed_addr constant [{} x i8] c\"{name}\\00\", align 1",
                name.len() + 1
            );
        }
        if serial % 16 == 1 {
            let _ = writeln!(
                text,
                "@_ZTV{serial} = linkonce_odr dso_local unnamed_addr constant {{ [4 x ptr] }} {{ \
                 [4 x ptr] [ptr null, ptr @.str.{}, ptr @{name}, ptr getelementptr inbounds (i8, \
                 ptr @.str.{}, i64 2)] }}, comdat, align 8",
                serial - serial % 5,
                serial - serial % 5
            );
            let _ = writeln!(text, "$_ZTV{serial} = comdat any");
        }
        if serial % 7 == 3 {
            let _ = writeln!(text, "declare noundef i32 @ext{serial}(ptr noundef, i64 noundef) #1");
        }
        let _ = writeln!(text, "$\"{name}\" = comdat any");
        let dbg = if debug_info { format!(" !dbg !{nodes}") } else { String::new() };
        let _ = writeln!(
            text,
            "; Function Attrs: mustprogress noinline nounwind optnone uwtable\ndefine \
             linkonce_odr dso_local noundef i32 @\"{name}\"(ptr noundef nonnull align 8 \
             dereferenceable(16) %0, i32 noundef %1) #0 comdat align 2{dbg} {{"
        );
        let start = text.len();
        text.push_str("  %3 = alloca ptr, align 8\n  %4 = alloca i32, align 4\n");
        text.push_str("  store ptr %0, ptr %3, align 8\n  store i32 %1, ptr %4, align 4\n");
        let mut value = 5;
        for block in 0..2 + rng.below(8) {
            for _ in 0..3 + rng.below(8) {
                let location = if debug_info { ", !dbg !3" } else { "" };
                let _ = writeln!(text, "  %{value} = load ptr, ptr %3, align 8{location}");
                let _ = writeln!(
                    text,
                    "  %{} = getelementptr inbounds %struct.S, ptr %{value}, i32 0, i32 {}",
                    value + 1,
                    rng.below(3)
                );
                let _ = writeln!(
                    text,
                    "  %{} = call noundef i32 @\"{name}\"(ptr noundef %{}, i32 noundef {}){location}",
                    value + 2,
                    value + 1,
                    rng.below(1000)
                );
                let _ = writeln!(text, "  store i32 %{}, ptr %4, align 4", value + 2);
                value += 3;
            }
            let _ = writeln!(text, "  br label %bb{block}\n\nbb{block}:");
        }
        text.push_str("  %r = load i32, ptr %4, align 4\n  ret i32 %r\n");
        body_bytes += text.len() - start;
        text.push_str("}\n\n");
        if debug_info {
            let _ = writeln!(
                text,
                "!{nodes} = distinct !DISubprogram(name: \"f{serial}\", linkageName: \"{name}\", \
                 scope: !1, file: !1, line: {serial}, type: !4, scopeLine: {serial}, flags: \
                 DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)"
            );
            nodes += 1;
        }
    }
    text.push_str("attributes #0 = { mustprogress noinline nounwind optnone uwtable ");
    text.push_str("\"frame-pointer\"=\"all\" \"min-legal-vector-width\"=\"0\" ");
    text.push_str("\"no-trapping-math\"=\"true\" \"stack-protector-buffer-size\"=\"8\" ");
    text.push_str("\"target-cpu\"=\"x86-64\" \"tune-cpu\"=\"generic\" }\n");
    text.push_str("attributes #1 = { \"frame-pointer\"=\"all\" \"target-cpu\"=\"x86-64\" }\n\n");
    if debug_info {
        text.push_str("!llvm.module.flags = !{!5, !6, !7, !9}\n!llvm.ident = !{!8}\n");
        text.push_str("!llvm.dbg.cu = !{!0}\n");
    } else {
        text.push_str("!llvm.module.flags = !{!5, !6, !7}\n!llvm.ident = !{!8}\n");
    }
    text.push_str("!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, ");
    text.push_str("producer: \"clang\", isOptimized: false, runtimeVersion: 0, emissionKind: ");
    text.push_str("FullDebug)\n!1 = !DIFile(filename: \"synthetic.cpp\", directory: \"/tmp\")\n");
    text.push_str("!2 = !{}\n!3 = !DILocation(line: 1, column: 1, scope: !10)\n");
    text.push_str("!4 = !DISubroutineType(types: !2)\n!5 = !{i32 8, !\"PIC Level\", i32 2}\n");
    text.push_str("!6 = !{i32 7, !\"uwtable\", i32 2}\n!7 = !{i32 7, !\"frame-pointer\", i32 2}\n");
    text.push_str("!8 = !{!\"clang\"}\n");
    if debug_info {
        text.push_str("!9 = !{i32 2, !\"Debug Info Version\", i32 3}\n");
    } else {
        text.push_str("!9 = !{}\n");
    }
    if !debug_info {
        text.push_str("!10 = !{}\n");
    }
    (text.into_bytes(), body_bytes)
}

/// The fastest of [`RUNS`] runs of `step`, and its last result.
fn timed<T>(mut step: impl FnMut() -> T) -> (Duration, T) {
    let mut best: Option<(Duration, T)> = None;
    for _ in 0..RUNS {
        let start = Instant::now();
        let result = step();
        let time = start.elapsed();
        if best.as_ref().is_none_or(|(fastest, _)| time < *fastest) {
            best = Some((time, result));
        }
    }
    best.expect("at least one run")
}

/// `label: 12.34 ms, 567.8 MiB/s, 1.68 ns/byte`.
fn report(label: &str, bytes: usize, time: Duration) -> String {
    let seconds = time.as_secs_f64();
    format!(
        "  {label:<22} {:9.2} ms {:9.1} MiB/s {:7.2} ns/byte",
        seconds * 1e3,
        bytes as f64 / 1_048_576.0 / seconds,
        seconds * 1e9 / bytes as f64
    )
}

fn status_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The variable that makes this test binary a memory probe, and the line
/// the probe answers with.
const PROBE: &str = "HOD_LL_PERF_PROBE";
const PEAK: &str = "peak-kib ";
/// Set beside [`PROBE`]: the probe parses the bodies too.
const PROBE_BODIES: &str = "HOD_LL_PERF_PROBE_BODIES";

/// Parses every body of `parsed`, one alive at a time (`spec/ll.md`
/// §11.1); the number of instructions.
fn parse_every_body(parsed: &mut ParsedModule<'_>) -> usize {
    let ParsedModule { module, text } = parsed;
    let mut bodies = BodyParser::new(text);
    let mut insts = 0;
    for function in text.functions() {
        if let Some(body) = bodies.parse(module, function).expect("a body") {
            insts += body.inst_count();
            bodies.recycle(body);
        }
    }
    insts
}

/// Not a test of its own: with [`PROBE`] naming a file, parses it and
/// prints how far the resident memory of the process rose above what it
/// was with the text loaded. A process of its own, because memory that
/// an earlier parse returned to the allocator hides what a later one
/// takes.
#[test]
fn memory_probe() {
    let Some(path) = std::env::var_os(PROBE) else {
        return;
    };
    let text = std::fs::read(path).expect("the text to measure");
    // Starts the peak again from the present, where the kernel offers it.
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let Some(before) = status_kib("VmRSS:") else {
        return;
    };
    let mut parsed = parse_module(&text, &ParseOptions::default()).expect("the parse");
    if std::env::var_os(PROBE_BODIES).is_some() {
        parse_every_body(&mut parsed);
    }
    let peak = status_kib("VmHWM:").unwrap_or(before);
    println!("\n{PEAK}{}", peak.saturating_sub(before));
    drop(parsed);
}

/// The peak memory of a parse of `text` in KiB, measured by this binary
/// run again as a probe; `None` where `/proc` does not say.
fn peak_kib(text: &[u8], with_bodies: bool) -> Option<u64> {
    let scratch = Scratch::new("hod_ll_perf");
    let path = scratch.write("text.ll", text);
    let mut command = Command::new(std::env::current_exe().ok()?);
    command.args(["memory_probe", "--exact", "--nocapture", "--test-threads=1"]).env(PROBE, path);
    if with_bodies {
        command.env(PROBE_BODIES, "1");
    }
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    let output = outcome.stdout_text();
    let line = output.lines().find_map(|line| line.strip_prefix(PEAK))?;
    line.trim().parse().ok()
}

/// Measures the steps on `text`: the parsed module, and the lines to
/// print, headed by `title`. Tests run side by side, so each prints its
/// lines at once.
fn measure(title: &str, text: &[u8], options: &ParseOptions) -> (ParsedModule<'static>, String) {
    // The parsed module borrows the text: leaked, once per measured text.
    let text: &'static [u8] = Box::leak(text.to_vec().into_boxed_slice());
    let mut lines = Vec::new();

    let (time, tokens) = timed(|| {
        let mut lexer = Lexer::new(text);
        let mut tokens = 0usize;
        while lexer.next_token().kind != Tok::Eof {
            tokens += 1;
        }
        tokens
    });
    lines.push(report("lexing every token", text.len(), time));

    let (time, scanned) = timed(|| scan_only(text).expect("the index scan"));
    lines.push(report("index scan", text.len(), time));

    let (time, parsed) = timed(|| parse_module(text, options).expect("the parse"));
    let peak = peak_kib(text, false);
    let module_time = time;
    lines.push(report("module-level parse", text.len(), time));

    // What the module-level parse has to lex after the scan: everything
    // but the bodies.
    let body_bytes: usize = parsed.functions().iter().map(|f| f.body.of(text).len()).sum();
    let (time, outside) = timed(|| {
        let mut lexer = Lexer::new(text);
        let mut functions = parsed.functions().iter();
        let mut next = functions.next();
        let mut tokens = 0usize;
        loop {
            let token = lexer.next_token();
            if token.kind == Tok::Eof {
                return tokens;
            }
            tokens += 1;
            if let Some(function) = next
                && token.end == function.body.start
            {
                lexer.set_pos(function.body.end as usize);
                next = functions.next();
            }
        }
    });
    lines.push(report("lexing outside bodies", text.len() - body_bytes, time));

    let mut bodies = b"define void @f() {".to_vec();
    for function in parsed.functions() {
        bodies.extend_from_slice(function.body.of(text));
    }
    bodies.push(b'}');
    let (time, skipped) = timed(|| scan_only(&bodies).expect("one large body"));
    assert_eq!(skipped, (1, bodies.len() - 19), "the bodies are one body");
    lines.push(report("stepping over bodies", bodies.len(), time));

    // §11: the module level and then every body, from the text each time.
    let (time, insts) = timed(|| {
        let mut parsed = parse_module(text, options).expect("the parse");
        parse_every_body(&mut parsed)
    });
    let peak_all = peak_kib(text, true);
    lines.push(report("module and bodies", text.len(), time));
    lines.push(report("bodies alone", body_bytes, time.saturating_sub(module_time)));

    assert_eq!(scanned.1, body_bytes, "the scan and the parse find the same bodies");
    let mut out = format!(
        "{title}: {:.2} MiB, {tokens} tokens ({outside} outside bodies), {} top-level entities, \
         {} symbols, {} bodies ({:.0}% of the text), {} metadata nodes",
        text.len() as f64 / 1_048_576.0,
        scanned.0,
        parsed.module.symbol_count(),
        parsed.functions().len(),
        body_bytes as f64 * 100.0 / text.len() as f64,
        parsed.metadata().nodes().len()
    );
    for line in lines {
        let _ = write!(out, "\n{line}");
    }
    let _ = match peak {
        Some(kib) => write!(
            out,
            "\n  peak memory of the parse {:8.2} MiB, {:.2} bytes per byte of text",
            kib as f64 / 1024.0,
            kib as f64 * 1024.0 / text.len() as f64
        ),
        None => write!(out, "\n  peak memory of the parse: not measurable here"),
    };
    if let Some(kib) = peak_all {
        let _ = write!(
            out,
            "\n  peak memory with bodies  {:8.2} MiB, {:.2} bytes per byte of text; {insts} instructions",
            kib as f64 / 1024.0,
            kib as f64 * 1024.0 / text.len() as f64
        );
    }
    (parsed, out)
}

/// §9: the synthetic module, without and with debug information.
#[test]
fn rates_on_a_synthetic_module() {
    let profile = if cfg!(debug_assertions) { "dev" } else { "release" };
    for debug_info in [false, true] {
        let (text, body_bytes) = synthetic(debug_info);
        let title = if debug_info { "synthetic, -g" } else { "synthetic" };
        let (parsed, lines) = measure(title, &text, &ParseOptions::default());
        println!("perf ({profile}), best of {RUNS}: {lines}");
        assert_eq!(parsed.functions().len(), FUNCTIONS);
        let bodies: usize = parsed.functions().iter().map(|f| f.body.of(&text).len()).sum();
        // A span is the inside of the braces: the newline after `{` too.
        assert_eq!(bodies, body_bytes + FUNCTIONS);
        let debug: Vec<_> = parsed.module.unsupported().map(|u| u.construct).collect();
        assert_eq!(debug, Vec::<&str>::new());
        let expected = if debug_info { FUNCTIONS } else { 0 };
        assert_eq!(parsed.functions().iter().filter(|f| f.dbg.is_some()).count(), expected);
    }
}

/// §9: what clang makes of `stl.cpp`, and `llvm-as` on the same text.
/// `llvm-as` parses the bodies too, verifies the module and writes
/// bitcode: its time is the cost of the reader `hod_ll` replaces, not a
/// like-for-like figure.
#[test]
fn rates_on_clang_output_beside_llvm_as() {
    for llvm in Llvm::all() {
        for options in [&[][..], &["-g"][..]] {
            let text = llvm.clang("stl.cpp", Arch::X86_64, options);
            let title = format!("clang {} stl.cpp {}", llvm.major, options.join(" "));
            let (parsed, lines) = measure(title.trim_end(), &text, &ParseOptions::default());
            assert!(parsed.functions().len() > 1000, "{title}: a large module");
            let (time, ()) = timed(|| {
                let mut command = Command::new(llvm.tool("llvm-as"));
                command.args(["-o", "/dev/null", "-"]);
                assert!(run_with(&mut command, &text, DEFAULT_TIMEOUT).success());
            });
            println!("{lines}\n{}", report(&format!("llvm-as {}", llvm.major), text.len(), time));
        }
    }
}
