//! `spec/eh.md` §8: the cost of `write_eh_frame` per FDE, for a synthetic
//! object of 100,000 functions with the AArch64 rows of
//! `spec/codegen.md` §5.5 (eight rows each), every eighth with a
//! personality and an LSDA. Run with
//! `cargo test -p hod_eh --release --test throughput -- --ignored --nocapture`.

use std::time::Instant;

use hod_eh::{Fde, Lsda, Op, Row, write_eh_frame};
use hod_obj::{Arch, Object, StrId, Symbol};

#[test]
#[ignore = "a measurement"]
fn eh_frame_per_fde() {
    const N: u32 = 100_000;
    let rows: Vec<Vec<Row>> = (0..N)
        .map(|i| {
            let frame = 16 * (2 + i as i32 % 300);
            let size = 64 + 4 * (i % 500);
            vec![
                Row { offset: 4, op: Op::DefCfaOffset(frame % 4096) },
                Row { offset: 8, op: Op::DefCfaOffset(frame) },
                Row { offset: 12, op: Op::Offset { reg: 29, cfa_offset: -frame } },
                Row { offset: 12, op: Op::Offset { reg: 30, cfa_offset: 8 - frame } },
                Row { offset: 16, op: Op::DefCfa { reg: 29, offset: frame } },
                Row { offset: size - 12, op: Op::DefCfa { reg: 31, offset: frame } },
                Row { offset: size - 8, op: Op::Restore { reg: 29 } },
                Row { offset: size - 8, op: Op::Restore { reg: 30 } },
            ]
        })
        .collect();
    let mut best = f64::INFINITY;
    let mut bytes = 0;
    for _ in 0..5 {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        let personality = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        let table = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        let mut at = 0i64;
        let fdes: Vec<Fde<'_>> = rows
            .iter()
            .enumerate()
            .map(|(i, rows)| {
                let size = 64 + 4 * (i as u32 % 500);
                let eh = i % 8 == 0;
                let fde = Fde {
                    order: i as u32,
                    start: text,
                    start_addend: at,
                    size,
                    rows,
                    personality: eh.then_some(personality),
                    lsda: eh.then_some(Lsda { symbol: table, addend: i as i64 * 16 }),
                };
                at += i64::from(size);
                fde
            })
            .collect();
        let start = Instant::now();
        let section = write_eh_frame(&mut obj, &fdes).unwrap().unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        bytes = obj.data(section).len() as u32;
        best = best.min(elapsed);
    }
    eprintln!(
        "{N} FDEs, {bytes} bytes of .eh_frame: {:.2} ms, {:.1} ns per FDE, {:.0} MB/s",
        best * 1e3,
        best * 1e9 / f64::from(N),
        f64::from(bytes) / best / 1e6
    );
}
