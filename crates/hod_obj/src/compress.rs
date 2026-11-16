//! Compressed sections (`spec/object.md` "Compressed sections", WP-061):
//! `SHF_COMPRESSED` with an `Elf64_Chdr` and a zlib stream, as
//! `clang -gz=zlib` and `ld --compress-debug-sections=zlib` write them.

use crate::deflate::{Deflater, MAX_INPUT};
use crate::elf::{SHF_ALLOC, SHF_COMPRESSED, header_fields};
use crate::model::{Object, SectionId, SectionKind};
use crate::{Error, Limit};

/// `ELFCOMPRESS_ZLIB`.
pub const ELFCOMPRESS_ZLIB: u32 = 1;
/// The size of an `Elf64_Chdr`.
pub const CHDR_SIZE: usize = 24;

/// How sections are compressed. zstd (`ELFCOMPRESS_ZSTD`) is not
/// written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Compression {
    /// Not compressed.
    #[default]
    None,
    /// `ELFCOMPRESS_ZLIB`: a zlib stream (RFC 1950) after the header.
    Zlib,
}

/// The size of an `Elf32_Chdr` (WP-284): `ch_type`, `ch_size`,
/// `ch_addralign`, four bytes each.
pub const CHDR32_SIZE: usize = 12;

/// The uncompressed size an `SHF_COMPRESSED` section's header records
/// (an `Elf32_Chdr` when `elf32`), or 0 when its data is too short to hold
/// a header.
pub(crate) fn uncompressed_size(data: &[u8], elf32: bool) -> u64 {
    if elf32 {
        return data
            .get(4..8)
            .map_or(0, |b| u64::from(u32::from_le_bytes(b.try_into().unwrap_or([0; 4]))));
    }
    data.get(8..16).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
}

impl Object {
    /// Compresses a section's contents in place: its data becomes an
    /// `Elf64_Chdr` (`ch_type` [`ELFCOMPRESS_ZLIB`], `ch_size` the old
    /// size, `ch_addralign` the old alignment) and the zlib stream of the
    /// old data; its kind gains `SHF_COMPRESSED` (as a
    /// [`SectionKind::Elf`] with the same type, flags and entry size); its
    /// alignment becomes 8. Relocations keep their offsets, which are
    /// offsets into the uncompressed data (the writer checks them against
    /// the recorded size). A section is left as it is, and `Ok(false)`
    /// returned, when compression is [`Compression::None`] or would not
    /// make it smaller. Nothing may be appended to a section once it is
    /// compressed.
    ///
    /// # Errors
    /// [`Error::NotCompressible`] for a section that is part of the
    /// loaded image (`SHF_ALLOC`), occupies no file space, or is already
    /// compressed; [`Error::TooLarge`] with [`Limit::CompressedBytes`] for
    /// data longer than 4 GiB less one byte.
    pub fn compress_section(
        &mut self,
        section: SectionId,
        compression: Compression,
    ) -> Result<bool, Error> {
        self.compress_with(section, compression, &mut Deflater::new())
    }

    /// [`Object::compress_section`] for every section whose name starts
    /// with `.debug_` and that is not part of the loaded image, in section
    /// order, and returns how many were compressed. This is what
    /// `clang -gz=zlib` compresses.
    ///
    /// # Errors
    /// As [`Object::compress_section`].
    pub fn compress_debug_sections(&mut self, compression: Compression) -> Result<u32, Error> {
        if compression == Compression::None {
            return Ok(0);
        }
        let mut deflater = Deflater::new();
        let mut count = 0;
        for index in 0..self.sections.len() {
            let section = SectionId(index as u32);
            let sec = &self.sections[index];
            let (_, flags, _) = header_fields(sec.kind);
            if !self.section_name(section).starts_with(b".debug_")
                || flags & (SHF_ALLOC | SHF_COMPRESSED) != 0
                || sec.kind.is_nobits()
            {
                continue;
            }
            count += u32::from(self.compress_with(section, compression, &mut deflater)?);
        }
        Ok(count)
    }

    fn compress_with(
        &mut self,
        section: SectionId,
        compression: Compression,
        deflater: &mut Deflater,
    ) -> Result<bool, Error> {
        if compression == Compression::None {
            return Ok(false);
        }
        let elf32 = self.machine.is_elf32();
        let sec = &self.sections[section.0 as usize];
        let (sh_type, sh_flags, sh_entsize) = header_fields(sec.kind);
        if sec.kind.is_nobits() || sh_flags & (SHF_ALLOC | SHF_COMPRESSED) != 0 {
            return Err(Error::NotCompressible { section });
        }
        let size = sec.data.len() as u64;
        if size > MAX_INPUT {
            return Err(Error::TooLarge(Limit::CompressedBytes));
        }
        // An `SHT_REL` object's addends are in the bytes: they are added
        // before compression, which hides the fields (WP-284).
        let patched = if elf32 { crate::elf::addends_into(self, section)? } else { None };
        let sec = &mut self.sections[section.0 as usize];
        let input = patched.as_deref().unwrap_or(&sec.data);
        let mut out = Vec::with_capacity(CHDR_SIZE + input.len() / 3 + 64);
        out.extend_from_slice(&ELFCOMPRESS_ZLIB.to_le_bytes());
        if elf32 {
            // `Elf32_Chdr`; the size is below `MAX_INPUT`, the alignment
            // was checked by `addends_into`.
            out.extend_from_slice(&(size as u32).to_le_bytes());
            out.extend_from_slice(&(sec.align() as u32).to_le_bytes());
        } else {
            out.extend_from_slice(&0u32.to_le_bytes()); // ch_reserved
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&sec.align().to_le_bytes());
        }
        deflater.zlib(input, &mut out).map_err(|()| Error::TooLarge(Limit::CompressedBytes))?;
        if out.len() as u64 >= size {
            return Ok(false);
        }
        sec.data = out;
        sec.kind = SectionKind::Elf { sh_type, sh_flags: sh_flags | SHF_COMPRESSED, sh_entsize };
        // LLVM aligns a compressed section to its class's word.
        sec.align_log2 = if elf32 { 2 } else { 3 };
        Ok(true)
    }
}
