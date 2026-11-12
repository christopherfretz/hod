//! The measurement of `spec/bc.md` §9.5 and §11.10:
//! `bcread <bc|walk|ll|bc-bodies|ll-bodies> <file> [n]` reads `file` `n`
//! times (default 20) and prints the fastest read. `bc` is
//! `hod_bc::read_module` on bitcode, `walk` its first pass alone, `ll`
//! `hod_ll::parse_module` on text; the `-bodies` modes read every body
//! after the module and print the instruction count. Peak memory is measured from
//! outside (`/usr/bin/time -f %M`), one mode per process.

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, mode, file, rest @ ..] = &args[..] else {
        eprintln!("usage: bcread <bc|walk|ll|bc-bodies|ll-bodies> <file> [n]");
        std::process::exit(2);
    };
    let n: usize = rest.first().and_then(|n| n.parse().ok()).unwrap_or(20);
    let data = std::fs::read(file).expect("the input");
    let mut best = Duration::MAX;
    let mut insts = 0;
    for _ in 0..n {
        let start = Instant::now();
        let ok = match mode.as_str() {
            "bc" => hod_bc::read_module(&data, &hod_bc::ReadOptions::default()).is_ok(),
            "walk" => hod_bc::walk_only(&data).is_ok(),
            "ll" => hod_ll::parse_module(&data, &hod_ll::ParseOptions::default()).is_ok(),
            "bc-bodies" => {
                let mut read =
                    hod_bc::read_module(&data, &hod_bc::ReadOptions::default()).expect("module");
                insts = 0;
                hod_bc::read_bodies::<hod_bc::BcError>(&mut read.module, &read.bits, |_, read| {
                    insts += read.body.map_or(0, hod_ir::Body::inst_count);
                    Ok(())
                })
                .is_ok()
            }
            "ll-bodies" => {
                let parsed = hod_ll::parse_module(&data, &hod_ll::ParseOptions::default());
                let parsed = parsed.expect("module");
                let mut module = parsed.module;
                insts = 0;
                hod_ll::parse_bodies::<hod_ll::LlError>(&mut module, &parsed.text, |_, read| {
                    insts += read.body.map_or(0, hod_ir::Body::inst_count);
                    Ok(())
                })
                .is_ok()
            }
            _ => panic!("mode {mode}"),
        };
        best = best.min(start.elapsed());
        assert!(ok, "{mode} refused {file}");
    }
    #[allow(clippy::cast_precision_loss)] // A count far below 2^52.
    let per = if insts > 0 { best.as_secs_f64() * 1e9 / insts as f64 } else { 0.0 };
    println!("{mode} {file} {:.3} ms {insts} insts {per:.1} ns/inst", best.as_secs_f64() * 1e3);
}
