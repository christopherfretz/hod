//! Parses the module level of each `.ll` file named on the command line
//! and prints what it found and how long it took: a hand tool for
//! `spec/ll.md` §9, also what `/usr/bin/time -v` is pointed at to read
//! the parser's peak memory.

#![allow(clippy::cast_precision_loss)] // Sizes and rates for display.

use std::process::ExitCode;
use std::time::Instant;

use hod_ll::{Lexer, ParseOptions, Tok, parse_module, scan_only};

fn main() -> ExitCode {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        let source = match std::fs::read(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{path}: {error}");
                failed = true;
                continue;
            }
        };
        let mib = source.len() as f64 / (1024.0 * 1024.0);

        let start = Instant::now();
        let mut lexer = Lexer::new(&source);
        let mut tokens = 0u64;
        while lexer.next_token().kind != Tok::Eof {
            tokens += 1;
        }
        let lex = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let scanned = scan_only(&source);
        let scan = start.elapsed().as_secs_f64();

        let start = Instant::now();
        let parsed = parse_module(&source, &ParseOptions::default());
        let parse = start.elapsed().as_secs_f64();

        println!("{path}: {mib:.2} MiB, {tokens} tokens");
        println!("  lex   {:8.2} ms  {:8.1} MiB/s", lex * 1e3, mib / lex);
        if let Ok((entities, bodies)) = scanned {
            println!(
                "  scan  {:8.2} ms  {:8.1} MiB/s  ({entities} entities, {bodies} body bytes)",
                scan * 1e3,
                mib / scan
            );
        }
        println!("  parse {:8.2} ms  {:8.1} MiB/s", parse * 1e3, mib / parse);
        match parsed {
            Ok(parsed) => {
                let mut bodies = b"define void @f() {".to_vec();
                for function in parsed.functions() {
                    bodies.extend_from_slice(function.body.of(&source));
                }
                bodies.push(b'}');
                let start = Instant::now();
                let ok = scan_only(&bodies).is_ok();
                let skip = start.elapsed().as_secs_f64();
                let body_mib = bodies.len() as f64 / (1024.0 * 1024.0);
                println!(
                    "  skip  {:8.2} ms  {:8.1} MiB/s  (bodies only, {body_mib:.2} MiB, ok {ok})",
                    skip * 1e3,
                    body_mib / skip
                );
                let module = &parsed.module;
                println!(
                    "  {} symbols, {} functions with bodies, {} types, {} constants, {} metadata \
                     nodes",
                    module.symbol_count(),
                    parsed.functions().len(),
                    module.types.len(),
                    module.consts.len(),
                    parsed.metadata().nodes().len()
                );
                let mut seen: Vec<&str> = module.unsupported().map(|u| u.construct).collect();
                seen.sort_unstable();
                seen.dedup();
                if !seen.is_empty() {
                    println!("  unsupported: {}", seen.join(", "));
                }
            }
            Err(error) => {
                println!("  error: {error} ({:?})", error.kind);
                failed = true;
            }
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
