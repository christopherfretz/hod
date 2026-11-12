//! The LSDA round trip of `spec/eh.md` §9.8: llc's `.gcc_except_table`
//! decoded (with its relocations and the `DW.stub` words they reach), turned
//! into `hod_eh`'s input rows, written again by `write_lsdas`, compared, and
//! spliced into llc's object.

#![allow(dead_code, clippy::too_many_lines, clippy::type_complexity)]

use std::collections::{BTreeMap, BTreeSet};

use hod_eh::{CallSite, Clause, FuncLsda, Lsda, LsdaOptions, NO_PAD, Pad, write_lsdas};
use hod_obj::elf::{SHF_ALLOC, SHT_PROGBITS};
use hod_obj::{Arch, GroupKind, Object, SectionId, SectionKind, Symbol, SymbolId};

use super::{Elf, Rela, hex};

fn uleb(d: &[u8], p: &mut usize) -> u64 {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let b = d[*p];
        *p += 1;
        v |= u64::from(b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            return v;
        }
    }
}

fn sleb(d: &[u8], p: &mut usize) -> i64 {
    let (mut v, mut shift) = (0i64, 0);
    loop {
        let b = d[*p];
        *p += 1;
        v |= i64::from(b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            if shift < 64 && b & 0x40 != 0 {
                v |= -1 << shift;
            }
            return v;
        }
    }
}

/// One action of a call site's chain, by meaning.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Act {
    Catch(Option<String>),
    Filter(Vec<Option<String>>),
    Cleanup,
}

/// One call-site entry, by meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub start: u64,
    pub len: u64,
    pub landing: u64,
    pub action: u64,
    pub chain: Vec<Act>,
}

/// An LSDA, decoded.
#[derive(Clone, Debug)]
pub struct Decoded {
    pub ttype_encoding: u8,
    pub sites: Vec<Site>,
    /// The type table, type id 1 first: the type-info's name, `None` for
    /// catch-all.
    pub types: Vec<Option<String>>,
    /// The type ids of each filter value met, by value.
    pub filters: BTreeMap<i64, Vec<u64>>,
    /// The raw action chain of each first action: (filter value) list.
    pub chains: BTreeMap<u64, Vec<i64>>,
    /// Whether two chains share a record that is not the first of both.
    pub shared_tail: bool,
}

/// Decodes the LSDA at `at` in `data`; `type_at(offset)` names the
/// type-info a type-table field at that offset of `data` refers to.
pub fn decode(
    data: &[u8],
    at: usize,
    pointer_size: usize,
    type_at: &dyn Fn(usize) -> Option<String>,
) -> Decoded {
    let mut p = at;
    assert_eq!(data[p], 0xff, "LPStart omitted");
    let ttype_encoding = data[p + 1];
    p += 2;
    let ttbase = if ttype_encoding == 0xff {
        None
    } else {
        let off = uleb(data, &mut p) as usize;
        Some(p + off)
    };
    assert_eq!(data[p], 1, "call sites in ULEB128");
    p += 1;
    let cst_len = uleb(data, &mut p) as usize;
    let cst_end = p + cst_len;
    let mut raw = Vec::new();
    while p < cst_end {
        let s = uleb(data, &mut p);
        let l = uleb(data, &mut p);
        let lp = uleb(data, &mut p);
        let a = uleb(data, &mut p);
        raw.push((s, l, lp, a));
    }
    assert_eq!(p, cst_end, "call-site table length");
    let mut chains = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut steps = 0;
    let mut actions_end = cst_end;
    for &(_, _, _, a) in &raw {
        if a == 0 || chains.contains_key(&a) {
            continue;
        }
        let mut chain = Vec::new();
        let mut q = cst_end + a as usize - 1;
        loop {
            visited.insert(q);
            steps += 1;
            let v = sleb(data, &mut q);
            chain.push(v);
            let field = q;
            let d = sleb(data, &mut q);
            actions_end = actions_end.max(q);
            if d == 0 {
                break;
            }
            q = (field as i64 + d) as usize;
        }
        chains.insert(a, chain);
    }
    let mut types = Vec::new();
    let mut filters = BTreeMap::new();
    if let Some(base) = ttbase {
        let first = actions_end.next_multiple_of(4).max(at);
        let n = (base - first) / pointer_size;
        for k in 1..=n {
            let field = base - k * pointer_size;
            let t = type_at(field);
            if t.is_none() {
                assert!(data[field..field + pointer_size].iter().all(|&b| b == 0), "catch-all");
            }
            types.push(t);
        }
        for chain in chains.values() {
            for &v in chain.iter().filter(|&&v| v < 0) {
                let mut q = base + (-v - 1) as usize;
                let mut ids = Vec::new();
                loop {
                    let t = uleb(data, &mut q);
                    if t == 0 {
                        break;
                    }
                    ids.push(t);
                }
                filters.insert(v, ids);
            }
        }
    }
    let name = |t: u64| types[t as usize - 1].clone();
    let sites = raw
        .iter()
        .map(|&(start, len, landing, action)| {
            let chain = chains
                .get(&action)
                .map(|c| {
                    c.iter()
                        .map(|&v| match v {
                            0 => Act::Cleanup,
                            v if v > 0 => Act::Catch(name(v as u64)),
                            v => Act::Filter(filters[&v].iter().map(|&t| name(t)).collect()),
                        })
                        .collect()
                })
                .unwrap_or_else(|| if landing != 0 { vec![Act::Cleanup] } else { Vec::new() });
            Site { start, len, landing, action, chain }
        })
        .collect();
    Decoded { ttype_encoding, sites, types, filters, chains, shared_tail: steps > visited.len() }
}

/// A function with an LSDA in llc's object.
#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    /// llc's section index of its code, and its offset there.
    pub text: usize,
    pub offset: u64,
    pub size: u32,
    /// llc's section index of its LSDA, and the LSDA's offset there.
    pub lsda_section: usize,
    pub lsda_offset: u64,
}

/// The functions with an LSDA, from `.eh_frame`'s relocations (an FDE's
/// initial location at +8, its size at +12, its LSDA pointer at +17).
pub fn functions(elf: &Elf) -> Vec<Func> {
    let eh = elf.section(".eh_frame").expect(".eh_frame");
    let relas = elf.relas(elf.rela_of(eh).expect(".rela.eh_frame"));
    let data = elf.bytes(eh);
    let mut out = Vec::new();
    for r in &relas {
        let s = &elf.symbols[r.sym as usize];
        if s.info & 0xf != 3
            || !elf.sections[s.shndx as usize].name.starts_with(".gcc_except_table")
        {
            continue;
        }
        let fde = r.offset - 17;
        let pc = relas.iter().find(|q| q.offset == fde + 8).expect("pc relocation");
        let size =
            u32::from_le_bytes(data[fde as usize + 12..fde as usize + 16].try_into().unwrap());
        out.push(Func {
            name: elf.function_at(pc.sym, pc.addend),
            text: elf.symbols[pc.sym as usize].shndx as usize,
            offset: pc.addend as u64,
            size,
            lsda_section: s.shndx as usize,
            lsda_offset: r.addend as u64,
        });
    }
    out
}

/// The group section (its signature symbol) that has `section` as a member.
pub fn group_of(elf: &Elf, section: usize) -> Option<u32> {
    elf.sections.iter().enumerate().find_map(|(i, s)| {
        (s.ty == 17
            && elf.bytes(i)[4..]
                .chunks_exact(4)
                .any(|m| u32::from_le_bytes(m.try_into().unwrap()) as usize == section))
        .then_some(s.info)
    })
}

/// The type-info each `DW.stub` word of llc's `.data` holds, by offset.
pub fn stubs(elf: &Elf) -> BTreeMap<i64, String> {
    let Some(data) = elf.section(".data") else { return BTreeMap::new() };
    let Some(rela) = elf.rela_of(data) else { return BTreeMap::new() };
    elf.relas(rela)
        .iter()
        .map(|r| (r.offset as i64, elf.symbols[r.sym as usize].name.clone()))
        .collect()
}

/// The callees llc's IR declares `nounwind`.
pub fn nounwind(ir: &str) -> BTreeSet<String> {
    ir.lines()
        .filter(|l| l.starts_with("declare ") && l.contains("nounwind"))
        .filter_map(|l| {
            let at = l.find('@')? + 1;
            let end = l[at..].find('(')? + at;
            Some(l[at..end].to_string())
        })
        .collect()
}

/// The calls (start, length, callee) in `[offset, offset + size)` of llc's
/// section `text`.
fn calls(elf: &Elf, arch: Arch, text: usize, offset: u64, size: u32) -> Vec<(u32, u32, String)> {
    let Some(rela) = elf.rela_of(text) else { return Vec::new() };
    let mut out: Vec<(u32, u32, String)> = elf
        .relas(rela)
        .iter()
        .filter(|r| match arch {
            Arch::AArch64 => r.ty == 283,
            Arch::X86_64 => r.ty == 4,
        })
        .filter(|r| r.offset >= offset && r.offset < offset + u64::from(size))
        .map(|r| {
            let at = (r.offset - offset) as u32;
            let (start, len) = match arch {
                Arch::AArch64 => (at, 4),
                Arch::X86_64 => (at - 1, 5),
            };
            (start, len, elf.symbols[r.sym as usize].name.clone())
        })
        .collect();
    out.sort();
    out
}

/// hod's input rows for one function, owned.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub call_sites: Vec<CallSite>,
    pub pads: Vec<Pad>,
    pub clauses: Vec<Clause>,
    pub type_ids: Vec<Option<SymbolId>>,
    pub filter_ids: Vec<u32>,
}

/// The rows `hod_cg` would record for the function whose LSDA llc wrote
/// as `d` (`spec/eh.md` §9.8): the type table as llc's; one pad per
/// landing address with the clauses of its chain; a row per call that can
/// throw (a padded entry split at each such call, so that hod has to merge
/// them again), none for a `nounwind` callee.
pub fn rows_from(
    elf: &Elf,
    arch: Arch,
    f: &Func,
    d: &Decoded,
    nounwind: &BTreeSet<String>,
    symbol_of: &dyn Fn(&str) -> SymbolId,
) -> Rows {
    let mut rows = Rows {
        type_ids: d.types.iter().map(|t| t.as_deref().map(symbol_of)).collect(),
        ..Rows::default()
    };
    let mut landings: Vec<(u64, u64)> =
        d.sites.iter().filter(|s| s.landing != 0).map(|s| (s.landing, s.action)).collect();
    landings.sort_unstable();
    landings.dedup();
    for w in landings.windows(2) {
        assert_ne!(w[0].0, w[1].0, "{}: one action per landing pad", f.name);
    }
    for (block, &(landing, action)) in landings.iter().enumerate() {
        let start = rows.clauses.len() as u32;
        let mut cleanup = action == 0;
        for &v in d.chains.get(&action).map_or(&[][..], Vec::as_slice) {
            match v {
                0 => cleanup = true,
                v if v > 0 => rows.clauses.push(Clause::Catch(v as u32)),
                v => {
                    let s = rows.filter_ids.len() as u32;
                    rows.filter_ids.extend(d.filters[&v].iter().map(|&t| t as u32));
                    rows.clauses.push(Clause::Filter(s..rows.filter_ids.len() as u32));
                }
            }
        }
        rows.pads.push(Pad {
            block: block as u32,
            offset: landing as u32,
            cleanup,
            clauses: start..rows.clauses.len() as u32,
        });
    }
    let throwing: Vec<(u32, u32)> = calls(elf, arch, f.text, f.offset, f.size)
        .into_iter()
        .filter(|c| !nounwind.contains(&c.2))
        .map(|c| (c.0, c.1))
        .collect();
    let mut covered = vec![false; throwing.len()];
    for s in &d.sites {
        let (start, end) = (s.start as u32, (s.start + s.len) as u32);
        let inside: Vec<usize> = (0..throwing.len())
            .filter(|&i| throwing[i].0 >= start && throwing[i].0 < end)
            .collect();
        for &i in &inside {
            covered[i] = true;
        }
        if s.landing == 0 {
            assert!(!inside.is_empty(), "{}: a no-pad entry covers a call that can throw", f.name);
            for &i in &inside {
                rows.call_sites.push(CallSite {
                    start: throwing[i].0,
                    len: throwing[i].1,
                    pad: NO_PAD,
                    landing: 0,
                });
            }
        } else {
            let pad = landings.iter().position(|&(l, _)| l == s.landing).unwrap() as u32;
            let mut cuts: Vec<u32> =
                inside.iter().map(|&i| throwing[i].0 + throwing[i].1).collect();
            cuts.retain(|&c| c < end);
            cuts.push(end);
            let mut from = start;
            for (k, &to) in cuts.iter().enumerate() {
                // Every other row names its landing address, the others
                // say "the pad's own" (0).
                let landing = if k % 2 == 0 { 0 } else { s.landing as u32 };
                rows.call_sites.push(CallSite { start: from, len: to - from, pad, landing });
                from = to;
            }
        }
    }
    assert!(covered.iter().all(|&c| c), "{}: every call that can throw has an entry", f.name);
    rows
}

/// The result of re-writing llc's LSDAs with `hod_eh`.
pub struct LsdaTrip {
    /// llc's object with hod's LSDA sections (and their relocations) in
    /// place of llc's.
    pub patched: Vec<u8>,
    /// Each function's LSDA as `write_lsdas` placed it, in llc's symbol
    /// numbering (`SymbolId::from_index(llc index - 1)`).
    pub lsdas: BTreeMap<String, Lsda>,
    /// Whether every LSDA section and its relocations are byte-identical
    /// to llc's.
    pub identical: bool,
    pub functions: usize,
    pub sites: usize,
    /// The case features seen: "catch-all", "filter", "cleanup", "shared
    /// tail", "no-pad entry", ...
    pub features: BTreeSet<&'static str>,
}

/// Re-writes the LSDAs of `object` (llc's, from `ir`) with `hod_eh`, checks
/// them against llc's (`spec/eh.md` §9.8), and splices them in.
/// `mutate` may change the rows first (the wrong-table test).
pub fn lsda_trip(
    label: &str,
    arch: Arch,
    ir: &str,
    object: &[u8],
    function_sections: bool,
    reverse: bool,
    mutate: &dyn Fn(&str, &mut Rows),
) -> LsdaTrip {
    let elf = Elf::parse(object.to_vec());
    let pointer_size = match arch {
        Arch::AArch64 => 8,
        Arch::X86_64 => 4,
    };
    let stubs = stubs(&elf);
    let funcs = functions(&elf);
    let nounwind = nounwind(ir);
    let order_of = |name: &str| {
        let (plain, quoted) = (format!("@{name}("), format!("@\"{name}\"("));
        ir.lines()
            .filter(|l| l.starts_with("define "))
            .position(|l| l.contains(&plain) || l.contains(&quoted))
            .unwrap_or_else(|| panic!("a definition of {name} in the IR")) as u32
    };

    // llc's type-table fields: their section, offset → the type-info.
    let (elf_ref, stubs_ref) = (&elf, &stubs);
    let llc_type_at = |section: usize| {
        let (elf, stubs) = (elf_ref, stubs_ref);
        let relas = elf.rela_of(section).map(|r| elf.relas(r)).unwrap_or_default();
        move |at: usize| {
            relas.iter().find(|r| r.offset == at as u64).map(|r| {
                assert_eq!(elf.sections[elf.symbols[r.sym as usize].shndx as usize].name, ".data");
                stubs[&r.addend].clone()
            })
        }
    };

    // hod's object: one placeholder per llc symbol (named, so that stubs
    // sort by name), llc's code sections with their groups, a `.data`.
    let mut obj = Object::new(arch);
    for s in &elf.symbols[1..] {
        let name = obj.add_string(s.name.as_bytes()).unwrap();
        obj.add_symbol(Symbol::undefined(name));
    }
    let symbol_index: BTreeMap<String, u32> =
        elf.symbols.iter().enumerate().skip(1).map(|(i, s)| (s.name.clone(), i as u32)).collect();
    let symbol_of = |name: &str| SymbolId::from_index(symbol_index[name] - 1);
    let mut texts: BTreeMap<usize, SectionId> = BTreeMap::new();
    for f in &funcs {
        texts.entry(f.text).or_insert_with(|| {
            let s = obj
                .add_section(elf.sections[f.text].name.as_bytes(), SectionKind::Text, 4)
                .unwrap();
            if let Some(sig) = group_of(&elf, f.text) {
                let g = obj.add_group(SymbolId::from_index(sig - 1), GroupKind::Comdat);
                obj.add_group_member(g, s).unwrap();
            }
            s
        });
    }
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();

    let mut features = BTreeSet::new();
    let mut decoded = Vec::new();
    let mut all_rows = Vec::new();
    for f in &funcs {
        let section = elf.bytes(f.lsda_section);
        let d = decode(section, f.lsda_offset as usize, pointer_size, &llc_type_at(f.lsda_section));
        let mut rows = rows_from(&elf, arch, f, &d, &nounwind, &symbol_of);
        mutate(&f.name, &mut rows);
        for s in &d.sites {
            if s.landing == 0 {
                features.insert("no-pad entry");
            }
            for a in &s.chain {
                features.insert(match a {
                    Act::Catch(None) => "catch-all",
                    Act::Catch(Some(_)) => "catch",
                    Act::Filter(v) if v.is_empty() => "empty filter",
                    Act::Filter(_) => "filter",
                    Act::Cleanup => "cleanup",
                });
            }
        }
        if rows.call_sites.len() > d.sites.len() {
            features.insert("merged rows");
        }
        if d.shared_tail {
            features.insert("shared tail");
        }
        if group_of(&elf, f.text).is_some() {
            features.insert("comdat");
        }
        if d.sites.iter().any(|s| s.start + s.len > 127 || s.landing > 127) || section.len() > 140 {
            features.insert("long ULEB128");
        }
        decoded.push(d);
        all_rows.push(rows);
    }
    let mut inputs: Vec<FuncLsda<'_>> = funcs
        .iter()
        .zip(&all_rows)
        .map(|(f, r)| FuncLsda {
            order: order_of(&f.name),
            name: f.name.as_bytes(),
            section: texts[&f.text],
            size: f.size,
            call_sites: &r.call_sites,
            pads: &r.pads,
            clauses: &r.clauses,
            type_ids: &r.type_ids,
            filter_ids: &r.filter_ids,
        })
        .collect();
    if reverse {
        inputs.reverse();
    }
    let mut lsdas =
        write_lsdas(&mut obj, &inputs, LsdaOptions { function_sections, stubs: Some(data) })
            .unwrap_or_else(|e| panic!("{label}: {e}"));
    if reverse {
        lsdas.reverse();
    }

    // The stubs: llc's `.data` holds only them (the cases have no data).
    let hod_stubs: BTreeMap<i64, String> = obj
        .relocations(data)
        .map(|r| (r.offset as i64, elf.symbols[r.symbol.index() as usize + 1].name.clone()))
        .collect();
    assert_eq!(hod_stubs, stubs, "{label}: the DW.stub words");
    let llc_stub_of: BTreeMap<&String, i64> = stubs.iter().map(|(&o, n)| (n, o)).collect();

    // hod's symbols → llc's: placeholders by arithmetic, section symbols by
    // section name.
    let llc_section_symbol = |name: &str| {
        let sec = elf.section(name).unwrap_or_else(|| panic!("{label}: llc has no {name}"));
        elf.symbols
            .iter()
            .position(|s| s.info & 0xf == 3 && s.shndx as usize == sec)
            .unwrap_or_else(|| panic!("{label}: no section symbol for {name}")) as u32
    };
    let section_of_symbol: BTreeMap<SymbolId, SectionId> = (0..obj.section_count())
        .map(SectionId::from_index)
        .map(|sec| (obj.section_symbol(sec), sec))
        .collect();

    // Each LSDA section of hod's: llc's section of that name, its header,
    // group, bytes and relocations; collected for the splice.
    let mut identical = true;
    let mut replacements: Vec<(usize, Vec<u8>)> = Vec::new();
    let stub_sym = Some(obj.section_symbol(data));
    for sec in (0..obj.section_count()).map(SectionId::from_index) {
        let name = std::str::from_utf8(obj.section_name(sec)).unwrap().to_string();
        if !name.starts_with(".gcc_except_table") {
            continue;
        }
        let index = elf.section(&name).unwrap_or_else(|| panic!("{label}: llc has no {name}"));
        let llc = &elf.sections[index];
        assert_eq!(
            obj.section_kind(sec),
            SectionKind::Elf { sh_type: SHT_PROGBITS, sh_flags: SHF_ALLOC, sh_entsize: 0 },
            "{label}: {name} kind"
        );
        assert_eq!(llc.ty, SHT_PROGBITS, "{label}: {name}");
        assert_eq!(llc.flags & !0x200, SHF_ALLOC, "{label}: {name} flags");
        assert_eq!(obj.section_align(sec), llc.align, "{label}: {name} alignment");
        let group = obj.section_group(sec).map(|g| obj.group_signature(g).index() + 1);
        assert_eq!(group, group_of(&elf, index), "{label}: {name} group");
        assert_eq!(llc.flags & 0x200 != 0, group.is_some(), "{label}: {name} SHF_GROUP");
        let bytes = obj.data(sec).to_vec();
        let relas: Vec<Rela> = obj
            .relocations(sec)
            .map(|r| {
                assert_eq!(Some(r.symbol), stub_sym, "{label}: a type-table field names a stub");
                let ti = &hod_stubs[&r.addend];
                Rela {
                    offset: r.offset,
                    sym: llc_section_symbol(".data"),
                    ty: r.kind.elf_type(),
                    addend: llc_stub_of[ti],
                }
            })
            .collect();
        let llc_relas = elf.rela_of(index).map(|r| elf.relas(r)).unwrap_or_default();
        let same = bytes == elf.bytes(index) && relas == llc_relas;
        if !same {
            eprintln!(
                "{label}: {name} differs from llc's\n  llc {}\n  hod {}",
                hex(elf.bytes(index)),
                hex(&bytes)
            );
        }
        identical &= same;
        replacements.push((index, bytes));
        match elf.rela_of(index) {
            Some(r) => replacements.push((r, super::raw_relas(&relas))),
            None => assert!(relas.is_empty(), "{label}: {name}: llc has no relocations"),
        }
    }

    // Decoded equality, per function.
    let mut map = BTreeMap::new();
    let mut sites = 0;
    for (k, f) in funcs.iter().enumerate() {
        let l = lsdas[k];
        let sec = section_of_symbol[&l.symbol];
        let name = std::str::from_utf8(obj.section_name(sec)).unwrap();
        assert_eq!(name, elf.sections[f.lsda_section].name, "{label}: {}: section", f.name);
        let relocs: Vec<(u64, i64)> = obj.relocations(sec).map(|r| (r.offset, r.addend)).collect();
        let hod_type_at =
            |at: usize| relocs.iter().find(|r| r.0 == at as u64).map(|r| hod_stubs[&r.1].clone());
        let ours = decode(obj.data(sec), l.addend as usize, pointer_size, &hod_type_at);
        let theirs = &decoded[k];
        assert_eq!(ours.ttype_encoding, theirs.ttype_encoding, "{label}: {}: TType", f.name);
        assert_eq!(ours.types, theirs.types, "{label}: {}: type table", f.name);
        let meaning = |d: &Decoded| -> Vec<(u64, u64, u64, Vec<Act>)> {
            d.sites.iter().map(|s| (s.start, s.len, s.landing, s.chain.clone())).collect()
        };
        assert_eq!(meaning(&ours), meaning(theirs), "{label}: {}: call sites", f.name);
        if l.addend as u64 != f.lsda_offset {
            identical = false;
        }
        sites += ours.sites.len();
        map.insert(
            f.name.clone(),
            Lsda { symbol: SymbolId::from_index(llc_section_symbol(name) - 1), addend: l.addend },
        );
    }
    LsdaTrip {
        patched: elf.splice(&replacements),
        lsdas: map,
        identical,
        functions: funcs.len(),
        sites,
        features,
    }
}
