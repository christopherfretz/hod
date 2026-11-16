//! DIEs (`spec/dwarf.md` §6): the unit plan, which inverts `hod_ir`'s
//! scope links, and the writer of `.debug_info`, `.debug_abbrev`,
//! `.debug_str`, `.debug_str_offsets`, `.debug_addr` and
//! `.debug_rnglists`.

use hod_ir::{
    ChecksumKind, ConstPool, DebugInfo, DiCompileUnit, DiFile, DiGlobalVariable, DiId, DiIds,
    DiKind, DiNamespace, DiNode, DiRecord, DiStr, DiView, EmissionKind, ExprId, Module, TypeTable,
};
use hod_obj::elf::{SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_obj::{Object, SectionId, SectionKind, SymbolId};
use std::collections::HashMap;

use crate::Error;
use crate::abbrev::{Used, code, fam, write_abbrev};
use crate::leb::write_uleb;
use crate::line::FileTable;
use crate::strings::{DebugStrings, FxBuild};
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
    /// A template parameter (not keyed; §6.11).
    TemplateParam,
    /// An imported entity (`DIImportedEntity`, keyed; §6.11).
    Imported,
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
    /// A call site (WP-113, §6.19; not keyed); its node is its
    /// function's `DISubprogram`.
    CallSite,
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
    /// WP-221: `DW_AT_producer`, the producer and the recorded command
    /// line (§6.30).
    pub(crate) producer: std::borrow::Cow<'a, [u8]>,
    pub(crate) name: &'a [u8],
    pub(crate) comp_dir: &'a [u8],
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
    /// `(DIE, bytes)`, sorted: a static member's constant wider than 64
    /// bits, as `llc` writes it (§6.28).
    pub(crate) wide_values: Vec<(u32, Vec<u8>)>,
    /// The `__ARRAY_SIZE_TYPE__` DIE, or `NONE`.
    pub(crate) index_die: u32,
    /// While the plan is built: the type graph's work (§6.8).
    pub(crate) build: crate::types::Build,
    /// WP-308: the DIEs whose construction has started and not ended,
    /// innermost last, each with its node and, for a member function,
    /// its signature's types: decoded once for all the steps of the DIE
    /// (a step is one call, and the steps of the DIEs a step makes run
    /// before its next).
    pub(crate) constructing: Vec<(u32, DiNode<'a>, &'a [DiId])>,
    /// Whether the plan is sealed (§6.4): inverted, and taking no more
    /// functions.
    pub(crate) sealed: bool,
    /// The functions, sorted by subprogram DIE.
    pub(crate) funcs: Vec<crate::func::FnData>,
    /// The variables, blocks and labels, sorted by DIE.
    pub(crate) locals: Vec<crate::func::LocalData>,
    /// The blocks' ranges: code offsets of their function.
    pub(crate) ranges: Vec<(u32, u32)>,
    /// WP-162: the located ranges of the variables described by value
    /// records, by list (`LocalWhat::List`, §6.23).
    pub(crate) value_ranges: Vec<crate::loclists::Range>,
    /// WP-225: what each range holds, by `Range::pieces` (one piece, or
    /// the fragments of a variable described in pieces, §6.31).
    pub(crate) value_pieces: Vec<crate::loclists::Piece>,
    /// WP-230: the places of the `DIArgList` values the pieces name
    /// (`Piece::args`, §6.32).
    pub(crate) value_args: Vec<crate::loclists::ValueLoc>,
    /// WP-230: the slots of the variables declared in fragments
    /// (`LocalWhat::Pieces`, §6.32), by fragment offset: `(frame
    /// offset, the declaration's expression)`.
    pub(crate) frame_pieces: Vec<(i64, ExprId)>,
    /// WP-225: the base types the located expressions convert to
    /// (`DW_OP_convert`), in first use: `(bits, DW_ATE_*)`.
    pub(crate) base_types: Vec<(u8, u8)>,
    /// WP-230: the unit is written split, so its conversions take
    /// `llc`'s legacy form (§6.31, §6.34; [`UnitPlan::set_split`]).
    pub(crate) legacy_converts: bool,
    pub(crate) piece_scratch: crate::loclists::PieceScratch,
    /// The `.debug_addr` entries the functions' DIEs take.
    pub(crate) code_addrs: u32,
    pub(crate) fn_scratch: crate::func::FnScratch,
    /// WP-059's state (§6.11).
    pub(crate) scope_dies: crate::scope::ScopeDies,
    /// Abstract instances and functions' statics (WP-085).
    pub(crate) inline: crate::inline::Inline,
    /// The DWARF version: 4 or 5 (WP-114, §6.20).
    pub(crate) version: u16,
    /// Call sites (WP-113, §6.19).
    pub(crate) calls: crate::call_sites::Calls,
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
        let mut plan = Self::build(debug, None, None, unit, None)?;
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

    /// Says the unit will be written split (`SplitDwarf` not `Off`),
    /// before any function is added: a split unit writes conversions in
    /// `llc`'s legacy form, which has no location for a sign extension
    /// to 128 bits (`spec/dwarf.md` §6.34).
    pub fn set_split(&mut self, split: bool) {
        self.legacy_converts = split;
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
        let consts = Some((&module.consts, &module.types));
        Self::build(&module.debug, Some(module), consts, unit, Some(bodies))
    }

    fn build(
        debug: &'a DebugInfo,
        module: Option<&'a Module>,
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
        crate::split::check_unit(unit, &cu)?; // WP-062: §6.15
        let Some(file) = cu.file.filter(|&f| debug.find_kind(f) == Some(DiKind::File)) else {
            return Err(bad(unit, "file"));
        };
        let DiNode::File(file) = debug.node(file) else { return Err(bad(unit, "file")) };

        // The file table: directory 0 and file 0 from the unit, then every
        // file node in the order of its contents, never of its id: ids are
        // the input's numbering, which the text and the bitcode of one
        // module do not share (`spec/dwarf.md` §4.4, §5.7; WP-192).
        let mut files = FileTable::new_with_source(
            file.directory,
            file.filename,
            md5_of(&file),
            source_of(&file),
        );
        let count = debug.node_count();
        let mut by_content: Vec<_> = (0..count as u32)
            .filter(|&raw| debug.kind(DiId::new(raw)) == DiKind::File)
            .filter_map(|raw| match debug.node(DiId::new(raw)) {
                DiNode::File(f) => {
                    Some(((f.directory, f.filename, md5_of(&f), source_of(&f)), raw))
                }
                _ => None,
            })
            .collect();
        by_content.sort_unstable();
        let mut file_index: Vec<(u32, u32)> = by_content
            .into_iter()
            .map(|((directory, filename, md5, source), raw)| {
                (raw, files.add_with_source(directory, filename, md5, source))
            })
            .collect();
        file_index.sort_unstable();

        let mut plan = UnitPlan {
            debug,
            unit,
            cu,
            producer: crate::producer::producer(&cu), // WP-221: §6.30
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
            wide_values: Vec::new(),
            index_die: NONE,
            build: crate::types::Build::default(),
            constructing: Vec::new(),
            sealed: false,
            funcs: Vec::new(),
            locals: Vec::new(),
            ranges: Vec::new(),
            value_ranges: Vec::new(),
            value_pieces: Vec::new(),
            value_args: Vec::new(),
            frame_pieces: Vec::new(),
            base_types: Vec::new(),
            legacy_converts: false,
            piece_scratch: crate::loclists::PieceScratch::default(),
            code_addrs: 0,
            fn_scratch: crate::func::FnScratch::default(),
            scope_dies: crate::scope::ScopeDies::new(module),
            inline: crate::inline::Inline::new(bodies),
            version: module.map_or(5, crate::dwarf4::version_of), // WP-114: §6.20
            calls: crate::call_sites::Calls::default(),
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

    /// Node `id` as a record of `R`'s kind, its fields read one at a time
    /// (WP-311); `bad(from, field)` when it is not a node of that kind.
    pub(crate) fn view_of<R: DiRecord>(
        &self,
        from: DiId,
        field: &'static str,
        id: DiId,
    ) -> Result<DiView<'a, R>, Error> {
        self.debug.view::<R>(id).ok_or(bad(from, field))
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
        if let Some(die) = self.discover_scope_global(gve, var, &v, e.expr, chain)? {
            self.by_expr.push((gve.raw(), die));
            return Ok(());
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

    pub(crate) fn die_of_expr(&self, expr: DiId) -> Option<u32> {
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
    /// `.debug_loclists`, when some variable is described by value
    /// records (WP-162, §6.23).
    pub loclists: Option<SectionId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sec {
    Info,
    StrOffsets,
    Addr,
}

#[derive(Clone, Copy)]
pub(crate) enum Target {
    Abbrev,
    Str,
    StrOffsets,
    Line,
    Addr,
    Rnglists,
    /// WP-162: a variable's location list.
    Loclists,
    Symbol(SymbolId),
    /// WP-136: a thread-local variable's offset in its TLS block
    /// (`crate::x86_64::tls_offset_reloc`).
    Tls(SymbolId),
}

#[derive(Clone, Copy)]
pub(crate) struct Reloc {
    pub(crate) sec: Sec,
    pub(crate) offset: usize,
    pub(crate) target: Target,
    pub(crate) addend: i64,
}

/// The output of the writer before the object is touched.
pub(crate) struct Writer<'a> {
    pub(crate) info: Vec<u8>,
    pub(crate) str_offsets: Vec<u8>,
    pub(crate) addr: Vec<u8>,
    pub(crate) rnglists: Vec<u8>,
    pub(crate) strings: DebugStrings<'a>,
    pub(crate) used: Used,
    pub(crate) relocs: Vec<Reloc>,
    // Per unit, reused.
    pub(crate) offsets: Vec<u32>,
    pub(crate) patches: Vec<(usize, u32)>,
    pub(crate) stack: Vec<(u32, usize)>,
    pub(crate) addr_of: Vec<u32>,
    pub(crate) addrs: Vec<(SymbolId, i64)>,
    /// WP-296: the index of each distinct entry of `addrs`, by symbol,
    /// addend and whether it is a thread-local offset (§6.4).
    pub(crate) addr_index: HashMap<(SymbolId, i64, bool), u32, FxBuild>,
    pub(crate) spans: Vec<CodeSpan>,
    /// The `.debug_addr` index of each span, pooled after the DIEs'
    /// addresses (`contributions`, WP-296).
    pub(crate) span_index: Vec<u32>,
    /// The current unit's start in `info`, and its globals' addresses.
    pub(crate) unit_start: usize,
    pub(crate) cur: Vec<(SymbolId, i64)>,
    /// A location expression being built.
    pub(crate) expr: Vec<u8>,
    /// Per function of the unit: its `.debug_addr` index.
    pub(crate) fn_addr: Vec<u32>,
    /// The unit's blocks' range lists, and where each starts in it: a
    /// list's `DW_FORM_rnglistx` index is its position in `block_starts`
    /// (WP-163).
    pub(crate) block_lists: Vec<u8>,
    pub(crate) block_starts: Vec<usize>,
    /// WP-162: `.debug_loclists`, and the current unit's contribution's
    /// start (`loclists::NO_LISTS` before its first list).
    pub(crate) loclists: Vec<u8>,
    pub(crate) loclists_unit: usize,
    /// WP-189: whether this writer writes a split unit, and the indices
    /// of the unit's `.debug_addr` entries that are thread-local offsets
    /// (`R_X86_64_DTPOFF64`), ascending (§6.24).
    pub(crate) split: bool,
    pub(crate) tls_addrs: Vec<u32>,
    /// WP-225: the unit offsets of the base type DIEs of
    /// `UnitPlan::base_types` (empty in a split unit, whose conversions
    /// take `llc`'s legacy form, §6.31).
    pub(crate) base_type_dies: Vec<u32>,
}

/// Writes `v` as a ULEB128 of exactly `out.len()` bytes, padded with
/// `0x80` continuation bytes if it needs fewer (DWARF 5 §7.6 allows it).
fn put_uleb_padded(out: &mut [u8], mut v: u64) {
    let last = out.len() - 1;
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = (v & 0x7f) as u8 | if i < last { 0x80 } else { 0 };
        v >>= 7;
    }
    debug_assert_eq!(v, 0, "the reserved width holds the index");
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

    /// [`Writer::strx`] of a string read from a node of the unit's store,
    /// found by its id (WP-318).
    #[inline]
    pub(crate) fn strx_str(&mut self, s: DiStr<'a>) -> Result<(), Error> {
        let index = self.strings.index_str(s)?;
        write_uleb(&mut self.info, u64::from(index));
        Ok(())
    }

    /// [`Writer::strx`] of `text`, the string field that `field` reads
    /// of node `n` (an `R`), by the field's id: for a writer that holds
    /// the node decoded, whose strings are bytes. Reading the one field
    /// again is cheaper than hashing it.
    #[inline]
    pub(crate) fn strx_of<R: DiRecord>(
        &mut self,
        debug: &'a DebugInfo,
        n: DiId,
        text: &'a [u8],
        field: impl FnOnce(DiIds<'a, R>) -> DiStr<'a>,
    ) -> Result<(), Error> {
        match debug.view::<R>(n) {
            Some(v) => {
                let s = field(v.ids());
                debug_assert_eq!(s.text(), text, "the field read again is the decoded one");
                self.strx_str(s)
            }
            None => self.strx(text),
        }
    }

    /// [`Writer::strx_of`] if `text` is not empty.
    #[inline]
    pub(crate) fn opt_name_of<R: DiRecord>(
        &mut self,
        debug: &'a DebugInfo,
        n: DiId,
        text: &'a [u8],
        field: impl FnOnce(DiIds<'a, R>) -> DiStr<'a>,
    ) -> Result<(), Error> {
        if text.is_empty() { Ok(()) } else { self.strx_of(debug, n, text, field) }
    }

    /// The unit's `.debug_addr` entry for `symbol` + `addend`, added if it
    /// is new: one entry per distinct address, as `llc`'s address pool
    /// (WP-296, §6.4). Its index.
    pub(crate) fn push_addr(&mut self, symbol: SymbolId, addend: i64) -> u32 {
        self.pool_addr(symbol, addend, false).0
    }

    /// The entry for a thread-local offset (`Target::Tls`), kept apart
    /// from an address of the same symbol; its index, and whether it is
    /// new.
    pub(crate) fn pool_addr(&mut self, symbol: SymbolId, addend: i64, tls: bool) -> (u32, bool) {
        let next = self.addrs.len() as u32;
        let index = *self.addr_index.entry((symbol, addend, tls)).or_insert(next);
        if index == next {
            self.addrs.push((symbol, addend));
        }
        (index, index == next)
    }

    /// The `DW_AT_ranges` of the scope whose list starts at `at` in
    /// `block_lists`: a `DW_FORM_rnglistx` index, the lists numbered in
    /// DIE order as `llc` numbers them (WP-163, §6.4). No relocation.
    pub(crate) fn block_list(&mut self, at: usize) {
        write_uleb(&mut self.info, self.block_starts.len() as u64);
        self.block_starts.push(at);
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

    /// Sorts the unit's code spans into `self.spans`, merging those that
    /// overlap. Spans that only touch stay two ranges (WP-305, §6.17):
    /// each span is a run of functions, and `llc` starts a range where a
    /// run ends even when the next run starts at that address (AArch64,
    /// which has no padding between functions).
    pub(crate) fn spans(&mut self, code: &[CodeSpan], symbols: u32) -> Result<(), Error> {
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
                if last.symbol == s.symbol && (s.addend as u64) < end {
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

    /// The functions' state of a unit (§6.4). The `.debug_addr` entries
    /// are the DIEs' distinct addresses in DIE order, then the spans' not
    /// already among them, as `llc`'s address pool (WP-296). Whether the
    /// unit has an address, and a bound on its entry count (what it was
    /// without the pool).
    pub(crate) fn begin_functions(&mut self, plan: &UnitPlan<'a>, globals: usize) -> (bool, usize) {
        let bound = globals + plan.code_addrs() as usize + self.spans.len();
        self.fn_addr.clear();
        self.fn_addr.resize(plan.funcs.len(), 0);
        self.block_lists.clear();
        self.block_starts.clear();
        (bound > 0, bound)
    }

    /// The unit's one span, `DW_AT_low_pc` and `DW_AT_high_pc`: the
    /// span's index is known after the DIEs, so its ULEB is reserved at
    /// the width of the largest index the unit's `bound` entries can
    /// have, to be patched (WP-296). Where, and the width.
    fn reserve_low_pc(&mut self, bound: usize) -> Result<(usize, usize), Error> {
        let size = u32::try_from(self.spans[0].size).map_err(|_| Error::TooLarge)?;
        let at = self.info.len();
        let width = uleb_len(bound as u64 - 1);
        self.info.resize(at + width, 0);
        self.info.extend_from_slice(&size.to_le_bytes());
        Ok((at, width))
    }

    /// Pools the spans' entries, after the DIEs' (WP-296).
    fn pool_spans(&mut self) {
        self.span_index.clear();
        for k in 0..self.spans.len() {
            let s = self.spans[k];
            let index = self.push_addr(s.symbol, s.addend);
            self.span_index.push(index);
        }
    }

    /// The unit header (DWARF 5 §7.5.1.1), its length patched later; where
    /// it starts in `info`.
    fn unit_header(&mut self) -> usize {
        let unit_start = self.info.len();
        self.unit_start = unit_start;
        self.info.extend_from_slice(&[0; 4]);
        self.info.extend_from_slice(&5u16.to_le_bytes());
        self.info.push(DW_UT_COMPILE);
        self.info.push(8);
        self.sec_offset(Target::Abbrev, 0);
        debug_assert_eq!(self.info.len() - unit_start, UNIT_HEADER);
        unit_start
    }

    /// Empties the unit's address pool and its thread-local entries.
    pub(crate) fn clear_addrs(&mut self) {
        self.addrs.clear();
        self.addr_index.clear();
        self.tls_addrs.clear();
    }

    /// WP-225 (§6.31): the base types of the unit's `DW_OP_convert`s are
    /// its first children, as `llc`'s `createBaseTypeDIEs` puts them, so
    /// that their offsets fit the operands' four bytes. A unit without
    /// children has none: they come from its variables' lists. Then the
    /// walk of the other children starts (`stack`).
    fn base_types(&mut self, plan: &UnitPlan<'_>, unit_start: usize) -> Result<(), Error> {
        self.base_type_dies.clear();
        self.stack.clear();
        if plan.dies.len() > 1 {
            self.stack.push((0, plan.children_of(0).start));
        }
        for &(bits, encoding) in &plan.base_types {
            let offset =
                u32::try_from(self.info.len() - unit_start).map_err(|_| Error::TooLarge)?;
            self.base_type_dies.push(offset);
            self.abbrev(code(fam::BASE_TYPE, 0, false));
            self.strx(crate::loclists::base_type_name(bits, encoding))?;
            self.info.push(encoding);
            write_uleb(&mut self.info, u64::from(bits).div_ceil(8));
        }
        Ok(())
    }

    pub(crate) fn unit(&mut self, u: &UnitInput<'a, 'a>, symbols: u32) -> Result<(), Error> {
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
        // `.debug_addr`: one entry per span, then the DIEs' distinct
        // addresses in DIE order (`begin_functions`).
        let globals = u
            .globals
            .iter()
            .filter(|g| plan.die_of_expr(g.expr).is_some_and(|d| plan.global_takes_address(d)))
            .count()
            + plan.template_addrs();
        self.clear_addrs();
        self.offsets.clear();
        self.offsets.resize(n, NONE);
        self.patches.clear();
        self.strings.begin_unit();
        self.strings.use_store(plan.debug);

        let str_offsets_base = self.str_offsets.len() + 8;
        let addr_base = self.addr.len() + 8;
        let rnglists_base = self.rnglists.len() + 12;
        let unit_start = self.unit_header();

        let ranges = self.spans.len() > 1;
        let (has_addr, bound) = self.begin_functions(plan, globals);
        let lists = plan.scope_lists(); // WP-163: numbered before the unit's

        // The unit DIE.
        let cu = &plan.cu;
        let pc = self.spans.len() == 1;
        // One span and scope lists: `DW_AT_rnglists_base`, no `ranges`
        // (version 4 has no base: `crate::dwarf4` reads the table).
        let pc_lists = pc && lists > 0 && plan.version == 5;
        let mask = u32::from(pc) | u32::from(ranges) << 1 | u32::from(has_addr) << 2;
        let (family, mask) =
            if pc_lists { (fam::COMPILE_UNIT_LISTS, 0) } else { (fam::COMPILE_UNIT, mask) };
        self.offsets[0] = UNIT_HEADER as u32;
        self.abbrev(code(family, mask, n > 1));
        self.strx(&plan.producer)?;
        self.info.extend_from_slice(&cu.language.to_le_bytes());
        self.strx(plan.name)?;
        self.sec_offset(Target::StrOffsets, str_offsets_base as i64);
        self.sec_offset(Target::Line, i64::from(u.line_offset));
        self.strx(plan.comp_dir)?;
        let low_pc_at = if pc { Some(self.reserve_low_pc(bound)?) } else { None };
        if ranges {
            self.info.extend_from_slice(&[0; 8]); // DW_AT_low_pc 0, no relocation
            write_uleb(&mut self.info, lists as u64); // the unit's list, the last
        }
        if has_addr {
            self.sec_offset(Target::Addr, addr_base as i64);
        }
        if ranges || pc_lists {
            self.sec_offset(Target::Rnglists, rnglists_base as i64);
        }

        // The children, depth first, after the base types (WP-225).
        self.base_types(plan, unit_start)?;
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
        if self.block_starts.len() != lists {
            return Err(unsupported(plan.unit, "a scope range list the plan did not count"));
        }

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

        self.contributions(has_addr, ranges)?;
        if let Some((at, width)) = low_pc_at {
            put_uleb_padded(&mut self.info[at..at + width], u64::from(self.span_index[0]));
        }
        Ok(())
    }

    /// The unit's contributions to `.debug_str_offsets`, `.debug_addr`
    /// and `.debug_rnglists`, after its DIEs; its `.debug_loclists`
    /// contribution is closed (WP-162).
    pub(crate) fn contributions(&mut self, has_addr: bool, ranges: bool) -> Result<(), Error> {
        self.finish_loclists()?;
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
        self.pool_spans();
        if has_addr {
            let length = u32::try_from(4 + 8 * self.addrs.len()).map_err(|_| Error::TooLarge)?;
            self.addr.extend_from_slice(&length.to_le_bytes());
            self.addr.extend_from_slice(&5u16.to_le_bytes());
            self.addr.extend_from_slice(&[8, 0]);
            for (i, &(symbol, addend)) in self.addrs.iter().enumerate() {
                let offset = self.addr.len();
                // WP-189: a split unit's thread-local offset (§6.24).
                let tls = self.tls_addrs.binary_search(&(i as u32)).is_ok();
                let target = if tls { Target::Tls(symbol) } else { Target::Symbol(symbol) };
                self.relocs.push(Reloc { sec: Sec::Addr, offset, target, addend });
                self.addr.extend_from_slice(&[0; 8]);
            }
        }

        // `.debug_rnglists` (DWARF 5 §7.28), as `llc` writes it (WP-163):
        // one offset per list, every `DW_AT_ranges` a `DW_FORM_rnglistx`
        // index of them; the scopes' lists in DIE order (§6.4), then the
        // unit's own, one `DW_RLE_startx_length` per span.
        let lists = self.block_starts.len() + usize::from(ranges);
        if lists > 0 {
            let start = self.rnglists.len();
            let table = 4 * lists;
            let count = u32::try_from(lists).map_err(|_| Error::TooLarge)?;
            self.rnglists.extend_from_slice(&[0; 4]);
            self.rnglists.extend_from_slice(&5u16.to_le_bytes());
            self.rnglists.extend_from_slice(&[8, 0]);
            self.rnglists.extend_from_slice(&count.to_le_bytes()); // offset_entry_count
            let own = ranges.then_some(self.block_lists.len());
            for &at in self.block_starts.iter().chain(own.iter()) {
                let offset = u32::try_from(table + at).map_err(|_| Error::TooLarge)?;
                self.rnglists.extend_from_slice(&offset.to_le_bytes());
            }
            self.rnglists.extend_from_slice(&self.block_lists);
            if ranges {
                for (s, &index) in self.spans.iter().zip(&self.span_index) {
                    self.rnglists.push(DW_RLE_STARTX_LENGTH);
                    write_uleb(&mut self.rnglists, u64::from(index));
                    write_uleb(&mut self.rnglists, s.size);
                }
                self.rnglists.push(DW_RLE_END_OF_LIST);
            }
            let length =
                u32::try_from(self.rnglists.len() - start - 4).map_err(|_| Error::TooLarge)?;
            self.rnglists[start..start + 4].copy_from_slice(&length.to_le_bytes());
        }
        Ok(())
    }

    /// Writes DIE `die` of `plan` (without its children).
    pub(crate) fn die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        children: bool,
    ) -> Result<(), Error> {
        let d = plan.dies[die as usize];
        self.offsets[die as usize] =
            u32::try_from(self.info.len() - self.unit_start).map_err(|_| Error::TooLarge)?;
        // WP-311: the DIEs whose writers read their node themselves, before
        // the node is decoded for the others.
        match d.kind {
            Kind::TemplateParam => return self.template_param_die(plan, d.node, children),
            Kind::Imported => return self.imported_die(plan, d.node),
            Kind::LocalParam | Kind::LocalVar | Kind::Block | Kind::Label => {
                return self.local_die(plan, die, d.kind, d.node);
            }
            Kind::AbstractSubprogram
            | Kind::AbstractParam
            | Kind::AbstractVar
            | Kind::AbstractLabel
            | Kind::AbstractBlock
            | Kind::Inlined => return self.inline_die(plan, die, d.kind, d.node, children),
            Kind::CallSite => return self.call_site_die(plan, die), // WP-113: §6.19
            // WP-320: the type DIEs whose writer does not read the node.
            Kind::Param | Kind::Unspecified | Kind::IndexType => {
                return self.type_die(plan, die, d, DiNode::Undefined, children);
            }
            Kind::Unit => unreachable!("the unit DIE is written by `unit`"),
            _ => {}
        }
        match (d.kind, plan.debug.node(d.node)) {
            (Kind::Variable, DiNode::GlobalVariable(v)) if plan.scope_dies.var(die).is_some() => {
                self.scope_variable_die(plan, die, d.node, &v, children)?;
            }
            (Kind::Variable, DiNode::GlobalVariable(v)) => {
                let file = match v.file {
                    Some(f) if v.line != 0 => Some(plan.file_index(f).ok_or(bad(d.node, "file"))?),
                    _ => None,
                };
                let addr = self.addr_of[die as usize];
                let mask = u32::from(!v.is_local)
                    | u32::from(file.is_some()) << 1
                    | u32::from(v.align >= 8) << 2
                    | u32::from(addr != NONE) << 3
                    | u32::from(!v.linkage_name.is_empty()) << 4;
                if v.name.is_empty() {
                    // A string literal's (clang 22): no name (§6.8).
                    self.abbrev(code(fam::GLOBAL_NAMELESS, mask, false));
                } else {
                    self.abbrev(code(fam::GLOBAL_VARIABLE, mask, false));
                    self.strx_of::<DiGlobalVariable>(plan.debug, d.node, v.name, |v| v.name())?;
                }
                self.ref_to(plan, d.node, v.ty.unwrap_or(DiId::NONE))?;
                if let Some(file) = file {
                    write_uleb(&mut self.info, u64::from(file));
                    write_uleb(&mut self.info, u64::from(v.line));
                }
                if v.align >= 8 {
                    write_uleb(&mut self.info, u64::from(v.align / 8));
                }
                if addr != NONE {
                    let (symbol, addend) = self.cur[addr as usize];
                    let index = u64::from(self.push_addr(symbol, addend));
                    write_uleb(&mut self.info, 1 + uleb_len(index) as u64);
                    self.info.push(DW_OP_ADDRX);
                    write_uleb(&mut self.info, index);
                }
                self.opt_name_of::<DiGlobalVariable>(plan.debug, d.node, v.linkage_name, |v| {
                    v.linkage_name()
                })?;
            }
            (Kind::Namespace, DiNode::Namespace(ns)) => {
                let mask = u32::from(!ns.name.is_empty()) | u32::from(ns.export_symbols) << 1;
                self.abbrev(code(fam::NAMESPACE, mask, children));
                self.opt_name_of::<DiNamespace>(plan.debug, d.node, ns.name, |v| v.name())?;
            }
            (Kind::Subprogram, DiNode::Subprogram(sp)) => {
                self.subprogram_die(plan, die, d.node, &sp, children)?;
            }
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
#[expect(clippy::too_many_lines, reason = "the writer's state, then every section in order")]
pub fn write_debug_info(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
) -> Result<DieSections, Error> {
    crate::dwarf4::check_dwarf5(units)?; // `NoUnit`, and WP-114: §6.20
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
        addr_index: HashMap::default(),
        spans: Vec::new(),
        span_index: Vec::new(),
        unit_start: 0,
        cur: Vec::new(),
        expr: Vec::new(),
        fn_addr: Vec::new(),
        block_lists: Vec::new(),
        block_starts: Vec::new(),
        loclists: Vec::new(),
        loclists_unit: crate::loclists::NO_LISTS,
        split: false,
        tls_addrs: Vec::new(),
        base_type_dies: Vec::new(),
    };
    if let Some(u) = units.iter().find(|u| !u.plan.sealed) {
        return Err(unsupported(u.plan.unit, "a unit plan that is not sealed"));
    }
    for u in units {
        w.unit(u, symbols)?;
    }
    let abbrev = write_abbrev(&w.used);
    let lens =
        [w.info.len(), w.str_offsets.len(), w.addr.len(), w.rnglists.len(), w.loclists.len()];
    if lens.iter().any(|&len| u32::try_from(len).is_err()) {
        return Err(Error::TooLarge);
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
    let addr = optional_section(obj, b".debug_addr", w.addr)?;
    let rnglists = optional_section(obj, b".debug_rnglists", w.rnglists)?;
    let loclists = optional_section(obj, b".debug_loclists", w.loclists)?; // WP-162
    obj.append_owned(abbrev_sec, abbrev);
    obj.append_owned(info, w.info);
    obj.append_owned(str_offsets, w.str_offsets);
    obj.append_owned(str_sec, w.strings.into_data());
    let sym_abbrev = obj.section_symbol(abbrev_sec);
    let sym_str = obj.section_symbol(str_sec);
    let sym_str_offsets = obj.section_symbol(str_offsets);
    let sym_line = obj.section_symbol(line);
    let sym_addr = addr.map(|a| obj.section_symbol(a));
    let sym_rnglists = rnglists.map(|r| obj.section_symbol(r));
    let sym_loclists = loclists.map(|l| obj.section_symbol(l));
    obj.reserve(0, 0, w.relocs.len(), 0);
    for r in &w.relocs {
        let (symbol, wide) = match r.target {
            Target::Abbrev => (sym_abbrev, false),
            Target::Str => (sym_str, false),
            Target::StrOffsets => (sym_str_offsets, false),
            Target::Line => (sym_line, false),
            Target::Addr => (sym_addr.unwrap_or(sym_abbrev), false),
            Target::Rnglists => (sym_rnglists.unwrap_or(sym_abbrev), false),
            Target::Loclists => (sym_loclists.unwrap_or(sym_abbrev), false),
            Target::Symbol(s) | Target::Tls(s) => (s, true),
        };
        let section = match r.sec {
            Sec::Info => info,
            Sec::StrOffsets => str_offsets,
            Sec::Addr => addr.unwrap_or(info),
        };
        let kind = crate::x86_64::reloc_kind(r.target, wide, target); // WP-136
        obj.add_relocation(section, r.offset as u64, kind, symbol, r.addend);
    }
    Ok(DieSections {
        abbrev: abbrev_sec,
        info,
        str_offsets,
        str: str_sec,
        addr,
        rnglists,
        loclists,
    })
}

/// A `.debug_*` section holding `data`, unless `data` is empty.
fn optional_section(
    obj: &mut Object,
    name: &[u8],
    data: Vec<u8>,
) -> Result<Option<SectionId>, Error> {
    if data.is_empty() {
        return Ok(None);
    }
    let section = obj.add_section(name, SectionKind::Debug, 1)?;
    obj.append_owned(section, data);
    Ok(Some(section))
}
