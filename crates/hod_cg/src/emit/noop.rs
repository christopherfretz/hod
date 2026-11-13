//! The noop coroutine frame (`spec/codegen.md` "Coroutines", WP-105):
//! what LLVM's `coro-early` makes of `llvm.coro.noop`, once per object.
//!
//! - `__NoopCoro_ResumeDestroy`: a function that only returns, whose
//!   code is the target's `Target::NOOP_CORO_FN` (carried by the chunk
//!   that referred to the frame), in `.text` after every other function
//!   (`.text.__NoopCoro_ResumeDestroy` with function sections), with a
//!   local `STT_FUNC` symbol of that name: LLVM 22 makes it `internal`
//!   (LLVM 19 `private`, so its `llc` writes no symbol). No unwind entry:
//!   it calls nothing and changes no register but the program counter.
//! - `NoopCoro.Frame.Const`: a private constant `{ptr, ptr}`, both
//!   pointers the function, in `.data.rel.ro` (PIC) or `.rodata`, as
//!   `llc` places it. Private: no symbol; references name the section.
//!
//! Each object has its own frame, as each `llc` object has: two noop
//! handles compare equal within an object and not across objects.

use hod_obj::{Binding, SectionId, Symbol, SymbolId, SymbolKind};

use super::section::{Found, Kind, NO_GROUP};
use super::{ModuleEmitter, obj_err};
use crate::error::{CgError, CgResult};
use crate::record::Chunk;

/// The function's name: LLVM's.
const FUNCTION: &[u8] = b"__NoopCoro_ResumeDestroy";
/// The frame's name as `llc` names a private symbol (it has no symbol).
const FRAME: &[u8] = b".LNoopCoro.Frame.Const";
/// The frame: two pointers.
const FRAME_SIZE: u64 = 16;
const FRAME_ALIGN: u64 = 8;

/// The state of the noop frame in one object.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NoopFrame {
    /// The function's code, from a chunk that referred to the frame.
    body: &'static [u8],
    /// Where the frame is, once placed.
    placed: Option<(SectionId, u64)>,
}

impl ModuleEmitter<'_> {
    /// Takes the noop function's code from `chunk`, if one of its
    /// functions referred to the frame.
    pub(super) fn note_noop(&mut self, chunk: &Chunk) {
        if !chunk.noop_coro_fn.is_empty() {
            self.noop.body = chunk.noop_coro_fn;
        }
    }

    /// The symbol and addend a relocation to the frame plus `addend`
    /// names: the frame's section. The frame and its function are placed
    /// on the first call; every function is placed by then.
    ///
    /// # Errors
    /// An internal error for a reference to the frame when no chunk
    /// carried the function's code.
    pub(super) fn noop_target(&mut self, addend: i64) -> CgResult<(SymbolId, i64)> {
        let (section, offset) = match self.noop.placed {
            Some(placed) => placed,
            None => self.place_noop()?,
        };
        Ok((self.obj.section_symbol(section), addend.wrapping_add(offset as i64)))
    }

    /// The section of `kind`, as `llc` chooses it for a symbol named
    /// `own` with no COMDAT, created if it does not exist.
    fn noop_section(
        &mut self,
        kind: Kind,
        unique: bool,
        own: &[u8],
        align: u64,
    ) -> CgResult<SectionId> {
        let mut name = std::mem::take(&mut self.name);
        let section =
            match self.sections.implicit(kind, unique, false, NO_GROUP, own, align, &mut name) {
                Found::Old(section) => section,
                Found::New(new, pending) => {
                    let section = self.obj.add_section(&name, new.kind, 1).map_err(obj_err)?;
                    self.sections.created(&name, &pending, section, Some((kind, align)));
                    section
                }
            };
        self.name = name;
        Ok(section)
    }

    fn place_noop(&mut self) -> CgResult<(SectionId, u64)> {
        let body = self.noop.body;
        if body.is_empty() {
            return Err(CgError::internal(
                "a reference to the noop coroutine frame and no chunk with its function",
            ));
        }
        let arch = self.arch();
        let Some(abs) = arch.abs(8) else {
            return Err(CgError::internal("no 8-byte absolute relocation"));
        };
        // The function.
        let align = arch.text_align().max(4);
        let text =
            self.noop_section(Kind::Text, self.options.function_sections, FUNCTION, align)?;
        arch.pad_code(&mut self.obj, text, align);
        let at = self.obj.append(text, body);
        if arch.has_mapping_symbols() {
            let index = text.index() as usize;
            if self.mapping.len() <= index {
                self.mapping.resize(index + 1, 0);
            }
            if self.mapping[index] != 1 {
                let symbol =
                    Symbol::defined(self.mapping_names[0], SymbolKind::NoType, text, at, 0)
                        .with_binding(Binding::Local);
                self.obj.add_symbol(symbol);
                self.mapping[index] = 1;
            }
        }
        let name = self.obj.add_string(FUNCTION).map_err(obj_err)?;
        let symbol = Symbol::defined(name, SymbolKind::Func, text, at, body.len() as u64)
            .with_binding(Binding::Local);
        self.obj.add_symbol(symbol);
        // The frame.
        let kind =
            if self.options.reloc_model.is_pic() { Kind::ReadOnlyWithRel } else { Kind::ReadOnly };
        let data = self.noop_section(kind, self.options.data_sections, FRAME, FRAME_ALIGN)?;
        self.obj.raise_section_align(data, FRAME_ALIGN);
        let offset = self.obj.align_to(data, FRAME_ALIGN, 0);
        self.obj.append_zeros(data, FRAME_SIZE);
        let function = self.obj.section_symbol(text);
        for field in [0, 8] {
            self.obj.add_relocation(data, offset + field, abs, function, at as i64);
        }
        self.noop.placed = Some((data, offset));
        Ok((data, offset))
    }
}
