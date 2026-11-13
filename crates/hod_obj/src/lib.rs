//! Object-file model and the ELF64 relocatable writer.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/object.md` for
//! its contract. In short: build an [`Object`] (sections, symbols,
//! relocations, groups), then serialize it with [`elf::write`] or
//! [`elf::to_vec`].
//!
//! ```
//! use hod_obj::{A64Reloc, Arch, Object, SectionKind, Symbol, SymbolKind, elf};
//!
//! let mut obj = Object::new(Arch::AArch64);
//! let text = obj.add_section(b".text", SectionKind::Text, 4)?;
//! // bl callee; ret
//! let at = obj.append(text, &[0x00, 0x00, 0x00, 0x94, 0xc0, 0x03, 0x5f, 0xd6]);
//! let name = obj.add_string(b"caller")?;
//! obj.add_symbol(Symbol::defined(name, SymbolKind::Func, text, at, 8));
//! let callee = obj.add_string(b"callee")?;
//! let callee = obj.add_symbol(Symbol::undefined(callee));
//! obj.add_relocation(text, at, A64Reloc::Call26, callee, 0);
//! let file = elf::to_vec(&obj)?;
//! assert_eq!(&file[..4], b"\x7fELF");
//! # Ok::<(), hod_obj::Error>(())
//! ```

mod compress;
pub mod deflate;
mod dwo;
pub mod elf;
mod error;
mod model;
mod names;
mod reloc;

pub use compress::{CHDR_SIZE, Compression, ELFCOMPRESS_ZLIB};
pub use error::{Error, Limit, SymbolProblem, WriteError};
pub use model::{
    Arch, Binding, GroupId, GroupKind, Object, Relocation, SectionId, SectionKind, StackNote,
    StrId, Symbol, SymbolId, SymbolKind, SymbolPlace, Visibility,
};
pub use reloc::{A64Reloc, RelocKind, X64Reloc};
