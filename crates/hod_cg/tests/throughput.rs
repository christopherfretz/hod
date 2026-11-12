//! The core's throughput on the test target: IR instructions compiled
//! per second, generated programs, no stress option. A measurement, not
//! a gate (`spec/codegen.md` §12, performance); run it in release:
//!
//! `cargo test -p hod_cg --release --test throughput -- --ignored --nocapture`

mod common;

use std::time::Instant;

use common::harness::compile_all;
use common::target::Tm;
use hod_cg::{Chunk, Options, Scratch};
use hod_interp::generate::{Options as GenOptions, generate};

#[test]
#[ignore = "a measurement: run in release"]
#[allow(clippy::cast_precision_loss, reason = "a rate to print")]
fn ir_instructions_per_second() {
    let programs: Vec<_> =
        (0..64).map(|seed| generate(seed, &GenOptions::default().sized(8, 200))).collect();
    let insts: u64 = programs
        .iter()
        .flat_map(|program| &program.bodies)
        .map(|body| body.inst_count() as u64)
        .sum();
    let (tm, options) = (Tm::default(), Options::default());
    let (mut scratch, mut chunk) = (Scratch::default(), Chunk::new());
    let mut best = f64::INFINITY;
    let mut bytes = 0;
    for _ in 0..5 {
        let start = Instant::now();
        bytes = 0;
        for program in &programs {
            chunk.clear();
            compile_all(&tm, &program.module, &program.bodies, options, &mut scratch, &mut chunk)
                .expect("a generated program compiles");
            bytes += chunk.code.len() as u64;
        }
        best = best.min(start.elapsed().as_secs_f64());
    }
    let rate = insts as f64 / best;
    println!(
        "{insts} IR instructions, {bytes} code bytes: best of 5 {:.1} ms, {:.2} M insts/s, {:.1} MB/s",
        best * 1e3,
        rate / 1e6,
        bytes as f64 / best / 1e6
    );
    assert!(rate > 0.0);
}
