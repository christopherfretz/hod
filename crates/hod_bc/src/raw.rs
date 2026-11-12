//! The first pass (`spec/bc.md` §3): the walk of the file's blocks, which
//! keeps the module-level records it needs, raw, in one operand pool, and
//! the spans of the function blocks. Nothing is interpreted here beyond
//! the container; the build ([`crate::module`]) reads the records in the
//! order `hod_ll` decides things in.

use crate::bitstream::{BlockInfo, BlockSpan, Cursor, Entry, BLOCKINFO_BLOCK};
use crate::codes::{self, block, md, module};
use crate::error::{fail, BcErrorKind, BcResult};

/// A record kept: its code, where it is, and its operands in the pool.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rec {
    pub code: u32,
    pub pos: u64,
    start: u32,
    len: u32,
}

impl Rec {
    /// Its operands.
    pub fn ops<'p>(&self, pool: &'p [u64]) -> &'p [u64] {
        &pool[self.start as usize..][..self.len as usize]
    }
}

/// The module METADATA block's string table: `count` strings whose VBR6
/// lengths start the blob, and whose characters start at `offset`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MdStrings<'a> {
    pub pos: u64,
    pub count: u64,
    pub offset: u64,
    pub blob: &'a [u8],
    /// The metadata id of the first string.
    pub first: u32,
}

/// One metadata id: the record that defined it (`None` for a string).
#[derive(Clone, Copy, Debug)]
pub(crate) enum MdSlot {
    /// The `index`th string of `strings[table]`.
    String { table: u32, index: u32 },
    /// A NODE, DISTINCT_NODE or VALUE record, with its operands.
    Kept(Rec),
    /// A debug-info node (or another record that defines an id), not kept.
    Skipped { code: u32, pos: u64 },
}

/// What the first pass keeps.
#[derive(Debug, Default)]
pub(crate) struct Raw<'a> {
    pub pool: Vec<u64>,
    pub info: BlockInfo,
    pub producer: Option<Rec>,
    pub epoch: Option<Rec>,
    pub version: Option<Rec>,
    pub triple: Option<Rec>,
    pub datalayout: Option<Rec>,
    pub source_filename: Option<Rec>,
    pub asm: Vec<Rec>,
    pub section_names: Vec<Rec>,
    pub gc_names: Vec<Rec>,
    pub comdats: Vec<Rec>,
    /// GLOBALVAR, FUNCTION, ALIAS and IFUNC records, in order: the order
    /// of their value ids.
    pub globals: Vec<Rec>,
    pub types: Vec<Rec>,
    pub attr_groups: Vec<Rec>,
    pub attr_lists: Vec<Rec>,
    /// The module CONSTANTS block(s), SETTYPE records included.
    pub constants: Vec<Rec>,
    pub md_strings: Vec<MdStrings<'a>>,
    pub md: Vec<MdSlot>,
    pub md_names: Vec<(Rec, Rec)>,
    pub md_kinds: Vec<Rec>,
    pub md_decl_attachments: Vec<Rec>,
    pub bundle_tags: Vec<Rec>,
    pub sync_scopes: Vec<Rec>,
    pub functions: Vec<BlockSpan>,
    pub strtab: Option<(u64, &'a [u8])>,
    /// The bit of the MODULE block's header.
    pub module_pos: u64,
}

const WRAPPER_MAGIC: u32 = 0x0B17_C0DE;
const MAGIC: [u8; 4] = *b"BC\xC0\xDE";

/// Walks `data`: a bitcode file, or one in a wrapper header.
pub(crate) fn walk(data: &[u8]) -> BcResult<Raw<'_>> {
    let (start, end) = stream_range(data)?;
    let mut cursor = Cursor::new_in(data, start + 32, end)?;
    let mut raw = Raw::default();
    let mut ops = Vec::new();
    let mut seen_module = false;
    let mut seen_ident = false;
    while !cursor.at_top_end() {
        let pos = cursor.bits().pos();
        match cursor.next()? {
            Entry::Block(BLOCKINFO_BLOCK) => cursor.read_block_info()?,
            Entry::Block(block::IDENTIFICATION) => {
                if seen_ident {
                    return fail(
                        BcErrorKind::Unsupported("multiple-modules"),
                        pos,
                        "a second IDENTIFICATION block: a file of more than one module",
                    );
                }
                seen_ident = true;
                cursor.enter()?;
                each_record(&mut cursor, &mut ops, |code, at, ops, _| {
                    let rec = push(&mut raw.pool, code, at, ops)?;
                    match code {
                        codes::ident::STRING => raw.producer = Some(rec),
                        codes::ident::EPOCH => raw.epoch = Some(rec),
                        _ => return unknown_record(at, "IDENTIFICATION", code),
                    }
                    Ok(())
                })?;
            }
            Entry::Block(block::MODULE) => {
                if seen_module {
                    return fail(
                        BcErrorKind::Unsupported("multiple-modules"),
                        pos,
                        "a second MODULE block: a file of more than one module",
                    );
                }
                seen_module = true;
                raw.module_pos = pos;
                cursor.enter()?;
                walk_module(&mut cursor, &mut raw, &mut ops)?;
            }
            Entry::Block(block::STRTAB) => {
                cursor.enter()?;
                each_record(&mut cursor, &mut ops, |code, at, _, blob| {
                    match (code, blob) {
                        (codes::STRTAB_BLOB, Some(blob)) if raw.strtab.is_none() => {
                            raw.strtab = Some((at, blob));
                        }
                        (codes::STRTAB_BLOB, _) => {
                            return fail(BcErrorKind::Invalid, at, "a second string table, or one without a blob");
                        }
                        _ => return unknown_record(at, "STRTAB", code),
                    }
                    Ok(())
                })?;
            }
            // The symbol table is for linkers working on bitcode; it
            // repeats what the module says.
            Entry::Block(block::SYMTAB) => cursor.skip(),
            Entry::Block(id) => {
                return fail(
                    BcErrorKind::Unsupported("block"),
                    pos,
                    format!("a top-level block of id {id}"),
                );
            }
            Entry::End | Entry::Record(_) => unreachable!("the cursor is at the top level"),
        }
    }
    if !seen_module {
        return fail(BcErrorKind::Malformed, start, "no MODULE block");
    }
    raw.info = cursor.into_block_info();
    Ok(raw)
}

/// The bits of the bitstream in `data`, magic number included, after a
/// wrapper header if there is one.
fn stream_range(data: &[u8]) -> BcResult<(u64, u64)> {
    let word = |at: usize| -> Option<u32> {
        let bytes = data.get(at..at + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let (offset, size) = if word(0) == Some(WRAPPER_MAGIC) {
        let (Some(offset), Some(size)) = (word(8), word(12)) else {
            return fail(BcErrorKind::Malformed, 0, "a wrapper header cut short");
        };
        let (offset, size) = (offset as usize, size as usize);
        if offset.checked_add(size).is_none_or(|end| end > data.len()) {
            return fail(BcErrorKind::Malformed, 64, "a wrapper header whose stream is not in the file");
        }
        (offset, size)
    } else {
        (0, data.len())
    };
    if data.get(offset..offset + 4) != Some(&MAGIC[..]) || size < 4 {
        return fail(BcErrorKind::Malformed, offset as u64 * 8, "not LLVM bitcode: no 'BC' 0xC0DE magic number");
    }
    // LLVM writes whole words; trailing bytes of a word cut short are
    // padding the walk never reaches.
    Ok((offset as u64 * 8, (offset + size) as u64 * 8))
}

/// Keeps a record's operands in the pool.
fn push(pool: &mut Vec<u64>, code: u32, pos: u64, ops: &[u64]) -> BcResult<Rec> {
    let (Ok(start), Ok(len)) = (u32::try_from(pool.len()), u32::try_from(ops.len())) else {
        return fail(BcErrorKind::Limit, pos, "more than 2^32 record operands in one module");
    };
    if u32::MAX - start < len {
        return fail(BcErrorKind::Limit, pos, "more than 2^32 record operands in one module");
    }
    pool.extend_from_slice(ops);
    Ok(Rec { code, pos, start, len })
}

#[cold]
fn unknown_record<T>(pos: u64, block: &str, code: u32) -> BcResult<T> {
    fail(BcErrorKind::Unsupported("record"), pos, format!("record code {code} in a {block} block"))
}

/// Calls `f(code, pos, ops, blob)` on each record of the block just
/// entered, through its end. A block inside it is an error.
fn each_record<'a>(
    cursor: &mut Cursor<'a, '_>,
    ops: &mut Vec<u64>,
    mut f: impl FnMut(u32, u64, &[u64], Option<&'a [u8]>) -> BcResult<()>,
) -> BcResult<()> {
    let outer = cursor.block_id();
    loop {
        match cursor.next()? {
            Entry::End => return Ok(()),
            Entry::Record(id) => {
                let at = cursor.entry_pos();
                let (code, blob) = cursor.record(id, ops)?;
                f(code, at, ops, blob)?;
            }
            Entry::Block(id) => {
                return fail(
                    BcErrorKind::Unsupported("block"),
                    cursor.entry_pos(),
                    format!("block {id} inside block {}", outer.unwrap_or(0)),
                );
            }
        }
    }
}

fn walk_module<'a>(cursor: &mut Cursor<'a, '_>, raw: &mut Raw<'a>, ops: &mut Vec<u64>) -> BcResult<()> {
    loop {
        match cursor.next()? {
            Entry::End => return Ok(()),
            Entry::Record(id) => {
                let at = cursor.entry_pos();
                let (code, _) = cursor.record(id, ops)?;
                let rec = push(&mut raw.pool, code, at, ops)?;
                let list = match code {
                    module::VERSION => {
                        raw.version = Some(rec);
                        continue;
                    }
                    module::TRIPLE => {
                        raw.triple = Some(rec);
                        continue;
                    }
                    module::DATALAYOUT => {
                        raw.datalayout = Some(rec);
                        continue;
                    }
                    module::SOURCE_FILENAME => {
                        raw.source_filename = Some(rec);
                        continue;
                    }
                    module::ASM => &mut raw.asm,
                    module::SECTIONNAME => &mut raw.section_names,
                    module::GCNAME => &mut raw.gc_names,
                    module::COMDAT => &mut raw.comdats,
                    module::GLOBALVAR | module::FUNCTION | module::ALIAS | module::IFUNC => &mut raw.globals,
                    // The offset of the function-level symbol table, and the
                    // module hash: neither says anything about the module.
                    module::VSTOFFSET | module::HASH => continue,
                    module::DEPLIB => {
                        return fail(BcErrorKind::Unsupported("deplib"), at, "the pre-3.0 DEPLIB record");
                    }
                    module::ALIAS_OLD => {
                        return fail(BcErrorKind::Unsupported("alias-old"), at, "the pre-3.8 ALIAS record");
                    }
                    _ => return unknown_record(at, "MODULE", code),
                };
                list.push(rec);
            }
            Entry::Block(id) => {
                let pos = cursor.entry_pos();
                match id {
                    BLOCKINFO_BLOCK => cursor.read_block_info()?,
                    block::FUNCTION => {
                        raw.functions.push(cursor.pending_span());
                        cursor.skip();
                    }
                    // Names of function-local values, use-list orders and
                    // summaries say nothing about the module.
                    block::VALUE_SYMTAB
                    | block::USELIST
                    | block::GLOBALVAL_SUMMARY
                    | block::FULL_LTO_GLOBALVAL_SUMMARY => cursor.skip(),
                    block::TYPE => collect(cursor, ops, &mut raw.pool, &mut raw.types)?,
                    block::PARAMATTR_GROUP => collect(cursor, ops, &mut raw.pool, &mut raw.attr_groups)?,
                    block::PARAMATTR => collect(cursor, ops, &mut raw.pool, &mut raw.attr_lists)?,
                    block::CONSTANTS => collect(cursor, ops, &mut raw.pool, &mut raw.constants)?,
                    block::METADATA_KIND => collect(cursor, ops, &mut raw.pool, &mut raw.md_kinds)?,
                    block::OPERAND_BUNDLE_TAGS => collect(cursor, ops, &mut raw.pool, &mut raw.bundle_tags)?,
                    block::SYNC_SCOPE_NAMES => collect(cursor, ops, &mut raw.pool, &mut raw.sync_scopes)?,
                    block::METADATA => {
                        cursor.enter()?;
                        walk_metadata(cursor, raw, ops)?;
                    }
                    _ => {
                        return fail(
                            BcErrorKind::Unsupported("block"),
                            pos,
                            format!("block {id} in a MODULE block"),
                        );
                    }
                }
            }
        }
    }
}

/// Keeps every record of the block whose header was just read.
fn collect(cursor: &mut Cursor<'_, '_>, ops: &mut Vec<u64>, pool: &mut Vec<u64>, out: &mut Vec<Rec>) -> BcResult<()> {
    cursor.enter()?;
    each_record(cursor, ops, |code, at, ops, _| {
        out.push(push(pool, code, at, ops)?);
        Ok(())
    })
}

/// The module METADATA block: strings and the records the module level
/// reads are kept; debug-info nodes only take their id (`spec/bc.md`
/// §6).
fn walk_metadata<'a>(cursor: &mut Cursor<'a, '_>, raw: &mut Raw<'a>, ops: &mut Vec<u64>) -> BcResult<()> {
    let mut name: Option<Rec> = None;
    let Raw { pool, md: slots, md_strings, md_names, md_kinds, md_decl_attachments, .. } = raw;
    each_record(cursor, ops, |code, at, ops, blob| {
        if let Some(pending) = name.take() {
            if code != md::NAMED_NODE {
                return fail(BcErrorKind::Invalid, pending.pos, "a metadata NAME not followed by its NAMED_NODE");
            }
            md_names.push((pending, push(pool, code, at, ops)?));
            return Ok(());
        }
        match code {
            md::STRINGS => {
                let (&[count, offset], Some(blob)) = (ops, blob) else {
                    return fail(BcErrorKind::Invalid, at, "a METADATA_STRINGS record without its count, offset and blob");
                };
                // Each string takes at least its length's six bits.
                if count > blob.len() as u64 * 8 / 6 || offset > blob.len() as u64 {
                    return fail(BcErrorKind::Invalid, at, "a METADATA_STRINGS count or offset past its blob");
                }
                let (Ok(table), Ok(first)) = (u32::try_from(md_strings.len()), u32::try_from(slots.len())) else {
                    return fail(BcErrorKind::Limit, at, "more than 2^32 metadata");
                };
                md_strings.push(MdStrings { pos: at, count, offset, blob, first });
                slots.reserve(count as usize);
                for index in 0..count as u32 {
                    slots.push(MdSlot::String { table, index });
                }
            }
            md::NAME => name = Some(push(pool, code, at, ops)?),
            md::NAMED_NODE => return fail(BcErrorKind::Invalid, at, "a NAMED_NODE without its NAME"),
            md::KIND => md_kinds.push(push(pool, code, at, ops)?),
            md::GLOBAL_DECL_ATTACHMENT => md_decl_attachments.push(push(pool, code, at, ops)?),
            md::NODE | md::DISTINCT_NODE | md::VALUE => slots.push(MdSlot::Kept(push(pool, code, at, ops)?)),
            md::STRING_OLD => {
                return fail(BcErrorKind::Unsupported("metadata-string-old"), at, "a pre-4.0 METADATA_STRING_OLD record");
            }
            _ => match md::defines_id(code) {
                Some(true) => slots.push(MdSlot::Skipped { code, pos: at }),
                Some(false) => {}
                None => return unknown_record(at, "METADATA", code),
            },
        }
        Ok(())
    })?;
    if let Some(pending) = name {
        return fail(BcErrorKind::Invalid, pending.pos, "a metadata NAME at the end of its block");
    }
    if u32::try_from(slots.len()).is_err() {
        return fail(BcErrorKind::Limit, cursor.entry_pos(), "more than 2^32 metadata");
    }
    Ok(())
}
