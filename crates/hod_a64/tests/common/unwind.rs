//! WP-038: test glue that gives an emitted module its exception tables
//! (`.eh_frame` and `.gcc_except_table`) from the rows the core records,
//! through `hod_eh::write_eh_frame` and `hod_eh::write_lsdas`, so that
//! real exceptions can be thrown through hod's frames before module
//! emission writes the tables itself.
//!
//! **Replaced by `emit_module`'s own tables when WP-049 lands.**
//!
//! What the records lack for that writer (the WP-049 list): a function's
//! personality is only in the IR (`hod_ir::SymbolKind::Function`), not in
//! `hod_cg::FuncRecord` (`crates/hod_cg/src/record.rs:187`), so this glue
//! looks it up per function; the type-info table holds IR symbols
//! (`Chunk::type_ids`, `record.rs:244`), which are object symbols only
//! because emission numbers them alike (`hod_cg/src/emit/mod.rs:339`),
//! and a declaration no code refers to (the personality, a type-info
//! object) is left unnamed (`emit/mod.rs:363`), so this glue names it.

use hod_cg::{ClauseRow, Emitted};
use hod_eh::{CallSite, Clause, Fde, FuncLsda, LsdaOptions, Op, Pad, Row};
use hod_ir::{Module, SymbolKind};
use hod_obj::SymbolId;

fn op(op: hod_cg::CfiOp) -> Op {
    match op {
        hod_cg::CfiOp::DefCfa { reg, offset } => Op::DefCfa { reg, offset },
        hod_cg::CfiOp::DefCfaOffset(offset) => Op::DefCfaOffset(offset),
        hod_cg::CfiOp::DefCfaRegister(reg) => Op::DefCfaRegister(reg),
        hod_cg::CfiOp::Offset { reg, cfa_offset } => Op::Offset { reg, cfa_offset },
        hod_cg::CfiOp::Restore { reg } => Op::Restore { reg },
    }
}

fn sym(id: hod_ir::SymId) -> SymbolId {
    SymbolId::from_index(id.index() as u32)
}

/// The object symbol of `id`, named if emission left it unnamed.
fn named(module: &Module, obj: &mut hod_obj::Object, id: hod_ir::SymId) -> SymbolId {
    let symbol = sym(id);
    if obj.symbol(symbol).name == hod_obj::StrId::EMPTY {
        let name = obj.add_string(module.symbol(id).name()).expect("a name");
        obj.symbol_mut(symbol).name = name;
    }
    symbol
}

/// Writes `.eh_frame` (one FDE per function) and the LSDA of every
/// function with a personality into `emitted.object`.
///
/// # Panics
/// Panics when a writer refuses the rows.
pub fn add_eh_tables(module: &Module, emitted: &mut Emitted) {
    let rows = &emitted.rows;
    let obj = &mut emitted.object;
    struct Func {
        rows: Vec<Row>,
        call_sites: Vec<CallSite>,
        pads: Vec<Pad>,
        clauses: Vec<Clause>,
        type_ids: Vec<Option<SymbolId>>,
        personality: Option<SymbolId>,
    }
    let mut funcs = Vec::new();
    for placed in &emitted.funcs {
        let r = &placed.record;
        let range = |range: &std::ops::Range<u32>| range.start as usize..range.end as usize;
        let personality = match module.symbol(r.sym).kind() {
            SymbolKind::Function { personality, .. } => personality.map(|p| named(module, obj, p)),
            _ => None,
        };
        funcs.push(Func {
            rows: rows.cfi[range(&r.cfi)]
                .iter()
                .map(|c| Row { offset: c.offset, op: op(c.op) })
                .collect(),
            call_sites: rows.call_sites[range(&r.call_sites)]
                .iter()
                .map(|c| CallSite { start: c.start, len: c.len, pad: c.pad, landing: c.landing })
                .collect(),
            pads: rows.pads[range(&r.pads)]
                .iter()
                .map(|p| Pad {
                    block: p.block.index() as u32,
                    offset: p.offset,
                    cleanup: p.cleanup,
                    clauses: p.clauses.clone(),
                })
                .collect(),
            clauses: rows.clauses[range(&r.clauses)]
                .iter()
                .map(|c| match c {
                    ClauseRow::Catch(id) => Clause::Catch(*id),
                    ClauseRow::Filter(ids) => Clause::Filter(ids.clone()),
                })
                .collect(),
            type_ids: rows.type_ids[range(&r.type_ids)]
                .iter()
                .map(|t| t.map(|t| named(module, obj, t)))
                .collect(),
            personality,
        });
    }
    let names: Vec<Vec<u8>> =
        emitted.funcs.iter().map(|p| module.symbol(p.record.sym).name().to_vec()).collect();
    let mut lsda_funcs = Vec::new();
    let mut with_lsda = Vec::new();
    for (order, (placed, f)) in emitted.funcs.iter().zip(&funcs).enumerate() {
        if f.personality.is_none() {
            continue;
        }
        let r = &placed.record;
        let range = r.filter_ids.start as usize..r.filter_ids.end as usize;
        lsda_funcs.push(FuncLsda {
            order: order as u32,
            name: &names[order],
            section: placed.section,
            size: r.code.end - r.code.start,
            call_sites: &f.call_sites,
            pads: &f.pads,
            clauses: &f.clauses,
            type_ids: &f.type_ids,
            filter_ids: &rows.filter_ids[range],
        });
        with_lsda.push(order);
    }
    let lsdas = hod_eh::write_lsdas(obj, &lsda_funcs, LsdaOptions::default())
        .unwrap_or_else(|e| panic!("write_lsdas: {e}"));
    let mut slots: Vec<(SymbolId, SymbolId)> = Vec::new();
    let mut fdes = Vec::new();
    for (order, (placed, f)) in emitted.funcs.iter().zip(&funcs).enumerate() {
        let r = &placed.record;
        let personality = f.personality.map(|p| {
            if let Some(&(_, slot)) = slots.iter().find(|(of, _)| *of == p) {
                return slot;
            }
            let slot = hod_eh::personality_ref(obj, p).expect("personality slot");
            slots.push((p, slot));
            slot
        });
        let lsda = with_lsda.iter().position(|&o| o == order).map(|i| lsdas[i]);
        fdes.push(Fde {
            order: order as u32,
            start: obj.section_symbol(placed.section),
            start_addend: i64::from(r.code.start),
            size: r.code.end - r.code.start,
            rows: &f.rows,
            personality,
            lsda,
        });
    }
    hod_eh::write_eh_frame(obj, &fdes).unwrap_or_else(|e| panic!("write_eh_frame: {e}"));
}
