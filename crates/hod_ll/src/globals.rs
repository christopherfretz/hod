//! Global values (`spec/ll.md` §5.7 to §5.9): variables, aliases,
//! IFUNCs, function headers, COMDATs and the special LLVM arrays.

use hod_ir::{
    AliasDesc, CallConv, ComdatId, ComdatKind, Const, ConstId, FunctionDesc, IFuncDesc, Linkage,
    Retained, SymId, TlsModel, UnnamedAddr, VariableDesc, Visibility,
};

use crate::attrs::call_conv_keyword;
use crate::error::{LlErrorKind, PResult, fail};
use crate::index::{Item, ItemKind, parse_u32};
use crate::lexer::{Tok, Token};
use crate::parser::{Parser, ir};
use crate::{FunctionText, Span};

/// What precedes the kind of every global value.
struct Prefix {
    linkage: Linkage,
    /// Whether a linkage keyword was written.
    explicit: bool,
    visibility: Visibility,
    dso_local: bool,
}

/// What may follow a variable's initializer or a function's attributes.
#[derive(Default)]
struct Placement<'a> {
    section: Option<std::borrow::Cow<'a, [u8]>>,
    comdat: Option<ComdatId>,
    align: Option<u64>,
}

/// The LLVM arrays that are not symbols.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Special {
    Ctors,
    Dtors,
    Used,
    CompilerUsed,
    /// `llvm.global.annotations` (WP-095): in section `llvm.metadata`,
    /// which `llc` never emits, it is dropped; anywhere else it is noted
    /// `special-global:llvm.global.annotations` (§5.9).
    Annotations,
}

impl Parser<'_, '_> {
    /// Creates every COMDAT.
    pub(crate) fn define_comdats(&mut self) -> PResult<()> {
        for at in 0..self.index.comdats.len() {
            let ent = self.index.comdats[at];
            let name = self.name(ent.name).unwrap_or_default();
            let kind = match self.tok_text(ent.kind) {
                b"any" => ComdatKind::Any,
                b"exactmatch" => ComdatKind::ExactMatch,
                b"largest" => ComdatKind::Largest,
                b"nodeduplicate" => ComdatKind::NoDeduplicate,
                b"samesize" => ComdatKind::SameSize,
                _ => {
                    return fail(LlErrorKind::Syntax, ent.kind.start, "unknown selection kind");
                }
            };
            if self.module.find_comdat(&name).is_some() {
                return fail(LlErrorKind::Redefined, ent.name.start, "redefinition of comdat");
            }
            let id = ir(ent.name.start, self.module.comdat_named(&name))?;
            self.module.set_comdat_kind(id, kind);
        }
        Ok(())
    }

    /// The symbol a `@name` or `@42` token refers to, created unresolved
    /// at its first mention.
    pub(crate) fn global_ref(&mut self, token: Token) -> PResult<SymId> {
        if let Some(text) = self.text {
            // A function body: every global is described, or missing.
            let found = match token.kind {
                Tok::Global => {
                    let src = self.src;
                    self.module.find_symbol(&src[token.start as usize + 1..token.end as usize])
                }
                Tok::GlobalQuoted => self.module.find_symbol(&self.name(token).unwrap_or_default()),
                Tok::GlobalId => text.numbered_global(self.global_number(token)?),
                _ => return fail(LlErrorKind::Syntax, token.start, "expected a global name"),
            };
            let Some(sym) = found else {
                let name = String::from_utf8_lossy(self.tok_text(token)).into_owned();
                let message = format!("use of undefined value '{name}'");
                return fail(LlErrorKind::Undefined, token.start, message);
            };
            return Ok(sym);
        }
        let sym = match token.kind {
            Tok::Global => {
                let src = self.src;
                let name = &src[token.start as usize + 1..token.end as usize];
                ir(token.start, self.module.symbol_named(name))?
            }
            Tok::GlobalQuoted => {
                let name = self.name(token).unwrap_or_default();
                ir(token.start, self.module.symbol_named(&name))?
            }
            Tok::GlobalId => {
                let number = self.global_number(token)?;
                match self.numbered_globals.binary_search_by_key(&number, |&(n, _)| n) {
                    Ok(found) => self.numbered_globals[found].1,
                    Err(_) => {
                        return fail(
                            LlErrorKind::Undefined,
                            token.start,
                            format!("use of undefined value '@{number}'"),
                        );
                    }
                }
            }
            _ => return fail(LlErrorKind::Syntax, token.start, "expected a global name"),
        };
        if sym.index() >= self.first_ref.len() {
            self.first_ref.resize(sym.index() + 1, token.start);
            self.described.resize(sym.index() + 1, false);
        }
        Ok(sym)
    }

    fn global_number(&self, token: Token) -> PResult<u32> {
        match parse_u32(&self.tok_text(token)[1..]) {
            Some(number) => Ok(number),
            None => fail(LlErrorKind::Syntax, token.start, "global number is too large"),
        }
    }

    /// The name token of the declaration `item`: the first global name
    /// outside brackets. The index has the names of assignments and
    /// definitions only.
    fn declared_name(&mut self, item: Item) -> PResult<Option<Token>> {
        self.goto(item.start);
        while self.tok.start < item.end {
            match self.tok.kind {
                Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => return Ok(Some(self.tok)),
                Tok::Eof => break,
                Tok::LParen | Tok::LBracket | Tok::LBrace => {
                    self.skip_brackets()?;
                }
                _ => self.bump(),
            }
        }
        Ok(None)
    }

    /// What the assignment `item` defines, by the word that says it:
    /// `global` or `constant`, `alias`, `ifunc`; `None` if there is none
    /// (an error, reported when the item is read).
    fn assigned_kind(&mut self, item: Item) -> PResult<Option<GlobalKind>> {
        self.goto(item.start);
        while self.tok.start < item.end {
            match self.tok.kind {
                Tok::Word => {
                    let kind = match self.text() {
                        b"global" | b"constant" => Some(GlobalKind::Variable),
                        b"alias" => Some(GlobalKind::Alias),
                        b"ifunc" => Some(GlobalKind::IFunc),
                        _ => None,
                    };
                    if kind.is_some() {
                        return Ok(kind);
                    }
                    self.bump();
                }
                Tok::LParen => {
                    self.skip_brackets()?;
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// Gives every global its symbol before anything refers to one, in
    /// the order of LLVM's module (`spec/ll.md` §5.7): the variables, then
    /// the functions, the aliases and the IFUNCs, each kind in the order
    /// of its definitions and declarations in the file, wherever a global
    /// is first mentioned.
    ///
    /// A numbered global with internal or private linkage is an anonymous
    /// symbol. One with any other linkage needs a name in the object: it
    /// takes `__unnamed_K`, `K` counting the numbered globals from 1 in
    /// file order, and is noted, because LLVM numbers such names in the
    /// order it emits them.
    pub(crate) fn number_globals(&mut self) -> PResult<()> {
        // File order: each global's kind and name; the numbers ascend.
        let mut globals: Vec<(GlobalKind, Token, u32, Item)> = Vec::new();
        let mut ordinal = 0u32;
        let mut last_number = None;
        for at in 0..self.index.items.len() {
            let item = self.index.items[at];
            let (kind, name) = match item.kind {
                ItemKind::Declare => match self.declared_name(item)? {
                    Some(name) => (GlobalKind::Function, name),
                    None => continue,
                },
                ItemKind::Define => (GlobalKind::Function, item.name),
                ItemKind::Assign => match self.assigned_kind(item)? {
                    Some(kind) => (kind, item.name),
                    None => continue,
                },
            };
            if kind == GlobalKind::Variable && is_special(self.tok_text(name)) {
                continue;
            }
            let mut unnamed = 0;
            if name.kind == Tok::GlobalId {
                ordinal += 1;
                unnamed = ordinal;
                let number = self.global_number(name)?;
                // As in LLVM, the numbers ascend.
                if let Some(last) = last_number
                    && number <= last
                {
                    let known = globals.iter().any(|&(_, other, _, _)| {
                        other.kind == Tok::GlobalId
                            && self.global_number(other).ok() == Some(number)
                    });
                    return if known {
                        let message = format!("redefinition of global '@{number}'");
                        fail(LlErrorKind::Redefined, name.start, message)
                    } else {
                        let message =
                            format!("global expected to be numbered '@{}' or greater", last + 1);
                        fail(LlErrorKind::Syntax, name.start, message)
                    };
                }
                last_number = Some(number);
            }
            globals.push((kind, name, unnamed, item));
        }
        // LLVM's order: kind by kind (stable: file order within a kind).
        globals.sort_by_key(|&(kind, _, _, _)| kind);
        let mut numbered = Vec::new();
        for (_, name, unnamed, item) in globals {
            let sym = match name.kind {
                Tok::GlobalId => {
                    self.goto(item.start);
                    while item.kind == ItemKind::Declare && self.tok.kind == Tok::Meta {
                        self.bump();
                        self.bump();
                    }
                    if self.at_word(b"internal") || self.at_word(b"private") {
                        ir(name.start, self.module.anonymous_symbol())?
                    } else {
                        let text = format!("__unnamed_{unnamed}");
                        let sym = ir(name.start, self.module.symbol_named(text.as_bytes()))?;
                        self.note("symbol:anonymous-external");
                        self.flush(Some(sym));
                        sym
                    }
                }
                Tok::GlobalQuoted => {
                    let text = self.name(name).unwrap_or_default();
                    ir(name.start, self.module.symbol_named(&text))?
                }
                _ => {
                    let text = &self.src[name.start as usize + 1..name.end as usize];
                    ir(name.start, self.module.symbol_named(text))?
                }
            };
            if name.kind == Tok::GlobalId {
                numbered.push((self.global_number(name)?, sym));
            }
            if sym.index() >= self.first_ref.len() {
                self.first_ref.resize(sym.index() + 1, name.start);
                self.described.resize(sym.index() + 1, false);
            }
        }
        numbered.sort_unstable_by_key(|&(number, _)| number);
        self.numbered_globals = numbered;
        Ok(())
    }
}

/// The kinds of global in the order of LLVM's module: its lists of
/// variables, functions, aliases and IFUNCs, as the bitcode writer and
/// its value numbering take them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum GlobalKind {
    Variable,
    Function,
    Alias,
    IFunc,
}

/// Whether `name` (with its `@`) is one of the special arrays, which are
/// not symbols (§5.9).
fn is_special(name: &[u8]) -> bool {
    matches!(
        name,
        b"@llvm.global_ctors"
            | b"@llvm.global_dtors"
            | b"@llvm.used"
            | b"@llvm.compiler.used"
            | b"@llvm.global.annotations"
    )
}

impl<'a> Parser<'a, '_> {
    /// The symbol the entity named by `token` describes; an error if an
    /// earlier entity described it.
    fn describe(&mut self, token: Token) -> PResult<SymId> {
        let sym = self.global_ref(token)?;
        if std::mem::replace(&mut self.described[sym.index()], true) {
            let name = String::from_utf8_lossy(self.tok_text(token)).into_owned();
            return fail(
                LlErrorKind::Redefined,
                token.start,
                format!("redefinition of global '{name}'"),
            );
        }
        Ok(sym)
    }

    /// Parses every global value, in file order.
    pub(crate) fn items(&mut self) -> PResult<()> {
        for at in 0..self.index.items.len() {
            let item = self.index.items[at];
            self.goto(item.start);
            self.dbg.clear();
            match item.kind {
                ItemKind::Assign => self.assignment(item)?,
                ItemKind::Declare | ItemKind::Define => self.function(item)?,
            }
            if self.tok.start != item.end {
                return self.syntax("expected a top-level entity");
            }
        }
        Ok(())
    }

    /// `[linkage] [dso_local] [visibility] [dll storage]`.
    fn prefix(&mut self) -> Prefix {
        let mut prefix = Prefix {
            linkage: Linkage::External,
            explicit: false,
            visibility: Visibility::Default,
            dso_local: false,
        };
        if self.tok.kind == Tok::Word {
            let linkage = match self.text() {
                b"private" => Some(Linkage::Private),
                b"internal" => Some(Linkage::Internal),
                b"weak" => Some(Linkage::WeakAny),
                b"weak_odr" => Some(Linkage::WeakOdr),
                b"linkonce" => Some(Linkage::LinkOnceAny),
                b"linkonce_odr" => Some(Linkage::LinkOnceOdr),
                b"available_externally" => Some(Linkage::AvailableExternally),
                b"appending" => Some(Linkage::Appending),
                b"common" => Some(Linkage::Common),
                b"extern_weak" => Some(Linkage::ExternWeak),
                b"external" => Some(Linkage::External),
                _ => None,
            };
            if let Some(linkage) = linkage {
                prefix.linkage = linkage;
                prefix.explicit = true;
                self.bump();
            }
        }
        if self.eat_word(b"dso_local") {
            prefix.dso_local = true;
        } else {
            self.eat_word(b"dso_preemptable");
        }
        if self.eat_word(b"hidden") {
            prefix.visibility = Visibility::Hidden;
        } else if self.eat_word(b"protected") {
            prefix.visibility = Visibility::Protected;
        } else {
            self.eat_word(b"default");
        }
        if self.eat_word(b"dllimport") || self.eat_word(b"dllexport") {
            self.note("dll-storage");
        }
        prefix
    }

    /// `[thread_local[(model)]]`.
    fn tls(&mut self) -> PResult<TlsModel> {
        if !self.eat_word(b"thread_local") {
            return Ok(TlsModel::None);
        }
        if !self.eat(Tok::LParen) {
            return Ok(TlsModel::GeneralDynamic);
        }
        let model = match self.text() {
            b"localdynamic" => TlsModel::LocalDynamic,
            b"initialexec" => TlsModel::InitialExec,
            b"localexec" => TlsModel::LocalExec,
            _ => return self.syntax("expected localdynamic, initialexec or localexec"),
        };
        self.bump();
        self.expect(Tok::RParen, "expected ')' after the thread-local model")?;
        Ok(model)
    }

    fn unnamed_addr(&mut self) -> UnnamedAddr {
        if self.eat_word(b"unnamed_addr") {
            UnnamedAddr::Global
        } else if self.eat_word(b"local_unnamed_addr") {
            UnnamedAddr::Local
        } else {
            UnnamedAddr::None
        }
    }

    /// `[addrspace(N)]`.
    fn addr_space(&mut self) -> PResult<u32> {
        if !self.eat_word(b"addrspace") {
            return Ok(0);
        }
        self.expect(Tok::LParen, "expected '(' in an address space")?;
        let space = self.expect_u32("expected an address space number")?;
        self.expect(Tok::RParen, "expected ')' in an address space")?;
        Ok(space)
    }

    /// `comdat` or `comdat($name)`, after the keyword; `own` is the name
    /// token of the global, whose name a bare `comdat` means.
    fn comdat(&mut self, own: Token) -> PResult<ComdatId> {
        let at = self.tok.start;
        let name = if self.eat(Tok::LParen) {
            let token = self.tok;
            if !matches!(token.kind, Tok::Comdat | Tok::ComdatQuoted) {
                return self.syntax("expected a comdat name");
            }
            self.bump();
            self.expect(Tok::RParen, "expected ')' after the comdat name")?;
            self.name(token)
        } else {
            self.name(own)
        };
        match name.and_then(|name| self.module.find_comdat(&name)) {
            Some(id) => Ok(id),
            None => fail(LlErrorKind::Undefined, at, "use of undefined comdat"),
        }
    }

    /// One of `section "s"`, `partition "p"`, `comdat…`, `align N` at
    /// the current token, if it is one.
    fn placement(&mut self, own: Token, out: &mut Placement<'a>) -> PResult<bool> {
        if self.tok.kind != Tok::Word {
            return Ok(false);
        }
        match self.text() {
            b"section" => {
                self.bump();
                let name = self.expect(Tok::Str, "expected a section name")?;
                out.section = Some(self.string(name));
            }
            b"partition" => {
                self.bump();
                self.expect(Tok::Str, "expected a partition name")?;
                self.note("partition");
            }
            b"comdat" => {
                self.bump();
                out.comdat = Some(self.comdat(own)?);
            }
            b"align" => {
                self.bump();
                out.align = Some(self.expect_u64("expected an alignment")?);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// What follows a variable's initializer: `, property` any number of
    /// times, then attributes.
    fn variable_properties(&mut self, name: Token) -> PResult<Placement<'a>> {
        let mut place = Placement::default();
        while self.eat(Tok::Comma) {
            if self.placement(name, &mut place)? {
                continue;
            }
            match (self.tok.kind, self.text()) {
                (Tok::Meta, _) => self.attachment()?,
                (Tok::Word, b"code_model") => {
                    self.bump();
                    let model = self.expect(Tok::Str, "expected a code model")?;
                    let known: [&[u8]; 5] = [b"tiny", b"small", b"kernel", b"medium", b"large"];
                    if !known.contains(&&*self.string(model)) {
                        return fail(LlErrorKind::Syntax, model.start, "invalid code model");
                    }
                    self.note("global-code-model");
                }
                (Tok::Word, b"sanitize_memtag") => {
                    self.bump();
                    self.note("global-sanitize-memtag");
                }
                // Dropped: read by instrumentation passes only.
                (
                    Tok::Word,
                    b"no_sanitize_address" | b"no_sanitize_hwaddress" | b"sanitize_address_dyninit",
                ) => self.bump(),
                _ => return self.syntax("unknown global variable property"),
            }
        }
        // Attributes of a variable: none is kept, and the strings clang
        // writes here (`"bss-section"`) move the variable.
        self.fa.clear();
        loop {
            if self.tok.kind == Tok::AttrGroup {
                let id = parse_u32(&self.text()[1..]).unwrap_or(u32::MAX);
                if let Ok(found) = self.groups.binary_search_by_key(&id, |group| group.id) {
                    for name in self.groups[found].rejected.clone() {
                        self.note(format!("attr:{name}"));
                    }
                    if self.groups[found].attrs != hod_ir::FnAttrsId::EMPTY {
                        self.note("global-attributes");
                    }
                }
                self.bump();
            } else if !self.fn_attr(false)? {
                break;
            }
        }
        if !self.fa.is_empty() {
            self.note("global-attributes");
        }
        Ok(place)
    }

    /// `@name = …`: a variable, an alias or an IFUNC.
    fn assignment(&mut self, item: Item) -> PResult<()> {
        let prefix = self.prefix();
        let tls = self.tls()?;
        let unnamed_addr = self.unnamed_addr();
        if self.at_word(b"alias") || self.at_word(b"ifunc") {
            return self.indirect(item, &prefix, tls, unnamed_addr);
        }
        let addr_space = self.addr_space()?;
        // Dropped: it does not change where the variable goes.
        self.eat_word(b"externally_initialized");
        let constant = if self.eat_word(b"constant") {
            true
        } else if self.eat_word(b"global") {
            false
        } else {
            return self.syntax("expected 'global' or 'constant'");
        };
        let ty_at = self.tok.start;
        let ty = self.parse_type()?;
        let declaration =
            prefix.explicit && matches!(prefix.linkage, Linkage::External | Linkage::ExternWeak);
        let init = if declaration { None } else { Some(self.parse_const(ty)?) };

        let place = self.variable_properties(item.name)?;

        let special = match self.tok_text(item.name) {
            b"@llvm.global_ctors" => Some(Special::Ctors),
            b"@llvm.global_dtors" => Some(Special::Dtors),
            b"@llvm.used" => Some(Special::Used),
            b"@llvm.compiler.used" => Some(Special::CompilerUsed),
            b"@llvm.global.annotations" => Some(Special::Annotations),
            _ => None,
        };
        if let Some(special) = special {
            // Not a symbol (`spec/ir.md` §2.2).
            let bit = 1 << special as u8;
            if self.specials_seen & bit != 0 {
                return fail(LlErrorKind::Redefined, item.name.start, "redefinition of global");
            }
            self.specials_seen |= bit;
            if special == Special::Annotations {
                // `llc` emits nothing of a variable in `llvm.metadata`.
                if place.section.as_deref() != Some(&b"llvm.metadata"[..]) {
                    self.note("special-global:llvm.global.annotations");
                }
            } else if let Some(init) = init {
                self.special(special, init, item.name.start)?;
            }
            self.flush(None);
            return Ok(());
        }
        let sym = self.describe(item.name)?;
        if self.tok_text(item.name).starts_with(b"@llvm.") {
            let name = String::from_utf8_lossy(&self.tok_text(item.name)[1..]).into_owned();
            self.note(format!("special-global:{name}"));
        }
        let desc = VariableDesc {
            linkage: prefix.linkage,
            visibility: prefix.visibility,
            dso_local: prefix.dso_local,
            unnamed_addr,
            section: place.section.as_deref(),
            comdat: place.comdat,
            align: place.align,
            ty,
            constant,
            tls,
            addr_space,
            init,
        };
        ir(ty_at, self.module.set_variable(sym, &desc))?;
        self.variable_dbg(sym)?;
        self.finish_symbol(sym);
        Ok(())
    }

    /// Records what was noted against `sym`.
    fn finish_symbol(&mut self, sym: SymId) {
        self.flush(Some(sym));
    }

    /// `alias type, ptr target` or `ifunc type, ptr resolver`.
    fn indirect(
        &mut self,
        item: Item,
        prefix: &Prefix,
        tls: TlsModel,
        unnamed_addr: UnnamedAddr,
    ) -> PResult<()> {
        let is_alias = self.at_word(b"alias");
        self.bump();
        let ty_at = self.tok.start;
        let ty = self.parse_type()?;
        self.expect(Tok::Comma, "expected ',' after the type")?;
        let target_at = self.tok.start;
        // LLVM writes the aliasee without its type when it is one of
        // these four expressions.
        let bare = [&b"getelementptr"[..], b"bitcast", b"addrspacecast", b"inttoptr"];
        let target = if self.tok.kind == Tok::Word && bare.contains(&self.text()) {
            self.parse_const(hod_ir::TypeId::PTR)?
        } else {
            self.parse_typed_const()?
        };
        while self.eat(Tok::Comma) {
            if self.at_word(b"partition") {
                self.bump();
                self.expect(Tok::Str, "expected a partition name")?;
                self.note("partition");
            } else if self.tok.kind == Tok::Meta {
                self.attachment()?;
            } else {
                return self.syntax("unknown alias or ifunc property");
            }
        }
        let sym = self.describe(item.name)?;
        if is_alias {
            let desc = AliasDesc {
                linkage: prefix.linkage,
                visibility: prefix.visibility,
                dso_local: prefix.dso_local,
                unnamed_addr,
                tls,
                ty,
                target,
            };
            ir(ty_at, self.module.set_alias(sym, &desc))?;
        } else {
            let Const::Addr { sym: resolver, addend: 0 } = self.module.consts.get(target) else {
                return fail(LlErrorKind::Type, target_at, "an ifunc's resolver is a function");
            };
            let desc = IFuncDesc {
                linkage: prefix.linkage,
                visibility: prefix.visibility,
                dso_local: prefix.dso_local,
                unnamed_addr,
                ty,
                resolver,
            };
            ir(ty_at, self.module.set_ifunc(sym, &desc))?;
        }
        self.finish_symbol(sym);
        Ok(())
    }

    /// The elements of the array constant `init`: none for a zero or
    /// undefined array.
    fn array_elements(&self, init: ConstId, at: u32) -> PResult<Vec<ConstId>> {
        match self.module.consts.get(init) {
            Const::Aggregate { elems, .. } => Ok(elems.to_vec()),
            Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Ok(Vec::new()),
            _ => fail(LlErrorKind::Type, at, "this special array is not an array of its members"),
        }
    }

    /// Delivers one of the special arrays.
    fn special(&mut self, which: Special, init: ConstId, at: u32) -> PResult<()> {
        let symbol_of = |parser: &Self, id: ConstId| match parser.module.consts.get(id) {
            Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
            Const::Null(_) => Ok(None),
            _ => fail(LlErrorKind::Type, at, "a member of this special array is not a symbol"),
        };
        for elem in self.array_elements(init, at)? {
            match which {
                Special::Used | Special::CompilerUsed => {
                    let Some(sym) = symbol_of(self, elem)? else {
                        return fail(LlErrorKind::Type, at, "a used symbol cannot be null");
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
                        // An entry that is all zeros has a null function.
                        Const::Zero(_) => continue,
                        _ => {
                            let message = "an entry is { i32 priority, ptr function, ptr data }";
                            return fail(LlErrorKind::Type, at, message);
                        }
                    };
                    let Some(priority) = self.module.consts.int_value(fields[0]) else {
                        return fail(LlErrorKind::Type, at, "a priority is an integer constant");
                    };
                    // An entry whose function is null is skipped.
                    let Some(func) = symbol_of(self, fields[1])? else {
                        continue;
                    };
                    let associated = match fields.get(2) {
                        Some(&data) => symbol_of(self, data)?,
                        None => None,
                    };
                    let priority = priority as u32;
                    if which == Special::Ctors {
                        self.module.add_ctor(priority, func, associated);
                    } else {
                        self.module.add_dtor(priority, func, associated);
                    }
                }
            }
        }
        Ok(())
    }

    /// `type @name(parameters)`: the function's type and its name token.
    /// The parameters' attributes are pushed on `pattrs`.
    fn signature(&mut self) -> PResult<(hod_ir::TypeId, Token)> {
        let ty_at = self.tok.start;
        let ret = self.parse_type()?;
        let name = self.tok;
        if !matches!(name.kind, Tok::Global | Tok::GlobalQuoted | Tok::GlobalId) {
            return self.syntax("expected a function name");
        }
        self.bump();

        // Parameters: `type attributes [%name]`, and `...` last.
        self.expect(Tok::LParen, "expected '(' in a function's parameter list")?;
        let tys_base = self.tys.len();
        let mut variadic = false;
        if self.tok.kind != Tok::RParen {
            loop {
                if self.eat(Tok::DotDotDot) {
                    variadic = true;
                    break;
                }
                let ty = self.parse_type()?;
                let attrs = self.param_attrs()?;
                if matches!(self.tok.kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
                    self.bump();
                }
                self.tys.push(ty);
                self.pattrs.push(attrs);
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "expected ')' at the end of the parameter list")?;
        let ty = self.module.types.function(ret, &self.tys[tys_base..], variadic);
        self.tys.truncate(tys_base);
        Ok((ir(ty_at, ty)?, name))
    }

    /// `gc`, `prefix`, `prologue` and `personality`, in any order: the
    /// personality function.
    fn function_data(&mut self) -> PResult<Option<SymId>> {
        let mut personality = None;
        loop {
            if self.eat_word(b"gc") {
                self.expect(Tok::Str, "expected a collector name")?;
                self.note("gc");
            } else if self.at_word(b"prefix") || self.at_word(b"prologue") {
                self.note(if self.at_word(b"prefix") { "prefix-data" } else { "prologue-data" });
                self.bump();
                self.parse_typed_const()?;
            } else if self.eat_word(b"personality") {
                let value = self.parse_typed_const()?;
                match self.module.consts.get(value) {
                    Const::Addr { sym, addend: 0 } => personality = Some(sym),
                    _ => self.note("personality-not-a-symbol"),
                }
            } else {
                return Ok(personality);
            }
        }
    }

    /// `declare` or `define`: the header, up to the body.
    fn function(&mut self, item: Item) -> PResult<()> {
        let defined = item.kind == ItemKind::Define;
        if !defined {
            while self.tok.kind == Tok::Meta {
                self.attachment()?;
            }
        }
        let prefix = self.prefix();
        let cc_at = self.tok.start;
        let mut cc = 0;
        if self.eat_word(b"cc") {
            cc = self.expect_u32("expected a calling convention number")?;
        } else if self.tok.kind == Tok::Word
            && let Some(number) = call_conv_keyword(self.text())
        {
            cc = number;
            self.bump();
        }
        let cc = ir(cc_at, CallConv::from_llvm(cc))?;
        let ret_attrs = self.param_attrs()?;
        let attrs_base = self.pattrs.len();
        let (ty, name) = self.signature()?;

        let unnamed_addr = self.unnamed_addr();
        let addr_space = self.addr_space()?;
        self.fa.clear();
        self.fa_align = None;
        let mut groups = std::mem::take(&mut self.group_refs);
        groups.clear();
        loop {
            if self.tok.kind == Tok::AttrGroup {
                let id = parse_u32(&self.text()[1..]).unwrap_or(u32::MAX);
                groups.push((id, self.tok.start));
                self.bump();
            } else if !self.fn_attr(false)? {
                break;
            }
        }
        let attrs = self.function_attrs(&groups, defined);
        self.group_refs = groups;
        let (attrs, group_align) = attrs?;

        let mut place = Placement { align: self.fa_align, ..Placement::default() };
        while self.placement(name, &mut place)? {}
        let personality = self.function_data()?;
        if defined {
            while self.tok.kind == Tok::Meta {
                self.attachment()?;
            }
        }

        let sym = self.describe(name)?;
        let mut used = self.pattrs.len();
        while used > attrs_base && self.pattrs[used - 1].is_none() {
            used -= 1;
        }
        let subprogram = self.function_dbg()?;
        let desc = FunctionDesc {
            linkage: prefix.linkage,
            visibility: prefix.visibility,
            dso_local: prefix.dso_local,
            unnamed_addr,
            section: place.section.as_deref(),
            comdat: place.comdat,
            align: group_align.or(place.align),
            ty,
            cc,
            ret_attrs,
            param_attrs: &self.pattrs[attrs_base..used],
            attrs,
            personality,
            subprogram,
            addr_space,
            has_body: defined,
        };
        let described = self.module.set_function(sym, &desc);
        self.pattrs.truncate(attrs_base);
        ir(name.start, described)?;
        if defined {
            self.functions.push(FunctionText {
                sym,
                header: Span { start: item.start, end: item.end },
                params: item.params,
                body: item.body,
                dbg: self.dbg.first().copied(),
            });
        }
        self.finish_symbol(sym);
        Ok(())
    }
}
