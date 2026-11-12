//! A measurement, not a test (WP-096): the cost of `hod_ll`'s module-level
//! parse and of its body reading on one text, repeated in one process so
//! that the binary's own pages are faulted in once and the figures are the
//! reader's work alone.
//!
//! ```text
//! cargo run --release -p hod_ll --example profile -- <file.ll> [runs] [lex|scan|module|bodies|recycled|all]
//! ```
//!
//! Prints, per step, the median and minimum wall time over `runs` and, for
//! the bodies, nanoseconds per instruction (instructions as `hod_ir`
//! counts them). The bodies are read as the command line reads them: each
//! body by value, never recycled (`crates/hod/src/front.rs`); `recycled`
//! is the same with `BodyParser::recycle`.
#![allow(clippy::cast_precision_loss)]

use std::time::{Duration, Instant};

use hod_ll::{BodyParser, Lexer, ParseOptions, Tok, parse_module, scan_only};

fn stats(mut times: Vec<Duration>) -> (Duration, Duration) {
    times.sort();
    (times[times.len() / 2], times[0])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: profile <file.ll> [runs] [lex|scan|module|bodies|recycled|all]");
        std::process::exit(2);
    };
    let text = std::fs::read(path).expect("the input");
    let runs: usize = args.get(1).map_or(15, |n| n.parse().expect("a number of runs"));
    let what = args.get(2).map_or("all", String::as_str);
    let options = ParseOptions::default();
    let all = what == "all";

    if all || what == "lex" {
        let mut tokens = 0usize;
        let times = (0..runs)
            .map(|_| {
                let start = Instant::now();
                let mut lexer = Lexer::new(&text);
                tokens = 0;
                while !matches!(lexer.next_token().kind, Tok::Eof | Tok::Error) {
                    tokens += 1;
                }
                start.elapsed()
            })
            .collect();
        let (median, min) = stats(times);
        println!(
            "lex        {:>9.0} / {:>9.0} us  {tokens} tokens, {:.2} ns per byte, {:.2} ns per token",
            median.as_micros(),
            min.as_micros(),
            min.as_nanos() as f64 / text.len() as f64,
            min.as_nanos() as f64 / tokens as f64
        );
    }
    if all || what == "scan" {
        let times = (0..runs)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(scan_only(&text).expect("a scan"));
                start.elapsed()
            })
            .collect();
        let (median, min) = stats(times);
        println!("scan       {:>9.0} / {:>9.0} us", median.as_micros(), min.as_micros());
    }
    if all || what == "module" {
        let times = (0..runs)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(parse_module(&text, &options).expect("a module"));
                start.elapsed()
            })
            .collect();
        let (median, min) = stats(times);
        println!("module     {:>9.0} / {:>9.0} us", median.as_micros(), min.as_micros());
    }
    if all || what == "bodies" || what == "recycled" {
        let recycle = what == "recycled";
        let mut insts = 0;
        let mut functions = 0;
        let mut times = Vec::new();
        for _ in 0..runs {
            let mut parsed = parse_module(&text, &options).expect("a module");
            let mut parser = BodyParser::new(&parsed.text);
            let mut count = 0;
            let start = Instant::now();
            for function in parsed.text.functions() {
                match parser.parse(&mut parsed.module, function) {
                    Ok(Some(body)) => {
                        count += body.inst_count();
                        if recycle {
                            parser.recycle(body);
                        } else {
                            drop(std::hint::black_box(body));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => panic!("{error}"),
                }
            }
            times.push(start.elapsed());
            insts = count;
            functions = parsed.text.functions().len();
        }
        let (median, min) = stats(times);
        println!(
            "bodies     {:>9.0} / {:>9.0} us  {functions} functions, {insts} instructions, \
             {:.1} / {:.1} ns per instruction",
            median.as_micros(),
            min.as_micros(),
            median.as_nanos() as f64 / insts as f64,
            min.as_nanos() as f64 / insts as f64
        );
    }
}
