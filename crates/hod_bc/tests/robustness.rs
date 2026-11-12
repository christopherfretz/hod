//! `spec/bc.md` §9.4: input `hod_bc` did not expect is an error, never a
//! panic, a hang, or memory beyond a bound — over every truncation and
//! single-byte corruption of a small file, and over random bytes.
//!
//! The file is `tests/data/module-llvm19.bc`, `llvm-as` 19 of
//! `tests/data/module.ll`, checked in so the test needs no LLVM. A read
//! that returns at all is a pass (an `Err` or an `Ok`); a panic fails the
//! test, a read over `SLOW` fails it as a hang, and the process's peak
//! resident memory may grow by at most `MEMORY` over the run.
//! `HOD_BC_ALL_BYTES=1` puts all 255 other values at each position, not
//! the default eight.

use std::time::{Duration, Instant};

const FILE: &[u8] = include_bytes!("data/module-llvm19.bc");
const SLOW: Duration = Duration::from_secs(2);
const MEMORY: u64 = 256 << 20;

fn read(data: &[u8]) -> Duration {
    let start = Instant::now();
    let _ = hod_bc::read_module(data, &hod_bc::ReadOptions::default());
    start.elapsed()
}

/// The process's peak resident set, in bytes (`VmHWM`; 0 where unknown).
fn peak_memory() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else { return 0 };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb << 10)
}

struct Run {
    reads: usize,
    slowest: Duration,
    before: u64,
}

impl Run {
    fn new() -> Self {
        Run { reads: 0, slowest: Duration::ZERO, before: peak_memory() }
    }

    fn read(&mut self, data: &[u8], what: &dyn Fn() -> String) {
        let took = read(data);
        assert!(took < SLOW, "{} took {took:?}", what());
        self.reads += 1;
        self.slowest = self.slowest.max(took);
    }

    fn finish(self, name: &str) {
        let grew = peak_memory().saturating_sub(self.before);
        eprintln!(
            "{name}: {} reads, slowest {:?}, peak memory grew {grew} bytes",
            self.reads, self.slowest
        );
        assert!(grew <= MEMORY, "{name}: peak memory grew {grew} bytes");
    }
}

#[test]
fn the_file_reads() {
    hod_bc::read_module(FILE, &hod_bc::ReadOptions::default()).expect("the checked-in bitcode");
}

#[test]
fn every_truncation_is_an_error() {
    let mut run = Run::new();
    for len in 0..FILE.len() {
        let data = &FILE[..len];
        run.read(data, &|| format!("the first {len} bytes"));
        assert!(
            hod_bc::read_module(data, &hod_bc::ReadOptions::default()).is_err(),
            "the first {len} bytes read as a module"
        );
    }
    run.finish("truncation");
}

#[test]
fn every_single_byte_corruption_returns() {
    let all = std::env::var_os("HOD_BC_ALL_BYTES").is_some();
    let mut run = Run::new();
    let mut data = FILE.to_vec();
    for at in 0..data.len() {
        let original = data[at];
        let values: Vec<u8> = if all {
            (0..=255).filter(|&v| v != original).collect()
        } else {
            [0x00, 0xFF, 0x01, 0x02, 0x10, 0x80, 0x40, 0x7F]
                .map(|bit| if bit == 0 || bit == 0xFF { bit } else { original ^ bit })
                .into_iter()
                .filter(|&v| v != original)
                .collect()
        };
        for value in values {
            data[at] = value;
            run.read(&data, &|| format!("byte {at} set to {value:#04x}"));
        }
        data[at] = original;
    }
    run.finish("corruption");
}

#[test]
fn random_bytes_return() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut run = Run::new();
    for case in 0..2000 {
        let len = (next() % 4096) as usize;
        let mut data: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Half behind a real magic, so the reader gets past it.
        if case % 2 == 0 && data.len() >= 4 {
            data[..4].copy_from_slice(b"BC\xC0\xDE");
        }
        // And some as the real file with a random tail.
        if case % 7 == 0 {
            let cut = (next() as usize) % FILE.len();
            data = [&FILE[..cut], &data[..]].concat();
        }
        run.read(&data, &|| format!("random case {case}"));
    }
    run.finish("random");
}
