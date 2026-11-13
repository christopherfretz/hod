//! Module-level assembly of code and data (`spec/codegen.md` §9.7.3,
//! `spec/asm.md` §9): WP-112. `hod_asm::module` reads the text into
//! sections, symbols and relocations; this places them in the object
//! the way `llc` does, which emits the module's assembly before anything
//! of the module's own:
//!
//! - in [`ModuleEmitter::new`], right after `.text` exists and before any
//!   variable is placed, each section the text writes is created (or
//!   found, for `.text`) and gets the text's bytes first, its alignment,
//!   its relocations and, for code, its mapping symbols; a symbol the
//!   module does not have becomes a symbol of the object, defined or
//!   undefined as the text says;
//! - in [`ModuleEmitter::finish`], after `define`, a module symbol the
//!   text defines (a declaration, `declare` or `external global`) is
//!   defined at its label, and the directives the text gives a module
//!   symbol it does not define take effect (the rules of `spec/ir.md`
//!   §2.3, as for text of symbol directives only).
//!
//! Only AArch64: `hod_asm` has no x86-64 assembler, and an x86-64 module
//! with assembly is refused (`module:asm`, in `asm::check`). What the
//! text does that `llc` resolves by a rule hod does not reproduce is
//! refused by name (`asm:*`).

use hod_asm::a64::{Features, RefKind};
use hod_asm::module::{
    self as asm, Binding as AsmBinding, Def, ModuleCode, RelocKind as AsmReloc, SectionKind,
    SymbolType, Target, Visibility as AsmVisibility,
};
use hod_ir::{Linkage, SymId, SymbolKind as IrKind, TlsModel};
use hod_obj::{
    A64Reloc, Binding, SectionId, Symbol, SymbolId, SymbolKind, SymbolPlace, Visibility,
};

use super::section::{Found, Kind, NO_GROUP};
use super::{ModuleEmitter, obj_err, visibility};
use crate::error::{CgError, CgResult};
use crate::sym::RuntimeFn;

/// What [`ModuleEmitter::finish`] still writes for module-level assembly:
/// one entry per module symbol the text defines or gives a directive to.
#[derive(Debug, Default)]
pub(super) struct Deferred {
    entries: Vec<Entry>,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    sym: SymId,
    /// The label that defines it: section, offset, size.
    def: Option<(SectionId, u64, u64)>,
    binding: Option<Binding>,
    visibility: Option<Visibility>,
    kind: Option<SymbolKind>,
    retain: bool,
}

fn refused(what: &'static str) -> CgError {
    CgError::unsupported(what)
}

const SYMBOL: &str = hod_asm::reason::other::SYMBOL;

fn obj_binding(b: AsmBinding) -> Binding {
    match b {
        AsmBinding::Global => Binding::Global,
        AsmBinding::Weak => Binding::Weak,
    }
}

fn obj_visibility(v: AsmVisibility) -> Visibility {
    match v {
        AsmVisibility::Hidden => Visibility::Hidden,
        AsmVisibility::Protected => Visibility::Protected,
    }
}

fn obj_type(t: SymbolType) -> SymbolKind {
    match t {
        SymbolType::Function => SymbolKind::Func,
        SymbolType::Object => SymbolKind::Object,
        SymbolType::NoType => SymbolKind::NoType,
    }
}

fn reloc_kind(kind: AsmReloc) -> A64Reloc {
    match kind {
        AsmReloc::Code(RefKind::Jump26) => A64Reloc::Jump26,
        AsmReloc::Code(RefKind::Call26) => A64Reloc::Call26,
        AsmReloc::Code(RefKind::Branch19) => A64Reloc::CondBr19,
        AsmReloc::Code(RefKind::Test14) => A64Reloc::TstBr14,
        AsmReloc::Code(RefKind::Literal19) => A64Reloc::LdPrelLo19,
        AsmReloc::Code(RefKind::Adr21) => A64Reloc::AdrPrelLo21,
        AsmReloc::Abs64 => A64Reloc::Abs64,
        AsmReloc::Abs32 => A64Reloc::Abs32,
        AsmReloc::Abs16 => A64Reloc::Abs16,
    }
}

impl ModuleEmitter<'_> {
    /// Assembles the module's assembly and places it (module
    /// documentation). Text of symbol directives only is left to
    /// `module_asm_symbols` (WP-075), unchanged.
    ///
    /// # Errors
    /// `hod_asm`'s `asm:*` refusals; [`SYMBOL`] for a use of a module
    /// symbol `llc` does not resolve plainly.
    pub(super) fn place_module_asm(&mut self) -> CgResult<()> {
        let module = self.module;
        if module.module_asm().is_empty()
            || hod_ir::module_asm_effects(module, &mut Vec::new()).is_ok()
        {
            return Ok(());
        }
        // `asm::check` refused it on x86-64.
        let text = std::str::from_utf8(module.module_asm())
            .map_err(|_| refused(hod_asm::reason::other::LEXICAL))?;
        let mut code = ModuleCode::default();
        asm::assemble_module(text, Features::BASE, &mut code).map_err(|r| refused(r.0))?;
        let sections = self.asm_sections(&code)?;
        let (ids, deferred) = self.asm_symbols(&code, &sections)?;
        for (s, &id) in code.sections.iter().zip(&sections) {
            for r in &s.relocs {
                let (target, addend) = match r.target {
                    Target::Symbol(i) => {
                        let Some(target) = ids[i as usize] else {
                            return Err(CgError::internal("a relocation against a temporary"));
                        };
                        (target, r.addend)
                    }
                    Target::Section(j) => (self.obj.section_symbol(sections[j as usize]), r.addend),
                };
                self.obj.add_relocation(id, r.offset, reloc_kind(r.kind), target, addend);
            }
        }
        self.asm_deferred = Some(Box::new(deferred));
        Ok(())
    }

    /// Creates or finds each section and writes the text's contents into
    /// it, first.
    fn asm_sections(&mut self, code: &ModuleCode) -> CgResult<Vec<SectionId>> {
        let mut ids = Vec::with_capacity(code.sections.len());
        let mut name = std::mem::take(&mut self.name);
        for s in &code.sections {
            let kind = match s.kind {
                SectionKind::Text => Kind::Text,
                SectionKind::ReadOnly => Kind::ReadOnly,
                SectionKind::Data => Kind::Data,
                SectionKind::Bss => Kind::Bss { local: false },
            };
            let standard = matches!(s.name.as_str(), ".text" | ".rodata" | ".data" | ".bss");
            let found = if standard {
                self.sections.implicit(kind, false, false, NO_GROUP, b"", 1, &mut name)
            } else {
                name.clear();
                name.extend_from_slice(s.name.as_bytes());
                self.sections.explicit(s.name.as_bytes(), kind, false, NO_GROUP, 1)
            };
            let id = match found {
                Found::Old(id) => id,
                Found::New(new, pending) => {
                    let id = self.obj.add_section(&name, new.kind, s.align).map_err(obj_err)?;
                    let cache = standard.then_some((kind, 1));
                    self.sections.created(&name, &pending, id, cache);
                    id
                }
            };
            if self.obj.section_size(id) != 0 {
                self.name = name;
                return Err(CgError::internal("module assembly after a section's contents"));
            }
            self.obj.raise_section_align(id, s.align);
            if s.kind.is_nobits() {
                self.obj.append_zeros(id, s.size);
            } else {
                self.obj.append(id, &s.bytes);
            }
            if self.arch().has_mapping_symbols() && s.kind == SectionKind::Text {
                for &(at, m) in &s.mapping {
                    let which = usize::from(m == asm::Mapping::Data);
                    let symbol =
                        Symbol::defined(self.mapping_names[which], SymbolKind::NoType, id, at, 0)
                            .with_binding(Binding::Local);
                    self.obj.add_symbol(symbol);
                }
                if let Some(&(_, m)) = s.mapping.last() {
                    let index = id.index() as usize;
                    if self.mapping.len() <= index {
                        self.mapping.resize(index + 1, 0);
                    }
                    self.mapping[index] = if m == asm::Mapping::Code { 1 } else { 2 };
                }
            }
            ids.push(id);
        }
        self.name = name;
        Ok(ids)
    }

    /// The object symbol of each of the text's symbols (`None` for a
    /// `.L` name), and what `finish` still writes for module symbols.
    fn asm_symbols(
        &mut self,
        code: &ModuleCode,
        sections: &[SectionId],
    ) -> CgResult<(Vec<Option<SymbolId>>, Deferred)> {
        let module = self.module;
        let mut ids = Vec::with_capacity(code.symbols.len());
        let mut deferred = Deferred::default();
        for s in &code.symbols {
            let label = match s.def {
                Def::Label(section, offset) => Some((sections[section as usize], offset)),
                _ => None,
            };
            if let Some(sym) = module.find_symbol(s.name.as_bytes()) {
                let entry = if let Some((section, offset)) = label {
                    self.asm_definition(sym, s, section, offset)?
                } else if s.def == Def::Undefined {
                    self.asm_directives(sym, s)?
                } else {
                    // `.set` of a module symbol's name.
                    return Err(refused(SYMBOL));
                };
                deferred.entries.push(entry);
                ids.push(Some(Self::obj_sym(sym)));
                continue;
            }
            if s.is_temporary() {
                if s.binding.is_some() || s.visibility.is_some() || s.ty.is_some() {
                    return Err(refused(SYMBOL));
                }
                ids.push(None);
                continue;
            }
            // A name hod gives anonymous symbols.
            if s.name.starts_with("__unnamed_") {
                return Err(refused(SYMBOL));
            }
            let name = self.obj.add_string(s.name.as_bytes()).map_err(obj_err)?;
            let mut symbol = match s.def {
                Def::Label(..) => {
                    let (section, offset) = label.expect("a label");
                    Symbol::defined(name, SymbolKind::NoType, section, offset, s.size.unwrap_or(0))
                        .with_binding(Binding::Local)
                }
                Def::Absolute(value) => Symbol {
                    value: value as u64,
                    place: SymbolPlace::Absolute,
                    binding: Binding::Local,
                    ..Symbol::undefined(name)
                },
                Def::Undefined => Symbol::undefined(name),
            };
            if let Some(b) = s.binding {
                symbol.binding = obj_binding(b);
            }
            if let Some(v) = s.visibility {
                symbol.visibility = obj_visibility(v);
            }
            if let Some(t) = s.ty {
                symbol.kind = obj_type(t);
            }
            let id = self.obj.add_symbol(symbol);
            // A symbol the text names is written even when nothing refers
            // to it, as `llc` writes it.
            self.obj.retain_symbol(id);
            // Code that calls a runtime function of this name calls this
            // symbol: the assembler has one symbol per name.
            if let Some(func) = RuntimeFn::ALL.iter().find(|f| f.name() == s.name) {
                self.runtime[*func as usize] = id.index();
            }
            ids.push(Some(id));
        }
        Ok((ids, deferred))
    }

    /// A module symbol the text defines by a label: a plain declaration
    /// (`external`, not thread-local) given `.globl` or `.weak`, which
    /// `llc` writes as the text defines it, with the module's visibility
    /// when it is not the default (emitted after the text).
    fn asm_definition(
        &self,
        sym: SymId,
        s: &asm::Symbol,
        section: SectionId,
        offset: u64,
    ) -> CgResult<Entry> {
        let symbol = self.module.symbol(sym);
        let plain = symbol.linkage() == Linkage::External
            && !symbol.is_definition()
            && !symbol.is_unresolved()
            && symbol.tls() == TlsModel::None
            && matches!(symbol.kind(), IrKind::Function { .. } | IrKind::Variable { .. });
        // A label of local binding that the module names: `llc` would
        // reach it through its section.
        let Some(binding) = s.binding.filter(|_| plain) else { return Err(refused(SYMBOL)) };
        let ir_visibility = visibility(symbol.visibility());
        Ok(Entry {
            sym,
            def: Some((section, offset, s.size.unwrap_or(0))),
            binding: Some(obj_binding(binding)),
            visibility: if ir_visibility == Visibility::Default {
                Some(s.visibility.map_or(Visibility::Default, obj_visibility))
            } else {
                None
            },
            kind: Some(s.ty.map_or(SymbolKind::NoType, obj_type)),
            retain: true,
        })
    }

    /// A module symbol the text names but does not define: the rules of
    /// `spec/ir.md` §2.3 (`hod_ir::module_asm_effects`) for its
    /// directives; a reference alone changes nothing.
    fn asm_directives(&self, sym: SymId, s: &asm::Symbol) -> CgResult<Entry> {
        let symbol = self.module.symbol(sym);
        let linkage = symbol.linkage();
        let weakish = matches!(
            linkage,
            Linkage::LinkOnceAny | Linkage::LinkOnceOdr | Linkage::WeakAny | Linkage::WeakOdr
        );
        if matches!(
            linkage,
            Linkage::Internal
                | Linkage::Private
                | Linkage::Appending
                | Linkage::AvailableExternally
        ) || symbol.is_unresolved()
        {
            return Err(refused(SYMBOL));
        }
        let definition = symbol.is_definition();
        let binding = match (s.binding, definition) {
            (Some(AsmBinding::Global), _) if weakish || linkage == Linkage::ExternWeak => {
                return Err(refused(SYMBOL));
            }
            (None | Some(AsmBinding::Global), _) => None,
            (Some(AsmBinding::Weak), true) if weakish => None,
            (Some(AsmBinding::Weak), true) => return Err(refused(SYMBOL)),
            (Some(AsmBinding::Weak), false) if linkage == Linkage::ExternWeak => None,
            (Some(AsmBinding::Weak), false) => Some(Binding::Weak),
        };
        if linkage == Linkage::Common && binding.is_some() {
            return Err(refused(SYMBOL));
        }
        let visibility = if symbol.visibility() == hod_ir::Visibility::Default {
            s.visibility.map(obj_visibility)
        } else {
            None
        };
        let natural = match symbol.kind() {
            IrKind::Function { .. } => Some(SymbolType::Function),
            IrKind::Variable { tls: TlsModel::None, .. } => Some(SymbolType::Object),
            _ => None,
        };
        let kind = match s.ty {
            None => None,
            Some(_) if symbol.tls() != TlsModel::None => return Err(refused(SYMBOL)),
            Some(ty) if definition => {
                if natural != Some(ty) {
                    return Err(refused(SYMBOL));
                }
                None
            }
            Some(ty) => Some(obj_type(ty)),
        };
        let named = s.binding.is_some() || s.visibility.is_some() || s.ty.is_some();
        Ok(Entry { sym, def: None, binding, visibility, kind, retain: named && !definition })
    }

    /// Writes what [`ModuleEmitter::place_module_asm`] left for after
    /// `define`.
    ///
    /// # Errors
    /// An internal error if the object refuses a name.
    pub(super) fn finish_module_asm(&mut self, deferred: &Deferred) -> CgResult<()> {
        for e in &deferred.entries {
            let id = Self::obj_sym(e.sym);
            if e.retain {
                self.name_symbol(e.sym)?;
                self.obj.retain_symbol(id);
            }
            let target = self.obj.symbol_mut(id);
            if let Some((section, value, size)) = e.def {
                target.place = SymbolPlace::Section(section);
                target.value = value;
                target.size = size;
            }
            if let Some(b) = e.binding {
                target.binding = b;
            }
            if let Some(v) = e.visibility {
                target.visibility = v;
            }
            if let Some(k) = e.kind {
                target.kind = k;
            }
        }
        Ok(())
    }
}
