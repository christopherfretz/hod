//! Signed return addresses (WP-185, `spec/eh.md` §4.5): the
//! `DW_CFA_AARCH64_negate_ra_state` encoding, the B key's CIE
//! augmentation in `.eh_frame` (§2.2, the order of §5) and the FDE order
//! of `.debug_frame` (§10.2).

use hod_eh::{AARCH64, Fde, Lsda, Op, Row, write_debug_frame, write_eh_frame, write_op};
use hod_obj::{A64Reloc, Arch, Object, RelocKind, StrId, Symbol, SymbolId};

const SIGNED: [Row; 4] = [
    Row { offset: 0, op: Op::BKeyFrame },
    Row { offset: 4, op: Op::NegateRaState },
    Row { offset: 8, op: Op::DefCfaOffset(16) },
    Row { offset: 60, op: Op::NegateRaState },
];
const SIGNED_A: [Row; 3] = [
    Row { offset: 4, op: Op::NegateRaState },
    Row { offset: 8, op: Op::DefCfaOffset(16) },
    Row { offset: 60, op: Op::NegateRaState },
];

fn object(n: u32) -> (Object, Vec<SymbolId>) {
    let mut obj = Object::new(Arch::AArch64);
    let syms = (0..n).map(|_| obj.add_symbol(Symbol::undefined(StrId::EMPTY))).collect();
    (obj, syms)
}

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

/// Each entry: offset, CIE id or pointer, and for a CIE its augmentation.
fn entries(data: &[u8]) -> Vec<(usize, u32, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let len = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let id = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap());
        let aug = if id == 0 || id == u32::MAX {
            let start = at + 9;
            let end = start + data[start..].iter().position(|&b| b == 0).unwrap();
            String::from_utf8(data[start..end].to_vec()).unwrap()
        } else {
            String::new()
        };
        out.push((at, id, aug));
        at += len + 4;
    }
    out
}

#[test]
fn the_row_encodings() {
    let mut out = Vec::new();
    write_op(&mut out, &AARCH64, Op::NegateRaState).unwrap();
    assert_eq!(out, [0x2d]);
    out.clear();
    write_op(&mut out, &AARCH64, Op::BKeyFrame).unwrap();
    assert!(out.is_empty());
}

/// §2.2, §5: `B` CIEs after their plain twin; the FDEs of each in order;
/// the instructions of a signed FDE are `advance 4, negate, ...`.
#[test]
fn b_key_cies_in_eh_frame() {
    let (mut obj, s) = object(3);
    let (text, p, lsda) = (s[0], s[1], s[2]);
    let with = |order, rows| Fde {
        personality: Some(p),
        lsda: Some(Lsda { symbol: lsda, addend: i64::from(order) }),
        ..fde(order, text, rows)
    };
    let fdes = [
        with(5, &SIGNED_A),
        fde(4, text, &SIGNED),
        with(3, &SIGNED),
        fde(2, text, &SIGNED_A),
        fde(1, text, &SIGNED),
        fde(0, text, &SIGNED_A),
    ];
    let section = write_eh_frame(&mut obj, &fdes).unwrap().unwrap();
    let data = obj.data(section).to_vec();
    let e = entries(&data);
    let shape: Vec<&str> = e.iter().map(|x| if x.1 == 0 { x.2.as_str() } else { "fde" }).collect();
    assert_eq!(shape, ["zR", "fde", "fde", "zRB", "fde", "fde", "zPLR", "fde", "zPLRB", "fde"]);
    let pcs: Vec<i64> = obj
        .relocations(section)
        .filter(|r| r.kind == RelocKind::A64(A64Reloc::Prel32))
        .map(|r| r.addend)
        .collect();
    assert_eq!(pcs, [0, 128, 64, 256, 320, 192]);
    // Every FDE's instructions begin with `advance_loc 4; negate_ra_state`.
    for &(at, id, _) in &e {
        if id == 0 {
            continue;
        }
        let aug_len = data[at + 16] as usize;
        let first = at + 17 + aug_len;
        assert_eq!(data[first..first + 3], [0x44, 0x2d, 0x44], "FDE at {at}");
    }
}

/// §10.2: one CIE without augmentation; the B-key FDEs after the others.
#[test]
fn debug_frame_sorts_b_key_last() {
    let (mut obj, s) = object(1);
    let fdes = [fde(2, s[0], &SIGNED), fde(1, s[0], &SIGNED_A), fde(0, s[0], &SIGNED)];
    let section = write_debug_frame(&mut obj, &fdes).unwrap().unwrap();
    let e = entries(obj.data(section));
    assert_eq!(e.iter().filter(|x| x.1 == u32::MAX).count(), 1);
    assert_eq!(e[0].2, "");
    let pcs: Vec<i64> = obj
        .relocations(section)
        .filter(|r| r.kind == RelocKind::A64(A64Reloc::Abs64))
        .map(|r| r.addend)
        .collect();
    assert_eq!(pcs, [64, 0, 128]);
}
