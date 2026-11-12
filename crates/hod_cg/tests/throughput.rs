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

/// What the unwind tables cost `emit_module` (WP-049, `spec/eh.md`
/// §9.7): one large generated module with exceptions, emitted with every
/// function's tables, and again with every function marked `nounwind`
/// without `uwtable`, personality or pads (no `.eh_frame`, no LSDA).
/// Prints ns per function for each and the bytes of the tables.
#[test]
#[ignore = "a measurement: run in release"]
#[allow(clippy::cast_precision_loss, reason = "a rate to print")]
fn unwind_table_cost_per_function() {
    let generated = GenOptions { exceptions: true, ..GenOptions::everything() }.sized(4000, 30);
    let program = generate(0x49, &generated);
    let tm = Tm::default();
    let (mut scratch, mut chunk) = (Scratch::default(), Chunk::new());
    compile_all(
        &tm,
        &program.module,
        &program.bodies,
        Options::default(),
        &mut scratch,
        &mut chunk,
    )
    .expect("the program compiles");
    let mut bare = chunk.clone();
    for func in &mut bare.funcs {
        (func.nounwind, func.uwtable, func.personality, func.has_pads) = (true, false, None, false);
    }
    let functions = chunk.funcs.len();
    let time = |chunk: &Chunk| {
        let chunks = std::slice::from_ref(chunk);
        let mut best = f64::INFINITY;
        let mut sizes = (0, 0, 0);
        for _ in 0..7 {
            let start = Instant::now();
            let emitted =
                hod_cg::emit_module(&program.module, &Options::default(), chunks).unwrap();
            best = best.min(start.elapsed().as_secs_f64());
            let obj = &emitted.object;
            let size = |prefix: &[u8]| -> usize {
                (0..obj.section_count())
                    .map(hod_obj::SectionId::from_index)
                    .filter(|&s| obj.section_name(s).starts_with(prefix))
                    .map(|s| obj.data(s).len())
                    .sum()
            };
            let fdes = emitted.funcs.iter().filter(|f| f.record.needs_unwind_entry()).count();
            sizes = (size(b".eh_frame"), size(b".gcc_except_table"), fdes);
        }
        (best * 1e9 / functions as f64, sizes)
    };
    let (with, (eh_frame, lsda, fdes)) = time(&chunk);
    let (without, _) = time(&bare);
    let lsdas = chunk.funcs.iter().filter(|f| f.has_pads).count();
    println!(
        "{functions} functions ({fdes} FDEs, {lsdas} LSDAs): emit_module {with:.0} ns/function with \
         tables, {without:.0} without ({:.0} ns per function for the tables); .eh_frame {eh_frame} \
         bytes, .gcc_except_table {lsda} bytes",
        with - without
    );
    assert!(fdes > 0 && lsdas > 0 && eh_frame > 0 && lsda > 0);
}
