//! The module (`spec/ir.md` §2): the owner of the tables, and the
//! producer-facing API for everything that is not a type, a constant or a
//! function body.
//!
//! WP-002 provided the target, the type table and the constant pool;
//! WP-003 the symbols (§2.2), COMDATs, inline assembly, function
//! attribute sets (§5.7), module properties and flags (§2.1),
//! constructors and destructors, module-level assembly, linker options
//! and the list of constructs a producer could not represent. WP-005
//! added the debug tables (§6): the field `debug`, the attachment of
//! global-variable expressions to symbols, and the two checks.

use std::sync::Arc;

use crate::asm::{AsmTable, InlineAsm, OwnedAsm};
use crate::attrs::{FnAttrTable, FnAttrs, FnAttrsId, FnAttrsView};
use crate::comdat::{Comdat, ComdatKind, ComdatTable};
use crate::constant::ConstPool;
use crate::debug::node::{DiKind, DiNode};
use crate::debug::{DebugCounts, DebugDelta, DebugInfo, DebugRemap};
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{AsmId, ComdatId, ConstId, DiId, SymId, TypeId};
use crate::intern::pool_start;
use crate::props::{FlagTable, FlagValue, ModuleFlag, ModuleProps};
use crate::symbol::{
    AliasDesc, FunctionDesc, IFuncDesc, Retained, Symbol, SymbolKind, SymbolTable, VariableDesc,
};
use crate::target::TargetSpec;
use crate::types::TypeTable;

/// One entry of `llvm.global_ctors` or `llvm.global_dtors` (§2.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Structor {
    /// The priority, 0 to 65535; lower runs first among constructors.
    /// 65535 is the default and means the plain `.init_array` section.
    pub priority: u16,
    /// The function to call.
    pub func: SymId,
    /// The symbol the entry is associated with: when that symbol is in a
    /// COMDAT, the entry goes into the same group and is dropped with it.
    pub associated: Option<SymId>,
}

/// A construct a producer met and `hod_ir` cannot represent (§8).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Unsupported<'a> {
    /// The construct's name, in `classify`'s vocabulary:
    /// `attr:jumptable`, `global:prefix-data`.
    pub construct: &'a str,
    /// The symbol it was met on, if it belongs to one.
    pub symbol: Option<SymId>,
}

/// Lists of byte strings in the order they were added: the operands of
/// `!llvm.linker.options` and of `!llvm.dependent-libraries`.
#[derive(Clone, Debug, Default)]
pub struct StringLists {
    bytes: Vec<u8>,
    /// `(start, len)` in `bytes`.
    strings: Vec<(u32, u32)>,
    /// `(first, count)` in `strings`.
    lists: Vec<(u32, u32)>,
}

impl StringLists {
    fn push(&mut self, list: &[&[u8]]) -> IrResult<()> {
        let total = list.iter().map(|text| text.len()).sum();
        let mut start = pool_start(self.bytes.len(), total, "string list byte")?;
        let first = pool_start(self.strings.len(), list.len(), "string list")?;
        for text in list {
            self.bytes.extend_from_slice(text);
            self.strings.push((start, text.len() as u32));
            start += text.len() as u32;
        }
        self.lists.push((first, list.len() as u32));
        Ok(())
    }

    /// Number of lists.
    pub fn len(&self) -> usize {
        self.lists.len()
    }

    /// Whether there is no list.
    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }

    /// The lists, each as an iterator over its strings.
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = impl ExactSizeIterator<Item = &[u8]> + '_> + '_ {
        self.lists.iter().map(move |&(first, count)| {
            self.strings[first as usize..][..count as usize]
                .iter()
                .map(move |&(start, len)| &self.bytes[start as usize..][..len as usize])
        })
    }
}

/// One translation unit's IR.
///
/// The type table and the constant pool are public fields so that a
/// caller can borrow them separately: constant constructors take the type
/// table by reference (`module.consts.int(&module.types, ty, value)`).
/// Everything else is reached through methods. All tables are
/// append-only: an id, once returned, stays valid. The entry of a type or
/// constant never changes; a symbol's description changes only as §2.2
/// allows.
#[derive(Clone, Debug)]
pub struct Module {
    target: TargetSpec,
    /// The type table (§3), laid out for the target's data layout.
    pub types: TypeTable,
    /// The constant pool (§4).
    pub consts: ConstPool,
    /// The module properties (§2.1). A producer sets them directly or
    /// through [`Module::set_module_flag`].
    pub props: ModuleProps,
    /// The tables bodies only read: one copy, which the forks of a frozen
    /// module share (§5.1, "Reading bodies in parallel").
    shared: Tables,
    asms: AsmTable,
    fn_attrs: FnAttrTable,
    unsupported: Vec<(Box<str>, Option<SymId>)>,
    /// The debug tables (§6): nodes, expressions, locations, compile
    /// units. Empty, and without an allocation, for a module without
    /// debug information.
    pub debug: DebugInfo,
    /// What a fork of a frozen module knows of where it came from; `None`
    /// in a module that is not a fork.
    fork: Option<Box<ForkOrigin>>,
}

/// [`Shared`], owned until the module is frozen (so that changing it
/// costs no atomic operation), shared by the forks after. Owned in place,
/// not boxed: the one-thread reader reaches the symbol tables with no
/// pointer to follow.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant, reason = "one per module; a box costs every lookup a load")]
enum Tables {
    Own(Shared),
    Frozen(Arc<Shared>),
}

impl std::ops::Deref for Tables {
    type Target = Shared;

    #[inline]
    fn deref(&self) -> &Shared {
        match self {
            Tables::Own(shared) => shared,
            Tables::Frozen(shared) => shared,
        }
    }
}

/// The parts of a [`Module`] that reading a body does not add to.
#[derive(Clone, Debug, Default)]
struct Shared {
    symbols: SymbolTable,
    comdats: ComdatTable,
    flags: FlagTable,
    ctors: Vec<Structor>,
    dtors: Vec<Structor>,
    asm_text: Vec<u8>,
    linker_options: StringLists,
    dependent_libraries: StringLists,
    /// `!dbg` attachments of variables (§6.7), in the order attached.
    global_exprs: Vec<(SymId, DiId)>,
}

/// The shared tables, to change: in a fork, the change makes the fork's
/// work one it cannot hand over ([`Module::delta`]).
fn shared_mut<'a>(shared: &'a mut Tables, fork: &mut Option<Box<ForkOrigin>>) -> &'a mut Shared {
    if let Some(fork) = fork {
        fork.refused = true;
    }
    match shared {
        Tables::Own(shared) => shared,
        Tables::Frozen(shared) => Arc::make_mut(shared),
    }
}

impl Module {
    /// An empty module for `target`.
    pub fn new(target: TargetSpec) -> Module {
        let types = TypeTable::new(target.data_layout());
        Module {
            target,
            types,
            consts: ConstPool::new(),
            props: ModuleProps::default(),
            shared: Tables::Own(Shared::default()),
            asms: AsmTable::default(),
            fn_attrs: FnAttrTable::default(),
            unsupported: Vec::new(),
            debug: DebugInfo::default(),
            fork: None,
        }
    }

    /// What the module is compiled for.
    pub fn target(&self) -> &TargetSpec {
        &self.target
    }

    // ---- §2.2 symbols ----

    /// The symbol called `name`, created if the module has none.
    ///
    /// A new symbol is *unresolved*: it has an id that constants and
    /// instructions can use, and nothing else until one of the `set_*`
    /// methods describes it. This is how a producer refers to a symbol
    /// before it has seen its declaration.
    ///
    /// # Errors
    /// `InvalidSymbol` for an empty name or a name with a zero byte;
    /// `TableFull`.
    pub fn symbol_named(&mut self, name: &[u8]) -> IrResult<SymId> {
        // A module no fork shares a table with interns at once; one
        // that does looks first, so that finding changes nothing.
        if self.shares_tables()
            && let Some(sym) = self.shared.symbols.find(name)
        {
            return Ok(sym);
        }
        shared_mut(&mut self.shared, &mut self.fork).symbols.named(name)
    }

    /// A new unresolved symbol without a name (LLVM's `@0`). It can only
    /// become an `internal` or `private` symbol: an object file has no
    /// way to export a symbol without a name.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn anonymous_symbol(&mut self) -> IrResult<SymId> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.anonymous()
    }

    /// The symbol called `name`, if the module has one.
    #[inline]
    pub fn find_symbol(&self, name: &[u8]) -> Option<SymId> {
        self.shared.symbols.find(name)
    }

    /// The symbol `id`.
    ///
    /// # Panics
    /// If `id` is not a symbol of this module.
    #[inline]
    pub fn symbol(&self, id: SymId) -> Symbol<'_> {
        self.shared.symbols.get(id)
    }

    /// Number of symbols; their ids are `0..symbol_count()`.
    pub fn symbol_count(&self) -> usize {
        self.shared.symbols.len()
    }

    /// The symbols in id order.
    pub fn symbols(&self) -> impl ExactSizeIterator<Item = Symbol<'_>> + '_ {
        (0..self.shared.symbols.len() as u32).map(|raw| self.shared.symbols.get(SymId::new(raw)))
    }

    /// Makes `sym` the variable `desc` describes: a declaration, or with
    /// an initializer a definition.
    ///
    /// Like the other three `set_*` methods it replaces whatever `sym`
    /// was, of any kind, unless `sym` is a definition; the id and the
    /// "retained" marks stay. On an error `sym` is unchanged.
    ///
    /// # Errors
    /// - `SymbolRedefined` if `sym` is a definition.
    /// - `InvalidSymbol` for an anonymous symbol with a linkage that is
    ///   not local; a local linkage with a non-default visibility; an
    ///   `appending` variable that is not an array; an address space
    ///   above `MAX_ADDR_SPACE`; and for a definition that is
    ///   `extern_weak`, is `available_externally` in a COMDAT, or is
    ///   `common` and constant, in a COMDAT or not zero-initialized.
    /// - `InvalidType` for a value type that is `void`, `label`,
    ///   `metadata`, `token`, a function type or scalable.
    /// - `UnsizedType` for a definition whose type has no size.
    /// - `TypeMismatch` if the initializer's type is not the value type.
    /// - `InvalidOperand` for an alignment that is not a power of two up
    ///   to 2^32.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn set_variable(&mut self, sym: SymId, desc: &VariableDesc<'_>) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_variable(
            &self.types,
            &self.consts,
            sym,
            desc,
        )
    }

    /// Makes `sym` the function `desc` describes: a declaration, or with
    /// `has_body` a definition. See [`Module::set_variable`] for what it
    /// replaces.
    ///
    /// The parameter attributes are stored as given; that `sret` is on a
    /// pointer parameter and the like is the producer's to check
    /// (§5.7).
    ///
    /// # Errors
    /// - `SymbolRedefined` if `sym` is a definition.
    /// - `InvalidType` if `desc.ty` is not a function type.
    /// - `InvalidSymbol` for the linkage and visibility cases of
    ///   [`Module::set_variable`], and for a `common` or `appending`
    ///   function.
    /// - `InvalidOperand` for more parameter attributes than the type
    ///   has parameters, and for a bad alignment.
    ///
    /// # Panics
    /// If `sym` or `desc.personality` is not a symbol of this module.
    pub fn set_function(&mut self, sym: SymId, desc: &FunctionDesc<'_>) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_function(
            &self.types,
            &self.consts,
            sym,
            desc,
        )
    }

    /// Makes `sym` the alias `desc` describes. An alias is a definition.
    /// See [`Module::set_variable`] for what it replaces.
    ///
    /// What the target points at is checked by
    /// [`Module::verify_symbols`], so the target's symbol may be
    /// described later.
    ///
    /// # Errors
    /// - `SymbolRedefined` if `sym` is a definition.
    /// - `InvalidSymbol` for the linkage and visibility cases of
    ///   [`Module::set_variable`]; for an `extern_weak`, `common` or
    ///   `appending` alias; and for a target that is not a symbol
    ///   address, an absolute address or an unreduced expression (so not
    ///   `null`, `undef` or a block address).
    /// - `TypeMismatch` if the target is not a pointer.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn set_alias(&mut self, sym: SymId, desc: &AliasDesc) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_alias(
            &self.types,
            &self.consts,
            sym,
            desc,
        )
    }

    /// Makes `sym` the IFUNC `desc` describes. An IFUNC is a definition.
    /// See [`Module::set_variable`] for what it replaces.
    ///
    /// The resolver is checked by [`Module::verify_symbols`], so it may
    /// be described later.
    ///
    /// # Errors
    /// - `SymbolRedefined` if `sym` is a definition.
    /// - `InvalidSymbol` for the linkage and visibility cases of
    ///   [`Module::set_variable`], and for an `extern_weak`, `common`,
    ///   `appending` or `available_externally` IFUNC.
    ///
    /// # Panics
    /// If `sym` or `desc.resolver` is not a symbol of this module.
    pub fn set_ifunc(&mut self, sym: SymId, desc: &IFuncDesc) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_ifunc(sym, desc)
    }

    /// [`Module::symbol_named`], then [`Module::set_variable`].
    ///
    /// # Errors
    /// Those of the two.
    pub fn add_variable(&mut self, name: &[u8], desc: &VariableDesc<'_>) -> IrResult<SymId> {
        let sym = shared_mut(&mut self.shared, &mut self.fork).symbols.named(name)?;
        self.set_variable(sym, desc)?;
        Ok(sym)
    }

    /// [`Module::symbol_named`], then [`Module::set_function`].
    ///
    /// # Errors
    /// Those of the two.
    pub fn add_function(&mut self, name: &[u8], desc: &FunctionDesc<'_>) -> IrResult<SymId> {
        let sym = shared_mut(&mut self.shared, &mut self.fork).symbols.named(name)?;
        self.set_function(sym, desc)?;
        Ok(sym)
    }

    /// [`Module::symbol_named`], then [`Module::set_alias`].
    ///
    /// # Errors
    /// Those of the two.
    pub fn add_alias(&mut self, name: &[u8], desc: &AliasDesc) -> IrResult<SymId> {
        let sym = shared_mut(&mut self.shared, &mut self.fork).symbols.named(name)?;
        self.set_alias(sym, desc)?;
        Ok(sym)
    }

    /// [`Module::symbol_named`], then [`Module::set_ifunc`].
    ///
    /// # Errors
    /// Those of the two.
    pub fn add_ifunc(&mut self, name: &[u8], desc: &IFuncDesc) -> IrResult<SymId> {
        let sym = shared_mut(&mut self.shared, &mut self.fork).symbols.named(name)?;
        self.set_ifunc(sym, desc)?;
        Ok(sym)
    }

    /// Gives the declared variable `sym` its initializer, which makes it
    /// a definition.
    ///
    /// # Errors
    /// `InvalidSymbol` if `sym` is not a variable, `SymbolRedefined` if
    /// it has an initializer, `TypeMismatch` if `init` is not of its
    /// type, and what [`Module::set_variable`] refuses a definition for.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn set_initializer(&mut self, sym: SymId, init: ConstId) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_initializer(
            &self.types,
            &self.consts,
            sym,
            init,
        )
    }

    /// Says that a body will be delivered for the declared function
    /// `sym`, which makes it a definition.
    ///
    /// # Errors
    /// `InvalidSymbol` if `sym` is not a function, `SymbolRedefined` if
    /// it already gets a body, and what [`Module::set_function`] refuses
    /// a definition for.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn set_has_body(&mut self, sym: SymId) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.set_has_body(
            &self.types,
            &self.consts,
            sym,
        )
    }

    /// Takes the definition of `sym` away so that it can be described
    /// again, with the same id.
    ///
    /// A defined variable or function becomes an `external` declaration
    /// with no COMDAT (and a function loses its personality and
    /// subprogram), as LLVM's `convertToDeclaration` does; this is what
    /// happens to an `available_externally` body that will not be
    /// compiled. An alias or IFUNC becomes unresolved. Nothing happens
    /// to a declaration or an unresolved symbol.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn discard_definition(&mut self, sym: SymId) {
        shared_mut(&mut self.shared, &mut self.fork).symbols.discard_definition(sym);
    }

    /// Marks `sym` as a member of `llvm.used` or `llvm.compiler.used`.
    /// The marks are independent of what the symbol is and survive a
    /// new description.
    ///
    /// # Errors
    /// `InvalidSymbol` for an anonymous symbol.
    ///
    /// # Panics
    /// If `sym` is not a symbol of this module.
    pub fn retain(&mut self, sym: SymId, how: Retained) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).symbols.retain(sym, how)
    }

    /// The checks that need the whole module (§2.2 "End checks"); a
    /// producer calls it once, after the last symbol and constant.
    ///
    /// # Errors
    /// - `UndefinedSymbol` for a symbol that is still unresolved, and
    ///   for a constant that refers to a symbol id the module does not
    ///   have.
    /// - `InvalidSymbol` for a declaration whose linkage is not
    ///   `external` or `extern_weak` or that is in a COMDAT; an alias
    ///   whose target symbol is not a definition, is
    ///   `available_externally` (unless the alias is too, and then it
    ///   must be), or is a `weak` or `linkonce` alias; aliases that form
    ///   a cycle; and an IFUNC whose resolver is not a defined function
    ///   that returns a pointer, directly or through aliases at offset
    ///   zero.
    pub fn verify_symbols(&self) -> IrResult<()> {
        self.shared.symbols.verify(&self.types, &self.consts)
    }

    // ---- §2 COMDATs ----

    /// The COMDAT called `name`, created with kind `any` if the module
    /// has none. COMDAT names are a name space of their own.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn comdat_named(&mut self, name: &[u8]) -> IrResult<ComdatId> {
        // A module no fork shares a table with interns at once; one
        // that does looks first, so that finding changes nothing.
        if self.shares_tables()
            && let Some(id) = self.shared.comdats.find(name)
        {
            return Ok(id);
        }
        shared_mut(&mut self.shared, &mut self.fork).comdats.named(name)
    }

    /// Sets the selection kind of `id`.
    ///
    /// # Panics
    /// If `id` is not a COMDAT of this module.
    pub fn set_comdat_kind(&mut self, id: ComdatId, kind: ComdatKind) {
        shared_mut(&mut self.shared, &mut self.fork).comdats.set_kind(id, kind);
    }

    /// The COMDAT called `name`, if the module has one.
    pub fn find_comdat(&self, name: &[u8]) -> Option<ComdatId> {
        self.shared.comdats.find(name)
    }

    /// The COMDAT `id`.
    ///
    /// # Panics
    /// If `id` is not a COMDAT of this module.
    pub fn comdat(&self, id: ComdatId) -> Comdat<'_> {
        self.shared.comdats.get(id)
    }

    /// Number of COMDATs; their ids are `0..comdat_count()`.
    pub fn comdat_count(&self) -> usize {
        self.shared.comdats.len()
    }

    // ---- §2 inline assembly ----

    /// The id of the inline-assembly callee `asm`, added if new.
    ///
    /// # Errors
    /// `InvalidType` if `asm.ty` is not a function type,
    /// `InvalidOperand` for a flag bit that is not a flag, `TableFull`.
    pub fn inline_asm(&mut self, asm: &InlineAsm<'_>) -> IrResult<AsmId> {
        self.asms.intern(&self.types, asm)
    }

    /// The inline-assembly callee `id`.
    ///
    /// # Panics
    /// If `id` is not a callee of this module.
    pub fn asm(&self, id: AsmId) -> InlineAsm<'_> {
        self.asms.get(id)
    }

    /// Number of inline-assembly callees; their ids are `0..asm_count()`.
    pub fn asm_count(&self) -> usize {
        self.asms.len()
    }

    // ---- §5.7 function attributes ----

    /// The id of the function attribute set `attrs`, added if new. Equal
    /// sets have equal ids; the empty set is [`FnAttrsId::EMPTY`].
    ///
    /// # Errors
    /// `TableFull`.
    pub fn intern_fn_attrs(&mut self, attrs: &FnAttrs) -> IrResult<FnAttrsId> {
        self.fn_attrs.intern(attrs)
    }

    /// The function attribute set `id`.
    ///
    /// # Panics
    /// If `id` is not a set of this module.
    pub fn fn_attrs(&self, id: FnAttrsId) -> FnAttrsView<'_> {
        self.fn_attrs.get(id)
    }

    /// Number of distinct function attribute sets, the empty one
    /// included; their ids are `0..fn_attrs_count()`.
    pub fn fn_attrs_count(&self) -> usize {
        self.fn_attrs.len()
    }

    // ---- §2.1 module flags ----

    /// Records one LLVM module flag: `!{i32 behavior, !"name", value}`.
    ///
    /// A flag hod knows sets its field of [`Module::props`]. Any other
    /// flag, and every flag with the `Require` behavior (3), is kept and
    /// listed by [`Module::other_flags`].
    ///
    /// # Errors
    /// `InvalidModuleFlag` for a behavior outside 1 to 8; a name set
    /// twice (not counting `Require` flags); a `Max` or `Min` flag whose
    /// value is not an integer; a `Require`, `Append` or `AppendUnique`
    /// flag whose value is not a metadata node (`FlagValue::Other`); a
    /// `wchar_size` or `SemanticInterposition` flag whose value is not an
    /// integer; and a known flag whose value is not an
    /// integer or is not one of the flag's values. The last is stricter
    /// than LLVM, which takes any integer and fails or misbehaves later.
    pub fn set_module_flag(
        &mut self,
        behavior: u32,
        name: &[u8],
        value: FlagValue<'_>,
    ) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).flags.set(
            &mut self.props,
            behavior,
            name,
            value,
        )
    }

    /// The module flags that did not map onto [`Module::props`], in the
    /// order they were set. `classify` refuses a module that has one
    /// that is not `ignorable`.
    pub fn other_flags(&self) -> impl ExactSizeIterator<Item = ModuleFlag<'_>> {
        self.shared.flags.others()
    }

    // ---- §2.2 constructors and destructors ----

    /// Adds an entry of `llvm.global_ctors`. `priority` is the entry's
    /// `i32` read as unsigned; like LLVM, anything above 65535 is 65535.
    /// An entry whose function is `null` is not an entry: skip it.
    ///
    /// # Panics
    /// If `func` or `associated` is not a symbol of this module.
    pub fn add_ctor(&mut self, priority: u32, func: SymId, associated: Option<SymId>) {
        let structor = self.structor(priority, func, associated);
        shared_mut(&mut self.shared, &mut self.fork).ctors.push(structor);
    }

    /// Adds an entry of `llvm.global_dtors`; see [`Module::add_ctor`].
    ///
    /// # Panics
    /// If `func` or `associated` is not a symbol of this module.
    pub fn add_dtor(&mut self, priority: u32, func: SymId, associated: Option<SymId>) {
        let structor = self.structor(priority, func, associated);
        shared_mut(&mut self.shared, &mut self.fork).dtors.push(structor);
    }

    fn structor(&self, priority: u32, func: SymId, associated: Option<SymId>) -> Structor {
        // An id of another module panics here, not at code generation.
        let _ = self.shared.symbols.get(func);
        let _ = associated.map(|sym| self.shared.symbols.get(sym));
        Structor { priority: u16::try_from(priority).unwrap_or(u16::MAX), func, associated }
    }

    /// The constructors, in the order they were added.
    pub fn ctors(&self) -> &[Structor] {
        &self.shared.ctors
    }

    /// The destructors, in the order they were added.
    pub fn dtors(&self) -> &[Structor] {
        &self.shared.dtors
    }

    // ---- §2.1 module-level assembly, linker options ----

    /// Appends one `module asm "..."` string. As in LLVM, each string
    /// ends a line: a newline is added if `text` does not end in one.
    pub fn append_module_asm(&mut self, text: &[u8]) {
        shared_mut(&mut self.shared, &mut self.fork).asm_text.extend_from_slice(text);
        if text.last().is_some_and(|&last| last != b'\n') {
            shared_mut(&mut self.shared, &mut self.fork).asm_text.push(b'\n');
        }
    }

    /// The module-level assembly: the strings appended so far, one per
    /// line.
    pub fn module_asm(&self) -> &[u8] {
        &self.shared.asm_text
    }

    /// Adds one operand of `!llvm.linker.options`: a list of strings.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn add_linker_option(&mut self, strings: &[&[u8]]) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).linker_options.push(strings)
    }

    /// The operands of `!llvm.linker.options`.
    pub fn linker_options(&self) -> &StringLists {
        &self.shared.linker_options
    }

    /// Adds one operand of `!llvm.dependent-libraries`: a library name.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn add_dependent_library(&mut self, name: &[u8]) -> IrResult<()> {
        shared_mut(&mut self.shared, &mut self.fork).dependent_libraries.push(&[name])
    }

    /// The operands of `!llvm.dependent-libraries`, each a list of one
    /// string.
    pub fn dependent_libraries(&self) -> &StringLists {
        &self.shared.dependent_libraries
    }

    // ---- §8 what the producer could not represent ----

    /// Records that the producer met `construct`, which `hod_ir` cannot
    /// represent, on `symbol` or on the module. `classify` refuses a
    /// module with a record, naming the first; a record cannot be taken
    /// back.
    ///
    /// This is how a rejected attribute (`attr:<name>`, §5.7) and
    /// everything listed in §2.2 "Not representable" reaches `classify`
    /// without `hod_ir` growing a field for each.
    pub fn record_unsupported(&mut self, construct: &str, symbol: Option<SymId>) {
        self.unsupported.push((construct.into(), symbol));
    }

    /// The records of [`Module::record_unsupported`], in order.
    pub fn unsupported(&self) -> impl ExactSizeIterator<Item = Unsupported<'_>> {
        self.unsupported
            .iter()
            .map(|(construct, symbol)| Unsupported { construct, symbol: *symbol })
    }

    // ---- §6 debug information ----

    /// Attaches the global-variable expression `expr` to the variable
    /// `sym`: one `!dbg` attachment of a global variable (§6.7). A
    /// variable can have several, one for each fragment or each variable
    /// that shares its storage.
    ///
    /// The symbol's kind is not looked at here, because a producer may
    /// attach before it has described the symbol;
    /// [`Module::verify_debug`] checks it.
    ///
    /// # Errors
    /// `UndefinedSymbol` if `sym` is not a symbol of this module;
    /// `UndefinedDebugNode` if `expr` is not a node of it;
    /// `InvalidDebugInfo` if `expr` is defined as anything but a
    /// global-variable expression.
    pub fn attach_global_expr(&mut self, sym: SymId, expr: DiId) -> IrResult<()> {
        if sym.index() >= self.shared.symbols.len() {
            return err(IrErrorKind::UndefinedSymbol, format!("{sym:?} is not a symbol"));
        }
        match self.debug.find_kind(expr) {
            None => err(IrErrorKind::UndefinedDebugNode, format!("{expr:?} does not exist")),
            Some(DiKind::GlobalVariableExpression | DiKind::Undefined) => {
                shared_mut(&mut self.shared, &mut self.fork).global_exprs.push((sym, expr));
                Ok(())
            }
            Some(kind) => err(
                IrErrorKind::InvalidDebugInfo,
                format!("{expr:?} is a {}, not a DIGlobalVariableExpression", kind.llvm_name()),
            ),
        }
    }

    /// Every attachment of [`Module::attach_global_expr`], in the order
    /// made: the symbol that is the variable's storage and the
    /// global-variable expression. The DWARF writer reads this list
    /// once to find the address of each global it describes.
    pub fn global_exprs(&self) -> &[(SymId, DiId)] {
        &self.shared.global_exprs
    }

    /// The global-variable expressions attached to `sym`, in the order
    /// attached. A scan of [`Module::global_exprs`]: for one symbol, not
    /// for a loop over all of them.
    pub fn global_exprs_of(&self, sym: SymId) -> impl Iterator<Item = DiId> + '_ {
        self.shared
            .global_exprs
            .iter()
            .filter(move |(owner, _)| *owner == sym)
            .map(|&(_, expr)| expr)
    }

    /// Closes the debug tables (§6.3): a producer calls it once, after
    /// the last node and before the first body is compiled.
    ///
    /// It checks that every reserved node and location was defined (two
    /// compares when that holds), and records what the tables hold that
    /// hod cannot use with [`Module::record_unsupported`]:
    /// `debug:<kind>` for each kind of [`crate::DiOther`] node and
    /// `debug:DW_OP_<name>` for each [`crate::DiOp::Other`] element.
    /// Calling it again records nothing twice.
    ///
    /// # Errors
    /// `UndefinedDebugNode`, naming the first id that is not defined.
    pub fn finish_debug(&mut self) -> IrResult<()> {
        self.debug.check_defined()?;
        for construct in self.debug.unsupported_constructs() {
            if !self.unsupported.iter().any(|(known, _)| **known == *construct) {
                self.unsupported.push((construct.into(), None));
            }
        }
        Ok(())
    }

    /// Checks the debug information of the module (§6.8):
    /// [`DebugInfo::verify`] for the tables, that a constant used as
    /// metadata is in the constant pool, and for the attachments
    /// that a function's subprogram is a subprogram, that the one of a
    /// function with a body is distinct and attached to no other
    /// function with a body, and that a global-variable expression is
    /// attached to a variable.
    ///
    /// Like [`Module::verify_symbols`] it is one pass over the module,
    /// for tests and for a producer that wants its output checked; code
    /// generation does not need it to have run.
    ///
    /// # Errors
    /// `UndefinedDebugNode` and `InvalidDebugInfo`: the first violation
    /// found.
    pub fn verify_debug(&self) -> IrResult<()> {
        self.debug.verify()?;
        let invalid = |message: String| err(IrErrorKind::InvalidDebugInfo, message);
        for raw in 0..self.debug.node_count() as u32 {
            if let DiNode::Constant(node) = self.debug.node(DiId::new(raw))
                && node.value.index() >= self.consts.len()
            {
                return invalid(format!(
                    "di{raw}: {:?} is not a constant of the module",
                    node.value
                ));
            }
        }
        let mut owners: Vec<(DiId, SymId)> = Vec::new();
        for (index, symbol) in self.symbols().enumerate() {
            let sym = SymId::new(index as u32);
            let SymbolKind::Function { subprogram: Some(subprogram), has_body, .. } = symbol.kind()
            else {
                continue;
            };
            match self.debug.find_kind(subprogram) {
                Some(DiKind::Subprogram) => {}
                found => {
                    return invalid(format!(
                        "{sym:?}: its subprogram {subprogram:?} is {}",
                        found.map_or("no node", DiKind::llvm_name)
                    ));
                }
            }
            if has_body {
                if !self.debug.is_distinct(subprogram) {
                    return invalid(format!(
                        "{sym:?}: the subprogram {subprogram:?} of a defined function is not distinct"
                    ));
                }
                owners.push((subprogram, sym));
            }
        }
        owners.sort_unstable();
        if let Some(pair) = owners.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return invalid(format!(
                "{:?} is the subprogram of both {:?} and {:?}",
                pair[0].0, pair[0].1, pair[1].1
            ));
        }
        for &(sym, expr) in &self.shared.global_exprs {
            if !self.symbol(sym).is_variable() {
                return invalid(format!(
                    "{sym:?} has a global-variable expression and is no variable"
                ));
            }
            if self.debug.kind(expr) != DiKind::GlobalVariableExpression {
                return invalid(format!("{sym:?}: {expr:?} is not a DIGlobalVariableExpression"));
            }
        }
        Ok(())
    }
}

// ---- §5.1 reading bodies in parallel ----

/// What a fork knows of the module it was made of.
#[derive(Clone, Debug)]
struct ForkOrigin {
    /// A table bodies only read was changed: the fork's work since can
    /// not be handed over.
    refused: bool,
    props: ModuleProps,
    debug: DebugCounts,
}

/// See [`Module::stamp`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    shared: usize,
    props: ModuleProps,
    debug: DebugCounts,
    touched: bool,
}

/// Where a fork's tables end: the start of the next body's
/// [`Delta`] (`spec/ir.md` §5.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    types: usize,
    consts: usize,
    asms: usize,
    fn_attrs: usize,
    debug: (usize, usize),
    unsupported: usize,
}

/// What a fork added to its tables between a [`Mark`] and its end: the
/// types, constants, inline assembly, function attributes, debug
/// expressions and locations and unsupported constructs one body read
/// into it, owned, for [`Module::adopt`].
#[derive(Clone, Debug)]
pub struct Delta {
    types_from: usize,
    /// `None`: no type added (an empty table costs allocations).
    types: Option<Box<TypeTable>>,
    consts_from: usize,
    consts: Option<Box<ConstPool>>,
    asms: Vec<OwnedAsm>,
    fn_attrs: Vec<FnAttrs>,
    debug: DebugDelta,
    unsupported: Vec<(Box<str>, Option<SymId>)>,
}

/// A fork's ids to the ids of the module it was made of: the identity
/// below the fork's base, and below it what [`Module::adopt`] made of
/// each entry the fork added. One per fork, grown by every adopt of its
/// deltas, in their order.
#[derive(Clone, Debug)]
pub struct Remap {
    types_base: usize,
    types: Vec<TypeId>,
    consts_base: usize,
    consts: Vec<ConstId>,
    asms_base: usize,
    asms: Vec<AsmId>,
    pub(crate) debug: DebugRemap,
}

impl Remap {
    #[inline]
    pub(crate) fn ty(&self, ty: TypeId) -> TypeId {
        match ty.index().checked_sub(self.types_base) {
            Some(at) => self.types[at],
            None => ty,
        }
    }

    #[inline]
    pub(crate) fn constant(&self, id: ConstId) -> ConstId {
        match id.index().checked_sub(self.consts_base) {
            Some(at) => self.consts[at],
            None => id,
        }
    }

    #[inline]
    pub(crate) fn asm(&self, id: AsmId) -> AsmId {
        match id.index().checked_sub(self.asms_base) {
            Some(at) => self.asms[at],
            None => id,
        }
    }

    /// Whether every id maps to itself: the fork added nothing.
    pub fn is_identity(&self) -> bool {
        self.types.is_empty()
            && self.consts.is_empty()
            && self.asms.is_empty()
            && self.debug.is_identity()
    }
}

impl Module {
    /// Moves the tables bodies add to (types, constants, inline
    /// assembly, function attributes, debug expressions and locations)
    /// into a base that forks share, and the debug nodes with them, so
    /// that [`Module::fork`] copies nothing. A frozen module is used as
    /// before; [`Module::thaw`] undoes it once the forks are gone.
    pub fn freeze(&mut self) {
        if let Tables::Own(shared) = &mut self.shared {
            self.shared = Tables::Frozen(Arc::new(std::mem::take(shared)));
        }
        self.types.freeze();
        self.consts.freeze();
        self.asms.freeze();
        self.fn_attrs.freeze();
        self.debug.freeze();
    }

    /// Back to one layer per table, where no fork shares the base any
    /// more. The module is the same module before and after.
    pub fn thaw(&mut self) {
        if let Tables::Frozen(shared) = &mut self.shared
            && let Some(shared) = Arc::get_mut(shared)
        {
            self.shared = Tables::Own(std::mem::take(shared));
        }
        self.types.thaw();
        self.consts.thaw();
        self.asms.thaw();
        self.fn_attrs.thaw();
        self.debug.thaw();
    }

    /// A module that reads as this one, frozen, and adds to tables of its
    /// own: a body read against it is read as against this module, with
    /// the ids of what it adds its own. Shares every table; copies
    /// nothing but the properties, the debug compile units and the kinds
    /// of other debug nodes (a few words).
    ///
    /// # Panics
    /// In a debug build, if this module was not frozen since it was last
    /// added to.
    #[must_use]
    pub fn fork(&self) -> Module {
        debug_assert!(
            self.types.local_is_empty()
                && self.consts.local_is_empty()
                && self.asms.local_is_empty()
                && self.fn_attrs.local_is_empty(),
            "a fork of a module that is not frozen"
        );
        let debug = self.debug.fork();
        let origin = ForkOrigin { refused: false, props: self.props, debug: debug.counts() };
        Module {
            target: self.target.clone(),
            types: self.types.fork(),
            consts: self.consts.fork(),
            props: self.props,
            shared: match &self.shared {
                Tables::Frozen(shared) => Tables::Frozen(Arc::clone(shared)),
                Tables::Own(shared) => Tables::Own(shared.clone()),
            },
            asms: self.asms.fork(),
            fn_attrs: self.fn_attrs.fork(),
            unsupported: Vec::new(),
            debug,
            fork: Some(Box::new(origin)),
        }
    }

    /// The remap of a fork made now: the identity until a delta is
    /// adopted with it.
    pub fn remap(&self) -> Remap {
        Remap {
            types_base: self.types.len(),
            types: Vec::new(),
            consts_base: self.consts.len(),
            consts: Vec::new(),
            asms_base: self.asms.len(),
            asms: Vec::new(),
            debug: self.debug.remap(),
        }
    }

    /// Where the tables end now.
    pub fn mark(&self) -> Mark {
        Mark {
            types: self.types.len(),
            consts: self.consts.len(),
            asms: self.asms.len(),
            fn_attrs: self.fn_attrs.len(),
            debug: self.debug.mark(),
            unsupported: self.unsupported.len(),
        }
    }

    /// What this fork added since `mark`, owned. `None` if, since it was
    /// made, the fork changed what it cannot hand over: a symbol, a
    /// COMDAT, a module property or flag, a debug node, a reserved
    /// location; or if it is not a fork.
    ///
    /// # Errors
    /// None in practice: a function attribute set that no longer interns.
    pub fn delta(&self, mark: Mark) -> IrResult<Option<Delta>> {
        let Some(origin) = &self.fork else { return Ok(None) };
        if origin.refused || self.props != origin.props || self.debug.changed_since(&origin.debug) {
            return Ok(None);
        }
        let Some(debug) = self.debug.delta(mark.debug) else { return Ok(None) };
        Ok(Some(Delta {
            types_from: mark.types,
            types: (self.types.len() > mark.types).then(|| Box::new(self.types.tail(mark.types))),
            consts_from: mark.consts,
            consts: (self.consts.len() > mark.consts)
                .then(|| Box::new(self.consts.tail(mark.consts))),
            asms: self.asms.owned_since(mark.asms),
            fn_attrs: self.fn_attrs.owned_since(mark.fn_attrs)?,
            debug,
            unsupported: self.unsupported[mark.unsupported..].to_vec(),
        }))
    }

    /// Adds what `delta` holds, as the fork added it and in the same
    /// order, with every id translated by `remap`, which is extended with
    /// the ids the new entries became. Adopting a fork's deltas in the
    /// order of their bodies leaves this module as reading those bodies
    /// against it would have (`spec/ir.md` §5.1).
    ///
    /// # Errors
    /// `TableFull`, as reading the bodies would have.
    pub fn adopt(&mut self, delta: &Delta, remap: &mut Remap) -> IrResult<()> {
        if let Some(types) = &delta.types {
            self.types.adopt(types, delta.types_from, &mut remap.types, remap.types_base)?;
        }
        if let Some(consts) = &delta.consts {
            self.consts.adopt(
                consts,
                delta.consts_from,
                &mut remap.consts,
                remap.consts_base,
                &remap.types,
                remap.types_base,
            )?;
        }
        for asm in &delta.asms {
            let id = self.asms.intern(&self.types, &asm.view(remap.ty(asm.ty)))?;
            remap.asms.push(id);
        }
        for attrs in &delta.fn_attrs {
            self.fn_attrs.intern(attrs)?;
        }
        self.debug.adopt(&delta.debug, &mut remap.debug)?;
        self.unsupported.extend(delta.unsupported.iter().cloned());
        Ok(())
    }

    /// What changes when the module changes a table that its forks read
    /// and do not add to: a symbol, a COMDAT, a property or flag, a debug
    /// node, a location in place. Equal stamps: the forks still read
    /// this module.
    pub fn stamp(&self) -> Stamp {
        Stamp {
            shared: std::ptr::from_ref::<Shared>(&self.shared) as usize,
            props: self.props,
            debug: self.debug.counts(),
            touched: self.debug.touched(),
        }
    }

    /// Whether this module is a fork ([`Module::fork`]).
    pub fn is_fork(&self) -> bool {
        self.fork.is_some()
    }

    /// Whether this module is a fork or frozen: whether a fork may share
    /// its tables.
    #[inline]
    fn shares_tables(&self) -> bool {
        matches!(self.shared, Tables::Frozen(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{FnStr, ParamAttrs};
    use crate::error::IrErrorKind;
    use crate::ids::TypeId;
    use crate::props::{BranchProtection, FlagBehavior, PicLevel};
    use crate::symbol::{Linkage, SymbolKind, TlsModel};

    fn module() -> Module {
        Module::new(TargetSpec::x86_64_linux_gnu())
    }

    /// §2.1: a module's type table is laid out for its target.
    #[test]
    fn module_tables_follow_the_target() {
        let a64 = Module::new(TargetSpec::aarch64_linux_gnu());
        let x64 = Module::new(TargetSpec::x86_64_linux_gnu());
        assert_eq!(a64.target().triple(), "aarch64-unknown-linux-gnu");
        assert_eq!(a64.types.data_layout(), a64.target().data_layout());
        assert_eq!(a64.types.pref_align_of(TypeId::I8), 4);
        assert_eq!(x64.types.pref_align_of(TypeId::I8), 1);
        assert_eq!(a64.consts.len(), 0);
        assert_eq!(a64.symbol_count(), 0);
        assert_eq!(a64.props, ModuleProps::default());
    }

    /// §2.2: the shape of a small C++ translation unit through the
    /// module API: a callee met before its declaration, an inline
    /// function in a COMDAT, a string constant, a constructor.
    #[test]
    fn symbols_through_the_module() {
        let mut m = module();
        let void_fn = m.types.function(TypeId::VOID, &[], false).unwrap();
        let int_fn = m.types.function(TypeId::I32, &[TypeId::PTR], false).unwrap();

        // `main` calls `puts` before anything says what `puts` is.
        let puts = m.symbol_named(b"puts").unwrap();
        assert!(m.symbol(puts).is_unresolved());
        let mut attrs = FnAttrs::new();
        attrs.set(FnAttrs::NOUNWIND).unwrap();
        attrs.set_string(FnStr::TargetCpu, b"x86-64").unwrap();
        let attrs = m.intern_fn_attrs(&attrs).unwrap();
        let main = m
            .add_function(
                b"main",
                &FunctionDesc {
                    dso_local: true,
                    attrs,
                    has_body: true,
                    ..FunctionDesc::new(void_fn)
                },
            )
            .unwrap();
        m.set_function(puts, &FunctionDesc::new(int_fn)).unwrap();

        let group = m.comdat_named(b"_Z6inlinev").unwrap();
        let inline = m
            .add_function(
                b"_Z6inlinev",
                &FunctionDesc {
                    linkage: Linkage::LinkOnceOdr,
                    comdat: Some(group),
                    has_body: true,
                    ..FunctionDesc::new(void_fn)
                },
            )
            .unwrap();

        let text = m.consts.bytes(&mut m.types, b"hi\0").unwrap();
        let string = m.anonymous_symbol().unwrap();
        m.set_variable(
            string,
            &VariableDesc {
                linkage: Linkage::Private,
                constant: true,
                init: Some(text),
                ..VariableDesc::new(m.consts.ty(text))
            },
        )
        .unwrap();

        let counter = m.add_variable(b"counter", &VariableDesc::new(TypeId::I32)).unwrap();
        let zero = m.consts.int(&m.types, TypeId::I32, 0).unwrap();
        m.set_initializer(counter, zero).unwrap();
        let address = m.consts.addr(counter, 0).unwrap();
        let alias = m.add_alias(b"counter_alias", &AliasDesc::new(TypeId::I32, address)).unwrap();
        let resolver_ty = m.types.function(TypeId::PTR, &[], false).unwrap();
        let resolver = m.add_function(b"resolve", &FunctionDesc::new(resolver_ty)).unwrap();
        m.set_has_body(resolver).unwrap();
        let ifunc = m.add_ifunc(b"dispatched", &IFuncDesc::new(void_fn, resolver)).unwrap();
        m.retain(counter, Retained::Used).unwrap();
        m.verify_symbols().unwrap();

        assert_eq!(m.symbol_count(), 8);
        assert_eq!(m.find_symbol(b"main"), Some(main));
        assert_eq!(m.find_symbol(b"nothing"), None);
        assert_eq!(m.symbol(inline).comdat(), Some(group));
        assert_eq!(m.comdat(group).name, b"_Z6inlinev");
        assert!(m.symbol(string).is_anonymous());
        assert!(m.symbol(counter).is_used());
        assert!(matches!(m.symbol(alias).kind(), SymbolKind::Alias { tls: TlsModel::None, .. }));
        assert_eq!(m.symbol(ifunc).kind(), SymbolKind::IFunc { resolver });
        let SymbolKind::Function { attrs: read, has_body, .. } = m.symbol(main).kind() else {
            panic!("main is a function");
        };
        assert!(has_body);
        assert!(m.fn_attrs(read).has(FnAttrs::NOUNWIND));
        assert_eq!(m.fn_attrs(read).string(FnStr::TargetCpu), Some(&b"x86-64"[..]));
        assert_eq!(m.fn_attrs_count(), 2);
        assert_eq!(m.symbol(puts).param_attrs(0), ParamAttrs::NONE);
        let names: Vec<&[u8]> = m.symbols().map(|symbol| symbol.name()).collect();
        assert_eq!(names[..3], [&b"puts"[..], b"main", b"_Z6inlinev"]);
        assert_eq!(m.symbols().len(), 8);

        // A definition is replaced only after it has been discarded.
        let again = FunctionDesc { has_body: true, ..FunctionDesc::new(int_fn) };
        assert_eq!(m.add_function(b"main", &again).unwrap_err().kind, IrErrorKind::SymbolRedefined);
        m.discard_definition(main);
        assert_eq!(m.add_function(b"main", &again).unwrap(), main);
        assert_eq!(m.symbol(main).ty(), int_fn);
        // The by-name forms fail on a bad name before they create anything.
        let count = m.symbol_count();
        let decl = VariableDesc::new(TypeId::I32);
        assert_eq!(m.add_variable(b"", &decl).unwrap_err().kind, IrErrorKind::InvalidSymbol);
        assert_eq!(m.symbol_count(), count);
    }

    /// §2: COMDATs and inline assembly through the module.
    #[test]
    fn comdats_and_inline_asm_through_the_module() {
        let mut m = module();
        let a = m.comdat_named(b"a").unwrap();
        assert_eq!(m.comdat_named(b"a").unwrap(), a);
        assert_eq!(m.find_comdat(b"a"), Some(a));
        assert_eq!(m.find_comdat(b"b"), None);
        m.set_comdat_kind(a, ComdatKind::Largest);
        assert_eq!(m.comdat(a), Comdat { name: b"a", kind: ComdatKind::Largest });
        assert_eq!(m.comdat_count(), 1);
        // A COMDAT and a symbol of one name are unrelated.
        assert_eq!(m.find_symbol(b"a"), None);

        let ty = m.types.function(TypeId::VOID, &[], false).unwrap();
        let nop =
            InlineAsm { template: b"nop", constraints: b"", flags: InlineAsm::SIDE_EFFECT, ty };
        let id = m.inline_asm(&nop).unwrap();
        assert_eq!(m.inline_asm(&nop).unwrap(), id);
        assert_eq!(m.asm(id), nop);
        assert_eq!(m.asm_count(), 1);
        let bad = InlineAsm { ty: TypeId::I32, ..nop };
        assert_eq!(m.inline_asm(&bad).unwrap_err().kind, IrErrorKind::InvalidType);
    }

    /// §2.1: module flags set the properties or are kept by name.
    #[test]
    fn module_flags_through_the_module() {
        let mut m = module();
        m.set_module_flag(8, b"PIC Level", FlagValue::Int(2)).unwrap();
        m.set_module_flag(8, b"cf-protection-branch", FlagValue::Int(1)).unwrap();
        m.set_module_flag(1, b"wchar_size", FlagValue::Int(4)).unwrap();
        m.set_module_flag(7, b"RtLibUseGOT", FlagValue::Int(1)).unwrap();
        assert_eq!(m.props.pic, PicLevel::Big);
        assert!(m.props.branch_protection.has(BranchProtection::CET_IBT));
        let others: Vec<ModuleFlag<'_>> = m.other_flags().collect();
        assert_eq!(others.len(), 2);
        assert!(others[0].ignorable);
        assert_eq!(others[1].name, b"RtLibUseGOT");
        assert_eq!(others[1].behavior, FlagBehavior::Max);
        assert!(!others[1].ignorable);
        assert_eq!(
            m.set_module_flag(8, b"PIC Level", FlagValue::Int(2)).unwrap_err().kind,
            IrErrorKind::InvalidModuleFlag
        );
        // A producer may also set a property directly.
        m.props.pie = PicLevel::Big;
        assert_eq!(m.props.pie, PicLevel::Big);
    }

    /// §2.2: constructors and destructors keep their order, and a
    /// priority above 65535 is 65535 (what `llc` does with the `i32`).
    #[test]
    fn ctors_and_dtors() {
        let mut m = module();
        let void_fn = m.types.function(TypeId::VOID, &[], false).unwrap();
        let init = m.add_function(b"init", &FunctionDesc::new(void_fn)).unwrap();
        let fini = m.add_function(b"fini", &FunctionDesc::new(void_fn)).unwrap();
        let data = m.add_variable(b"data", &VariableDesc::new(TypeId::I32)).unwrap();
        m.add_ctor(65535, init, None);
        m.add_ctor(101, init, Some(data));
        m.add_ctor(70000, fini, None);
        m.add_ctor(u32::MAX, fini, None);
        m.add_dtor(0, fini, Some(data));
        assert_eq!(
            m.ctors(),
            [
                Structor { priority: 65535, func: init, associated: None },
                Structor { priority: 101, func: init, associated: Some(data) },
                Structor { priority: 65535, func: fini, associated: None },
                Structor { priority: 65535, func: fini, associated: None },
            ]
        );
        assert_eq!(m.dtors(), [Structor { priority: 0, func: fini, associated: Some(data) }]);
        assert_eq!(size_of::<Structor>(), 16);
    }

    /// §2.1, §7: module-level assembly is one text with a line per
    /// string; linker options and dependent libraries keep their order.
    #[test]
    fn module_asm_and_linker_options() {
        let mut m = module();
        assert_eq!(m.module_asm(), b"");
        m.append_module_asm(b".globl foo");
        m.append_module_asm(b"foo:\n");
        m.append_module_asm(b"");
        m.append_module_asm(b"  ret");
        assert_eq!(m.module_asm(), b".globl foo\nfoo:\n  ret\n");

        assert!(m.linker_options().is_empty());
        m.add_linker_option(&[b"lib", b"m"]).unwrap();
        m.add_linker_option(&[]).unwrap();
        m.add_linker_option(&[b"-lpthread"]).unwrap();
        m.add_dependent_library(b"z").unwrap();
        let read = |lists: &StringLists| -> Vec<Vec<Vec<u8>>> {
            lists.iter().map(|list| list.map(<[u8]>::to_vec).collect()).collect()
        };
        assert_eq!(
            read(m.linker_options()),
            [vec![b"lib".to_vec(), b"m".to_vec()], vec![], vec![b"-lpthread".to_vec()]]
        );
        assert_eq!(m.linker_options().len(), 3);
        assert_eq!(read(m.dependent_libraries()), [vec![b"z".to_vec()]]);
    }

    /// §8: what a producer could not represent is recorded by name for
    /// `classify`, with the symbol it was met on.
    #[test]
    fn unsupported_constructs_are_recorded() {
        let mut m = module();
        assert_eq!(m.unsupported().len(), 0);
        let f = m.symbol_named(b"f").unwrap();
        m.record_unsupported("attr:jumptable", Some(f));
        m.record_unsupported("global:partition", None);
        let records: Vec<Unsupported<'_>> = m.unsupported().collect();
        assert_eq!(
            records,
            [
                Unsupported { construct: "attr:jumptable", symbol: Some(f) },
                Unsupported { construct: "global:partition", symbol: None },
            ]
        );
    }
}
