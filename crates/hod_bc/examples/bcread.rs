//! The measurement of `spec/bc.md` §9.5: `bcread <bc|walk|ll> <file> [n]`
//! reads `file` `n` times (default 20) and prints the fastest read.
//! `bc` is `hod_bc::read_module` on bitcode, `walk` its first pass alone,
//! `ll` `hod_ll::parse_module` on text. Peak memory is measured from
//! outside (`/usr/bin/time -f %M`), one mode per process.

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, mode, file, rest @ ..] = &args[..] else {
        eprintln!("usage: bcread <bc|walk|ll> <file> [n]");
        std::process::exit(2);
    };
    let n: usize = rest.first().and_then(|n| n.parse().ok()).unwrap_or(20);
    let data = std::fs::read(file).expect("the input");
    let mut best = Duration::MAX;
    for _ in 0..n {
        let start = Instant::now();
        let ok = match mode.as_str() {
            "bc" => hod_bc::read_module(&data, &hod_bc::ReadOptions::default()).is_ok(),
            "walk" => hod_bc::walk_only(&data).is_ok(),
            "ll" => hod_ll::parse_module(&data, &hod_ll::ParseOptions::default()).is_ok(),
            _ => panic!("mode {mode}"),
        };
        best = best.min(start.elapsed());
        assert!(ok, "{mode} refused {file}");
    }
    println!("{mode} {file} {:.3} ms", best.as_secs_f64() * 1e3);
}
