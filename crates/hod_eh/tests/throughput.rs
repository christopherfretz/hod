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

/// `spec/eh.md` §9.7: the cost of `write_lsdas` per call site and per
/// function, for 100,000 functions of 16 call sites (12 with a pad, 4
/// without), 3 pads (a cleanup with two catches, a catch-all, a cleanup
/// only) and 3 type ids, every eighth with a filter, in one
/// `.gcc_except_table`. Run with
/// `cargo test -p hod_eh --release --test throughput -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement"]
fn lsda_per_call_site() {
    use hod_eh::{CallSite, Clause, FuncLsda, LsdaOptions, NO_PAD, Pad, write_lsdas};
    const N: u32 = 100_000;
    const SITES: u32 = 16;
    let sites: Vec<CallSite> = (0..SITES)
        .map(|i| CallSite {
            start: 8 + 12 * i,
            len: 8,
            pad: if i % 4 == 3 { NO_PAD } else { i % 3 },
            landing: 0,
        })
        .collect();
    let pads = [
        Pad { block: 1, offset: 400, cleanup: true, clauses: 0..2 },
        Pad { block: 2, offset: 420, cleanup: false, clauses: 2..3 },
        Pad { block: 3, offset: 440, cleanup: true, clauses: 3..3 },
    ];
    let plain = [Clause::Catch(1), Clause::Catch(2), Clause::Catch(3)];
    let filtered = [Clause::Catch(1), Clause::Filter(0..2), Clause::Catch(3)];
    let filter_ids = [1, 2];
    let mut best = f64::INFINITY;
    let mut bytes = 0;
    for _ in 0..5 {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", hod_obj::SectionKind::Text, 4).unwrap();
        let ti: Vec<_> = (0..64)
            .map(|i| {
                let name = obj.add_string(format!("_ZTI{i}").as_bytes()).unwrap();
                obj.add_symbol(Symbol::undefined(name))
            })
            .collect();
        let type_ids: Vec<[Option<hod_obj::SymbolId>; 3]> =
            (0..N as usize).map(|i| [Some(ti[i % 64]), Some(ti[(i * 7 + 1) % 64]), None]).collect();
        let funcs: Vec<FuncLsda<'_>> = (0..N)
            .map(|i| FuncLsda {
                order: i,
                name: b"f",
                section: text,
                size: 460,
                call_sites: &sites,
                pads: &pads,
                clauses: if i % 8 == 0 { &filtered } else { &plain },
                type_ids: &type_ids[i as usize],
                filter_ids: &filter_ids,
            })
            .collect();
        let start = Instant::now();
        let lsdas = write_lsdas(&mut obj, &funcs, LsdaOptions::default()).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        bytes = lsdas.last().unwrap().addend as u64;
        best = best.min(elapsed);
    }
    let _ = StrId::EMPTY;
    eprintln!(
        "{N} LSDAs, {} call sites, ~{bytes} bytes: {:.2} ms, {:.1} ns per function, \
         {:.1} ns per call site",
        N * SITES,
        best * 1e3,
        best * 1e9 / f64::from(N),
        best * 1e9 / f64::from(N * SITES)
    );
}
