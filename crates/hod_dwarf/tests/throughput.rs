//! `spec/dwarf.md` §8.5: line-table throughput on a large synthetic
//! input. Ignored by default; measure with
//!
//! ```text
//! cargo test --release -p hod_dwarf --test throughput -- --ignored --nocapture
//! ```
//!
//! 20 000 functions of 100 transitions each (2 000 000 rows) shaped like
//! `-O0` code: 4-byte instructions, a new location every few
//! instructions, small line steps with an occasional jump or file
//! change. Reports the policy and the writer separately, the best of
//! five runs each.

use std::time::Instant;

use hod_dwarf::{
    FileEntry, FunctionLines, LineHeader, Row, Sequence, SourceLoc, Transition, function_rows,
    write_debug_line,
};
use hod_obj::{Arch, Object, SectionKind};

const FUNCTIONS: u32 = 20_000;
const PER_FUNCTION: u32 = 100;

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// Per function: its transitions and code size.
fn input() -> Vec<(Vec<Transition>, u32)> {
    let mut r = Lcg(52);
    (0..FUNCTIONS)
        .map(|_| {
            let mut file = r.below(50) as u32;
            let mut line = 1 + r.below(5000) as u32;
            let mut offset = 0;
            let ts = (0..PER_FUNCTION)
                .map(|_| {
                    let t = Transition {
                        offset,
                        loc: Some(SourceLoc { file, line, column: 1 + r.below(40) as u16 }),
                    };
                    offset += 4 * (1 + r.below(6) as u32);
                    match r.below(20) {
                        0 => file = r.below(50) as u32,
                        1 => line = 1 + r.below(5000) as u32,
                        2 => line = line.saturating_sub(3).max(1),
                        _ => line += r.below(3) as u32,
                    }
                    t
                })
                .collect();
            (ts, offset + 8)
        })
        .collect()
}

/// The fastest of five runs, and the last run's result.
fn best<T>(mut f: impl FnMut() -> T) -> (f64, T) {
    let mut out = None;
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        let v = f();
        best = best.min(start.elapsed().as_secs_f64());
        out = Some(v);
    }
    (best, out.unwrap())
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss)] // the counts are far below 2^52
fn line_table_throughput() {
    let functions = input();
    let names: Vec<Vec<u8>> = (0..50).map(|k| format!("file{k}.h").into_bytes()).collect();
    let dirs: Vec<&[u8]> = vec![b"/work/src", b"/usr/include/c++/v1"];
    let files: Vec<FileEntry<'_>> = names
        .iter()
        .enumerate()
        .map(|(k, n)| FileEntry { name: n, dir: (k % 2) as u32, md5: Some([k as u8; 16]) })
        .collect();
    let header = LineHeader { dirs: &dirs, files: &files };

    // The policy: every function's rows into one buffer, with the start
    // of each function's rows.
    let (policy_s, (rows, starts)) = best(|| {
        let mut rows: Vec<Row> = Vec::with_capacity(functions.len() * (PER_FUNCTION as usize + 1));
        let mut starts = Vec::with_capacity(functions.len() + 1);
        for (ts, size) in &functions {
            starts.push(rows.len());
            let scope = ts[0].loc.unwrap();
            let f = FunctionLines {
                scope: SourceLoc { line: scope.line.saturating_sub(1).max(1), ..scope },
                size: *size,
                transitions: ts,
                epilogue: false,
            };
            function_rows(&f, &mut rows).unwrap();
        }
        starts.push(rows.len());
        (rows, starts)
    });

    let total: u64 = functions.iter().map(|(_, s)| u64::from(*s)).sum();
    // Only the writer is timed, not making the object.
    let mut write_s = f64::MAX;
    let (_, (line_bytes, str_bytes)) = best(|| {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append_zeros(text, total);
        let sym = obj.section_symbol(text);
        let mut addend = 0i64;
        let seqs: Vec<Sequence<'_>> = functions
            .iter()
            .enumerate()
            .map(|(i, (_, size))| {
                let s = Sequence {
                    order: i as u32,
                    symbol: sym,
                    addend,
                    size: *size,
                    rows: &rows[starts[i]..starts[i + 1]],
                };
                addend += i64::from(*size);
                s
            })
            .collect();
        let start = Instant::now();
        let out = write_debug_line(&mut obj, &header, &seqs).unwrap();
        write_s = write_s.min(start.elapsed().as_secs_f64());
        (obj.data(out.line).len(), obj.data(out.line_str).len())
    });

    let n = rows.len() as f64;
    let mb = line_bytes as f64 / 1e6;
    eprintln!(
        "{} functions, {} rows: policy {:.1} ns/row ({:.1} ms); writer {:.1} ns/row ({:.1} ms, {:.0} MB/s of .debug_line); {} bytes of .debug_line ({:.2} bytes/row), {} of .debug_line_str",
        functions.len(),
        rows.len(),
        policy_s * 1e9 / n,
        policy_s * 1e3,
        write_s * 1e9 / n,
        write_s * 1e3,
        mb / write_s,
        line_bytes,
        line_bytes as f64 / n,
        str_bytes,
    );
    assert!(rows.len() > (FUNCTIONS * PER_FUNCTION) as usize * 9 / 10);
}
