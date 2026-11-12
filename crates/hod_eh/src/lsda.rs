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

use crate::leb::{Sink, sleb, uleb};
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

/// One action record: its type filter, the record its chain goes on to,
/// and its offset in the action table.
#[derive(Clone, Copy)]
struct Action {
    value: i64,
    next: Option<u32>,
    pos: u32,
}

/// One call-site entry: start, end, landing address, first action.
type Entry = (u32, u32, u32, u64);

/// The buffers one function's encoding needs, reused for the next.
#[derive(Default)]
struct Work {
    /// Per pad, LLVM's type-id list, as a range of `lists`.
    lists: Vec<i64>,
    list_of: Vec<Range<u32>>,
    sorted: Vec<u32>,
    filters: Filters,
    filter_offsets: Vec<i64>,
    actions: Vec<Action>,
    chain: Vec<u32>,
    first_action: Vec<u64>,
    sites: Vec<Entry>,
}

/// Bytes written at increasing offsets into a buffer sized beforehand.
struct Cursor<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl Sink for Cursor<'_> {
    #[inline]
    fn byte(&mut self, b: u8) {
        self.buf[self.at] = b;
        self.at += 1;
    }
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        self.buf[self.at..self.at + b.len()].copy_from_slice(b);
        self.at += b.len();
    }
}

/// The number of bytes SLEB128 takes for `value`.
const fn sleb_len(value: i64) -> usize {
    // The bits to store, the sign bit included.
    let bits = 65 - if value < 0 { (!value).leading_zeros() } else { value.leading_zeros() };
    (bits as usize).div_ceil(7)
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
    fn clear(&mut self) {
        self.ids.clear();
        self.ends.clear();
    }

    /// The filter id (negative) of `list`.
    fn id_for(&mut self, list: &[u32]) -> i64 {
        for &end in &self.ends {
            if end >= list.len() && self.ids[end - list.len()..end] == *list {
                return -(1 + (end - list.len()) as i64);
            }
        }
        let id = -(1 + self.ids.len() as i64);
        self.ids.extend_from_slice(list);
        self.ends.push(self.ids.len());
        self.ids.push(0);
        id
    }
}

impl Work {
    /// Per pad, LLVM's type-id list (`MachineFunction::addLandingPad`): 0
    /// for a cleanup that has clauses, then the clauses last to first;
    /// filters by their (negative) filter id. Then the filter table's byte
    /// offsets.
    fn type_lists(&mut self, f: &FuncLsda<'_>) -> Result<(), Error> {
        let order = f.order;
        let types = f.type_ids.len() as u64;
        let type_ok = |t: u32| t >= 1 && u64::from(t) <= types;
        // the clauses last to first; filters by their (negative) filter id.
        self.lists.clear();
        self.list_of.clear();
        self.filters.clear();
        for (pad, p) in f.pads.iter().enumerate() {
            let (start, end) = (p.clauses.start as usize, p.clauses.end as usize);
            if start > end || end > f.clauses.len() {
                return Err(Error::BadClauses { order, pad });
            }
            if start == end && !p.cleanup {
                return Err(Error::EmptyPad { order, pad });
            }
            let from = self.lists.len() as u32;
            if p.cleanup && start != end {
                self.lists.push(0);
            }
            for clause in (start..end).rev() {
                match &f.clauses[clause] {
                    &Clause::Catch(t) => {
                        if !type_ok(t) {
                            return Err(Error::BadTypeId { order, clause });
                        }
                        self.lists.push(i64::from(t));
                    }
                    Clause::Filter(r) => {
                        let (s, e) = (r.start as usize, r.end as usize);
                        let ids =
                            f.filter_ids.get(s..e).filter(|ids| ids.iter().all(|&t| type_ok(t)));
                        let Some(ids) = ids else {
                            return Err(Error::BadFilter { order, clause });
                        };
                        let id = self.filters.id_for(ids);
                        self.lists.push(id);
                    }
                }
            }
            self.list_of.push(from..self.lists.len() as u32);
        }
        // The byte offset (negative, biased by one) of each filter-table entry.
        self.filter_offsets.clear();
        let mut at = -1i64;
        for &id in &self.filters.ids {
            self.filter_offsets.push(at);
            at -= uleb_len(u64::from(id)) as i64;
        }
        Ok(())
    }

    /// The action table; returns its size in bytes.
    fn action_table(&mut self, pads: usize) -> u32 {
        // The action table (`EHStreamer::computeActionsTable`): pads sorted by
        // their lists, each sharing the longest common prefix of its list with
        // the previous pad's (its chain's tail).
        let Work { lists, list_of, sorted, filter_offsets, actions, chain, first_action, .. } =
            self;
        let list = |pad: u32| {
            &lists[list_of[pad as usize].start as usize..list_of[pad as usize].end as usize]
        };
        sorted.clear();
        sorted.extend(0..pads as u32);
        if !sorted.is_sorted_by(|&a, &b| list(a) <= list(b)) {
            sorted.sort_by(|&a, &b| list(a).cmp(list(b)));
        }
        actions.clear();
        chain.clear();
        first_action.clear();
        first_action.resize(pads, 0);
        let mut action_bytes = 0u32;
        let mut prev: Option<u32> = None;
        let mut first = 0u64;
        for &pad in sorted.iter() {
            let this = list(pad);
            let shared =
                prev.map_or(0, |q| list(q).iter().zip(this).take_while(|(a, b)| a == b).count());
            if shared < this.len() {
                chain.truncate(shared);
                for &t in &this[shared..] {
                    let value = if t < 0 { filter_offsets[(-1 - t) as usize] } else { t };
                    let pos = action_bytes;
                    let next = chain.last().copied();
                    let field = pos + sleb_len(value) as u32;
                    let disp =
                        next.map_or(0, |n| i64::from(actions[n as usize].pos) - i64::from(field));
                    action_bytes = field + sleb_len(disp) as u32;
                    chain.push(actions.len() as u32);
                    actions.push(Action { value, next, pos });
                }
                first = u64::from(actions[*chain.last().unwrap_or(&0) as usize].pos) + 1;
            }
            first_action[pad as usize] = first;
            prev = Some(pad);
        }
        action_bytes
    }

    /// The call-site table.
    fn call_site_table(&mut self, f: &FuncLsda<'_>) -> Result<(), Error> {
        let order = f.order;
        // The call-site table (`EHStreamer::computeCallSiteTable`): adjacent
        // rows with the same landing and action merge; a range between two
        // padded rows (or the function's ends) gets an entry without a pad
        // exactly when a call without a pad is in it.
        let sites = &mut self.sites;
        sites.clear();
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
            let action = self.first_action[c.pad as usize];
            last_end = end;
            if prev_is_invoke
                && let Some(last) = sites.last_mut()
                && last.2 == landing
                && last.3 == action
            {
                last.1 = end;
                continue;
            }
            sites.push((c.start, end, landing, action));
            prev_is_invoke = true;
        }
        if saw_throwing {
            sites.push((last_end, f.size, 0, 0));
        }
        Ok(())
    }
}

/// Encodes one function's LSDA (`spec/eh.md` §9.2 to §9.5) at the end of
/// `out`, with its type-table fields (offset in `out`, type-info) in
/// `relocs`; or the first problem with its rows, `out` and `relocs`
/// unchanged.
fn encode(
    f: &FuncLsda<'_>,
    w: &mut Work,
    pointer_size: usize,
    ttype_encoding: u8,
    out: &mut Vec<u8>,
    relocs: &mut Vec<(u32, SymbolId)>,
) -> Result<(), Error> {
    w.type_lists(f)?;
    let action_bytes = w.action_table(f.pads.len());
    w.call_site_table(f)?;
    let Work { actions, sites, filters, .. } = w;
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
    let filter_ids = &filters.ids;
    let have_types = !f.type_ids.is_empty() || !filter_ids.is_empty();
    let type_bytes = f.type_ids.len() * pointer_size;
    // After the TType base offset: the call-site encoding, the table's
    // length, the table, the actions.
    let after = 1 + uleb_len(cst_len as u64) + cst_len + action_bytes as usize;
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
    let filter_bytes: usize = filter_ids.iter().map(|&t| uleb_len(u64::from(t))).sum();
    let head = 2 + ttbase.1 + after;
    let table = if have_types { head.next_multiple_of(4) } else { head };
    let total = (table + type_bytes + filter_bytes).next_multiple_of(4);
    // Zeros: the padding and the type-table fields are already written.
    let base = out.len();
    out.resize(base + total, 0);
    let mut c = Cursor { buf: &mut out[base..], at: 0 };
    c.byte(DW_EH_PE_OMIT); // @LPStart
    if have_types {
        c.byte(ttype_encoding);
        let mut v = ttbase.0;
        for i in 0..ttbase.1 {
            let more = if i + 1 < ttbase.1 { 0x80 } else { 0 };
            c.byte((v & 0x7f) as u8 | more);
            v >>= 7;
        }
    } else {
        c.byte(DW_EH_PE_OMIT);
    }
    c.byte(DW_EH_PE_ULEB128);
    uleb(&mut c, cst_len as u64);
    for &(s, e, l, a) in sites.iter() {
        uleb(&mut c, u64::from(s));
        uleb(&mut c, u64::from(e - s));
        uleb(&mut c, u64::from(l));
        uleb(&mut c, a);
    }
    for a in actions.iter() {
        sleb(&mut c, a.value);
        let field = i64::from(a.pos) + sleb_len(a.value) as i64;
        sleb(&mut c, a.next.map_or(0, |n| i64::from(actions[n as usize].pos) - field));
    }
    if have_types {
        let mut at = base + table;
        for t in f.type_ids.iter().rev() {
            if let Some(symbol) = *t {
                relocs.push((at as u32, symbol));
            }
            at += pointer_size;
        }
        c.at = table + type_bytes;
        for &t in filter_ids {
            uleb(&mut c, u64::from(t));
        }
    }
    Ok(())
}

/// Writes the LSDA of each function in `funcs` into `obj`
/// (`spec/eh.md` §9): in `.gcc_except_table`, or in a section
/// `.gcc_except_table.<name>` of the function's own when it is in a COMDAT
/// group (whose member the table becomes) or `options.function_sections`
/// is set; with one 8-byte stub per type-info object in `options.stubs`.
///
/// Returns each function's LSDA, in the order of `funcs`, as
/// [`Fde::lsda`](crate::Fde::lsda) takes it. The output depends only on the
/// set of functions, not on the order of the slice. On an error in the
/// rows `obj` is unchanged.
#[allow(clippy::too_many_lines)] // Encode all, then the stubs, then place.
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
    let mut by_order: Vec<u32> = (0..funcs.len() as u32).collect();
    if !by_order.is_sorted_by_key(|&i| funcs[i as usize].order) {
        by_order.sort_unstable_by_key(|&i| funcs[i as usize].order);
    }
    if let Some(w) =
        by_order.windows(2).find(|w| funcs[w[0] as usize].order == funcs[w[1] as usize].order)
    {
        return Err(Error::DuplicateOrder { order: funcs[w[0] as usize].order });
    }
    let own_section = |f: &FuncLsda<'_>, obj: &Object| {
        options.function_sections || obj.section_group(f.section).is_some()
    };
    // Everything that can fail, before `obj` is touched: every LSDA,
    // encoded one after another in module order, each at a multiple of 4.
    let rows: usize = funcs.iter().map(|f| f.call_sites.len()).sum();
    let mut all = Vec::with_capacity(funcs.len() * 24 + rows * 4);
    let mut spans = Vec::with_capacity(funcs.len());
    let mut relocs: Vec<(u32, SymbolId)> = Vec::new();
    let mut work = Work::default();
    let mut any_own = false;
    for &i in &by_order {
        let f = &funcs[i as usize];
        let own = own_section(f, obj);
        if own && f.name.contains(&0) {
            return Err(Error::BadName { order: f.order });
        }
        any_own |= own;
        let start = all.len();
        let first_reloc = relocs.len();
        encode(f, &mut work, pointer_size, target.personality_encoding, &mut all, &mut relocs)?;
        spans.push((start..all.len(), first_reloc..relocs.len()));
    }
    if u32::try_from(all.len()).is_err() {
        return Err(Error::TooLarge);
    }

    // The stubs: one per type-info object, ordered by name as `llc`
    // orders them.
    let mut types: Vec<SymbolId> = relocs.iter().map(|r| r.1).collect();
    types.sort_unstable();
    types.dedup();
    let stubs = if types.is_empty() {
        None
    } else {
        let mut named = types.clone();
        let name = |s: SymbolId| obj.string(obj.symbol(s).name);
        named.sort_by(|&a, &b| name(a).cmp(name(b)).then(a.cmp(&b)));
        let section = match options.stubs {
            Some(s) => s,
            None => obj.add_section(b".data", SectionKind::Data, 8)?,
        };
        obj.raise_section_align(section, 8);
        obj.align_to(section, 8, 0);
        let mut offsets = vec![0i64; types.len()];
        obj.reserve(0, 0, named.len(), 0);
        for &t in &named {
            let at = obj.append_zeros(section, 8);
            obj.add_relocation(section, at, target.abs_reloc, t, 0);
            offsets[types.binary_search(&t).unwrap_or(0)] = at as i64;
        }
        Some((obj.section_symbol(section), offsets))
    };

    let kind = SectionKind::Elf { sh_type: SHT_PROGBITS, sh_flags: SHF_ALLOC, sh_entsize: 0 };
    let mut result = vec![Lsda { symbol: SymbolId::from_index(0), addend: 0 }; funcs.len()];
    obj.reserve(0, 0, relocs.len(), 0);
    let mut place = |obj: &mut Object, section: SectionId, k: usize, base: u64| {
        let (bytes, rel) = &spans[k];
        if let Some((symbol, offsets)) = &stubs {
            for &(at, t) in &relocs[rel.clone()] {
                let offset = base + u64::from(at) - bytes.start as u64;
                let stub = offsets[types.binary_search(&t).unwrap_or(0)];
                obj.add_relocation(section, offset, target.pointer_reloc, *symbol, stub);
            }
        }
        result[by_order[k] as usize] =
            Lsda { symbol: obj.section_symbol(section), addend: base as i64 };
    };
    if any_own {
        let mut shared: Option<SectionId> = None;
        for (k, &i) in by_order.iter().enumerate() {
            let f = &funcs[i as usize];
            let bytes = &all[spans[k].0.clone()];
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
            let base = obj.append(section, bytes);
            place(obj, section, k, base);
        }
    } else {
        // Every table in the one shared section: its bytes as encoded.
        let section = obj.add_section(b".gcc_except_table", kind, 4)?;
        let starts: Vec<u64> = spans.iter().map(|s| s.0.start as u64).collect();
        for (k, base) in starts.into_iter().enumerate() {
            place(obj, section, k, base);
        }
        obj.append_owned(section, all);
    }
    Ok(result)
}
