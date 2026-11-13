//! The bitstream container (`spec/bc.md` §2): bits, abbreviations,
//! BLOCKINFO, blocks and records.
//!
//! Everything here is generic over what the blocks mean. The cursor keeps
//! its own stack of open blocks, so input nesting never becomes Rust
//! recursion; every read is checked against the end of the innermost open
//! block, so a block cannot read past its declared length; and nothing is
//! allocated in proportion to a number read from the input before that
//! number has been checked against the bits that are left.

use crate::error::{BcErrorKind, BcResult, fail};
use crate::limits::{MAX_ABBREV_OPS, MAX_BLOCK_DEPTH};

/// The abbreviation id that ends a block.
pub const END_BLOCK: u32 = 0;
/// The abbreviation id that opens a block.
pub const ENTER_SUBBLOCK: u32 = 1;
/// The abbreviation id that defines an abbreviation.
pub const DEFINE_ABBREV: u32 = 2;
/// The abbreviation id of a record written without an abbreviation.
pub const UNABBREV_RECORD: u32 = 3;
/// The first abbreviation id a definition gets.
pub const FIRST_ABBREV: u32 = 4;
/// The block id of BLOCKINFO.
pub const BLOCKINFO_BLOCK: u32 = 0;

/// Reads bits, least significant first, from a byte slice.
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: u64,
    end: u64,
}

impl<'a> BitReader<'a> {
    /// A reader over all of `data`, at bit 0.
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, pos: 0, end: data.len() as u64 * 8 }
    }

    /// The bytes read from.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The current bit position.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// The position no read may pass.
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Bits left before [`Self::end`].
    pub fn left(&self) -> u64 {
        self.end - self.pos
    }

    /// Moves to bit `pos` and bounds reads by `end`.
    ///
    /// # Errors
    /// `Malformed` unless `pos <= end <= ` the bits of the data.
    pub fn seek(&mut self, pos: u64, end: u64) -> BcResult<()> {
        if pos > end || end > self.data.len() as u64 * 8 {
            return fail(BcErrorKind::Malformed, self.pos, "a position past the end of the data");
        }
        self.pos = pos;
        self.end = end;
        Ok(())
    }

    /// Eight bytes from byte `at`, little-endian, zeros past the end.
    #[inline]
    fn word_at(&self, at: usize) -> u64 {
        if let Some(bytes) = self.data.get(at..at + 8) {
            let mut word = [0u8; 8];
            word.copy_from_slice(bytes);
            return u64::from_le_bytes(word);
        }
        let mut word = [0u8; 8];
        if at < self.data.len() {
            let tail = &self.data[at..];
            word[..tail.len()].copy_from_slice(tail);
        }
        u64::from_le_bytes(word)
    }

    /// The next `width` bits, `width <= 64`, as a number.
    ///
    /// # Errors
    /// `Malformed` if fewer than `width` bits are left.
    #[inline]
    pub fn read(&mut self, width: u32) -> BcResult<u64> {
        debug_assert!(width <= 64, "a field is at most 64 bits");
        if u64::from(width) > self.end - self.pos {
            return self.truncated();
        }
        if width == 0 {
            return Ok(0);
        }
        let byte = (self.pos >> 3) as usize;
        let shift = (self.pos & 7) as u32;
        let mut value = self.word_at(byte) >> shift;
        if shift + width > 64 {
            let next = u64::from(self.data[byte + 8]);
            value |= next << (64 - shift);
        }
        self.pos += u64::from(width);
        Ok(if width == 64 { value } else { value & ((1u64 << width) - 1) })
    }

    /// A variable-width number in chunks of `width` bits, `2 <= width <=
    /// 32`: each chunk's top bit says whether another follows.
    ///
    /// # Errors
    /// `Malformed` at the end of the bits, or for a number of more than
    /// 64 bits.
    #[inline]
    pub fn vbr(&mut self, width: u32) -> BcResult<u64> {
        debug_assert!((2..=32).contains(&width), "a VBR chunk is 2 to 32 bits");
        // The common case, inline: at least 64 bits are left, so the
        // word at the current byte is all in the data and holds at least
        // 57 of them; a number whose chunks all lie there is decoded from
        // it. Its payload is under 57 bits, so none of the errors of
        // `vbr_long` can arise; anything else starts over there.
        if self.end - self.pos >= 64 {
            let shift = (self.pos & 7) as u32;
            let word = self.word_at((self.pos >> 3) as usize) >> shift;
            let mask = (1u64 << width) - 1;
            let high = 1u64 << (width - 1);
            let mut chunk = word & mask;
            if chunk & high == 0 {
                self.pos += u64::from(width);
                return Ok(chunk);
            }
            let avail = 64 - shift;
            let mut value = chunk & (high - 1);
            let mut used = width;
            let mut out = width - 1;
            while used + width <= avail {
                chunk = (word >> used) & mask;
                value |= (chunk & (high - 1)) << out;
                used += width;
                out += width - 1;
                if chunk & high == 0 {
                    self.pos += u64::from(used);
                    return Ok(value);
                }
            }
        }
        self.vbr_long(width)
    }

    /// [`Self::vbr`] chunk by chunk: near the end of the bits, or for a
    /// number longer than a word.
    #[inline(never)]
    fn vbr_long(&mut self, width: u32) -> BcResult<u64> {
        let high = 1u64 << (width - 1);
        let mut chunk = self.read(width)?;
        if chunk & high == 0 {
            return Ok(chunk);
        }
        let at = self.pos - u64::from(width);
        let payload_bits = width - 1;
        let mut value = chunk & (high - 1);
        let mut shift = payload_bits;
        loop {
            chunk = self.read(width)?;
            let payload = chunk & (high - 1);
            if shift < 64 {
                if shift + payload_bits > 64 && payload >> (64 - shift) != 0 {
                    return fail(
                        BcErrorKind::Malformed,
                        at,
                        "a variable-width number over 64 bits",
                    );
                }
                value |= payload << shift;
            } else if payload != 0 {
                return fail(BcErrorKind::Malformed, at, "a variable-width number over 64 bits");
            }
            if chunk & high == 0 {
                return Ok(value);
            }
            shift += payload_bits;
            if shift > 128 {
                return fail(
                    BcErrorKind::Malformed,
                    at,
                    "a variable-width number that does not end",
                );
            }
        }
    }

    /// Moves to the next multiple of 32 bits.
    ///
    /// # Errors
    /// `Malformed` if that is past the end.
    pub fn align32(&mut self) -> BcResult<()> {
        let aligned = (self.pos + 31) & !31;
        if aligned > self.end {
            return self.truncated();
        }
        self.pos = aligned;
        Ok(())
    }

    /// The next `len` bytes, at a byte boundary.
    ///
    /// # Errors
    /// `Malformed` if fewer are left.
    pub fn bytes(&mut self, len: u64) -> BcResult<&'a [u8]> {
        debug_assert!(self.pos.is_multiple_of(8), "bytes are read at a byte boundary");
        if len > (self.end - self.pos) / 8 {
            return self.truncated();
        }
        let start = (self.pos / 8) as usize;
        self.pos += len * 8;
        Ok(&self.data[start..start + len as usize])
    }

    #[cold]
    fn truncated<T>(&self) -> BcResult<T> {
        fail(BcErrorKind::Malformed, self.pos, "the data ends inside a field, record or block")
    }
}

/// One operand of an abbreviation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbbrevOp {
    /// A value that is not written.
    Literal(u64),
    /// A field of this many bits, 1 to 64.
    Fixed(u8),
    /// A variable-width field in chunks of this many bits, 2 to 32.
    Vbr(u8),
    /// A count (VBR6), then that many elements encoded as the next
    /// operand, which is the last.
    Array,
    /// Six bits: `[a-zA-Z0-9._]`.
    Char6,
    /// A count (VBR6), then that many bytes from the next 32-bit
    /// boundary, then padding to the next one. The last operand.
    Blob,
}

/// The abbreviations a block id gets from BLOCKINFO, and the operands of
/// every abbreviation of a stream, in one arena.
#[derive(Clone, Debug, Default)]
pub struct BlockInfo {
    ops: Vec<AbbrevOp>,
    /// Per block id with entries: its abbreviations, as ranges of `ops`.
    blocks: Vec<(u32, Vec<(u32, u32)>)>,
}

impl BlockInfo {
    /// The abbreviations BLOCKINFO gives block `id`.
    fn of(&self, id: u32) -> &[(u32, u32)] {
        self.blocks.iter().find(|(block, _)| *block == id).map_or(&[], |(_, list)| list)
    }

    fn add(&mut self, id: u32, abbrev: (u32, u32)) {
        if let Some((_, list)) = self.blocks.iter_mut().find(|(block, _)| *block == id) {
            list.push(abbrev);
        } else {
            self.blocks.push((id, vec![abbrev]));
        }
    }

    /// The number of abbreviations BLOCKINFO gives block `id`.
    pub fn abbrev_count(&self, id: u32) -> usize {
        self.of(id).len()
    }
}

/// What the cursor met.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// A record, written with this abbreviation id; read it with
    /// [`Cursor::record`].
    Record(u32),
    /// The header of block `id`; enter it ([`Cursor::enter`]) or step
    /// over it ([`Cursor::skip`]) next.
    Block(u32),
    /// The end of the innermost open block, which is closed.
    End,
}

/// A block the cursor is inside.
#[derive(Clone, Copy, Debug)]
struct Open {
    id: u32,
    width: u32,
    /// Where its local abbreviations start in `Cursor::local`.
    local: usize,
    /// Its end, and the end of the block around it.
    end: u64,
    outer_end: u64,
}

/// A block header just read and not yet entered or skipped.
#[derive(Clone, Copy, Debug)]
struct Pending {
    id: u32,
    width: u32,
    start: u64,
    end: u64,
}

/// Where a block is: what [`Cursor::enter_at`] needs to read it again
/// later, from any thread.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockSpan {
    /// The block id.
    pub id: u32,
    /// Its abbreviation width.
    pub width: u32,
    /// The bit after its header: its first abbreviation id.
    pub start: u64,
    /// The bit after its last word.
    pub end: u64,
}

/// Walks the blocks and records of a bitstream.
#[derive(Debug)]
pub struct Cursor<'a, 'b> {
    bits: BitReader<'a>,
    info: InfoRef<'b>,
    stack: Vec<Open>,
    /// The local abbreviations of the open blocks, as ranges of `ops`.
    local: Vec<(u32, u32)>,
    /// Operands of local abbreviations.
    ops: Vec<AbbrevOp>,
    pending: Option<Pending>,
    /// The position of the current entry's abbreviation id.
    at: u64,
}

#[derive(Debug)]
enum InfoRef<'b> {
    Owned(BlockInfo),
    Shared(&'b BlockInfo),
}

impl InfoRef<'_> {
    fn get(&self) -> &BlockInfo {
        match self {
            InfoRef::Owned(info) => info,
            InfoRef::Shared(info) => info,
        }
    }
}

impl<'a, 'b> Cursor<'a, 'b> {
    /// A cursor at the top level of `data` (abbreviation width 2), which
    /// collects the BLOCKINFO it reads.
    pub fn new(data: &'a [u8]) -> Cursor<'a, 'static> {
        Cursor {
            bits: BitReader::new(data),
            info: InfoRef::Owned(BlockInfo::default()),
            stack: Vec::new(),
            local: Vec::new(),
            ops: Vec::new(),
            pending: None,
            at: 0,
        }
    }

    /// A cursor at the top level of the bits `start..end` of `data`: the
    /// stream inside a wrapper header, with positions still counted from
    /// the first byte of `data`.
    ///
    /// # Errors
    /// `Malformed` if the range is not inside the data.
    pub fn new_in(data: &'a [u8], start: u64, end: u64) -> BcResult<Cursor<'a, 'static>> {
        let mut cursor = Cursor::new(data);
        cursor.bits.seek(start, end)?;
        cursor.at = start;
        Ok(cursor)
    }

    /// A cursor inside the block `span` of `data`, with the abbreviations
    /// of `info`: for reading a block found earlier, such as a function
    /// body, on its own. The cursor ends at the block's end.
    ///
    /// # Errors
    /// `Malformed` if the span is not inside the data.
    pub fn enter_at(
        data: &'a [u8],
        info: &'b BlockInfo,
        span: BlockSpan,
    ) -> BcResult<Cursor<'a, 'b>> {
        let mut bits = BitReader::new(data);
        bits.seek(span.start, span.end)?;
        let open =
            Open { id: span.id, width: span.width, local: 0, end: span.end, outer_end: span.end };
        Ok(Cursor {
            bits,
            info: InfoRef::Shared(info),
            stack: vec![open],
            local: Vec::new(),
            ops: Vec::new(),
            pending: None,
            at: span.start,
        })
    }

    /// The BLOCKINFO read so far.
    pub fn block_info(&self) -> &BlockInfo {
        self.info.get()
    }

    /// The BLOCKINFO read, for later cursors.
    pub fn into_block_info(self) -> BlockInfo {
        match self.info {
            InfoRef::Owned(info) => info,
            InfoRef::Shared(info) => info.clone(),
        }
    }

    /// The bit reader, for positions.
    pub fn bits(&self) -> &BitReader<'a> {
        &self.bits
    }

    /// The position of the abbreviation id of the entry last returned.
    pub fn entry_pos(&self) -> u64 {
        self.at
    }

    /// The number of open blocks.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The id of the innermost open block.
    pub fn block_id(&self) -> Option<u32> {
        self.stack.last().map(|open| open.id)
    }

    fn width(&self) -> u32 {
        self.stack.last().map_or(2, |open| open.width)
    }

    /// Whether the top level has nothing left: no bits, or fewer than a
    /// word (LLVM pads the end of a file to 32 bits).
    pub fn at_top_end(&self) -> bool {
        self.stack.is_empty() && self.bits.left() < 32
    }

    /// The next entry. Abbreviation definitions are taken in on the way.
    ///
    /// # Errors
    /// `Malformed` for a bitstream that is not well formed; `Limit` past
    /// [`MAX_BLOCK_DEPTH`] or [`MAX_ABBREV_OPS`].
    #[allow(clippy::should_implement_trait)] // Fallible: not an iterator.
    pub fn next(&mut self) -> BcResult<Entry> {
        if self.pending.is_some() {
            return fail(BcErrorKind::Malformed, self.at, "a block header that was not consumed");
        }
        loop {
            self.at = self.bits.pos();
            let id = self.bits.read(self.width())? as u32;
            match id {
                END_BLOCK => {
                    let Some(open) = self.stack.pop() else {
                        return fail(BcErrorKind::Malformed, self.at, "END_BLOCK at the top level");
                    };
                    self.bits.align32()?;
                    let pos = self.bits.pos();
                    self.local.truncate(open.local);
                    // The block's length bounds it; LLVM's writers end
                    // each block exactly at its length.
                    self.bits.seek(pos, open.outer_end)?;
                    return Ok(Entry::End);
                }
                ENTER_SUBBLOCK => {
                    let block = self.bits.vbr(8)?;
                    let width = self.bits.vbr(4)?;
                    self.bits.align32()?;
                    let words = self.bits.read(32)?;
                    let start = self.bits.pos();
                    let Ok(block) = u32::try_from(block) else {
                        return fail(BcErrorKind::Malformed, self.at, "a block id over 32 bits");
                    };
                    if !(1..=32).contains(&width) {
                        return fail(
                            BcErrorKind::Malformed,
                            self.at,
                            "an abbreviation width of 0 or over 32",
                        );
                    }
                    let end = start + words * 32;
                    if end > self.bits.end() {
                        return fail(
                            BcErrorKind::Malformed,
                            self.at,
                            "a block longer than what holds it",
                        );
                    }
                    self.pending = Some(Pending { id: block, width: width as u32, start, end });
                    return Ok(Entry::Block(block));
                }
                DEFINE_ABBREV => {
                    let range = self.define_abbrev(false)?;
                    self.local.push(range);
                }
                _ => {
                    if self.stack.is_empty() {
                        return fail(BcErrorKind::Malformed, self.at, "a record at the top level");
                    }
                    return Ok(Entry::Record(id));
                }
            }
        }
    }

    /// The block whose header [`Self::next`] just returned, as a span.
    ///
    /// # Panics
    /// If no header is pending.
    pub fn pending_span(&self) -> BlockSpan {
        let pending = self.pending.expect("a block header was just read");
        BlockSpan { id: pending.id, width: pending.width, start: pending.start, end: pending.end }
    }

    /// Enters the block whose header [`Self::next`] just returned.
    ///
    /// # Errors
    /// `Limit` past [`MAX_BLOCK_DEPTH`] open blocks.
    ///
    /// # Panics
    /// If no header is pending.
    pub fn enter(&mut self) -> BcResult<()> {
        let pending = self.pending.take().expect("a block header was just read");
        if self.stack.len() >= MAX_BLOCK_DEPTH {
            return fail(BcErrorKind::Limit, self.at, "blocks nested deeper than MAX_BLOCK_DEPTH");
        }
        let outer_end = self.bits.end();
        self.stack.push(Open {
            id: pending.id,
            width: pending.width,
            local: self.local.len(),
            end: pending.end,
            outer_end,
        });
        self.bits.seek(pending.start, pending.end)
    }

    /// Steps over the block whose header [`Self::next`] just returned, by
    /// its length.
    ///
    /// # Panics
    /// If no header is pending.
    pub fn skip(&mut self) {
        let pending = self.pending.take().expect("a block header was just read");
        let end = self.bits.end();
        // Cannot fail: `next` checked that the block ends inside.
        let _ = self.bits.seek(pending.end, end);
    }

    /// Steps over the rest of the innermost open block, closing it.
    ///
    /// # Errors
    /// As [`Self::next`] for the bits up to its end.
    pub fn skip_rest(&mut self) -> BcResult<()> {
        let Some(open) = self.stack.pop() else {
            return fail(BcErrorKind::Malformed, self.at, "no block to step out of");
        };
        self.local.truncate(open.local);
        self.bits.seek(open.end, open.outer_end)
    }

    /// Reads a BLOCKINFO block whose header [`Self::next`] just returned,
    /// adding its abbreviations for later blocks.
    ///
    /// # Errors
    /// As [`Self::next`]; `Malformed` for an abbreviation before `SETBID`
    /// or a record other than those of BLOCKINFO with a bad operand.
    pub fn read_block_info(&mut self) -> BcResult<()> {
        self.enter()?;
        let mut current: Option<u32> = None;
        let mut ops = Vec::new();
        loop {
            self.at = self.bits.pos();
            let id = self.bits.read(self.width())? as u32;
            match id {
                END_BLOCK => {
                    self.pending = None;
                    let open = self.stack.pop().expect("BLOCKINFO is open");
                    self.bits.align32()?;
                    let pos = self.bits.pos();
                    self.local.truncate(open.local);
                    return self.bits.seek(pos, open.outer_end);
                }
                ENTER_SUBBLOCK => {
                    // A block inside BLOCKINFO means nothing: step over it.
                    self.bits.vbr(8)?;
                    self.bits.vbr(4)?;
                    self.bits.align32()?;
                    let words = self.bits.read(32)?;
                    let end = self.bits.pos() + words * 32;
                    if end > self.bits.end() {
                        return fail(
                            BcErrorKind::Malformed,
                            self.at,
                            "a block longer than what holds it",
                        );
                    }
                    let limit = self.bits.end();
                    self.bits.seek(end, limit)?;
                }
                DEFINE_ABBREV => {
                    let Some(block) = current else {
                        return fail(
                            BcErrorKind::Malformed,
                            self.at,
                            "an abbreviation in BLOCKINFO before SETBID",
                        );
                    };
                    let range = self.define_abbrev(true)?;
                    let InfoRef::Owned(info) = &mut self.info else {
                        return fail(
                            BcErrorKind::Malformed,
                            self.at,
                            "BLOCKINFO inside a block read on its own",
                        );
                    };
                    info.add(block, range);
                }
                _ => {
                    let code = self.record(id, &mut ops)?.0;
                    // SETBID; BLOCKNAME and SETRECORDNAME are names only.
                    if code == 1 {
                        let Some(&block) = ops.first() else {
                            return fail(
                                BcErrorKind::Malformed,
                                self.at,
                                "SETBID without a block id",
                            );
                        };
                        let Ok(block) = u32::try_from(block) else {
                            return fail(
                                BcErrorKind::Malformed,
                                self.at,
                                "a block id over 32 bits",
                            );
                        };
                        current = Some(block);
                    }
                }
            }
        }
    }

    /// Reads an abbreviation definition into the right arena; its range.
    fn define_abbrev(&mut self, info: bool) -> BcResult<(u32, u32)> {
        let at = self.at;
        let count = self.bits.vbr(5)?;
        // Each operand is at least two bits.
        if count == 0 || count > self.bits.left() / 2 {
            return fail(
                BcErrorKind::Malformed,
                at,
                "an abbreviation with no operands or more than the bits left",
            );
        }
        let arena_len = match (&self.info, info) {
            (InfoRef::Owned(block_info), true) => block_info.ops.len(),
            _ => self.ops.len(),
        };
        let total = arena_len as u64 + count;
        if total > MAX_ABBREV_OPS as u64 {
            return fail(BcErrorKind::Limit, at, "more abbreviation operands than MAX_ABBREV_OPS");
        }
        let mut parsed: Vec<AbbrevOp> = Vec::with_capacity(count as usize);
        let mut index = 0;
        while index < count {
            let literal = self.bits.read(1)? == 1;
            if literal {
                parsed.push(AbbrevOp::Literal(self.bits.vbr(8)?));
                index += 1;
                continue;
            }
            let encoding = self.bits.read(3)?;
            let op = match encoding {
                1 | 2 => {
                    let width = self.bits.vbr(5)?;
                    let limit = if encoding == 1 { 64 } else { 32 };
                    if width > limit {
                        return fail(
                            BcErrorKind::Malformed,
                            at,
                            "an abbreviation field that is too wide",
                        );
                    }
                    match (encoding, width) {
                        // LLVM reads a zero-width field as the literal 0.
                        (_, 0) => AbbrevOp::Literal(0),
                        (1, _) => AbbrevOp::Fixed(width as u8),
                        (_, 1) => {
                            return fail(BcErrorKind::Malformed, at, "a VBR field of one bit");
                        }
                        _ => AbbrevOp::Vbr(width as u8),
                    }
                }
                3 => AbbrevOp::Array,
                4 => AbbrevOp::Char6,
                5 => AbbrevOp::Blob,
                _ => return fail(BcErrorKind::Malformed, at, "an unknown abbreviation encoding"),
            };
            parsed.push(op);
            index += 1;
        }
        // The shape: an array is next to last and its element is a
        // scalar encoding; a blob is last; neither is the record code.
        for (at_op, op) in parsed.iter().enumerate() {
            let last = at_op + 1 == parsed.len();
            match op {
                AbbrevOp::Array => {
                    let element = parsed.get(at_op + 1);
                    let fits = at_op + 2 == parsed.len()
                        && matches!(
                            element,
                            Some(AbbrevOp::Fixed(_) | AbbrevOp::Vbr(_) | AbbrevOp::Char6)
                        );
                    if !fits || at_op == 0 {
                        return fail(
                            BcErrorKind::Malformed,
                            at,
                            "an array that is not next to last with a scalar element",
                        );
                    }
                }
                AbbrevOp::Blob if !last || at_op == 0 => {
                    return fail(BcErrorKind::Malformed, at, "a blob that is not the last operand");
                }
                _ => {}
            }
        }
        let arena = match (&mut self.info, info) {
            (InfoRef::Owned(block_info), true) => &mut block_info.ops,
            _ => &mut self.ops,
        };
        let start = arena.len() as u32;
        arena.extend_from_slice(&parsed);
        Ok((start, parsed.len() as u32))
    }

    /// The operands of abbreviation `id` of the innermost block. A
    /// function of the fields it reads, so that the bit reader can be
    /// borrowed mutably while they are walked.
    fn abbrev<'s>(
        info: &'s InfoRef<'_>,
        stack: &[Open],
        local: &[(u32, u32)],
        ops: &'s [AbbrevOp],
        id: u32,
        at: u64,
    ) -> BcResult<&'s [AbbrevOp]> {
        let block = stack.last().map_or(u32::MAX, |open| open.id);
        let index = (id - FIRST_ABBREV) as usize;
        let info = info.get();
        let inherited = info.of(block);
        if let Some(&(start, len)) = inherited.get(index) {
            return Ok(&info.ops[start as usize..][..len as usize]);
        }
        let base = stack.last().map_or(0, |open| open.local);
        match local.get(base + index - inherited.len()) {
            Some(&(start, len)) => Ok(&ops[start as usize..][..len as usize]),
            None => {
                fail(BcErrorKind::Malformed, at, format!("abbreviation id {id} is not defined"))
            }
        }
    }

    /// Reads the record whose abbreviation id [`Self::next`] returned: its
    /// code, and its blob if the abbreviation has one. The operands
    /// (without the code) replace the contents of `ops`.
    ///
    /// # Errors
    /// `Malformed` for a record that does not fit its block, an undefined
    /// abbreviation, a code over 32 bits.
    pub fn record(&mut self, id: u32, ops: &mut Vec<u64>) -> BcResult<(u32, Option<&'a [u8]>)> {
        ops.clear();
        let at = self.at;
        if id == UNABBREV_RECORD {
            let code = self.bits.vbr(6)?;
            let count = self.bits.vbr(6)?;
            if count > self.bits.left() / 6 {
                return fail(
                    BcErrorKind::Malformed,
                    at,
                    "a record with more operands than the bits left",
                );
            }
            ops.reserve(count as usize);
            for _ in 0..count {
                ops.push(self.bits.vbr(6)?);
            }
            return Ok((code_u32(code, at)?, None));
        }
        let shape = Self::abbrev(&self.info, &self.stack, &self.local, &self.ops, id, at)?;
        let bits = &mut self.bits;
        let mut code = None;
        let mut blob = None;
        let mut index = 0;
        while index < shape.len() {
            let op = shape[index];
            let value = match op {
                AbbrevOp::Array => {
                    let element = shape[index + 1];
                    let count = bits.vbr(6)?;
                    let min = match element {
                        AbbrevOp::Fixed(width) | AbbrevOp::Vbr(width) => u64::from(width),
                        _ => 6,
                    };
                    if count > bits.left() / min {
                        return fail(
                            BcErrorKind::Malformed,
                            at,
                            "an array with more elements than the bits left",
                        );
                    }
                    ops.reserve(count as usize);
                    for _ in 0..count {
                        let value = scalar(bits, element)?;
                        ops.push(value);
                    }
                    index += 2;
                    continue;
                }
                AbbrevOp::Blob => {
                    let len = bits.vbr(6)?;
                    bits.align32()?;
                    let bytes = bits.bytes(len)?;
                    bits.align32()?;
                    blob = Some(bytes);
                    index += 1;
                    continue;
                }
                _ => scalar(bits, op)?,
            };
            if code.is_none() {
                code = Some(value);
            } else {
                ops.push(value);
            }
            index += 1;
        }
        let Some(code) = code else {
            return fail(BcErrorKind::Malformed, at, "a record without a code");
        };
        Ok((code_u32(code, at)?, blob))
    }
}

/// One scalar operand of an abbreviated record.
#[inline]
fn scalar(bits: &mut BitReader<'_>, op: AbbrevOp) -> BcResult<u64> {
    match op {
        AbbrevOp::Literal(value) => Ok(value),
        AbbrevOp::Fixed(width) => bits.read(u32::from(width)),
        AbbrevOp::Vbr(width) => bits.vbr(u32::from(width)),
        AbbrevOp::Char6 => Ok(u64::from(char6(bits.read(6)? as u8))),
        AbbrevOp::Array | AbbrevOp::Blob => {
            unreachable!("define_abbrev only lets scalars stand here")
        }
    }
}

fn code_u32(code: u64, at: u64) -> BcResult<u32> {
    match u32::try_from(code) {
        Ok(code) => Ok(code),
        Err(_) => fail(BcErrorKind::Malformed, at, "a record code over 32 bits"),
    }
}

/// The character a six-bit value stands for.
pub fn char6(value: u8) -> u8 {
    match value {
        0..=25 => b'a' + value,
        26..=51 => b'A' + (value - 26),
        52..=61 => b'0' + (value - 52),
        62 => b'.',
        _ => b'_',
    }
}

#[cfg(test)]
mod tests;
