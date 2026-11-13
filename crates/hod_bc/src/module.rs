//! The second pass (`spec/bc.md` §5): the records the first pass kept,
//! built into a `hod_ir::Module` in the order `hod_ll` decides things in,
//! with its decisions — what is delivered, what is dropped, what is noted
//! as unsupported and against which symbol.

use std::borrow::Cow;

use hod_ir::{
    AliasDesc, Arch, CallConv, ComdatId, ComdatKind, Const, ConstId, DiId, DiKind, FlagValue,
    FnAttrs, FnAttrsId, FnStr, FunctionDesc, IFuncDesc, Linkage, Module, ParamAttrs, Retained,
    SymId, TargetSpec, TlsModel, TypeId, TypeKind, UnnamedAddr, VariableDesc, Visibility,
};

use crate::attrs::{self, Attr, FUNCTION_INDEX, FnGroup, chars};
use crate::bitstream::{BitReader, BlockInfo, BlockSpan};
use crate::codes::{attr, md, module as mc};
use crate::consts::{GlobalValue, Values};
use crate::debug::{DebugBits, Dx, FnDebug, Handle, INVALID, LSlot, LocalBlock, LocalMd, Scratch};
use crate::error::{BcErrorKind, BcResult, fail, ir};
use crate::raw::{MdSlot, Raw, Rec};
use crate::types::{self, Types};

/// How to read a module.
#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    /// The target of a module that names none (`spec/ll.md` §3.1).
    pub default_target: Option<TargetSpec>,
}

/// A module read: the module level built, the function bodies found.
#[derive(Debug)]
pub struct BcModule<'a> {
    /// Everything but the function bodies.
    pub module: Module,
    /// What the function-block stage needs.
    pub bits: ModuleBits<'a>,
}

/// A function definition's body, not yet read.
#[derive(Clone, Copy, Debug)]
pub struct FunctionBits {
    /// The function.
    pub sym: SymId,
    /// Its function type.
    pub ty: TypeId,
    /// Its function type's bitcode type id.
    pub type_id: u64,
    /// Its FUNCTION block; read it with
    /// [`crate::bitstream::Cursor::enter_at`] and [`ModuleBits::block_info`].
    pub span: BlockSpan,
}

/// The tables a function-block reader needs (`spec/bc.md` §7): the
/// bodies' spans and what their records refer to by number.
#[derive(Debug)]
pub struct ModuleBits<'a> {
    data: &'a [u8],
    pub(crate) raw: Raw<'a>,
    pub(crate) types: Types,
    pub(crate) values: Values,
    /// Attribute group entries: (group id, index, record), sorted.
    pub(crate) groups: Vec<(u64, u64, Rec)>,
    numbered: Vec<SymId>,
    functions: Vec<FunctionBits>,
    md_kinds: Vec<(u64, Vec<u8>)>,
    /// Metadata strings, decoded: (start, len) by table and index.
    md_text: Vec<Vec<(u32, u32)>>,
    sync_scopes: Vec<Vec<u8>>,
    bundle_tags: Vec<Vec<u8>>,
    pub(crate) debug: DebugBits,
}

impl<'a> ModuleBits<'a> {
    /// The bytes read.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The abbreviations of BLOCKINFO, for cursors on the bodies.
    pub fn block_info(&self) -> &BlockInfo {
        &self.raw.info
    }

    /// The definitions, in the order of their FUNCTION records.
    pub fn functions(&self) -> &[FunctionBits] {
        &self.functions
    }

    /// Bitcode type id `id`.
    pub fn type_id(&self, id: u64) -> Option<TypeId> {
        usize::try_from(id).ok().and_then(|i| self.types.ids.get(i)).copied()
    }

    /// The number of module-level values (globals, then constants): the
    /// first value id of a function body.
    pub fn value_count(&self) -> usize {
        self.values.len()
    }

    /// The symbol of global value `id`; `None` for a constant or one of
    /// the special arrays.
    pub fn global(&self, id: u64) -> Option<SymId> {
        self.values.global(id).and_then(|global| global.sym)
    }

    /// The type of module-level value `id`.
    pub fn value_type(&self, id: u64) -> Option<TypeId> {
        self.values.type_of(id)
    }

    /// Module-level value `id` as a constant of `module` (the module this
    /// was read with), built if it was not.
    ///
    /// # Errors
    /// As [`read_module`] for a constant that cannot be built.
    pub fn constant(&mut self, module: &mut Module, id: u64) -> BcResult<ConstId> {
        let (raw, values) = (&self.raw, &mut self.values);
        values.constant(module, &raw.constants, &raw.pool, id, 0).map(|(built, _)| built)
    }

    /// The globals without a name, numbered as `llvm-dis` numbers them
    /// (variables, aliases, IFUNCs, functions).
    pub fn numbered_globals(&self) -> &[SymId] {
        &self.numbered
    }

    /// The metadata kinds: id and name.
    pub fn metadata_kinds(&self) -> &[(u64, Vec<u8>)] {
        &self.md_kinds
    }

    /// The sync-scope names, by number.
    pub fn sync_scopes(&self) -> &[Vec<u8>] {
        &self.sync_scopes
    }

    /// The operand-bundle tags, by number.
    pub fn bundle_tags(&self) -> &[Vec<u8>] {
        &self.bundle_tags
    }

    /// The operands of PARAMATTR list `index` (1-based, as records name
    /// them): its group ids.
    pub fn attr_list(&self, index: u64) -> Option<&[u64]> {
        let at = usize::try_from(index.checked_sub(1)?).ok()?;
        self.raw.attr_lists.get(at).map(|rec| rec.ops(&self.raw.pool))
    }

    /// The number of module-level metadata ids: function-local metadata
    /// (a body's METADATA block) is numbered from here.
    pub fn metadata_count(&self) -> usize {
        self.raw.md.len()
    }

    /// Module metadata `id`, as the module's METADATA blocks define it
    /// (`spec/bc.md` §11.8); `None` past [`Self::metadata_count`].
    pub fn metadata(&self, id: u64) -> Option<Metadata<'a, '_>> {
        let slot = *self.raw.md.get(usize::try_from(id).ok()?)?;
        Some(match slot {
            MdSlot::String { table, index } => {
                let (start, len) = *self.md_text.get(table as usize)?.get(index as usize)?;
                let blob = self.raw.md_strings.get(table as usize)?.blob;
                Metadata::String(blob.get(start as usize..)?.get(..len as usize)?)
            }
            MdSlot::Kept(rec) if rec.code == md::VALUE => match rec.ops(&self.raw.pool) {
                &[ty, value] => Metadata::Value { ty, value },
                _ => Metadata::Skipped,
            },
            MdSlot::Kept(rec) if rec.code == md::NODE || rec.code == md::DISTINCT_NODE => {
                Metadata::Node {
                    distinct: rec.code == md::DISTINCT_NODE,
                    operands: rec.ops(&self.raw.pool),
                }
            }
            MdSlot::Kept(_) => Metadata::Skipped,
        })
    }

    /// The producer string of the IDENTIFICATION block.
    pub fn producer(&self) -> Option<Vec<u8>> {
        self.raw.producer.and_then(|rec| chars(rec.ops(&self.raw.pool), rec.pos).ok())
    }
}

/// One module metadata id (see [`ModuleBits::metadata`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metadata<'a, 'm> {
    /// An `!"..."` string.
    String(&'a [u8]),
    /// A tuple: its operands as NODE records hold them, metadata id + 1,
    /// 0 for `null`.
    Node { distinct: bool, operands: &'m [u64] },
    /// A value as metadata: its bitcode type id and module value id.
    Value { ty: u64, value: u64 },
    /// A debug-info node or another record this reader does not keep.
    Skipped,
}

/// Reads the module level of the bitcode `data`.
///
/// # Errors
/// A [`crate::BcError`]: never a panic, for any `data`.
pub fn read_module<'a>(data: &'a [u8], options: &ReadOptions) -> BcResult<BcModule<'a>> {
    let raw = crate::raw::walk(data)?;
    let mut b = Build::new(data, &raw, options)?;
    b.run()?;
    let Build {
        module, types, values, groups, numbered, functions, md_kinds, md_text, debug, ..
    } = b;
    let sync_scopes = text_list(&raw, &raw.sync_scopes)?;
    let bundle_tags = text_list(&raw, &raw.bundle_tags)?;
    let bits = ModuleBits {
        data,
        raw,
        types,
        values,
        groups,
        numbered,
        functions,
        md_kinds,
        md_text,
        sync_scopes,
        bundle_tags,
        debug,
    };
    Ok(BcModule { module, bits })
}

fn text_list(raw: &Raw<'_>, recs: &[Rec]) -> BcResult<Vec<Vec<u8>>> {
    recs.iter().map(|rec| chars(rec.ops(&raw.pool), rec.pos)).collect()
}

/// The notes of the entity being built, each once (`hod_ll`'s `note`).
#[derive(Default)]
pub(crate) struct Notes(Vec<Cow<'static, str>>);

impl Notes {
    fn add(&mut self, construct: impl Into<Cow<'static, str>>) {
        let construct = construct.into();
        if !self.0.contains(&construct) {
            self.0.push(construct);
        }
    }

    fn bits(&mut self, bits: u8) {
        for name in types::note_names(bits) {
            self.add(name);
        }
    }

    fn flush(&mut self, module: &mut Module, sym: Option<SymId>) {
        for construct in self.0.drain(..) {
            module.record_unsupported(&construct, sym);
        }
    }
}

/// The AArch64 branch-protection flags (`spec/ll.md` §7.3).
#[derive(Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
struct Upgrade {
    bte: bool,
    pauth_lr: bool,
    gcs: bool,
    sign: bool,
    sign_all: bool,
    sign_b_key: bool,
}

/// The arrays that are not symbols.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Special {
    Ctors,
    Dtors,
    Used,
    CompilerUsed,
    /// `llvm.global.annotations` (WP-095, `hod_ll`'s).
    Annotations,
}

pub(crate) struct Build<'r, 'a> {
    data: &'a [u8],
    raw: &'r Raw<'a>,
    module: Module,
    types: Types,
    values: Values,
    comdats: Vec<ComdatId>,
    /// Attribute group entries: (group id, index, record), sorted.
    groups: Vec<(u64, u64, Rec)>,
    fn_groups: Vec<(u64, FnGroup)>,
    upgrade: Option<Upgrade>,
    upgraded: Vec<(FnAttrsId, FnAttrsId)>,
    numbered: Vec<SymId>,
    functions: Vec<FunctionBits>,
    md_kinds: Vec<(u64, Vec<u8>)>,
    /// Metadata strings, decoded: (table, start, len) by slot.
    md_text: Vec<Vec<(u32, u32)>>,
    notes: Notes,
    scratch: Vec<Attr<'r>>,
    /// The symbols described, by index.
    described: Vec<bool>,
    /// `!llvm.dbg.cu`'s operands and where it is.
    dbg_cu: Option<(Vec<u64>, u64)>,
    debug: DebugBits,
    dscratch: Scratch<'a>,
    local: LocalBlock<'a>,
    /// The metadata ids each definition's `!dbg` names, by definition.
    fn_dbg: Vec<Vec<Option<DiId>>>,
    /// The subprogram of the function being described.
    fn_subprogram: Option<DiId>,
    /// The definitions met.
    definitions: usize,
}

fn text(raw: &Raw<'_>, rec: Rec) -> BcResult<Vec<u8>> {
    chars(rec.ops(&raw.pool), rec.pos)
}

impl<'r, 'a> Build<'r, 'a> {
    fn new(data: &'a [u8], raw: &'r Raw<'a>, options: &ReadOptions) -> BcResult<Self> {
        let at = raw.module_pos;
        // The bitcode epoch has been 0 since LLVM 4; LLVM refuses others.
        if let Some(rec) = raw.epoch
            && rec.ops(&raw.pool) != [0]
        {
            return fail(
                BcErrorKind::Unsupported("epoch"),
                rec.pos,
                "a bitcode epoch other than 0",
            );
        }
        match raw.version.map(|rec| (rec.pos, rec.ops(&raw.pool))) {
            Some((_, [2])) => {}
            Some((pos, [version])) if *version < 2 => {
                return fail(
                    BcErrorKind::Unsupported("module-version"),
                    pos,
                    format!("module version {version}: a module without a string table"),
                );
            }
            Some((pos, _)) => {
                return fail(
                    BcErrorKind::Unsupported("module-version"),
                    pos,
                    "a module version other than 2",
                );
            }
            None => {
                return fail(
                    BcErrorKind::Unsupported("module-version"),
                    at,
                    "a module without its VERSION record",
                );
            }
        }
        let triple = raw.triple.map(|rec| text(raw, rec)).transpose()?;
        let target = match (triple, &options.default_target) {
            (Some(triple), _) => {
                let pos = raw.triple.map_or(at, |rec| rec.pos);
                ir(pos, TargetSpec::from_triple(&String::from_utf8_lossy(&triple)))?
            }
            (None, Some(target)) => target.clone(),
            (None, None) => {
                return fail(
                    BcErrorKind::Target,
                    at,
                    "the module names no target triple and no default target was given",
                );
            }
        };
        if let Some(rec) = raw.datalayout {
            ir(rec.pos, target.check_data_layout(&String::from_utf8_lossy(&text(raw, rec)?)))?;
        }
        let mut module = Module::new(target);
        for &rec in &raw.asm {
            // `llvm-dis` prints the module's assembly one line at a time.
            let asm = text(raw, rec)?;
            let mut rest: &[u8] = &asm;
            loop {
                let (line, after) = match rest.iter().position(|&c| c == b'\n') {
                    Some(at) => (&rest[..at], &rest[at + 1..]),
                    None => (rest, &[][..]),
                };
                module.append_module_asm(line);
                rest = after;
                if rest.is_empty() {
                    break;
                }
            }
        }
        let types = types::build(raw, &mut module)?;
        let values = Values::new(Vec::new(), &[], &[], &types)?;
        Ok(Build {
            data,
            raw,
            module,
            types,
            values,
            comdats: Vec::new(),
            groups: Vec::new(),
            fn_groups: Vec::new(),
            upgrade: Some(Upgrade::default()),
            upgraded: Vec::new(),
            numbered: Vec::new(),
            functions: Vec::new(),
            md_kinds: Vec::new(),
            md_text: Vec::new(),
            notes: Notes::default(),
            scratch: Vec::new(),
            described: Vec::new(),
            dbg_cu: None,
            debug: DebugBits::default(),
            dscratch: Scratch::default(),
            local: LocalBlock::default(),
            fn_dbg: Vec::new(),
            fn_subprogram: None,
            definitions: 0,
        })
    }

    fn pool(&self) -> &'r [u64] {
        &self.raw.pool
    }

    /// The name at `ops[0..2]` (string-table offset and size).
    fn strtab_name(&self, ops: &[u64], at: u64) -> BcResult<&'a [u8]> {
        let [offset, size, ..] = ops else {
            return fail(BcErrorKind::Invalid, at, "a record without its name");
        };
        if *size == 0 {
            return Ok(&[]);
        }
        let Some((_, strtab)) = self.raw.strtab else {
            return fail(
                BcErrorKind::Unsupported("no-strtab"),
                at,
                "a named global in a module without a string table",
            );
        };
        match offset.checked_add(*size).filter(|&end| end <= strtab.len() as u64) {
            Some(end) => Ok(&strtab[*offset as usize..end as usize]),
            None => fail(BcErrorKind::Invalid, at, "a name past the end of the string table"),
        }
    }

    fn run(&mut self) -> BcResult<()> {
        let mut notes = std::mem::take(&mut self.notes);
        notes.bits(self.types.defined_notes);
        notes.flush(&mut self.module, None);
        self.notes = notes;
        self.comdats()?;
        self.attr_groups()?;
        self.kinds()?;
        self.symbols()?;
        self.metadata()?;
        self.debug()?;
        self.items()?;
        self.finish_debug()?;
        ir(self.raw.module_pos, self.module.verify_symbols())
    }

    fn comdats(&mut self) -> BcResult<()> {
        for &rec in &self.raw.comdats {
            let ops = rec.ops(self.pool());
            let name = self.strtab_name(ops, rec.pos)?;
            let kind = match ops.get(2) {
                Some(1) => ComdatKind::Any,
                Some(2) => ComdatKind::ExactMatch,
                Some(3) => ComdatKind::Largest,
                Some(4) => ComdatKind::NoDeduplicate,
                Some(5) => ComdatKind::SameSize,
                _ => return fail(BcErrorKind::Invalid, rec.pos, "a COMDAT selection kind"),
            };
            if self.module.find_comdat(name).is_some() {
                return fail(BcErrorKind::Redefined, rec.pos, "redefinition of comdat");
            }
            let id = ir(rec.pos, self.module.comdat_named(name))?;
            self.module.set_comdat_kind(id, kind);
            self.comdats.push(id);
        }
        Ok(())
    }

    fn attr_groups(&mut self) -> BcResult<()> {
        for &rec in &self.raw.attr_groups {
            if rec.code != attr::GRP_ENTRY {
                return fail(
                    BcErrorKind::Unsupported("record"),
                    rec.pos,
                    format!("record code {} in a PARAMATTR_GROUP block", rec.code),
                );
            }
            let [id, index, ..] = rec.ops(self.pool()) else {
                return fail(
                    BcErrorKind::Invalid,
                    rec.pos,
                    "an attribute group without its id and index",
                );
            };
            self.groups.push((*id, *index, rec));
        }
        self.groups.sort_by_key(|&(id, index, _)| (id, index));
        if self.groups.windows(2).any(|w| (w[0].0, w[0].1) == (w[1].0, w[1].1)) {
            let at = self.raw.attr_groups.first().map_or(0, |rec| rec.pos);
            return fail(BcErrorKind::Redefined, at, "an attribute group defined twice");
        }
        for &rec in &self.raw.attr_lists {
            if rec.code != attr::ENTRY {
                return fail(
                    BcErrorKind::Unsupported("attribute-list-old"),
                    rec.pos,
                    "a PARAMATTR record other than a list of groups",
                );
            }
        }
        Ok(())
    }

    /// The groups of PARAMATTR list `list` (1-based; 0 for none).
    fn list(&self, list: u64, at: u64) -> BcResult<&'r [u64]> {
        if list == 0 {
            return Ok(&[]);
        }
        match usize::try_from(list - 1).ok().and_then(|i| self.raw.attr_lists.get(i)) {
            Some(rec) => Ok(rec.ops(&self.raw.pool)),
            None => fail(BcErrorKind::Invalid, at, format!("attribute list {list} is not defined")),
        }
    }

    /// The index and attributes of group `id`.
    fn group(&mut self, id: u64, at: u64) -> BcResult<(u64, Rec)> {
        match self.groups.binary_search_by_key(&id, |&(gid, _, _)| gid) {
            Ok(found) => {
                let (_, index, rec) = self.groups[found];
                Ok((index, rec))
            }
            Err(_) => {
                fail(BcErrorKind::Invalid, at, format!("attribute group {id} is not defined"))
            }
        }
    }

    /// The function-attribute group `id`, built once.
    fn fn_group(&mut self, id: u64, rec: Rec) -> BcResult<FnGroup> {
        if let Some((_, group)) = self.fn_groups.iter().find(|(gid, _)| *gid == id) {
            return Ok(group.clone());
        }
        let mut scratch = std::mem::take(&mut self.scratch);
        attrs::decode(&rec.ops(self.pool())[2..], rec.pos, &mut scratch)?;
        let group = attrs::fn_group(&mut self.module, &scratch, rec.pos);
        self.scratch = scratch;
        let group = group?;
        self.fn_groups.push((id, group.clone()));
        Ok(group)
    }

    fn kinds(&mut self) -> BcResult<()> {
        for &rec in &self.raw.md_kinds {
            let ops = rec.ops(self.pool());
            if rec.code != md::KIND || ops.is_empty() {
                return fail(
                    BcErrorKind::Invalid,
                    rec.pos,
                    "a METADATA_KIND block record other than a kind",
                );
            }
            self.md_kinds.push((ops[0], chars(&ops[1..], rec.pos)?));
        }
        Ok(())
    }

    fn kind_name(&self, kind: u64, at: u64) -> BcResult<&[u8]> {
        match self.md_kinds.iter().find(|(id, _)| *id == kind) {
            Some((_, name)) => Ok(name),
            None => fail(BcErrorKind::Invalid, at, format!("metadata kind {kind} is not defined")),
        }
    }

    // ---- symbols ----

    /// Creates the symbol of every global value, in record order, which
    /// is the order of LLVM's module and `hod_ll`'s (`spec/bc.md` §5.5,
    /// `spec/ll.md` §5.7).
    fn symbols(&mut self) -> BcResult<()> {
        let raw = self.raw;
        let mut globals = Vec::with_capacity(raw.globals.len());
        let mut unnamed: Vec<(u8, usize)> = Vec::new();
        let mut specials = 0u8;
        let mut numbered = Vec::new();
        // `llvm-dis` numbers the unnamed globals variables first, then
        // aliases, IFUNCs and functions; `hod_ll` names a non-local one
        // `__unnamed_K` by that number.
        for (index, &rec) in raw.globals.iter().enumerate() {
            let ops = rec.ops(self.pool());
            if self.strtab_name(ops, rec.pos)?.is_empty() {
                let order = match rec.code {
                    mc::GLOBALVAR => 0,
                    mc::FUNCTION => 3,
                    mc::ALIAS => 1,
                    _ => 2,
                };
                unnamed.push((order, index));
            }
        }
        unnamed.sort_unstable();
        for (index, &rec) in raw.globals.iter().enumerate() {
            let ops = rec.ops(self.pool());
            let name = self.strtab_name(ops, rec.pos)?;
            let field = |k: usize| ops.get(k).copied().unwrap_or(0);
            let (space, defined) = match rec.code {
                mc::GLOBALVAR => (field(3) >> 2, false),
                mc::FUNCTION => (field(18), field(4) == 0),
                _ => (field(3), false),
            };
            let Ok(space) = u32::try_from(space) else {
                return fail(BcErrorKind::Invalid, rec.pos, "an address space over 32 bits");
            };
            let ptr_ty = ir(rec.pos, self.module.types.ptr(space))?;
            let special = (rec.code == mc::GLOBALVAR).then(|| special_of(name)).flatten();
            let sym = if let Some(special) = special {
                let bit = 1 << special as u8;
                if specials & bit != 0 {
                    return fail(BcErrorKind::Redefined, rec.pos, "redefinition of global");
                }
                specials |= bit;
                None
            } else if name.is_empty() {
                let ordinal = unnamed.iter().position(|&(_, at)| at == index).unwrap_or(0);
                let local = matches!(ops.get(5), Some(3 | 9));
                let sym = if local {
                    ir(rec.pos, self.module.anonymous_symbol())?
                } else {
                    let name = format!("__unnamed_{}", ordinal + 1);
                    let sym = ir(rec.pos, self.module.symbol_named(name.as_bytes()))?;
                    self.module.record_unsupported("symbol:anonymous-external", Some(sym));
                    sym
                };
                numbered.push((ordinal, sym));
                Some(sym)
            } else {
                Some(ir(rec.pos, self.module.symbol_named(name))?)
            };
            globals.push(GlobalValue {
                sym,
                ptr_ty,
                defined_function: defined && rec.code == mc::FUNCTION,
            });
        }
        // The numbered globals in `llvm-dis`'s order of numbers.
        numbered.sort_unstable_by_key(|&(ordinal, _)| ordinal);
        self.numbered.extend(numbered.into_iter().map(|(_, sym)| sym));
        self.values = Values::new(globals, &raw.constants, &raw.pool, &self.types)?;
        Ok(())
    }

    fn constant(&mut self, id: u64, at: u64) -> BcResult<(ConstId, u8)> {
        let raw = self.raw;
        self.values.constant(&mut self.module, &raw.constants, &raw.pool, id, at)
    }

    /// Marks `sym` described; an error if it was.
    fn describe(&mut self, sym: SymId, at: u64) -> BcResult<()> {
        if self.described.len() <= sym.index() {
            self.described.resize(sym.index() + 1, false);
        }
        if std::mem::replace(&mut self.described[sym.index()], true) {
            let name = String::from_utf8_lossy(self.module.symbol(sym).name()).into_owned();
            return fail(BcErrorKind::Redefined, at, format!("redefinition of global '@{name}'"));
        }
        Ok(())
    }

    // ---- items ----

    fn items(&mut self) -> BcResult<()> {
        let raw = self.raw;
        // Attachments of variables and declarations.
        let mut attached: Vec<(u64, u64, u64, u64)> = Vec::new();
        for &rec in &raw.md_decl_attachments {
            let ops = rec.ops(self.pool());
            if ops.len() % 2 != 1 {
                return fail(
                    BcErrorKind::Invalid,
                    rec.pos,
                    "a global attachment record of the wrong length",
                );
            }
            for pair in ops[1..].chunks(2) {
                attached.push((ops[0], pair[0], pair[1], rec.pos));
            }
        }
        // By value, in record order.
        attached.sort_by_key(|&(value, ..)| value);
        let mut next_attached = 0;
        let mut dbg: Vec<Option<DiId>> = Vec::new();
        let mut definitions = raw.functions.iter();
        for (index, &rec) in raw.globals.iter().enumerate() {
            let id = index as u64;
            dbg.clear();
            while let Some(&(value, kind, md_id, at)) = attached.get(next_attached) {
                if value > id {
                    break;
                }
                next_attached += 1;
                if value < id {
                    continue;
                }
                let name = self.kind_name(kind, at)?;
                match name {
                    b"dbg" => dbg.push(self.debug_node(md_id)),
                    b"type" | b"vcall_visibility" | b"prof" | b"section_prefix" | b"annotation" => {
                    }
                    _ => {
                        let construct =
                            format!("global-metadata:{}", String::from_utf8_lossy(name));
                        self.notes.add(construct);
                    }
                }
            }
            match rec.code {
                mc::GLOBALVAR => {
                    self.variable_dbg(index, &dbg, rec.pos)?;
                    self.variable(index, rec)?;
                }
                mc::FUNCTION => {
                    let defined = rec.ops(self.pool()).get(4) == Some(&0);
                    let span = if defined {
                        match definitions.next() {
                            Some(&span) => Some(span),
                            None => {
                                return fail(
                                    BcErrorKind::Invalid,
                                    rec.pos,
                                    "a function definition without its FUNCTION block",
                                );
                            }
                        }
                    } else {
                        None
                    };
                    if defined {
                        if let Some(mds) = self.fn_dbg.get_mut(self.definitions) {
                            dbg.append(mds);
                        }
                        self.definitions += 1;
                    }
                    self.function_dbg(&dbg);
                    self.function(index, rec, span)?;
                }
                _ => self.indirect(index, rec)?,
            }
        }
        if definitions.next().is_some() {
            return fail(
                BcErrorKind::Invalid,
                raw.module_pos,
                "a FUNCTION block without its definition",
            );
        }
        Ok(())
    }

    /// The global-variable expressions a variable's `!dbg` names,
    /// attached; anything else is `debug:invalid` (as in `hod_ll`).
    fn variable_dbg(&mut self, index: usize, dbg: &[Option<DiId>], at: u64) -> BcResult<()> {
        let Some(sym) = self.sym(index) else { return Ok(()) };
        for &node in dbg {
            match node {
                Some(node) if self.module.debug.kind(node) == DiKind::GlobalVariableExpression => {
                    ir(at, self.module.attach_global_expr(sym, node))?;
                }
                _ => self.notes.add(INVALID),
            }
        }
        Ok(())
    }

    /// The subprogram a function's `!dbg` names, for its description.
    fn function_dbg(&mut self, dbg: &[Option<DiId>]) {
        self.fn_subprogram = None;
        for &node in dbg {
            match node {
                Some(node) if self.module.debug.kind(node) == DiKind::Subprogram => {
                    self.fn_subprogram = Some(node);
                }
                _ => self.notes.add(INVALID),
            }
        }
    }

    fn sym(&self, index: usize) -> Option<SymId> {
        self.values.globals[index].sym
    }

    #[allow(clippy::too_many_lines)] // One record's fields, in order.
    fn variable(&mut self, index: usize, rec: Rec) -> BcResult<()> {
        let at = rec.pos;
        let ops = rec.ops(self.pool());
        if ops.len() < 8 {
            return fail(BcErrorKind::Invalid, at, "a GLOBALVAR record cut short");
        }
        let field = |k: usize| ops.get(k).copied().unwrap_or(0);
        let flags = field(3);
        if flags & 2 == 0 {
            return fail(
                BcErrorKind::Unsupported("typed-pointers"),
                at,
                "a variable without its explicit value type",
            );
        }
        let ty = self.types.get(field(2), at)?;
        self.notes.bits(self.types.notes_of(field(2)));
        let linkage = linkage(field(5), at)?;
        let init = match field(4) {
            0 => None,
            value => {
                let (init, notes) = self.constant(value - 1, at)?;
                self.notes.bits(notes);
                Some(init)
            }
        };
        if init.is_none() && !matches!(linkage, Linkage::External | Linkage::ExternWeak) {
            return fail(
                BcErrorKind::Invalid,
                at,
                "a variable without an initializer whose linkage needs one",
            );
        }
        let align = alignment(field(6), at)?;
        let section = self.section(field(7), at)?;
        let visibility = visibility(field(8), at)?;
        let tls = tls(field(9), at)?;
        let unnamed_addr = unnamed_addr(field(10), at)?;
        if field(12) != 0 {
            self.notes.add("dll-storage");
        }
        let comdat = self.comdat(field(13), at)?;
        // Attributes of a variable: none is kept (`hod_ll`'s
        // `variable_properties`).
        for &group in self.list(field(14), at)? {
            let (gindex, grec) = self.group(group, at)?;
            if gindex != FUNCTION_INDEX {
                continue;
            }
            let built = self.fn_group(group, grec)?;
            for name in &built.rejected {
                self.notes.add(format!("attr:{name}"));
            }
            if built.kept_any {
                self.notes.add("global-attributes");
            }
        }
        let dso_local = field(15) != 0;
        if field(17) != 0 {
            self.notes.add("partition");
        }
        if field(18) & 4 != 0 {
            self.notes.add("global-sanitize-memtag");
        }
        if field(19) != 0 {
            self.notes.add("global-code-model");
        }
        let Some(sym) = self.sym(index) else {
            let name = self.strtab_name(ops, at)?;
            let special = special_of(name).expect("only the special arrays have no symbol");
            if special == Special::Annotations {
                // `llc` emits nothing of a variable in `llvm.metadata`.
                if section.as_deref() != Some(&b"llvm.metadata"[..]) {
                    self.notes.add("special-global:llvm.global.annotations");
                }
            } else if let Some(init) = init {
                self.special(special, init, at)?;
            }
            let mut notes = std::mem::take(&mut self.notes);
            notes.flush(&mut self.module, None);
            self.notes = notes;
            return Ok(());
        };
        self.describe(sym, at)?;
        let name = self.module.symbol(sym).name().to_vec();
        if name.starts_with(b"llvm.")
            && name.iter().all(|&c| c.is_ascii_alphanumeric() || b"-._$".contains(&c))
        {
            self.notes.add(format!("special-global:{}", String::from_utf8_lossy(&name)));
        }
        let desc = VariableDesc {
            linkage,
            visibility,
            dso_local,
            unnamed_addr,
            section: section.as_deref(),
            comdat,
            align,
            ty,
            constant: flags & 1 != 0,
            tls,
            addr_space: match self.module.types.kind(self.values.globals[index].ptr_ty) {
                TypeKind::Ptr { addr_space } => addr_space,
                _ => 0,
            },
            init,
        };
        ir(at, self.module.set_variable(sym, &desc))?;
        self.flush(sym);
        Ok(())
    }

    fn flush(&mut self, sym: SymId) {
        let mut notes = std::mem::take(&mut self.notes);
        notes.flush(&mut self.module, Some(sym));
        self.notes = notes;
    }

    fn section(&self, field: u64, at: u64) -> BcResult<Option<Vec<u8>>> {
        if field == 0 {
            return Ok(None);
        }
        match usize::try_from(field - 1).ok().and_then(|i| self.raw.section_names.get(i)) {
            Some(&rec) => Ok(Some(text(self.raw, rec)?)),
            None => fail(BcErrorKind::Invalid, at, format!("section name {field} is not defined")),
        }
    }

    fn comdat(&self, field: u64, at: u64) -> BcResult<Option<ComdatId>> {
        if field == 0 {
            return Ok(None);
        }
        match usize::try_from(field - 1).ok().and_then(|i| self.comdats.get(i)) {
            Some(&id) => Ok(Some(id)),
            None => fail(BcErrorKind::Invalid, at, format!("comdat {field} is not defined")),
        }
    }

    /// Delivers one of the special arrays (`hod_ll`'s `special`).
    fn special(&mut self, which: Special, init: ConstId, at: u64) -> BcResult<()> {
        let consts = &self.module.consts;
        let elems: Vec<ConstId> = match consts.get(init) {
            Const::Aggregate { elems, .. } => elems.to_vec(),
            Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Vec::new(),
            _ => {
                return fail(
                    BcErrorKind::Type,
                    at,
                    "this special array is not an array of its members",
                );
            }
        };
        let symbol_of = |module: &Module, id: ConstId| match module.consts.get(id) {
            Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
            Const::Null(_) => Ok(None),
            _ => fail(BcErrorKind::Type, at, "a member of this special array is not a symbol"),
        };
        for elem in elems {
            match which {
                Special::Used | Special::CompilerUsed => {
                    let Some(sym) = symbol_of(&self.module, elem)? else {
                        return fail(BcErrorKind::Type, at, "a used symbol cannot be null");
                    };
                    let how = if which == Special::Used {
                        Retained::Used
                    } else {
                        Retained::CompilerUsed
                    };
                    ir(at, self.module.retain(sym, how))?;
                }
                Special::Annotations => {}
                Special::Ctors | Special::Dtors => {
                    let fields = match self.module.consts.get(elem) {
                        Const::Aggregate { elems, .. } if matches!(elems.len(), 2 | 3) => {
                            elems.to_vec()
                        }
                        Const::Zero(_) => continue,
                        _ => {
                            return fail(
                                BcErrorKind::Type,
                                at,
                                "an entry is { i32 priority, ptr function, ptr data }",
                            );
                        }
                    };
                    let Some(priority) = self.module.consts.int_value(fields[0]) else {
                        return fail(BcErrorKind::Type, at, "a priority is an integer constant");
                    };
                    let Some(func) = symbol_of(&self.module, fields[1])? else {
                        continue;
                    };
                    let associated = match fields.get(2) {
                        Some(&data) => symbol_of(&self.module, data)?,
                        None => None,
                    };
                    if which == Special::Ctors {
                        self.module.add_ctor(priority as u32, func, associated);
                    } else {
                        self.module.add_dtor(priority as u32, func, associated);
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // One record's fields, in order.
    fn function(&mut self, index: usize, rec: Rec, span: Option<BlockSpan>) -> BcResult<()> {
        let at = rec.pos;
        let ops = rec.ops(self.pool());
        if ops.len() < 10 {
            return fail(BcErrorKind::Invalid, at, "a FUNCTION record cut short");
        }
        let field = |k: usize| ops.get(k).copied().unwrap_or(0);
        let ty = self.types.get(field(2), at)?;
        let TypeKind::Function { params, .. } = self.module.types.kind(ty) else {
            return fail(
                BcErrorKind::Unsupported("typed-pointers"),
                at,
                "a function whose type is not a function type",
            );
        };
        let param_count = params.len();
        self.notes.bits(self.types.notes_of(field(2)));
        let Ok(cc) = u32::try_from(field(3)) else {
            return fail(BcErrorKind::Invalid, at, "a calling convention over 32 bits");
        };
        let cc = ir(at, CallConv::from_llvm(cc))?;
        let defined = field(4) == 0;
        let linkage = linkage(field(5), at)?;
        // Attributes: the function's group, and return and parameter
        // attributes as `llvm-dis` writes them in place.
        let mut fn_attrs: Option<FnAttrsId> = None;
        let mut group_align = None;
        let mut ret_attrs = ParamAttrs::NONE;
        let mut param_attrs = vec![ParamAttrs::NONE; param_count];
        let mut fn_groups = Vec::new();
        for &group in self.list(field(6), at)? {
            let (gindex, grec) = self.group(group, at)?;
            if gindex == FUNCTION_INDEX {
                fn_groups.push((group, grec));
                continue;
            }
            let mut scratch = std::mem::take(&mut self.scratch);
            attrs::decode(&grec.ops(self.pool())[2..], grec.pos, &mut scratch)?;
            let (types, notes) = (&self.types, &mut self.notes);
            let mut type_of = |id: u64| {
                notes.bits(types.notes_of(id));
                types.get(id, grec.pos)
            };
            let mut pending = Vec::new();
            let built = attrs::param_attrs(&scratch, grec.pos, &mut type_of, &mut |name| {
                pending.push(name);
            });
            self.scratch = scratch;
            for name in pending {
                self.notes.add(name);
            }
            let built = built?;
            if gindex == 0 {
                ret_attrs = built;
            } else {
                match usize::try_from(gindex - 1).ok().and_then(|i| param_attrs.get_mut(i)) {
                    Some(slot) => *slot = built,
                    None => {
                        return fail(
                            BcErrorKind::Invalid,
                            grec.pos,
                            "attributes of a parameter the function does not have",
                        );
                    }
                }
            }
        }
        for (group, grec) in fn_groups {
            let built = self.fn_group(group, grec)?;
            group_align = built.align.or(group_align);
            for name in &built.rejected {
                self.notes.add(format!("attr:{name}"));
            }
            fn_attrs = Some(match fn_attrs {
                None => built.attrs,
                Some(previous) => {
                    let mut merged = FnAttrs::new();
                    for id in [previous, built.attrs] {
                        merge(&mut merged, &self.module, id, at)?;
                    }
                    ir(at, self.module.intern_fn_attrs(&merged))?
                }
            });
        }
        let mut fn_attrs = match fn_attrs {
            Some(id) => id,
            None => ir(at, self.module.intern_fn_attrs(&FnAttrs::new()))?,
        };
        if defined
            && self.module.target().arch() == Arch::Aarch64
            && let Some(upgrade) = self.upgrade
        {
            fn_attrs = self.upgrade_attrs(fn_attrs, upgrade, at)?;
        }
        while param_attrs.last().is_some_and(|attrs| attrs.is_none()) {
            param_attrs.pop();
        }
        let align = alignment(field(7), at)?;
        let section = self.section(field(8), at)?;
        let visibility = visibility(field(9), at)?;
        if field(10) != 0 {
            self.notes.add("gc");
        }
        let unnamed_addr = unnamed_addr(field(11), at)?;
        if field(12) != 0 {
            self.notes.add("prologue-data");
            let (_, notes) = self.constant(field(12) - 1, at)?;
            self.notes.bits(notes);
        }
        if field(13) != 0 {
            self.notes.add("dll-storage");
        }
        let comdat = self.comdat(field(14), at)?;
        if field(15) != 0 {
            self.notes.add("prefix-data");
            let (_, notes) = self.constant(field(15) - 1, at)?;
            self.notes.bits(notes);
        }
        let mut personality = None;
        if field(16) != 0 {
            let (value, notes) = self.constant(field(16) - 1, at)?;
            self.notes.bits(notes);
            match self.module.consts.get(value) {
                Const::Addr { sym, addend: 0 } => personality = Some(sym),
                _ => self.notes.add("personality-not-a-symbol"),
            }
        }
        let dso_local = field(17) != 0;
        let Ok(addr_space) = u32::try_from(field(18)) else {
            return fail(BcErrorKind::Invalid, at, "an address space over 32 bits");
        };
        if field(20) != 0 {
            self.notes.add("partition");
        }
        let sym = self.sym(index).expect("every function has a symbol");
        self.describe(sym, at)?;
        let desc = FunctionDesc {
            linkage,
            visibility,
            dso_local,
            unnamed_addr,
            section: section.as_deref(),
            comdat,
            align: group_align.or(align),
            ty,
            cc,
            ret_attrs,
            param_attrs: &param_attrs,
            attrs: fn_attrs,
            personality,
            subprogram: self.fn_subprogram,
            addr_space,
            has_body: defined,
        };
        ir(at, self.module.set_function(sym, &desc))?;
        if let Some(span) = span {
            self.functions.push(FunctionBits { sym, ty, type_id: field(2), span });
        }
        self.flush(sym);
        Ok(())
    }

    /// `hod_ll`'s `upgrade_attrs` (`spec/ll.md` §7.3).
    fn upgrade_attrs(&mut self, id: FnAttrsId, upgrade: Upgrade, at: u64) -> BcResult<FnAttrsId> {
        if let Some(&(_, done)) = self.upgraded.iter().find(|(from, _)| *from == id) {
            return Ok(done);
        }
        let view = self.module.fn_attrs(id);
        let mut out = FnAttrs::new();
        ir(at, out.set(view.flags()))?;
        out.set_uwtable(view.uwtable());
        if let Some(bytes) = view.stack_align() {
            ir(at, out.set_stack_align(bytes))?;
        }
        let drop_sign = !upgrade.sign && view.string(FnStr::SignReturnAddress) == Some(b"none");
        for (key, value) in view.strings() {
            let value = match key {
                FnStr::SignReturnAddress | FnStr::SignReturnAddressKey if drop_sign => continue,
                FnStr::BranchTargetEnforcement
                | FnStr::BranchProtectionPauthLr
                | FnStr::GuardedControlStack => {
                    if value.eq_ignore_ascii_case(b"false") {
                        continue;
                    }
                    if value.eq_ignore_ascii_case(b"true") { b"" } else { value }
                }
                _ => value,
            };
            ir(at, out.set_string(key, value))?;
        }
        let mut default = |key: FnStr, wanted: bool, value: &[u8]| {
            if wanted && view.string(key).is_none() {
                return out.set_string(key, value);
            }
            Ok(())
        };
        let kind: &[u8] = if upgrade.sign_all { b"all" } else { b"non-leaf" };
        let sign_key: &[u8] = if upgrade.sign_b_key { b"b_key" } else { b"a_key" };
        ir(at, default(FnStr::SignReturnAddress, upgrade.sign, kind))?;
        ir(at, default(FnStr::SignReturnAddressKey, upgrade.sign, sign_key))?;
        ir(at, default(FnStr::BranchTargetEnforcement, upgrade.bte, b""))?;
        ir(at, default(FnStr::BranchProtectionPauthLr, upgrade.pauth_lr, b""))?;
        ir(at, default(FnStr::GuardedControlStack, upgrade.gcs, b""))?;
        let done = ir(at, self.module.intern_fn_attrs(&out))?;
        self.upgraded.push((id, done));
        Ok(done)
    }

    fn indirect(&mut self, index: usize, rec: Rec) -> BcResult<()> {
        let at = rec.pos;
        let ops = rec.ops(self.pool());
        let is_alias = rec.code == mc::ALIAS;
        if ops.len() < if is_alias { 7 } else { 6 } {
            return fail(BcErrorKind::Invalid, at, "an ALIAS or IFUNC record cut short");
        }
        let field = |k: usize| ops.get(k).copied().unwrap_or(0);
        let ty = self.types.get(field(2), at)?;
        self.notes.bits(self.types.notes_of(field(2)));
        let (target, notes) = self.constant(field(4), at)?;
        self.notes.bits(notes);
        let linkage = linkage(field(5), at)?;
        let visibility = visibility(field(6), at)?;
        let sym = self.sym(index).expect("every alias has a symbol");
        if is_alias {
            if field(7) != 0 {
                self.notes.add("dll-storage");
            }
            if field(12) != 0 {
                self.notes.add("partition");
            }
            self.describe(sym, at)?;
            let desc = AliasDesc {
                linkage,
                visibility,
                dso_local: field(10) != 0,
                unnamed_addr: unnamed_addr(field(9), at)?,
                tls: tls(field(8), at)?,
                ty,
                target,
            };
            ir(at, self.module.set_alias(sym, &desc))?;
        } else {
            if field(9) != 0 {
                self.notes.add("partition");
            }
            let Const::Addr { sym: resolver, addend: 0 } = self.module.consts.get(target) else {
                return fail(BcErrorKind::Type, at, "an ifunc's resolver is a function");
            };
            self.describe(sym, at)?;
            let desc = IFuncDesc {
                linkage,
                visibility,
                dso_local: field(7) != 0,
                unnamed_addr: UnnamedAddr::None,
                ty,
                resolver,
            };
            ir(at, self.module.set_ifunc(sym, &desc))?;
        }
        self.flush(sym);
        Ok(())
    }

    // ---- metadata ----

    /// Decodes the metadata strings' lengths.
    fn md_strings(&mut self) -> BcResult<()> {
        for table in &self.raw.md_strings {
            let mut bits = BitReader::new(table.blob);
            let offset = table.offset as usize;
            if bits.seek(0, offset as u64 * 8).is_err() {
                return fail(
                    BcErrorKind::Invalid,
                    table.pos,
                    "a METADATA_STRINGS offset past its blob",
                );
            }
            let mut spans = Vec::with_capacity(table.count as usize);
            let mut start = offset as u64;
            for _ in 0..table.count {
                let Ok(len) = bits.vbr(6) else {
                    return fail(
                        BcErrorKind::Invalid,
                        table.pos,
                        "METADATA_STRINGS lengths cut short",
                    );
                };
                let end = start.saturating_add(len);
                if end > table.blob.len() as u64 {
                    return fail(
                        BcErrorKind::Invalid,
                        table.pos,
                        "a metadata string past its blob",
                    );
                }
                spans.push((start as u32, len as u32));
                start = end;
            }
            self.md_text.push(spans);
        }
        Ok(())
    }

    /// Metadata `id`'s slot.
    fn slot(&self, id: u64, at: u64) -> BcResult<MdSlot> {
        match usize::try_from(id).ok().and_then(|i| self.raw.md.get(i)) {
            Some(&slot) => Ok(slot),
            None => fail(BcErrorKind::Invalid, at, format!("metadata {id} is not defined")),
        }
    }

    /// Metadata `id` as a string, if it is one.
    fn md_string(&self, id: u64, at: u64) -> BcResult<Option<&'a [u8]>> {
        Ok(match self.slot(id, at)? {
            MdSlot::String { table, index } => {
                let (start, len) = self.md_text[table as usize][index as usize];
                Some(&self.raw.md_strings[table as usize].blob[start as usize..][..len as usize])
            }
            MdSlot::Kept(_) => None,
        })
    }

    /// The operands of tuple `id`, as `NODE` records hold them (id + 1).
    fn tuple(&self, id: u64, at: u64) -> BcResult<&'r [u64]> {
        match self.slot(id, at)? {
            MdSlot::Kept(rec) if rec.code == md::NODE || rec.code == md::DISTINCT_NODE => {
                Ok(rec.ops(&self.raw.pool))
            }
            _ => fail(BcErrorKind::Invalid, at, format!("metadata {id} is not a tuple")),
        }
    }

    /// The strings of tuple `id`.
    fn string_tuple(&self, id: u64, at: u64) -> BcResult<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        for &op in self.tuple(id, at)? {
            match op.checked_sub(1).map(|id| self.md_string(id, at)).transpose()?.flatten() {
                Some(text) => out.push(text.to_vec()),
                None => return fail(BcErrorKind::Invalid, at, "expected a metadata string"),
            }
        }
        Ok(out)
    }

    /// Metadata `id` as a constant, if it is a value.
    fn md_value(&mut self, id: u64, at: u64) -> BcResult<Option<ConstId>> {
        match self.slot(id, at)? {
            MdSlot::Kept(rec) if rec.code == md::VALUE => {
                let [_, value] = rec.ops(&self.raw.pool) else {
                    return fail(
                        BcErrorKind::Invalid,
                        rec.pos,
                        "a METADATA_VALUE record cut short",
                    );
                };
                let (built, notes) = self.constant(*value, rec.pos)?;
                self.notes.bits(notes);
                Ok(Some(built))
            }
            _ => Ok(None),
        }
    }

    fn metadata(&mut self) -> BcResult<()> {
        self.md_strings()?;
        let raw = self.raw;
        let mut upgrade = Some(Upgrade::default());
        for &(name_rec, node_rec) in &raw.md_names {
            let name = text(raw, name_rec)?;
            let at = name_rec.pos;
            let nodes = node_rec.ops(&raw.pool);
            match &name[..] {
                b"llvm.module.flags" => {
                    for &id in nodes {
                        self.module_flag(id, at, &mut upgrade)?;
                    }
                }
                b"llvm.linker.options" => {
                    for &id in nodes {
                        let strings = self.string_tuple(id, at)?;
                        let refs: Vec<&[u8]> = strings.iter().map(Vec::as_slice).collect();
                        ir(at, self.module.add_linker_option(&refs))?;
                    }
                }
                b"llvm.dependent-libraries" => {
                    for &id in nodes {
                        let strings = self.string_tuple(id, at)?;
                        let [library] = strings.as_slice() else {
                            return fail(
                                BcErrorKind::Invalid,
                                at,
                                "an operand of !llvm.dependent-libraries is one string",
                            );
                        };
                        ir(at, self.module.add_dependent_library(library))?;
                    }
                }
                b"llvm.ident" | b"llvm.errno.tbaa" => {}
                b"llvm.dbg.cu" => self.dbg_cu = Some((nodes.to_vec(), at)),
                other => {
                    let construct = format!("named-metadata:{}", String::from_utf8_lossy(other));
                    self.module.record_unsupported(&construct, None);
                }
            }
        }
        let mut notes = std::mem::take(&mut self.notes);
        notes.flush(&mut self.module, None);
        self.notes = notes;
        self.upgrade = upgrade;
        Ok(())
    }

    // ---- debug information (`spec/bc.md` §6) ----

    /// The reader of the debug records, over the module's block and the
    /// function block `local` (empty at the module level).
    fn dx<'x>(&'x mut self, local: &'x mut LocalBlock<'a>) -> Dx<'x, 'a> {
        Dx {
            module: &mut self.module,
            values: &mut self.values,
            raw: self.raw,
            md_text: &self.md_text,
            handles: &mut self.debug.module,
            lslots: &local.slots,
            lpool: &local.pool,
            lhandles: &mut local.handles,
            scratch: &mut self.dscratch,
            notes: 0,
        }
    }

    /// The module's debug nodes and every function's local ones, read in
    /// two passes (reserve, then define) so a node may name a later one.
    /// A module without debug information costs nothing here.
    fn debug(&mut self) -> BcResult<()> {
        let raw = self.raw;
        if !raw.md_debug && self.dbg_cu.is_none() {
            return Ok(());
        }
        self.debug.on = true;
        let count = raw.md.len() as u64;
        self.debug.module = vec![Handle::None; raw.md.len()];
        let mut local = std::mem::take(&mut self.local);
        let units = self.dbg_cu.take();
        let mut notes = 0;
        {
            let mut dx = self.dx(&mut local);
            dx.reserve(0, count)?;
            dx.define(0, count)?;
            if let Some((units, at)) = &units {
                dx.compile_units(units, *at)?;
            }
            notes |= dx.notes;
        }
        self.dbg_cu = units;
        let dbg_kind = self.md_kinds.iter().find(|(_, name)| name == b"dbg").map(|&(id, _)| id);
        let mut ops = Vec::new();
        self.fn_dbg.reserve(raw.functions.len());
        for &span in &raw.functions {
            local.slots.clear();
            local.pool.clear();
            local.handles.clear();
            let dbg = crate::debug::scan_function(
                self.data, &raw.info, span, dbg_kind, &mut ops, &mut local,
            )?;
            if local.slots.is_empty() {
                let subprogram = dbg.iter().map(|&id| self.debug_node(id)).collect();
                self.fn_dbg.push(subprogram);
                continue;
            }
            local.handles.resize(local.slots.len(), Handle::None);
            let len = local.slots.len() as u64;
            {
                let mut dx = self.dx(&mut local);
                dx.reserve(count, count + len)?;
                dx.define(count, count + len)?;
                notes |= dx.notes;
            }
            let (Ok(start), Ok(len32)) =
                (u32::try_from(self.debug.local.len()), u32::try_from(len))
            else {
                return fail(BcErrorKind::Limit, span.start, "over 2^32 local metadata ids");
            };
            for (slot, &handle) in local.slots.iter().zip(&local.handles) {
                let md = match *slot {
                    LSlot::String(bytes) => {
                        let start = self.debug.strs.len() as u32;
                        self.debug.strs.extend_from_slice(bytes);
                        LocalMd::String { start, len: bytes.len() as u32 }
                    }
                    LSlot::Rec { code, start, len, .. } => {
                        let ops = &local.pool[start as usize..][..len as usize];
                        match (code, ops) {
                            (md::VALUE, &[ty, value]) => LocalMd::Value { ty, value },
                            (md::NODE, []) => LocalMd::EmptyTuple,
                            (md::ARG_LIST, ops) => {
                                let start = self.debug.args.len() as u32;
                                self.debug.args.extend_from_slice(ops);
                                LocalMd::ArgList { start, len: ops.len() as u32 }
                            }
                            _ if handle != Handle::None => LocalMd::Debug(handle),
                            (md::NODE, ops) => {
                                let start = self.debug.args.len() as u32;
                                self.debug.args.extend_from_slice(ops);
                                LocalMd::Node { start, len: ops.len() as u32 }
                            }
                            _ => LocalMd::Other,
                        }
                    }
                };
                self.debug.local.push(md);
            }
            self.debug.functions.push((span.start, FnDebug { start, len: len32 }));
            let subprogram = dbg
                .iter()
                .map(|&id| match id.checked_sub(count) {
                    None => self.debug_node(id),
                    Some(local_id) => match local.handles.get(local_id as usize) {
                        Some(&Handle::Node(node)) => Some(node),
                        _ => None,
                    },
                })
                .collect();
            self.fn_dbg.push(subprogram);
        }
        self.local = local;
        let mut pending = std::mem::take(&mut self.notes);
        pending.bits(notes);
        pending.flush(&mut self.module, None);
        self.notes = pending;
        Ok(())
    }

    /// The debug node module metadata `id` is, if it is one.
    fn debug_node(&self, id: u64) -> Option<DiId> {
        match self.debug.module.get(usize::try_from(id).ok()?)? {
            Handle::Node(node) => Some(*node),
            _ => None,
        }
    }

    /// After the items, as `hod_ll` does: the tables completed and, when
    /// the module uses them, checked as LLVM's verifier checks them.
    fn finish_debug(&mut self) -> BcResult<()> {
        if self.module.debug.is_empty() && self.dbg_cu.is_none() {
            return Ok(());
        }
        ir(self.raw.module_pos, self.module.finish_debug())?;
        let used = self.dbg_cu.is_some()
            || !self.module.global_exprs().is_empty()
            || self.module.symbols().any(|symbol| {
                matches!(symbol.kind(), hod_ir::SymbolKind::Function { subprogram: Some(_), .. })
            });
        let version = self.module.props.debug_info_version;
        if used && (version != Some(3) || self.module.verify_debug().is_err()) {
            self.module.record_unsupported(INVALID, None);
        }
        Ok(())
    }

    fn module_flag(&mut self, id: u64, at: u64, upgrade: &mut Option<Upgrade>) -> BcResult<()> {
        let ops = self.tuple(id, at)?;
        let [behavior, name, value] = ops else {
            return fail(BcErrorKind::Invalid, at, "a module flag has three operands");
        };
        let behavior = match behavior.checked_sub(1) {
            Some(id) => self.md_value(id, at)?,
            None => None,
        };
        let Some(behavior) = behavior.and_then(|value| self.module.consts.int_value(value)) else {
            return fail(BcErrorKind::Invalid, at, "a module flag's behavior is an integer");
        };
        let Some(name) =
            name.checked_sub(1).map(|id| self.md_string(id, at)).transpose()?.flatten()
        else {
            return fail(BcErrorKind::Invalid, at, "a module flag's name is a string");
        };
        let mut int = None;
        let mut string = None;
        if let Some(value) = value.checked_sub(1) {
            if let Some(text) = self.md_string(value, at)? {
                string = Some(text);
            } else if let Some(built) = self.md_value(value, at)? {
                int = self.module.consts.int_value(built);
            }
        }
        let flag = match (int, string) {
            (Some(value), _) => FlagValue::Int(value),
            (None, Some(text)) => FlagValue::Str(text),
            (None, None) => FlagValue::Other,
        };
        let behavior = u32::try_from(behavior).unwrap_or(u32::MAX);
        ir(at, self.module.set_module_flag(behavior, name, flag))?;
        if let Some(value) = int {
            let field = upgrade.as_mut().and_then(|up| match name {
                b"branch-target-enforcement" => Some(&mut up.bte),
                b"branch-protection-pauth-lr" => Some(&mut up.pauth_lr),
                b"guarded-control-stack" => Some(&mut up.gcs),
                b"sign-return-address" => Some(&mut up.sign),
                b"sign-return-address-all" => Some(&mut up.sign_all),
                b"sign-return-address-with-bkey" => Some(&mut up.sign_b_key),
                _ => None,
            });
            if let Some(field) = field {
                if value == 2 {
                    *upgrade = None;
                } else {
                    *field = value == 1;
                }
            }
        }
        Ok(())
    }
}

/// Adds the attributes `id` of `module` to `out`.
fn merge(out: &mut FnAttrs, module: &Module, id: FnAttrsId, at: u64) -> BcResult<()> {
    let view = module.fn_attrs(id);
    ir(at, out.set(view.flags()))?;
    if view.uwtable() != hod_ir::UwTable::None {
        out.set_uwtable(view.uwtable());
    }
    if let Some(bytes) = view.stack_align() {
        ir(at, out.set_stack_align(bytes))?;
    }
    for (key, value) in view.strings() {
        ir(at, out.set_string(key, value))?;
    }
    Ok(())
}

fn special_of(name: &[u8]) -> Option<Special> {
    match name {
        b"llvm.global_ctors" => Some(Special::Ctors),
        b"llvm.global_dtors" => Some(Special::Dtors),
        b"llvm.used" => Some(Special::Used),
        b"llvm.compiler.used" => Some(Special::CompilerUsed),
        b"llvm.global.annotations" => Some(Special::Annotations),
        _ => None,
    }
}

/// A linkage code (`getDecodedLinkage`), the older codes included.
fn linkage(code: u64, at: u64) -> BcResult<Linkage> {
    Ok(match code {
        0 => Linkage::External,
        2 => Linkage::Appending,
        3 => Linkage::Internal,
        7 => Linkage::ExternWeak,
        8 => Linkage::Common,
        9 => Linkage::Private,
        12 => Linkage::AvailableExternally,
        1 | 16 => Linkage::WeakAny,
        10 | 17 => Linkage::WeakOdr,
        4 | 18 => Linkage::LinkOnceAny,
        11 | 19 => Linkage::LinkOnceOdr,
        _ => return fail(BcErrorKind::Unsupported("linkage"), at, format!("linkage code {code}")),
    })
}

fn visibility(code: u64, at: u64) -> BcResult<Visibility> {
    Ok(match code {
        0 => Visibility::Default,
        1 => Visibility::Hidden,
        2 => Visibility::Protected,
        _ => return fail(BcErrorKind::Invalid, at, format!("visibility code {code}")),
    })
}

fn tls(code: u64, at: u64) -> BcResult<TlsModel> {
    Ok(match code {
        0 => TlsModel::None,
        1 => TlsModel::GeneralDynamic,
        2 => TlsModel::LocalDynamic,
        3 => TlsModel::InitialExec,
        4 => TlsModel::LocalExec,
        _ => return fail(BcErrorKind::Invalid, at, format!("thread-local model code {code}")),
    })
}

fn unnamed_addr(code: u64, at: u64) -> BcResult<UnnamedAddr> {
    Ok(match code {
        0 => UnnamedAddr::None,
        1 => UnnamedAddr::Global,
        2 => UnnamedAddr::Local,
        _ => return fail(BcErrorKind::Invalid, at, format!("unnamed_addr code {code}")),
    })
}

/// An alignment field: log2 + 1, 0 for none.
fn alignment(field: u64, at: u64) -> BcResult<Option<u64>> {
    match field {
        0 => Ok(None),
        1..=33 => Ok(Some(1 << (field - 1))),
        _ => fail(BcErrorKind::Invalid, at, format!("alignment field {field}")),
    }
}

/// The debug-record reader over the module `data`, read up to its
/// bodies, for the unit tests of `crate::debug`.
#[cfg(test)]
pub(crate) fn with_dx<R>(data: &[u8], f: impl FnOnce(&mut Dx<'_, '_>) -> R) -> R {
    let raw = crate::raw::walk(data).expect("the test module walks");
    let mut b = Build::new(data, &raw, &ReadOptions::default()).expect("the test module");
    b.run().expect("the test module reads");
    let mut local = LocalBlock::default();
    let mut dx = b.dx(&mut local);
    f(&mut dx)
}
