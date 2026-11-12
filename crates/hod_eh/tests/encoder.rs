//! Units of `spec/eh.md`: LEB128 (§4.4), the advance opcodes (§4.1), the
//! instruction forms (§4.2), CIE sharing and order (§5), padding (§3.4),
//! determinism (§5) and errors (§3.5).

use hod_eh::{
    AARCH64, Error, Fde, Lsda, Op, Row, X86_64, personality_ref, uleb_len, write_advance,
    write_eh_frame, write_op, write_sleb, write_uleb,
};
use hod_obj::{Arch, Object, RelocKind, SectionKind, StrId, Symbol, SymbolId};

fn uleb(v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    write_uleb(&mut out, v);
    out
}

fn sleb(v: i64) -> Vec<u8> {
    let mut out = Vec::new();
    write_sleb(&mut out, v);
    out
}

/// §4.4: DWARF 5 §7.6's examples and the edges of each length.
#[test]
fn leb128() {
    let u: &[(u64, &[u8])] = &[
        (0, &[0]),
        (2, &[2]),
        (127, &[0x7f]),
        (128, &[0x80, 1]),
        (129, &[0x81, 1]),
        (130, &[0x82, 1]),
        (12857, &[0xb9, 0x64]),
        (624_485, &[0xe5, 0x8e, 0x26]),
        (u64::MAX, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1]),
    ];
    for &(v, bytes) in u {
        assert_eq!(uleb(v), bytes, "uleb {v}");
        assert_eq!(uleb_len(v), bytes.len(), "uleb_len {v}");
    }
    let s: &[(i64, &[u8])] = &[
        (0, &[0]),
        (2, &[2]),
        (-2, &[0x7e]),
        (63, &[0x3f]),
        (64, &[0xc0, 0]),
        (-64, &[0x40]),
        (-65, &[0xbf, 0x7f]),
        (127, &[0xff, 0]),
        (-127, &[0x81, 0x7f]),
        (128, &[0x80, 1]),
        (-128, &[0x80, 0x7f]),
        (-123_456, &[0xc0, 0xbb, 0x78]),
        (i64::MIN, &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f]),
        (i64::MAX, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]),
    ];
    for &(v, bytes) in s {
        assert_eq!(sleb(v), bytes, "sleb {v}");
    }
}

/// §4.1: the shortest advance at each boundary.
#[test]
fn advance_opcode_boundaries() {
    let cases: &[(u32, &[u8])] = &[
        (0, &[]),
        (1, &[0x41]),
        (0x3f, &[0x7f]),
        (0x40, &[0x02, 0x40]),
        (0xff, &[0x02, 0xff]),
        (0x100, &[0x03, 0x00, 0x01]),
        (0xffff, &[0x03, 0xff, 0xff]),
        (0x1_0000, &[0x04, 0x00, 0x00, 0x01, 0x00]),
        (u32::MAX, &[0x04, 0xff, 0xff, 0xff, 0xff]),
    ];
    for &(delta, bytes) in cases {
        let mut out = Vec::new();
        write_advance(&mut out, delta);
        assert_eq!(out, bytes, "delta {delta:#x}");
    }
}

fn op(target: &hod_eh::CfiTarget, op: Op) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    write_op(&mut out, target, op).map(|()| out)
}

/// §4.2: the form of each instruction, including those `llc` never needs
/// (negative factored offsets, registers from 64 on).
#[test]
fn instruction_forms() {
    let a = &AARCH64;
    let cases: &[(Op, &[u8])] = &[
        (Op::DefCfa { reg: 31, offset: 0 }, &[0x0c, 31, 0]),
        (Op::DefCfa { reg: 29, offset: 200 }, &[0x0c, 29, 0xc8, 1]),
        (Op::DefCfa { reg: 29, offset: -16 }, &[0x12, 29, 4]),
        (Op::DefCfaOffset(70016), &[0x0e, 0x80, 0xa3, 0x04]),
        (Op::DefCfaOffset(-8), &[0x13, 2]),
        (Op::DefCfaRegister(29), &[0x0d, 29]),
        (Op::DefCfaRegister(300), &[0x0d, 0xac, 2]),
        (Op::Offset { reg: 30, cfa_offset: -8 }, &[0x80 | 0x1e, 2]),
        (Op::Offset { reg: 0, cfa_offset: 0 }, &[0x80, 0]),
        (Op::Offset { reg: 72, cfa_offset: -40 }, &[0x05, 72, 10]),
        (Op::Offset { reg: 19, cfa_offset: 8 }, &[0x11, 19, 0x7e]),
        (Op::Offset { reg: 72, cfa_offset: 8 }, &[0x11, 72, 0x7e]),
        (Op::Restore { reg: 30 }, &[0xc0 | 0x1e]),
        (Op::Restore { reg: 63 }, &[0xff]),
        (Op::Restore { reg: 64 }, &[0x06, 64]),
    ];
    for &(o, bytes) in cases {
        assert_eq!(op(a, o).as_deref(), Some(bytes), "{o:?}");
    }
    // Offsets the data alignment factor does not divide.
    assert_eq!(op(a, Op::Offset { reg: 30, cfa_offset: -6 }), None);
    assert_eq!(op(a, Op::DefCfa { reg: 29, offset: -6 }), None);
    assert_eq!(op(a, Op::DefCfaOffset(-2)), None);
    // A positive CFA offset needs no factoring.
    assert_eq!(op(a, Op::DefCfaOffset(6)).as_deref(), Some(&[0x0e, 6][..]));
    assert_eq!(
        op(&X86_64, Op::Offset { reg: 6, cfa_offset: -16 }).as_deref(),
        Some(&[0x86, 2][..])
    );
}

/// An object with `n` placeholder symbols to name.
fn object(n: u32) -> (Object, Vec<SymbolId>) {
    let mut obj = Object::new(Arch::AArch64);
    let syms = (0..n).map(|_| obj.add_symbol(Symbol::undefined(StrId::EMPTY))).collect();
    (obj, syms)
}

const PROLOGUE: [Row; 4] = [
    Row { offset: 4, op: Op::DefCfaOffset(32) },
    Row { offset: 12, op: Op::DefCfa { reg: 29, offset: 16 } },
    Row { offset: 12, op: Op::Offset { reg: 30, cfa_offset: -8 } },
    Row { offset: 12, op: Op::Offset { reg: 29, cfa_offset: -16 } },
];

fn fde(order: u32, start: SymbolId, rows: &[Row]) -> Fde<'_> {
    Fde {
        order,
        start,
        start_addend: i64::from(order) * 64,
        size: 64,
        rows,
        personality: None,
        lsda: None,
    }
}

/// One entry of the section: offset, length, and the CIE id or pointer.
fn entries(data: &[u8]) -> Vec<(usize, u32, u32)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let len = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let id = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap());
        out.push((at, len, id));
        at += len as usize + 4;
    }
    assert_eq!(at, data.len(), "entries cover the section");
    out
}

/// §5: one CIE per personality (and one for none), the one without first,
/// then by the first function that uses each; FDEs by `order` within.
#[test]
fn cie_sharing_and_order() {
    let (mut obj, s) = object(4);
    let (text, p, q, lsda) = (s[0], s[1], s[2], s[3]);
    let with = |order, personality, rows| Fde {
        personality: Some(personality),
        lsda: Some(Lsda { symbol: lsda, addend: i64::from(order) }),
        ..fde(order, text, rows)
    };
    // Module order: plain 0, q 1, p 2, plain 3, q 4, p 5.
    let fdes = [
        with(5, p, &PROLOGUE),
        fde(3, text, &PROLOGUE),
        with(1, q, &PROLOGUE),
        with(2, p, &PROLOGUE),
        fde(0, text, &PROLOGUE),
        with(4, q, &PROLOGUE),
    ];
    let section = write_eh_frame(&mut obj, &fdes).unwrap().unwrap();
    let data = obj.data(section);
    let e = entries(data);
    let cies: Vec<usize> = e.iter().filter(|x| x.2 == 0).map(|x| x.0).collect();
    assert_eq!(cies.len(), 3);
    // CIE, plain 0, plain 3, CIE(q), q 1, q 4, CIE(p), p 2, p 5.
    let kinds: Vec<Option<usize>> = e
        .iter()
        .map(|&(at, _, id)| {
            (id != 0).then(|| cies.iter().position(|&c| c == at + 4 - id as usize).unwrap())
        })
        .collect();
    assert_eq!(kinds, [None, Some(0), Some(0), None, Some(1), Some(1), None, Some(2), Some(2)]);
    let relocs: Vec<_> = obj.relocations(section).collect();
    // The initial locations' addends give the FDE order; the personality
    // relocations name q, then p.
    let pcs: Vec<i64> = relocs
        .iter()
        .filter(|r| r.kind == RelocKind::A64(hod_obj::A64Reloc::Prel32))
        .map(|r| r.addend)
        .collect();
    assert_eq!(pcs, [0, 192, 64, 256, 128, 320]);
    let personalities: Vec<SymbolId> =
        relocs.iter().filter(|r| r.symbol == p || r.symbol == q).map(|r| r.symbol).collect();
    assert_eq!(personalities, [q, p]);
    let lsdas: Vec<i64> = relocs.iter().filter(|r| r.symbol == lsda).map(|r| r.addend).collect();
    assert_eq!(lsdas, [1, 4, 2, 5]);
    // Relocations are added in offset order.
    assert!(relocs.windows(2).all(|w| w[0].offset < w[1].offset));
}

/// §3.4: every entry is padded with `DW_CFA_nop` to 4 bytes, the last to
/// the section's alignment, 8.
#[test]
fn padding() {
    let rows: Vec<Row> = (0..12).map(|i| Row { offset: 4 * i, op: Op::DefCfaOffset(16) }).collect();
    for n in 0..rows.len() {
        for last in 0..rows.len() {
            let (mut obj, s) = object(1);
            let fdes = [fde(0, s[0], &rows[..n]), fde(1, s[0], &rows[..last])];
            let section = write_eh_frame(&mut obj, &fdes).unwrap().unwrap();
            let data = obj.data(section);
            assert_eq!(data.len() % 8, 0, "{n} {last}: section size");
            let e = entries(data);
            for &(at, len, _) in &e {
                assert_eq!((at + len as usize + 4) % 4, 0, "{n} {last}: entry at {at}");
            }
            // Each FDE's instructions are its rows' (one `DW_CFA_advance_loc`
            // and a two-byte `DW_CFA_def_cfa_offset` each, the first row at
            // offset 0 without an advance) and then at most the padding
            // alignment needs, all `DW_CFA_nop`.
            for (&(at, len, _), rows) in e[1..].iter().zip([n, last]) {
                let insns = 17 + 3 * rows - usize::from(rows > 0);
                let align = if at == e.last().unwrap().0 { 8 } else { 4 };
                let end = at + len as usize + 4;
                assert!(end - (at + insns) < align, "{n} {last}: padding at {at}");
                assert!(data[at + insns..end].iter().all(|&b| b == 0), "{n} {last}: nops");
            }
        }
    }
}

/// §5: the output depends on the set of FDEs, not on the order they are
/// passed in, as when worker threads finish in any order.
#[test]
fn determinism_under_permuted_input() {
    let (_, s) = object(4);
    let rows: Vec<Vec<Row>> = (0..50u32)
        .map(|i| {
            let mut r = PROLOGUE.to_vec();
            r.push(Row { offset: 16 + 4 * (i % 7), op: Op::DefCfa { reg: 31, offset: 32 } });
            r
        })
        .collect();
    let base: Vec<Fde<'_>> = (0..50u32)
        .map(|i| {
            let mut f = fde(i * 3, s[0], &rows[i as usize]);
            if i % 4 == 1 {
                f.personality = Some(s[1 + (i as usize / 4) % 2]);
                f.lsda = Some(Lsda { symbol: s[3], addend: i64::from(i) * 12 });
            }
            f
        })
        .collect();
    let encode = |fdes: &[Fde<'_>]| {
        let (mut obj, _) = object(4);
        let section = write_eh_frame(&mut obj, fdes).unwrap().unwrap();
        (obj.data(section).to_vec(), obj.relocations(section).collect::<Vec<_>>())
    };
    let want = encode(&base);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for round in 0..40 {
        let mut permuted = base.clone();
        for i in (1..permuted.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            permuted.swap(i, (seed % (i as u64 + 1)) as usize);
        }
        assert_eq!(encode(&permuted), want, "permutation {round}");
    }
}

/// §3.5: input the encoder cannot represent is an error naming the FDE and
/// row, never a panic, and leaves the object as it was.
#[test]
fn errors_leave_the_object_unchanged() {
    let check = |fdes: &[Fde<'_>], want: Error| {
        let (mut obj, _) = object(4);
        let before = obj.section_count();
        assert_eq!(write_eh_frame(&mut obj, fdes), Err(want));
        assert_eq!(obj.section_count(), before);
    };
    let (_, s) = object(4);
    let out_of_order =
        [Row { offset: 8, op: Op::DefCfaOffset(16) }, Row { offset: 4, op: Op::DefCfaOffset(32) }];
    check(
        &[fde(0, s[0], &PROLOGUE), fde(1, s[0], &out_of_order)],
        Error::RowOutOfOrder { order: 1, row: 1 },
    );
    let beyond = [Row { offset: 65, op: Op::DefCfaOffset(16) }];
    check(&[fde(7, s[0], &beyond)], Error::RowBeyondEnd { order: 7, row: 0 });
    let unfactored = [PROLOGUE[0], Row { offset: 8, op: Op::Offset { reg: 30, cfa_offset: -6 } }];
    check(&[fde(2, s[0], &unfactored)], Error::Unfactored { order: 2, row: 1 });
    let lsda = Fde { lsda: Some(Lsda { symbol: s[3], addend: 0 }), ..fde(3, s[0], &PROLOGUE) };
    check(&[lsda], Error::LsdaWithoutPersonality { order: 3 });
    check(&[fde(4, s[0], &PROLOGUE), fde(4, s[1], &PROLOGUE)], Error::DuplicateOrder { order: 4 });
    let with = |order, p| Fde { personality: Some(p), ..fde(order, s[0], &PROLOGUE) };
    check(&[with(5, s[1]), with(5, s[2])], Error::DuplicateOrder { order: 5 });
    // A row at the very end of the function is accepted.
    let at_end = [Row { offset: 64, op: Op::DefCfaOffset(0) }];
    let (mut obj, _) = object(1);
    assert!(write_eh_frame(&mut obj, &[fde(0, s[0], &at_end)]).is_ok());
    // No FDEs: no section.
    let (mut obj, _) = object(1);
    assert_eq!(write_eh_frame(&mut obj, &[]), Ok(None));
    assert_eq!(obj.section_count(), 0);
}

/// §2.2: the personality slot's shape (its agreement with llc's is checked
/// in `llc_oracle.rs`).
#[test]
fn personality_slot() {
    let mut obj = Object::new(Arch::AArch64);
    let name = obj.add_string(b"__gxx_personality_v0").unwrap();
    let routine = obj.add_symbol(Symbol::undefined(name));
    let slot = personality_ref(&mut obj, routine).unwrap();
    let symbol = *obj.symbol(slot);
    assert_eq!(obj.string(symbol.name), b"DW.ref.__gxx_personality_v0");
    let hod_obj::SymbolPlace::Section(section) = symbol.place else { panic!("defined") };
    assert_eq!(obj.section_name(section), b".data.DW.ref.__gxx_personality_v0");
    assert_eq!(obj.section_kind(section), SectionKind::Data);
    assert_eq!(obj.data(section), [0; 8]);
    let group = obj.section_group(section).unwrap();
    assert_eq!(obj.group_signature(group), slot);
    let relocs: Vec<_> = obj.relocations(section).collect();
    assert_eq!(relocs.len(), 1);
    assert_eq!(
        (relocs[0].symbol, relocs[0].kind),
        (routine, RelocKind::A64(hod_obj::A64Reloc::Abs64))
    );
}
