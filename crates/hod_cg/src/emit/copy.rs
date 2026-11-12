//! Local copies of `available_externally` `alwaysinline` functions
//! (`spec/codegen.md` §9.7.1, design 0001 point 6, WP-021).
//!
//! `llc` writes nothing for an `available_externally` function: every
//! reference is to the external symbol. A module that never met `-O0`'s
//! always-inliner may still call such a function directly when it is
//! `alwaysinline`, and the program then links only if the calls go
//! somewhere this object provides. When module emission is given the
//! body of one, it places the code under a local symbol of its own,
//! `<name>.alwaysinline` (`STB_LOCAL`, `STT_FUNC`, in the section the
//! function's own name selects, no group), and sends the direct calls
//! and tail calls of the module's code to it. Every other reference (an
//! address taken in code or in data, an alias) stays on the external
//! symbol, so a function pointer compares equal across objects. Its
//! unwind entry and its subprogram are written as any function's.
//!
//! A function whose body is not given, or that is not `alwaysinline`, is
//! what it was: not emitted, referenced externally.

use hod_ir::{FnAttrs, Linkage, SymId, SymbolKind as IrKind};
use hod_obj::X64Reloc;
use hod_obj::{A64Reloc, Binding, RelocKind, SectionId, Symbol, SymbolId, SymbolKind};

use super::{ModuleEmitter, obj_err};
use crate::error::CgResult;
use crate::sym::SymRef;

/// The suffix of a copy's symbol name: `<name>.alwaysinline` (a GNU
/// demangler reads it as a clone, `[clone .alwaysinline]`).
pub const COPY_SUFFIX: &str = ".alwaysinline";

/// Whether `kind` is a branch to a function (a call or a tail call): the
/// only relocations that go to a copy.
fn is_branch(kind: RelocKind) -> bool {
    matches!(
        kind,
        RelocKind::A64(A64Reloc::Call26 | A64Reloc::Jump26) | RelocKind::X64(X64Reloc::Plt32)
    )
}

impl ModuleEmitter<'_> {
    /// Whether the code of `sym`, when given, is placed as a copy: an
    /// `available_externally` `alwaysinline` function.
    pub(super) fn is_copy(&self, sym: SymId) -> bool {
        let symbol = self.module.symbol(sym);
        match symbol.kind() {
            IrKind::Function { attrs, .. } => {
                symbol.linkage() == Linkage::AvailableExternally
                    && self.module.fn_attrs(attrs).has(FnAttrs::ALWAYS_INLINE)
            }
            _ => false,
        }
    }

    /// Defines the copy of `sym`, a new local symbol: its code is `size`
    /// bytes at `offset` in `section`.
    pub(super) fn place_copy(
        &mut self,
        sym: SymId,
        section: SectionId,
        offset: u64,
        size: u64,
    ) -> CgResult<()> {
        self.name.clear();
        self.name.extend_from_slice(self.module.symbol(sym).name());
        self.name.extend_from_slice(COPY_SUFFIX.as_bytes());
        let name = self.obj.add_string(&self.name).map_err(obj_err)?;
        let symbol = Symbol::defined(name, SymbolKind::Func, section, offset, size)
            .with_binding(Binding::Local);
        let id = self.obj.add_symbol(symbol);
        self.copies.insert(sym, id);
        Ok(())
    }

    /// Where a code relocation of `kind` to `sym` goes when it may go to
    /// a copy: `Some(Some(copy))` once the copy is placed, `Some(None)`
    /// while it may still be (the relocation waits for `finish`), `None`
    /// for every other relocation. In `finish` (`last`), a copy that was
    /// never placed leaves the relocation on the external symbol.
    pub(super) fn copy_target(
        &self,
        kind: RelocKind,
        sym: SymRef,
        last: bool,
    ) -> Option<Option<SymbolId>> {
        let SymRef::Module(sym) = sym else { return None };
        if !is_branch(kind) || !self.is_copy(sym) {
            return None;
        }
        match self.copies.get(&sym) {
            Some(&copy) => Some(Some(copy)),
            None if last => None,
            None => Some(None),
        }
    }
}
