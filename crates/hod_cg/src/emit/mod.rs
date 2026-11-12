//! Module emission (`spec/codegen.md` §9.7): the module's symbols,
//! variables, aliases, IFUNCs, constructors and linker options, and the
//! functions' code merged from chunks, into one `hod_obj::Object` laid out
//! as `llc` lays it out.
//!
//! [`ModuleEmitter::new`] declares every module symbol, in `SymId` order
//! (so that a `SymId` is its `hod_obj::SymbolId`), and places every
//! variable; [`ModuleEmitter::add_chunk`] appends the functions of a chunk,
//! in module order; [`ModuleEmitter::finish`] places aliases and IFUNCs,
//! writes the initializers' relocations, the constructor and destructor
//! tables and the linker options, and defines the symbols. Nothing is
//! allocated per symbol or per function beyond the object's own tables
//! and the merged rows.

mod asm;
mod data;
mod lines;
mod section;
mod unwind;

use std::collections::HashMap;

use hod_ir::{
    ComdatKind, Const, Fixup, FixupTarget, Linkage, Module, SymId, SymbolKind as IrKind, TlsModel,
    TypeKind,
};
use hod_obj::elf::{SHF_EXCLUDE, SHF_MERGE, SHF_STRINGS, SHF_TLS};
use hod_obj::{
    A64Reloc, Arch, Binding, GroupId, GroupKind, Object, RelocKind, SectionId, SectionKind, Symbol,
    SymbolId, SymbolKind, SymbolPlace, Visibility, X64Reloc,
};

use self::data::Relocs;
use self::lines::DebugLines;
use self::section::{Found, Kind, NO_GROUP, Sections};
use crate::access::prefers_local_alias;
use crate::error::{CgError, CgResult};
use crate::options::Options;
use crate::record::{Chunk, FuncRecord};
use crate::sym::{RuntimeFn, SymRef};

/// The construct names module emission rejects (§10).
pub mod unsupported {
    /// A symbol difference in an initializer that no relocation encodes:
    /// the subtracted symbol is neither in the section of the field nor
    /// in the section of the added symbol.
    pub const SYM_DIFF: &str = "init:sym-diff-unencodable";
    /// An address field of a width without an absolute relocation (1
    /// byte).
    pub const FIELD_WIDTH: &str = "init:address-width";
    /// `dso_local_equivalent` outside a difference.
    pub const DSO_LOCAL_EQUIVALENT: &str = "init:dso-local-equivalent";
    /// A `blockaddress` of a block that has no code offset (§9.6, §13).
    pub const BLOCK_ADDRESS: &str = "init:blockaddress-unplaced";
    /// A non-zero initializer in a section that has no file space.
    pub const NOBITS_INIT: &str = "global:initializer-in-bss";
    /// An alias whose aliasee is not a symbol plus an offset, or is a
    /// declaration.
    pub const ALIAS_TARGET: &str = "global:alias-target";
    /// An IFUNC whose resolver is not defined here.
    pub const IFUNC_RESOLVER: &str = "global:ifunc-resolver";
    /// A COMDAT selection kind ELF has no group for.
    pub const COMDAT_KIND: &str = "comdat:selection-kind";
    /// A variable of `appending` linkage that is not one of the special
    /// arrays `hod_ir` reads.
    pub const APPENDING: &str = "linkage:appending";
    /// Module-level assembly an assembler must read (`hod_asm`'s, not
    /// yet): anything but the symbol directives of `spec/ir.md` §2.3.
    pub const MODULE_ASM: &str = "module:asm";
    /// An `!llvm.linker.options` entry that is not a pair of strings,
    /// which ELF `llc` refuses ("invalid llvm.linker.options").
    pub const LINKER_OPTION: &str = "module:linker-option-not-a-pair";
    /// A compile unit whose `emissionKind` is `LineTablesOnly` or
    /// `DebugDirectivesOnly`: their DIEs (a unit and its subprograms
    /// only) are not written yet (`spec/dwarf.md` §9).
    pub const DEBUG_EMISSION_KIND: &str = "debug:emission-kind";
    /// A compile unit with a node the DIE writer does not write yet
    /// (`spec/dwarf.md` §6.5).
    pub const DEBUG_DIE: &str = "debug:die-unsupported";
    /// A function with an unwind-table entry, no landing pads and a
    /// personality LLVM does not know: `llc` gives it the personality and
    /// an LSDA of call sites hod does not record (`spec/eh.md` §1.1).
    pub const PERSONALITY_WITHOUT_PADS: &str = "eh:unknown-personality-without-pads";
}

/// Not in a section.
const NOWHERE: u32 = u32::MAX;

/// A symbol's definition, until [`ModuleEmitter::finish`] writes it.
#[derive(Clone, Copy, Debug)]
struct Place {
    /// The section index, or [`NOWHERE`].
    section: u32,
    /// Offset in the section, or the alignment of a common symbol.
    value: u64,
    size: u64,
    kind: SymbolKind,
    common: bool,
}

impl Place {
    const NONE: Place =
        Place { section: NOWHERE, value: 0, size: 0, kind: SymbolKind::NoType, common: false };

    fn is_placed(self) -> bool {
        self.section != NOWHERE || self.common
    }
}

/// A function placed in the object.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlacedFunc {
    /// Its record. `code` is its byte range in `section`; the row ranges
    /// are ranges of [`Emitted::rows`]; `relocs` is empty (the relocations
    /// are in the object).
    pub record: FuncRecord,
    /// The section its code is in.
    pub section: SectionId,
}

/// What module emission produces.
#[derive(Debug)]
pub struct Emitted {
    /// The object: every section, symbol and relocation of the module.
    pub object: Object,
    /// The functions, in the order they were placed (module order).
    pub funcs: Vec<PlacedFunc>,
    /// The rows of every function, merged: the tables `hod_eh` and
    /// `hod_dwarf` read. `code`, `relocs` and `funcs` are empty.
    pub rows: Chunk,
}

/// What a COMDAT became.
#[derive(Clone, Copy, Debug)]
struct Group {
    index: u32,
    /// The signature symbol made for a COMDAT no module symbol names, or
    /// [`NOWHERE`].
    signature: u32,
    /// Its first member section, where that symbol is defined.
    first: u32,
}

/// Module emission in progress (§9.7).
#[derive(Debug)]
pub struct ModuleEmitter<'m> {
    module: &'m Module,
    options: Options,
    obj: Object,
    sections: Sections,
    /// Scratch: a section name.
    name: Vec<u8>,
    places: Vec<Place>,
    groups: Vec<Group>,
    runtime: Vec<u32>,
    local_aliases: HashMap<SymId, SymbolId>,
    /// The group of each signature symbol: `MCContext` has one group per
    /// signature name, whether a COMDAT or an associated structor made it.
    signatures: HashMap<SymbolId, GroupId>,
    /// Code relocations whose target is placed later: section, offset,
    /// kind, target, addend.
    pending: Vec<(SectionId, u64, RelocKind, SymRef, i64)>,
    /// The initializers' fixups, by section, with section offsets.
    fixups: Vec<(SectionId, Fixup)>,
    /// Scratch: one initializer's image and fixups.
    image: Vec<u8>,
    image_fixups: Vec<Fixup>,
    /// The mapping state of each code section: 0 none, 1 code, 2 data.
    mapping: Vec<u8>,
    mapping_names: [hod_obj::StrId; 2],
    funcs: Vec<PlacedFunc>,
    rows: Chunk,
    /// The line tables and DIEs, when the module has a unit to write.
    debug: Option<Box<DebugLines<'m>>>,
}

fn obj_err(error: hod_obj::Error) -> CgError {
    CgError::internal(format!("object: {error}"))
}

fn binding(linkage: Linkage) -> Binding {
    match linkage {
        Linkage::Internal | Linkage::Private => Binding::Local,
        Linkage::LinkOnceAny
        | Linkage::LinkOnceOdr
        | Linkage::WeakAny
        | Linkage::WeakOdr
        | Linkage::ExternWeak => Binding::Weak,
        _ => Binding::Global,
    }
}

fn visibility(visibility: hod_ir::Visibility) -> Visibility {
    match visibility {
        hod_ir::Visibility::Default => Visibility::Default,
        hod_ir::Visibility::Hidden => Visibility::Hidden,
        hod_ir::Visibility::Protected => Visibility::Protected,
    }
}

/// Whether the linker merges or the thread model owns a section's
/// contents, so that a relocation must name the symbol rather than the
/// section (`ELFObjectWriter::shouldRelocateWithSymbol`).
fn merge_and_tls(kind: SectionKind) -> (bool, bool) {
    match kind {
        SectionKind::MergeableStrings { .. } | SectionKind::MergeableConstants { .. } => {
            (true, false)
        }
        SectionKind::Tls | SectionKind::TlsBss => (false, true),
        SectionKind::Elf { sh_flags, .. } => (sh_flags & SHF_MERGE != 0, sh_flags & SHF_TLS != 0),
        _ => (false, false),
    }
}

/// The arch-specific parts of emission.
#[derive(Clone, Copy, Debug)]
struct ArchInfo {
    arch: Arch,
}

impl ArchInfo {
    /// The absolute relocation of an address field of `size` bytes.
    fn abs(self, size: u8) -> Option<RelocKind> {
        Some(match (self.arch, size) {
            (Arch::AArch64, 8) => A64Reloc::Abs64.into(),
            (Arch::AArch64, 4) => A64Reloc::Abs32.into(),
            (Arch::AArch64, 2) => A64Reloc::Abs16.into(),
            (Arch::X86_64, 8) => X64Reloc::Abs64.into(),
            (Arch::X86_64, 4) => X64Reloc::Abs32.into(),
            (Arch::X86_64, 2) => X64Reloc::Abs16.into(),
            _ => return None,
        })
    }

    /// The PC-relative relocation of a field of `size` bytes.
    fn prel(self, size: u8) -> Option<RelocKind> {
        Some(match (self.arch, size) {
            (Arch::AArch64, 8) => A64Reloc::Prel64.into(),
            (Arch::AArch64, 4) => A64Reloc::Prel32.into(),
            (Arch::AArch64, 2) => A64Reloc::Prel16.into(),
            (Arch::X86_64, 8) => X64Reloc::Pc64.into(),
            (Arch::X86_64, 4) => X64Reloc::Pc32.into(),
            (Arch::X86_64, 2) => X64Reloc::Pc16.into(),
            _ => return None,
        })
    }

    /// `R_*_PLT32`.
    fn plt32(self) -> RelocKind {
        match self.arch {
            Arch::AArch64 => A64Reloc::Plt32.into(),
            Arch::X86_64 => X64Reloc::Plt32.into(),
        }
    }

    /// The alignment of `.text` before any code is in it.
    fn text_align(self) -> u64 {
        match self.arch {
            Arch::AArch64 => 4,
            Arch::X86_64 => 1,
        }
    }

    /// Whether the target marks code and data in code with mapping
    /// symbols (`$x`, `$d`).
    fn has_mapping_symbols(self) -> bool {
        self.arch == Arch::AArch64
    }

    /// Pads code to `align`: with `nop`s, as `llc`'s assembler does.
    fn pad_code(self, obj: &mut Object, section: SectionId, align: u64) {
        const A64_NOPS: [u8; 64] = {
            let mut nops = [0u8; 64];
            let mut i = 0;
            while i < 64 {
                nops[i] = [0x1f, 0x20, 0x03, 0xd5][i % 4];
                i += 1;
            }
            nops
        };
        let size = obj.section_size(section);
        let mut pad = (size.wrapping_neg() & (align - 1)) as usize;
        obj.raise_section_align(section, align);
        if self.arch == Arch::AArch64 && size.is_multiple_of(4) {
            while pad > 0 {
                let n = pad.min(A64_NOPS.len());
                obj.append(section, &A64_NOPS[..n]);
                pad -= n;
            }
        } else {
            obj.align_to(section, align, if self.arch == Arch::X86_64 { 0x90 } else { 0 });
        }
    }
}

impl<'m> ModuleEmitter<'m> {
    /// Declares every symbol of `module` and places its variables.
    ///
    /// # Errors
    /// [`CgError::Unsupported`] for a variable this emission cannot lay
    /// out (see [`unsupported`]); an internal error if the object refuses
    /// a name.
    pub fn new(module: &'m Module, options: &Options) -> CgResult<ModuleEmitter<'m>> {
        let arch = match module.target().arch() {
            hod_ir::Arch::Aarch64 => Arch::AArch64,
            hod_ir::Arch::X86_64 => Arch::X86_64,
        };
        let mut obj = Object::new(arch);
        let count = module.symbol_count();
        obj.reserve(count / 2 + 16, count + 16, count, count * 16);
        let mapping_names =
            [obj.add_string(b"$x").map_err(obj_err)?, obj.add_string(b"$d").map_err(obj_err)?];
        let mut emitter = ModuleEmitter {
            module,
            options: *options,
            obj,
            sections: Sections::new(),
            name: Vec::new(),
            places: vec![Place::NONE; count],
            groups: Vec::new(),
            runtime: vec![NOWHERE; RuntimeFn::ALL.len()],
            local_aliases: HashMap::new(),
            signatures: HashMap::new(),
            pending: Vec::new(),
            fixups: Vec::new(),
            image: Vec::new(),
            image_fixups: Vec::new(),
            mapping: Vec::new(),
            mapping_names,
            funcs: Vec::new(),
            rows: Chunk::new(),
            debug: None,
        };
        asm::check(module)?;
        // `TargetLoweringObjectFileELF::emitModuleMetadata`.
        if module.linker_options().iter().any(|list| list.len() != 2) {
            return Err(CgError::unsupported(unsupported::LINKER_OPTION));
        }
        emitter.declare()?;
        emitter.debug = DebugLines::new(module, arch)?;
        // `.text` exists in every object `llc` writes.
        let text = emitter.arch().text_align();
        let mut name = std::mem::take(&mut emitter.name);
        match emitter.sections.implicit(Kind::Text, false, false, NO_GROUP, b"", 1, &mut name) {
            Found::New(new, pending) => {
                let id = emitter.obj.add_section(&name, new.kind, text).map_err(obj_err)?;
                emitter.sections.created(&name, &pending, id, Some((Kind::Text, 1)));
            }
            Found::Old(_) => {}
        }
        emitter.name = name;
        for sym in module.symbols() {
            if let IrKind::Variable { constant, tls, init: Some(init) } = sym.kind() {
                emitter.place_variable(sym.id(), constant, tls, init)?;
            }
        }
        Ok(emitter)
    }

    fn arch(&self) -> ArchInfo {
        ArchInfo { arch: self.obj.arch() }
    }

    fn obj_sym(sym: SymId) -> SymbolId {
        SymbolId::from_index(sym.index() as u32)
    }

    /// Adds every module symbol as undefined, in `SymId` order, with the
    /// name `llc` gives it: `.L` before a private name, `__unnamed_<n>` for
    /// an anonymous one.
    fn declare(&mut self) -> CgResult<()> {
        let mut anonymous = 0u32;
        let mut name = std::mem::take(&mut self.name);
        for sym in self.module.symbols() {
            name.clear();
            if sym.linkage() == Linkage::Private {
                name.extend_from_slice(b".L");
            }
            if sym.is_anonymous() {
                anonymous += 1;
                let _ = std::io::Write::write_fmt(&mut name, format_args!("__unnamed_{anonymous}"));
            } else {
                name.extend_from_slice(sym.name());
            }
            // A declaration or a named private symbol is named in
            // `define`, and only if the object keeps it: the string table
            // holds no name of a symbol the writer leaves out.
            let lazy =
                !sym.is_definition() || (sym.linkage() == Linkage::Private && !sym.is_anonymous());
            let id = if lazy {
                hod_obj::StrId::EMPTY
            } else {
                self.obj.add_string(&name).map_err(obj_err)?
            };
            let mut symbol = Symbol::undefined(id).with_visibility(visibility(sym.visibility()));
            if sym.linkage() == Linkage::ExternWeak {
                symbol.binding = Binding::Weak;
            }
            if sym.tls() != TlsModel::None {
                symbol.kind = SymbolKind::Tls;
            }
            self.obj.add_symbol(symbol);
        }
        self.name = name;
        Ok(())
    }

    /// The section for `sym`, of `kind` and alignment `align`, created if
    /// it does not exist (`TargetLoweringObjectFileELF`).
    fn select(&mut self, sym: SymId, kind: Kind, align: u64) -> CgResult<SectionId> {
        let symbol = self.module.symbol(sym);
        let retain = symbol.is_used();
        let comdat = symbol.comdat();
        let group = comdat.map_or(NO_GROUP, |c| c.index() as u32);
        let explicit = symbol.section();
        let found = if let Some(name) = explicit {
            self.sections.explicit(name, kind, retain, group, align)
        } else {
            let mergeable = kind.flags() & SHF_MERGE != 0;
            let split = if kind == Kind::Text {
                self.options.function_sections
            } else {
                self.options.data_sections
            };
            let unique = group != NO_GROUP || (!mergeable && split);
            let own = self.obj.string(self.obj.symbol(Self::obj_sym(sym)).name);
            self.sections.implicit(kind, unique, retain, group, own, align, &mut self.name)
        };
        let (new, pending) = match found {
            Found::Old(section) => return Ok(section),
            Found::New(new, pending) => (new, pending),
        };
        let name: &[u8] = explicit.unwrap_or(&self.name);
        let section = self.obj.add_section(name, new.kind, 1).map_err(obj_err)?;
        let cache = explicit.is_none().then_some((kind, align));
        self.sections.created(name, &pending, section, cache);
        if let Some(comdat) = comdat {
            let group = self.group(comdat, section)?;
            self.obj.add_group_member(group, section).map_err(obj_err)?;
        }
        Ok(section)
    }

    /// The group of a COMDAT, made on its first member `section`.
    fn group(&mut self, comdat: hod_ir::ComdatId, section: SectionId) -> CgResult<GroupId> {
        let index = comdat.index();
        if self.groups.len() <= index {
            self.groups.resize(
                self.module.comdat_count(),
                Group { index: NOWHERE, signature: NOWHERE, first: NOWHERE },
            );
        }
        if self.groups[index].index != NOWHERE {
            return Ok(GroupId::from_index(self.groups[index].index));
        }
        let comdat = self.module.comdat(comdat);
        let kind = match comdat.kind {
            ComdatKind::Any => GroupKind::Comdat,
            ComdatKind::NoDeduplicate => GroupKind::Plain,
            _ => return Err(CgError::unsupported(unsupported::COMDAT_KIND)),
        };
        let (signature, made) = if let Some(sym) = self.module.find_symbol(comdat.name) {
            (Self::obj_sym(sym), NOWHERE)
        } else {
            let name = self.obj.add_string(comdat.name).map_err(obj_err)?;
            let id = self.obj.add_symbol(Symbol::undefined(name));
            (id, id.index())
        };
        let group = self.signature_group(signature, kind);
        self.groups[index] =
            Group { index: group.index(), signature: made, first: section.index() };
        Ok(group)
    }

    /// The group whose signature is `signature`, made on first use.
    fn signature_group(&mut self, signature: SymbolId, kind: GroupKind) -> GroupId {
        if let Some(&group) = self.signatures.get(&signature) {
            return group;
        }
        let group = self.obj.add_group(signature, kind);
        self.signatures.insert(signature, group);
        group
    }

    /// `getKindForGlobal` and the placement of one variable.
    fn place_variable(
        &mut self,
        sym: SymId,
        constant: bool,
        tls: TlsModel,
        init: hod_ir::ConstId,
    ) -> CgResult<()> {
        let module = self.module;
        let symbol = module.symbol(sym);
        let linkage = symbol.linkage();
        if linkage == Linkage::AvailableExternally || symbol.section() == Some(b"llvm.metadata") {
            return Ok(());
        }
        if linkage == Linkage::Appending {
            return Err(CgError::unsupported(unsupported::APPENDING));
        }
        let ty = symbol.ty();
        let has_section = symbol.section().is_some();
        let size = module.types.alloc_size_of(ty);
        let align = module.types.global_align(ty, symbol.align(), has_section);
        if linkage == Linkage::Common {
            self.places[sym.index()] = Place {
                section: NOWHERE,
                value: align,
                size: size.max(1),
                kind: SymbolKind::Object,
                common: true,
            };
            return Ok(());
        }
        let zero = data::is_bss_suitable(module, init);
        let local = matches!(linkage, Linkage::Internal | Linkage::Private);
        let mut kind = if tls != TlsModel::None {
            if zero && !constant && !has_section { Kind::ThreadBss } else { Kind::ThreadData }
        } else if zero && !constant && !has_section {
            Kind::Bss { local }
        } else {
            Kind::Data
        };
        if !kind.is_nobits() {
            self.image.clear();
            self.image_fixups.clear();
            module
                .consts
                .flatten(&module.types, init, &mut self.image, &mut self.image_fixups)
                .map_err(|e| CgError::internal(format!("initializer: {e}")))?;
            if constant && tls == TlsModel::None {
                kind = match data::reloc_info(module, &self.image_fixups) {
                    Relocs::None if symbol.unnamed_addr() != hod_ir::UnnamedAddr::Global => {
                        Kind::ReadOnly
                    }
                    Relocs::None => match data::cstring_char_size(module, ty, init) {
                        Some(char_size) => Kind::CString(char_size),
                        None => match size {
                            4 | 8 | 16 | 32 => Kind::Const(size as u8),
                            _ => Kind::ReadOnly,
                        },
                    },
                    _ if !self.options.reloc_model.is_pic() => Kind::ReadOnly,
                    Relocs::Local => Kind::ReadOnly,
                    Relocs::Global => Kind::ReadOnlyWithRel,
                };
            }
        }
        if let Some(name) = symbol.section() {
            let named = section::named_kind(name, kind);
            if named.is_nobits() && !kind.is_nobits() && !zero {
                return Err(CgError::unsupported(unsupported::NOBITS_INIT));
            }
            kind = named;
        }
        let section = self.select(sym, kind, align)?;
        let mut size = size;
        if kind == (Kind::Bss { local: true })
            && size == 0
            && self.sections.generic(Kind::Bss { local: true }) == Some(section)
        {
            // `.local` and `.comm` of zero bytes: one byte.
            size = 1;
        }
        self.obj.raise_section_align(section, align);
        let offset = self.obj.align_to(section, align, 0);
        if kind.is_nobits() {
            self.obj.append_zeros(section, size);
        } else {
            self.obj.append(section, &self.image);
            self.fixups.extend(
                self.image_fixups
                    .iter()
                    .map(|fixup| (section, Fixup { offset: fixup.offset + offset, ..*fixup })),
            );
        }
        let tls_section = merge_and_tls(self.obj.section_kind(section)).1;
        self.places[sym.index()] = Place {
            section: section.index(),
            value: offset,
            size,
            kind: if tls_section { SymbolKind::Tls } else { SymbolKind::Object },
            common: false,
        };
        Ok(())
    }

    /// Appends the functions of `chunk`, in its order. Chunks are added in
    /// module order (§9.7).
    ///
    /// # Errors
    /// An internal error for a record of a symbol that is not a function
    /// with a body, or one placed twice.
    pub fn add_chunk(&mut self, chunk: &Chunk) -> CgResult<()> {
        for func in &chunk.funcs {
            self.add_func(chunk, func)?;
        }
        Ok(())
    }

    fn add_func(&mut self, chunk: &Chunk, func: &FuncRecord) -> CgResult<()> {
        let symbol = self.module.symbol(func.sym);
        if !matches!(symbol.kind(), IrKind::Function { has_body: true, .. }) {
            return Err(CgError::internal("a function record of a symbol without a body"));
        }
        if self.places[func.sym.index()].is_placed() {
            return Err(CgError::internal("a function placed twice"));
        }
        if symbol.linkage() == Linkage::AvailableExternally {
            return Ok(());
        }
        let arch = self.arch();
        let section = self.select(func.sym, Kind::Text, u64::from(func.align))?;
        arch.pad_code(&mut self.obj, section, u64::from(func.align.max(1)));
        let code = chunk.code_of(func);
        let offset = self.obj.append(section, code);
        let len = code.len() as u64;
        if arch.has_mapping_symbols() && len > 0 {
            self.mapping_symbols(chunk, func, section, offset, len);
        }
        for reloc in &chunk.relocs[func.relocs.start as usize..func.relocs.end as usize] {
            let at = offset + u64::from(reloc.offset);
            match self.code_target(reloc.sym, reloc.addend) {
                Some((target, addend)) => {
                    self.obj.add_relocation(section, at, reloc.kind, target, addend);
                }
                None => self.pending.push((section, at, reloc.kind, reloc.sym, reloc.addend)),
            }
        }
        self.places[func.sym.index()] = Place {
            section: section.index(),
            value: offset,
            size: len,
            kind: SymbolKind::Func,
            common: false,
        };
        if let Some(debug) = &mut self.debug {
            let lines = &chunk.lines[func.lines.start as usize..func.lines.end as usize];
            let vars = &chunk.vars[func.vars.start as usize..func.vars.end as usize];
            let placed = lines::Placed { section, offset, size: len as u32, frame: func.frame };
            debug.add_function(self.module, &mut self.obj, func.sym, lines, vars, placed)?;
        }
        let record = self.merge_rows(chunk, func, offset);
        self.funcs.push(PlacedFunc { record, section });
        Ok(())
    }

    /// `$x` where code starts and after each island, `$d` at each island:
    /// a mapping symbol where the state changes, as `llc` emits them.
    fn mapping_symbols(
        &mut self,
        chunk: &Chunk,
        func: &FuncRecord,
        section: SectionId,
        offset: u64,
        len: u64,
    ) {
        let index = section.index() as usize;
        if self.mapping.len() <= index {
            self.mapping.resize(index + 1, 0);
        }
        let mut state = self.mapping[index];
        let mut mark = |obj: &mut Object, names: &[hod_obj::StrId; 2], at: u64, to: u8| {
            if state != to {
                let symbol =
                    Symbol::defined(names[usize::from(to - 1)], SymbolKind::NoType, section, at, 0)
                        .with_binding(Binding::Local);
                obj.add_symbol(symbol);
                state = to;
            }
        };
        let mut at = 0u64;
        for island in &chunk.islands[func.islands.start as usize..func.islands.end as usize] {
            let start = u64::from(island.offset);
            if start > at {
                mark(&mut self.obj, &self.mapping_names, offset + at, 1);
            }
            if island.len > 0 {
                mark(&mut self.obj, &self.mapping_names, offset + start, 2);
            }
            at = start + u64::from(island.len);
        }
        if at < len {
            mark(&mut self.obj, &self.mapping_names, offset + at, 1);
        }
        self.mapping[index] = state;
    }

    /// Copies a function's rows into the merged tables.
    fn merge_rows(&mut self, chunk: &Chunk, func: &FuncRecord, offset: u64) -> FuncRecord {
        fn copy<T: Clone>(
            to: &mut Vec<T>,
            from: &[T],
            range: &std::ops::Range<u32>,
        ) -> std::ops::Range<u32> {
            let start = to.len() as u32;
            to.extend_from_slice(&from[range.start as usize..range.end as usize]);
            start..to.len() as u32
        }
        let rows = &mut self.rows;
        let len = func.code.end - func.code.start;
        FuncRecord {
            sym: func.sym,
            align: func.align,
            frame: func.frame,
            has_pads: func.has_pads,
            personality: func.personality,
            nounwind: func.nounwind,
            uwtable: func.uwtable,
            code: offset as u32..offset as u32 + len,
            relocs: 0..0,
            cfi: copy(&mut rows.cfi, &chunk.cfi, &func.cfi),
            call_sites: copy(&mut rows.call_sites, &chunk.call_sites, &func.call_sites),
            pads: copy(&mut rows.pads, &chunk.pads, &func.pads),
            clauses: copy(&mut rows.clauses, &chunk.clauses, &func.clauses),
            type_ids: copy(&mut rows.type_ids, &chunk.type_ids, &func.type_ids),
            filter_ids: copy(&mut rows.filter_ids, &chunk.filter_ids, &func.filter_ids),
            lines: copy(&mut rows.lines, &chunk.lines, &func.lines),
            vars: copy(&mut rows.vars, &chunk.vars, &func.vars),
            islands: copy(&mut rows.islands, &chunk.islands, &func.islands),
            block_offsets: copy(&mut rows.block_offsets, &chunk.block_offsets, &func.block_offsets),
        }
    }

    /// The symbol and addend a code relocation names.
    fn code_target(&mut self, sym: SymRef, addend: i64) -> Option<(SymbolId, i64)> {
        let sym = match sym {
            SymRef::Runtime(func) => return Some((self.runtime_symbol(func), addend)),
            SymRef::Module(sym) => sym,
        };
        let local_alias = prefers_local_alias(self.module, self.options, sym);
        if !local_alias && binding(self.module.symbol(sym).linkage()) != Binding::Local {
            return Some((Self::obj_sym(sym), addend));
        }
        // A function, alias or IFUNC not placed yet: decided in `finish`,
        // when everything is.
        if !self.places[sym.index()].is_placed() {
            return None;
        }
        Some(self.local_target(sym, addend, local_alias))
    }

    /// The target of a reference to a local symbol, or to one reached
    /// through its local alias: the section if a relocation may name it,
    /// else the symbol or its alias.
    fn local_target(&mut self, sym: SymId, addend: i64, local_alias: bool) -> (SymbolId, i64) {
        if let Some(target) = self.via_section(sym, addend) {
            return target;
        }
        if local_alias {
            return (self.local_alias(sym), addend);
        }
        (Self::obj_sym(sym), addend)
    }

    /// The section symbol and the addend that reach `sym` + `addend`, when
    /// a relocation may name the section rather than the symbol.
    fn via_section(&mut self, sym: SymId, addend: i64) -> Option<(SymbolId, i64)> {
        let place = self.places[sym.index()];
        if place.section == NOWHERE || place.kind == SymbolKind::GnuIfunc {
            return None;
        }
        let section = SectionId::from_index(place.section);
        let (merge, tls) = merge_and_tls(self.obj.section_kind(section));
        if tls || (merge && addend != 0) {
            return None;
        }
        Some((self.obj.section_symbol(section), addend.wrapping_add(place.value as i64)))
    }

    /// The symbol and addend a data relocation names. Everything is
    /// placed by now, functions included. A symbol `llc` would reach
    /// through its local alias (`AsmPrinter::lowerConstant` uses
    /// `getSymbolPreferLocal`) is reached through its section, except in a
    /// difference (`lowerRelativeReference` names the symbol itself):
    /// `prefer_local` is false there.
    fn data_target(&mut self, sym: SymId, addend: i64, prefer_local: bool) -> (SymbolId, i64) {
        let local_alias = prefer_local && prefers_local_alias(self.module, self.options, sym);
        if local_alias || binding(self.module.symbol(sym).linkage()) == Binding::Local {
            return self.local_target(sym, addend, local_alias);
        }
        (Self::obj_sym(sym), addend)
    }

    /// `.L<name>$local`: what code in a shared object refers to a
    /// preemptible-looking definition by (`getSymbolPreferLocal`).
    fn local_alias(&mut self, sym: SymId) -> SymbolId {
        if let Some(&id) = self.local_aliases.get(&sym) {
            return id;
        }
        self.name.clear();
        self.name.extend_from_slice(b".L");
        self.name.extend_from_slice(self.module.symbol(sym).name());
        self.name.extend_from_slice(b"$local");
        let name = self.obj.add_string(&self.name).unwrap_or(hod_obj::StrId::EMPTY);
        let id = self.obj.add_symbol(Symbol::undefined(name));
        self.local_aliases.insert(sym, id);
        id
    }

    /// The symbol of a runtime function: the module's own symbol of that
    /// name, else a new undefined one (§7.3).
    fn runtime_symbol(&mut self, func: RuntimeFn) -> SymbolId {
        let slot = &mut self.runtime[func as usize];
        if *slot == NOWHERE {
            let name = func.name().as_bytes();
            *slot = if let Some(sym) = self.module.find_symbol(name) {
                sym.index() as u32
            } else {
                let name = self.obj.add_string(name).unwrap_or(hod_obj::StrId::EMPTY);
                self.obj.add_symbol(Symbol::undefined(name)).index()
            };
        }
        SymbolId::from_index(*slot)
    }

    /// Finishes the object: aliases and IFUNCs, the initializers'
    /// relocations, constructors and destructors, linker options, and
    /// every symbol's definition.
    ///
    /// # Errors
    /// An internal error if a function with a body was never placed;
    /// [`CgError::Unsupported`] for an alias, IFUNC or initializer this
    /// emission cannot express (see [`unsupported`]).
    pub fn finish(mut self) -> CgResult<Emitted> {
        let module = self.module;
        for symbol in module.symbols() {
            match symbol.kind() {
                IrKind::Function { has_body: true, .. }
                    if symbol.linkage() != Linkage::AvailableExternally
                        && !self.places[symbol.id().index()].is_placed() =>
                {
                    return Err(CgError::internal(format!(
                        "function {} has a body but no code",
                        String::from_utf8_lossy(symbol.name())
                    )));
                }
                IrKind::Alias { .. } => {
                    self.place_alias(symbol.id(), 0)?;
                }
                IrKind::IFunc { resolver } => {
                    let place = self.places[resolver.index()];
                    if place.section == NOWHERE || place.kind != SymbolKind::Func {
                        return Err(CgError::unsupported(unsupported::IFUNC_RESOLVER));
                    }
                    self.places[symbol.id().index()] =
                        Place { kind: SymbolKind::GnuIfunc, ..place };
                }
                _ => {}
            }
        }
        for (section, at, kind, target, addend) in std::mem::take(&mut self.pending) {
            let SymRef::Module(sym) = target else { continue };
            let local_alias = prefers_local_alias(module, self.options, sym);
            let (target, addend) = self.local_target(sym, addend, local_alias);
            self.obj.add_relocation(section, at, kind, target, addend);
        }
        self.relocate_data()?;
        self.structors(true)?;
        self.structors(false)?;
        self.linker_options()?;
        self.unwind_tables()?;
        self.debug_tables()?;
        self.define()?;
        self.module_asm_symbols()?;
        let ModuleEmitter { obj, funcs, rows, .. } = self;
        Ok(Emitted { object: obj, funcs, rows })
    }

    /// The line tables and DIEs (`spec/dwarf.md` §9), after the
    /// functions are placed.
    fn debug_tables(&mut self) -> CgResult<()> {
        let Some(mut debug) = self.debug.take() else { return Ok(()) };
        let module = self.module;
        let places = &self.places;
        debug.finish(module, &mut self.obj, |obj, sym| {
            let place = places[sym.index()];
            if !place.is_placed() {
                return None;
            }
            let section = (place.section != NOWHERE).then(|| SectionId::from_index(place.section));
            let linkage = binding(module.symbol(sym).linkage());
            Some(lines::variable_address(obj, linkage, section, place.value, Self::obj_sym(sym)))
        })
    }

    /// Places an alias at its aliasee plus offset.
    fn place_alias(&mut self, sym: SymId, depth: u32) -> CgResult<Place> {
        let place = self.places[sym.index()];
        if place.is_placed() {
            return Ok(place);
        }
        let module = self.module;
        let symbol = module.symbol(sym);
        let IrKind::Alias { target, .. } = symbol.kind() else { return Ok(place) };
        let Const::Addr { sym: base, addend } = module.consts.get(target) else {
            return Err(CgError::unsupported(unsupported::ALIAS_TARGET));
        };
        if depth > 64 {
            return Err(CgError::unsupported(unsupported::ALIAS_TARGET));
        }
        let base_place = self.place_alias(base, depth + 1)?;
        if base_place.section == NOWHERE {
            return Err(CgError::unsupported(unsupported::ALIAS_TARGET));
        }
        // The object at the end of a chain of aliases.
        let mut object = base;
        while let IrKind::Alias { target, .. } = module.symbol(object).kind() {
            match module.consts.get(target) {
                Const::Addr { sym, .. } => object = sym,
                _ => break,
            }
        }
        let value_ty = symbol.ty();
        let function = matches!(module.types.kind(value_ty), TypeKind::Function { .. });
        let size = if module.symbol(object).linkage() == Linkage::Private
            && module.types.is_sized(value_ty)
        {
            module.types.alloc_size_of(value_ty)
        } else {
            base_place.size
        };
        let kind = if function || base_place.kind == SymbolKind::Func {
            SymbolKind::Func
        } else {
            base_place.kind
        };
        let place = Place {
            section: base_place.section,
            value: base_place.value.wrapping_add(addend as u64),
            size,
            kind,
            common: false,
        };
        self.places[sym.index()] = place;
        Ok(place)
    }

    /// Turns the initializers' fixups into relocations, or into numbers
    /// where the distance is known.
    fn relocate_data(&mut self) -> CgResult<()> {
        let fixups = std::mem::take(&mut self.fixups);
        let arch = self.arch();
        let mut blocks: Vec<(u32, u32, u32)> = Vec::new();
        if fixups.iter().any(|(_, f)| matches!(f.target, FixupTarget::Block { .. })) {
            for func in &self.funcs {
                let rows = &self.rows.block_offsets[func.record.block_offsets.start as usize
                    ..func.record.block_offsets.end as usize];
                blocks.extend(rows.iter().map(|row| {
                    (func.record.sym.index() as u32, row.block.index() as u32, row.offset)
                }));
            }
            blocks.sort_unstable();
        }
        for &(section, fixup) in &fixups {
            let place = fixup.offset;
            let width = fixup.size;
            let abs = || arch.abs(width).ok_or(CgError::unsupported(unsupported::FIELD_WIDTH));
            match fixup.target {
                FixupTarget::Sym(sym) => {
                    let kind = abs()?;
                    let (target, addend) = self.data_target(sym, fixup.addend, true);
                    self.obj.add_relocation(section, place, kind, target, addend);
                }
                FixupTarget::Plt(_) => {
                    return Err(CgError::unsupported(unsupported::DSO_LOCAL_EQUIVALENT));
                }
                FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
                    let plt = matches!(fixup.target, FixupTarget::PltDiff { .. })
                        && !self.module.symbol(a).dso_local();
                    let (pa, pb) = (self.places[a.index()], self.places[b.index()]);
                    if !plt && pa.section != NOWHERE && pa.section == pb.section {
                        let value =
                            pa.value.wrapping_sub(pb.value).wrapping_add(fixup.addend as u64);
                        let bytes = value.to_le_bytes();
                        let data = self.obj.data_mut(section);
                        data[place as usize..][..usize::from(width)]
                            .copy_from_slice(&bytes[..usize::from(width)]);
                        continue;
                    }
                    if pb.section != section.index() {
                        return Err(CgError::unsupported(unsupported::SYM_DIFF));
                    }
                    let addend = fixup.addend.wrapping_add(place.wrapping_sub(pb.value) as i64);
                    if plt {
                        if width != 4 {
                            return Err(CgError::unsupported(unsupported::FIELD_WIDTH));
                        }
                        let target = Self::obj_sym(a);
                        self.obj.add_relocation(section, place, arch.plt32(), target, addend);
                    } else {
                        let kind = arch
                            .prel(width)
                            .ok_or(CgError::unsupported(unsupported::FIELD_WIDTH))?;
                        let (target, addend) = self.data_target(a, addend, false);
                        self.obj.add_relocation(section, place, kind, target, addend);
                    }
                }
                FixupTarget::Block { func, block } => {
                    let kind = abs()?;
                    let key = (func.index() as u32, block.index() as u32);
                    let found = blocks
                        .binary_search_by(|&(f, b, _)| (f, b).cmp(&key))
                        .map_err(|_| CgError::unsupported(unsupported::BLOCK_ADDRESS))?;
                    let fplace = self.places[func.index()];
                    let fsection = SectionId::from_index(fplace.section);
                    let target = self.obj.section_symbol(fsection);
                    let addend = fixup
                        .addend
                        .wrapping_add((fplace.value + u64::from(blocks[found].2)) as i64);
                    self.obj.add_relocation(section, place, kind, target, addend);
                }
            }
        }
        self.fixups = fixups;
        Ok(())
    }

    /// `.init_array` or `.fini_array`: one section per priority and
    /// associated symbol, entries in stable priority order
    /// (`AsmPrinter::emitXXStructorList`).
    fn structors(&mut self, ctors: bool) -> CgResult<()> {
        let module = self.module;
        let list = if ctors { module.ctors() } else { module.dtors() };
        if list.is_empty() {
            return Ok(());
        }
        let mut order: Vec<u32> = (0..list.len() as u32).collect();
        order.sort_by_key(|&i| list[i as usize].priority);
        let arch = self.arch();
        let abs = arch.abs(8).expect("both targets have a 64-bit absolute relocation");
        let mut sections: HashMap<(u16, u32), SectionId> = HashMap::new();
        for index in order {
            let structor = list[index as usize];
            let key = match structor.associated {
                Some(sym) => {
                    let symbol = module.symbol(sym);
                    if !symbol.is_definition() || symbol.linkage() == Linkage::AvailableExternally {
                        continue;
                    }
                    sym.index() as u32
                }
                None => NOWHERE,
            };
            let section = if let Some(&section) = sections.get(&(structor.priority, key)) {
                section
            } else {
                let section = if ctors {
                    hod_obj::elf::add_init_array(&mut self.obj, structor.priority)
                } else {
                    hod_obj::elf::add_fini_array(&mut self.obj, structor.priority)
                }
                .map_err(obj_err)?;
                if key != NOWHERE {
                    let group = self.signature_group(SymbolId::from_index(key), GroupKind::Comdat);
                    self.obj.add_group_member(group, section).map_err(obj_err)?;
                }
                sections.insert((structor.priority, key), section);
                section
            };
            let offset = self.obj.append_zeros(section, 8);
            let (target, addend) = self.data_target(structor.func, 0, true);
            self.obj.add_relocation(section, offset, abs, target, addend);
        }
        Ok(())
    }

    /// `.linker-options` and `.deplibs`.
    fn linker_options(&mut self) -> CgResult<()> {
        let module = self.module;
        let lists: [(&[u8], &hod_ir::StringLists, SectionKind); 2] = [
            (
                b".linker-options",
                module.linker_options(),
                SectionKind::Elf { sh_type: 0x6fff_4c01, sh_flags: SHF_EXCLUDE, sh_entsize: 0 },
            ),
            (
                b".deplibs",
                module.dependent_libraries(),
                SectionKind::Elf {
                    sh_type: 0x6fff_4c04,
                    sh_flags: SHF_MERGE | SHF_STRINGS,
                    sh_entsize: 1,
                },
            ),
        ];
        for (name, list, kind) in lists {
            if list.is_empty() {
                continue;
            }
            let section = self.obj.add_section(name, kind, 1).map_err(obj_err)?;
            for strings in list.iter() {
                for text in strings {
                    self.obj.append(section, text);
                    self.obj.append(section, &[0]);
                }
            }
        }
        Ok(())
    }

    /// Writes every placed symbol's definition. A private symbol is
    /// written only if a relocation or group names it, as `llc` keeps
    /// `.L` names out of the symbol table otherwise.
    fn define(&mut self) -> CgResult<()> {
        let module = self.module;
        for symbol in module.symbols() {
            let sym = symbol.id();
            let place = self.places[sym.index()];
            let id = Self::obj_sym(sym);
            if !place.is_placed() {
                // `llc` writes a declaration it has a directive for
                // (`.hidden`, `.protected`, `.weak`) even when nothing
                // refers to it; such a symbol has no type.
                let directive = symbol.visibility() != hod_ir::Visibility::Default
                    || symbol.linkage() == Linkage::ExternWeak;
                if directive && !symbol.is_unresolved() && !self.obj.is_referenced(id) {
                    self.obj.symbol_mut(id).kind = SymbolKind::NoType;
                    self.obj.retain_symbol(id);
                }
                if !symbol.is_definition() && self.obj.is_referenced(id) {
                    self.name_symbol(sym)?;
                }
                continue;
            }
            if symbol.linkage() == Linkage::Private && !self.obj.is_referenced(id) {
                continue;
            }
            if symbol.linkage() == Linkage::Private && !symbol.is_anonymous() {
                self.name_symbol(sym)?;
            }
            let target = self.obj.symbol_mut(id);
            target.binding = binding(symbol.linkage());
            target.value = place.value;
            target.size = place.size;
            target.kind = place.kind;
            target.place = if place.common {
                SymbolPlace::Common
            } else {
                SymbolPlace::Section(SectionId::from_index(place.section))
            };
        }
        for (&sym, &id) in &self.local_aliases {
            let place = self.places[sym.index()];
            if place.section == NOWHERE {
                return Err(CgError::internal("a local alias of a symbol not defined here"));
            }
            let target = self.obj.symbol_mut(id);
            target.binding = Binding::Local;
            target.value = place.value;
            target.size = place.size;
            target.kind = place.kind;
            target.place = SymbolPlace::Section(SectionId::from_index(place.section));
        }
        for group in &self.groups {
            if group.signature != NOWHERE {
                let target = self.obj.symbol_mut(SymbolId::from_index(group.signature));
                target.binding = Binding::Local;
                target.place = SymbolPlace::Section(SectionId::from_index(group.first));
            }
        }
        Ok(())
    }
}

/// Emits a module whose functions are compiled into `chunks`, in module
/// order (§9.7).
///
/// # Errors
/// As [`ModuleEmitter::new`], [`ModuleEmitter::add_chunk`] and
/// [`ModuleEmitter::finish`].
pub fn emit_module(module: &Module, options: &Options, chunks: &[Chunk]) -> CgResult<Emitted> {
    let mut emitter = ModuleEmitter::new(module, options)?;
    for chunk in chunks {
        emitter.add_chunk(chunk)?;
    }
    emitter.finish()
}
