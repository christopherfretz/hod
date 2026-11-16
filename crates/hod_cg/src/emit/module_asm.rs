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
//! AArch64 and, since WP-204 (`spec/asm.md` §14), x86-64. The text
//! may also give `.symver` directives (x86-64 since WP-204, AArch64
//! since WP-212, §15): in `finish`, each one adds the
//! versioned alias of a defined symbol, or renames the symbol (`@@@`,
//! or an undefined one), as LLVM's assembler does. What the text does
//! that `llc` resolves by a rule hod does not reproduce is refused by
//! name (`asm:*`).

use hod_asm::a64::{Features, RefKind};
use hod_asm::module::{
    self as asm, Binding as AsmBinding, Def, ModuleCode, RelocKind as AsmReloc, SectionKind,
    SymbolType, Target, Visibility as AsmVisibility,
};
use hod_ir::{Linkage, SymId, SymbolKind as IrKind, TlsModel};
use hod_obj::{
    A64Reloc, Binding, I386Reloc, RelocKind, SectionId, Symbol, SymbolId, SymbolKind, SymbolPlace,
    Visibility, X64Reloc,
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
    /// The `.symver` directives, in order: the object symbol of the
    /// name, and the versioned name.
    symvers: Vec<(SymbolId, String)>,
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

/// The object's kind of a relocation the text writes, for the target's
/// architecture.
fn reloc_kind(kind: AsmReloc, arch: hod_ir::Arch) -> RelocKind {
    use hod_asm::x64::RefKind as X;
    match arch {
        hod_ir::Arch::X86_64 => {}
        hod_ir::Arch::I386 => return i386_reloc_kind(kind).into(),
        _ => return a64_reloc_kind(kind).into(),
    }
    let k = match kind {
        AsmReloc::X64(X::Pc32) => X64Reloc::Pc32,
        AsmReloc::X64(X::Plt32) => X64Reloc::Plt32,
        AsmReloc::X64(X::GotPcRel) => X64Reloc::GotPcrel,
        AsmReloc::X64(X::GotPcRelX) => X64Reloc::GotPcrelX,
        AsmReloc::X64(X::RexGotPcRelX) => X64Reloc::RexGotPcrelX,
        AsmReloc::X64(X::GotTpOff) => X64Reloc::GotTpOff,
        AsmReloc::X64(X::TpOff32) => X64Reloc::TpOff32,
        AsmReloc::X64(X::DtpOff32) => X64Reloc::DtpOff32,
        AsmReloc::X64(X::Abs32S) => X64Reloc::Abs32S,
        AsmReloc::Abs64 => X64Reloc::Abs64,
        AsmReloc::Abs32 => X64Reloc::Abs32,
        AsmReloc::Abs16 => X64Reloc::Abs16,
        // The x86-64 assembler writes no AArch64 or 32-bit field.
        AsmReloc::X64(X::None) | AsmReloc::Code(_) | AsmReloc::X86(_) => X64Reloc::None,
    };
    k.into()
}

/// The 32-bit x86 relocation of a field (`spec/asm.md` §23). The kinds
/// `hod_obj` has no name for are written by number: `R_386_GOT32` (3),
/// `R_386_16` (20), `R_386_TLS_IE_32` (33), `R_386_TLS_LE_32` (34); the
/// caller puts their addends in place ([`I386Reloc::field_size`] is
/// unknown to the writer for them).
fn i386_reloc_kind(kind: AsmReloc) -> I386Reloc {
    use hod_asm::x86::RefKind as K;
    match kind {
        AsmReloc::X86(K::Abs32) | AsmReloc::Abs32 => I386Reloc::Abs32,
        AsmReloc::X86(K::Pc32) => I386Reloc::Pc32,
        AsmReloc::X86(K::Plt32) => I386Reloc::Plt32,
        AsmReloc::X86(K::Got32) => I386Reloc::Other(3),
        AsmReloc::X86(K::Got32X) => I386Reloc::Got32X,
        AsmReloc::X86(K::GotOff) => I386Reloc::GotOff,
        AsmReloc::X86(K::GotPc) => I386Reloc::GotPc,
        AsmReloc::X86(K::TlsLe) => I386Reloc::TlsLe,
        AsmReloc::X86(K::TlsLe32) => I386Reloc::Other(34),
        AsmReloc::X86(K::TlsIe) => I386Reloc::TlsIe,
        AsmReloc::X86(K::TlsIe32) => I386Reloc::Other(33),
        AsmReloc::X86(K::TlsGotIe) => I386Reloc::TlsGotIe,
        AsmReloc::X86(K::TlsGd) => I386Reloc::TlsGd,
        AsmReloc::X86(K::TlsLdm) => I386Reloc::TlsLdm,
        AsmReloc::X86(K::TlsLdo32) => I386Reloc::TlsLdo32,
        AsmReloc::Abs16 => I386Reloc::Other(20),
        // The 32-bit assembler writes no 64-bit field (`.quad` of a
        // symbol is refused) and no AArch64 or x86-64 one.
        AsmReloc::Abs64 | AsmReloc::X64(_) | AsmReloc::Code(_) => I386Reloc::None,
    }
}

fn a64_reloc_kind(kind: AsmReloc) -> A64Reloc {
    match kind {
        // The AArch64 assembler writes no x86 field.
        AsmReloc::X64(_) | AsmReloc::X86(_) => A64Reloc::None,
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

/// Puts the addend of each 32-bit relocation whose kind `hod_obj` writes
/// by number ([`i386_reloc_kind`]) into its field, as `SHT_REL` keeps it,
/// and leaves the relocation's own addend zero: the ELF32 writer stores
/// addends of the kinds whose width it knows only.
///
/// # Errors
/// `asm:operand/range` for an addend the field does not hold.
fn i386_fields(code: &mut ModuleCode) -> CgResult<()> {
    for s in &mut code.sections {
        for r in &mut s.relocs {
            if r.addend == 0 || i386_reloc_kind(r.kind).field_size().is_some() {
                continue;
            }
            let width = if r.kind == AsmReloc::Abs16 { 2 } else { 4 };
            let bits = 8 * width as u32;
            if r.addend < -(1i64 << (bits - 1)) || r.addend >= 1i64 << bits {
                return Err(refused(hod_asm::reason::operand::RANGE));
            }
            let at = usize::try_from(r.offset).map_err(|_| CgError::internal("a field offset"))?;
            let Some(field) = s.bytes.get_mut(at..at + width) else {
                return Err(CgError::internal("a relocation outside its section"));
            };
            field.copy_from_slice(&r.addend.to_le_bytes()[..width]);
            r.addend = 0;
        }
    }
    Ok(())
}

/// Whether a 32-bit thread-local field of the text is against symbol
/// `sym` (an index into [`ModuleCode::symbols`]).
fn tls_field(code: &ModuleCode, sym: usize) -> bool {
    code.sections.iter().flat_map(|s| &s.relocs).any(|r| {
        matches!(r.kind, AsmReloc::X86(k) if k.is_tls()) && r.target == Target::Symbol(sym as u32)
    })
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
        let text = std::str::from_utf8(module.module_asm())
            .map_err(|_| refused(hod_asm::reason::other::LEXICAL))?;
        let mut code = ModuleCode::default();
        let arch = module.target().arch();
        match arch {
            hod_ir::Arch::X86_64 => asm::assemble_module_x64(text, &mut code),
            hod_ir::Arch::I386 => asm::assemble_module_x86(text, &mut code),
            _ => asm::assemble_module(text, Features::BASE, &mut code),
        }
        .map_err(|r| refused(r.0))?;
        if arch == hod_ir::Arch::I386 {
            i386_fields(&mut code)?;
        }
        // A global of an explicit section that names a `@nobits` section
        // of the text's own: `llc` joins a zero one to it (and rejects an
        // initialized one); hod does not place a global in such a section.
        for s in &code.sections {
            if matches!(s.kind, SectionKind::Custom { .. })
                && s.kind.is_nobits()
                && module.symbols().any(|sym| sym.section() == Some(s.name.as_bytes()))
            {
                return Err(refused(hod_asm::reason::other::SECTION));
            }
        }
        let sections = self.asm_sections(&code)?;
        self.asm_idents(&code.idents)?;
        let (ids, mut deferred) = self.asm_symbols(&code, &sections)?;
        for v in &code.symvers {
            match ids[v.sym as usize] {
                Some(id) => deferred.symvers.push((id, v.alias.clone())),
                // A `.L` name.
                None if code.symbols[v.sym as usize].is_temporary() => {
                    return Err(refused(SYMBOL));
                }
                // Undefined and unreferenced: nothing is written.
                None => {}
            }
        }
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
                self.obj.add_relocation(id, r.offset, reloc_kind(r.kind, arch), target, addend);
            }
        }
        self.asm_deferred = Some(Box::new(deferred));
        Ok(())
    }

    /// The `.ident` strings in `.comment`, as LLVM's assembler writes them
    /// (`MCELFStreamer::emitIdent`): `SHT_PROGBITS`, `SHF_MERGE |
    /// SHF_STRINGS`, entries of 1 byte, a NUL byte, then each string with
    /// its NUL.
    fn asm_idents(&mut self, idents: &[Vec<u8>]) -> CgResult<()> {
        if idents.is_empty() {
            return Ok(());
        }
        let kind = hod_obj::SectionKind::Elf {
            sh_type: hod_obj::elf::SHT_PROGBITS,
            sh_flags: hod_obj::elf::SHF_MERGE | hod_obj::elf::SHF_STRINGS,
            sh_entsize: 1,
        };
        let id = self.obj.add_section(b".comment", kind, 1).map_err(obj_err)?;
        let mut bytes = vec![0u8];
        for s in idents {
            bytes.extend_from_slice(s);
            bytes.push(0);
        }
        self.obj.append_owned(id, bytes);
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
                // A name of the text's own: its type, flags and entry
                // size as written (`spec/asm.md` §9.1).
                SectionKind::Custom { sh_type, flags, entsize } => {
                    name.clear();
                    name.extend_from_slice(s.name.as_bytes());
                    let id = match self.sections.assembly(&name, sh_type, flags, entsize) {
                        Found::Old(id) => id,
                        Found::New(new, pending) => {
                            let id =
                                self.obj.add_section(&name, new.kind, s.align).map_err(obj_err)?;
                            self.sections.created(&name, &pending, id, None);
                            id
                        }
                    };
                    self.obj.raise_section_align(id, s.align);
                    if s.kind.is_nobits() {
                        self.obj.append_zeros(id, s.size);
                    } else {
                        self.obj.append(id, &s.bytes);
                    }
                    ids.push(id);
                    continue;
                }
            };
            let standard = matches!(s.name.as_str(), ".text" | ".rodata" | ".data" | ".bss");
            let found = if standard {
                self.sections.implicit(kind, false, false, false, NO_GROUP, b"", 1, &mut name)
            } else {
                name.clear();
                name.extend_from_slice(s.name.as_bytes());
                self.sections.explicit(s.name.as_bytes(), kind, false, false, NO_GROUP, 1)
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
        let versioned = |i: usize| code.symvers.iter().any(|v| v.sym as usize == i);
        for s in &code.symbols {
            // Named by `.symver` alone: no directive, no reference.
            let only_versioned = s.def == Def::Undefined
                && !s.referenced
                && s.binding.is_none()
                && s.visibility.is_none()
                && s.ty.is_none()
                && versioned(ids.len());
            let label = match s.def {
                Def::Label(section, offset) => Some((sections[section as usize], offset)),
                _ => None,
            };
            if let Some(sym) = module.find_symbol(s.name.as_bytes()) {
                let entry = if let Some((section, offset)) = label {
                    self.asm_definition(sym, s, section, offset)?
                } else if only_versioned
                    && module.symbol(sym).linkage() == Linkage::Internal
                    && module.symbol(sym).is_definition()
                {
                    // `.symver` of an internal definition: the alias is
                    // local, as LLVM writes it.
                    ids.push(Some(Self::obj_sym(sym)));
                    continue;
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
            // A name only `.symver` gives, undefined and unreferenced:
            // LLVM's assembler writes nothing for it.
            if only_versioned {
                ids.push(None);
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
                    // ELF32's `st_value` is the value modulo 2^32, as
                    // LLVM writes it (`k = -1` is 0xffffffff).
                    value: if module.target().arch() == hod_ir::Arch::I386 {
                        u64::from(value as u32)
                    } else {
                        value as u64
                    },
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
            } else if tls_field(code, ids.len()) {
                // A thread-local field's symbol is `STT_TLS`, as LLVM's
                // ELF writer marks it (32-bit x86 only: no x86-64 or
                // AArch64 kind is one of these).
                symbol.kind = SymbolKind::Tls;
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
        for (id, alias) in &deferred.symvers {
            self.symver(*id, alias)?;
        }
        Ok(())
    }

    /// `.symver name, base@V` (`base@@V`, `base@@@V`), as LLVM's
    /// assembler writes it (`spec/asm.md` §14): for a defined name, a
    /// symbol `base@V` (`base@@V`) at the same place, of the same size,
    /// type, binding and visibility, the name kept; with `@@@`, the name
    /// itself renamed `base@@V`. An undefined name is renamed `base@V`
    /// (`@` and `@@@` alike: what refers to it refers to the version);
    /// `@@` of an undefined name is an error to LLVM, refused as
    /// [`SYMBOL`].
    fn symver(&mut self, id: SymbolId, alias: &str) -> CgResult<()> {
        let at = alias.find('@').ok_or_else(|| CgError::internal("a .symver without @"))?;
        let (base, rest) = alias.split_at(at);
        let version = rest.trim_start_matches('@');
        let ats = rest.len() - version.len();
        let original = *self.obj.symbol(id);
        let defined = matches!(original.place, SymbolPlace::Section(_));
        let mut name = std::mem::take(&mut self.name);
        name.clear();
        name.extend_from_slice(base.as_bytes());
        let rename = match (defined, ats) {
            (true, 1 | 2) => false,
            (true, _) => {
                name.extend_from_slice(b"@@");
                true
            }
            (false, 2) => {
                self.name = name;
                return Err(refused(SYMBOL));
            }
            (false, _) => {
                name.push(b'@');
                true
            }
        };
        if !rename {
            name.extend_from_slice(&rest.as_bytes()[..ats]);
        }
        name.extend_from_slice(version.as_bytes());
        let added = self.obj.add_string(&name).map_err(obj_err);
        self.name = name;
        let added = added?;
        if rename {
            self.obj.symbol_mut(id).name = added;
        } else {
            let alias = self.obj.add_symbol(Symbol { name: added, ..original });
            self.obj.retain_symbol(alias);
        }
        Ok(())
    }
}
