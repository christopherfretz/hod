//! The unwind tables of the placed functions (`spec/codegen.md` §9.7,
//! `spec/eh.md` §1.1 and §9.1): `.gcc_except_table` from the call-site,
//! pad and clause rows, then `.eh_frame` from the CFI rows, each in one
//! `hod_eh` call over the whole object; and in a module with debug
//! information, `.debug_frame` from the CFI rows of the functions that
//! need no `.eh_frame` entry (`spec/eh.md` §10, WP-150).
//!
//! Everything is read from the merged rows ([`Emitted::rows`]) and the
//! placed records, which do not depend on how the functions were divided
//! into chunks, so neither do the tables. The CFI rows are `hod_eh`'s own
//! row type and are passed as they are; the LSDA rows are mapped once per
//! object into buffers sized from the row counts (a pad's block becomes
//! its index, a type id's `SymId` its `SymbolId`, and a call that cannot
//! unwind and has no pad is left out, as `llc` leaves it out).
//!
//! [`Emitted::rows`]: super::Emitted::rows

use std::ops::Range;

use hod_eh::{CallSite, Fde, FuncLsda, Lsda, LsdaOptions, Pad};
use hod_ir::{Linkage, SymId};
use hod_obj::{StrId, SymbolId};

use super::section::Kind;
use super::{ModuleEmitter, obj_err, unsupported};
use crate::error::{CgError, CgResult};
use crate::record::NO_PAD;

/// The personalities LLVM knows (`classifyEHPersonality`). Each of them
/// does nothing for a function without landing pads, so such a function
/// gets no personality and no LSDA (`isNoOpWithoutInvoke`).
const KNOWN_PERSONALITIES: [&[u8]; 17] = [
    b"__gnat_eh_personality",
    b"__gxx_personality_seh0",
    b"__gxx_personality_v0",
    b"__gxx_personality_sj0",
    b"__gcc_personality_v0",
    b"__gcc_personality_seh0",
    b"__gcc_personality_sj0",
    b"__objc_personality_v0",
    b"_except_handler3",
    b"_except_handler4",
    b"__C_specific_handler",
    b"__CxxFrameHandler3",
    b"ProcessCLRException",
    b"rust_eh_personality",
    b"__gxx_wasm_personality_v0",
    b"__xlcxx_personality_v1",
    b"__zos_cxx_personality_v2",
];

/// One function's ranges of the mapped LSDA buffers.
struct Ranges {
    func: u32,
    call_sites: Range<usize>,
    pads: Range<usize>,
    type_ids: Range<usize>,
}

fn obj_sym(sym: SymId) -> SymbolId {
    SymbolId::from_index(sym.index() as u32)
}

fn eh_err(error: hod_eh::Error) -> CgError {
    CgError::internal(format!("unwind tables: {error}"))
}

impl ModuleEmitter<'_> {
    /// Gives a module symbol its name in the object now, if it has none
    /// yet (a declaration, or a named private symbol, is named only once
    /// something refers to it): `hod_eh` reads the names of the
    /// personality and of the type-info symbols.
    pub(super) fn name_symbol(&mut self, sym: SymId) -> CgResult<()> {
        let id = obj_sym(sym);
        if self.obj.symbol(id).name != StrId::EMPTY {
            return Ok(());
        }
        let symbol = self.module.symbol(sym);
        let mut name = std::mem::take(&mut self.name);
        name.clear();
        if symbol.is_definition() && symbol.linkage() == Linkage::Private {
            name.extend_from_slice(b".L");
        }
        name.extend_from_slice(symbol.name());
        let added = self.obj.add_string(&name).map_err(obj_err);
        self.name = name;
        self.obj.symbol_mut(id).name = added?;
        Ok(())
    }

    /// Writes the LSDAs and `.eh_frame` of every placed function that
    /// needs them (`spec/eh.md` §1.1), or `.debug_frame` (§10).
    ///
    /// In a module with debug information every function has CFI, as in
    /// `llc` (`AsmPrinter::getFunctionCFISectionType`: `CFISection::Debug`
    /// for one that needs no unwind entry). `llc` chooses the section once
    /// for the module (`ModuleCFISection`, from the functions it emits):
    /// `.eh_frame` for all of them when any one needs an unwind entry,
    /// else `.debug_frame` (WP-150, checked with `llc` 19 and 22 on both
    /// targets).
    pub(super) fn unwind_tables(&mut self) -> CgResult<()> {
        let debug = self.debug.is_some();
        let any_eh = self.funcs.iter().any(|f| f.record.needs_unwind_entry());
        if debug && !any_eh {
            return self.debug_frame();
        }
        self.eh_tables(debug)
    }

    /// `.debug_frame` (`spec/eh.md` §10, WP-150): an FDE of each function's
    /// CFI rows, the rows its `.eh_frame` entry would have.
    fn debug_frame(&mut self) -> CgResult<()> {
        if self.funcs.is_empty() {
            return Ok(());
        }
        let mut frames = Vec::with_capacity(self.funcs.len());
        for placed in &self.funcs {
            let record = &placed.record;
            frames.push(Fde {
                order: record.sym.index() as u32,
                start: self.obj.section_symbol(placed.section),
                start_addend: i64::from(record.code.start),
                size: record.code.end - record.code.start,
                rows: &self.rows.cfi[record.cfi.start as usize..record.cfi.end as usize],
                personality: None,
                lsda: None,
            });
        }
        hod_eh::write_debug_frame(&mut self.obj, &frames).map_err(eh_err)?;
        Ok(())
    }

    /// The LSDAs and `.eh_frame`; `all`: every function has an FDE (a
    /// module with debug information, [`Self::unwind_tables`]).
    fn eh_tables(&mut self, all: bool) -> CgResult<()> {
        let module = self.module;
        let (entries, with_pads) = self.count_entries(all)?;
        if entries == 0 {
            return Ok(());
        }

        // The LSDA rows, mapped once for the object.
        let rows = &self.rows;
        let mut ranges = Vec::with_capacity(with_pads);
        let (mut sites, mut pads, mut types) = (Vec::new(), Vec::new(), Vec::new());
        if with_pads > 0 {
            let (mut n_sites, mut n_pads, mut n_types) = (0, 0, 0);
            for func in self.funcs.iter().filter(|f| f.record.has_pads) {
                n_sites += func.record.call_sites.len();
                n_pads += func.record.pads.len();
                n_types += func.record.type_ids.len();
            }
            sites.reserve_exact(n_sites);
            pads.reserve_exact(n_pads);
            types.reserve_exact(n_types);
        }
        for (index, func) in self.funcs.iter().enumerate() {
            let record = &func.record;
            if !record.has_pads || !record.needs_unwind_entry() {
                continue;
            }
            if record.personality.is_none() {
                return Err(CgError::internal("a function with landing pads and no personality"));
            }
            let start = (sites.len(), pads.len(), types.len());
            let range = |r: &Range<u32>| r.start as usize..r.end as usize;
            sites.extend(
                rows.call_sites[range(&record.call_sites)]
                    .iter()
                    .filter(|row| row.pad != NO_PAD || !row.nounwind)
                    .map(|row| CallSite {
                        start: row.start,
                        len: row.len,
                        pad: row.pad,
                        landing: row.landing,
                    }),
            );
            pads.extend(rows.pads[range(&record.pads)].iter().map(|pad| Pad {
                block: pad.block.raw(),
                offset: pad.offset,
                cleanup: pad.cleanup,
                clauses: pad.clauses.clone(),
            }));
            types.extend(rows.type_ids[range(&record.type_ids)].iter().map(|t| t.map(obj_sym)));
            ranges.push(Ranges {
                func: index as u32,
                call_sites: start.0..sites.len(),
                pads: start.1..pads.len(),
                type_ids: start.2..types.len(),
            });
        }
        for id in types.iter().flatten() {
            self.name_symbol(SymId::new(id.index()))?;
        }

        // The personality slots, one per personality, in first use.
        let mut slots: Vec<(SymId, SymbolId)> = Vec::new();
        for range in &ranges {
            let Some(personality) = self.funcs[range.func as usize].record.personality else {
                continue;
            };
            if slots.iter().all(|&(p, _)| p != personality) {
                self.name_symbol(personality)?;
                let slot =
                    hod_eh::personality_ref(&mut self.obj, obj_sym(personality)).map_err(eh_err)?;
                slots.push((personality, slot));
            }
        }

        let lsdas: Vec<Lsda> = if ranges.is_empty() {
            Vec::new()
        } else {
            let rows = &self.rows;
            let mut funcs = Vec::with_capacity(ranges.len());
            for range in &ranges {
                let placed = &self.funcs[range.func as usize];
                let record = &placed.record;
                let r = |r: &Range<u32>| r.start as usize..r.end as usize;
                funcs.push(FuncLsda {
                    order: record.sym.index() as u32,
                    name: module.symbol(record.sym).name(),
                    section: placed.section,
                    size: record.code.end - record.code.start,
                    call_sites: &sites[range.call_sites.clone()],
                    pads: &pads[range.pads.clone()],
                    clauses: &rows.clauses[r(&record.clauses)],
                    type_ids: &types[range.type_ids.clone()],
                    filter_ids: &rows.filter_ids[r(&record.filter_ids)],
                });
            }
            let options = LsdaOptions {
                function_sections: self.options.function_sections,
                stubs: self.sections.generic(Kind::Data),
            };
            hod_eh::write_lsdas(&mut self.obj, &funcs, options).map_err(eh_err)?
        };

        self.write_fdes(all, entries, &ranges, &lsdas, &slots)
    }

    /// The functions that need an FDE (all of them when `all`), and those
    /// of them with landing pads; an unknown personality without pads is
    /// refused.
    fn count_entries(&self, all: bool) -> CgResult<(usize, usize)> {
        let module = self.module;
        let (mut entries, mut with_pads) = (0usize, 0usize);
        for func in &self.funcs {
            let record = &func.record;
            if !all && !record.needs_unwind_entry() {
                continue;
            }
            entries += 1;
            if record.has_pads {
                with_pads += 1;
            } else if let Some(personality) = record.personality
                && !KNOWN_PERSONALITIES.contains(&module.symbol(personality).name())
            {
                // `llc` writes such a function's personality and an LSDA
                // whose call sites hod does not record.
                return Err(CgError::unsupported(unsupported::PERSONALITY_WITHOUT_PADS));
            }
        }
        Ok((entries, with_pads))
    }

    /// `.eh_frame`: one FDE per function that needs one (each when
    /// `all`), its LSDA by the function's position in `ranges`.
    fn write_fdes(
        &mut self,
        all: bool,
        entries: usize,
        ranges: &[Ranges],
        lsdas: &[Lsda],
        slots: &[(SymId, SymbolId)],
    ) -> CgResult<()> {
        let mut frames = Vec::with_capacity(entries);
        let mut next_lsda = 0usize;
        for (index, placed) in self.funcs.iter().enumerate() {
            let record = &placed.record;
            if !all && !record.needs_unwind_entry() {
                continue;
            }
            let (mut personality, mut lsda) = (None, None);
            if record.has_pads {
                if ranges.get(next_lsda).map(|r| r.func) != Some(index as u32) {
                    return Err(CgError::internal("an LSDA out of step with its function"));
                }
                lsda = lsdas.get(next_lsda).copied();
                next_lsda += 1;
                let slot = slots.iter().find(|&&(p, _)| Some(p) == record.personality);
                personality = slot.map(|&(_, slot)| slot);
            }
            frames.push(Fde {
                order: record.sym.index() as u32,
                start: self.obj.section_symbol(placed.section),
                start_addend: i64::from(record.code.start),
                size: record.code.end - record.code.start,
                rows: &self.rows.cfi[record.cfi.start as usize..record.cfi.end as usize],
                personality,
                lsda,
            });
        }
        hod_eh::write_eh_frame(&mut self.obj, &frames).map_err(eh_err)?;
        Ok(())
    }
}
