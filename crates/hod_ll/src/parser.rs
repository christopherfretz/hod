//! The module-level parser's state and its phases (`spec/ll.md` §4.3).

use std::borrow::Cow;
use std::collections::HashMap;

use hod_ir::{
    ConstId, FnAttrs, FnAttrsId, IrResult, Module, ParamAttrs, SymId, TargetSpec, TypeId,
};

use crate::consts::{BlockLabels, LabelKey};
use crate::error::{LlErrorKind, PResult, fail, ir_fail};
use crate::index::{Index, scan};
use crate::lexer::{Lexer, MAX_SOURCE_BYTES, Tok, Token, unescape};
use crate::debug::{DebugMap, DebugScratch};
use crate::meta::{MdRef, MetadataIndex, Upgrade};
use crate::names::NameMap;
use crate::skip::skip_group;
use crate::{AttrGroup, FunctionText, ModuleText, ParseOptions, ParsedModule, Span, limits};

/// Where the definition of a named type stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TypeSlot {
    Pending,
    /// Being parsed: a reference to it now is a type that contains
    /// itself.
    Busy,
    Done(TypeId),
}

pub(crate) struct Parser<'a, 'm> {
    pub src: &'a [u8],
    pub lx: Lexer<'a>,
    /// The current token: one token of lookahead.
    pub tok: Token,
    pub module: &'m mut Module,
    pub index: Index,
    /// Set when a function body is parsed (`spec/ll.md` §6): the tables
    /// the module level left behind answer for named types, numbered
    /// globals, metadata nodes and attribute groups, in place of the
    /// parser's own, which are empty then.
    pub text: Option<&'m ModuleText<'a>>,

    /// Named types: name to index in `index.types` and `type_slots`.
    pub type_names: NameMap,
    /// Numbered types: `(number, index)`, sorted.
    pub type_numbers: Vec<(u32, u32)>,
    pub type_slots: Vec<TypeSlot>,

    /// Scratch stacks: field and parameter types, aggregate elements,
    /// the words of a wide integer, the bytes of an unescaped string,
    /// parameter attributes.
    pub tys: Vec<TypeId>,
    pub elems: Vec<ConstId>,
    pub words: Vec<u64>,
    pub buf: Vec<u8>,
    pub pattrs: Vec<ParamAttrs>,
    /// How deeply types and constants are nested right now.
    pub depth: u32,

    /// Per symbol, by index: where it was first mentioned, and whether a
    /// top-level entity has described it.
    pub first_ref: Vec<u32>,
    pub described: Vec<bool>,
    /// `@42`: `(number, symbol)`, sorted.
    pub numbered_globals: Vec<(u32, SymId)>,
    /// Unsupported constructs met in the entity being parsed, recorded
    /// with its symbol once that is known.
    pub pending: Vec<Cow<'static, str>>,
    /// `!dbg` attachments of the entity being parsed.
    pub dbg: Vec<MdRef>,

    /// Attribute groups, sorted by id.
    pub groups: Vec<AttrGroup>,
    /// The kept attributes of the function or group being parsed, and
    /// the `align` of a group.
    pub fa: FnAttrs,
    pub fa_align: Option<u64>,
    /// The groups the function being parsed names: `(id, offset)`.
    pub group_refs: Vec<(u32, u32)>,
    /// Which of the special arrays have been met: a bit each.
    pub specials_seen: u8,
    pub upgrade: Option<Upgrade>,
    /// Attribute sets already put through the upgrade.
    pub upgraded: Vec<(FnAttrsId, FnAttrsId)>,

    pub md: MetadataIndex,
    pub functions: Vec<FunctionText>,
    /// What each numbered metadata node became (`spec/ll.md` §7.4), the
    /// scratch storage of the debug nodes, and the inside of
    /// `!llvm.dbg.cu`.
    pub dmap: DebugMap,
    pub dscratch: DebugScratch,
    pub dbg_cu: Option<Span>,
    /// For `blockaddress`: the definitions by name, filled at the first
    /// one; and, per definition named by one, each label of its body
    /// with the ordinal of its block, sorted.
    pub defined: Option<HashMap<LabelKey, usize>>,
    pub labels: HashMap<usize, BlockLabels>,
    /// In a body: `(symbol, index in text.functions())`, sorted, filled
    /// at the first `blockaddress`.
    pub by_sym: Vec<(u32, u32)>,
}

/// The scratch stacks of a [`Parser`] and its `blockaddress` tables: a
/// body parser keeps them from one function to the next.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    pub tys: Vec<TypeId>,
    pub elems: Vec<ConstId>,
    pub words: Vec<u64>,
    pub buf: Vec<u8>,
    pub pattrs: Vec<ParamAttrs>,
    pub pending: Vec<Cow<'static, str>>,
    pub defined: Option<HashMap<LabelKey, usize>>,
    pub labels: HashMap<usize, BlockLabels>,
    pub by_sym: Vec<(u32, u32)>,
}

/// Maps a refusal of `hod_ir` to a failure at `at`.
pub(crate) fn ir<T>(at: u32, result: IrResult<T>) -> PResult<T> {
    result.map_err(|error| ir_fail(at, error))
}

/// Parses `src` (`spec/ll.md` §4).
pub(crate) fn run<'a>(src: &'a [u8], options: &ParseOptions) -> PResult<ParsedModule<'a>> {
    if src.len() > MAX_SOURCE_BYTES {
        return fail(LlErrorKind::Limit, 0, "source is larger than MAX_SOURCE_BYTES");
    }
    let index = scan(src)?;
    let target = target_of(src, &index, options)?;
    let mut module = Module::new(target);
    let mut parser = Parser::new(src, &mut module, index, None, Scratch::default());
    for at in 0..parser.index.module_asm.len() {
        let text = parser.string(parser.index.module_asm[at]);
        parser.module.append_module_asm(&text);
    }
    parser.number_globals()?;
    parser.define_types()?;
    parser.flush(None);
    parser.define_comdats()?;
    parser.define_groups()?;
    parser.index_metadata()?;
    parser.named_metadata()?;
    parser.build_debug()?;
    parser.flush(None);
    parser.items()?;
    parser.finish_debug()?;
    let text = parser.finish()?;
    Ok(ParsedModule { module, text })
}

/// The target: the module's triple, or the caller's default; the
/// module's data layout is checked against it (`spec/ir.md` §2.1).
fn target_of(src: &[u8], index: &Index, options: &ParseOptions) -> PResult<TargetSpec> {
    let contents = |token: Token| {
        let mut bytes = Vec::new();
        unescape(&src[token.start as usize + 1..token.end as usize - 1], &mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let target = match (index.triple, &options.default_target) {
        (Some(token), _) => ir(token.start, TargetSpec::from_triple(&contents(token)))?,
        (None, Some(target)) => target.clone(),
        (None, None) => {
            return fail(
                LlErrorKind::Target,
                0,
                "the module has no 'target triple' and no default target was given",
            );
        }
    };
    if let Some(token) = index.datalayout {
        ir(token.start, target.check_data_layout(&contents(token)))?;
    }
    Ok(target)
}

impl<'a, 'm> Parser<'a, 'm> {
    /// A parser over `src` that adds to `module`. `text` is given for a
    /// function body, and `index` is empty then.
    pub(crate) fn new(
        src: &'a [u8],
        module: &'m mut Module,
        index: Index,
        text: Option<&'m ModuleText<'a>>,
        scratch: Scratch,
    ) -> Parser<'a, 'm> {
        Parser {
            src,
            lx: Lexer::new(src),
            tok: Token { kind: Tok::Eof, start: 0, end: 0 },
            module,
            index,
            text,
            type_names: NameMap::default(),
            type_numbers: Vec::new(),
            type_slots: Vec::new(),
            tys: scratch.tys,
            elems: scratch.elems,
            words: scratch.words,
            buf: scratch.buf,
            pattrs: scratch.pattrs,
            depth: 0,
            first_ref: Vec::new(),
            described: Vec::new(),
            numbered_globals: Vec::new(),
            pending: scratch.pending,
            dbg: Vec::new(),
            groups: Vec::new(),
            fa: FnAttrs::new(),
            fa_align: None,
            group_refs: Vec::new(),
            specials_seen: 0,
            upgrade: None,
            upgraded: Vec::new(),
            md: MetadataIndex::default(),
            functions: Vec::new(),
            dmap: DebugMap::default(),
            dscratch: DebugScratch::default(),
            dbg_cu: None,
            defined: scratch.defined,
            labels: scratch.labels,
            by_sym: scratch.by_sym,
        }
    }

    /// Gives the scratch stacks back, empty, for the next parser.
    pub(crate) fn into_scratch(mut self) -> Scratch {
        self.tys.clear();
        self.elems.clear();
        self.pattrs.clear();
        self.pending.clear();
        Scratch {
            tys: self.tys,
            elems: self.elems,
            words: self.words,
            buf: self.buf,
            pattrs: self.pattrs,
            pending: self.pending,
            defined: self.defined,
            labels: self.labels,
            by_sym: self.by_sym,
        }
    }

    /// Whether the metadata node `!id` is defined.
    pub(crate) fn has_md_node(&self, id: u32) -> bool {
        match self.text {
            Some(text) => text.metadata().node(id).is_some(),
            None => self.md.node(id).is_some(),
        }
    }

    // ---- tokens ----

    pub(crate) fn bump(&mut self) {
        self.tok = self.lx.next_token();
    }

    /// Makes the token at `pos` the current one.
    pub(crate) fn goto(&mut self, pos: u32) {
        self.lx.set_pos(pos as usize);
        self.bump();
    }

    pub(crate) fn tok_text(&self, token: Token) -> &'a [u8] {
        let src = self.src;
        &src[token.start as usize..token.end as usize]
    }

    /// The bytes of the current token.
    pub(crate) fn text(&self) -> &'a [u8] {
        self.tok_text(self.tok)
    }

    pub(crate) fn at_word(&self, word: &[u8]) -> bool {
        self.tok.kind == Tok::Word && self.text() == word
    }

    pub(crate) fn eat_word(&mut self, word: &[u8]) -> bool {
        let found = self.at_word(word);
        if found {
            self.bump();
        }
        found
    }

    pub(crate) fn eat(&mut self, kind: Tok) -> bool {
        let found = self.tok.kind == kind;
        if found {
            self.bump();
        }
        found
    }

    /// A failure at the current token: the lexer's, if that is no token.
    pub(crate) fn syntax<T>(&self, message: &str) -> PResult<T> {
        if self.tok.kind == Tok::Error {
            return fail(LlErrorKind::Lex, self.tok.start, self.lx.error());
        }
        fail(LlErrorKind::Syntax, self.tok.start, message)
    }

    pub(crate) fn expect(&mut self, kind: Tok, what: &str) -> PResult<Token> {
        let token = self.tok;
        if token.kind != kind {
            return self.syntax(what);
        }
        self.bump();
        Ok(token)
    }

    pub(crate) fn expect_word(&mut self, word: &[u8], what: &str) -> PResult<()> {
        if !self.eat_word(word) {
            return self.syntax(what);
        }
        Ok(())
    }

    /// A decimal number that fits 64 bits.
    pub(crate) fn expect_u64(&mut self, what: &str) -> PResult<u64> {
        let token = self.expect(Tok::Int, what)?;
        match crate::index::parse_u64(self.tok_text(token)) {
            Some(value) => Ok(value),
            None => fail(LlErrorKind::Syntax, token.start, what),
        }
    }

    /// A decimal number that fits 32 bits.
    pub(crate) fn expect_u32(&mut self, what: &str) -> PResult<u32> {
        let at = self.tok.start;
        match u32::try_from(self.expect_u64(what)?) {
            Ok(value) => Ok(value),
            Err(_) => fail(LlErrorKind::Syntax, at, what),
        }
    }

    /// Steps over the group the current token opens; the span of its
    /// inside. The token after the group becomes current.
    pub(crate) fn skip_brackets(&mut self) -> PResult<Span> {
        let open = self.tok;
        let from = open.end as usize;
        let end = match open.kind {
            Tok::LParen => skip_group::<b'(', b')'>(self.src, from),
            Tok::LBracket => skip_group::<b'[', b']'>(self.src, from),
            _ => skip_group::<b'{', b'}'>(self.src, from),
        };
        let Ok(end) = end else {
            return fail(LlErrorKind::Syntax, open.start, "this bracket is never closed");
        };
        self.goto(end as u32);
        Ok(Span { start: open.end, end: end as u32 - 1 })
    }

    /// The contents of the string token `token`.
    pub(crate) fn string(&self, token: Token) -> Cow<'a, [u8]> {
        let src = self.src;
        unquote(&src[token.start as usize + 1..token.end as usize - 1])
    }

    /// The name a `@name`, `%name` or `$name` token gives, quoted or not;
    /// `None` for a numbered one.
    pub(crate) fn name(&self, token: Token) -> Option<Cow<'a, [u8]>> {
        let src = self.src;
        let (start, end) = (token.start as usize, token.end as usize);
        match token.kind {
            Tok::Global | Tok::Local | Tok::Comdat => Some(Cow::Borrowed(&src[start + 1..end])),
            Tok::GlobalQuoted | Tok::LocalQuoted | Tok::ComdatQuoted => {
                Some(unquote(&src[start + 2..end - 1]))
            }
            _ => None,
        }
    }

    // ---- nesting ----

    pub(crate) fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > limits::MAX_NESTING {
            return fail(
                LlErrorKind::Limit,
                self.tok.start,
                "types or constants are nested deeper than MAX_NESTING",
            );
        }
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

    // ---- unsupported constructs ----

    /// Notes an unsupported construct of the entity being parsed.
    pub(crate) fn note(&mut self, construct: impl Into<Cow<'static, str>>) {
        let construct = construct.into();
        if !self.pending.contains(&construct) {
            self.pending.push(construct);
        }
    }

    /// Records the noted constructs against `symbol`.
    pub(crate) fn flush(&mut self, symbol: Option<SymId>) {
        for construct in self.pending.drain(..) {
            self.module.record_unsupported(&construct, symbol);
        }
    }

    // ---- the end ----

    fn finish(mut self) -> PResult<ModuleText<'a>> {
        for symbol in self.module.symbols() {
            if symbol.is_unresolved() {
                let at = self.first_ref.get(symbol.id().index()).copied().unwrap_or(0);
                let name = String::from_utf8_lossy(symbol.name()).into_owned();
                return fail(
                    LlErrorKind::Undefined,
                    at,
                    format!("use of undefined value '@{name}'"),
                );
            }
        }
        ir(self.src.len() as u32, self.module.verify_symbols())?;
        let slots = std::mem::take(&mut self.type_slots);
        let type_of = |slot: u32| match slots[slot as usize] {
            TypeSlot::Done(ty) => ty.raw(),
            _ => TypeId::VOID.raw(),
        };
        self.type_names.map_values(type_of);
        let numbered_types =
            self.type_numbers.iter().map(|&(number, slot)| (number, type_of(slot))).collect();
        Ok(ModuleText {
            source: self.src,
            functions: self.functions,
            named_types: self.type_names,
            numbered_types,
            numbered_globals: self.numbered_globals,
            groups: self.groups,
            metadata: self.md,
            debug_map: self.dmap,
        })
    }
}

/// What the contents `raw` of a string or quoted name stand for.
pub(crate) fn unquote(raw: &[u8]) -> Cow<'_, [u8]> {
    if raw.contains(&b'\\') {
        let mut bytes = Vec::with_capacity(raw.len());
        unescape(raw, &mut bytes);
        Cow::Owned(bytes)
    } else {
        Cow::Borrowed(raw)
    }
}
