//! `spec/dwarf.md` §8.1: the line-table writer and the line policy, unit
//! by unit; and, with `llvm-dwarfdump` of every installed major, a sweep
//! of hand-made tables (many files and directories, long sequences, large
//! jumps both ways, several sections, a COMDAT function) decoded and
//! verified.

mod common;

use hod_dwarf::{
    Error, FileEntry, FileTable, FunctionLines, LineHeader, Row, Sequence, SourceLoc, Transition,
    flags, function_rows, write_debug_line,
};
use hod_obj::{Arch, Binding, GroupKind, Object, SectionKind, Symbol, SymbolId, SymbolKind};
use hod_testutil::Scratch;

const STMT: u8 = flags::IS_STMT;
const PE: u8 = flags::PROLOGUE_END;
const EB: u8 = flags::EPILOGUE_BEGIN;

fn row(offset: u32, file: u32, line: u32, column: u16, flags: u8) -> Row {
    Row { offset, file, line, column, flags }
}

/// An object with a 64 KiB `.text`, and its section symbol.
fn object(arch: Arch) -> (Object, SymbolId) {
    let mut obj = Object::new(arch);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 1 << 16);
    let sym = obj.section_symbol(text);
    (obj, sym)
}

const DIRS: [&[u8]; 2] = [b"/src", b"/usr/include"];
const FILES: [FileEntry<'static>; 2] = [
    FileEntry { name: b"main.cpp", dir: 0, md5: Some([1; 16]) },
    FileEntry { name: b"stdio.h", dir: 1, md5: Some([2; 16]) },
];

fn header() -> LineHeader<'static> {
    LineHeader { dirs: &DIRS, files: &FILES }
}

// ----------------------------------------------------------- header ----

/// §4.1: the header's fields and forms, as `llc` writes them.
#[test]
fn header_layout() {
    let (mut obj, _) = object(Arch::AArch64);
    let s = write_debug_line(&mut obj, &header(), &[]).unwrap();
    let d = obj.data(s.line);
    let len = u32::from_le_bytes(d[..4].try_into().unwrap()) as usize;
    assert_eq!(len + 4, d.len(), "unit_length");
    assert_eq!(&d[4..8], [5, 0, 8, 0], "version, address size, selector size");
    let hlen = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
    assert_eq!(12 + hlen, d.len(), "header_length: no program without sequences");
    assert_eq!(&d[12..30], [1, 1, 1, 0xfb, 14, 13, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]);
    // Directory format, count, two offsets; file format with MD5.
    assert_eq!(&d[30..34], [1, 1, 0x1f, 2]);
    assert_eq!(&d[42..50], [3, 1, 0x1f, 2, 0x0f, 5, 0x1e, 2]);
    assert_eq!(obj.data(s.line_str), b"/src\0/usr/include\0main.cpp\0stdio.h\0");
    let offsets: Vec<(u64, i64)> = obj.relocations(s.line).map(|r| (r.offset, r.addend)).collect();
    assert_eq!(offsets, [(34, 0), (38, 5), (50, 18), (71, 27)]);
}

/// §4.1: MD5 only when every file has one.
#[test]
fn md5_only_when_every_file_has_one() {
    let files = [FILES[0], FileEntry { md5: None, ..FILES[1] }];
    let (mut obj, _) = object(Arch::X86_64);
    let s = write_debug_line(&mut obj, &LineHeader { dirs: &DIRS, files: &files }, &[]).unwrap();
    let d = obj.data(s.line);
    assert_eq!(&d[42..48], [2, 1, 0x1f, 2, 0x0f, 2]);
    assert_eq!(d.len(), 48 + 2 * 5, "two files of offset and directory, no checksum");
}

/// §4.3: each distinct name once, in table order.
#[test]
fn line_strings_are_pooled() {
    let dirs: [&[u8]; 3] = [b"/a", b"x", b"/a/x"];
    let files = [
        FileEntry { name: b"x", dir: 0, md5: None },
        FileEntry { name: b"y", dir: 2, md5: None },
        FileEntry { name: b"x", dir: 2, md5: None },
    ];
    let (mut obj, _) = object(Arch::AArch64);
    let s = write_debug_line(&mut obj, &LineHeader { dirs: &dirs, files: &files }, &[]).unwrap();
    assert_eq!(obj.data(s.line_str), b"/a\0x\0/a/x\0y\0");
    let addends: Vec<i64> = obj.relocations(s.line).map(|r| r.addend).collect();
    assert_eq!(addends, [0, 3, 5, 3, 10, 3]);
}

/// §4.4: `llc`'s directory rules.
#[test]
fn file_table_rules() {
    let mut t = FileTable::new(b"/w", b"main.cpp", None);
    assert_eq!(t.add(b"/w", b"main.cpp", None), 0, "the primary file");
    assert_eq!(t.add(b"/w", b"./inc/util.h", None), 1);
    assert_eq!(t.add(b"/w", b"a.h", None), 2);
    assert_eq!(t.add(b"/other", b"sub/b.h", None), 3);
    assert_eq!(t.add(b"", b"/abs/c.h", None), 4);
    assert_eq!(t.add(b"/w", b"/x.h", None), 5);
    assert_eq!(t.add(b"/w", b"./inc/util.h", Some([9; 16])), 1, "first add wins");
    assert_eq!(t.add(b"/w", b"inc/", None), 6, "a trailing slash is not split");
    assert_eq!(t.dirs(), [&b"/w"[..], b"./inc", b"/other", b"/abs", b"/"]);
    let files: Vec<(&[u8], u32)> = t.files().iter().map(|f| (f.name, f.dir)).collect();
    assert_eq!(
        files,
        [
            (&b"main.cpp"[..], 0),
            (b"util.h", 1),
            (b"a.h", 0),
            (b"sub/b.h", 2),
            (b"c.h", 3),
            (b"x.h", 4),
            (b"inc/", 0)
        ]
    );
    assert!(t.files()[1].md5.is_none());
    let h = t.header();
    assert_eq!((h.dirs.len(), h.files.len()), (5, 7));
}

// ---------------------------------------------------------- program ----

/// §4.5: a sequence written as `llc` writes it: `set_file` before
/// `set_address` when the first row is in file 0, `negate_stmt`, the
/// flags, the end.
#[test]
fn sequence_bytes() {
    let (mut obj, text) = object(Arch::AArch64);
    let rows = [row(0, 0, 3, 0, STMT), row(20, 0, 4, 7, STMT | PE), row(24, 0, 4, 9, EB)];
    let seq = Sequence { order: 0, symbol: text, addend: 0x40, size: 36, rows: &rows };
    let s = write_debug_line(&mut obj, &header(), &[seq]).unwrap();
    let d = obj.data(s.line);
    let program = &d[d.len() - 29..];
    assert_eq!(
        program,
        [
            0x04, 0, // set_file 0
            0, 9, 2, 0, 0, 0, 0, 0, 0, 0, 0,    // set_address
            0x14, // line +2
            0x05, 7, 0x0a, 0x08, 0x3d, // column 7, prologue_end, +17, (+3, +1)
            0x05, 9, 0x06, 0x0b, 0x4a, // column 9, not a statement, epilogue_begin, (+4, 0)
            0x02, 12, 0, 1, 1, // advance 12, end_sequence
        ]
    );
    let last = obj.relocations(s.line).last().unwrap();
    assert_eq!((last.offset as usize, last.symbol, last.addend), (d.len() - 24, text, 0x40));
}

/// §4.5: the address relocation carries the first row's offset.
#[test]
fn first_row_after_the_start() {
    let (mut obj, text) = object(Arch::X86_64);
    let rows = [row(8, 1, 1, 0, STMT)];
    let seq = Sequence { order: 0, symbol: text, addend: 0x100, size: 9, rows: &rows };
    let s = write_debug_line(&mut obj, &header(), &[seq]).unwrap();
    let last = obj.relocations(s.line).last().unwrap();
    assert_eq!(last.addend, 0x108);
    assert_eq!(last.kind.elf_type(), 1, "R_X86_64_64");
    assert!(obj.data(s.line).ends_with(&[0x01, 0x02, 1, 0, 1, 1]), "copy, advance 1, end");
}

/// §4.5: a sequence without rows writes nothing.
#[test]
fn empty_sequence_writes_nothing() {
    let (mut a, text) = object(Arch::AArch64);
    let (mut b, _) = object(Arch::AArch64);
    let seq = Sequence { order: 0, symbol: text, addend: 0, size: 4, rows: &[] };
    let x = write_debug_line(&mut a, &header(), &[seq]).unwrap();
    let y = write_debug_line(&mut b, &header(), &[]).unwrap();
    assert_eq!(a.data(x.line), b.data(y.line));
}

/// §4.6: the output depends on the set of sequences only.
#[test]
fn order_of_input_does_not_matter() {
    let rows: Vec<Vec<Row>> = (0..6u32)
        .map(|k| (0..=k).map(|i| row(i * 8, i % 2, 10 * k + i * 3, i as u16, STMT)).collect())
        .collect();
    let seqs = || -> Vec<Sequence<'_>> {
        rows.iter()
            .enumerate()
            .map(|(k, r)| Sequence {
                order: 100 - 7 * k as u32,
                symbol: SymbolId::from_index(0),
                addend: 64 * k as i64,
                size: 64,
                rows: r,
            })
            .collect()
    };
    let reference = {
        let (mut obj, _) = object(Arch::AArch64);
        let input = seqs();
        let s = write_debug_line(&mut obj, &header(), &input).unwrap();
        (obj.data(s.line).to_vec(), obj.relocations(s.line).collect::<Vec<_>>())
    };
    let mut perm: Vec<usize> = (0..6).collect();
    for round in 0..40 {
        // Deterministic shuffles.
        perm.rotate_left(round % 5 + 1);
        perm.swap(round % 6, (round * 7 + 3) % 6);
        let base = seqs();
        let input: Vec<Sequence<'_>> = perm.iter().map(|&i| base[i]).collect();
        let (mut obj, _) = object(Arch::AArch64);
        let s = write_debug_line(&mut obj, &header(), &input).unwrap();
        assert_eq!(obj.data(s.line), reference.0, "round {round}");
        assert_eq!(obj.relocations(s.line).collect::<Vec<_>>(), reference.1);
    }
}

// ----------------------------------------------------------- errors ----

/// §4.6: every error, and the object unchanged.
#[test]
fn errors_leave_the_object_unchanged() {
    let good = [row(0, 0, 1, 0, STMT), row(4, 1, 2, 0, STMT)];
    let dirs_bad = [FileEntry { name: b"a", dir: 2, md5: None }];
    let nul = [FileEntry { name: b"a\0b", dir: 0, md5: None }];
    let bad_file = [row(0, 2, 1, 0, 0)];
    let backwards = [row(4, 0, 1, 0, 0), row(0, 0, 1, 0, 0)];
    let bad_flags = [row(0, 0, 1, 0, 8)];
    let s = |order, rows, size| Sequence {
        order,
        symbol: SymbolId::from_index(0),
        addend: 0,
        size,
        rows,
    };
    let cases: Vec<(LineHeader<'_>, Vec<Sequence<'_>>, Error)> = vec![
        (LineHeader { dirs: &[], files: &FILES }, vec![], Error::NoPrimary),
        (LineHeader { dirs: &DIRS, files: &[] }, vec![], Error::NoPrimary),
        (LineHeader { dirs: &DIRS, files: &dirs_bad }, vec![], Error::BadDirectory { file: 0 }),
        (LineHeader { dirs: &DIRS, files: &nul }, vec![], Error::NulInName),
        (LineHeader { dirs: &[b"/a\0"], files: &FILES[..1] }, vec![], Error::NulInName),
        (header(), vec![s(3, &bad_file, 4)], Error::BadFile { order: 3, row: 0 }),
        (header(), vec![s(1, &backwards, 8)], Error::RowOutOfOrder { order: 1, row: 1 }),
        (header(), vec![s(2, &good, 4)], Error::RowBeyondEnd { order: 2, row: 1 }),
        (header(), vec![s(4, &bad_flags, 4)], Error::BadFlags { order: 4, row: 0 }),
        (header(), vec![s(5, &good, 8), s(5, &good, 8)], Error::DuplicateOrder { order: 5 }),
    ];
    for (h, seqs, want) in cases {
        let (mut obj, _) = object(Arch::AArch64);
        let before = (obj.section_count(), obj.symbol_count());
        assert_eq!(write_debug_line(&mut obj, &h, &seqs), Err(want.clone()));
        assert_eq!((obj.section_count(), obj.symbol_count()), before, "{want:?}");
        assert!(!want.to_string().is_empty());
    }
}

// ----------------------------------------------------------- policy ----

#[allow(clippy::unnecessary_wraps)]
fn at(line: u32, column: u16) -> Option<SourceLoc> {
    Some(SourceLoc { file: 0, line, column })
}

fn policy(transitions: &[Transition], size: u32, epilogue: bool) -> Vec<Row> {
    let f = FunctionLines {
        scope: SourceLoc { file: 0, line: 3, column: 0 },
        size,
        transitions,
        epilogue,
    };
    let mut out = vec![row(99, 9, 9, 9, 0)]; // the caller's rows stay
    function_rows(&f, &mut out).unwrap();
    assert_eq!(out.remove(0), row(99, 9, 9, 9, 0));
    out
}

fn t(offset: u32, loc: Option<SourceLoc>) -> Transition {
    Transition { offset, loc }
}

/// §5.1 rules 1 and 2, §5.2: the start row, inherited entry code, the
/// prologue end, statements, the epilogue.
#[test]
fn policy_of_a_plain_function() {
    let ts = [t(4, None), t(12, at(4, 7)), t(16, at(4, 9)), t(20, at(5, 3)), t(28, at(5, 3))];
    assert_eq!(
        policy(&ts, 40, true),
        [
            row(0, 0, 3, 0, STMT),
            row(12, 0, 4, 7, STMT | PE),
            row(16, 0, 4, 9, 0),
            row(20, 0, 5, 3, STMT),
            row(28, 0, 5, 3, EB),
        ]
    );
}

/// §5.1 rule 1: no start row when the prologue is empty.
#[test]
fn policy_without_prologue() {
    assert_eq!(policy(&[t(0, at(1, 11))], 4, false), [row(0, 0, 1, 11, STMT | PE)]);
    // A line-0 first instruction does not end the prologue.
    assert_eq!(
        policy(&[t(0, at(0, 2)), t(4, at(7, 1))], 8, false),
        [row(0, 0, 3, 0, STMT), row(0, 0, 0, 2, 0), row(4, 0, 7, 1, STMT | PE)]
    );
}

/// §5.1 rule 3: line 0 is not a statement, not repeated, and a return
/// to the same line is not a new statement.
#[test]
fn policy_line_zero() {
    let ts = [t(8, at(6, 7)), t(12, at(0, 9)), t(16, at(0, 4)), t(20, at(6, 7)), t(24, at(7, 1))];
    assert_eq!(
        policy(&ts, 32, false),
        [
            row(0, 0, 3, 0, STMT),
            row(8, 0, 6, 7, STMT | PE),
            row(12, 0, 0, 9, 0),
            row(20, 0, 6, 7, 0),
            row(24, 0, 7, 1, STMT),
        ]
    );
}

/// §5.1 rules 2 and 4: no location inherits; a transition that changes
/// nothing (a scope change only) makes no row.
#[test]
fn policy_inherits_and_merges() {
    let ts = [t(8, at(6, 7)), t(12, None), t(16, at(6, 7)), t(20, at(6, 8))];
    assert_eq!(
        policy(&ts, 24, false),
        [row(0, 0, 3, 0, STMT), row(8, 0, 6, 7, STMT | PE), row(20, 0, 6, 8, 0)]
    );
}

/// §5.2: a change of file at the same line is a statement.
#[test]
fn policy_file_change_is_a_statement() {
    let other = Some(SourceLoc { file: 1, line: 6, column: 7 });
    let ts = [t(8, at(6, 7)), t(12, other), t(16, at(6, 7))];
    assert_eq!(
        policy(&ts, 24, false),
        [
            row(0, 0, 3, 0, STMT),
            row(8, 0, 6, 7, STMT | PE),
            row(12, 1, 6, 7, STMT),
            row(16, 0, 6, 7, STMT),
        ]
    );
}

/// §5.4: an epilogue without a location is a line-0 row; one at the end
/// of the code is dropped (§5.1 rule 5).
#[test]
fn policy_epilogue() {
    assert_eq!(
        policy(&[t(8, at(6, 7)), t(16, None)], 24, true),
        [row(0, 0, 3, 0, STMT), row(8, 0, 6, 7, STMT | PE), row(16, 0, 0, 7, EB)]
    );
    assert_eq!(
        policy(&[t(8, at(6, 7)), t(16, at(6, 7))], 16, true),
        [row(0, 0, 3, 0, STMT), row(8, 0, 6, 7, STMT | PE)]
    );
    // Not the epilogue: a trailing `None` inherits.
    assert_eq!(policy(&[t(8, at(6, 7)), t(16, None)], 24, false).len(), 2);
    assert!(policy(&[t(0, at(6, 7))], 0, true).is_empty(), "no code, no rows");
}

/// §5: transitions out of order are an error and `out` is unchanged.
#[test]
fn policy_error() {
    let f = FunctionLines {
        scope: SourceLoc { file: 0, line: 1, column: 0 },
        size: 16,
        transitions: &[t(8, at(2, 1)), t(4, at(3, 1))],
        epilogue: false,
    };
    let mut out = Vec::new();
    assert_eq!(function_rows(&f, &mut out), Err(Error::TransitionOutOfOrder { index: 1 }));
    assert!(out.is_empty());
}

// ------------------------------------------------------------ sweep ----

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// §8.1: hand-made tables decoded by `llvm-dwarfdump` of every installed
/// major as written, and `--verify` clean: 300 files in 40 directories,
/// sequences of up to 5000 rows, every kind of advance, line jumps of
/// ±2^20, three sections, one of them a COMDAT member.
#[test]
#[allow(clippy::too_many_lines)]
fn sweep_decodes_as_written() {
    let dumps = common::dwarfdumps();
    if dumps.is_empty() {
        let _ = hod_testutil::tool("llvm-dwarfdump");
        return;
    }
    let scratch = Scratch::new("dwarf-sweep");
    for arch in [Arch::AArch64, Arch::X86_64] {
        let mut r = Lcg(7);
        let names: Vec<Vec<u8>> = (0..300).map(|k| format!("file{k}.h").into_bytes()).collect();
        let dirs: Vec<Vec<u8>> = (0..40).map(|k| format!("/dir/{k}").into_bytes()).collect();
        let dir_refs: Vec<&[u8]> = dirs.iter().map(Vec::as_slice).collect();
        let files: Vec<FileEntry<'_>> = names
            .iter()
            .enumerate()
            .map(|(k, n)| FileEntry { name: n, dir: (k % 40) as u32, md5: Some([k as u8; 16]) })
            .collect();

        let mut obj = Object::new(arch);
        let mut sections = Vec::new();
        for (i, name) in [&b".text"[..], b".text.a", b".text.f"].iter().enumerate() {
            let s = obj.add_section(name, SectionKind::Text, 16).unwrap();
            obj.append_zeros(s, 1 << 22);
            if i == 2 {
                let n = obj.add_string(b"f").unwrap();
                let sig = obj.add_symbol(
                    Symbol::defined(n, SymbolKind::Func, s, 0, 1 << 22).with_binding(Binding::Weak),
                );
                let g = obj.add_group(sig, GroupKind::Comdat);
                obj.add_group_member(g, s).unwrap();
            }
            sections.push(obj.section_symbol(s));
        }
        let mut all_rows: Vec<Vec<Row>> = Vec::new();
        let mut layout = Vec::new();
        let mut cursor = [0i64; 3];
        for k in 0..24u32 {
            let count = if k == 5 { 5000 } else { 1 + r.below(400) as u32 };
            let mut rows = Vec::new();
            let mut offset = 0u32;
            let mut line = 1 + r.below(1000) as i64;
            for _ in 0..count {
                line += match r.below(12) {
                    0 => 1 << 20,
                    1 => -(1 << 19),
                    2 => -6,
                    3 => 9,
                    4 => -5,
                    5 => 8,
                    _ => r.below(4) as i64 - 1,
                };
                line = line.clamp(0, 1 << 28);
                rows.push(row(
                    offset,
                    r.below(300) as u32,
                    line as u32,
                    r.below(200) as u16,
                    [0, STMT, STMT | PE, EB][r.below(4) as usize],
                ));
                offset += match r.below(10) {
                    0 => 0,
                    1 => 17,
                    2 => 18 + r.below(16) as u32,
                    3 => 300 + r.below(5000) as u32,
                    _ => 1 + r.below(16) as u32,
                };
            }
            let size = offset + 1 + r.below(40) as u32;
            let section = (k % 3) as usize;
            layout.push((sections[section], cursor[section], size));
            cursor[section] += i64::from(size.next_multiple_of(16));
            assert!(cursor[section] <= 1 << 22, "the sweep fits its sections");
            all_rows.push(rows);
        }
        let seqs: Vec<Sequence<'_>> = all_rows
            .iter()
            .zip(&layout)
            .enumerate()
            .map(|(k, (rows, &(symbol, addend, size)))| Sequence {
                order: k as u32,
                symbol,
                addend,
                size,
                rows,
            })
            .collect();
        let header = LineHeader { dirs: &dir_refs, files: &files };
        write_debug_line(&mut obj, &header, &seqs).unwrap();
        let bytes = hod_obj::elf::to_vec(&obj).unwrap();
        for (major, d) in &dumps {
            let label = format!("sweep-{arch:?}-{major}.o");
            common::verify(d, &scratch, &label, &bytes);
            let dump = common::dump_lines(d, &scratch, &label, &bytes);
            assert_eq!(dump.dirs.len(), 40);
            assert_eq!(dump.files.len(), 300);
            assert_eq!(dump.files[123].name, "file123.h");
            assert_eq!((dump.files[123].dir, dump.files[123].md5), (3, Some([123; 16])));
            let mut decoded = dump.rows.iter();
            for (k, (rows, &(_, addend, size))) in all_rows.iter().zip(&layout).enumerate() {
                for want in rows {
                    let got = decoded.next().unwrap();
                    assert_eq!(
                        (got.address, got.file, got.line, got.column, got.flags, got.end),
                        (
                            addend as u64 + u64::from(want.offset),
                            want.file,
                            want.line,
                            want.column,
                            want.flags,
                            false
                        ),
                        "{label}: sequence {k}"
                    );
                }
                let end = decoded.next().unwrap();
                assert!(end.end, "{label}: end of sequence {k}");
                assert_eq!(end.address, addend as u64 + u64::from(size));
            }
            assert!(decoded.next().is_none());
        }
    }
}
