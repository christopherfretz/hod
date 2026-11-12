//! `spec/bc.md` §2: the bit reader and abbreviation decoding on
//! hand-built streams.

use super::{AbbrevOp, BitReader, Cursor, Entry, char6};
use crate::error::BcErrorKind;

/// Writes bits least significant first, as LLVM's `BitstreamWriter`.
#[derive(Default)]
pub(crate) struct BitWriter {
    pub bytes: Vec<u8>,
    bit: u64,
    /// Open blocks: the byte offset of the length word, and the width.
    blocks: Vec<(usize, u32)>,
    width: u32,
}

impl BitWriter {
    pub fn new() -> BitWriter {
        BitWriter { width: 2, ..BitWriter::default() }
    }

    pub fn emit(&mut self, value: u64, width: u32) {
        for bit in 0..width {
            let byte = (self.bit / 8) as usize;
            if byte == self.bytes.len() {
                self.bytes.push(0);
            }
            if value >> bit & 1 == 1 {
                self.bytes[byte] |= 1 << (self.bit % 8);
            }
            self.bit += 1;
        }
    }

    pub fn vbr(&mut self, mut value: u64, width: u32) {
        let high = 1u64 << (width - 1);
        while value >= high {
            self.emit(value & (high - 1) | high, width);
            value >>= width - 1;
        }
        self.emit(value, width);
    }

    pub fn align32(&mut self) {
        while self.bit % 32 != 0 {
            self.emit(0, 1);
        }
    }

    pub fn enter(&mut self, id: u32, width: u32) {
        self.emit(1, self.width);
        self.vbr(u64::from(id), 8);
        self.vbr(u64::from(width), 4);
        self.align32();
        let at = self.bytes.len();
        self.emit(0, 32);
        self.blocks.push((at, self.width));
        self.width = width;
    }

    pub fn end(&mut self) {
        self.emit(0, self.width);
        self.align32();
        let (at, width) = self.blocks.pop().expect("a block is open");
        let words = ((self.bytes.len() - at - 4) / 4) as u32;
        self.bytes[at..at + 4].copy_from_slice(&words.to_le_bytes());
        self.width = width;
    }

    pub fn unabbrev(&mut self, code: u32, ops: &[u64]) {
        self.emit(3, self.width);
        self.vbr(u64::from(code), 6);
        self.vbr(ops.len() as u64, 6);
        for &op in ops {
            self.vbr(op, 6);
        }
    }

    /// `DEFINE_ABBREV` with operands written as LLVM does: `(literal,
    /// value)` or `(encoding, width)`.
    pub fn define(&mut self, ops: &[AbbrevOp]) {
        self.emit(2, self.width);
        self.vbr(ops.len() as u64, 5);
        for op in ops {
            match *op {
                AbbrevOp::Literal(value) => {
                    self.emit(1, 1);
                    self.vbr(value, 8);
                }
                AbbrevOp::Fixed(width) => {
                    self.emit(0, 1);
                    self.emit(1, 3);
                    self.vbr(u64::from(width), 5);
                }
                AbbrevOp::Vbr(width) => {
                    self.emit(0, 1);
                    self.emit(2, 3);
                    self.vbr(u64::from(width), 5);
                }
                AbbrevOp::Array => {
                    self.emit(0, 1);
                    self.emit(3, 3);
                }
                AbbrevOp::Char6 => {
                    self.emit(0, 1);
                    self.emit(4, 3);
                }
                AbbrevOp::Blob => {
                    self.emit(0, 1);
                    self.emit(5, 3);
                }
            }
        }
    }

    pub fn abbrev_id(&mut self, id: u32) {
        self.emit(u64::from(id), self.width);
    }
}

fn char6_code(byte: u8) -> u64 {
    (0..64).find(|&value| char6(value) == byte).map(u64::from).expect("a char6 character")
}

#[test]
fn fixed_fields_of_every_width_and_alignment() {
    // §2.1: a field of 0 to 64 bits at every bit offset.
    for offset in 0..64u32 {
        for width in 0..=64u32 {
            let value =
                0xdead_beef_cafe_f00d_u64 & if width == 64 { u64::MAX } else { (1 << width) - 1 };
            let mut writer = BitWriter::new();
            writer.emit(0, offset);
            writer.emit(value, width);
            writer.emit(0b101, 3);
            let mut reader = BitReader::new(&writer.bytes);
            assert_eq!(reader.read(offset).unwrap(), 0);
            assert_eq!(reader.read(width).unwrap(), value, "width {width} at {offset}");
            assert_eq!(reader.read(3).unwrap(), 0b101);
        }
    }
}

#[test]
fn reads_past_the_end_are_errors() {
    // §2.1, §8: never a panic at the end of the data.
    let bytes = [0xffu8; 3];
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read(20).unwrap(), 0xf_ffff);
    assert!(reader.read(5).is_err());
    assert_eq!(reader.read(4).unwrap(), 0xf);
    assert!(reader.read(1).is_err());
    let mut reader = BitReader::new(&[]);
    assert!(reader.read(1).is_err());
    assert!(reader.vbr(6).is_err());
    assert!(reader.align32().is_ok());
}

#[test]
fn vbr_numbers() {
    // §2.1: chunks of 2 to 32 bits, values up to 64 bits.
    let values = [0u64, 1, 31, 32, 33, 1000, u64::from(u32::MAX), 1 << 40, u64::MAX - 1, u64::MAX];
    for width in 2..=32u32 {
        for &value in &values {
            let mut writer = BitWriter::new();
            writer.emit(1, 1);
            writer.vbr(value, width);
            let mut reader = BitReader::new(&writer.bytes);
            reader.read(1).unwrap();
            assert_eq!(reader.vbr(width).unwrap(), value, "{value} in chunks of {width}");
        }
    }
}

#[test]
fn vbr_over_64_bits_is_an_error() {
    // 65 significant bits in chunks of 6.
    let mut writer = BitWriter::new();
    for _ in 0..13 {
        writer.emit(0b11_1111, 6);
    }
    writer.emit(0b00_0001, 6);
    let mut reader = BitReader::new(&writer.bytes);
    let error = reader.vbr(6).unwrap_err();
    assert_eq!(error.kind, BcErrorKind::Malformed);
    // Zero chunks past 64 bits do not change the value, up to a bound.
    let mut writer = BitWriter::new();
    writer.emit(0b10_0001, 6);
    for _ in 0..12 {
        writer.emit(0b10_0000, 6);
    }
    writer.emit(0, 6);
    let mut reader = BitReader::new(&writer.bytes);
    assert_eq!(reader.vbr(6).unwrap(), 1);
    // A number that never ends is an error, not a loop.
    let bytes = vec![0xffu8; 4096];
    let mut reader = BitReader::new(&bytes);
    assert!(reader.vbr(2).is_err());
}

#[test]
fn char6_is_lowercase_uppercase_digits_dot_underscore() {
    let all: Vec<u8> = (0..64).map(char6).collect();
    assert_eq!(all, b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._");
}

#[test]
fn blocks_records_and_abbreviations() {
    // §2.2, §2.3: a block with every operand encoding, nested blocks,
    // skipping by length.
    let mut writer = BitWriter::new();
    writer.enter(8, 3);
    writer.unabbrev(1, &[2, 1000]);
    writer.define(&[
        AbbrevOp::Literal(7),
        AbbrevOp::Fixed(3),
        AbbrevOp::Vbr(4),
        AbbrevOp::Array,
        AbbrevOp::Char6,
    ]);
    writer.define(&[AbbrevOp::Vbr(6), AbbrevOp::Blob]);
    writer.abbrev_id(4);
    writer.emit(5, 3);
    writer.vbr(300, 4);
    writer.vbr(3, 6);
    for byte in b"a.Z" {
        writer.emit(char6_code(*byte), 6);
    }
    writer.abbrev_id(5);
    writer.vbr(9, 6);
    writer.vbr(5, 6);
    writer.align32();
    for byte in b"hello" {
        writer.emit(u64::from(*byte), 8);
    }
    writer.align32();
    writer.enter(17, 4);
    writer.unabbrev(3, &[]);
    writer.end();
    writer.enter(12, 5);
    writer.unabbrev(4, &[1, 2, 3]);
    writer.end();
    writer.unabbrev(2, &[u64::MAX]);
    writer.end();

    let mut cursor = Cursor::new(&writer.bytes);
    let mut ops = Vec::new();
    assert_eq!(cursor.next().unwrap(), Entry::Block(8));
    cursor.enter().unwrap();
    let Entry::Record(id) = cursor.next().unwrap() else { panic!("a record") };
    assert_eq!(cursor.record(id, &mut ops).unwrap(), (1, None));
    assert_eq!(ops, [2, 1000]);
    let Entry::Record(id) = cursor.next().unwrap() else { panic!("a record") };
    assert_eq!(id, 4);
    assert_eq!(cursor.record(id, &mut ops).unwrap(), (7, None));
    assert_eq!(ops, [5, 300, u64::from(b'a'), u64::from(b'.'), u64::from(b'Z')]);
    let Entry::Record(id) = cursor.next().unwrap() else { panic!("a record") };
    let (code, blob) = cursor.record(id, &mut ops).unwrap();
    assert_eq!((code, blob), (9, Some(&b"hello"[..])));
    assert!(ops.is_empty());
    assert_eq!(cursor.next().unwrap(), Entry::Block(17));
    cursor.enter().unwrap();
    let Entry::Record(id) = cursor.next().unwrap() else { panic!("a record") };
    assert_eq!(cursor.record(id, &mut ops).unwrap().0, 3);
    assert_eq!(cursor.next().unwrap(), Entry::End);
    assert_eq!(cursor.next().unwrap(), Entry::Block(12));
    cursor.skip();
    let Entry::Record(id) = cursor.next().unwrap() else { panic!("a record") };
    assert_eq!(cursor.record(id, &mut ops).unwrap().0, 2);
    assert_eq!(ops, [u64::MAX]);
    assert_eq!(cursor.next().unwrap(), Entry::End);
    assert!(cursor.at_top_end());
}

#[test]
fn local_abbreviations_end_with_their_block() {
    // §2.3: an abbreviation defined in a block is gone after it.
    let mut writer = BitWriter::new();
    writer.enter(8, 3);
    writer.enter(9, 3);
    writer.define(&[AbbrevOp::Literal(1)]);
    writer.abbrev_id(4);
    writer.end();
    writer.abbrev_id(4);
    writer.end();
    let mut cursor = Cursor::new(&writer.bytes);
    let mut ops = Vec::new();
    assert_eq!(cursor.next().unwrap(), Entry::Block(8));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Block(9));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Record(4));
    assert_eq!(cursor.record(4, &mut ops).unwrap().0, 1);
    assert_eq!(cursor.next().unwrap(), Entry::End);
    assert_eq!(cursor.next().unwrap(), Entry::Record(4));
    assert_eq!(cursor.record(4, &mut ops).unwrap_err().kind, BcErrorKind::Malformed);
}

#[test]
fn block_info_abbreviations_come_first() {
    // §2.4: BLOCKINFO abbreviations of a block id take the first ids of
    // every block of that id, before its own.
    let mut writer = BitWriter::new();
    writer.enter(8, 3);
    writer.enter(0, 2);
    writer.unabbrev(1, &[11]);
    writer.define(&[AbbrevOp::Literal(40), AbbrevOp::Fixed(8)]);
    writer.unabbrev(2, &[u64::from(b'x')]);
    writer.unabbrev(1, &[12]);
    writer.define(&[AbbrevOp::Literal(50)]);
    writer.end();
    writer.enter(11, 4);
    writer.define(&[AbbrevOp::Literal(41)]);
    writer.abbrev_id(5);
    writer.abbrev_id(4);
    writer.emit(200, 8);
    writer.end();
    writer.enter(12, 4);
    writer.abbrev_id(4);
    writer.end();
    writer.end();
    let mut cursor = Cursor::new(&writer.bytes);
    let mut ops = Vec::new();
    assert_eq!(cursor.next().unwrap(), Entry::Block(8));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Block(0));
    cursor.read_block_info().unwrap();
    assert_eq!(cursor.block_info().abbrev_count(11), 1);
    assert_eq!(cursor.next().unwrap(), Entry::Block(11));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Record(5));
    assert_eq!(cursor.record(5, &mut ops).unwrap().0, 41);
    assert_eq!(cursor.next().unwrap(), Entry::Record(4));
    assert_eq!(cursor.record(4, &mut ops).unwrap().0, 40);
    assert_eq!(ops, [200]);
    assert_eq!(cursor.next().unwrap(), Entry::End);
    assert_eq!(cursor.next().unwrap(), Entry::Block(12));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Record(4));
    assert_eq!(cursor.record(4, &mut ops).unwrap().0, 50);
}

#[test]
fn malformed_abbreviations_are_refused() {
    // §2.3: the shapes LLVM refuses.
    let shapes: [&[AbbrevOp]; 5] = [
        &[AbbrevOp::Array, AbbrevOp::Fixed(8)],
        &[AbbrevOp::Literal(1), AbbrevOp::Array],
        &[AbbrevOp::Literal(1), AbbrevOp::Array, AbbrevOp::Array],
        &[AbbrevOp::Literal(1), AbbrevOp::Blob, AbbrevOp::Fixed(1)],
        &[AbbrevOp::Literal(1), AbbrevOp::Array, AbbrevOp::Fixed(8), AbbrevOp::Fixed(1)],
    ];
    for shape in shapes {
        let mut writer = BitWriter::new();
        writer.enter(8, 3);
        writer.define(shape);
        writer.end();
        let mut cursor = Cursor::new(&writer.bytes);
        assert_eq!(cursor.next().unwrap(), Entry::Block(8));
        cursor.enter().unwrap();
        assert_eq!(cursor.next().unwrap_err().kind, BcErrorKind::Malformed, "{shape:?}");
    }
    // An unknown encoding (6), a fixed field of 65 bits.
    for (encoding, width) in [(6u64, None), (1, Some(65u64)), (2, Some(33))] {
        let mut writer = BitWriter::new();
        writer.enter(8, 3);
        writer.emit(2, 3);
        writer.vbr(2, 5);
        writer.emit(1, 1);
        writer.vbr(1, 8);
        writer.emit(0, 1);
        writer.emit(encoding, 3);
        if let Some(width) = width {
            writer.vbr(width, 5);
        }
        writer.end();
        let mut cursor = Cursor::new(&writer.bytes);
        assert_eq!(cursor.next().unwrap(), Entry::Block(8));
        cursor.enter().unwrap();
        assert_eq!(cursor.next().unwrap_err().kind, BcErrorKind::Malformed);
    }
}

#[test]
fn zero_width_fields_are_literal_zeros() {
    // §2.3: LLVM reads Fixed(0) and VBR(0) as the literal 0.
    let mut writer = BitWriter::new();
    writer.enter(8, 3);
    writer.emit(2, 3);
    writer.vbr(3, 5);
    writer.emit(1, 1);
    writer.vbr(6, 8);
    writer.emit(0, 1);
    writer.emit(1, 3);
    writer.vbr(0, 5);
    writer.emit(0, 1);
    writer.emit(2, 3);
    writer.vbr(0, 5);
    writer.abbrev_id(4);
    writer.end();
    let mut cursor = Cursor::new(&writer.bytes);
    let mut ops = Vec::new();
    assert_eq!(cursor.next().unwrap(), Entry::Block(8));
    cursor.enter().unwrap();
    assert_eq!(cursor.next().unwrap(), Entry::Record(4));
    assert_eq!(cursor.record(4, &mut ops).unwrap().0, 6);
    assert_eq!(ops, [0, 0]);
}

#[test]
fn counts_are_checked_before_anything_is_allocated() {
    // §8: an operand count, an array length or a blob length larger than
    // the bits left is an error, whatever its size.
    for count in [1000u64, 1 << 40, u64::MAX >> 1] {
        let mut writer = BitWriter::new();
        writer.enter(8, 3);
        writer.emit(3, 3);
        writer.vbr(1, 6);
        writer.vbr(count, 6);
        writer.end();
        let mut cursor = Cursor::new(&writer.bytes);
        let mut ops = Vec::new();
        cursor.next().unwrap();
        cursor.enter().unwrap();
        assert_eq!(cursor.next().unwrap(), Entry::Record(3));
        assert_eq!(cursor.record(3, &mut ops).unwrap_err().kind, BcErrorKind::Malformed);
        for shape in [
            [AbbrevOp::Literal(1), AbbrevOp::Array, AbbrevOp::Fixed(8)].as_slice(),
            &[AbbrevOp::Literal(1), AbbrevOp::Blob],
        ] {
            let mut writer = BitWriter::new();
            writer.enter(8, 3);
            writer.define(shape);
            writer.abbrev_id(4);
            writer.vbr(count, 6);
            writer.end();
            let mut cursor = Cursor::new(&writer.bytes);
            cursor.next().unwrap();
            cursor.enter().unwrap();
            assert_eq!(cursor.next().unwrap(), Entry::Record(4));
            assert_eq!(cursor.record(4, &mut ops).unwrap_err().kind, BcErrorKind::Malformed);
        }
    }
}

#[test]
fn a_block_cannot_read_past_its_length() {
    // §2.2: the declared length bounds the block.
    let mut writer = BitWriter::new();
    writer.enter(8, 3);
    writer.unabbrev(1, &[1, 2, 3]);
    writer.end();
    writer.enter(9, 3);
    writer.end();
    // Shorten the first block's length to one word.
    writer.bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    let mut cursor = Cursor::new(&writer.bytes);
    let mut ops = Vec::new();
    cursor.next().unwrap();
    cursor.enter().unwrap();
    let outcome = cursor.next().and_then(|entry| match entry {
        Entry::Record(id) => cursor.record(id, &mut ops).map(|_| ()),
        _ => Ok(()),
    });
    let outcome = outcome.and_then(|()| cursor.next().map(|_| ()));
    assert_eq!(outcome.unwrap_err().kind, BcErrorKind::Malformed);
}

#[test]
fn deep_nesting_is_a_limit_not_a_stack_overflow() {
    // §8: blocks nested 100,000 deep.
    let mut writer = BitWriter::new();
    for _ in 0..100_000 {
        writer.enter(8, 2);
    }
    for _ in 0..100_000 {
        writer.end();
    }
    let mut cursor = Cursor::new(&writer.bytes);
    let error = loop {
        match cursor.next() {
            Ok(Entry::Block(_)) => {
                if let Err(error) = cursor.enter() {
                    break error;
                }
            }
            Ok(other) => panic!("{other:?}"),
            Err(error) => break error,
        }
    };
    assert_eq!(error.kind, BcErrorKind::Limit);
}
