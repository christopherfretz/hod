//! Function bodies (`spec/ll.md` §11): the text between the braces of a
//! definition, read into a [`hod_ir::Body`].
//!
//! This file has the session a caller holds ([`BodyParser`]), the tables
//! of one function (local names, numbers, blocks) and the walk over its
//! blocks and statements. The instructions themselves are in `inst.rs`,
//! calls in `call.rs`.

use std::borrow::Cow;
use std::collections::HashMap;

use hod_ir::{
    BlockId, Body, DiId, ExprId, FunctionBuilder, Intrinsic, IrErrorKind, Layering, LocId, Module,
    NoCode, NoLayer, ParamAttrs, TypeId, TypeKind, Value, ValueKind, kit,
};

use crate::debug::{Handle, INVALID};
use crate::error::{LlErrorKind, PResult, fail, ir_fail};
use crate::index::{Index, parse_u32};
use crate::kw::Kw;
use crate::lexer::{Lexer, Tok, Token, unescape};
use crate::meta::{MdKind, MdRef};
use crate::names::hash;
use crate::parser::{Parser, Scratch, ir};
use crate::skip::skip_group;
use crate::{FunctionText, LlError, ModuleText, Span};

/// Where a local name stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Local {
    /// Not met yet.
    Undef,
    /// A value and its type.
    Value(Value, TypeId),
    /// A value used before its definition: the placeholder, and the type
    /// it was used with.
    Forward(Value, TypeId),
    /// A block, by its ordinal in the text.
    Block(u32),
}

/// The named locals of one function: open addressing over an arena of
/// the names, emptied between functions without touching the slots.
#[derive(Debug, Default)]
pub(crate) struct LocalNames {
    bytes: Vec<u8>,
    /// `(start in bytes, length, what the name is)`.
    entries: Vec<(u32, u32, Local)>,
    /// `(generation, entry index plus one)`; a slot of another generation
    /// is free.
    slots: Vec<(u32, u32)>,
    generation: u32,
}

impl LocalNames {
    fn clear(&mut self) {
        self.bytes.clear();
        self.entries.clear();
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.slots.fill((0, 0));
            self.generation = 1;
        }
    }

    fn key(&self, entry: usize) -> &[u8] {
        let (start, len, _) = self.entries[entry];
        &self.bytes[start as usize..(start + len) as usize]
    }

    /// The slot `key` is in or would go in.
    fn slot(&self, key: &[u8]) -> usize {
        let mask = self.slots.len() - 1;
        let mut at = hash(key) as usize & mask;
        loop {
            let (generation, entry) = self.slots[at];
            if generation != self.generation || entry == 0 || self.key(entry as usize - 1) == key {
                return at;
            }
            at = (at + 1) & mask;
        }
    }

    fn get(&self, key: &[u8]) -> Local {
        if self.slots.is_empty() {
            return Local::Undef;
        }
        match self.slots[self.slot(key)] {
            (generation, entry) if generation == self.generation && entry != 0 => {
                self.entries[entry as usize - 1].2
            }
            _ => Local::Undef,
        }
    }

    fn set(&mut self, key: &[u8], local: Local) {
        if (self.entries.len() + 1) * 2 > self.slots.len() {
            let size = (self.slots.len() * 2).max(16);
            self.slots.clear();
            self.slots.resize(size, (0, 0));
            for entry in 0..self.entries.len() {
                let at = self.slot(self.key(entry));
                self.slots[at] = (self.generation, entry as u32 + 1);
            }
        }
        let at = self.slot(key);
        let (generation, entry) = self.slots[at];
        if generation == self.generation && entry != 0 {
            self.entries[entry as usize - 1].2 = local;
            return;
        }
        let start = self.bytes.len() as u32;
        self.bytes.extend_from_slice(key);
        self.entries.push((start, key.len() as u32, local));
        self.slots[at] = (self.generation, self.entries.len() as u32);
    }
}

/// What is known about a symbol as a callee (`spec/ir.md` §5.5), found
/// once per symbol.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CalleeInfo {
    Unknown,
    /// Not an intrinsic.
    Plain,
    /// An intrinsic that generates no code.
    NoCode(NoCode),
    /// A member of hod's list with its overload types, for calls made
    /// with the declaration's type.
    Known(Intrinsic<'static>, [TypeId; 3], u8),
    /// Any other `llvm.*` name.
    Other,
}

/// A debug record, or a call of an `llvm.dbg.*` intrinsic read and
/// waiting for the call's `!dbg`, which is the record's location
/// (`spec/ll.md` §11.9); its values are in `FnState::dbg_vals`.
pub(crate) use hod_ir::kit::DebugRecord as OldRecord;
/// The four debug records.
pub(crate) use hod_ir::kit::DebugRecordKind as RecordKind;

/// How far a number may lie beyond the numbers met so far and still get
/// a place in the dense table.
const DENSE_GAP: usize = 4096;

/// Everything a body parser keeps from one function to the next, so that
/// a function costs no allocation once the tables have grown.
#[derive(Debug, Default)]
pub(crate) struct FnState {
    pub storage: Option<Body>,
    pub nums: Vec<Local>,
    pub sparse: HashMap<u32, Local>,
    pub names: LocalNames,
    /// Each placeholder with the offset of the name's first use.
    pub forwards: Vec<(Value, u32)>,
    /// The numbers of the blocks without a label and their ordinals, as
    /// the numbering pass found them.
    pub unlabelled: Vec<(u32, u32)>,
    pub need_numbering: bool,
    pub numbered: bool,
    /// Whether the function is variadic: a `musttail` call in it ends its
    /// arguments with `...` (§11.6).
    pub variadic: bool,

    pub vals: Vec<Value>,
    pub arg_tys: Vec<TypeId>,
    pub arg_attrs: Vec<ParamAttrs>,
    /// Where each argument of a call starts.
    pub arg_at: Vec<u32>,
    pub bundle_vals: Vec<Value>,
    /// Per bundle: the tag (in `bundle_tags`) and the inputs (in
    /// `bundle_vals`).
    pub bundles: Vec<(u32, u32, u32, u32)>,
    pub bundle_tags: String,
    pub incoming: Vec<(Value, BlockId)>,
    pub targets: Vec<BlockId>,
    pub cases: Vec<(hod_ir::ConstId, BlockId)>,
    pub case_ids: Vec<u32>,
    pub indices: Vec<(Value, u64)>,
    pub mask: Vec<u32>,
    pub path: Vec<u32>,
    pub clauses: Vec<hod_ir::Clause>,
    pub callees: Vec<CalleeInfo>,
    /// The values of the debug record being read, and a call of a debug
    /// intrinsic waiting for its `!dbg`.
    pub dbg_vals: Vec<Value>,
    pub old_record: Option<OldRecord>,
}

/// Parses the bodies of one module's functions (`spec/ll.md` §11).
///
/// A parser belongs to one [`ModuleText`] and is used with the module
/// that text was parsed into, or with a clone of it. It keeps its tables
/// from one function to the next: after the largest function, parsing a
/// body allocates nothing.
///
/// ```
/// use hod_ll::{BodyParser, ParseOptions, parse_module};
/// let source = b"target triple = \"aarch64-unknown-linux-gnu\"\n\
///     define i32 @twice(i32 %x) {\n  %y = add i32 %x, %x\n  ret i32 %y\n}\n";
/// let mut parsed = parse_module(source, &ParseOptions::default())?;
/// let mut bodies = BodyParser::new(&parsed.text);
/// let function = parsed.text.functions()[0];
/// let body = bodies.parse(&mut parsed.module, &function)?.expect("supported");
/// body.verify(&parsed.module).expect("a valid body");
/// assert_eq!(body.inst_count(), 2);
/// # Ok::<(), hod_ll::LlError>(())
/// ```
#[derive(Debug)]
pub struct BodyParser<'t, 'a> {
    text: &'t ModuleText<'a>,
    scratch: Scratch,
    state: FnState,
    unsupported: Vec<Cow<'static, str>>,
}

impl<'t, 'a> BodyParser<'t, 'a> {
    /// A parser for the functions of `text`.
    pub fn new(text: &'t ModuleText<'a>) -> BodyParser<'t, 'a> {
        BodyParser {
            text,
            scratch: Scratch::default(),
            state: FnState::default(),
            unsupported: Vec::new(),
        }
    }

    /// Parses the body of `function`, one of `text.functions()`.
    ///
    /// `module` is the module the text was parsed into, or a clone of it
    /// (`spec/ll.md` §11.1): the types, constants and inline assembly the
    /// body mentions are added to it, and the body is to be read against
    /// that same module.
    ///
    /// `Ok(None)` says that the body has a construct `hod_ir` cannot
    /// represent: each is named in [`BodyParser::unsupported`] and
    /// recorded in `module` against the function's symbol.
    ///
    /// # Errors
    /// [`LlError`] with the position of the first thing that is not LLVM
    /// IR, or that `hod_ir` refuses.
    pub fn parse<L: Layering>(
        &mut self,
        module: &mut Module<L>,
        function: &FunctionText,
    ) -> Result<Option<Body>, LlError> {
        // The body is built in the parser's own storage, whose tables
        // have grown to the largest function so far; the caller gets a
        // copy of exactly the size of its tables: one allocation per
        // table instead of one per doubling, and no spare capacity in the
        // bodies a consumer keeps (WP-096). A large body is the storage
        // itself, handed over: a copy would double the memory it takes
        // while the consumer holds it.
        if !self.read(module, function)? {
            return Ok(None);
        }
        if self.built().table_bytes() >= HAND_OVER_BYTES {
            return Ok(self.state.storage.take());
        }
        Ok(Some(self.built().clone()))
    }

    /// Parses the body of `function` into the parser's storage; whether
    /// a body was built there ([`BodyParser::built`]).
    fn read<L: Layering>(
        &mut self,
        module: &mut Module<L>,
        function: &FunctionText,
    ) -> Result<bool, LlError> {
        let source = self.text.source();
        self.unsupported.clear();
        if function.sym.index() >= module.symbol_count() {
            let failure = fail::<()>(
                LlErrorKind::Undefined,
                function.header.start,
                "this function is not a symbol of the module given",
            );
            return Err(failure.expect_err("a failure").locate(source));
        }
        let scratch = std::mem::take(&mut self.scratch);
        let mut parser = Parser::new(source, module, Index::default(), Some(self.text), scratch);
        let result = body(&mut parser, &mut self.state, function).map(|built| match built {
            Some(built) => {
                self.state.storage = Some(built);
                true
            }
            None => false,
        });
        if result.is_ok() {
            self.unsupported.append(&mut parser.pending);
        }
        self.scratch = parser.into_scratch();
        for construct in &self.unsupported {
            module.record_unsupported(construct, Some(function.sym));
        }
        result.map_err(|failure| failure.locate(source))
    }

    /// The body the last [`BodyParser::read`] that returned true built.
    fn built(&self) -> &Body {
        self.state.storage.as_ref().expect("a body was built")
    }

    /// The constructs that made the last body parsed unsupported, each
    /// once; empty for a body that was returned.
    pub fn unsupported(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.unsupported.iter().map(|construct| &**construct)
    }

    /// Gives the parser a body its consumer is done with. A parser builds
    /// every body in storage of its own, which grows to the largest
    /// function, and returns copies of exactly their size; `body` becomes
    /// that storage only while the parser has none (`spec/ll.md` §11.1).
    pub fn recycle(&mut self, body: Body) {
        if self.state.storage.is_none() {
            self.state.storage = Some(body);
        }
    }
}

/// One function's body as [`parse_bodies`] hands it over.
#[derive(Debug)]
pub struct ParsedBody<'p> {
    /// The function.
    pub function: &'p FunctionText,
    /// The body; `None` for a function with an unsupported construct.
    pub body: Option<&'p Body>,
    /// The unsupported constructs of the body, each once.
    pub unsupported: &'p [Cow<'static, str>],
}

/// The size of a body's tables from which [`BodyParser::parse`] hands
/// over the storage it was built in rather than a copy (`spec/ll.md`
/// §11.1).
pub const HAND_OVER_BYTES: usize = 256 << 10;

/// Parses the body of every function of `text`, in file order, and hands
/// each to `each` with the module as it then is. One body is alive at a
/// time, built in the parser's storage, which is reused for the next, so
/// peak memory follows the largest function (`spec/ir.md` §5.1).
///
/// # Errors
/// The first [`LlError`] of a body, converted to `E`, or the first error
/// `each` returns.
pub fn parse_bodies<E: From<LlError>>(
    module: &mut Module,
    text: &ModuleText<'_>,
    mut each: impl FnMut(&Module, ParsedBody<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let mut parser = BodyParser::new(text);
    for function in text.functions() {
        let body = parser.read(module, function)?.then(|| parser.built());
        each(module, ParsedBody { function, body, unsupported: &parser.unsupported })?;
    }
    Ok(())
}

/// [`parse_bodies`] on up to `threads` threads, the calling one included
/// (`spec/ll.md` §11.1): the module is `module(host)`, and each body goes
/// to `each` in file order, translated to the module's ids, with the
/// module as reading the bodies before it and that one sequentially
/// leaves it. A body is `Ok` or the constructs that made it unsupported
/// (recorded in the module as [`BodyParser::parse`] does). `each`
/// returns whether to go on.
///
/// With one thread this is [`BodyParser::parse`] of every function in
/// order. With more, the module's tables, every id and every body are
/// the same as with one (`spec/ir.md` §5.1).
///
/// # Errors
/// The [`LlError`] of the first body in file order that has one: the
/// error a sequential reading returns. `Ok`: how the reading went.
pub fn parse_bodies_on<H>(
    host: &mut H,
    module: fn(&mut H) -> &mut Module,
    text: &ModuleText<'_>,
    threads: usize,
    mut each: impl FnMut(&mut H, &FunctionText, Result<Body, &[Box<str>]>) -> bool,
) -> Result<hod_ir::ReadStats, LlError> {
    let functions = text.functions();
    let new_reader = || BodyParser::new(text);
    let mut deliver = |host: &mut H, index: usize, read: hod_ir::BodyRead<'_>| {
        let read = match read {
            hod_ir::BodyRead::Body(body) => Ok(body),
            hod_ir::BodyRead::Unsupported(constructs) => Err(constructs),
        };
        each(host, &functions[index], read)
    };
    hod_ir::read_bodies(host, module, functions.len(), threads, &new_reader, &mut deliver)
}

impl hod_ir::BodySource for BodyParser<'_, '_> {
    type Error = LlError;

    fn read_body<L: Layering>(
        &mut self,
        module: &mut Module<L>,
        index: usize,
    ) -> Result<Option<Body>, LlError> {
        let text = self.text;
        self.parse(module, &text.functions()[index])
    }

    fn unsupported(&self) -> Vec<Box<str>> {
        self.unsupported.iter().map(|construct| (**construct).into()).collect()
    }

    /// The body's bytes of text.
    fn weight(&self, index: usize) -> usize {
        let body = &self.text.functions()[index].body;
        (body.end - body.start) as usize
    }
}

/// Parses one body; with a second pass before it if the text refers to a
/// block that has no label before reaching it (`spec/ll.md` §11.3).
fn body<L: Layering>(
    p: &mut Parser<'_, '_, L>,
    state: &mut FnState,
    function: &FunctionText,
) -> PResult<Option<Body>> {
    state.unlabelled.clear();
    state.numbered = false;
    state.need_numbering = false;
    let first = build(p, state, function, false);
    if !state.need_numbering {
        return first;
    }
    state.need_numbering = false;
    build(p, state, function, true)?;
    state.numbered = true;
    build(p, state, function, false)
}

/// One pass over the body. With `numbering`, nothing is built to be
/// kept: the pass finds the numbers of the blocks without labels.
fn build<L: Layering>(
    p: &mut Parser<'_, '_, L>,
    state: &mut FnState,
    function: &FunctionText,
    numbering: bool,
) -> PResult<Option<Body>> {
    let fn_ty = p.module.symbol(function.sym).ty();
    let TypeKind::Function { ret, variadic, .. } = p.module.types.kind(fn_ty) else {
        return fail(LlErrorKind::Type, function.header.start, "this symbol is not a function");
    };
    let at = function.body.start;
    let storage = state.storage.take().unwrap_or_else(Body::empty);
    let builder = FunctionBuilder::reusing(p.module, function.sym, fn_ty, storage);
    let builder = ir(at, builder)?;
    state.nums.clear();
    state.sparse.clear();
    state.names.clear();
    state.forwards.clear();
    state.old_record = None;
    p.pending.clear();
    p.depth = 0;
    state.variadic = variadic;
    let mut fx = Fx {
        p,
        b: builder,
        s: state,
        end: function.body.end,
        ret_ty: ret,
        next: 0,
        blocks: 0,
        open_forwards: 0,
        numbering,
        scanned: false,
        stmt: function.body.start,
        extra_comma: false,
    };
    let walked = fx.function(function);
    let Fx { p, b, s, .. } = fx;
    if let Err(failure) = walked {
        s.storage = Some(b.abandon());
        return Err(failure);
    }
    if numbering || !p.pending.is_empty() {
        s.storage = Some(b.abandon());
        return Ok(None);
    }
    match b.finish() {
        // A body whose debug information breaks a rule of LLVM's
        // verifier (`spec/ir.md` §6.8), which drops it then.
        Ok(body) if !p.module.debug.is_empty() && body.verify_debug(p.module).is_err() => {
            p.note(INVALID);
            s.storage = Some(body);
            Ok(None)
        }
        Ok(body) => Ok(Some(body)),
        // Every name has its definition, so what is left undefined is a
        // ring of no-code calls that each stand for the next
        // (`spec/ll.md` §11.7).
        Err(error) if error.kind == IrErrorKind::UndefinedValue => {
            p.note("no-code-cycle");
            Ok(None)
        }
        Err(error) => Err(ir_fail(function.body.end, error)),
    }
}

/// The parser of one function body.
pub(crate) struct Fx<'p, 'a, 'm, L: Layering = NoLayer> {
    pub p: &'p mut Parser<'a, 'm, L>,
    pub b: FunctionBuilder,
    pub s: &'p mut FnState,
    /// Where the body ends: the offset of its `}`.
    pub end: u32,
    pub ret_ty: TypeId,
    /// The number the next unnamed value or block takes.
    pub next: u64,
    /// How many blocks have been begun: the ordinal of the next one.
    pub blocks: u32,
    pub open_forwards: u32,
    /// The numbering pass (`spec/ll.md` §11.3).
    pub numbering: bool,
    /// Whether the labels after the current statement are in the tables.
    pub scanned: bool,
    /// Where the current statement starts.
    pub stmt: u32,
    /// An instruction's parser ate the comma before its metadata.
    pub extra_comma: bool,
}

/// The key of a local name: a number, or the name's bytes.
enum Key<'k> {
    Num(u32),
    Name(Cow<'k, [u8]>),
}

impl<'a, L: Layering> Fx<'_, 'a, '_, L> {
    // ---- local names ----

    fn key(&self, token: Token) -> PResult<Key<'a>> {
        let text = self.p.tok_text(token);
        match token.kind {
            Tok::LocalId => match parse_u32(&text[1..]) {
                Some(number) => Ok(Key::Num(number)),
                None => fail(LlErrorKind::Syntax, token.start, "value number is too large"),
            },
            Tok::LabelId => match parse_u32(&text[..text.len() - 1]) {
                Some(number) => Ok(Key::Num(number)),
                None => fail(LlErrorKind::Syntax, token.start, "label number is too large"),
            },
            Tok::Label => Ok(Key::Name(Cow::Borrowed(&text[..text.len() - 1]))),
            Tok::LabelQuoted => {
                let raw = &text[1..text.len() - 2];
                if raw.contains(&b'\\') {
                    let mut bytes = Vec::with_capacity(raw.len());
                    unescape(raw, &mut bytes);
                    Ok(Key::Name(Cow::Owned(bytes)))
                } else {
                    Ok(Key::Name(Cow::Borrowed(raw)))
                }
            }
            _ => match self.p.name(token) {
                Some(name) => Ok(Key::Name(name)),
                None => fail(LlErrorKind::Syntax, token.start, "expected a local name"),
            },
        }
    }

    fn get(&self, key: &Key<'_>) -> Local {
        match key {
            Key::Num(number) => match self.s.nums.get(*number as usize) {
                Some(&local) => local,
                None if self.s.sparse.is_empty() => Local::Undef,
                None => self.s.sparse.get(number).copied().unwrap_or(Local::Undef),
            },
            Key::Name(name) => self.s.names.get(name),
        }
    }

    fn set(&mut self, key: &Key<'_>, local: Local) {
        match key {
            Key::Num(number) => {
                let (at, len) = (*number as usize, self.s.nums.len());
                if at >= len + DENSE_GAP {
                    self.s.sparse.insert(*number, local);
                    return;
                }
                if at >= len {
                    self.s.nums.resize(at + 1, Local::Undef);
                    // A number is in one table only, and in the sparse one
                    // only beyond the dense one: what the dense table now
                    // covers moves into it, or a block the label scan put
                    // in the sparse table would read as undefined once a
                    // later number grew the dense table over it.
                    if !self.s.sparse.is_empty() {
                        for moved in len..at {
                            if let Some(found) = self.s.sparse.remove(&(moved as u32)) {
                                self.s.nums[moved] = found;
                            }
                        }
                    }
                }
                self.s.nums[at] = local;
                if !self.s.sparse.is_empty() {
                    self.s.sparse.remove(number);
                }
            }
            Key::Name(name) => self.s.names.set(name, local),
        }
    }

    fn spelled(&self, token: Token) -> String {
        let text = self.p.tok_text(token);
        let text = match token.kind {
            Tok::Label | Tok::LabelId | Tok::LabelQuoted => &text[..text.len() - 1],
            _ => text,
        };
        String::from_utf8_lossy(text).into_owned()
    }

    /// The value the local name `token` stands for, used with type `ty`:
    /// a placeholder if it is not defined yet.
    pub(crate) fn local_value(&mut self, token: Token, ty: TypeId) -> PResult<Value> {
        // The common case first: `%42`, defined already with this type.
        // Anything else takes the general path, which finds the same.
        if token.kind == Tok::LocalId
            && let Some(number) = parse_u32(&self.p.tok_text(token)[1..])
            && let Some(&(Local::Value(value, has) | Local::Forward(value, has))) =
                self.s.nums.get(number as usize)
            && has == ty
        {
            return Ok(value);
        }
        let key = self.key(token)?;
        match self.get(&key) {
            Local::Value(value, has) | Local::Forward(value, has) if has == ty => Ok(value),
            Local::Value(..) | Local::Forward(..) => fail(
                LlErrorKind::Type,
                token.start,
                format!("'{}' has another type than the one it is used with", self.spelled(token)),
            ),
            Local::Block(_) => fail(
                LlErrorKind::Type,
                token.start,
                format!("'{}' is a block, not a value", self.spelled(token)),
            ),
            Local::Undef => {
                let kind = self.p.module.types.kind(ty);
                if matches!(kind, TypeKind::Void | TypeKind::Function { .. } | TypeKind::Label) {
                    return fail(
                        LlErrorKind::Type,
                        token.start,
                        "invalid use of a type that no value has",
                    );
                }
                let value = ir(token.start, self.b.reserve(ty))?;
                self.s.forwards.push((value, token.start));
                self.open_forwards += 1;
                self.set(&key, Local::Forward(value, ty));
                Ok(value)
            }
        }
    }

    /// Gives the result of a statement its name: `name` is the token
    /// before the `=`, if there is one.
    pub(crate) fn define(&mut self, name: Option<Token>, ty: TypeId, value: Value) -> PResult<()> {
        if ty == TypeId::VOID {
            return match name {
                Some(token) => fail(
                    LlErrorKind::Syntax,
                    token.start,
                    "instructions returning void cannot have a name",
                ),
                None => Ok(()),
            };
        }
        let key = match name {
            None => self.take_number(None, "instruction")?,
            Some(token) if token.kind == Tok::LocalId => {
                self.take_number(Some(token), "instruction")?
            }
            Some(token) => self.key(token)?,
        };
        let at = name.map_or(self.stmt, |token| token.start);
        match self.get(&key) {
            Local::Undef => {}
            Local::Forward(placeholder, used) => {
                if used != ty {
                    return fail(
                        LlErrorKind::Type,
                        at,
                        "this instruction was referred to with another type than it has",
                    );
                }
                ir(at, self.b.resolve(placeholder, value))?;
                self.open_forwards -= 1;
            }
            Local::Value(..) | Local::Block(_) => {
                let message = match name {
                    Some(token) => {
                        format!(
                            "multiple definition of local value named '{}'",
                            self.spelled(token)
                        )
                    }
                    None => "multiple definition of a local value".to_string(),
                };
                return fail(LlErrorKind::Redefined, at, message);
            }
        }
        self.set(&key, Local::Value(value, ty));
        Ok(())
    }

    /// The number an unnamed thing takes, or the one `token` writes: it
    /// may skip numbers, never go back.
    fn take_number(&mut self, token: Option<Token>, what: &str) -> PResult<Key<'a>> {
        let number = match token {
            None => self.next,
            Some(token) => {
                let Key::Num(number) = self.key(token)? else {
                    return fail(LlErrorKind::Syntax, token.start, "expected a number");
                };
                if u64::from(number) < self.next {
                    // LLVM's words: a label's number is written without
                    // its `%`.
                    let sigil = if what == "label" { "" } else { "%" };
                    return fail(
                        LlErrorKind::Syntax,
                        token.start,
                        format!("{what} expected to be numbered '{sigil}{}' or greater", self.next),
                    );
                }
                u64::from(number)
            }
        };
        let Ok(number) = u32::try_from(number) else {
            return fail(LlErrorKind::Limit, self.stmt, "more than 2^32 numbered values");
        };
        self.next = u64::from(number) + 1;
        Ok(Key::Num(number))
    }

    // ---- blocks ----

    /// The block of ordinal `ordinal`, created if it was not.
    fn block_id(&mut self, ordinal: u32) -> PResult<BlockId> {
        while self.b.block_count() <= ordinal as usize {
            ir(self.stmt, self.b.create_block())?;
        }
        Ok(BlockId::new(ordinal))
    }

    /// `%name` where a block is expected.
    pub(crate) fn block_ref(&mut self) -> PResult<BlockId> {
        let token = self.p.tok;
        if !matches!(token.kind, Tok::Local | Tok::LocalQuoted | Tok::LocalId) {
            return self.p.syntax("expected a block name");
        }
        self.p.bump();
        let key = self.key(token)?;
        let mut found = self.get(&key);
        if found == Local::Undef && !self.scanned {
            self.scan_labels();
            found = self.get(&key);
        }
        match found {
            Local::Block(ordinal) => self.block_id(ordinal),
            Local::Value(..) | Local::Forward(..) => fail(
                LlErrorKind::Type,
                token.start,
                format!("'{}' is not a basic block", self.spelled(token)),
            ),
            // The numbering pass does not know the blocks ahead that
            // have no label: that is what it is finding out.
            Local::Undef if self.numbering => Ok(BlockId::new(0)),
            Local::Undef => {
                if matches!(key, Key::Num(_)) && !self.s.numbered {
                    self.s.need_numbering = true;
                }
                fail(
                    LlErrorKind::Undefined,
                    token.start,
                    format!("use of undefined value '{}'", self.spelled(token)),
                )
            }
        }
    }

    /// `label %name`.
    pub(crate) fn label(&mut self) -> PResult<BlockId> {
        self.p.expect_word(Kw::Label, "expected 'label' before a block name")?;
        self.block_ref()
    }

    /// Puts every label from the current statement to the end of the body
    /// in the tables, with the ordinal of its block: a block has exactly
    /// one terminator, so the ordinal is the number of terminators before
    /// the label (`spec/ll.md` §11.3).
    fn scan_labels(&mut self) {
        self.scanned = true;
        let src = self.p.src;
        let mut lexer = Lexer::new(src);
        lexer.set_pos(self.stmt as usize);
        let mut ordinal = self.blocks.saturating_sub(1);
        loop {
            let token = lexer.next_token();
            if token.start >= self.end {
                return;
            }
            match token.kind {
                Tok::Eof | Tok::Error => return,
                // Nothing in parentheses is a label or a terminator.
                Tok::LParen => match skip_group::<b'(', b')'>(src, token.end as usize) {
                    Ok(end) => lexer.set_pos(end),
                    Err(_) => return,
                },
                Tok::Label | Tok::LabelId | Tok::LabelQuoted => {
                    if let Ok(key) = self.key(token)
                        && self.get(&key) == Local::Undef
                    {
                        self.set(&key, Local::Block(ordinal));
                    }
                }
                Tok::Word if token.kw.is_terminator() => ordinal += 1,
                _ => {}
            }
        }
    }

    /// Begins the next block; `label` is its label token, if it has one.
    fn begin_block(&mut self, label: Option<Token>) -> PResult<()> {
        let ordinal = self.blocks;
        let at = label.map_or(self.p.tok.start, |token| token.start);
        let key = match label {
            None => {
                let key = self.take_number(None, "label")?;
                if self.numbering
                    && let Key::Num(number) = key
                {
                    self.s.unlabelled.push((number, ordinal));
                }
                key
            }
            Some(token) if token.kind == Tok::LabelId => self.take_number(Some(token), "label")?,
            Some(token) => self.key(token)?,
        };
        match self.get(&key) {
            Local::Undef => self.set(&key, Local::Block(ordinal)),
            Local::Block(known) if known == ordinal => {}
            Local::Block(_) => {
                let message = match label {
                    Some(token) => format!("redefinition of block '{}'", self.spelled(token)),
                    None => "a block takes the number of another".to_string(),
                };
                return fail(LlErrorKind::Redefined, at, message);
            }
            Local::Value(..) | Local::Forward(..) => {
                return fail(LlErrorKind::Type, at, "a block has the name of a value");
            }
        }
        let id = self.block_id(ordinal)?;
        ir(at, self.b.begin_block(id))?;
        self.blocks += 1;
        Ok(())
    }

    // ---- the walk ----

    /// The parameters' names, then the blocks.
    fn function(&mut self, function: &FunctionText) -> PResult<()> {
        self.params(function.params)?;
        // What the numbering pass found.
        for at in 0..self.s.unlabelled.len() {
            let (number, ordinal) = self.s.unlabelled[at];
            if !self.numbering {
                self.set(&Key::Num(number), Local::Block(ordinal));
            }
        }
        if self.numbering {
            self.s.unlabelled.clear();
        }
        self.p.goto(function.body.start);
        if self.p.tok.start >= self.end {
            return self.p.syntax("function body requires at least one basic block");
        }
        while self.p.tok.start < self.end {
            if self.p.at_word(Kw::Uselistorder) {
                self.use_list_orders()?;
                break;
            }
            let label = match self.p.tok.kind {
                Tok::Label | Tok::LabelId | Tok::LabelQuoted => {
                    let token = self.p.tok;
                    self.p.bump();
                    Some(token)
                }
                _ => None,
            };
            self.begin_block(label)?;
            loop {
                self.stmt = self.p.tok.start;
                if self.stmt >= self.end {
                    return self.p.syntax("expected instruction opcode");
                }
                if self.p.tok.kind == Tok::HashWord {
                    self.debug_record()?;
                    continue;
                }
                if self.statement()? {
                    break;
                }
            }
        }
        if self.p.tok.start != self.end {
            return self.p.syntax("expected the end of the function body");
        }
        if self.open_forwards > 0 {
            return self.undefined();
        }
        Ok(())
    }

    /// Names the arguments from the text of the parameter list: unnamed
    /// ones take the numbers from 0.
    fn params(&mut self, params: Span) -> PResult<()> {
        self.p.goto(params.start);
        let mut index = 0u32;
        while self.p.tok.start < params.end {
            if self.p.eat(Tok::DotDotDot) {
                break;
            }
            let param_at = self.p.tok.start;
            let ty = self.p.parse_type()?;
            self.p.param_attrs()?;
            let token = self.p.tok;
            let value = ir(token.start, self.b.arg(index))?;
            let key = match token.kind {
                Tok::LocalId => {
                    self.p.bump();
                    // LLVM places a number out of order at the
                    // parameter's type.
                    self.take_number(Some(token), "argument").map_err(|mut failure| {
                        failure.offset = param_at;
                        failure
                    })?
                }
                Tok::Local | Tok::LocalQuoted => {
                    self.p.bump();
                    self.key(token)?
                }
                _ => self.take_number(None, "argument")?,
            };
            if self.get(&key) != Local::Undef {
                return fail(
                    LlErrorKind::Redefined,
                    token.start,
                    format!("redefinition of argument '{}'", self.spelled(token)),
                );
            }
            self.set(&key, Local::Value(value, ty));
            index += 1;
            if !self.p.eat(Tok::Comma) {
                break;
            }
        }
        // What the attributes noted is recorded already, with the header.
        self.p.pending.clear();
        Ok(())
    }

    /// `uselistorder type value, { order }` directives at the end of a
    /// body: the order of use lists means nothing to hod.
    fn use_list_orders(&mut self) -> PResult<()> {
        while self.p.eat_word(Kw::Uselistorder) {
            while self.p.tok.kind != Tok::LBrace {
                if self.p.tok.start >= self.end || self.p.tok.kind == Tok::Error {
                    return self.p.syntax("expected '{' in a uselistorder directive");
                }
                self.p.bump();
            }
            self.skip_group()?;
        }
        Ok(())
    }

    /// Steps over the bracket group the current token opens; it must
    /// close inside the body.
    pub(crate) fn skip_group(&mut self) -> PResult<Span> {
        let at = self.p.tok.start;
        let span = self.p.skip_brackets()?;
        if span.end >= self.end {
            return fail(LlErrorKind::Syntax, at, "this bracket is never closed");
        }
        Ok(span)
    }

    /// The failure for the first name that was used and never defined.
    fn undefined(&mut self) -> PResult<()> {
        let open = |local: &Local| match *local {
            Local::Forward(value, _) => Some(value),
            _ => None,
        };
        let state = &*self.s;
        let numbered = state.nums.iter().chain(state.sparse.values()).filter_map(open);
        let named = state.names.entries.iter().filter_map(|entry| open(&entry.2));
        let offset_of = |value: Value| {
            state.forwards.iter().find(|forward| forward.0 == value).map(|forward| forward.1)
        };
        let at = numbered.chain(named).filter_map(offset_of).min().unwrap_or(self.end);
        let mut lexer = Lexer::new(self.p.src);
        lexer.set_pos(at as usize);
        let name = self.spelled(lexer.next_token());
        fail(LlErrorKind::Undefined, at, format!("use of undefined value '{name}'"))
    }

    /// One statement: `[%name =] instruction {, !kind !node}`. True if it
    /// ended the block.
    fn statement(&mut self) -> PResult<bool> {
        let name = match self.p.tok.kind {
            Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                let token = self.p.tok;
                self.p.bump();
                self.p.expect(Tok::Equal, "expected '=' after instruction name")?;
                Some(token)
            }
            _ => None,
        };
        let before = self.b.inst_count();
        let (ty, value, terminator) = self.instruction()?;
        let mut loc = LocId::NONE;
        if std::mem::take(&mut self.extra_comma) || self.p.eat(Tok::Comma) {
            let outside_frame;
            (loc, outside_frame) = self.attachments()?;
            if loc != LocId::NONE {
                self.b.set_loc_since(before, loc);
            }
            // `!coro.outside.frame` on a static `alloca` (`spec/ir.md` §5.2).
            if outside_frame && let ValueKind::Slot(slot) = value.kind() {
                ir(self.p.tok.start, self.b.set_slot_outside_frame(slot))?;
            }
        }
        if let Some(record) = self.s.old_record.take() {
            self.old_record(record, loc)?;
        }
        self.define(name, ty, value)?;
        Ok(terminator)
    }

    /// `!kind !node {, !kind !node}` after an instruction: the location
    /// `!dbg` names (`spec/ir.md` §6), and whether `!coro.outside.frame`
    /// is present (§5.2), the only ones kept; every node must exist.
    fn attachments(&mut self) -> PResult<(LocId, bool)> {
        let mut loc = LocId::NONE;
        let mut outside_frame = false;
        loop {
            if self.p.tok.kind != Tok::Meta {
                return self.p.syntax("expected metadata after comma");
            }
            let dbg = self.p.text() == b"!dbg";
            outside_frame |= self.p.text() == b"!coro.outside.frame";
            self.p.bump();
            let at = self.p.tok.start;
            let node = self.p.md_ref()?;
            if let MdRef::Inline(_, span) = node
                && span.end >= self.end
            {
                return fail(LlErrorKind::Syntax, at, "this bracket is never closed");
            }
            if dbg && !self.numbering {
                let resume = self.p.tok.start;
                loc = self.p.loc_of(at, node)?.unwrap_or(LocId::NONE);
                self.p.goto(resume);
            }
            if !self.p.eat(Tok::Comma) {
                return Ok((loc, outside_frame));
            }
        }
    }

    // ---- debug records (`spec/ll.md` §11.9) ----

    /// `#dbg_xxx(…)`, at the `#`.
    fn debug_record(&mut self) -> PResult<()> {
        let name = self.p.text();
        let at = self.p.tok.start;
        self.p.bump();
        if self.p.tok.kind != Tok::LParen {
            return self.p.syntax("expected '(' in a debug record");
        }
        let args = self.skip_group()?;
        if self.numbering {
            return Ok(());
        }
        let Some(kind) = RecordKind::of_record(name) else {
            let Some(construct) = kit::unsupported_record_construct(name) else {
                return fail(LlErrorKind::Syntax, at, "expected a debug record");
            };
            self.p.note(construct);
            return Ok(());
        };
        let resume = self.p.tok.start;
        self.p.goto(args.start);
        let record = self.record_args(kind, false)?;
        self.p.expect(Tok::Comma, "expected ',' before the record's location")?;
        let loc_at = self.p.tok.start;
        let node = self.p.md_ref()?;
        let loc = self.p.loc_of(loc_at, node)?;
        if self.p.tok.start != args.end {
            return self.p.syntax("expected ')' at the end of a debug record");
        }
        self.p.goto(resume);
        match loc {
            Some(loc) => self.emit_record(record, loc, at),
            None => Ok(()),
        }
    }

    /// The operands of a debug record before its location; with
    /// `old`, of a call of the intrinsic, each after `metadata`.
    fn record_args(&mut self, kind: RecordKind, old: bool) -> PResult<OldRecord> {
        let mut record = OldRecord::new(kind);
        self.s.dbg_vals.clear();
        if kind == RecordKind::Label {
            record.var = self.record_node(old, MdKind::Label)?;
            return Ok(record);
        }
        record.list = self.record_value(old)?;
        self.p.expect(Tok::Comma, "expected ',' between operands")?;
        record.var = self.record_node(old, MdKind::LocalVariable)?;
        self.p.expect(Tok::Comma, "expected ',' between operands")?;
        record.expr = self.record_expr(old)?;
        if kind == RecordKind::Assign {
            self.p.expect(Tok::Comma, "expected ',' between operands")?;
            self.metadata_word(old)?;
            // The `DIAssignID` is not kept (`spec/ir.md` §6.9).
            let at = self.p.tok.start;
            match self.p.md_ref()? {
                MdRef::Node(id) if self.p.md_kind(id) == Some(MdKind::AssignId) => {}
                MdRef::Inline(MdKind::AssignId, _) => {}
                _ => return fail(LlErrorKind::Syntax, at, "expected a DIAssignID"),
            }
            self.p.expect(Tok::Comma, "expected ',' between operands")?;
            let mark = self.s.dbg_vals.len();
            if self.record_value(old)? {
                self.p.note(kit::ASSIGN_ARG_LIST);
            }
            record.address = self.s.dbg_vals.get(mark).copied().unwrap_or(Value::NONE);
            self.s.dbg_vals.truncate(mark);
            self.p.expect(Tok::Comma, "expected ',' between operands")?;
            record.address_expr = self.record_expr(old)?;
            if record.list {
                self.p.note(kit::ASSIGN_ARG_LIST);
            }
        }
        Ok(record)
    }

    fn metadata_word(&mut self, old: bool) -> PResult<()> {
        if old {
            self.p.expect_word(Kw::Metadata, "expected 'metadata' before the operand")?;
        }
        Ok(())
    }

    /// A record's value: `type value`, `!{}` (none), or a `DIArgList`
    /// (true). Pushed to `dbg_vals`.
    fn record_value(&mut self, old: bool) -> PResult<bool> {
        self.metadata_word(old)?;
        match self.p.tok.kind {
            Tok::Exclaim => {
                let at = self.p.tok.start;
                self.p.bump();
                if self.p.tok.kind != Tok::LBrace {
                    return fail(LlErrorKind::Syntax, at, "expected a value of a debug record");
                }
                let span = self.skip_group()?;
                if span.start != span.end
                    && !self.p.src[span.start as usize..span.end as usize]
                        .iter()
                        .all(u8::is_ascii_whitespace)
                {
                    return fail(LlErrorKind::Syntax, at, "expected a value of a debug record");
                }
                self.s.dbg_vals.push(Value::NONE);
                Ok(false)
            }
            Tok::Meta if self.p.text() == b"!DIArgList" => {
                self.p.bump();
                self.p.expect(Tok::LParen, "expected '(' here")?;
                while self.p.tok.kind != Tok::RParen {
                    let (_, value) = self.typed()?;
                    self.s.dbg_vals.push(value);
                    if !self.p.eat(Tok::Comma) {
                        break;
                    }
                }
                self.p.expect(Tok::RParen, "expected ')' here")?;
                Ok(true)
            }
            Tok::Meta => self.p.syntax("expected a value of a debug record"),
            _ => {
                let at = self.p.tok.start;
                let (ty, value) = self.typed()?;
                if ty == TypeId::METADATA {
                    return fail(
                        LlErrorKind::Syntax,
                        at,
                        "invalid metadata-value-metadata roundtrip",
                    );
                }
                self.s.dbg_vals.push(value);
                Ok(false)
            }
        }
    }

    /// The variable or label of a record: a numbered node of `kind`.
    fn record_node(&mut self, old: bool, kind: MdKind) -> PResult<DiId> {
        self.metadata_word(old)?;
        match self.p.md_ref()? {
            MdRef::Node(id) => match self.p.handle(id) {
                Handle::Node(node) if self.p.md_kind(id) == Some(kind) => Ok(node),
                _ => {
                    self.p.note(INVALID);
                    Ok(DiId::NONE)
                }
            },
            MdRef::Inline(found, _) if found == kind => {
                self.p.note("debug:node-in-body");
                Ok(DiId::NONE)
            }
            MdRef::Inline(..) => {
                self.p.note(INVALID);
                Ok(DiId::NONE)
            }
        }
    }

    /// The expression of a record.
    fn record_expr(&mut self, old: bool) -> PResult<ExprId> {
        self.metadata_word(old)?;
        let mut scratch = std::mem::take(&mut self.p.dscratch);
        let expr = self.p.expr_ref(&mut scratch);
        self.p.dscratch = scratch;
        expr
    }

    /// Hands a record to the builder.
    fn emit_record(&mut self, record: OldRecord, loc: LocId, at: u32) -> PResult<()> {
        if !ir(at, kit::emit_debug_record(&mut self.b, &record, &self.s.dbg_vals, loc))? {
            self.p.note(INVALID);
        }
        Ok(())
    }

    /// A call of an `llvm.dbg.*` intrinsic, at the `(` of its
    /// arguments: read, and kept until the call's `!dbg` is known.
    pub(crate) fn debug_call(&mut self, name: &[u8]) -> PResult<()> {
        let at = self.p.tok.start;
        let args = self.skip_group()?;
        if self.numbering {
            return Ok(());
        }
        let Some(kind) = RecordKind::of_intrinsic(name) else {
            self.p.note(kit::debug_intrinsic_construct(name));
            return Ok(());
        };
        let resume = self.p.tok.start;
        self.p.goto(args.start);
        let record = self.record_args(kind, true)?;
        if self.p.tok.start != args.end {
            return fail(LlErrorKind::Syntax, at, "expected ')' at the end of the call");
        }
        self.p.goto(resume);
        self.s.old_record = Some(record);
        Ok(())
    }

    /// The record a call of a debug intrinsic stands for, now that its
    /// `!dbg` is known: LLVM's verifier wants one.
    fn old_record(&mut self, record: OldRecord, loc: LocId) -> PResult<()> {
        if loc == LocId::NONE {
            self.p.note(INVALID);
            return Ok(());
        }
        let at = self.stmt;
        self.emit_record(record, loc, at)
    }
}

/// Whether `word` is the keyword of an instruction that ends a block.
pub(crate) fn is_terminator(word: &[u8]) -> bool {
    matches!(
        word,
        b"ret"
            | b"br"
            | b"switch"
            | b"indirectbr"
            | b"invoke"
            | b"resume"
            | b"unreachable"
            | b"cleanupret"
            | b"catchret"
            | b"catchswitch"
            | b"callbr"
    )
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use crate::{BodyParser, ParseOptions, parse_module};

    /// A module of `functions` functions, each with `calls` calls whose
    /// arguments and results carry the usual attributes.
    fn calls(functions: usize, calls: usize) -> String {
        let mut text = String::from(
            "target triple = \"aarch64-unknown-linux-gnu\"\n\
             declare noundef nonnull ptr @g(ptr noundef nonnull align 8 dereferenceable(16), i32 noundef signext)\n",
        );
        for f in 0..functions {
            let _ = writeln!(text, "define void @f{f}(ptr %p) {{");
            for _ in 0..calls {
                text.push_str(
                    "  %r{} = call noundef nonnull ptr @g(ptr noundef nonnull align 8 \
                     dereferenceable(16) %p, i32 noundef signext 7)\n",
                );
            }
            text.push_str("  ret void\n}\n");
        }
        let mut n = 0;
        while let Some(at) = text.find("%r{}") {
            text.replace_range(at..at + 4, &format!("%r{n}"));
            n += 1;
        }
        text
    }

    /// WP-096: `hod_ir`'s attribute table is searched once per keyword,
    /// not once per word after every type of every call: 800 calls of 7
    /// attribute words and 2 type words each search it for the 8
    /// distinct words only (`ptr` and `i32` are no attributes).
    #[test]
    fn attribute_keywords_are_looked_up_once() {
        let text = calls(20, 40);
        let mut parsed = parse_module(text.as_bytes(), &ParseOptions::default()).unwrap();
        let mut bodies = BodyParser::new(&parsed.text);
        for function in parsed.text.functions() {
            assert!(bodies.parse(&mut parsed.module, function).unwrap().is_some());
        }
        let lookups = bodies.scratch.attr_memo.lookups;
        assert!((5..=8).contains(&lookups), "{lookups} searches of the attribute table");
    }
}
