//! The merge of function chunks into one object (`spec/codegen.md` §9.7,
//! §11): generated programs compiled by the test target, divided into
//! chunks every way the scheduler might divide them and compiled on more
//! than one thread, must give byte-identical objects and identical merged
//! rows; each function's code, size, relocations and rows must be carried
//! through exactly.

mod common;

use common::harness::option_sets;
use common::target::Tm;
use hod_cg::{
    CallSiteRow, Chunk, ClauseRow, Emitted, NO_PAD, Options, PadRow, RelocModel, Scratch, SymRef,
    compile_function, emit_module,
};
use hod_interp::generate::{Generated, generate};
use hod_obj::{SymbolId, SymbolPlace};

/// Compiles `bodies[range]` into a new chunk.
fn compile(tm: &Tm, program: &Generated, range: std::ops::Range<usize>) -> Chunk {
    let mut chunk = Chunk::new();
    let mut scratch = Scratch::default();
    for body in &program.bodies[range] {
        compile_function(tm, &program.module, body, &Options::default(), &mut scratch, &mut chunk)
            .expect("the generated program compiles");
    }
    chunk
}

/// The chunks of `parts` contiguous runs of the bodies.
fn divided(tm: &Tm, program: &Generated, parts: usize) -> Vec<Chunk> {
    let n = program.bodies.len();
    (0..parts).map(|i| compile(tm, program, i * n / parts..(i + 1) * n / parts)).collect()
}

/// The same, compiled on `threads` threads.
fn threaded(tm: &Tm, program: &Generated, threads: usize) -> Vec<Chunk> {
    let n = program.bodies.len();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|i| {
                scope.spawn(move || compile(tm, program, i * n / threads..(i + 1) * n / threads))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn extend<T: Clone>(
    to: &mut Vec<T>,
    from: &[T],
    range: &std::ops::Range<u32>,
    extra: Vec<T>,
) -> std::ops::Range<u32> {
    let start = to.len() as u32;
    to.extend_from_slice(&from[range.start as usize..range.end as usize]);
    to.extend(extra);
    start..to.len() as u32
}

/// `chunk` with, for each function, landing-pad rows the test target
/// does not make: a call site, a pad with a catch and a filter clause,
/// two type ids and a filter id, at the end of the function's ranges.
fn with_eh_rows(chunk: &Chunk) -> Chunk {
    let mut out = chunk.clone();
    out.funcs.clear();
    out.call_sites.clear();
    out.pads.clear();
    out.clauses.clear();
    out.type_ids.clear();
    out.filter_ids.clear();
    for func in &chunk.funcs {
        let mut func = func.clone();
        // Rows of the function's own: not of where it is in the chunk.
        let n = func.sym.index() as u32;
        func.call_sites = extend(
            &mut out.call_sites,
            &chunk.call_sites,
            &func.call_sites,
            vec![
                CallSiteRow { start: n, len: 4, pad: 0, landing: 0 },
                CallSiteRow { start: n + 4, len: 4, pad: NO_PAD, landing: 0 },
            ],
        );
        func.pads = extend(
            &mut out.pads,
            &chunk.pads,
            &func.pads,
            vec![PadRow {
                block: hod_ir::BlockId::new(n),
                offset: n,
                cleanup: n.is_multiple_of(2),
                clauses: 0..2,
            }],
        );
        func.clauses = extend(
            &mut out.clauses,
            &chunk.clauses,
            &func.clauses,
            vec![ClauseRow::Catch(1), ClauseRow::Filter(0..1)],
        );
        func.type_ids =
            extend(&mut out.type_ids, &chunk.type_ids, &func.type_ids, vec![None, Some(func.sym)]);
        func.filter_ids =
            extend(&mut out.filter_ids, &chunk.filter_ids, &func.filter_ids, vec![n + 1]);
        func.has_pads = true;
        out.funcs.push(func);
    }
    out
}

fn bytes(emitted: &Emitted) -> Vec<u8> {
    hod_obj::elf::to_vec(&emitted.object).unwrap()
}

fn slice<T: Clone>(v: &[T], range: &std::ops::Range<u32>) -> Vec<T> {
    v[range.start as usize..range.end as usize].to_vec()
}

/// Every function of the single-chunk compile is in the object as it was
/// compiled: code, symbol, relocations, rows.
fn check_carried(program: &Generated, chunk: &Chunk, emitted: &Emitted) {
    let obj = &emitted.object;
    assert_eq!(emitted.funcs.len(), chunk.funcs.len());
    for (func, placed) in chunk.funcs.iter().zip(&emitted.funcs) {
        assert_eq!(func.sym, placed.record.sym);
        let code = chunk.code_of(func);
        let at = placed.record.code.start as usize;
        assert_eq!(&obj.data(placed.section)[at..at + code.len()], code, "code of {:?}", func.sym);
        let symbol = obj.symbol(SymbolId::from_index(func.sym.index() as u32));
        assert_eq!(symbol.place, SymbolPlace::Section(placed.section));
        assert_eq!(symbol.value, at as u64);
        assert_eq!(symbol.size, code.len() as u64);
        assert_eq!(symbol.kind, hod_obj::SymbolKind::Func);
        // Relocations: the chunk's, rebased, against the module's symbols.
        let relocs: Vec<_> = obj
            .relocations(placed.section)
            .filter(|r| r.offset >= at as u64 && r.offset < (at + code.len()) as u64)
            .collect();
        let ours = &chunk.relocs[func.relocs.start as usize..func.relocs.end as usize];
        assert_eq!(relocs.len(), ours.len(), "relocations of {:?}", func.sym);
        for (r, o) in relocs.iter().zip(ours) {
            assert_eq!(r.offset, at as u64 + u64::from(o.offset));
            assert_eq!(r.kind, o.kind);
            let name = obj.string(obj.symbol(r.symbol).name);
            match o.sym {
                SymRef::Module(sym) => {
                    let symbol = program.module.symbol(sym);
                    if obj.symbol(r.symbol).kind == hod_obj::SymbolKind::Section {
                        assert!(symbol.is_variable(), "a section relocation for {sym:?}");
                    } else {
                        assert_eq!(r.symbol.index() as usize, sym.index());
                        assert_eq!(r.addend, o.addend);
                    }
                }
                SymRef::Runtime(f) => assert_eq!(name, f.name().as_bytes()),
            }
        }
        // Rows: the same rows, function-relative, in the merged tables.
        let rows = &emitted.rows;
        let r = &placed.record;
        assert_eq!(slice(&rows.cfi, &r.cfi), slice(&chunk.cfi, &func.cfi));
        assert_eq!(
            slice(&rows.call_sites, &r.call_sites),
            slice(&chunk.call_sites, &func.call_sites)
        );
        assert_eq!(slice(&rows.pads, &r.pads), slice(&chunk.pads, &func.pads));
        assert_eq!(slice(&rows.clauses, &r.clauses), slice(&chunk.clauses, &func.clauses));
        assert_eq!(slice(&rows.type_ids, &r.type_ids), slice(&chunk.type_ids, &func.type_ids));
        assert_eq!(
            slice(&rows.filter_ids, &r.filter_ids),
            slice(&chunk.filter_ids, &func.filter_ids)
        );
        assert_eq!(slice(&rows.vars, &r.vars), slice(&chunk.vars, &func.vars));
        assert_eq!(slice(&rows.lines, &r.lines), slice(&chunk.lines, &func.lines));
        assert_eq!(slice(&rows.islands, &r.islands), slice(&chunk.islands, &func.islands));
        assert_eq!(
            slice(&rows.block_offsets, &r.block_offsets),
            slice(&chunk.block_offsets, &func.block_offsets)
        );
        assert_eq!(r.frame, func.frame);
        assert_eq!(r.has_pads, func.has_pads);
        assert_eq!(r.align, func.align);
        assert!(placed.record.code.start.is_multiple_of(func.align.max(1)));
    }
}

#[test]
fn merge_is_independent_of_chunks_and_threads() {
    let tm = Tm::default();
    let mut functions = 0;
    let mut cfi = 0;
    let mut call_sites = 0;
    let mut lines = 0;
    let mut relocs = 0;
    let sets = option_sets();
    for seed in 0..(2 * sets.len() as u64) {
        let program = generate(0x3200 + seed, &sets[seed as usize / 2].1);
        for reloc_model in [RelocModel::Static, RelocModel::Pic] {
            let options = Options { reloc_model, ..Options::default() };
            let one = vec![with_eh_rows(&divided(&tm, &program, 1)[0])];
            let reference = emit_module(&program.module, &options, &one).expect("emission");
            check_carried(&program, &one[0], &reference);
            let expected = bytes(&reference);
            let n = program.bodies.len();
            for parts in [2, 3, n.max(1)] {
                let chunks: Vec<Chunk> =
                    divided(&tm, &program, parts).iter().map(with_eh_rows).collect();
                let emitted = emit_module(&program.module, &options, &chunks).expect("emission");
                assert_eq!(bytes(&emitted), expected, "seed {seed}: {parts} chunks");
                assert_eq!(emitted.funcs, reference.funcs, "seed {seed}: {parts} chunks");
                assert_eq!(emitted.rows, reference.rows, "seed {seed}: {parts} chunks");
            }
            let chunks: Vec<Chunk> = threaded(&tm, &program, 2).iter().map(with_eh_rows).collect();
            let emitted = emit_module(&program.module, &options, &chunks).expect("emission");
            assert_eq!(bytes(&emitted), expected, "seed {seed}: 2 threads");
            functions += one[0].funcs.len();
            cfi += one[0].cfi.len();
            call_sites += one[0].call_sites.len();
            relocs += one[0].relocs.len();
            lines += one[0].lines.len();
        }
    }
    eprintln!(
        "emit_merge: {functions} functions merged, {relocs} relocations, {cfi} CFI rows, {call_sites} call sites, {lines} line rows"
    );
    assert!(functions > 50 && relocs > 0);
}
