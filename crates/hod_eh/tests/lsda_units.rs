//! `spec/eh.md` §9.8: units of the LSDA writer: exact bytes of small
//! tables, the type table and its stubs, filters sharing a terminator, the
//! call-site merge and gap rules, the `TType` base offset at every size,
//! sections and groups, independence from the input order, and every error
//! with the object unchanged.

use hod_eh::{CallSite, Clause, Error, FuncLsda, LsdaOptions, NO_PAD, Pad, write_lsdas};
use hod_obj::{
    A64Reloc, Arch, GroupKind, Object, RelocKind, SectionId, SectionKind, Symbol, SymbolId,
};

/// An object with a `.text` and named undefined type-info symbols.
fn object(arch: Arch, types: &[&str]) -> (Object, SectionId, Vec<SymbolId>) {
    let mut obj = Object::new(arch);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let syms = types
        .iter()
        .map(|t| {
            let name = obj.add_string(t.as_bytes()).unwrap();
            obj.add_symbol(Symbol::undefined(name))
        })
        .collect();
    (obj, text, syms)
}

#[allow(clippy::too_many_arguments)]
fn func<'a>(
    order: u32,
    name: &'a [u8],
    section: SectionId,
    size: u32,
    call_sites: &'a [CallSite],
    pads: &'a [Pad],
    clauses: &'a [Clause],
    type_ids: &'a [Option<SymbolId>],
    filter_ids: &'a [u32],
) -> FuncLsda<'a> {
    FuncLsda { order, name, section, size, call_sites, pads, clauses, type_ids, filter_ids }
}

fn pad(offset: u32, cleanup: bool, clauses: std::ops::Range<u32>) -> Pad {
    Pad { block: 0, offset, cleanup, clauses }
}

fn site(start: u32, len: u32, pad: u32) -> CallSite {
    CallSite { start, len, pad, landing: 0 }
}

fn find(obj: &Object, name: &[u8]) -> Option<SectionId> {
    (0..obj.section_count()).map(SectionId::from_index).find(|&s| obj.section_name(s) == name)
}

#[test]
fn cleanup_only_bytes() {
    let (mut obj, text, _) = object(Arch::AArch64, &[]);
    let pads = [pad(16, true, 0..0)];
    let sites = [site(4, 4, 0), site(8, 4, NO_PAD)];
    let f = func(0, b"f", text, 24, &sites, &pads, &[], &[], &[]);
    let lsdas = write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
    let s = find(&obj, b".gcc_except_table").unwrap();
    // LPStart and TType omitted, call sites in ULEB128, 8 bytes of them: the
    // padded call and the gap after it to the end; no actions.
    assert_eq!(obj.data(s), [0xff, 0xff, 0x01, 0x08, 4, 4, 16, 0, 8, 16, 0, 0]);
    assert_eq!(obj.relocation_count(s), 0);
    assert_eq!(lsdas[0].addend, 0);
    assert_eq!(lsdas[0].symbol, obj.section_symbol(s));
    assert!(find(&obj, b".data").is_none(), "no stubs without types");
    assert_eq!(obj.section_align(s), 4);
}

#[test]
fn type_table_and_stubs() {
    let (mut obj, text, t) = object(Arch::AArch64, &["_ZTIl", "_ZTIi"]);
    // catch (int) catch (...) catch (long); type ids: 1 = int, 2 = all,
    // 3 = long.
    let types = [Some(t[1]), None, Some(t[0])];
    let clauses = [Clause::Catch(1), Clause::Catch(2), Clause::Catch(3)];
    let pads = [pad(20, false, 0..3)];
    let sites = [site(4, 4, 0)];
    let f = func(0, b"f", text, 24, &sites, &pads, &clauses, &types, &[]);
    write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
    let s = find(&obj, b".gcc_except_table").unwrap();
    let d = obj.data(s).to_vec();
    // Header: TType indirect|pcrel|sdata8, base offset; one call site; the
    // chain 1 -> 2 -> 3 as three records, the last first.
    assert_eq!(&d[..2], &[0xff, 0x9c]);
    let ttbase = 3 + d[2] as usize;
    assert_eq!(&d[3..9], &[0x01, 0x04, 4, 4, 20, 5]);
    assert_eq!(&d[9..15], &[3, 0, 2, 0x7d, 1, 0x7d]);
    assert_eq!(ttbase, 16 + 24, "three 8-byte entries after alignment to 4");
    assert_eq!(&d[15..16], &[0]);
    // Entries type id 3, 2, 1: long (a stub), catch-all (zero), int.
    let data = find(&obj, b".data").expect("a .data for the stubs");
    let stubs: Vec<_> = obj.relocations(data).map(|r| (r.offset, r.symbol, r.kind)).collect();
    let abs = RelocKind::A64(A64Reloc::Abs64);
    // By name: _ZTIi before _ZTIl.
    assert_eq!(stubs, [(0, t[1], abs), (8, t[0], abs)]);
    let stub = obj.section_symbol(data);
    let relocs: Vec<_> = obj.relocations(s).map(|r| (r.offset, r.symbol, r.addend)).collect();
    assert_eq!(relocs, [(16, stub, 8), (32, stub, 0)]);
    assert!(d[24..32].iter().all(|&b| b == 0), "catch-all is a null entry");
    assert_eq!(d.len(), ttbase);
}

#[test]
fn filters_share_tails_and_the_empty_filter_a_terminator() {
    let (mut obj, text, t) = object(Arch::X86_64, &["_ZTIi", "_ZTIl"]);
    let types = [Some(t[0]), Some(t[1])];
    // Pad 0: throw(int, long); pad 1: throw(long), the tail of pad 0's;
    // pad 2: throw(), which ends at pad 0's terminator.
    let filter_ids = [1, 2, 2];
    let clauses = [Clause::Filter(0..2), Clause::Filter(2..3), Clause::Filter(0..0)];
    let pads = [pad(40, false, 0..1), pad(44, false, 1..2), pad(48, false, 2..3)];
    let sites = [site(0, 5, 0), site(5, 5, 1), site(10, 5, 2)];
    let f = func(0, b"f", text, 52, &sites, &pads, &clauses, &types, &filter_ids);
    write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
    let s = find(&obj, b".gcc_except_table").unwrap();
    let d = obj.data(s);
    assert_eq!(d[1], 0x9b, "x86-64 TType: indirect|pcrel|sdata4");
    let ttbase = 3 + d[2] as usize;
    // The exception specifications: one list, 1 2 0.
    assert_eq!(&d[ttbase..ttbase + 3], &[1, 2, 0]);
    // Actions, sorted by filter id (-3 for throw(), -2 for the tail, -1):
    // records -3 (offset of the terminator: -3), -2, -1.
    let cst = 5;
    assert_eq!(d[3..5], [0x01, 12]);
    let actions = &d[cst + 12..cst + 18];
    assert_eq!(actions, &[0x7d, 0, 0x7e, 0, 0x7f, 0]);
    // Call sites: pad 0 gets the record of -1 (action 5), pad 1 of -2 (3),
    // pad 2 of -3 (1).
    assert_eq!(&d[cst..cst + 12], &[0, 5, 40, 5, 5, 5, 44, 3, 10, 5, 48, 1]);
    assert_eq!(obj.relocation_count(s), 2);
}

#[test]
fn rows_merge_and_gaps_get_entries() {
    let (mut obj, text, _) = object(Arch::AArch64, &[]);
    let pads = [pad(100, true, 0..0), pad(104, true, 0..0)];
    // A call without a pad first; two rows of pad 0 back to back (merged);
    // a row of pad 0 after a gap with no call (merged across it); a call
    // without a pad; pad 0 again (not merged); pad 1 (not merged); with an
    // explicit landing address (a stub at 108).
    let sites = [
        site(0, 4, NO_PAD),
        site(8, 4, 0),
        site(12, 8, 0),
        site(40, 4, 0),
        site(48, 4, NO_PAD),
        site(56, 4, 0),
        site(64, 4, 1),
        CallSite { start: 72, len: 4, pad: 1, landing: 108 },
    ];
    let f = func(0, b"f", text, 112, &sites, &pads, &[], &[], &[]);
    write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
    let d = obj.data(find(&obj, b".gcc_except_table").unwrap());
    let cst = &d[4..4 + d[3] as usize];
    assert_eq!(
        cst,
        &[
            0, 8, 0, 0, // the first call: from the start to the first pad's row
            8, 36, 100, 0, // 8..44, merged
            44, 12, 0, 0, // the gap with the call at 48
            56, 4, 100, 0, // pad 0
            64, 4, 104, 0, // pad 1
            72, 4, 108, 0, // pad 1 at its stub
        ]
    );
    // Only calls without pads: one entry for the whole function.
    let (mut obj, text, _) = object(Arch::AArch64, &[]);
    let sites = [site(4, 4, NO_PAD), site(12, 4, NO_PAD)];
    let f = func(0, b"g", text, 20, &sites, &pads, &[], &[], &[]);
    write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
    let d = obj.data(find(&obj, b".gcc_except_table").unwrap());
    assert_eq!(d, [0xff, 0xff, 0x01, 4, 0, 20, 0, 0]);
}

/// The `TType` base offset is ULEB128 whose size the alignment before the
/// type table depends on; at every size the field must point exactly at
/// the end of the type table, and the field grows only as needed.
#[test]
fn ttype_base_offset_at_every_size() {
    for types in 1..24u32 {
        for calls in 1..48u32 {
            let (mut obj, text, syms) =
                object(Arch::AArch64, &(0..types).map(|_| "t").collect::<Vec<_>>());
            let type_ids: Vec<Option<SymbolId>> = syms.iter().map(|&s| Some(s)).collect();
            let clauses: Vec<Clause> = (1..=types).map(Clause::Catch).collect();
            let pads = [pad(4 * calls, false, 0..types)];
            let sites: Vec<CallSite> = (0..calls).map(|i| site(4 * i, 4, 0)).collect();
            let f = func(0, b"f", text, 4 * calls + 4, &sites, &pads, &clauses, &type_ids, &[]);
            write_lsdas(&mut obj, &[f], LsdaOptions::default()).unwrap();
            let d = obj.data(find(&obj, b".gcc_except_table").unwrap());
            let (mut p, mut value, mut shift) = (2usize, 0usize, 0);
            loop {
                let b = d[p];
                p += 1;
                value |= usize::from(b & 0x7f) << shift;
                shift += 7;
                if b & 0x80 == 0 {
                    break;
                }
            }
            let ttbase = p + value;
            assert_eq!(ttbase, d.len(), "{types} types, {calls} calls: the base ends the table");
            let need = hod_eh::uleb_len(value as u64);
            assert!(p - 2 <= need + 1, "{types} types, {calls} calls: the field is not oversized");
        }
    }
}

#[test]
fn sections_follow_the_function() {
    let (mut obj, text, _) = object(Arch::AArch64, &[]);
    let in_group = obj.add_section(b".text.g", SectionKind::Text, 4).unwrap();
    let sig_name = obj.add_string(b"g").unwrap();
    let sig = obj.add_symbol(Symbol::undefined(sig_name));
    let group = obj.add_group(sig, GroupKind::Comdat);
    obj.add_group_member(group, in_group).unwrap();
    let pads = [pad(8, true, 0..0)];
    let sites = [site(0, 4, 0)];
    let a = func(0, b"a", text, 12, &sites, &pads, &[], &[], &[]);
    let b = func(1, b"b", text, 12, &sites, &pads, &[], &[], &[]);
    let g = func(2, b"g", in_group, 12, &sites, &pads, &[], &[], &[]);
    let lsdas = write_lsdas(&mut obj, &[a, b, g], LsdaOptions::default()).unwrap();
    let shared = find(&obj, b".gcc_except_table").unwrap();
    let own = find(&obj, b".gcc_except_table.g").unwrap();
    assert_eq!(obj.section_group(own), Some(group));
    assert_eq!(obj.section_group(shared), None);
    assert_eq!([lsdas[0].addend, lsdas[1].addend, lsdas[2].addend], [0, 8, 0]);
    assert_eq!(lsdas[2].symbol, obj.section_symbol(own));
    // With function sections every function has its own.
    let (mut obj, text, _) = object(Arch::AArch64, &[]);
    let a = func(0, b"a", text, 12, &sites, &pads, &[], &[], &[]);
    let b = func(1, b"b", text, 12, &sites, &pads, &[], &[], &[]);
    let opts = LsdaOptions { function_sections: true, stubs: None };
    write_lsdas(&mut obj, &[a, b], opts).unwrap();
    assert!(find(&obj, b".gcc_except_table").is_none());
    assert!(find(&obj, b".gcc_except_table.a").is_some());
    assert!(find(&obj, b".gcc_except_table.b").is_some());
}

/// Everything about `obj` a test can see.
type Snapshot = Vec<(Vec<u8>, Vec<u8>, Vec<(u64, u32, i64)>)>;

fn snapshot(obj: &Object) -> Snapshot {
    (0..obj.section_count())
        .map(SectionId::from_index)
        .map(|s| {
            let relocs =
                obj.relocations(s).map(|r| (r.offset, r.symbol.index(), r.addend)).collect();
            (obj.section_name(s).to_vec(), obj.data(s).to_vec(), relocs)
        })
        .collect()
}

#[test]
fn output_does_not_depend_on_input_order() {
    let (_, text, t) = object(Arch::AArch64, &["c", "a", "b"]);
    let types: Vec<[Option<SymbolId>; 2]> = (0..6)
        .map(|i| [Some(t[i % 3]), if i % 2 == 0 { None } else { Some(t[(i + 1) % 3]) }])
        .collect();
    let clauses = [Clause::Catch(1), Clause::Catch(2)];
    let pads = [pad(16, true, 0..2), pad(20, false, 1..2)];
    let sites = [site(0, 4, 0), site(4, 4, NO_PAD), site(8, 4, 1)];
    let funcs: Vec<FuncLsda<'_>> = (0..6)
        .map(|i| func(10 - i, b"f", text, 24, &sites, &pads, &clauses, &types[i as usize], &[]))
        .collect();
    let mut want = None;
    let mut perm = funcs.clone();
    for k in 0..40usize {
        // A different order each time: rotations and swaps.
        perm.rotate_left(k % 6);
        perm.swap(k % 6, (k * 7 + 3) % 6);
        let mut obj = object(Arch::AArch64, &["c", "a", "b"]).0;
        let lsdas = write_lsdas(&mut obj, &perm, LsdaOptions::default()).unwrap();
        let by_order: std::collections::BTreeMap<u32, i64> =
            perm.iter().zip(&lsdas).map(|(f, l)| (f.order, l.addend)).collect();
        let got = (snapshot(&obj), by_order);
        match &want {
            None => want = Some(got),
            Some(w) => assert_eq!(&got, w, "permutation {k}"),
        }
    }
}

#[test]
fn errors_leave_the_object_unchanged() {
    let (base, text, t) = object(Arch::AArch64, &["t"]);
    let types = [Some(t[0])];
    let good_pads = [pad(16, false, 0..1)];
    let catch = [Clause::Catch(1)];
    let sites = [site(0, 4, 0)];
    let ok = func(0, b"f", text, 24, &sites, &good_pads, &catch, &types, &[]);
    let cases: Vec<(FuncLsda<'_>, Error)> = vec![
        (
            FuncLsda {
                call_sites: &[
                    CallSite { start: 8, len: 4, pad: 0, landing: 0 },
                    CallSite { start: 4, len: 4, pad: 0, landing: 0 },
                ],
                ..ok
            },
            Error::CallSiteOutOfOrder { order: 0, row: 1 },
        ),
        (
            FuncLsda {
                call_sites: &[
                    CallSite { start: 0, len: 4, pad: 0, landing: 0 },
                    CallSite { start: 2, len: 4, pad: 0, landing: 0 },
                ],
                ..ok
            },
            Error::CallSiteOutOfOrder { order: 0, row: 1 },
        ),
        (
            FuncLsda { call_sites: &[CallSite { start: 20, len: 8, pad: 0, landing: 0 }], ..ok },
            Error::CallSiteBeyondEnd { order: 0, row: 0 },
        ),
        (
            FuncLsda {
                call_sites: &[CallSite { start: u32::MAX, len: 8, pad: 0, landing: 0 }],
                ..ok
            },
            Error::CallSiteBeyondEnd { order: 0, row: 0 },
        ),
        (
            FuncLsda { call_sites: &[CallSite { start: 0, len: 4, pad: 1, landing: 0 }], ..ok },
            Error::BadPad { order: 0, row: 0 },
        ),
        (
            FuncLsda { call_sites: &[CallSite { start: 0, len: 4, pad: 0, landing: 24 }], ..ok },
            Error::LandingBeyondEnd { order: 0, row: 0 },
        ),
        (
            FuncLsda { pads: &[Pad { block: 0, offset: 16, cleanup: false, clauses: 0..2 }], ..ok },
            Error::BadClauses { order: 0, pad: 0 },
        ),
        (
            FuncLsda { pads: &[Pad { block: 0, offset: 16, cleanup: false, clauses: 0..0 }], ..ok },
            Error::EmptyPad { order: 0, pad: 0 },
        ),
        (FuncLsda { clauses: &[Clause::Catch(0)], ..ok }, Error::BadTypeId { order: 0, clause: 0 }),
        (FuncLsda { clauses: &[Clause::Catch(2)], ..ok }, Error::BadTypeId { order: 0, clause: 0 }),
        (
            FuncLsda { clauses: &[Clause::Filter(0..1)], ..ok },
            Error::BadFilter { order: 0, clause: 0 },
        ),
        (
            FuncLsda { clauses: &[Clause::Filter(0..1)], filter_ids: &[2], ..ok },
            Error::BadFilter { order: 0, clause: 0 },
        ),
        (FuncLsda { name: b"a\0b", ..ok }, Error::BadName { order: 0 }),
    ];
    let before = snapshot(&base);
    for (k, (bad, err)) in cases.into_iter().enumerate() {
        let mut obj = object(Arch::AArch64, &["t"]).0;
        let opts = LsdaOptions { function_sections: k == 12, stubs: None };
        let later = FuncLsda { order: 1, ..ok };
        assert_eq!(write_lsdas(&mut obj, &[later, bad], opts), Err(err), "case {k}");
        assert_eq!(snapshot(&obj), before, "case {k}: object unchanged");
        assert_eq!(obj.symbol_count(), base.symbol_count(), "case {k}: no symbols added");
    }
    let mut obj = object(Arch::AArch64, &["t"]).0;
    let dup = FuncLsda { order: 0, ..ok };
    assert_eq!(
        write_lsdas(&mut obj, &[ok, dup], LsdaOptions::default()),
        Err(Error::DuplicateOrder { order: 0 })
    );
    assert_eq!(snapshot(&obj), before);
    let mut obj = object(Arch::AArch64, &["t"]).0;
    assert_eq!(write_lsdas(&mut obj, &[], LsdaOptions::default()), Ok(Vec::new()));
    assert_eq!(snapshot(&obj), before, "no functions, no sections");
}
