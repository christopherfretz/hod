//! The language-specific data areas: `.gcc_except_table` (`spec/eh.md` §9).
//!
//! One LSDA per function with landing pads, laid out as `llc` lays it out
//! for Linux position-independent code (LLVM `EHStreamer`): the header,
//! the call-site table, the action table with shared tails, the type table
//! (indirect, PC-relative references through one `DW.stub` word per
//! type-info object) and the exception specifications.

use std::ops::Range;

use hod_obj::elf::{SHF_ALLOC, SHT_PROGBITS};
use hod_obj::{Object, SectionId, SectionKind, SymbolId};

use crate::leb::{sleb, uleb};
use crate::{Error, Lsda, for_arch, uleb_len};

/// "No landing pad" in [`CallSite::pad`]: a plain call or a runtime call.
pub const NO_PAD: u32 = u32::MAX;

/// One call instruction, in code order: `hod_cg`'s `CallSiteRow`
/// (`spec/codegen.md` §9.4) field for field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallSite {
    /// The offset of the start of what was emitted for the call.
    pub start: u32,
    /// Its length in bytes.
    pub len: u32,
    /// The index of its landing pad in [`FuncLsda::pads`]; [`NO_PAD`]
    /// for none.
    pub pad: u32,
    /// The landing address: an offset in the function, or 0 for the pad's
    /// own [`Pad::offset`]. Ignored without a pad.
    pub landing: u32,
}

/// One landing pad: `hod_cg`'s `PadRow` field for field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pad {
    /// The index of the landing-pad block (`hod_ir::BlockId`); the writer
    /// does not use it.
    pub block: u32,
    /// The code offset of the block.
    pub offset: u32,
    /// Whether the pad runs cleanups.
    pub cleanup: bool,
    /// Its clauses: a range of [`FuncLsda::clauses`].
    pub clauses: Range<u32>,
}

/// One clause of a landing pad: `hod_cg`'s `ClauseRow`. Type ids are
/// indices + 1 into [`FuncLsda::type_ids`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Clause {
    /// `catch` of this type id.
    Catch(u32),
    /// `filter` of these type ids: a range of [`FuncLsda::filter_ids`].
    Filter(Range<u32>),
}

/// One function's exception tables, the input of [`write_lsdas`]
/// (`spec/eh.md` §9.1). The slices are the function's ranges of the
/// core's chunk tables, so every index in them is relative to the
/// function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuncLsda<'a> {
    /// The function's position in the module; tables in a shared section
    /// are written in this order, whatever order they are passed in.
    pub order: u32,
    /// The function's symbol name, which names its own table section.
    pub name: &'a [u8],
    /// The section the function's code is in.
    pub section: SectionId,
    /// The size of the function's code.
    pub size: u32,
    /// The call sites, in code order.
    pub call_sites: &'a [CallSite],
    /// The landing pads.
    pub pads: &'a [Pad],
    /// The pads' clauses.
    pub clauses: &'a [Clause],
    /// The type-info table, in type-id order; `None` is catch-all.
    pub type_ids: &'a [Option<SymbolId>],
    /// The type ids of the `filter` clauses.
    pub filter_ids: &'a [u32],
}

/// Where [`write_lsdas`] puts things (`spec/eh.md` §9.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LsdaOptions {
    /// Every function's table in a section of its own, as `llc
    /// -function-sections` does; otherwise only a function in a COMDAT
    /// group has one.
    pub function_sections: bool,
    /// The writable section the type-info stubs are appended to (the
    /// object's `.data`); `None` makes a new `.data` when there are stubs.
    pub stubs: Option<SectionId>,
}

/// What `llc` writes for the LSDA header (`spec/eh.md` §9.2).
const DW_EH_PE_OMIT: u8 = 0xff;
const DW_EH_PE_ULEB128: u8 = 0x01;

/// One action record: its type filter and the record its chain goes on
/// to.
struct Action {
    value: i64,
    next: Option<usize>,
    pos: usize,
}

/// One function's LSDA, encoded but not yet placed.
struct Encoded {
    bytes: Vec<u8>,
    /// Type-table fields: their offset in `bytes` and the type-info symbol.
    relocs: Vec<(u32, SymbolId)>,
}

/// Appends `value` as ULEB128 of exactly `len` bytes (padded with
/// continuation bytes, as the assembler pads a fragment that may not
/// shrink); `len` is at least [`uleb_len`] of it.
fn uleb_padded(out: &mut Vec<u8>, mut value: u64, len: usize) {
    for i in 0..len {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if i + 1 < len {
            byte |= 0x80;
        }
        out.push(byte);
    }
}

fn sleb_len(value: i64) -> usize {
    let mut v = Vec::new();
    sleb(&mut v, value);
    v.len()
}

/// The filter table as LLVM builds it (`MachineFunction::getFilterIDFor`):
/// type ids with a 0 after each filter, a new filter sharing the tail of an
/// existing one when it can.
#[derive(Default)]
struct Filters {
    ids: Vec<u32>,
    ends: Vec<usize>,
}

impl Filters {
    /// The filter id (negative) of `list`.
    fn id_for(&mut self, list: &[u32]) -> i64 {
        for &end in &self.ends {
            let (mut i, mut j) = (end, list.len());
            let mut matches = true;
            while i > 0 && j > 0 {
                i -= 1;
                j -= 1;
                if self.ids[i] != list[j] {
                    matches = false;
                    break;
                }
            }
            if matches && j == 0 {
                return -(1 + i as i64);
            }
        }
        let id = -(1 + self.ids.len() as i64);
        self.ids.extend_from_slice(list);
        self.ends.push(self.ids.len());
        self.ids.push(0);
        id
    }
}

/// Encodes one function's LSDA (`spec/eh.md` §9.2 to §9.5), or the first
/// problem with its rows.
fn encode(f: &FuncLsda<'_>, pointer_size: usize, ttype_encoding: u8) -> Result<Encoded, Error> {
    let order = f.order;
    let types = f.type_ids.len() as u64;
    let type_ok = |t: u32| t >= 1 && u64::from(t) <= types;

    // Per pad, LLVM's type-id list: 0 for a cleanup that has clauses, then
    // the clauses last to first; filters by their (negative) filter id.
    let mut filters = Filters::default();
    let mut type_lists: Vec<Vec<i64>> = Vec::with_capacity(f.pads.len());
    let mut list_of_filter = Vec::new();
    for (pad, p) in f.pads.iter().enumerate() {
        let (start, end) = (p.clauses.start as usize, p.clauses.end as usize);
        if start > end || end > f.clauses.len() {
            return Err(Error::BadClauses { order, pad });
        }
        if start == end && !p.cleanup {
            return Err(Error::EmptyPad { order, pad });
        }
        let mut list = Vec::with_capacity(end - start + 1);
        if p.cleanup && start != end {
            list.push(0);
        }
        for clause in (start..end).rev() {
            match &f.clauses[clause] {
                &Clause::Catch(t) => {
                    if !type_ok(t) {
                        return Err(Error::BadTypeId { order, clause });
                    }
                    list.push(i64::from(t));
                }
                Clause::Filter(r) => {
                    let (s, e) = (r.start as usize, r.end as usize);
                    if s > e
                        || e > f.filter_ids.len()
                        || !f.filter_ids[s..e].iter().all(|&t| type_ok(t))
                    {
                        return Err(Error::BadFilter { order, clause });
                    }
                    list_of_filter.clear();
                    list_of_filter.extend_from_slice(&f.filter_ids[s..e]);
                    list.push(filters.id_for(&list_of_filter));
                }
            }
        }
        type_lists.push(list);
    }
    // The byte offset (negative, biased by one) of each filter-table entry.
    let mut filter_offsets = Vec::with_capacity(filters.ids.len());
    let mut at = -1i64;
    for &id in &filters.ids {
        filter_offsets.push(at);
        at -= uleb_len(u64::from(id)) as i64;
    }
    let value_of = |t: i64| if t < 0 { filter_offsets[(-1 - t) as usize] } else { t };

    // The action table (`EHStreamer::computeActionsTable`): pads sorted by
    // their lists, each sharing the longest common prefix of its list with
    // the previous pad's (its chain's tail).
    let mut sorted: Vec<usize> = (0..f.pads.len()).collect();
    sorted.sort_by(|&a, &b| type_lists[a].cmp(&type_lists[b]));
    let mut actions: Vec<Action> = Vec::new();
    let mut action_bytes = 0usize;
    let mut first_action = vec![0u64; f.pads.len()];
    // The record of each entry of the previous pad's list.
    let mut prev_chain: Vec<usize> = Vec::new();
    let mut prev: Option<usize> = None;
    let mut first = 0u64;
    for &pad in &sorted {
        let list = &type_lists[pad];
        let shared =
            prev.map_or(0, |q| type_lists[q].iter().zip(list).take_while(|(a, b)| a == b).count());
        if shared < list.len() {
            prev_chain.truncate(shared);
            for &t in &list[shared..] {
                let value = value_of(t);
                let pos = action_bytes;
                let next = prev_chain.last().copied();
                let disp =
                    next.map_or(0, |n| actions[n].pos as i64 - (pos + sleb_len(value)) as i64);
                action_bytes += sleb_len(value) + sleb_len(disp);
                prev_chain.push(actions.len());
                actions.push(Action { value, next, pos });
            }
            first = actions[*prev_chain.last().unwrap_or(&0)].pos as u64 + 1;
        }
        first_action[pad] = first;
        prev = Some(pad);
    }

    // The call-site table (`EHStreamer::computeCallSiteTable`): adjacent
    // rows with the same landing and action merge; a range between two
    // padded rows (or the function's ends) gets an entry without a pad
    // exactly when a call without a pad is in it.
    let mut sites: Vec<(u32, u32, u32, u64)> = Vec::with_capacity(f.call_sites.len() + 1);
    let mut last_end = 0u32;
    let mut prev_end = 0u32;
    let mut saw_throwing = false;
    let mut prev_is_invoke = false;
    for (row, c) in f.call_sites.iter().enumerate() {
        if c.start < prev_end {
            return Err(Error::CallSiteOutOfOrder { order, row });
        }
        let end = c.start.checked_add(c.len).filter(|&e| e <= f.size);
        let Some(end) = end else { return Err(Error::CallSiteBeyondEnd { order, row }) };
        prev_end = end;
        if c.pad == NO_PAD {
            saw_throwing = true;
            continue;
        }
        let Some(pad) = f.pads.get(c.pad as usize) else {
            return Err(Error::BadPad { order, row });
        };
        let landing = if c.landing == 0 { pad.offset } else { c.landing };
        if landing >= f.size {
            return Err(Error::LandingBeyondEnd { order, row });
        }
        if saw_throwing {
            sites.push((last_end, c.start, 0, 0));
            prev_is_invoke = false;
            saw_throwing = false;
        }
        let action = first_action[c.pad as usize];
        if prev_is_invoke {
            if let Some(last) = sites.last_mut() {
                if last.2 == landing && last.3 == action {
                    last.1 = end;
                    last_end = end;
                    continue;
                }
            }
        }
        sites.push((c.start, end, landing, action));
        prev_is_invoke = true;
        last_end = end;
    }
    if saw_throwing {
        sites.push((last_end, f.size, 0, 0));
    }

    // Sizes, then bytes.
    let cst_len: usize = sites
        .iter()
        .map(|&(s, e, l, a)| {
            uleb_len(u64::from(s))
                + uleb_len(u64::from(e - s))
                + uleb_len(u64::from(l))
                + uleb_len(a)
        })
        .sum();
    let have_types = !f.type_ids.is_empty() || !filters.ids.is_empty();
    let filter_bytes: usize = filters.ids.iter().map(|&t| uleb_len(u64::from(t))).sum();
    let type_bytes = f.type_ids.len() * pointer_size;
    // After the TType base offset: the call-site encoding, the table's
    // length, the table, the actions.
    let after = 1 + uleb_len(cst_len as u64) + cst_len + action_bytes;
    // The TType base offset's own size, as the assembler settles it
    // (§9.2): from nothing, growing, never shrinking.
    let mut ttbase = (0u64, 0usize);
    if have_types {
        let mut size = 0usize;
        loop {
            let before_types = 2 + size + after;
            let pad = before_types.next_multiple_of(4) - before_types;
            let value = (after + pad + type_bytes) as u64;
            let need = uleb_len(value);
            if need <= size {
                ttbase = (value, size);
                break;
            }
            size = need;
        }
    }
    let total = 2 + ttbase.1 + after + 3 + type_bytes + filter_bytes + 3;
    let mut out = Vec::with_capacity(total);
    out.push(DW_EH_PE_OMIT); // @LPStart
    if have_types {
        out.push(ttype_encoding);
        uleb_padded(&mut out, ttbase.0, ttbase.1);
    } else {
        out.push(DW_EH_PE_OMIT);
    }
    out.push(DW_EH_PE_ULEB128);
    uleb(&mut out, cst_len as u64);
    for &(s, e, l, a) in &sites {
        uleb(&mut out, u64::from(s));
        uleb(&mut out, u64::from(e - s));
        uleb(&mut out, u64::from(l));
        uleb(&mut out, a);
    }
    for a in &actions {
        sleb(&mut out, a.value);
        let next_field = (a.pos + sleb_len(a.value)) as i64;
        sleb(&mut out, a.next.map_or(0, |n| actions[n].pos as i64 - next_field));
    }
    let mut relocs = Vec::new();
    if have_types {
        out.resize(out.len().next_multiple_of(4), 0);
        for t in f.type_ids.iter().rev() {
            if let Some(symbol) = *t {
                relocs.push((out.len() as u32, symbol));
            }
            out.resize(out.len() + pointer_size, 0);
        }
        for &t in &filters.ids {
            uleb(&mut out, u64::from(t));
        }
    }
    out.resize(out.len().next_multiple_of(4), 0);
    Ok(Encoded { bytes: out, relocs })
}

/// Writes the LSDA of each function in `funcs` into `obj`
/// (`spec/eh.md` §9): in `.gcc_except_table`, or in a section
/// `.gcc_except_table.<name>` of the function's own when it is in a COMDAT
/// group (whose member the table becomes) or `options.function_sections`
/// is set; with one 8-byte stub per type-info object in `options.stubs`.
///
/// Returns each function's LSDA, in the order of `funcs`, as
/// [`Fde::lsda`](crate::Fde::lsda) takes it. The output depends only on the
/// set of functions, not on the order of the slice. On an error `obj` is
/// unchanged.
pub fn write_lsdas(
    obj: &mut Object,
    funcs: &[FuncLsda<'_>],
    options: LsdaOptions,
) -> Result<Vec<Lsda>, Error> {
    if funcs.is_empty() {
        return Ok(Vec::new());
    }
    let target = for_arch(obj.arch());
    let pointer_size = usize::from(target.pointer_size);
    let mut by_order: Vec<usize> = (0..funcs.len()).collect();
    if !by_order.is_sorted_by_key(|&i| funcs[i].order) {
        by_order.sort_unstable_by_key(|&i| funcs[i].order);
    }
    if let Some(w) = by_order.windows(2).find(|w| funcs[w[0]].order == funcs[w[1]].order) {
        return Err(Error::DuplicateOrder { order: funcs[w[0]].order });
    }
    let own_section = |f: &FuncLsda<'_>, obj: &Object| {
        options.function_sections || obj.section_group(f.section).is_some()
    };
    // Everything that can fail, before `obj` is touched.
    let mut encoded = Vec::with_capacity(funcs.len());
    for &i in &by_order {
        let f = &funcs[i];
        if own_section(f, obj) && f.name.contains(&0) {
            return Err(Error::BadName { order: f.order });
        }
        encoded.push(encode(f, pointer_size, target.personality_encoding)?);
    }

    // The stubs: one per type-info object, ordered by name as `llc`
    // orders them.
    let mut types: Vec<SymbolId> =
        encoded.iter().flat_map(|e| e.relocs.iter().map(|r| r.1)).collect();
    types.sort_unstable_by_key(|s| s.index());
    types.dedup();
    types.sort_by(|&a, &b| {
        obj.string(obj.symbol(a).name)
            .cmp(obj.string(obj.symbol(b).name))
            .then(a.index().cmp(&b.index()))
    });
    let mut stub_of = Vec::new(); // (type symbol, stub offset), by symbol
    let mut stub_symbol = None;
    if !types.is_empty() {
        let section = match options.stubs {
            Some(s) => s,
            None => obj.add_section(b".data", SectionKind::Data, 8)?,
        };
        obj.raise_section_align(section, 8);
        obj.align_to(section, 8, 0);
        for &t in &types {
            let at = obj.append_zeros(section, 8);
            obj.add_relocation(section, at, target.abs_reloc, t, 0);
            stub_of.push((t, at as i64));
        }
        stub_of.sort_unstable_by_key(|&(t, _)| t.index());
        stub_symbol = Some(obj.section_symbol(section));
    }
    let stub = |t: SymbolId| {
        let k = stub_of.binary_search_by_key(&t.index(), |&(s, _)| s.index()).unwrap_or(0);
        stub_of[k].1
    };

    let kind = SectionKind::Elf { sh_type: SHT_PROGBITS, sh_flags: SHF_ALLOC, sh_entsize: 0 };
    let mut shared: Option<SectionId> = None;
    let mut result = vec![Lsda { symbol: SymbolId::from_index(0), addend: 0 }; funcs.len()];
    for (k, &i) in by_order.iter().enumerate() {
        let f = &funcs[i];
        let e = &encoded[k];
        let section = if own_section(f, obj) {
            let mut name = Vec::with_capacity(18 + f.name.len());
            name.extend_from_slice(b".gcc_except_table.");
            name.extend_from_slice(f.name);
            let section = obj.add_section(&name, kind, 4)?;
            if let Some(group) = obj.section_group(f.section) {
                obj.add_group_member(group, section)?;
            }
            section
        } else if let Some(s) = shared {
            s
        } else {
            let s = obj.add_section(b".gcc_except_table", kind, 4)?;
            shared = Some(s);
            s
        };
        let base = obj.align_to(section, 4, 0);
        obj.append(section, &e.bytes);
        if let Some(stubs) = stub_symbol {
            for &(at, t) in &e.relocs {
                obj.add_relocation(
                    section,
                    base + u64::from(at),
                    target.pointer_reloc,
                    stubs,
                    stub(t),
                );
            }
        }
        result[i] = Lsda { symbol: obj.section_symbol(section), addend: base as i64 };
    }
    Ok(result)
}
