//! `hod_ll` against the objects `llc -O0` writes for the same modules:
//! every variable's initializer, flattened by `hod_ir`, against the bytes
//! and relocations of its section; every symbol against the symbol
//! table; sections, COMDAT groups, alignment and thread-local storage;
//! and the constructor lists against `.init_array`.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use hod_ir::{
    Const, Fixup, FixupTarget, Linkage, Module, SymId, Symbol, SymbolKind, TlsModel, UnnamedAddr,
    Visibility,
};
use hod_ll::ParsedModule;
use hod_testutil::{DEFAULT_TIMEOUT, Scratch, run_with};

use crate::common::elf::{
    Elf, Reloc, RelocClass, SHF_TLS, SHN_COMMON, SHN_UNDEF, SHT_NOBITS, STB_GLOBAL, STB_LOCAL,
    STB_WEAK, STT_FUNC, STT_GNU_IFUNC, STT_NOTYPE, STT_OBJECT, STT_SECTION, STT_TLS,
};
use crate::common::{Case, Llvm, corpus, parsed};

/// `SHF_WRITE`.
const SHF_WRITE: u64 = 1;
/// `SHF_GNU_RETAIN`.
const SHF_GNU_RETAIN: u64 = 0x20_0000;

/// Where something is once the object is laid out: in a section, or
/// left to the linker by name.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Place {
    Section(u16, u64),
    Named(String, i64),
}

/// One module with its object.
struct View<'a> {
    case: &'a Case,
    elf: &'a Elf,
    parsed: &'a ParsedModule<'a>,
    /// The name each symbol has in the object.
    names: Vec<String>,
    relocs: BTreeMap<(usize, u64), Reloc>,
}

/// What was compared, for the report.
#[derive(Default, Debug)]
struct Tally {
    variables: usize,
    bytes: usize,
    relocations: usize,
    folded: usize,
    functions: usize,
    aliases: usize,
    got_equivalents: usize,
    /// Variables whose initializer `hod_ir` has no image for.
    without_image: BTreeSet<String>,
    /// Variables holding a label difference (WP-095).
    label_differences: BTreeSet<String>,
}

fn binding(linkage: Linkage) -> u8 {
    match linkage {
        Linkage::Internal | Linkage::Private => STB_LOCAL,
        Linkage::External | Linkage::Common | Linkage::AvailableExternally => STB_GLOBAL,
        _ => STB_WEAK,
    }
}

fn visibility(visibility: Visibility) -> u8 {
    match visibility {
        Visibility::Default => 0,
        Visibility::Hidden => 2,
        Visibility::Protected => 3,
    }
}

impl<'a> View<'a> {
    fn new(case: &'a Case, elf: &'a Elf, parsed: &'a ParsedModule<'a>) -> View<'a> {
        // LLVM names what has no name `__unnamed_K`, counting from 1 in
        // the order in which it first needs each name.
        let anonymous: Vec<SymId> = (0..4096).filter_map(|n| parsed.numbered_global(n)).collect();
        let names = parsed
            .module
            .symbols()
            .map(|symbol| {
                let private = symbol.linkage() == Linkage::Private;
                let name = match anonymous.iter().position(|&sym| sym == symbol.id()) {
                    Some(at) if symbol.is_anonymous() => format!("__unnamed_{}", at + 1),
                    _ => String::from_utf8_lossy(symbol.name()).into_owned(),
                };
                // A name that starts with byte 1 is used as it is.
                match name.strip_prefix('\u{1}') {
                    Some(verbatim) => verbatim.to_string(),
                    None if private => format!(".L{name}"),
                    None => name,
                }
            })
            .collect();
        let relocs =
            elf.relocs.iter().map(|reloc| ((reloc.section, reloc.offset), *reloc)).collect();
        let mut view = View { case, elf, parsed, names, relocs };
        view.name_anonymous_variables(&anonymous);
        view
    }

    /// Finds the object's name of each variable that has none. LLVM's
    /// numbers follow its order of emission, which only code generation
    /// knows, so the variables are told apart by what they are: size,
    /// bytes, and where their relocations are and point (another
    /// anonymous variable counts as any).
    fn name_anonymous_variables(&mut self, anonymous: &[SymId]) {
        type Signature = (bool, Vec<u8>, Vec<(u64, Option<Place>)>);
        let unnamed = |name: &str| name.trim_start_matches(".L").starts_with("__unnamed_");
        let candidates: Vec<&crate::common::elf::Symbol> = self
            .elf
            .symbols
            .iter()
            .filter(|entry| unnamed(&entry.name) && matches!(entry.kind, STT_OBJECT | STT_TLS))
            .collect();
        let anonymous_place = |place: &Place| match place {
            Place::Section(section, offset) => candidates.iter().any(|entry| {
                entry.shndx == *section
                    && (entry.value..entry.value + entry.size.max(1)).contains(offset)
            }),
            Place::Named(..) => false,
        };
        let theirs: Vec<Signature> = candidates
            .iter()
            .map(|entry| {
                let index = usize::from(entry.shndx);
                let section = &self.elf.sections[index];
                let (start, size) = (entry.value as usize, entry.size as usize);
                let bytes = match section.kind {
                    SHT_NOBITS => vec![0; size],
                    _ => section.data[start..start + size].to_vec(),
                };
                let range = (index, entry.value)..(index, entry.value + entry.size);
                let relocs = self.relocs.range(range).map(|(&(_, offset), reloc)| {
                    let place = self.place_of(reloc.symbol, reloc.addend);
                    (offset - entry.value, Some(place).filter(|place| !anonymous_place(place)))
                });
                (entry.name.starts_with(".L"), bytes, relocs.collect())
            })
            .collect();
        let module = &self.parsed.module;
        let mut taken = vec![false; candidates.len()];
        let mut found = Vec::new();
        for &sym in anonymous {
            let symbol = module.symbol(sym);
            let SymbolKind::Variable { init: Some(init), .. } = symbol.kind() else {
                continue;
            };
            if !symbol.is_anonymous() {
                continue;
            }
            let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
            module.consts.flatten(&module.types, init, &mut bytes, &mut fixups).expect("an image");
            let relocs = fixups.iter().map(|fixup| {
                let place = match fixup.target {
                    FixupTarget::Sym(target) if !module.symbol(target).is_anonymous() => {
                        Some(self.place(target, fixup.addend))
                    }
                    _ => None,
                };
                (fixup.offset, place)
            });
            let mine: Signature = (symbol.linkage() == Linkage::Private, bytes, relocs.collect());
            let at = (0..theirs.len())
                .find(|&at| !taken[at] && theirs[at] == mine)
                .unwrap_or_else(|| panic!("{}: no object symbol is like {mine:?}", self.case.name));
            taken[at] = true;
            found.push((sym, candidates[at].name.clone()));
        }
        for (sym, name) in found {
            self.names[sym.index()] = name;
        }
    }

    /// What `symbol` points to if LLVM leaves it out of the object as a
    /// "GOT equivalent": a discardable `unnamed_addr` constant that holds
    /// the address of a symbol and is used in differences only. A
    /// difference with it becomes a reference to the GOT entry of what
    /// it points to.
    fn got_equivalent(&self, symbol: Symbol<'_>) -> Option<SymId> {
        let SymbolKind::Variable { constant: true, init: Some(init), .. } = symbol.kind() else {
            return None;
        };
        let absent = self.elf.symbol(&self.names[symbol.id().index()]).is_none();
        let discardable = symbol.linkage().is_local()
            || matches!(symbol.linkage(), Linkage::LinkOnceAny | Linkage::LinkOnceOdr);
        match self.parsed.module.consts.get(init) {
            Const::Addr { sym, addend: 0 }
                if absent && discardable && symbol.unnamed_addr() == UnnamedAddr::Global =>
            {
                Some(sym)
            }
            _ => None,
        }
    }

    fn context(&self, sym: SymId) -> String {
        format!("{}: {}", self.case.name, self.names[sym.index()])
    }

    /// The place of the object's symbol `index`, plus `addend`.
    fn place_of(&self, index: usize, addend: i64) -> Place {
        let symbol = &self.elf.symbols[index];
        if symbol.shndx == SHN_UNDEF || symbol.shndx == SHN_COMMON {
            return Place::Named(symbol.name.clone(), addend);
        }
        Place::Section(symbol.shndx, symbol.value.wrapping_add(addend as u64))
    }

    /// The place of the module's symbol `sym`, plus `addend`.
    fn place(&self, sym: SymId, addend: i64) -> Place {
        let name = &self.names[sym.index()];
        let (index, _) = self
            .elf
            .symbol(name)
            .unwrap_or_else(|| panic!("{}: {name} is not in the symbol table", self.case.name));
        self.place_of(index, addend)
    }

    /// Checks binding, visibility, COMDAT group and explicit section of
    /// a definition, and returns its symbol table entry.
    fn definition(&self, symbol: Symbol<'_>) -> &'a crate::common::elf::Symbol {
        let context = self.context(symbol.id());
        let name = &self.names[symbol.id().index()];
        let (_, entry) =
            self.elf.symbol(name).unwrap_or_else(|| panic!("{context}: not in the symbol table"));
        assert_eq!(entry.bind, binding(symbol.linkage()), "{context}: binding");
        assert_eq!(entry.visibility, visibility(symbol.visibility()), "{context}: visibility");
        assert_ne!(entry.shndx, SHN_UNDEF, "{context}: defined");
        if entry.shndx == SHN_COMMON {
            return entry;
        }
        let section = &self.elf.sections[usize::from(entry.shndx)];
        if let Some(name) = symbol.section() {
            assert_eq!(section.name, String::from_utf8_lossy(name), "{context}: section");
        }
        let comdat = symbol.comdat().map(|id| self.parsed.module.comdat(id).name);
        let group = self.elf.group_of(usize::from(entry.shndx));
        let comdat = comdat.map(String::from_utf8_lossy);
        assert_eq!(group, comdat.as_deref(), "{context}: COMDAT group");
        if symbol.is_used() {
            assert_ne!(section.flags & SHF_GNU_RETAIN, 0, "{context}: llvm.used keeps the section");
        }
        entry
    }

    /// §5.3 to §5.7: one defined variable.
    fn variable(&self, symbol: Symbol<'_>, constant: bool, tls: TlsModel, tally: &mut Tally) {
        let module = &self.parsed.module;
        let context = self.context(symbol.id());
        let entry = self.definition(symbol);
        let SymbolKind::Variable { init: Some(init), .. } = symbol.kind() else {
            unreachable!("a defined variable");
        };
        let thread_local = tls != TlsModel::None;
        let kind = if thread_local { STT_TLS } else { STT_OBJECT };
        assert_eq!(entry.kind, kind, "{context}: symbol type");
        let size = module.types.alloc_size_of(symbol.ty());
        // LLVM gives a common symbol of no size one byte.
        let common = entry.shndx == SHN_COMMON;
        assert_eq!(entry.size, if common { size.max(1) } else { size }, "{context}: size");
        let has_section = symbol.section().is_some();
        let mut align = module.types.global_align(symbol.ty(), symbol.align(), has_section);
        if self.case.llvm < 20 && module.types.int_bits(symbol.ty()) == Some(1) {
            // Before LLVM 20, `i1` did not take the preferred alignment
            // the data layout gives `i8` (4 on AArch64): `hod_ir` reads
            // a data layout as LLVM 22 does.
            align = symbol.align().unwrap_or(1);
        }
        if entry.shndx == SHN_COMMON {
            assert_eq!(symbol.linkage(), Linkage::Common, "{context}: common");
            assert_eq!(entry.value, align, "{context}: alignment of a common symbol");
            assert!(module.consts.is_zero(init), "{context}: a common symbol is zero");
            tally.variables += 1;
            return;
        }
        assert_ne!(symbol.linkage(), Linkage::Common, "{context}: not common");
        let index = usize::from(entry.shndx);
        let section = &self.elf.sections[index];
        assert_eq!(section.flags & SHF_TLS != 0, thread_local, "{context}: SHF_TLS");
        if !constant {
            assert_ne!(section.flags & SHF_WRITE, 0, "{context}: a variable is writable");
        }
        assert_eq!(entry.value % align, 0, "{context}: placed at its alignment {align}");
        assert!(section.align >= align, "{context}: section alignment {}", section.align);
        if section.size == size && size > 0 {
            assert_eq!(section.align, align, "{context}: alignment");
        }

        let (mut image, mut fixups) = (Vec::new(), Vec::new());
        if let Err(error) = module.consts.flatten(&module.types, init, &mut image, &mut fixups) {
            assert_eq!(error.kind, hod_ir::IrErrorKind::UnsupportedInitializer, "{context}");
            tally.without_image.insert(self.names[symbol.id().index()].clone());
            return;
        }
        assert_eq!(image.len() as u64, size, "{context}: image size");
        // A label difference (WP-095) is a number once the function is
        // laid out, which `llc` writes as `.word .La-.Lb`; under
        // `--save-temp-labels` its assembler keeps relocations for it. Its
        // value depends on the code layout: not compared.
        if fixups.iter().any(|f| matches!(f.target, FixupTarget::BlockDiff { .. })) {
            tally.label_differences.insert(self.names[symbol.id().index()].clone());
            return;
        }
        let mut relocations = 0;
        for fixup in &fixups {
            let field = format!("{context}: field at {}", fixup.offset);
            let at = (index, entry.value + fixup.offset);
            relocations += usize::from(self.fixup(module, fixup, at, &mut image, tally, &field));
        }
        let theirs = self.relocs.range((index, entry.value)..(index, entry.value + size)).count();
        assert_eq!(theirs, relocations, "{context}: number of relocations");
        if section.kind == SHT_NOBITS {
            assert!(image.iter().all(|&byte| byte == 0), "{context}: in a section without bytes");
            assert_eq!(relocations, 0, "{context}: relocations in a section without bytes");
        } else {
            let start = entry.value as usize;
            let bytes = &section.data[start..start + image.len()];
            assert_eq!(bytes, image.as_slice(), "{context}: initializer image");
        }
        tally.variables += 1;
        tally.bytes += image.len();
        tally.relocations += relocations;
    }

    /// §5.5: one fixup of a variable's image against the relocation at
    /// its place `at` (section index and offset). True if the object has
    /// a relocation there; false if the assembler computed the field,
    /// which is then written into `image`.
    fn fixup(
        &self,
        module: &Module,
        fixup: &Fixup,
        (index, at): (usize, u64),
        image: &mut [u8],
        tally: &mut Tally,
        field: &str,
    ) -> bool {
        let reloc = self.relocs.get(&(index, at));
        let class = reloc.map(|reloc| {
            let (width, class) = self
                .elf
                .reloc_class(reloc.kind)
                .unwrap_or_else(|| panic!("{field}: relocation type {}", reloc.kind));
            assert_eq!(width, fixup.size, "{field}: width");
            (class, self.place_of(reloc.symbol, reloc.addend))
        });
        match fixup.target {
            FixupTarget::Sym(sym) => {
                let (class, target) = class.unwrap_or_else(|| panic!("{field}: no relocation"));
                assert_eq!(class, RelocClass::Absolute, "{field}");
                assert_eq!(target, self.place(sym, fixup.addend), "{field}");
            }
            FixupTarget::Plt(sym) => {
                let (class, target) = class.unwrap_or_else(|| panic!("{field}: no relocation"));
                assert_ne!(class, RelocClass::Relative, "{field}");
                assert_eq!(target, self.place(sym, fixup.addend), "{field}");
            }
            FixupTarget::Block { func, .. } => {
                let (class, target) = class.unwrap_or_else(|| panic!("{field}: no relocation"));
                assert_eq!(class, RelocClass::Absolute, "{field}");
                let (Place::Section(theirs, _), Place::Section(mine, _)) =
                    (target, self.place(func, 0))
                else {
                    panic!("{field}: a block is in a section");
                };
                assert_eq!(theirs, mine, "{field}: the block is in its function's section");
            }
            FixupTarget::BlockDiff { .. } => unreachable!("{field}: checked by the caller"),
            FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
                let plt = matches!(fixup.target, FixupTarget::PltDiff { .. });
                let minus = self.place(b, 0);
                if let Some(pointee) = self.got_equivalent(module.symbol(a)) {
                    let (class, target) = class.unwrap_or_else(|| panic!("{field}: no reloc"));
                    let Place::Section(_, minus) = minus else {
                        panic!("{field}: the subtracted symbol is not placed");
                    };
                    assert_eq!(class, RelocClass::Got, "{field}");
                    let shift = at.wrapping_sub(minus) as i64;
                    assert_eq!(target, self.place(pointee, fixup.addend.wrapping_add(shift)));
                    return true;
                }
                let plus = self.place(a, fixup.addend);
                match (class, &plus, &minus) {
                    // Two places in one section: the assembler knows
                    // the distance.
                    (None, Place::Section(x, plus), Place::Section(y, minus)) if x == y => {
                        let distance = plus.wrapping_sub(*minus).to_le_bytes();
                        let width = usize::from(fixup.size);
                        let start = fixup.offset as usize;
                        image[start..start + width].copy_from_slice(&distance[..width]);
                        tally.folded += 1;
                        return false;
                    }
                    // `a - b` with `b` in the section of the field
                    // at P is `a + (P - b) - P`: relative to the
                    // place.
                    (Some((class, target)), _, Place::Section(y, minus))
                        if usize::from(*y) == index =>
                    {
                        let wanted = if plt { RelocClass::Plt } else { RelocClass::Relative };
                        assert_eq!(class, wanted, "{field}");
                        let shift = at.wrapping_sub(*minus) as i64;
                        assert_eq!(target, self.place(a, fixup.addend.wrapping_add(shift)));
                    }
                    other => {
                        panic!("{field}: a difference the object does not have: {other:?}")
                    }
                }
            }
        }
        true
    }

    /// §5.8: one alias or IFUNC, against where its target is.
    fn indirect(&self, symbol: Symbol<'_>, tally: &mut Tally) {
        let context = self.context(symbol.id());
        let entry = self.definition(symbol);
        let here = Place::Section(entry.shndx, entry.value);
        match symbol.kind() {
            SymbolKind::Alias { target, .. } => {
                let Const::Addr { sym, addend } = self.parsed.module.consts.get(target) else {
                    panic!("{context}: the corpus has aliases of symbols and of places in them");
                };
                assert_eq!(here, self.place(sym, addend), "{context}: where the alias is");
                let (_, aliasee) = self.elf.symbol(&self.names[sym.index()]).expect("the aliasee");
                assert_eq!(entry.kind, aliasee.kind, "{context}: symbol type");
            }
            SymbolKind::IFunc { resolver } => {
                assert_eq!(entry.kind, STT_GNU_IFUNC, "{context}: symbol type");
                assert_eq!(here, self.place(resolver, 0), "{context}: the resolver");
            }
            _ => unreachable!("an alias or an IFUNC"),
        }
        tally.aliases += 1;
    }

    /// §5.8: something declared; the object names it only if it is used.
    fn declaration(&self, symbol: Symbol<'_>) {
        let context = self.context(symbol.id());
        let Some((_, entry)) = self.elf.symbol(&self.names[symbol.id().index()]) else {
            return;
        };
        assert_eq!(entry.shndx, SHN_UNDEF, "{context}: undefined");
        // An undefined symbol has no type, unless a thread-local access
        // gave it one.
        let thread_local =
            matches!(symbol.kind(), SymbolKind::Variable { tls, .. } if tls != TlsModel::None);
        let kind = if thread_local && entry.kind == STT_TLS { STT_TLS } else { STT_NOTYPE };
        assert_eq!(entry.kind, kind, "{context}: symbol type");
        let weak = symbol.linkage() == Linkage::ExternWeak;
        assert_eq!(entry.bind, if weak { STB_WEAK } else { STB_GLOBAL }, "{context}: binding");
        assert_eq!(entry.visibility, visibility(symbol.visibility()), "{context}: visibility");
    }

    fn check(&self, tally: &mut Tally) {
        let mut defined = BTreeSet::new();
        for symbol in self.parsed.module.symbols() {
            let name = self.names[symbol.id().index()].as_str();
            let dropped = symbol.linkage() == Linkage::AvailableExternally
                || symbol.section() == Some(b"llvm.metadata");
            if dropped {
                // Not emitted: at most a reference to another module's.
                let entry = self.elf.symbol(name);
                assert!(entry.is_none_or(|(_, entry)| entry.shndx == SHN_UNDEF), "{name}");
                continue;
            }
            if self.got_equivalent(symbol).is_some() {
                tally.got_equivalents += 1;
                continue;
            }
            match symbol.kind() {
                SymbolKind::Variable { constant, tls, init: Some(_) } => {
                    self.variable(symbol, constant, tls, tally);
                }
                SymbolKind::Function { has_body: true, .. } => {
                    let entry = self.definition(symbol);
                    assert_eq!(entry.kind, STT_FUNC, "{}: symbol type", self.context(symbol.id()));
                    tally.functions += 1;
                }
                SymbolKind::Alias { .. } | SymbolKind::IFunc { .. } => self.indirect(symbol, tally),
                _ => {
                    self.declaration(symbol);
                    continue;
                }
            }
            defined.insert(name);
        }
        // And nothing else is defined for other modules to see, except
        // what module-level assembly defines and what code generation
        // adds (`DW.ref.*` for personalities).
        let module_asm = String::from_utf8_lossy(self.parsed.module.module_asm());
        for entry in &self.elf.symbols {
            let visible = entry.bind != STB_LOCAL && entry.shndx != SHN_UNDEF;
            let from_asm = module_asm.contains(entry.name.as_str());
            if visible && !from_asm && !entry.name.starts_with("DW.ref.") {
                assert!(
                    defined.contains(entry.name.as_str()),
                    "{}: the object defines {}, the module does not",
                    self.case.name,
                    entry.name
                );
            }
        }
        self.structors();
        self.linker_directives();
    }

    /// §5.9: `llvm.global_ctors` and `llvm.global_dtors` against the
    /// `.init_array` and `.fini_array` sections, by priority.
    fn structors(&self) {
        let module = &self.parsed.module;
        for (list, base) in [(module.ctors(), ".init_array"), (module.dtors(), ".fini_array")] {
            let mut mine: BTreeMap<String, Vec<Place>> = BTreeMap::new();
            for entry in list {
                let name = if entry.priority == 65535 {
                    base.to_string()
                } else {
                    format!("{base}.{}", entry.priority)
                };
                mine.entry(name).or_default().push(self.place(entry.func, 0));
                if let Some(associated) = entry.associated {
                    // The entry dies with what it is associated with:
                    // that symbol has to be one of the module's.
                    assert!(!module.symbol(associated).is_unresolved(), "{}", self.case.name);
                }
            }
            let mut theirs: BTreeMap<String, Vec<Place>> = BTreeMap::new();
            for (&(section, _), reloc) in &self.relocs {
                let name = &self.elf.sections[section].name;
                if name == base || name.starts_with(&format!("{base}.")) {
                    let place = self.place_of(reloc.symbol, reloc.addend);
                    theirs.entry(name.clone()).or_default().push(place);
                }
            }
            for places in mine.values_mut().chain(theirs.values_mut()) {
                places.sort_by_key(|place| format!("{place:?}"));
            }
            assert_eq!(mine, theirs, "{}: {base}", self.case.name);
        }
    }

    /// §7.2: linker options and dependent libraries against the sections
    /// `llc` writes them to.
    fn linker_directives(&self) {
        let module = &self.parsed.module;
        for (lists, name) in [
            (module.linker_options(), ".linker-options"),
            (module.dependent_libraries(), ".deplibs"),
        ] {
            let mut mine = Vec::new();
            for list in lists.iter() {
                for string in list {
                    mine.extend_from_slice(string);
                    mine.push(0);
                }
            }
            let theirs = self.elf.sections.iter().find(|section| section.name == name);
            let theirs = theirs.map(|section| section.data.clone()).unwrap_or_default();
            assert_eq!(mine, theirs, "{}: {name}", self.case.name);
        }
    }
}

/// §5.3 to §5.9: everything the module level delivers about symbols and
/// their initializers, against the object `llc` writes.
#[test]
fn symbols_and_initializers_match_llc_objects() {
    let mut tally = Tally::default();
    let mut objects = 0;
    for case in corpus() {
        let elf = match &case.object {
            Ok(elf) => elf,
            Err(error) => {
                assert!(case.name.contains("_noobj"), "{}: llc failed: {error}", case.name);
                continue;
            }
        };
        let parsed = parsed(case);
        View::new(case, elf, &parsed).check(&mut tally);
        objects += 1;
    }
    println!("{objects} objects: {tally:?}");
    if !corpus().is_empty() {
        assert!(tally.variables > 3_000 && tally.relocations > 2_000, "{tally:?}");
        assert!(tally.aliases > 0 && tally.got_equivalents > 0 && tally.folded > 0, "{tally:?}");
        // Differences of block addresses have an image since WP-095.
        assert_eq!(tally.without_image, BTreeSet::new(), "variables without an image");
        let known = ["labels.relative"].map(String::from);
        assert_eq!(tally.label_differences, BTreeSet::from(known), "label differences");
    }
}

/// The object reader of these tests against `llvm-readelf`: the symbol
/// table row by row, and the number of relocations.
#[test]
fn the_object_reader_agrees_with_llvm_readelf() {
    let scratch = Scratch::new("hod_ll_readelf");
    let mut compared = 0;
    for case in corpus().iter().filter(|case| case.name.starts_with("globals.c -g")) {
        let Ok(elf) = &case.object else {
            panic!("{}: no object", case.name);
        };
        let llvm = Llvm::find(case.llvm).expect("the LLVM that built the corpus");
        let path = scratch.write("object.o", &case.object_bytes);
        let mut command = Command::new(llvm.tool("llvm-readelf"));
        command.args(["-s", "-r", "--wide"]).arg(&path);
        let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
        assert!(outcome.success(), "{}", outcome.stderr_text());
        let text = outcome.stdout_text();
        let (mut symbols, mut relocs) = (0, 0);
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let numbered = fields.first().and_then(|first| first.strip_suffix(':'));
            if let Some(index) = numbered.and_then(|index| index.parse::<usize>().ok()) {
                let entry = &elf.symbols[index];
                let kind = match fields[3] {
                    "NOTYPE" => STT_NOTYPE,
                    "OBJECT" => STT_OBJECT,
                    "FUNC" => STT_FUNC,
                    "SECTION" => STT_SECTION,
                    "FILE" => 4,
                    "TLS" => STT_TLS,
                    "IFUNC" => STT_GNU_IFUNC,
                    other => panic!("symbol type {other}"),
                };
                let bind = match fields[4] {
                    "LOCAL" => STB_LOCAL,
                    "GLOBAL" => STB_GLOBAL,
                    "WEAK" => STB_WEAK,
                    other => panic!("binding {other}"),
                };
                let visibility = match fields[5] {
                    "DEFAULT" => 0,
                    "HIDDEN" => 2,
                    "PROTECTED" => 3,
                    other => panic!("visibility {other}"),
                };
                let shndx = match fields[6] {
                    "UND" => SHN_UNDEF,
                    "COM" => SHN_COMMON,
                    "ABS" => 0xfff1,
                    number => number.parse().expect("a section index"),
                };
                assert_eq!(u64::from_str_radix(fields[1], 16), Ok(entry.value), "{line}");
                assert_eq!(fields[2].parse(), Ok(entry.size), "{line}");
                let read = (entry.kind, entry.bind, entry.visibility, entry.shndx);
                assert_eq!(read, (kind, bind, visibility, shndx), "{line}");
                assert_eq!(fields.get(7).copied().unwrap_or(""), entry.name, "{line}");
                symbols += 1;
            } else if fields.len() >= 5 && fields[2].starts_with("R_") {
                relocs += 1;
            }
        }
        assert_eq!(symbols, elf.symbols.len(), "{}: symbols", case.name);
        assert_eq!(relocs, elf.relocs.len(), "{}: relocations", case.name);
        compared += 1;
    }
    assert!(corpus().is_empty() || compared == 4, "one module per LLVM and target: {compared}");
}
