//! DIEs (`spec/dwarf.md` §6): the unit plan, which inverts `hod_ir`'s
//! scope links, and the writer of `.debug_info`, `.debug_abbrev`,
//! `.debug_str`, `.debug_str_offsets`, `.debug_addr` and
//! `.debug_rnglists`.

use hod_ir::{
    ChecksumKind, ConstPool, DebugInfo, DiCompileUnit, DiFile, DiId, DiKind, DiNode, EmissionKind,
    ExprId, Module, TypeTable,
};
use hod_obj::elf::{SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_obj::{Object, SectionId, SectionKind, SymbolId};

use crate::Error;
use crate::abbrev::{Used, code, fam, write_abbrev};
use crate::leb::write_uleb;
use crate::line::FileTable;
use crate::strings::DebugStrings;
use crate::target::for_arch;

pub(crate) const NONE: u32 = u32::MAX;
const DW_UT_COMPILE: u8 = 0x01;
const DW_OP_ADDRX: u8 = 0xa1;
const DW_RLE_END_OF_LIST: u8 = 0x00;
const DW_RLE_STARTX_LENGTH: u8 = 0x03;
/// The size of a DWARF 5 unit header (32-bit format): the offset of a
/// unit's first DIE.
const UNIT_HEADER: usize = 12;

/// What a DIE of the plan is written as; its node is the `hod_ir` node
/// it is written from (`spec/dwarf.md` §6.2, §6.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Unit,
    Namespace,
    /// A global variable (`DIGlobalVariable`).
    Variable,
    /// A `DIBasicType`: a base or unspecified type.
    BaseType,
    /// A `DIDerivedType` that is a type: pointer, reference, qualifier,
    /// typedef, template alias, pointer to member.
    Derived,
    /// A `DIDerivedType` member or inheritance of a class (not keyed:
    /// one node listed by two classes is two DIEs, as in `llc`).
    Member,
    /// A static data member's declaration (keyed).
    StaticMember,
    /// A `DW_TAG_friend` element (not keyed).
    Friend,
    /// A structure, class or union.
    Composite,
    Enumeration,
    /// Its node is the `DIEnumerator` (not keyed: enumerators are
    /// uniqued, so two enumerations may share one).
    Enumerator,
    Array,
    /// Its node is the `DISubrange` (not keyed).
    Subrange,
    /// The unit's `__ARRAY_SIZE_TYPE__`; its node is the unit.
    IndexType,
    Subroutine,
    /// A formal parameter of a subroutine type or a member function
    /// declaration; its node is the parameter's type (not keyed).
    Param,
    /// `DW_TAG_unspecified_parameters`; its node is its parent's.
    Unspecified,
    /// A member function declaration (`DISubprogram`, keyed).
    Method,
    /// A function's definition (`DISubprogram`, keyed; §6.4).
    Subprogram,
    /// A parameter of a definition (`DILocalVariable`, keyed).
    LocalParam,
    /// A local variable (`DILocalVariable`, keyed).
    LocalVar,
    /// A lexical block (`DILexicalBlock`, keyed).
    Block,
    /// A label (`DILabel`, keyed).
    Label,
    // WP-085 (none keyed: `Inline` maps the nodes of abstract DIEs).
    /// An abstract instance of a subprogram (`DW_AT_inline`).
    AbstractSubprogram,
    /// A parameter, local variable, label or lexical block of an
    /// abstract instance.
    AbstractParam,
    AbstractVar,
    AbstractLabel,
    AbstractBlock,
    /// An inlined subroutine; its node is the inlined `DISubprogram`.
    Inlined,
}

/// One DIE of the plan: the node it is written from and its parent's
/// index.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Die {
    pub(crate) node: DiId,
    pub(crate) parent: u32,
    pub(crate) kind: Kind,
}

/// The DIE tree of one compile unit, before anything is written
/// (`spec/dwarf.md` §6.2): which nodes of `hod_ir`'s tables become DIEs,
/// their parents, and each parent's run of children.
///
/// Built once per unit from the tables alone, so it can be built before
/// or while code is generated, and shared read-only by the workers. No
/// tree of nodes is built: a DIE is a node id, a parent index and a kind
/// (twelve bytes), children are a counting sort of the DIEs by parent,
/// and the map from a node to its DIE is four bytes per node.
#[derive(Clone, Debug)]
pub struct UnitPlan<'a> {
    pub(crate) debug: &'a DebugInfo,
    pub(crate) unit: DiId,
    pub(crate) cu: DiCompileUnit<'a>,
    name: &'a [u8],
    comp_dir: &'a [u8],
    files: FileTable<'a>,
    /// `(DiFile id, file index)`, sorted by id.
    file_index: Vec<(u32, u32)>,
    /// In discovery order; DIE 0 is the unit.
    pub(crate) dies: Vec<Die>,
    /// Per DIE, where its children start in `children`; one more entry
    /// than `dies`.
    child_start: Vec<u32>,
    pub(crate) children: Vec<u32>,
    /// Per node of the table: its DIE, or `NONE`.
    pub(crate) slot: Vec<u32>,
    /// `(DIGlobalVariableExpression id, DIE)`, sorted by id.
    by_expr: Vec<(u32, u32)>,
    /// The module's constants, for static members' values (§6.8).
    pub(crate) consts: Option<(&'a ConstPool, &'a TypeTable)>,
    /// `(DIE, DIE)`, sorted: the variable a subrange's count names, or
    /// the containing type of a virtual member function (§6.8).
    pub(crate) aux: Vec<(u32, u32)>,
    /// `(DIE, value, signed)`, sorted: a static member's constant.
    pub(crate) values: Vec<(u32, u64, bool)>,
    /// The `__ARRAY_SIZE_TYPE__` DIE, or `NONE`.
    pub(crate) index_die: u32,
    /// While the plan is built: the type graph's work (§6.8).
    pub(crate) build: crate::types::Build,
    /// Whether the plan is sealed (§6.4): inverted, and taking no more
    /// functions.
    pub(crate) sealed: bool,
    /// The functions, sorted by subprogram DIE.
    pub(crate) funcs: Vec<crate::func::FnData>,
    /// The variables, blocks and labels, sorted by DIE.
    pub(crate) locals: Vec<crate::func::LocalData>,
    /// The blocks' ranges: code offsets of their function.
    pub(crate) ranges: Vec<(u32, u32)>,
    /// The `.debug_addr` entries the functions' DIEs take.
    pub(crate) code_addrs: u32,
    pub(crate) fn_scratch: crate::func::FnScratch,
    /// Abstract instances and functions' statics (WP-085).
    pub(crate) inline: crate::inline::Inline,
}

fn md5_of(f: &DiFile<'_>) -> Option<[u8; 16]> {
    if f.checksum_kind != ChecksumKind::Md5 || f.checksum.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        let hex = std::str::from_utf8(&f.checksum[2 * i..2 * i + 2]).ok()?;
        *b = u8::from_str_radix(hex, 16).ok()?;
    }
    Some(out)
}

/// A file's embedded source (`DW_LNCT_LLVM_source`); an empty one is
/// none (`spec/ir.md` §6).
fn source_of<'a>(f: &DiFile<'a>) -> Option<&'a [u8]> {
    (!f.source.is_empty()).then_some(f.source)
}

pub(crate) const fn unsupported(node: DiId, what: &'static str) -> Error {
    Error::Unsupported { node: node.raw(), what }
}

pub(crate) const fn bad(node: DiId, field: &'static str) -> Error {
    Error::BadReference { node: node.raw(), field }
}

impl<'a> UnitPlan<'a> {
    /// The plan of compile unit `unit` of `debug`.
    ///
    /// Its DIEs are the unit's global variables (`globals`), the
    /// namespaces that enclose them and the types they name, each once,
    /// in order of discovery: per global, its enclosing namespaces from
    /// the outermost, the variable, then the types it names (`llc`'s
    /// order). A DIE's children are in the same order.
    ///
    /// # Errors
    /// `NotACompileUnit`; `BadReference` for a field that names no node
    /// or a node of the wrong kind, or a namespace cycle; `Unsupported`
    /// for what this writer does not write yet (`spec/dwarf.md` §6.5).
    pub fn new(debug: &'a DebugInfo, unit: DiId) -> Result<Self, Error> {
        let mut plan = Self::build(debug, None, unit, None)?;
        plan.seal()?;
        Ok(plan)
    }

    /// The plan of compile unit `unit` of `module`'s debug tables, with
    /// the module's constants for the values of static data members
    /// (`spec/dwarf.md` §6.8); [`UnitPlan::new`] refuses a static member
    /// with a value.
    ///
    /// # Errors
    /// As [`UnitPlan::new`].
    pub fn of_module(module: &'a Module, unit: DiId) -> Result<Self, Error> {
        let mut plan = Self::for_functions(module, unit)?;
        plan.seal()?;
        Ok(plan)
    }

    /// The plan of [`UnitPlan::of_module`], open: the code generator adds
    /// its placed functions ([`UnitPlan::add_function`]), in module
    /// order, then seals it ([`UnitPlan::seal`]) before it is written
    /// (`spec/dwarf.md` §6.4).
    ///
    /// # Errors
    /// As [`UnitPlan::new`].
    pub fn for_functions(module: &'a Module, unit: DiId) -> Result<Self, Error> {
        let bodies = crate::inline::bodies(module);
        Self::build(&module.debug, Some((&module.consts, &module.types)), unit, Some(bodies))
    }

    fn build(
        debug: &'a DebugInfo,
        consts: Option<(&'a ConstPool, &'a TypeTable)>,
        unit: DiId,
        bodies: Option<Vec<u32>>,
    ) -> Result<Self, Error> {
        let Some(DiKind::CompileUnit) = debug.find_kind(unit) else {
            return Err(Error::NotACompileUnit { node: unit.raw() });
        };
        let DiNode::CompileUnit(cu) = debug.node(unit) else {
            return Err(Error::NotACompileUnit { node: unit.raw() });
        };
        if cu.emission_kind != EmissionKind::FullDebug {
            crate::reduced::check_unit(unit, &cu)?; // WP-061: §6.10
        }
        let Some(file) = cu.file.filter(|&f| debug.find_kind(f) == Some(DiKind::File)) else {
            return Err(bad(unit, "file"));
        };
        let DiNode::File(file) = debug.node(file) else { return Err(bad(unit, "file")) };

        // The file table: directory 0 and file 0 from the unit, then every
        // file node in id order (`spec/dwarf.md` §4.4, §5.7).
        let mut files = FileTable::new_with_source(
            file.directory,
            file.filename,
            md5_of(&file),
            source_of(&file),
        );
        let mut file_index = Vec::new();
        let count = debug.node_count();
        for raw in 0..count as u32 {
            let id = DiId::new(raw);
            if debug.kind(id) == DiKind::File {
                let DiNode::File(f) = debug.node(id) else { continue };
                let at = files.add_with_source(f.directory, f.filename, md5_of(&f), source_of(&f));
                file_index.push((raw, at));
            }
        }

        let mut plan = UnitPlan {
            debug,
            unit,
            cu,
            name: file.filename,
            comp_dir: file.directory,
            files,
            file_index,
            dies: Vec::with_capacity(cu.globals.len() * 2 + 1),
            child_start: Vec::new(),
            children: Vec::new(),
            slot: vec![NONE; count],
            by_expr: Vec::with_capacity(cu.globals.len()),
            consts,
            aux: Vec::new(),
            values: Vec::new(),
            index_die: NONE,
            build: crate::types::Build::default(),
            sealed: false,
            funcs: Vec::new(),
            locals: Vec::new(),
            ranges: Vec::new(),
            code_addrs: 0,
            fn_scratch: crate::func::FnScratch::default(),
            inline: crate::inline::Inline::new(bodies),
        };
        plan.push(unit, 0, Kind::Unit);
        let mut chain = Vec::new();
        for &gve in cu.globals {
            plan.discover_global(gve, &mut chain)?;
        }
        // Then the unit's enumerations and retained types, as `llc`'s
        // `beginModule` (§6.8).
        for &e in cu.enums {
            plan.reference_type(unit, "enums", e)?;
        }
        for &t in cu.retained_types {
            if debug.find_kind(t) == Some(DiKind::Subprogram) {
                continue; // a retained function is WP-057's
            }
            plan.reference_type(unit, "retainedTypes", t)?;
        }
        Ok(plan)
    }

    /// The last step of [`UnitPlan::seal`].
    pub(crate) fn seal_tables(&mut self) {
        self.by_expr.sort_unstable();
        self.invert();
        self.fn_scratch = crate::func::FnScratch::default();
        self.sealed = true;
    }

    /// A DIE for `node`, keyed: `die_of(node)` finds it.
    pub(crate) fn push(&mut self, node: DiId, parent: u32, kind: Kind) -> u32 {
        let index = self.push_anon(node, parent, kind);
        self.slot[node.index()] = index;
        index
    }

    /// A DIE written from `node` that `die_of(node)` does not find.
    pub(crate) fn push_anon(&mut self, node: DiId, parent: u32, kind: Kind) -> u32 {
        let index = self.dies.len() as u32;
        self.dies.push(Die { node, parent, kind });
        index
    }

    pub(crate) fn node_of(
        &self,
        from: DiId,
        field: &'static str,
        id: DiId,
    ) -> Result<DiNode<'a>, Error> {
        if !self.debug.contains(id) {
            return Err(bad(from, field));
        }
        Ok(self.debug.node(id))
    }

    fn discover_global(&mut self, gve: DiId, chain: &mut Vec<DiId>) -> Result<(), Error> {
        let DiNode::GlobalVariableExpression(e) = self.node_of(self.unit, "globals", gve)? else {
            return Err(bad(self.unit, "globals"));
        };
        let var = e.var.ok_or(bad(gve, "var"))?;
        let DiNode::GlobalVariable(v) = self.node_of(gve, "var", var)? else {
            return Err(bad(gve, "var"));
        };
        let seen = self.slot[var.index()];
        if seen != NONE {
            if self.by_expr.iter().any(|&(e, d)| e == gve.raw() && d == seen) {
                return Ok(()); // listed twice
            }
            return Err(unsupported(var, "a global variable with several expressions"));
        }
        if e.expr != ExprId::EMPTY {
            return Err(unsupported(gve, "a global variable expression that is not empty"));
        }
        if !v.is_definition || v.declaration.is_some() {
            return Err(unsupported(var, "a declaration or a static member's definition"));
        }
        if !v.template_params.is_empty() {
            return Err(unsupported(var, "a variable template's parameters"));
        }
        if v.ty.is_none() {
            return Err(unsupported(var, "a global variable without a type"));
        }
        if v.line != 0 && v.file.is_none() {
            return Err(unsupported(var, "a declaration line without a file"));
        }
        let parent = match v.scope {
            // A function's `static`: under the function's DIE (WP-085).
            Some(s)
                if matches!(
                    self.debug.find_kind(s),
                    Some(DiKind::Subprogram | DiKind::LexicalBlock | DiKind::LexicalBlockFile)
                ) =>
            {
                self.static_parent(var, s)?
            }
            _ => self.scope_parent(var, v.scope, chain)?,
        };
        let die = self.push(var, parent, Kind::Variable);
        self.by_expr.push((gve.raw(), die));
        self.reference_type(var, "type", v.ty.unwrap_or(DiId::NONE)).map(|_| ())
    }

    /// The DIE that encloses a node whose `scope` field is `scope`: the
    /// unit, or a namespace DIE, discovered from the outermost namespace
    /// in when it is new.
    pub(crate) fn scope_parent(
        &mut self,
        of: DiId,
        scope: Option<DiId>,
        chain: &mut Vec<DiId>,
    ) -> Result<u32, Error> {
        let Some(s) = scope else { return Ok(0) };
        match self.debug.find_kind(s) {
            None => return Err(bad(of, "scope")),
            Some(DiKind::CompileUnit | DiKind::File) => return Ok(0),
            Some(DiKind::Namespace) => {}
            Some(_) => return Err(unsupported(of, "a scope that is not a namespace or the unit")),
        }
        if self.slot[s.index()] != NONE {
            return Ok(self.slot[s.index()]);
        }
        chain.clear();
        let mut at = s;
        let top = loop {
            chain.push(at);
            if chain.len() > self.dies.len() + self.slot.len() {
                return Err(bad(s, "scope")); // a cycle
            }
            let DiNode::Namespace(n) = self.debug.node(at) else { return Err(bad(of, "scope")) };
            let Some(p) = n.scope else { break 0 };
            match self.debug.find_kind(p) {
                None => return Err(bad(at, "scope")),
                Some(DiKind::CompileUnit | DiKind::File) => break 0,
                Some(DiKind::Namespace) if self.slot[p.index()] != NONE => {
                    break self.slot[p.index()];
                }
                Some(DiKind::Namespace) => at = p,
                Some(_) => {
                    return Err(unsupported(at, "a namespace inside something else"));
                }
            }
        };
        let mut parent = top;
        for &ns in chain.iter().rev() {
            parent = self.push(ns, parent, Kind::Namespace);
        }
        Ok(parent)
    }

    /// The scope inversion (§6.2): a counting sort of the DIEs by parent,
    /// stable, so that each parent's children are in discovery order.
    fn invert(&mut self) {
        let n = self.dies.len();
        let mut start = vec![0u32; n + 1];
        for d in &self.dies[1..] {
            start[d.parent as usize + 1] += 1;
        }
        for i in 1..=n {
            start[i] += start[i - 1];
        }
        let mut fill = start[..n].to_vec();
        let mut children = vec![0u32; n - 1];
        for (i, d) in self.dies.iter().enumerate().skip(1) {
            let at = &mut fill[d.parent as usize];
            children[*at as usize] = i as u32;
            *at += 1;
        }
        self.child_start = start;
        self.children = children;
    }

    /// The compile unit.
    pub fn unit(&self) -> DiId {
        self.unit
    }

    /// The unit's line-table files, by `llc`'s rules (§4.4): the header
    /// for [`crate::write_debug_line`], and the numbering of
    /// `DW_AT_decl_file`.
    pub fn files(&self) -> &FileTable<'a> {
        &self.files
    }

    /// The line-table file index of file node `file`.
    pub fn file_index(&self, file: DiId) -> Option<u32> {
        let at = self.file_index.binary_search_by_key(&file.raw(), |&(id, _)| id).ok()?;
        Some(self.file_index[at].1)
    }

    /// The number of DIEs, the unit's included.
    pub fn die_count(&self) -> usize {
        self.dies.len()
    }

    /// The DIE index of node `node`, if it has a DIE in this unit: what a
    /// later run of DIEs refers to it by.
    pub fn die_of(&self, node: DiId) -> Option<u32> {
        self.slot.get(node.index()).copied().filter(|&d| d != NONE)
    }

    fn die_of_expr(&self, expr: DiId) -> Option<u32> {
        let at = self.by_expr.binary_search_by_key(&expr.raw(), |&(e, _)| e).ok()?;
        Some(self.by_expr[at].1)
    }

    pub(crate) fn children_of(&self, die: u32) -> std::ops::Range<usize> {
        self.child_start[die as usize] as usize..self.child_start[die as usize + 1] as usize
    }
}

/// The address of a global variable: what the variable's
/// `DW_OP_addrx` entry in `.debug_addr` is relocated against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalAddr {
    /// The `DIGlobalVariableExpression` the variable's symbol names
    /// (`Module::global_exprs`); one of the unit's `globals`.
    pub expr: DiId,
    /// The symbol: the variable's own, or, for one with internal
    /// linkage, its section's symbol (`llc` does either).
    pub symbol: SymbolId,
    /// The offset from `symbol`.
    pub addend: i64,
}

/// A run of the unit's code: one function, or several contiguous ones of
/// one section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeSpan {
    /// The section symbol of the code's section (or the function's own
    /// symbol).
    pub symbol: SymbolId,
    /// The start's offset from `symbol`; not negative.
    pub addend: i64,
    /// The length in bytes.
    pub size: u64,
}

/// One unit to write: its plan, where its line table is, and the
/// addresses the code generator placed. `globals` and `code` may come in
/// any order and from any number of chunks: the output depends on them
/// as sets (§7.1).
#[derive(Clone, Copy, Debug)]
pub struct UnitInput<'p, 'a> {
    /// The unit's plan.
    pub plan: &'p UnitPlan<'a>,
    /// The offset of the unit's line table in `.debug_line`
    /// (`DW_AT_stmt_list`).
    pub line_offset: u32,
    /// The addresses of the unit's global variables. A global without
    /// one gets a DIE without `DW_AT_location` (it was optimized out).
    pub globals: &'p [GlobalAddr],
    /// The unit's code, for `DW_AT_low_pc`/`DW_AT_high_pc` or
    /// `DW_AT_ranges`. Spans of one symbol that touch or overlap are
    /// merged.
    pub code: &'p [CodeSpan],
}

/// The sections [`write_debug_info`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DieSections {
    /// `.debug_abbrev`.
    pub abbrev: SectionId,
    /// `.debug_info`.
    pub info: SectionId,
    /// `.debug_str_offsets`.
    pub str_offsets: SectionId,
    /// `.debug_str`.
    pub str: SectionId,
    /// `.debug_addr`, when some unit has an address.
    pub addr: Option<SectionId>,
    /// `.debug_rnglists`, when some unit's code is in several sections.
    pub rnglists: Option<SectionId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sec {
    Info,
    StrOffsets,
    Addr,
}

#[derive(Clone, Copy)]
enum Target {
    Abbrev,
    Str,
    StrOffsets,
    Line,
    Addr,
    Rnglists,
    Symbol(SymbolId),
}

#[derive(Clone, Copy)]
struct Reloc {
    sec: Sec,
    offset: usize,
    target: Target,
    addend: i64,
}

/// The output of the writer before the object is touched.
pub(crate) struct Writer<'a> {
    pub(crate) info: Vec<u8>,
    pub(crate) str_offsets: Vec<u8>,
    addr: Vec<u8>,
    rnglists: Vec<u8>,
    pub(crate) strings: DebugStrings<'a>,
    pub(crate) used: Used,
    relocs: Vec<Reloc>,
    // Per unit, reused.
    pub(crate) offsets: Vec<u32>,
    pub(crate) patches: Vec<(usize, u32)>,
    stack: Vec<(u32, usize)>,
    addr_of: Vec<u32>,
    addrs: Vec<(SymbolId, i64)>,
    spans: Vec<CodeSpan>,
    /// The current unit's start in `info`, and its globals' addresses.
    pub(crate) unit_start: usize,
    cur: Vec<(SymbolId, i64)>,
    /// A location expression being built.
    pub(crate) expr: Vec<u8>,
    /// Per function of the unit: its `.debug_addr` index.
    pub(crate) fn_addr: Vec<u32>,
    /// The unit's blocks' range lists, and where they will start in
    /// `.debug_rnglists`.
    pub(crate) block_lists: Vec<u8>,
    pub(crate) block_lists_base: usize,
}

#[inline]
pub(crate) fn uleb_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

impl<'a> Writer<'a> {
    #[inline]
    pub(crate) fn abbrev(&mut self, code: u32) {
        self.used.mark(code);
        write_uleb(&mut self.info, u64::from(code));
    }

    #[inline]
    pub(crate) fn strx(&mut self, s: &'a [u8]) -> Result<(), Error> {
        let index = self.strings.index(s)?;
        write_uleb(&mut self.info, u64::from(index));
        Ok(())
    }

    /// A new `.debug_addr` entry of the unit; its index.
    pub(crate) fn push_addr(&mut self, symbol: SymbolId, addend: i64) -> u32 {
        self.addrs.push((symbol, addend));
        (self.addrs.len() - 1) as u32
    }

    /// A `DW_FORM_sec_offset` into `.debug_rnglists`.
    pub(crate) fn rnglists_offset(&mut self, offset: i64) {
        self.sec_offset(Target::Rnglists, offset);
    }

    #[inline]
    fn sec_offset(&mut self, target: Target, addend: i64) {
        self.relocs.push(Reloc { sec: Sec::Info, offset: self.info.len(), target, addend });
        self.info.extend_from_slice(&[0; 4]);
    }

    /// A `DW_FORM_ref4` to DIE `die`: written now if the DIE is, else
    /// patched once it is.
    #[inline]
    pub(crate) fn ref4(&mut self, die: u32) {
        let at = self.offsets[die as usize];
        if at == NONE {
            self.patches.push((self.info.len(), die));
            self.info.extend_from_slice(&[0; 4]);
        } else {
            self.info.extend_from_slice(&at.to_le_bytes());
        }
    }

    pub(crate) fn ref_to(
        &mut self,
        plan: &UnitPlan<'_>,
        from: DiId,
        node: DiId,
    ) -> Result<(), Error> {
        let die = plan.die_of(node).ok_or(bad(from, "type"))?;
        self.ref4(die);
        Ok(())
    }

    /// Sorts and merges the unit's code spans into `self.spans`.
    fn spans(&mut self, code: &[CodeSpan], symbols: u32) -> Result<(), Error> {
        self.spans.clear();
        for (i, s) in code.iter().enumerate() {
            if s.symbol.index() >= symbols {
                return Err(Error::BadSymbol { symbol: s.symbol.index() });
            }
            let fits = u64::try_from(s.addend).ok().and_then(|a| a.checked_add(s.size));
            if fits.is_none_or(|end| end > i64::MAX as u64) {
                return Err(Error::BadSpan { index: i as u32 });
            }
            self.spans.push(*s);
        }
        self.spans.sort_unstable_by_key(|s| (s.symbol.index(), s.addend, s.size));
        let mut out = 0;
        for i in 0..self.spans.len() {
            let s = self.spans[i];
            if out > 0 {
                let last = &mut self.spans[out - 1];
                let end = last.addend as u64 + last.size;
                if last.symbol == s.symbol && s.addend as u64 <= end {
                    last.size = end.max(s.addend as u64 + s.size) - last.addend as u64;
                    continue;
                }
            }
            self.spans[out] = s;
            out += 1;
        }
        self.spans.truncate(out);
        Ok(())
    }

    /// The functions' state of a unit (§6.4): their `.debug_addr`
    /// entries follow the globals', in DIE order, then the spans'; the
    /// blocks' lists follow the unit's own list in `.debug_rnglists`.
    /// The first span's address index.
    fn begin_functions(
        &mut self,
        plan: &UnitPlan<'a>,
        globals: usize,
        ranges: bool,
        rnglists_base: usize,
    ) -> usize {
        let span_base = globals + plan.code_addrs() as usize;
        self.fn_addr.clear();
        self.fn_addr.resize(plan.funcs.len(), 0);
        self.block_lists.clear();
        self.block_lists_base = rnglists_base
            + if ranges {
                4 + crate::func::cu_list_len(span_base as u32, self.spans.iter().map(|s| s.size))
            } else {
                0
            };
        span_base
    }

    fn unit(&mut self, u: &UnitInput<'_, 'a>, symbols: u32) -> Result<(), Error> {
        let plan = u.plan;
        let n = plan.dies.len();
        self.addr_of.clear();
        self.addr_of.resize(n, NONE);
        self.cur.clear();
        self.cur.extend(u.globals.iter().map(|g| (g.symbol, g.addend)));
        for (i, g) in u.globals.iter().enumerate() {
            if g.symbol.index() >= symbols {
                return Err(Error::BadSymbol { symbol: g.symbol.index() });
            }
            let die =
                plan.die_of_expr(g.expr).ok_or(Error::GlobalNotInUnit { expr: g.expr.raw() })?;
            if self.addr_of[die as usize] != NONE {
                return Err(Error::DuplicateGlobal { expr: g.expr.raw() });
            }
            self.addr_of[die as usize] = i as u32;
        }
        self.spans(u.code, symbols)?;
        // `.debug_addr`: the globals' entries in DIE order, then one per
        // span.
        let globals = u.globals.len();
        self.addrs.clear();
        self.offsets.clear();
        self.offsets.resize(n, NONE);
        self.patches.clear();
        self.strings.begin_unit();

        // The unit header (DWARF 5 §7.5.1.1).
        let unit_start = self.info.len();
        self.unit_start = unit_start;
        let str_offsets_base = self.str_offsets.len() + 8;
        let addr_base = self.addr.len() + 8;
        let rnglists_base = self.rnglists.len() + 12;
        self.info.extend_from_slice(&[0; 4]);
        self.info.extend_from_slice(&5u16.to_le_bytes());
        self.info.push(DW_UT_COMPILE);
        self.info.push(8);
        self.sec_offset(Target::Abbrev, 0);
        debug_assert_eq!(self.info.len() - unit_start, UNIT_HEADER);

        let ranges = self.spans.len() > 1;
        let span_base = self.begin_functions(plan, globals, ranges, rnglists_base);

        // The unit DIE.
        let cu = &plan.cu;
        let pc = self.spans.len() == 1;
        let has_addr = span_base > 0 || !self.spans.is_empty();
        let mask = u32::from(pc) | u32::from(ranges) << 1 | u32::from(has_addr) << 2;
        self.offsets[0] = UNIT_HEADER as u32;
        self.abbrev(code(fam::COMPILE_UNIT, mask, n > 1));
        self.strx(cu.producer)?;
        self.info.extend_from_slice(&cu.language.to_le_bytes());
        self.strx(plan.name)?;
        self.sec_offset(Target::StrOffsets, str_offsets_base as i64);
        self.sec_offset(Target::Line, i64::from(u.line_offset));
        self.strx(plan.comp_dir)?;
        if pc {
            let size = u32::try_from(self.spans[0].size).map_err(|_| Error::TooLarge)?;
            write_uleb(&mut self.info, span_base as u64);
            self.info.extend_from_slice(&size.to_le_bytes());
        }
        if ranges {
            self.info.extend_from_slice(&[0; 8]); // DW_AT_low_pc 0, no relocation
            write_uleb(&mut self.info, 0);
        }
        if has_addr {
            self.sec_offset(Target::Addr, addr_base as i64);
        }
        if ranges {
            self.sec_offset(Target::Rnglists, rnglists_base as i64);
        }

        // The children, depth first.
        self.stack.clear();
        if n > 1 {
            self.stack.push((0, plan.children_of(0).start));
        }
        while let Some(top) = self.stack.last_mut() {
            let (die, cursor) = *top;
            if cursor == plan.children_of(die).end {
                self.stack.pop();
                self.info.push(0);
                continue;
            }
            top.1 += 1;
            let child = plan.children[cursor];
            let has_children = !plan.children_of(child).is_empty();
            self.die(plan, child, has_children)?;
            if has_children {
                self.stack.push((child, plan.children_of(child).start));
            }
        }
        debug_assert_eq!(self.addrs.len(), span_base);

        for &(at, die) in &self.patches {
            let offset = self.offsets[die as usize];
            debug_assert_ne!(offset, NONE, "every DIE of the plan is written");
            self.info[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        }
        let length = u32::try_from(self.info.len() - unit_start - 4)
            .ok()
            .filter(|&l| l < 0xffff_fff0)
            .ok_or(Error::TooLarge)?;
        self.info[unit_start..unit_start + 4].copy_from_slice(&length.to_le_bytes());

        self.contributions(has_addr, ranges, span_base)
    }

    /// The unit's contributions to `.debug_str_offsets`, `.debug_addr`
    /// and `.debug_rnglists`, after its DIEs.
    fn contributions(
        &mut self,
        has_addr: bool,
        ranges: bool,
        span_base: usize,
    ) -> Result<(), Error> {
        // `.debug_str_offsets` (DWARF 5 §7.26).
        let offs = self.strings.unit_offsets();
        let length = u32::try_from(4 + 4 * offs.len()).map_err(|_| Error::TooLarge)?;
        self.str_offsets.extend_from_slice(&length.to_le_bytes());
        self.str_offsets.extend_from_slice(&5u16.to_le_bytes());
        self.str_offsets.extend_from_slice(&[0; 2]);
        self.relocs.reserve(offs.len());
        for &off in offs {
            let offset = self.str_offsets.len();
            self.relocs.push(Reloc {
                sec: Sec::StrOffsets,
                offset,
                target: Target::Str,
                addend: i64::from(off),
            });
            self.str_offsets.extend_from_slice(&[0; 4]);
        }

        // `.debug_addr` (DWARF 5 §7.27).
        if has_addr {
            for s in &self.spans {
                self.addrs.push((s.symbol, s.addend));
            }
            let length = u32::try_from(4 + 8 * self.addrs.len()).map_err(|_| Error::TooLarge)?;
            self.addr.extend_from_slice(&length.to_le_bytes());
            self.addr.extend_from_slice(&5u16.to_le_bytes());
            self.addr.extend_from_slice(&[8, 0]);
            for &(symbol, addend) in &self.addrs {
                let offset = self.addr.len();
                self.relocs.push(Reloc {
                    sec: Sec::Addr,
                    offset,
                    target: Target::Symbol(symbol),
                    addend,
                });
                self.addr.extend_from_slice(&[0; 8]);
            }
        }

        // `.debug_rnglists` (DWARF 5 §7.28): the unit's list, one
        // `DW_RLE_startx_length` per span, reached by the one offset;
        // then the blocks' lists, reached by `DW_FORM_sec_offset` (§6.4).
        if ranges || !self.block_lists.is_empty() {
            let start = self.rnglists.len();
            self.rnglists.extend_from_slice(&[0; 4]);
            self.rnglists.extend_from_slice(&5u16.to_le_bytes());
            self.rnglists.extend_from_slice(&[8, 0]);
            self.rnglists.extend_from_slice(&u32::from(ranges).to_le_bytes()); // offset_entry_count
            if ranges {
                self.rnglists.extend_from_slice(&4u32.to_le_bytes()); // offsets[0]
                for (k, s) in self.spans.iter().enumerate() {
                    self.rnglists.push(DW_RLE_STARTX_LENGTH);
                    write_uleb(&mut self.rnglists, (span_base + k) as u64);
                    write_uleb(&mut self.rnglists, s.size);
                }
                self.rnglists.push(DW_RLE_END_OF_LIST);
            }
            debug_assert_eq!(self.rnglists.len(), self.block_lists_base);
            self.rnglists.extend_from_slice(&self.block_lists);
            let length =
                u32::try_from(self.rnglists.len() - start - 4).map_err(|_| Error::TooLarge)?;
            self.rnglists[start..start + 4].copy_from_slice(&length.to_le_bytes());
        }
        Ok(())
    }

    /// Writes DIE `die` of `plan` (without its children).
    fn die(&mut self, plan: &UnitPlan<'a>, die: u32, children: bool) -> Result<(), Error> {
        let d = plan.dies[die as usize];
        self.offsets[die as usize] =
            u32::try_from(self.info.len() - self.unit_start).map_err(|_| Error::TooLarge)?;
        match (d.kind, plan.debug.node(d.node)) {
            (Kind::Variable, DiNode::GlobalVariable(v)) => {
                let file = match v.file {
                    Some(f) if v.line != 0 => Some(plan.file_index(f).ok_or(bad(d.node, "file"))?),
                    _ => None,
                };
                let addr = self.addr_of[die as usize];
                let mask = u32::from(!v.is_local)
                    | u32::from(file.is_some()) << 1
                    | u32::from(v.align != 0) << 2
                    | u32::from(addr != NONE) << 3
                    | u32::from(!v.linkage_name.is_empty()) << 4;
                if v.name.is_empty() {
                    // A string literal's (clang 22): no name (§6.8).
                    self.abbrev(code(fam::GLOBAL_NAMELESS, mask, false));
                } else {
                    self.abbrev(code(fam::GLOBAL_VARIABLE, mask, false));
                    self.strx(v.name)?;
                }
                self.ref_to(plan, d.node, v.ty.unwrap_or(DiId::NONE))?;
                if let Some(file) = file {
                    write_uleb(&mut self.info, u64::from(file));
                    write_uleb(&mut self.info, u64::from(v.line));
                }
                if v.align != 0 {
                    write_uleb(&mut self.info, u64::from(v.align / 8));
                }
                if addr != NONE {
                    let index = self.addrs.len() as u64;
                    let g = self.cur[addr as usize];
                    self.addrs.push(g);
                    write_uleb(&mut self.info, 1 + uleb_len(index) as u64);
                    self.info.push(DW_OP_ADDRX);
                    write_uleb(&mut self.info, index);
                }
                if !v.linkage_name.is_empty() {
                    self.strx(v.linkage_name)?;
                }
            }
            (Kind::Namespace, DiNode::Namespace(ns)) => {
                let mask = u32::from(!ns.name.is_empty()) | u32::from(ns.export_symbols) << 1;
                self.abbrev(code(fam::NAMESPACE, mask, children));
                if !ns.name.is_empty() {
                    self.strx(ns.name)?;
                }
            }
            (Kind::Subprogram, DiNode::Subprogram(sp)) => {
                self.subprogram_die(plan, die, d.node, &sp, children)?;
            }
            (Kind::LocalParam | Kind::LocalVar | Kind::Block | Kind::Label, _) => {
                self.local_die(plan, die, d.kind, d.node)?;
            }
            (
                Kind::AbstractSubprogram
                | Kind::AbstractParam
                | Kind::AbstractVar
                | Kind::AbstractLabel
                | Kind::AbstractBlock
                | Kind::Inlined,
                _,
            ) => self.inline_die(plan, die, d.kind, d.node, children)?,
            (Kind::Unit, _) => unreachable!("the unit DIE is written by `unit`"),
            (_, node) => self.type_die(plan, die, d, node, children)?,
        }
        Ok(())
    }
}

/// Writes the DIEs of `units` into new `.debug_abbrev`, `.debug_info`,
/// `.debug_str_offsets` and `.debug_str` sections of `obj`, and
/// `.debug_addr` and `.debug_rnglists` when a unit needs them, with their
/// relocations (`spec/dwarf.md` §6). `line` is the `.debug_line` section
/// the units' `DW_AT_stmt_list` point into.
///
/// Units are written in the order given; within a unit the output
/// depends on the plan and on `globals` and `code` as sets, not on their
/// order. On an error `obj` is unchanged.
///
/// # Errors
/// `NoUnit`; `GlobalNotInUnit`, `DuplicateGlobal`, `BadSymbol`,
/// `BadSpan` for the addresses; `BadReference` for a file the plan does
/// not number; `NulInName`; `TooLarge`; `Object`.
pub fn write_debug_info(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
) -> Result<DieSections, Error> {
    if units.is_empty() {
        return Err(Error::NoUnit);
    }
    if line.index() >= obj.section_count() {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }
    let symbols = obj.symbol_count();
    let dies: usize = units.iter().map(|u| u.plan.dies.len()).sum();
    let mut w = Writer {
        info: Vec::with_capacity(16 + dies * 10),
        str_offsets: Vec::with_capacity(8 + dies * 4),
        addr: Vec::new(),
        rnglists: Vec::new(),
        strings: DebugStrings::new(),
        used: Used::new(),
        relocs: Vec::with_capacity(8 + dies * 2),
        offsets: Vec::new(),
        patches: Vec::new(),
        stack: Vec::new(),
        addr_of: Vec::new(),
        addrs: Vec::new(),
        spans: Vec::new(),
        unit_start: 0,
        cur: Vec::new(),
        expr: Vec::new(),
        fn_addr: Vec::new(),
        block_lists: Vec::new(),
        block_lists_base: 0,
    };
    if let Some(u) = units.iter().find(|u| !u.plan.sealed) {
        return Err(unsupported(u.plan.unit, "a unit plan that is not sealed"));
    }
    for u in units {
        w.unit(u, symbols)?;
    }
    let abbrev = write_abbrev(&w.used);
    for len in [w.info.len(), w.str_offsets.len(), w.addr.len(), w.rnglists.len()] {
        if u32::try_from(len).is_err() {
            return Err(Error::TooLarge);
        }
    }
    if obj.section_count() >= u32::MAX - 7 {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }

    // Only now touch the object.
    let target = for_arch(obj.arch());
    let abbrev_sec = obj.add_section(b".debug_abbrev", SectionKind::Debug, 1)?;
    let info = obj.add_section(b".debug_info", SectionKind::Debug, 1)?;
    let str_offsets = obj.add_section(b".debug_str_offsets", SectionKind::Debug, 1)?;
    let str_kind = SectionKind::Elf {
        sh_type: SHT_PROGBITS,
        sh_flags: SHF_MERGE | SHF_STRINGS,
        sh_entsize: 1,
    };
    let str_sec = obj.add_section(b".debug_str", str_kind, 1)?;
    let addr = if w.addr.is_empty() {
        None
    } else {
        Some(obj.add_section(b".debug_addr", SectionKind::Debug, 1)?)
    };
    let rnglists = if w.rnglists.is_empty() {
        None
    } else {
        Some(obj.add_section(b".debug_rnglists", SectionKind::Debug, 1)?)
    };
    obj.append_owned(abbrev_sec, abbrev);
    obj.append_owned(info, w.info);
    obj.append_owned(str_offsets, w.str_offsets);
    obj.append_owned(str_sec, w.strings.into_data());
    if let Some(a) = addr {
        obj.append_owned(a, w.addr);
    }
    if let Some(r) = rnglists {
        obj.append_owned(r, w.rnglists);
    }
    let sym_abbrev = obj.section_symbol(abbrev_sec);
    let sym_str = obj.section_symbol(str_sec);
    let sym_str_offsets = obj.section_symbol(str_offsets);
    let sym_line = obj.section_symbol(line);
    let sym_addr = addr.map(|a| obj.section_symbol(a));
    let sym_rnglists = rnglists.map(|r| obj.section_symbol(r));
    obj.reserve(0, 0, w.relocs.len(), 0);
    for r in &w.relocs {
        let (symbol, wide) = match r.target {
            Target::Abbrev => (sym_abbrev, false),
            Target::Str => (sym_str, false),
            Target::StrOffsets => (sym_str_offsets, false),
            Target::Line => (sym_line, false),
            Target::Addr => (sym_addr.unwrap_or(sym_abbrev), false),
            Target::Rnglists => (sym_rnglists.unwrap_or(sym_abbrev), false),
            Target::Symbol(s) => (s, true),
        };
        let section = match r.sec {
            Sec::Info => info,
            Sec::StrOffsets => str_offsets,
            Sec::Addr => addr.unwrap_or(info),
        };
        let kind = if wide { target.address_reloc } else { target.offset_reloc };
        obj.add_relocation(section, r.offset as u64, kind, symbol, r.addend);
    }
    Ok(DieSections { abbrev: abbrev_sec, info, str_offsets, str: str_sec, addr, rnglists })
}
