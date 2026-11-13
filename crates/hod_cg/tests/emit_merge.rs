//! The merge of function chunks into one object (`spec/codegen.md` §9.7,
//! §11): generated programs compiled by the test target, divided into
//! chunks every way the scheduler might divide them and compiled on more
//! than one thread, must give byte-identical objects and identical merged
//! rows; each function's code, size, relocations and rows must be carried
//! through exactly; the unwind tables are read back and must be each
//! function's (`spec/eh.md` §1.1, §9.1), whatever the chunk boundaries.

mod common;

use common::ehview;
use common::harness::option_sets;
use common::target::Tm;
use hod_cg::{
    CallSiteRow, Chunk, ClauseRow, Emitted, NO_PAD, Options, PadRow, RelocModel, Scratch, SymRef,
    compile_function, emit_module,
};
use hod_interp::generate::{Generated, generate};
use hod_ir::SymId;
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

/// The chunks between consecutive `cuts` (some of them empty).
fn cut(tm: &Tm, program: &Generated, cuts: &[usize]) -> Vec<Chunk> {
    cuts.windows(2).map(|w| compile(tm, program, w[0]..w[1])).collect()
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

/// `chunk` with landing-pad rows the test target does not make, for each
/// function large enough that has none of its own: a call site with a
/// pad and two without (one of them `nounwind`), a pad with a catch and
/// a filter clause, two type ids (catch-all and the function's own
/// symbol, so that each LSDA names its function) and a filter id; and
/// unwind attributes that vary with the symbol, so that some functions
/// get no FDE (`spec/eh.md` §1.1).
fn with_eh_rows(chunk: &Chunk, personality: SymId) -> Chunk {
    let mut out = chunk.clone();
    out.funcs.clear();
    out.call_sites.clear();
    out.pads.clear();
    out.clauses.clear();
    out.type_ids.clear();
    out.filter_ids.clear();
    for func in &chunk.funcs {
        let mut func = func.clone();
        let n = func.sym.index() as u32;
        let size = func.code.end - func.code.start;
        let synthetic = !func.has_pads && size >= 16 && n % 4 != 3;
        let (sites, pads, clauses, types, filters) = if synthetic {
            func.has_pads = true;
            func.personality = Some(personality);
            (
                vec![
                    CallSiteRow { start: 0, len: 4, pad: 0, landing: 0, nounwind: false },
                    CallSiteRow {
                        start: 4,
                        len: 4,
                        pad: NO_PAD,
                        landing: 0,
                        nounwind: n.is_multiple_of(2),
                    },
                    CallSiteRow { start: 8, len: 4, pad: NO_PAD, landing: 0, nounwind: false },
                ],
                vec![PadRow {
                    block: hod_ir::BlockId::new(n),
                    offset: size - 4,
                    cleanup: n.is_multiple_of(2),
                    clauses: 0..2,
                }],
                vec![ClauseRow::Catch(1), ClauseRow::Filter(0..1)],
                vec![None, Some(func.sym)],
                vec![2],
            )
        } else {
            if !func.has_pads {
                func.nounwind = (n / 4).is_multiple_of(2);
                func.uwtable = (n / 4).is_multiple_of(3);
            }
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };
        func.call_sites = extend(&mut out.call_sites, &chunk.call_sites, &func.call_sites, sites);
        func.pads = extend(&mut out.pads, &chunk.pads, &func.pads, pads);
        func.clauses = extend(&mut out.clauses, &chunk.clauses, &func.clauses, clauses);
        func.type_ids = extend(&mut out.type_ids, &chunk.type_ids, &func.type_ids, types);
        func.filter_ids = extend(&mut out.filter_ids, &chunk.filter_ids, &func.filter_ids, filters);
        out.funcs.push(func);
    }
    out
}

/// The section symbol and addend `target` resolves to as a section and
/// offset.
fn place_of(obj: &hod_obj::Object, target: (SymbolId, i64)) -> Option<(hod_obj::SectionId, u64)> {
    ehview::section_of(obj, target.0).map(|section| (section, target.1 as u64))
}

/// `spec/eh.md` §1.1, §9.1 read back from the object: exactly one FDE per
/// function that needs one, at the function's place, of its size, with
/// its rows; an LSDA exactly for a function with pads, whose landing
/// pads and type table are that function's; the personality's slot.
/// Returns the number of FDEs and of LSDAs.
fn check_tables(program: &Generated, emitted: &Emitted) -> (usize, usize) {
    let obj = &emitted.object;
    let rows = &emitted.rows;
    let expected = emitted.funcs.iter().filter(|f| f.record.needs_unwind_entry()).count();
    let Some(section) = ehview::find_section(obj, b".eh_frame") else {
        assert_eq!(expected, 0, "no .eh_frame for {expected} functions that need one");
        return (0, 0);
    };
    let reloc = |offset| ehview::object_reloc(obj, section, offset);
    let (cies, fdes) = ehview::eh_frame(obj.data(section), &reloc).expect(".eh_frame decodes");
    assert_eq!(fdes.len(), expected, "one FDE per function that needs one");
    let mut lsdas = 0;
    for placed in &emitted.funcs {
        let r = &placed.record;
        let name = String::from_utf8_lossy(program.module.symbol(r.sym).name()).into_owned();
        let found: Vec<_> = fdes
            .iter()
            .filter(|f| place_of(obj, f.start) == Some((placed.section, u64::from(r.code.start))))
            .collect();
        if !r.needs_unwind_entry() {
            assert!(found.is_empty(), "{name}: an FDE for a function that needs none");
            continue;
        }
        let [fde] = found[..] else { panic!("{name}: {} FDEs at its place", found.len()) };
        assert_eq!(fde.size, r.code.end - r.code.start, "{name}: FDE range");
        assert_eq!(fde.rows, slice(&rows.cfi, &r.cfi), "{name}: rows");
        assert!(!fde.remembered);
        let cie = &cies[fde.cie];
        let Some(lsda) = fde.lsda else {
            assert!(!r.has_pads, "{name}: no LSDA");
            assert_eq!(cie.augmentation, "zR", "{name}: a personality without pads");
            continue;
        };
        assert!(r.has_pads, "{name}: an LSDA without pads");
        lsdas += 1;
        // The personality: the slot of this function's routine.
        let (slot, _) = cie.personality.expect("a personality");
        let personality = program.module.symbol(r.personality.unwrap()).name();
        assert_eq!(obj.string(obj.symbol(slot).name), [b"DW.ref.", personality].concat());
        // The LSDA is this function's: its landing pads and types.
        let (table, at) = place_of(obj, lsda).expect("an LSDA in a section");
        assert!(obj.section_name(table).starts_with(b".gcc_except_table"));
        let treloc = |offset| ehview::object_reloc(obj, table, offset);
        let view = ehview::lsda(obj.data(table), at as usize, 8, &treloc).expect("LSDA decodes");
        let pads = slice(&rows.pads, &r.pads);
        let sites: Vec<_> = slice(&rows.call_sites, &r.call_sites)
            .into_iter()
            .filter(|s| s.pad != NO_PAD)
            .collect();
        for site in &sites {
            let landing =
                if site.landing != 0 { site.landing } else { pads[site.pad as usize].offset };
            let entry = view
                .sites
                .iter()
                .find(|e| e.0 <= u64::from(site.start) && u64::from(site.start) < e.0 + e.1);
            let entry =
                entry.unwrap_or_else(|| panic!("{name}: no entry for the call at {}", site.start));
            assert_eq!(
                entry.2,
                u64::from(landing),
                "{name}: landing pad of the call at {}",
                site.start
            );
        }
        let types = slice(&rows.type_ids, &r.type_ids);
        let stubs = ehview::find_section(obj, b".data");
        for (k, ty) in types.iter().enumerate() {
            let Some(entry) = view.types.get(k) else { continue };
            match (ty, entry) {
                (None, None) => {}
                (Some(sym), Some(stub)) => {
                    let (stub_section, at) = place_of(obj, *stub).expect("a stub in .data");
                    assert_eq!(Some(stub_section), stubs);
                    let (target, _) = ehview::object_reloc(obj, stub_section, at).expect("a stub");
                    assert_eq!(target.index() as usize, sym.index(), "{name}: type id {}", k + 1);
                }
                other => panic!("{name}: type id {}: {other:?}", k + 1),
            }
        }
    }
    (fdes.len(), lsdas)
}

/// Cut points for `parts` chunks of `n` functions, drawn from `seed`.
fn shuffled_cuts(n: usize, seed: u64) -> Vec<usize> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut cuts: Vec<usize> = (0..4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % (n as u64 + 1)) as usize
        })
        .collect();
    cuts.push(0);
    cuts.push(n);
    cuts.sort_unstable();
    cuts
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
                // The test target never refers to it (WP-105).
                SymRef::NoopCoroFrame => unreachable!("the noop coroutine frame"),
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
        assert_eq!(
            (r.personality, r.nounwind, r.uwtable),
            (func.personality, func.nounwind, func.uwtable)
        );
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
    let (mut fdes, mut lsdas) = (0, 0);
    let sets = option_sets();
    for seed in 0..(2 * sets.len() as u64) {
        let program = generate(0x3200 + seed, &sets[seed as usize / 2].1);
        let personality = program
            .module
            .symbols()
            .find(|s| matches!(s.kind(), hod_ir::SymbolKind::Function { .. }))
            .map(|s| s.id())
            .expect("a function");
        let eh = |chunk: &Chunk| with_eh_rows(chunk, personality);
        for (reloc_model, function_sections) in
            [(RelocModel::Static, false), (RelocModel::Pic, false), (RelocModel::Pic, true)]
        {
            let options = Options { reloc_model, function_sections, ..Options::default() };
            let one = vec![eh(&divided(&tm, &program, 1)[0])];
            let reference = emit_module(&program.module, &options, &one).expect("emission");
            check_carried(&program, &one[0], &reference);
            let (f, l) = check_tables(&program, &reference);
            (fdes, lsdas) = (fdes + f, lsdas + l);
            let expected = bytes(&reference);
            let n = program.bodies.len();
            for parts in [2, 3, n.max(1)] {
                let chunks: Vec<Chunk> = divided(&tm, &program, parts).iter().map(eh).collect();
                let emitted = emit_module(&program.module, &options, &chunks).expect("emission");
                assert_eq!(bytes(&emitted), expected, "seed {seed}: {parts} chunks");
                assert_eq!(emitted.funcs, reference.funcs, "seed {seed}: {parts} chunks");
                assert_eq!(emitted.rows, reference.rows, "seed {seed}: {parts} chunks");
            }
            for draw in 0..3 {
                let cuts = shuffled_cuts(n, seed * 3 + draw);
                let chunks: Vec<Chunk> = cut(&tm, &program, &cuts).iter().map(eh).collect();
                let emitted = emit_module(&program.module, &options, &chunks).expect("emission");
                assert_eq!(bytes(&emitted), expected, "seed {seed}: cuts {cuts:?}");
            }
            let chunks: Vec<Chunk> = threaded(&tm, &program, 2).iter().map(eh).collect();
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
        "emit_merge: {functions} functions merged, {relocs} relocations, {cfi} CFI rows, {call_sites} call sites, {lines} line rows, {fdes} FDEs, {lsdas} LSDAs"
    );
    assert!(functions > 50 && relocs > 0);
    assert!(functions > fdes && fdes > lsdas && lsdas > 20, "{fdes} FDEs, {lsdas} LSDAs");
}
