//! The symbol table (`spec/ir.md` §2.2): functions, variables, aliases and
//! IFUNCs in one [`SymId`] space.
//!
//! A symbol is created by name ([`SymbolTable::named`]) and can be
//! referred to by id from then on; what it *is* arrives separately, as a
//! description ([`VariableDesc`], [`FunctionDesc`], [`AliasDesc`],
//! [`IFuncDesc`]). So a producer that meets a call to `@f` before the
//! definition of `@f` asks for the id, uses it, and describes the symbol
//! when it gets there. The states and the transitions between them are
//! §2.2's; in short, a description replaces whatever a symbol was unless
//! it was a definition, and ids never change.
//!
//! An entry is 48 bytes. Names live in one byte buffer of the table, and
//! the name index maps straight to the id.

use std::fmt;

use crate::attrs::{CallConv, FnAttrsId, ParamAttrs, SigAttrTable, decode_align, encode_align};
use crate::constant::{Const, ConstPool};
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{ComdatId, ConstId, DiId, SymId, TypeId};
use crate::intern::Layering;
use crate::intern::{InternTable, next_id, pool_start};
use crate::strings::{StrId, StrPool, hash_bytes};
use crate::target::MAX_ADDR_SPACE;
use crate::types::{TypeKind, TypeTable};

/// A linkage: the eleven of LLVM, in LLVM's order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum Linkage {
    /// `external`: visible to other objects. The linkage of a declaration.
    #[default]
    External,
    /// `available_externally`: a definition for the optimizer's eyes; the
    /// object does not contain it.
    AvailableExternally,
    /// `linkonce`: weak, may be replaced by another definition, and
    /// discardable when unreferenced.
    LinkOnceAny,
    /// `linkonce_odr`: as `linkonce`, and all definitions are equivalent.
    LinkOnceOdr,
    /// `weak`: weak, may be replaced by another definition.
    WeakAny,
    /// `weak_odr`: as `weak`, and all definitions are equivalent.
    WeakOdr,
    /// `appending`: an array variable whose definitions are concatenated.
    Appending,
    /// `internal`: a local symbol.
    Internal,
    /// `private`: a local symbol that is not in the symbol table.
    Private,
    /// `extern_weak`: a declaration that may stay undefined.
    ExternWeak,
    /// `common`: a zero-initialized tentative definition.
    Common,
}

impl Linkage {
    /// The eleven linkages.
    pub const ALL: [Linkage; 11] = [
        Linkage::External,
        Linkage::AvailableExternally,
        Linkage::LinkOnceAny,
        Linkage::LinkOnceOdr,
        Linkage::WeakAny,
        Linkage::WeakOdr,
        Linkage::Appending,
        Linkage::Internal,
        Linkage::Private,
        Linkage::ExternWeak,
        Linkage::Common,
    ];

    /// The keyword LLVM's text uses.
    pub const fn keyword(self) -> &'static str {
        match self {
            Linkage::External => "external",
            Linkage::AvailableExternally => "available_externally",
            Linkage::LinkOnceAny => "linkonce",
            Linkage::LinkOnceOdr => "linkonce_odr",
            Linkage::WeakAny => "weak",
            Linkage::WeakOdr => "weak_odr",
            Linkage::Appending => "appending",
            Linkage::Internal => "internal",
            Linkage::Private => "private",
            Linkage::ExternWeak => "extern_weak",
            Linkage::Common => "common",
        }
    }

    /// `internal` or `private`: the symbol is local to the object.
    #[inline]
    pub const fn is_local(self) -> bool {
        matches!(self, Linkage::Internal | Linkage::Private)
    }

    /// Whether another object's definition may replace this one at link
    /// time with different contents (LLVM's `isInterposableLinkage`).
    #[inline]
    pub const fn is_interposable(self) -> bool {
        matches!(
            self,
            Linkage::WeakAny | Linkage::LinkOnceAny | Linkage::Common | Linkage::ExternWeak
        )
    }
}

/// An ELF symbol visibility.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum Visibility {
    /// `default`.
    #[default]
    Default,
    /// `hidden`.
    Hidden,
    /// `protected`.
    Protected,
}

/// Whether a symbol's address is significant.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum UnnamedAddr {
    /// The address is significant.
    #[default]
    None,
    /// `local_unnamed_addr`: not significant within this module.
    Local,
    /// `unnamed_addr`: not significant anywhere; equal constants may be
    /// merged.
    Global,
}

/// A thread-local storage model.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum TlsModel {
    /// Not thread-local.
    #[default]
    None,
    /// `thread_local`: general dynamic.
    GeneralDynamic,
    /// `thread_local(localdynamic)`.
    LocalDynamic,
    /// `thread_local(initialexec)`.
    InitialExec,
    /// `thread_local(localexec)`.
    LocalExec,
}

/// Which of LLVM's two "keep this" arrays a symbol is a member of.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Retained {
    /// `llvm.used`: neither the compiler nor the linker may discard it
    /// (the symbol's section gets `SHF_GNU_RETAIN`).
    Used,
    /// `llvm.compiler.used`: the compiler may not discard it; the linker
    /// may.
    CompilerUsed,
}

/// The description of a variable: `Module::set_variable`.
///
/// With `init` it is a definition, without a declaration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VariableDesc<'a> {
    /// The linkage.
    pub linkage: Linkage,
    /// The visibility.
    pub visibility: Visibility,
    /// `dso_local`. Implied, and stored as true, for a local linkage and
    /// for a non-default visibility unless the linkage is `extern_weak`.
    pub dso_local: bool,
    /// `unnamed_addr` or `local_unnamed_addr`.
    pub unnamed_addr: UnnamedAddr,
    /// The section name. `Some(b"")` means `None`.
    pub section: Option<&'a [u8]>,
    /// The COMDAT.
    pub comdat: Option<ComdatId>,
    /// The alignment in bytes: a power of two up to 2^32.
    pub align: Option<u64>,
    /// The value type.
    pub ty: TypeId,
    /// `constant`, not `global`.
    pub constant: bool,
    /// The thread-local model.
    pub tls: TlsModel,
    /// The address space. hod supports 0; another one is kept so that
    /// `classify` can refuse the symbol by name.
    pub addr_space: u32,
    /// The initializer, of type `ty`.
    pub init: Option<ConstId>,
}

impl VariableDesc<'_> {
    /// An external, writable declaration of type `ty` with every other
    /// field at its default.
    pub fn new(ty: TypeId) -> Self {
        VariableDesc {
            linkage: Linkage::External,
            visibility: Visibility::Default,
            dso_local: false,
            unnamed_addr: UnnamedAddr::None,
            section: None,
            comdat: None,
            align: None,
            ty,
            constant: false,
            tls: TlsModel::None,
            addr_space: 0,
            init: None,
        }
    }
}

/// The description of a function: `Module::set_function`.
///
/// With `has_body` it is a definition, without a declaration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FunctionDesc<'a> {
    /// The linkage.
    pub linkage: Linkage,
    /// The visibility.
    pub visibility: Visibility,
    /// `dso_local`; implied as for a variable.
    pub dso_local: bool,
    /// `unnamed_addr` or `local_unnamed_addr`.
    pub unnamed_addr: UnnamedAddr,
    /// The section name. `Some(b"")` means `None`.
    pub section: Option<&'a [u8]>,
    /// The COMDAT.
    pub comdat: Option<ComdatId>,
    /// The alignment in bytes: a power of two up to 2^32.
    pub align: Option<u64>,
    /// The function type.
    pub ty: TypeId,
    /// The calling convention.
    pub cc: CallConv,
    /// The attributes of the return value.
    pub ret_attrs: ParamAttrs,
    /// The attributes of the first `param_attrs.len()` parameters; the
    /// rest have none.
    pub param_attrs: &'a [ParamAttrs],
    /// The function attributes (`Module::intern_fn_attrs`).
    pub attrs: FnAttrsId,
    /// The personality function.
    pub personality: Option<SymId>,
    /// The `DISubprogram` (§6).
    pub subprogram: Option<DiId>,
    /// The address space; see [`VariableDesc::addr_space`].
    pub addr_space: u32,
    /// Whether a body will be delivered (§5.1).
    pub has_body: bool,
}

impl FunctionDesc<'_> {
    /// An external declaration of type `ty` with the C calling convention
    /// and no attributes.
    pub fn new(ty: TypeId) -> Self {
        FunctionDesc {
            linkage: Linkage::External,
            visibility: Visibility::Default,
            dso_local: false,
            unnamed_addr: UnnamedAddr::None,
            section: None,
            comdat: None,
            align: None,
            ty,
            cc: CallConv::C,
            ret_attrs: ParamAttrs::NONE,
            param_attrs: &[],
            attrs: FnAttrsId::EMPTY,
            personality: None,
            subprogram: None,
            addr_space: 0,
            has_body: false,
        }
    }
}

/// The description of an alias: `Module::set_alias`. Always a definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AliasDesc {
    /// The linkage: not `extern_weak`, `common` or `appending`.
    pub linkage: Linkage,
    /// The visibility.
    pub visibility: Visibility,
    /// `dso_local`; implied as for a variable.
    pub dso_local: bool,
    /// `unnamed_addr` or `local_unnamed_addr`.
    pub unnamed_addr: UnnamedAddr,
    /// The thread-local model.
    pub tls: TlsModel,
    /// The value type: what the alias is an alias *as*.
    pub ty: TypeId,
    /// The aliasee: a pointer constant that is a symbol address
    /// ([`Const::Addr`]), or what is left of a constant expression
    /// ([`Const::AddrOfInt`], [`Const::Expr`]).
    pub target: ConstId,
}

impl AliasDesc {
    /// An external alias of value type `ty` for `target`.
    pub fn new(ty: TypeId, target: ConstId) -> Self {
        AliasDesc {
            linkage: Linkage::External,
            visibility: Visibility::Default,
            dso_local: false,
            unnamed_addr: UnnamedAddr::None,
            tls: TlsModel::None,
            ty,
            target,
        }
    }
}

/// The description of an IFUNC: `Module::set_ifunc`. Always a definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IFuncDesc {
    /// The linkage: `external`, `internal`, `private`, or one of the four
    /// weak ones.
    pub linkage: Linkage,
    /// The visibility.
    pub visibility: Visibility,
    /// `dso_local`; implied as for a variable.
    pub dso_local: bool,
    /// `unnamed_addr` or `local_unnamed_addr`.
    pub unnamed_addr: UnnamedAddr,
    /// The value type: the type of the function the resolver returns.
    pub ty: TypeId,
    /// The resolver: a defined function, or an alias of one.
    pub resolver: SymId,
}

impl IFuncDesc {
    /// An external IFUNC of value type `ty` resolved by `resolver`.
    pub fn new(ty: TypeId, resolver: SymId) -> Self {
        IFuncDesc {
            linkage: Linkage::External,
            visibility: Visibility::Default,
            dso_local: false,
            unnamed_addr: UnnamedAddr::None,
            ty,
            resolver,
        }
    }
}

/// What a symbol is, with the fields of its kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolKind<'a> {
    /// Named, and not described yet. `Module::verify_symbols` refuses a
    /// module that still has one.
    Unresolved,
    /// A variable.
    Variable {
        /// `constant`, not `global`.
        constant: bool,
        /// The thread-local model.
        tls: TlsModel,
        /// The initializer; `None` for a declaration.
        init: Option<ConstId>,
    },
    /// A function.
    Function {
        /// The calling convention.
        cc: CallConv,
        /// The function attributes.
        attrs: FnAttrsId,
        /// The attributes of the return value.
        ret_attrs: ParamAttrs,
        /// The attributes of the leading parameters, up to the last one
        /// that has any. [`Symbol::param_attrs`] reads one by index.
        param_attrs: &'a [ParamAttrs],
        /// The personality function.
        personality: Option<SymId>,
        /// The `DISubprogram`.
        subprogram: Option<DiId>,
        /// Whether a body will be delivered; false for a declaration.
        has_body: bool,
    },
    /// An alias.
    Alias {
        /// The thread-local model.
        tls: TlsModel,
        /// The aliasee.
        target: ConstId,
    },
    /// An IFUNC.
    IFunc {
        /// The resolver.
        resolver: SymId,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Kind {
    Unresolved,
    Variable,
    Function,
    Alias,
    IFunc,
}

/// "None" in an entry field that holds an optional id.
const NONE: u32 = u32::MAX;

const VISIBILITY_MASK: u16 = 0b11;
const UNNAMED_SHIFT: u16 = 2;
const UNNAMED_MASK: u16 = 0b11 << UNNAMED_SHIFT;
const DSO_LOCAL: u16 = 1 << 4;
const CONSTANT: u16 = 1 << 5;
const HAS_BODY: u16 = 1 << 6;
const USED: u16 = 1 << 7;
const COMPILER_USED: u16 = 1 << 8;
/// The flags that are not part of a description and survive one.
const STICKY: u16 = USED | COMPILER_USED;

/// A stored symbol: 48 bytes.
#[derive(Clone, Copy, Debug)]
struct Entry {
    name_start: u32,
    name_len: u32,
    /// The value type, or a function's function type.
    ty: TypeId,
    /// A `StrId` of the section pool, or `NONE`.
    section: u32,
    /// A `ComdatId`, or `NONE`.
    comdat: u32,
    /// Variable: the initializer or `NONE`. Function: the `FnAttrsId`.
    /// Alias: the target. IFUNC: the resolver.
    a: u32,
    /// Function: the parameter attribute list.
    b: u32,
    /// Function: the personality symbol or `NONE`.
    c: u32,
    /// Function: the subprogram or `NONE`.
    d: u32,
    addr_space: u32,
    cc: CallConv,
    flags: u16,
    kind: Kind,
    linkage: Linkage,
    tls: TlsModel,
    /// 0 for none, else log2(bytes) + 1.
    align: u8,
}

impl Entry {
    fn unresolved(name_start: u32, name_len: u32) -> Entry {
        Entry {
            name_start,
            name_len,
            ty: TypeId::VOID,
            section: NONE,
            comdat: NONE,
            a: NONE,
            b: SigAttrTable::EMPTY,
            c: NONE,
            d: NONE,
            addr_space: 0,
            cc: CallConv::C,
            flags: 0,
            kind: Kind::Unresolved,
            linkage: Linkage::External,
            tls: TlsModel::None,
            align: 0,
        }
    }

    fn is_definition(&self) -> bool {
        match self.kind {
            Kind::Unresolved => false,
            Kind::Variable => self.a != NONE,
            Kind::Function => self.flags & HAS_BODY != 0,
            Kind::Alias | Kind::IFunc => true,
        }
    }

    fn visibility(&self) -> Visibility {
        match self.flags & VISIBILITY_MASK {
            0 => Visibility::Default,
            1 => Visibility::Hidden,
            _ => Visibility::Protected,
        }
    }

    fn unnamed_addr(&self) -> UnnamedAddr {
        match (self.flags & UNNAMED_MASK) >> UNNAMED_SHIFT {
            0 => UnnamedAddr::None,
            1 => UnnamedAddr::Local,
            _ => UnnamedAddr::Global,
        }
    }
}

/// The fields every description has; the three that aliases and IFUNCs
/// lack are `None` for them.
struct Common<'a> {
    linkage: Linkage,
    visibility: Visibility,
    dso_local: bool,
    unnamed_addr: UnnamedAddr,
    section: Option<&'a [u8]>,
    comdat: Option<ComdatId>,
    align: Option<u64>,
}

fn optional(raw: u32) -> Option<u32> {
    if raw == NONE { None } else { Some(raw) }
}

/// The symbols of a module.
#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolTable {
    entries: Vec<Entry>,
    names: Vec<u8>,
    by_name: InternTable,
    sections: StrPool,
    sig_attrs: SigAttrTable,
}

impl SymbolTable {
    fn name(&self, index: u32) -> &[u8] {
        let entry = &self.entries[index as usize];
        &self.names[entry.name_start as usize..][..entry.name_len as usize]
    }

    /// `@name`, for messages.
    fn label(&self, sym: SymId) -> String {
        let name = self.name(sym.raw());
        if name.is_empty() {
            format!("the anonymous symbol {sym:?}")
        } else {
            format!("@{}", String::from_utf8_lossy(name))
        }
    }

    fn invalid<T>(&self, sym: SymId, what: &str) -> IrResult<T> {
        err(IrErrorKind::InvalidSymbol, format!("{}: {what}", self.label(sym)))
    }

    /// Number of symbols.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The symbol called `name`, if there is one.
    #[inline]
    pub(crate) fn find(&self, name: &[u8]) -> Option<SymId> {
        self.by_name.find(hash_bytes(name), |id| self.name(id) == name).map(SymId::new)
    }

    /// The symbol called `name`, created unresolved if new.
    pub(crate) fn named(&mut self, name: &[u8]) -> IrResult<SymId> {
        let hash = hash_bytes(name);
        if let Some(found) = self.by_name.find(hash, |id| self.name(id) == name) {
            return Ok(SymId::new(found));
        }
        if name.is_empty() {
            return err(
                IrErrorKind::InvalidSymbol,
                "a symbol name is not empty; an unnamed symbol is an anonymous one",
            );
        }
        if name.contains(&0) {
            return err(
                IrErrorKind::InvalidSymbol,
                format!("the symbol name {:?} has a zero byte", String::from_utf8_lossy(name)),
            );
        }
        let id = next_id(self.entries.len(), "symbol")?;
        let start = pool_start(self.names.len(), name.len(), "symbol name")?;
        self.names.extend_from_slice(name);
        self.entries.push(Entry::unresolved(start, name.len() as u32));
        self.by_name.insert(hash, id);
        Ok(SymId::new(id))
    }

    /// A new symbol without a name.
    pub(crate) fn anonymous(&mut self) -> IrResult<SymId> {
        let id = next_id(self.entries.len(), "symbol")?;
        self.entries.push(Entry::unresolved(self.names.len() as u32, 0));
        Ok(SymId::new(id))
    }

    /// The symbol `id`.
    #[inline]
    pub(crate) fn get(&self, id: SymId) -> Symbol<'_> {
        Symbol { table: self, entry: &self.entries[id.index()], id }
    }

    /// The checks every description shares, and the entry that results
    /// from them: the old entry's name and sticky flags with the common
    /// fields of the description.
    fn describe(&mut self, sym: SymId, common: &Common<'_>) -> IrResult<Entry> {
        let old = self.entries[sym.index()];
        if old.is_definition() {
            return err(
                IrErrorKind::SymbolRedefined,
                format!("{} is already defined", self.label(sym)),
            );
        }
        if old.name_len == 0 && !common.linkage.is_local() {
            return self.invalid(sym, "an anonymous symbol is internal or private");
        }
        if common.linkage.is_local() && common.visibility != Visibility::Default {
            return self.invalid(sym, "a symbol with local linkage has default visibility");
        }
        let align = match common.align {
            Some(bytes) => encode_align(bytes)?,
            None => 0,
        };
        let implied = common.linkage.is_local()
            || (common.visibility != Visibility::Default && common.linkage != Linkage::ExternWeak);
        let mut flags = old.flags & STICKY;
        flags |= common.visibility as u16;
        flags |= (common.unnamed_addr as u16) << UNNAMED_SHIFT;
        if common.dso_local || implied {
            flags |= DSO_LOCAL;
        }
        let section = match common.section {
            Some(name) if !name.is_empty() => self.sections.intern(name)?.raw(),
            _ => NONE,
        };
        let mut entry = Entry::unresolved(old.name_start, old.name_len);
        entry.flags = flags;
        entry.linkage = common.linkage;
        entry.section = section;
        entry.comdat = common.comdat.map_or(NONE, ComdatId::raw);
        entry.align = align;
        Ok(entry)
    }

    /// What must hold of a definition as soon as it is one.
    fn check_definition<L: Layering>(
        &self,
        sym: SymId,
        entry: &Entry,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
    ) -> IrResult<()> {
        match entry.linkage {
            Linkage::ExternWeak => {
                return self.invalid(sym, "a definition is not extern_weak");
            }
            Linkage::AvailableExternally if entry.comdat != NONE => {
                return self.invalid(sym, "an available_externally definition is not in a COMDAT");
            }
            Linkage::Common => {
                if entry.flags & CONSTANT != 0 {
                    return self.invalid(sym, "a common variable is not constant");
                }
                if entry.comdat != NONE {
                    return self.invalid(sym, "a common variable is not in a COMDAT");
                }
                if !consts.is_zero(ConstId::new(entry.a)) {
                    return self.invalid(sym, "a common variable has a zero initializer");
                }
            }
            _ => {}
        }
        if entry.kind == Kind::Variable && !types.is_sized(entry.ty) {
            return err(
                IrErrorKind::UnsizedType,
                format!("{}: a defined variable has a sized type", self.label(sym)),
            );
        }
        Ok(())
    }

    fn check_addr_space(&self, sym: SymId, addr_space: u32) -> IrResult<()> {
        if addr_space > MAX_ADDR_SPACE {
            return self.invalid(sym, &format!("{addr_space} is not an address space"));
        }
        Ok(())
    }

    /// Makes `sym` the variable `desc` describes.
    pub(crate) fn set_variable<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
        sym: SymId,
        desc: &VariableDesc<'_>,
    ) -> IrResult<()> {
        let mut entry = self.describe(
            sym,
            &Common {
                linkage: desc.linkage,
                visibility: desc.visibility,
                dso_local: desc.dso_local,
                unnamed_addr: desc.unnamed_addr,
                section: desc.section,
                comdat: desc.comdat,
                align: desc.align,
            },
        )?;
        let kind = types.kind(desc.ty);
        let no_value = matches!(
            kind,
            TypeKind::Void
                | TypeKind::Label
                | TypeKind::Metadata
                | TypeKind::Token
                | TypeKind::Function { .. }
        );
        if no_value || types.is_scalable(desc.ty) {
            return err(
                IrErrorKind::InvalidType,
                format!("{}: a variable cannot have the type {kind:?}", self.label(sym)),
            );
        }
        if desc.linkage == Linkage::Appending && !matches!(kind, TypeKind::Array { .. }) {
            return self.invalid(sym, "an appending variable is an array");
        }
        self.check_addr_space(sym, desc.addr_space)?;
        if let Some(init) = desc.init
            && consts.ty(init) != desc.ty
        {
            return err(
                IrErrorKind::TypeMismatch,
                format!(
                    "{}: the initializer has type {:?}, the variable {:?}",
                    self.label(sym),
                    consts.ty(init),
                    desc.ty
                ),
            );
        }
        entry.kind = Kind::Variable;
        entry.ty = desc.ty;
        entry.tls = desc.tls;
        entry.addr_space = desc.addr_space;
        entry.a = desc.init.map_or(NONE, ConstId::raw);
        if desc.constant {
            entry.flags |= CONSTANT;
        }
        if entry.is_definition() {
            self.check_definition(sym, &entry, types, consts)?;
        }
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Makes `sym` the function `desc` describes.
    pub(crate) fn set_function<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
        sym: SymId,
        desc: &FunctionDesc<'_>,
    ) -> IrResult<()> {
        let mut entry = self.describe(
            sym,
            &Common {
                linkage: desc.linkage,
                visibility: desc.visibility,
                dso_local: desc.dso_local,
                unnamed_addr: desc.unnamed_addr,
                section: desc.section,
                comdat: desc.comdat,
                align: desc.align,
            },
        )?;
        let TypeKind::Function { ret, params, .. } = types.kind(desc.ty) else {
            return err(
                IrErrorKind::InvalidType,
                format!("{}: {:?} is not a function type", self.label(sym), desc.ty),
            );
        };
        if matches!(desc.linkage, Linkage::Common | Linkage::Appending) {
            return self.invalid(sym, "a function is not common or appending");
        }
        if desc.param_attrs.len() > params.len() {
            return err(
                IrErrorKind::InvalidOperand,
                format!(
                    "{}: attributes for {} parameters, and the type has {}",
                    self.label(sym),
                    desc.param_attrs.len(),
                    params.len()
                ),
            );
        }
        let sig = crate::verify::Signature {
            ret: (ret, desc.ret_attrs),
            params: params.len(),
            args: params,
            // LLVM has it on a call's argument alone.
            elementtype: false,
        };
        let attrs = |index: usize| desc.param_attrs.get(index).copied().unwrap_or(ParamAttrs::NONE);
        if let Err(what) = crate::verify::signature(types, &sig, attrs) {
            return err(IrErrorKind::InvalidAttribute, format!("{}: {what}", self.label(sym)));
        }
        self.check_addr_space(sym, desc.addr_space)?;
        if let Some(personality) = desc.personality {
            // An id of another module panics here, not at code generation.
            let _ = self.entries[personality.index()];
        }
        entry.kind = Kind::Function;
        entry.ty = desc.ty;
        entry.cc = desc.cc;
        entry.addr_space = desc.addr_space;
        entry.a = desc.attrs.raw();
        entry.b = self.sig_attrs.intern(desc.ret_attrs, desc.param_attrs)?;
        entry.c = desc.personality.map_or(NONE, SymId::raw);
        entry.d = desc.subprogram.map_or(NONE, DiId::raw);
        if desc.has_body {
            entry.flags |= HAS_BODY;
            self.check_definition(sym, &entry, types, consts)?;
        }
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Makes `sym` the alias `desc` describes.
    pub(crate) fn set_alias<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
        sym: SymId,
        desc: &AliasDesc,
    ) -> IrResult<()> {
        let mut entry = self.describe(
            sym,
            &Common {
                linkage: desc.linkage,
                visibility: desc.visibility,
                dso_local: desc.dso_local,
                unnamed_addr: desc.unnamed_addr,
                section: None,
                comdat: None,
                align: None,
            },
        )?;
        if matches!(desc.linkage, Linkage::ExternWeak | Linkage::Common | Linkage::Appending) {
            return self.invalid(sym, "an alias is not extern_weak, common or appending");
        }
        if !matches!(types.kind(consts.ty(desc.target)), TypeKind::Ptr { .. }) {
            return err(
                IrErrorKind::TypeMismatch,
                format!("{}: the target of an alias is a pointer", self.label(sym)),
            );
        }
        if !matches!(
            consts.get(desc.target),
            Const::Addr { .. } | Const::AddrOfInt { .. } | Const::Expr { .. }
        ) {
            return self.invalid(
                sym,
                "the target of an alias is a symbol's address or a constant expression",
            );
        }
        entry.kind = Kind::Alias;
        entry.ty = desc.ty;
        entry.tls = desc.tls;
        entry.a = desc.target.raw();
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Makes `sym` the IFUNC `desc` describes.
    pub(crate) fn set_ifunc(&mut self, sym: SymId, desc: &IFuncDesc) -> IrResult<()> {
        let mut entry = self.describe(
            sym,
            &Common {
                linkage: desc.linkage,
                visibility: desc.visibility,
                dso_local: desc.dso_local,
                unnamed_addr: desc.unnamed_addr,
                section: None,
                comdat: None,
                align: None,
            },
        )?;
        if matches!(
            desc.linkage,
            Linkage::ExternWeak
                | Linkage::Common
                | Linkage::Appending
                | Linkage::AvailableExternally
        ) {
            return self.invalid(
                sym,
                "an IFUNC is not extern_weak, common, appending or available_externally",
            );
        }
        // An id of another module panics here, not at code generation.
        let _ = self.entries[desc.resolver.index()];
        entry.kind = Kind::IFunc;
        entry.ty = desc.ty;
        entry.a = desc.resolver.raw();
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Gives the declared variable `sym` its initializer.
    pub(crate) fn set_initializer<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
        sym: SymId,
        init: ConstId,
    ) -> IrResult<()> {
        let mut entry = self.entries[sym.index()];
        if entry.kind != Kind::Variable {
            return self.invalid(sym, "only a variable gets an initializer");
        }
        if entry.a != NONE {
            return err(
                IrErrorKind::SymbolRedefined,
                format!("{} already has an initializer", self.label(sym)),
            );
        }
        if consts.ty(init) != entry.ty {
            return err(
                IrErrorKind::TypeMismatch,
                format!(
                    "{}: the initializer has type {:?}, the variable {:?}",
                    self.label(sym),
                    consts.ty(init),
                    entry.ty
                ),
            );
        }
        entry.a = init.raw();
        self.check_definition(sym, &entry, types, consts)?;
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Says that a body will be delivered for the declared function `sym`.
    pub(crate) fn set_has_body<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
        sym: SymId,
    ) -> IrResult<()> {
        let mut entry = self.entries[sym.index()];
        if entry.kind != Kind::Function {
            return self.invalid(sym, "only a function gets a body");
        }
        if entry.flags & HAS_BODY != 0 {
            return err(
                IrErrorKind::SymbolRedefined,
                format!("{} already has a body", self.label(sym)),
            );
        }
        entry.flags |= HAS_BODY;
        self.check_definition(sym, &entry, types, consts)?;
        self.entries[sym.index()] = entry;
        Ok(())
    }

    /// Turns a definition back into what it was before it was one.
    pub(crate) fn discard_definition(&mut self, sym: SymId) {
        let entry = &mut self.entries[sym.index()];
        if !entry.is_definition() {
            return;
        }
        match entry.kind {
            Kind::Variable => entry.a = NONE,
            Kind::Function => {
                entry.flags &= !HAS_BODY;
                entry.c = NONE;
                entry.d = NONE;
            }
            Kind::Alias | Kind::IFunc | Kind::Unresolved => {
                let mut fresh = Entry::unresolved(entry.name_start, entry.name_len);
                fresh.flags = entry.flags & STICKY;
                *entry = fresh;
                return;
            }
        }
        entry.linkage = Linkage::External;
        entry.comdat = NONE;
    }

    /// Marks `sym` as a member of `llvm.used` or `llvm.compiler.used`.
    pub(crate) fn retain(&mut self, sym: SymId, how: Retained) -> IrResult<()> {
        if self.entries[sym.index()].name_len == 0 {
            return self.invalid(sym, "a retained symbol has a name");
        }
        self.entries[sym.index()].flags |= match how {
            Retained::Used => USED,
            Retained::CompilerUsed => COMPILER_USED,
        };
        Ok(())
    }

    /// The end-of-module checks; see `Module::verify_symbols`.
    pub(crate) fn verify<L: Layering>(
        &self,
        types: &TypeTable<L>,
        consts: &ConstPool<L>,
    ) -> IrResult<()> {
        let count = self.entries.len();
        for index in 0..consts.len() {
            let id = ConstId::new(index as u32);
            let (first, second) = match consts.get(id) {
                Const::Addr { sym, .. } | Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => {
                    (sym, sym)
                }
                Const::BlockAddr { func, .. } => (func, func),
                Const::SymDiff { a, b, .. } => (a, b),
                _ => continue,
            };
            for sym in [first, second] {
                if sym.index() >= count {
                    return err(
                        IrErrorKind::UndefinedSymbol,
                        format!("{id:?} refers to {sym:?}, and the module has {count} symbols"),
                    );
                }
            }
        }
        let mut aliases = 0usize;
        for (index, entry) in self.entries.iter().enumerate() {
            let sym = SymId::new(index as u32);
            match entry.kind {
                Kind::Unresolved => {
                    return err(
                        IrErrorKind::UndefinedSymbol,
                        format!("{} is referred to and never declared", self.label(sym)),
                    );
                }
                Kind::Variable | Kind::Function if !entry.is_definition() => {
                    if !matches!(entry.linkage, Linkage::External | Linkage::ExternWeak) {
                        return self.invalid(sym, "a declaration is external or extern_weak");
                    }
                    if entry.comdat != NONE {
                        return self.invalid(sym, "a declaration is not in a COMDAT");
                    }
                }
                Kind::Alias => aliases += 1,
                _ => {}
            }
        }
        // For each alias, the symbol at the end of its chain of aliases
        // and whether every step of the chain has offset zero.
        let mut ends: Vec<(u32, bool)> = Vec::new();
        if aliases != 0 {
            ends = vec![(UNSEEN, false); count];
            let mut path = Vec::new();
            for index in 0..count {
                if self.entries[index].kind == Kind::Alias {
                    self.resolve_alias(consts, index as u32, &mut ends, &mut path)?;
                    self.check_alias(consts, SymId::new(index as u32))?;
                }
            }
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.kind == Kind::IFunc {
                self.check_ifunc(types, SymId::new(index as u32), entry, &ends)?;
            }
        }
        Ok(())
    }

    /// Fills `ends` for the alias `start` and every alias on its chain.
    fn resolve_alias<L: Layering>(
        &self,
        consts: &ConstPool<L>,
        start: u32,
        ends: &mut [(u32, bool)],
        path: &mut Vec<u32>,
    ) -> IrResult<()> {
        path.clear();
        let mut at = start;
        let (end, mut exact) = loop {
            let entry = &self.entries[at as usize];
            if entry.kind != Kind::Alias {
                break (at, true);
            }
            match ends[at as usize] {
                (UNSEEN, _) => {}
                (ACTIVE, _) => {
                    return self.invalid(SymId::new(start), "aliases form a cycle");
                }
                found => break found,
            }
            ends[at as usize].0 = ACTIVE;
            path.push(at);
            match consts.get(ConstId::new(entry.a)) {
                Const::Addr { sym, .. } => at = sym.raw(),
                _ => break (NO_SYMBOL, false),
            }
        };
        while let Some(alias) = path.pop() {
            let target = ConstId::new(self.entries[alias as usize].a);
            exact = exact && matches!(consts.get(target), Const::Addr { addend: 0, .. });
            ends[alias as usize] = (end, exact);
        }
        Ok(())
    }

    /// LLVM's rules for what an alias may point at, for a target that is
    /// a symbol's address. Any other target is `classify`'s to refuse.
    fn check_alias<L: Layering>(&self, consts: &ConstPool<L>, sym: SymId) -> IrResult<()> {
        let alias = &self.entries[sym.index()];
        let Const::Addr { sym: target, addend } = consts.get(ConstId::new(alias.a)) else {
            return Ok(());
        };
        let base = &self.entries[target.index()];
        if alias.linkage == Linkage::AvailableExternally {
            if addend != 0 || base.linkage != Linkage::AvailableExternally {
                return self.invalid(
                    sym,
                    "an available_externally alias points at an available_externally symbol",
                );
            }
        } else if !base.is_definition() || base.linkage == Linkage::AvailableExternally {
            return self.invalid(sym, "an alias points at a definition");
        }
        if base.kind == Kind::Alias && base.linkage.is_interposable() {
            return self.invalid(sym, "an alias does not point at an interposable alias");
        }
        Ok(())
    }

    /// LLVM's rules for an IFUNC's resolver.
    fn check_ifunc<L: Layering>(
        &self,
        types: &TypeTable<L>,
        sym: SymId,
        ifunc: &Entry,
        ends: &[(u32, bool)],
    ) -> IrResult<()> {
        let mut resolver = ifunc.a;
        if self.entries[resolver as usize].kind == Kind::Alias {
            let (end, exact) = ends[resolver as usize];
            if end == NO_SYMBOL || !exact {
                return self.invalid(sym, "the resolver of an IFUNC is a function definition");
            }
            resolver = end;
        }
        let function = &self.entries[resolver as usize];
        if function.kind != Kind::Function
            || !function.is_definition()
            || function.linkage == Linkage::AvailableExternally
        {
            return self.invalid(sym, "the resolver of an IFUNC is a function definition");
        }
        let returns_pointer = match types.kind(function.ty) {
            TypeKind::Function { ret, .. } => matches!(types.kind(ret), TypeKind::Ptr { .. }),
            _ => false,
        };
        if !returns_pointer {
            return self.invalid(sym, "the resolver of an IFUNC returns a pointer");
        }
        Ok(())
    }
}

const UNSEEN: u32 = u32::MAX;
const ACTIVE: u32 = u32::MAX - 1;
const NO_SYMBOL: u32 = u32::MAX - 2;

/// A symbol, read through its module (`Module::symbol`).
#[derive(Clone, Copy)]
pub struct Symbol<'a> {
    table: &'a SymbolTable,
    entry: &'a Entry,
    id: SymId,
}

impl<'a> Symbol<'a> {
    /// The symbol's id.
    #[inline]
    pub fn id(&self) -> SymId {
        self.id
    }

    /// The name; empty for an anonymous symbol.
    #[inline]
    pub fn name(&self) -> &'a [u8] {
        self.table.name(self.id.raw())
    }

    /// Whether the symbol has no name.
    #[inline]
    pub fn is_anonymous(&self) -> bool {
        self.entry.name_len == 0
    }

    /// What the symbol is.
    pub fn kind(&self) -> SymbolKind<'a> {
        let entry = self.entry;
        match entry.kind {
            Kind::Unresolved => SymbolKind::Unresolved,
            Kind::Variable => SymbolKind::Variable {
                constant: entry.flags & CONSTANT != 0,
                tls: entry.tls,
                init: optional(entry.a).map(ConstId::new),
            },
            Kind::Function => {
                let list = self.table.sig_attrs.get(entry.b);
                SymbolKind::Function {
                    cc: entry.cc,
                    attrs: FnAttrsId::new(entry.a),
                    ret_attrs: list.first().copied().unwrap_or(ParamAttrs::NONE),
                    param_attrs: list.get(1..).unwrap_or(&[]),
                    personality: optional(entry.c).map(SymId::new),
                    subprogram: optional(entry.d).map(DiId::new),
                    has_body: entry.flags & HAS_BODY != 0,
                }
            }
            Kind::Alias => SymbolKind::Alias { tls: entry.tls, target: ConstId::new(entry.a) },
            Kind::IFunc => SymbolKind::IFunc { resolver: SymId::new(entry.a) },
        }
    }

    /// Whether the symbol is a function.
    #[inline]
    pub fn is_function(&self) -> bool {
        self.entry.kind == Kind::Function
    }

    /// Whether the symbol is a variable.
    #[inline]
    pub fn is_variable(&self) -> bool {
        self.entry.kind == Kind::Variable
    }

    /// Whether the symbol has not been described yet.
    #[inline]
    pub fn is_unresolved(&self) -> bool {
        self.entry.kind == Kind::Unresolved
    }

    /// Whether the symbol is a definition: a variable with an
    /// initializer, a function that gets a body, an alias or an IFUNC.
    #[inline]
    pub fn is_definition(&self) -> bool {
        self.entry.is_definition()
    }

    /// Whether the symbol is a declaration: a variable without an
    /// initializer or a function that gets no body.
    #[inline]
    pub fn is_declaration(&self) -> bool {
        matches!(self.entry.kind, Kind::Variable | Kind::Function) && !self.entry.is_definition()
    }

    /// The value type of a variable, alias or IFUNC, or the function type
    /// of a function. `void` for an unresolved symbol.
    #[inline]
    pub fn ty(&self) -> TypeId {
        self.entry.ty
    }

    /// The linkage.
    #[inline]
    pub fn linkage(&self) -> Linkage {
        self.entry.linkage
    }

    /// The visibility.
    #[inline]
    pub fn visibility(&self) -> Visibility {
        self.entry.visibility()
    }

    /// `dso_local`, stated or implied.
    #[inline]
    pub fn dso_local(&self) -> bool {
        self.entry.flags & DSO_LOCAL != 0
    }

    /// `unnamed_addr` or `local_unnamed_addr`.
    #[inline]
    pub fn unnamed_addr(&self) -> UnnamedAddr {
        self.entry.unnamed_addr()
    }

    /// The section name.
    pub fn section(&self) -> Option<&'a [u8]> {
        StrId::from_raw(self.entry.section).map(|id| self.table.sections.get(id))
    }

    /// The COMDAT.
    #[inline]
    pub fn comdat(&self) -> Option<ComdatId> {
        optional(self.entry.comdat).map(ComdatId::new)
    }

    /// The stated alignment in bytes.
    #[inline]
    pub fn align(&self) -> Option<u64> {
        decode_align(self.entry.align)
    }

    /// The address space.
    #[inline]
    pub fn addr_space(&self) -> u32 {
        self.entry.addr_space
    }

    /// The thread-local model of a variable or alias.
    #[inline]
    pub fn tls(&self) -> TlsModel {
        self.entry.tls
    }

    /// Whether the symbol is a member of `llvm.used`.
    #[inline]
    pub fn is_used(&self) -> bool {
        self.entry.flags & USED != 0
    }

    /// Whether the symbol is a member of `llvm.compiler.used`.
    #[inline]
    pub fn is_compiler_used(&self) -> bool {
        self.entry.flags & COMPILER_USED != 0
    }

    /// Whether the symbol is retained: a member of either array.
    #[inline]
    pub fn is_retained(&self) -> bool {
        self.entry.flags & STICKY != 0
    }

    /// The attributes of a function's parameter `index`; none for a
    /// parameter without attributes and for a symbol that is no function.
    pub fn param_attrs(&self, index: usize) -> ParamAttrs {
        if self.entry.kind != Kind::Function {
            return ParamAttrs::NONE;
        }
        let list = self.table.sig_attrs.get(self.entry.b);
        list.get(index + 1).copied().unwrap_or(ParamAttrs::NONE)
    }
}

impl fmt::Debug for Symbol<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Symbol")
            .field("id", &self.id)
            .field("name", &String::from_utf8_lossy(self.name()))
            .field("linkage", &self.linkage())
            .field("visibility", &self.visibility())
            .field("ty", &self.ty())
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::target::TargetSpec;

    struct Fixture {
        types: TypeTable,
        consts: ConstPool,
        table: SymbolTable,
        void_fn: TypeId,
        ptr_fn: TypeId,
    }

    fn fixture() -> Fixture {
        let mut types = TypeTable::new(TargetSpec::x86_64_linux_gnu().data_layout());
        let void_fn = types.function(TypeId::VOID, &[], false).unwrap();
        let ptr_fn = types.function(TypeId::PTR, &[], false).unwrap();
        Fixture { types, consts: ConstPool::new(), table: SymbolTable::default(), void_fn, ptr_fn }
    }

    impl Fixture {
        fn var(&mut self, sym: SymId, desc: &VariableDesc<'_>) -> IrResult<()> {
            self.table.set_variable(&self.types, &self.consts, sym, desc)
        }

        fn func(&mut self, sym: SymId, desc: &FunctionDesc<'_>) -> IrResult<()> {
            self.table.set_function(&self.types, &self.consts, sym, desc)
        }

        fn alias(&mut self, sym: SymId, desc: &AliasDesc) -> IrResult<()> {
            self.table.set_alias(&self.types, &self.consts, sym, desc)
        }

        fn verify(&self) -> IrResult<()> {
            self.table.verify(&self.types, &self.consts)
        }

        fn zero(&mut self) -> ConstId {
            self.consts.int(&self.types, TypeId::I32, 0).unwrap()
        }

        fn one(&mut self) -> ConstId {
            self.consts.int(&self.types, TypeId::I32, 1).unwrap()
        }

        /// A defined `i32` variable called `name`.
        fn defined_var(&mut self, name: &str) -> SymId {
            let sym = self.table.named(name.as_bytes()).unwrap();
            let init = self.one();
            self.var(sym, &VariableDesc { init: Some(init), ..VariableDesc::new(TypeId::I32) })
                .unwrap();
            sym
        }

        /// A defined function called `name` of type `ty`.
        fn defined_fn(&mut self, name: &str, ty: TypeId) -> SymId {
            let sym = self.table.named(name.as_bytes()).unwrap();
            self.func(sym, &FunctionDesc { has_body: true, ..FunctionDesc::new(ty) }).unwrap();
            sym
        }
    }

    fn kind<T: fmt::Debug>(result: IrResult<T>) -> IrErrorKind {
        result.unwrap_err().kind
    }

    /// §2.2: a name is one symbol, an id is stable, and the entry size is
    /// what the spec says.
    #[test]
    fn names_are_interned_and_ids_are_stable() {
        assert_eq!(size_of::<Entry>(), 48);
        assert!(size_of::<SymbolKind<'_>>() <= 48);
        let mut f = fixture();
        assert_eq!(f.table.find(b"main"), None);
        let main = f.table.named(b"main").unwrap();
        let other = f.table.named(b"_ZN3fooC2Ev").unwrap();
        assert_eq!(f.table.named(b"main").unwrap(), main);
        assert_eq!(f.table.find(b"main"), Some(main));
        assert_ne!(main, other);
        // Names are bytes: not UTF-8, and with a quote and a newline.
        let odd = f.table.named(b"\xff\"\n").unwrap();
        assert_eq!(f.table.get(odd).name(), b"\xff\"\n");
        let anon_a = f.table.anonymous().unwrap();
        let anon_b = f.table.anonymous().unwrap();
        assert_ne!(anon_a, anon_b);
        assert!(f.table.get(anon_a).is_anonymous());
        assert_eq!(f.table.get(anon_a).name(), b"");
        assert_eq!(f.table.find(b""), None);
        assert_eq!(f.table.len(), 5);
        let symbol = f.table.get(main);
        assert_eq!(symbol.id(), main);
        assert_eq!(symbol.name(), b"main");
        assert!(symbol.is_unresolved());
        assert!(!symbol.is_definition() && !symbol.is_declaration());
        assert_eq!(symbol.kind(), SymbolKind::Unresolved);
        assert_eq!(symbol.ty(), TypeId::VOID);
        for i in 0..3000u32 {
            let name = format!("_ZN4test{i}Ev");
            let id = f.table.named(name.as_bytes()).unwrap();
            assert_eq!(f.table.find(name.as_bytes()), Some(id));
        }
        assert_eq!(f.table.find(b"main"), Some(main));
        assert_eq!(f.table.get(other).name(), b"_ZN3fooC2Ev");
    }

    /// §2.2 `InvalidSymbol`: a name is not empty and has no zero byte.
    #[test]
    fn bad_names_are_errors() {
        let mut f = fixture();
        assert_eq!(kind(f.table.named(b"")), IrErrorKind::InvalidSymbol);
        assert_eq!(kind(f.table.named(b"a\0b")), IrErrorKind::InvalidSymbol);
        assert_eq!(f.table.len(), 0);
    }

    /// §2.2: every field of a variable description reads back.
    #[test]
    fn variable_round_trip() {
        let mut f = fixture();
        let sym = f.table.named(b"v").unwrap();
        let init = f.one();
        let desc = VariableDesc {
            linkage: Linkage::WeakOdr,
            visibility: Visibility::Protected,
            dso_local: false,
            unnamed_addr: UnnamedAddr::Local,
            section: Some(b".mydata"),
            comdat: Some(ComdatId::new(3)),
            align: Some(64),
            ty: TypeId::I32,
            constant: true,
            tls: TlsModel::InitialExec,
            addr_space: 0,
            init: Some(init),
        };
        f.var(sym, &desc).unwrap();
        let symbol = f.table.get(sym);
        assert!(symbol.is_variable() && symbol.is_definition() && !symbol.is_declaration());
        assert_eq!(symbol.linkage(), Linkage::WeakOdr);
        assert_eq!(symbol.visibility(), Visibility::Protected);
        assert!(symbol.dso_local(), "implied by the visibility");
        assert_eq!(symbol.unnamed_addr(), UnnamedAddr::Local);
        assert_eq!(symbol.section(), Some(&b".mydata"[..]));
        assert_eq!(symbol.comdat(), Some(ComdatId::new(3)));
        assert_eq!(symbol.align(), Some(64));
        assert_eq!(symbol.ty(), TypeId::I32);
        assert_eq!(symbol.tls(), TlsModel::InitialExec);
        assert_eq!(symbol.addr_space(), 0);
        assert_eq!(
            symbol.kind(),
            SymbolKind::Variable { constant: true, tls: TlsModel::InitialExec, init: Some(init) }
        );
        assert_eq!(symbol.param_attrs(0), ParamAttrs::NONE);
        assert!(format!("{symbol:?}").contains("name: \"v\""));

        // The defaults, and each value of the small enums.
        let plain = f.table.named(b"plain").unwrap();
        f.var(plain, &VariableDesc::new(TypeId::I64)).unwrap();
        let symbol = f.table.get(plain);
        assert!(symbol.is_declaration());
        assert_eq!(symbol.linkage(), Linkage::External);
        assert_eq!(symbol.visibility(), Visibility::Default);
        assert!(!symbol.dso_local());
        assert_eq!(symbol.unnamed_addr(), UnnamedAddr::None);
        assert_eq!((symbol.section(), symbol.comdat(), symbol.align()), (None, None, None));
        assert_eq!(
            symbol.kind(),
            SymbolKind::Variable { constant: false, tls: TlsModel::None, init: None }
        );
        for visibility in [Visibility::Default, Visibility::Hidden, Visibility::Protected] {
            for unnamed_addr in [UnnamedAddr::None, UnnamedAddr::Local, UnnamedAddr::Global] {
                let desc =
                    VariableDesc { visibility, unnamed_addr, ..VariableDesc::new(TypeId::I8) };
                f.var(plain, &desc).unwrap();
                assert_eq!(f.table.get(plain).visibility(), visibility);
                assert_eq!(f.table.get(plain).unnamed_addr(), unnamed_addr);
            }
        }
        for tls in [
            TlsModel::None,
            TlsModel::GeneralDynamic,
            TlsModel::LocalDynamic,
            TlsModel::InitialExec,
            TlsModel::LocalExec,
        ] {
            f.var(plain, &VariableDesc { tls, ..VariableDesc::new(TypeId::I8) }).unwrap();
            assert_eq!(f.table.get(plain).tls(), tls);
        }
        // An empty section name is no section; 2^32 is the largest alignment.
        let desc = VariableDesc {
            section: Some(b""),
            align: Some(1 << 32),
            ..VariableDesc::new(TypeId::I8)
        };
        f.var(plain, &desc).unwrap();
        assert_eq!(f.table.get(plain).section(), None);
        assert_eq!(f.table.get(plain).align(), Some(1 << 32));
        // Another address space is kept for `classify`.
        f.var(plain, &VariableDesc { addr_space: 1, ..VariableDesc::new(TypeId::I8) }).unwrap();
        assert_eq!(f.table.get(plain).addr_space(), 1);
    }

    /// §2.2: every field of a function description reads back.
    #[test]
    fn function_round_trip() {
        let mut f = fixture();
        let ty =
            f.types.function(TypeId::I32, &[TypeId::PTR, TypeId::I32, TypeId::I8], true).unwrap();
        let personality = f.table.named(b"__gxx_personality_v0").unwrap();
        let sym = f.table.named(b"f").unwrap();
        let sret = ParamAttrs::NONE.with_mem(crate::ParamMem::Sret, TypeId::I64).unwrap();
        let sext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
        let desc = FunctionDesc {
            linkage: Linkage::LinkOnceOdr,
            visibility: Visibility::Hidden,
            dso_local: false,
            unnamed_addr: UnnamedAddr::Global,
            section: Some(b".text.hot"),
            comdat: Some(ComdatId::new(0)),
            align: Some(16),
            ty,
            cc: CallConv::from_llvm(8).unwrap(),
            ret_attrs: sext,
            param_attrs: &[sret, ParamAttrs::NONE],
            attrs: FnAttrsId::new(2),
            personality: Some(personality),
            subprogram: Some(DiId::new(7)),
            addr_space: 0,
            has_body: true,
        };
        f.func(sym, &desc).unwrap();
        let symbol = f.table.get(sym);
        assert!(symbol.is_function() && symbol.is_definition());
        assert_eq!(symbol.linkage(), Linkage::LinkOnceOdr);
        assert!(symbol.dso_local(), "implied by the visibility");
        assert_eq!(symbol.unnamed_addr(), UnnamedAddr::Global);
        assert_eq!(symbol.section(), Some(&b".text.hot"[..]));
        assert_eq!(symbol.comdat(), Some(ComdatId::new(0)));
        assert_eq!(symbol.align(), Some(16));
        assert_eq!(symbol.ty(), ty);
        assert_eq!(
            symbol.kind(),
            SymbolKind::Function {
                cc: CallConv::from_llvm(8).unwrap(),
                attrs: FnAttrsId::new(2),
                ret_attrs: sext,
                param_attrs: &[sret],
                personality: Some(personality),
                subprogram: Some(DiId::new(7)),
                has_body: true,
            }
        );
        assert_eq!(symbol.param_attrs(0), sret);
        assert_eq!(symbol.param_attrs(1), ParamAttrs::NONE);
        assert_eq!(symbol.param_attrs(99), ParamAttrs::NONE);

        let plain = f.table.named(b"plain").unwrap();
        f.func(plain, &FunctionDesc::new(f.void_fn)).unwrap();
        let symbol = f.table.get(plain);
        assert!(symbol.is_declaration());
        assert_eq!(
            symbol.kind(),
            SymbolKind::Function {
                cc: CallConv::C,
                attrs: FnAttrsId::EMPTY,
                ret_attrs: ParamAttrs::NONE,
                param_attrs: &[],
                personality: None,
                subprogram: None,
                has_body: false,
            }
        );
        // A declaration may name a personality: LLVM's verifier refuses
        // that, hod does not care.
        let desc = FunctionDesc { personality: Some(personality), ..FunctionDesc::new(f.void_fn) };
        f.func(plain, &desc).unwrap();
    }

    /// §2.2: aliases and IFUNCs read back, and are definitions.
    #[test]
    fn alias_and_ifunc_round_trip() {
        let mut f = fixture();
        let v = f.defined_var("v");
        let resolver = f.defined_fn("resolve", f.ptr_fn);
        let target = f.consts.addr(v, 4).unwrap();
        let a = f.table.named(b"a").unwrap();
        let desc = AliasDesc {
            linkage: Linkage::WeakAny,
            visibility: Visibility::Hidden,
            dso_local: false,
            unnamed_addr: UnnamedAddr::Global,
            tls: TlsModel::LocalExec,
            ty: TypeId::I8,
            target,
        };
        f.alias(a, &desc).unwrap();
        let symbol = f.table.get(a);
        assert!(symbol.is_definition() && !symbol.is_declaration());
        assert!(!symbol.is_variable() && !symbol.is_function());
        assert_eq!(symbol.kind(), SymbolKind::Alias { tls: TlsModel::LocalExec, target });
        assert_eq!(symbol.ty(), TypeId::I8);
        assert_eq!(symbol.linkage(), Linkage::WeakAny);
        assert_eq!(symbol.visibility(), Visibility::Hidden);
        assert!(symbol.dso_local());
        assert_eq!((symbol.section(), symbol.comdat(), symbol.align()), (None, None, None));

        let i = f.table.named(b"i").unwrap();
        let desc = IFuncDesc { linkage: Linkage::Internal, ..IFuncDesc::new(f.void_fn, resolver) };
        f.table.set_ifunc(i, &desc).unwrap();
        let symbol = f.table.get(i);
        assert!(symbol.is_definition());
        assert_eq!(symbol.kind(), SymbolKind::IFunc { resolver });
        assert_eq!(symbol.ty(), f.void_fn);
        assert!(symbol.dso_local());
        f.verify().unwrap();
    }

    /// §2.2 "States": a symbol can be used by id before it is described;
    /// a description replaces an unresolved symbol or a declaration,
    /// whatever kind it was, and the id stays.
    #[test]
    fn declarations_are_replaced_and_ids_stay() {
        let mut f = fixture();
        let sym = f.table.named(b"f").unwrap();
        // Used before it is anything.
        let address = f.consts.addr(sym, 0).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::UndefinedSymbol);

        f.func(sym, &FunctionDesc::new(f.void_fn)).unwrap();
        assert!(f.table.get(sym).is_declaration());
        f.verify().unwrap();
        // Declared again with another type: the last description wins.
        f.func(sym, &FunctionDesc::new(f.ptr_fn)).unwrap();
        assert_eq!(f.table.get(sym).ty(), f.ptr_fn);
        // A declared function becomes a variable declaration, ...
        f.var(sym, &VariableDesc::new(TypeId::I32)).unwrap();
        assert!(f.table.get(sym).is_variable());
        // ... then an alias (design review case (b)), without a new id.
        let v = f.defined_var("v");
        let target = f.consts.addr(v, 0).unwrap();
        f.func(sym, &FunctionDesc::new(f.void_fn)).unwrap();
        f.alias(sym, &AliasDesc::new(f.void_fn, target)).unwrap();
        assert_eq!(f.table.find(b"f"), Some(sym));
        assert_eq!(f.consts.get(address), Const::Addr { sym, addend: 0 });
        assert_eq!(f.table.get(sym).kind(), SymbolKind::Alias { tls: TlsModel::None, target });
        f.verify().unwrap();

        // And a declared function becomes an IFUNC.
        let g = f.table.named(b"g").unwrap();
        let resolver = f.defined_fn("resolve", f.ptr_fn);
        f.func(g, &FunctionDesc::new(f.void_fn)).unwrap();
        f.table.set_ifunc(g, &IFuncDesc::new(f.void_fn, resolver)).unwrap();
        assert_eq!(f.table.get(g).kind(), SymbolKind::IFunc { resolver });
        f.verify().unwrap();
    }

    /// §2.2 "States": a declaration becomes a definition by a new
    /// description, by `set_initializer`, or by `set_has_body`.
    #[test]
    fn declarations_become_definitions() {
        let mut f = fixture();
        let one = f.one();
        let v = f.table.named(b"v").unwrap();
        f.var(v, &VariableDesc::new(TypeId::I32)).unwrap();
        f.table.set_initializer(&f.types, &f.consts, v, one).unwrap();
        assert!(f.table.get(v).is_definition());
        assert_eq!(
            f.table.get(v).kind(),
            SymbolKind::Variable { constant: false, tls: TlsModel::None, init: Some(one) }
        );

        let w = f.table.named(b"w").unwrap();
        f.var(w, &VariableDesc::new(TypeId::I32)).unwrap();
        let desc = VariableDesc {
            linkage: Linkage::Internal,
            init: Some(one),
            ..VariableDesc::new(TypeId::I32)
        };
        f.var(w, &desc).unwrap();
        assert!(f.table.get(w).is_definition());
        assert_eq!(f.table.get(w).linkage(), Linkage::Internal);

        let g = f.table.named(b"g").unwrap();
        f.func(g, &FunctionDesc::new(f.void_fn)).unwrap();
        f.table.set_has_body(&f.types, &f.consts, g).unwrap();
        assert!(f.table.get(g).is_definition());
        f.verify().unwrap();

        // A definition's linkage can be stated before it is a definition;
        // until then the module does not verify.
        let h = f.table.named(b"h").unwrap();
        f.func(h, &FunctionDesc { linkage: Linkage::Internal, ..FunctionDesc::new(f.void_fn) })
            .unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
        f.table.set_has_body(&f.types, &f.consts, h).unwrap();
        f.verify().unwrap();
    }

    /// §2.2 `SymbolRedefined`: a definition is not described again, and
    /// gets no second initializer or body.
    #[test]
    fn redefinition_is_an_error() {
        let mut f = fixture();
        let one = f.one();
        let v = f.defined_var("v");
        let g = f.defined_fn("g", f.void_fn);
        let target = f.consts.addr(v, 0).unwrap();
        let a = f.table.named(b"a").unwrap();
        f.alias(a, &AliasDesc::new(TypeId::I32, target)).unwrap();
        let i = f.table.named(b"i").unwrap();
        let resolver = f.defined_fn("resolve", f.ptr_fn);
        f.table.set_ifunc(i, &IFuncDesc::new(f.void_fn, resolver)).unwrap();
        for sym in [v, g, a, i] {
            let before = format!("{:?}", f.table.get(sym).kind());
            let redefined = IrErrorKind::SymbolRedefined;
            assert_eq!(kind(f.var(sym, &VariableDesc::new(TypeId::I32))), redefined);
            assert_eq!(kind(f.func(sym, &FunctionDesc::new(f.void_fn))), redefined);
            assert_eq!(kind(f.alias(sym, &AliasDesc::new(TypeId::I32, target))), redefined);
            assert_eq!(
                kind(f.table.set_ifunc(sym, &IFuncDesc::new(f.void_fn, resolver))),
                redefined
            );
            assert_eq!(format!("{:?}", f.table.get(sym).kind()), before);
        }
        assert_eq!(
            kind(f.table.set_initializer(&f.types, &f.consts, v, one)),
            IrErrorKind::SymbolRedefined
        );
        assert_eq!(
            kind(f.table.set_has_body(&f.types, &f.consts, g)),
            IrErrorKind::SymbolRedefined
        );
        // The wrong kind of symbol for the shortcut.
        assert_eq!(
            kind(f.table.set_initializer(&f.types, &f.consts, g, one)),
            IrErrorKind::InvalidSymbol
        );
        assert_eq!(kind(f.table.set_has_body(&f.types, &f.consts, v)), IrErrorKind::InvalidSymbol);
        let fresh = f.table.named(b"fresh").unwrap();
        assert_eq!(
            kind(f.table.set_has_body(&f.types, &f.consts, fresh)),
            IrErrorKind::InvalidSymbol
        );
        // The initializer has the variable's type.
        let d = f.table.named(b"d").unwrap();
        f.var(d, &VariableDesc::new(TypeId::I64)).unwrap();
        assert_eq!(
            kind(f.table.set_initializer(&f.types, &f.consts, d, one)),
            IrErrorKind::TypeMismatch
        );
        assert!(f.table.get(d).is_declaration());
    }

    /// §2.2 "States": `discard_definition` makes a definition replaceable
    /// and keeps the id; a function or variable becomes an external
    /// declaration, an alias or IFUNC an unresolved symbol.
    #[test]
    fn discarding_a_definition() {
        let mut f = fixture();
        let one = f.one();
        let personality = f.table.named(b"__gxx_personality_v0").unwrap();
        f.func(personality, &FunctionDesc::new(f.void_fn)).unwrap();
        let g = f.table.named(b"g").unwrap();
        let desc = FunctionDesc {
            linkage: Linkage::LinkOnceOdr,
            comdat: Some(ComdatId::new(0)),
            section: Some(b".text.g"),
            personality: Some(personality),
            subprogram: Some(DiId::new(1)),
            has_body: true,
            ..FunctionDesc::new(f.void_fn)
        };
        f.func(g, &desc).unwrap();
        f.table.retain(g, Retained::Used).unwrap();
        f.table.discard_definition(g);
        let symbol = f.table.get(g);
        assert!(symbol.is_declaration());
        assert_eq!(symbol.linkage(), Linkage::External);
        assert_eq!(symbol.comdat(), None);
        assert_eq!(symbol.section(), Some(&b".text.g"[..]));
        assert!(symbol.is_used());
        assert!(matches!(
            symbol.kind(),
            SymbolKind::Function { personality: None, subprogram: None, has_body: false, .. }
        ));
        f.verify().unwrap();
        // It can be defined again, differently.
        f.func(g, &FunctionDesc { has_body: true, ..FunctionDesc::new(f.ptr_fn) }).unwrap();
        assert_eq!(f.table.get(g).ty(), f.ptr_fn);
        assert!(f.table.get(g).is_used());

        let v = f.table.named(b"v").unwrap();
        let desc = VariableDesc {
            linkage: Linkage::AvailableExternally,
            constant: true,
            init: Some(one),
            ..VariableDesc::new(TypeId::I32)
        };
        f.var(v, &desc).unwrap();
        f.table.discard_definition(v);
        assert_eq!(
            f.table.get(v).kind(),
            SymbolKind::Variable { constant: true, tls: TlsModel::None, init: None }
        );
        assert_eq!(f.table.get(v).linkage(), Linkage::External);
        f.table.set_initializer(&f.types, &f.consts, v, one).unwrap();

        let target = f.consts.addr(v, 0).unwrap();
        let a = f.table.named(b"a").unwrap();
        f.alias(a, &AliasDesc::new(TypeId::I32, target)).unwrap();
        f.table.retain(a, Retained::CompilerUsed).unwrap();
        f.table.discard_definition(a);
        assert!(f.table.get(a).is_unresolved());
        assert!(f.table.get(a).is_compiler_used());
        assert_eq!(f.table.get(a).name(), b"a");
        let i = f.table.named(b"i").unwrap();
        f.table.set_ifunc(i, &IFuncDesc::new(f.void_fn, g)).unwrap();
        f.table.discard_definition(i);
        assert!(f.table.get(i).is_unresolved());

        // Nothing happens to a declaration or an unresolved symbol.
        let d = f.table.named(b"d").unwrap();
        f.table.discard_definition(d);
        assert!(f.table.get(d).is_unresolved());
        f.var(d, &VariableDesc { linkage: Linkage::ExternWeak, ..VariableDesc::new(TypeId::I32) })
            .unwrap();
        f.table.discard_definition(d);
        assert_eq!(f.table.get(d).linkage(), Linkage::ExternWeak);
    }

    /// §2.2 "retained": the two arrays are two bits, kept apart, set at
    /// any time and untouched by a description.
    #[test]
    fn retained_bits() {
        let mut f = fixture();
        let a = f.table.named(b"a").unwrap();
        let b = f.table.named(b"b").unwrap();
        let c = f.table.named(b"c").unwrap();
        f.table.retain(a, Retained::Used).unwrap();
        f.table.retain(b, Retained::CompilerUsed).unwrap();
        for sym in [a, b, c] {
            f.var(sym, &VariableDesc::new(TypeId::I32)).unwrap();
        }
        f.table.retain(c, Retained::Used).unwrap();
        f.table.retain(c, Retained::CompilerUsed).unwrap();
        let bits = |f: &Fixture, sym| {
            let symbol = f.table.get(sym);
            (symbol.is_used(), symbol.is_compiler_used(), symbol.is_retained())
        };
        assert_eq!(bits(&f, a), (true, false, true));
        assert_eq!(bits(&f, b), (false, true, true));
        assert_eq!(bits(&f, c), (true, true, true));
        let d = f.table.named(b"d").unwrap();
        assert_eq!(bits(&f, d), (false, false, false));
        let anon = f.table.anonymous().unwrap();
        assert_eq!(kind(f.table.retain(anon, Retained::Used)), IrErrorKind::InvalidSymbol);
    }

    /// §2.2 "Immediate checks": what a variable description is refused
    /// for, each with its error kind, and a refused description changes
    /// nothing.
    #[test]
    fn variable_description_errors() {
        let mut f = fixture();
        let zero = f.zero();
        let one = f.one();
        let sym = f.table.named(b"s").unwrap();
        let var = VariableDesc::new(TypeId::I32);
        let def = VariableDesc { init: Some(zero), ..var };
        let array = f.types.array(TypeId::I32, 1).unwrap();
        let scalable = f.types.scalable_vector(TypeId::I32, 4).unwrap();
        let opaque = f.types.opaque_struct().unwrap();
        let undef_opaque = f.consts.undef(&f.types, opaque);
        let local_hidden =
            VariableDesc { linkage: Linkage::Internal, visibility: Visibility::Hidden, ..var };
        let cases: Vec<(VariableDesc<'_>, IrErrorKind)> = vec![
            (local_hidden, IrErrorKind::InvalidSymbol),
            (VariableDesc { align: Some(3), ..var }, IrErrorKind::InvalidOperand),
            (VariableDesc { align: Some(0), ..var }, IrErrorKind::InvalidOperand),
            (VariableDesc { align: Some(1 << 33), ..var }, IrErrorKind::InvalidOperand),
            (VariableDesc::new(TypeId::VOID), IrErrorKind::InvalidType),
            (VariableDesc::new(TypeId::LABEL), IrErrorKind::InvalidType),
            (VariableDesc::new(TypeId::METADATA), IrErrorKind::InvalidType),
            (VariableDesc::new(TypeId::TOKEN), IrErrorKind::InvalidType),
            (VariableDesc::new(f.void_fn), IrErrorKind::InvalidType),
            (VariableDesc::new(scalable), IrErrorKind::InvalidType),
            (VariableDesc { linkage: Linkage::Appending, ..var }, IrErrorKind::InvalidSymbol),
            (VariableDesc { addr_space: 1 << 24, ..var }, IrErrorKind::InvalidSymbol),
            (
                VariableDesc { init: Some(zero), ..VariableDesc::new(TypeId::I64) },
                IrErrorKind::TypeMismatch,
            ),
            (VariableDesc { linkage: Linkage::ExternWeak, ..def }, IrErrorKind::InvalidSymbol),
            (
                VariableDesc {
                    linkage: Linkage::AvailableExternally,
                    comdat: Some(ComdatId::new(0)),
                    ..def
                },
                IrErrorKind::InvalidSymbol,
            ),
            (
                VariableDesc { linkage: Linkage::Common, constant: true, ..def },
                IrErrorKind::InvalidSymbol,
            ),
            (
                VariableDesc { linkage: Linkage::Common, comdat: Some(ComdatId::new(0)), ..def },
                IrErrorKind::InvalidSymbol,
            ),
            (
                VariableDesc { linkage: Linkage::Common, init: Some(one), ..var },
                IrErrorKind::InvalidSymbol,
            ),
        ];
        for (desc, expected) in cases {
            assert_eq!(kind(f.var(sym, &desc)), expected, "{desc:?}");
            assert!(f.table.get(sym).is_unresolved());
        }
        if let Ok(init) = undef_opaque {
            let desc = VariableDesc { init: Some(init), ..VariableDesc::new(opaque) };
            assert_eq!(kind(f.var(sym, &desc)), IrErrorKind::UnsizedType);
        }
        // What is allowed next to those: an opaque declaration, a common
        // definition with a section, an appending array.
        f.var(sym, &VariableDesc::new(opaque)).unwrap();
        let common = VariableDesc {
            linkage: Linkage::Common,
            section: Some(b".mybss"),
            tls: TlsModel::GeneralDynamic,
            align: Some(8),
            ..def
        };
        f.var(sym, &common).unwrap();
        let appending = f.table.named(b"llvm.metadata.like").unwrap();
        let init = f.consts.zero(&f.types, array).unwrap();
        let desc = VariableDesc {
            linkage: Linkage::Appending,
            init: Some(init),
            ..VariableDesc::new(array)
        };
        f.var(appending, &desc).unwrap();

        // An anonymous symbol is local.
        let anon = f.table.anonymous().unwrap();
        assert_eq!(kind(f.var(anon, &def)), IrErrorKind::InvalidSymbol);
        for linkage in [Linkage::Internal, Linkage::Private] {
            f.table.discard_definition(anon);
            f.var(anon, &VariableDesc { linkage, ..def }).unwrap();
            assert!(f.table.get(anon).dso_local());
        }
    }

    /// §2.2 "Immediate checks": what a function description is refused
    /// for.
    #[test]
    fn function_description_errors() {
        let mut f = fixture();
        let g = f.table.named(b"g").unwrap();
        let func = FunctionDesc::new(f.void_fn);
        let one_param = [ParamAttrs::NONE];
        let cases = [
            (FunctionDesc::new(TypeId::I32), IrErrorKind::InvalidType),
            (FunctionDesc { linkage: Linkage::Common, ..func }, IrErrorKind::InvalidSymbol),
            (FunctionDesc { linkage: Linkage::Appending, ..func }, IrErrorKind::InvalidSymbol),
            (FunctionDesc { param_attrs: &one_param, ..func }, IrErrorKind::InvalidOperand),
            (FunctionDesc { addr_space: u32::MAX, ..func }, IrErrorKind::InvalidSymbol),
            (
                FunctionDesc { linkage: Linkage::ExternWeak, has_body: true, ..func },
                IrErrorKind::InvalidSymbol,
            ),
            (
                FunctionDesc {
                    linkage: Linkage::AvailableExternally,
                    comdat: Some(ComdatId::new(0)),
                    has_body: true,
                    ..func
                },
                IrErrorKind::InvalidSymbol,
            ),
            (
                FunctionDesc {
                    linkage: Linkage::Private,
                    visibility: Visibility::Protected,
                    ..func
                },
                IrErrorKind::InvalidSymbol,
            ),
        ];
        for (desc, expected) in cases {
            assert_eq!(kind(f.func(g, &desc)), expected, "{desc:?}");
            assert!(f.table.get(g).is_unresolved());
        }
        // `set_has_body` runs the same checks as a defining description.
        f.func(g, &FunctionDesc { linkage: Linkage::ExternWeak, ..func }).unwrap();
        assert_eq!(kind(f.table.set_has_body(&f.types, &f.consts, g)), IrErrorKind::InvalidSymbol);
        assert!(f.table.get(g).is_declaration());
    }

    /// §2.2 "Immediate checks": what an alias or IFUNC description is
    /// refused for.
    #[test]
    fn alias_and_ifunc_description_errors() {
        let mut f = fixture();
        let one = f.one();
        let g = f.table.named(b"g").unwrap();
        let v = f.defined_var("v");
        let target = f.consts.addr(v, 0).unwrap();
        let null = f.consts.null(&f.types, TypeId::PTR).unwrap();
        let block = f.consts.block_addr(g, crate::BlockId::new(0)).unwrap();
        let a = f.table.named(b"a").unwrap();
        let alias = AliasDesc::new(TypeId::I32, target);
        for linkage in [Linkage::ExternWeak, Linkage::Common, Linkage::Appending] {
            assert_eq!(
                kind(f.alias(a, &AliasDesc { linkage, ..alias })),
                IrErrorKind::InvalidSymbol
            );
        }
        assert_eq!(
            kind(f.alias(a, &AliasDesc { target: one, ..alias })),
            IrErrorKind::TypeMismatch
        );
        for target in [null, block] {
            assert_eq!(
                kind(f.alias(a, &AliasDesc { target, ..alias })),
                IrErrorKind::InvalidSymbol
            );
        }
        assert!(f.table.get(a).is_unresolved());
        let absolute = f.consts.addr_of_int(4).unwrap();
        f.alias(a, &AliasDesc { target: absolute, ..alias }).unwrap();

        let i = f.table.named(b"i").unwrap();
        let resolver = f.defined_fn("resolve", f.ptr_fn);
        let ifunc = IFuncDesc::new(f.void_fn, resolver);
        for linkage in
            [Linkage::ExternWeak, Linkage::Common, Linkage::Appending, Linkage::AvailableExternally]
        {
            assert_eq!(
                kind(f.table.set_ifunc(i, &IFuncDesc { linkage, ..ifunc })),
                IrErrorKind::InvalidSymbol
            );
        }
        assert!(f.table.get(i).is_unresolved());
    }

    /// §2.2: `dso_local` is implied by a local linkage and by a
    /// non-default visibility, except on an `extern_weak` symbol
    /// (LLVM's `isImplicitDSOLocal`).
    #[test]
    fn dso_local_is_implied() {
        let mut f = fixture();
        let sym = f.table.named(b"s").unwrap();
        for linkage in Linkage::ALL {
            for visibility in [Visibility::Default, Visibility::Hidden, Visibility::Protected] {
                if linkage.is_local() && visibility != Visibility::Default {
                    continue;
                }
                if matches!(linkage, Linkage::Common | Linkage::Appending) {
                    continue;
                }
                let implied = linkage.is_local()
                    || (visibility != Visibility::Default && linkage != Linkage::ExternWeak);
                for dso_local in [false, true] {
                    let desc = FunctionDesc {
                        linkage,
                        visibility,
                        dso_local,
                        ..FunctionDesc::new(f.void_fn)
                    };
                    f.func(sym, &desc).unwrap();
                    assert_eq!(
                        f.table.get(sym).dso_local(),
                        dso_local || implied,
                        "{linkage:?} {visibility:?}"
                    );
                }
            }
        }
        let keywords: Vec<&str> = Linkage::ALL.iter().map(|linkage| linkage.keyword()).collect();
        assert_eq!(
            keywords,
            [
                "external",
                "available_externally",
                "linkonce",
                "linkonce_odr",
                "weak",
                "weak_odr",
                "appending",
                "internal",
                "private",
                "extern_weak",
                "common"
            ]
        );
        let interposable: Vec<Linkage> =
            Linkage::ALL.into_iter().filter(|linkage| linkage.is_interposable()).collect();
        assert_eq!(
            interposable,
            [Linkage::LinkOnceAny, Linkage::WeakAny, Linkage::ExternWeak, Linkage::Common]
        );
    }

    /// §2.2 "End checks": `verify` refuses unresolved symbols,
    /// declarations with a definition's linkage or a COMDAT, and
    /// constants that refer to a symbol the module does not have.
    #[test]
    fn verify_declarations_and_references() {
        let mut f = fixture();
        f.verify().unwrap();
        let sym = f.table.named(b"s").unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::UndefinedSymbol);
        for linkage in Linkage::ALL {
            if linkage == Linkage::Appending {
                continue;
            }
            f.var(sym, &VariableDesc { linkage, ..VariableDesc::new(TypeId::I32) }).unwrap();
            let fine = matches!(linkage, Linkage::External | Linkage::ExternWeak);
            assert_eq!(f.verify().is_ok(), fine, "{linkage:?}");
            if !fine {
                assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
            }
        }
        // Hidden, with a section and an alignment: fine on a declaration.
        let desc = VariableDesc {
            visibility: Visibility::Hidden,
            section: Some(b".x"),
            align: Some(4),
            ..VariableDesc::new(TypeId::I32)
        };
        f.var(sym, &desc).unwrap();
        f.verify().unwrap();
        let desc =
            VariableDesc { comdat: Some(ComdatId::new(0)), ..VariableDesc::new(TypeId::I32) };
        f.var(sym, &desc).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
        f.var(sym, &VariableDesc::new(TypeId::I32)).unwrap();
        f.verify().unwrap();

        for which in 0..4u32 {
            let missing = SymId::new(f.table.len() as u32);
            match which {
                0 => f.consts.addr(missing, 0).unwrap(),
                1 => f.consts.block_addr(missing, crate::BlockId::new(0)).unwrap(),
                2 => f.consts.dso_local_equivalent(missing).unwrap(),
                _ => f.consts.no_cfi(missing).unwrap(),
            };
            assert_eq!(kind(f.verify()), IrErrorKind::UndefinedSymbol);
            let name = format!("s{which}");
            let next = f.table.named(name.as_bytes()).unwrap();
            assert_eq!(next, missing);
            f.var(next, &VariableDesc::new(TypeId::I32)).unwrap();
            f.verify().unwrap();
        }
        f.consts.sym_diff(&f.types, TypeId::I64, sym, SymId::new(9), 0, false).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::UndefinedSymbol);
    }

    /// §2.2 "End checks": LLVM's rules for what an alias points at.
    #[test]
    fn verify_aliases() {
        let mut f = fixture();
        let one = f.one();
        let def = f.defined_var("def");
        let decl = f.table.named(b"decl").unwrap();
        f.var(decl, &VariableDesc::new(TypeId::I32)).unwrap();
        let ae = f.table.named(b"ae").unwrap();
        let desc = VariableDesc {
            linkage: Linkage::AvailableExternally,
            init: Some(one),
            ..VariableDesc::new(TypeId::I32)
        };
        f.var(ae, &desc).unwrap();
        let a = f.table.named(b"a").unwrap();
        let check = |f: &mut Fixture, linkage, target: SymId, addend, fine: bool| {
            let target = f.consts.addr(target, addend).unwrap();
            f.table.discard_definition(a);
            f.alias(a, &AliasDesc { linkage, ..AliasDesc::new(TypeId::I32, target) }).unwrap();
            assert_eq!(f.verify().is_ok(), fine, "{linkage:?} alias of {target:?}");
            if !fine {
                assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
            }
        };
        check(&mut f, Linkage::External, def, 0, true);
        check(&mut f, Linkage::Internal, def, 2, true);
        check(&mut f, Linkage::External, decl, 0, false);
        check(&mut f, Linkage::External, ae, 0, false);
        check(&mut f, Linkage::AvailableExternally, ae, 0, true);
        check(&mut f, Linkage::AvailableExternally, ae, 1, false);
        check(&mut f, Linkage::AvailableExternally, def, 0, false);
        check(&mut f, Linkage::External, a, 0, false);
        check(&mut f, Linkage::External, def, 0, true);

        // Alias of an alias: fine unless the inner one is interposable.
        let b = f.table.named(b"b").unwrap();
        let to_a = f.consts.addr(a, 0).unwrap();
        f.alias(b, &AliasDesc::new(TypeId::I32, to_a)).unwrap();
        f.verify().unwrap();
        for (linkage, fine) in [
            (Linkage::WeakAny, false),
            (Linkage::LinkOnceAny, false),
            (Linkage::WeakOdr, true),
            (Linkage::LinkOnceOdr, true),
            (Linkage::Private, true),
        ] {
            check(&mut f, linkage, def, 0, fine);
        }
        // A cycle of two, and a long chain that ends in one.
        let to_b = f.consts.addr(b, 8).unwrap();
        f.table.discard_definition(a);
        f.alias(a, &AliasDesc::new(TypeId::I32, to_b)).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
        check(&mut f, Linkage::External, def, 0, true);
        let mut previous = b;
        for i in 0..2000 {
            let next = f.table.named(format!("chain{i}").as_bytes()).unwrap();
            let target = f.consts.addr(previous, 0).unwrap();
            f.alias(next, &AliasDesc::new(TypeId::I32, target)).unwrap();
            previous = next;
        }
        f.verify().unwrap();
        let to_last = f.consts.addr(previous, 0).unwrap();
        f.table.discard_definition(a);
        f.alias(a, &AliasDesc::new(TypeId::I32, to_last)).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
    }

    /// §2.2 "End checks": an IFUNC's resolver is a defined function that
    /// returns a pointer, reached directly or through aliases at offset 0.
    #[test]
    fn verify_ifuncs() {
        let mut f = fixture();
        let good = f.defined_fn("good", f.ptr_fn);
        let returns_void = f.defined_fn("returns_void", f.void_fn);
        let declared = f.table.named(b"declared").unwrap();
        f.func(declared, &FunctionDesc::new(f.ptr_fn)).unwrap();
        let ae = f.table.named(b"ae").unwrap();
        let desc = FunctionDesc {
            linkage: Linkage::AvailableExternally,
            has_body: true,
            ..FunctionDesc::new(f.ptr_fn)
        };
        f.func(ae, &desc).unwrap();
        let weak = f.table.named(b"weak").unwrap();
        f.func(
            weak,
            &FunctionDesc {
                linkage: Linkage::WeakAny,
                has_body: true,
                ..FunctionDesc::new(f.ptr_fn)
            },
        )
        .unwrap();
        let variable = f.defined_var("variable");
        let to_good = f.consts.addr(good, 0).unwrap();
        let alias = f.table.named(b"alias").unwrap();
        f.alias(alias, &AliasDesc::new(f.ptr_fn, to_good)).unwrap();
        let to_alias = f.consts.addr(alias, 0).unwrap();
        let alias2 = f.table.named(b"alias2").unwrap();
        f.alias(alias2, &AliasDesc::new(f.ptr_fn, to_alias)).unwrap();
        let into_good = f.consts.addr(good, 4).unwrap();
        let offset = f.table.named(b"offset").unwrap();
        f.alias(offset, &AliasDesc::new(f.ptr_fn, into_good)).unwrap();
        let to_offset = f.consts.addr(offset, 0).unwrap();
        let via_offset = f.table.named(b"via_offset").unwrap();
        f.alias(via_offset, &AliasDesc::new(f.ptr_fn, to_offset)).unwrap();
        let absolute = f.consts.addr_of_int(64).unwrap();
        let fixed = f.table.named(b"fixed").unwrap();
        f.alias(fixed, &AliasDesc::new(f.ptr_fn, absolute)).unwrap();
        let to_variable = f.consts.addr(variable, 0).unwrap();
        let data = f.table.named(b"data").unwrap();
        f.alias(data, &AliasDesc::new(TypeId::I32, to_variable)).unwrap();
        f.verify().unwrap();

        let i = f.table.named(b"i").unwrap();
        for (resolver, fine) in [
            (good, true),
            (weak, true),
            (alias, true),
            (alias2, true),
            (returns_void, false),
            (declared, false),
            (ae, false),
            (variable, false),
            (offset, false),
            (via_offset, false),
            (fixed, false),
            (data, false),
        ] {
            f.table.discard_definition(i);
            f.table.set_ifunc(i, &IFuncDesc::new(f.void_fn, resolver)).unwrap();
            assert_eq!(f.verify().is_ok(), fine, "resolver {:?}", f.table.get(resolver));
            if !fine {
                assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
            }
        }
        // An IFUNC is not its own resolver.
        f.table.discard_definition(i);
        f.table.set_ifunc(i, &IFuncDesc::new(f.void_fn, i)).unwrap();
        assert_eq!(kind(f.verify()), IrErrorKind::InvalidSymbol);
    }
}
