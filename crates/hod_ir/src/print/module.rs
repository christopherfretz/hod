//! What a module owns, as text (`spec/ir.md` §9.4): the target, module
//! assembly, COMDATs, every symbol, the retained lists, constructors,
//! intrinsic declarations, attribute groups and the metadata.

use std::fmt::Write;

use super::body::write_intrinsic_name;
use super::{Part, PrintError, R, Writer, write_escaped, write_name, write_quoted};
use crate::attrs::{FnAttrs, FnAttrsId, FnStr, ParamAttrs, ParamMem, UwTable};
use crate::body::Body;
use crate::constant::Const;
use crate::ids::{ComdatId, ConstId, SymId, TypeId};
use crate::intrinsic::Intrinsic;
use crate::module::{StringLists, Structor};
use crate::props::{BranchProtection, FlagValue};
use crate::symbol::{Linkage, Symbol, SymbolKind, TlsModel, UnnamedAddr, Visibility};
use crate::target::{AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19};
use crate::types::TypeKind;

/// The branch-protection flags in the order of `KNOWN_MODULE_FLAGS`, and
/// whether each is one of the AArch64 ones.
const PROTECTION_FLAGS: [(u8, &str, bool); 8] = [
    (BranchProtection::BTI, "branch-target-enforcement", true),
    (BranchProtection::GCS, "guarded-control-stack", true),
    (BranchProtection::PAC_RET, "sign-return-address", true),
    (BranchProtection::PAC_RET_ALL, "sign-return-address-all", true),
    (BranchProtection::PAC_B_KEY, "sign-return-address-with-bkey", true),
    (BranchProtection::PAC_PAUTH_LR, "branch-protection-pauth-lr", true),
    (BranchProtection::CET_IBT, "cf-protection-branch", false),
    (BranchProtection::CET_SHSTK, "cf-protection-return", false),
];

/// The first LLVM major that knows a function attribute keyword.
fn attr_since(name: &str) -> u32 {
    match name {
        "sanitize_realtime" | "sanitize_realtime_blocking" | "sanitize_type" => 20,
        "sanitize_alloc_token" => 22,
        _ => 19,
    }
}

impl<'a, W: Write + ?Sized> Writer<'a, W> {
    /// The whole text of one part.
    pub(super) fn module(&mut self) -> R {
        let m = self.m;
        // What the producer could not represent is missing from the text,
        // and the object would differ.
        if let Some(record) = m.unsupported().next() {
            return Err(PrintError::Unsupported {
                construct: record.construct.to_string(),
                symbol: record.symbol,
            });
        }
        if let Some(symbol) = m.symbols().find(Symbol::is_unresolved) {
            self.cur = Some(symbol.id());
            return self.unsupported("unresolved-symbol");
        }
        if self.part != Part::Whole {
            let mut whole = vec![true; m.comdat_count()];
            for symbol in m.symbols() {
                let slot = symbol.comdat().and_then(|comdat| whole.get_mut(comdat.index()));
                if let Some(slot) = slot {
                    *slot &= self.defined(symbol.id());
                }
            }
            self.comdat_whole = whole;
        }

        self.header()?;
        self.module_asm()?;
        for index in 0..m.types.len() {
            if let TypeKind::Opaque(number) = m.types.kind(TypeId::new(index as u32)) {
                writeln!(self.out, "%hod.opaque.{number} = type opaque")?;
            }
        }
        for index in 0..m.comdat_count() {
            let comdat = m.comdat(ComdatId::new(index as u32));
            write_name(self.out, '$', comdat.name)?;
            writeln!(self.out, " = comdat {}", comdat.kind.keyword())?;
        }
        for symbol in m.symbols() {
            self.symbol(symbol)?;
        }
        self.cur = None;
        self.retained("llvm.used", Symbol::is_used)?;
        self.retained("llvm.compiler.used", Symbol::is_compiler_used)?;
        if self.part != Part::Rest {
            self.structors("llvm.global_ctors", m.ctors())?;
            self.structors("llvm.global_dtors", m.dtors())?;
        }
        self.intrinsic_decls()?;
        self.attribute_groups()?;
        self.metadata()
    }

    /// Whether LLVM 19's AArch64 data layout is written in place of the
    /// module's: the same layout but for the preferred alignment of `i1`
    /// (§9.2) and the address spaces 270 to 272.
    fn old_aarch64_layout(&self) -> bool {
        self.dialect.major() < 20
            && self.m.target().data_layout_str() == AARCH64_LINUX_DATA_LAYOUT
    }

    fn header(&mut self) -> R {
        let target = self.m.target();
        let layout = if self.old_aarch64_layout() {
            AARCH64_LINUX_DATA_LAYOUT_LLVM19
        } else {
            target.data_layout_str()
        };
        writeln!(self.out, "target datalayout = \"{layout}\"")?;
        writeln!(self.out, "target triple = \"{}\"", target.triple())?;
        Ok(())
    }

    /// Whether a pointer type of `addr_space` has, in the layout the text
    /// states, the width the module's type table gave it.
    pub(super) fn addr_space_is_laid_out(&self, addr_space: u32) -> bool {
        if !self.old_aarch64_layout() {
            return true;
        }
        let layout = self.m.types.data_layout();
        let (here, plain) = (layout.pointer(addr_space), layout.pointer(0));
        (here.bits, here.index_bits, here.align) == (plain.bits, plain.index_bits, plain.align)
    }

    /// One `module asm` per line. Every string a producer appended ends
    /// its line, so nothing follows the last newline.
    fn module_asm(&mut self) -> R {
        let text = self.m.module_asm();
        if self.part == Part::Rest || text.is_empty() {
            return Ok(());
        }
        for line in text.strip_suffix(b"\n").unwrap_or(text).split(|&byte| byte == b'\n') {
            self.s("module asm ")?;
            write_quoted(self.out, line)?;
            self.s("\n")?;
        }
        Ok(())
    }

    // ---- which symbols this text defines ----

    /// Whether this text defines symbol `id` (§9.6): a function whose body
    /// was supplied, a variable with an initializer (not in `Part::Rest`),
    /// an alias or IFUNC of something defined.
    pub(super) fn defined(&self, id: SymId) -> bool {
        let m = self.m;
        let mut id = id;
        // A chain of aliases longer than the table is a cycle.
        for _ in 0..=m.symbol_count() {
            if id.index() >= m.symbol_count() {
                return false;
            }
            match m.symbol(id).kind() {
                SymbolKind::Unresolved => return false,
                SymbolKind::Variable { init, .. } => {
                    return init.is_some() && self.part != Part::Rest;
                }
                SymbolKind::Function { .. } => return self.fn_defined(id),
                SymbolKind::IFunc { resolver } => id = resolver,
                SymbolKind::Alias { target, .. } => match self.alias_target(target) {
                    Some(next) => id = next,
                    // An absolute address or an expression: no symbol to
                    // wait for.
                    None => return self.part != Part::Rest,
                },
            }
        }
        false
    }

    /// The symbol an aliasee names, if it is a symbol's address.
    fn alias_target(&self, target: ConstId) -> Option<SymId> {
        if target.index() >= self.m.consts.len() {
            return None;
        }
        match self.m.consts.get(target) {
            Const::Addr { sym, .. } => Some(sym),
            _ => None,
        }
    }

    /// The COMDAT a definition is written in. A split keeps a COMDAT only
    /// in a part that defines all its members; elsewhere a member that the
    /// linker may see twice (weak, or local before the split) leaves it.
    fn comdat_of(&self, symbol: Symbol<'_>) -> Result<Option<ComdatId>, PrintError> {
        let Some(comdat) = symbol.comdat() else { return Ok(None) };
        if comdat.index() >= self.m.comdat_count() {
            return self.malformed(format!("{comdat:?} is not a COMDAT of the module"));
        }
        if self.part == Part::Whole
            || self.comdat_whole.get(comdat.index()).copied().unwrap_or(false)
        {
            return Ok(Some(comdat));
        }
        match symbol.linkage() {
            Linkage::LinkOnceAny
            | Linkage::LinkOnceOdr
            | Linkage::WeakAny
            | Linkage::WeakOdr
            | Linkage::Internal
            | Linkage::Private => Ok(None),
            _ => self.unsupported("split:comdat"),
        }
    }

    // ---- symbols ----

    fn symbol(&mut self, symbol: Symbol<'a>) -> R {
        let id = symbol.id();
        self.cur = Some(id);
        match symbol.kind() {
            SymbolKind::Unresolved => self.unsupported("unresolved-symbol"),
            SymbolKind::Variable { constant, tls, init } => {
                self.variable(symbol, constant, tls, init)
            }
            SymbolKind::Function { .. } => {
                let body = self.bodies.get(id.index()).copied().flatten();
                self.function(symbol, symbol, body)
            }
            SymbolKind::Alias { tls, target } => self.alias(symbol, tls, target),
            SymbolKind::IFunc { resolver } => self.ifunc(symbol, resolver),
        }
    }

    /// `linkage dso_local visibility`, each with a space after it.
    /// `named_external` is whether an external declaration says so
    /// (a variable does, a `declare` does not).
    pub(super) fn linkage(&mut self, symbol: Symbol<'_>, define: bool, named_external: bool) -> R {
        let (linkage, visibility) = self.linkage_of(symbol, define);
        if linkage != Linkage::External || (!define && named_external) {
            self.s(linkage.keyword())?;
            self.s(" ")?;
        }
        let implied = linkage.is_local()
            || (visibility != Visibility::Default && linkage != Linkage::ExternWeak);
        if symbol.dso_local() && !implied {
            self.s("dso_local ")?;
        }
        self.s(match visibility {
            Visibility::Default => "",
            Visibility::Hidden => "hidden ",
            Visibility::Protected => "protected ",
        })
    }

    fn tls(&mut self, tls: TlsModel) -> R {
        self.s(match tls {
            TlsModel::None => "",
            TlsModel::GeneralDynamic => "thread_local ",
            TlsModel::LocalDynamic => "thread_local(localdynamic) ",
            TlsModel::InitialExec => "thread_local(initialexec) ",
            TlsModel::LocalExec => "thread_local(localexec) ",
        })
    }

    fn unnamed_addr(&mut self, symbol: Symbol<'_>) -> R {
        self.s(match symbol.unnamed_addr() {
            UnnamedAddr::None => "",
            UnnamedAddr::Local => "local_unnamed_addr ",
            UnnamedAddr::Global => "unnamed_addr ",
        })
    }

    fn variable(
        &mut self,
        symbol: Symbol<'a>,
        constant: bool,
        tls: TlsModel,
        init: Option<ConstId>,
    ) -> R {
        let m = self.m;
        let define = init.is_some() && self.part != Part::Rest;
        self.sym_name(symbol.id())?;
        self.s(" = ")?;
        self.linkage(symbol, define, true)?;
        self.tls(tls)?;
        self.unnamed_addr(symbol)?;
        if symbol.addr_space() != 0 {
            write!(self.out, "addrspace({}) ", symbol.addr_space())?;
        }
        self.s(if constant { "constant " } else { "global " })?;
        self.ty(symbol.ty())?;
        if let (true, Some(init)) = (define, init) {
            self.s(" ")?;
            self.constant(init)?;
        }
        if let Some(section) = symbol.section() {
            self.s(", section ")?;
            write_quoted(self.out, section)?;
        }
        if define && let Some(comdat) = self.comdat_of(symbol)? {
            self.s(", comdat(")?;
            write_name(self.out, '$', m.comdat(comdat).name)?;
            self.s(")")?;
        }
        let mut align = symbol.align();
        if align.is_none() && define && self.dialect.major() < 20 && self.is_bits(symbol.ty()) {
            // LLVM 19 prefers byte alignment for `i1`; later ones, and the
            // module's type table, give it `i8`'s.
            align = Some(m.types.global_align(symbol.ty(), None, symbol.section().is_some()));
        }
        if let Some(align) = align {
            write!(self.out, ", align {align}")?;
        }
        self.s("\n")
    }

    /// Whether `ty` is `i1` or an array, at any depth, of `i1`.
    fn is_bits(&self, ty: TypeId) -> bool {
        let types = &self.m.types;
        let mut ty = ty;
        loop {
            match types.kind(ty) {
                TypeKind::Int(1) => return true,
                TypeKind::Array { elem, .. } => ty = elem,
                _ => return false,
            }
        }
    }

    fn alias(&mut self, symbol: Symbol<'a>, tls: TlsModel, target: ConstId) -> R {
        let m = self.m;
        let id = symbol.id();
        if self.defined(id) {
            self.sym_name(id)?;
            self.s(" = ")?;
            self.linkage(symbol, true, false)?;
            self.tls(tls)?;
            self.unnamed_addr(symbol)?;
            self.s("alias ")?;
            self.ty(symbol.ty())?;
            self.s(", ")?;
            self.typed_const(target)?;
            return self.s("\n");
        }
        // What it is an alias of is defined in another text: there it is
        // an alias, and here a declaration of what it is an alias as.
        if matches!(m.types.kind(symbol.ty()), TypeKind::Function { .. }) {
            // The callee's convention and attributes, when it is known.
            let mut proto = symbol;
            let mut next = Some(id);
            for _ in 0..=m.symbol_count() {
                let Some(at) = next.filter(|at| at.index() < m.symbol_count()) else { break };
                let base = m.symbol(at);
                match base.kind() {
                    SymbolKind::Alias { target, .. } => next = self.alias_target(target),
                    SymbolKind::Function { .. } if base.ty() == symbol.ty() => {
                        proto = base;
                        break;
                    }
                    _ => break,
                }
            }
            return self.function(symbol, proto, None);
        }
        self.sym_name(id)?;
        self.s(" = ")?;
        self.linkage(symbol, false, true)?;
        self.tls(tls)?;
        self.unnamed_addr(symbol)?;
        self.s("global ")?;
        if matches!(m.types.kind(symbol.ty()), TypeKind::Void | TypeKind::Label) {
            self.s("i8")?;
        } else {
            self.ty(symbol.ty())?;
        }
        self.s("\n")
    }

    fn ifunc(&mut self, symbol: Symbol<'a>, resolver: SymId) -> R {
        let id = symbol.id();
        if self.defined(resolver) {
            self.sym_name(id)?;
            self.s(" = ")?;
            self.linkage(symbol, true, false)?;
            self.unnamed_addr(symbol)?;
            self.s("ifunc ")?;
            self.ty(symbol.ty())?;
            self.s(", ptr ")?;
            self.sym_ref(resolver)?;
            return self.s("\n");
        }
        // The other text has the resolver, and so the IFUNC.
        if matches!(self.m.types.kind(symbol.ty()), TypeKind::Function { .. }) {
            return self.function(symbol, symbol, None);
        }
        self.sym_name(id)?;
        self.s(" = ")?;
        self.linkage(symbol, false, true)?;
        self.s("global i8\n")
    }

    /// A function's `define` or `declare`. `symbol` gives the name, the
    /// type and the linkage; `proto` the convention and the attributes
    /// (the function itself, or the function a declared alias stands for).
    pub(super) fn function(
        &mut self,
        symbol: Symbol<'a>,
        proto: Symbol<'a>,
        body: Option<&'a Body>,
    ) -> R {
        let m = self.m;
        let TypeKind::Function { ret, params, variadic } = m.types.kind(symbol.ty()) else {
            return self.malformed("a function whose type is not a function type");
        };
        let (cc, attrs, ret_attrs, personality) = match proto.kind() {
            SymbolKind::Function { cc, attrs, ret_attrs, personality, .. } => {
                (Some(cc), attrs, ret_attrs, personality)
            }
            _ => (None, FnAttrsId::EMPTY, ParamAttrs::NONE, None),
        };
        let define = body.is_some();
        self.s(if define { "\ndefine " } else { "declare " })?;
        self.linkage(symbol, define, false)?;
        if let Some(cc) = cc {
            self.call_conv(cc)?;
        }
        self.param_attrs(ret_attrs, false)?;
        self.ty(ret)?;
        self.s(" ")?;
        self.sym_name(symbol.id())?;
        self.s("(")?;
        for (index, &param) in params.iter().enumerate() {
            if index > 0 {
                self.s(", ")?;
            }
            self.ty(param)?;
            self.param_attrs(proto.param_attrs(index), true)?;
            if define {
                write!(self.out, " %a{index}")?;
            }
        }
        if variadic {
            self.s(if params.is_empty() { "..." } else { ", ..." })?;
        }
        self.s(")")?;
        self.s(match symbol.unnamed_addr() {
            UnnamedAddr::None => "",
            UnnamedAddr::Local => " local_unnamed_addr",
            UnnamedAddr::Global => " unnamed_addr",
        })?;
        if symbol.addr_space() != 0 {
            write!(self.out, " addrspace({})", symbol.addr_space())?;
        }
        if attrs.index() >= m.fn_attrs_count() {
            return self.malformed("a function whose attribute set the module lacks");
        }
        if !m.fn_attrs(attrs).is_empty() {
            write!(self.out, " #{}", attrs.raw())?;
            if let Some(used) = self.attrs_used.get_mut(attrs.index()) {
                *used = true;
            }
        }
        if let Some(section) = symbol.section() {
            self.s(" section ")?;
            write_quoted(self.out, section)?;
        }
        if define && let Some(comdat) = self.comdat_of(symbol)? {
            self.s(" comdat(")?;
            write_name(self.out, '$', m.comdat(comdat).name)?;
            self.s(")")?;
        }
        if let Some(align) = symbol.align() {
            write!(self.out, " align {align}")?;
        }
        let Some(body) = body else {
            return self.s("\n");
        };
        if let Some(personality) = personality {
            self.s(" personality ptr ")?;
            self.sym_ref(personality)?;
        }
        self.dbg_function(symbol)?;
        self.s(" {\n")?;
        self.body(body)?;
        self.s("}\n")
    }

    /// `cc `, unless it is the C convention.
    pub(super) fn call_conv(&mut self, cc: crate::attrs::CallConv) -> R {
        if cc == crate::attrs::CallConv::C {
            return Ok(());
        }
        match cc.keyword() {
            Some(keyword) => write!(self.out, "{keyword} ")?,
            None => write!(self.out, "cc {} ", cc.llvm_id())?,
        }
        Ok(())
    }

    /// The attributes of a parameter, an argument or a return value, in
    /// one fixed order. After a type (`after_type`) each is preceded by a
    /// space; before one, each is followed by a space.
    pub(super) fn param_attrs(&mut self, attrs: ParamAttrs, after_type: bool) -> R {
        const FLAGS: [(u16, &str); 7] = [
            (ParamAttrs::ZEROEXT, "zeroext"),
            (ParamAttrs::SIGNEXT, "signext"),
            (ParamAttrs::INREG, "inreg"),
            (ParamAttrs::NEST, "nest"),
            (ParamAttrs::SWIFT_SELF, "swiftself"),
            (ParamAttrs::SWIFT_ERROR, "swifterror"),
            (ParamAttrs::SWIFT_ASYNC, "swiftasync"),
        ];
        if attrs.is_none() {
            return Ok(());
        }
        let (before, after) = if after_type { (" ", "") } else { ("", " ") };
        for (flag, keyword) in FLAGS {
            if attrs.has(flag) {
                self.s(before)?;
                self.s(keyword)?;
                self.s(after)?;
            }
        }
        let keyword = match attrs.mem() {
            ParamMem::None => None,
            ParamMem::Sret => Some("sret"),
            ParamMem::ByVal => Some("byval"),
            ParamMem::ByRef => Some("byref"),
            ParamMem::InAlloca => Some("inalloca"),
            ParamMem::Preallocated => Some("preallocated"),
            ParamMem::ElementType => Some("elementtype"),
        };
        if let Some(keyword) = keyword {
            let Some(ty) = attrs.mem_type() else {
                return self.malformed(format!("`{keyword}` without its type"));
            };
            self.s(before)?;
            self.s(keyword)?;
            self.s("(")?;
            self.ty(ty)?;
            self.s(")")?;
            self.s(after)?;
        }
        if let Some(align) = attrs.align() {
            write!(self.out, "{before}align {align}{after}")?;
        }
        if let Some(align) = attrs.stack_align() {
            write!(self.out, "{before}alignstack({align}){after}")?;
        }
        Ok(())
    }

    // ---- what follows the symbols ----

    /// `@llvm.used` or `@llvm.compiler.used`: the retained symbols this
    /// text defines, in id order.
    fn retained(&mut self, name: &str, pick: fn(&Symbol<'a>) -> bool) -> R {
        let m = self.m;
        let listed = |writer: &Self, symbol: &Symbol<'a>| pick(symbol) && writer.defined(symbol.id());
        let count = m.symbols().filter(|symbol| listed(self, symbol)).count();
        if count == 0 {
            return Ok(());
        }
        write!(self.out, "@{name} = appending global [{count} x ptr] [")?;
        let mut first = true;
        for symbol in m.symbols() {
            if !listed(self, &symbol) {
                continue;
            }
            self.cur = Some(symbol.id());
            if symbol.is_anonymous() && self.part == Part::Whole {
                // LLVM's verifier wants the members named.
                return self.unsupported(format!("{name}:anonymous-symbol"));
            }
            self.s(if first { "ptr " } else { ", ptr " })?;
            first = false;
            self.sym_ref(symbol.id())?;
        }
        self.cur = None;
        self.s("], section \"llvm.metadata\"\n")
    }

    fn structors(&mut self, name: &str, list: &[Structor]) -> R {
        if list.is_empty() {
            return Ok(());
        }
        write!(self.out, "@{name} = appending global [{} x {{ i32, ptr, ptr }}] [", list.len())?;
        for (index, structor) in list.iter().enumerate() {
            if index > 0 {
                self.s(", ")?;
            }
            write!(self.out, "{{ i32, ptr, ptr }} {{ i32 {}, ptr ", structor.priority)?;
            self.sym_ref(structor.func)?;
            self.s(", ptr ")?;
            match structor.associated {
                Some(symbol) => self.sym_ref(symbol)?,
                None => self.s("null")?,
            }
            self.s(" }")?;
        }
        self.s("]\n")
    }

    /// A `declare` for each intrinsic the printed bodies call, unless the
    /// module has a symbol of that name.
    fn intrinsic_decls(&mut self) -> R {
        let m = self.m;
        let decls = std::mem::take(&mut self.decls);
        let mut name = String::new();
        for (key, &fn_ty) in &decls {
            let intrinsic = match Intrinsic::from_code(key.code) {
                Some(known) => known,
                None => Intrinsic::Other(key.name),
            };
            let overloads = &key.overloads[..intrinsic.overload_count().min(key.overloads.len())];
            name.clear();
            write_intrinsic_name(&mut name, &m.types, self.dialect, intrinsic, overloads)?;
            if m.find_symbol(name.as_bytes()).is_some() {
                continue;
            }
            let TypeKind::Function { ret, params, variadic } = m.types.kind(fn_ty) else {
                return self.malformed("an intrinsic call whose type is not a function type");
            };
            self.s("declare ")?;
            self.ty(ret)?;
            self.s(" ")?;
            write_name(self.out, '@', name.as_bytes())?;
            self.s("(")?;
            self.ty_list(params)?;
            if variadic {
                self.s(if params.is_empty() { "..." } else { ", ..." })?;
            }
            self.s(")\n")?;
        }
        Ok(())
    }

    /// `attributes #N = { ... }` for each attribute set a printed function
    /// names; `N` is the set's id.
    fn attribute_groups(&mut self) -> R {
        let m = self.m;
        let major = self.dialect.major();
        for index in 0..m.fn_attrs_count() {
            if !self.attrs_used.get(index).copied().unwrap_or(false) {
                continue;
            }
            let view = m.fn_attrs(FnAttrsId::new(index as u32));
            write!(self.out, "attributes #{index} = {{")?;
            for (name, flag) in FnAttrs::FLAG_NAMES {
                if !view.has(flag) {
                    continue;
                }
                if attr_since(name) > major {
                    return self.unsupported(format!("attr:{name}"));
                }
                self.s(" ")?;
                self.s(name)?;
            }
            self.s(match view.uwtable() {
                UwTable::None => "",
                UwTable::Sync => " uwtable(sync)",
                UwTable::Async => " uwtable",
            })?;
            if let Some(align) = view.stack_align() {
                write!(self.out, " alignstack={align}")?;
            }
            for (key, value) in view.strings() {
                self.s(" \"")?;
                self.s(key.name())?;
                if key == FnStr::NoBuiltin {
                    // The value is the rest of the attribute's name.
                    write_escaped(self.out, value)?;
                    self.s("\"")?;
                } else if value.is_empty() {
                    self.s("\"")?;
                } else {
                    self.s("\"=")?;
                    write_quoted(self.out, value)?;
                }
            }
            self.s(" }\n")?;
        }
        Ok(())
    }

    /// `!N = !{i32 behavior, !"name", i32 value}`, the value an `i64` if
    /// it does not fit.
    fn flag(&mut self, next: &mut u32, behavior: u32, name: &str, value: u64) -> R {
        let width = if u32::try_from(value).is_ok() { 32 } else { 64 };
        writeln!(self.out, "!{next} = !{{i32 {behavior}, !\"{name}\", i{width} {value}}}")?;
        *next += 1;
        Ok(())
    }

    /// The module flags, the linker options and the dependent libraries.
    fn metadata(&mut self) -> R {
        let m = self.m;
        let major = self.dialect.major();
        let props = m.props;
        let mut next = 0u32;
        if props.pic as u8 != 0 {
            self.flag(&mut next, 8, "PIC Level", u64::from(props.pic as u8))?;
        }
        if props.pie as u8 != 0 {
            self.flag(&mut next, 7, "PIE Level", u64::from(props.pie as u8))?;
        }
        if let Some(model) = props.code_model {
            self.flag(&mut next, 1, "Code Model", u64::from(model as u8))?;
        }
        if let Some(threshold) = props.large_data_threshold {
            // An `i64` whatever its value.
            writeln!(
                self.out,
                "!{next} = !{{i32 1, !\"Large Data Threshold\", i64 {threshold}}}"
            )?;
            next += 1;
        }
        if props.uwtable != UwTable::None {
            self.flag(&mut next, 7, "uwtable", u64::from(props.uwtable as u8))?;
        }
        if props.frame_pointer as u8 != 0 {
            if props.frame_pointer as u8 == 4 && major < 22 {
                return self.unsupported("module-flag:frame-pointer=4");
            }
            self.flag(&mut next, 7, "frame-pointer", u64::from(props.frame_pointer as u8))?;
        }
        if let Some(version) = props.dwarf_version {
            self.flag(&mut next, 7, "Dwarf Version", u64::from(version))?;
        }
        if let Some(version) = props.debug_info_version {
            self.flag(&mut next, 2, "Debug Info Version", u64::from(version))?;
        }
        for (bit, name, aarch64) in PROTECTION_FLAGS {
            if props.branch_protection.has(bit) {
                // From LLVM 20 on, 2 says that every function states its
                // own protection, which is what the attribute sets do; 1
                // would have LLVM add it to functions that say nothing.
                let value = if aarch64 && major >= 20 { 2 } else { 1 };
                self.flag(&mut next, 8, name, value)?;
            }
        }
        for other in m.other_flags() {
            let behavior = other.behavior as u8;
            write!(self.out, "!{next} = !{{i32 {behavior}, !")?;
            write_quoted(self.out, other.name)?;
            match other.value {
                FlagValue::Int(value) => {
                    let width = if u32::try_from(value).is_ok() { 32 } else { 64 };
                    write!(self.out, ", i{width} {value}")?;
                }
                FlagValue::Str(text) => {
                    self.s(", !")?;
                    write_quoted(self.out, text)?;
                }
                FlagValue::Other => {
                    let name = String::from_utf8_lossy(other.name);
                    return self.unsupported(format!("module-flag:{name}"));
                }
            }
            self.s("}\n")?;
            next += 1;
        }
        self.named_metadata("llvm.module.flags", 0, next)?;
        if self.part != Part::Rest {
            let first = next;
            self.string_lists(m.linker_options(), &mut next)?;
            self.named_metadata("llvm.linker.options", first, next)?;
            let first = next;
            self.string_lists(m.dependent_libraries(), &mut next)?;
            self.named_metadata("llvm.dependent-libraries", first, next)?;
        }
        Ok(())
    }

    /// One node of strings per list.
    fn string_lists(&mut self, lists: &StringLists, next: &mut u32) -> R {
        for list in lists.iter() {
            write!(self.out, "!{next} = !{{")?;
            for (index, text) in list.enumerate() {
                self.s(if index > 0 { ", !" } else { "!" })?;
                write_quoted(self.out, text)?;
            }
            self.s("}\n")?;
            *next += 1;
        }
        Ok(())
    }

    /// `!name = !{!first, ...}` for the nodes `first..end`, if there are any.
    fn named_metadata(&mut self, name: &str, first: u32, end: u32) -> R {
        if first == end {
            return Ok(());
        }
        write!(self.out, "!{name} = !{{")?;
        for node in first..end {
            write!(self.out, "{}!{node}", if node > first { ", " } else { "" })?;
        }
        self.s("}\n")
    }
}
